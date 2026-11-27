// SPDX-License-Identifier: GPL-3.0-or-later

//! The files the launcher's code reads and writes: the disk, or a store of the page's own in
//! the browser (rustyAC's, not the launcher's: .NET's `File` and `Directory` always mean the
//! disk). Every file call of the bridge goes through [`net`](crate::net) to here, so the
//! browser mounts its store with [`mount`] and the same code runs on it. Paths are the
//! launcher's own (`\` or `/`, any case, as Windows takes them).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::io::{Error, ErrorKind, Result};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rustyac_content::imports::{Layers, Remembered};

pub trait Files: Send + Sync {
    fn read(&self, path: &str) -> Result<Vec<u8>>;
    /// Makes or replaces the file; its folder must be there.
    fn write(&self, path: &str, bytes: &[u8]) -> Result<()>;
    fn is_file(&self, path: &str) -> bool;
    fn is_dir(&self, path: &str) -> bool;
    /// The folder's entries, (name, is a folder), in the order Windows lists them.
    fn list(&self, folder: &str) -> Result<Vec<(String, bool)>>;
    fn create_dir_all(&self, path: &str) -> Result<()>;
    /// Fails when `to` is there.
    fn rename(&self, from: &str, to: &str) -> Result<()>;
    fn remove_file(&self, path: &str) -> Result<()>;
    fn remove_dir_all(&self, path: &str) -> Result<()>;
    fn modified(&self, path: &str) -> Option<SystemTime>;
    fn created(&self, path: &str) -> Option<SystemTime>;

    fn copy(&self, from: &str, to: &str) -> Result<()> {
        let bytes = self.read(from)?;
        self.write(to, &bytes)
    }

    fn append(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let mut all = self.read(path)?;
        all.extend_from_slice(bytes);
        self.write(path, &all)
    }

    fn len(&self, path: &str) -> Option<u64> {
        self.read(path).ok().map(|b| b.len() as u64)
    }
}

/// The disk, through `std::fs`.
pub struct Disk;

/// A launcher path for `std::fs`: as it is on Windows; elsewhere (wasm under WASI) its `\` as `/`.
fn os(path: &str) -> std::borrow::Cow<'_, str> {
    if cfg!(windows) {
        path.into()
    } else {
        path.replace('\\', "/").into()
    }
}

/// `op` on the path, then on the disk's spelling of it when it is not there as spelled (off
/// Windows, AC's names in another letter case: `vfs::on_disk`).
fn found<T>(path: &str, op: impl Fn(&std::path::Path) -> Result<T>) -> Result<T> {
    rustyac_content::vfs::or_on_disk(std::path::Path::new(&*os(path)), op)
}

impl Files for Disk {
    fn read(&self, path: &str) -> Result<Vec<u8>> {
        found(path, |p| std::fs::read(p))
    }
    fn write(&self, path: &str, bytes: &[u8]) -> Result<()> {
        rustyac_content::guard::write(&*os(path), bytes)
    }
    fn is_file(&self, path: &str) -> bool {
        let p = os(path);
        let p = std::path::Path::new(&*p);
        p.is_file() || rustyac_content::vfs::on_disk(p).is_file()
    }
    fn is_dir(&self, path: &str) -> bool {
        let p = os(path);
        let p = std::path::Path::new(&*p);
        p.is_dir() || rustyac_content::vfs::on_disk(p).is_dir()
    }
    fn list(&self, folder: &str) -> Result<Vec<(String, bool)>> {
        let mut out = Vec::new();
        for entry in found(folder, |p| std::fs::read_dir(p))? {
            let entry = entry?;
            out.push((entry.file_name().to_string_lossy().into_owned(), entry.file_type()?.is_dir()));
        }
        Ok(out)
    }
    fn create_dir_all(&self, path: &str) -> Result<()> {
        rustyac_content::guard::create_dir_all(&*os(path))
    }
    fn rename(&self, from: &str, to: &str) -> Result<()> {
        if std::path::Path::new(&*os(to)).exists() {
            return Err(Error::from(ErrorKind::AlreadyExists));
        }
        rustyac_content::guard::rename(&*os(from), &*os(to))
    }
    fn remove_file(&self, path: &str) -> Result<()> {
        rustyac_content::guard::remove_file(&*os(path))
    }
    fn remove_dir_all(&self, path: &str) -> Result<()> {
        rustyac_content::guard::remove_dir_all(&*os(path))
    }
    fn modified(&self, path: &str) -> Option<SystemTime> {
        found(path, |p| std::fs::metadata(p)).and_then(|m| m.modified()).ok()
    }
    fn created(&self, path: &str) -> Option<SystemTime> {
        found(path, |p| std::fs::metadata(p)).and_then(|m| m.created()).ok()
    }
    fn copy(&self, from: &str, to: &str) -> Result<()> {
        rustyac_content::guard::copy(&*os(from), &*os(to)).map(|_| ())
    }
    fn append(&self, path: &str, bytes: &[u8]) -> Result<()> {
        use std::io::Write as _;
        { rustyac_content::guard::check(std::path::Path::new(&*os(path)))?; std::fs::OpenOptions::new().append(true).open(&*os(path))? }.write_all(bytes)
    }
    fn len(&self, path: &str) -> Option<u64> {
        found(path, |p| std::fs::metadata(p)).map(|m| m.len()).ok()
    }
}

