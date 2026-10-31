use crate::{auth, db::User, voice::Voice, AppState};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use serde::Deserialize;
use serde_json::json;
use std::{
    sync::{atomic::Ordering, MutexGuard},
    time::Duration,
};
use tokio::sync::{broadcast::error::RecvError, mpsc};

const MAX_TEXT: usize = 2000;
const HISTORY_LIMIT: i64 = 50;
const DEFAULT_CHANNEL: i64 = 1;

/// Messaggi che il client può mandare. Il primo deve essere `auth`.
#[derive(Deserialize)]
#[serde(tag = "t")]
enum ClientMsg {
    #[serde(rename = "auth")]
    Auth { token: String },
    #[serde(rename = "send")]
    Send { channel: i64, text: String },
    #[serde(rename = "voice_join")]
    VoiceJoin,
    #[serde(rename = "voice_leave")]
    VoiceLeave,
    /// Stato locale: microfono spento, camera accesa, schermo condiviso.
    #[serde(rename = "voice_meta")]
    VoiceMeta {
        muted: bool,
        cam: bool,
        screen: bool,
    },
    /// Offerte/risposte WebRTC e candidati ICE, inoltrati a un altro membro della chiamata.
    #[serde(rename = "signal")]
    Signal { to: i64, data: serde_json::Value },
}

pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    // Le descrizioni SDP con video possono superare i 16 KB.
    ws.max_message_size(64 * 1024)
        .on_upgrade(move |socket| handle_socket(socket, state))
}

/// Blocca lo stato della voce. Non va mai tenuto attraverso un `.await`.
fn voice(state: &AppState) -> MutexGuard<'_, Voice> {
    state.voice.lock().unwrap_or_else(|e| e.into_inner())
}

async fn send_json(
    sender: &mut SplitSink<WebSocket, Message>,
    value: serde_json::Value,
) -> Result<(), axum::Error> {
    sender.send(Message::Text(value.to_string().into())).await
}

/// Aspetta il primo messaggio di testo, che deve essere `{"t":"auth","token":"..."}`.
/// Il token viaggia nel messaggio e non nell'URL, così non finisce nei log.
async fn authenticate(state: &AppState, receiver: &mut SplitStream<WebSocket>) -> Option<User> {
    while let Some(Ok(msg)) = receiver.next().await {
        if let Message::Text(raw) = msg {
            return match serde_json::from_str::<ClientMsg>(&raw) {
                Ok(ClientMsg::Auth { token }) => auth::user_from_token(&state.db, &token).await,
                _ => None,
            };
        }
    }
    None
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();

    // 1. Autenticazione (entro 10 secondi)
    let auth_result =
        tokio::time::timeout(Duration::from_secs(10), authenticate(&state, &mut receiver)).await;
    let Ok(Some(user)) = auth_result else {
        let _ = send_json(&mut sender, json!({ "t": "error", "error": "non autenticato" })).await;
        return;
    };

    let conn_id = state.next_conn.fetch_add(1, Ordering::Relaxed);
    let user_id = user.id;

    // 2. Ci iscriviamo al broadcast PRIMA di leggere la cronologia, così nessun
    //    messaggio va perso nel mezzo (il client scarta gli eventuali doppioni per id).
    let mut rx = state.tx.subscribe();
    let history = state
        .db
        .history(DEFAULT_CHANNEL, HISTORY_LIMIT)
        .await
        .unwrap_or_default();
    let voice_state = voice(&state).state_json();
    if send_json(&mut sender, json!({ "t": "ready", "user": user, "ice": *state.ice }))
        .await
        .is_err()
        || send_json(
            &mut sender,
            json!({ "t": "history", "channel": DEFAULT_CHANNEL, "messages": history }),
        )
        .await
        .is_err()
        || send_json(&mut sender, voice_state).await.is_err()
    {
        return;
    }

    // Canale privato di questa connessione (risposte alla voce e segnalazione WebRTC).
    let (ptx, mut prx) = mpsc::channel::<String>(256);

    // 3. Verso questo client: broadcast + messaggi privati
    let mut send_task = tokio::spawn(async move {
        loop {
            let msg = tokio::select! {
                r = rx.recv() => match r {
                    Ok(m) => m,
                    Err(RecvError::Lagged(_)) => continue, // client lento: salta i messaggi persi
                    Err(RecvError::Closed) => break,
                },
                p = prx.recv() => match p {
                    Some(m) => m,
                    None => break,
                },
            };
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // 4. Da questo client verso database, broadcast e chiamata
    let st = state.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            let Message::Text(raw) = msg else { continue };
            let Ok(parsed) = serde_json::from_str::<ClientMsg>(&raw) else {
                continue;
            };
            match parsed {
                ClientMsg::Send { channel, text } => {
                    let text: String = text.trim().chars().take(MAX_TEXT).collect();
                    if text.is_empty() {
                        continue;
                    }
                    // Il nome dell'autore viene dalla sessione, mai dal client.
                    if let Ok(Some(m)) = st.db.insert_message(channel, user.clone(), text).await {
                        let _ = st
                            .tx
                            .send(json!({ "t": "message", "message": m }).to_string());
                    }
                }
                ClientMsg::VoiceJoin => {
                    let result = voice(&st).join(conn_id, &user, ptx.clone());
                    match result {
                        Ok((joined, new_state)) => {
                            let _ = st.tx.send(new_state.to_string());
                            let _ = ptx.try_send(joined.to_string());
                        }
                        Err(e) => {
                            let _ = ptx
                                .try_send(json!({ "t": "voice_error", "error": e }).to_string());
                        }
                    }
                }
                ClientMsg::VoiceLeave => {
                    let new_state = voice(&st).leave(conn_id, user_id);
                    if let Some(s) = new_state {
                        let _ = st.tx.send(s.to_string());
                    }
                }
                ClientMsg::VoiceMeta { muted, cam, screen } => {
                    let new_state = voice(&st).set_meta(conn_id, user_id, muted, cam, screen);
                    if let Some(s) = new_state {
                        let _ = st.tx.send(s.to_string());
                    }
                }
                ClientMsg::Signal { to, data } => voice(&st).signal(conn_id, user_id, to, data),
                ClientMsg::Auth { .. } => {}
            }
        }
    });

    // Quando uno dei due task finisce (disconnessione), chiudi l'altro
    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }
    // Chi si disconnette esce anche dalla chiamata.
    let new_state = voice(&state).leave(conn_id, user_id);
    if let Some(s) = new_state {
        let _ = state.tx.send(s.to_string());
    }
}
