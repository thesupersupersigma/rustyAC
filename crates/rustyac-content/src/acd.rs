// SPDX-License-Identifier: MIT OR Apache-2.0
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! A car's packed data, `content/cars/<car>/data.acd`, read the way the game reads it:
//! decrypted in memory, never written anywhere.
//!
//! * [`key_from_string`]: `ksSecurity::keyFromString` @ 0x1402cfe00, the key made from the
//!   car folder's name.
//! * [`Acd`]: `FolderEncrypter::decryptFile` @ 0x14023bdd0, the walk through the container
//!   and the per-byte decryption.
//! * [`read`] / [`exists`]: a file of a car's `data` folder. As in the game, the archive next
//!   to the folder (`<folder>.acd`) is what is read when it is there; the plain folder is
//!   read otherwise (the SDK's cars, unpacked mods, the test cars).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// The optional header of a container: this number, then one more `i32`.
const ACD_MAGIC: i32 = -1111;

/// `ksSecurity::keyFromString`: eight numbers 0..255 from the folder name, printed as
/// `"%d-%d-%d-%d-%d-%d-%d-%d"`. All sums and products are 32-bit and wrap; the divisions are
/// the processor's signed `idiv` (toward zero), which faults on a zero divisor: a name that
/// would make the game crash there is the `Err` here.
pub fn key_from_string(name: &str) -> Result<String, String> {
    // the game reads the low byte of each wchar_t, sign-extended
    let c: Vec<i32> = name.chars().map(|ch| (ch as u32 as u8) as i8 as i32).collect();
    let n = c.len() as i64;
    let at = |i: i64| c[i as usize];
    let div = |a: i32, b: i32| a.checked_div(b).ok_or_else(|| format!("no key can be made from the folder name {name:?} (the game divides by zero)"));
    let rem = |a: i32, b: i32| a.checked_rem(b).ok_or_else(|| format!("no key can be made from the folder name {name:?} (the game divides by zero)"));

    let mut k1 = 0i32;
    for &ch in &c {
        k1 = k1.wrapping_add(ch);
    }
    let mut k2 = 0i32;
    let mut i = 0;
    while i < n - 1 {
        k2 = k2.wrapping_mul(at(i)).wrapping_sub(at(i + 1));
        i += 2;
    }
    let mut k3 = 0i32;
    let mut i = 1;
    while i < n - 3 {
        k3 = k3.wrapping_mul(at(i));
        k3 = div(k3, at(i + 1).wrapping_add(0x1b))?;
        k3 = k3.wrapping_add((-0x1b_i32).wrapping_sub(at(i - 1)));
        i += 3;
    }
    let mut k4 = 0x1683i32;
    for i in 1..n {
        k4 = k4.wrapping_sub(at(i));
    }
    let mut k5 = 0x42i32;
    let mut i = 1;
    while i < n - 4 {
        k5 = at(i).wrapping_add(0xf).wrapping_mul(k5);
        k5 = at(i - 1).wrapping_add(0xf).wrapping_mul(k5).wrapping_add(0x16);
        i += 4;
    }
    let mut k6 = 0x65i32;
    let mut i = 0;
    while i < n - 2 {
        k6 = k6.wrapping_sub(at(i));
        i += 2;
    }
    let mut k7 = 0xabi32;
    let mut i = 0;
    while i < n - 2 {
        k7 = rem(k7, at(i))?;
        i += 2;
    }
    let mut k8 = 0xabi32;
    for i in 0..n - 1 {
        k8 = div(k8, at(i))?.wrapping_add(at(i + 1));
    }
    Ok([k1, k2, k3, k4, k5, k6, k7, k8].map(|k| (k & 0xff).to_string()).join("-"))
}

/// A decrypted container: its files in the container's order.
#[derive(Debug, Default)]
pub struct Acd {
    files: Vec<(String, Vec<u8>)>,
}

impl Acd {
    /// `FolderEncrypter::decryptFile`: every entry is a name (`i32` length, bytes), an `i32`
    /// size and `size` 32-bit numbers, of which only the low byte counts; a plain byte is
    /// that byte minus the key's character at the same position (the key repeating).
    pub fn decrypt(container: &[u8], key: &str) -> Result<Acd, String> {
        let key = key.as_bytes();
        if key.is_empty() {
            return Err("empty key".to_string());
        }
        let int = |pos: usize| -> Option<i32> { Some(i32::from_le_bytes(container.get(pos..pos + 4)?.try_into().ok()?)) };
        let mut pos = 0;
        if int(0) == Some(ACD_MAGIC) {
            pos = 8;
        }
        let mut files = Vec::new();
        while pos + 4 <= container.len() {
            let start = pos;
            let broken = || format!("the container is cut short or is not a data.acd (entry at byte {start})");
            let name_len = usize::try_from(int(pos).ok_or_else(broken)?).map_err(|_| broken())?;
            pos += 4;
            let name = container.get(pos..pos.checked_add(name_len).ok_or_else(broken)?).ok_or_else(broken)?;
            pos += name_len;
            let size = usize::try_from(int(pos).ok_or_else(broken)?).map_err(|_| broken())?;
            pos += 4;
            let stored = container.get(pos..size.checked_mul(4).and_then(|s| pos.checked_add(s)).ok_or_else(broken)?).ok_or_else(broken)?;
            pos += size * 4;
            let plain = stored.chunks_exact(4).enumerate().map(|(i, word)| word[0].wrapping_sub(key[i % key.len()])).collect();
            files.push((String::from_utf8_lossy(name).into_owned(), plain));
        }
        Ok(Acd { files })
    }

