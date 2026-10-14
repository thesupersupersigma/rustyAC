// SPDX-License-Identifier: GPL-3.0-or-later

//! The ESC menu's page "Telemetry" (`ComparativeTelemetry`, Task 33) in the GUI oracle: the
//! script, the game's side (a `ComparativeTelemetryRecorder` among `Sim`'s game objects with
//! the script's laps, the track and the car the opponents' files are held against) and the
//! port's (`SimLink::telemetry`).
//!
//! The laps are the user's own `.tc` files of Monza in the F2004 and of Spa (read only, copied
//! into the scratch root): the best and the last lap, and three opponents' files of which two
//! are of this track, layout and car.

use std::sync::Mutex;

use rustyac_gui::comparative_telemetry::{load_ct_opponent, TelemetryLink};
use rustyac_physics::replay::telemetry::CtData;

use crate::ac::{rd, wr, write_wstring, Game};
use crate::gui_seq::Op;
use crate::gui_seq_menu::hud;

const DT: f32 = 1.0 / 60.0;
/// `const ComparativeTelemetryRecorder::vftable'`
const VA_RECORDER_VFTABLE: usize = 0x1_404b_02e0;
const VA_GET_AI_SPLINE: usize = 0x1_401c_8010;
const VA_SPEED_USE_MPH: usize = 0x1_4155_a5ae;
const TRACK: &str = "monza";
const CAR: &str = "ks_ferrari_f2004";

#[derive(Default)]
struct TeleScript {
    active: bool,
    max_gear: i32,
    length: f32,
    best: (u32, Vec<CtData>),
    last: (u32, Vec<CtData>),
    mph: bool,
    /// the game's blocks: the recorder, the AI spline
    recorder: usize,
    spline: usize,
}

static SCRIPT: Mutex<Option<TeleScript>> = Mutex::new(None);

fn script<T>(f: impl FnOnce(&mut TeleScript) -> T) -> T {
    f(SCRIPT.lock().unwrap().get_or_insert_with(TeleScript::default))
}

/// The page is the game's own in this sequence (else the stand-in form of Task 30).
pub fn active() -> bool {
    script(|s| s.active)
}

/// A `.tc` file's time and lap.
fn read_tc(path: &std::path::Path) -> Result<(u32, Vec<CtData>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut at = 4usize;
    for _ in 0..4 {
        let len = i32::from_le_bytes(bytes.get(at..at + 4).ok_or("short .tc")?.try_into().map_err(|_| "short .tc")?);
        at += 4 + len.max(0) as usize;
    }
    let time = u32::from_le_bytes(bytes.get(at..at + 4).ok_or("short .tc")?.try_into().map_err(|_| "short .tc")?);
    Ok((time, load_ct_opponent(path)))
}

fn folder() -> std::path::PathBuf {
    crate::root::documents().join("Assetto Corsa/ctelemetry")
}

/// A script's change of the page's session (true: it was one).
pub fn input(what: &str, number: f32, text: &str) -> Result<bool, String> {
    match what {
        "tele.setup" => script(|s| {
            s.active = true;
            s.max_gear = number as i32;
            s.best = (u32::MAX, Vec::new());
            s.last = (u32::MAX, Vec::new());
        }),
        "tele.length" => script(|s| s.length = number),
        "tele.best" => {
            let lap = read_tc(&folder().join(text))?;
            script(|s| s.best = lap);
        }
        "tele.last" => {
            let lap = read_tc(&folder().join(text))?;
            script(|s| s.last = lap);
        }
        "tele.mph" => script(|s| s.mph = number != 0.0),
        _ => return Ok(false),
    }
    Ok(true)
}

/// The user's laps in the scratch root's Documents: `best.tc`, `best2.tc` (the same lap 0.8 s
/// faster), `last.tc` (a lap of Spa); the opponents: the Monza lap as it is, as "Rival" with
/// another time, the Spa lap (not of this track: not listed).
pub fn prepare_root(args: &crate::Args, name: &str) -> Result<(), String> {
    if !name.starts_with("esc_telemetry") {
        return Ok(());
    }
    let documents = dirs_documents().ok_or("no Documents folder")?;
    let player = documents.join("Assetto Corsa/ctelemetry/player");
    let monza = std::fs::read(player.join("_monza__.tc")).map_err(|e| format!("_monza__.tc: {e}"))?;
    let spa = std::fs::read(player.join("_spa__.tc")).map_err(|e| format!("_spa__.tc: {e}"))?;
    let out = args.root.join("documents/Assetto Corsa/ctelemetry");
    let opponents = out.join("opponents");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&opponents).map_err(|e| e.to_string())?;
    let retime = |bytes: &[u8], driver: &str, delta: i64| -> Vec<u8> {
        let mut at = 4usize;
        let mut names = Vec::new();
        for _ in 0..4 {
            let len = i32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize;
            names.push(bytes[at + 4..at + 4 + len].to_vec());
            at += 4 + len;
        }
        names[0] = driver.as_bytes().to_vec();
        let time = u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as i64 + delta;
        let mut out = bytes[..4].to_vec();
        for n in names {
            out.extend_from_slice(&(n.len() as i32).to_le_bytes());
            out.extend_from_slice(&n);
        }
        out.extend_from_slice(&(time as u32).to_le_bytes());
        out.extend_from_slice(&bytes[at + 4..]);
        out
    };
    let w = |path: std::path::PathBuf, bytes: &[u8]| std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()));
    w(out.join("best.tc"), &monza)?;
    w(out.join("best2.tc"), &retime(&monza, "thesupersupersigma", -800))?;
    w(out.join("last.tc"), &spa)?;
    w(opponents.join("a_monza.tc"), &monza)?;
    w(opponents.join("b_rival.tc"), &retime(&monza, "Rival", 1234))?;
    w(opponents.join("c_spa.tc"), &spa)?;
    Ok(())
}

