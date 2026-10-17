// SPDX-License-Identifier: GPL-3.0-or-later

//! The files the launcher's code reads and writes: the disk, or a store of the page's own in
//! the browser (rustyAC's, not the launcher's: .NET's `File` and `Directory` always mean the
//! disk). Every file call of the bridge goes through [`net`](crate::net) to here, so the
//! browser mounts its store with [`mount`] and the same code runs on it. Paths are the
//! launcher's own (`\` or `/`, any case, as Windows takes them).

use std::collections::BTreeMap;
use std::io::{Error, ErrorKind, Result};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

impl Files for Disk {
    fn read(&self, path: &str) -> Result<Vec<u8>> {
        std::fs::read(&*os(path))
    }
    fn write(&self, path: &str, bytes: &[u8]) -> Result<()> {
        std::fs::write(&*os(path), bytes)
    }
    fn is_file(&self, path: &str) -> bool {
        std::path::Path::new(&*os(path)).is_file()
    }
    fn is_dir(&self, path: &str) -> bool {
        std::path::Path::new(&*os(path)).is_dir()
    }
    fn list(&self, folder: &str) -> Result<Vec<(String, bool)>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&*os(folder))? {
            let entry = entry?;
            out.push((entry.file_name().to_string_lossy().into_owned(), entry.file_type()?.is_dir()));
        }
        Ok(out)
    }
    fn create_dir_all(&self, path: &str) -> Result<()> {
        std::fs::create_dir_all(&*os(path))
    }
    fn rename(&self, from: &str, to: &str) -> Result<()> {
        if std::path::Path::new(&*os(to)).exists() {
            return Err(Error::from(ErrorKind::AlreadyExists));
        }
        std::fs::rename(&*os(from), &*os(to))
    }
    fn remove_file(&self, path: &str) -> Result<()> {
        std::fs::remove_file(&*os(path))
    }
    fn remove_dir_all(&self, path: &str) -> Result<()> {
        std::fs::remove_dir_all(&*os(path))
    }
    fn modified(&self, path: &str) -> Option<SystemTime> {
        std::fs::metadata(&*os(path)).and_then(|m| m.modified()).ok()
    }
    fn created(&self, path: &str) -> Option<SystemTime> {
        std::fs::metadata(&*os(path)).and_then(|m| m.created()).ok()
    }
    fn copy(&self, from: &str, to: &str) -> Result<()> {
        std::fs::copy(&*os(from), &*os(to)).map(|_| ())
    }
    fn append(&self, path: &str, bytes: &[u8]) -> Result<()> {
        use std::io::Write as _;
        std::fs::OpenOptions::new().append(true).open(&*os(path))?.write_all(bytes)
    }
    fn len(&self, path: &str) -> Option<u64> {
        std::fs::metadata(&*os(path)).map(|m| m.len()).ok()
    }
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
        // shortcut: the clock is a counter (no wall clock in wasm32-unknown-unknown); only the
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
        // shortcut: files only (the launcher renames replays), not folders
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
