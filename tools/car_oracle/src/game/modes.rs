//! Task 45: the game's own special-event goals against the port's
//! (`rustyac_physics::race::game_mode`), in-process:
//!
//! * `SimpleFileEncrypter::decryptFile` on every `system/data/se/SPECIAL_EVENT_*.acd` of the
//!   install against `rustyac_content::acd::decrypt_simple`, compared in memory (nothing
//!   decrypted is written anywhere);
//! * `GameMode::GameMode` reading a race.ini (the oracle's own `cfg/race.ini`, and through it
//!   every official event) against `GameMode::new`;
//! * `HotlapMode`'s lap handler on made-up laps against `Mode::on_lap_completed`.

use std::cell::RefCell;
use std::path::Path;

use rustyac_physics::data::ini::IniReader;
use rustyac_physics::race::game_mode::{Condition, GameMode, Mode};
use rustyac_physics::race::manager::SESSION_HOTLAP;
use rustyac_physics::track::timing::OnLapCompletedEvent;

use super::race_ini::{rd, text, wr, write_wstring, zeroed};
use crate::acs::Acs;

const VA_DECRYPT_FILE: usize = 0x1_4023_b650; // vector<char> SimpleFileEncrypter::decryptFile(const wstring&, const wstring&)
const VA_GAME_MODE_CTOR: usize = 0x1_400b_c810; // GameMode::GameMode(const wstring&, Game*)
const VA_HOTLAP_LAP: usize = 0x1_400b_eb80; // HotlapMode's lambda on OnLapCompletedEvent
const VA_ADD_MESSAGE: usize = 0x1_401b_61c0; // SystemMessage::addMessage(wstring, wstring, type)
const VA_INIREADERDOCUMENTS_INITIALIZED: usize = 0x1_4155_a588;
const VA_INIREADERDOCUMENTS_BASE_PATH: usize = 0x1_4151_d100;
const GM_GUID: usize = 0x88;
const GM_CONDITIONS: usize = 0xa8;
const GM_TIER: usize = 0xc4;
const HOTLAP_SIZE: usize = 0xd8;
const HOTLAP_SIM: usize = 0xd0;