// ---------------------------------------------------------------- the game's files laid over (Task W5)

/// The launcher's files laid over the game's (a zip, extra cars and tracks): reads under the
/// game's folder go through the layers, everything else (the player's folder) is the disk.
pub struct LayeredFiles(pub Arc<Layers>);

impl LayeredFiles {
    fn under(&self, path: &str) -> Option<PathBuf> {
        let p = PathBuf::from(path.replace('\\', "/"));
        p.starts_with(&self.0.root).then_some(p)
    }
}

impl Files for LayeredFiles {
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        use rustyac_content::vfs::Vfs;
        match self.under(path) {
            Some(p) => self.0.read(&p).map(|b| b.to_vec()),
            None => Disk.read(path),
        }
    }
    fn write(&self, path: &str, bytes: &[u8]) -> std::io::Result<()> {
        Disk.write(path, bytes)
    }
    fn is_file(&self, path: &str) -> bool {
        use rustyac_content::vfs::Vfs;
        self.under(path).map_or_else(|| Disk.is_file(path), |p| self.0.is_file(&p))
    }
    fn is_dir(&self, path: &str) -> bool {
        use rustyac_content::vfs::Vfs;
        self.under(path).map_or_else(|| Disk.is_dir(path), |p| self.0.is_dir(&p))
    }
    fn list(&self, folder: &str) -> std::io::Result<Vec<(String, bool)>> {
        use rustyac_content::vfs::Vfs;
        self.under(folder).map_or_else(|| Disk.list(folder), |p| self.0.read_dir(&p))
    }
    fn create_dir_all(&self, path: &str) -> std::io::Result<()> {
        Disk.create_dir_all(path)
    }
    fn rename(&self, from: &str, to: &str) -> std::io::Result<()> {
        Disk.rename(from, to)
    }
    fn remove_file(&self, path: &str) -> std::io::Result<()> {
        Disk.remove_file(path)
    }
    fn remove_dir_all(&self, path: &str) -> std::io::Result<()> {
        Disk.remove_dir_all(path)
    }
    fn modified(&self, path: &str) -> Option<std::time::SystemTime> {
        // shortcut: a file inside a zip has no time here (zip times are not read); read them if the theme ever sorts the game's own files by date
        self.under(path).map_or_else(|| Disk.modified(path), |_| None)
    }
    fn created(&self, path: &str) -> Option<std::time::SystemTime> {
        self.under(path).map_or_else(|| Disk.created(path), |_| None)
    }
}

/// The file that names the extra cars and tracks: `RUSTYAC_GAME_FILES` (a proof's own), else
/// rustyAC's settings file ([`Remembered::file`]; never in a proof run, Task 46c).
pub fn extras_file() -> Option<PathBuf> {
    std::env::var_os("RUSTYAC_GAME_FILES").map(PathBuf::from).or_else(|| std::env::var_os("RUSTYAC_PROOF_RUN").is_none().then(Remembered::file).flatten())
}

/// The remembered file while extras are mounted: the game is given it (`RUSTYAC_GAME_FILES`).
static GAME_FILES: RwLock<Option<PathBuf>> = RwLock::new(None);

