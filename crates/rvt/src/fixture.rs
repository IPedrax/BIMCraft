//! Synthetic Revit-shaped files for tests. Autodesk's files are never committed: a test builds the container it needs here,
//! out of streams it names, and the bytes are whatever the test says they are.

use std::io::{Cursor, Write};

use flate2::{Compression, Crc, write::DeflateEncoder};

use crate::gzip::{PAGE_PAYLOAD, PAGE_STORED};

/// A gzip member as a standard writer makes it: the 10-byte header, the deflate stream, CRC-32 and the length.
pub fn gzip(data: &[u8]) -> Vec<u8> {
    let mut out = truncated_gzip(data);
    let mut crc = Crc::new();
    crc.update(data);
    out.extend_from_slice(&crc.sum().to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out
}

/// A gzip member whose deflate stream stores its data as it is, so every byte of `data` can be found in the member.
pub fn stored_gzip(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1f, 0x8b, 0x08, 0, 0, 0, 0, 0, 0, 0x0b];
    let mut deflate = DeflateEncoder::new(&mut out, Compression::none());
    let _ = deflate.write_all(data);
    let _ = deflate.finish();
    out
}

/// A gzip member without its trailer, the form Revit's own readers are said to accept.
pub fn truncated_gzip(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1f, 0x8b, 0x08, 0, 0, 0, 0, 0, 0, 0x0b];
    let mut deflate = DeflateEncoder::new(&mut out, Compression::fast());
    let _ = deflate.write_all(data);
    let _ = deflate.finish();
    out
}

/// What a `Global/*` stream holds: eight bytes, then one gzip member.
pub fn global(data: &[u8]) -> Vec<u8> {
    let mut out = vec![1, 0, 0, 0, 0, 0, 0, 0];
    out.extend_from_slice(&gzip(data));
    out
}

/// A partition stream: a 44-byte header, then each chunk as a gzip member, its trailer and a 32-byte header.
pub fn partition(chunks: &[&[u8]]) -> Vec<u8> {
    let mut out = vec![9, 0, 0, 0, 0, 0, 0, 0];
    out.resize(44, 0x5a);
    for (i, chunk) in chunks.iter().enumerate() {
        out.extend_from_slice(&gzip(chunk));
        if i + 1 < chunks.len() {
            out.extend_from_slice(&[0x77; 32]);
        }
    }
    out
}

/// What the container stores for a stream Revit writes in pages: after every `PAGE_PAYLOAD` bytes, a trailer that is not data
/// (`PAGE_STORED - PAGE_PAYLOAD` bytes, filled with a pattern a test can look for).
pub fn paged(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + payload.len() / PAGE_PAYLOAD * (PAGE_STORED - PAGE_PAYLOAD));
    for chunk in payload.chunks(PAGE_PAYLOAD) {
        out.extend_from_slice(chunk);
        if chunk.len() == PAGE_PAYLOAD {
            out.resize(out.len() + PAGE_STORED - PAGE_PAYLOAD, 0xa5);
        }
    }
    out
}

/// A Compound File holding these streams (`"Global/Latest"` makes the storage `Global` too), as bytes.
pub fn container(streams: &[(&str, &[u8])]) -> Vec<u8> {
    build(streams).unwrap_or_default()
}

fn build(streams: &[(&str, &[u8])]) -> std::io::Result<Vec<u8>> {
    let mut file = cfb::CompoundFile::create(Cursor::new(Vec::new()))?;
    for (name, data) in streams {
        let mut path = String::new();
        let parts: Vec<&str> = name.split('/').collect();
        for storage in &parts[..parts.len() - 1] {
            path.push('/');
            path.push_str(storage);
            if !file.exists(&path) {
                file.create_storage(&path)?;
            }
        }
        file.create_stream(format!("/{name}"))?.write_all(data)?;
    }
    file.flush()?;
    Ok(file.into_inner().into_inner())
}

/// What goes into a synthetic `BasicFileInfo`.
pub struct Basic<'a> {
    pub release: u32,
    pub build: &'a str,
    pub guid: &'a str,
    pub username: &'a str,
    pub path: &'a str,
    /// A one-byte field in the binary head puts the text block at an odd offset, as it is in some releases.
    pub odd: bool,
    /// Revit 2016 to 2018 wrote `Revit Build: Autodesk Revit 2016 (Build: ...)` where later releases write `Format:` and `Build:`.
    pub legacy: bool,
}

