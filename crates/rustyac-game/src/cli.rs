// SPDX-License-Identifier: GPL-3.0-or-later

//! The command line.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// `--car`: a folder under `cardata/`, or a path to a car's data folder.
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
}

impl Default for Options {
    fn default() -> Options {
        Options {
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
        }
    }
}

pub const USAGE: &str = "\
rustyac: drive the Rust port of Assetto Corsa's car on a track or on an endless flat road (debug view)

usage: rustyac [options]

  --car <folder>        car data folder under cardata/, or a path (default ks_ferrari_f2004)
  --track <folder>      a track: a folder, or a name under Assetto Corsa's content/tracks
                        (for example --track spa); default: the endless flat road
  --flat                the endless flat road (the default)
  --spawn <where>       on a track: hotlap (default), pit or start
  --boxes               draw the car as boxes, not with its 3D model
  --texture-size <px>   longest texture side put on the graphics card (default 1024, 0 = as stored)
  --no-textures         flat colours instead of textures
  --width <px>          size of the picture (default 1280 x 720; with --windowed the window's
  --height <px>         client area, otherwise the off-screen picture of --screenshot)
  --windowed            a window instead of borderless full screen
  --no-shm              do not publish acpmf_physics / acpmf_graphics / acpmf_static
  --record <file>       log the inputs of every physics step
  --replay <file>       drive from a recorded input file
  --ffb                 force feedback to a DirectInput wheel, if one is found (low strength)
  --no-rumble           no rumble on the Xbox pad
  --auto-shifter        start with the automatic gearbox aid on (Ctrl+G toggles it)
  --no-auto-clutch      switch the automatic clutch aid off
  --controls <ini>      bindings file to use (default: AC's own controls.ini, read-only)
  --default-controls    ignore AC's controls.ini and use the built-in Xbox / keyboard layout
  --camera <name>       chase (default) or cockpit
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
        let mut args = args;
        while let Some(arg) = args.next() {
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
                "--no-auto-clutch" => o.no_auto_clutch = true,
                "--controls" => o.controls = Some(PathBuf::from(value("--controls")?)),
                "--default-controls" => o.default_controls = true,
                "--bench-render" => o.bench_render = true,
                "--no-focus" => o.no_focus = true,
                "--vsync" => o.vsync = value("--vsync")? != "0",
                "--list-devices" => o.list_devices = true,
                "--track" => o.track = Some(value("--track")?),
                "--flat" => o.track = None,
                "--spawn" => o.spawn = value("--spawn")?,
                "--boxes" => o.boxes = true,
                "--texture-size" => o.texture_size = value("--texture-size")?.parse().map_err(|e| format!("--texture-size: {e}"))?,
                "--no-textures" => o.no_textures = true,
                "--help" | "-h" | "/?" => return Err(USAGE.to_string()),
                other => return Err(format!("unknown option {other}\n\n{USAGE}")),
            }
        }
        if o.width < 320 || o.height < 200 || o.width > 7680 || o.height > 4320 {
            return Err(format!("--width / --height: {} x {} is out of range", o.width, o.height));
        }
        if o.camera != "chase" && o.camera != "cockpit" {
            return Err(format!("--camera: {:?} is neither chase nor cockpit", o.camera));
        }
        if !["hotlap", "pit", "start"].contains(&o.spawn.as_str()) {
            return Err(format!("--spawn: {:?} is not one of hotlap, pit, start", o.spawn));
        }
        if o.record.is_some() && o.replay.is_some() {
            return Err("--record and --replay cannot be used together".to_string());
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
