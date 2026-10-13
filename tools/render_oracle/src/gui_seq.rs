// SPDX-License-Identifier: GPL-3.0-or-later

//! The GUI oracle's scripts: what both sides build, the mouse and key events they are fed and
//! the frames they draw. A script is a list of [`Op`]s; objects are named by the order they
//! were made in (the game screen is object 0).

use rustyac_physics::vecmath::{xm_matrix_inverse, Mat44f};

#[derive(Clone, Debug)]
pub enum Val {
    F(f32),
    B(bool),
    I(i32),
    C([f32; 4]),
}

#[derive(Clone, Debug)]
pub enum Op {
    /// a new object of a class of ksgui: class, name, a flag for the classes that take one
    New(&'static str, &'static str, bool),
    /// the n-th child of an object becomes an object of its own
    Child(usize, usize),
    /// `GameScreen::addControl(control, showInTaskbar)` on the game screen
    AddScreen(usize, bool),
    /// `parent->addControl(child)`
    Add(usize, usize),
    Pos(usize, f32, f32),
    Size(usize, f32, f32),
    Text(usize, &'static str),
    Visible(usize, bool),
    /// a member written directly (the names of [`FIELDS`])
    Set(usize, &'static str, Val),
    /// a method of the object's class (the names each side knows)
    Call(usize, &'static str, f32, &'static str),
    MouseMove(i32, i32),
    MouseDown(i32, i32),
    MouseUp(i32, i32),
    Wheel(i32, i32, f32),
    KeyDown(u32),
    KeyChar(u32),
    /// a font of its own for the text of [`Op::Blit`]: type (0 Segoe UI, 1 Consolas), scale, italic, bold
    Font(i32, f32, bool, bool),
    /// text drawn after the GUI in every frame from here on: font, x, y, text, size, align, colour, shadowed
    Blit(usize, f32, f32, &'static str, f32, i32, [f32; 4], bool),
    /// the text list is emptied
    ClearText,
    /// a call on the HUD objects, or a change of the session's state they read
    Hud(&'static str, f32, &'static str),
    /// one frame is drawn (and compared): the frame's time
    Frame(f32),
    /// `GUI::serializeForms`: the layout file is written (and compared)
    Save,
}

/// Members a script may write: name, offset in the game's object, kind (f float, b bool, i int,
/// c four floats).
pub const FIELDS: [(&str, usize, char); 41] = [
    ("backColor", 0x38, 'c'),
    ("borderColor", 0x48, 'c'),
    ("foreColor", 0x58, 'c'),
    ("highlightTextColor", 0x68, 'c'),
    ("highlight", 0x78, 'b'),
    ("backTextureColor", 0x7c, 'c'),
    ("fontAlign", 0xc0, 'i'),
    ("drawBorder", 0x110, 'b'),
    ("drawBackground", 0x111, 'b'),
    ("repeatInterval", 0x114, 'f'),
    ("fontScale", 0x118, 'f'),
    ("label.maxChars", 0x178, 'i'),
    ("progress.max", 0x178, 'f'),
    ("progress.min", 0x17c, 'f'),
    ("progress.value", 0x180, 'f'),
    ("progress.vertical", 0x184, 'b'),
    ("progress.inverted", 0x185, 'b'),
    ("button.status", 0x1c0, 'i'),
    ("button.blanked", 0x1c4, 'b'),
    ("button.highlightOnCursorOver", 0x1c5, 'b'),
    ("button.isActive", 0x1c6, 'b'),
    ("slider.value", 0x1a8, 'f'),
    ("screen.appInteractionEnabled", 0x178, 'b'),
    ("form.devApp", 0x180, 'b'),
    ("graph.minY", 0x178, 'f'),
    ("graph.maxY", 0x17c, 'f'),
    ("graph.maxValues", 0x180, 'i'),
    ("graph.auto", 0x184, 'b'),
    ("spinner.homogeneous", 0x178, 'b'),
    ("spinner.mult", 0x17c, 'f'),
    ("spinner.min", 0x180, 'i'),
    ("spinner.max", 0x184, 'i'),
    ("spinner.step", 0x188, 'i'),
    ("spinner.popover", 0x1b0, 'b'),
    ("spinner.underline", 0x1b1, 'b'),
    ("spinner.central", 0x1b2, 'b'),
    ("spinner.label", 0x1b3, 'b'),
    ("spinner.textSize", 0x1d8, 'f'),
    ("spinner.readOnly", 0x1dc, 'b'),
    ("cspin.press", 0x178, 'b'),
    ("cspin.bar", 0x1d0, 'f'),
];

/// An item of a list as a script writes it: "name;column;column;…" and a number that picks
/// its colours (the same on both sides).
pub fn list_item(text: &str, number: f32) -> (String, Vec<String>, [f32; 4], Vec<[f32; 4]>) {
    let mut parts = text.split(';');
    let name = parts.next().unwrap_or("").to_string();
    let columns: Vec<String> = parts.map(|s| s.to_string()).collect();
    let code = number as u32;
    let bit = |n: u32| ((code >> n) & 1) as f32 * 0.8 + 0.1;
    let back = [bit(0), bit(1), bit(2), 0.5];
    let fore = (0..columns.len()).map(|i| if !columns.is_empty() && i == code as usize % columns.len() { [1.0, 1.0, 0.0, 1.0] } else { [1.0; 4] }).collect();
    (name, columns, back, fore)
}

/// A colour of a graph's series from a script's number.
pub fn code_color(number: f32) -> [f32; 3] {
    let code = number as u32;
    [(code & 1) as f32, ((code >> 1) & 1) as f32, ((code >> 2) & 1) as f32]
}

/// The values a script fills a series with: "serie:count" and an amplitude.
pub fn fill_values(text: &str, amplitude: f32) -> (u32, Vec<f32>) {
    let (serie, count) = text.split_once(':').unwrap_or(("0", "0"));
    let count: u32 = count.parse().unwrap_or(0);
    (serie.parse().unwrap_or(0), (0..count).map(|i| (((i * 37) % 100) as f32 / 50.0 - 1.0) * amplitude).collect())
}

pub fn field(name: &str) -> Result<(usize, char), String> {
    FIELDS.iter().find(|f| f.0 == name).map(|f| (f.1, f.2)).ok_or(format!("no member {name:?} in the oracle's table"))
}

/// A layout file for the sequences that want forms to come up as a session left them.
pub const ACOS_SAMPLE: &str = "[HEADER]\nVERSION=2\nDESKTOP_SELECTED=1\n\n[DESK_0_FORM_ALPHA]\nPOSX=120\nPOSY=140\nVISIBLE=1\nBLOCKED=0\nSCALE=1\n\n[DESK_0_FORM_BETA]\nPOSX=520.5\nPOSY=300\nVISIBLE=1\nBLOCKED=1\nSCALE=1.25\n\n[DESK_1_FORM_ALPHA]\nPOSX=700\nPOSY=90\nVISIBLE=1\nBLOCKED=0\nSCALE=1.5\n\n[DESK_1_FORM_BETA]\nPOSX=5000\nPOSY=20\nVISIBLE=0\nBLOCKED=0\nSCALE=1\n\n[DESK_2_FORM_GAMMA]\nPOSX=40\nPOSY=5000\nVISIBLE=1\nBLOCKED=0\n\n";

pub struct Sequence {
    pub ops: Vec<Op>,
    /// files of `system/cfg` the session finds changed: name, text
    pub cfg: &'static [(&'static str, &'static str)],
    /// `cfg/acos.ini` as the session finds it (none: no file)
    pub acos: Option<&'static str>,
    /// `cfg/gameplay.ini [GUI] ALLOW_OVERLAPPING_FORMS`
    pub allow_overlapping: bool,
}

const DT: f32 = 1.0 / 60.0;

/// A car as the HUD scripts start it: a valid lap, no place yet.
pub fn default_car() -> rustyac_gui::hud::HudCar {
    rustyac_gui::hud::HudCar { current_lap_valid: true, leaderboard_position: -1, steer_lock: 1.0, ping: -1, body_matrix: Mat44f::IDENTITY, engine_life_left: 1000.0, gearbox: 1.0, tyre_inflation: [1.0; 4], ..Default::default() }
}

/// A script's change of what the HUD reads of the session (true: it was one).
pub fn hud_input(input: &mut rustyac_gui::hud::HudInput, what: &str, number: f32, text: &str) -> bool {
    let on = number != 0.0;
    match what {
        "session" => input.session_type = number as i32,
        "pqText" => input.practice_qualify_text = text.to_string(),
        "raceText" => input.race_text = text.to_string(),
        "tts" => input.time_to_session_start = number as f64,
        "esc" => {
            input.esc_menu_visible = on;
            input.esc_menu_width = text.parse().unwrap_or(0.0);
        }
        "pause" => input.pause_menu_visible = on,
        "replay" => input.in_replay = on,
        _ if what.starts_with("car.") => {
            // a number: the car is the text; a text: the car is the number
            let textual = matches!(what, "car.name" | "car.guiName" | "car.unixName");
            let index: usize = if textual { number as usize } else { text.parse().unwrap_or(0) };
            let Some(car) = input.cars.get_mut(index) else { return true };
            match what {
                "car.name" => car.driver_name = text.to_string(),
                "car.guiName" => car.gui_name = text.to_string(),
                "car.unixName" => car.unix_name = text.to_string(),
                "car.steer" => car.steer = number,
                "car.steerLock" => car.steer_lock = number,
                "car.penalty" => car.penalty_time = number as f64,
                "car.lock" => car.lock_controls_time = number,
                "car.valid" => car.current_lap_valid = on,
                "car.pos" => car.leaderboard_position = number as i32,
                "car.last" => car.last_lap_ms = number as u32,
                _ => return crate::gui_seq::hud_input2(input, what, number, text),
            }
        }
        _ => return crate::gui_seq::hud_input2(input, what, number, text),
    }
    true
}

/// (the state of the HUD objects of part two)
pub fn hud_input2(input: &mut rustyac_gui::hud::HudInput, what: &str, number: f32, text: &str) -> bool {
    let on = number != 0.0;
    match what {
        "focus" => input.focused_car_index = number as i32,
        "raceOver" => input.race_over = on,
        "timeLeft" => input.session_time_left = number as f64,
        "triple" => input.triple_screen = on,
        "mph" => input.use_mph = on,
        "cameraMode" => input.camera_mode = number as i32,
        "ended" => input.end_session_active = on,
        _ if what.starts_with("car.") => {
            if what == "car.nation" {
                if let Some(car) = input.cars.get_mut(number as usize) {
                    car.nation_code = text.to_string();
                }
                return true;
            }
            let Some(car) = text.parse::<usize>().ok().and_then(|i| input.cars.get_mut(i)) else { return false };
            match what {
                "car.clearDisabled" => car.min_speed_penalty_clear_disabled = on,
                "car.pitLaps" => car.pit_penalty_laps = number as i16,
                "car.fuel" => car.fuel = number,
                _ => return hud_input3(input, what, number, text),
            }
        }
        _ => return hud_input3(input, what, number, text),
    }
    true
}

/// A turn about the y axis (degrees) that keeps the matrix's place.
fn yaw(m: &mut Mat44f, degrees: f32) {
    let (s, c) = degrees.to_radians().sin_cos();
    m.m[0] = [c, 0.0, -s, 0.0];
    m.m[1] = [0.0, 1.0, 0.0, 0.0];
    m.m[2] = [s, 0.0, c, 0.0];
}

pub fn hud_input3(input: &mut rustyac_gui::hud::HudInput, what: &str, number: f32, text: &str) -> bool {
    let on = number != 0.0;
    let index = |name: &str, prefix: &str| name.strip_prefix(prefix).and_then(|k| k.parse::<usize>().ok()).filter(|k| *k < 4);
    match what {
        "camX" => input.camera_matrix.m[3][0] = number,
        "camY" => input.camera_matrix.m[3][1] = number,
        "camZ" => input.camera_matrix.m[3][2] = number,
        "camYaw" => yaw(&mut input.camera_matrix, number),
        "online" => input.online = on,
        "now" => input.game_time_now = number as f64,
        _ if what.starts_with("car.") => {
            let Some(car) = text.parse::<usize>().ok().and_then(|i| input.cars.get_mut(i)) else { return false };
            match what {
                "car.x" | "car.y" | "car.z" => {
                    let k = (what.as_bytes()[4] - b'x') as usize;
                    car.world_position[k] = number;
                    car.body_matrix.m[3][k] = number;
                }
                "car.yaw" => yaw(&mut car.body_matrix, number),
                "car.connected" => car.connected = on,
                "car.laps" => car.lap_count = number as i32,
                "car.rtpos" => car.real_time_position = number as i32,
                "car.ping" => car.ping = number as i32,
                "car.engine" => car.engine_life_left = number,
                "car.gearbox" => car.gearbox = number,
                _ => {
                    if let Some(k) = index(what, "car.zone") {
                        car.damage_zone_level[k] = number;
                    } else if let Some(k) = index(what, "car.tyre") {
                        car.tyre_inflation[k] = number;
                    } else if let Some(k) = index(what, "car.sus") {
                        car.sus_damage[k] = number;
                    } else if let Some(k) = index(what, "car.flat") {
                        car.tyre_flat_spot[k] = number;
                    } else {
                        return false;
                    }
                }
            }
        }
        _ => return false,
    }
    true
}

/// The view and the perspective both sides take for a camera's matrix: its inverse, and 60
/// degrees up and down on a 16:9 screen from 0.1 to 1000 m.
pub fn hud_view(camera: &Mat44f) -> (Mat44f, Mat44f) {
    let h = 1.732_050_8f32;
    let w = h * 9.0 / 16.0;
    let (n, f) = (0.1f32, 1000.0f32);
    let perspective = Mat44f { m: [[w, 0.0, 0.0, 0.0], [0.0, h, 0.0, 0.0], [0.0, 0.0, f / (n - f), -1.0], [0.0, 0.0, n * f / (n - f), 0.0]] };
    (xm_matrix_inverse(camera), perspective)
}

pub fn damage_line(timer: f32, initialized: bool, enabled: bool, items: &[(f32, f32)]) -> String {
    let items: Vec<String> = items.iter().map(|(h, b)| format!("{:08x}:{:08x}", h.to_bits(), b.to_bits())).collect();
    format!("hud damage timer={:08x} init={} enabled={} items={}\n", timer.to_bits(), initialized as u8, enabled as u8, items.join(","))
}

/// The name tags, the proximity arrows and the damage picture.
fn hud_cars() -> Vec<Op> {
    use Op::*;
    let mut o = vec![Hud("create", 5.0, ""), Hud("session", 1.0, ""), Hud("pqText", 0.0, "Practice"), Hud("camY", 1.2, "")];
    let cars: [(&str, &str, &str, [f32; 3]); 5] = [("Tazio Nuvolari", "ITA", "0", [0.0, 0.0, -4.0]), ("Rudolf Caracciola", "DEU", "1", [-2.0, 0.0, -12.0]), ("Bernd Rosemeyer", "", "2", [4.0, 0.0, -35.0]), ("A. N. Other", "ZZZ", "3", [0.5, 0.0, -80.0]), ("Behind", "GBR", "4", [0.0, 0.0, 30.0])];
    for (i, (name, nation, index, at)) in cars.into_iter().enumerate() {
        o.extend([Hud("car.name", i as f32, name), Hud("car.nation", i as f32, nation), Hud("car.x", at[0], index), Hud("car.y", at[1], index), Hud("car.z", at[2], index), Hud("car.connected", 1.0, index), Hud("car.pos", i as f32 + 1.0, index)]);
    }
    o.extend([Frame(DT), Frame(DT)]);
    // a car comes nearer, a name grows, a place changes
    o.extend([Hud("car.z", -8.0, "1"), Frame(DT), Hud("car.name", 2.0, "Bernd Rosemeyer of the Auto Union"), Hud("car.pos", 12.0, "2"), Frame(DT), Hud("car.nation", 2.0, "ITA"), Frame(DT)]);
    // a race: places as they are on the road, laps ahead and behind
    o.extend([Hud("session", 3.0, ""), Hud("raceText", 0.0, "Race"), Hud("car.laps", 2.0, "0"), Hud("car.laps", 3.0, "1"), Hud("car.laps", 1.0, "2"), Hud("car.laps", 2.0, "3"), Hud("car.rtpos", 0.0, "1"), Hud("car.rtpos", 3.0, "2"), Hud("car.rtpos", 1.0, "3"), Frame(DT)]);
    // online: the pings
    o.extend([Hud("online", 1.0, ""), Hud("car.ping", 50.0, "1"), Hud("car.ping", 150.0, "2"), Hud("car.ping", 300.0, "3"), Frame(DT), Hud("car.ping", 250.0, "3"), Hud("car.ping", 100.0, "2"), Frame(DT), Hud("online", 0.0, ""), Frame(DT)]);
    // from outside the car; another car in focus; the switch; the menus
    o.extend([Hud("cameraMode", 1.0, ""), Frame(DT), Hud("focus", 1.0, ""), Frame(DT), Hud("cameraMode", 0.0, ""), Frame(DT), Hud("focus", 0.0, ""), Frame(DT)]);
    o.extend([Hud("namesActive", 0.0, ""), Frame(DT), Hud("namesActive", 1.0, ""), Frame(DT), Hud("pause", 1.0, ""), Frame(DT), Hud("pause", 0.0, ""), Hud("esc", 1.0, "300"), Frame(DT), Hud("esc", 0.0, "300"), Frame(DT)]);
    // the camera turns and moves; a car too far; a car that left
    o.extend([Hud("camYaw", 25.0, ""), Frame(DT), Hud("camYaw", -40.0, ""), Hud("camX", 3.0, ""), Frame(DT), Hud("camYaw", 0.0, ""), Hud("camX", 0.0, ""), Hud("car.z", -150.0, "3"), Frame(DT), Hud("car.connected", 0.0, "2"), Frame(DT), Hud("camYaw", 180.0, ""), Frame(DT), Hud("camYaw", 0.0, ""), Frame(DT)]);
    // the arrows: cars within ten metres, around a car that turns
    o.extend([Hud("car.x", 2.0, "1"), Hud("car.z", -6.0, "1"), Frame(DT), Hud("car.x", -3.0, "4"), Hud("car.z", 2.0, "4"), Frame(DT), Hud("car.yaw", 90.0, "0"), Frame(DT), Hud("car.yaw", 200.0, "0"), Frame(DT), Hud("car.x", 0.0, "1"), Hud("car.z", -4.0, "1"), Frame(DT)]);
    o.extend([Hud("cameraMode", 3.0, ""), Frame(DT), Hud("cameraMode", 2.0, ""), Frame(DT), Hud("cameraMode", 0.0, ""), Hud("proximityActive", 0.0, ""), Frame(DT), Hud("proximityActive", 1.0, ""), Hud("car.connected", 0.0, "4"), Frame(DT), Hud("car.z", -30.0, "1"), Hud("car.yaw", 0.0, "0"), Frame(DT)]);
    // the damage picture: a knock at the front, the blinking, every part, the timer
    o.extend([Hud("car.zone0", 30.0, "0"), Frame(DT), Hud("now", 100.0, ""), Frame(DT), Hud("now", 150.0, ""), Frame(0.2), Hud("now", 200.0, ""), Frame(DT), Hud("now", 300.0, ""), Frame(0.4), Frame(DT)]);
    o.extend([Hud("car.zone1", 80.0, "0"), Hud("car.zone2", 15.0, "0"), Hud("car.zone3", -5.0, "0"), Hud("car.engine", 500.0, "0"), Hud("car.gearbox", 0.2, "0"), Hud("now", 0.0, ""), Frame(DT), Hud("car.sus0", 0.3, "0"), Hud("car.sus3", 1.0, "0"), Hud("car.flat1", 0.6, "0"), Hud("car.flat2", 0.1, "0"), Hud("car.tyre1", 0.4, "0"), Frame(DT), Frame(3.0), Frame(3.0), Frame(DT)]);
    o.extend([Hud("car.tyre0", 0.0, "0"), Frame(DT), Frame(5.0), Frame(DT), Hud("car.tyre0", 1.0, "0"), Hud("car.sus3", 0.5, "0"), Frame(DT), Frame(5.0), Frame(DT), Hud("damageShow", 0.0, ""), Frame(DT), Hud("damageEnabled", 0.0, ""), Frame(DT), Hud("damageEnabled", 1.0, ""), Hud("ended", 1.0, ""), Frame(DT), Hud("ended", 0.0, ""), Frame(DT)]);
    o.extend([Hud("focus", 1.0, ""), Hud("car.zone0", 20.0, "1"), Frame(DT), Frame(DT), Hud("focus", 0.0, ""), Hud("car.zone0", 45.0, "0"), Frame(DT), Frame(DT)]);
    o
}

pub fn flags_line(flag: i32, timer: f32, invalid: f32, valid: bool, x_pos: f32, text_pos: f32) -> String {
    format!("hud flags flag={flag} timer={:08x} invalid={:08x} valid={} fuelX={:08x} textPos={:08x}\n", timer.to_bits(), invalid.to_bits(), valid as u8, x_pos.to_bits(), text_pos.to_bits())
}

/// The flags, the three banners, the wrong-way picture and the fuel light.
fn hud_flags(triple: bool) -> Vec<Op> {
    use Op::*;
    let mut o = vec![];
    if triple {
        o.push(Hud("triple", 1.0, ""));
    }
    o.extend([Hud("create", 2.0, ""), Hud("car.name", 0.0, "Tazio Nuvolari"), Hud("car.fuel", 50.0, "0"), Hud("session", 3.0, ""), Hud("raceText", 0.0, "Race  -  Lap 2/5"), Frame(DT)]);
    // every flag, one after the other; the yellow one runs out after five seconds
    o.extend([Hud("setBlueFlag", 0.0, ""), Frame(DT), Hud("setFlag", 4.0, ""), Frame(DT), Frame(3.0), Frame(3.0), Frame(DT), Hud("setFlag", 6.0, ""), Frame(DT), Hud("setFlag", 0.0, ""), Frame(DT)]);
    o.extend([Hud("setFlag", 4.0, ""), Hud("setBlueFlag", 0.0, ""), Frame(DT), Hud("clearFlag", 0.0, ""), Hud("setBlueFlag", 0.0, ""), Frame(DT)]);
    // the gates: a replay, the pause menu, the ESC menu, another car in focus, the results
    for what in ["replay", "pause", "esc", "focus", "ended", "raceOver"] {
        o.extend([Hud(what, 1.0, "300"), Frame(DT), Hud(what, 0.0, "300"), Frame(DT)]);
    }
    // the black flag with its caption, and a yellow one that may not replace it
    o.extend([Hud("blackFlag", 1.0, "You have been disqualified for not clearing the JumpStart penalty"), Frame(DT), Hud("setFlag", 4.0, ""), Frame(DT), Hud("car.pitLaps", 3.0, "0"), Frame(DT), Hud("blackFlag", 0.0, ""), Frame(DT)]);
    // the chequered flag: a race that is over, a qualifying out of time
    o.extend([Hud("raceOver", 1.0, ""), Frame(DT), Frame(DT), Hud("ended", 1.0, ""), Frame(DT), Hud("ended", 0.0, ""), Hud("raceOver", 0.0, ""), Hud("clearFlag", 0.0, ""), Frame(DT)]);
    o.extend([Hud("session", 2.0, ""), Hud("timeLeft", 1000.0, ""), Frame(DT), Hud("timeLeft", -1.0, ""), Frame(DT), Hud("timeLeft", 5000.0, ""), Hud("clearFlag", 0.0, ""), Frame(DT)]);
    // the drive-through lines under the penalty flag
    o.extend([Hud("setFlag", 6.0, ""), Frame(DT), Hud("car.pitLaps", 1.0, "0"), Frame(DT), Hud("pause", 1.0, ""), Frame(DT), Hud("pause", 0.0, ""), Hud("car.pitLaps", 0.0, "0"), Hud("setFlag", 0.0, ""), Frame(DT)]);
    // a cut: five seconds of banner (not in a race), and a message that it clears
    o.extend([Hud("message", 0.0, "Under|the banner"), Frame(DT), Hud("car.valid", 0.0, "0"), Frame(DT), Frame(2.0), Hud("session", 3.0, ""), Frame(2.0), Hud("session", 1.0, ""), Frame(2.0), Frame(2.0), Frame(DT), Hud("car.valid", 1.0, "0"), Frame(DT)]);
    // a penalty in seconds: the long line, the short one, miles
    o.extend([Hud("car.penalty", 12.9, "0"), Frame(DT), Hud("car.clearDisabled", 1.0, "0"), Frame(DT), Hud("car.clearDisabled", 0.0, "0"), Hud("mph", 1.0, ""), Frame(DT), Hud("mph", 0.0, ""), Hud("pause", 1.0, ""), Frame(DT), Hud("pause", 0.0, ""), Hud("replay", 1.0, ""), Hud("car.valid", 0.0, "0"), Frame(DT), Hud("replay", 0.0, ""), Hud("car.valid", 1.0, "0"), Hud("car.penalty", 0.0, "0"), Frame(DT)]);
    // locked controls
    o.extend([Hud("car.lock", 4999.0, "0"), Frame(DT), Hud("car.lock", 20000.0, "0"), Frame(DT), Hud("esc", 1.0, "300"), Frame(DT), Hud("esc", 0.0, "300"), Hud("car.lock", 0.0, "0"), Frame(DT)]);
    // the wrong way picture, not from the track cameras
    o.extend([Hud("wrongWay", 1.0, ""), Frame(DT), Hud("cameraMode", 3.0, ""), Frame(DT), Hud("cameraMode", 0.0, ""), Hud("replayActive", 0.0, ""), Frame(DT), Hud("replayActive", 1.0, ""), Frame(DT), Hud("wrongWay", 0.0, ""), Frame(DT)]);
    // the fuel light: yellow to red, the empty tank, beside a flag, another reserve
    for fuel in [3.0, 2.0, 0.5, 0.0, -1.0] {
        o.extend([Hud("car.fuel", fuel, "0"), Frame(DT)]);
    }
    o.extend([Hud("car.fuel", 1.5, "0"), Hud("setFlag", 6.0, ""), Frame(DT), Hud("clearFlag", 0.0, ""), Frame(DT), Hud("setBlueFlag", 0.0, ""), Frame(DT), Hud("blackFlag", 1.0, ""), Frame(DT), Hud("blackFlag", 0.0, ""), Frame(DT)]);
    o.extend([Hud("fuelLimit", 8.0, ""), Hud("car.fuel", 6.0, "0"), Frame(DT), Hud("esc", 1.0, "300"), Frame(DT), Hud("esc", 0.0, "300"), Hud("raceOver", 1.0, ""), Frame(DT), Hud("raceOver", 0.0, ""), Hud("focus", 1.0, ""), Frame(DT), Hud("focus", 0.0, ""), Frame(DT)]);
    o
}

pub fn message_line(status: i32, timer: f32, session_text: &str, last_valid: bool, invalid_time: f32, car_index: u32) -> String {
    format!("hud message status={status} timer={:08x} session=\"{session_text}\" valid={} invalid={:08x} car={car_index}\n", timer.to_bits(), last_valid as u8, invalid_time.to_bits())
}

pub fn notification_line(status: i32, timer: f32, blink: f32, show: bool) -> String {
    format!("hud notification status={status} timer={:08x} blink={:08x} show={}\n", timer.to_bits(), blink.to_bits(), show as u8)
}

/// The messages of the HUD: the banner, the session's line with its gates, the bar of a car,
/// the two notifications.
fn hud_messages() -> Vec<Op> {
    use Op::*;
    let mut o = vec![
        Hud("create", 3.0, ""),
        Hud("car.name", 0.0, "Tazio Nuvolari"),
        Hud("car.guiName", 0.0, "Ferrari F2004"),
        Hud("car.unixName", 0.0, "ks_ferrari_f2004"),
        Hud("car.name", 1.0, "Rudolf Caracciola"),
        Hud("car.guiName", 1.0, "A car with no logo"),
        Hud("car.unixName", 1.0, "no_such_car"),
        Hud("car.name", 2.0, "Bernd Rosemeyer"),
        Hud("car.guiName", 2.0, "Third car"),
        Hud("car.steerLock", 450.0, "0"),
        Hud("session", 1.0, ""),
        Hud("pqText", 0.0, "Practice  -  Time left 12:34"),
        Hud("raceText", 0.0, "Race  -  Lap 2/5"),
        Frame(DT),
        Frame(DT),
        Hud("message", 8.0, "ABS|Set on level : 2/3"),
        Frame(DT),
        Frame(1.0),
        Hud("message", 9.0, "TC|OFF"),
        Frame(DT),
        Hud("message", 0.0, "A title long enough to run past the line under it, and then some more of it|with a description"),
        Frame(2.0),
        Frame(2.0),
        Frame(2.0),
        Frame(DT),
        Hud("message", 11.0, "Brake Bias|58.5 %"),
        Frame(DT),
        Hud("clear", 0.0, ""),
        Frame(DT),
    ];
    // the bar of a car, in the three kinds of session
    for (session, car, position, last) in [(1, 0, -1, 0), (2, 1, 3, 101234), (3, 2, 1, 83007), (3, 0, -1, 59999), (0, 1, 2, 0)] {
        o.extend([Hud("session", session as f32, ""), Hud("car.pos", position as f32, ["0", "1", "2"][car]), Hud("car.last", last as f32, ["0", "1", "2"][car]), Hud("carMessage", car as f32, "0"), Frame(DT), Frame(DT)]);
    }
    // (a car index equal to the number of cars crashes the game itself: only one past it here)
    o.extend([Hud("carMessage", 4.0, "0"), Frame(DT)]);
    // the ESC menu shows: the message is gone, the bars begin at its right edge
    o.extend([Hud("session", 3.0, ""), Hud("carMessage", 0.0, "0"), Frame(DT), Hud("esc", 1.0, "400"), Frame(DT), Hud("message", 0.0, "Hidden|by the menu"), Frame(DT), Hud("esc", 0.0, "400"), Frame(DT), Hud("message", 0.0, "Shown|again"), Frame(DT), Frame(6.0), Frame(DT)]);
    // the session's line: gone for five seconds after the lap went invalid, with a penalty, locked controls, the pause menu, a replay
    o.extend([Hud("car.valid", 0.0, "0"), Frame(DT), Frame(2.0), Frame(2.0), Frame(2.0), Frame(DT), Hud("car.valid", 1.0, "0"), Frame(DT)]);
    for what in ["car.penalty", "car.lock", "pause", "replay"] {
        o.extend([Hud(what, 1.0, "0"), Frame(DT), Hud(what, 0.0, "0"), Frame(DT)]);
    }
    o.extend([Hud("session", 2.0, ""), Frame(DT), Hud("session", 5.0, ""), Frame(DT), Hud("session", 3.0, ""), Hud("raceText", 0.0, ""), Frame(DT), Hud("raceText", 0.0, "Race  -  Last lap"), Frame(DT)]);
    // the down-shift protection icon: it blinks, and goes after 2.2 seconds
    o.push(Hud("notify", 1.0, ""));
    for _ in 0..30 {
        o.push(Frame(0.1));
    }
    // the wheel is not centred before the start of a race: the warning with its bar, left and right
    o.extend([Hud("tts", 5000.0, ""), Hud("car.steer", 120.0, "0"), Frame(DT), Frame(DT), Hud("car.steer", -300.0, "0"), Frame(DT), Hud("car.steer", 600.0, "0"), Frame(DT), Hud("car.steer", 10.0, "0"), Frame(DT), Hud("car.steer", 95.0, "0"), Frame(DT), Hud("tts", 500.0, ""), Frame(1.0), Frame(1.0), Frame(1.5), Frame(DT)]);
    o.extend([Hud("notify", 2.0, ""), Frame(DT), Hud("esc", 1.0, "300"), Frame(DT), Hud("esc", 0.0, "300"), Frame(DT)]);
    o
}

/// The game's clock (`ksGetTime`, milliseconds) at a frame: both sides are told the same.
pub fn time_ms(frame: usize) -> f64 {
    frame as f64 * (1000.0 / 60.0)
}

fn click(o: &mut Vec<Op>, x: i32, y: i32) {
    o.extend([Op::MouseMove(x, y), Op::MouseDown(x, y), Op::MouseUp(x, y), Op::Frame(DT)]);
}

/// The third set of widgets: a list box with its rows and scroll bar, a graph, spinners
/// (plain, with pictures, with items, mirrored) and the two custom spinners.
fn widgets_c() -> Vec<Op> {
    use Op::*;
    let mut o = vec![
        New("Form", "WIDGETS_C", true), // 1
        Call(1, "title", 0.0, "Lists, graphs, spinners"),
        Size(1, 1000.0, 560.0),
        AddScreen(1, true),
        Visible(1, true),
        Pos(1, 20.0, 90.0),
        New("ListBox", "LIST", false), // 2
        Add(1, 2),
        Pos(2, 20.0, 70.0),
        Size(2, 420.0, 180.0),
        Child(2, 0), // 3: the title row
        Child(3, 0), // 4
        Child(3, 1), // 5
        Child(3, 2), // 6
        Text(4, "Pos"),
        Text(5, "Driver"),
        Text(6, "Best lap"),
        New("Graph", "GRAPH", false), // 7
        Add(1, 7),
        Pos(7, 480.0, 70.0),
        Size(7, 300.0, 150.0),
        Call(7, "addSerie", 1.0, ""),
        Call(7, "addSerie", 6.0, ""),
        Call(7, "fill", 0.8, "0:60"),
        Call(7, "fill", 1.5, "1:150"),
        Call(7, "axis", 0.5, "h"),
        Call(7, "axis", 0.25, "v"),
        New("Spinner", "Camber", false), // 8
        Add(1, 8),
        Call(8, "spPos", 480.0, "290"),
        Size(8, 240.0, 24.0),
        New("Spinner", "Compound", true), // 9
        Add(1, 9),
        Size(9, 260.0, 26.0),
        Call(9, "spPos", 480.0, "360"),
        Call(9, "spItem", 0.0, "Soft"),
        Call(9, "spItem", 0.0, "Medium compound with a long name"),
        Call(9, "spItem", 0.0, "Hard"),
        New("SpinnerMirrored", "Toe", true), // 10
        Add(1, 10),
        Size(10, 260.0, 26.0),
        Call(10, "spPos", 480.0, "430"),
        Call(10, "spUnits", 0.0, "deg"),
        Set(10, "spinner.mult", Val::F(0.1)),
        Set(10, "spinner.min", Val::I(-20)),
        Set(10, "spinner.max", Val::I(20)),
        Set(10, "spinner.step", Val::I(2)),
        Call(10, "spValue", 6.0, ""),
        New("CustomSpinner", "CS_ROUND", false), // 11
        Add(1, 11),
        Pos(11, 800.0, 70.0),
        Size(11, 120.0, 120.0),
        New("CustomSpinner", "CS_BAR", true), // 12
        Add(1, 12),
        Pos(12, 940.0, 70.0),
        Size(12, 30.0, 160.0),
        Set(12, "cspin.bar", Val::F(0.4)),
        Frame(DT),
        Frame(DT),
    ];
    // the list: four items (they fit), then eight more (the scroll bar shows)
    for (i, item) in ["a;1;Ayrton;1:41.2", "b;2;Gilles;1:41.9", "c;3;Juan Manuel;1:42.0", "d;4;Jim;1:42.4"].into_iter().enumerate() {
        o.push(Call(2, "addItem", i as f32, item));
    }
    o.extend([Frame(DT), Frame(DT)]);
    click(&mut o, 100, 205);
    click(&mut o, 100, 205);
    for (i, item) in ["e;5;Niki;1:42.6", "f;6;Alain;1:42.7", "g;7;Jackie;1:43.0", "h;8;Stirling;1:43.3", "i;9;Alberto;1:43.9", "j;10;Nigel;1:44.0", "k;11;Mika;1:44.4", "l;12;Jochen;1:45.1"].into_iter().enumerate() {
        o.push(Call(2, "addItem", (i + 4) as f32, item));
    }
    o.extend([Frame(DT), Frame(DT)]);
    click(&mut o, 100, 235);
    o.extend([Wheel(200, 250, 120.0), Frame(DT), Wheel(200, 250, 120.0), Wheel(200, 250, 120.0), Frame(DT), Wheel(200, 250, -120.0), Frame(DT), Wheel(700, 500, -120.0), Frame(DT)]);
    click(&mut o, 448, 172);
    click(&mut o, 448, 328);
    o.extend([Call(2, "lbSelectName", 0.0, "Jackie"), Frame(DT), Call(2, "lbSelect", 0.0, ""), Frame(DT), Call(2, "lbTop", 0.0, ""), Frame(DT), Call(2, "lbCells", 0.0, ""), Frame(DT), Call(2, "lbCells", 1.0, ""), Call(2, "lbSelect", 11.0, ""), Frame(DT)]);
    // the graph: the top of its range follows the values, a series is emptied, more values than it keeps
    o.extend([Set(7, "graph.auto", Val::B(true)), Call(7, "fill", 3.0, "0:30"), Frame(DT), Call(7, "clearSerie", 1.0, ""), Frame(DT), Set(7, "graph.maxValues", Val::I(40)), Call(7, "fill", 0.5, "1:300"), Frame(DT), Set(7, "graph.minY", Val::F(-4.0)), Frame(DT)]);
    // the plain spinner: plus, minus below the range, the strip, plus held (it repeats after half a second)
    click(&mut o, 728, 392);
    click(&mut o, 728, 392);
    for _ in 0..3 {
        click(&mut o, 512, 392);
    }
    click(&mut o, 620, 406);
    click(&mut o, 700, 406);
    o.extend([MouseMove(728, 392), MouseDown(728, 392)]);
    frames(&mut o, 75);
    o.extend([MouseUp(728, 392), Frame(DT)]);
    o.extend([Set(8, "spinner.homogeneous", Val::B(true)), MouseDown(512, 392)]);
    frames(&mut o, 40);
    o.extend([MouseUp(512, 392), Frame(DT)]);
    // the one with pictures and items: through its items, the strip, arrows in place of plus and minus
    click(&mut o, 747, 463);
    click(&mut o, 747, 463);
    click(&mut o, 747, 463);
    click(&mut o, 513, 463);
    click(&mut o, 630, 478);
    o.extend([Call(9, "spArrows", 1.0, ""), Frame(DT), Call(9, "spArrows", 0.0, ""), Call(9, "spValueItem", 0.0, "Hard"), Frame(DT)]);
    // the mirrored one: a negative value, the strip, no central value, no label, read only
    click(&mut o, 513, 533);
    for _ in 0..5 {
        click(&mut o, 747, 533);
    }
    click(&mut o, 540, 548);
    o.extend([Set(10, "spinner.central", Val::B(false)), Frame(DT), Set(10, "spinner.central", Val::B(true)), Set(10, "spinner.underline", Val::B(false)), Frame(DT), Set(10, "spinner.underline", Val::B(true)), Set(10, "spinner.readOnly", Val::B(true)), Frame(DT)]);
    click(&mut o, 540, 548);
    o.extend([Set(10, "spinner.readOnly", Val::B(false)), Set(10, "spinner.label", Val::B(false)), Frame(DT)]);
    // the help bubble of a spinner: under the mouse on its label, switched by a click on it
    o.extend([Child(8, 4), Call(13, "popTitle", 0.0, "Camber"), Call(13, "popText", 0.0, "Wheel angle.\nMore negative: more grip in corners."), Call(13, "drawPopOver", 1.0, ""), Set(8, "spinner.popover", Val::B(true)), Call(8, "spLabel", 0.0, "Camber LF"), MouseMove(600, 370), Frame(DT), Frame(DT), MouseMove(600, 300), Frame(DT)]);
    click(&mut o, 600, 370);
    o.extend([Frame(DT), Call(13, "drawPopOver", 0.0, "")]);
    // the custom spinners: a held arm clicks every frame; the bar's place
    o.extend([MouseMove(830, 220), MouseDown(830, 220), Frame(DT), Frame(DT), Frame(DT), MouseUp(830, 220), Frame(DT), Set(11, "cspin.press", Val::B(false)), MouseDown(930, 220), Frame(DT), MouseUp(930, 220), Set(12, "cspin.bar", Val::F(1.0)), Frame(DT), Set(12, "cspin.bar", Val::F(0.0)), Frame(DT)]);
    // everything at other sizes, and the rows' font (the GUI's own, so every text changes)
    o.extend([Call(1, "scaleByMult", 1.25, ""), Frame(DT), Frame(DT), Call(1, "scaleByMult", 0.75, ""), Frame(DT), Call(1, "scaleByMult", 1.0, ""), Call(2, "lbFont", 12.0, ""), Frame(DT), Call(2, "lbClear", 0.0, ""), Frame(DT)]);
    o
}

/// The second set of widgets: scroll bars (plain and with pictures), text boxes, text inputs
/// with the keyboard, a pop-over, connected labels, a tab bar and the message banner.
fn widgets_b() -> Vec<Op> {
    use Op::*;
    let mut o = vec![
        New("Form", "WIDGETS_B", true), // 1
        Call(1, "title", 0.0, "More widgets"),
        Size(1, 900.0, 420.0),
        AddScreen(1, true),
        Visible(1, true),
        Pos(1, 40.0, 220.0),
        New("ScrollBar", "SB_PLAIN", false), // 2
        Add(1, 2),
        Pos(2, 20.0, 50.0),
        Size(2, 18.0, 200.0),
        New("ScrollBar", "SB_DRESSED", true), // 3
        Add(1, 3),
        Pos(3, 60.0, 50.0),
        Size(3, 18.0, 200.0),
        Call(3, "sbItems", 40.0, ""),
        Call(3, "sbPerPage", 8.0, ""),
        Call(3, "repeat", 0.1, ""),
        New("TextBox", "TB", false), // 4
        Add(1, 4),
        Pos(4, 100.0, 50.0),
        Size(4, 260.0, 40.0),
        Call(4, "formatted", 0.0, "First line\nSecond line, a longer one\n\nFourth, after an empty one"),
        New("TextBox", "TB_MONO", true), // 5
        Add(1, 5),
        Pos(5, 100.0, 180.0),
        Size(5, 260.0, 30.0),
        Call(5, "formatted", 0.0, "mono 0123\nsecond\n"),
        New("TextInput", "TI", false), // 6
        Add(1, 6),
        Pos(6, 380.0, 50.0),
        Size(6, 220.0, 26.0),
        Text(6, "typed"),
        New("TextInput", "TI2", false), // 7
        Add(1, 7),
        Pos(7, 380.0, 90.0),
        Size(7, 220.0, 26.0),
        New("PopOver", "POP", false), // 8
        Add(7, 8),
        Call(8, "popTitle", 0.0, "Camber"),
        Call(8, "popText", 0.0, "The angle of the wheel seen from the front.\nNegative leans the top inwards.\nA third line"),
        Call(8, "drawPopOver", 1.0, ""),
        New("ConnectedLabel", "Speed", true), // 9
        Add(1, 9),
        Pos(9, 380.0, 280.0),
        Size(9, 200.0, 20.0),
        Call(9, "connect", 123.4567, ""),
        Call(9, "suffix", 0.0, " km/h"),
        New("ConnectedLabel", "Gear", false), // 10
        Add(1, 10),
        Pos(10, 380.0, 310.0),
        Size(10, 200.0, 20.0),
        Call(10, "connect", 4.0, ""),
        New("Form", "TAB_FORM", false), // 11
        Size(11, 1000.0, 120.0),
        AddScreen(11, true),
        Visible(11, true),
        Pos(11, 0.0, 0.0),
        New("TabBar", "TABS", false), // 12
        Add(11, 12),
        Call(12, "addTab", 0.0, "Generic"),
        Call(12, "addTab", 0.0, "Tyres"),
        Call(12, "addTab", 0.0, "Electronics"),
        Call(12, "addTab", 0.0, "Fuel"),
        Call(12, "addTab", 0.0, "Aero"),
        New("DefaultSystemMessage", "", false), // 13
        AddScreen(13, false),
        Pos(13, 0.0, 130.0),
        Size(13, 1280.0, 80.0),
        Call(13, "message", 0.0, "Traction Control|Level 3 of 8"),
        Frame(DT),
        Frame(DT),
    ];
    // the plain scroll bar: minus and plus (minus at 0 wraps to the end), the wheel
    for _ in 0..3 {
        click(&mut o, 68, 278);
    }
    for _ in 0..4 {
        click(&mut o, 68, 460);
    }
    o.extend([Wheel(68, 350, 120.0), Frame(DT), Wheel(68, 350, -120.0), Wheel(68, 350, -120.0), Frame(DT)]);
    // the one with pictures: plus held for a second and a half (it repeats), then its thumb dragged
    o.extend([MouseMove(108, 460), MouseDown(108, 460)]);
    frames(&mut o, 90);
    o.extend([MouseUp(108, 460), Frame(DT)]);
    o.extend([MouseMove(108, 330), MouseDown(108, 330), Frame(DT)]);
    for y in [336, 342, 350, 370, 330, 300] {
        o.extend([MouseMove(108, y), Frame(DT)]);
    }
    o.extend([MouseUp(108, 300), Frame(DT), Call(3, "sbValue", 39.0, ""), Frame(DT), Call(3, "sbValue", 2.0, ""), Frame(DT)]);
    // the text input: a click gives it the keyboard; letters, a backspace, more than it holds, Enter
    click(&mut o, 430, 280);
    for (i, c) in "Ab1 x".chars().enumerate() {
        o.push(KeyChar(c as u32));
        if i % 2 == 0 {
            o.push(Frame(DT));
        }
    }
    o.extend([KeyChar(8), KeyChar(8), Frame(DT), KeyDown(0x25), KeyChar(0x1b), KeyChar(0xe9), Frame(DT)]);
    // the caret: half a second on, half a second off
    frames(&mut o, 62);
    for _ in 0..70 {
        o.push(KeyChar('w' as u32));
    }
    o.extend([Frame(DT), KeyChar(13), Frame(DT), KeyChar('z' as u32), Frame(DT)]);
    // the second input takes the keyboard, a click elsewhere takes it away
    click(&mut o, 430, 320);
    o.extend([KeyChar('q' as u32), Frame(DT)]);
    click(&mut o, 700, 600);
    o.extend([KeyChar('q' as u32), Frame(DT)]);
    // the text box grows with its text; one line too long ends the text
    o.extend([Call(4, "formatted", 0.0, "one\ntwo\nthree\nfour\nfive\nsix\nseven"), Frame(DT), Frame(DT), Call(8, "popText", 0.0, "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n14"), Frame(DT), Frame(DT), Frame(DT)]);
    // the connected labels: rounding at a tie, precisions, a negative number
    for (value, precision) in [(0.0625, 3.0), (2.5, 0.0), (-0.125, 2.0), (1234567.9, 1.0), (0.0004, 3.0)] {
        o.extend([Call(9, "connect", value, ""), Call(9, "precision", precision, ""), Call(10, "connect", value * 100.0, ""), Frame(DT)]);
    }
    o.extend([Call(9, "labelText", 0.0, "Top speed"), Call(9, "suffix", 0.0, ""), Frame(DT)]);
    // the tab bar: the mouse over a tab, a click on the second and the third (they slide), the arrows round
    o.extend([MouseMove(600, 15), Frame(DT)]);
    click(&mut o, 600, 15);
    frames(&mut o, 8);
    click(&mut o, 800, 15);
    frames(&mut o, 8);
    for _ in 0..4 {
        click(&mut o, 950, 15);
    }
    frames(&mut o, 10);
    for _ in 0..2 {
        click(&mut o, 100, 15);
    }
    frames(&mut o, 10);
    o.extend([Call(12, "orderTabs", 0.0, ""), Frame(DT), Frame(DT)]);
    click(&mut o, 100, 15);
    frames(&mut o, 10);
    // the banner: another message, a long one, an empty description
    o.extend([Call(13, "message", 0.0, "ABS|Off"), Frame(DT), Call(13, "message", 0.0, "A title that is long enough to be wider than the line under it|and a description"), Frame(DT), Call(13, "message", 0.0, "Alone|"), Frame(DT)]);
    // all of the first form at another size
    o.extend([Call(1, "scaleByMult", 1.25, ""), Frame(DT), Frame(DT)]);
    o
}

fn frames(ops: &mut Vec<Op>, count: usize) {
    for _ in 0..count {
        ops.push(Op::Frame(DT));
    }
}

/// Plain controls and labels: colours, frames, text in the three alignments, children, a
/// highlighted text under the mouse, a held control that repeats its click, a scaled tree.
fn base() -> Vec<Op> {
    use Op::*;
    let mut o = vec![
        New("Control", "PANEL", false), // 1
        AddScreen(1, false),
        Pos(1, 100.0, 120.0),
        Size(1, 400.0, 220.0),
        Text(1, "A panel: Control"),
        New("Label", "First label", false), // 2
        Add(1, 2),
        Pos(2, 10.0, 40.0),
        Size(2, 200.0, 24.0),
        New("Control", "RIGHT", false), // 3
        Add(1, 3),
        Pos(3, 10.0, 80.0),
        Size(3, 380.0, 30.0),
        Text(3, "right aligned 0123456789"),
        Set(3, "fontAlign", Val::I(1)),
        Set(3, "backColor", Val::C([0.1, 0.2, 0.6, 0.9])),
        Set(3, "borderColor", Val::C([1.0, 0.8, 0.0, 1.0])),
        New("Control", "LEFT", false), // 4
        Add(1, 4),
        Pos(4, 10.0, 120.0),
        Size(4, 380.0, 30.0),
        Text(4, "left aligned, highlighted under the mouse"),
        Set(4, "fontAlign", Val::I(0)),
        Set(4, "highlight", Val::B(true)),
        Set(4, "highlightTextColor", Val::C([1.0, 0.0, 0.0, 1.0])),
        Set(4, "drawBorder", Val::B(false)),
        New("Label", "A label whose text is far too long for what it may show", false), // 5
        Add(1, 5),
        Pos(5, 10.0, 160.0),
        Size(5, 380.0, 24.0),
        Set(5, "label.maxChars", Val::I(18)),
        Set(5, "foreColor", Val::C([0.5, 1.0, 0.5, 0.75])),
        New("Control", "REPEATER", false), // 6
        AddScreen(6, false),
        Pos(6, 600.0, 120.0),
        Size(6, 120.0, 40.0),
        Text(6, "hold me"),
        Set(6, "repeatInterval", Val::F(0.25)),
        New("Control", "INNER", false), // 7
        Add(6, 7),
        Pos(7, 4.0, 22.0),
        Size(7, 60.0, 14.0),
        Set(7, "backColor", Val::C([0.0, 0.0, 0.0, 1.0])),
        Set(7, "fontScale", Val::F(0.6)),
        Text(7, "child"),
        Frame(DT),
        Frame(DT),
        MouseMove(200, 250),
        Frame(DT),
        MouseMove(50, 50),
        Frame(DT),
        MouseMove(640, 135),
        MouseDown(640, 135),
    ];
    // held for a second and a half: the click repeats
    frames(&mut o, 90);
    o.push(MouseUp(640, 135));
    frames(&mut o, 2);
    // clicks that hit a child and its parent, and one that hits nothing
    o.extend([MouseDown(110, 205), MouseUp(110, 205), Frame(DT), MouseDown(5, 5), MouseUp(5, 5), Frame(DT)]);
    // the panel and all in it at one and a half times the size, then at a half
    o.extend([Call(1, "scaleByMult", 1.5, ""), Frame(DT), Call(1, "scaleByMult", 0.5, ""), Frame(DT), Call(1, "scaleByMult", 1.0, ""), Frame(DT)]);
    // hidden, moved, shown
    o.extend([Visible(3, false), Frame(DT), Pos(1, 300.0, 300.0), Visible(3, true), Frame(DT), Visible(1, false), Frame(DT)]);
    o
}

/// Forms: one that can be scaled and one that cannot, the buttons that show under the mouse,
/// dragging (over another form: red, and back), pinning, zooming, the app bar sliding in, its
/// icons and their titles, the four desktops, "show desktop", and the layout file written.
fn forms() -> Vec<Op> {
    use Op::*;
    let mut o = vec![
        New("Form", "ALPHA", true), // 1
        Call(1, "title", 0.0, "Alpha app"),
        Size(1, 260.0, 160.0),
        AddScreen(1, true),
        New("Label", "Speed: 123 km/h", false), // 2
        Add(1, 2),
        Pos(2, 12.0, 40.0),
        Size(2, 200.0, 22.0),
        New("Form", "BETA", false), // 3
        Call(3, "title", 0.0, "Beta, not scalable"),
        Size(3, 300.0, 120.0),
        AddScreen(3, true),
        New("Form", "GAMMA", true), // 4
        Call(4, "title", 0.0, "Gamma"),
        Size(4, 180.0, 90.0),
        AddScreen(4, true),
        New("Form", "HIDDEN_DEV", true), // 5: not on the bar
        Size(5, 100.0, 50.0),
        AddScreen(5, false),
        Frame(DT),
        Frame(DT),
    ];
    // the mouse over the first form: pin and zoom show; a click on zoom in, twice, zoom out once
    o.extend([MouseMove(200, 200), Frame(DT)]);
    o
}

fn forms_more(o: &mut Vec<Op>) {
    use Op::*;
    // everything visible to begin with, wherever the layout file put it
    for id in [1, 3, 4] {
        o.push(Visible(id, true));
    }
    o.extend([Pos(1, 120.0, 140.0), Pos(3, 520.0, 300.0), Pos(4, 60.0, 420.0), Frame(DT)]);
    // hover the first form, press its zoom buttons (above its top edge, right to left: pin, out, in)
    o.extend([MouseMove(200, 200), Frame(DT)]);
    for (x, n) in [(120 + 260 - 1 - 26 - 26 - 13, 2), (120 + 260 - 1 - 26 - 13, 1)] {
        for _ in 0..n {
            o.extend([MouseMove(x, 126), MouseDown(x, 126), MouseUp(x, 126), Frame(DT)]);
        }
    }
    // drag it by its body, over the second form (red), let go there (it goes back), drag clear
    o.extend([MouseMove(200, 200), MouseDown(200, 200), Frame(DT)]);
    for step in 1..=12 {
        o.extend([MouseMove(200 + step * 30, 200 + step * 12), Frame(DT)]);
    }
    o.extend([MouseUp(560, 344), Frame(DT), Frame(DT)]);
    o.extend([MouseMove(200, 200), MouseDown(200, 200)]);
    for step in 1..=8 {
        o.extend([MouseMove(200 + step * 10, 200 - step * 20), Frame(DT)]);
    }
    o.extend([MouseUp(280, 40), Frame(DT)]);
    // pin the second form (its pin is inside, top right), then try to drag it
    o.extend([MouseMove(700, 320), Frame(DT), MouseMove(520 + 300 - 33 + 10, 300 + 10), MouseDown(520 + 300 - 33 + 10, 300 + 10), MouseUp(520 + 300 - 33 + 10, 300 + 10), Frame(DT)]);
    o.extend([MouseMove(600, 380), MouseDown(600, 380), MouseMove(640, 420), Frame(DT), MouseMove(700, 480), Frame(DT), MouseUp(700, 480), Frame(DT)]);
    // auto-hide on the pinned form: frame and title only under the mouse
    o.extend([Call(3, "autohide", 1.0, ""), Frame(DT), MouseMove(100, 700), Frame(DT), MouseMove(600, 380), Frame(DT)]);
    // the logo closes a form
    o.extend([MouseMove(70, 430), MouseDown(70, 430), MouseUp(70, 430), Frame(DT)]);
    // the app bar: the mouse at the right edge calls it in; it slides for a quarter of a second
    o.push(MouseMove(1270, 300));
    frames(o, 24);
    // over an icon: its title slides out; click: the form shows again; again: it hides
    o.push(MouseMove(1250, 60));
    frames(o, 22);
    o.extend([MouseDown(1250, 60), MouseUp(1250, 60), Frame(DT), Frame(DT), MouseMove(1250, 130), Frame(DT), Frame(DT), MouseDown(1250, 130), MouseUp(1250, 130), Frame(DT)]);
    // the wheel on the bar, the scroll buttons
    o.extend([Wheel(1250, 300, -120.0), Frame(DT), Wheel(1250, 300, 120.0), Frame(DT), MouseMove(1250, 10), MouseDown(1250, 10), MouseUp(1250, 10), Frame(DT), MouseMove(1250, 700), MouseDown(1250, 700), MouseUp(1250, 700), Frame(DT)]);
    // the desktops: the second button, the fourth, back to the first
    for x in [1216 + 18 + 7, 1216 + 48 + 7, 1216 + 2 + 7] {
        o.extend([MouseMove(x, 686), MouseDown(x, 686), MouseUp(x, 686), Frame(DT), Frame(DT)]);
    }
    // the key's way round the desktops
    o.extend([Call(0, "cycleDesktop", 0.0, ""), Frame(DT), Call(0, "cycleDesktop", 0.0, ""), Frame(DT), Call(0, "cycleDesktop", 0.0, ""), Call(0, "cycleDesktop", 0.0, ""), Frame(DT)]);
    // the mouse leaves: the bar slides out
    o.push(MouseMove(400, 600));
    frames(o, 24);
    // "show desktop": everything hides, and comes back
    o.extend([Call(0, "showDesktop", 0.0, ""), Frame(DT), Call(0, "showDesktop", 0.0, ""), Frame(DT)]);
    o.push(Save);
}

/// The first widgets: progress bars both ways, buttons in their states, check boxes, the
/// replay's slider with its markers.
fn widgets_a() -> Vec<Op> {
    use Op::*;
    let mut o = vec![
        New("Form", "WIDGETS", true), // 1
        Call(1, "title", 0.0, "Widgets"),
        Size(1, 700.0, 420.0),
        AddScreen(1, true),
        Visible(1, true),
        Pos(1, 60.0, 120.0),
        New("ProgressBar", "PB_V", false), // 2
        Add(1, 2),
        Pos(2, 20.0, 50.0),
        Size(2, 20.0, 120.0),
        Set(2, "progress.value", Val::F(0.3)),
        New("ProgressBar", "PB_H", false), // 3
        Add(1, 3),
        Pos(3, 60.0, 50.0),
        Size(3, 200.0, 18.0),
        Set(3, "progress.vertical", Val::B(false)),
        Set(3, "progress.value", Val::F(0.8)),
        New("ProgressBar", "PB_HI", false), // 4
        Add(1, 4),
        Pos(4, 60.0, 80.0),
        Size(4, 200.0, 18.0),
        Set(4, "progress.vertical", Val::B(false)),
        Set(4, "progress.inverted", Val::B(true)),
        Set(4, "progress.max", Val::F(200.0)),
        Set(4, "progress.min", Val::F(-100.0)),
        Set(4, "progress.value", Val::F(250.0)),
        Text(4, "over the top"),
        New("ActiveButton", "BUTTON_PLAIN", false), // 5
        Add(1, 5),
        Pos(5, 60.0, 120.0),
        Size(5, 120.0, 30.0),
        Text(5, "Plain button"),
        New("ActiveButton", "BUTTON_TEX", false), // 6
        Add(1, 6),
        Pos(6, 200.0, 120.0),
        Size(6, 40.0, 40.0),
        Call(6, "multipleTexture", 0.0, "content/gui/controls/form/pin_icn"),
        New("ActiveButton", "BUTTON_OFF", false), // 7
        Add(1, 7),
        Pos(7, 260.0, 120.0),
        Size(7, 120.0, 30.0),
        Text(7, "Inactive"),
        Set(7, "button.isActive", Val::B(false)),
        New("ActiveButton", "BUTTON_MISSING", false), // 8
        Add(1, 8),
        Pos(8, 400.0, 120.0),
        Size(8, 40.0, 40.0),
        Call(8, "multipleTexture", 0.0, "content/gui/controls/form/there_is_no_such"),
        Text(8, "blank"),
        Set(8, "button.blanked", Val::B(true)),
        New("CheckBox", "Check me", false), // 9
        Add(1, 9),
        Pos(9, 60.0, 180.0),
        New("CheckBox", "Checked", false), // 10
        Add(1, 10),
        Pos(10, 60.0, 215.0),
        Call(10, "setCheck", 1.0, ""),
        New("Slider", "SLIDER", true), // 11
        Add(1, 11),
        Pos(11, 60.0, 290.0),
        Size(11, 500.0, 12.0),
        Set(11, "slider.value", Val::F(0.35)),
        New("Slider", "SLIDER_BARE", false), // 12
        Add(1, 12),
        Pos(12, 60.0, 340.0),
        Size(12, 300.0, 10.0),
        Frame(DT),
        Frame(DT),
    ];
    // hover the buttons, select one, click the check boxes (the box, then the label)
    for (x, y) in [(180, 255), (280, 260), (380, 255), (480, 260)] {
        o.extend([MouseMove(x, y), Frame(DT)]);
    }
    o.extend([Call(5, "setSelected", 1.0, ""), Frame(DT), Call(5, "setSelected", 0.0, ""), Call(6, "setSelected", 1.0, ""), Frame(DT)]);
    o.extend([MouseMove(130, 312), MouseDown(130, 312), MouseUp(130, 312), Frame(DT), MouseDown(130, 312), MouseUp(130, 312), Frame(DT), MouseMove(200, 312), MouseDown(200, 312), MouseUp(200, 312), Frame(DT), MouseMove(128, 345), MouseDown(128, 345), MouseUp(128, 345), Frame(DT)]);
    o.extend([Text(9, "A new text for the box"), Frame(DT)]);
    // the slider: a click moves the time, the markers are dragged, the cut is told
    o.extend([MouseMove(300, 415), MouseDown(300, 415), MouseUp(300, 415), Frame(DT)]);
    o.extend([MouseMove(112, 414), MouseDown(112, 414), MouseMove(200, 414), Frame(DT), MouseMove(260, 420), Frame(DT), MouseUp(260, 420), Frame(DT)]);
    o.extend([MouseMove(626, 414), MouseDown(626, 414), MouseMove(500, 414), Frame(DT), MouseMove(100, 414), Frame(DT), MouseUp(100, 414), Frame(DT)]);
    o.extend([Call(11, "markerLeft", 0.2, ""), Call(11, "markerRight", 0.9, ""), Frame(DT), Call(11, "updateMarkers", 0.0, ""), Frame(DT), Call(11, "resetMarkers", 0.0, ""), Frame(DT)]);
    // all of it scaled by the form's zoom
    o.extend([Call(1, "scaleByMult", 1.25, ""), Frame(DT), Call(1, "scaleByMult", 0.75, ""), Frame(DT)]);
    o
}

/// Text alone: both families, bold and italic, sizes from tiny to huge, the three alignments,
/// a shadow, a long string, characters beyond Latin.
fn text() -> Vec<Op> {
    use Op::*;
    let white = [1.0, 1.0, 1.0, 1.0];
    let mut o = vec![
        Font(0, 1.0, false, false), // 0
        Font(1, 1.0, false, false), // 1
        Font(0, 1.0, false, true),  // 2
        Font(0, 1.0, true, false),  // 3
        Font(1, 1.0, true, true),   // 4
        Font(0, 2.0, false, false), // 5: a font with a scale of its own
    ];
    let line = "Sphinx of black quartz, judge my vow 0123456789 AVWAT fi ffl";
    for (i, size) in [6.0f32, 9.0, 12.0, 16.0, 22.0, 30.0, 44.0].into_iter().enumerate() {
        o.push(Blit(i % 5, 20.0, 20.0 + 48.0 * i as f32, line, size, 0, [1.0, 1.0 - 0.1 * i as f32, 0.4, 1.0], false));
    }
    o.extend([Frame(DT), ClearText]);
    o.extend([
        Blit(0, 640.0, 40.0, "left of the point", 20.0, 0, white, false),
        Blit(0, 640.0, 70.0, "right of the point", 20.0, 1, white, false),
        Blit(0, 640.0, 100.0, "centred on the point", 20.0, 2, white, false),
        Blit(2, 100.0, 160.0, "A shadow under bold text", 36.0, 0, [1.0, 0.9, 0.2, 1.0], true),
        Blit(1, 100.0, 220.0, "half transparent, with its shadow", 24.0, 0, [0.4, 0.8, 1.0, 0.5], true),
        Blit(5, 100.0, 270.0, "a font with scale 2 at size 14", 14.0, 0, white, false),
        Blit(0, 20.0, 330.0, "A very long string that runs off the right edge of the screen without being wrapped anywhere, because the game draws every string with word wrap switched off, however long it is: 0123456789 0123456789 0123456789 0123456789 0123456789", 18.0, 0, white, false),
        Blit(0, 20.0, 380.0, "Wide: \u{e0}\u{e8}\u{ec}\u{f2}\u{f9} \u{c4}\u{d6}\u{dc}\u{df} \u{f1} \u{20ac} \u{3a9}\u{3c0} \u{416}\u{434}\u{44f} \u{65e5}\u{672c}\u{8a9e} \u{d55c}\u{ae00} \u{2603}", 28.0, 0, white, false),
        Blit(3, 20.0, 440.0, "Italic \u{2014} with a dash, \u{201c}quotes\u{201d} and an ellipsis\u{2026}", 26.0, 0, white, false),
        Blit(4, 20.0, 500.0, "Consolas bold italic 0O1lI| {}[]()<>", 26.0, 0, [0.6, 1.0, 0.6, 1.0], false),
        Blit(0, 20.0, 560.0, "", 26.0, 0, white, false),
        Blit(0, 20.0, 600.0, "two\nlines", 20.0, 0, white, false),
        Frame(DT),
        Frame(DT),
    ]);
    o
}

pub fn sequence(name: &str) -> Result<Sequence, String> {
    let plain = |ops: Vec<Op>| Sequence { ops, cfg: &[], acos: None, allow_overlapping: false };
    Ok(match name {
        "base" => plain(base()),
        "text" => plain(text()),
        "widgets_a" => plain(widgets_a()),
        "widgets_b" => plain(widgets_b()),
        "widgets_c" => plain(widgets_c()),
        "hud_messages" => plain(hud_messages()),
        "hud_flags" => plain(hud_flags(false)),
        "hud_flags_triple" => plain(hud_flags(true)),
        "hud_cars" => plain(hud_cars()),
        // the plain tags, bigger, the focused car's too, a shorter sight; the damage in numbers
        "hud_cars_simple" => Sequence {
            ops: hud_cars(),
            cfg: &[("name_displayer.ini", "[MAIN]\nMAX_DISTANCE=60\nFONT_COLOR=1,1,1\nDRAW_FOCUSEDCAR=1\nSCALE=1.5\nSIMPLE_LABEL=1\n"), ("damage_displayer.ini", "[MAIN]\nTIME=2\nMAX_DAMAGE_KMH=40\nPOSITION_X=300\nDISTANCE_FROM_CENTER_Y=50\nPRINT_VALUES=1\n")],
            acos: None,
            allow_overlapping: false,
        },
        // the tags without a flag, as big as they get
        "hud_cars_noflag" => Sequence { ops: hud_cars(), cfg: &[("name_displayer.ini", "[MAIN]\nMAX_DISTANCE=100\nFONT_COLOR=1,1,1\nRENDER_FLAG=0\nSCALE=9\nSIMPLE_LABEL=0\n")], acos: None, allow_overlapping: false },
        // forms as a first session finds them (no layout file)
        "forms" => {
            let mut ops = forms();
            forms_more(&mut ops);
            plain(ops)
        }
        // … as a layout file left them, on its second desktop
        "forms_layout" => {
            let mut ops = forms();
            ops.extend([Op::Frame(DT), Op::Call(0, "cycleDesktop", 0.0, ""), Op::Frame(DT), Op::Call(0, "cycleDesktop", 0.0, ""), Op::Frame(DT), Op::Save]);
            Sequence { ops, cfg: &[], acos: Some(ACOS_SAMPLE), allow_overlapping: false }
        }
        // … and with forms allowed over each other
        "forms_overlap" => {
            let mut ops = forms();
            forms_more(&mut ops);
            Sequence { ops, cfg: &[], acos: None, allow_overlapping: true }
        }
        "pit_menu" => plain(crate::gui_seq_menu::pit_menu(false, false)),
        "pit_menu_small" => plain(crate::gui_seq_menu::pit_menu(true, false)),
        "pit_menu_lane" => plain(crate::gui_seq_menu::pit_menu(false, true)),
        // other looks: top left, bold, bigger, one preset more, other colours; no coming up by itself
        "pit_menu_looks" => Sequence {
            ops: crate::gui_seq_menu::pit_menu(false, true),
            cfg: &[("pitstop.ini", "[SETTINGS]
USE_MOUSE_PITSTOP=0
VISIBILITY_MAX_TIME=2
STAY_IN_CAR=1
AUTO_APP_ON_PITLANE=0
PRESETS_COUNT=4
SIZE_MULT=1.5
FONT_IS_BOLD=1
HEADER_FONT=11
HEADER_HEIGHT=19
OBJECT_POS_X=33
OBJECT_POS_Y=47
FORM_ALIGNMENT_V=0
FORM_ALIGNMENT_H=0
ITEM_HEIGHT=18
ITEM_TITLE_WIDTH=141
ITEM_VALUE_WIDTH=77
ITEM_FOREGROUND_HEADER=1,1,0,1
ITEM_BACKGROUND_HEADER=0,0,0.5,0.9
ITEM_FOREGROUND_UNSELECTED=0.9,0.9,0.9,1
ITEM_BACKGROUND_UNSELECTED=0.2,0.2,0.2,0.6
ITEM_FOREGROUND_SELECTED=0,0,0,1
ITEM_BACKGROUND_SELECTED=0,0.8,0.2,1
")],
            acos: None,
            allow_overlapping: false,
        },
        // … at the bottom, in the middle; and a file with nothing in it
        "pit_menu_bottom" => Sequence { ops: crate::gui_seq_menu::pit_menu(true, true), cfg: &[("pitstop.ini", "[SETTINGS]
VISIBILITY_MAX_TIME=5
AUTO_APP_ON_PITLANE=1
PRESETS_COUNT=2
SIZE_MULT=1
HEADER_FONT=9
HEADER_HEIGHT=16
OBJECT_POS_X=10
OBJECT_POS_Y=20
FORM_ALIGNMENT_V=2
FORM_ALIGNMENT_H=1
ITEM_HEIGHT=20
ITEM_TITLE_WIDTH=130
ITEM_VALUE_WIDTH=80
ITEM_FOREGROUND_HEADER=0,0,0,1
ITEM_BACKGROUND_HEADER=1,1,1,1
ITEM_FOREGROUND_UNSELECTED=1,1,1,1
ITEM_BACKGROUND_UNSELECTED=0,0,0,0.5
ITEM_FOREGROUND_SELECTED=1,1,1,1
ITEM_BACKGROUND_SELECTED=0.74,0,0,1
")], acos: None, allow_overlapping: false },
        "pit_stop" => plain(crate::gui_seq_menu::pit_stop()),
        "pause_menu" => plain(crate::gui_seq_menu::pause_menu(0)),
        "pause_menu_online" => plain(crate::gui_seq_menu::pause_menu(1)),
        "pause_menu_race" => plain(crate::gui_seq_menu::pause_menu(2)),
        "pause_menu_plain" => plain(crate::gui_seq_menu::pause_menu(3)),
        other => return Err(format!("no GUI sequence {other:?} (pause_menu, pause_menu_online, pause_menu_race, pause_menu_plain, pit_menu, pit_menu_small, pit_menu_lane, pit_menu_looks, pit_menu_bottom, pit_stop, base, text, forms, forms_layout, forms_overlap, widgets_a, widgets_b, widgets_c, hud_messages, hud_flags, hud_flags_triple, hud_cars, hud_cars_simple, hud_cars_noflag)")),
    })
}

/// What one side found, for the comparison.
#[derive(Default)]
pub struct Outcome {
    /// a line a frame: calls, draws, the hashes of the command log and of the picture
    pub frames: Vec<String>,
    /// the state of every control after every frame, and the events since the frame before
    pub state: String,
    /// the layout file of every `Save`
    pub saved: Vec<Vec<u8>>,
}

/// What both sides write down of a control (floats as their bits).
pub struct ControlState {
    pub class: &'static str,
    pub name: String,
    pub rect: [f32; 4],
    pub rect_base: [f32; 4],
    pub visible: bool,
    pub is_mouse_down: bool,
    pub highlight: bool,
    pub is_highlight: bool,
    pub draw_border: bool,
    pub draw_background: bool,
    pub text: String,
    pub font_scale: f32,
    pub scale_mult: f32,
    pub font_align: i32,
    /// back, border, fore, highlighted text, back texture
    pub colors: [[f32; 4]; 5],
    pub has_texture: bool,
    pub repeat: (f32, f32, bool),
    /// what the class adds
    pub extra: String,
}

pub fn bits4(v: &[f32; 4]) -> String {
    format!("{:08x},{:08x},{:08x},{:08x}", v[0].to_bits(), v[1].to_bits(), v[2].to_bits(), v[3].to_bits())
}

pub fn control_line(depth: usize, c: &ControlState) -> String {
    format!(
        "{}{} \"{}\" r={} rb={} vis={} md={} hl={}/{} draw={}{} text=\"{}\" fs={:08x} sm={:08x} al={} back={} border={} fore={} hlt={} btc={} tex={} rep={:08x},{:08x},{}{}
",
        "  ".repeat(depth),
        c.class,
        c.name,
        bits4(&c.rect),
        bits4(&c.rect_base),
        c.visible as u8,
        c.is_mouse_down as u8,
        c.highlight as u8,
        c.is_highlight as u8,
        c.draw_border as u8,
        c.draw_background as u8,
        c.text,
        c.font_scale.to_bits(),
        c.scale_mult.to_bits(),
        c.font_align,
        bits4(&c.colors[0]),
        bits4(&c.colors[1]),
        bits4(&c.colors[2]),
        bits4(&c.colors[3]),
        bits4(&c.colors[4]),
        c.has_texture as u8,
        c.repeat.0.to_bits(),
        c.repeat.1.to_bits(),
        c.repeat.2 as u8,
        c.extra,
    )
}
