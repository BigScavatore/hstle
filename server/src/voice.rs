use crate::db::User;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use tokio::sync::mpsc;

/// Con la mesh P2P ognuno invia il proprio video a tutti gli altri:
/// oltre 8 persone nello stesso canale la banda in upload diventa il problema.
pub const MAX_MEMBERS: usize = 8;

struct Member {
    /// Connessione WebSocket con cui questa persona è entrata in un canale vocale.
    conn: u64,
    /// Canale vocale in cui si trova.
    channel: i64,
    username: String,
    muted: bool,
    /// Cuffie mute: non sente gli altri (e di norma ha anche il microfono spento).
    deaf: bool,
    cam: bool,
    screen: bool,
    /// Canale privato verso quella connessione (segnalazione WebRTC, "kicked").
    tx: mpsc::Sender<String>,
}

/// Chi c'è nei canali vocali, solo in memoria. Il server non vede mai audio né video:
/// tiene l'elenco di chi c'è e inoltra i messaggi con cui i browser si accordano.
/// Ogni persona (id utente) può stare in un solo canale vocale, con una sola connessione.
/// Che i canali esistano lo decide il database: qui si tiene solo chi è dentro.
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
                    "channel": m.channel,
                    "muted": m.muted,
                    "deaf": m.deaf,
                    "cam": m.cam,
                    "screen": m.screen,
                })
            })
            .collect();
        json!({ "t": "voice_state", "v": self.v, "members": members })
    }

    /// Fa entrare `user` nel canale vocale `channel`. Restituisce
    /// `(voice_joined per chi entra, voice_state per tutti)`.
    /// Se la stessa persona era già dentro da un'altra connessione, quella viene espulsa.
    pub fn join(
        &mut self,
        conn: u64,
        user: &User,
        channel: i64,
        tx: mpsc::Sender<String>,
    ) -> Result<(Value, Value), &'static str> {
        let already_here = self
            .members
            .get(&user.id)
            .is_some_and(|m| m.channel == channel);
        let in_channel = self.members.values().filter(|m| m.channel == channel).count();
        if !already_here && in_channel >= MAX_MEMBERS {
            return Err("Il canale vocale è pieno.");
        }
        if let Some(old) = self.members.get(&user.id) {
            if old.conn != conn {
                let msg = json!({
                    "t": "voice_kicked",
                    "reason": "Sei entrato nel canale vocale da un altro dispositivo."
                });
                let _ = old.tx.try_send(msg.to_string());
            }
        }
        self.members.insert(
            user.id,
            Member {
                conn,
                channel,
                username: user.username.clone(),
                muted: false,
                deaf: false,
                cam: false,
                screen: false,
                tx,
            },
        );
        self.v += 1;
        // Ci si collega solo con chi è nello stesso canale.
        let peers: Vec<Value> = self
            .members
            .iter()
            .filter(|(id, m)| **id != user.id && m.channel == channel)
            .map(|(id, m)| json!({ "id": id, "username": m.username }))
            .collect();
        let joined = json!({ "t": "voice_joined", "v": self.v, "channel": channel, "peers": peers });
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

    /// Il canale è stato eliminato: chi era dentro viene fatto uscire.
    pub fn drop_channel(&mut self, channel: i64) -> Option<Value> {
        let ids: Vec<i64> = self
            .members
            .iter()
            .filter(|(_, m)| m.channel == channel)
            .map(|(id, _)| *id)
            .collect();
        if ids.is_empty() {
            return None;
        }
        for id in ids {
            if let Some(m) = self.members.remove(&id) {
                let msg = json!({
                    "t": "voice_kicked",
                    "reason": "Il canale vocale è stato eliminato."
                });
                let _ = m.tx.try_send(msg.to_string());
            }
        }
        self.v += 1;
        Some(self.state_json())
    }

    /// Aggiorna microfono/camera/schermo di chi è nel canale.
    pub fn set_meta(
        &mut self,
        conn: u64,
        user_id: i64,
        muted: bool,
        deaf: bool,
        cam: bool,
        screen: bool,
    ) -> Option<Value> {
        match self.members.get_mut(&user_id) {
            Some(m) if m.conn == conn => {
                m.muted = muted;
                m.deaf = deaf;
                m.cam = cam;
                m.screen = screen;
                self.v += 1;
                Some(self.state_json())
            }
            _ => None,
        }
    }

    /// Inoltra un messaggio di segnalazione, solo tra due membri dello stesso canale.
    pub fn signal(&self, conn: u64, from: i64, to: i64, data: Value) {
        let Some(sender) = self.members.get(&from) else { return };
        if sender.conn != conn || from == to {
            return;
        }
        if let Some(target) = self.members.get(&to) {
            if target.channel != sender.channel {
                return;
            }
            let msg = json!({ "t": "signal", "from": from, "data": data });
            let _ = target.tx.try_send(msg.to_string());
        }
    }
}
