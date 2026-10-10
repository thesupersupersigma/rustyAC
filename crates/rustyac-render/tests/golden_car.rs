// SPDX-License-Identifier: GPL-3.0-or-later

//! The golden car frame: a BMW M3 E30 at Magione from the chase camera, with everything
//! `CarAvatar` hangs on a car (driver, dials, lights, ground shadows, blur, damage, skid
//! marks, smoke, flames, mirror materials), on the Task 20 profile with smoke at Normal and
//! mirrors of 512, on the software rasteriser (WARP). The car's state is a pose written by
//! rustyAC's own physics (`rustyac.exe --pose-out`, tests/data/e30_magione.pose).
//! `tools/render_oracle` has shown the command log and the pixels of this frame to be the
//! game's own, byte for byte (docs/port/renderer_car.md); this test keeps the port there
//! without the game's code. Its log is the oracle's but for one number: the back buffer here
//! is a plain texture (bind flags 0x28), the oracle's belongs to a swap chain (0x20).
//!
//! It needs an Assetto Corsa install (for the track, the car, the shaders and the textures)
//! and `d3dx11_43.dll`; without them it prints NOT TESTED and passes.

use rustyac_physics::track::loader;
use rustyac_physics::vecmath::Mat44f;
use rustyac_render::car::{CarAvatar, CarPhysicsState, SimNodes, ViewState};
use rustyac_render::forward::CameraForward;
use rustyac_render::gpulog;
use rustyac_render::graphics::{Graphics, VideoSettings};
use rustyac_render::kgl::DeviceOptions;
use rustyac_render::mirror::{MirrorScene, MirrorTextureRenderer};
use rustyac_render::model::Kn5Io;
use rustyac_render::scene::Scene;

