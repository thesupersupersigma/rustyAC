// SPDX-License-Identifier: GPL-3.0-or-later

//! The preview pack: a folder with the files of at most two cars and two tracks, laid out as
//! in the game's own folder (`content/cars/<car>/data.acd`, `content/tracks/<track>/...`,
//! `system/data/...`), and a `manifest.json` that lists them with sizes and SHA-256 hashes.
//!
//! `tools/web_pack` builds it from an Assetto Corsa install; the page fetches the manifest
//! and then only the files of the car and track that were picked. The pack is somebody's
//! own copy of Kunos' files for their own use: it is never part of this repository or of a
//! release.

use std::fmt::Write as _;

/// The most cars and the most tracks a pack may hold.
pub const MAX_CARS: usize = 2;
pub const MAX_TRACKS: usize = 2;

pub const FORMAT: &str = "rustyac-preview-pack-1";

/// One file of the pack.
#[derive(Clone, Debug, PartialEq)]
pub struct PackFile {
    /// Relative to the game's folder, with forward slashes.
    pub path: String,
    pub bytes: u64,
    /// Lower-case hex.
    pub sha256: String,
}

/// A car or a track layout of the pack and the files a drive with it reads.
#[derive(Clone, Debug, PartialEq)]
pub struct PackItem {
    /// The folder's name.
    pub id: String,
    /// A track's layout; empty for a car or a track without layouts.
    pub layout: String,
    /// The menu's name.
    pub name: String,
    /// Indices into [`Manifest::files`].
    pub files: Vec<usize>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Manifest {
    pub cars: Vec<PackItem>,
    pub tracks: Vec<PackItem>,
    /// The game's own tables every drive reads (`system/data/...`): indices into `files`.
    pub shared: Vec<usize>,
    pub files: Vec<PackFile>,
}

fn json_text(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

impl Manifest {
    /// The number of a file, added when it is new.
    pub fn file(&mut self, file: PackFile) -> usize {
        match self.files.iter().position(|f| f.path.eq_ignore_ascii_case(&file.path)) {
            Some(index) => index,
            None => {
                self.files.push(file);
                self.files.len() - 1
            }
        }
    }

    /// The bytes of a set of files.
    pub fn bytes_of(&self, files: &[usize]) -> u64 {
        files.iter().map(|&i| self.files[i].bytes).sum()
    }

    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes).sum()
    }

    pub fn to_json(&self) -> String {
        let mut out = String::from("{\n  \"format\": ");
        json_text(&mut out, FORMAT);
        let list = |files: &[usize]| files.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
        for (key, items) in [("cars", &self.cars), ("tracks", &self.tracks)] {
            let _ = write!(out, ",\n  \"{key}\": [");
            for (n, item) in items.iter().enumerate() {
                out.push_str(if n == 0 { "\n    {\"id\": " } else { ",\n    {\"id\": " });
                json_text(&mut out, &item.id);
                out.push_str(", \"layout\": ");
                json_text(&mut out, &item.layout);
                out.push_str(", \"name\": ");
                json_text(&mut out, &item.name);
                let _ = write!(out, ", \"bytes\": {}, \"files\": [{}]}}", self.bytes_of(&item.files), list(&item.files));
            }
            out.push_str("\n  ]");
        }
        let _ = write!(out, ",\n  \"shared\": [{}],\n  \"bytes\": {},\n  \"files\": [", list(&self.shared), self.total_bytes());
        for (n, file) in self.files.iter().enumerate() {
            out.push_str(if n == 0 { "\n    {\"path\": " } else { ",\n    {\"path\": " });
            json_text(&mut out, &file.path);
            let _ = write!(out, ", \"bytes\": {}, \"sha256\": \"{}\"}}", file.bytes, file.sha256);
        }
        out.push_str("\n  ]\n}\n");
        out
    }
}

/// SHA-256 (FIPS 180-4), fed in pieces so a 441 MB model need not be in memory.
pub struct Sha256 {
    state: [u32; 8],
    block: [u8; 64],
    filled: usize,
    length: u64,
}

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
    0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

impl Default for Sha256 {
    fn default() -> Sha256 {
        Sha256 {
            state: [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19],
            block: [0; 64],
            filled: 0,
            length: 0,
        }
    }
}

impl Sha256 {
    pub fn new() -> Sha256 {
        Sha256::default()
    }

    fn compress(state: &mut [u32; 8], block: &[u8]) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes([block[4 * i], block[4 * i + 1], block[4 * i + 2], block[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            (h, g, f, e, d, c, b, a) = (g, f, e, d.wrapping_add(t1), c, b, a, t1.wrapping_add(t2));
        }
        for (s, v) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *s = s.wrapping_add(v);
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.length += data.len() as u64;
        if self.filled > 0 {
            let take = (64 - self.filled).min(data.len());
            self.block[self.filled..self.filled + take].copy_from_slice(&data[..take]);
            self.filled += take;
            data = &data[take..];
            if self.filled < 64 {
                return;
            }
            let block = self.block;
            Sha256::compress(&mut self.state, &block);
            self.filled = 0;
        }
        let mut chunks = data.chunks_exact(64);
        for chunk in &mut chunks {
            Sha256::compress(&mut self.state, chunk);
        }
        let rest = chunks.remainder();
        self.block[..rest.len()].copy_from_slice(rest);
        self.filled = rest.len();
    }

    /// The hash as lower-case hex.
    pub fn finish(mut self) -> String {
        let bits = self.length.wrapping_mul(8);
        self.update(&[0x80]);
        while self.filled != 56 {
            self.update(&[0]);
        }
        self.update(&bits.to_be_bytes());
        self.state.iter().map(|word| format!("{word:08x}")).collect()
    }
}

pub fn sha256_hex(data: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(data);
    hash.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_of_known_texts() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        // across the block boundary, in pieces
        let text = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        let mut hash = Sha256::new();
        hash.update(&text[..10]);
        hash.update(&text[10..]);
        assert_eq!(hash.finish(), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
        assert_eq!(sha256_hex(&[b'a'; 1000]), "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3");
    }

    #[test]
    fn the_manifest_is_json_with_sizes_per_car_and_track() {
        let mut m = Manifest::default();
        let a = m.file(PackFile { path: "content/cars/a/data.acd".into(), bytes: 10, sha256: "00".into() });
        let b = m.file(PackFile { path: "system/data/surfaces.ini".into(), bytes: 5, sha256: "11".into() });
        assert_eq!(m.file(PackFile { path: "content/cars/A/data.acd".into(), bytes: 10, sha256: "00".into() }), a);
        m.cars.push(PackItem { id: "a".into(), layout: String::new(), name: "The \"A\"".into(), files: vec![a] });
        m.shared.push(b);
        let json = m.to_json();
        assert!(json.contains("\"format\": \"rustyac-preview-pack-1\""), "{json}");
        assert!(json.contains("{\"id\": \"a\", \"layout\": \"\", \"name\": \"The \\\"A\\\"\", \"bytes\": 10, \"files\": [0]}"), "{json}");
        assert!(json.contains("\"shared\": [1],\n  \"bytes\": 15"), "{json}");
    }
}
