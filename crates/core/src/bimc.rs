//! A project file's bytes: its JSON, compact, in a zip of one entry (`project.json`), deflated on every core. Any zip tool opens
//! one, and the JSON inside is what a project file always held, so a file is still read kindly ([`crate::lenient`]).
//!
//! The JSON is cut into pieces of [`PIECE`] bytes, each deflated on its own by a compressor of its own and ended with a sync
//! flush, as pigz does: put together they are one deflate stream, which any inflater reads, and since no piece refers back
//! past its own start, each can be inflated on its own too. Where they start and how long each is goes in a second entry,
//! `pieces.json`, so a reader inflates them all at once; a file without it, or whose numbers don't add up, is inflated whole,
//! in one go. A file that doesn't begin as a zip does is read as the JSON it is: every project saved before this.
//!
//! A file is hostile: what an entry says it inflates to is held to what the caller allows before anything is inflated, and
//! inflating stops at that size whatever the entry said.

use std::io::{Read, Write};

use flate2::{Compress, Compression, Crc, FlushCompress, Status};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

/// The entry the project's JSON is in.
pub const ENTRY: &str = "project.json";
/// The entry that says where each piece of it starts.
const PIECES: &str = "pieces.json";
/// How much of the JSON each piece deflates: big enough that cutting it costs nothing in size, small enough that a project of
/// a few megabytes still uses every core.
const PIECE: usize = 1 << 20;
/// The most pieces a file may say it has: 2^16 MiB of JSON.
const MAX_PIECES: usize = 1 << 16;

/// What the start of a zip's local file header is.
const LOCAL: &[u8; 4] = b"PK\x03\x04";

/// Whether `bytes` are a zip, as a project file in its container is.
pub fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(LOCAL)
}

/// The pieces of a deflated entry, in order: how many bytes each takes in the entry, and how many it inflates to.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pieces {
    packed: Vec<u64>,
    raw: Vec<u64>,
}

