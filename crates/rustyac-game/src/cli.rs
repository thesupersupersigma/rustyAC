// SPDX-License-Identifier: GPL-3.0-or-later

//! The command line.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// `--car`: a car of the install by name, or a path to a car folder / data folder.
    pub car: String,
    pub width: u32,
    pub height: u32,
    /// `--windowed`: a window instead of the borderless full screen.
    pub windowed: bool,
    /// `--no-shm`: do not publish the shared-memory pages.
    pub no_shm: bool,
    /// `--record <file>`: write the inputs of every step.
    pub record: Option<PathBuf>,
    /// `--replay <file>`: drive from a recorded input file.
    pub replay: Option<PathBuf>,
    /// `--ffb`: force feedback out to a DirectInput wheel, if one is found.
    pub ffb: bool,
    /// `--no-rumble`: no rumble on the Xbox pad.
    pub no_rumble: bool,
    /// `--headless`: no window. With `--replay` the file is run as fast as possible (in real
    /// time with `--realtime`); without, the car runs in real time with nobody driving.
    pub headless: bool,
    /// `--realtime`: with `--replay --headless`, keep to the clock (and publish shared memory).
    pub realtime: bool,
    /// `--duration <s>`: stop by itself after this many seconds.
    pub duration: Option<f64>,
    /// `--dump-states <file>`: with `--replay --headless`, write the car's state after every step.
    pub dump_states: Option<PathBuf>,
    /// `--screenshot <png>`: draw one frame off screen into a PNG and stop (no window).
    pub screenshot: Option<PathBuf>,
    /// `--at <s>`: with `--screenshot`, how far into the drive the picture is taken.
    pub at: Option<f64>,
    /// `--lead-in <s>`: with `--screenshot`, the seconds before it that are drawn frame by
    /// frame first (smoke, skid marks and flames need the frames before them)
    pub lead_in: f64,
    /// `--sun <angle>`: `SUN_ANGLE` whatever race.ini says (-80 sunrise, 0 one o'clock, 80
    /// sunset)
    pub sun: Option<f32>,
    /// `--weather <folder of content/weather>`: the weather whatever race.ini says;
    /// `--time-mult <n>`: race.ini `TIME_MULT` (how fast the sun and the clouds move)
    pub weather: Option<String>,
    pub time_mult: Option<f32>,
    /// `--render-seed <n>`: what the picture's `rand()` starts from (default: the clock while
    /// driving, 1 for `--screenshot` and `--headless`, the recording's for `--replay`)
    pub render_seed: Option<u32>,
    /// `--mirror-hq <0|1>`: `[MIRROR] HQ` whatever `video.ini` says
    pub mirror_hq: Option<bool>,
    /// `--world-detail <0..5>`: `[ASSETTOCORSA] WORLD_DETAIL` whatever `video.ini` says (clouds
    /// need 1, crowds 3 to 5, skid marks 1)
    pub world_detail: Option<i32>,
    /// `--show-lights`, `--show-damage f,r,l,r,c`: with `--screenshot`, the picture is drawn
    /// with the headlights on / with these damage levels (km/h) whatever the physics says
    pub show_lights: bool,
    pub show_damage: Option<[f32; 5]>,
    /// `--flat-out`: with `--screenshot`, nobody steers and the throttle is held down
    pub flat_out: bool,
    /// `--flat-out-brake <0..1>`: with `--flat-out`, the brake pedal is held this far as well
    /// (the front wheels hold the car back while the rear ones spin: a burnout nearly on the spot)
    pub flat_out_brake: f32,
    /// `--mirror-size <n>`: `[MIRROR] SIZE` whatever `video.ini` says (0: no mirrors)
    pub mirror_size: Option<i32>,
    /// `--virtual-mirror`: the F11 mirror on from the start
    pub virtual_mirror: bool,
    /// `--debug-view`: the debug view (own shader, flat colours) instead of AC's renderer. It
    /// stays: a fast picture for slow machines, and a physics debugging aid.
    pub debug_view: bool,
    /// `--warp`: AC's renderer on WARP, Microsoft's software rasteriser (slow; the same pixels
    /// on every machine).
    pub warp: bool,
    /// `--gpu-log <file>`: write the Direct3D command log of the second frame.
    pub gpu_log: Option<PathBuf>,
    /// `--skin <folder>`: the car's skin (default: race.ini's, else the first one).
    pub skin: Option<String>,
    /// `--video-ini-exact`: follow `video.ini` also where plain acs.exe cannot make a proper
    /// picture with it (a shadow map or cube map size of 0 or less).
    pub video_ini_exact: bool,
    /// `--video-ini <file>`: the video settings from this file instead of
    /// `Documents/Assetto Corsa/cfg/video.ini`; `--graphics max|low`: built-in settings (every
    /// one at the highest / lowest value AC's launcher offers), no file is read
    pub video_ini: Option<PathBuf>,
    pub graphics: Option<String>,
    /// `--no-a2c`: NOT Assetto Corsa, an experiment: no alpha to coverage (see kgl.rs)
    pub no_a2c: bool,
    /// `--show-fps`: the frame-rate counter (Ctrl+F) on from the start
    pub show_fps: bool,
    /// `--cube-faces <n>`: how many faces of the reflection cube map are drawn again every
    /// frame (`[CUBEMAP] FACES_PER_FRAME` of `video.ini`), 0 to 6.
    pub cube_faces: Option<i32>,
    /// `--cube-size <n>`: `[CUBEMAP] SIZE` whatever the video settings say
    pub cube_size: Option<i32>,
    /// `--aa <1|2|4|8>`: `[VIDEO] AASAMPLES` whatever the video settings say
    pub aa_samples: Option<i32>,
    /// `--post <0|1>`, `--fxaa <0|1>`, `--motion-blur <0..12>`, `--hdr-levels`
    pub post: Option<bool>,
    pub fxaa: Option<bool>,
    pub motion_blur: Option<f32>,
    pub hdr_levels: bool,
    /// `--supersample <1..4>`: with `--screenshot`, the picture is drawn this many times wider
    /// and higher and averaged down to `--width` x `--height` (1: as it is drawn)
    pub supersample: u32,
    /// `--free-camera <right,up,ahead>[/<right,up,ahead>[/<fov>]]`: with `--screenshot`, a camera
    /// that stands still beside the car of the picture and looks at a point of it
    pub free_camera: Option<FreeCamera>,
    /// `--no-hud`: with `--screenshot`, the picture without rustyAC's own panels and numbers and
    /// without the virtual mirror
    pub no_hud: bool,
    /// `--debug-hud`: rustyAC's own panels and numbers are drawn too (in the window: F10)
    pub debug_hud: bool,
    /// `--no-gui`: AC's GUI and HUD are not made
    pub no_gui: bool,
    /// `--dev-apps`: AC's developers' apps get their icons in the app bar whatever
    /// `system/cfg/assetto_corsa.ini [AC_APPS] ENABLE_DEV_APPS` says
    pub dev_apps: bool,
    /// `--language <code>`: the language of AC's GUI (`system/locales/<code>.ini`; default: the
    /// game's `cfg/launcher.ini [WINDOW] LANGUAGE`, else English)
    pub language: Option<String>,
    /// `--cfg-dir <folder>`: where rustyAC keeps its own `acos.ini` and `gameplay.ini`
    /// (default: `cfg` beside rustyac.exe; never Assetto Corsa's own folders)
    pub cfg_dir: Option<PathBuf>,
    /// `--esc-start` / `--no-esc-start`: the session begins in the ESC menu with the car held
    /// until "Drive", as in the game (default: on for a drive at the wheel in a window; off
    /// for replays, unattended drives, pictures and online)
    pub esc_start: Option<bool>,
    /// `--setups-dir <folder>`: where the setup editor saves (default: `setups` beside
    /// rustyac.exe; never Assetto Corsa's own folders)
    pub setups_dir: Option<PathBuf>,
    /// `--no-ac-setups`: the setups of `Documents\Assetto Corsa\setups` are not listed
    pub no_ac_setups: bool,
    /// set by the program, not by an option: the drive is on a server (joined with `--join`
    /// or through race.ini's `[REMOTE]`)
    pub online_session: bool,
    /// `--gui-do <commands>`: mouse moves, keys and a few events for AC's GUI, done before the
    /// first frame (`render::gui::GameGui::script`)
    pub gui_do: Option<String>,
    /// `--pose-out <file>`: with `--screenshot`, also write the car's state the picture is made
    /// from (for the render oracle: the game's own renderer is given the same state).
    pub pose_out: Option<PathBuf>,
    /// `--camera chase|cockpit`: the camera at start.
    pub camera: String,
    /// `--no-race-cars`: the other cars of race.ini (`[CAR_1]` …) are left out
    pub no_race_cars: bool,
    /// `--focus <n>`: the car the camera is on at the start (0: yours; `Sim::focusedCarIndex`).
    pub focus: usize,
    /// `--focus-script next@2,previous@4,player@6`: the focus keys by the clock (for unattended
    /// checks; seconds since the window opened)
    pub focus_script: Vec<(f64, String)>,
    /// `--auto-shifter`: the automatic gearbox aid on at start.
    pub auto_shifter: bool,
    /// `--no-auto-clutch`: the automatic clutch aid off.
    pub no_auto_clutch: bool,
    /// `--auto-clutch`, `--no-auto-shifter`, `--auto-blip`, `--no-auto-blip`: the other ways
    /// round, over assists.ini.
    pub auto_clutch: bool,
    pub no_auto_shifter: bool,
    pub auto_blip: Option<bool>,
    /// `--ideal-line` / `--no-ideal-line`: the ideal line whatever assists.ini says
    pub ideal_line: Option<bool>,
    /// `--start-lights <seconds>`: the track's start lights count down a race start
    pub start_lights: Option<f32>,
    /// Task 28c: `--no-ac-replay`: AC's replay is not recorded (the program as before v0.28.2)
    pub no_ac_replay: bool,
    /// `--replay-dir <folder>`: where replays are saved (default: `replay` beside rustyac.exe;
    /// never one of Assetto Corsa's folders)
    pub replay_dir: Option<PathBuf>,
    /// `--ghost <off|record|play|both>`: AC's ghost car whatever race.ini's `[GHOST_CAR]` says:
    /// `record` keeps the best lap, `play` loads the last ghost and shows it, `both` does both
    pub ghost: Option<String>,
    /// `--ghost-file <file>`: that `.ghost` file is loaded (race.ini's `FILE`)
    pub ghost_file: Option<PathBuf>,
    /// `--ghost-advantage <seconds>`: the ghost runs this far ahead (`SECONDS_ADVANTAGE`)
    pub ghost_advantage: Option<f32>,
    /// `--ghost-dir <folder>`: where ghost cars are kept (default: `GhostCar` beside
    /// rustyac.exe; never one of Assetto Corsa's folders)
    pub ghost_dir: Option<PathBuf>,
    /// `--telemetry-dir <folder>`: where the comparative telemetry's best laps are kept (default:
    /// `ctelemetry` beside rustyac.exe; never one of Assetto Corsa's folders). Headless runs
    /// and replayed drives record telemetry only when this is given.
    pub telemetry_dir: Option<PathBuf>,
    /// `--no-telemetry`: no comparative telemetry
    pub no_telemetry: bool,
    /// `--save-replay <file>`: the session's replay is written to this file when the run ends
    pub save_replay: Option<PathBuf>,
    /// `--replay` named one of AC's replays (not a `.ryin` recording): the file
    pub ac_replay: Option<PathBuf>,
    /// `--replay-script <key@seconds,…>`: the replay's keys pressed by the clock (for checks)
    pub replay_script: Vec<(f64, String)>,
    /// `--new-session-at <seconds>`: N pressed by the clock (for checks)
    pub new_session_at: Option<f64>,
    /// `--spawn` was given (else race.ini's `[SESSION_0] SPAWN_SET` decides).
    pub spawn_given: bool,
    /// `--controls <ini>`: read the bindings from this file instead of AC's / rustyAC's.
    pub controls: Option<PathBuf>,
    /// `--default-controls`: ignore AC's controls.ini, use the built-in layout.
    pub default_controls: bool,
    /// `--bench-render`: with `--headless`, also draw frames off screen and report their rate.
    pub bench_render: bool,
    /// `--bench-shot <png>`: with `--headless --bench-render`, the last frame drawn, as a PNG.
    pub bench_shot: Option<PathBuf>,
    /// `--no-focus`: show the window without activating it and keep running unfocused
    /// (for automatic checks, so the window never takes the keyboard away).
    pub no_focus: bool,
    /// `--vsync 0|1`
    pub vsync: bool,
    /// `--list-devices`: print the detected devices and bindings and stop.
    pub list_devices: bool,
    /// `--track <folder>`: a track folder, or a name under Assetto Corsa's `content/tracks`.
    /// Without it (or with `--flat`) the car drives on the endless flat road.
    pub track: Option<String>,
    /// `--layout <name>`: the track's layout (`CONFIG_TRACK`); `-` for the track without one.
    pub layout: Option<String>,
    /// `--list-tracks`: print every installed track and layout and stop.
    pub list_tracks: bool,
    /// `--spawn hotlap|pit|start`: where on the track the car starts.
    pub spawn: String,
    /// `--boxes`: draw the car as boxes even when its 3D model is found.
    pub boxes: bool,
    /// `--texture-size <px>`: the longest side of a texture that is put on the card (0: as stored).
    pub texture_size: u32,
    /// `--no-textures`: flat colours instead of textures.
    pub no_textures: bool,
    /// `--no-audio`, `--volume`, `--audio-wav`, `--audio-log` and the sound's test switches.
    pub audio: crate::audio::AudioOptions,
    /// `--race-ini` (true) / `--no-race-ini` (false): read the game's last session file or not;
    /// not given: read it when it exists.
    pub race_ini: Option<bool>,
    /// `--race-ini <file>`: that file instead of `Documents\Assetto Corsa\cfg\race.ini`.
    pub race_ini_file: Option<PathBuf>,
    /// `--air`, `--road`: temperatures, deg C.
    pub air: Option<f32>,
    pub road: Option<f32>,
    /// `--grip`: the track's grip, fixed (percent, or 0..1).
    pub grip: Option<f32>,
    /// `--wind`: km/h, exactly (0: none); `--wind-dir`: degrees.
    pub wind: Option<f32>,
    pub wind_dir: Option<f32>,
    /// `--wind-from-log`: the wind of Assetto Corsa's own last session, from its log.
    pub wind_from_log: bool,
    /// `--setup`: a saved setup, by name or file.
    pub setup: Option<String>,
    /// `--air-density`: kg/m^3 instead of the game's formula (an experiment, not AC).
    pub air_density: Option<f32>,
    /// `--damage <percent>`: assists.ini `DAMAGE` (the game's mechanical damage rate) whatever
    /// the file says.
    pub damage: Option<i32>,
    /// `--pressure-law <psi at 26 C>,<psi per C>`: another tyre pressure law (an experiment,
    /// not AC).
    pub pressure_law: Option<(f32, f32)>,
    /// `--autodrive`: a driver that follows the track's AI line (needs `--track`).
    pub autodrive: bool,
    /// Task 25. `--join <host:port>`: the server to join (its TCP port). Task 26: or
    /// `ws://...` / `wss://...`, a gateway next to the server.
    pub join: Option<String>,
    /// `--join-header <Name: value>` (repeatable): more header fields of the WebSocket
    /// request to a gateway (a tunnel's service token).
    pub ws_headers: Vec<(String, String)>,
    /// `--tls-root <file.pem>`: one more root certificate for `wss://` (a gateway behind a
    /// certificate of your own).
    pub tls_root: Option<PathBuf>,
    /// `--net-arrivals <file.csv>`: when every state of another car arrived (for
    /// measuring a transport's delay).
    pub net_arrivals: Option<PathBuf>,
    /// `--offline`: no server, whatever race.ini's `[REMOTE]` says.
    pub offline: bool,
    /// `--name`, `--team`, `--password`: who joins.
    pub driver_name: Option<String>,
    pub team: Option<String>,
    pub password: Option<String>,
    /// `--guid`: the player id to send (else Steam's, else rustyAC's own).
    pub guid: Option<String>,
    /// `--net-identity <file>`: rustyAC's own player id from this file (made there if it
    /// has none), Steam not asked.
    pub net_identity: Option<PathBuf>,
    /// `--no-steam`: Steam is not asked for the player id.
    pub no_steam: bool,
    /// `--net-tape <file>`: every message of the session, written there at its end.
    pub net_tape: Option<PathBuf>,
    /// `--chat <text>@<seconds>` (repeatable): lines sent to the chat, for checks.
    pub chat: Vec<(String, f64)>,
    /// `--car` was given.
    pub car_given: bool,
    /// Task 24. `--cars <n>`: n cars in all; the others are the driver's car again, driven
    /// by the line follower on a track and parked on the flat road.
    pub cars: Option<usize>,
    /// `--opponent <car>[:<skin>]:<ai | parked | auto | file.ryin>`, once per other car.
    pub opponents: Vec<String>,
    /// Task 27. `--ai-cars <n>`: n cars in all; the others are the driver's car again, each
    /// with AC's AI driver.
    pub ai_cars: Option<usize>,
    /// `--ai-level <0..100>`, `--ai-aggression <0..100>`: for every AI driver (without them:
    /// race.ini's, else 100 and 0).
    pub ai_level: Option<f32>,
    pub ai_aggression: Option<f32>,
    /// `--ai-drive`: an AI driver drives your car from the start (as `INPUT_METHOD=AI`);
    /// `--ai-at <s>`: it takes over after s seconds.
    pub ai_drive: bool,
    pub ai_at: Option<f64>,
    /// `--ai-laps <n>`: the race's laps as the drivers are told (their fuel); `--race-start-in
    /// <s>`: the green light s seconds after the start of a race; `--ai-seed <n>`.
    pub ai_laps: Option<i32>,
    pub race_start_in: Option<f64>,
    /// Task 28, `--sessions <list>`: the referee's session list without a race.ini, e.g.
    /// `p10,q5,r3` (practice 10 min, qualifying 5 min, a race of 3 laps), `rt20` (a race of
    /// 20 minutes), `rt20+` (and the lap after)
    pub sessions: Option<String>,
    /// `--no-referee`: no race rules, as before v0.28
    pub no_referee: bool,
    /// `--penalties <0|1>`, `--jump-start <0|1|2>`: race.ini's `PENALTIES` and `JUMP_START_PENALTY`
    pub penalties: Option<bool>,
    pub jump_start: Option<i32>,
    /// `--no-pit-stop`: no pit stop for the player, as before v0.28.1
    pub no_pit_stop: bool,
    /// `--pit-window <open>,<close>`: a mandatory pit window (the leader's laps; minutes in a
    /// timed race)
    pub pit_window: Option<(u16, u16)>,
    /// `--pit-key <key>@<s>` (repeatable; left, up, right, down; Task 31: drive, the ESC menu's "Drive"): the pit menu's keys, pressed
    /// at a time of the run (unattended checks)
    pub pit_keys: Vec<(f64, u32)>,
    /// `--ai-pit-at <s>` (repeatable): the AI driver of your car (`--ai-drive`) is called in
    /// for a pit stop at a time of the run
    pub ai_pit_at: Vec<f64>,
    /// `--next-session-at <s>`, `--restart-session-at <s>` (repeatable): the referee's two
    /// buttons, pressed at a time of the run (unattended checks)
    pub next_session_at: Vec<f64>,
    pub restart_session_at: Vec<f64>,
    /// `--out-dir <folder>`: where `race_out.json` and `laps.ini` go (default: `out` beside
    /// rustyac.exe; never Assetto Corsa's own folders)
    pub out_dir: Option<PathBuf>,
    pub ai_seed: Option<u32>,
    /// `--stand-in-drivers`: race.ini's other cars with the line follower instead of the AI.
    pub stand_in_drivers: bool,
    /// `--physics-threads <n>`: worker threads of the physics engine with several cars
    /// (0: none); without it, as the game decides (`[PHYSICS_THREADING] THREADS`).
    pub physics_threads: Option<usize>,
    /// `--state-hash`: with `--replay --headless`, print a hash over every car's rigid-body
    /// state of every step, and the time a step took.
    pub state_hash: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            race_ini: None,
            race_ini_file: None,
            air: None,
            road: None,
            grip: None,
            wind: None,
            wind_dir: None,
            wind_from_log: false,
            setup: None,
            air_density: None,
            damage: None,
            pressure_law: None,
            car: "ks_ferrari_f2004".to_string(),
            width: 1280,
            height: 720,
            windowed: false,
            no_shm: false,
            record: None,
            replay: None,
            ffb: false,
            no_rumble: false,
            headless: false,
            realtime: false,
            duration: None,
            dump_states: None,
            screenshot: None,
            pose_out: None,
            supersample: 1,
            no_hud: false,
            debug_hud: false,
            no_gui: false,
            dev_apps: false,
            language: None,
            cfg_dir: None,
            esc_start: None,
            setups_dir: None,
            no_ac_setups: false,
            online_session: false,
            gui_do: None,
            free_camera: None,
            debug_view: false,
            warp: false,
            gpu_log: None,
            skin: None,
            video_ini_exact: false,
            cube_faces: None,
            cube_size: None,
            aa_samples: None,
            post: None,
            fxaa: None,
            motion_blur: None,
            hdr_levels: false,
            video_ini: None,
            graphics: None,
            no_a2c: false,
            show_fps: false,
            at: None,
            lead_in: 0.0,
            mirror_size: None,
            flat_out: false,
            flat_out_brake: 0.0,
            sun: None,
            weather: None,
            time_mult: None,
            render_seed: None,
            mirror_hq: None,
            world_detail: None,
            show_lights: false,
            show_damage: None,
            virtual_mirror: false,
            camera: "chase".to_string(),
            no_race_cars: false,
            focus: 0,
            focus_script: Vec::new(),
            auto_shifter: false,
            no_auto_clutch: false,
            auto_clutch: false,
            no_auto_shifter: false,
            auto_blip: None,
            ideal_line: None,
            start_lights: None,
            replay_script: Vec::new(),
            no_ac_replay: false,
            replay_dir: None,
            save_replay: None,
            telemetry_dir: None,
            no_telemetry: false,
            ghost: None,
            ghost_file: None,
            ghost_advantage: None,
            ghost_dir: None,
            ac_replay: None,
            new_session_at: None,
            spawn_given: false,
            controls: None,
            default_controls: false,
            bench_render: false,
            bench_shot: None,
            no_focus: false,
            vsync: true,
            list_devices: false,
            track: None,
            layout: None,
            list_tracks: false,
            spawn: "hotlap".to_string(),
            boxes: false,
            texture_size: 1024,
            no_textures: false,
            audio: crate::audio::AudioOptions::default(),
            autodrive: false,
            join: None,
            ws_headers: Vec::new(),
            tls_root: None,
            net_arrivals: None,
            offline: false,
            driver_name: None,
            team: None,
            password: None,
            guid: None,
            net_identity: None,
            no_steam: false,
            net_tape: None,
            chat: Vec::new(),
            car_given: false,
            cars: None,
            opponents: Vec::new(),
            ai_cars: None,
            ai_level: None,
            ai_aggression: None,
            ai_drive: false,
            ai_at: None,
            ai_laps: None,
            race_start_in: None,
            sessions: None,
            no_referee: false,
            no_pit_stop: false,
            pit_window: None,
            pit_keys: Vec::new(),
            ai_pit_at: Vec::new(),
            penalties: None,
            jump_start: None,
            next_session_at: Vec::new(),
            restart_session_at: Vec::new(),
            out_dir: None,
            ai_seed: None,
            stand_in_drivers: false,
            physics_threads: None,
            state_hash: false,
        }
    }
}

