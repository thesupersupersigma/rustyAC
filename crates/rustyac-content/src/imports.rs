// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the player chose as the game's files, on every platform (`docs/platforms.md`, section
//! 5 and Task P3c): a full Assetto Corsa folder, or a folder or zip that holds only cars and / or
//! tracks (`content/cars|tracks/...`, `cars|tracks/...`, or one car or track folder).
//!
//! A partial import **adds** content on top of the full install (or the browser's preview pack),
//! never replaces it: [`Layers`] keeps the base and the extra roots and reads every file in
//! place (no copying into AC's folder), as a [`Vfs`] the game's readers already go through.
//! [`Remembered`] is the choice kept between runs, in rustyAC's own settings file.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::vfs::Vfs;
use crate::zip::Zip;

/// A folder on the disk or a zip, read in place. Paths inside it are relative, `/` between parts.
#[derive(Clone)]
pub enum Source {
    Folder(PathBuf),
    Zip(Arc<Zip>),
}

impl Source {
    /// A folder, or a file ending in `.zip`.
    pub fn open(path: &Path) -> io::Result<Source> {
        if path.is_dir() {
            Ok(Source::Folder(path.to_path_buf()))
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
            Ok(Source::Zip(Arc::new(Zip::open(path)?)))
        } else {
            Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{}: not a folder or a .zip", path.display())))
        }
    }

    /// The folder or zip itself.
    pub fn path(&self) -> &Path {
        match self {
            Source::Folder(p) => p,
            Source::Zip(z) => z.path(),
        }
    }

    fn disk(&self, rel: &str) -> PathBuf {
        let mut p = self.path().to_path_buf();
        p.extend(rel.split('/').filter(|s| !s.is_empty()));
        p
    }

    pub fn read_dir(&self, rel: &str) -> io::Result<Vec<(String, bool)>> {
        match self {
            Source::Folder(_) => Ok(std::fs::read_dir(self.disk(rel))?.flatten().map(|e| (e.file_name().to_string_lossy().into_owned(), e.path().is_dir())).collect()),
            Source::Zip(z) => z.read_dir(rel),
        }
    }

    pub fn is_file(&self, rel: &str) -> bool {
        match self {
            Source::Folder(_) => self.disk(rel).is_file(),
            Source::Zip(z) => z.is_file(rel),
        }
    }

    pub fn is_dir(&self, rel: &str) -> bool {
        match self {
            Source::Folder(_) => self.disk(rel).is_dir(),
            Source::Zip(z) => z.is_dir(rel),
        }
    }

    pub fn read(&self, rel: &str) -> io::Result<Vec<u8>> {
        match self {
            Source::Folder(_) => std::fs::read(self.disk(rel)),
            Source::Zip(z) => z.read(rel),
        }
    }
}

/// What a chosen folder or zip holds.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// The whole game (`content` with cars or tracks, and `system`).
    Full,
    /// Cars and / or tracks: `cars/` and / or `tracks/` in [`Import::at`].
    Content,
    /// One car's folder, with the car's id (its folder's name).
    Car(String),
    /// One track's folder, with the track's id.
    Track(String),
}

/// A recognised folder or zip.
#[derive(Clone)]
pub struct Import {
    pub source: Source,
    pub kind: Kind,
    /// Where inside the source the recognised folder is (`""`: its top; else `a/b`).
    pub at: String,
}

fn join(a: &str, b: &str) -> String {
    match (a.is_empty(), b.is_empty()) {
        (true, _) => b.to_string(),
        (_, true) => a.to_string(),
        _ => format!("{a}/{b}"),
    }
}

