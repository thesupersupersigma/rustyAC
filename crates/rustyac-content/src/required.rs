// SPDX-License-Identifier: MIT OR Apache-2.0

//! The required-files lists shipped with the native builds (`docs/platforms.md`, section 12.3;
//! written by `tools/web_pack --required`): `base.json` for the game without DLCs, one
//! `dlc_<Steam app>.json` per DLC. Paths and sizes of Kunos' files only.
//!
//! * A chosen full install is checked against `base.json`: missing files make it "not a
//!   complete Assetto Corsa", named by car, track or folder.
//! * A picked car or track that a DLC list holds is checked against that list: "this car needs
//!   the <name> DLC".
//! * Anything in no list (a mod, or a car or track of a partial import) is checked by whether it
//!   loads ([`loads`]); a mod whose models carry a DLC's key needs that DLC's files too.

use std::path::Path;

use crate::vfs::PathExt;

/// The DLCs: the content key in their models' headers, their Steam app, Steam's name
/// (`SteamInit`'s order; the names from Steam's app cache, section 12.3).
pub const DLCS: [(u32, u32, &str); 11] = [
    (0xa4691, 347990, "Dream Pack 1"),
    (0xdeafd, 404430, "Dream Pack 2"),
    (0xbebb1, 423630, "Dream Pack 3"),
    (0x3b787, 467980, "Japanese Pack"),
    (0xb5735, 493190, "Red Pack"),
    (0x4c791, 509350, "Tripl3 Pack"),
    (0xe09a1, 526060, "Porsche Pack I"),
    (0xcc03f, 540710, "Porsche Pack II"),
    (0x905f2, 540711, "Porsche Pack III"),
    (0x7e4bf, 619680, "Ready to Race Pack"),
    (0xdebda, 675590, "Ferrari Pack"),
];

/// The DLC key in a kn5's first 14 bytes (version 6 and up), 0 for the base game or none.
pub fn kn5_key(head: &[u8]) -> u32 {
    if head.len() < 14 || &head[..6] != b"sc6969" || u32::from_le_bytes(head[6..10].try_into().unwrap()) <= 5 {
        return 0;
    }
    u32::from_le_bytes(head[10..14].try_into().unwrap())
}

/// The DLC key of a kn5 file (0: none, or it cannot be read).
pub fn kn5_file_key(path: &Path) -> u32 {
    let mut head = [0u8; 14];
    crate::vfs::File::open(path).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)).map_or(0, |_| kn5_key(&head))
}

/// A car or a track layout of a list.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    pub layout: String,
    pub files: Vec<usize>,
}

/// One list (the preview pack's manifest format, `rustyac-preview-pack-1`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct List {
    pub cars: Vec<Item>,
    pub tracks: Vec<Item>,
    pub shared: Vec<usize>,
    pub launcher: Vec<usize>,
    /// Path (relative to the game's folder, `/`) and size.
    pub files: Vec<(String, u64)>,
}

/// The JSON string after `key` in a line.
fn text_of(line: &str, key: &str) -> Option<String> {
    let rest = &line[line.find(&format!("\"{key}\": \""))? + key.len() + 5..];
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'u' => out.push(char::from_u32(u32::from_str_radix(&chars.by_ref().take(4).collect::<String>(), 16).ok()?)?),
                c => out.push(c),
            },
            c => out.push(c),
        }
    }
    None
}

/// The number list after `key` in a line.
fn numbers_of(line: &str, key: &str) -> Option<Vec<usize>> {
    let rest = &line[line.find(&format!("\"{key}\": ["))? + key.len() + 5..];
    rest[..rest.find(']')?].split(',').map(str::trim).filter(|n| !n.is_empty()).map(|n| n.parse().ok()).collect()
}

