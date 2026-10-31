# Hstle

Chat self-hostata leggera in stile Discord, scritta in Rust.
Pensata per pochi utenti e server con poca RAM.

## Stato attuale

- Server Rust (axum + WebSocket) con database SQLite
- Account con password (argon2), sessioni a token, registrazione solo su invito
- Il primo utente registrato è il proprietario: ha sempre tutti i permessi
- Più canali di testo (creare, rinominare, eliminare) con cronologia salvata
- Ruoli con permessi, gerarchia e colore; assegnazione ai membri
- Modifica ed eliminazione dei messaggi inviati
- Client web incorporato nel binario
- Più canali vocali (creare, rinominare, eliminare) con audio, webcam e condivisione schermo (WebRTC mesh, fino a 8 persone per canale)

## Avvio

```powershell
cd server
cargo run
```

Apri http://127.0.0.1:3000, vai su **Registrati** e crea il primo account
(non serve nessun invito: diventa admin). Poi, con il pulsante **Crea invito**,
generi i codici da dare agli altri.

Variabili d'ambiente:

| Variabile    | Default          | Significato                  |
|--------------|------------------|------------------------------|
| `HSTLE_ADDR` | `127.0.0.1:3000` | indirizzo e porta di ascolto |
| `HSTLE_DB`   | `hstle.db`       | percorso del file database   |
| `HSTLE_ICE`  | STUN di Google   | server STUN/TURN per la voce (array JSON) |

```powershell
$env:HSTLE_ADDR = "0.0.0.0:3000"; cargo run
```

Il database (`hstle.db`) è un singolo file: per il backup basta copiarlo
a server fermo.

## Protocollo WebSocket

Connessione su `/ws`. Il primo messaggio del client deve essere l'autenticazione:

```json
{"t":"auth","token":"..."}
```

Il server risponde con `ready` (utente), `channels` (elenco dei canali),
`directory` (ruoli, membri e permessi di ognuno) e `voice_state`. La cronologia
di un canale si chiede quando serve: `{"t":"load_history","channel":1}` →
`{"t":"history","channel":1,"messages":[...]}` (ultimi 50).

Messaggi dal client:

| Messaggio | Permesso richiesto |
|-----------|--------------------|
| `{"t":"send","channel":1,"text":"ciao"}` | Scrivere messaggi |
| `{"t":"edit","id":42,"text":"..."}` | Scrivere messaggi, e solo sui propri |
| `{"t":"delete","id":42}` | nessuno sui propri; *Gestire i messaggi* sugli altri |
| `{"t":"channel_create","name":"..."}`, `channel_rename` (`id`,`name`), `channel_delete` (`id`) | Gestire i canali |
| `{"t":"role_create","name","color","permissions"}`, `role_edit` (`id` + gli stessi campi), `role_delete` (`id`), `role_move` (`id`,`up`) | Gestire i ruoli |
| `{"t":"member_roles","user":3,"roles":[2,5]}` | Gestire i ruoli |

Eventi dal server a tutti: `message`, `message_edited` (messaggio completo con
`edited`), `message_deleted` (`channel`,`id`), `channels`, `directory`. Se
un'azione viene rifiutata, solo chi l'ha chiesta riceve
`{"t":"notice","error":"..."}`.

I nomi dei canali vengono normalizzati (minuscolo, spazi → `-`, 1-32 caratteri
tra lettere, numeri, `-` e `_`). Si può eliminare anche `#generale`, ma ne deve
restare almeno uno; eliminando un canale si cancellano i suoi messaggi.

## Ruoli e permessi

Ogni utente ha i permessi del ruolo `everyone` (all'inizio: scrivere e usare la
voce) più quelli dei ruoli che gli vengono assegnati. I permessi sono bit
(`server/src/perms.rs`):

