// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C6a-2c: the configs CSP loads for a car and a track, in its order, each read by INIpp with
//! `read()` answering from the car or track (dwrite.dll: car loader sub_18150E200 and its per-file
//! sub_18150D840, track loader sub_18141A1A0 / sub_181416320, file search sub_18142BFC0, car reader
//! sub_18150C4D0). docs/csp/lua_host.md, "C6a-2c".

use std::cell::Cell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::inipp::{self, Reader, Sections};
use crate::data::ini::IniReader;

/// The folders CSP's loaders look in.
#[derive(Clone)]
pub struct Folders {
    /// Assetto Corsa's folder
    pub ac: PathBuf,
    /// `Documents\Assetto Corsa\cfg\extension` (CSP's user configs)
    pub user: Option<PathBuf>,
}

impl Folders {
    /// `extension\config` (CSP's folder 17)
    fn config(&self) -> PathBuf {
        self.ac.join("extension").join("config")
    }
}

/// One config a loader read.
pub struct Config {
    /// the file (the generic config when no other was found)
    pub file: PathBuf,
    pub sections: Sections,
    /// CSP keeps it in `cache\csp_configs` (its reads didn't ask for the car's index, skin or track data)
    pub cached: bool,
}

/// The files of `dirs` (a file is itself, a folder is searched for each name, depth first,
/// `deferred` subfolders after the others), in sub_18142BFC0's order, each once.
fn search(dirs: &[PathBuf], names: &[String], deferred: &str) -> Vec<PathBuf> {
    let mut queue: VecDeque<(PathBuf, &str)> = dirs.iter().flat_map(|d| names.iter().map(move |n| (d.clone(), n.as_str()))).collect();
    let mut out: Vec<PathBuf> = Vec::new();
    while let Some((p, name)) = queue.pop_front() {
        if p.is_file() {
            if !out.iter().any(|o| same(o, &p)) {
                out.push(p);
            }
            continue;
        }
        let Ok(list) = std::fs::read_dir(&p) else { continue };
        // FindFirstFile's order on NTFS
        let mut kids: Vec<(String, PathBuf, bool)> = list.flatten().map(|e| (e.file_name().to_string_lossy().into_owned(), e.path(), e.path().is_dir())).collect();
        kids.sort_by_key(|k| k.0.to_uppercase());
        let (mut now, mut later) = (Vec::new(), Vec::new());
        for (n, path, dir) in kids {
            if dir && n.eq_ignore_ascii_case(deferred) {
                later.push(path);
            } else if dir || n.eq_ignore_ascii_case(name) {
                now.push(path);
            }
        }
        for p in now.into_iter().chain(later).rev() {
            queue.push_front((p, name));
        }
    }
    out
}

fn same(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
}

/// Is `file` inside `dir` (case aside)?
fn under(file: &Path, dir: &Path) -> bool {
    let (f, d) = (file.to_string_lossy(), dir.to_string_lossy());
    f.len() > d.len() && f.is_char_boundary(d.len()) && f[..d.len()].eq_ignore_ascii_case(&d) && matches!(f.as_bytes()[d.len()], b'/' | b'\\')
}

/// The per-file loop both loaders share: `files` read with index `n-1` down to 0 (`PREFIX_...`
/// numbers from 10000 × index), the generic config after the first file outside the user folder
/// (alone when there is none), `[BASIC] IGNORE_OTHER_CONFIGS` dropping later `extension\config` files.
fn load_all(f: &Folders, std: &[u8], files: Vec<PathBuf>, kind: &str, dirs: Vec<PathBuf>, reader: &dyn Fn() -> (Reader, Rc<Cell<u8>>)) -> Vec<Config> {
    let config = f.config();
    let generic = config.join(kind).join("generic").join("generic.ini");
    let user = f.user.as_ref().map(|u| u.join(kind));
    let (mut ignore, mut with_generic) = (false, false);
    let mut out = Vec::new();
    let n = files.len();
    let mut one = |file: &Path, after: Vec<PathBuf>, base: i64, cache: bool| -> Option<Sections> {
        let (read, flags) = reader();
        let sections = inipp::load(file, inipp::Load { std, dirs: dirs.clone(), after, base, read })?;
        // a car skin's own file may read `car/skin` and still be cached
        let skin = file.parent().and_then(Path::parent).and_then(Path::file_name).is_some_and(|n| n.eq_ignore_ascii_case("skins"));
        let flags = if skin { flags.get() & !4 } else { flags.get() };
        out.push(Config { file: file.to_path_buf(), sections: sections.clone(), cached: cache && flags & 5 == 0 });
        Some(sections)
    };
    for (i, file) in files.iter().enumerate() {
        let first = !with_generic && !user.as_ref().is_some_and(|u| under(file, u));
        with_generic |= first;
        if ignore && under(file, &config) {
            continue;
        }
        let after = if first { vec![generic.clone()] } else { Vec::new() };
        let Some(s) = one(file, after, 10000 * (n - 1 - i) as i64, true) else { continue };
        let basic = s.iter().find(|(name, _)| name == b"BASIC");
        ignore |= basic.and_then(|(_, keys)| keys.iter().find(|(k, _)| k == b"IGNORE_OTHER_CONFIGS")).is_some_and(|(_, v)| inipp::truthy(v.first()));
    }
    if !with_generic {
        one(&generic, Vec::new(), 0, false);
    }
    out
}

