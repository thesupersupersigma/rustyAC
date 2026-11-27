// SPDX-License-Identifier: MIT OR Apache-2.0

//! A zip of the game's files, read in place (`docs/platforms.md`, section 5): only the central
//! directory is read when it is opened, and each file is read out of the archive when it is
//! asked for. Zip64 (an archive over 4 GB, as the whole game is) is read; files are stored or
//! deflated, anything else gets "unzip it and choose the folder".

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

fn bad(why: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.into())
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

#[derive(Clone, Debug)]
struct Entry {
    /// The name as the archive spells it, `/` between parts.
    name: String,
    method: u16,
    crc: u32,
    packed: u64,
    size: u64,
    /// Where its local header starts.
    offset: u64,
}

/// Where a zip's bytes come from: a file on the disk, or (the browser) the parts of a picked
/// file the page has handed over so far.
pub trait Bytes: Send + Sync {
    fn size(&self) -> io::Result<u64>;
    fn read_at(&self, at: u64, buf: &mut [u8]) -> io::Result<()>;
}

struct Disk(PathBuf);

impl Bytes for Disk {
    fn size(&self) -> io::Result<u64> {
        Ok(std::fs::metadata(&self.0)?.len())
    }
    fn read_at(&self, at: u64, buf: &mut [u8]) -> io::Result<()> {
        let mut file = std::fs::File::open(&self.0)?;
        file.seek(SeekFrom::Start(at))?;
        file.read_exact(buf)
    }
}

/// The parts of a file handed over so far: the browser reads a picked file asynchronously, so a
/// read of a part not here yet fails with [`Need`], the page hands that part over
/// ([`Parts::give`]) and asks again.
pub struct Parts {
    len: u64,
    parts: Mutex<Vec<(u64, Vec<u8>)>>,
}

/// The bytes a read of [`Parts`] lacked: `len` of them from `at`.
#[derive(Debug)]
pub struct Need {
    pub at: u64,
    pub len: u64,
}

impl std::fmt::Display for Need {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "needs {} bytes from {} of the file", self.len, self.at)
    }
}

impl std::error::Error for Need {}

impl Need {
    /// The [`Need`] behind a failed read, if that is why it failed.
    pub fn of(e: &io::Error) -> Option<&Need> {
        e.get_ref()?.downcast_ref::<Need>()
    }
}

impl Parts {
    pub fn new(len: u64) -> Parts {
        Parts { len, parts: Mutex::new(Vec::new()) }
    }

    pub fn give(&self, at: u64, bytes: Vec<u8>) {
        self.parts.lock().unwrap().push((at, bytes));
    }

    /// Lets go of every part (after a read: a big file's bytes are not kept twice).
    pub fn forget(&self) {
        self.parts.lock().unwrap().clear();
    }
}

impl Bytes for Parts {
    fn size(&self) -> io::Result<u64> {
        Ok(self.len)
    }
    fn read_at(&self, at: u64, buf: &mut [u8]) -> io::Result<()> {
        let end = at + buf.len() as u64;
        if end > self.len {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        let parts = self.parts.lock().unwrap();
        match parts.iter().find(|(from, b)| *from <= at && end <= from + b.len() as u64) {
            Some((from, b)) => {
                buf.copy_from_slice(&b[(at - from) as usize..(end - from) as usize]);
                Ok(())
            }
            None => Err(io::Error::other(Need { at, len: buf.len() as u64 })),
        }
    }
}

/// An open zip: its directory, by lower-case path (Windows, where the game's files were made,
/// does not tell `Spa.kn5` from `spa.kn5`).
pub struct Zip {
    path: PathBuf,
    bytes: Arc<dyn Bytes>,
    entries: BTreeMap<String, Entry>,
    dirs: BTreeSet<String>,
}

/// The archive's own spelling of a path: `/` between parts, no leading or trailing `/`, lower case.
fn key(path: &str) -> String {
    path.replace('\\', "/").trim_matches('/').to_ascii_lowercase()
}

impl Zip {
    /// Reads the archive's directory (nothing else).
    pub fn open(path: &Path) -> io::Result<Zip> {
        Zip::from_bytes(path, Arc::new(Disk(path.to_path_buf())))
    }

