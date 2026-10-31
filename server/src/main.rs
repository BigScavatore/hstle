mod auth;
mod db;
mod voice;
mod ws;

use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::sync::{atomic::AtomicU64, Arc, Mutex};
use tokio::sync::broadcast;

/// Stato condiviso tra tutte le richieste.
#[derive(Clone)]
pub struct AppState {
    pub db: db::Db,
    /// Canale broadcast: ogni client connesso si iscrive e riceve i nuovi messaggi.
    pub tx: broadcast::Sender<String>,
    /// Chi è nel canale vocale (solo in memoria).
    pub voice: Arc<Mutex<voice::Voice>>,
    /// Contatore per dare un id a ogni connessione WebSocket.
    pub next_conn: Arc<AtomicU64>,
    /// Server STUN/TURN che i client usano per la voce.
    pub ice: Arc<Value>,
}

/// Errore restituito come JSON `{"error": "..."}` con il codice HTTP indicato.
pub struct ApiError(pub StatusCode, pub &'static str);

impl ApiError {
    pub fn internal() -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, "errore interno")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

/// Server ICE per WebRTC. Di default usa lo STUN pubblico di Google; per un TURN
/// proprio imposta HSTLE_ICE con un array JSON, per esempio:
/// [{"urls":["turn:esempio.it:3478"],"username":"u","credential":"p"}]
fn ice_servers() -> Value {
    let default = json!([{ "urls": ["stun:stun.l.google.com:19302"] }]);
    match std::env::var("HSTLE_ICE") {
        Ok(s) if !s.trim().is_empty() => match serde_json::from_str::<Value>(&s) {
            Ok(v) if v.is_array() => v,
            _ => {
                eprintln!("HSTLE_ICE non è un array JSON valido: uso il default");
                default
            }
        },
        _ => default,
    }
}

#[tokio::main]
async fn main() {
    let db_path = std::env::var("HSTLE_DB").unwrap_or_else(|_| "hstle.db".to_string());
    let db = db::Db::open(&db_path).expect("impossibile aprire il database");
    let (tx, _) = broadcast::channel::<String>(256);

    let state = AppState {
        db,
        tx,
        voice: Arc::new(Mutex::new(voice::Voice::default())),
        next_conn: Arc::new(AtomicU64::new(1)),
        ice: Arc::new(ice_servers()),
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/api/register", post(auth::register))
        .route("/api/login", post(auth::login))
        .route("/api/logout", post(auth::logout))
        .route("/api/invites", post(auth::create_invite))
        .route("/ws", get(ws::ws_handler))
        .with_state(state);

    let addr = std::env::var("HSTLE_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".to_string());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("impossibile aprire la porta");
    println!("Hstle in ascolto su http://{addr} (database: {db_path})");
    axum::serve(listener, app).await.expect("errore del server");
}

/// La pagina del client è incorporata nel binario.
async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}
