// SPDX-License-Identifier: GPL-3.0-or-later

//! The port's side of the GUI oracle: the widgets of `rustyac-gui` drawn by `rustyac-render`'s
//! painter through the same command log as the game's, driven by the same script.

use rustyac_gui::control::{Id, Kind, MouseEvent};
use rustyac_gui::font::{Font, FontType};
use rustyac_gui::gui::{Class, Gui, GuiFiles};
use rustyac_gui::painter::Painter;
use rustyac_gui::widgets::Widget;
use rustyac_physics::data::ini::IniReader;
use rustyac_render::graphics::{Graphics, VideoSettings};
use rustyac_render::gui_paint::GuiRenderer;
use rustyac_render::kgl::DeviceOptions;

use crate::gui_seq::{ControlState, Op, Outcome, Sequence, Val};
use crate::Args;

pub fn graphics(args: &Args) -> Result<Graphics, String> {
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
    Graphics::new(video, DeviceOptions { warp: !crate::ac::hardware(), window: Some(window), log: !crate::timing() }, &args.game)
}

fn class_name(gui: &Gui, id: Id) -> &'static str {
    match &gui.controls[id].kind {
        Kind::Control => "Control",
        Kind::Label { .. } => "Label",
        Kind::Form(_) => "Form",
        Kind::GameScreen(_) => "GameScreen",
        Kind::Taskbar(_) => "Taskbar",
        Kind::TaskBarIcon(_) => "TaskBarIcon",
        Kind::Menu(_) => "PauseMenu",
        Kind::Widget(w) => match &**w {
            Widget::ProgressBar(_) => "ProgressBar",
            Widget::ActiveButton(_) => "ActiveButton",
            Widget::CheckBox(_) => "CheckBox",
            Widget::Slider(_) => "Slider",
            Widget::More(_) => crate::gui_port2::class_name(gui, id),
        },
    }
}

pub fn dump_control(gui: &Gui, id: Id, depth: usize, out: &mut String) {
    let c = &gui.controls[id];
    let r = |r: &rustyac_gui::painter::Rect| [r.left, r.right, r.top, r.bottom];
    let class = class_name(gui, id);
    let extra = match &c.kind {
        Kind::Form(d) => format!(" form={}{}{} save={:08x} step={:08x}", d.dev_app as u8, d.autohide as u8, d.blocked as u8, d.scale_to_be_saved.to_bits(), d.scale_step.to_bits()),
        Kind::TaskBarIcon(d) => format!(" icon={} target={:08x}", d.status, d.target_x.to_bits()),
        Kind::Taskbar(d) => format!(" bar={},{},{},{}", d.target_x, d.task_bar_full_showed as u8, d.top_app_index, d.virtual_desktop),
        Kind::GameScreen(d) => format!(" screen={} hidden={}", d.app_interaction_enabled as u8, d.old_apps_opened.len()),
        Kind::Menu(_) => crate::gui_port_menu::extra(gui, id),
        Kind::Widget(w) => match &**w {
            Widget::ActiveButton(b) => format!(" button={} sel={} multi={} active={} label=\"{}\"", b.status, b.selected as u8, b.using_multiple_textures as u8, b.is_active as u8, b.active_button_text),
            Widget::ProgressBar(p) => format!(" progress={:08x},{:08x},{:08x},{}{}", p.max_value.to_bits(), p.min_value.to_bits(), p.value.to_bits(), p.is_vertical as u8, p.is_inverted as u8),
            Widget::CheckBox(b) => format!(" check={}", b.box_value as u8),
            Widget::Slider(s) => format!(" slider={:08x}", s.value.to_bits()),
            Widget::More(_) => crate::gui_port2::extra(gui, id),
        },
        _ => String::new(),
    };
    out.push_str(&crate::gui_seq::control_line(
        depth,
        &ControlState {
            class,
            name: c.name.clone(),
            rect: r(&c.rect),
            rect_base: r(&c.rect_base),
            visible: c.visible,
            is_mouse_down: c.is_mouse_down,
            highlight: c.highlight,
            is_highlight: c.is_highlight,
            draw_border: c.draw_border,
            draw_background: c.draw_background,
            text: c.text.clone(),
            font_scale: c.font_scale,
            scale_mult: c.scale_mult,
            font_align: c.font_align,
            colors: [c.back_color, c.border_color, c.fore_color, c.highlight_text_color, c.back_texture_color],
            has_texture: c.back_texture.kid.is_some(),
            repeat: (c.repeat_interval, c.interval_counter, c.is_repeating),
            extra,
        },
    ));
    if let Kind::TaskBarIcon(d) = &c.kind {
        dump_control(gui, d.description_rect, depth + 1, out);
    }
    for child in &c.controls {
        dump_control(gui, *child, depth + 1, out);
    }
}

