// SPDX-License-Identifier: GPL-3.0-or-later

//! The port's side: the same frame with `rustyac-render`, on the same kind of device (WARP,
//! a swap chain on a window that is never shown), through the same command log.

use rustyac_physics::vecmath::Mat44f;
use rustyac_render::forward::CameraForward;
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

pub fn render(args: &Args, frame: &Frame) -> Result<(Vec<u8>, Rendered), String> {
    let window = unsafe { crate::ac::hidden_window(args.width, args.height)? };
    let video = VideoSettings {
        aa_samples: 1,
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
        pp_hdr_enabled: false,
        triple_buffer: false,
    };
    rustyac_render::gpulog::capture_from_install(true);
    let mut graphics = Graphics::new(video, DeviceOptions { warp: true, window: Some(window), log: true }, &args.game)?;
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
    if let Some(track) = &frame.track {
        let model = scene.node(&format!("TRACK {}", track.name));
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
    }

    // Sim::createCamera, Sim::Sim, Sim::initCubemaps
    let mut camera = CameraForward::new(&mut graphics)?;
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
    graphics.documents_folder = crate::root::documents();
    graphics.crt_rand = rustyac_physics::session::MsvcRand(crate::frames::RAND_SEED);
    // Sim::addCar: CarAvatar::init3D
    let mut car = match &frame.car {
        Some(spec) => Some(rustyac_render::car::CarAvatar::init_3d(&mut graphics, &mut scene, cars, &spec.folder, std::path::Path::new(&spec.folder), &spec.skin, Some(spec.steer_lock), 0)?),
        None => None,
    };

    // Sim::Sim: the mirror texture and the virtual mirror, when video.ini asks for mirrors
    let profile = crate::root::profile();
    let mut mirror = match profile.mirror_size {
        0 => None,
        size => Some(rustyac_render::mirror::MirrorTextureRenderer::new(&mut graphics, size, profile.mirror_smoke)?),
    };
    let mut virtual_mirror = mirror.as_ref().map(|_| rustyac_render::mirror::VirtualMirrorRenderer::new(&graphics, profile.virtual_mirror));
    if let (Some(car), Some(mirror)) = (&mut car, &mirror) {
        car.init_mirror_materials(&mut graphics, &mut scene, &mirror.texture)?;
    }
    // Sim::onPostLoad: CarAvatar::onPostLoad
    if let Some(car) = &mut car {
        let sim = rustyac_render::car::SimNodes { root, cars, skid_marks, particles, car_shadows, before_cars, render_finished };
        car.init_common_post_physics(&mut graphics, &mut scene, &sim, frame.car.as_ref().map(|c| c.tyre_width).unwrap_or([0.0; 4]))?;
        car.on_post_load(&mut graphics, &mut scene, car_shadows);
    }
    let cube_model = rustyac_render::cubemap::load_static_cubemap_model(&mut graphics, &mut scene, frame.track.as_ref().map(|t| t.folder.as_str()))?;
    rustyac_render::gpulog::begin_capture("static cube map");
    rustyac_render::cubemap::render_static_cubemap(&mut camera, &mut graphics, &mut scene, cube_model);
    let cube_log = rustyac_render::gpulog::end_capture().text;

    let mut rendered = Rendered { cube_log, frames: Vec::new(), width: 0, height: 0 };
    for (index, step) in frame.steps.iter().enumerate() {
        camera.base.camera.fov = step.camera.fov;
        camera.base.camera.matrix = mat(&step.camera.matrix);
        camera.base.camera.near_plane = step.camera.near;
        if let Some(far) = step.camera.far {
            camera.base.camera.far_plane = far;
        }
        let s = step.camera.splits;
        camera.base.set_shadow_maps_splits(&mut graphics, s[0], s[1], s[2], s[3]);
        // SunAnimator::update, the clouds' part
        if let Some(sky) = &mut camera.base.sky_box {
            sky.update_clouds_animation(crate::frames::DT * args.time_mult);
        }
        // Game::update: the car's objects, then the handlers of evOnPostUpdate
        if let Some(car) = &mut car {
            car.max_gear = frame.car.as_ref().map(|c| c.max_gear).unwrap_or(0);
            car.game_time_ms = crate::frames::game_time_ms(index);
            car.view = rustyac_render::car::ViewState {
                camera_mode: step.camera.mode,
                drivable_mode: step.camera.drivable_mode,
                focused_car_index: 0,
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
            car.update(&mut graphics, &mut scene, &step.state, crate::frames::DT);
            car.post_update(&mut scene, &step.state, crate::frames::DT, &mat(&step.camera.matrix), step.camera.fov, false);
        }

        if index >= frame.capture {
            rustyac_render::gpulog::begin_capture(&frame.name);
        }
        graphics.begin_scene();
        scene.traverse(root);
        // Sim::renderScene: the mirror before the picture
        if let (Some(mirror), Some(car)) = (&mut mirror, &car) {
            let active = virtual_mirror.as_ref().is_some_and(|v| v.active);
            if mirror.wants_render(step.camera.mode, step.camera.drivable_mode, active) {
                let nodes = car.visibility_nodes();
                let s = rustyac_render::mirror::MirrorScene { root, particles, before_cars, ideal_line: None, body_transform: car.body_transform, car_nodes: &nodes, mirror_position: car.mirror_position };
                mirror.render(&mut graphics, &mut scene, camera.base.sky_box.as_mut(), &s);
            }
        }
        camera.render(&mut graphics, &mut scene, Some(blurred), root)?;
        graphics.set_screen_space_mode();
        // Game::evOnPreGUI
        if let (Some(mirror), Some(v)) = (&mirror, &mut virtual_mirror) {
            v.render(&mut graphics, &mirror.texture, step.camera.mode);
        }
        if index >= frame.capture {
            let capture = rustyac_render::gpulog::end_capture();
            let (width, height, pixels) = graphics.kgl.read_screen()?;
            rendered.width = width;
            rendered.height = height;
            rendered.frames.push(Captured::new(index, capture.text, capture.draws, pixels, !frame.sequence || args.dump.contains(&index)));
        }
        graphics.end_scene();
    }
    Ok((init.text, rendered))
}
