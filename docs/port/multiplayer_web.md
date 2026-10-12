# Task 26 (M2): multiplayer in the browser

## Resume here

Work in progress. Order of the task: (1) transport + non-blocking join, (2) the gateway,
(3) desktop `--join ws://`, (4) the browser client, (5) the web picture of other cars,
(6) proofs, (7) deploy notes, then the version.

- **Done and committed:** steps 1 to 3.
  - `crates/rustyac-net/src/ws.rs`: `WsTransport` (one lane byte per WebSocket message) over
    a `Pipe`; `client.rs`: `Join`, the join as a state machine (`poll` for the browser,
    `wait` for the desktop); `native.rs`: `WsPipe`, the desktop's WebSocket client.
  - `crates/rustyac-gateway`: the gateway (plain std, no dependencies) and the WebSocket
    code both sides use. Tests: `cargo test --release -p rustyac-gateway`.
  - `crates/rustyac-game/src/gateway.rs`: `--join ws://` and `wss://` (rustls), the join's
    checks shared with the browser in `online.rs` (`finish_join`).
  - Seen working: a local `acServer` copy, the gateway and two `rustyac.exe` (one over UDP,
    one over `ws://`): `sh re/scratch/task26/smoke.sh`.
- **Next:** step 4, the browser client (`crates/rustyac-web`, `web/app.js`,
  `web/servers.json`).
- Scratch: `re/scratch/task26/` (git-ignored). Test servers: Task 25's copies in
  `re/scratch/task25/server*`.
