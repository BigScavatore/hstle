# Hstle

Chat self-hostata leggera in stile Discord, scritta in Rust.
Pensata per pochi utenti e server con poca RAM.

## Stato attuale

Scheletro del server: WebSocket con broadcast a tutti i client connessi
e una pagina di test incorporata nel binario.

## Avvio

```powershell
cd server
cargo run
```

Poi apri http://127.0.0.1:3000 in due schede del browser e scrivi.

Per cambiare indirizzo/porta:

```powershell
$env:HSTLE_ADDR = "0.0.0.0:3000"; cargo run
```

## Build per il server

```bash
cargo build --release
```

Il binario risultante è in `server/target/release/`.

## Roadmap

1. Utenti e login (SQLite + argon2)
2. Gilde, canali, cronologia messaggi
3. Ruoli e permessi
4. Allegati, reazioni, presenza
5. Voce con WebRTC (SFU con `str0m`)
6. Client web vero (Svelte/Solid), PWA, notifiche
