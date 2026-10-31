use crate::ApiError;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Durata di una sessione (30 giorni).
pub const SESSION_TTL: i64 = 30 * 24 * 3600;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS users (
    id            INTEGER PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    is_admin      INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
    token_hash TEXT PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS invites (
    code       TEXT PRIMARY KEY,
    created_by INTEGER NOT NULL REFERENCES users(id),
    used_by    INTEGER REFERENCES users(id),
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS channels (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS messages (
    id         INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL REFERENCES channels(id),
    user_id    INTEGER NOT NULL REFERENCES users(id),
    text       TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_messages_channel ON messages(channel_id, id);
INSERT OR IGNORE INTO channels(id, name) VALUES (1, 'generale');
";

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Clone, Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
}

#[derive(Serialize)]
pub struct UserRef {
    pub id: i64,
    pub username: String,
}

#[derive(Serialize)]
pub struct Msg {
    pub id: i64,
    pub channel: i64,
    pub user: UserRef,
    pub text: String,
    pub ts: i64,
}

pub enum Register {
    Ok(User),
    NameTaken,
    BadInvite,
}

/// Un'unica connessione SQLite protetta da un Mutex: per una decina di utenti
/// basta e avanza, e occupa pochissima memoria.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Esegue una query su un thread dedicato, senza bloccare il runtime async.
    async fn call<T, F>(&self, f: F) -> Result<T, ApiError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> rusqlite::Result<T> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        let res = tokio::task::spawn_blocking(move || {
            let mut guard = conn.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut guard)
        })
        .await;
        match res {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => {
                eprintln!("errore database: {e}");
                Err(ApiError::internal())
            }
            Err(_) => Err(ApiError::internal()),
        }
    }

    /// Crea un utente. Il primo utente in assoluto diventa admin e non ha
    /// bisogno di invito; tutti gli altri devono presentare un invito valido.
    pub async fn register(
        &self,
        username: String,
        password_hash: String,
        invite: Option<String>,
    ) -> Result<Register, ApiError> {
        self.call(move |c| {
            let tx = c.transaction()?;
            let count: i64 = tx.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
            let first = count == 0;
            let code = invite.unwrap_or_default();

            if !first {
                let valid: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM invites WHERE code = ?1 AND used_by IS NULL)",
                    [&code],
                    |r| r.get(0),
                )?;
                if !valid {
                    return Ok(Register::BadInvite);
                }
            }

            let taken: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM users WHERE username = ?1)",
                [&username],
                |r| r.get(0),
            )?;
            if taken {
                return Ok(Register::NameTaken); // la transazione viene annullata: l'invito non si "brucia"
            }

            tx.execute(
                "INSERT INTO users(username, password_hash, is_admin, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![username, password_hash, first as i64, now()],
            )?;
            let id = tx.last_insert_rowid();

            if !first {
                let n = tx.execute(
                    "UPDATE invites SET used_by = ?1 WHERE code = ?2 AND used_by IS NULL",
                    params![id, code],
                )?;
                if n != 1 {
                    return Ok(Register::BadInvite);
                }
            }
            tx.commit()?;
            Ok(Register::Ok(User {
                id,
                username,
                is_admin: first,
            }))
        })
        .await
    }

    /// Restituisce l'utente e il suo hash della password, se esiste.
    pub async fn find_login(&self, username: String) -> Result<Option<(User, String)>, ApiError> {
        self.call(move |c| {
            c.query_row(
                "SELECT id, username, is_admin, password_hash FROM users WHERE username = ?1",
                [&username],
                |r| {
                    Ok((
                        User {
                            id: r.get(0)?,
                            username: r.get(1)?,
                            is_admin: r.get::<_, i64>(2)? != 0,
                        },
                        r.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
        })
        .await
    }

    pub async fn create_session(&self, user_id: i64, token_hash: String) -> Result<(), ApiError> {
        self.call(move |c| {
            let t = now();
            c.execute(
                "DELETE FROM sessions WHERE created_at < ?1",
                [t - SESSION_TTL],
            )?;
            c.execute(
                "INSERT INTO sessions(token_hash, user_id, created_at) VALUES (?1, ?2, ?3)",
                params![token_hash, user_id, t],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn user_by_token(&self, token_hash: String) -> Result<Option<User>, ApiError> {
        self.call(move |c| {
            c.query_row(
                "SELECT u.id, u.username, u.is_admin
                 FROM sessions s JOIN users u ON u.id = s.user_id
                 WHERE s.token_hash = ?1 AND s.created_at > ?2",
                params![token_hash, now() - SESSION_TTL],
                |r| {
                    Ok(User {
                        id: r.get(0)?,
                        username: r.get(1)?,
                        is_admin: r.get::<_, i64>(2)? != 0,
                    })
                },
            )
            .optional()
        })
        .await
    }

    pub async fn delete_session(&self, token_hash: String) -> Result<(), ApiError> {
        self.call(move |c| {
            c.execute("DELETE FROM sessions WHERE token_hash = ?1", [&token_hash])?;
            Ok(())
        })
        .await
    }

    pub async fn create_invite(&self, code: String, created_by: i64) -> Result<(), ApiError> {
        self.call(move |c| {
            c.execute(
                "INSERT INTO invites(code, created_by, created_at) VALUES (?1, ?2, ?3)",
                params![code, created_by, now()],
            )?;
            Ok(())
        })
        .await
    }

    /// Salva un messaggio. Restituisce None se il canale non esiste.
    pub async fn insert_message(
        &self,
        channel: i64,
        user: User,
        text: String,
    ) -> Result<Option<Msg>, ApiError> {
        self.call(move |c| {
            let exists: bool = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM channels WHERE id = ?1)",
                [channel],
                |r| r.get(0),
            )?;
            if !exists {
                return Ok(None);
            }
            let ts = now();
            c.execute(
                "INSERT INTO messages(channel_id, user_id, text, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![channel, user.id, text, ts],
            )?;
            Ok(Some(Msg {
                id: c.last_insert_rowid(),
                channel,
                user: UserRef {
                    id: user.id,
                    username: user.username,
                },
                text,
                ts,
            }))
        })
        .await
    }

    /// Gli ultimi `limit` messaggi del canale, dal più vecchio al più recente.
    pub async fn history(&self, channel: i64, limit: i64) -> Result<Vec<Msg>, ApiError> {
        self.call(move |c| {
            let mut stmt = c.prepare(
                "SELECT m.id, m.channel_id, u.id, u.username, m.text, m.created_at
                 FROM messages m JOIN users u ON u.id = m.user_id
                 WHERE m.channel_id = ?1
                 ORDER BY m.id DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![channel, limit], |r| {
                Ok(Msg {
                    id: r.get(0)?,
                    channel: r.get(1)?,
                    user: UserRef {
                        id: r.get(2)?,
                        username: r.get(3)?,
                    },
                    text: r.get(4)?,
                    ts: r.get(5)?,
                })
            })?;
            let mut v = rows.collect::<rusqlite::Result<Vec<_>>>()?;
            v.reverse();
            Ok(v)
        })
        .await
    }
}
