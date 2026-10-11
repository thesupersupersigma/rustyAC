// SPDX-License-Identifier: GPL-3.0-or-later

//! `site_clips`: sound clips of one car for the website, written without a clock through
//! FMOD's non-real-time WAV writer (one 800-sample block per 1/60 s frame, 48 kHz, stereo,
//! 16 bit), with Assetto Corsa's own banks and the player's own FMOD DLLs, by the same
//! `CarAudioFMOD` port the game plays. One clip per run (FMOD is loaded once per process).
//!
//! ```text
//! cargo run --release -p rustyac-game --example site_clips -- steady --out on_7000.wav --rpm 7000 --gas 1
//! cargo run --release -p rustyac-game --example site_clips -- steady --out spin.wav --rpm 17000 --gas 1 --spin 250 --speed 8 --engine-level 0
//! cargo run --release -p rustyac-game --example site_clips -- event --out gun.wav --path event:/common/screw
//! cargo run --release -p rustyac-game --example site_clips -- drive --out burnout.wav --track spa --after-pop 2
//! cargo run --release -p rustyac-game --example site_clips -- drive --out limiter.wav --neutral --seconds 10
//! ```
//!
//! * `steady`: no physics. The car stands at the origin and its sound is given the same state
//!   every frame: `--rpm`, `--gas` (the engine event's `rpms` and `throttle`), and for the
//!   tyres `--spin <rad/s>` (the rear wheels' speed, with a slip ratio of `--slip`, default 1)
//!   and `--speed <m/s>` (the car's speed as the sound reads it: AC's skid volume is
//!   `speed x 3.6 x 0.05`, so a car that does not move at all has silent tyres), `--inflation`
//!   and `--limiter` (the limiter flag).
//! * `event`: one event by its path, started once at the car's place (`--param name=value`
//!   sets a parameter first, `--base-volume` its volume; the car itself is silent).
//! * `drive`: rustyAC's physics drives, nobody steers and the throttle is held down, as
//!   `rustyac.exe --flat-out --no-auto-clutch` does (`--neutral`: in neutral, the engine on
//!   its limiter; `--brake <0..1>`: the brake pedal held too, as `--flat-out-brake` does: a
//!   burnout on the spot). It ends after `--seconds`, or `--after-pop` seconds after a tyre bursts.
//!
//! The listener is the free camera's (the engine 2.5 times louder, as the game does for the
//! track and free cameras), `--listener <right,up,ahead>` metres from the car (default
//! 5.5,1.2,-2.5: 6 m away, beside and a little behind), moving with the car. There is no
//! track sound: no ambience, no crowd, no reverb. `--volume` is the master level (FMOD's
//! writer clips, so keep it low and raise all clips by one gain afterwards); `--engine-level`
//! and `--tyres-level` are `[LEVELS] ENGINE` and `TYRES` of the audio.ini the run is given.

#[cfg(not(windows))]
fn main() {
    eprintln!("site_clips needs Windows: the sound is Assetto Corsa's FMOD DLLs");
}