/// The include folders (ini +136): user, `extension\config`, `extension\lua`, each `\<kind>`.
fn include_dirs(f: &Folders, kind: &str) -> Vec<PathBuf> {
    f.user.iter().cloned().chain([f.config(), f.ac.join("extension").join("lua")]).map(|d| d.join(kind)).collect()
}

/// The car `id` with skin `skin` (sub_18150E200); `index` its place in the race (`car/index`).
// shortcut: a car's own `data\csp_config.ini` (read first, as is, no INIpp), `sfp.ini` and CSP's
// built-in fallback configs are not read; no installed car has csp_config.ini
pub fn car(f: &Folders, std: &[u8], id: &str, skin: &str, index: Option<i32>) -> Vec<Config> {
    let car_dir = f.ac.join("content").join("cars").join(id);
    let dirs: Vec<PathBuf> = f.user.iter().map(|u| u.join("cars")).chain([car_dir.join("skins").join(skin).join("ext_config.ini"), car_dir.join("extension").join("ext_config.ini"), f.config().join("cars")]).collect();
    let files = search(&dirs, &[format!("{id}__{skin}.ini"), format!("{id}.ini")], "loaded");
    let ctx = Rc::new((f.clone(), id.to_string(), skin.to_string(), index));
    let reader = || -> (Reader, Rc<Cell<u8>>) {
        let flags = Rc::new(Cell::new(0u8));
        let (ctx, fl) = (ctx.clone(), flags.clone());
        (Box::new(move |p: &[u8]| car_read(&ctx.0, &ctx.1, &ctx.2, ctx.3, p, &fl)), flags)
    };
    load_all(f, std, files, "cars", include_dirs(f, "cars"), &reader)
}

/// The track `id` with layout `layout` ("" for none) (sub_18141A1A0).
pub fn track(f: &Folders, std: &[u8], id: &str, layout: &str) -> Vec<Config> {
    let dir = f.ac.join("content").join("tracks").join(id);
    let mut dirs: Vec<PathBuf> = f.user.iter().map(|u| u.join("tracks")).collect();
    // Content Manager's active track skins
    let active = std::fs::read_to_string(dir.join("skins").join("default").join("cm_skins_active.json")).unwrap_or_default();
    for name in active.split('"').skip(1).step_by(2) {
        let p = dir.join("skins").join("cm_skins").join(name).join("ext_config.ini");
        if p.is_file() {
            dirs.push(p);
        }
    }
    let layout_dir = if layout.is_empty() { dir.clone() } else { dir.join(layout) };
    dirs.extend([layout_dir.join("extension").join("ext_config.ini"), dir.join("extension").join("ext_config.ini"), f.config().join("tracks")]);
    let mut names = Vec::new();
    if !layout.is_empty() {
        names.extend([format!("{id}__{layout}.ini"), format!("{id}_{layout}.ini")]);
    }
    names.push(format!("{id}.ini"));
    let files = search(&dirs, &names, "loaded");
    let (f2, id2) = (f.clone(), id.to_string());
    let reader = || -> (Reader, Rc<Cell<u8>>) {
        let flags = Rc::new(Cell::new(0u8));
        let (f, id, fl) = (f2.clone(), id2.clone(), flags.clone());
        (Box::new(move |p: &[u8]| track_read(&f, &id, p, &fl)), flags)
    };
    load_all(f, std, files, "tracks", include_dirs(f, "tracks"), &reader)
}

/// The name CSP's ConfigsCache gives a config in `cache\csp_configs\<id>.zip` (sub_181510A00):
/// XXH3-64's low half, hex, of the path in lower case with `\` (sub_180308880).
pub fn cache_name(file: &Path) -> String {
    let p = file.to_string_lossy().to_lowercase().replace('/', "\\");
    format!("{:x}", twox_hash::XxHash3_64::oneshot(p.as_bytes()) as u32)
}

