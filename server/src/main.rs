use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::broadcast::{self, error::RecvError};

const MAX_NAME: usize = 32;
const MAX_TEXT: usize = 2000;

/// Stato condiviso: un canale broadcast a cui si iscrive ogni client.
#[derive(Clone)]
struct AppState {
    tx: broadcast::Sender<String>,
}

/// Messaggio che arriva dal client.
#[derive(Deserialize)]
struct ClientMsg {
    name: String,
    text: String,
}

#[tokio::main]
async fn main() {
    let (tx, _) = broadcast::channel::<String>(256);
    let state = AppState { tx };

    let app = Router::new()
        .route("/", get(index))
        .route("/ws", get(ws_handler))
        .with_state(state);

    let addr = std::env::var("HSTLE_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".to_string());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("impossibile aprire la porta");
    println!("Hstle in ascolto su http://{addr}");
    axum::serve(listener, app).await.expect("errore del server");
}

/// La pagina di test è incorporata nel binario.
async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();
    let tx = state.tx.clone();

    // Dal broadcast verso questo client
    let mut send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    if sender.send(Message::Text(msg.into())).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(_)) => continue, // client lento: salta i messaggi persi
                Err(RecvError::Closed) => break,
            }
        }
    });

    // Da questo client verso il broadcast
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                let Ok(m) = serde_json::from_str::<ClientMsg>(&text) else {
                    continue;
                };
                let name: String = m.name.trim().chars().take(MAX_NAME).collect();
                let body: String = m.text.chars().take(MAX_TEXT).collect();
                if name.is_empty() || body.trim().is_empty() {
                    continue;
                }
                let out = serde_json::json!({ "t": "message", "name": name, "text": body });
                let _ = tx.send(out.to_string());
            }
        }
    });

    // Quando uno dei due task finisce (disconnessione), chiudi l'altro
    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }
}