/// `--free-camera`: where the camera stands and what it looks at, in metres to the right of,
/// above and ahead of the car's body, on axes that stay level (the car's heading on the
/// ground and the world's up); the vertical field of view in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FreeCamera {
    pub eye: [f32; 3],
    pub look: [f32; 3],
    pub fov: f32,
}

impl FreeCamera {
    pub fn parse(text: &str) -> Result<FreeCamera, String> {
        let usage = || format!("--free-camera {text}: <right,up,ahead>[/<right,up,ahead>[/<fov>]] is expected, in metres from the car, for example -4,0.6,-5/0,0.4,0/45");
        let triple = |part: &str| -> Result<[f32; 3], String> {
            let numbers: Vec<f32> = part.split(',').map(|n| n.trim().parse::<f32>()).collect::<Result<_, _>>().map_err(|_| usage())?;
            <[f32; 3]>::try_from(numbers).map_err(|_| usage())
        };
        let mut parts = text.split('/');
        let eye = triple(parts.next().unwrap_or(""))?;
        let look = match parts.next() {
            Some(part) => triple(part)?,
            None => [0.0, 0.4, 0.0],
        };
        let fov = match parts.next() {
            Some(part) => part.trim().parse::<f32>().ok().filter(|fov| *fov > 1.0 && *fov < 170.0).ok_or_else(usage)?,
            None => 60.0,
        };
        if parts.next().is_some() || eye == look || eye.iter().chain(&look).any(|n| !n.is_finite()) {
            return Err(usage());
        }
        Ok(FreeCamera { eye, look, fov })
    }
}

