use crate::{auth, db::User, perms, voice::Voice, ApiError, AppState};
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
use serde_json::{json, Value};
use std::{
    sync::{atomic::Ordering, MutexGuard},
    time::Duration,
};
use tokio::sync::{broadcast::error::RecvError, mpsc};

const MAX_TEXT: usize = 2000;
const HISTORY_LIMIT: i64 = 50;

/// Messaggi che il client può mandare. Il primo deve essere `auth`.
#[derive(Deserialize)]
#[serde(tag = "t")]
enum ClientMsg {
    #[serde(rename = "auth")]
    Auth { token: String },

    // --- chat ---
    /// Chiede la cronologia di un canale (il server risponde con `history`).
    #[serde(rename = "load_history")]
    LoadHistory { channel: i64 },
    #[serde(rename = "send")]
    Send { channel: i64, text: String },
    #[serde(rename = "edit")]
    Edit { id: i64, text: String },
    #[serde(rename = "delete")]
    Delete { id: i64 },

    // --- canali (serve "Gestire i canali") ---
    #[serde(rename = "channel_create")]
    ChannelCreate { name: String },
    #[serde(rename = "channel_rename")]
    ChannelRename { id: i64, name: String },
    #[serde(rename = "channel_delete")]
    ChannelDelete { id: i64 },

    // --- ruoli (serve "Gestire i ruoli") ---
    #[serde(rename = "role_create")]
    RoleCreate {
        name: String,
        color: String,
        permissions: i64,
    },
    #[serde(rename = "role_edit")]
    RoleEdit {
        id: i64,
        name: String,
        color: String,
        permissions: i64,
    },
    #[serde(rename = "role_delete")]
    RoleDelete { id: i64 },
    #[serde(rename = "role_move")]
    RoleMove { id: i64, up: bool },
    /// Elenco completo dei ruoli di un utente.
    #[serde(rename = "member_roles")]
    MemberRoles { user: i64, roles: Vec<i64> },

    // --- voce ---
    #[serde(rename = "voice_join")]
    VoiceJoin { channel: i64 },
    // Creare/rinominare/eliminare canali vocali: serve "Gestire i canali".
    #[serde(rename = "voice_channel_create")]
    VoiceChannelCreate { name: String },
    #[serde(rename = "voice_channel_rename")]
    VoiceChannelRename { id: i64, name: String },
    #[serde(rename = "voice_channel_delete")]
    VoiceChannelDelete { id: i64 },
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

// ---------- invii a tutti i client ----------

fn announce(state: &AppState, value: Value) {
    let _ = state.tx.send(value.to_string());
}

async fn channels_msg(state: &AppState) -> Option<Value> {
    let list = state.db.channels().await.ok()?;
    Some(json!({ "t": "channels", "channels": list }))
}

async fn voice_channels_msg(state: &AppState) -> Option<Value> {
    let list = state.db.voice_channels().await.ok()?;
    Some(json!({ "t": "voice_channels", "channels": list }))
}

pub async fn broadcast_voice_channels(state: &AppState) {
    if let Some(v) = voice_channels_msg(state).await {
        announce(state, v);
    }
}

async fn directory_msg(state: &AppState) -> Option<Value> {
    let d = state.db.directory().await.ok()?;
    Some(json!({ "t": "directory", "roles": d.roles, "members": d.members }))
}

/// Elenco dei canali aggiornato, per tutti.
pub async fn broadcast_channels(state: &AppState) {
    if let Some(v) = channels_msg(state).await {
        announce(state, v);
    }
}

/// Ruoli, membri e permessi aggiornati, per tutti. Si usa anche alla registrazione di un nuovo utente.
pub async fn broadcast_directory(state: &AppState) {
    if let Some(v) = directory_msg(state).await {
        announce(state, v);
    }
}

// ---------- connessione ----------

/// Una connessione già autenticata.
struct Conn {
    id: u64,
    user: User,
    /// Canale privato verso questo client (risposte, errori, segnalazione WebRTC).
    ptx: mpsc::Sender<String>,
}

impl Conn {
    fn reply(&self, v: Value) {
        let _ = self.ptx.try_send(v.to_string());
    }