impl List {
    /// Reads a list as `web_pack` writes it (one car, track or file per line).
    pub fn parse(text: &str) -> Result<List, String> {
        if !text.contains("\"format\": \"rustyac-preview-pack-1\"") {
            return Err("not a required-files list (rustyac-preview-pack-1)".into());
        }
        let mut list = List::default();
        let mut section = "";
        for line in text.lines() {
            let t = line.trim_start();
            for name in ["cars", "tracks", "files"] {
                if t.starts_with(&format!("\"{name}\": [")) && !t.contains(']') {
                    section = name;
                }
            }
            if t.starts_with("\"shared\": ") {
                list.shared = numbers_of(t, "shared").ok_or("a broken shared list")?;
            }
            if t.starts_with("\"launcher\": ") {
                list.launcher = numbers_of(t, "launcher").ok_or("a broken launcher list")?;
            }
            if t.starts_with("{\"id\": ") {
                let item = Item { id: text_of(t, "id").ok_or("a car or track without an id")?, layout: text_of(t, "layout").unwrap_or_default(), files: numbers_of(t, "files").ok_or("an item without files")? };
                match section {
                    "cars" => list.cars.push(item),
                    "tracks" => list.tracks.push(item),
                    _ => return Err("an item outside cars and tracks".into()),
                }
            } else if t.starts_with("{\"path\": ") && section == "files" {
                let bytes = t.split("\"bytes\": ").nth(1).and_then(|r| r.split([',', '}']).next()).and_then(|n| n.trim().parse().ok()).ok_or("a file without its size")?;
                list.files.push((text_of(t, "path").ok_or("a file without a path")?, bytes));
            }
        }
        let n = list.files.len();
        if list.cars.iter().chain(&list.tracks).flat_map(|i| &i.files).chain(&list.shared).chain(&list.launcher).any(|&i| i >= n) {
            return Err("a file number past the list's files".into());
        }
        Ok(list)
    }

    /// The list's files that `exists` says are not there.
    pub fn missing<'a>(&'a self, exists: &dyn Fn(&str) -> bool) -> Vec<&'a str> {
        self.files.iter().map(|(p, _)| p.as_str()).filter(|p| !exists(p)).collect()
    }

    /// The files of a car (`cars`) or of every layout of a track (`tracks`); `None`: not in the list.
    pub fn item_files(&self, kind: &str, id: &str) -> Option<Vec<&str>> {
        let items = if kind == "cars" { &self.cars } else { &self.tracks };
        let mut all: Vec<usize> = items.iter().filter(|i| i.id.eq_ignore_ascii_case(id)).flat_map(|i| i.files.iter().copied()).collect();
        if all.is_empty() && !items.iter().any(|i| i.id.eq_ignore_ascii_case(id)) {
            return None;
        }
        all.sort_unstable();
        all.dedup();
        Some(all.into_iter().map(|i| self.files[i].0.as_str()).collect())
    }
}

/// Missing files grouped as a player reads them: a car or track (`content/cars/<id>`), else the
/// first two folders (`system/data`), each with its count, in the list's order.
pub fn grouped(missing: &[&str]) -> Vec<(String, usize)> {
    let mut groups: Vec<(String, usize)> = Vec::new();
    for path in missing {
        let parts: Vec<&str> = path.split('/').collect();
        let n = if parts.first().is_some_and(|p| p.eq_ignore_ascii_case("content")) { 3 } else { 2 };
        let group = parts[..n.min(parts.len().saturating_sub(1)).max(1)].join("/");
        match groups.iter_mut().find(|(g, _)| *g == group) {
            Some((_, count)) => *count += 1,
            None => groups.push((group, 1)),
        }
    }
    groups
}

/// The player's message for a folder that misses base files: what is missing, named.
pub fn incomplete(missing: &[&str]) -> String {
    let groups = grouped(missing);
    let shown: Vec<String> = groups.iter().take(12).map(|(g, n)| format!("{g} ({n} file{})", if *n == 1 { "" } else { "s" })).collect();
    let more = if groups.len() > 12 { format!(", and {} more", groups.len() - 12) } else { String::new() };
    format!("this folder is not a complete Assetto Corsa: {} files of the base game are missing: {}{more}", missing.len(), shown.join(", "))
}

