//! The files a command reads and writes. On a computer they are the computer's; in a browser there is no file system, so
//! they are the page's own, held in memory: the front end puts in what the user picked (`put`) before a command reads it,
//! and takes out what commands wrote (`written`, `take`) to hand it to the user as a download. Commands name files by path
//! either way.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

/// The most the page's files may hold in all: four of the largest project the core opens.
pub const MAX_HELD: usize = 1 << 30;

/// Files held in memory, by path: what a browser has in place of a disk.
#[derive(Default)]
pub struct Memory {
    files: BTreeMap<PathBuf, Vec<u8>>,
    /// What commands wrote and the front end hasn't taken yet.
    written: BTreeSet<PathBuf>,
}

impl Memory {
    fn held(&self) -> usize {
        self.files.values().map(Vec::len).sum()
    }

    /// What a command writes: kept, and noted for the front end to hand over.
    pub fn write(&mut self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.put(path, bytes)?;
        self.written.insert(path.to_path_buf());
        Ok(())
    }

    /// Keeps `bytes` at `path`, in place of what was there. Refused when the page would hold more than [`MAX_HELD`].
    pub fn put(&mut self, path: &Path, bytes: &[u8]) -> Result<()> {
        let before = self.files.get(path).map_or(0, Vec::len);
        if self.held() - before + bytes.len() > MAX_HELD {
            return Err(Error::new(ErrorKind::OutOfMemory, "the page holds as many files as it can: reload it to start afresh"));
        }
        self.files.insert(path.to_path_buf(), bytes.to_vec());
        Ok(())
    }

    pub fn read(&self, path: &Path) -> Result<Vec<u8>> {
        self.files.get(path).cloned().ok_or_else(|| not_found(path))
    }

    pub fn len(&self, path: &Path) -> Result<u64> {
        self.files.get(path).map(|f| f.len() as u64).ok_or_else(|| not_found(path))
    }

    /// What is at `path`, no longer held.
    pub fn take(&mut self, path: &Path) -> Option<Vec<u8>> {
        self.written.remove(path);
        self.files.remove(path)
    }

    /// The files commands wrote since this was last asked, oldest path first.
    pub fn written(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.written).into_iter().collect()
    }
}

fn not_found(path: &Path) -> Error {
    Error::new(ErrorKind::NotFound, format!("there's no file {} in this page: pick it again", path.display()))
}

fn utf8(bytes: Vec<u8>) -> Result<String> {
    String::from_utf8(bytes).map_err(|e| Error::new(ErrorKind::InvalidData, e))
}

#[cfg(not(target_arch = "wasm32"))]
mod place {
    use super::*;

    pub fn read(path: &Path) -> Result<Vec<u8>> {
        std::fs::read(path)
    }

    pub fn len(path: &Path) -> Result<u64> {
        Ok(std::fs::metadata(path)?.len())
    }

    pub fn write(path: &Path, bytes: &[u8]) -> Result<()> {
        std::fs::write(path, bytes)
    }
}

#[cfg(target_arch = "wasm32")]
mod place {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static PAGE: RefCell<Memory> = RefCell::default();
    }

    pub fn read(path: &Path) -> Result<Vec<u8>> {
        PAGE.with_borrow(|m| m.read(path))
    }

    pub fn len(path: &Path) -> Result<u64> {
        PAGE.with_borrow(|m| m.len(path))
    }

    pub fn write(path: &Path, bytes: &[u8]) -> Result<()> {
        PAGE.with_borrow_mut(|m| m.write(path, bytes))
    }

    pub fn put(path: &Path, bytes: &[u8]) -> Result<()> {
        PAGE.with_borrow_mut(|m| m.put(path, bytes))
    }

    pub fn take(path: &Path) -> Option<Vec<u8>> {
        PAGE.with_borrow_mut(|m| m.take(path))
    }

    pub fn written() -> Vec<PathBuf> {
        PAGE.with_borrow_mut(Memory::written)
    }
}

/// The bytes of the file at `path`.
pub fn read(path: impl AsRef<Path>) -> Result<Vec<u8>> {
    place::read(path.as_ref())
}