    /// Messaggio di errore da mostrare all'utente (permesso negato, nome già usato, ...).
    fn notice(&self, error: &str) {
        self.reply(json!({ "t": "notice", "error": error }));
    }
}

/// Le operazioni del database restituiscono `Result<Result<T, motivo>, errore interno>`:
/// qui le appiattiamo in un unico `Result` con il testo da mostrare.
fn flat<T>(r: Result<Result<T, &'static str>, ApiError>) -> Result<T, &'static str> {
    match r {
        Ok(inner) => inner,
        Err(_) => Err("errore interno"),
    }
}

async fn channels_done(st: &AppState, c: &Conn, r: Result<(), &'static str>) {
    match r {
        Ok(()) => broadcast_channels(st).await,
        Err(e) => c.notice(e),
    }
}

async fn voice_channels_done(st: &AppState, c: &Conn, r: Result<(), &'static str>) {
    match r {
        Ok(()) => broadcast_voice_channels(st).await,
        Err(e) => c.notice(e),
    }
}

async fn directory_done(st: &AppState, c: &Conn, r: Result<(), &'static str>) {
    match r {
        Ok(()) => broadcast_directory(st).await,
        Err(e) => c.notice(e),
    }
}

// ---------- validazione ----------

fn clean_text(t: String) -> String {
    t.trim().chars().take(MAX_TEXT).collect()
}

/// Nomi dei canali come su Discord: minuscolo, spazi → trattini, 1-32 caratteri.
fn clean_channel_name(raw: &str) -> Option<String> {
    let name = raw
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-");
    let ok = (1..=32).contains(&name.chars().count())
        && name
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '-' || ch == '_');
    ok.then_some(name)
}

fn clean_role_name(raw: &str) -> Option<String> {
    let name = raw.trim();
    let ok = (1..=32).contains(&name.chars().count()) && !name.chars().any(|ch| ch.is_control());
    ok.then(|| name.to_string())
}

/// Colore del ruolo: vuoto (nessuno) oppure `#rrggbb`.
fn clean_color(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return Some(String::new());
    }
    let ok = raw.len() == 7
        && raw.starts_with('#')
        && raw[1..].chars().all(|ch| ch.is_ascii_hexdigit());
    ok.then(|| raw.to_lowercase())
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

    // 2. Ci iscriviamo al broadcast PRIMA di leggere lo stato, così nessun evento
    //    va perso nel mezzo (il client scarta i doppioni per id).
    let mut rx = state.tx.subscribe();
    let voice_state = voice(&state).state_json();
    let mut initial = vec![json!({ "t": "ready", "user": user, "ice": *state.ice })];
    initial.extend(channels_msg(&state).await);
    initial.extend(directory_msg(&state).await);
    initial.extend(voice_channels_msg(&state).await);
    initial.push(voice_state);
    // La cronologia di un canale la chiede il client con `load_history`.
    for v in initial {
        if send_json(&mut sender, v).await.is_err() {
            return;
        }
    }

    // Canale privato di questa connessione (risposte alla voce e segnalazione WebRTC).
    let (ptx, mut prx) = mpsc::channel::<String>(256);
    let conn = Conn {
        id: conn_id,
        user,
        ptx,
    };

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
            handle(&st, &conn, parsed).await;
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
        announce(&state, s);
    }
}