/// `base.json` and the DLC lists, as shipped beside the program.
#[derive(Clone, Debug, Default)]
pub struct Lists {
    pub base: List,
    /// (Steam app, its list)
    pub dlcs: Vec<(u32, List)>,
}

/// "car" for `cars`, "track" for `tracks`.
fn noun(kind: &str) -> &'static str {
    if kind == "cars" {
        "car"
    } else {
        "track"
    }
}

fn dlc_name(app: u32) -> &'static str {
    DLCS.iter().find(|d| d.1 == app).map_or("a", |d| d.2)
}

impl Lists {
    /// Reads `base.json` and every `dlc_<app>.json` of the folder.
    pub fn load(folder: &Path) -> Result<Lists, String> {
        let read = |name: &str| std::fs::read_to_string(folder.join(name)).map_err(|e| format!("{}: {e}", folder.join(name).display())).and_then(|t| List::parse(&t).map_err(|e| format!("{name}: {e}")));
        let mut lists = Lists { base: read("base.json")?, dlcs: Vec::new() };
        for (_, app, _) in DLCS {
            let name = format!("dlc_{app}.json");
            if folder.join(&name).is_file() {
                lists.dlcs.push((app, read(&name)?));
            }
        }
        Ok(lists)
    }

    /// A chosen full install: `Err` with the player's message when base files are missing.
    pub fn check_full(&self, exists: &dyn Fn(&str) -> bool) -> Result<(), String> {
        let missing = self.base.missing(exists);
        if missing.is_empty() {
            Ok(())
        } else {
            Err(incomplete(&missing))
        }
    }

    /// The DLC whose list holds this car or track (`None`: the base game's or nobody's).
    pub fn dlc_of(&self, kind: &str, id: &str) -> Option<(u32, Vec<&str>)> {
        self.dlcs.iter().find_map(|(app, list)| list.item_files(kind, id).map(|files| (*app, files)))
    }

    /// Is this DLC's content there (every file of its list)?
    pub fn dlc_present(&self, app: u32, exists: &dyn Fn(&str) -> bool) -> bool {
        self.dlcs.iter().find(|(a, _)| *a == app).is_some_and(|(_, list)| list.missing(exists).is_empty())
    }

    /// A picked car (`cars`) or track (`tracks`) of a DLC list: `Err("this car needs the <name>
    /// DLC")` when its files are missing. `Ok(true)`: checked by a list; `Ok(false)`: in no
    /// list (base game: checked with the folder; else check it with [`loads`]).
    pub fn check_pick(&self, kind: &str, id: &str, exists: &dyn Fn(&str) -> bool) -> Result<bool, String> {
        if let Some((app, files)) = self.dlc_of(kind, id) {
            if files.iter().any(|f| !exists(f)) {
                return Err(format!("this {} needs the {} DLC", noun(kind), dlc_name(app)));
            }
            return Ok(true);
        }
        Ok(self.base.item_files(kind, id).is_some())
    }
}