| Permesso | Bit | Cosa permette |
|----------|-----|---------------|
| Scrivere messaggi | 1 | scrivere e modificare i propri messaggi |
| Canale vocale | 2 | entrare nel canale vocale |
| Gestire i messaggi | 4 | eliminare i messaggi degli altri |
| Gestire i canali | 8 | creare, rinominare, eliminare canali |
| Gestire i ruoli | 16 | creare/modificare/assegnare ruoli |
| Creare inviti | 32 | `POST /api/invites` |
| Amministratore | 64 | tutti i permessi |

Regole della gerarchia (le stesse di Discord): i ruoli hanno una posizione
(si spostano con le frecce nella finestra *Ruoli e membri*); chi gestisce i
ruoli può agire solo su ruoli **più in basso** del proprio ruolo più alto, non
può dare o togliere permessi che non ha, e non può modificare chi sta allo
stesso livello o sopra. Il proprietario sta sopra a tutti e non è modificabile.
I permessi si controllano sempre nel server a ogni azione; il client li usa solo
per mostrare o nascondere i pulsanti.

Database già esistenti: all'avvio vengono aggiunte da sole le tabelle dei ruoli
e la colonna della data di modifica, senza perdere nulla.

## Voce e video

La voce usa WebRTC in modalità mesh: ogni partecipante si collega direttamente a
tutti gli altri, quindi audio e video **non passano dal server**, che inoltra solo
i messaggi di segnalazione (`signal`) e tiene l'elenco di chi è nel canale
(`voice_state`). Con 8 persone in chiamata ognuno invia il proprio video a 7 altri:
il limite pratico è la banda in upload di chi trasmette.

- Microfono e camera funzionano su `localhost` o su HTTPS; da altre macchine in
  HTTP semplice il browser li blocca.
- Fuori dalla rete locale serve un server TURN (per esempio `coturn`) per chi sta
  dietro NAT restrittivi. Si configura con `HSTLE_ICE`:

```powershell
$env:HSTLE_ICE = '[{"urls":["stun:stun.l.google.com:19302"]},{"urls":["turn:turn.esempio.it:3478"],"username":"utente","credential":"password"}]'
```

I canali vocali si gestiscono con `voice_channel_create` (`name`), `voice_channel_rename` (`id`,`name`) e `voice_channel_delete` (`id`), con lo stesso permesso dei canali di testo (*Gestire i canali*); il server manda `voice_channels` con l'elenco. Si entra con `voice_join` (`channel`); si sta in un solo canale vocale alla volta, e eliminando un canale chi è dentro riceve `voice_kicked`. Altri messaggi della voce: `voice_join`, `voice_leave`, `voice_meta`
(muto/camera/schermo) e `signal` dal client; `voice_joined`, `voice_state`,
`voice_error`, `voice_kicked` e `signal` dal server. Se la stessa persona entra
da un altro dispositivo, la prima connessione viene espulsa dal canale.

## API HTTP

| Metodo e percorso    | Descrizione                                       |
|----------------------|---------------------------------------------------|
| `POST /api/register` | `{username, password, invite}` → `{token, user}`  |
| `POST /api/login`    | `{username, password}` → `{token, user}`          |
| `POST /api/logout`   | invalida il token (header `Authorization: Bearer`) |
| `POST /api/invites`  | serve il permesso *Creare inviti* → `{code}` (monouso) |

## Build per il server

```bash
cargo build --release
```

Il binario risultante è in `server/target/release/`.

## Roadmap

1. ~~Utenti e login (SQLite + argon2)~~
2. ~~Più canali di testo, ruoli e permessi~~; gilde, permessi per singolo canale, canali privati
3. Allegati, reazioni, presenza, "sta scrivendo", cronologia a pagine, risposte ai messaggi
4. ~~Voce, webcam e schermo (WebRTC mesh)~~
5. TURN (coturn), HTTPS con Caddy, messa online
6. Limite ai tentativi di login (o delegarlo al reverse proxy)
7. Se serviranno chiamate più grandi: SFU in Rust (`str0m`)
8. Client web vero (Svelte/Solid), PWA, notifiche
