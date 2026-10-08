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
    /// `--camera chase|cockpit`: the camera at start.
    pub camera: String,
    /// `--auto-shifter`: the automatic gearbox aid on at start.
    pub auto_shifter: bool,
    /// `--no-auto-clutch`: the automatic clutch aid off.
    pub no_auto_clutch: bool,
    /// `--auto-clutch`, `--no-auto-shifter`, `--auto-blip`, `--no-auto-blip`: the other ways
    /// round, over assists.ini.
    pub auto_clutch: bool,
    pub no_auto_shifter: bool,
    pub auto_blip: Option<bool>,
    /// `--spawn` was given (else race.ini's `[SESSION_0] SPAWN_SET` decides).
    pub spawn_given: bool,
    /// `--controls <ini>`: read the bindings from this file instead of AC's / rustyAC's.
    pub controls: Option<PathBuf>,
    /// `--default-controls`: ignore AC's controls.ini, use the built-in layout.
    pub default_controls: bool,
    /// `--bench-render`: with `--headless`, also draw frames off screen and report their rate.
    pub bench_render: bool,
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
    /// `--spawn hotlap|pit|start`: where on the track the car starts.
    pub spawn: String,
    /// `--boxes`: draw the car as boxes even when its 3D model is found.
    pub boxes: bool,
    /// `--texture-size <px>`: the longest side of a texture that is put on the card (0: as stored).
    pub texture_size: u32,
    /// `--no-textures`: flat colours instead of textures.
    pub no_textures: bool,
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
    /// `--pressure-law <psi at 26 C>,<psi per C>`: another tyre pressure law (an experiment,
    /// not AC).
    pub pressure_law: Option<(f32, f32)>,
    /// `--autodrive`: a driver that follows the track's AI line (needs `--track`).
    pub autodrive: bool,
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
            at: None,
            camera: "chase".to_string(),
            auto_shifter: false,
            no_auto_clutch: false,
            auto_clutch: false,
            no_auto_shifter: false,
            auto_blip: None,
            spawn_given: false,
            controls: None,
            default_controls: false,
            bench_render: false,
            no_focus: false,
            vsync: true,
            list_devices: false,
            track: None,
            spawn: "hotlap".to_string(),
            boxes: false,
            texture_size: 1024,
            no_textures: false,
            autodrive: false,
        }
    }
}

pub const USAGE: &str = "\
rustyac: drive the Rust port of Assetto Corsa's car on a track or on an endless flat road (debug view)

usage: rustyac [options]

  --car <name|folder>   a car of your Assetto Corsa install by its folder name (default
                        ks_ferrari_f2004; read out of its data.acd in memory), or a path to
                        a car folder or to a folder with the plain data files
  --track <folder>      a track: a folder, or a name under Assetto Corsa's content/tracks
                        (for example --track spa); default: the endless flat road
  --flat                the endless flat road (the default)
  --spawn <where>       on a track: hotlap, pit or start (default: where race.ini's session
                        starts, [SESSION_0] SPAWN_SET; without a race.ini hotlap). In a
                        hot-lap session (a [SESSION_n] with TYPE=4, or no race.ini) the clock
                        starts again when the car first crosses the line; in any other it
                        runs from the spawn point and lap 1 includes the run-up, as in the game
  --boxes               draw the car as boxes, not with its 3D model
  --texture-size <px>   longest texture side put on the graphics card (default 1024, 0 = as stored)
  --no-textures         flat colours instead of textures
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
  --air-density <kg/m3> NOT Assetto Corsa: a fixed air density instead of the game's
                        1.2922 - 0.0041 x air temperature, to try what Custom Shaders Patch's
                        thinner air at altitude does (about 1.165 at Spa at 14 C)
  --pressure-law <a>,<b> NOT Assetto Corsa: tyre pressure = a + b x (core temperature - 26 C)
                        instead of the game's static pressure + 0.16 x (core - 26), to try
                        what Custom Shaders Patch's pressures do (the F2004 at 14 C: 10.97,0.084)
  --autodrive           on a track: nobody at the controls, a simple driver follows the
                        track's AI line (automatic gearbox on); for checks and for watching
  --width <px>          size of the picture (default 1280 x 720; with --windowed the window's
  --height <px>         client area, otherwise the off-screen picture of --screenshot)
  --windowed            a window instead of borderless full screen
  --no-shm              do not publish acpmf_physics / acpmf_graphics / acpmf_static
  --record <file>       log the inputs of every physics step
  --replay <file>       drive from a recorded input file
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
  --vsync <0|1>         wait for the display (default 1)
  --list-devices        print the detected devices and the active bindings, then stop

