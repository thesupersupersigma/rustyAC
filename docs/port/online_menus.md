# Online menus, chat, voting, fixed setup, pit strategy (Task 34)

## Resume here

State after the last commit (update after every commit):

- **Done and proven in the render oracle:** the ESC menu online (`esc_menu_online`): the page
  "Race control" with the server's rules (`RaceControlSessionServer`), the votes
  (`RaceControlVoting`) and the chat (`ACChat`), against the game's own classes on made-up
  server states, every frame identical (command log, WARP pixels, control state, events).
- **Done, not yet proven live:** the game glue. The ESC menu is made online too; the client's
  chat lines, votes and system messages reach the GUI (`OnlineGuiEvent`), the GUI's chat lines
  and votes reach the client (`OnlineRequest`); `ACClientVotingManager` (keys Y and N, the
  prompt, "not passed"); the console's `vote_next`, `vote_restart`, `vote_kick`.
- **Done and proven in the render oracle:** the Chat app (`FormChat`, `app_chat.rs`):
  `app_chat` (shipped `chat_app.ini`, off line: typed lines echoed, wrapped, scrolled, faded),
  `app_chat_export` (times shown, export with a time stamp in its name), `app_chat_online`
  (drivers', server's and player's lines; `out/chat.txt`), every frame identical and the exports
  byte-identical. In the game it is made after Essentials and writes into rustyAC's `out`.
- **Next, in the task's order:** the server's fixed setup (TCP 0x52), the pit strategy page and `.sp` files, the
  camera at the start and F7, Ctrl+M, the old mouse pit menu; then the live run against a local
  acServer copy and the end checks.
- **Code:** `crates/rustyac-gui/src/race_control_online.rs` (panels, `OnlineLink`,
  `VotingManager`), `esc_menu.rs` (online branch), `crates/rustyac-game/src/online.rs`
  (`OnlineGui`, events, requests, messages), `physics_thread.rs`, `render/gui.rs`,
  `render/console.rs`, `desktop.rs`.
- **Oracle:** `tools/render_oracle/src/gui_seq_menu.rs` (`esc_menu_online`, ops `menu.server*`,
  `menu.vote`, `menu.netCar`, `menu.chat`), `gui_ac_esc.rs` (the stand-in `ACClient`, its voting
  manager and `netCars`, `sendChat` stand-in), `gui_port_menu.rs`. One run:
  `sh re/scratch/task34/cmp.sh esc_menu_online`. The Chat app: `gui_seq_apps.rs` (`chat()`,
  its `chat_app.ini` variants of the shipped file's size, `compare_files` of `out/`),
  `gui_ac_apps.rs` (FormChat in `APPS`, its clock through the import slots of `_time64` /
  `_localtime64`, `~FormChat` at `app.shutdown chat`), `gui_ac_esc::apps_client`.
- **Disk at start:** 35 GB free.