fn s(v: impl ToString) -> Option<Vec<u8>> {
    Some(v.to_string().into_bytes())
}

/// The first value of an ini key (CSP's values split at commas).
fn ini_value(ini: &IniReader, sec: &str, key: &str) -> Option<Vec<u8>> {
    ini.has_key(sec, key).then(|| ini.get_string(sec, key).split(',').next().unwrap_or("").trim().as_bytes().to_vec())
}

/// `settings/<section>/<key>`: graphics_adjustments.ini, the user's over CSP's defaults.
fn settings(f: &Folders, rest: &str, flags: &Cell<u8>) -> Option<Vec<u8>> {
    flags.set(flags.get() | 0x10);
    let (sec, key) = rest.split_once('/')?;
    let files = f.user.iter().cloned().chain([f.config()]).map(|d| d.join("graphics_adjustments.ini"));
    files.filter_map(|p| IniReader::load(&p).ok()).find_map(|ini| ini_value(&ini, sec, key))
}

/// The car reader (sub_18150C4D0); `flags` as CSP's provider +16 (1 race, 2 car data, 4 skin, 8 meta, 0x10 settings).
// shortcut: `track/data/...` and the meta keys no config reads (brand, country, racing_car, ...) answer nothing
fn car_read(f: &Folders, id: &str, skin: &str, index: Option<i32>, path: &[u8], flags: &Cell<u8>) -> Option<Vec<u8>> {
    let path = std::str::from_utf8(path).ok()?;
    let set = |b: u8| flags.set(flags.get() | b);
    let car = f.ac.join("content").join("cars").join(id);
    if let Some(rest) = path.strip_prefix("data/") {
        set(2);
        let [file, sec, key] = <[&str; 3]>::try_from(rest.split('/').collect::<Vec<_>>()).ok()?;
        return ini_value(&IniReader::load(&car.join("data").join(file)).ok()?, sec, key);
    }
    match path {
        "car/id" => s(id),
        "car/skin" => {
            set(4);
            s(skin)
        }
        "car/index" => {
            set(1);
            index.and_then(s)
        }
        "meta/has_rain_tyres" => {
            set(2);
            let data = car.join("data");
            // CSP's ini reader also ends a line at a lone CR (urd_darche_992_23's tyres.ini)
            let text = crate::data::read(&data.join("tyres.ini")).ok().flatten().map(|b| String::from_utf8_lossy(&b).replace("\r\n", "\n").replace('\r', "\n"));
            let rain = text.is_some_and(|text| {
                let ini = IniReader::from_text(&data.join("tyres.ini"), &text);
                let car_ini = IniReader::load(&data.join("car.ini"));
                let extended = car_ini.is_ok_and(|c| ["extended-", "extendedV2-"].iter().any(|p| c.get_string("HEADER", "VERSION").starts_with(p)));
                // CSP makes them for every car once the player's car is loaded (byte set in sub_1805E32E0)
                has_rain_tyres(&ini, extended || index.is_some_and(|i| i > 0))
            });
            s(rain as u8)
        }
        "meta/year" => {
            set(8);
            s(car_year(f, id))
        }
        "meta/open_wheeler" => {
            set(8);
            const TAGS: [&str; 7] = ["singleseater", "open wheeler", "openwheeler", "f1", "formula1", "formula 1", "kart"];
            let json = std::fs::read_to_string(car.join("ui").join("ui_car.json")).unwrap_or_default();
            let tags = json_strings(&json, "tags");
            s(tags.iter().any(|t| TAGS.iter().any(|x| t.trim_matches([' ', '\t', '#']).eq_ignore_ascii_case(x))) as u8)
        }
        _ if path.starts_with("meta/") => {
            set(8);
            None
        }
        _ if path.starts_with("track/data/") => {
            set(1);
            None
        }
        _ => {
            if let Some(rest) = path.strip_prefix("fixed_random/") {
                return Some(fixed_random(path, rest, id));
            }
            settings(f, path.strip_prefix("settings/")?, flags)
        }
    }
}

/// The track side: `track/id`, settings.
// shortcut: CSP's track reader not decompiled beyond these two (the only ones track configs read)
fn track_read(f: &Folders, id: &str, path: &[u8], flags: &Cell<u8>) -> Option<Vec<u8>> {
    let path = std::str::from_utf8(path).ok()?;
    match path {
        "track/id" => s(id),
        _ if path.starts_with("track/data/") => {
            flags.set(flags.get() | 1);
            None
        }
        _ => settings(f, path.strip_prefix("settings/")?, flags),
    }
}

