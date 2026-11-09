//! The fonts the system has: the folders Linux, Windows and macOS keep them in, looked through once, each file read only as
//! far as what names its faces (the OpenType table directory, and the `name`, `OS/2` and `head` tables: ISO/IEC 14496-22,
//! the public OpenType specification), so a system's hundreds of fonts are listed in moments. A face is read whole only
//! when a note is lettered in it ([`load`]). A font file is hostile: every offset is checked, a table's read is capped,
//! and a file that doesn't read is left out.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// Font files one scan looks at, and how deep into folders.
const MAX_FILES: usize = 20_000;
const MAX_DEPTH: u32 = 6;
/// Faces one collection may name, and tables one face's directory.
const MAX_FACES: u32 = 64;
const MAX_TABLES: u16 = 128;
/// The most of a `name` table that is read.
const MAX_NAME: u32 = 512 * 1024;
/// The largest font file read whole: a CJK collection is about 20 MB.
pub const MAX_FONT: u64 = 100 * 1024 * 1024;
/// Whole files kept read at once.
const KEPT: usize = 24;

/// A face of a font file: its family, which of the file's faces it is, its weight (100 to 900) and whether it is italic,
/// and whether it varies its weight (a variable font's `wght`, which [`load`]ers set for bold).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemFace {
    pub family: String,
    pub path: Arc<Path>,
    pub index: u32,
    pub weight: u16,
    pub italic: bool,
    pub variable: bool,
    /// How wide it is, 1 (ultra-condensed) to 9 (ultra-expanded), 5 normal: OS/2's `usWidthClass`, or its style's name.
    pub width: u16,
}

/// The faces found, by family in lower case.
#[derive(Clone, Debug, Default)]
pub struct Index {
    pub families: BTreeMap<String, Vec<SystemFace>>,
}

impl Index {
    /// The faces of family `name`, in any case.
    pub fn family(&self, name: &str) -> Option<&[SystemFace]> {
        self.families.get(&name.trim().to_lowercase()).map(Vec::as_slice).filter(|f| !f.is_empty())
    }

    /// Every family's name, as its fonts write it.
    pub fn names(&self) -> Vec<String> {
        self.families.values().filter_map(|f| f.first().map(|x| x.family.clone())).collect()
    }
}

/// The folders this system keeps fonts in.
pub fn folders() -> Vec<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut out = Vec::new();
    if cfg!(windows) {
        out.extend(env("WINDIR").or_else(|| env("SystemRoot")).map(|w| w.join("Fonts")));
        out.extend(env("LOCALAPPDATA").map(|l| l.join("Microsoft").join("Windows").join("Fonts")));
    } else if cfg!(target_os = "macos") {
        out.extend(["/System/Library/Fonts", "/Library/Fonts"].map(PathBuf::from));
        out.extend(env("HOME").map(|h| h.join("Library").join("Fonts")));
    } else {
        out.extend(["/usr/share/fonts", "/usr/local/share/fonts"].map(PathBuf::from));
        let data = env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local").join("share")));
        out.extend(data.map(|d| d.join("fonts")));
        out.extend(env("HOME").map(|h| h.join(".fonts")));
    }
    out
}

/// This system's fonts, looked through the first time they are asked for.
pub fn index() -> &'static Index {
    static INDEX: OnceLock<Index> = OnceLock::new();
    INDEX.get_or_init(|| scan(&folders()))
}

/// The faces of the font files in `folders` and the folders in them.
pub fn scan(folders: &[PathBuf]) -> Index {
    let mut files = Vec::new();
    for f in folders {
        walk(f, MAX_DEPTH, &mut files);
    }
    files.sort();
    files.dedup();
    let mut index = Index::default();
    for path in files {
        let path: Arc<Path> = Arc::from(path.as_path());
        for face in faces(&path).unwrap_or_default() {
            index.families.entry(face.family.to_lowercase()).or_default().push(face);
        }
    }
    index
}

fn walk(folder: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        if out.len() >= MAX_FILES {
            return;
        }
        let path = entry.path();
        // Not through links, which may loop.
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            if depth > 0 {
                walk(&path, depth - 1, out);
            }
        } else if path.extension().is_some_and(|e| matches!(e.to_string_lossy().to_lowercase().as_str(), "ttf" | "otf" | "ttc" | "otc")) {
            out.push(path);
        }
    }
}

/// The faces file `path` holds, read from its headers.
fn faces(path: &Arc<Path>) -> Option<Vec<SystemFace>> {
    let mut f = std::fs::File::open(path).ok()?;
    let head = read_at(&mut f, 0, 12)?;
    let starts: Vec<u32> = if head.get(0..4)? == b"ttcf" {
        let n = be32(&head, 8)?.min(MAX_FACES);
        let offsets = read_at(&mut f, 12, n.checked_mul(4)?)?;
        (0..n as usize).filter_map(|i| be32(&offsets, i * 4)).collect()
    } else {
        vec![0]
    };
    let mut out = Vec::new();
    for (index, start) in starts.into_iter().enumerate() {
        if let Some(face) = face(&mut f, start, path, index as u32) {
            out.push(face);
        }
    }
    Some(out)
}

