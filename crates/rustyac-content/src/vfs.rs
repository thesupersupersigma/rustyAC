// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where the game's files come from: the disk, or a set of files held in memory.
//!
//! Every reader of this crate and of `rustyac-physics` asks here instead of `std::fs`. With
//! nothing mounted (the desktop, always) each call *is* the `std::fs` call it replaced. A
//! browser has no disk: `rustyac-web` mounts a [`MemFs`] filled with the files of the chosen
//! car and track (fetched from a preview pack, or read out of the folder the player picked)
//! and the same loaders run unchanged.
//!
//! [`set_log`] records every path asked for, found or not. `tools/web_pack` loads a car on a
//! track once with it and so learns exactly which files a pack needs.

use std::collections::BTreeMap;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

/// A source of files that is not the disk.
pub trait Vfs: Send + Sync {
    fn read(&self, path: &Path) -> io::Result<Arc<[u8]>>;
    fn is_file(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    /// The entries of a folder: name, "is a folder".
    fn read_dir(&self, path: &Path) -> io::Result<Vec<(String, bool)>>;
}

static MOUNT: RwLock<Option<Arc<dyn Vfs>>> = RwLock::new(None);
static LOG: Mutex<Option<Vec<(PathBuf, bool)>>> = Mutex::new(None);

/// Replaces the disk by `fs` for every later call (`None`: the disk again).
pub fn mount(fs: Option<Arc<dyn Vfs>>) {
    *MOUNT.write().unwrap() = fs;
}

fn mounted() -> Option<Arc<dyn Vfs>> {
    MOUNT.read().unwrap().clone()
}

/// Starts (`true`) or stops recording the paths asked for.
pub fn set_log(on: bool) {
    *LOG.lock().unwrap() = on.then(Vec::new);
}

/// The paths asked for since [`set_log`], each with "was there"; the record starts again.
pub fn take_log() -> Vec<(PathBuf, bool)> {
    LOG.lock().unwrap().as_mut().map(std::mem::take).unwrap_or_default()
}

fn note(path: &Path, found: bool) {
    if let Ok(mut log) = LOG.try_lock() {
        if let Some(log) = log.as_mut() {
            log.push((path.to_path_buf(), found));
        }
    }
}

/// `std::fs::read`.
pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    let result = match mounted() {
        Some(fs) => fs.read(path).map(|bytes| bytes.to_vec()),
        None => std::fs::read(path),
    };
    note(path, result.is_ok());
    result
}

/// `std::fs::read_to_string`.
pub fn read_to_string(path: &Path) -> io::Result<String> {
    match mounted() {
        Some(_) => String::from_utf8(read(path)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
        None => {
            let result = std::fs::read_to_string(path);
            note(path, result.is_ok());
            result
        }
    }
}

/// `Path::is_file`.
pub fn is_file(path: &Path) -> bool {
    let found = match mounted() {
        Some(fs) => fs.is_file(path),
        None => path.is_file(),
    };
    note(path, found);
    found
}

/// `Path::is_dir`.
pub fn is_dir(path: &Path) -> bool {
    match mounted() {
        Some(fs) => fs.is_dir(path),
        None => path.is_dir(),
    }
}

/// `std::fs::read_dir`, as (name, "is a folder") in the order the source gives.
pub fn read_dir(path: &Path) -> io::Result<Vec<(String, bool)>> {
    match mounted() {
        Some(fs) => fs.read_dir(path),
        None => Ok(std::fs::read_dir(path)?
            .flatten()
            .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path().is_dir()))
            .collect()),
    }
}

/// `path.vfs_is_file()` / `path.vfs_is_dir()`: [`is_file`] / [`is_dir`] spelled like the
/// `std` methods they stand in for.
pub trait PathExt {
    fn vfs_is_file(&self) -> bool;
    fn vfs_is_dir(&self) -> bool;
}

impl PathExt for Path {
    fn vfs_is_file(&self) -> bool {
        is_file(self)
    }

    fn vfs_is_dir(&self) -> bool {
        is_dir(self)
    }
}

/// An open file: the disk's, or bytes in memory.
pub enum File {
    Disk(std::fs::File),
    Memory(io::Cursor<Arc<[u8]>>),
}