/// "Does it load", for a car or track no list knows (a mod, a partial import's): under `root`
/// (AC's folder as the game sees it, through [`crate::vfs`]), the car's data (`data.acd` or
/// `data/`) and the models its `lods.ini` names, or the track layout's models, must be there
/// and open as kn5 files. A model that carries a DLC's key needs that DLC's files
/// (`dlc_present`, through `lists`): "this car needs the <name> DLC".
pub fn loads(root: &Path, kind: &str, id: &str, layout: &str, lists: &Lists, exists: &dyn Fn(&str) -> bool) -> Result<(), String> {
    let folder = root.join("content").join(kind).join(id);
    if !folder.vfs_is_dir() {
        return Err(format!("the {} {id} is not there", noun(kind)));
    }
    let models: Vec<std::path::PathBuf> = if kind == "cars" {
        let data = folder.join("data");
        if !crate::acd::exists(&data.join("car.ini")) {
            return Err(format!("the car {id} has no data (data.acd or data/car.ini)"));
        }
        let lods = crate::acd::read(&data.join("lods.ini"))?.ok_or_else(|| format!("the car {id} has no lods.ini"))?;
        let lods = String::from_utf8_lossy(&lods);
        let files: Vec<_> = lods.lines().filter_map(|l| l.trim().strip_prefix("FILE=")).map(|f| folder.join(f.split(';').next().unwrap_or("").trim())).collect();
        if files.is_empty() {
            return Err(format!("the car {id}'s lods.ini names no model"));
        }
        files
    } else {
        crate::track_files::TrackFiles::find(&folder, layout)?.models.into_iter().map(|m| m.file).collect()
    };
    for model in models {
        if !model.vfs_is_file() {
            return Err(format!("{}'s model {} is missing", id, model.display()));
        }
        let key = kn5_file_key(&model);
        if let Some(&(_, app, name)) = DLCS.iter().find(|d| d.0 == key) {
            if !lists.dlc_present(app, exists) {
                return Err(format!("this {} needs the {name} DLC", noun(kind)));
            }
        }
        crate::kn5::Kn5::open(&model).map_err(|e| format!("{id} does not load: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = "{\n  \"format\": \"rustyac-preview-pack-1\",\n  \"cars\": [\n    {\"id\": \"abarth500\", \"layout\": \"\", \"name\": \"Abarth \\\"500\\\"\", \"bytes\": 3, \"files\": [0, 1]}\n  ],\n  \"tracks\": [\n    {\"id\": \"ks_nordschleife\", \"layout\": \"endurance\", \"name\": \"N\", \"bytes\": 4, \"files\": [2]},\n    {\"id\": \"ks_nordschleife\", \"layout\": \"nordschleife\", \"name\": \"N\", \"bytes\": 4, \"files\": [2, 3]}\n  ],\n  \"shared\": [4],\n  \"launcher\": [5],\n  \"bytes\": 10,\n  \"files\": [\n    {\"path\": \"content/cars/abarth500/data.acd\", \"bytes\": 1, \"sha256\": \"\"},\n    {\"path\": \"content/cars/abarth500/abarth500.kn5\", \"bytes\": 2, \"sha256\": \"\"},\n    {\"path\": \"content/tracks/ks_nordschleife/a.kn5\", \"bytes\": 3, \"sha256\": \"\"},\n    {\"path\": \"content/tracks/ks_nordschleife/b \\u00e9.kn5\", \"bytes\": 1, \"sha256\": \"\"},\n    {\"path\": \"system/data/surfaces.ini\", \"bytes\": 1, \"sha256\": \"\"},\n    {\"path\": \"launcher/themes/default/index.html\", \"bytes\": 2, \"sha256\": \"\"}\n  ]\n}\n";

    #[test]
    fn a_list_reads_back() {
        let list = List::parse(LIST).unwrap();
        assert_eq!(list.cars, [Item { id: "abarth500".into(), layout: "".into(), files: vec![0, 1] }]);
        assert_eq!(list.tracks.len(), 2);
        assert_eq!((list.shared.as_slice(), list.launcher.as_slice()), (&[4][..], &[5][..]));
        assert_eq!(list.files[3], ("content/tracks/ks_nordschleife/b \u{e9}.kn5".to_string(), 1));
        assert_eq!(list.item_files("tracks", "KS_Nordschleife").unwrap().len(), 2);
        assert_eq!(list.item_files("cars", "nobody"), None);
        assert!(List::parse("{}").is_err());
        assert!(List::parse(&LIST.replace("[4]", "[40]")).is_err());
    }

    #[test]
    fn the_checks_name_what_is_missing() {
        let base = List::parse(LIST).unwrap();
        let dlc = List::parse(&LIST.replace("abarth500", "ks_toyota_ae86").replace("ks_nordschleife", "ks_brands_hatch")).unwrap();
        let lists = Lists { base, dlcs: vec![(467980, dlc)] };
        assert_eq!(lists.check_full(&|_| true), Ok(()));
        let err = lists.check_full(&|p| !p.starts_with("content/cars/abarth500") && !p.starts_with("system")).unwrap_err();
        assert_eq!(err, "this folder is not a complete Assetto Corsa: 3 files of the base game are missing: content/cars/abarth500 (2 files), system/data (1 file)");
        // a DLC car: checked against its list only when picked
        let no_dlc = |p: &str| !p.contains("ks_toyota_ae86");
        assert_eq!(lists.check_pick("cars", "ks_toyota_ae86", &no_dlc), Err("this car needs the Japanese Pack DLC".to_string()));
        assert_eq!(lists.check_pick("cars", "ks_toyota_ae86", &|_| true), Ok(true));
        assert_eq!(lists.check_pick("cars", "abarth500", &no_dlc), Ok(true));
        assert_eq!(lists.check_pick("cars", "my_mod", &no_dlc), Ok(false));
        assert!(!lists.dlc_present(467980, &no_dlc) && lists.dlc_present(467980, &|_| true) && !lists.dlc_present(1, &|_| true));
        assert_eq!(grouped(&["acs.exe", "a/b/c/d"]), [("acs.exe".to_string(), 1), ("a/b".to_string(), 1)]);
    }

    #[test]
    fn a_models_header_names_its_dlc() {
        let mut head = b"sc6969".to_vec();
        head.extend(6u32.to_le_bytes());
        head.extend(0x3b787u32.to_le_bytes());
        assert_eq!(kn5_key(&head), 0x3b787);
        head[6] = 5;
        assert_eq!(kn5_key(&head), 0);
        assert_eq!(kn5_key(b"sc69"), 0);
    }

    #[test]
    fn a_mod_is_checked_by_whether_it_loads() {
        let dir = crate::zip::tests::scratch("loads");
        let car = dir.join("content/cars/my_mod");
        std::fs::create_dir_all(car.join("data")).unwrap();
        let lists = Lists::default();
        let all = |_: &str| true;
        assert!(loads(&dir, "cars", "nobody", "", &lists, &all).unwrap_err().contains("not there"));
        assert!(loads(&dir, "cars", "my_mod", "", &lists, &all).unwrap_err().contains("no data"));
        std::fs::write(car.join("data/car.ini"), "[HEADER]\n").unwrap();
        std::fs::write(car.join("data/lods.ini"), "[LOD_0]\nFILE=my_mod.kn5 ; the body\n").unwrap();
        assert!(loads(&dir, "cars", "my_mod", "", &lists, &all).unwrap_err().contains("missing"));
        // a model with the Japanese Pack's key, the pack not there
        let mut kn5 = b"sc6969".to_vec();
        kn5.extend(6u32.to_le_bytes());
        kn5.extend(0x3b787u32.to_le_bytes());
        std::fs::write(car.join("my_mod.kn5"), &kn5).unwrap();
        let dlc = Lists { dlcs: vec![(467980, List { files: vec![("content/cars/ks_toyota_ae86/data.acd".into(), 1)], ..List::default() })], ..Lists::default() };
        assert_eq!(loads(&dir, "cars", "my_mod", "", &dlc, &|_| false), Err("this car needs the Japanese Pack DLC".to_string()));
        // the pack there: the model itself is read (this one is cut short)
        assert!(loads(&dir, "cars", "my_mod", "", &dlc, &all).unwrap_err().contains("does not load"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The real lists, when this PC has them (made by `web_pack --required`), against the real
    /// install: complete, and a base car loads.
    #[test]
    fn the_real_lists_check_the_real_install() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../re/scratch/taskP3/required");
        let (Ok(lists), Some(root)) = (Lists::load(&folder), crate::install::ac_root()) else { return };
        assert_eq!(lists.dlcs.len(), 11);
        let exists = |p: &str| root.join(p).is_file();
        assert_eq!(lists.check_full(&exists), Ok(()));
        eprintln!("{} base files there in {}", lists.base.files.len(), root.display());
        assert_eq!(lists.check_pick("cars", "ks_mazda_mx5_cup", &exists), Ok(true));
        assert_eq!(loads(&root, "cars", "abarth500", "", &lists, &exists), Ok(()));
    }
}
