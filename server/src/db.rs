use crate::{perms, ApiError};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Durata di una sessione (30 giorni).
pub const SESSION_TTL: i64 = 30 * 24 * 3600;

const MAX_CHANNELS: i64 = 50;
const MAX_ROLES: i64 = 50;

const DENIED: &str = "non hai il permesso di farlo";
const TOO_HIGH: &str = "quel ruolo è più in alto del tuo";
const CANT_GRANT: &str = "non puoi dare o togliere permessi che non hai";

/// Esito di un'operazione che può essere negata: `Err` è il messaggio per l'utente.
pub type Verdict<T> = Result<T, &'static str>;

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
    created_at INTEGER NOT NULL,
    edited_at  INTEGER
);
CREATE INDEX IF NOT EXISTS idx_messages_channel ON messages(channel_id, id);
CREATE TABLE IF NOT EXISTS roles (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE COLLATE NOCASE,
    color       TEXT NOT NULL DEFAULT '',
    permissions INTEGER NOT NULL,
    position    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS user_roles (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id INTEGER NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, role_id)
);
CREATE TABLE IF NOT EXISTS voice_channels (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);
INSERT INTO voice_channels(id, name) SELECT 1, 'vocale' WHERE NOT EXISTS (SELECT 1 FROM voice_channels);
-- Il canale iniziale si crea solo se non ce n'è nessuno (così #generale si può eliminare).
INSERT INTO channels(id, name) SELECT 1, 'generale' WHERE NOT EXISTS (SELECT 1 FROM channels);
CREATE TABLE IF NOT EXISTS dm_messages (
    id         INTEGER PRIMARY KEY,
    sender     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    recipient  INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    text       TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    edited_at  INTEGER
);
CREATE INDEX IF NOT EXISTS idx_dm_sender ON dm_messages(sender, recipient, id);
CREATE INDEX IF NOT EXISTS idx_dm_recipient ON dm_messages(recipient, sender, id);
-- Fino a quale messaggio privato di `peer_id` l'utente ha già letto.
CREATE TABLE IF NOT EXISTS dm_reads (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    peer_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    last_id INTEGER NOT NULL,
    PRIMARY KEY (user_id, peer_id)
);
";

/// Colonne del profilo, aggiunte ai database creati prima: (nome, definizione).
const PROFILE_COLUMNS: [(&str, &str); 5] = [
    ("display_name", "TEXT NOT NULL DEFAULT ''"),
    ("bio", "TEXT NOT NULL DEFAULT ''"),
    ("accent", "TEXT NOT NULL DEFAULT ''"),
    ("avatar", "BLOB"),
    ("avatar_mime", "TEXT NOT NULL DEFAULT ''"),
];

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
    /// Quando è stato modificato l'ultima volta (None = mai modificato).
    pub edited: Option<i64>,
}

#[derive(Serialize)]
pub struct Channel {
    pub id: i64,
    pub name: String,
}

#[derive(Serialize)]
pub struct RoleInfo {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub permissions: i64,
    pub position: i64,
}

#[derive(Serialize)]
pub struct Member {
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
    /// Ruoli assegnati (`everyone` è implicito e non compare).
    pub roles: Vec<i64>,
    /// Permessi effettivi, già calcolati.
    pub perms: i64,
    /// Profilo: nome visualizzato (vuoto = si usa il nome utente), presentazione, colore.
    pub display_name: String,
    pub bio: String,
    pub accent: String,
    /// Versione dell'avatar (0 = nessun avatar): serve a ricaricarlo quando cambia.
    pub avatar: i64,
}

/// Messaggio privato tra due utenti.
#[derive(Serialize)]
pub struct DmMsg {
    pub id: i64,
    pub from: i64,
    pub to: i64,
    pub text: String,
    pub ts: i64,
    pub edited: Option<i64>,
}

/// Una conversazione privata: con chi, l'ultimo messaggio e quanti ne mancano da leggere.
#[derive(Serialize)]
pub struct DmConv {
    pub user: i64,
    pub last_id: i64,
    pub last_ts: i64,
    pub unread: i64,
}

pub struct Avatar {
    pub mime: String,
    pub bytes: Vec<u8>,
}

pub struct Directory {
    pub roles: Vec<RoleInfo>,
    pub members: Vec<Member>,
}

/// Permessi di un utente e posizione del suo ruolo più alto (il proprietario sta sopra a tutti).
pub struct Access {
    perms: i64,
    top: i64,
    owner: bool,
}