pub fn game_files() -> Option<PathBuf> {
    GAME_FILES.read().unwrap().clone()
}

/// Lays the remembered extra cars and tracks over the install at `root` for the launcher's reads
/// (and names them for the game), or takes them away when there are none. Each extra with why it
/// is left out ("": it is in); one that no longer opens (moved, deleted) is left out, not fatal.
pub fn mount_extras(root: &Path) -> Vec<(PathBuf, String)> {
    let file = extras_file();
    let extra = file.as_deref().map(Remembered::load).unwrap_or_default().extra;
    let status: Vec<(PathBuf, String)> = extra.iter().map(|p| (p.clone(), Layers::open(Some(root), std::slice::from_ref(p)).err().unwrap_or_default())).collect();
    let good: Vec<PathBuf> = status.iter().filter(|s| s.1.is_empty()).map(|s| s.0.clone()).collect();
    let layers = (!good.is_empty()).then(|| Layers::open(Some(root), &good).ok()).flatten();
    *GAME_FILES.write().unwrap() = layers.as_ref().and(file);
    match layers {
        Some(layers) => {
            let shared = Arc::new(layers);
            rustyac_content::vfs::mount(Some(shared.clone()));
            mount(Some(Arc::new(LayeredFiles(shared))));
        }
        None => {
            rustyac_content::vfs::mount(None);
            mount(None);
        }
    }
    status
}

/// Adds (`Some`) or removes (`index`) an extra in the remembered file, then mounts again.
pub fn change_extras(root: &Path, add: Option<PathBuf>, remove: Option<usize>) -> std::io::Result<Vec<(PathBuf, String)>> {
    let file = extras_file().ok_or_else(|| Error::other("rustyAC's settings folder is unknown (no APPDATA)"))?;
    let mut r = Remembered::load(&file);
    if let Some(p) = add.filter(|p| !r.extra.contains(p)) {
        r.extra.push(p);
    }
    if let Some(i) = remove.filter(|&i| i < r.extra.len()) {
        r.extra.remove(i);
    }
    r.save(&file)?;
    Ok(mount_extras(root))
}


static MOUNTED: RwLock<Option<Arc<dyn Files>>> = RwLock::new(None);

/// Puts `files` under the launcher's code (`None`: the disk again).
pub fn mount(files: Option<Arc<dyn Files>>) {
    *MOUNTED.write().unwrap() = files;
}

/// Runs `f` on the mounted files, the disk when none are.
pub fn with<R>(f: impl FnOnce(&dyn Files) -> R) -> R {
    match &*MOUNTED.read().unwrap() {
        Some(files) => f(&**files),
        None => f(&Disk),
    }
}

/// A path as a key: `/` as `\`, no `\` doubled or at the end, upper case (as Windows compares).
fn key(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars().map(|c| if c == '/' { '\\' } else { c }) {
        if !(c == '\\' && out.ends_with('\\')) {
            out.extend(c.to_uppercase());
        }
    }
    out.trim_end_matches('\\').to_string()
}

fn parent(key: &str) -> &str {
    key.rfind('\\').map_or("", |i| &key[..i])
}

struct Entry {
    /// The name as it was made.
    name: String,
    /// `None`: a folder.
    bytes: Option<Vec<u8>>,
    created: u64,
    modified: u64,
}

/// Files in memory, the browser's store while the page is open (the page saves and loads it).
/// Folders are made by `create_dir_all`; the root (a key with no `\`) is always there.
#[derive(Default)]
pub struct MemFiles {
    entries: Mutex<(BTreeMap<String, Entry>, u64)>,
}

impl MemFiles {
    pub fn new() -> MemFiles {
        MemFiles::default()
    }

    /// Every file: (path as made, bytes), for the page to keep.
    pub fn files(&self) -> Vec<(String, Vec<u8>)> {
        let entries = self.entries.lock().unwrap();
        let path = |k: &str| -> String {
            // the names as made, folder by folder
            let mut out = Vec::new();
            let mut at = k;
            while let Some(e) = entries.0.get(at) {
                out.push(e.name.clone());
                at = parent(at);
                if at.is_empty() {
                    break;
                }
            }
            if !at.is_empty() && !entries.0.contains_key(at) {
                out.push(at.to_string());
            }
            out.reverse();
            out.join("\\")
        };
        entries.0.iter().filter_map(|(k, e)| e.bytes.as_ref().map(|b| (path(k), b.clone()))).collect()
    }