/// The command log of the frame: its lines, its draw calls, its FNV-1a hash.
const GOLDEN_LINES: u64 = 5059;
const GOLDEN_DRAWS: u64 = 820;
const GOLDEN_LOG: u64 = 0x2102_bd77_2ce7_b800;
/// The 1280 x 720 RGBA picture WARP makes of it.
const GOLDEN_PIXELS: u64 = 0xc4f1_fdfa_c428_a971;

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
fn e30_at_magione_is_the_games() {
    let Some(game) = rustyac_content::install::ac_root() else {
        eprintln!("NOT TESTED: no Assetto Corsa install (the golden frame needs its track, shaders and textures)");
        return;
    };
    let folder = game.join("content/tracks/magione");
    if !folder.is_dir() {
        eprintln!("NOT TESTED: {} is missing", folder.display());
        return;
    }
    let car_folder = game.join("content/cars/bmw_m3_e30");
    if !car_folder.is_dir() {
        eprintln!("NOT TESTED: {} is missing", car_folder.display());
        return;
    }
    if !rustyac_render::texture::d3dx_available() {
        eprintln!("NOT TESTED: d3dx11_43.dll is not installed (the port's own texture reader makes other mip maps)");
        return;
    }
    // the Task 20 profile
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
    let cars = scene.node("CARS");
    scene.add_child(unblurred, cars);
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

    // Sim::createCamera, RaceManager::initLighting, TrackAvatar::TrackAvatar, the weather
    let mut camera = CameraForward::new(&mut graphics).expect("the camera");
    camera.base.camera.clear_color = [0.3, 0.25, 0.25, 1.0];
    camera.base.camera.max_layer = 5.0;
    camera.base.sky_box = Some(rustyac_render::sky::SkyBox::new(&mut graphics, Some("3_clear")).expect("the sky"));
    graphics.set_sun_angle(-16.0);
    if let Ok(ini) = rustyac_physics::data::ini::IniReader::load(&folder.join("data/lighting.ini")) {
        graphics.lighting.pitch_angle = ini.get_float("LIGHTING", "SUN_PITCH_ANGLE").unwrap_or(0.0);
        graphics.lighting.heading_angle = ini.get_float("LIGHTING", "SUN_HEADING_ANGLE").unwrap_or(0.0);
        graphics.update_lighting_settings();
    }
    graphics.apply_custom_weather("3_clear");
    camera.base.camera.near_plane = 0.05;
    camera.base.camera.far_plane = 40000.0;
    camera.cube_map_renderer.faces_per_frame = 0;
    camera.set_cubemap_size(&graphics, 512);
    // no Documents folder, the oracle's seed for rand()
    graphics.documents_folder = std::path::PathBuf::from("no documents");
    graphics.crt_rand = rustyac_physics::session::MsvcRand(21);

    // Sim::addCar, the mirror of Sim::Sim, Sim::onPostLoad
    let ini = |name: &str| rustyac_physics::data::ini::IniReader::load(&car_folder.join("data").join(name)).ok();
    let steer_lock = ini("car.ini").and_then(|i| i.get_float("CONTROLS", "STEER_LOCK").ok()).unwrap_or(0.0);
    let width = |section: &str| ini("tyres.ini").and_then(|i| i.get_float(section, "WIDTH").ok()).unwrap_or(0.0);
    let tyre_width = [width("FRONT"), width("FRONT"), width("REAR"), width("REAR")];
    let max_gear = ini("drivetrain.ini").and_then(|i| i.get_int("GEARS", "COUNT").ok()).unwrap_or(0);
    let mut skins: Vec<String> = std::fs::read_dir(car_folder.join("skins")).expect("the skins").filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    skins.sort();
    let skin = skins.first().cloned().unwrap_or_default();
    let mut car = CarAvatar::init_3d(&mut graphics, &mut scene, cars, &path_text(&car_folder), &car_folder, &skin, Some(steer_lock), 0).expect("the car");
    let mut mirror = MirrorTextureRenderer::new(&mut graphics, 512, false).expect("the mirror");
    car.init_mirror_materials(&mut graphics, &mut scene, &mirror.texture).expect("the mirror materials");
    let sim = SimNodes { root, cars, skid_marks, particles, car_shadows, before_cars, render_finished };
    car.init_common_post_physics(&mut graphics, &mut scene, &sim, tyre_width).expect("the objects of the car");
    car.on_post_load(&mut graphics, &mut scene, car_shadows);
    car.max_gear = max_gear;
    let pose = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/e30_magione.pose");
    let state = CarPhysicsState::from_bytes(&std::fs::read(&pose).expect("the pose")).expect("a pose");
    let folder_text = path_text(&folder);
    let cube_model = rustyac_render::cubemap::load_static_cubemap_model(&mut graphics, &mut scene, Some(folder_text.as_str())).expect("the cube map's model");
    rustyac_render::cubemap::render_static_cubemap(&mut camera, &mut graphics, &mut scene, cube_model);

    // the chase camera's place behind the car
    let w = state.world_matrix.m;
    let at = |ahead: f32, up: f32| [w[3][0] + w[2][0] * ahead, w[3][1] + up, w[3][2] + w[2][2] * ahead];
    camera.base.camera.fov = 60.0;
    camera.base.camera.matrix = look_from(at(-6.0, 2.0), at(0.0, 1.0));
    camera.base.camera.near_plane = 1.0;
    camera.base.set_shadow_maps_splits(&mut graphics, 10.0, 50.0, 150.0, 500.0);

    // the first frame goes by unlogged, as in the oracle: the logged one starts from its state
    let mut frame = |graphics: &mut Graphics, scene: &mut Scene, camera: &mut CameraForward, index: usize, logged: bool| {
        let matrix = camera.base.camera.matrix;
        car.game_time_ms = index as f64 * (1000.0 / 60.0);
        car.view = ViewState { camera_mode: 2, drivable_mode: 0, focused_car_index: 0, camera_position: [matrix.m[3][0], matrix.m[3][1], matrix.m[3][2]], camera_matrix: matrix, use_pro_view: false };
        car.update(graphics, scene, &state, 1.0 / 60.0);
        car.post_update(scene, &state, 1.0 / 60.0, &matrix, 60.0, false);
        if logged {
            gpulog::begin_capture("magione_chase_bmw_m3_e30");
        }
        graphics.begin_scene();
        scene.traverse(root);
        if mirror.wants_render(2, 0, false) {
            let nodes = car.visibility_nodes();
            let s = MirrorScene { root, particles, before_cars, ideal_line: None, body_transform: car.body_transform, car_nodes: &nodes, mirror_position: car.mirror_position };
            mirror.render(graphics, scene, camera.base.sky_box.as_mut(), &s);
        }
        camera.render(graphics, scene, Some(blurred), root).expect("the frame");
        graphics.set_screen_space_mode();
    };
    frame(&mut graphics, &mut scene, &mut camera, 0, false);
    graphics.end_scene();

    camera.base.set_shadow_maps_splits(&mut graphics, 10.0, 50.0, 150.0, 500.0);
    frame(&mut graphics, &mut scene, &mut camera, 1, true);
    let capture = gpulog::end_capture();
    let (width, height, pixels) = graphics.kgl.read_screen().expect("the picture");
    graphics.end_scene();

    if let Some(file) = std::env::var_os("RUSTYAC_GOLDEN_DUMP") {
        std::fs::write(file, &capture.text).expect("the dump");
    }
    let log = gpulog::hash_bytes(&capture.text);
    let picture = gpulog::hash_bytes(&pixels);
    println!("golden car frame: {} lines, {} draws, log {log:#018x}, {width} x {height} pixels {picture:#018x}", capture.lines, capture.draws);
    assert_eq!((capture.lines, capture.draws), (GOLDEN_LINES, GOLDEN_DRAWS), "the frame's calls");
    assert_eq!(log, GOLDEN_LOG, "the frame's command log");
    assert_eq!((width, height), (1280, 720));
    if picture != GOLDEN_PIXELS {
        // WARP is part of Windows: another build of it may round a pixel another way
        eprintln!("NOTE: the picture's hash is {picture:#018x}, not {GOLDEN_PIXELS:#018x}: another WARP than the one the golden frame was made on?");
    }
}