impl File {
    /// `std::fs::File::open`.
    pub fn open(path: &Path) -> io::Result<File> {
        let result = match mounted() {
            Some(fs) => fs.read(path).map(|bytes| File::Memory(io::Cursor::new(bytes))),
            None => std::fs::File::open(path).map(File::Disk),
        };
        note(path, result.is_ok());
        result
    }

    /// The file's length in bytes.
    pub fn len(&self) -> io::Result<u64> {
        match self {
            File::Disk(file) => Ok(file.metadata()?.len()),
            File::Memory(cursor) => Ok(cursor.get_ref().len() as u64),
        }
    }

    pub fn is_empty(&self) -> io::Result<bool> {
        Ok(self.len()? == 0)
    }
}

impl Read for File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            File::Disk(file) => file.read(buf),
            File::Memory(cursor) => cursor.read(buf),
        }
    }
}

impl Seek for File {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match self {
            File::Disk(file) => file.seek(pos),
            File::Memory(cursor) => cursor.seek(pos),
        }
    }
}

/// How a [`MemFs`] spells a path: forward slashes, lower case (Windows, where the game's
/// files were made, does not tell `Spa.kn5` from `spa.kn5`), no `.` parts, `..` resolved.
pub fn key_of(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    let mut parts: Vec<&str> = Vec::new();
    for part in text.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}

/// One file of a [`MemFs`].
#[derive(Clone)]
struct Entry {
    /// The name as the source spelled it.
    name: String,
    /// `None`: known to exist (its folder was listed) but not fetched yet.
    bytes: Option<Arc<[u8]>>,
}

/// Files in memory, by [`key_of`] path. A file can be *listed* without its bytes: then
/// [`Vfs::is_file`] says yes, a read fails and the path joins [`MemFs::take_wanted`], so the
/// caller can fetch it and run the load again.
#[derive(Default)]
pub struct MemFs {
    files: RwLock<BTreeMap<String, Entry>>,
    wanted: Mutex<Vec<String>>,
}

impl MemFs {
    pub fn new() -> MemFs {
        MemFs::default()
    }

    /// Adds or replaces a file.
    pub fn insert(&self, path: &str, bytes: Arc<[u8]>) {
        let name = path.replace('\\', "/");
        self.files.write().unwrap().insert(key_of(Path::new(path)), Entry { name, bytes: Some(bytes) });
    }

    /// Says a file exists without giving its bytes yet.
    pub fn list(&self, path: &str) {
        let name = path.replace('\\', "/");
        self.files.write().unwrap().entry(key_of(Path::new(path))).or_insert(Entry { name, bytes: None });
    }

    /// Drops a file's bytes (it stays listed), to give the memory back after a load.
    pub fn forget(&self, path: &str) {
        if let Some(entry) = self.files.write().unwrap().get_mut(&key_of(Path::new(path))) {
            entry.bytes = None;
        }
    }

    /// Is the file known at all (with or without its bytes)?
    pub fn is_listed(&self, path: &str) -> bool {
        self.files.read().unwrap().contains_key(&key_of(Path::new(path)))
    }

    pub fn has_bytes(&self, path: &str) -> bool {
        self.files.read().unwrap().get(&key_of(Path::new(path))).is_some_and(|e| e.bytes.is_some())
    }

    /// The listed files a read asked for and did not get, each once; the list starts again.
    pub fn take_wanted(&self) -> Vec<String> {
        let mut wanted = std::mem::take(&mut *self.wanted.lock().unwrap());
        wanted.sort();
        wanted.dedup();
        wanted
    }

    /// Every path held or listed, as its source spelled it.
    pub fn paths(&self) -> Vec<String> {
        self.files.read().unwrap().values().map(|e| e.name.clone()).collect()
    }

    /// The bytes held, in total.
    pub fn bytes_held(&self) -> u64 {
        self.files.read().unwrap().values().filter_map(|e| e.bytes.as_ref()).map(|b| b.len() as u64).sum()
    }
}