    /// Every folder made, as made, for the page to keep with [`MemFiles::files`].
    pub fn folders(&self) -> Vec<String> {
        let entries = self.entries.lock().unwrap();
        let names = |k: &str| -> String {
            let mut out = Vec::new();
            let mut at = k;
            while let Some(e) = entries.0.get(at) {
                out.push(e.name.clone());
                at = parent(at);
            }
            if !at.is_empty() {
                out.push(at.to_string());
            }
            out.reverse();
            out.join("\\")
        };
        entries.0.iter().filter(|(_, e)| e.bytes.is_none()).map(|(k, _)| names(k)).collect()
    }

    fn folder_there(map: &BTreeMap<String, Entry>, key: &str) -> bool {
        // a key without `\` is a drive or a root the page names, always there
        !key.contains('\\') || map.get(key).is_some_and(|e| e.bytes.is_none())
    }
}

fn not_found() -> Error {
    Error::from(ErrorKind::NotFound)
}

fn time(t: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(t)
}

impl Files for MemFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>> {
        let entries = self.entries.lock().unwrap();
        entries.0.get(&key(path)).and_then(|e| e.bytes.clone()).ok_or_else(not_found)
    }
    fn write(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let k = key(path);
        let mut entries = self.entries.lock().unwrap();
        if !MemFiles::folder_there(&entries.0, parent(&k)) {
            return Err(not_found());
        }
        if entries.0.get(&k).is_some_and(|e| e.bytes.is_none()) {
            return Err(Error::from(ErrorKind::PermissionDenied));
        }
        // shortcut (browser only, shortcuts.md #27): the clock is a counter (no wall clock in wasm32-unknown-unknown); only the
        // order of the times is right, which is what the launcher sorts by
        entries.1 += 1;
        let now = entries.1;
        let name = path.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next().unwrap_or_default().to_string();
        let created = entries.0.get(&k).map_or(now, |e| e.created);
        entries.0.insert(k, Entry { name, bytes: Some(bytes.to_vec()), created, modified: now });
        Ok(())
    }
    fn is_file(&self, path: &str) -> bool {
        self.entries.lock().unwrap().0.get(&key(path)).is_some_and(|e| e.bytes.is_some())
    }
    fn is_dir(&self, path: &str) -> bool {
        MemFiles::folder_there(&self.entries.lock().unwrap().0, &key(path))
    }
    fn list(&self, folder: &str) -> Result<Vec<(String, bool)>> {
        let k = key(folder);
        let entries = self.entries.lock().unwrap();
        if !MemFiles::folder_there(&entries.0, &k) {
            return Err(not_found());
        }
        let prefix = format!("{k}\\");
        // the keys are upper case, so this is NTFS's order: by the upper-case names
        Ok(entries.0.range(prefix.clone()..).take_while(|(key, _)| key.starts_with(&prefix)).filter(|(key, _)| !key[prefix.len()..].contains('\\')).map(|(_, e)| (e.name.clone(), e.bytes.is_none())).collect())
    }
    fn create_dir_all(&self, path: &str) -> Result<()> {
        let k = key(path);
        let mut entries = self.entries.lock().unwrap();
        let names: Vec<&str> = path.trim_end_matches(['\\', '/']).split(['\\', '/']).filter(|n| !n.is_empty()).collect();
        let mut at = String::new();
        for (i, part) in k.split('\\').enumerate() {
            if i > 0 {
                at.push('\\');
            }
            at.push_str(part);
            if i == 0 {
                continue;
            }
            match entries.0.get(&at) {
                Some(e) if e.bytes.is_some() => return Err(Error::from(ErrorKind::AlreadyExists)),
                Some(_) => {}
                None => {
                    entries.1 += 1;
                    let now = entries.1;
                    let name = names.get(i).copied().unwrap_or(part).to_string();
                    entries.0.insert(at.clone(), Entry { name, bytes: None, created: now, modified: now });
                }
            }
        }
        Ok(())
    }
    fn rename(&self, from: &str, to: &str) -> Result<()> {
        let (f, t) = (key(from), key(to));
        let mut entries = self.entries.lock().unwrap();
        if entries.0.contains_key(&t) {
            return Err(Error::from(ErrorKind::AlreadyExists));
        }
        if !MemFiles::folder_there(&entries.0, parent(&t)) {
            return Err(not_found());
        }
        // shortcut (unreachable, shortcuts.md #28): files only (the launcher renames replays), not folders
        let mut e = entries.0.remove(&f).filter(|e| e.bytes.is_some()).ok_or_else(not_found)?;
        e.name = to.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next().unwrap_or_default().to_string();
        entries.0.insert(t, e);
        Ok(())
    }
    fn remove_file(&self, path: &str) -> Result<()> {
        let k = key(path);
        let mut entries = self.entries.lock().unwrap();
        match entries.0.get(&k) {
            Some(e) if e.bytes.is_some() => {
                entries.0.remove(&k);
                Ok(())
            }
            Some(_) => Err(Error::from(ErrorKind::PermissionDenied)),
            None => Err(not_found()),
        }
    }
    fn remove_dir_all(&self, path: &str) -> Result<()> {
        let k = key(path);
        let mut entries = self.entries.lock().unwrap();
        if !entries.0.get(&k).is_some_and(|e| e.bytes.is_none()) {
            return Err(not_found());
        }
        let prefix = format!("{k}\\");
        entries.0.retain(|key, _| key != &k && !key.starts_with(&prefix));
        Ok(())
    }
    fn modified(&self, path: &str) -> Option<SystemTime> {
        self.entries.lock().unwrap().0.get(&key(path)).map(|e| time(e.modified))
    }
    fn created(&self, path: &str) -> Option<SystemTime> {
        self.entries.lock().unwrap().0.get(&key(path)).map(|e| time(e.created))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mem_files_behave_as_the_disk_does_for_the_launcher() {
        let m = MemFiles::new();
        assert!(m.is_dir("C:"));
        assert_eq!(m.write("C:\\docs\\a.ini", b"x").unwrap_err().kind(), ErrorKind::NotFound);
        m.create_dir_all("C:\\Docs\\cfg\\").unwrap();
        assert!(m.is_dir("c:/docs/CFG"));
        m.write("C:\\docs\\cfg\\b.ini", b"1").unwrap();
        m.write("C:\\docs\\cfg\\A.ini", b"2").unwrap();
        m.create_dir_all("C:\\docs\\cfg\\sub").unwrap();
        assert_eq!(m.list("c:\\DOCS\\cfg").unwrap(), [("A.ini".to_string(), false), ("b.ini".to_string(), false), ("sub".to_string(), true)]);
        assert_eq!(m.read("C:/docs/cfg/a.INI").unwrap(), b"2");
        m.append("C:\\docs\\cfg\\a.ini", b"\n").unwrap();
        assert_eq!(m.read("C:\\docs\\cfg\\A.ini").unwrap(), b"2\n");
        assert!(m.modified("C:\\docs\\cfg\\a.ini") > m.modified("C:\\docs\\cfg\\b.ini"));
        assert!(m.rename("C:\\docs\\cfg\\a.ini", "C:\\docs\\cfg\\b.ini").is_err());
        m.rename("C:\\docs\\cfg\\a.ini", "C:\\docs\\cfg\\c.ini").unwrap();
        assert!(!m.is_file("C:\\docs\\cfg\\a.ini") && m.is_file("C:\\docs\\cfg\\c.ini"));
        m.copy("C:\\docs\\cfg\\c.ini", "C:\\docs\\cfg\\sub\\d.ini").unwrap();
        assert_eq!(m.files().len(), 3);
        assert!(m.files().iter().any(|(p, _)| p == "C:\\Docs\\cfg\\sub\\d.ini"));
        m.remove_file("C:\\docs\\cfg\\b.ini").unwrap();
        assert!(m.remove_file("C:\\docs\\cfg\\b.ini").is_err());
        m.remove_dir_all("C:\\docs\\cfg").unwrap();
        assert!(!m.is_dir("C:\\docs\\cfg") && m.is_dir("C:\\docs") && m.files().is_empty());
    }
}