    /// [`Zip::open`] over other bytes; `path` is the name the zip goes by.
    pub fn from_bytes(path: &Path, file: Arc<dyn Bytes>) -> io::Result<Zip> {
        let len = file.size()?;
        // the end record is the last 22 bytes, unless a comment (at most 64 KB) follows it
        let tail_len = len.min(22 + 65535 + 20);
        let mut tail = vec![0; tail_len as usize];
        file.read_at(len - tail_len, &mut tail)?;
        let end = (0..tail.len().saturating_sub(21)).rev().find(|&i| u32_at(&tail, i) == 0x0605_4b50).ok_or_else(|| bad("not a zip (no end of central directory)"))?;
        let (mut count, mut dir_len, mut dir_at) = (u16_at(&tail, end + 10) as u64, u32_at(&tail, end + 12) as u64, u32_at(&tail, end + 16) as u64);
        // zip64: a locator just before the end record points at the zip64 end record
        if end >= 20 && u32_at(&tail, end - 20) == 0x0706_4b50 {
            let mut rec = [0u8; 56];
            file.read_at(u64_at(&tail, end - 12), &mut rec)?;
            if u32_at(&rec, 0) != 0x0606_4b50 {
                return Err(bad("broken zip64 end record"));
            }
            (count, dir_len, dir_at) = (u64_at(&rec, 32), u64_at(&rec, 40), u64_at(&rec, 48));
        }
        let mut dir = vec![0; usize::try_from(dir_len).map_err(|_| bad("central directory too big"))?];
        file.read_at(dir_at, &mut dir)?;
        let mut zip = Zip { path: path.to_path_buf(), bytes: file, entries: BTreeMap::new(), dirs: BTreeSet::new() };
        let mut at = 0;
        for _ in 0..count {
            if at + 46 > dir.len() || u32_at(&dir, at) != 0x0201_4b50 {
                return Err(bad("broken central directory"));
            }
            let (flags, method, crc) = (u16_at(&dir, at + 8), u16_at(&dir, at + 10), u32_at(&dir, at + 16));
            let (mut packed, mut size, mut offset) = (u32_at(&dir, at + 20) as u64, u32_at(&dir, at + 24) as u64, u32_at(&dir, at + 42) as u64);
            let (name_len, extra_len, comment_len) = (u16_at(&dir, at + 28) as usize, u16_at(&dir, at + 30) as usize, u16_at(&dir, at + 32) as usize);
            let next = at + 46 + name_len + extra_len + comment_len;
            if next > dir.len() {
                return Err(bad("broken central directory"));
            }
            let name = String::from_utf8_lossy(&dir[at + 46..at + 46 + name_len]).replace('\\', "/");
            // the zip64 extra field holds, in this order, the values whose 32-bit field is all ones
            let mut extra = &dir[at + 46 + name_len..at + 46 + name_len + extra_len];
            while extra.len() >= 4 {
                let (id, n) = (u16_at(extra, 0), u16_at(extra, 2) as usize);
                let body = &extra[4..(4 + n).min(extra.len())];
                if id == 1 {
                    let mut values = body.chunks_exact(8).map(|c| u64_at(c, 0));
                    for field in [&mut size, &mut packed, &mut offset] {
                        if *field == 0xFFFF_FFFF {
                            *field = values.next().ok_or_else(|| bad("broken zip64 field"))?;
                        }
                    }
                }
                extra = &extra[(4 + n).min(extra.len())..];
            }
            at = next;
            if flags & 1 != 0 && !name.ends_with('/') {
                return Err(bad(format!("{name} is encrypted: unzip it and choose the folder")));
            }
            zip.insert(Entry { name, method, crc, packed, size, offset });
        }
        Ok(zip)
    }

    /// A folder's listing as a zip with no bytes (the browser's folders, for [`crate::imports::detect`]):
    /// paths relative to the folder, a folder's own ending in `/`. Reading a file fails.
    pub fn listing<'a>(path: &Path, names: impl IntoIterator<Item = &'a str>) -> Zip {
        struct Nothing;
        impl Bytes for Nothing {
            fn size(&self) -> io::Result<u64> {
                Ok(0)
            }
            fn read_at(&self, _: u64, _: &mut [u8]) -> io::Result<()> {
                Err(io::Error::from(io::ErrorKind::NotFound))
            }
        }
        let mut zip = Zip { path: path.to_path_buf(), bytes: Arc::new(Nothing), entries: BTreeMap::new(), dirs: BTreeSet::new() };
        for name in names {
            zip.insert(Entry { name: name.replace('\\', "/"), method: 0, crc: 0, packed: 0, size: 0, offset: 0 });
        }
        zip
    }