/// Recognises what `source` holds. `name` is the id a single car or track at its top gets (the
/// folder's or the zip's name without `.zip`). A folder or zip with nothing else in it than one
/// folder is looked into (a zip of a folder), a few levels deep.
pub fn detect(source: Source, name: &str) -> Option<Import> {
    let mut at = String::new();
    let mut name = name.to_string();
    for _ in 0..4 {
        let entries = source.read_dir(&at).ok()?;
        let has = |n: &str, dir: bool| entries.iter().any(|(e, d)| *d == dir && e.eq_ignore_ascii_case(n));
        let found = |kind| Some(Import { source: source.clone(), kind, at: at.clone() });
        let holds_content = |folder: &str| ["cars", "tracks"].iter().any(|k| source.is_dir(&join(folder, k)));
        if has("content", true) && has("system", true) && holds_content(&join(&at, "content")) {
            return found(Kind::Full);
        }
        if has("content", true) && holds_content(&join(&at, "content")) {
            return found(Kind::Content).map(|i| Import { at: join(&at, "content"), ..i });
        }
        if has("cars", true) || has("tracks", true) {
            return found(Kind::Content);
        }
        // a car: data.acd or data/, and ui/ui_car.json (a track has no ui_car.json)
        if (has("data.acd", false) || has("data", true)) && source.is_file(&join(&at, "ui/ui_car.json")) {
            return found(Kind::Car(name));
        }
        // a track: models*.ini or a .kn5 at its top, and ui/
        let lower = |e: &String| e.to_ascii_lowercase();
        if has("ui", true) && entries.iter().any(|(e, d)| !d && (lower(e).ends_with(".kn5") || (lower(e).starts_with("models") && lower(e).ends_with(".ini")))) {
            return found(Kind::Track(name));
        }
        match entries.as_slice() {
            [(only, true)] => {
                at = join(&at, only);
                name = only.clone();
            }
            _ => return None,
        }
    }
    None
}

impl Import {
    /// The import's cars (`cars`) or tracks (`tracks`), each with its folder inside the source.
    pub fn items(&self, kind: &str) -> Vec<(String, String)> {
        match &self.kind {
            Kind::Car(id) if kind == "cars" => vec![(id.clone(), self.at.clone())],
            Kind::Track(id) if kind == "tracks" => vec![(id.clone(), self.at.clone())],
            Kind::Full | Kind::Content => {
                let folder = join(&if self.kind == Kind::Full { join(&self.at, "content") } else { self.at.clone() }, kind);
                self.source.read_dir(&folder).unwrap_or_default().into_iter().filter(|(_, dir)| *dir).map(|(id, _)| (id.clone(), join(&folder, &id))).collect()
            }
            _ => Vec::new(),
        }
    }

    /// The folder of one car or track inside the source, if the import has it (any case).
    fn item(&self, kind: &str, id: &str) -> Option<String> {
        self.items(kind).into_iter().find(|(i, _)| i.eq_ignore_ascii_case(id)).map(|(_, at)| at)
    }
}

/// The game's files: the base (a full install) under `root`, the path the game is given as AC's
/// folder, with the extra imports' cars and tracks laid over its `content/cars` and
/// `content/tracks`. What the base has is never replaced; among the extras the first one wins.
/// Paths outside `root` are the disk's.
pub struct Layers {
    pub root: PathBuf,
    pub base: Option<Import>,
    pub extra: Vec<Import>,
}

impl Layers {
    /// The path the game sees as AC's folder for a base import: its folder, or the zip's path
    /// with the folder inside it (`/x/ac.zip/assettocorsa`).
    pub fn root_of(base: &Import) -> PathBuf {
        let mut p = base.source.path().to_path_buf();
        p.extend(base.at.split('/').filter(|s| !s.is_empty()));
        p
    }

    /// Opens a choice: `root` must be the whole game (a folder or zip), each extra a folder or
    /// zip of cars and / or tracks. `Err` is the player's message.
    pub fn open(root: Option<&Path>, extra: &[PathBuf]) -> Result<Layers, String> {
        let root = root.ok_or("Assetto Corsa's install folder wasn't found: choose the folder with the game files")?;
        let name = |p: &Path| p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let base = Source::open(root).map_err(|e| format!("{}: {e}", root.display()))?;
        let base = match detect(base, &name(root)) {
            Some(i) if i.kind == Kind::Full => i,
            Some(_) => return Err(format!("{} holds cars or tracks only, not the whole game: choose the folder with the game files, then add this one to the extra cars and tracks", root.display())),
            None => return Err(format!("{} is not Assetto Corsa's folder (no content and system folders in it): choose the folder with the game files", root.display())),
        };
        let mut layers = Layers { root: Layers::root_of(&base), base: Some(base), extra: Vec::new() };
        for p in extra {
            let source = Source::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
            layers.extra.push(detect(source, &name(p)).ok_or_else(|| format!("{} holds no cars or tracks (content/cars or content/tracks, cars/ or tracks/, or one car or track folder)", p.display()))?);
        }
        Ok(layers)
    }

    /// Is this just a folder on the disk, with nothing laid over it (no mount needed)?
    pub fn is_plain(&self) -> bool {
        self.extra.is_empty() && matches!(&self.base, Some(Import { source: Source::Folder(_), .. }))
    }