thread_local! {
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

extern "C" fn add_message(_messages: *mut u8, message: *const u8, description: *const u8, _kind: i32) {
    let line = unsafe { format!("Message({} | {})", text(message), text(description)) };
    SAID.with(|s| s.borrow_mut().push(line));
}

#[derive(Default)]
pub struct Report {
    pub files: usize,
    pub bytes: usize,
    pub inis: usize,
    pub conditions: usize,
    pub laps: usize,
    pub messages: usize,
    pub json_bytes: usize,
    pub mismatch: Option<String>,
}

/// The game's goals of a `GameMode` block.
unsafe fn game_side(mode: *const u8) -> (String, Vec<Condition>, i32) {
    let (begin, end): (usize, usize) = (rd(mode, GM_CONDITIONS), rd(mode, GM_CONDITIONS + 8));
    let conditions = (begin..end)
        .step_by(0x48)
        .map(|c| {
            let c = c as *const u8;
            Condition { kind: text(c), name: text(c.add(0x20)), objective: rd(c, 0x40), achieved: rd::<u8>(c, 0x44) != 0 }
        })
        .collect();
    (text(mode.add(GM_GUID)), conditions, rd(mode, GM_TIER))
}

/// `root`: the oracle's scratch folder (its `cfg/race.ini` is written there); `ac`: the
/// install, the game's working folder while it reads (it only reads).
pub unsafe fn micro(acs: &Acs, root: &Path, ac: &Path, seed: u32, laps_per_case: usize) -> Result<Report, String> {
    let stub = |va: usize, target: usize| {
        let mut code = vec![0x48, 0xb8];
        code.extend_from_slice(&(target as u64).to_le_bytes());
        code.extend_from_slice(&[0xff, 0xe0]);
        acs.patch(acs.va(va), &code);
    };
    stub(VA_ADD_MESSAGE, add_message as *const () as usize);
    acs.set_global(VA_INIREADERDOCUMENTS_INITIALIZED, 1u8);
    write_wstring(acs.va(VA_INIREADERDOCUMENTS_BASE_PATH) as *mut u8, &format!("{}/", root.to_string_lossy().replace('\\', "/")));
    let mut report = Report::default();
    let differ = |report: &mut Report, what: String| {
        if report.mismatch.is_none() {
            report.mismatch = Some(what);
        }
    };

    // 1. every official event, decrypted by both
    let se = ac.join("system/data/se");
    let mut guids = Vec::new();
    let decrypt: extern "C" fn(*mut [usize; 3], *const u8, *const u8) -> *mut [usize; 3] = std::mem::transmute(acs.va(VA_DECRYPT_FILE));
    for entry in std::fs::read_dir(&se).map_err(|e| format!("{}: {e}", se.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let Some(guid) = stem.strip_prefix("SPECIAL_EVENT_").and_then(|n| n.parse::<i32>().ok()) else { continue };
        let key = format!("ARUMBA_{stem}");
        let (path_w, key_w) = (zeroed(0x20), zeroed(0x20));
        write_wstring(path_w, &path.to_string_lossy());
        write_wstring(key_w, &key);
        let mut out = [0usize; 3];
        decrypt(&mut out, path_w, key_w);
        let game = std::slice::from_raw_parts(out[0] as *const u8, out[1] - out[0]);
        let file = std::fs::read(&path).map_err(|e| e.to_string())?;
        let port = rustyac_content::acd::decrypt_simple(&file, &key);
        if port.as_deref() != Some(game) {
            differ(&mut report, format!("{stem}: decrypted {} bytes by the game, {:?} by the port", game.len(), port.map(|p| p.len())));
        }
        report.files += 1;
        report.bytes += game.len();
        guids.push(guid);
    }
    guids.sort();

    // 2. race.ini files: none, a mod's event (no file), one that names a missing file, every official one
    let mut cases = vec![
        "[RACE]\nTRACK=magione\n".to_string(),
        "[SPECIAL_EVENT]\nGUID=999999\n[CONDITION_0]\nTYPE=TIME\nOBJECTIVE=90000\nACHIEVED=1\n[CONDITION_1]\nTYPE=TIME\nOBJECTIVE=87000\n[CONDITION_2]\nTYPE=POINTS\nOBJECTIVE=85000\nACHIEVED=1\n[CONDITION_3]\nTYPE=TIME\nOBJECTIVE=84000\n[CONDITION_4]\nOBJECTIVE=-5\nACHIEVED=0\n".to_string(),
        // GUID 0 names no file, so race.ini stays (a GUID that is not a number throws in the game)
        "[SPECIAL_EVENT]\nGUID=0\n[CONDITION_0]\nTYPE=TIME\nOBJECTIVE=60000\n".to_string(),
    ];
    cases.extend(guids.iter().enumerate().map(|(i, guid)| format!("[SPECIAL_EVENT]\nGUID={guid}\n[CONDITION_{}]\nACHIEVED=1\n", i % 4)));
    let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_GAME_MODE_CTOR));
    let lap_handler: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(acs.va(VA_HOTLAP_LAP));
    let here = std::env::current_dir().map_err(|e| e.to_string())?;
    let file = root.join("cfg/race.ini");
    std::fs::create_dir_all(root.join("cfg")).map_err(|e| e.to_string())?;
    let mut kept = None;
    let mut rand = seed;
    let mut next = move || {
        rand = rand.wrapping_mul(214_013).wrapping_add(2_531_011);
        rand >> 16 & 0x7fff
    };
    for (case, ini) in cases.iter().enumerate() {
        std::fs::write(&file, ini).map_err(|e| e.to_string())?;
        let mode = zeroed(HOTLAP_SIZE);
        let name = zeroed(0x20);
        write_wstring(name, "HOTLAP_MODE");
        std::env::set_current_dir(ac).map_err(|e| e.to_string())?;
        ctor(mode, name, zeroed(0x278));
        std::env::set_current_dir(&here).map_err(|e| e.to_string())?;
        let documents = IniReader::load(&file)?;
        let official = documents.official_special_event();
        let mut port = GameMode::new(official.as_ref().unwrap_or(&documents), &documents, official.is_some());
        let game = game_side(mode);
        if (&port.guid, &port.conditions, port.current_tier) != (&game.0, &game.1, game.2) {
            differ(&mut report, format!("race.ini case {case}: the game read {game:?}, the port {:?}", (&port.guid, &port.conditions, port.current_tier)));
            continue;
        }
        report.inis += 1;
        report.conditions += port.conditions.len();

        // 3. the hotlap judge on made-up laps
        wr(mode, HOTLAP_SIM, zeroed(0x2d0));
        let lambda = zeroed(8);
        wr(lambda, 0, mode);
        port.official = false;
        let mut port_mode = Mode::of(SESSION_HOTLAP, &port).ok_or("no hotlap mode")?;
        let objectives: Vec<i32> = port.conditions.iter().map(|c| c.objective).collect();
        for lap in 0..laps_per_case {
            let base = objectives.get(next() as usize % objectives.len().max(1)).copied().unwrap_or(60_000);
            let event = OnLapCompletedEvent {
                lap_time: if next() % 16 == 0 { 0 } else { (base as i64 + next() as i64 % 4001 - 2000).max(0) as u32 },
                is_valid: next() % 8 != 0,
                cuts: (next() % 8 == 0) as i32,
                ..OnLapCompletedEvent::default()
            };
            let block = zeroed(0x40);
            wr(block, 4, event.lap_time);
            wr(block, 0x30, event.is_valid as u8);
            wr(block, 0x34, event.cuts);
            SAID.with(|s| s.borrow_mut().clear());
            lap_handler(lambda, block);
            let game_said = SAID.with(|s| std::mem::take(&mut *s.borrow_mut()));
            let mut port_said = Vec::new();
            port_mode.on_lap_completed(&event, &mut |a, b| port_said.push(format!("Message({a} | {b})")));
            let game = game_side(mode);
            let base = port_mode.base();
            if game_said != port_said || (&base.conditions, base.current_tier) != (&game.1, game.2) {
                differ(&mut report, format!("race.ini case {case} lap {lap} {event:?}: the game said {game_said:?} tier {}, the port {port_said:?} tier {}", game.2, base.current_tier));
                break;
            }
            report.laps += 1;
            report.messages += game_said.len();
        }
        if case == 1 {
            kept = Some((mode, port_mode.extras()));
        }
    }
    if report.mismatch.is_some() {
        return Ok(report);
    }

    // 4. race_out.json with the mod event's mode among the game objects (one drive per process)
    let race = super::race::micro(acs, seed, 0, 2, 20_000, false, kept)?;
    report.json_bytes = race.json_bytes;
    if let Some(what) = race.mismatch {
        differ(&mut report, format!("the race with a mode: {what}"));
    }
    Ok(report)
}