#[cfg(windows)]
fn main() {
    if let Err(message) = clips::run() {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
mod clips {
    use std::collections::HashMap;
    use std::ffi::CString;
    use std::path::PathBuf;

    use rustyac_audio::car::{CameraView, CarFrame, CarInfo, SimView};
    use rustyac_audio::engine::{AudioEngine, EngineFiles, Mat, REVERB_OFF};
    use rustyac_audio::fmod::raw;
    use rustyac_audio::sim::{AudioWorld, CarSound, FrameInput, PhysicsEvent};
    use rustyac_game::audio::{capture, NRT_BLOCK, NRT_DT, NRT_RATE};
    use rustyac_game::cli::Options;
    use rustyac_game::input_file::SimSetup;
    use rustyac_game::render::scene::free_camera;
    use rustyac_game::sim::{find_car_data, DriverSource, FlatOutSource, GameSim, SpawnSequence};
    use rustyac_physics::car::{CarControls, CarControlsInput};

    /// The throttle held down in neutral, the clutch pedal up.
    struct NeutralSource;

    impl DriverSource for NeutralSource {
        fn acquire(&mut self, controls: &mut CarControls, _dt: f32, _input: &CarControlsInput) {
            *controls = CarControls { clutch: 1.0, gas: 1.0, ..CarControls::default() };
        }
    }

    struct Args {
        mode: String,
        values: HashMap<String, Vec<String>>,
    }

    impl Args {
        fn parse() -> Result<Args, String> {
            let mut args = std::env::args().skip(1);
            let mode = args.next().ok_or("site_clips <steady|event|drive> --out <wav> ... (see the top of examples/site_clips.rs)")?;
            let mut values: HashMap<String, Vec<String>> = HashMap::new();
            while let Some(name) = args.next() {
                let key = name.strip_prefix("--").ok_or(format!("{name}: an option is expected"))?.to_string();
                if ["neutral", "limiter"].contains(&key.as_str()) {
                    values.entry(key).or_default().push("1".to_string());
                } else {
                    values.entry(key).or_default().push(args.next().ok_or(format!("{name} needs a value"))?);
                }
            }
            Ok(Args { mode, values })
        }

        fn text(&self, key: &str) -> Option<&str> {
            self.values.get(key).and_then(|v| v.last()).map(String::as_str)
        }

        fn number(&self, key: &str, default: f32) -> Result<f32, String> {
            match self.text(key) {
                Some(text) => text.parse().map_err(|_| format!("--{key} {text}: a number is expected")),
                None => Ok(default),
            }
        }
    }

    fn flat(m: &[[f32; 4]; 4]) -> Mat {
        std::array::from_fn(|i| m[i / 4][i % 4])
    }

    /// The listener for a car: the free camera `offset` metres to the right of, above and
    /// ahead of it, looking at it, moving as it moves.
    fn listener(frame: &CarFrame, offset: [f32; 3]) -> (Mat, [f32; 3]) {
        let w = &frame.world_matrix;
        let body: [[f32; 4]; 4] = std::array::from_fn(|r| std::array::from_fn(|c| w[r * 4 + c]));
        (flat(&free_camera(&body, offset, [0.0, 0.3, 0.0])), frame.velocity)
    }

    /// The state of a car that stands at the origin, for `steady` and `event`.
    fn standing(rpm: f32, gas: f32) -> CarFrame {
        let at = |x: f32, y: f32, z: f32| -> Mat { [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, x, y, z, 1.0] };
        CarFrame {
            world_matrix: at(0.0, 0.35, 0.0),
            // left front, right front, left rear, right rear (+x is the car's left)
            tyre_matrix: [at(0.75, 0.33, 1.75), at(-0.75, 0.33, 1.75), at(0.72, 0.33, -1.3), at(-0.72, 0.33, -1.3)],
            engine_rpm: rpm,
            is_engine_limiter_on: false,
            wheel_angular_speed: [0.0; 4],
            gas,
            brake: 0.0,
            gear: 1,
            speed: 0.0,
            velocity: [0.0; 3],
            slip_ratio: [0.0; 4],
            nd_slip: [0.0; 4],
            load: [1500.0, 1500.0, 2000.0, 2000.0],
            tyre_dirty_level: [0.0; 4],
            surface_wav: Default::default(),
            surface_grip_mod: [1.0; 4],
            drivetrain_speed: 0.0,
            turbo_boost: 0.0,
            is_gear_grinding: false,
            body_work_volume: 0.0,
            air_density: 1.186,
            fuel: 30.0,
            engine_life_left: 1000.0,
            turbo_bov: 0.0,
            actions_state: 0,
            tyre_inflation: [1.0; 4],
            sus_damage: [0.0; 4],
            node_active: true,
            tc_in_action: false,
            body_position: [0.0, 0.35, 0.0],
            pit_position: [0.0, -1000.0, 0.0],
        }
    }

    pub fn run() -> Result<(), String> {
        let args = Args::parse()?;
        let out = PathBuf::from(args.text("out").ok_or("--out <wav> is needed")?);
        let out = std::path::absolute(&out).map_err(|e| e.to_string())?;
        let car_name = args.text("car").unwrap_or("ks_ferrari_f2004").to_string();
        let seconds = args.number("seconds", 10.0)?;
        let offset: [f32; 3] = match args.text("listener") {
            Some(text) => {
                let numbers: Vec<f32> = text.split(',').filter_map(|n| n.trim().parse().ok()).collect();
                <[f32; 3]>::try_from(numbers).map_err(|_| format!("--listener {text}: <right,up,ahead> in metres is expected"))?
            }
            None => [5.5, 1.2, -2.5],
        };

        // the levels of this run: an audio.ini of its own beside the WAV
        let ini = out.with_extension("audio.ini");
        let levels = format!(
            "[LEVELS]\nMASTER=1\nENGINE={}\nTYRES={}\nSURFACES=1\nWIND=1\nDIRT_BOTTOM=1\nOPPONENTS=1\n\n[SKIDS]\nENTRY_POINT=100\n",
            args.number("engine-level", 1.0)?,
            args.number("tyres-level", 1.0)?
        );
        std::fs::write(&ini, levels).map_err(|e| format!("{}: {e}", ini.display()))?;

        let ac = rustyac_content::install::ac_root().ok_or("Assetto Corsa's folder was not found: its FMOD DLLs and sound banks are needed")?;
        raw::load(&ac, raw::Output::WavNrt { file: out.clone(), rate: NRT_RATE, block: NRT_BLOCK })?;
        let mut engine = AudioEngine::new(EngineFiles { content_root: ac.clone(), audio_engine_ini: ac.join("system/cfg/audio_engine.ini"), audio_ini: ini })?;
        engine.set_volume(args.number("volume", 0.2)?.clamp(0.0, 1.0));
        let data_folder = find_car_data(&car_name)?;
        let car = CarSound::new(&mut engine, CarInfo { unix_name: car_name.clone(), guid: 0, data_folder, car_cameras_external_sound: Vec::new() })?;
        let audio = car.audio.as_ref().ok_or(format!("{car_name} has no sound bank"))?;
        println!("{car_name}: optional events in its bank: {:?}; core events: {:?}", audio.optional_events(), audio.core_events(&engine));
        let mut world = AudioWorld::new(engine, None, vec![car]);
        let sim = SimView { focused_car_index: 0, camera: Some(CameraView::FREE) };
        world.set_audio_distance_scale(&sim, 1.0);
        let one = |world: &mut AudioWorld, state: &CarFrame, events: &[PhysicsEvent]| {
            world.frame(&FrameInput { cars: std::slice::from_ref(state), events, sim, listener: Some(listener(state, offset)), dt: NRT_DT });
        };
        let total = (seconds * 60.0).round() as usize;

        match args.mode.as_str() {
            "steady" => {
                let mut state = standing(args.number("rpm", 4000.0)?, args.number("gas", 0.0)?);
                state.is_engine_limiter_on = args.text("limiter").is_some();
                state.speed = args.number("speed", 0.0)?;
                let spin = args.number("spin", 0.0)?;
                if spin != 0.0 {
                    let slip = args.number("slip", 1.0)?;
                    for wheel in 2..4 {
                        state.wheel_angular_speed[wheel] = spin;
                        state.slip_ratio[wheel] = slip;
                        state.nd_slip[wheel] = slip * 10.0;
                    }
                }
                let inflation = args.number("inflation", 1.0)?;
                state.tyre_inflation[2] = inflation;
                state.tyre_inflation[3] = inflation;
                for _ in 0..total {
                    one(&mut world, &state, &[]);
                }
                println!("steady: {} frames at {} rpm, gas {}, rear wheels {} rad/s, speed {} m/s", total, state.engine_rpm, state.gas, spin, state.speed);
            }
            "event" => {
                let path = args.text("path").ok_or("--path <event:/...> is needed")?.to_string();
                if !world.engine.has_event(&path) {
                    return Err(format!("{path}: no such event in the banks that are loaded"));
                }
                // the car is there (its bank is loaded) and silent
                let mut state = standing(0.0, 0.0);
                state.node_active = false;
                let event = world.engine.create_event(&path, REVERB_OFF);
                world.engine.event_set_base_volume(event, args.number("base-volume", 1.0)?);
                let place = match args.text("at") {
                    // CarAudioFMOD::computeTransforms: half a metre behind the rear axle
                    Some("exhaust") => [0.0, 0.33, -1.8],
                    _ => [0.0, 0.35, 0.0],
                };
                let mut m = state.world_matrix;
                m[12..15].copy_from_slice(&place);
                // two seconds first: the master level has come up, and the start can be found
                let lead = 120;
                for frame in 0..lead + total {
                    if frame == lead {
                        world.engine.event_set_3d(event, &m, &[0.0; 3]);
                        for pair in args.values.get("param").map(Vec::as_slice).unwrap_or(&[]) {
                            let (name, value) = pair.split_once('=').ok_or(format!("--param {pair}: name=value is expected"))?;
                            let value: f32 = value.parse().map_err(|_| format!("--param {pair}: a number is expected"))?;
                            world.engine.event_set_parameter(event, &CString::new(name).map_err(|e| e.to_string())?, value);
                        }
                        world.engine.event_start(event);
                    }
                    one(&mut world, &state, &[]);
                }
                println!("event: {path} started after {lead} frames, {total} frames after it; still playing at the end: {}", world.engine.event_is_playing(event));
                world.engine.destroy_event(event);
            }
            "drive" => {
                let neutral = args.text("neutral").is_some();
                let mut line = vec!["--car".to_string(), car_name.clone(), "--no-race-ini".to_string(), "--no-auto-clutch".to_string()];
                if let Some(track) = args.text("track") {
                    line.extend(["--track".to_string(), track.to_string()]);
                }
                let options = Options::parse(line.into_iter())?;
                let (mut track, mut layout) = (String::new(), String::new());
                if let Some(name) = &options.track {
                    let found = rustyac_game::sim::resolve_track(name, None, None)?;
                    track = found.entry.track.clone();
                    layout = found.entry.layout;
                }
                let mut setup = SimSetup { car: car_name.clone(), auto_clutch: true, track, layout, track_objects: true, session_transfer: true, spawn: options.spawn.clone(), session_starts_at_spawn: true, drs_zones: true, ..SimSetup::default() };
                // a wheel's bindings: the device does not force the automatic clutch
                for note in rustyac_game::conditions::apply(&options, &mut setup, "WHEEL")? {
                    println!("{note}");
                }
                let source: Box<dyn DriverSource> = if neutral { Box::new(SpawnSequence::new(NeutralSource, false)) } else { Box::new(SpawnSequence::new(FlatOutSource::with_brake(args.number("brake", 0.0)?), true)) };
                let mut game = GameSim::new(setup, source)?;
                let after_pop = args.number("after-pop", -1.0)?;
                let mut sticky = Default::default();
                let (mut frames, mut popped_at, mut pending): (u64, Option<u64>, Vec<PhysicsEvent>) = (0, None, Vec::new());
                loop {
                    game.step()?;
                    for e in &game.car.car.physics_events {
                        pending.push(PhysicsEvent { kind: e.kind, param1: e.param1, param2: e.param2, param3: e.param3, param4: e.param4, v_param1: [e.v_param1.x, e.v_param1.y, e.v_param1.z], v_param2: [e.v_param2.x, e.v_param2.y, e.v_param2.z], ul_param0: e.ul_param0 as u64 });
                    }
                    // a frame after every floor(n * 50 / 9) steps: 60 a second of 3 ms steps
                    if (frames + 1) * 50 / 9 > game.steps {
                        continue;
                    }
                    frames += 1;
                    let state = capture(&game, &mut sticky);
                    one(&mut world, &state, &pending);
                    pending.clear();
                    if popped_at.is_none() && state.tyre_inflation.iter().any(|i| *i <= 0.0) {
                        popped_at = Some(frames);
                        println!("drive: a tyre burst after {:.2} s (inflation {:?})", frames as f32 / 60.0, state.tyre_inflation);
                    }
                    if frames % 30 == 0 {
                        println!(
                            "drive: {:5.1} s  {:6.0} rpm  limiter {}  {:6.1} km/h  gear {}  rear wheels {:6.1} rad/s  slip {:5.2}  inflation {:?}  at {:.1}, {:.1}",
                            frames as f32 / 60.0,
                            state.engine_rpm,
                            state.is_engine_limiter_on as u8,
                            state.speed * 3.6,
                            state.gear - 1,
                            state.wheel_angular_speed[2],
                            state.slip_ratio[2],
                            state.tyre_inflation,
                            state.world_matrix[12],
                            state.world_matrix[14]
                        );
                    }
                    let done = match popped_at {
                        Some(at) if after_pop >= 0.0 => frames >= at + (after_pop * 60.0).round() as u64,
                        _ => false,
                    };
                    if done || frames as usize >= total {
                        break;
                    }
                }
                println!("drive: {frames} frames ({:.2} s), {} physics steps", frames as f32 / 60.0, game.steps);
            }
            other => return Err(format!("{other}: steady, event or drive is expected")),
        }
        world.destroy();
        println!("{}: written", out.display());
        Ok(())
    }
}
