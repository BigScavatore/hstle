# Hstle

Chat self-hostata leggera in stile Discord, scritta in Rust.
Pensata per pochi utenti e server con poca RAM.

## Stato attuale

- Server Rust (axum + WebSocket) con database SQLite
- Account con password (argon2), sessioni a token, registrazione solo su invito
- Il primo utente registrato diventa admin e può creare inviti
- Un canale (`#generale`) con cronologia salvata
- Client web incorporato nel binario
- Canale vocale con audio, webcam e condivisione schermo (WebRTC mesh, fino a 8 persone)

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

Il server risponde con `ready` (utente) e `history` (ultimi 50 messaggi).
Per scrivere:

```json
{"t":"send","channel":1,"text":"ciao"}
```

e tutti i client ricevono `{"t":"message","message":{...}}`.

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

Messaggi WebSocket della voce: `voice_join`, `voice_leave`, `voice_meta`
(muto/camera/schermo) e `signal` dal client; `voice_joined`, `voice_state`,
`voice_error`, `voice_kicked` e `signal` dal server. Se la stessa persona entra
da un altro dispositivo, la prima connessione viene espulsa dal canale.

## API HTTP

| Metodo e percorso    | Descrizione                                       |
|----------------------|---------------------------------------------------|
| `POST /api/register` | `{username, password, invite}` → `{token, user}`  |
| `POST /api/login`    | `{username, password}` → `{token, user}`          |
| `POST /api/logout`   | invalida il token (header `Authorization: Bearer`) |
| `POST /api/invites`  | solo admin → `{code}` (monouso)                   |

## Build per il server

```bash
cargo build --release
```

Il binario risultante è in `server/target/release/`.

## Roadmap

1. ~~Utenti e login (SQLite + argon2)~~
2. Gilde e più canali, ruoli e permessi
3. Allegati, reazioni, presenza, "sta scrivendo"
4. ~~Voce, webcam e schermo (WebRTC mesh)~~
5. TURN (coturn), HTTPS con Caddy, messa online
6. Limite ai tentativi di login (o delegarlo al reverse proxy)
7. Se serviranno chiamate più grandi: SFU in Rust (`str0m`)
8. Client web vero (Svelte/Solid), PWA, notifiche
