# AC's widget library, its fonts and the HUD (Task 29)

## Resume here

State after the last commit (update after every commit):

- **Done and proven:** the text drawing (`crates/rustyac-render/src/fw1.rs`), all 23 objects
  of ksGUI (`crates/rustyac-gui`), input routing, form dragging, the app bar with its
  desktops, `acos.ini` read and written; of the HUD: the messages (`hud.rs`), the flags, the
  three banners, the wrong-way picture and the fuel light (`hud_flags.rs`), the name tags,
  the proximity arrows and the damage picture (`hud_cars.rs`): every HUD object of the task.
- **Proof batch:** `sh re/scratch/task29/gui_all.sh` (all GUI sequences through the oracle,
  about 2 minutes; result lines in `re/scratch/task29/gui_all.txt`). One sequence:
  `tools/render_oracle/target/release/render_oracle.exe gui --gui <name> --root re/scratch/task29/root --out re/scratch/task29/out`
  (names: `text base forms forms_layout forms_overlap widgets_a widgets_b widgets_c hud_messages
  hud_flags hud_flags_triple hud_cars hud_cars_simple hud_cars_noflag`).
- **In `rustyac.exe`:** `crates/rustyac-game/src/render/gui.rs` (`GameGui`) makes the GUI and
  the HUD objects and feeds them; `AcRenderer::draw` calls it at `GUI::render` and
  `Game::renderHUD`; the window sends mouse, keys and characters; Ctrl+H / U / L / J / Q and
  F10; `--debug-hud`, `--no-gui`, `--cfg-dir`, `--gui-do`. Own files go to `cfg` beside the
  exe (`acos.ini`, `gameplay.ini`).
- **Pictures:** `rustyac.exe --no-race-ini --track spa --autodrive --screenshot x.png --at 3
  --gui-do "message:ABS|Set on level : 2/3"` (more in the report's last section).
- **Next:** the browser's text fallback, the unchanged-proofs run, the report, the version.
- **Briefs** (git-ignored, written by readers from the disassembly; claims, not facts, until
  the oracle agrees): `re/scratch/task29/spec_*.md`. Patch scripts used on the way:
  `re/scratch/task29/p*.py`.
- **Gotchas found so far:** a Bash heredoc with an apostrophe in it fails (write files with
  the Write tool); the oracle's `--root` must be absolute after the game's side changes
  directory (the option makes it so); the game's side needs its handlers hung on events
  before any script call can fire one (`hook_all` runs before every op).