/// The text of the file at `path`, which must be UTF-8.
pub fn read_to_string(path: impl AsRef<Path>) -> Result<String> {
    utf8(read(path)?)
}

/// How many bytes the file at `path` holds.
pub fn len(path: impl AsRef<Path>) -> Result<u64> {
    place::len(path.as_ref())
}

/// Writes `bytes` to the file at `path`, in place of what it held.
pub fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> Result<()> {
    place::write(path.as_ref(), bytes.as_ref())
}

/// Writes `text` to the file at `path` whole or not at all: on a computer the old file is replaced only once the new one is
/// complete (`autosave::atomic_write`), as a project is saved; in a browser the page's file is replaced at once.
pub fn replace(path: impl AsRef<Path>, text: &str) -> std::result::Result<(), String> {
    let path = path.as_ref();
    if cfg!(target_arch = "wasm32") {
        return place::write(path, text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()));
    }
    crate::autosave::atomic_write(path, text)
}

/// In a browser: puts a file the user picked in the page, at `path`, for a command to read.
#[cfg(target_arch = "wasm32")]
pub fn put(path: impl AsRef<Path>, bytes: &[u8]) -> Result<()> {
    place::put(path.as_ref(), bytes)
}

/// In a browser: the files commands wrote since this was last asked, to [`take`] and hand to the user.
#[cfg(target_arch = "wasm32")]
pub fn written() -> Vec<PathBuf> {
    place::written()
}

/// In a browser: takes out the file a command wrote at `path`, to hand to the user.
#[cfg(target_arch = "wasm32")]
pub fn take(path: impl AsRef<Path>) -> Option<Vec<u8>> {
    place::take(path.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_keeps_what_is_written_until_it_is_taken() {
        let mut m = Memory::default();
        let path = Path::new("/page/house.bimcraft");
        assert_eq!(m.read(path).map_err(|e| e.kind()), Err(ErrorKind::NotFound));
        m.write(path, b"first").unwrap();
        m.write(path, b"second").unwrap();
        assert_eq!(m.read(path).unwrap(), b"second");
        assert_eq!(m.len(path).unwrap(), 6);
        assert_eq!(m.take(path).as_deref(), Some(&b"second"[..]));
        assert!(m.read(path).is_err() && m.take(path).is_none());
    }

    #[test]
    fn memory_lists_what_commands_wrote_not_what_was_put_in() {
        let mut m = Memory::default();
        m.put(Path::new("/picked.ifc"), b"ISO-10303-21;").unwrap();
        m.write(Path::new("/b.pdf"), b"%PDF").unwrap();
        m.write(Path::new("/a.png"), b"png").unwrap();
        m.write(Path::new("/taken.svg"), b"svg").unwrap();
        m.take(Path::new("/taken.svg"));
        assert_eq!(m.written(), [Path::new("/a.png"), Path::new("/b.pdf")]);
        assert!(m.written().is_empty(), "each is listed once");
        assert_eq!(m.read(Path::new("/b.pdf")).unwrap(), b"%PDF");
    }

    #[test]
    fn memory_refuses_more_than_it_may_hold() {
        let mut m = Memory::default();
        let big = vec![0; MAX_HELD / 2];
        m.write(Path::new("a"), &big).unwrap();
        m.write(Path::new("b"), &big).unwrap();
        assert_eq!(m.write(Path::new("c"), b"x").map_err(|e| e.kind()), Err(ErrorKind::OutOfMemory));
        // Writing over a file counts what it held as given back.
        m.write(Path::new("b"), b"small").unwrap();
        m.write(Path::new("c"), b"x").unwrap();
    }

    #[test]
    fn a_computer_s_files_are_its_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plan.svg");
        write(&path, "<svg/>").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "<svg/>");
        assert_eq!((read_to_string(&path).unwrap().as_str(), len(&path).unwrap()), ("<svg/>", 6));
        assert!(read(dir.path().join("none")).is_err());
        replace(&path, "<svg></svg>").unwrap();
        assert_eq!(read_to_string(&path).unwrap(), "<svg></svg>");
    }
}