/// `json` in a project file's container: deflated on every core at `level` (0 to 9).
pub fn pack(json: &str, level: u32) -> Result<Vec<u8>, String> {
    let data = json.as_bytes();
    if data.len() as u64 >= u64::from(u32::MAX) {
        return Err(format!("a project's JSON of {} bytes is past what a zip of this BIMCraft holds (4 GiB)", data.len()));
    }
    let chunks: Vec<&[u8]> = if data.is_empty() { vec![data] } else { data.chunks(PIECE).collect() };
    let last = chunks.len() - 1;
    let deflated: Vec<(Vec<u8>, Crc)> = chunks
        .par_iter()
        .enumerate()
        .map(|(i, chunk)| {
            let mut crc = Crc::new();
            crc.update(chunk);
            (deflate(chunk, level, i == last), crc)
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|(d, crc)| d.map(|d| (d, crc)))
        .collect::<Result<_, _>>()?;
    let mut crc = Crc::new();
    for (_, c) in &deflated {
        crc.combine(c);
    }
    let pieces =
        Pieces { packed: deflated.iter().map(|(d, _)| d.len() as u64).collect(), raw: chunks.iter().map(|c| c.len() as u64).collect() };
    let packed: Vec<u8> = deflated.into_iter().flat_map(|(d, _)| d).collect();
    let index = serde_json::to_vec(&pieces).map_err(|e| e.to_string())?;
    let mut index_crc = Crc::new();
    index_crc.update(&index);
    let mut zip = Zip::default();
    zip.add(ENTRY, 8, &packed, crc.sum(), data.len() as u64)?;
    zip.add(PIECES, 0, &index, index_crc.sum(), index.len() as u64)?;
    zip.finish()
}

/// `chunk` deflated by a compressor of its own: ended with a sync flush, so the next piece follows it, or, the `last`, finished.
fn deflate(chunk: &[u8], level: u32, last: bool) -> Result<Vec<u8>, String> {
    let mut c = Compress::new(Compression::new(level.min(9)), false);
    let mut out = Vec::with_capacity(chunk.len() / 4 + 64);
    let flush = if last { FlushCompress::Finish } else { FlushCompress::Sync };
    loop {
        let before = c.total_in() as usize;
        out.reserve(chunk.len() / 8 + 1024);
        let status = c.compress_vec(&chunk[before.min(chunk.len())..], &mut out, flush).map_err(|e| e.to_string())?;
        let done = c.total_in() as usize >= chunk.len();
        match status {
            Status::StreamEnd => return Ok(out),
            // A sync flush has emptied its output when it leaves room in the buffer.
            Status::Ok | Status::BufError if done && !last && out.len() < out.capacity() => return Ok(out),
            Status::Ok | Status::BufError => {}
        }
    }
}

/// What the project in zip `bytes` says it inflates to: none when they aren't a zip holding one.
pub fn inflated_size(bytes: &[u8]) -> Option<u64> {
    entries(bytes).ok()?.into_iter().find(|e| e.name == ENTRY).map(|e| e.size)
}

/// The JSON of a project file's `bytes`: inflated from its container, held to `most` bytes; or the bytes as they are, when they
/// aren't a zip (a file from before the container, or written plain).
pub fn unpack(bytes: Vec<u8>, most: u64) -> Result<String, String> {
    if !is_zip(&bytes) {
        return String::from_utf8(bytes).map_err(|e| format!("it isn't text: {e}"));
    }
    let entries = entries(&bytes)?;
    let Some(entry) = entries.iter().find(|e| e.name == ENTRY) else {
        return Err(format!("it is a zip without a `{ENTRY}`: not a BIMCraft project"));
    };
    if entry.size > most {
        return Err(format!("its project inflates to {} bytes, past the {most} it may take here", entry.size));
    }
    let data = entry.data(&bytes)?;
    let raw = match entry.method {
        0 => data.to_vec(),
        8 => {
            let pieces = entries.iter().find(|e| e.name == PIECES).and_then(|e| e.data(&bytes).ok()).and_then(|d| {
                let p: Pieces = serde_json::from_slice(d).ok()?;
                p.fits(data.len() as u64, entry.size).then_some(p)
            });
            match pieces {
                Some(p) => inflate_pieces(data, &p)?,
                None => inflate(data, entry.size.min(most))?,
            }
        }
        m => return Err(format!("its project is compressed in a way this BIMCraft doesn't read (zip method {m})")),
    };
    if raw.len() as u64 != entry.size {
        return Err(format!("its project inflates to {} bytes where the zip says {}: the file is damaged", raw.len(), entry.size));
    }
    let mut crc = Crc::new();
    crc.update(&raw);
    if crc.sum() != entry.crc {
        return Err("its project doesn't match its checksum: the file is damaged".into());
    }
    String::from_utf8(raw).map_err(|e| format!("its project isn't text: {e}"))
}

impl Pieces {
    /// Whether these pieces are the whole of an entry of `packed` bytes inflating to `raw`.
    fn fits(&self, packed: u64, raw: u64) -> bool {
        let sum = |v: &[u64]| v.iter().try_fold(0u64, |a, b| a.checked_add(*b));
        self.packed.len() == self.raw.len()
            && !self.packed.is_empty()
            && self.packed.len() <= MAX_PIECES
            && sum(&self.packed) == Some(packed)
            && sum(&self.raw) == Some(raw)
            && self.raw.iter().all(|r| *r <= PIECE as u64)
    }
}

/// `data`, raw deflate, inflated to at most `most` bytes.
fn inflate(data: &[u8], most: u64) -> Result<Vec<u8>, String> {
    // As much as JSON usually inflates to from this much, not what a hostile header says: the rest grows as it comes.
    let likely = (data.len() as u64).saturating_mul(16).min(most).min(1 << 28);
    let mut out = Vec::with_capacity(usize::try_from(likely).unwrap_or(0));
    flate2::read::DeflateDecoder::new(data)
        .take(most.saturating_add(1))
        .read_to_end(&mut out)
        .map_err(|e| format!("it can't be inflated: {e}"))?;
    Ok(out)
}

/// `data` inflated a piece at a time, on every core.
fn inflate_pieces(data: &[u8], p: &Pieces) -> Result<Vec<u8>, String> {
    let mut at = 0usize;
    let mut slices = Vec::with_capacity(p.packed.len());
    for (packed, raw) in p.packed.iter().zip(&p.raw) {
        let end = at + *packed as usize;
        slices.push((data.get(at..end).ok_or("its pieces lie past its end")?, *raw));
        at = end;
    }
    let parts: Vec<Vec<u8>> = slices
        .par_iter()
        .map(|(slice, raw)| {
            let mut out = Vec::with_capacity(*raw as usize);
            let mut d = flate2::Decompress::new(false);
            // A piece but the last ends on a sync flush, not the stream's end: inflated until its bytes are all read.
            loop {
                let (i, o) = (d.total_in() as usize, out.len());
                out.reserve((*raw as usize).saturating_sub(out.len()).max(64));
                let status = d
                    .decompress_vec(&slice[i.min(slice.len())..], &mut out, flate2::FlushDecompress::Sync)
                    .map_err(|e| format!("it can't be inflated: {e}"))?;
                if out.len() as u64 > *raw {
                    return Err("a piece inflates to more than it says".to_string());
                }
                if status == Status::StreamEnd || d.total_in() as usize >= slice.len() && out.len() as u64 == *raw {
                    break;
                }
                if d.total_in() as usize == i && out.len() == o {
                    return Err("a piece ends before it inflates to what it says".to_string());
                }
            }
            Ok(out)
        })
        .collect::<Result<_, String>>()?;
    let mut out = Vec::with_capacity(p.raw.iter().sum::<u64>() as usize);
    for part in parts {
        out.extend_from_slice(&part);
    }
    Ok(out)
}

/// A zip's entry, as its central directory lists it.
struct Entry {
    name: String,
    method: u16,
    crc: u32,
    packed: u64,
    size: u64,
    local: u64,
}

impl Entry {
    /// Its bytes in the zip, as they are stored.
    fn data<'a>(&self, b: &'a [u8]) -> Result<&'a [u8], String> {
        let local = usize::try_from(self.local).map_err(|_| DAMAGED)?;
        if b.get(local..local.saturating_add(4)) != Some(LOCAL.as_slice()) {
            return Err(DAMAGED.into());
        }
        let start = local + 30 + u16_at(b, local + 26).ok_or(DAMAGED)? + u16_at(b, local + 28).ok_or(DAMAGED)?;
        let len = usize::try_from(self.packed).map_err(|_| DAMAGED)?;
        b.get(start..start.saturating_add(len)).ok_or_else(|| DAMAGED.into())
    }
}