/// The face whose table directory starts at `start`.
fn face(f: &mut std::fs::File, start: u32, path: &Arc<Path>, index: u32) -> Option<SystemFace> {
    let head = read_at(f, start, 12)?;
    let version = be32(&head, 0)?;
    if !matches!(version, 0x0001_0000 | 0x4F54_544F | 0x7472_7565) {
        return None;
    }
    let n = be16(&head, 4)?.min(MAX_TABLES);
    let dir = read_at(f, start.checked_add(12)?, u32::from(n) * 16)?;
    let mut tables: HashMap<[u8; 4], (u32, u32)> = HashMap::new();
    for i in 0..usize::from(n) {
        let r = dir.get(i * 16..i * 16 + 16)?;
        let tag = [r[0], r[1], r[2], r[3]];
        tables.insert(tag, (be32(r, 8)?, be32(r, 12)?));
    }
    let name = tables.get(b"name").and_then(|&(o, l)| read_at(f, o, l.min(MAX_NAME)))?;
    let family = name_of(&name, &[16, 1])?;
    let (mut weight, mut italic, mut width) = (400, false, 5);
    if let Some(os2) = tables.get(b"OS/2").and_then(|&(o, l)| read_at(f, o, l.min(96))) {
        weight = be16(&os2, 4).filter(|w| (1..=1000).contains(w)).unwrap_or(400);
        width = be16(&os2, 6).filter(|w| (1..=9).contains(w)).unwrap_or(5);
        italic = be16(&os2, 62).is_some_and(|s| s & 0x201 != 0);
    } else if let Some(head) = tables.get(b"head").and_then(|&(o, l)| read_at(f, o, l.min(54))) {
        let style = be16(&head, 44).unwrap_or(0);
        (weight, italic) = (if style & 1 != 0 { 700 } else { 400 }, style & 2 != 0);
    }
    let sub = name_of(&name, &[17, 2]).unwrap_or_default().to_lowercase();
    italic |= sub.contains("italic") || sub.contains("oblique");
    if width == 5 && ["narrow", "condensed", "compressed", "cond"].iter().any(|w| sub.split_whitespace().any(|x| x == *w)) {
        width = 3;
    }
    Some(SystemFace { family, path: path.clone(), index, weight, italic, variable: tables.contains_key(b"fvar"), width })
}

/// The first of the names `ids` a `name` table has, in English where it says which: Windows' or Unicode's (UTF-16), else
/// a Macintosh one in plain ASCII.
fn name_of(t: &[u8], ids: &[u16]) -> Option<String> {
    let count = be16(t, 2)?;
    let strings = usize::from(be16(t, 4)?);
    for &want in ids {
        let mut best: Option<(u8, String)> = None;
        for i in 0..usize::from(count) {
            let r = t.get(6 + i * 12..18 + i * 12)?;
            let (platform, encoding, language, id) = (be16(r, 0)?, be16(r, 2)?, be16(r, 4)?, be16(r, 6)?);
            if id != want {
                continue;
            }
            let (len, off) = (usize::from(be16(r, 8)?), usize::from(be16(r, 10)?));
            let Some(bytes) = t.get(strings + off..strings + off + len) else { continue };
            let (rank, s) = match (platform, encoding) {
                (3, 1 | 10) | (0, _) => (if language == 0x409 || platform == 0 { 0 } else { 2 }, utf16(bytes)),
                (1, 0) if bytes.is_ascii() => (1, Some(String::from_utf8_lossy(bytes).into_owned())),
                _ => continue,
            };
            if let Some(s) = s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s.chars().count() <= crate::model::MAX_TEXT)
                && best.as_ref().is_none_or(|(r, _)| rank < *r)
            {
                best = Some((rank, s));
            }
        }
        if let Some((_, s)) = best {
            return Some(s);
        }
    }
    None
}

fn utf16(b: &[u8]) -> Option<String> {
    let units: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
    String::from_utf16(&units).ok().filter(|s| !s.chars().any(char::is_control))
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    b.get(at..at + 2).map(|x| u16::from_be_bytes([x[0], x[1]]))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|x| u32::from_be_bytes([x[0], x[1], x[2], x[3]]))
}

fn read_at(f: &mut std::fs::File, at: u32, len: u32) -> Option<Vec<u8>> {
    f.seek(SeekFrom::Start(u64::from(at))).ok()?;
    let mut buf = Vec::new();
    f.take(u64::from(len)).read_to_end(&mut buf).ok()?;
    (buf.len() == len as usize).then_some(buf)
}