    /// Reads and decrypts `<car folder>/data.acd`; the key comes from the car folder's name.
    pub fn open(acd: &Path) -> Result<Acd, String> {
        let name_of = |path: &Path| Some(path.parent()?.file_name()?.to_string_lossy().into_owned());
        let name = name_of(acd)
            .or_else(|| name_of(&acd.canonicalize().ok()?))
            .ok_or_else(|| format!("{}: the car folder's name (the key) is not known", acd.display()))?;
        Acd::open_as(acd, &name)
    }

    /// As [`Acd::open`] for a car folder that was renamed after packing: `name` is the name
    /// the data was packed under.
    pub fn open_as(acd: &Path, name: &str) -> Result<Acd, String> {
        let container = std::fs::read(acd).map_err(|e| format!("{}: {e}", acd.display()))?;
        let key = key_from_string(name)?;
        Acd::decrypt(&container, &key).map_err(|e| format!("{}: {e}", acd.display()))
    }

    /// A file's bytes. Names are matched exactly first, then without regard to ASCII case
    /// (as a folder on Windows would).
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.files
            .iter()
            .find(|(n, _)| n == name)
            .or_else(|| self.files.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)))
            .map(|(_, bytes)| bytes.as_slice())
    }

    /// The names, in the container's order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(|(name, _)| name.as_str())
    }

    /// Every entry with its bytes, in the container's order (a name can come twice).
    pub fn entries(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.files.iter().map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// `Path::getPath(file) + L".acd"`: the archive the game reads instead of a file of a folder
/// (`.../cars/<car>/data/car.ini` -> `.../cars/<car>/data.acd`).
pub fn sibling_acd(file: &Path) -> Option<PathBuf> {
    let dir = file.parent()?;
    let name = dir.file_name()?;
    let mut acd = name.to_os_string();
    acd.push(".acd");
    Some(dir.with_file_name(acd))
}

type Cache = Mutex<HashMap<PathBuf, Result<Arc<Acd>, String>>>;

/// The decrypted archives of this process, by path: a car's files are asked for dozens of
/// times while it is built. Memory only.
fn cached(acd: &Path) -> Result<Arc<Acd>, String> {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Cache::default).lock().unwrap_or_else(|e| e.into_inner());
    cache.entry(acd.to_path_buf()).or_insert_with(|| Acd::open(acd).map(Arc::new)).clone()
}

/// The archive that holds `file`, if its folder has one.
fn archive_of(file: &Path) -> Option<Result<Arc<Acd>, String>> {
    let acd = sibling_acd(file).filter(|acd| acd.is_file())?;
    Some(cached(&acd))
}

/// The bytes of a data file: from the archive next to its folder when there is one, else
/// the plain file. `Ok(None)`: there is no such file. `Err`: the archive cannot be read.
pub fn read(file: &Path) -> Result<Option<Vec<u8>>, String> {
    match archive_of(file) {
        Some(archive) => {
            let archive = archive?;
            let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            Ok(archive.get(&name).map(<[u8]>::to_vec))
        }
        None => Ok(std::fs::read(file).ok()),
    }
}

/// Is there such a data file (in the archive when the folder has one, else on disk)?
pub fn exists(file: &Path) -> bool {
    match archive_of(file) {
        Some(Ok(archive)) => file.file_name().is_some_and(|n| archive.get(&n.to_string_lossy()).is_some()),
        Some(Err(_)) => false,
        None => file.is_file(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_of_a_folder_name() {
        // what tools/acd_extract.py (checked against the game's files) prints
        assert_eq!(key_from_string("abc").unwrap(), "38-158-0-190-66-4-74-100");
        assert_eq!(key_from_string("ks_ferrari_f2004").unwrap(), "179-44-163-59-166-193-14-53");
        assert_eq!(key_from_string("").unwrap(), "0-0-0-131-66-101-171-171");
        assert!(key_from_string("a").is_ok());
    }

    #[test]
    fn a_container_decrypts_to_its_files() {
        let key = "1-2-3";
        let mut container = Vec::new();
        let mut put = |name: &str, plain: &[u8]| {
            container.extend((name.len() as i32).to_le_bytes());
            container.extend(name.as_bytes());
            container.extend((plain.len() as i32).to_le_bytes());
            for (i, b) in plain.iter().enumerate() {
                let stored = b.wrapping_add(key.as_bytes()[i % key.len()]) as u32;
                container.extend(stored.to_le_bytes());
            }
        };
        put("car.ini", b"[HEADER]\nVERSION=2\n");
        put("Power.lut", b"0|10\n");
        let acd = Acd::decrypt(&container, key).unwrap();
        assert_eq!(acd.names().collect::<Vec<_>>(), ["car.ini", "Power.lut"]);
        assert_eq!(acd.get("car.ini"), Some(&b"[HEADER]\nVERSION=2\n"[..]));
        assert_eq!(acd.get("power.lut"), Some(&b"0|10\n"[..]), "any case");
        assert_eq!(acd.get("tyres.ini"), None);
        // with the optional header in front
        let mut with_header = (-1111i32).to_le_bytes().to_vec();
        with_header.extend(7i32.to_le_bytes());
        with_header.extend(&container);
        assert_eq!(Acd::decrypt(&with_header, key).unwrap().len(), 2);
        // cut short
        assert!(Acd::decrypt(&container[..container.len() - 3], key).is_err());
    }

    #[test]
    fn the_archive_is_the_folder_s_sibling() {
        assert_eq!(sibling_acd(Path::new("cars/x/data/car.ini")), Some(PathBuf::from("cars/x/data.acd")));
        assert_eq!(sibling_acd(Path::new("car.ini")), None);
    }
}