const DAMAGED: &str = "it is a damaged zip";

fn u16_at(b: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?) as usize)
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The entries the central directory of zip `b` lists.
fn entries(b: &[u8]) -> Result<Vec<Entry>, String> {
    // The end of the central directory: 22 bytes, and a comment of at most 65,535 after them.
    let from = b.len().saturating_sub(22 + 65_535);
    let end = (from..b.len().saturating_sub(21)).rev().find(|&i| b.get(i..i + 4) == Some(b"PK\x05\x06")).ok_or(DAMAGED)?;
    let count = u16_at(b, end + 10).ok_or(DAMAGED)?;
    let mut at = u32_at(b, end + 16).ok_or(DAMAGED)? as usize;
    let mut out = Vec::with_capacity(count.min(64));
    for _ in 0..count.min(64) {
        if b.get(at..at + 4) != Some(b"PK\x01\x02") {
            return Err(DAMAGED.into());
        }
        let method = u16_at(b, at + 10).ok_or(DAMAGED)? as u16;
        let crc = u32_at(b, at + 16).ok_or(DAMAGED)?;
        let packed = u64::from(u32_at(b, at + 20).ok_or(DAMAGED)?);
        let size = u64::from(u32_at(b, at + 24).ok_or(DAMAGED)?);
        let (name_len, extra, comment) =
            (u16_at(b, at + 28).ok_or(DAMAGED)?, u16_at(b, at + 30).ok_or(DAMAGED)?, u16_at(b, at + 32).ok_or(DAMAGED)?);
        let local = u64::from(u32_at(b, at + 42).ok_or(DAMAGED)?);
        let name = String::from_utf8_lossy(b.get(at + 46..at + 46 + name_len).ok_or(DAMAGED)?).into_owned();
        out.push(Entry { name, method, crc, packed, size, local });
        at += 46 + name_len + extra + comment;
    }
    Ok(out)
}

/// A zip being written: its entries' bytes, and their central directory.
#[derive(Default)]
struct Zip {
    out: Vec<u8>,
    central: Vec<u8>,
    count: u16,
}

