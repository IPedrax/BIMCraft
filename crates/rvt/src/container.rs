// Portions of this file are ported from rvt-rs (https://github.com/DrunkOnJava/rvt-rs), Apache-2.0,
// Copyright 2026 Griffin Long, at commit b6086d41b9758fd9b92f42f0a47b8345b9c58056: src/reader.rs (opening a file as a
// Compound File, its size and stream limits, listing and reading streams) and src/streams.rs (the stream names).
// Changed: rewritten for BIMCraft; the file is read through the container on demand instead of being loaded whole, every
// limit is one field of `Limits`, the signature and Revit-ness are checked up front with an error that says what the file
// is, and a failure inside the container library is an error, never a panic.
//! Opening a Revit file: the Compound File container, its streams, and the limits that keep a hostile file harmless.
//!
//! A Revit file is a Compound File (MS-CFB): a small file system in a file. This module reads it with the `cfb` crate and
//! hands back streams as bytes, the stored ones ([`Revit::read_stream`]) or inflated ([`Revit::inflate`],
//! [`Revit::inflate_all`]).

use std::io::{Read, Seek, SeekFrom};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use crate::error::{Error, Result};
use crate::gzip::{self, Fail, Inflated};

/// The 8 bytes every Compound File starts with.
const SIGNATURE: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];

/// The stream every Revit file has and no other Compound File is likely to.
pub const BASIC_FILE_INFO: &str = "BasicFileInfo";

/// Caps on what a file may make the reader do. A file over any of them is refused with an error that names the limit.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Bytes in the file.
    pub max_file: u64,
    /// Streams in the container.
    pub max_streams: usize,
    /// Stored bytes of one stream, as its directory entry declares them (a stream is read whole).
    pub max_stream: u64,
    /// Bytes one stream inflates to, over all its gzip members.
    pub max_inflated: usize,
}

impl Default for Limits {
    /// Above anything seen in Autodesk's samples (a 6 MB file, streams of a few MB) by a wide margin, and low enough that a
    /// hostile file cannot make a desktop run out of memory: the worst case is one stored stream and its inflated form. A
    /// workshared model of 128 MB, a real one, inflates its one big partition to 664 MiB, so the inflated limit is 2 GiB.
    fn default() -> Self {
        Self { max_file: 2 << 30, max_streams: 1024, max_stream: 512 << 20, max_inflated: 2 << 30 }
    }
}

trait Source: Read + Seek {}
impl<T: Read + Seek> Source for T {}

/// An open Revit file.
pub struct Revit {
    file: cfb::CompoundFile<Box<dyn Source>>,
    /// Every stream, by path without the leading slash, with the size its directory entry declares; sorted.
    streams: Vec<(String, u64)>,
    limits: Limits,
}

impl std::fmt::Debug for Revit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Revit").field("streams", &self.streams).field("limits", &self.limits).finish_non_exhaustive()
    }
}

impl Revit {
    /// Opens the file at `path`. Only its container is read now; streams are read when asked for.
    pub fn open(path: &Path, limits: Limits) -> Result<Self> {
        let named = |e: std::io::Error| Error::Io(format!("{}: {e}", path.display()));
        let meta = std::fs::metadata(path).map_err(named)?;
        if meta.is_dir() {
            return Err(Error::Io(format!("{}: is a folder, not a Revit file", path.display())));
        }
        check_size(meta.len(), &limits)?;
        let file = std::fs::File::open(path).map_err(named)?;
        Self::new(Box::new(std::io::BufReader::new(file)), meta.len(), limits)
    }

    /// Opens a file already in memory.
    pub fn from_bytes(bytes: Vec<u8>, limits: Limits) -> Result<Self> {
        check_size(bytes.len() as u64, &limits)?;
        let size = bytes.len() as u64;
        Self::new(Box::new(std::io::Cursor::new(bytes)), size, limits)
    }

    fn new(mut source: Box<dyn Source>, size: u64, limits: Limits) -> Result<Self> {
        let mut head = [0u8; 8];
        let got = read_up_to(&mut source, &mut head).map_err(|e| Error::Io(e.to_string()))?;
        source.seek(SeekFrom::Start(0)).map_err(|e| Error::Io(e.to_string()))?;
        if size == 0 {
            return Err(Error::NotRevit("the file is empty, so it is not a Revit file".into()));
        }
        if head[..got] != SIGNATURE[..got.min(8)] || got < 8 {
            let seen: Vec<String> = head[..got].iter().map(|b| format!("{b:02X}")).collect();
            return Err(Error::NotRevit(format!(
                "not a Revit file: it starts with {}, not the Compound File signature D0 CF 11 E0 A1 B1 1A E1 that Revit files start with",
                seen.join(" ")
            )));
        }
        let file = guarded(|| cfb::OpenOptions::new().open_with(source))
            .map_err(|e| Error::Damaged(format!("the Compound File container is damaged: {e}")))?;
        let streams = list(&file, &limits)?;
        let revit = Self { file, streams, limits };
        if revit.stream_len(BASIC_FILE_INFO).is_none() {
            return Err(Error::NotRevit(format!(
                "this is a Compound File but not a Revit one: it has no {BASIC_FILE_INFO} stream (its streams: {})",
                revit.streams.iter().take(8).map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ")
            )));
        }
        Ok(revit)
    }

