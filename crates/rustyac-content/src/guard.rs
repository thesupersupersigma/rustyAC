// SPDX-License-Identifier: MIT OR Apache-2.0

//! The one gate every file rustyAC writes, makes, moves or deletes goes through: nothing is
//! ever written into Assetto Corsa's own folder ([`crate::install::ac_root`]) or into the
//! user's real `Documents\Assetto Corsa` (`--documents` points elsewhere, never there). A
//! refused path is an `io::ErrorKind::PermissionDenied` error that names it.

use std::fs::File;
use std::io;
use std::path::{Component, Path, PathBuf};

/// The user's real `Documents\Assetto Corsa`, whatever `--documents` says.
fn real_documents() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("USERPROFILE")?).join("Documents").join("Assetto Corsa"))
}

/// The absolute path with links and junctions of its existing part resolved, `..` folded, in
/// lower case (Windows' file names are not case sensitive).
fn resolved(path: &Path) -> Option<PathBuf> {
    let path = std::path::absolute(path).ok()?;
    let mut existing = path.as_path();
    let mut rest = Vec::new();
    let mut base = loop {
        if let Ok(real) = existing.canonicalize() {
            break real;
        }
        rest.push(existing.file_name()?.to_os_string());
        existing = existing.parent()?;
    };
    for part in rest.iter().rev() {
        match Path::new(part).components().next() {
            Some(Component::ParentDir) => {
                base.pop();
            }
            Some(Component::CurDir) | None => {}
            _ => base.push(part),
        }
    }
    Some(PathBuf::from(base.to_string_lossy().to_lowercase()))
}

/// `Ok` when `path` may be written: it is outside Assetto Corsa's folder and the user's
/// `Documents\Assetto Corsa`.
pub fn check(path: &Path) -> io::Result<()> {
    let Some(target) = resolved(path) else { return Ok(()) };
    for (what, folder) in [("Assetto Corsa's folder", crate::install::ac_root()), ("Documents\\Assetto Corsa", real_documents())] {
        if let Some(folder) = folder.and_then(|f| resolved(&f)) {
            if target.starts_with(&folder) {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("refusing to write {}: it is inside {what} ({}), which rustyAC only reads", path.display(), folder.display())));
            }
        }
    }
    Ok(())
}

pub fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
    check(path.as_ref())?;
    std::fs::write(path, contents)
}

pub fn create(path: impl AsRef<Path>) -> io::Result<File> {
    check(path.as_ref())?;
    File::create(path)
}

/// Opens for appending, made if missing.
pub fn append(path: impl AsRef<Path>) -> io::Result<File> {
    check(path.as_ref())?;
    std::fs::OpenOptions::new().create(true).append(true).open(path)
}

pub fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    check(path.as_ref())?;
    std::fs::create_dir_all(path)
}

pub fn create_dir(path: impl AsRef<Path>) -> io::Result<()> {
    check(path.as_ref())?;
    std::fs::create_dir(path)
}

pub fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
    check(path.as_ref())?;
    std::fs::remove_file(path)
}

pub fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    check(path.as_ref())?;
    std::fs::remove_dir_all(path)
}

pub fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
    check(from.as_ref())?;
    check(to.as_ref())?;
    std::fs::rename(from, to)
}

pub fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<u64> {
    check(to.as_ref())?;
    std::fs::copy(from, to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documents_refused_elsewhere_allowed() {
        let Some(docs) = real_documents() else { return };
        assert!(check(&docs.join("cfg").join("race.ini")).is_err());
        assert!(check(&docs.join("new_folder").join("..").join("x.txt")).is_err());
        assert!(check(&Path::new(env!("CARGO_MANIFEST_DIR")).join("no_such_file.txt")).is_ok());
    }
}
