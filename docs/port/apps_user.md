# AC's built-in apps, part 1: the apps players use (Task 32)

## Resume here

State after the last commit (update after every commit):

- **Done and proven in the oracle:** the app framework (`crates/rustyac-gui/src/apps.rs`: an
  app's members hang on its form, `App` trait for its overrides, `AppLink` = what it reads of
  `Sim`), Laps (`app_laps.rs`), Race Position (`app_position.rs`), Essentials
  (`app_essentials.rs`), Performance Delta (`app_delta.rs`).
- **Oracle:** `tools/render_oracle/src/gui_seq_apps.rs` (scripts; the race tapes and the
  stand-in referee both sides read), `gui_ac_apps.rs` (the game's apps on a hand-laid `Sim`),
  `gui_port_apps.rs`. One sequence: `sh re/scratch/task32/cmp.sh app_laps` (`@short` for the
  first 211 frames). Tapes: `oracle/t32/race` (git-ignored; re-record with
  `CAR_ORACLE_PERF_METER=1 tools/car_oracle/target/release/car_oracle.exe multi --scenario
  rc_race --track magione --steps 112000 --threads 0 --audio-tape --out oracle/t32/race --root
  re/scratch/task32/croot`, about 95 s, then delete the `.mcrec`).
- **Next:** Realtime, then Tyres / Gears / Pedals / Electronics, the track map; the apps in
  `rustyac.exe` (not wired yet).
- **Briefs** (git-ignored, written by readers from the disassembly; claims until the oracle
  agrees): `re/scratch/task32/*.md`.