impl Vfs for MemFs {
    fn read(&self, path: &Path) -> io::Result<Arc<[u8]>> {
        let key = key_of(path);
        match self.files.read().unwrap().get(&key) {
            Some(Entry { bytes: Some(bytes), .. }) => Ok(bytes.clone()),
            Some(entry) => {
                self.wanted.lock().unwrap().push(entry.name.clone());
                Err(io::Error::new(io::ErrorKind::WouldBlock, format!("{}: not fetched yet", entry.name)))
            }
            None => Err(io::Error::new(io::ErrorKind::NotFound, format!("{key}: no such file"))),
        }
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.read().unwrap().contains_key(&key_of(path))
    }

    fn is_dir(&self, path: &Path) -> bool {
        let prefix = key_of(path) + "/";
        prefix == "/" || self.files.read().unwrap().range(prefix.clone()..).next().is_some_and(|(k, _)| k.starts_with(&prefix))
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<(String, bool)>> {
        let mut prefix = key_of(path);
        if !prefix.is_empty() {
            prefix.push('/');
        }
        let files = self.files.read().unwrap();
        let mut out: Vec<(String, bool)> = Vec::new();
        for (key, entry) in files.range(prefix.clone()..).take_while(|(k, _)| k.starts_with(&prefix)) {
            let rest = &key[prefix.len()..];
            let (first, is_dir) = match rest.split_once('/') {
                Some((first, _)) => (first, true),
                None => (rest, false),
            };
            // the name as the source spelled it: the same part of the entry's own name
            let depth = prefix.matches('/').count();
            let spelled = entry.name.trim_start_matches("./").split('/').filter(|p| !p.is_empty() && *p != ".").nth(depth).unwrap_or(first);
            if out.last().is_none_or(|(last, _)| !last.eq_ignore_ascii_case(spelled)) {
                out.push((spelled.to_string(), is_dir));
            }
        }
        // (nothing below the name: there is no such folder)
        if out.is_empty() && !prefix.is_empty() {
            return Err(io::Error::new(io::ErrorKind::NotFound, format!("{prefix}: no such folder")));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_memory_file_system_answers_like_a_disk() {
        let fs = MemFs::new();
        fs.insert("content/tracks/Spa/models.ini", Arc::from(&b"[MODEL_0]"[..]));
        fs.insert("content\\tracks\\spa\\ai\\fast_lane.ai", Arc::from(&b"x"[..]));
        fs.list("content/tracks/spa/spa.kn5");
        fs.insert("content/cars/abarth500/data.acd", Arc::from(&b"y"[..]));
        let p = Path::new;
        assert!(fs.is_file(p("content/tracks/spa/MODELS.INI")));
        assert!(fs.is_file(p("./content/tracks/spa/data/../spa.kn5")));
        assert!(!fs.is_file(p("content/tracks/spa")));
        assert!(fs.is_dir(p("content/tracks/spa")) && fs.is_dir(p("content")) && !fs.is_dir(p("content/tracks/sp")));
        assert_eq!(&*fs.read(p("content/tracks/spa/models.ini")).unwrap(), b"[MODEL_0]");
        assert_eq!(fs.read(p("content/tracks/spa/spa.kn5")).unwrap_err().kind(), io::ErrorKind::WouldBlock);
        assert_eq!(fs.read(p("content/tracks/spa/nothing")).unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(fs.take_wanted(), vec!["content/tracks/spa/spa.kn5".to_string()]);
        assert!(fs.take_wanted().is_empty());
        let mut names = fs.read_dir(p("content/tracks/spa")).unwrap();
        names.sort();
        assert_eq!(names, vec![("ai".to_string(), true), ("models.ini".to_string(), false), ("spa.kn5".to_string(), false)]);
        assert_eq!(fs.read_dir(p("content")).unwrap(), vec![("cars".to_string(), true), ("tracks".to_string(), true)]);
        // (one of the spellings the files came with)
        let tracks = fs.read_dir(p("content/tracks")).unwrap();
        assert!(tracks.len() == 1 && tracks[0].0.eq_ignore_ascii_case("spa") && tracks[0].1, "{tracks:?}");
        assert!(fs.read_dir(p("nowhere")).is_err());
        assert_eq!(fs.bytes_held(), 11);
    }
}