impl Access {
    pub fn can(&self, p: i64) -> bool {
        (self.perms & p) == p
    }

    /// Si può agire su un ruolo solo se è più in basso del proprio ruolo più alto.
    fn outranks(&self, pos: i64) -> bool {
        self.owner || pos < self.top
    }
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
        conn.execute(
            "INSERT OR IGNORE INTO roles(id, name, color, permissions, position)
             VALUES (1, 'everyone', '', ?1, 0)",
            [perms::EVERYONE_DEFAULT],
        )?;
        // Database creati prima delle modifiche ai messaggi: aggiungi la colonna.
        let has_edited: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('messages') WHERE name = 'edited_at'",
            [],
            |r| r.get(0),
        )?;
        if has_edited == 0 {
            conn.execute("ALTER TABLE messages ADD COLUMN edited_at INTEGER", [])?;
        }
        // Profili (nome visualizzato, bio, colore, avatar): colonne aggiunte ai database esistenti.
        let mut cols: Vec<(&str, &str)> = PROFILE_COLUMNS.to_vec();
        cols.push(("avatar_v", "INTEGER NOT NULL DEFAULT 0"));
        for (name, ddl) in cols {
            let present: i64 = conn.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('users') WHERE name = ?1",
                [name],
                |r| r.get(0),
            )?;
            if present == 0 {
                conn.execute(&format!("ALTER TABLE users ADD COLUMN {name} {ddl}"), [])?;
            }
        }
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

    // ---------- permessi ----------

    /// Permessi e posizione gerarchica di un utente.
    pub async fn access(&self, user_id: i64) -> Result<Access, ApiError> {
        self.call(move |c| access_in(c, user_id)).await
    }

    /// Ruoli e membri (con ruoli e permessi effettivi): viene mandato a tutti i client.
    pub async fn directory(&self) -> Result<Directory, ApiError> {
        self.call(|c| {
            let mut stmt = c.prepare(
                "SELECT id, name, color, permissions, position FROM roles ORDER BY position DESC",
            )?;
            let roles = stmt
                .query_map([], |r| {
                    Ok(RoleInfo {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        color: r.get(2)?,
                        permissions: r.get(3)?,
                        position: r.get(4)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let role_perms: HashMap<i64, i64> =
                roles.iter().map(|r| (r.id, r.permissions)).collect();
            let everyone = role_perms.get(&1).copied().unwrap_or(0);

            let mut links: HashMap<i64, Vec<i64>> = HashMap::new();
            let mut stmt = c.prepare("SELECT user_id, role_id FROM user_roles ORDER BY role_id")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
            for row in rows {
                let (user, role) = row?;
                links.entry(user).or_default().push(role);
            }

            let mut stmt = c.prepare(
                "SELECT id, username, is_admin, display_name, bio, accent,
                        CASE WHEN avatar IS NULL THEN 0 ELSE avatar_v END
                 FROM users ORDER BY username COLLATE NOCASE",
            )?;
            let users = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)? != 0,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, i64>(6)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let members = users
                .into_iter()
                .map(|(id, username, is_admin, display_name, bio, accent, avatar)| {
                    let assigned = links.remove(&id).unwrap_or_default();
                    let granted = assigned
                        .iter()
                        .fold(everyone, |acc, rid| acc | role_perms.get(rid).copied().unwrap_or(0));
                    Member {
                        id,
                        username,
                        is_admin,
                        perms: perms::effective(is_admin, granted),
                        roles: assigned,
                        display_name,
                        bio,
                        accent,
                        avatar,
                    }
                })
                .collect();
            Ok(Directory { roles, members })
        })
        .await
    }

    // ---------- canali ----------

    pub async fn channels(&self) -> Result<Vec<Channel>, ApiError> {
        self.call(|c| {
            let mut stmt = c.prepare("SELECT id, name FROM channels ORDER BY id")?;
            let list = stmt
                .query_map([], |r| {
                    Ok(Channel {
                        id: r.get(0)?,
                        name: r.get(1)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(list)
        })
        .await
    }

    pub async fn channel_create(&self, actor: i64, name: String) -> Result<Verdict<()>, ApiError> {
        self.call(move |c| {
            if !access_in(c, actor)?.can(perms::MANAGE_CHANNELS) {
                return Ok(Err(DENIED));
            }
            let n: i64 = c.query_row("SELECT COUNT(*) FROM channels", [], |r| r.get(0))?;
            if n >= MAX_CHANNELS {
                return Ok(Err("hai raggiunto il numero massimo di canali"));
            }
            if channel_name_taken(c, &name, 0)? {
                return Ok(Err("esiste già un canale con questo nome"));
            }
            c.execute("INSERT INTO channels(name) VALUES (?1)", [&name])?;
            Ok(Ok(()))
        })
        .await
    }

    pub async fn channel_rename(
        &self,
        actor: i64,
        id: i64,
        name: String,
    ) -> Result<Verdict<()>, ApiError> {
        self.call(move |c| {
            if !access_in(c, actor)?.can(perms::MANAGE_CHANNELS) {
                return Ok(Err(DENIED));
            }
            if channel_name_taken(c, &name, id)? {
                return Ok(Err("esiste già un canale con questo nome"));
            }
            let n = c.execute("UPDATE channels SET name = ?1 WHERE id = ?2", params![name, id])?;
            if n == 0 {
                return Ok(Err("il canale non esiste"));
            }
            Ok(Ok(()))
        })
        .await
    }

    /// Elimina il canale con tutti i suoi messaggi. Ne deve restare almeno uno.
    pub async fn channel_delete(&self, actor: i64, id: i64) -> Result<Verdict<()>, ApiError> {
        self.call(move |c| {
            let tx = c.transaction()?;
            if !access_in(&tx, actor)?.can(perms::MANAGE_CHANNELS) {
                return Ok(Err(DENIED));
            }
            let n: i64 = tx.query_row("SELECT COUNT(*) FROM channels", [], |r| r.get(0))?;
            if n <= 1 {
                return Ok(Err("deve restare almeno un canale"));
            }
            tx.execute("DELETE FROM messages WHERE channel_id = ?1", [id])?;
            if tx.execute("DELETE FROM channels WHERE id = ?1", [id])? == 0 {
                return Ok(Err("il canale non esiste"));
            }
            tx.commit()?;
            Ok(Ok(()))
        })
        .await
    }

    // ---------- canali vocali ----------
    // Stessi permessi dei canali di testo ("Gestire i canali").

    pub async fn voice_channels(&self) -> Result<Vec<Channel>, ApiError> {
        self.call(|c| {
            let mut stmt = c.prepare("SELECT id, name FROM voice_channels ORDER BY id")?;
            let list = stmt
                .query_map([], |r| {
                    Ok(Channel {
                        id: r.get(0)?,
                        name: r.get(1)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(list)
        })
        .await
    }

    pub async fn voice_channel_create(
        &self,
        actor: i64,
        name: String,
    ) -> Result<Verdict<()>, ApiError> {
        self.call(move |c| {
            if !access_in(c, actor)?.can(perms::MANAGE_CHANNELS) {
                return Ok(Err(DENIED));
            }
            let n: i64 = c.query_row("SELECT COUNT(*) FROM voice_channels", [], |r| r.get(0))?;
            if n >= MAX_CHANNELS {
                return Ok(Err("hai raggiunto il numero massimo di canali vocali"));
            }
            if voice_name_taken(c, &name, 0)? {
                return Ok(Err("esiste già un canale vocale con questo nome"));
            }
            c.execute("INSERT INTO voice_channels(name) VALUES (?1)", [&name])?;
            Ok(Ok(()))
        })
        .await
    }

    pub async fn voice_channel_rename(
        &self,
        actor: i64,
        id: i64,
        name: String,
    ) -> Result<Verdict<()>, ApiError> {
        self.call(move |c| {
            if !access_in(c, actor)?.can(perms::MANAGE_CHANNELS) {
                return Ok(Err(DENIED));
            }
            if voice_name_taken(c, &name, id)? {
                return Ok(Err("esiste già un canale vocale con questo nome"));
            }
            let n = c.execute(
                "UPDATE voice_channels SET name = ?1 WHERE id = ?2",
                params![name, id],
            )?;
            if n == 0 {
                return Ok(Err("il canale vocale non esiste"));
            }
            Ok(Ok(()))
        })
        .await
    }

    /// Ne deve restare almeno uno.
    pub async fn voice_channel_delete(&self, actor: i64, id: i64) -> Result<Verdict<()>, ApiError> {
        self.call(move |c| {
            if !access_in(c, actor)?.can(perms::MANAGE_CHANNELS) {
                return Ok(Err(DENIED));
            }
            let n: i64 = c.query_row("SELECT COUNT(*) FROM voice_channels", [], |r| r.get(0))?;
            if n <= 1 {
                return Ok(Err("deve restare almeno un canale vocale"));
            }
            if c.execute("DELETE FROM voice_channels WHERE id = ?1", [id])? == 0 {
                return Ok(Err("il canale vocale non esiste"));
            }
            Ok(Ok(()))
        })
        .await
    }

    // ---------- messaggi ----------

    /// Salva un messaggio. Serve il permesso di scrivere.
    pub async fn send_message(
        &self,
        channel: i64,
        user: User,
        text: String,
    ) -> Result<Verdict<Msg>, ApiError> {
        self.call(move |c| {
            if !access_in(c, user.id)?.can(perms::SEND_MESSAGES) {
                return Ok(Err("non hai il permesso di scrivere"));
            }
            let exists: bool = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM channels WHERE id = ?1)",
                [channel],
                |r| r.get(0),
            )?;
            if !exists {
                return Ok(Err("il canale non esiste"));
            }
            let ts = now();
            c.execute(
                "INSERT INTO messages(channel_id, user_id, text, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![channel, user.id, text, ts],
            )?;
            Ok(Ok(Msg {
                id: c.last_insert_rowid(),
                channel,
                user: UserRef {
                    id: user.id,
                    username: user.username,
                },
                text,
                ts,
                edited: None,
            }))
        })
        .await
    }

    /// Modifica un messaggio: solo l'autore, e solo se può ancora scrivere.
    pub async fn edit_message(
        &self,
        user_id: i64,
        id: i64,
        text: String,
    ) -> Result<Verdict<Msg>, ApiError> {
        self.call(move |c| {
            let author: Option<i64> = c
                .query_row("SELECT user_id FROM messages WHERE id = ?1", [id], |r| r.get(0))
                .optional()?;
            let Some(author) = author else {
                return Ok(Err("il messaggio non esiste più"));
            };
            if author != user_id {
                return Ok(Err("puoi modificare solo i tuoi messaggi"));
            }
            if !access_in(c, user_id)?.can(perms::SEND_MESSAGES) {
                return Ok(Err("non hai il permesso di scrivere"));
            }
            c.execute(
                "UPDATE messages SET text = ?1, edited_at = ?2 WHERE id = ?3",
                params![text, now(), id],
            )?;
            let msg = c.query_row(&format!("{MSG_SELECT} WHERE m.id = ?1"), [id], row_msg)?;
            Ok(Ok(msg))
        })
        .await
    }

    /// Elimina un messaggio: l'autore, oppure chi ha "Gestire i messaggi".
    /// Restituisce l'id del canale a cui apparteneva.
    pub async fn delete_message(&self, user_id: i64, id: i64) -> Result<Verdict<i64>, ApiError> {
        self.call(move |c| {
            let row: Option<(i64, i64)> = c
                .query_row(
                    "SELECT user_id, channel_id FROM messages WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let Some((author, channel)) = row else {
                return Ok(Err("il messaggio non esiste più"));
            };
            if author != user_id && !access_in(c, user_id)?.can(perms::MANAGE_MESSAGES) {
                return Ok(Err("non puoi eliminare i messaggi degli altri"));
            }
            c.execute("DELETE FROM messages WHERE id = ?1", [id])?;
            Ok(Ok(channel))
        })
        .await
    }

    /// Gli ultimi `limit` messaggi del canale, dal più vecchio al più recente.
    pub async fn history(&self, channel: i64, limit: i64) -> Result<Vec<Msg>, ApiError> {
        self.call(move |c| {
            let mut stmt = c.prepare(&format!(
                "{MSG_SELECT} WHERE m.channel_id = ?1 ORDER BY m.id DESC LIMIT ?2"
            ))?;
            let mut v = stmt
                .query_map(params![channel, limit], row_msg)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            v.reverse();
            Ok(v)
        })
        .await
    }

    // ---------- ruoli ----------
    //
    // Regole (come su Discord): serve "Gestire i ruoli"; si può agire solo su ruoli
    // più in basso del proprio ruolo più alto; non si possono dare o togliere
    // permessi che non si hanno. Il proprietario (primo utente) è sopra a tutti.

    pub async fn role_create(
        &self,
        actor: i64,
        name: String,
        color: String,
        permissions: i64,
    ) -> Result<Verdict<()>, ApiError> {
        let permissions = permissions & perms::ALL;
        self.call(move |c| {
            let tx = c.transaction()?;
            let a = access_in(&tx, actor)?;
            if !a.can(perms::MANAGE_ROLES) {
                return Ok(Err(DENIED));
            }
            if !a.owner && a.top < 1 {
                return Ok(Err("per creare ruoli devi avere almeno un ruolo"));
            }
            if permissions & !a.perms != 0 {
                return Ok(Err(CANT_GRANT));
            }
            let n: i64 = tx.query_row("SELECT COUNT(*) FROM roles", [], |r| r.get(0))?;
            if n >= MAX_ROLES {
                return Ok(Err("hai raggiunto il numero massimo di ruoli"));
            }
            if role_name_taken(&tx, &name, 0)? {
                return Ok(Err("esiste già un ruolo con questo nome"));
            }
            // Il nuovo ruolo nasce subito sopra `everyone`, sotto tutti gli altri.
            tx.execute("UPDATE roles SET position = position + 1 WHERE id != 1", [])?;
            tx.execute(
                "INSERT INTO roles(name, color, permissions, position) VALUES (?1, ?2, ?3, 1)",
                params![name, color, permissions],
            )?;
            tx.commit()?;
            Ok(Ok(()))
        })
        .await
    }

    /// Per `everyone` (id 1) si possono cambiare solo i permessi.
    pub async fn role_edit(
        &self,
        actor: i64,
        id: i64,
        name: String,
        color: String,
        permissions: i64,
    ) -> Result<Verdict<()>, ApiError> {
        let permissions = permissions & perms::ALL;
        self.call(move |c| {
            let tx = c.transaction()?;
            let a = access_in(&tx, actor)?;
            if !a.can(perms::MANAGE_ROLES) {
                return Ok(Err(DENIED));
            }
            let old: Option<(i64, i64)> = tx
                .query_row(
                    "SELECT permissions, position FROM roles WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let Some((old_perms, pos)) = old else {
                return Ok(Err("il ruolo non esiste"));
            };
            if !a.outranks(pos) {
                return Ok(Err(TOO_HIGH));
            }
            if (old_perms ^ permissions) & !a.perms != 0 {
                return Ok(Err(CANT_GRANT));
            }
            if id == 1 {
                tx.execute("UPDATE roles SET permissions = ?1 WHERE id = 1", [permissions])?;
            } else {
                if role_name_taken(&tx, &name, id)? {
                    return Ok(Err("esiste già un ruolo con questo nome"));
                }
                tx.execute(
                    "UPDATE roles SET name = ?1, color = ?2, permissions = ?3 WHERE id = ?4",
                    params![name, color, permissions, id],
                )?;
            }
            tx.commit()?;
            Ok(Ok(()))
        })
        .await
    }

    pub async fn role_delete(&self, actor: i64, id: i64) -> Result<Verdict<()>, ApiError> {
        self.call(move |c| {
            let tx = c.transaction()?;
            let a = access_in(&tx, actor)?;
            if !a.can(perms::MANAGE_ROLES) {
                return Ok(Err(DENIED));
            }
            if id == 1 {
                return Ok(Err("il ruolo everyone non si può eliminare"));
            }
            let pos: Option<i64> = tx
                .query_row("SELECT position FROM roles WHERE id = ?1", [id], |r| r.get(0))
                .optional()?;
            let Some(pos) = pos else {
                return Ok(Err("il ruolo non esiste"));
            };
            if !a.outranks(pos) {
                return Ok(Err(TOO_HIGH));
            }
            tx.execute("DELETE FROM roles WHERE id = ?1", [id])?; // user_roles segue in cascata
            tx.commit()?;
            Ok(Ok(()))
        })
        .await
    }

    /// Sposta un ruolo di un gradino verso l'alto (più potente) o verso il basso.
    pub async fn role_move(&self, actor: i64, id: i64, up: bool) -> Result<Verdict<()>, ApiError> {
        self.call(move |c| {
            let tx = c.transaction()?;
            let a = access_in(&tx, actor)?;
            if !a.can(perms::MANAGE_ROLES) {
                return Ok(Err(DENIED));
            }
            if id == 1 {
                return Ok(Err("il ruolo everyone resta sempre in fondo"));
            }
            let pos: Option<i64> = tx
                .query_row("SELECT position FROM roles WHERE id = ?1", [id], |r| r.get(0))
                .optional()?;
            let Some(pos) = pos else {
                return Ok(Err("il ruolo non esiste"));
            };
            if !a.outranks(pos) {
                return Ok(Err(TOO_HIGH));
            }
            let neighbor: Option<(i64, i64)> = if up {
                tx.query_row(
                    "SELECT id, position FROM roles WHERE position > ?1 ORDER BY position ASC LIMIT 1",
                    [pos],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
            } else {
                tx.query_row(
                    "SELECT id, position FROM roles
                     WHERE position < ?1 AND id != 1 ORDER BY position DESC LIMIT 1",
                    [pos],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
            };
            let Some((other, other_pos)) = neighbor else {
                return Ok(Ok(())); // già in cima o in fondo
            };
            if !a.outranks(other_pos) {
                return Ok(Err(TOO_HIGH));
            }
            tx.execute("UPDATE roles SET position = ?1 WHERE id = ?2", params![other_pos, id])?;
            tx.execute("UPDATE roles SET position = ?1 WHERE id = ?2", params![pos, other])?;
            tx.commit()?;
            Ok(Ok(()))
        })
        .await
    }

    /// Imposta l'elenco completo dei ruoli di un utente (`everyone` c'è sempre).
    pub async fn set_member_roles(
        &self,
        actor: i64,
        target: i64,
        role_ids: Vec<i64>,
    ) -> Result<Verdict<()>, ApiError> {
        self.call(move |c| {
            let tx = c.transaction()?;
            let a = access_in(&tx, actor)?;
            if !a.can(perms::MANAGE_ROLES) {
                return Ok(Err(DENIED));
            }
            let target_owner: Option<bool> = tx
                .query_row("SELECT is_admin FROM users WHERE id = ?1", [target], |r| {
                    Ok(r.get::<_, i64>(0)? != 0)
                })
                .optional()?;
            let Some(target_owner) = target_owner else {
                return Ok(Err("utente non trovato"));
            };
            if target_owner {
                return Ok(Err("il proprietario ha già tutti i permessi"));
            }
            if target != actor && !a.outranks(access_in(&tx, target)?.top) {
                return Ok(Err("quell'utente ha un ruolo più alto del tuo"));
            }

            let wanted: BTreeSet<i64> = role_ids.into_iter().filter(|r| *r != 1).collect();
            let mut stmt = tx.prepare("SELECT role_id FROM user_roles WHERE user_id = ?1")?;
            let current = stmt
                .query_map([target], |r| r.get::<_, i64>(0))?
                .collect::<rusqlite::Result<BTreeSet<i64>>>()?;
            drop(stmt);

            // Solo i ruoli aggiunti o tolti devono stare sotto al ruolo di chi agisce.
            for rid in wanted.symmetric_difference(&current) {
                let pos: Option<i64> = tx
                    .query_row("SELECT position FROM roles WHERE id = ?1", [*rid], |r| r.get(0))
                    .optional()?;
                match pos {
                    None => return Ok(Err("il ruolo non esiste")),
                    Some(p) if !a.outranks(p) => return Ok(Err(TOO_HIGH)),
                    Some(_) => {}
                }
            }

            tx.execute("DELETE FROM user_roles WHERE user_id = ?1", [target])?;
            for rid in &wanted {
                tx.execute(
                    "INSERT INTO user_roles(user_id, role_id) VALUES (?1, ?2)",
                    params![target, rid],
                )?;
            }
            tx.commit()?;
            Ok(Ok(()))
        })
        .await
    }

    // ---------- profilo ----------

    pub async fn profile_update(
        &self,
        user_id: i64,
        display_name: String,
        bio: String,
        accent: String,
    ) -> Result<(), ApiError> {
        self.call(move |c| {
            c.execute(
                "UPDATE users SET display_name = ?1, bio = ?2, accent = ?3 WHERE id = ?4",
                params![display_name, bio, accent, user_id],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn avatar_set(&self, user_id: i64, mime: String, bytes: Vec<u8>) -> Result<(), ApiError> {
        self.call(move |c| {
            c.execute(
                "UPDATE users SET avatar = ?1, avatar_mime = ?2, avatar_v = avatar_v + 1 WHERE id = ?3",
                params![bytes, mime, user_id],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn avatar_clear(&self, user_id: i64) -> Result<(), ApiError> {
        self.call(move |c| {
            c.execute(
                "UPDATE users SET avatar = NULL, avatar_mime = '', avatar_v = avatar_v + 1 WHERE id = ?1",
                [user_id],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn avatar_get(&self, user_id: i64) -> Result<Option<Avatar>, ApiError> {
        self.call(move |c| {
            c.query_row(
                "SELECT avatar_mime, avatar FROM users WHERE id = ?1 AND avatar IS NOT NULL",
                [user_id],
                |r| {
                    Ok(Avatar {
                        mime: r.get(0)?,
                        bytes: r.get(1)?,
                    })
                },
            )
            .optional()
        })
        .await
    }

    // ---------- messaggi privati ----------
    //
    // Li vedono solo i due partecipanti: ogni interrogazione parte dall'utente che chiede.

    pub async fn dm_send(&self, from: i64, to: i64, text: String) -> Result<Verdict<DmMsg>, ApiError> {
        self.call(move |c| {
            if from == to {
                return Ok(Err("non puoi scrivere a te stesso"));
            }
            if !access_in(c, from)?.can(perms::SEND_MESSAGES) {
                return Ok(Err("non hai il permesso di scrivere"));
            }
            let exists: bool =
                c.query_row("SELECT EXISTS(SELECT 1 FROM users WHERE id = ?1)", [to], |r| r.get(0))?;
            if !exists {
                return Ok(Err("l'utente non esiste"));
            }
            let ts = now();
            c.execute(
                "INSERT INTO dm_messages(sender, recipient, text, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![from, to, text, ts],
            )?;
            Ok(Ok(DmMsg {
                id: c.last_insert_rowid(),
                from,
                to,
                text,
                ts,
                edited: None,
            }))
        })
        .await
    }

    pub async fn dm_edit(&self, user_id: i64, id: i64, text: String) -> Result<Verdict<DmMsg>, ApiError> {
        self.call(move |c| {
            let sender: Option<i64> = c
                .query_row("SELECT sender FROM dm_messages WHERE id = ?1", [id], |r| r.get(0))
                .optional()?;
            let Some(sender) = sender else {
                return Ok(Err("il messaggio non esiste più"));
            };
            if sender != user_id {
                return Ok(Err("puoi modificare solo i tuoi messaggi"));
            }
            if !access_in(c, user_id)?.can(perms::SEND_MESSAGES) {
                return Ok(Err("non hai il permesso di scrivere"));
            }
            c.execute(
                "UPDATE dm_messages SET text = ?1, edited_at = ?2 WHERE id = ?3",
                params![text, now(), id],
            )?;
            let msg = c.query_row(&format!("{DM_SELECT} WHERE id = ?1"), [id], row_dm)?;
            Ok(Ok(msg))
        })
        .await
    }

    /// Elimina un proprio messaggio privato. Restituisce (mittente, destinatario).
    pub async fn dm_delete(&self, user_id: i64, id: i64) -> Result<Verdict<(i64, i64)>, ApiError> {
        self.call(move |c| {
            let row: Option<(i64, i64)> = c
                .query_row(
                    "SELECT sender, recipient FROM dm_messages WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let Some((sender, recipient)) = row else {
                return Ok(Err("il messaggio non esiste più"));
            };
            if sender != user_id {
                return Ok(Err("puoi eliminare solo i tuoi messaggi"));
            }
            c.execute("DELETE FROM dm_messages WHERE id = ?1", [id])?;
            Ok(Ok((sender, recipient)))
        })
        .await
    }

    /// Gli ultimi `limit` messaggi tra `user` e `peer`, dal più vecchio al più recente.
    pub async fn dm_history(&self, user: i64, peer: i64, limit: i64) -> Result<Vec<DmMsg>, ApiError> {
        self.call(move |c| {
            let mut stmt = c.prepare(&format!(
                "{DM_SELECT} WHERE (sender = ?1 AND recipient = ?2) OR (sender = ?2 AND recipient = ?1)
                 ORDER BY id DESC LIMIT ?3"
            ))?;
            let mut v = stmt
                .query_map(params![user, peer, limit], row_dm)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            v.reverse();
            Ok(v)
        })
        .await
    }

    /// Segna come letti i messaggi che `peer` ha mandato a `user`.
    pub async fn dm_read(&self, user: i64, peer: i64) -> Result<(), ApiError> {
        self.call(move |c| {
            c.execute(
                "INSERT INTO dm_reads(user_id, peer_id, last_id)
                 VALUES (?1, ?2, COALESCE((SELECT MAX(id) FROM dm_messages WHERE sender = ?2 AND recipient = ?1), 0))
                 ON CONFLICT(user_id, peer_id) DO UPDATE SET last_id = excluded.last_id",
                params![user, peer],
            )?;
            Ok(())
        })
        .await
    }

    /// Le conversazioni di un utente, dalla più recente, con i messaggi non letti.
    pub async fn dm_summary(&self, user: i64) -> Result<Vec<DmConv>, ApiError> {
        self.call(move |c| {
            let mut stmt = c.prepare(
                "SELECT peer, MAX(id), MAX(created_at) FROM (
                     SELECT recipient AS peer, id, created_at FROM dm_messages WHERE sender = ?1
                     UNION ALL
                     SELECT sender AS peer, id, created_at FROM dm_messages WHERE recipient = ?1
                 ) GROUP BY peer ORDER BY MAX(id) DESC",
            )?;
            let rows = stmt
                .query_map([user], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut out = Vec::with_capacity(rows.len());
            for (peer, last_id, last_ts) in rows {
                let unread: i64 = c.query_row(
                    "SELECT COUNT(*) FROM dm_messages
                     WHERE sender = ?2 AND recipient = ?1
                       AND id > COALESCE((SELECT last_id FROM dm_reads WHERE user_id = ?1 AND peer_id = ?2), 0)",
                    params![user, peer],
                    |r| r.get(0),
                )?;
                out.push(DmConv { user: peer, last_id, last_ts, unread });
            }
            Ok(out)
        })
        .await
    }
}

// ---------- funzioni d'appoggio ----------

const DM_SELECT: &str =
    "SELECT id, sender, recipient, text, created_at, edited_at FROM dm_messages";

fn row_dm(r: &rusqlite::Row<'_>) -> rusqlite::Result<DmMsg> {
    Ok(DmMsg {
        id: r.get(0)?,
        from: r.get(1)?,
        to: r.get(2)?,
        text: r.get(3)?,
        ts: r.get(4)?,
        edited: r.get(5)?,
    })
}

const MSG_SELECT: &str = "SELECT m.id, m.channel_id, u.id, u.username, m.text, m.created_at, m.edited_at
     FROM messages m JOIN users u ON u.id = m.user_id";

fn row_msg(r: &rusqlite::Row<'_>) -> rusqlite::Result<Msg> {
    Ok(Msg {
        id: r.get(0)?,
        channel: r.get(1)?,
        user: UserRef {
            id: r.get(2)?,
            username: r.get(3)?,
        },
        text: r.get(4)?,
        ts: r.get(5)?,
        edited: r.get(6)?,
    })
}

/// Permessi effettivi di un utente: `everyone` + i suoi ruoli. Funziona anche dentro una transazione.
fn access_in(c: &Connection, user_id: i64) -> rusqlite::Result<Access> {
    let owner: Option<bool> = c
        .query_row("SELECT is_admin FROM users WHERE id = ?1", [user_id], |r| {
            Ok(r.get::<_, i64>(0)? != 0)
        })
        .optional()?;
    let Some(owner) = owner else {
        // Utente inesistente (account eliminato): nessun permesso.
        return Ok(Access {
            perms: 0,
            top: -1,
            owner: false,
        });
    };
    let mut stmt = c.prepare(
        "SELECT permissions, position FROM roles
         WHERE id = 1 OR id IN (SELECT role_id FROM user_roles WHERE user_id = ?1)",
    )?;
    let rows = stmt.query_map([user_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    let (mut granted, mut top) = (0i64, 0i64);
    for row in rows {
        let (p, pos) = row?;
        granted |= p;
        top = top.max(pos);
    }
    Ok(Access {
        perms: perms::effective(owner, granted),
        top: if owner { i64::MAX } else { top },
        owner,
    })
}

fn role_name_taken(c: &Connection, name: &str, except: i64) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM roles WHERE name = ?1 AND id != ?2)",
        params![name, except],
        |r| r.get(0),
    )
}

fn voice_name_taken(c: &Connection, name: &str, except: i64) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM voice_channels WHERE name = ?1 AND id != ?2)",
        params![name, except],
        |r| r.get(0),
    )
}

fn channel_name_taken(c: &Connection, name: &str, except: i64) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM channels WHERE name = ?1 AND id != ?2)",
        params![name, except],
        |r| r.get(0),
    )
}