    /// Makes this the game's files for every later read ([`crate::vfs::mount`] when a zip or
    /// extra roots are in it) and its root AC's folder ([`crate::install::set_ac_root`]).
    pub fn mount(self) {
        crate::install::set_ac_root(Some(self.root.clone()));
        if !self.is_plain() {
            crate::vfs::mount(Some(Arc::new(self)));
        }
    }

    /// Every car (`cars`) or track (`tracks`) id, the base's first, without repeats (any case).
    pub fn ids(&self, kind: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for layer in self.base.iter().chain(&self.extra) {
            for (id, _) in layer.items(kind) {
                if !out.iter().any(|o| o.eq_ignore_ascii_case(&id)) {
                    out.push(id);
                }
            }
        }
        out
    }

    /// Which source holds this path and where inside it; `None`: not under `root`.
    pub fn route(&self, path: &Path) -> Option<(Source, String)> {
        let parts = parts(path.strip_prefix(&self.root).ok()?);
        if parts.len() >= 3 && parts[0].eq_ignore_ascii_case("content") && ["cars", "tracks"].iter().any(|k| parts[1].eq_ignore_ascii_case(k)) {
            let kind = parts[1].to_ascii_lowercase();
            for layer in self.base.iter().chain(&self.extra) {
                if let Some(at) = layer.item(&kind, &parts[2]) {
                    return Some((layer.source.clone(), join(&at, &parts[3..].join("/"))));
                }
            }
        }
        let base = self.base.as_ref()?;
        Some((base.source.clone(), join(&base.at, &parts.join("/"))))
    }

    fn is_list(&self, path: &Path) -> Option<&'static str> {
        let parts: Vec<String> = parts(path.strip_prefix(&self.root).ok()?).iter().map(|p| p.to_ascii_lowercase()).collect();
        match parts.as_slice() {
            [c, k] if c == "content" && k == "cars" => Some("cars"),
            [c, k] if c == "content" && k == "tracks" => Some("tracks"),
            _ => None,
        }
    }
}

/// A relative path's parts (`/` or `\` between them).
fn parts(rel: &Path) -> Vec<String> {
    rel.to_string_lossy().split(['/', '\\']).filter(|s| !s.is_empty() && *s != ".").map(str::to_string).collect()
}

fn not_found() -> io::Error {
    io::Error::from(io::ErrorKind::NotFound)
}

impl Vfs for Layers {
    fn read(&self, path: &Path) -> io::Result<Arc<[u8]>> {
        match self.route(path) {
            Some((source, rel)) => source.read(&rel).map(Into::into),
            None if path.starts_with(&self.root) => Err(not_found()),
            None => std::fs::read(path).map(Into::into),
        }
    }

    fn is_file(&self, path: &Path) -> bool {
        match self.route(path) {
            Some((source, rel)) => source.is_file(&rel),
            None => !path.starts_with(&self.root) && path.is_file(),
        }
    }

    fn is_dir(&self, path: &Path) -> bool {
        if self.is_list(path).is_some() {
            return true;
        }
        match self.route(path) {
            Some((source, rel)) => source.is_dir(&rel),
            None => !path.starts_with(&self.root) && path.is_dir(),
        }
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<(String, bool)>> {
        if let Some(kind) = self.is_list(path) {
            return Ok(self.ids(kind).into_iter().map(|id| (id, true)).collect());
        }
        match self.route(path) {
            Some((source, rel)) => source.read_dir(&rel),
            None if path.starts_with(&self.root) => Err(not_found()),
            None => Ok(std::fs::read_dir(path)?.flatten().map(|e| (e.file_name().to_string_lossy().into_owned(), e.path().is_dir())).collect()),
        }
    }
}

/// The choice kept between runs: the full install (a folder or zip) and the extra imports.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Remembered {
    pub root: Option<PathBuf>,
    pub extra: Vec<PathBuf>,
}

