//! File allegati ai messaggi.
//!
//! Il contenuto sta su disco, in una cartella accanto al database (`HSTLE_FILES`, di
//! default `<database>.files`); nel database restano nome, tipo e dimensione. Il caricamento
//! è a pezzi, direttamente su disco: un file grande non occupa memoria. Per scaricare serve
//! il token, e i file dei messaggi privati li vedono solo i due partecipanti.
//!
//! Limiti (variabili d'ambiente): `HSTLE_MAX_FILE_MB` per file (default 25) e
//! `HSTLE_FILES_TOTAL_MB` per tutti i file insieme (default 4096).

use crate::{auth, ApiError, AppState};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures_util::StreamExt;
use serde_json::json;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

pub struct Store {
    pub dir: PathBuf,
    pub max_file: u64,
    pub max_total: u64,
}

impl Store {
    pub fn from_env(db_path: &str) -> Self {
        let mb = |k: &str, d: u64| std::env::var(k).ok().and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(d) * 1024 * 1024;
        let dir = match std::env::var("HSTLE_FILES") {
            Ok(p) if !p.trim().is_empty() => PathBuf::from(p),
            _ => PathBuf::from(format!("{db_path}.files")),
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("impossibile creare la cartella dei file {}: {e}", dir.display());
        }
        Store {
            dir,
            max_file: mb("HSTLE_MAX_FILE_MB", 25),
            max_total: mb("HSTLE_FILES_TOTAL_MB", 4096),
        }
    }
}

/// Ogni dieci minuti toglie i file orfani (mai allegati, o di messaggi cancellati).
pub fn start_cleanup(state: AppState) {
    tokio::spawn(async move {
        loop {
            if let Ok(ids) = state.db.sweep_uploads().await {
                for id in ids {
                    let _ = tokio::fs::remove_file(state.files.dir.join(&id)).await;
                }
            }
            // File rimasti a metà (caricamento interrotto).
            if let Ok(mut rd) = tokio::fs::read_dir(&state.files.dir).await {
                while let Ok(Some(e)) = rd.next_entry().await {
                    let p = e.path();
                    if p.extension().is_some_and(|x| x == "part") {
                        if let Ok(age) = e.metadata().await.and_then(|m| m.modified()).map(|t| t.elapsed().unwrap_or_default()) {
                            if age.as_secs() > 3600 {
                                let _ = tokio::fs::remove_file(p).await;
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(600)).await;
        }
    });
}

/// Tipo dai primi byte: solo le immagini si mostrano nella chat, il resto è un download.
fn sniff(b: &[u8]) -> &'static str {
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if b.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        "image/png"
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        "image/gif"
    } else if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

/// Il nome arriva codificato (`encodeURIComponent`) nell'intestazione X-Filename.
fn decode_name(raw: &str) -> String {
    let b = raw.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && raw.is_char_boundary(i + 1) && raw.is_char_boundary(i + 3) {
            if let Ok(v) = u8::from_str_radix(&raw[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    let s = String::from_utf8_lossy(&out);
    let clean: String = s
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | '<' | '>' | '"' | ':' | '|' | '?' | '*'))
        .collect();
    let clean = clean.trim().trim_start_matches('.').trim();
    let clean: String = clean.chars().take(120).collect();
    if clean.is_empty() { "file".to_string() } else { clean }
}

pub async fn upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let user = auth::require_user(&state, &headers).await?;
    let too_big = ApiError(StatusCode::PAYLOAD_TOO_LARGE, "file troppo grande");
    let declared = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|n| n > state.files.max_file) {
        return Err(too_big);
    }
    let used = state.db.uploads_total().await?.max(0) as u64;
    if used + declared.unwrap_or(0) > state.files.max_total {
        return Err(ApiError(StatusCode::INSUFFICIENT_STORAGE, "lo spazio per i file sul server è esaurito"));
    }
    let name = decode_name(headers.get("x-filename").and_then(|v| v.to_str().ok()).unwrap_or(""));
    let id = auth::random_hex(16);
    let part = state.files.dir.join(format!("{id}.part"));
    let fin = state.files.dir.join(&id);
    let mut f = tokio::fs::File::create(&part).await.map_err(|_| ApiError::internal())?;
    let mut stream = body.into_data_stream();
    let (mut size, mut mime) = (0u64, None);
    let mut fail = None;
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            fail = Some(ApiError(StatusCode::BAD_REQUEST, "caricamento interrotto"));
            break;
        };
        if mime.is_none() && !chunk.is_empty() {
            mime = Some(sniff(&chunk));
        }
        size += chunk.len() as u64;
        if size > state.files.max_file || used + size > state.files.max_total {
            fail = Some(too_big);
            break;
        }
        if f.write_all(&chunk).await.is_err() {
            fail = Some(ApiError::internal());
            break;
        }
    }
    if fail.is_none() && size == 0 {
        fail = Some(ApiError(StatusCode::BAD_REQUEST, "file vuoto"));
    }
    if fail.is_none() && f.flush().await.is_err() {
        fail = Some(ApiError::internal());
    }
    drop(f);
    if let Some(e) = fail {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(e);
    }
    if tokio::fs::rename(&part, &fin).await.is_err() {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(ApiError::internal());
    }
    let mime = mime.unwrap_or("application/octet-stream").to_string();
    if let Err(e) = state.db.upload_add(id.clone(), user.id, name.clone(), mime.clone(), size as i64).await {
        let _ = tokio::fs::remove_file(&fin).await;
        return Err(e);
    }
    Ok(Json(json!({ "id": id, "name": name, "mime": mime, "size": size })).into_response())
}

pub async fn download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = auth::require_user(&state, &headers).await?;
    let nf = ApiError(StatusCode::NOT_FOUND, "file non trovato");
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(nf);
    }
    let Some(file) = state.db.file_for(id.clone(), user.id).await? else {
        return Err(nf);
    };
    let Ok(f) = tokio::fs::File::open(state.files.dir.join(&id)).await else {
        return Err(nf);
    };
    // Il file passa a pezzi dal disco alla rete, senza caricarlo tutto in memoria.
    let stream = futures_util::stream::unfold(f, |mut f| async move {
        use tokio::io::AsyncReadExt;
        let mut buf = vec![0u8; 64 * 1024];
        match f.read(&mut buf).await {
            Ok(0) | Err(_) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok::<_, std::io::Error>(axum::body::Bytes::from(buf)), f))
            }
        }
    });
    Ok((
        [
            (header::CONTENT_TYPE, file.mime),
            (header::CONTENT_LENGTH, file.size.to_string()),
            (header::CACHE_CONTROL, "private, max-age=31536000, immutable".to_string()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
            (header::CONTENT_SECURITY_POLICY, "sandbox".to_string()),
        ],
        Body::from_stream(stream),
    )
        .into_response())
}