/// CSP's tyre type of a section (sub_18090C9A0): `TYPE_HINT`, else guessed from the lowercase
/// name and short name. 1 street, 2 vintage, 3 racing treaded, 4 semislick, 5 slick, 6 rain, 7 gravel, 8 ice.
fn tyre_type(ini: &IniReader, sec: &str) -> u8 {
    match ini.get_string(sec, "TYPE_HINT").as_str() {
        "TREADED" => return 1,
        "VINTAGE_PERFORMANCE" => return 2,
        "RACING_TREADED" => return 3,
        "SEMISLICK" => return 4,
        "SLICK" => return 5,
        "RAIN" | "WET" => return 6,
        "GRAVEL" => return 7,
        "ICE" => return 8,
        _ => {}
    }
    let n = ini.get_string(sec, "NAME").to_lowercase();
    let s = ini.get_string(sec, "SHORT_NAME").to_lowercase();
    let any = |t: &str, xs: &[&str]| xs.iter().any(|x| t.contains(x));
    if any(&n, &["rain", "wet"]) || any(&s, &["rain", "wet"]) {
        6
    } else if any(&n, &["street", "road", "oem", "wdt enjoy"]) || any(&s, &["st", "sv", "oem", "hr"]) || (n.contains("pilot sport") && s.contains("cup 2")) {
        1
    } else if any(&n, &["semi", "intermediate"]) || n.starts_with("int") || any(&s, &["sm", "int"]) || s == "i" {
        4
    } else if (s == "v" || s.contains("gp67")) && !n.contains("gp70") {
        2
    } else {
        5
    }
}

/// `meta/has_rain_tyres` (sub_18150C4D0): a FRONT tyre of type rain, counting the rain tyre CSP makes
/// on loading tyres.ini (sub_180914710) for an extended-physics car with slicks, or semislicks and no
/// street tyres, unless `[_EXTENSION] GENERATE_EXTRA_TYRES=0` or a tyre has `_VIRTUAL_ORIGIN`.
// shortcut: "player's car loaded" taken as car index > 0 (the cache holds tatuusfa1 without, rss_formula_rss_3_v6 with); an encrypted tyres.ini (urd_darche_992_23) is read as its plain stub
fn has_rain_tyres(ini: &IniReader, extended: bool) -> bool {
    let fronts: Vec<&str> = ini.sections().filter(|n| n.starts_with("FRONT")).collect();
    let types: Vec<u8> = fronts.iter().map(|n| tyre_type(ini, n)).collect();
    if types.contains(&6) {
        return true;
    }
    let gen = ini.get_string("_EXTENSION", "GENERATE_EXTRA_TYRES");
    let off = ini.has_key("_EXTENSION", "GENERATE_EXTRA_TYRES") && !(gen.starts_with("true") || gen.trim().parse::<f64>().is_ok_and(|x| x != 0.0));
    let virt = ini.sections().any(|n| (n.starts_with("FRONT") || n.starts_with("REAR")) && ini.has_key(n, "_VIRTUAL_ORIGIN"));
    extended && !off && !virt && (types.contains(&5) || (types.contains(&4) && !types.contains(&1)))
}

/// `meta/year` (sub_181423430): ui_car.json's "year", else extension\config\data_car_years.ini, else 2000.
fn car_year(f: &Folders, id: &str) -> i64 {
    let json = std::fs::read_to_string(f.ac.join("content").join("cars").join(id).join("ui").join("ui_car.json")).unwrap_or_default();
    if let Some(y) = json_value(&json, "year").and_then(|v| v.trim_matches('"').trim().parse().ok()) {
        return y;
    }
    let ini = IniReader::load(&f.config().join("data_car_years.ini")).ok();
    ini.and_then(|ini| ini.has_key("YEARS", id).then(|| ini.get_string("YEARS", id).trim().parse().ok()).flatten()).unwrap_or(2000)
}

/// The raw value after `"key":` in a JSON text (ui_car.json is a flat object).
// shortcut: a text search, not a JSON parser (CSP uses nlohmann::json); enough for ui_car.json
fn json_value<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let at = json.find(&format!("\"{key}\""))? + key.len() + 2;
    let rest = json[at..].trim_start().strip_prefix(':')?.trim_start();
    let end = rest.find([',', '}', '\n']).unwrap_or(rest.len());
    Some(rest[..end].trim())
}

/// The strings of `"key": [ ... ]`.
fn json_strings(json: &str, key: &str) -> Vec<String> {
    let Some(at) = json.find(&format!("\"{key}\"")) else { return Vec::new() };
    let rest = &json[at + key.len() + 2..];
    let Some(open) = rest.find('[') else { return Vec::new() };
    let body = &rest[open + 1..rest[open..].find(']').map_or(rest.len(), |c| c + open)];
    body.split('"').skip(1).step_by(2).map(str::to_string).collect()
}

