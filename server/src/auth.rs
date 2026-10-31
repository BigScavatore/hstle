use crate::{
    db::{Db, Register, User},
    ApiError, AppState,
};
use argon2::{
    password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier},
    Argon2,
};
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_PASSWORD: usize = 128;
const MIN_PASSWORD: usize = 8;

// ---------- utilità ----------

/// `bytes` byte casuali dal sistema operativo, scritti in esadecimale.
pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("generatore casuale non disponibile");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// Nel database salviamo solo l'hash del token: se il file .db trapela,
/// i token delle sessioni non sono utilizzabili.
pub fn hash_token(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn hash_password(password: &str) -> Result<String, ApiError> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|_| ApiError::internal())
}

fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

fn valid_username(u: &str) -> bool {
    (3..=32).contains(&u.len())
        && u.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

async fn new_session(db: &Db, user_id: i64) -> Result<String, ApiError> {
    let token = random_hex(32);
    db.create_session(user_id, hash_token(&token)).await?;
    Ok(token)
}

/// Utente associato a un token (usato anche dal WebSocket).
pub async fn user_from_token(db: &Db, token: &str) -> Option<User> {
    db.user_by_token(hash_token(token)).await.ok().flatten()
}

async fn require_user(state: &AppState, headers: &HeaderMap) -> Result<User, ApiError> {
    let unauth = || ApiError(StatusCode::UNAUTHORIZED, "non autenticato");
    let token = bearer(headers).ok_or_else(unauth)?;
    user_from_token(&state.db, token).await.ok_or_else(unauth)
}

// ---------- handler ----------

#[derive(Deserialize)]
pub struct Credentials {
    username: String,
    password: String,
    invite: Option<String>,
}

#[derive(Serialize)]
pub struct AuthResponse {
    token: String,
    user: User,
}

pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<Credentials>,
) -> Result<Json<AuthResponse>, ApiError> {
    let username = req.username.trim().to_string();
    if !valid_username(&username) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "nome utente non valido (3-32 caratteri: lettere, numeri, _ - .)",
        ));
    }
    if req.password.len() < MIN_PASSWORD || req.password.len() > MAX_PASSWORD {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "la password deve avere da 8 a 128 caratteri",
        ));
    }
    let invite = req
        .invite
        .map(|i| i.trim().to_lowercase())
        .filter(|i| !i.is_empty());

    // L'hashing è volutamente lento: va fuori dal thread async.
    let password = req.password;
    let hash = tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|_| ApiError::internal())??;

    match state.db.register(username, hash, invite).await? {
        Register::Ok(user) => {
            let token = new_session(&state.db, user.id).await?;
            Ok(Json(AuthResponse { token, user }))
        }
        Register::NameTaken => Err(ApiError(StatusCode::CONFLICT, "nome utente già in uso")),
        Register::BadInvite => Err(ApiError(
            StatusCode::FORBIDDEN,
            "codice invito non valido o già usato",
        )),
    }
}

pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<Credentials>,
) -> Result<Json<AuthResponse>, ApiError> {
    let bad = || ApiError(StatusCode::UNAUTHORIZED, "nome utente o password errati");
    if req.password.len() > MAX_PASSWORD {
        return Err(bad());
    }
    let password = req.password;
    let found = state.db.find_login(req.username.trim().to_string()).await?;

    let Some((user, hash)) = found else {
        // Utente inesistente: facciamo comunque un hash per non rivelare,
        // dai tempi di risposta, quali nomi esistono.
        let _ = tokio::task::spawn_blocking(move || hash_password(&password)).await;
        return Err(bad());
    };

    let ok = tokio::task::spawn_blocking(move || verify_password(&password, &hash))
        .await
        .unwrap_or(false);
    if !ok {
        return Err(bad());
    }
    let token = new_session(&state.db, user.id).await?;
    Ok(Json(AuthResponse { token, user }))
}

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    if let Some(token) = bearer(&headers) {
        state.db.delete_session(hash_token(token)).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn create_invite(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user = require_user(&state, &headers).await?;
    if !user.is_admin {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "solo gli admin possono creare inviti",
        ));
    }
    let code = random_hex(8);
    state.db.create_invite(code.clone(), user.id).await?;
    Ok(Json(serde_json::json!({ "code": code })))
}