impl Remembered {
    /// rustyAC's own settings file: `$XDG_CONFIG_HOME/rustyAC/game_files.txt` (else
    /// `~/.config/rustyAC/`), `~/Library/Application Support/rustyAC/` on macOS,
    /// `%APPDATA%\rustyAC\` on Windows.
    pub fn file() -> Option<PathBuf> {
        let dir = if cfg!(windows) {
            PathBuf::from(std::env::var_os("APPDATA")?)
        } else if cfg!(target_os = "macos") {
            PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support")
        } else {
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".config")))?
        };
        Some(dir.join("rustyAC").join("game_files.txt"))
    }

    /// `root=<path>` and `extra=<path>` lines; anything else is skipped.
    pub fn parse(text: &str) -> Remembered {
        let mut r = Remembered::default();
        for line in text.lines() {
            match line.split_once('=') {
                Some(("root", p)) if !p.is_empty() => r.root = Some(PathBuf::from(p)),
                Some(("extra", p)) if !p.is_empty() => r.extra.push(PathBuf::from(p)),
                _ => {}
            }
        }
        r
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from("# rustyAC: where the game's files are (written by the launcher)\n");
        for (k, p) in self.root.iter().map(|p| ("root", p)).chain(self.extra.iter().map(|p| ("extra", p))) {
            out.push_str(&format!("{k}={}\n", p.display()));
        }
        out
    }

    /// The game's start (the launcher names its choice in `RUSTYAC_GAME_FILES`): the remembered
    /// file read, opened and mounted.
    pub fn mount_file(file: &Path) -> Result<(), String> {
        let r = Remembered::load(file);
        Layers::open(r.root.as_deref(), &r.extra)?.mount();
        Ok(())
    }

    pub fn load(file: &Path) -> Remembered {
        std::fs::read_to_string(file).map(|t| Remembered::parse(&t)).unwrap_or_default()
    }

    pub fn save(&self, file: &Path) -> io::Result<()> {
        if let Some(dir) = file.parent() {
            crate::guard::create_dir_all(dir)?;
        }
        crate::guard::write(file, self.to_text().as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zip::tests::{make_zip, scratch};

    fn tree(root: &Path, files: &[&str]) {
        for f in files {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, f.as_bytes()).unwrap();
        }
    }

    fn kind(path: &Path) -> Option<(Kind, String)> {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        detect(Source::open(path).ok()?, &name).map(|i| (i.kind, i.at))
    }

    const FULL: [&str; 5] = ["content/cars/abarth500/data.acd", "content/cars/abarth500/ui/ui_car.json", "content/tracks/monza/monza.kn5", "content/tracks/monza/ui/ui_track.json", "system/data/surfaces.ini"];

    #[test]
    fn every_kind_of_folder_and_zip_is_recognised() {
        let dir = scratch("detect");
        type Case<'a> = (&'a str, &'a [&'a str], Option<(Kind, &'a str)>);
        let cases: [Case; 9] = [
            ("full", &FULL, Some((Kind::Full, ""))),
            ("content_root", &["mods/content/cars/my_car/data/car.ini", "mods/content/cars/my_car/ui/ui_car.json"], Some((Kind::Content, "mods/content"))),
            ("cars_root", &["cars/a/data.acd", "tracks/t/t.kn5"], Some((Kind::Content, ""))),
            ("one_car", &["my_car/data/car.ini", "my_car/ui/ui_car.json", "my_car/my_car.kn5"], Some((Kind::Car("my_car".into()), "my_car"))),
            ("one_car_acd", &["data.acd", "ui/ui_car.json"], Some((Kind::Car("one_car_acd".into()), ""))),
            ("one_track", &["my_track/models_gp.ini", "my_track/ui/gp/ui_track.json", "my_track/data/surfaces.ini"], Some((Kind::Track("my_track".into()), "my_track"))),
            ("track_kn5", &["Track.KN5", "ui/ui_track.json"], Some((Kind::Track("track_kn5".into()), ""))),
            ("nothing", &["readme.txt", "pics/a.png"], None),
            ("car_without_ui", &["data.acd", "car.kn5"], None),
        ];
        for (name, files, want) in cases {
            let folder = dir.join(name);
            tree(&folder, files);
            let want = want.map(|(k, at)| (k, at.to_string()));
            assert_eq!(kind(&folder), want, "folder {name}");
            // the same as a zip
            let entries: Vec<(&str, &[u8])> = files.iter().map(|f| (*f, f.as_bytes())).collect();
            let zip = dir.join(format!("{name}.zip"));
            std::fs::write(&zip, make_zip(&entries, true, false)).unwrap();
            assert_eq!(kind(&zip), want, "zip {name}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn extra_roots_add_cars_and_tracks_without_replacing_the_base() {
        let dir = scratch("layers");
        tree(&dir.join("ac"), &FULL);
        tree(&dir.join("mods"), &["cars/my_car/data.acd", "cars/my_car/ui/ui_car.json", "cars/abarth500/data.acd", "cars/abarth500/ui/ui_car.json"]);
        let track: Vec<(&str, &[u8])> = vec![("my_track/my_track.kn5", b"kn5"), ("my_track/ui/ui_track.json", b"{}")];
        std::fs::write(dir.join("track.zip"), make_zip(&track, true, true)).unwrap();
        let open = |p: &str| detect(Source::open(&dir.join(p)).unwrap(), p).unwrap();
        let base = open("ac");
        let layers = Layers { root: Layers::root_of(&base), base: Some(base), extra: vec![open("mods"), open("track.zip")] };
        assert_eq!(layers.ids("cars"), ["abarth500", "my_car"]);
        assert_eq!(layers.ids("tracks"), ["monza", "my_track"]);
        let root = dir.join("ac");
        // the base's abarth500, not the mods folder's
        assert_eq!(&*layers.read(&root.join("content/cars/abarth500/data.acd")).unwrap(), b"content/cars/abarth500/data.acd");
        assert_eq!(&*layers.read(&root.join("content/cars/my_car/data.acd")).unwrap(), b"cars/my_car/data.acd");
        assert_eq!(&*layers.read(&root.join("content").join("tracks").join("MY_TRACK").join("my_track.kn5")).unwrap(), b"kn5");
        assert!(layers.is_dir(&root.join("content/tracks/my_track/ui")) && layers.is_file(&root.join("content/tracks/my_track/ui/ui_track.json")));
        let mut cars = layers.read_dir(&root.join("content/cars")).unwrap();
        cars.sort();
        assert_eq!(cars, [("abarth500".to_string(), true), ("my_car".to_string(), true)]);
        assert_eq!(&*layers.read(&root.join("system/data/surfaces.ini")).unwrap(), b"system/data/surfaces.ini");
        assert!(layers.read(&root.join("content/cars/nobody/data.acd")).is_err());
        // outside the root: the disk
        assert!(layers.is_file(&dir.join("track.zip")));
        // a zip as the base: its path stands for AC's folder
        let full: Vec<(&str, &[u8])> = FULL.iter().map(|f| (*f, f.as_bytes())).collect();
        let named: Vec<(String, &[u8])> = full.iter().map(|(f, b)| (format!("assettocorsa/{f}"), *b)).collect();
        let named: Vec<(&str, &[u8])> = named.iter().map(|(f, b)| (f.as_str(), *b)).collect();
        std::fs::write(dir.join("ac.zip"), make_zip(&named, true, true)).unwrap();
        let base = open("ac.zip");
        let root = Layers::root_of(&base);
        assert_eq!(root, dir.join("ac.zip").join("assettocorsa"));
        let layers = Layers { root: root.clone(), base: Some(base), extra: vec![open("mods")] };
        assert_eq!(&*layers.read(&root.join("system/data/surfaces.ini")).unwrap(), b"system/data/surfaces.ini");
        assert_eq!(&*layers.read(&root.join("content/cars/my_car/data.acd")).unwrap(), b"cars/my_car/data.acd");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_choice_opens_or_says_why_not() {
        let dir = scratch("open");
        tree(&dir.join("ac"), &FULL);
        tree(&dir.join("cars_only"), &["cars/a/data.acd", "cars/a/ui/ui_car.json"]);
        tree(&dir.join("junk"), &["readme.txt"]);
        let layers = Layers::open(Some(&dir.join("ac")), &[dir.join("cars_only")]).unwrap();
        assert!(!layers.is_plain() && Layers::open(Some(&dir.join("ac")), &[]).unwrap().is_plain());
        assert_eq!(layers.ids("cars"), ["abarth500", "a"]);
        assert!(Layers::open(None, &[]).err().unwrap().contains("wasn't found: choose the folder with the game files"));
        assert!(Layers::open(Some(&dir.join("cars_only")), &[]).err().unwrap().contains("holds cars or tracks only"));
        assert!(Layers::open(Some(&dir.join("junk")), &[]).err().unwrap().contains("is not Assetto Corsa's folder"));
        assert!(Layers::open(Some(&dir.join("ac")), &[dir.join("junk")]).err().unwrap().contains("holds no cars or tracks"));
        assert!(Layers::open(Some(&dir.join("nowhere")), &[]).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_choice_reads_back() {
        let r = Remembered { root: Some(PathBuf::from("/games/ac.zip")), extra: vec![PathBuf::from("/mods/a"), PathBuf::from("/mods/b.zip")] };
        assert_eq!(Remembered::parse(&r.to_text()), r);
        assert_eq!(Remembered::parse("junk\nroot=\nextra=/x\n"), Remembered { root: None, extra: vec![PathBuf::from("/x")] });
        assert!(Remembered::file().is_none_or(|f| f.ends_with("rustyAC/game_files.txt")));
    }
}
