# Pit stops: AC's PitStop, the pit menu, the mandatory pit window (Task 28b)

## Resume here

State while Task 28b is being worked on (this section is rewritten after every commit):

- **Done and proven so far**
  - The two leftovers of Task 28: the unexplained divergence (an AI driver passing a car
    asleep on the grid) is found and fixed; `cargo clippy --all-targets` is clean and is CI's
    command now.
  - The player's pit stop: `crates/rustyac-physics/src/race/pitstop.rs` (`PitStop`,
    `QuickMenu`, the applying job), wired into the referee
    (`crates/rustyac-physics/src/race/referee.rs`) and into rustyac.exe / the browser
    (text menu, keys, `pit_stop=` header key of a recording).
  - The mandatory pit window (`RaceManager::update_pit_window`, `check_mandatory_pit`).
  - The oracle: `tools/car_oracle/src/game/pit.rs` (the game's own `PitStop` beside the
    referee), scenarios `pit_box*`, `pit_ai_race`, `pit_window_*`, `pit_feed`,
    `ai_pass_*`, `rc_pass_*`.
- **Running / next**: the final proof batch (`sh re/scratch/task28b/final.sh a|b|c`), then
  online (the server's window and message 0x0e), the pit crew's picture and sound, the
  report below, the version.
- Scratch files: `re/scratch/task28b/` (the three reading briefs `spec_pitstop.md`,
  `spec_window_online.md`, `spec_crew.md` say where every field of the game's classes is).

(The report follows when the work is finished.)
