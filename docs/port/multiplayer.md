# Task 25 (M1): multiplayer on the desktop

## Resume here

State after commit "refactor(net): the session's step hooks live in the protocol crate":

- **Done and committed:** the server kit (`server/`), the protocol crate (`crates/rustyac-net`:
  packet buffer, transport trait, handshake, car list, MD5, car update out, datagrams in, every
  TCP message both ways, `NetCarStateProvider`, the session's step hooks), the engine's three
  missing pieces (`StepHooks`, velocity setters, mesh category), `rustyac.exe --join` and race.ini
  `[REMOTE]`, remote cars drawn and heard, the HUD's player list and chat lines.
- **Proven live so far:** two `rustyac.exe` clients on Kunos' `acServer` 1.15 at Laguna Seca for
  200 s: two laps each, counted by the server, 3,719 states of the other car taken in order
  each way, no disconnect (logs in `re/scratch/task25/interop/`).
- **Next:** the in-process oracle (`tools/car_oracle net`: the game's own `ACClient` +
  `NetCarStateProvider` against the port, fed with the tapes of the live runs), the golden
  test, the report's tables, wasm / clippy / lock files, version 0.25.0.
- Briefs read from the machine code (git-ignored): `re/scratch/task25/spec_1_transport.md` ...
  `spec_9_netcar_step.md`. Local test servers: `re/scratch/task25/server` (Spa) and
  `server_laguna`, copies of the game's `acServer.exe`.

(The full report follows when the proofs are in.)
