# AC's built-in apps, part 1: the apps players use (Task 32)

## Resume here

State after the last commit (update after every commit):

- **Done and proven in the oracle** (command log, WARP pixels, state, files all identical):
  the app framework (`crates/rustyac-gui/src/apps.rs`), Laps, Race Position, Essentials,
  Performance Delta, Realtime, Tyres, Pedals, Electronics, Gears, the track map, Help, Hide
  all, Running Time, Time of the day, Track Description, Audio (`app_volumes.rs`: audio.ini /
  camera_manager.ini byte-identical, the mix and master calls identical), Force Feedback
  (`app_ffb.rs`: controls.ini / car.ini byte-identical, gains and FF mult identical). All of
  them made in `rustyac.exe` (`crates/rustyac-game/src/render/gui_apps.rs`), their files in
  rustyAC's own `cfg`, their sound / force feedback calls carried out.
- **Oracle:** `tools/render_oracle/src/gui_seq_apps.rs` (scripts; the race tapes and the
  stand-in referee both sides read), `gui_ac_apps.rs` (the game's apps on a hand-laid `Sim`),
  `gui_port_apps.rs`. One sequence: `sh re/scratch/task32/cmp.sh app_laps` (`@short` for the
  first 211 frames). Tapes: `oracle/t32/race` (git-ignored; re-record with
  `CAR_ORACLE_PERF_METER=1 tools/car_oracle/target/release/car_oracle.exe multi --scenario
  rc_race --track magione --steps 112000 --threads 0 --audio-tape --out oracle/t32/race --root
  re/scratch/task32/croot`, about 95 s, then delete the `.mcrec`).
- **Next:** Onboard settings (driver eyes), MGU, Ballast if time; then the closing checks
  (older GUI sequences, clippy, tests, tapes, physics dumps vs `re/scratch/task32/tgt_old`),
  pictures, this report, v0.32.0.
- **Briefs** (git-ignored, written by readers from the disassembly; claims until the oracle
  agrees): `re/scratch/task32/*.md`.