impl Default for Basic<'_> {
    fn default() -> Self {
        Self {
            release: 2024,
            build: "20230308_1635(x64)",
            guid: "badabcab-cff7-4b43-8935-d6520041e7ca",
            username: "",
            path: "C:\\Users\\someone\\Documents\\house.rvt",
            odd: false,
            legacy: false,
        }
    }
}

fn wide(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// A `BasicFileInfo` as Revit writes it: a binary head with the release and build as counted UTF-16 strings, then lines of
/// `Key: value` in UTF-16 separated by CR LF, one of them (`IsSingleUserCloudModel`) with a narrow value inside the wide text.
pub fn basic_file_info(b: &Basic) -> Vec<u8> {
    let mut out = vec![0x0e, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    for field in [b.release.to_string(), b.build.to_string(), b.path.to_string()] {
        out.extend_from_slice(&(field.encode_utf16().count() as u32).to_le_bytes());
        out.extend_from_slice(&wide(&field));
    }
    if b.odd {
        out.push(1);
    }
    let build_line = if b.legacy {
        format!("Revit Build: Autodesk Revit {} (Build: {})", b.release, b.build.trim_end_matches("(x64)"))
    } else {
        format!("Format: {}", b.release)
    };
    let mut lines: Vec<Vec<u8>> =
        vec![wide("Worksharing: Not enabled"), wide(&format!("Username: {}", b.username)), wide("Central Model Path: "), wide(&build_line)];
    if !b.legacy {
        lines.push(wide(&format!("Build: {}", b.build)));
    }
    lines.push(wide(&format!("Last Save Path: {}", b.path)));
    lines.push(wide("Open Workset Default: 3"));
    lines.push(wide("Locale when saved: ENU"));
    lines.push(wide(&format!("Unique Document GUID: {}", b.guid)));
    lines.push(wide("Unique Document Increments: 22"));
    // A narrow value in the wide text: a NUL, the ASCII, a NUL.
    let mut cloud = wide("IsSingleUserCloudModel: ");
    cloud.extend_from_slice(b"\0False\0");
    lines.push(cloud);
    lines.push(wide("Author: Autodesk Revit"));
    lines.push(wide("ClientAppName: RevitApplication"));
    let last = lines.len() - 1;
    for (i, line) in lines.into_iter().enumerate() {
        out.extend_from_slice(&line);
        // Every line ends in CR LF as UTF-16; the stream's last one in a narrow CR LF.
        out.extend_from_slice(if i == last { &[0x0d, 0x0a] } else { &[0x0d, 0, 0x0a, 0] });
    }
    out
}

/// A ZIP archive with one entry, as Revit's `ProjectInformation` is (the entry's name is a path with a user folder in it).
pub fn zip_entry(body: &[u8], deflate: bool) -> Vec<u8> {
    let name = b"C:\\Users\\someone\\AppData\\Local\\Temp\\Revit0000.project.xml";
    let data = if deflate {
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        let _ = enc.write_all(body);
        enc.finish().unwrap_or_default()
    } else {
        body.to_vec()
    };
    let mut out = b"PK\x03\x04".to_vec();
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&(if deflate { 8u16 } else { 0 }).to_le_bytes());
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(name);
    out.extend_from_slice(&data);
    out.extend_from_slice(b"PK\x05\x06");
    out
}

/// A PNG of `width` by `height` black pixels, with the chunks and CRCs a decoder checks.
pub fn png(width: u32, height: u32) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut crc = Crc::new();
        crc.update(kind);
        crc.update(data);
        out.extend_from_slice(&crc.sum().to_be_bytes());
    }
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut raw = Vec::new();
    for _ in 0..height.min(64) {
        raw.push(0);
        raw.resize(raw.len() + width.min(64) as usize * 3, 0);
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), Compression::default());
    let _ = z.write_all(&raw);
    let idat = z.finish().unwrap_or_default();
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &idat);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// One class of a synthetic `Formats/Latest`: its name, the field names (each a `u32`) and the base class it derives from,
/// defined inline right after it when given.
pub struct Class<'a> {
    pub name: &'a str,
    pub fields: &'a [&'a str],
    pub base: Option<&'a Class<'a>>,
}

