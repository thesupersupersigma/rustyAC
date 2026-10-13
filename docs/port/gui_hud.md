# AC's widget library, its fonts and the HUD (Task 29)

## Resume here

State after the last commit (update after every commit):

- **Done and proven:** the text drawing (`crates/rustyac-render/src/fw1.rs`), all 23 objects
  of ksGUI (`crates/rustyac-gui`), input routing, form dragging, the app bar with its
  desktops, `acos.ini` read and written; of the HUD: the messages (`hud.rs`), the flags, the
  three banners, the wrong-way picture and the fuel light (`hud_flags.rs`).
- **Proof batch:** `sh re/scratch/task29/gui_all.sh` (all GUI sequences through the oracle,
  about 2 minutes; result lines in `re/scratch/task29/gui_all.txt`). One sequence:
  `tools/render_oracle/target/release/render_oracle.exe gui --gui <name> --root re/scratch/task29/root --out re/scratch/task29/out`
  (names: `text base forms forms_layout forms_overlap widgets_a widgets_b widgets_c hud_messages
  hud_flags hud_flags_triple`).
- **Next:** the HUD objects that follow the cars (brief `spec_hud_cars.md`: name tags,
  proximity arrows, damage picture; harness stubs `gui_ac_hud3.rs` / `gui_port_hud3.rs`),
  then the GUI pass and the keys in `rustyac.exe`
  (brief `spec_keys.md`), the browser's text fallback, the report, the version.
- **Briefs** (git-ignored, written by readers from the disassembly; claims, not facts, until
  the oracle agrees): `re/scratch/task29/spec_*.md`. Patch scripts used on the way:
  `re/scratch/task29/p*.py`.
- **Gotchas found so far:** a Bash heredoc with an apostrophe in it fails (write files with
  the Write tool); the oracle's `--root` must be absolute after the game's side changes
  directory (the option makes it so); the game's side needs its handlers hung on events
  before any script call can fire one (`hook_all` runs before every op).