pub const USAGE: &str = "\
rustyac: drive the Rust port of Assetto Corsa's car on a track or on an endless flat road

usage: rustyac [options]

  --car <name|folder>   a car of your Assetto Corsa install by its folder name (default
                        ks_ferrari_f2004; read out of its data.acd in memory), or a path to
                        a car folder or to a folder with the plain data files
  --track <name>        a track: its folder name under Assetto Corsa's content/tracks
                        (--track ks_laguna_seca), the name the game's menu shows or a part
                        of either that fits one track (--track laguna), or a folder;
                        default: the endless flat road
  --layout <name>       the track's layout (--track ks_nurburgring --layout layout_gp_a).
                        Without it: race.ini's CONFIG_TRACK when its TRACK is this track,
                        else the track's first layout
  --list-tracks         print every installed track and layout (and why some are refused)
  --flat                the endless flat road (the default)
  --spawn <where>       on a track: hotlap, pit or start (default: where race.ini's session
                        starts, [SESSION_0] SPAWN_SET; without a race.ini hotlap). In a
                        hot-lap session (a [SESSION_n] with TYPE=4, or no race.ini) the clock
                        starts again when the car first crosses the line; in any other it
                        runs from the spawn point and lap 1 includes the run-up, as in the game
  --boxes               draw the car as boxes, not with its 3D model
  --texture-size <px>   longest texture side put on the graphics card (default 1024, 0 = as stored)
  --no-textures         flat colours instead of textures
  --no-audio            no sound (the sound is Assetto Corsa's own: its FMOD DLLs and sound banks are
                        loaded from its folder; without them rustyAC is silent)
  --volume <0..1>       master volume instead of [LEVELS] MASTER of Documents\\Assetto Corsa\\cfg\\audio.ini
  --audio-wav <file>    write the mix to a WAV file instead of playing it (FMOD's non-real-time writer:
                        one 1/60 s block per 1/60 s of driving, whatever the PC's speed; also with --headless)
  --audio-log <file>    write every FMOD call to a text file
  --audio-null          FMOD's no-sound output (everything runs, nothing is heard; for timing runs)
  --race-ini [file]     the session's conditions from Assetto Corsa's last session file
                        (Documents\\Assetto Corsa\\cfg\\race.ini, with assists.ini beside it):
                        air and road temperature, track grip, wind, ballast, aids. This is
                        the default when the file exists.
  --no-race-ini         the built-in conditions: 26 C air, 30 C road, grip 100 %, no wind
  --air <C>             air temperature (over race.ini's)
  --road <C>            road temperature
  --grip <percent>      the track's grip, fixed (for example 97)
  --wind <km/h>         the wind, exactly this (not drawn like the game's); 0: none
  --wind-dir <deg>      the direction the wind is handed to the game with
  --wind-from-log       the wind Assetto Corsa itself drew in its last session (the line
                        Setting wind ... of Documents\\Assetto Corsa\\logs\\log.txt): for a
                        lap in the same wind as the lap just driven in the game
  --setup <name|file>   a saved setup: a file, or a name in Documents\\Assetto Corsa\\setups\\
                        <car>\\<track> (then ...\\generic), loaded as the game's setup screen does
  --damage <percent>    mechanical damage whatever assists.ini says (its DAMAGE: 0 off, 100 full).
                        At 0 a crash bends no suspension, wears no engine and no tyre can burst;
                        Content Manager and the launcher write the file anew for every session
  --air-density <kg/m3> NOT Assetto Corsa: a fixed air density instead of the game's
                        1.2922 - 0.0041 x air temperature, to try what Custom Shaders Patch's
                        thinner air at altitude does (about 1.165 at Spa at 14 C)
  --pressure-law <a>,<b> NOT Assetto Corsa: tyre pressure = a + b x (core temperature - 26 C)
                        instead of the game's static pressure + 0.16 x (core - 26), to try
                        what Custom Shaders Patch's pressures do (the F2004 at 14 C: 10.97,0.084)
  --autodrive           on a track: nobody at the controls, a simple driver follows the
                        track's AI line (automatic gearbox on); for checks and for watching
  --join <host:port>    multiplayer: join a Kunos acServer (its TCP port), as Assetto Corsa
                        does. The server names the track and gives you a car: --car asks for
                        a model, --name is your driver name (default: your Windows user
                        name), --password the server's, --team your team. Without --join, a
                        start without --car and --track follows race.ini's [REMOTE] section
                        when it has ACTIVE=1 (what Content Manager writes when you join a server)
  --join <ws://...>     the same through a WebSocket gateway next to the server
         <wss://...>    (crates/rustyac-gateway): for a server that is only reachable over
                        HTTPS (a Cloudflare Tunnel carries no UDP), e.g.
                        --join wss://rustyac.example.com/net/spa . wss:// is TLS (rustls).
  --join-header <Name: value>
                        one more header field of the WebSocket request (repeatable). A
                        Cloudflare Access service token is also taken from the environment:
                        CF_ACCESS_CLIENT_ID and CF_ACCESS_CLIENT_SECRET, sent only over
                        wss:// and only to the host named in CF_ACCESS_HOST
  --tls-root <file.pem> trust this certificate too for wss:// (a gateway of your own)
  --net-arrivals <csv>  write when every state of another car arrived (to measure delay)
  --offline             never join a server, whatever race.ini says
  --no-steam            do not ask Steam for your player id (rustyAC's own id is used: a
                        random number made once, kept in rustyac_net.ini next to the program)
  --net-identity <file> take rustyAC's own player id from this file (a second player on one PC)
  --net-tape <file>     write every message of the online session to this file at its end
                        (your player id and the server's password are blanked in it;
                        names and chat lines of everybody are in it)
  --chat <text>@<s>     send a chat line after s seconds (repeat it for more)
  --cars <n>            n cars in all: the others are copies of the driver's car on the next
                        grid (or pit) slots, driven by the line follower (parked on the flat
                        road). Their physics is the game's; AC's renderer and the sound do
                        not have them yet: --debug-view shows them as blue boxes
  --opponent <car>[:<skin>]:<ai|parked|auto|file.ryin>
                        another car (repeat it for more): driven by Assetto Corsa's own AI
                        driver, parked on its brakes, driven by the line follower, or by the
                        inputs of a recorded drive
  --ai-cars <n>         n cars in all: the others are copies of the driver's car, each with
                        Assetto Corsa's AI driver. With AI cars and no session of race.ini
                        the session is a race from the grid (practice with --spawn pit)
  --ai-level <0..100>   the AI drivers' level (AI_LEVEL; default: race.ini's, else 100)
  --ai-aggression <0..100>
                        the AI drivers' aggression (AI_AGGRESSION; default: race.ini's, else 0)
  --ai-drive            an AI driver drives your car from the start (as controls.ini
                        INPUT_METHOD=AI); while driving, Ctrl+C hands your car to one
                        (AC's command Activate AI; there is no way back but a new session)
  --ai-at <s>           hand your car to an AI driver after s seconds (for unattended runs)
  --ai-laps <n>         the race's laps as the AI drivers are told them (they fuel for
                        them); default: race.ini's, else 5
  --race-start-in <s>   the green light of a race comes s seconds after the session's start
                        (the AI drivers rev and wait for it; your gearbox is free)
  --ai-seed <n>         what the AI drivers' random numbers start from (a live session
                        takes the clock; pictures and unattended runs 0)
  --stand-in-drivers    race.ini's other cars with rustyAC's line follower, not the AI
  --physics-threads <n> worker threads that step the cars (0: none; with 1 the game's split
                        leaves the worker nothing). Default: as the game, [PHYSICS_THREADING]
                        THREADS of its assetto_corsa.ini (half the logical processors; none
                        below four). The result never depends on it
  --state-hash          with --replay --headless: print a hash over every car's state of
                        every step, and how long a step took
  --width <px>          size of the picture (default 1280 x 720; with --windowed the window's
  --height <px>         client area, otherwise the off-screen picture of --screenshot)
  --windowed            a window instead of borderless full screen
  --no-shm              do not publish acpmf_physics / acpmf_graphics / acpmf_static
  --record <file>       log the inputs of every physics step
  --replay <file>       drive from a recorded input file. In the window AC's replay keys work
                        (those of controls.ini; Ctrl is the command key): Ctrl+Space (or P)
                        play / pause, Ctrl+S slow motion on / off (half speed), Ctrl+F faster
                        (2, 3, 4, 5 times; Ctrl+Space back to normal), Home stop: back to the
                        start, stopped (rustyAC's own key: the game's Stop is a button).
                        Not there: Ctrl+D rewind, Ctrl+N / Ctrl+P lap jumps, Ctrl+R instant
                        replay (they need recorded frames to go back over)
  --new-session-at <s>  in a window: N (a new session) pressed by the clock, for unattended checks
  --replay <file>       also plays one of AC's own replays (.acreplay, format 15 or 16): the
                        file names its track and cars. Ctrl+R enters and leaves the replay of
                        a running session; in a replay: P play / pause, Ctrl+F faster,
                        Ctrl+D back, Ctrl+S slow motion, the full stop a frame on, Page Up /
                        Page Down the next / previous lap, Home to the start, F10 saves
  --replay-dir <dir>    where replays are saved (default: replay beside rustyac.exe; a new
                        session autosaves the last one into its temp folder)
  --save-replay <file>  write the session's replay to this file when the run ends
  --no-ac-replay        record no replay
  --ghost <mode>        AC's ghost car (the see-through car that drives your best lap
                        again): record keeps the best valid lap of the run and writes it
                        when the program ends, play loads the last one and shows it, both
                        does both, off neither (default: race.ini's [GHOST_CAR]; never
                        on a server)
  --ghost-file <file>   load this .ghost file (one of the game's too) instead
  --ghost-advantage <s> the ghost runs this many seconds ahead
  --ghost-dir <dir>     where ghost cars are kept (default: GhostCar beside rustyac.exe)
  --telemetry-dir <dir> where AC's comparative telemetry keeps your best lap's gear, speed
                        and pedals (a .tc file, written when the program ends; default:
                        ctelemetry beside rustyac.exe; a headless run records it only when
                        this is given; never on a server)
  --no-telemetry        record no comparative telemetry
  --replay-script <l>   with --replay in a window: press the replay's keys by the clock, for
                        unattended checks: a list of key@seconds with the keys pause, slow,
                        ffwd, stop (e.g. slow@2,pause@4,ffwd@5,stop@8,pause@9)
  --ffb                 force feedback to a DirectInput wheel, if one is found (low strength)
  --no-rumble           no rumble on the Xbox pad
  --auto-shifter        the automatic gearbox aid on (Alt+G or Ctrl+G toggles it); default:
  --no-auto-shifter     as assists.ini says (AUTO_SHIFTER), off without the file
  --auto-clutch         the automatic clutch aid on / off; default: on with a pad or the
  --no-auto-clutch      keyboard (the game forces it there and refuses to switch it off), else
                        as assists.ini says
  --auto-blip           the automatic throttle blip on / off; default: as assists.ini says.
  --no-auto-blip        Off: a car with an electronic blip blips anyway, and a car with an
                        H-pattern gearbox also stops cutting the ignition on up-shifts
  --controls <ini>      bindings file to use (default: AC's own controls.ini, read-only)
  --default-controls    ignore AC's controls.ini and use the built-in Xbox / keyboard layout
  --camera <name>       the view to start in: chase (default), chase2, bonnet, bumper, dash,
                        cockpit, or car0, car1 ... (the cameras of the car's cameras.ini);
                        F1 goes through the first six while driving, F6 through the car's
  --sun <angle>         the sun's angle whatever race.ini says: -80 sunrise, 0 one o'clock,
                        80 sunset, beyond that night
  --lead-in <s>         with --screenshot: draw the last seconds frame by frame first, so
                        that smoke, skid marks and flames are in the picture
  --flat-out            with --screenshot: nobody steers, the throttle is held down from the
                        start (a launch with wheelspin)
  --flat-out-brake <b>  with --flat-out: the brake pedal held too (0 to 1). Around 0.6 the F2004's
                        front wheels hold it back while the rear ones spin: a burnout that hardly
                        leaves the spot (more stalls the engine, less lets the car go)
  --show-lights         with --screenshot: the headlights on in the picture
  --show-damage f,r,l,r,c   with --screenshot: the picture with these damage levels (km/h
                        of impact: front, rear, left, right, centre) whatever the physics says
  --mirror-size <n>     the mirror texture's width whatever video.ini says (0: no mirrors)
  --mirror-hq <0|1>     the high-quality mirror ([MIRROR] HQ) whatever video.ini says
  --world-detail <0..5> WORLD_DETAIL whatever video.ini says (clouds and skid marks need 1,
                        crowds 3 to 5)
  --weather <name>      a folder of content/weather whatever race.ini says (7_heavy_clouds ...)
  --time-mult <n>       race.ini TIME_MULT: how fast the sun and the clouds move
  --render-seed <n>     what the picture's rand() starts from (default: the clock while driving,
                        1 for --screenshot and --headless, the recording's for --replay)
  --virtual-mirror      the virtual mirror on from the start (F11 switches it while driving;
                        it needs mirrors in video.ini: [MIRROR] SIZE above 0)
  --vsync <0|1>         wait for the display (default 1)
  --list-devices        print the detected devices and the active bindings, then stop

for checks, without anybody at the controls:
  --headless            no window: with --replay run the file as fast as possible,
                        otherwise run in real time with nobody driving
  --realtime            with --replay --headless: keep to the clock, publish shared memory
  --duration <s>        stop by itself after this many seconds
  --dump-states <file>  with --replay --headless: write the car's state after every step
  --debug-view          the debug view (own shader, fast, shows the physics) instead of Assetto Corsa's renderer
  --warp                draw with WARP, Windows' software rasteriser (slow; no graphics card needed)
  --gpu-log <file>      write the Direct3D command log of the second frame
  --skin <folder>       the car's skin (default: race.ini's, else the first one)
  --video-ini-exact     follow video.ini also where plain acs.exe cannot make a proper picture with
                        it (SHADOW_MAP_SIZE=-1 and [CUBEMAP] SIZE=0, which Content Manager writes
                        for Custom Shaders Patch: everything in shadow, no reflections)
  --video-ini <file>    the video settings from this file instead of Documents\\Assetto Corsa\\cfg\\
                        video.ini (which is only ever read, never written)
  --graphics max|low    built-in video settings, your video.ini is not read. max: everything this
                        renderer does at the highest value Assetto Corsa's launcher offers (shadows
                        4096, world detail 5, anisotropic 16, reflections 1024 with 6 faces a frame (the
                        launcher's highest, 2048, costs too many frames: --cube-size 2048 has it),
                        mirrors 1024 in high quality with smoke, the virtual mirror, smoke 5, 4 samples
                        per pixel; post-processing off until YEBIS is ported: --post 1 --fxaa 1
                        --motion-blur 6 switch its frame on); low: everything off or at the lowest
                        value. The options above still win.
  --ideal-line, --no-ideal-line
                        AC's ideal line (the coloured arrows on the road) on or off from the start,
                        whatever assists.ini [ASSISTS] IDEAL_LINE says; Ctrl+I switches it while
                        driving (the key of controls.ini [IDEAL_LINE])
  --start-lights <s>    a race start for the picture: the track's start lights (and the drag strip's
                        semaphore) count down, the start being <s> seconds away, give or take one
                        (the game: 20). The car is not held on the grid (no races yet). A race.ini
                        whose session is a race does the same with 20; N starts it again
  --show-fps            the frame-rate counter on from the start (Ctrl+F or F12 switches it while
                        driving: frame time, FPS, physics steps a second)
  --no-a2c              NOT Assetto Corsa, a diagnosis only: no alpha to coverage. It proves where
                        the dotted trees and fences of a picture without anti-aliasing come from
                        (plain acs.exe with AASAMPLES=1 draws the same dots); the price is that
                        trees and fences turn into solid cards
  --aa <n>              samples per pixel ([VIDEO] AASAMPLES: 1, 2, 4 or 8) whatever the video
                        settings say. With more than one, alpha to coverage has steps to work
                        with and the dots on trees, fences and crowds go
  --post <0|1>          the post-processing frame ([POST_PROCESS] ENABLED) whatever the video
                        settings say: the scene goes through a 16-bit target as in the game. YEBIS
                        itself (filter, tone mapping, glare, depth of field) is not there yet, so
                        the picture is the plain one plus what the two options below add
  --fxaa <0|1>          AC's FXAA pass ([POST_PROCESS] FXAA); 1 switches the post-processing frame
                        on too (acs.exe has FXAA nowhere else)
  --motion-blur <n>     AC's motion blur ([EFFECTS] MOTION_BLUR, 0 to 12); above 0 it switches the
                        post-processing frame on too (the same in acs.exe)
  --hdr-levels          with the post-processing frame: the engine's HDR light levels as the game
                        sets them. Washed out: only YEBIS' tone mapper brings them back
  --cube-size <n>       the side of the reflection cube map ([CUBEMAP] SIZE: 512, 1024, 2048) whatever
                        the video settings say. 2048 is costly, in the game too: every face drawn again
                        fills four times the pixels, and the mip levels are made every frame
  --cube-faces <n>      faces of the reflection cube map drawn again every frame, 0 to 6
                        (default: video.ini [CUBEMAP] FACES_PER_FRAME; 0 = only once at load)
  --screenshot <png>    draw one frame off screen into a PNG and stop
  --at <s>              with --screenshot: seconds into the drive (default: the end of the replay, or 3)
  --no-hud              with --screenshot: the picture alone, without AC's HUD and rustyAC's panels
  --debug-hud           rustyAC's own panels and numbers too (in the window: F10 switches them);
                        without it the screen shows AC's GUI and HUD only
  --no-gui              without AC's GUI and HUD (the app bar, messages, flags, name tags ...)
  --dev-apps            AC's developers' apps get their icons in the app bar (as the game's
                        system/cfg/assetto_corsa.ini [AC_APPS] ENABLE_DEV_APPS=1 does)
  --language <code>     the GUI's language: en, it, de, fr, es, chs, cht (system/locales/<code>.ini;
                        default: the game's cfg/launcher.ini [WINDOW] LANGUAGE, else en)
  --gui-do <commands>   for pictures and unattended checks: things done to AC's GUI before the
                        first frame, ; between them: mouse:X:Y  click:X:Y  wheel:X:Y:N  key:VK
                        apps  desktop  names  damage  hidedamage  message:Title|Text  car:N
                        notify:N  flag:blue|black|yellow|penalty|chequered|none
  --cfg-dir <folder>    where rustyAC keeps its own acos.ini (the apps' layout) and gameplay.ini
                        (default: cfg beside rustyac.exe; Assetto Corsa's files are only read)
                        and without the virtual mirror
  --supersample <n>     with --screenshot: draw the picture n times wider and higher (2, 3 or 4) and
                        average it down to --width x --height: smoother edges and finer detail than
                        the samples of --aa alone (the drawn picture may be 7680 x 4320 at most)
  --free-camera <r,u,a>[/<r,u,a>[/<fov>]]
                        with --screenshot: a camera that stands still beside the car of the picture:
                        its place in metres to the right of, above and ahead of the car (level axes),
                        then the point it looks at (default 0,0.4,0) and its field of view in degrees
                        (default 60). -4,0.6,-5/0,0.4,0/45 is low, behind and to the left
  --bench-render        with --headless: also draw frames off screen and report their rate
  --bench-shot <png>    with --headless --bench-render: write the last frame drawn to a PNG
  --no-focus            show the window without activating it and keep running unfocused
";

impl Options {
    pub fn parse(args: impl Iterator<Item = String>) -> Result<Options, String> {
        let mut o = Options::default();
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            if arg == "--race-ini" {
                // the file is optional
                o.race_ini = Some(true);
                if args.peek().is_some_and(|next| !next.starts_with("--")) {
                    o.race_ini_file = args.next().map(PathBuf::from);
                }
                continue;
            }
            let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
            match arg.as_str() {
                "--car" => {
                    o.car = value("--car")?;
                    o.car_given = true;
                }
                "--join" => o.join = Some(value("--join")?),
                "--join-header" => {
                    let field = value("--join-header")?;
                    let (name, text) = field.split_once(':').ok_or("--join-header \"<Name>: <value>\"")?;
                    o.ws_headers.push((name.trim().to_string(), text.trim().to_string()));
                }
                "--tls-root" => o.tls_root = Some(PathBuf::from(value("--tls-root")?)),
                "--net-arrivals" => o.net_arrivals = Some(PathBuf::from(value("--net-arrivals")?)),
                "--offline" => o.offline = true,
                "--name" => o.driver_name = Some(value("--name")?),
                "--team" => o.team = Some(value("--team")?),
                "--password" => o.password = Some(value("--password")?),
                "--guid" => o.guid = Some(value("--guid")?),
                "--net-identity" => o.net_identity = Some(PathBuf::from(value("--net-identity")?)),
                "--no-steam" => o.no_steam = true,
                "--net-tape" => o.net_tape = Some(PathBuf::from(value("--net-tape")?)),
                "--chat" => {
                    let text = value("--chat")?;
                    let (line, at) = text.rsplit_once('@').ok_or("--chat <text>@<seconds>")?;
                    o.chat.push((line.to_string(), at.parse().map_err(|_| "--chat <text>@<seconds>".to_string())?));
                }
                "--width" => o.width = value("--width")?.parse().map_err(|e| format!("--width: {e}"))?,
                "--height" => o.height = value("--height")?.parse().map_err(|e| format!("--height: {e}"))?,
                "--windowed" => o.windowed = true,
                "--no-shm" => o.no_shm = true,
                "--record" => o.record = Some(PathBuf::from(value("--record")?)),
                "--replay" => o.replay = Some(PathBuf::from(value("--replay")?)),
                "--ffb" => o.ffb = true,
                "--no-rumble" => o.no_rumble = true,
                "--headless" => o.headless = true,
                "--realtime" => o.realtime = true,
                "--duration" => o.duration = Some(value("--duration")?.parse().map_err(|e| format!("--duration: {e}"))?),
                "--dump-states" => o.dump_states = Some(PathBuf::from(value("--dump-states")?)),
                "--screenshot" => o.screenshot = Some(PathBuf::from(value("--screenshot")?)),
                "--at" => o.at = Some(value("--at")?.parse().map_err(|e| format!("--at: {e}"))?),
                "--no-hud" => o.no_hud = true,
                "--debug-hud" => o.debug_hud = true,
                "--no-gui" => o.no_gui = true,
                "--dev-apps" => o.dev_apps = true,
                "--language" => o.language = Some(value("--language")?),
                "--cfg-dir" => o.cfg_dir = Some(PathBuf::from(value("--cfg-dir")?)),
                "--setups-dir" => o.setups_dir = Some(PathBuf::from(value("--setups-dir")?)),
                "--no-ac-setups" => o.no_ac_setups = true,
                "--esc-start" => o.esc_start = Some(true),
                "--no-esc-start" => o.esc_start = Some(false),
                "--gui-do" => o.gui_do = Some(value("--gui-do")?),
                "--supersample" => o.supersample = value("--supersample")?.parse().ok().filter(|n| (1..=4).contains(n)).ok_or("--supersample <1 to 4>")?,
                "--free-camera" => o.free_camera = Some(FreeCamera::parse(&value("--free-camera")?)?),
                "--pose-out" => o.pose_out = Some(PathBuf::from(value("--pose-out")?)),
                "--lead-in" => o.lead_in = value("--lead-in")?.parse().map_err(|e| format!("--lead-in: {e}"))?,
                "--virtual-mirror" => o.virtual_mirror = true,
                "--mirror-size" => o.mirror_size = Some(value("--mirror-size")?.parse().map_err(|_| "--mirror-size <pixels, 0: no mirrors>".to_string())?),
                "--sun" => o.sun = Some(value("--sun")?.parse().map_err(|e| format!("--sun: {e}"))?),
                "--weather" => o.weather = Some(value("--weather")?),
                "--mirror-hq" => o.mirror_hq = Some(value("--mirror-hq")? != "0"),
                "--world-detail" => o.world_detail = Some(value("--world-detail")?.parse().map_err(|e| format!("--world-detail: {e}"))?),
                "--render-seed" => o.render_seed = Some(value("--render-seed")?.parse().map_err(|e| format!("--render-seed: {e}"))?),
                "--time-mult" => o.time_mult = Some(value("--time-mult")?.parse().map_err(|e| format!("--time-mult: {e}"))?),
                "--show-lights" => o.show_lights = true,
                "--flat-out" => o.flat_out = true,
                "--flat-out-brake" => o.flat_out_brake = value("--flat-out-brake")?.parse().ok().filter(|b| (0.0..=1.0).contains(b)).ok_or("--flat-out-brake <0 to 1>")?,
                "--show-damage" => {
                    let text = value("--show-damage")?;
                    let levels: Vec<f32> = text.split(',').filter_map(|t| t.trim().parse().ok()).collect();
                    o.show_damage = Some(<[f32; 5]>::try_from(levels).map_err(|_| "--show-damage front,rear,left,right,centre (km/h)".to_string())?);
                }
                "--debug-view" => o.debug_view = true,
                "--warp" => o.warp = true,
                "--gpu-log" => o.gpu_log = Some(PathBuf::from(value("--gpu-log")?)),
                "--skin" => o.skin = Some(value("--skin")?),
                "--video-ini-exact" => o.video_ini_exact = true,
                "--video-ini" => {
                    let file = PathBuf::from(value("--video-ini")?);
                    if !file.is_file() {
                        return Err(format!("--video-ini {}: no such file", file.display()));
                    }
                    o.video_ini = Some(file);
                }
                "--graphics" => {
                    let name = value("--graphics")?;
                    if name != "max" && name != "low" {
                        return Err(format!("--graphics {name}: max or low"));
                    }
                    o.graphics = Some(name);
                }
                "--no-a2c" => o.no_a2c = true,
                "--post" => o.post = Some(value("--post")? != "0"),
                "--fxaa" => o.fxaa = Some(value("--fxaa")? != "0"),
                "--motion-blur" => o.motion_blur = Some(value("--motion-blur")?.parse::<f32>().ok().filter(|n| (0.0..=12.0).contains(n)).ok_or("--motion-blur <0 to 12>")?),
                "--hdr-levels" => o.hdr_levels = true,
                "--aa" => o.aa_samples = Some(value("--aa")?.parse().ok().filter(|n| [1, 2, 4, 8].contains(n)).ok_or("--aa <samples per pixel: 1, 2, 4 or 8>")?),
                "--show-fps" => o.show_fps = true,
                "--cube-size" => o.cube_size = Some(value("--cube-size")?.parse().ok().filter(|size| (16..=4096).contains(size)).ok_or("--cube-size <pixels: 512, 1024 or 2048>")?),
                "--cube-faces" => o.cube_faces = Some(value("--cube-faces")?.parse().map_err(|_| "--cube-faces <0..6>".to_string())?),
                "--camera" => o.camera = value("--camera")?,
                "--no-race-cars" => o.no_race_cars = true,
                "--focus" => o.focus = value("--focus")?.parse().map_err(|_| "--focus <car number, 0 = yours>".to_string())?,
                "--focus-script" => {
                    for item in value("--focus-script")?.split(',').filter(|s| !s.is_empty()) {
                        let (name, at) = item.split_once('@').ok_or("--focus-script next@2,previous@4,player@6")?;
                        if !matches!(name, "next" | "previous" | "player") {
                            return Err(format!("--focus-script: {name:?} is not next, previous or player"));
                        }
                        o.focus_script.push((at.parse().map_err(|_| format!("--focus-script {name}@<seconds>"))?, name.to_string()));
                    }
                }
                "--auto-shifter" => o.auto_shifter = true,
                "--no-auto-shifter" => o.no_auto_shifter = true,
                "--no-auto-clutch" => o.no_auto_clutch = true,
                "--auto-clutch" => o.auto_clutch = true,
                "--auto-blip" => o.auto_blip = Some(true),
                "--no-auto-blip" => o.auto_blip = Some(false),
                "--ideal-line" => o.ideal_line = Some(true),
                "--no-ideal-line" => o.ideal_line = Some(false),
                "--new-session-at" => o.new_session_at = Some(value("--new-session-at")?.parse().map_err(|_| "--new-session-at <seconds>")?),
                "--no-ac-replay" => o.no_ac_replay = true,
                "--replay-dir" => o.replay_dir = Some(PathBuf::from(value("--replay-dir")?)),
                "--save-replay" => o.save_replay = Some(PathBuf::from(value("--save-replay")?)),
                "--ghost" => {
                    let mode = value("--ghost")?;
                    if !["off", "record", "play", "both"].contains(&mode.as_str()) {
                        return Err("--ghost <off|record|play|both>".into());
                    }
                    o.ghost = Some(mode);
                }
                "--ghost-file" => o.ghost_file = Some(PathBuf::from(value("--ghost-file")?)),
                "--ghost-advantage" => o.ghost_advantage = Some(value("--ghost-advantage")?.parse().map_err(|_| "--ghost-advantage <seconds>")?),
                "--ghost-dir" => o.ghost_dir = Some(PathBuf::from(value("--ghost-dir")?)),
                "--telemetry-dir" => o.telemetry_dir = Some(PathBuf::from(value("--telemetry-dir")?)),
                "--no-telemetry" => o.no_telemetry = true,
                "--replay-script" => {
                    for item in value("--replay-script")?.split(',').filter(|s| !s.is_empty()) {
                        let (name, at) = item.split_once('@').ok_or("--replay-script <key@seconds,…>")?;
                        if !["pause", "slow", "ffwd", "stop", "replay", "rewind", "frame", "nextlap", "prevlap", "save", "exit"].contains(&name) {
                            return Err(format!("--replay-script: {name} is not one of pause, slow, ffwd, stop, replay, rewind, frame, nextlap, prevlap, save, exit"));
                        }
                        o.replay_script.push((at.parse().map_err(|_| "--replay-script <key@seconds,…>")?, name.to_string()));
                    }
                }
                "--start-lights" => o.start_lights = Some(value("--start-lights")?.parse().ok().filter(|s: &f32| s.is_finite() && *s >= 0.0).ok_or("--start-lights <seconds>")?),
                "--controls" => o.controls = Some(PathBuf::from(value("--controls")?)),
                "--default-controls" => o.default_controls = true,
                "--bench-render" => o.bench_render = true,
                "--bench-shot" => o.bench_shot = Some(PathBuf::from(value("--bench-shot")?)),
                "--no-focus" => o.no_focus = true,
                "--vsync" => o.vsync = value("--vsync")? != "0",
                "--list-devices" => o.list_devices = true,
                "--track" => o.track = Some(value("--track")?),
                "--layout" => o.layout = Some(value("--layout")?),
                "--list-tracks" => o.list_tracks = true,
                "--flat" => o.track = None,
                "--spawn" => {
                    o.spawn = value("--spawn")?;
                    o.spawn_given = true;
                }
                "--boxes" => o.boxes = true,
                "--texture-size" => o.texture_size = value("--texture-size")?.parse().map_err(|e| format!("--texture-size: {e}"))?,
                "--no-textures" => o.no_textures = true,
                "--no-audio" => o.audio.off = true,
                "--volume" => {
                    let volume: f32 = value("--volume")?.parse().map_err(|e| format!("--volume: {e}"))?;
                    if !(0.0..=1.0).contains(&volume) {
                        return Err("--volume is between 0 and 1".to_string());
                    }
                    o.audio.volume = Some(volume);
                }
                "--audio-wav" => o.audio.wav = Some(PathBuf::from(value("--audio-wav")?)),
                "--audio-log" => o.audio.log = Some(PathBuf::from(value("--audio-log")?)),
                "--audio-null" => o.audio.null = true,
                "--audio-ini" => o.audio.ini = Some(PathBuf::from(value("--audio-ini")?)),
                "--audio-oracle-camera" => o.audio.oracle_camera = Some(value("--audio-oracle-camera")?),
                "--audio-script" => o.audio.script = Some(PathBuf::from(value("--audio-script")?)),
                "--autodrive" => o.autodrive = true,
                "--cars" => o.cars = Some(value("--cars")?.parse().map_err(|e| format!("--cars: {e}"))?),
                "--opponent" => o.opponents.push(value("--opponent")?),
                "--ai-cars" => o.ai_cars = Some(value("--ai-cars")?.parse().map_err(|e| format!("--ai-cars: {e}"))?),
                "--ai-level" => o.ai_level = Some(value("--ai-level")?.parse().map_err(|e| format!("--ai-level: {e}"))?),
                "--ai-aggression" => o.ai_aggression = Some(value("--ai-aggression")?.parse().map_err(|e| format!("--ai-aggression: {e}"))?),
                "--ai-drive" => o.ai_drive = true,
                "--ai-at" => o.ai_at = Some(value("--ai-at")?.parse().map_err(|e| format!("--ai-at: {e}"))?),
                "--ai-laps" => o.ai_laps = Some(value("--ai-laps")?.parse().map_err(|e| format!("--ai-laps: {e}"))?),
                "--race-start-in" => o.race_start_in = Some(value("--race-start-in")?.parse().map_err(|e| format!("--race-start-in: {e}"))?),
                "--sessions" => o.sessions = Some(value("--sessions")?),
                "--no-referee" => o.no_referee = true,
                "--no-pit-stop" => o.no_pit_stop = true,
                "--ai-pit-at" => o.ai_pit_at.push(value("--ai-pit-at")?.parse().map_err(|e| format!("--ai-pit-at: {e}"))?),
                "--pit-window" => {
                    let text = value("--pit-window")?;
                    let window = text.split_once(',').and_then(|(open, close)| Some((open.trim().parse().ok()?, close.trim().parse().ok()?)));
                    o.pit_window = Some(window.ok_or_else(|| format!("--pit-window: <open>,<close> expected, got {text:?}"))?);
                }
                "--pit-key" => {
                    let text = value("--pit-key")?;
                    let wrong = || format!("--pit-key: <left|up|right|down|drive>@<seconds> expected, got {text:?}");
                    let (key, at) = text.split_once('@').ok_or_else(wrong)?;
                    let bit = match key {
                        "left" => crate::input_file::event::PIT_LEFT,
                        "up" => crate::input_file::event::PIT_UP,
                        "right" => crate::input_file::event::PIT_RIGHT,
                        "down" => crate::input_file::event::PIT_DOWN,
                        // Task 31: the ESC menu's "Drive" (a session that begins in the menu)
                        "drive" => crate::input_file::event::PITS_DRIVE,
                        _ => return Err(wrong()),
                    };
                    o.pit_keys.push((at.parse().map_err(|_| wrong())?, bit));
                }
                "--penalties" => o.penalties = Some(value("--penalties")? != "0"),
                "--jump-start" => o.jump_start = Some(value("--jump-start")?.parse().map_err(|e| format!("--jump-start: {e}"))?),
                "--next-session-at" => o.next_session_at.push(value("--next-session-at")?.parse().map_err(|e| format!("--next-session-at: {e}"))?),
                "--restart-session-at" => o.restart_session_at.push(value("--restart-session-at")?.parse().map_err(|e| format!("--restart-session-at: {e}"))?),
                "--out-dir" => o.out_dir = Some(PathBuf::from(value("--out-dir")?)),
                "--ai-seed" => o.ai_seed = Some(value("--ai-seed")?.parse().map_err(|e| format!("--ai-seed: {e}"))?),
                "--stand-in-drivers" => o.stand_in_drivers = true,
                "--physics-threads" => o.physics_threads = Some(value("--physics-threads")?.parse().map_err(|e| format!("--physics-threads: {e}"))?),
                "--state-hash" => o.state_hash = true,
                "--no-race-ini" => o.race_ini = Some(false),
                "--air" => o.air = Some(value("--air")?.parse().map_err(|e| format!("--air: {e}"))?),
                "--road" => o.road = Some(value("--road")?.parse().map_err(|e| format!("--road: {e}"))?),
                "--grip" => o.grip = Some(value("--grip")?.parse().map_err(|e| format!("--grip: {e}"))?),
                "--wind" => o.wind = Some(value("--wind")?.parse().map_err(|e| format!("--wind: {e}"))?),
                "--wind-dir" => o.wind_dir = Some(value("--wind-dir")?.parse().map_err(|e| format!("--wind-dir: {e}"))?),
                "--setup" => o.setup = Some(value("--setup")?),
                "--wind-from-log" => o.wind_from_log = true,
                "--damage" => {
                    let percent: i32 = value("--damage")?.parse().map_err(|_| "--damage <percent>: 0 (off) to 100".to_string())?;
                    if !(0..=100).contains(&percent) {
                        return Err("--damage <percent>: 0 (off) to 100".to_string());
                    }
                    o.damage = Some(percent);
                }
                "--air-density" => o.air_density = Some(value("--air-density")?.parse().map_err(|e| format!("--air-density: {e}"))?),
                "--pressure-law" => {
                    let text = value("--pressure-law")?;
                    let numbers: Vec<f32> = text.split(',').map(|part| part.trim().parse::<f32>()).collect::<Result<_, _>>().map_err(|e| format!("--pressure-law {text}: {e}"))?;
                    let [at_26, per_degree] = numbers[..] else { return Err(format!("--pressure-law {text}: two numbers expected, for example 10.97,0.084")) };
                    o.pressure_law = Some((at_26, per_degree));
                }
                "--help" | "-h" | "/?" => return Err(USAGE.to_string()),
                other => return Err(format!("unknown option {other}\n\n{USAGE}")),
            }
        }
        if o.width < 320 || o.height < 200 || o.width > 7680 || o.height > 4320 {
            return Err(format!("--width / --height: {} x {} is out of range", o.width, o.height));
        }
        if o.width * o.supersample > 7680 || o.height * o.supersample > 4320 {
            return Err(format!("--supersample {}: the picture would be drawn at {} x {}, more than 7680 x 4320", o.supersample, o.width * o.supersample, o.height * o.supersample));
        }
        if (o.supersample > 1 || o.free_camera.is_some() || o.no_hud) && o.screenshot.is_none() {
            return Err("--supersample, --free-camera and --no-hud need --screenshot".to_string());
        }
        if o.flat_out_brake > 0.0 && !o.flat_out {
            return Err("--flat-out-brake needs --flat-out".to_string());
        }
        let car_camera = o.camera.strip_prefix("car").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if !["cockpit", "chase", "chase2", "bonnet", "bumper", "dash"].contains(&o.camera.as_str()) && !car_camera {
            return Err(crate::render::scene::camera_name_error(&o.camera));
        }
        if o.grip.is_some_and(|grip| !(grip > 0.0 && grip <= 150.0)) {
            return Err("--grip: a percentage is expected, for example 97 (or 0.97)".to_string());
        }
        if o.wind.is_some_and(|wind| !(0.0..=200.0).contains(&wind)) {
            return Err("--wind: km/h from 0 up is expected".to_string());
        }
        if o.air_density.is_some_and(|density| !(0.5..=2.0).contains(&density)) {
            return Err("--air-density: kg/m3 is expected, between 0.5 and 2".to_string());
        }
        if o.wind_from_log && (o.wind.is_some() || o.wind_dir.is_some()) {
            return Err("--wind-from-log cannot be used with --wind or --wind-dir".to_string());
        }
        let session_options = o.wind_from_log || o.damage.is_some() || o.air_density.is_some() || o.pressure_law.is_some() || o.race_ini.is_some() || o.air.is_some() || o.road.is_some() || o.grip.is_some() || o.wind.is_some() || o.wind_dir.is_some() || o.setup.is_some();
        if session_options && o.replay.is_some() {
            return Err("--replay drives in the conditions stored in the file: --race-ini, --no-race-ini, --air, --road, --grip, --wind, --wind-dir, --wind-from-log, --setup, --damage, --air-density and --pressure-law cannot be used with it".to_string());
        }
        if o.graphics.is_some() && o.video_ini.is_some() {
            return Err("--graphics and --video-ini cannot be used together: a preset reads no file".to_string());
        }
        if o.layout.is_some() && o.track.is_none() {
            return Err("--layout needs --track <name>".to_string());
        }
        if o.autodrive && o.track.is_none() && o.join.is_none() {
            return Err("--autodrive needs --track <folder>: the driver follows the track's AI line".to_string());
        }
        if o.cars.is_some_and(|n| n == 0 || n > 64) {
            return Err("--cars: 1 to 64 cars".to_string());
        }
        if o.cars.is_some() && !o.opponents.is_empty() {
            return Err("--cars and --opponent cannot be used together: name every other car with --opponent".to_string());
        }
        if (o.cars.is_some_and(|n| n > 1) || !o.opponents.is_empty()) && o.replay.is_some() {
            return Err("a recorded drive names its own other cars: --cars and --opponent cannot be used with --replay".to_string());
        }
        if o.state_hash && !(o.replay.is_some() && o.headless) {
            return Err("--state-hash needs --replay and --headless".to_string());
        }
        if o.autodrive && o.replay.is_some() {
            return Err("--autodrive and --replay cannot be used together".to_string());
        }
        if !["hotlap", "pit", "start"].contains(&o.spawn.as_str()) {
            return Err(format!("--spawn: {:?} is not one of hotlap, pit, start", o.spawn));
        }
        if o.record.is_some() && o.replay.is_some() {
            return Err("--record and --replay cannot be used together".to_string());
        }
        if o.record.is_some() && o.screenshot.is_some() && !o.autodrive && !o.flat_out {
            return Err("--record with --screenshot needs --autodrive or --flat-out (otherwise nobody drives)".to_string());
        }
        if o.dump_states.is_some() && !(o.replay.is_some() && o.headless) {
            return Err("--dump-states needs --replay and --headless".to_string());
        }
        if o.dump_states.is_some() && o.realtime {
            return Err("--dump-states cannot be used with --realtime".to_string());
        }
        for (name, value) in [("--duration", o.duration), ("--at", o.at)] {
            if value.is_some_and(|seconds| !(0.0..=1.0e9).contains(&seconds)) {
                return Err(format!("{name}: a number of seconds from 0 up is expected"));
            }
        }
        Ok(o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Result<Options, String> {
        Options::parse(line.split_whitespace().map(str::to_string))
    }

    #[test]
    fn the_screenshot_options_of_the_site_pictures() {
        // without them nothing changes
        let plain = parse("--screenshot a.png").unwrap();
        assert_eq!((plain.supersample, plain.free_camera, plain.no_hud), (1, None, false));
        assert!(parse("--screenshot a.png --no-hud").unwrap().no_hud && parse("--no-hud").is_err());
        let o = parse("--screenshot a.png --width 2560 --height 1440 --supersample 2 --free-camera -4,0.6,-5").unwrap();
        assert_eq!(o.supersample, 2);
        assert_eq!(o.free_camera, Some(FreeCamera { eye: [-4.0, 0.6, -5.0], look: [0.0, 0.4, 0.0], fov: 60.0 }));
        let o = parse("--screenshot a.png --free-camera 3,1,2/0,0.5,1/35").unwrap();
        assert_eq!(o.free_camera, Some(FreeCamera { eye: [3.0, 1.0, 2.0], look: [0.0, 0.5, 1.0], fov: 35.0 }));
        // the drawn picture has a largest size, and both options are a screenshot's
        assert!(parse("--screenshot a.png --width 2560 --height 1440 --supersample 4").unwrap_err().contains("10240 x 5760"));
        assert!(parse("--screenshot a.png --supersample 5").is_err());
        assert!(parse("--supersample 2").unwrap_err().contains("need --screenshot"));
        assert!(parse("--free-camera 1,2,3").unwrap_err().contains("need --screenshot"));
        assert_eq!(parse("--screenshot a.png --flat-out").unwrap().flat_out_brake, 0.0);
        assert_eq!(parse("--screenshot a.png --flat-out --flat-out-brake 0.6").unwrap().flat_out_brake, 0.6);
        assert!(parse("--screenshot a.png --flat-out-brake 0.6").is_err() && parse("--screenshot a.png --flat-out --flat-out-brake 2").is_err());
        for bad in ["1,2", "1,2,3/4,5", "1,2,3/1,2,3", "1,2,3/0,0,0/0", "1,2,3/0,0,0/60/1", "a,b,c"] {
            assert!(parse(&format!("--screenshot a.png --free-camera {bad}")).is_err(), "{bad}");
        }
    }
}
