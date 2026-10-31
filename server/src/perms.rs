//! Permessi dei ruoli: una maschera di bit salvata nel database come intero.
//! I valori devono restare uguali a quelli della costante `P` in `static/index.html`.

pub const SEND_MESSAGES: i64 = 1 << 0; // scrivere e modificare i propri messaggi
pub const CONNECT_VOICE: i64 = 1 << 1; // entrare nel canale vocale
pub const MANAGE_MESSAGES: i64 = 1 << 2; // eliminare i messaggi degli altri
pub const MANAGE_CHANNELS: i64 = 1 << 3; // creare, rinominare, eliminare canali
pub const MANAGE_ROLES: i64 = 1 << 4; // creare ruoli e assegnarli (solo quelli più bassi del proprio)
pub const CREATE_INVITES: i64 = 1 << 5; // generare codici di invito
pub const ADMINISTRATOR: i64 = 1 << 6; // tutti i permessi
pub const ALL: i64 = (1 << 7) - 1;

/// Permessi del ruolo `everyone` alla prima creazione del database.
pub const EVERYONE_DEFAULT: i64 = SEND_MESSAGES | CONNECT_VOICE;

/// Permessi effettivi: il proprietario (primo utente) e chi ha `ADMINISTRATOR` hanno tutto.
pub fn effective(owner: bool, granted: i64) -> i64 {
    if owner || granted & ADMINISTRATOR != 0 {
        ALL
    } else {
        granted & ALL
    }
}
