//! The files a command reads and writes. On a computer they are the computer's; in a browser there is no file system, so
//! they are the page's own, held in memory: the front end puts in what the user picked (`put`) before a command reads it,
//! and takes out what commands wrote (`written`, `take`) to hand it to the user as a download. Commands name files by path
//! either way.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

/// The most the page's files may hold at once: twice the largest project the core opens. A page's memory is 4 GB at most, and
/// a file read is a second copy of it; the front end takes out what was picked once it is read, and what was written once it
/// is handed over, so this is reached only by files that size in flight together.
pub const MAX_HELD: usize = 512 << 20;

/// Files held in memory, by path: what a browser has in place of a disk.
#[derive(Default)]
pub struct Memory {
    files: BTreeMap<PathBuf, Vec<u8>>,
    /// What commands wrote and the front end hasn't taken yet.
    written: BTreeSet<PathBuf>,
    /// Called when a command writes, so the front end hands it over without waiting for the next input.
    wake: Option<Box<dyn Fn()>>,
}

impl Memory {
    fn held(&self) -> usize {
        self.files.values().map(Vec::len).sum()
    }

    /// What a command writes: kept, and noted for the front end to hand over.
    pub fn write(&mut self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.put(path, bytes.to_vec())?;
        self.written.insert(path.to_path_buf());
        if let Some(wake) = &self.wake {
            wake();
        }
        Ok(())
    }

    /// Keeps `bytes` at `path`, in place of what was there, and no longer to be handed over if a command wrote it. Refused
    /// when the page would hold more than [`MAX_HELD`].
    pub fn put(&mut self, path: &Path, bytes: Vec<u8>) -> Result<()> {
        let before = self.files.get(path).map_or(0, Vec::len);
        if (self.held() - before).checked_add(bytes.len()).is_none_or(|held| held > MAX_HELD) {
            let most = MAX_HELD >> 20;
            return Err(Error::new(ErrorKind::OutOfMemory, format!("the page holds at most {most} MB of files at once")));
        }
        self.written.remove(path);
        self.files.insert(path.to_path_buf(), bytes);
        Ok(())
    }

    /// Calls `wake` each time a command writes a file.
    pub fn when_written(&mut self, wake: Box<dyn Fn()>) {
        self.wake = Some(wake);
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

    /// A folder, a pipe or a device is no file to read: a pipe says it holds nothing and then never ends.
    pub fn len(path: &Path) -> Result<u64> {
        let meta = std::fs::metadata(path)?;
        if !meta.is_file() {
            return Err(Error::new(ErrorKind::InvalidInput, format!("{} is not a file", path.display())));
        }
        Ok(meta.len())
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

    pub fn put(path: &Path, bytes: Vec<u8>) -> Result<()> {
        PAGE.with_borrow_mut(|m| m.put(path, bytes))
    }

    pub fn when_written(wake: Box<dyn Fn()>) {
        PAGE.with_borrow_mut(|m| m.when_written(wake));
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
pub fn put(path: impl AsRef<Path>, bytes: Vec<u8>) -> Result<()> {
    place::put(path.as_ref(), bytes)
}

/// In a browser: calls `wake` each time a command writes a file, for the front end to hand it over at once.
#[cfg(target_arch = "wasm32")]
pub fn when_written(wake: impl Fn() + 'static) {
    place::when_written(Box::new(wake));
}

/// In a browser: the files commands wrote since this was last asked, to [`take`] and hand to the user.
#[cfg(target_arch = "wasm32")]
pub fn written() -> Vec<PathBuf> {
    place::written()
}

/// In a browser: takes out the file at `path`, one a command wrote to hand to the user or one picked that has been read.
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
        m.put(Path::new("/picked.ifc"), b"ISO-10303-21;".to_vec()).unwrap();
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
    fn memory_wakes_the_front_end_when_a_command_writes_not_when_a_file_is_put_in() {
        let woken = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut m = Memory::default();
        let count = woken.clone();
        m.when_written(Box::new(move || count.set(count.get() + 1)));
        m.put(Path::new("/picked.ifc"), b"x".to_vec()).unwrap();
        assert_eq!(woken.get(), 0);
        m.write(Path::new("/house.bimcraft"), b"{}").unwrap();
        assert_eq!(woken.get(), 1);
    }

    #[test]
    fn a_file_put_in_is_not_handed_over_as_one_written() {
        let mut m = Memory::default();
        m.write(Path::new("/house.ifc"), b"written").unwrap();
        m.put(Path::new("/house.ifc"), b"picked".to_vec()).unwrap();
        assert!(m.written().is_empty());
        assert_eq!(m.read(Path::new("/house.ifc")).unwrap(), b"picked");
    }

    #[test]
    fn memory_full_says_so_without_asking_for_a_reload() {
        let mut m = Memory::default();
        m.put(Path::new("a"), vec![0; MAX_HELD]).unwrap();
        let e = m.write(Path::new("b"), b"x").unwrap_err();
        assert_eq!(e.kind(), ErrorKind::OutOfMemory);
        assert!(!e.to_string().contains("reload"), "{e}");
    }

    #[test]
    fn a_computer_s_folder_or_device_is_not_a_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(len(dir.path()).map_err(|e| e.kind()), Err(ErrorKind::InvalidInput));
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