impl Zip {
    /// An entry `name` of `data`, stored as `method` (0 stored, 8 deflated) with the checksum and size of what it holds.
    fn add(&mut self, name: &str, method: u16, data: &[u8], crc: u32, size: u64) -> Result<(), String> {
        let too_big = || "a project file of 4 GiB or more is past what this BIMCraft writes".to_string();
        let offset = u32::try_from(self.out.len()).map_err(|_| too_big())?;
        let packed = u32::try_from(data.len()).map_err(|_| too_big())?;
        let size = u32::try_from(size).map_err(|_| too_big())?;
        let header = |central: bool| {
            let mut h = Vec::with_capacity(46 + name.len());
            h.extend(if central { 0x0201_4b50u32 } else { 0x0403_4b50 }.to_le_bytes());
            if central {
                h.extend(20u16.to_le_bytes()); // made by
            }
            h.extend(20u16.to_le_bytes()); // version needed
            h.extend(0x0800u16.to_le_bytes()); // the name is UTF-8
            h.extend(method.to_le_bytes());
            h.extend([0u8; 4]); // time and date: none
            h.extend(crc.to_le_bytes());
            h.extend(packed.to_le_bytes());
            h.extend(size.to_le_bytes());
            h.extend((name.len() as u16).to_le_bytes());
            h.extend(0u16.to_le_bytes()); // extra
            if central {
                h.extend([0u8; 6]); // comment length, disk, internal attributes
                h.extend(0u32.to_le_bytes()); // external attributes
                h.extend(offset.to_le_bytes());
            }
            h.extend(name.as_bytes());
            h
        };
        self.out.extend(header(false));
        self.out.extend_from_slice(data);
        self.central.extend(header(true));
        self.count += 1;
        Ok(())
    }

    fn finish(mut self) -> Result<Vec<u8>, String> {
        let at = u32::try_from(self.out.len()).map_err(|_| "a project file of 4 GiB or more is past what this BIMCraft writes")?;
        let len = self.central.len() as u32;
        self.out.append(&mut self.central);
        self.out.extend(0x0605_4b50u32.to_le_bytes());
        self.out.extend([0u8; 4]); // disks
        self.out.extend(self.count.to_le_bytes());
        self.out.extend(self.count.to_le_bytes());
        self.out.extend(len.to_le_bytes());
        self.out.extend(at.to_le_bytes());
        self.out.extend(0u16.to_le_bytes()); // comment
        Ok(self.out)
    }
}

/// `m` as compact JSON, its elements written on every core: the text `serde_json::to_string` makes of it, byte for byte (a test
/// holds it to that), in a fraction of the time for a project of a million elements.
pub fn json<T: Serialize>(m: &T, elements: &crate::ids::Elements) -> Result<String, String> {
    // The rest of the model, with its elements left empty, then theirs put where its own empty map is.
    let rest = serde_json::to_string(m).map_err(|e| e.to_string())?;
    let Some(at) = top_level_key(&rest, "elements") else { return Err("a model's JSON has no `elements`".into()) };
    let empty = "{}";
    if rest.get(at..at + empty.len()) != Some(empty) {
        return Err("a model's elements weren't left empty to be written apart".into());
    }
    #[derive(Serialize)]
    struct With<'a> {
        guid: crate::ids::Guid,
        #[serde(flatten)]
        e: &'a crate::model::Element,
    }
    let all: Vec<(crate::ids::Id, crate::ids::Guid, &crate::model::Element)> = elements.with_guids().collect();
    let parts: Vec<String> = all
        .par_chunks(4096)
        .map(|chunk| {
            let mut out = Vec::with_capacity(chunk.len() * 256);
            for (i, (id, guid, e)) in chunk.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                serde_json::to_writer(&mut out, &id.to_string()).map_err(|e| e.to_string())?;
                out.push(b':');
                serde_json::to_writer(&mut out, &With { guid: *guid, e }).map_err(|e| e.to_string())?;
            }
            String::from_utf8(out).map_err(|e| e.to_string())
        })
        .collect::<Result<_, String>>()?;
    let mut out = String::with_capacity(rest.len() + parts.iter().map(String::len).sum::<usize>() + parts.len() + 2);
    out.push_str(&rest[..at]);
    out.push('{');
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(part);
    }
    out.push('}');
    out.push_str(&rest[at + empty.len()..]);
    Ok(out)
}

