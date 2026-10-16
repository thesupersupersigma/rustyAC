// SPDX-License-Identifier: GPL-3.0-or-later

//! The port's side: the same frame with `rustyac-render`, on the same kind of device (WARP,
//! a swap chain on a window that is never shown), through the same command log.

use rustyac_physics::vecmath::Mat44f;
use rustyac_render::graphics::{Graphics, VideoSettings};
use rustyac_render::kgl::DeviceOptions;
use rustyac_render::model::Kn5Io;
use rustyac_render::scene::Scene;

use crate::ac::{Captured, Rendered};
use crate::frames::Frame;
use crate::Args;

fn mat(flat: &[f32; 16]) -> Mat44f {
    let mut m = Mat44f::default();
    for r in 0..4 {
        for c in 0..4 {
            m.m[r][c] = flat[r * 4 + c];
        }
    }
    m
}

/// What a frame hands a car's avatar beside its physics state (the game's side: the fields of
/// the fake `Sim`, `RaceManager` and physics car that `update_car` writes).
fn feed(car: &mut rustyac_render::car::CarAvatar, spec: &crate::frames::CarSpec, step: &crate::frames::Step, index: usize, count: usize) {
    car.max_gear = spec.max_gear;
    car.game_time_ms = crate::frames::game_time_ms(index);
    car.view = rustyac_render::car::ViewState {
        camera_mode: step.camera.mode,
        drivable_mode: step.camera.drivable_mode,
        focused_car_index: step.extra.focus,
        camera_position: [step.camera.matrix[12], step.camera.matrix[13], step.camera.matrix[14]],
        camera_matrix: rustyac_physics::vecmath::Mat44f { m: std::array::from_fn(|r| std::array::from_fn(|c| step.camera.matrix[r * 4 + c])) },
        use_pro_view: false,
    };
    car.session_type = step.extra.session_type;
    car.real_time_position = step.extra.position;
    car.leaderboard_position = -1;
    car.tc_level = step.extra.tc;
    car.abs_level = step.extra.abs;
    car.ambient_temperature = step.extra.air;
    car.replay_mode = step.extra.replay;
    car.replay_scale = step.extra.replay_scale;
    car.pause_menu = step.extra.pause;
    car.wing_angles = step.extra.wing.map(|a| vec![a; 8]).unwrap_or_default();
    car.kers_max_j = step.extra.kers_max;
    car.ers_max_j = step.extra.ers_max;
    car.has_kers = step.extra.kers_max > 0.0;
    car.max_fuel = step.extra.max_fuel;
    car.session_laps = step.extra.session_laps;
    // (the game's side: the Sim's list of cars has the one car only with mirrors; with several
    // cars it has them all)
    car.cars_count = if count > 1 {
        count as i32
    } else if step.extra.cars > 0 {
        step.extra.cars
    } else if crate::root::profile().mirror_size != 0 {
        1
    } else {
        0
    };
    car.km_per_liter = step.extra.fuel_use().2;
    if step.extra.drive {
        car.drivetrain = Some((step.extra.torque, step.extra.ratio));
        car.has_energy_store = step.extra.kers_max > 0.0 || step.extra.ers_max > 0.0;
    }
}