/// Elabora un messaggio del client. I permessi si controllano sempre qui, nel server:
/// il client li usa solo per mostrare o nascondere i pulsanti.
async fn handle(st: &AppState, c: &Conn, msg: ClientMsg) {
    match msg {
        ClientMsg::Auth { .. } => {}

        // ----- chat -----
        ClientMsg::LoadHistory { channel } => {
            let messages = st.db.history(channel, HISTORY_LIMIT).await.unwrap_or_default();
            c.reply(json!({ "t": "history", "channel": channel, "messages": messages }));
        }
        ClientMsg::Send { channel, text } => {
            let text = clean_text(text);
            if text.is_empty() {
                return;
            }
            // Il nome dell'autore viene dalla sessione, mai dal client.
            match flat(st.db.send_message(channel, c.user.clone(), text).await) {
                Ok(m) => announce(st, json!({ "t": "message", "message": m })),
                Err(e) => c.notice(e),
            }
        }
        ClientMsg::Edit { id, text } => {
            let text = clean_text(text);
            if text.is_empty() {
                c.notice("il messaggio non può essere vuoto: per toglierlo, eliminalo");
                return;
            }
            match flat(st.db.edit_message(c.user.id, id, text).await) {
                Ok(m) => announce(st, json!({ "t": "message_edited", "message": m })),
                Err(e) => c.notice(e),
            }
        }
        ClientMsg::Delete { id } => match flat(st.db.delete_message(c.user.id, id).await) {
            Ok(channel) => announce(
                st,
                json!({ "t": "message_deleted", "channel": channel, "id": id }),
            ),
            Err(e) => c.notice(e),
        },

        // ----- canali -----
        ClientMsg::ChannelCreate { name } => {
            let Some(name) = clean_channel_name(&name) else {
                c.notice("nome del canale non valido (1-32 caratteri: lettere, numeri, - e _)");
                return;
            };
            let r = flat(st.db.channel_create(c.user.id, name).await);
            channels_done(st, c, r).await;
        }
        ClientMsg::ChannelRename { id, name } => {
            let Some(name) = clean_channel_name(&name) else {
                c.notice("nome del canale non valido (1-32 caratteri: lettere, numeri, - e _)");
                return;
            };
            let r = flat(st.db.channel_rename(c.user.id, id, name).await);
            channels_done(st, c, r).await;
        }
        ClientMsg::ChannelDelete { id } => {
            let r = flat(st.db.channel_delete(c.user.id, id).await);
            channels_done(st, c, r).await;
        }

        // ----- ruoli -----
        ClientMsg::RoleCreate {
            name,
            color,
            permissions,
        } => {
            let (Some(name), Some(color)) = (clean_role_name(&name), clean_color(&color)) else {
                c.notice("nome o colore del ruolo non validi");
                return;
            };
            let r = flat(st.db.role_create(c.user.id, name, color, permissions).await);
            directory_done(st, c, r).await;
        }
        ClientMsg::RoleEdit {
            id,
            name,
            color,
            permissions,
        } => {
            let (Some(name), Some(color)) = (clean_role_name(&name), clean_color(&color)) else {
                c.notice("nome o colore del ruolo non validi");
                return;
            };
            let r = flat(
                st.db
                    .role_edit(c.user.id, id, name, color, permissions)
                    .await,
            );
            directory_done(st, c, r).await;
        }
        ClientMsg::RoleDelete { id } => {
            let r = flat(st.db.role_delete(c.user.id, id).await);
            directory_done(st, c, r).await;
        }
        ClientMsg::RoleMove { id, up } => {
            let r = flat(st.db.role_move(c.user.id, id, up).await);
            directory_done(st, c, r).await;
        }
        ClientMsg::MemberRoles { user, roles } => {
            let r = flat(st.db.set_member_roles(c.user.id, user, roles).await);
            directory_done(st, c, r).await;
        }

        // ----- canali vocali -----
        ClientMsg::VoiceChannelCreate { name } => {
            let Some(name) = clean_channel_name(&name) else {
                c.notice("nome del canale non valido (1-32 caratteri: lettere, numeri, - e _)");
                return;
            };
            let r = flat(st.db.voice_channel_create(c.user.id, name).await);
            voice_channels_done(st, c, r).await;
        }
        ClientMsg::VoiceChannelRename { id, name } => {
            let Some(name) = clean_channel_name(&name) else {
                c.notice("nome del canale non valido (1-32 caratteri: lettere, numeri, - e _)");
                return;
            };
            let r = flat(st.db.voice_channel_rename(c.user.id, id, name).await);
            voice_channels_done(st, c, r).await;
        }
        ClientMsg::VoiceChannelDelete { id } => {
            match flat(st.db.voice_channel_delete(c.user.id, id).await) {
                Ok(()) => {
                    // Chi era dentro viene fatto uscire.
                    let dropped = voice(st).drop_channel(id);
                    broadcast_voice_channels(st).await;
                    if let Some(s) = dropped {
                        announce(st, s);
                    }
                }
                Err(e) => c.notice(e),
            }
        }

        // ----- voce -----
        ClientMsg::VoiceJoin { channel } => {
            let allowed = matches!(
                st.db.access(c.user.id).await,
                Ok(a) if a.can(perms::CONNECT_VOICE)
            );
            if !allowed {
                c.reply(json!({
                    "t": "voice_error",
                    "error": "non hai il permesso di entrare nel canale vocale"
                }));
                return;
            }
            let exists = matches!(
                st.db.voice_channels().await,
                Ok(l) if l.iter().any(|ch| ch.id == channel)
            );
            if !exists {
                c.reply(json!({ "t": "voice_error", "error": "il canale vocale non esiste" }));
                return;
            }
            let result = voice(st).join(c.id, &c.user, channel, c.ptx.clone());
            match result {
                Ok((joined, new_state)) => {
                    announce(st, new_state);
                    c.reply(joined);
                }
                Err(e) => c.reply(json!({ "t": "voice_error", "error": e })),
            }
        }
        ClientMsg::VoiceLeave => {
            let new_state = voice(st).leave(c.id, c.user.id);
            if let Some(s) = new_state {
                announce(st, s);
            }
        }
        ClientMsg::VoiceMeta { muted, cam, screen } => {
            let new_state = voice(st).set_meta(c.id, c.user.id, muted, cam, screen);
            if let Some(s) = new_state {
                announce(st, s);
            }
        }
        ClientMsg::Signal { to, data } => {
            voice(st).signal(c.id, c.user.id, to, data);
        }
    }
}