/// Where the value of key `key` of the object `json` is begins: its own key, not one of an object inside it, nor text in a string.
fn top_level_key(json: &str, key: &str) -> Option<usize> {
    let b = json.as_bytes();
    let quoted = format!("\"{key}\":");
    let (mut depth, mut i, mut in_string) = (0i32, 0usize, false);
    while i < b.len() {
        let c = b[i];
        if in_string {
            match c {
                b'\\' => i += 1,
                b'"' => in_string = false,
                _ => {}
            }
        } else {
            match c {
                b'"' if depth == 1 && json[i..].starts_with(&quoted) => return Some(i + quoted.len()),
                b'"' => in_string = true,
                b'{' | b'[' => depth += 1,
                b'}' | b']' => depth -= 1,
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// The JSON of the project file at `path`, zipped or not: what a test or a tool that reads a saved project as text reads.
pub fn read_to_string(path: impl AsRef<std::path::Path>) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    unpack(bytes, u64::MAX).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// `bytes` written to `path` in the way [`crate::autosave::atomic_write`] writes text: a file beside it first, then moved over it.
pub fn write(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| format!("{}: {e}", path.display()))?;
    tmp.write_all(bytes).and_then(|()| tmp.as_file().sync_all()).map_err(|e| format!("{}: {e}", path.display()))?;
    tmp.persist(path).map_err(|e| format!("{}: {}", path.display(), e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(n: usize) -> String {
        (0..n).map(|i| format!("{{\"id\":{i},\"name\":\"wall {}\"}},", i % 97)).collect()
    }

    #[test]
    fn a_packed_project_unpacks_to_its_json_in_pieces_and_whole_and_any_inflater_reads_it() {
        for n in [0, 1, 1000, 200_000] {
            let json = text(n);
            let packed = pack(&json, 6).unwrap();
            assert!(is_zip(&packed));
            if n > 1000 {
                assert!(packed.len() * 5 < json.len(), "{} of {}", packed.len(), json.len());
            }
            assert_eq!(unpack(packed.clone(), u64::MAX).unwrap(), json, "{n}");
            // One stream, which an inflater that knows nothing of the pieces reads.
            let entries = entries(&packed).unwrap();
            let e = entries.iter().find(|e| e.name == ENTRY).unwrap();
            assert_eq!(inflate(e.data(&packed).unwrap(), u64::MAX).unwrap(), json.as_bytes(), "{n} whole");
        }
    }

    #[test]
    fn a_file_that_isnt_a_zip_is_its_own_json_and_a_damaged_or_hostile_zip_is_an_error_not_a_crash() {
        assert_eq!(unpack(b"{\"format\": 20}".to_vec(), 10).unwrap(), "{\"format\": 20}");
        let json = text(50_000);
        let packed = pack(&json, 1).unwrap();
        // More than the caller allows, whatever it inflates to.
        assert!(unpack(packed.clone(), 1000).unwrap_err().contains("past the 1000"));
        // Every byte flipped in turn somewhere in the first and last kilobytes, and every length cut off: an error or the JSON.
        for i in (0..packed.len()).step_by(97).chain(packed.len().saturating_sub(1024)..packed.len()) {
            let mut bad = packed.clone();
            bad[i] ^= 0x5a;
            if let Ok(t) = unpack(bad, u64::MAX) {
                assert_eq!(t, json, "byte {i}");
            }
        }
        for cut in (0..packed.len()).step_by(4099) {
            let _ = unpack(packed[..cut].to_vec(), u64::MAX);
        }
        assert!(unpack(b"PK\x03\x04".to_vec(), 100).is_err());
    }

    #[test]
    fn a_model_written_on_every_core_is_the_json_serde_writes_of_it() {
        let mut s = crate::engine::Session::new();
        s.script(crate::sample::SAMPLE).unwrap();
        let m = &s.model;
        let mut bare = (**m).clone();
        bare.elements = crate::ids::Elements::new();
        assert_eq!(json(&bare, &m.elements).unwrap(), serde_json::to_string(&**m).unwrap());
        // A key named as the top's, inside a value or a string, isn't the top's.
        assert_eq!(top_level_key(r#"{"a":{"elements":1},"b":"\"elements\":2","elements":{}}"#, "elements"), Some(52));
    }

    #[test]
    fn pieces_that_dont_add_up_are_inflated_whole() {
        let json = text(80_000);
        let packed = pack(&json, 6).unwrap();
        // The index says one piece too few: the entry is inflated in one go, and is still the JSON.
        let entries = entries(&packed).unwrap();
        let e = entries.iter().find(|e| e.name == ENTRY).unwrap();
        let p: Pieces = serde_json::from_slice(entries.iter().find(|e| e.name == PIECES).unwrap().data(&packed).unwrap()).unwrap();
        assert!(p.fits(e.packed, e.size) && p.packed.len() > 1);
        let fewer = Pieces { packed: p.packed[1..].to_vec(), raw: p.raw[1..].to_vec() };
        assert!(!fewer.fits(e.packed, e.size));
    }
}
