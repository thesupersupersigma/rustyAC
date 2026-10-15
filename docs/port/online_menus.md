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
- **Done and proven in the render oracle:** the server's fixed setup (TCP 0x52):
  `setup_server@bmw_z4_gt3` (1036 frames) and `setup_server1@bmw_z4_gt3` (75) run the game's own
  `ACClient::onSetupReceived` on a made-up packet, every frame identical, `server_temp.ini`
  byte-identical, the named spinners read-only on both sides. The Setup page is made online
  too; the file goes into rustyAC's own `setups` folder.
- **Done and proven in the render oracle:** the setup editor's page "PITSTOP STRATEGY"
  (`SetupPitstopPresets`, `setup_presets.rs`): `setup_pit@bmw_z4_gt3` (302 frames: every preset
  changed, saved with a setup, changed again, loaded back, the server's fixed setup, saved
  again) every frame identical and the `.sp` files byte-identical; `setup_pitprobe@bmw_z4_gt3`
  (11). The `.sp` writer no longer writes the blank line the game does not;
  `QuickMenu::load_fixed_setup` (0x1400b0850). In the game the page edits a copy of the
  referee's (or online session's) pit menu and sends it to the physics thread; the arrow keys'
  changes come back. Only with pitstop.ini `USE_MOUSE_PITSTOP=0` (the shipped value).
- **Done and proven against the game's code:** Ctrl+M mouse steering (`KeyboardCarControl`
  `MOUSE_STEER` / `MOUSE_SPEED` / `MOUSE_ACCELERATOR_BRAKE`, `setMouseSteering` 0x140111350,
  `getMouseSteering` 0x14010fb70): `car_oracle mouse-micro --root re/scratch/task34/mouse_root`
  then `cargo run --release -p rustyac-game --example mouse_steering_check --
  re/scratch/task34/mouse_root/mouse.tape` → IDENTICAL. The camera of a session that starts in
  the ESC menu (`CameraModeStart` 0x140060470, the C library's `rand` from seed 1) and F7's free
  camera (`CameraMouseControlBase::update` 0x14020ff30; only with assetto_corsa.ini
  `[CAMERA] ALLOW_FREE_CAMERA=1`, shipped 0): `car_oracle start-camera-micro --root
  re/scratch/task34/camera_root` then `--example start_camera_check -- <start_camera.tape>
  <free_camera.tape>` → both IDENTICAL (seeds 7 and 34).
- **Next, in the task's order:** the old mouse pit menu (`re/scratch/task34/spec_mousepit.md`);
  then the live run against a local acServer copy and the end checks.
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