/// File `path` read whole, kept for the next letter: none past [`MAX_FONT`] or that doesn't read.
pub fn load(path: &Path) -> Option<Arc<Vec<u8>>> {
    type Kept = Mutex<HashMap<PathBuf, Option<Arc<Vec<u8>>>>>;
    static FILES: OnceLock<Kept> = OnceLock::new();
    let files = FILES.get_or_init(Default::default);
    if let Some(kept) = files.lock().ok().and_then(|m| m.get(path).cloned()) {
        return kept;
    }
    let read = std::fs::metadata(path).ok().filter(|m| m.len() <= MAX_FONT).and_then(|_| std::fs::read(path).ok()).map(Arc::new);
    if let Ok(mut m) = files.lock() {
        if m.len() >= KEPT {
            m.clear();
        }
        m.insert(path.to_path_buf(), read.clone());
    }
    read
}

/// The fonts that stand in for one a system hasn't, in order: metric-compatible ones first (Liberation and the Chrome OS
/// fonts are drawn to Arial's, Times New Roman's and Courier New's widths), then their look-alikes.
pub fn substitutes(name: &str) -> &'static [&'static str] {
    const SANS: &[&str] = &[
        "Arial",
        "Liberation Sans",
        "Arimo",
        "Helvetica",
        "Helvetica Neue",
        "Nimbus Sans",
        "Nimbus Sans L",
        "DejaVu Sans",
        "Noto Sans",
        "Segoe UI",
    ];
    const SERIF: &[&str] = &["Times New Roman", "Liberation Serif", "Tinos", "Times", "Nimbus Roman", "DejaVu Serif", "Noto Serif"];
    const MONO: &[&str] = &["Courier New", "Liberation Mono", "Cousine", "Courier", "Nimbus Mono PS", "DejaVu Sans Mono", "Noto Sans Mono"];
    const CALIBRI: &[&str] = &["Calibri", "Carlito"];
    const CAMBRIA: &[&str] = &["Cambria", "Caladea"];
    match name.trim().to_lowercase().as_str() {
        "arial" | "helvetica" | "helvetica neue" | "liberation sans" | "arimo" | "nimbus sans" | "sans-serif" | "sans serif" => SANS,
        "times new roman" | "times" | "liberation serif" | "tinos" | "serif" => SERIF,
        "courier new" | "courier" | "liberation mono" | "cousine" | "monospace" => MONO,
        "calibri" | "carlito" => CALIBRI,
        "cambria" | "caladea" => CAMBRIA,
        _ => &[],
    }
}

/// Fonts that letter what a note's own font, Plex Sans and Plex Mono haven't, where the system has them: Chinese,
/// Japanese and Korean, symbols.
pub const FALLBACKS: &[&str] = &[
    "Noto Sans CJK SC",
    "Noto Sans SC",
    "Source Han Sans SC",
    "Microsoft YaHei",
    "PingFang SC",
    "Hiragino Sans GB",
    "WenQuanYi Micro Hei",
    "Malgun Gothic",
    "Yu Gothic",
    "Noto Sans",
    "DejaVu Sans",
    "Segoe UI Symbol",
    "Arial Unicode MS",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_of_fonts_is_listed_by_family_from_the_headers_and_a_broken_file_is_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let deep = dir.path().join("a").join("b");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(dir.path().join("Mono.TTF"), crate::text::PLEX_MONO).unwrap();
        std::fs::write(deep.join("sans.ttf"), crate::text::PLEX_SANS).unwrap();
        // Garbage, a header that promises too much, a collection that points nowhere, a file that isn't a font.
        std::fs::write(dir.path().join("junk.ttf"), b"\x00\x01\x00\x00\xff\xff").unwrap();
        let mut lying = crate::text::PLEX_MONO[..64].to_vec();
        lying[4..6].copy_from_slice(&[0xff, 0xff]);
        std::fs::write(dir.path().join("lying.otf"), lying).unwrap();
        std::fs::write(dir.path().join("c.ttc"), b"ttcf\x00\x01\x00\x00\xff\xff\xff\xff\x7f\xff\xff\xff").unwrap();
        std::fs::write(dir.path().join("readme.txt"), b"not a font").unwrap();
        let index = scan(&[dir.path().to_path_buf(), dir.path().join("missing")]);
        assert_eq!(index.names(), ["IBM Plex Mono", "IBM Plex Sans"]);
        let mono = &index.family("ibm plex MONO").unwrap()[0];
        assert_eq!((mono.weight, mono.italic, mono.variable, mono.index), (400, false, false, 0));
        assert!(index.family("IBM Plex Sans").unwrap()[0].variable, "Plex Sans varies its weight");
        assert!(load(&mono.path).is_some_and(|b| b.len() == crate::text::PLEX_MONO.len()));
        assert!(load(&dir.path().join("missing.ttf")).is_none());
    }

    #[test]
    fn arial_stands_in_for_its_look_alikes_and_they_for_it() {
        assert_eq!(substitutes("ARIAL")[1], "Liberation Sans");
        assert!(substitutes("Helvetica").contains(&"Arial"));
        assert!(substitutes("Courier New").contains(&"Liberation Mono"));
        assert!(substitutes("Comic Sans MS").is_empty());
        assert!(!folders().is_empty());
    }
}