fn dirs_documents() -> Option<std::path::PathBuf> {
    std::env::var_os("USERPROFILE").map(|home| std::path::Path::new(&home).join("Documents"))
}

/// `TrackAvatar::getAISpline` 0x1401c8010 here: the script's spline.
unsafe extern "C" fn get_ai_spline(_this: *mut u8) -> *mut u8 {
    script(|s| s.spline) as *mut u8
}

unsafe fn empty_wstring(at: *mut u8) {
    std::ptr::write_bytes(at, 0, 0x20);
    wr(at, 0x18, 7usize);
}

/// A vector of `CTData` in the game's memory: begin, end, capacity.
unsafe fn write_ct_vector(at: *mut u8, data: &[CtData]) {
    let acs = crate::gui_ac_hud::acs();
    let block = acs.alloc(data.len().max(1) * 20);
    for (i, d) in data.iter().enumerate() {
        let mut bytes = Vec::new();
        d.to_bytes(&mut bytes);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), block.add(i * 20), 20);
    }
    wr(at, 0, block);
    wr(at, 8, block.add(data.len() * 20));
    wr(at, 0x10, block.add(data.len() * 20));
}

/// The game's side: the recorder and the track made at `tele.setup`, the laps written as they
/// change.
pub unsafe fn game_call(game: &Game, what: &str) -> Result<(), String> {
    let h = crate::gui_ac_hud::hud().ok_or("the HUD first")?;
    let acs = &game.acs;
    if what == "tele.setup" {
        // the recorder among Sim's game objects (its class's table: the page's dynamic cast)
        let recorder = acs.alloc(0x170);
        wr(recorder, 0, acs.va(VA_RECORDER_VFTABLE) as usize);
        wr(recorder, 0x58, h.cars[0]);
        let (begin, end): (*const usize, *const usize) = (rd(h.sim, 0x40), rd(h.sim, 0x48));
        let mut objects: Vec<usize> = if begin.is_null() { Vec::new() } else { (0..(end as usize - begin as usize) / 8).map(|i| *begin.add(i)).collect() };
        objects.push(recorder as usize);
        let list = acs.alloc(objects.len() * 8);
        for (i, o) in objects.iter().enumerate() {
            wr(list, i * 8, *o);
        }
        wr(h.sim, 0x40, list);
        wr(h.sim, 0x48, list.add(objects.len() * 8));
        wr(h.sim, 0x50, list.add(objects.len() * 8));
        // the track: its unixName (+0x80), its layout (+0xa0); its AI spline
        let mut track: *mut u8 = rd(h.sim, 0x150);
        if track.is_null() {
            track = acs.alloc(0x400);
            wr(h.sim, 0x150, track);
        }
        // (getCTOpponents compares the GameObject's name, +0x10, with the file's track)
        for at in [0x10, 0x80] {
            empty_wstring(track.add(at));
            write_wstring(acs, track.add(at), TRACK);
        }
        empty_wstring(track.add(0xa0));
        let spline = acs.alloc(0x100);
        let points = acs.alloc(0x40);
        wr(spline, 0x58, points);
        wr(spline, 0x60, points.add(20));
        script(|s| {
            s.recorder = recorder as usize;
            s.spline = spline as usize;
        });
        crate::gui_ac_hud::jump(game, VA_GET_AI_SPLINE, get_ai_spline as *const () as usize);
    }
    update(game);
    Ok(())
}

