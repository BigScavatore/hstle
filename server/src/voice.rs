use crate::db::User;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use tokio::sync::mpsc;

/// Con la mesh P2P ognuno invia il proprio video a tutti gli altri:
/// oltre 8 persone la banda in upload diventa il problema.
pub const MAX_MEMBERS: usize = 8;

struct Member {
    /// Connessione WebSocket con cui questa persona è entrata nel canale.
    conn: u64,
    username: String,
    muted: bool,
    cam: bool,
    screen: bool,
    /// Canale privato verso quella connessione (segnalazione WebRTC, "kicked").
    tx: mpsc::Sender<String>,
}

/// Un unico canale vocale, solo in memoria. Il server non vede mai audio né video:
/// tiene l'elenco di chi c'è e inoltra i messaggi con cui i browser si accordano.
/// Ogni persona (id utente) può stare nel canale con una sola connessione alla volta.
#[derive(Default)]
pub struct Voice {
    /// Versione dello stato: cresce a ogni cambio, così il client scarta gli stati vecchi.
    v: u64,
    members: BTreeMap<i64, Member>,
}

impl Voice {
    pub fn state_json(&self) -> Value {
        let members: Vec<Value> = self
            .members
            .iter()
            .map(|(id, m)| {
                json!({
                    "id": id,
                    "username": m.username,
                    "muted": m.muted,
                    "cam": m.cam,
                    "screen": m.screen,
                })
            })
            .collect();
        json!({ "t": "voice_state", "v": self.v, "members": members })
    }

    /// Fa entrare `user` nel canale. Restituisce `(voice_joined per chi entra, voice_state per tutti)`.
    /// Se la stessa persona era già dentro da un'altra connessione, quella viene espulsa.
    pub fn join(
        &mut self,
        conn: u64,
        user: &User,
        tx: mpsc::Sender<String>,
    ) -> Result<(Value, Value), &'static str> {
        if !self.members.contains_key(&user.id) && self.members.len() >= MAX_MEMBERS {
            return Err("Il canale vocale è pieno.");
        }
        if let Some(old) = self.members.get(&user.id) {
            if old.conn != conn {
                let _ = old.tx.try_send(json!({ "t": "voice_kicked" }).to_string());
            }
        }
        self.members.insert(
            user.id,
            Member {
                conn,
                username: user.username.clone(),
                muted: false,
                cam: false,
                screen: false,
                tx,
            },
        );
        self.v += 1;
        let peers: Vec<Value> = self
            .members
            .iter()
            .filter(|(id, _)| **id != user.id)
            .map(|(id, m)| json!({ "id": id, "username": m.username }))
            .collect();
        let joined = json!({ "t": "voice_joined", "v": self.v, "peers": peers });
        Ok((joined, self.state_json()))
    }

    /// Esce dal canale (solo se la connessione è proprio quella con cui si era entrati).
    /// Restituisce il nuovo stato da mandare a tutti, se qualcosa è cambiato.
    pub fn leave(&mut self, conn: u64, user_id: i64) -> Option<Value> {
        match self.members.get(&user_id) {
            Some(m) if m.conn == conn => {
                self.members.remove(&user_id);
                self.v += 1;
                Some(self.state_json())
            }
            _ => None,
        }
    }

    /// Aggiorna microfono/camera/schermo di chi è nel canale.
    pub fn set_meta(
        &mut self,
        conn: u64,
        user_id: i64,
        muted: bool,
        cam: bool,
        screen: bool,
    ) -> Option<Value> {
        match self.members.get_mut(&user_id) {
            Some(m) if m.conn == conn => {
                m.muted = muted;
                m.cam = cam;
                m.screen = screen;
                self.v += 1;
                Some(self.state_json())
            }
            _ => None,
        }
    }

    /// Inoltra un messaggio di segnalazione, solo tra due membri del canale.
    pub fn signal(&self, conn: u64, from: i64, to: i64, data: Value) {
        let Some(sender) = self.members.get(&from) else { return };
        if sender.conn != conn || from == to {
            return;
        }
        if let Some(target) = self.members.get(&to) {
            let msg = json!({ "t": "signal", "from": from, "data": data });
            let _ = target.tx.try_send(msg.to_string());
        }
    }
}