fn dump(gui: &Gui, hud: &crate::gui_port2::Hud, out: &mut String) {
    let name = gui.captured_form.map(|f| gui.controls[f].name.clone()).unwrap_or_else(|| "-".into());
    out.push_str(&format!(
        "gui captured={name} at={:08x},{:08x} good={:08x},{:08x} move={} area={} mouse={:08x},{:08x} desktop={}\n",
        gui.captured_pos[0].to_bits(),
        gui.captured_pos[1].to_bits(),
        gui.old_good_pos[0].to_bits(),
        gui.old_good_pos[1].to_bits(),
        gui.move_app as u8,
        gui.render_drawing_area as u8,
        gui.mouse_pos[0].to_bits(),
        gui.mouse_pos[1].to_bits(),
        gui.virtual_desktop,
    ));
    for root in &gui.roots {
        dump_control(gui, *root, 0, out);
    }
    dump_control(gui, gui.taskbar, 0, out);
    crate::gui_port_hud::dump(&hud.hud, gui, out);
}

fn op(gui: &mut Gui, painter: &mut dyn Painter, hud: &mut crate::gui_port2::Hud, screen: Id, objects: &mut Vec<Id>, op: &Op) -> Result<(), String> {
    match op {
        Op::New(class, name, flag) => {
            let id = match *class {
                "Control" => gui.new_control(painter, name),
                "Label" => gui.new_label(painter, name),
                "Form" => gui.new_form(painter, name, *flag),
                "ActiveButton" => gui.new_active_button(painter, name),
                "ProgressBar" => gui.new_progress_bar(painter, name),
                "CheckBox" => gui.new_check_box(painter, name),
                "Slider" => gui.new_slider(painter, name, *flag),
                other => crate::gui_port2::new_object(gui, painter, other, name, *flag)?,
            };
            objects.push(id);
        }
        Op::Child(parent, index) => {
            let child = *gui.controls[objects[*parent]].controls.get(*index).ok_or("no such child")?;
            objects.push(child);
        }
        Op::AddScreen(control, taskbar) => gui.game_screen_add_control(painter, screen, objects[*control], *taskbar),
        Op::Add(parent, child) => gui.add_control(objects[*parent], objects[*child]),
        Op::Pos(id, x, y) => gui.set_position(objects[*id], *x, *y),
        Op::Size(id, w, h) => gui.set_size(objects[*id], *w, *h),
        Op::Text(id, text) => gui.set_text(objects[*id], text),
        Op::Visible(id, visible) => gui.set_visible(objects[*id], *visible),
        Op::Set(id, name, value) => {
            let id = objects[*id];
            let c = &mut gui.controls[id];
            match (*name, value) {
                ("backColor", Val::C(v)) => c.back_color = *v,
                ("borderColor", Val::C(v)) => c.border_color = *v,
                ("foreColor", Val::C(v)) => c.fore_color = *v,
                ("highlightTextColor", Val::C(v)) => c.highlight_text_color = *v,
                ("highlight", Val::B(v)) => c.highlight = *v,
                ("backTextureColor", Val::C(v)) => c.back_texture_color = *v,
                ("fontAlign", Val::I(v)) => c.font_align = *v,
                ("drawBorder", Val::B(v)) => c.draw_border = *v,
                ("drawBackground", Val::B(v)) => c.draw_background = *v,
                ("repeatInterval", Val::F(v)) => c.repeat_interval = *v,
                ("fontScale", Val::F(v)) => c.font_scale = *v,
                ("label.maxChars", Val::I(v)) => c.kind = Kind::Label { max_chars: *v as u32 },
                ("progress.max", Val::F(v)) => gui.progress_bar_mut(id).max_value = *v,
                ("progress.min", Val::F(v)) => gui.progress_bar_mut(id).min_value = *v,
                ("progress.value", Val::F(v)) => gui.progress_bar_mut(id).value = *v,
                ("progress.vertical", Val::B(v)) => gui.progress_bar_mut(id).is_vertical = *v,
                ("progress.inverted", Val::B(v)) => gui.progress_bar_mut(id).is_inverted = *v,
                ("button.status", Val::I(v)) => gui.active_button_mut(id).status = *v,
                ("button.blanked", Val::B(v)) => gui.active_button_mut(id).blanked = *v,
                ("button.highlightOnCursorOver", Val::B(v)) => gui.active_button_mut(id).highlight_on_cursor_over = *v,
                ("button.isActive", Val::B(v)) => gui.active_button_mut(id).is_active = *v,
                ("slider.value", Val::F(v)) => gui.slider_mut(id).value = *v,
                ("form.devApp", Val::B(v)) => gui.form_data_mut(id).dev_app = *v,
                ("screen.appInteractionEnabled", Val::B(v)) => {
                    if let Kind::GameScreen(d) = &mut gui.controls[id].kind {
                        d.app_interaction_enabled = *v;
                    }
                }
                (other, value) => crate::gui_port2::set(gui, id, other, value)?,
            }
        }
        Op::Call(id, what, number, text) => {
            let id = objects[*id];
            match *what {
                "scaleByMult" => gui.scale_by_mult(id, *number),
                "title" => gui.form_set_title(id, text),
                "autohide" => gui.set_auto_hide_mode(id, *number != 0.0),
                "blocked" => gui.set_blocked(id, *number != 0.0),
                "cycleDesktop" => {
                    gui.cycle_desktop();
                }
                "showDesktop" => gui.show_desktop(screen),
                "multipleTexture" => gui.set_multiple_texture(painter, id, text),
                "setSelected" => gui.set_selected(id, *number != 0.0),
                "setCheck" => gui.set_check(id, *number != 0.0),
                "markerLeft" => gui.slider_set_marker_left_position(id, *number),
                "markerRight" => gui.slider_set_marker_right_position(id, *number),
                "updateMarkers" => gui.slider_update_markers(id),
                "resetMarkers" => gui.slider_reset_markers(id),
                other => crate::gui_port2::call(gui, painter, hud, id, other, *number, text)?,
            }
        }
        Op::MouseMove(x, y) => gui.mouse_move(&MouseEvent { x: *x, y: *y, button: -1, delta: 0.0 }),
        Op::MouseDown(x, y) => gui.mouse_down(&MouseEvent { x: *x, y: *y, button: 0, delta: 0.0 }),
        Op::MouseUp(x, y) => gui.mouse_up(&MouseEvent { x: *x, y: *y, button: 0, delta: 0.0 }),
        Op::Wheel(x, y, delta) => gui.mouse_wheel(&MouseEvent { x: *x, y: *y, button: -1, delta: *delta }),
        Op::KeyDown(code) => gui.key_down(*code),
        Op::KeyChar(code) => gui.key_char(*code),
        Op::Font(..) | Op::Blit(..) | Op::ClearText | Op::Frame(_) | Op::Save | Op::Hud(..) => {}
    }
    Ok(())
}