/// The script's state in the game's blocks (every op and frame).
pub unsafe fn update(game: &Game) {
    let Some(h) = crate::gui_ac_hud::hud() else { return };
    let (recorder, spline, max_gear, length, best, last, mph) = script(|s| (s.recorder, s.spline, s.max_gear, s.length, s.best.clone(), s.last.clone(), s.mph));
    if recorder == 0 {
        return;
    }
    let r = recorder as *mut u8;
    let car = h.cars[0];
    wr(car, 0xe44, max_gear);
    empty_wstring(car.add(0x138));
    write_wstring(&game.acs, car.add(0x138), CAR);
    wr(spline as *mut u8, 0x70, length);
    if rd::<u32>(r, 0x158) != best.0 || rd::<usize>(r, 0xc0) == 0 {
        write_ct_vector(r.add(0xc0), &best.1);
        wr(r, 0x158, best.0);
    }
    if rd::<u32>(r, 0x15c) != last.0 || rd::<usize>(r, 0xa8) == 0 {
        write_ct_vector(r.add(0xa8), &last.1);
        wr(r, 0x15c, last.0);
    }
    wr(game.acs.va(VA_SPEED_USE_MPH) as *mut u8, 0, mph as u8);
}

/// The port's side: `SimLink::telemetry` as the script has it.
pub fn port_sync(gui: &mut rustyac_gui::gui::Gui, player: &str) {
    let link = script(|s| {
        s.active.then(|| TelemetryLink {
            has_recorder: true,
            best_time: s.best.0,
            best: std::rc::Rc::new(s.best.1.clone()),
            last_time: s.last.0,
            last: std::rc::Rc::new(s.last.1.clone()),
            opponents_folder: Some(folder().join("opponents")),
            track: TRACK.to_string(),
            track_config: String::new(),
            car: CAR.to_string(),
            player_name: player.to_string(),
            max_gear: s.max_gear,
            ai_spline_length: s.length,
            use_mph: s.mph,
        })
    });
    if let Some(link) = link {
        let t = &mut gui.sim.telemetry;
        // (the laps are new vectors only when they changed, as on the game's side)
        if t.best_time != link.best_time || t.best.len() != link.best.len() || t.last_time != link.last_time || t.last.len() != link.last.len() || t.use_mph != link.use_mph || t.ai_spline_length != link.ai_spline_length || !t.has_recorder {
            *t = link;
        }
    }
}

/// `esc_telemetry`: the page opened from the ESC menu; the mouse over the graphs; the boxes
/// off and on; the opponents' list, a driver chosen and let go; a new best and last lap;
/// miles an hour; the page closed and opened.
pub fn sequence(name: &str) -> Option<Vec<Op>> {
    if !name.starts_with("esc_telemetry") {
        return None;
    }
    let mut o = Vec::new();
    crate::gui_seq_menu::field(&mut o, 12);
    o.extend([hud("race.sessions", 0.0, "1:0:Practice:0:30:0"), hud("race.current", 0.0, ""), hud("race.board", 0.0, crate::gui_seq_menu::board(12, false, 0)), hud("session", 1.0, ""), hud("timeLeft", 1_234_567.0, "")]);
    o.extend([hud("tele.setup", 6.0, ""), hud("tele.length", 5793.0, ""), hud("tele.best", 0.0, "best.tc"), hud("tele.last", 0.0, "last.tc")]);
    let click = |o: &mut Vec<Op>, x: i32, y: i32| o.extend([Op::MouseMove(x, y), Op::MouseDown(x, y), Op::MouseUp(x, y), Op::Frame(DT), Op::Frame(DT)]);
    o.extend([hud("menu.escMenu", 0.0, ""), Op::Frame(DT), hud("menu.escShow", 0.0, ""), Op::Frame(DT), Op::Frame(DT)]);
    // the page by its button
    click(&mut o, 50, 425 + 48);
    // the mouse along the lap, over every graph and below them
    for (x, y) in [(115, 50), (300, 200), (640, 300), (900, 450), (1200, 560), (1268, 100), (640, 640), (60, 300)] {
        o.extend([Op::MouseMove(x, y), Op::Frame(DT)]);
    }
    if name.ends_with("short") {
        return Some(o);
    }
    // the two boxes off, then on again
    click(&mut o, 126, 626);
    click(&mut o, 126, 651);
    o.extend([Op::MouseMove(500, 300), Op::Frame(DT)]);
    click(&mut o, 126, 626);
    click(&mut o, 126, 651);
    // the opponents: the list, the first driver, the second, the second again (let go)
    click(&mut o, 300, 682);
    o.extend([Op::MouseMove(700, 250), Op::Frame(DT)]);
    for (x, y) in [(260, 482), (260, 502), (260, 502), (260, 482)] {
        click(&mut o, x, y);
        o.extend([Op::MouseMove(800, 200), Op::Frame(DT)]);
    }
    click(&mut o, 300, 682);
    // a new best lap and a new last one; miles an hour
    o.extend([hud("tele.best", 0.0, "best2.tc"), Op::Frame(DT), hud("tele.last", 0.0, "best.tc"), Op::Frame(DT), hud("tele.mph", 1.0, ""), Op::MouseMove(450, 300), Op::Frame(DT), Op::Frame(DT)]);
    // the page closed and opened by its button (the laps again)
    click(&mut o, 50, 425 + 48);
    click(&mut o, 50, 425 + 48);
    o.extend([Op::MouseMove(1000, 300), Op::Frame(DT), Op::Frame(DT)]);
    Some(o)
}