pub fn render(args: &Args, frame: &Frame) -> Result<(Vec<u8>, Rendered), String> {
    let window = unsafe { crate::ac::hidden_window(args.width, args.height)? };
    let video = VideoSettings {
        aa_samples: crate::root::profile().aa_samples,
        width: args.width as i32,
        height: args.height as i32,
        is_fullscreen: false,
        v_sync: false,
        anisotropic: crate::root::profile().anisotropic,
        aa_quality: 0,
        shadow_map_size: crate::root::profile().shadow_map_size,
        fps_cap_ms: 0.0,
        world_detail: crate::root::profile().world_detail,
        smoke: Some(crate::root::profile().smoke),
        mirror_size: crate::root::profile().mirror_size,
        mirror_smoke: crate::root::profile().mirror_smoke,
        pp_hdr_enabled: crate::root::profile().post,
        pp_fxaa: crate::root::profile().fxaa,
        ldr_levels: crate::root::profile().ldr_levels,
        triple_buffer: false,
    };
    rustyac_render::gpulog::capture_from_install(true);
    let mut graphics = Graphics::new(video, DeviceOptions { warp: !crate::ac::hardware(), window: Some(window), log: !crate::timing() }, &args.game)?;
    let init = rustyac_render::gpulog::end_capture();

    // the splash screen's frame (see the game's side)
    graphics.begin_scene();
    graphics.set_screen_space_mode();
    graphics.end_scene();

    // Sim::initSceneGraph
    let mut scene = Scene::new();
    let root = scene.node("ROOT");
    let blurred = scene.node("BLURRED");
    let unblurred = scene.node("UNBLURRED");
    scene.add_child(root, blurred);
    scene.add_child(root, unblurred);
    let track_node = scene.node("TRACK");
    scene.add_child(blurred, track_node);
    let skid_marks = scene.node("SKIDMARKS");
    scene.add_child(blurred, skid_marks);
    let car_shadows = scene.node_event("CAR_SHADOWS");
    scene.add_child(blurred, car_shadows);
    let before_cars = scene.node_event("BEFORE_CARS_NODE");
    scene.add_child(unblurred, before_cars);
    let cars = scene.node("CARS");
    scene.add_child(unblurred, cars);
    let particles = scene.node("PARTICLES_NODE");
    scene.add_child(unblurred, particles);
    let render_finished = scene.node_event("RENDER FINISHED");
    scene.add_child(unblurred, render_finished);

    // TrackAvatar::init3D
    let mut dynamic_objects = Vec::new();
    let mut track_model = None;
    if let Some(track) = &frame.track {
        let model = scene.node(&format!("TRACK {}", track.name));
        track_model = Some(model);
        let mut io = Kn5Io::new();
        for entry in &track.models {
            io.skin_override_path.push(format!("{}/texture", rustyac_render::model::get_path(&entry.filename)));
            let top = io.load(&mut graphics, &mut scene, &entry.filename, std::path::Path::new(&entry.filename))?;
            scene.add_child(model, top);
            let mut flat = [0.0f32; 16];
            for r in 0..4 {
                for c in 0..4 {
                    flat[r * 4 + c] = scene.nodes[top].matrix.m[r][c];
                }
            }
            scene.nodes[top].matrix = mat(&crate::frames::placed(&flat, entry));
        }
        scene.compile(&graphics, model);
        scene.add_child(track_node, model);
        scene.hide_helpers(model);
        scene.show_track_objects(model);
        // TrackAvatar::initDynamicObjects
        dynamic_objects = rustyac_render::crowds::init_dynamic_objects(&mut graphics, &mut scene, std::path::Path::new(&track.folder), &track.layout, model);
    }
    // TrackAvatar::TrackAvatar: the ideal line
    let mut ideal_line = None;
    if let Some(file) = &frame.ideal_line {
        let bytes = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
        let spline = rustyac_physics::track::spline::AiSpline::parse_fast(&bytes)?;
        let line = rustyac_render::ideal_line::build(&mut graphics, &mut scene, &spline, &|from| crate::frames::ray_cast(from, &[0.0, -1.0, 0.0], 100.0).map(|hit| hit.0))?;
        scene.compile(&graphics, line);
        scene.add_child(track_node, line);
        ideal_line = Some(line);
    }
    // TrackAvatar::TrackAvatar: DynamicTrackManager, then CameraFacing
    let mut grooves = None;
    let mut _crowds = Vec::new();
    if let Some(track) = &frame.track {
        let data = std::path::Path::new(&track.data_folder);
        let manager = rustyac_render::crowds::DynamicTrackManager::new(&mut scene, data, track_node, None);
        graphics.cube_map_hidden = manager.meshes();
        grooves = Some(manager);
        _crowds = rustyac_render::crowds::camera_facing(&mut graphics, &mut scene, data, track_node, blurred, crate::frames::RAND_SEED_TRACK)?;
    }

    // Sim::Sim: StartingLights (two numbers of rand()), StartingTrackSemaphore
    let mut start_lights = None;
    if let (Some(track), true) = (&frame.track, frame.start_lights) {
        let data = std::path::Path::new(&track.data_folder);
        graphics.crt_rand = rustyac_physics::session::MsvcRand(crate::frames::RAND_SEED_TRACK);
        let lights = rustyac_render::start_lights::StartingLights::new(&mut graphics, &mut scene, track_node, data)?;
        let semaphore = rustyac_render::start_lights::StartingTrackSemaphore::new(&mut graphics, &mut scene, track_node, data)?;
        start_lights = Some((lights, semaphore));
    }
    // TimeAttack::TimeAttack, the gates' part
    let mut gates = None;
    if let (true, Some(track)) = (frame.time_attack, &frame.track) {
        let overlays = crate::frames::time_attack_overlays(&track.data_folder);
        let handler = std::rc::Rc::new(std::cell::RefCell::new(rustyac_render::time_attack::TimeAttackGates::new(&mut graphics, overlays)));
        scene.add_event_handler(before_cars, handler.clone());
        gates = Some(handler);
    }
    // Sim::createCamera, Sim::Sim, Sim::initCubemaps
    // Sim::createCamera 0x1401982e0: the plain camera, or the post-processing one (which is the
    // plain one when post-processing is off); blur only with post-processing
    let profile = crate::root::profile();
    let mut camera = rustyac_render::post::CameraForwardYebis::new(&mut graphics, profile.post && profile.motion_blur > 0)?;
    // Sim::Sim 0x140192070 (no VR): the solve's quality is twice the setting
    camera.blur_quality = profile.motion_blur as f32 * 2.0;
    camera.base.camera.clear_color = [0.3, 0.25, 0.25, 1.0];
    camera.base.camera.max_layer = crate::root::profile().world_detail as f32;
    // (the oracle's race.ini has [WEATHER]: the clouds are made a first time in the constructor)
    camera.base.sky_box = Some(rustyac_render::sky::SkyBox::new(&mut graphics, Some(&frame.weather))?);
    // RaceManager::initLighting, TrackAvatar::TrackAvatar, Sim::applyCustomWeather
    graphics.set_sun_angle(frame.sun_angle);
    if let Some(track) = &frame.track {
        if let (Some(pitch), Some(heading)) = (track.sun_pitch, track.sun_heading) {
            graphics.lighting.pitch_angle = pitch;
            graphics.lighting.heading_angle = heading;
            graphics.update_lighting_settings();
        }
    }
    if !frame.weather.is_empty() {
        if let Some(sky) = &mut camera.base.sky_box {
            sky.update_clouds_generation(&mut graphics, &frame.weather);
        }
        graphics.apply_custom_weather(&frame.weather);
    }
    camera.base.camera.near_plane = 0.05;
    camera.base.camera.far_plane = 40000.0;
    camera.cube_map_renderer.faces_per_frame = crate::root::profile().cubemap_faces_per_frame;
    if crate::root::profile().cubemap_far_plane != 0.0 {
        camera.cube_map_renderer.set_camera_near_far_planes(f32::from_bits(0x3c23_d70a), crate::root::profile().cubemap_far_plane);
    }
    camera.set_cubemap_size(&graphics, crate::root::profile().cubemap_size);
    graphics.documents_folder = crate::root::documents().join("Assetto Corsa");
    // (the game's side seeds rand() again where the car is loaded: only with a car)
    if frame.car.is_some() {
        graphics.crt_rand = rustyac_physics::session::MsvcRand(crate::frames::RAND_SEED);
    }
    // Sim::addCar: CarAvatar::init3D
    let count = 1 + frame.others.len();
    let mut car = match &frame.car {
        Some(spec) => Some(rustyac_render::car::CarAvatar::init_3d(&mut graphics, &mut scene, cars, &spec.folder, std::path::Path::new(&spec.folder), &spec.skin, Some(spec.steer_lock), 0)?),
        None => None,
    };
    if let (Some(car), true) = (&mut car, count > 1) {
        car.cars_to_be_loaded = frame.cars_to_be_loaded;
    }

    // Sim::Sim: TrackAvatar::initPitCrew, then the pit box marker of PitStop::PitStop
    let mut lollipops = Vec::new();
    let mut pit_marker = None;
    if let (true, Some(track), Some(model), Some(spec)) = (frame.pit_crew, &frame.track, track_model, &frame.car) {
        let pits = crate::frames::pit_matrices();
        let cars = [rustyac_render::pit_crew::CrewCar { folder_text: spec.folder.clone(), folder: std::path::PathBuf::from(&spec.folder), skin: spec.skin.clone(), guid: 0 }];
        lollipops = rustyac_render::pit_crew::init_pit_crew(&mut graphics, &mut scene, model, std::path::Path::new(&track.data_folder), &pits, &cars, &|from, length| crate::frames::ray_cast(from, &[0.0, -1.0, 0.0], length).map(|hit| hit.0))?;
        if let Some(pit) = pits.first() {
            pit_marker = Some(rustyac_render::pit_crew::PitIndicator::new(&mut graphics, &mut scene, track_node, pit, true)?);
        }
    }
    // Sim::Sim: the mirror texture and the virtual mirror, when video.ini asks for mirrors
    let profile = crate::root::profile();
    let mut mirror = match profile.mirror_size {
        0 => None,
        size => Some(rustyac_render::mirror::MirrorTextureRenderer::new(&mut graphics, size, profile.mirror_smoke, (profile.mirror_hq > 0).then_some(profile.mirror_hq))?),
    };
    let mut virtual_mirror = mirror.as_ref().map(|_| rustyac_render::mirror::VirtualMirrorRenderer::new(&graphics, profile.virtual_mirror));
    if let (Some(car), Some(mirror)) = (&mut car, &mirror) {
        car.init_mirror_materials(&mut graphics, &mut scene, &mirror.texture)?;
    }
    // Sim::onPostLoad: CarAvatar::onPostLoad
    if let Some(car) = &mut car {
        let sim = rustyac_render::car::SimNodes { root, cars, skid_marks, particles, car_shadows, before_cars, render_finished };
        car.init_common_post_physics(&mut graphics, &mut scene, &sim, frame.car.as_ref().map(|c| c.tyre_width).unwrap_or([0.0; 4]))?;
    }
    // RaceManager::initGhostcar: the ghost car of the player's car
    let mut ghost = None;
    let mut ghost_recorder = frame.ghost.clone();
    if let (Some(_), Some(spec)) = (&ghost_recorder, &frame.car) {
        ghost = Some(rustyac_render::ghost_car::GhostCar::new(&mut graphics, &mut scene, before_cars, &spec.folder, std::path::Path::new(&spec.folder))?);
    }
    // PitStop::PitStop: the tyre changers of the player's car
    let mut tyre_crew = None;
    let mut crew_began = false;
    if let (true, Some(spec), Some(car)) = (frame.tyre_crew, &frame.car, &car) {
        let crew_car = rustyac_render::pit_crew::CrewCar { folder_text: spec.folder.clone(), folder: std::path::PathBuf::from(&spec.folder), skin: spec.skin.clone(), guid: 0 };
        tyre_crew = Some(rustyac_render::tyre_crew::PitCrew::new(&mut graphics, &mut scene, blurred, &crew_car, car.car_node, crate::frames::tyre_change_time_sec(spec))?);
    }
    // Sim::addCar for the cars after the first: guid 1, 2, …
    let mut others: Vec<rustyac_render::car::CarAvatar> = Vec::new();
    for (i, spec) in frame.others.iter().enumerate() {
        let mut other = rustyac_render::car::CarAvatar::init_3d(&mut graphics, &mut scene, cars, &spec.folder, std::path::Path::new(&spec.folder), &spec.skin, Some(spec.steer_lock), i as i32 + 1)?;
        other.cars_to_be_loaded = frame.cars_to_be_loaded;
        if let Some(mirror) = &mirror {
            other.init_mirror_materials(&mut graphics, &mut scene, &mirror.texture)?;
        }
        let sim = rustyac_render::car::SimNodes { root, cars, skid_marks, particles, car_shadows, before_cars, render_finished };
        other.init_common_post_physics(&mut graphics, &mut scene, &sim, spec.tyre_width)?;
        others.push(other);
    }
    let cube_model = rustyac_render::cubemap::load_static_cubemap_model(&mut graphics, &mut scene, frame.track.as_ref().map(|t| t.folder.as_str()))?;
    rustyac_render::gpulog::begin_capture("static cube map");
    rustyac_render::cubemap::render_static_cubemap(&mut camera, &mut graphics, &mut scene, cube_model);
    let mut cube_log = rustyac_render::gpulog::end_capture().text;
    // Sim::onPostLoad, after the cube map: CarAvatar::onPostLoad (the flat ground shadows; a car
    // without body_shadow.png has its five pictures drawn here)
    if let Some(car) = &mut car {
        rustyac_render::gpulog::begin_capture("car shadows");
        car.on_post_load(&mut graphics, &mut scene, car_shadows);
        cube_log.extend_from_slice(&rustyac_render::gpulog::end_capture().text);
        if let Some(pictures) = car.fake_shadow.as_ref().and_then(|s| s.borrow().generated.clone()) {
            for (i, picture) in pictures.iter().enumerate() {
                cube_log.extend_from_slice(format!("generated shadow {i}: {} bytes {:016x}
", picture.len(), crate::ac::hash(picture)).as_bytes());
            }
        }
    }
    for other in &mut others {
        rustyac_render::gpulog::begin_capture("car shadows");
        other.on_post_load(&mut graphics, &mut scene, car_shadows);
        cube_log.extend_from_slice(&rustyac_render::gpulog::end_capture().text);
        if let Some(pictures) = other.fake_shadow.as_ref().and_then(|s| s.borrow().generated.clone()) {
            for (i, picture) in pictures.iter().enumerate() {
                cube_log.extend_from_slice(format!("generated shadow {i}: {} bytes {:016x}\n", picture.len(), crate::ac::hash(picture)).as_bytes());
            }
        }
    }

    let mut rendered = Rendered { cube_log, frames: Vec::new(), width: 0, height: 0 };
    let (mut last_pause, mut last_status) = (false, -1);
    let (mut lights_pause, mut lights_status) = (false, -1);
    let mut told_new_session = 0;
    // ACCameraManager::fadeMode: fadeTime and fadeTotalTime (nothing fades before --fade-at)
    let (mut fade_time, mut fade_total) = (0.0f32, 0.0f32);
    for (index, step) in frame.steps.iter().enumerate() {
        if frame.fade_at == Some(index) {
            (fade_time, fade_total) = (0.0, crate::frames::camera_fade_time());
        }
        // Sim::evOnNewSession: the track's handler, then the car's objects'
        if step.extra.new_session != told_new_session {
            told_new_session = step.extra.new_session;
            rustyac_render::crowds::on_new_session(&mut graphics.crt_rand, &mut dynamic_objects, crate::frames::game_time_ms(index), 0.0);
            if let Some(car) = &mut car {
                car.on_new_session(&mut scene);
            }
            for other in &mut others {
                other.on_new_session(&mut scene);
            }
        }
        let (_, session_started, tts) = crate::frames::session_start(index, step.extra.tts);
        if let Some((lights, semaphore)) = &mut start_lights {
            if step.extra.pause != lights_pause {
                lights_pause = step.extra.pause;
                lights.on_pause_mode_changed(lights_pause);
            }
            if step.extra.replay_status != lights_status {
                lights_status = step.extra.replay_status;
                lights.on_replay_status_changed(lights_status);
            }
            // Game::update: StartingTrackSemaphore::update
            semaphore.update(&mut scene, tts);
        }
        camera.base.camera.fov = step.camera.fov;
        camera.base.camera.matrix = mat(&step.camera.matrix);
        camera.base.camera.near_plane = step.camera.near;
        if let Some(far) = step.camera.far {
            camera.base.camera.far_plane = far;
        }
        let s = step.camera.splits;
        camera.base.set_shadow_maps_splits(&mut graphics, s[0], s[1], s[2], s[3]);
        if let Some(gates) = &gates {
            gates.borrow_mut().in_replay = step.extra.replay;
        }
        // DrivingAssistManager::update
        if let Some(line) = ideal_line {
            scene.nodes[line].is_active = step.extra.ideal;
        }
        // TrackAvatar::update
        rustyac_render::crowds::update_dynamic_objects(&mut scene, &mut dynamic_objects, crate::frames::DT);
        // SunAnimator::update, the clouds' part
        if let Some(sky) = &mut camera.base.sky_box {
            sky.update_clouds_animation(crate::frames::DT * args.time_mult);
        }
        // Game::update: the track's children first: LollipopCrew::update
        for crew in &mut lollipops {
            crew.update(&mut scene, crate::frames::DT, Some(step.extra.pit_state), false);
        }
        // Game::update: the car's objects, then the handlers of evOnPostUpdate
        if let (Some(car), Some(spec)) = (&mut car, &frame.car) {
            // Sim::update: CarNodeSorter::sort
            // RENDER_ORACLE_FAULT=nosort | nosmooth | focus0: the port leaves out the sorter, the
            // blend of the other cars' controls, or takes car 0 for the focused one: to show
            // that the comparison notices
            let fault = std::env::var("RENDER_ORACLE_FAULT").unwrap_or_default();
            if count > 1 && fault != "nosort" {
                let list: Vec<(rustyac_render::scene::NodeId, i32)> = std::iter::once((car.car_node, 0)).chain(others.iter().map(|o| (o.car_node, o.guid))).collect();
                scene.sort_cars(cars, &list, crate::frames::global_camera_index(&step.camera));
            }
            // Sim::setPauseMode and the replay's transport: the events (every car's handlers, in
            // the order the cars were made), before the updates
            if step.extra.pause != last_pause {
                last_pause = step.extra.pause;
                car.on_pause_mode_changed(last_pause);
                for other in &mut others {
                    other.on_pause_mode_changed(last_pause);
                }
            }
            if step.extra.replay_status != last_status {
                last_status = step.extra.replay_status;
                car.on_replay_status_changed(&mut scene, last_status, step.extra.replay_scale, 2.0);
                if let Some(crew) = &mut tyre_crew {
                    crew.on_replay_status_changed(&mut scene, last_status);
                }
                for other in &mut others {
                    other.on_replay_status_changed(&mut scene, last_status, step.extra.replay_scale, 2.0);
                }
            }
            // the cars' updates in the order they were made, then every handler of evOnPostUpdate
            feed(car, spec, step, index, count);
            car.update(&mut graphics, &mut scene, &step.state, crate::frames::DT);
            // (the car's children after it: GhostCar::update)
            if let (Some(ghost), Some(recorder)) = (&mut ghost, &mut ghost_recorder) {
                let b = scene.nodes[car.body_transform].matrix.m[3];
                ghost.update(&mut scene, recorder, step.state.lap_time, &|m| car.make_body_matrix(m), &[b[0], b[1], b[2]], false);
                if std::env::var_os("RENDER_ORACLE_GHOST").is_some() {
                    let g = scene.nodes[ghost.body_transform].matrix.m[3];
                    println!("ghost: frame {} of {} shows {} at {:?}, the car at {:?}", recorder.current_frame, recorder.playing_buffer.len(), scene.nodes[ghost.car_node].is_active, &g[..3], &b[..3]);
                }
            }
            // (PhysicsAvatar::update: CarAvatar::setNewPhysicsState of every car, before the cars update)
            let other_states: Vec<rustyac_render::car::CarPhysicsState> = others.iter_mut().zip(&step.others).map(|(other, state)| if fault == "nosmooth" { *state } else { other.set_new_physics_state(state, crate::frames::DT) }).collect();
            for ((other, spec), state) in others.iter_mut().zip(&frame.others).zip(&other_states) {
                feed(other, spec, step, index, count);
                other.update(&mut graphics, &mut scene, state, crate::frames::DT);
            }
            car.post_update(&mut scene, &step.state, crate::frames::DT, &mat(&step.camera.matrix), step.camera.fov, false);
            for (other, state) in others.iter_mut().zip(&other_states) {
                other.post_update(&mut scene, state, crate::frames::DT, &mat(&step.camera.matrix), step.camera.fov, false);
            }
        }
        // the pit stop's handler of evOnPostUpdate: PitCrew::pitCrewUpdate
        if let Some(crew) = &mut tyre_crew {
            let t = step.extra.crew;
            let inputs = rustyac_render::tyre_crew::CrewInputs {
                replay: false,
                pause_menu: step.extra.pause,
                session_type: step.extra.session_type,
                pit: rustyac_render::tyre_crew::CrewPitStop { state: step.extra.pit_state, pit_timer: t, max_pit_timer: t, tyre_time: t },
                tyre_matrix: step.state.tyre_matrix,
                tyre_contact_point: step.state.tyre_contact_point,
                focused: true,
            };
            let rand = &mut graphics.crt_rand;
            if t != 0.0 && !crew_began {
                crew_began = true;
                crew.on_pit_stop_begin(&mut scene, &mut QuietCrewHost, &mut || rand.next(), &inputs, &rustyac_render::tyre_crew::CrewStart { tyre_change_time: t, total_time: t, force_change_camera: false });
            }
            crew.pit_crew_update(&mut scene, &mut QuietCrewHost, crate::frames::DT, &inputs);
        }
        // PitStop::update (after the cars): the marker
        if let (Some(marker), Some(car)) = (&mut pit_marker, &car) {
            let b = scene.nodes[car.body_transform].matrix.m[3];
            let inputs = crate::frames::pit_inputs(step, [b[0], b[1], b[2]]);
            let rand = &mut graphics.crt_rand;
            marker.update(&mut scene, &mut || {
                rand.next();
            }, &inputs);
        }
        // DynamicTrackManager::update
        if let Some(grooves) = &mut grooves {
            let laps: Vec<u32> = car.iter().map(|_| step.state.lap_count).chain(step.others.iter().map(|s| s.lap_count)).collect();
            grooves.update(&mut scene, crate::frames::DT, None, &laps);
        }

        if index >= frame.capture && !crate::timing() {
            rustyac_render::gpulog::begin_capture(&frame.name);
        }
        let started = std::time::Instant::now();
        graphics.begin_scene();
        scene.traverse(root);
        // Sim::renderScene: the mirror before the picture
        // (MirrorTextureRenderer::render: the focused car's rear view)
        let focused = match if std::env::var("RENDER_ORACLE_FAULT").is_ok_and(|f| f == "focus0") { 0 } else { step.extra.focus } {
            0 => car.as_ref(),
            n => others.get(n as usize - 1),
        };
        if let (Some(mirror), Some(car)) = (&mut mirror, focused) {
            let active = virtual_mirror.as_ref().is_some_and(|v| v.active);
            if mirror.wants_render(step.camera.mode, step.camera.drivable_mode, active) {
                let nodes = car.visibility_nodes();
                let s = rustyac_render::mirror::MirrorScene { root, particles, before_cars, ideal_line, body_transform: car.body_transform, car_nodes: &nodes, mirror_position: car.mirror_position };
                mirror.render(&mut graphics, &mut scene, camera.base.sky_box.as_mut(), &s);
            }
        }
        camera.render(&mut graphics, &mut scene, Some(blurred), unblurred, root, 1.0 / 60.0)?;
        graphics.set_screen_space_mode();
        // Game::evOnPreGUI
        if let (Some(mirror), Some(v)) = (&mirror, &mut virtual_mirror) {
            v.render(&mut graphics, &mirror.texture, step.camera.mode);
        }
        // ksgui::GUI::render -> doRender2D 0x1402539d0 (no form shows): the states of the 2D pass
        graphics.reset_render_states();
        graphics.set_depth_mode(rustyac_render::graphics::DEPTH_OFF);
        graphics.set_blend_mode(rustyac_render::graphics::BLEND_ALPHA);
        // Game::renderHUD: TimeAttack::renderGuiStatus, the gates' flags
        if let Some(gates) = &gates {
            let mut gates = gates.borrow_mut();
            let count = gates.overlays.len();
            gates.set_active(step.extra.gate >= 0, step.extra.gate.max(0) as usize, count);
        }
        // Game::renderHUD: PitStop::renderHUD, the marker's part
        if let (Some(marker), Some(car)) = (&mut pit_marker, &car) {
            let b = scene.nodes[car.body_transform].matrix.m[3];
            marker.render_hud(&mut scene, &crate::frames::pit_inputs(step, [b[0], b[1], b[2]]));
        }
        // Game::renderHUD, after the scene: StartingLights::renderHUD
        if let Some((lights, _)) = &mut start_lights {
            if lights.is_active && lights.render_hud(&mut scene, step.extra.pause, session_started, step.extra.session_type == 3, tts) {
                lights.render_lamps(&mut graphics);
            }
        }
        // Game::evOnPostGui: the camera manager's fade
        if frame.fade_at.is_some() && !(fade_time >= fade_total * 2.0) {
            rustyac_render::gl::fade_overlay(&mut graphics, fade_time, fade_total);
            fade_time += crate::frames::DT;
        }
        if index >= frame.capture {
            let (text, draws) = if crate::timing() {
                (Vec::new(), 0)
            } else {
                let capture = rustyac_render::gpulog::end_capture();
                (capture.text, capture.draws)
            };
            let (width, height, pixels) = graphics.kgl.read_screen()?;
            crate::FRAME_MS.lock().unwrap().push(started.elapsed().as_secs_f64() * 1000.0);
            rendered.width = width;
            rendered.height = height;
            rendered.frames.push(Captured::new(index, text, draws, pixels, !frame.sequence || args.dump.contains(&index)));
        }
        graphics.end_scene();
    }
    Ok((init.text, rendered))
}

/// The camera and the sound of the tyre changers in a picture's proof: the camera stays, every
/// wheel gun "plays" already (the game's side has `AudioEvent` patched the same way).
struct QuietCrewHost;

impl rustyac_render::tyre_crew::CrewHost for QuietCrewHost {
    fn camera_mode(&mut self) -> i32 {
        0
    }
    fn set_camera_mode(&mut self, _mode: i32, _a: bool, _b: bool) {}
    fn force_theta_phi(&mut self, _theta: f32, _phi: f32, _radius: f32) {}
    fn lock_camera(&mut self, _locked: bool) {}
    fn fade_in(&mut self, _fade_total_time: f32) {}
    fn event_is_playing(&mut self, _wheel: usize, _screw: bool) -> bool {
        true
    }
    fn event_start(&mut self, _wheel: usize, _screw: bool, _matrix: &rustyac_physics::vecmath::Mat44f) {}
    fn event_stop(&mut self, _wheel: usize, _screw: bool) {}
}