pub fn run(args: &Args, sequence: &Sequence, name: &str) -> Result<Outcome, String> {
    let mut graphics = graphics(args)?;
    let mut renderer = GuiRenderer::new(&graphics);
    let mut outcome = Outcome::default();
    // what the game's side reads from its scratch root: the same texts here
    let read = |path: std::path::PathBuf| IniReader::load(&path).ok().filter(|ini| ini.ready);
    let files = GuiFiles {
        skin: IniReader::load(&args.game.join("content/gui/skin.ini")).map_err(|e| e.to_string())?,
        acos: sequence.acos.map(|text| IniReader::from_text(std::path::Path::new("cfg/acos.ini"), text)),
        gameplay: Some(IniReader::from_text(std::path::Path::new("cfg/gameplay.ini"), &format!("[GUI]\nALLOW_OVERLAPPING_FORMS={}\n", sequence.allow_overlapping as i32))),
        assetto_corsa: read(args.game.join("system/cfg/assetto_corsa.ini")),
    };
    let mut gui = {
        let mut painter = renderer.painter(&mut graphics);
        Gui::new(&mut painter, files)
    };
    let screen = {
        let mut painter = renderer.painter(&mut graphics);
        let screen = gui.new_game_screen(&mut painter, "SIM_GAME_SCREEN");
        gui.add_root(&mut painter, screen);
        screen
    };
    let mut hud = crate::gui_port2::Hud::default();
    let mut objects = vec![screen];
    let mut fonts: Vec<Font> = Vec::new();
    let mut blits: Vec<(usize, f32, f32, &str, f32, i32, [f32; 4], bool)> = Vec::new();
    let mut frame = 0usize;
    let mut fired = 0usize;
    let mut widget_events = 0usize;
    for o in &sequence.ops {
        match o {
            Op::Frame(dt) => {
                gui.time_ms = crate::gui_seq::time_ms(frame);
                rustyac_render::gpulog::begin_capture(name);
                graphics.begin_scene();
                graphics.set_screen_space_mode();
                {
                    let mut painter = renderer.painter(&mut graphics);
                    crate::gui_port_hud::update(&mut hud.hud, &mut gui, *dt);
                    gui.render_all(*dt, &mut painter);
                    crate::gui_port_hud::render_hud(&mut hud.hud, &mut gui, &mut painter, *dt);
                    hud.render(&mut gui, &mut painter, *dt);
                    for (font, x, y, text, size, align, color, shadowed) in &blits {
                        let font: &mut Font = &mut fonts[*font];
                        font.set_color(color[0], color[1], color[2], color[3]);
                        font.shadowed = *shadowed;
                        font.blit_string(&mut painter, *x, *y, text, *size, *align);
                    }
                }
                let capture = rustyac_render::gpulog::end_capture();
                let (width, height, pixels) = graphics.kgl.read_screen()?;
                graphics.end_scene();
                crate::gui_compare::frame_done(args, name, "port", frame, &capture.text, capture.draws, width, height, &pixels, &mut outcome)?;
                outcome.state.push_str(&format!("frame {frame}\n"));
                for event in &gui.fired[fired..] {
                    outcome.state.push_str(&format!("event click \"{}\" {} {}\n", crate::gui_port2::name(&gui, event.control), event.local_x, event.local_y));
                }
                fired = gui.fired.len();
                for event in &gui.widget_events[widget_events..] {
                    outcome.state.push_str(&format!("event {}\n", crate::gui_port2::event_text(&gui, event)));
                }
                widget_events = gui.widget_events.len();
                dump(&gui, &hud, &mut outcome.state);
                frame += 1;
            }
            Op::Save => outcome.saved.push(gui.serialize_forms().unwrap_or_default()),
            Op::Font(kind, scale, italic, bold) => {
                let mut painter = renderer.painter(&mut graphics);
                let kind = if *kind == 0 { FontType::Proportional } else { FontType::Monospaced };
                fonts.push(Font::new(&mut painter, "", kind, *scale, *italic, *bold));
            }
            Op::Blit(font, x, y, text, size, align, color, shadowed) => blits.push((*font, *x, *y, *text, *size, *align, *color, *shadowed)),
            Op::ClearText => blits.clear(),
            Op::Hud(what, number, text) => {
                let mut painter = renderer.painter(&mut graphics);
                crate::gui_port_hud::call(&mut hud.hud, &mut gui, &mut painter, &args.root, screen, what, *number, text)?;
                crate::gui_port_menu::drain(&mut hud.hud, &mut gui);
            }
            other => {
                let mut painter = renderer.painter(&mut graphics);
                op(&mut gui, &mut painter, &mut hud, screen, &mut objects, other)?;
                crate::gui_port_menu::drain(&mut hud.hud, &mut gui);
            }
        }
    }
    let _ = Class::Control;
    Ok(outcome)
}
