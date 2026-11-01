//! Avatar degli utenti. Il resto del profilo (nome, presentazione, colore) passa dal WebSocket.
//!
//! Le immagini arrivano già ridimensionate dal browser (poche decine di KB); qui si
//! controlla solo che siano davvero JPEG, PNG o WebP e che non siano troppo grandi.
//! Per leggerle serve il token: non sono pubbliche.

use crate::{auth::require_user, ws, ApiError, AppState};
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

const MAX_AVATAR: usize = 128 * 1024;

/// Riconosce il formato dai primi byte (non ci si fida del Content-Type dichiarato).
fn sniff(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if b.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

pub async fn set_avatar(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let user = require_user(&state, &headers).await?;
    if body.is_empty() || body.len() > MAX_AVATAR {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "immagine troppo grande (massimo 128 KB)",
        ));
    }
    let Some(mime) = sniff(&body) else {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "formato non supportato (JPEG, PNG o WebP)",
        ));
    };
    state.db.avatar_set(user.id, mime.to_string(), body.to_vec()).await?;
    ws::broadcast_directory(&state).await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn clear_avatar(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let user = require_user(&state, &headers).await?;
    state.db.avatar_clear(user.id).await?;
    ws::broadcast_directory(&state).await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_avatar(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    require_user(&state, &headers).await?;
    let Some(avatar) = state.db.avatar_get(id).await? else {
        return Err(ApiError(StatusCode::NOT_FOUND, "nessun avatar"));
    };
    Ok((
        [
            (header::CONTENT_TYPE, avatar.mime),
            // Il client aggiunge ?v=<versione>: cambiando avatar cambia l'indirizzo.
            (header::CACHE_CONTROL, "private, max-age=31536000, immutable".to_string()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        ],
        avatar.bytes,
    )
        .into_response())
}