    fn insert(&mut self, e: Entry) {
        let k = key(&e.name);
        // every folder above a file is a folder, listed or not
        let mut parent = k.as_str();
        while let Some(i) = parent.rfind('/') {
            parent = &parent[..i];
            self.dirs.insert(parent.to_string());
        }
        if e.name.ends_with('/') {
            self.dirs.insert(k);
        } else {
            self.entries.insert(k, e);
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Is there a file at this path inside the archive (any case)?
    pub fn is_file(&self, path: &str) -> bool {
        self.entries.contains_key(&key(path))
    }

    /// Is there a folder at this path (`""`: the archive's top)?
    pub fn is_dir(&self, path: &str) -> bool {
        let k = key(path);
        k.is_empty() || self.dirs.contains(&k)
    }

    /// The entries of a folder (`""`: the top): name as the archive spells it, "is a folder".
    pub fn read_dir(&self, path: &str) -> io::Result<Vec<(String, bool)>> {
        let k = key(path);
        if !self.is_dir(&k) {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        let prefix = if k.is_empty() { k } else { format!("{k}/") };
        let child = |p: &String| p.strip_prefix(&prefix).filter(|rest| !rest.is_empty() && !rest.contains('/')).map(|_| p.len() - prefix.len());
        let mut out: Vec<(String, bool)> = self.dirs.iter().filter_map(|d| child(d).map(|n| (d[d.len() - n..].to_string(), true))).collect();
        // files keep the archive's spelling of their name
        out.extend(self.entries.iter().filter_map(|(p, e)| child(p).map(|n| (e.name[e.name.len() - n..].to_string(), false))));
        Ok(out)
    }

    /// The size of a file, unpacked.
    pub fn len(&self, path: &str) -> Option<u64> {
        self.entries.get(&key(path)).map(|e| e.size)
    }

    /// Every file's path as the archive spells it.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.entries.values().map(|e| e.name.as_str())
    }

    /// One file, read out of the archive and checked against its CRC.
    pub fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        let e = self.entries.get(&key(path)).ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        let mut head = [0u8; 30];
        self.bytes.read_at(e.offset, &mut head)?;
        if u32_at(&head, 0) != 0x0403_4b50 {
            return Err(bad(format!("{}: broken local header", e.name)));
        }
        let mut packed = vec![0; usize::try_from(e.packed).map_err(|_| bad("file too big"))?];
        self.bytes.read_at(e.offset + 30 + u16_at(&head, 26) as u64 + u16_at(&head, 28) as u64, &mut packed)?;
        let bytes = match e.method {
            0 => packed,
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(&packed, e.size as usize).map_err(|err| bad(format!("{}: {err:?}", e.name)))?,
            m => return Err(bad(format!("{} is packed with method {m}, not stored or deflate: unzip it and choose the folder", e.name))),
        };
        if bytes.len() as u64 != e.size || crc32fast::hash(&bytes) != e.crc {
            return Err(bad(format!("{}: damaged (size or CRC)", e.name)));
        }
        Ok(bytes)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A zip of these files, deflated when `deflate`, with zip64 records when `zip64` (as an
    /// archive over 4 GB has them).
    pub(crate) fn make_zip(files: &[(&str, &[u8])], deflate: bool, zip64: bool) -> Vec<u8> {
        let (mut out, mut dir) = (Vec::new(), Vec::new());
        for (name, data) in files {
            let packed = if deflate { miniz_oxide::deflate::compress_to_vec(data, 6) } else { data.to_vec() };
            let (method, crc, offset) = (if deflate { 8u16 } else { 0 }, crc32fast::hash(data), out.len() as u64);
            out.extend(0x0403_4b50u32.to_le_bytes());
            out.extend([20, 0, 0, 0]);
            out.extend(method.to_le_bytes());
            out.extend([0; 4]);
            out.extend(crc.to_le_bytes());
            out.extend((packed.len() as u32).to_le_bytes());
            out.extend((data.len() as u32).to_le_bytes());
            out.extend((name.len() as u16).to_le_bytes());
            out.extend([0, 0]);
            out.extend(name.as_bytes());
            out.extend(&packed);
            let big = |v: u64| if zip64 { 0xFFFF_FFFFu32 } else { v as u32 };
            dir.extend(0x0201_4b50u32.to_le_bytes());
            dir.extend([45, 0, 45, 0, 0, 0]);
            dir.extend(method.to_le_bytes());
            dir.extend([0; 4]);
            dir.extend(crc.to_le_bytes());
            dir.extend(big(packed.len() as u64).to_le_bytes());
            dir.extend(big(data.len() as u64).to_le_bytes());
            dir.extend((name.len() as u16).to_le_bytes());
            dir.extend((if zip64 { 28u16 } else { 0 }).to_le_bytes());
            // comment length, disk, internal and external attributes
            dir.extend([0; 10]);
            dir.extend(big(offset).to_le_bytes());
            dir.extend(name.as_bytes());
            if zip64 {
                dir.extend([1, 0, 24, 0]);
                for v in [data.len() as u64, packed.len() as u64, offset] {
                    dir.extend(v.to_le_bytes());
                }
            }
        }
        let (dir_at, n) = (out.len() as u64, files.len() as u64);
        out.extend(&dir);
        if zip64 {
            let rec_at = out.len() as u64;
            out.extend(0x0606_4b50u32.to_le_bytes());
            out.extend(44u64.to_le_bytes());
            out.extend([45, 0, 45, 0]);
            out.extend([0; 8]);
            for v in [n, n, dir.len() as u64, dir_at] {
                out.extend(v.to_le_bytes());
            }
            out.extend(0x0706_4b50u32.to_le_bytes());
            out.extend([0; 4]);
            out.extend(rec_at.to_le_bytes());
            out.extend(1u32.to_le_bytes());
        }
        out.extend(0x0605_4b50u32.to_le_bytes());
        out.extend([0; 4]);
        let small = |v: u64, max: u64| if zip64 { max } else { v };
        out.extend((small(n, 0xFFFF) as u16).to_le_bytes());
        out.extend((small(n, 0xFFFF) as u16).to_le_bytes());
        out.extend((small(dir.len() as u64, 0xFFFF_FFFF) as u32).to_le_bytes());
        out.extend((small(dir_at, 0xFFFF_FFFF) as u32).to_le_bytes());
        out.extend([0, 0]);
        out
    }

    pub(crate) fn scratch(name: &str) -> PathBuf {
        // inside the repo (never the system's temp folder)
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch").join(format!("rustyac-content-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn stored_deflated_and_zip64_read_back() {
        let dir = scratch("zip");
        let big = vec![7u8; 100_000];
        let files: [(&str, &[u8]); 3] = [("Assetto/content/cars/a/data.acd", b"acd bytes"), ("assetto/content/cars/a/ui/ui_car.json", b"{}"), ("assetto/system/big.bin", &big)];
        for (deflate, zip64) in [(false, false), (true, false), (true, true), (false, true)] {
            let path = dir.join(format!("t{deflate}{zip64}.zip"));
            std::fs::write(&path, make_zip(&files, deflate, zip64)).unwrap();
            let zip = Zip::open(&path).unwrap();
            assert_eq!(zip.read("assetto/CONTENT/cars/a/data.acd").unwrap(), b"acd bytes");
            assert_eq!(zip.read("assetto\\system\\big.bin").unwrap(), big);
            assert_eq!(zip.len("assetto/system/big.bin"), Some(100_000));
            assert!(zip.is_dir("assetto/content/cars") && zip.is_dir("") && !zip.is_dir("assetto/content/cars/a/data.acd"));
            assert!(zip.is_file("assetto/content/cars/a/ui/ui_car.json") && !zip.is_file("assetto/content"));
            assert_eq!(zip.read_dir("assetto/content/cars/a").unwrap(), [("ui".to_string(), true), ("data.acd".to_string(), false)]);
            assert_eq!(zip.read_dir("").unwrap(), [("assetto".to_string(), true)]);
            assert!(zip.read("assetto/nothing").is_err());
        }
        // the browser's way: the parts each read lacks handed over, then the read again
        let bytes = make_zip(&files, true, true);
        let parts = Arc::new(Parts::new(bytes.len() as u64));
        let serve = |e: io::Error| {
            let n = Need::of(&e).unwrap_or_else(|| panic!("{e}"));
            parts.give(n.at, bytes[n.at as usize..(n.at + n.len) as usize].to_vec());
        };
        let zip = loop {
            match Zip::from_bytes(Path::new("picked.zip"), parts.clone()) {
                Ok(zip) => break zip,
                Err(e) => serve(e),
            }
        };
        parts.forget();
        let mut asked = 0;
        let read = loop {
            match zip.read("assetto/system/big.bin") {
                Ok(b) => break b,
                Err(e) => {
                    asked += 1;
                    serve(e)
                }
            }
        };
        assert_eq!((read, asked), (big.clone(), 2));
        assert!(Zip::listing(Path::new("f"), ["a/cars/x/", "a/b.txt"]).is_dir("a/cars/x") && Zip::listing(Path::new("f"), ["a/b.txt"]).read("a/b.txt").is_err());
        // a damaged byte inside a stored file is caught by its CRC
        let mut bytes = make_zip(&files, false, false);
        let at = bytes.windows(9).position(|w| w == b"acd bytes").unwrap();
        bytes[at] ^= 1;
        std::fs::write(dir.join("bad.zip"), bytes).unwrap();
        assert!(Zip::open(&dir.join("bad.zip")).unwrap().read("assetto/content/cars/a/data.acd").is_err());
        assert!(Zip::open(&dir.join("t.txt")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