for checks, without anybody at the controls:
  --headless            no window: with --replay run the file as fast as possible,
                        otherwise run in real time with nobody driving
  --realtime            with --replay --headless: keep to the clock, publish shared memory
  --duration <s>        stop by itself after this many seconds
  --dump-states <file>  with --replay --headless: write the car's state after every step
  --screenshot <png>    draw one frame off screen into a PNG and stop
  --at <s>              with --screenshot: seconds into the drive (default: the end of the replay, or 3)
  --bench-render        with --headless: also draw frames off screen and report their rate
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
                "--car" => o.car = value("--car")?,
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
                "--camera" => o.camera = value("--camera")?,
                "--auto-shifter" => o.auto_shifter = true,
                "--no-auto-shifter" => o.no_auto_shifter = true,
                "--no-auto-clutch" => o.no_auto_clutch = true,
                "--auto-clutch" => o.auto_clutch = true,
                "--auto-blip" => o.auto_blip = Some(true),
                "--no-auto-blip" => o.auto_blip = Some(false),
                "--controls" => o.controls = Some(PathBuf::from(value("--controls")?)),
                "--default-controls" => o.default_controls = true,
                "--bench-render" => o.bench_render = true,
                "--no-focus" => o.no_focus = true,
                "--vsync" => o.vsync = value("--vsync")? != "0",
                "--list-devices" => o.list_devices = true,
                "--track" => o.track = Some(value("--track")?),
                "--flat" => o.track = None,
                "--spawn" => {
                    o.spawn = value("--spawn")?;
                    o.spawn_given = true;
                }
                "--boxes" => o.boxes = true,
                "--texture-size" => o.texture_size = value("--texture-size")?.parse().map_err(|e| format!("--texture-size: {e}"))?,
                "--no-textures" => o.no_textures = true,
                "--autodrive" => o.autodrive = true,
                "--no-race-ini" => o.race_ini = Some(false),
                "--air" => o.air = Some(value("--air")?.parse().map_err(|e| format!("--air: {e}"))?),
                "--road" => o.road = Some(value("--road")?.parse().map_err(|e| format!("--road: {e}"))?),
                "--grip" => o.grip = Some(value("--grip")?.parse().map_err(|e| format!("--grip: {e}"))?),
                "--wind" => o.wind = Some(value("--wind")?.parse().map_err(|e| format!("--wind: {e}"))?),
                "--wind-dir" => o.wind_dir = Some(value("--wind-dir")?.parse().map_err(|e| format!("--wind-dir: {e}"))?),
                "--setup" => o.setup = Some(value("--setup")?),
                "--wind-from-log" => o.wind_from_log = true,
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
        let session_options = o.wind_from_log || o.air_density.is_some() || o.pressure_law.is_some() || o.race_ini.is_some() || o.air.is_some() || o.road.is_some() || o.grip.is_some() || o.wind.is_some() || o.wind_dir.is_some() || o.setup.is_some();
        if session_options && o.replay.is_some() {
            return Err("--replay drives in the conditions stored in the file: --race-ini, --no-race-ini, --air, --road, --grip, --wind, --wind-dir, --wind-from-log, --setup, --air-density and --pressure-law cannot be used with it".to_string());
        }
        if o.autodrive && o.track.is_none() {
            return Err("--autodrive needs --track <folder>: the driver follows the track's AI line".to_string());
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
        if o.record.is_some() && o.screenshot.is_some() && !o.autodrive {
            return Err("--record with --screenshot needs --autodrive (otherwise nobody drives)".to_string());
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