/// `fixed_random/<n>` (sub_18150C4D0): a number fixed per key and car: an integer below `n`, or for
/// n = 0 a float in [0, 1) (std::mt19937's first output).
fn fixed_random(key: &str, rest: &str, id: &str) -> Vec<u8> {
    use twox_hash::XxHash3_64;
    let n = c_number(rest);
    let mut h = XxHash3_64::oneshot(key.as_bytes());
    h ^= (h << 6).wrapping_add(h >> 2).wrapping_add(0x9e37_79b9).wrapping_add(h);
    let wide: Vec<u8> = id.encode_utf16().flat_map(u16::to_le_bytes).collect();
    h ^= (h >> 2).wrapping_add(0x9e37_79b9).wrapping_add(XxHash3_64::oneshot(&wide)).wrapping_add(h << 6);
    let seed = (h ^ (h >> 32)) as u32;
    if n != 0 {
        return (seed % n).to_string().into_bytes();
    }
    let x = (mt19937_first(seed) >> 8) as f32 * f32::from_bits(0x3380_0000);
    x.to_string().into_bytes()
}

/// sub_180A45660: leading decimal digits, or `0x` and hex digits; anything else 0.
fn c_number(s: &str) -> u32 {
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) => u32::from_str_radix(&h[..h.find(|c: char| !c.is_ascii_hexdigit()).unwrap_or(h.len())], 16).unwrap_or(0),
        None => s[..s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len())].parse().unwrap_or(0),
    }
}

/// The first output of std::mt19937 seeded with `seed`.
fn mt19937_first(seed: u32) -> u32 {
    let mut mt = [0u32; 625];
    mt[0] = seed;
    for i in 1..625 {
        mt[i] = 1_812_433_253u32.wrapping_mul(mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_add(i as u32);
    }
    let y = (mt[0] & 0x8000_0000) | (mt[1] & 0x7fff_ffff);
    let mut y = mt[397] ^ (y >> 1) ^ if y & 1 != 0 { 0x9908_b0df } else { 0 };
    y ^= y >> 11;
    y ^= (y << 7) & 0x9d2c_5680;
    y ^= (y << 15) & 0xefc6_0000;
    y ^ (y >> 18)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rain_tyres() {
        let ini = |t: &str| IniReader::from_text(Path::new("tyres.ini"), t);
        let slicks = ini("[FRONT]\nNAME=Soft\nSHORT_NAME=S\n[FRONT_1]\nNAME=Hard\nSHORT_NAME=H\n");
        assert_eq!(tyre_type(&slicks, "FRONT"), 5);
        assert!(!has_rain_tyres(&slicks, false));
        assert!(has_rain_tyres(&slicks, true));
        let street = ini("[FRONT]\nNAME=Street\nSHORT_NAME=ST\n[FRONT_1]\nNAME=Semislick\nSHORT_NAME=SM\n");
        assert_eq!((tyre_type(&street, "FRONT"), tyre_type(&street, "FRONT_1")), (1, 4));
        assert!(!has_rain_tyres(&street, true));
        assert!(has_rain_tyres(&ini("[FRONT]\nNAME=Slicks\nTYPE_HINT=RAIN\n"), false));
        assert!(!has_rain_tyres(&ini("[_EXTENSION]\nGENERATE_EXTRA_TYRES=0\n[FRONT]\nNAME=Soft\n"), true));
        assert!(!has_rain_tyres(&ini("[FRONT]\nNAME=Soft\n[REAR]\n_VIRTUAL_ORIGIN=1\n"), true));
    }

    #[test]
    fn mt19937_as_std() {
        // the C++ standard's check: the 10000th output of a default-seeded mt19937 is 4123659995;
        // the first output for seed 5489 is 3499211612
        assert_eq!(mt19937_first(5489), 3_499_211_612);
    }

    #[test]
    fn numbers_and_patterns() {
        assert_eq!(c_number("12ab"), 12);
        assert_eq!(c_number("0x1f"), 31);
        assert_eq!(c_number("generic_rain_tyres_0"), 0);
        assert!(under(Path::new("C:\\a\\B\\c.ini"), Path::new("c:\\A\\b")));
        assert!(!under(Path::new("C:\\a\\bc.ini"), Path::new("C:\\a\\b")));
        assert_eq!(json_value("{\n \"year\": 2016,\n}", "year"), Some("2016"));
        assert_eq!(json_strings("{\"tags\": [\"#F1\", \"x\"]}", "tags"), ["#F1", "x"]);
    }
}
