# AC's menus (Task 30)

## Resume here

State after the last commit (update after every commit):

- **Done and proven:** the gateway's race (own commit); `PauseMenu` (`crates/rustyac-gui/src/menus.rs`)
  against the game's own in the oracle, four sequences.
- **In `rustyac.exe`:** ESC opens the pause menu (`GameGui::esc_key` in
  `crates/rustyac-game/src/render/gui.rs`); its buttons go through `Gui::sim.actions` to the
  window loop (`crates/rustyac-game/src/desktop.rs`: pause, the referee's skip / restart /
  back to pits, the replay, the exit).
- **Proof batch:** `sh re/scratch/task30/gui_all.sh` (every GUI sequence of this task through
  the oracle; result lines in `re/scratch/task30/gui_all.txt`). One sequence:
  `tools/render_oracle/target/release/render_oracle.exe gui --gui <name> --root re/scratch/task30/root --out re/scratch/task30/out`
  (names: `pause_menu pause_menu_online pause_menu_race pause_menu_plain`). Task 29's 14
  sequences: `sh re/scratch/task29/gui_all.sh`.
- **Next, in this order:** the keys (step 1's second half), the pit menu, end of session,
  leaderboards, the ESC pages, the replay GUI, the online UI, the browser's painter.
- **Briefs** (git-ignored, written by readers from the disassembly; claims, not facts, until
  the oracle agrees): `re/scratch/task30/spec_*.md`.
- **Gotchas found:** never run two loops of the gateway's tests at once (they fight over
  ports); the oracle's menu ops are `Hud("menu.…")` and need `Hud("create", cars, "")` first.
