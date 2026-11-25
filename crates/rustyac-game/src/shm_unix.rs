// SPDX-License-Identifier: GPL-3.0-or-later

//! `shm.rs`'s two Windows calls on Linux: AC's named mappings as POSIX shared memory
//! (`shm_open("/acpmf_physics")`, the file `/dev/shm/acpmf_physics`), and the look for a
//! running game.
//!
//! As on Windows, the pages stay after the drive (a reader holding Windows' object keeps it;
//! here the file stays, `status` says `AC_OFF`), and the next drive writes into them again.

use std::ffi::CString;

use sdl3::libc;

/// Is the real game (under Proton or Wine: the process is still `acs.exe`) or another rustyAC
/// running? Looks at the names of the running processes, nothing more.
pub fn other_writer_running() -> Option<String> {
    let own = std::process::id().to_string();
    std::fs::read_dir("/proc").ok()?.flatten().find_map(|entry| {
        let pid = entry.file_name();
        let pid = pid.to_str().filter(|p| p.bytes().all(|b| b.is_ascii_digit()))?;
        let name = std::fs::read_to_string(entry.path().join("comm")).ok()?.trim_end().to_ascii_lowercase();
        match name.as_str() {
            "acs.exe" | "acs_x86.exe" => Some(format!("Assetto Corsa ({name})")),
            "rustyac" if pid != own => Some("another rustyac".to_string()),
            _ => None,
        }
    })
}

pub(super) struct Mapping {
    view: *mut u8,
    size: usize,
    /// The page was there already (left by an earlier drive or another writer).
    pub(super) existed: bool,
}

impl Mapping {
    /// `CreateFileMapping(name)` + `MapViewOfFile`: `Local\acpmf_physics` is `/acpmf_physics`.
    /// The file is whole 4096-byte pages, as Windows rounds a mapping (readers may map their
    /// own, longer struct).
    pub(super) fn create(name: &str, size: usize) -> Result<Mapping, String> {
        let name = format!("/{}", name.trim_start_matches("Local\\"));
        let c_name = CString::new(name.clone()).map_err(|e| e.to_string())?;
        let length = size.div_ceil(4096) * 4096;
        // SAFETY: a terminated name; the file is sized before it is mapped; the descriptor is
        // closed once mapped (the mapping holds the memory)
        unsafe {
            let mut existed = false;
            let mut fd = libc::shm_open(c_name.as_ptr(), libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC, 0o600);
            if fd < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST) {
                existed = true;
                fd = libc::shm_open(c_name.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC, 0);
            }
            if fd < 0 {
                return Err(format!("shm_open {name}: {}", std::io::Error::last_os_error()));
            }
            let mut stat: libc::stat = std::mem::zeroed();
            let sized = libc::fstat(fd, &mut stat) == 0 && (stat.st_size as usize >= length || libc::ftruncate(fd, length as libc::off_t) == 0);
            let view = if sized { libc::mmap(std::ptr::null_mut(), length, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0) } else { libc::MAP_FAILED };
            let error = std::io::Error::last_os_error();
            libc::close(fd);
            if view == libc::MAP_FAILED {
                return Err(format!("mapping {name}: {error}"));
            }
            Ok(Mapping { view: view.cast(), size: length, existed })
        }
    }

    pub(super) fn write(&self, bytes: &[u8]) {
        let count = bytes.len().min(self.size);
        // SAFETY: the view is `self.size` bytes long; readers copy without a lock, as on Windows
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.view, count) }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: the view was mapped in `create`, for `size` bytes, and is unmapped once
        unsafe { libc::munmap(self.view.cast(), self.size) };
    }
}

// SAFETY: the mapping is owned by whoever holds the struct; the view is plain memory.
unsafe impl Send for Mapping {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_in_dev_shm() {
        let name = format!("Local\\rustyac_test_{}", std::process::id());
        let first = Mapping::create(&name, 0x250).unwrap();
        assert!(!first.existed);
        first.write(&[7u8; 0x250]);
        let again = Mapping::create(&name, 0x250).unwrap();
        assert!(again.existed);
        let file = format!("/dev/shm/{}", name.trim_start_matches("Local\\"));
        let bytes = std::fs::read(&file).unwrap();
        std::fs::remove_file(&file).unwrap();
        assert_eq!((bytes.len(), bytes[0], bytes[0x24f], bytes[0x250]), (4096, 7, 7, 0));
    }
}