/// A page-stripped `Formats/Latest` in the grammar the reader knows: a zero `u16`, then each class (counted name, base
/// reference, version, field count, the fields, no GUIDs). Tags run from 12 in definition order, inline bases included.
pub fn schema(classes: &[Class]) -> Vec<u8> {
    fn class(out: &mut Vec<u8>, c: &Class, next_tag: &mut u16) {
        let tag = *next_tag;
        *next_tag += 1;
        out.extend_from_slice(&(c.name.len() as u16).to_le_bytes());
        out.extend_from_slice(c.name.as_bytes());
        match c.base {
            Some(base) => {
                out.extend_from_slice(&(0x8000 | *next_tag).to_le_bytes());
                out.extend_from_slice(&0u16.to_le_bytes());
                class(out, base, next_tag);
            }
            None => out.extend_from_slice(&0u16.to_le_bytes()),
        }
        let _ = tag;
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&(c.fields.len() as u32).to_le_bytes());
        for field in c.fields {
            out.extend_from_slice(&(field.len() as u32).to_le_bytes());
            out.extend_from_slice(field.as_bytes());
            out.extend_from_slice(&5u32.to_le_bytes());
        }
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    let mut out = Vec::new();
    let mut next_tag = 12;
    for c in classes {
        out.extend_from_slice(&0u16.to_le_bytes());
        class(&mut out, c, &mut next_tag);
    }
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// A field's type in a synthetic schema: its kind and modifier bytes, a fixed array's count, an array's inner type and the
/// class it names, by tag (never inline).
#[derive(Clone, Copy)]
pub struct Ty<'a> {
    pub kind: u8,
    pub modifier: u8,
    pub count: Option<u32>,
    pub inner: Option<&'a Ty<'a>>,
    pub class: Option<u16>,
}

impl Ty<'_> {
    /// A plain kind: `1` and `2` a byte, `4` and `5` a `u32`, `6` and `7` an `f64`, `11` a `u64`.
    pub const fn plain(kind: u8) -> Ty<'static> {
        Ty { kind, modifier: 0, count: None, inner: None, class: None }
    }

    /// So many of a plain kind, side by side.
    pub const fn fixed(kind: u8, count: u32) -> Ty<'static> {
        Ty { kind, modifier: 0x10, count: Some(count), inner: None, class: None }
    }

    /// An object of class `tag` with this modifier: `0` by value, `1` owned (an index and a tag), `3` weak (an index), `0x51`
    /// a counted list of owned ones, `0x50` a counted list by value.
    pub const fn object(modifier: u8, tag: Option<u16>) -> Ty<'static> {
        Ty { kind: 0x0e, modifier, count: None, inner: None, class: tag }
    }
}

/// A class of a synthetic schema written with typed fields: its name, the tag of its base (none: no base) and its fields.
pub struct TypedClass<'a> {
    pub name: &'a str,
    pub base: Option<u16>,
    pub fields: &'a [(&'a str, Ty<'a>)],
}

/// The records of `classes`, each after its zero `u16`, with no closing separator: they take the tags after those of the
/// schema they are appended to, in order.
pub fn typed_classes(classes: &[TypedClass]) -> Vec<u8> {
    fn ty(out: &mut Vec<u8>, t: &Ty) {
        out.extend_from_slice(&(u32::from(t.kind) | (u32::from(t.modifier) << 8)).to_le_bytes());
        if t.modifier & 0xf0 == 0x10 {
            out.extend_from_slice(&t.count.unwrap_or(0).to_le_bytes());
        }
        if t.kind == 0x0d {
            out.extend_from_slice(&[1, 0, 0, 0, 0x20]);
            let inner = t.inner.copied().unwrap_or(Ty::plain(7));
            ty(out, &inner);
            if inner.kind == 0x0e && inner.modifier & 0x0f == 0 {
                out.extend_from_slice(&t.class.unwrap_or(0).to_le_bytes());
            }
        } else if t.kind == 0x0e && t.modifier & 0x0f == 0 {
            out.extend_from_slice(&t.class.unwrap_or(0).to_le_bytes());
        }
    }
    let mut out = Vec::new();
    for c in classes {
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(c.name.len() as u16).to_le_bytes());
        out.extend_from_slice(c.name.as_bytes());
        out.extend_from_slice(&c.base.unwrap_or(0).to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&(c.fields.len() as u32).to_le_bytes());
        for (name, t) in c.fields {
            out.extend_from_slice(&(name.len() as u32).to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            ty(&mut out, t);
        }
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    out
}
