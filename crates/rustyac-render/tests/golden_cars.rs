// SPDX-License-Identifier: GPL-3.0-or-later

// the Direct3D 11 renderer against its recordings
#![cfg(not(feature = "backend-wgpu"))]

//! The golden frame of several cars: two Ferrari F2004 and a BMW M3 E30 two and a half seconds
//! after a start at Magione, from the chase camera of the last car (the focused one), with
//! everything a session of several cars adds to the picture: every car's own avatar with its
//! guid, the car node sorter (which draws last the car whose number is the camera's), the
//! blended wheel and pedals of the cars after the first, the levels of detail by distance, the
//! mirror texture drawn from the focused car, `RaceManager::carsToBeLoaded`.
//!
//! The cars' states are three pose files cut out of a recording of the game's own physics
//! (`car_oracle multi --scenario mc_field --track magione --audio-tape`, frame 150;
//! `render_oracle pose-of`). `tools/render_oracle` has shown the command log and the pixels of
//! this frame to be the game's own, byte for byte (`render_oracle compare --track magione
//! --poses … --set focus=2 --view chase --smoke 3 --mirror 512`, docs/port/draw_hear_cars.md);
//! this test keeps the port there without the game's code. Its log is the oracle's but for one
//! number: the back buffer here is a plain texture (bind flags 0x28), the oracle's belongs to
//! a swap chain (0x20).
//!
//! It needs an Assetto Corsa install (for the track, the cars, the shaders and the textures)
//! and `d3dx11_43.dll`; without them it prints NOT TESTED and passes.

use rustyac_physics::track::loader;
use rustyac_physics::vecmath::Mat44f;
use rustyac_render::car::{CarAvatar, CarPhysicsState, SimNodes, ViewState};
use rustyac_render::gpulog;
use rustyac_render::graphics::{Graphics, VideoSettings};
use rustyac_render::kgl::DeviceOptions;
use rustyac_render::mirror::{MirrorScene, MirrorTextureRenderer};
use rustyac_render::model::Kn5Io;
use rustyac_render::scene::{NodeId, Scene};

/// The command log of the frame: its lines, its draw calls, its FNV-1a hash.
const GOLDEN_LINES: u64 = 9186;
const GOLDEN_DRAWS: u64 = 1237;
const GOLDEN_LOG: u64 = 0xb1b3_0e71_1018_8d5c;
/// The 1280 x 720 RGBA picture WARP makes of it.
const GOLDEN_PIXELS: u64 = 0x5c69_c603_ce16_bd10;

/// The cars in the order they are made (guid 0, 1, 2) and the focused one.
const CARS: [&str; 3] = ["ks_ferrari_f2004", "bmw_m3_e30", "ks_ferrari_f2004"];
const FOCUS: usize = 2;
const DT: f32 = 1.0 / 60.0;

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normalized(a: [f32; 3]) -> [f32; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    [a[0] / l, a[1] / l, a[2] / l]
}

/// A camera world matrix that stands at `eye` and looks at `target` (as the oracle builds it).
fn look_from(eye: [f32; 3], target: [f32; 3]) -> Mat44f {
    let z = normalized(sub(eye, target));
    let x = normalized(cross([0.0, 1.0, 0.0], z));
    let y = cross(z, x);
    Mat44f { m: [[x[0], x[1], x[2], 0.0], [y[0], y[1], y[2], 0.0], [z[0], z[1], z[2], 0.0], [eye[0], eye[1], eye[2], 1.0]] }
}