    /// Every stream, by path (`Global/Latest`), sorted.
    pub fn stream_names(&self) -> Vec<&str> {
        self.streams.iter().map(|(name, _)| name.as_str()).collect()
    }

    /// The stored size a stream's directory entry declares.
    pub fn stream_len(&self, name: &str) -> Option<u64> {
        self.streams.iter().find(|(n, _)| n == name).map(|&(_, len)| len)
    }

    /// A stream's bytes as stored (pages and all).
    pub fn read_stream(&mut self, name: &str) -> Result<Vec<u8>> {
        let declared = self.stream_len(name).ok_or_else(|| Error::NoStream(name.to_string()))?;
        if declared > self.limits.max_stream {
            return Err(Error::TooBig {
                what: format!("stream {name} is {declared} bytes"),
                size: declared,
                limit: self.limits.max_stream,
            });
        }
        let mut out = Vec::new();
        let file = &mut self.file;
        guarded(|| {
            let mut stream = file.open_stream(format!("/{name}"))?;
            stream.by_ref().take(declared).read_to_end(&mut out)
        })
        .map_err(|e| Error::Damaged(format!("stream {name} cannot be read: {e}")))?;
        if out.len() as u64 != declared {
            return Err(Error::Damaged(format!(
                "stream {name} holds {} bytes but its directory entry says {declared}: the file is cut short or damaged",
                out.len()
            )));
        }
        Ok(out)
    }

    /// The first gzip member of a stream (a `Global/*` stream, the schema), inflated, its pages stripped first when
    /// the container pages it.
    pub fn inflate(&mut self, name: &str) -> Result<Inflated> {
        let data = self.unpaged(name)?;
        gzip::inflate_first(&data, self.limits.max_inflated).map_err(|f| self.failure(name, f))
    }

    /// Every gzip member of a stream (a partition), inflated and joined.
    pub fn inflate_all(&mut self, name: &str) -> Result<Inflated> {
        let data = self.unpaged(name)?;
        gzip::inflate_members(&data, self.limits.max_inflated).map_err(|f| self.failure(name, f))
    }

    fn unpaged(&mut self, name: &str) -> Result<Vec<u8>> {
        let stored = self.read_stream(name)?;
        Ok(if gzip::is_paged(name) { gzip::strip_pages(stored) } else { stored })
    }

    fn failure(&self, name: &str, fail: Fail) -> Error {
        let stream = name.to_string();
        match fail {
            Fail::NoMember => Error::NoGzip(stream),
            Fail::Bad { offset, reason } => Error::BadDeflate { stream, offset, reason },
            Fail::TooMuch => Error::TooMuch { stream, limit: self.limits.max_inflated },
        }
    }
}

fn check_size(size: u64, limits: &Limits) -> Result<()> {
    if size > limits.max_file {
        return Err(Error::TooBig { what: format!("the file is {size} bytes"), size, limit: limits.max_file });
    }
    Ok(())
}

/// Reads until `buf` is full or the source ends.
fn read_up_to(source: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut got = 0;
    while got < buf.len() {
        match source.read(&mut buf[got..])? {
            0 => break,
            n => got += n,
        }
    }
    Ok(got)
}

/// The container library is third-party code reading a hostile file: whatever it does, the reader gets an error.
fn guarded<T>(f: impl FnOnce() -> std::io::Result<T>) -> std::io::Result<T> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| Err(std::io::Error::other("the container library failed on this file")))
}

/// Every stream of the container, sorted, if there are no more than the limit.
fn list(file: &cfb::CompoundFile<Box<dyn Source>>, limits: &Limits) -> Result<Vec<(String, u64)>> {
    let count = file.walk().filter(|e| e.is_stream()).count();
    if count > limits.max_streams {
        return Err(Error::TooBig { what: format!("the file has {count} streams"), size: count as u64, limit: limits.max_streams as u64 });
    }
    let mut streams: Vec<(String, u64)> = file
        .walk()
        .filter(|e| e.is_stream())
        .map(|e| (e.path().to_string_lossy().replace('\\', "/").trim_start_matches('/').to_string(), e.len()))
        .collect();
    streams.sort();
    Ok(streams)
}