fn path_text(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[test]
fn three_cars_at_magione_are_the_games() {
    let Some(game) = rustyac_content::install::ac_root() else {
        eprintln!("NOT TESTED: no Assetto Corsa install (the golden frame needs its track, shaders and textures)");
        return;
    };
    let folder = game.join("content/tracks/magione");
    if !folder.is_dir() {
        eprintln!("NOT TESTED: {} is missing", folder.display());
        return;
    }
    for name in CARS {
        if !game.join("content/cars").join(name).is_dir() {
            eprintln!("NOT TESTED: the car {name} is missing");
            return;
        }
    }
    if !rustyac_render::texture::d3dx_available() {
        eprintln!("NOT TESTED: d3dx11_43.dll is not installed (the port's own texture reader makes other mip maps)");
        return;
    }
    // the Task 20 profile with smoke at Normal and mirrors of 512
    let video = VideoSettings {
        aa_samples: 1,
        width: 1280,
        height: 720,
        is_fullscreen: false,
        v_sync: false,
        anisotropic: 8,
        aa_quality: 0,
        shadow_map_size: 2048,
        fps_cap_ms: 0.0,
        world_detail: 5,
        pp_hdr_enabled: false,
        pp_fxaa: false,
        ldr_levels: false,
        triple_buffer: false,
        smoke: Some(3),
        mirror_size: 512,
        mirror_smoke: false,
    };
    let mut graphics = match Graphics::new(video, DeviceOptions { warp: true, window: None, log: true }, &game) {
        Ok(graphics) => graphics,
        Err(e) => {
            eprintln!("NOT TESTED: no WARP device ({e})");
            return;
        }
    };
    // the splash screen's frame
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
    let cars_node = scene.node("CARS");
    scene.add_child(unblurred, cars_node);
    let particles = scene.node("PARTICLES_NODE");
    scene.add_child(unblurred, particles);
    let render_finished = scene.node_event("RENDER FINISHED");
    scene.add_child(unblurred, render_finished);

    // TrackAvatar::init3D
    let model = scene.node("TRACK magione");
    let mut io = Kn5Io::new();
    for entry in loader::track_models(&folder, "").expect("the track's models") {
        let filename = path_text(&entry.file);
        io.skin_override_path.push(format!("{}/texture", rustyac_render::model::get_path(&filename)));
        let top = io.load(&mut graphics, &mut scene, &filename, &entry.file).expect("the track's model");
        scene.add_child(model, top);
        scene.nodes[top].matrix = loader::top_node_matrix(&scene.nodes[top].matrix, entry.position, entry.rotation);
    }
    scene.compile(&graphics, model);
    scene.add_child(track_node, model);
    scene.hide_helpers(model);
    scene.show_track_objects(model);
    let folder_text = path_text(&folder);
    let mut dynamic_objects = rustyac_render::crowds::init_dynamic_objects(&mut graphics, &mut scene, &folder, "", model);
    // TrackAvatar::TrackAvatar: DynamicTrackManager, then CameraFacing
    let mut grooves = rustyac_render::crowds::DynamicTrackManager::new(&mut scene, &folder, track_node, None);
    graphics.cube_map_hidden = grooves.meshes();
    let _crowds = rustyac_render::crowds::camera_facing(&mut graphics, &mut scene, &folder, track_node, blurred, 22).expect("the crowds");

    // Sim::createCamera, RaceManager::initLighting, TrackAvatar::TrackAvatar, the weather
    let mut camera = rustyac_render::post::CameraForwardYebis::new(&mut graphics, false).expect("the camera");
    camera.blur_quality = 0.0;
    camera.base.camera.clear_color = [0.3, 0.25, 0.25, 1.0];
    camera.base.camera.max_layer = 5.0;
    camera.base.sky_box = Some(rustyac_render::sky::SkyBox::new(&mut graphics, Some("3_clear")).expect("the sky"));
    graphics.set_sun_angle(-16.0);
    if let Ok(ini) = rustyac_physics::data::ini::IniReader::load(&folder.join("data/lighting.ini")) {
        graphics.lighting.pitch_angle = ini.get_float("LIGHTING", "SUN_PITCH_ANGLE").unwrap_or(0.0);
        graphics.lighting.heading_angle = ini.get_float("LIGHTING", "SUN_HEADING_ANGLE").unwrap_or(0.0);
        graphics.update_lighting_settings();
    }
    if let Some(sky) = &mut camera.base.sky_box {
        sky.update_clouds_generation(&mut graphics, "3_clear");
    }
    graphics.apply_custom_weather("3_clear");
    camera.base.camera.near_plane = 0.05;
    camera.base.camera.far_plane = 40000.0;
    camera.cube_map_renderer.faces_per_frame = 0;
    camera.set_cubemap_size(&graphics, 512);
    // no Documents folder, the oracle's seed for rand()
    graphics.documents_folder = std::path::PathBuf::from("no documents");
    graphics.crt_rand = rustyac_physics::session::MsvcRand(21);

    // Sim::addCar for every car (the first with the mirror of Sim::Sim made after its models,
    // as the oracle's port side has it), then Sim::onPostLoad
    let sim = SimNodes { root, cars: cars_node, skid_marks, particles, car_shadows, before_cars, render_finished };
    let mut mirror: Option<MirrorTextureRenderer> = None;
    let mut cars: Vec<CarAvatar> = Vec::new();
    let mut states: Vec<CarPhysicsState> = Vec::new();
    for (index, name) in CARS.iter().enumerate() {
        let car_folder = game.join("content/cars").join(name);
        let ini = |file: &str| rustyac_physics::data::ini::IniReader::load(&car_folder.join("data").join(file)).ok();
        let steer_lock = ini("car.ini").and_then(|i| i.get_float("CONTROLS", "STEER_LOCK").ok()).unwrap_or(0.0);
        let width = |section: &str| ini("tyres.ini").and_then(|i| i.get_float(section, "WIDTH").ok()).unwrap_or(0.0);
        let tyre_width = [width("FRONT"), width("FRONT"), width("REAR"), width("REAR")];
        // car n of a field takes the skin folder number n by name
        let mut skins: Vec<String> = std::fs::read_dir(car_folder.join("skins")).expect("the skins").filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        skins.sort();
        let skin = skins[index % skins.len()].clone();
        let mut car = CarAvatar::init_3d(&mut graphics, &mut scene, cars_node, &path_text(&car_folder), &car_folder, &skin, Some(steer_lock), index as i32).expect("the car");
        car.cars_to_be_loaded = CARS.len() as i32;
        car.max_gear = ini("drivetrain.ini").and_then(|i| i.get_int("GEARS", "COUNT").ok()).unwrap_or(0);
        if mirror.is_none() {
            mirror = Some(MirrorTextureRenderer::new(&mut graphics, 512, false, None).expect("the mirror"));
        }
        car.init_mirror_materials(&mut graphics, &mut scene, &mirror.as_ref().unwrap().texture).expect("the mirror materials");
        car.init_common_post_physics(&mut graphics, &mut scene, &sim, tyre_width).expect("the objects of the car");
        cars.push(car);
        let pose = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/data/field_magione_car{index}.pose"));
        states.push(CarPhysicsState::from_bytes(&std::fs::read(&pose).expect("the pose")).expect("a pose"));
    }
    let mut mirror = mirror.expect("the mirror");
    let cube_model = rustyac_render::cubemap::load_static_cubemap_model(&mut graphics, &mut scene, Some(folder_text.as_str())).expect("the cube map's model");
    rustyac_render::cubemap::render_static_cubemap(&mut camera, &mut graphics, &mut scene, cube_model);
    for car in &mut cars {
        car.on_post_load(&mut graphics, &mut scene, car_shadows);
    }

    // the chase camera's place behind the focused car
    let w = states[FOCUS].world_matrix.m;
    let at = |ahead: f32, up: f32| [w[3][0] + w[2][0] * ahead, w[3][1] + up, w[3][2] + w[2][2] * ahead];
    let matrix = look_from(at(-6.0, 2.0), at(0.0, 1.0));

    // the first frame goes by unlogged, as in the oracle: the logged one starts from its state
    let mut frame = |graphics: &mut Graphics, scene: &mut Scene, camera: &mut rustyac_render::post::CameraForwardYebis, index: usize, logged: bool| {
        camera.base.camera.fov = 60.0;
        camera.base.camera.matrix = matrix;
        camera.base.camera.near_plane = 1.0;
        camera.base.set_shadow_maps_splits(graphics, 10.0, 50.0, 150.0, 500.0);
        rustyac_render::crowds::update_dynamic_objects(scene, &mut dynamic_objects, DT);
        if let Some(sky) = &mut camera.base.sky_box {
            sky.update_clouds_animation(DT);
        }
        // Sim::update: CarNodeSorter::sort (the chase camera is camera number 1)
        let list: Vec<(NodeId, i32)> = cars.iter().map(|car| (car.car_node, car.guid)).collect();
        scene.sort_cars(cars_node, &list, 1);
        // PhysicsAvatar::update, then the cars' updates in the order they were made, then every
        // handler of evOnPostUpdate
        let mut kept = Vec::new();
        for (car, state) in cars.iter_mut().zip(&states) {
            let state = car.set_new_physics_state(state, DT);
            car.game_time_ms = index as f64 * (1000.0 / 60.0);
            car.view = ViewState { camera_mode: 2, drivable_mode: 0, focused_car_index: FOCUS as i32, camera_position: [matrix.m[3][0], matrix.m[3][1], matrix.m[3][2]], camera_matrix: matrix, use_pro_view: false };
            car.cars_count = CARS.len() as i32;
            car.update(graphics, scene, &state, DT);
            kept.push(state);
        }
        for (car, state) in cars.iter_mut().zip(&kept) {
            car.post_update(scene, state, DT, &matrix, 60.0, false);
        }
        let laps: Vec<u32> = states.iter().map(|s| s.lap_count).collect();
        grooves.update(scene, DT, None, &laps);
        if logged {
            gpulog::begin_capture("magione_chase_three_cars");
        }
        graphics.begin_scene();
        scene.traverse(root);
        // Sim::renderScene: the mirror of the focused car (the chase camera: only until the
        // texture has been drawn once)
        if mirror.wants_render(2, 0, false) {
            let focused = &cars[FOCUS];
            let nodes = focused.visibility_nodes();
            let s = MirrorScene { root, particles, before_cars, ideal_line: None, body_transform: focused.body_transform, car_nodes: &nodes, mirror_position: focused.mirror_position };
            mirror.render(graphics, scene, camera.base.sky_box.as_mut(), &s);
        }
        camera.render(graphics, scene, Some(blurred), unblurred, root, DT).expect("the frame");
        graphics.set_screen_space_mode();
        // ksgui::GUI::render -> doRender2D: the states of the 2D pass
        graphics.reset_render_states();
        graphics.set_depth_mode(rustyac_render::graphics::DEPTH_OFF);
        graphics.set_blend_mode(rustyac_render::graphics::BLEND_ALPHA);
    };
    frame(&mut graphics, &mut scene, &mut camera, 0, false);
    graphics.end_scene();
    frame(&mut graphics, &mut scene, &mut camera, 1, true);
    let capture = gpulog::end_capture();
    let (width, height, pixels) = graphics.kgl.read_screen().expect("the picture");
    graphics.end_scene();

    if let Some(file) = std::env::var_os("RUSTYAC_GOLDEN_DUMP") {
        std::fs::write(file, &capture.text).expect("the dump");
    }
    let log = gpulog::hash_bytes(&capture.text);
    let picture = gpulog::hash_bytes(&pixels);
    println!("golden frame of three cars: {} lines, {} draws, log {log:#018x}, {width} x {height} pixels {picture:#018x}", capture.lines, capture.draws);
    assert_eq!((capture.lines, capture.draws), (GOLDEN_LINES, GOLDEN_DRAWS), "the frame's calls");
    assert_eq!(log, GOLDEN_LOG, "the frame's command log");
    assert_eq!((width, height), (1280, 720));
    if picture != GOLDEN_PIXELS {
        // WARP is part of Windows: another build of it may round a pixel another way
        eprintln!("NOTE: the picture's hash is {picture:#018x}, not {GOLDEN_PIXELS:#018x}: another WARP than the one the golden frame was made on?");
    }
}
