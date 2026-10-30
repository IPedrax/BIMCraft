//! Fonts of the system's with Chinese and Korean glyphs, for the window to draw hanzi and Hangul with: egui's own have none,
//! and a CJK font is too big to bundle. The places each system keeps its fonts are tried in turn, then the folders of the fonts
//! a Linux desktop has installed are looked through.

use std::path::{Path, PathBuf};

/// The most of a font file that is read: Noto Sans CJK as a collection is about 20 MB.
const MAX_FONT: u64 = 100 * 1024 * 1024;

/// What a font is known to draw.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Script {
    /// The hanzi, as the Chinese interface and Chinese content need.
    Han,
    /// The Hangul, as Korean content needs.
    Hangul,
    /// Both of them.
    Both,
}

impl Script {
    /// Whether this font serves a window looking for `wanted`.
    fn has(self, wanted: Script) -> bool {
        self == Script::Both || self == wanted
    }
}

/// Fonts by where each system keeps them, with the face of the file to use (Noto Sans CJK's collection has Japanese, Korean,
/// Simplified, Traditional and Hong Kong, in that order: Simplified is the third) and what each draws. Windows keeps the hanzi
/// in the Chinese fonts and the Hangul in the Korean ones, which no one file has both of.
const KNOWN: &[(&str, u32, Script)] = &[
    (r"C:\Windows\Fonts\msyh.ttc", 0, Script::Han),
    (r"C:\Windows\Fonts\msyh.ttf", 0, Script::Han),
    (r"C:\Windows\Fonts\simsun.ttc", 0, Script::Han),
    (r"C:\Windows\Fonts\simhei.ttf", 0, Script::Han),
    (r"C:\Windows\Fonts\malgun.ttf", 0, Script::Hangul),
    (r"C:\Windows\Fonts\gulim.ttc", 0, Script::Hangul),
    (r"C:\Windows\Fonts\batang.ttc", 0, Script::Hangul),
    (r"C:\Windows\Fonts\dotum.ttc", 0, Script::Hangul),
    ("/System/Library/Fonts/PingFang.ttc", 0, Script::Han),
    ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0, Script::Han),
    ("/System/Library/Fonts/STHeiti Light.ttc", 0, Script::Han),
    ("/System/Library/Fonts/AppleSDGothicNeo.ttc", 0, Script::Hangul),
    ("/System/Library/Fonts/Supplemental/Apple SD Gothic Neo.ttc", 0, Script::Hangul),
    ("/Library/Fonts/Arial Unicode.ttf", 0, Script::Both),
    ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 2, Script::Both),
    ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 2, Script::Both),
    ("/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc", 2, Script::Both),
    ("/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc", 2, Script::Both),
    ("/usr/share/fonts/OTF/NotoSansCJK-Regular.ttc", 2, Script::Both),
    ("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc", 0, Script::Han),
    ("/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc", 0, Script::Han),
    // The Droid fallback has the ideographs for certain; whether it has the Hangul is not said, so it is not counted for them.
    ("/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf", 0, Script::Han),
];

/// Folders of a Linux desktop's fonts, looked through (a few levels down) when none of the known places has one.
const FOLDERS: &[&str] = &["/usr/share/fonts", "/usr/local/share/fonts", ".local/share/fonts", ".fonts"];

/// The file names that say what a font draws, in the order they are preferred: Noto's CJK collection, Source Han Sans, the
/// WenQuanYi fonts, and the Korean faces a desktop may have (Nanum Gothic, Noto Sans KR).
const NAMES: &[(&str, Script)] = &[
    ("notosanscjk-regular", Script::Both),
    ("sourcehansanssc-regular", Script::Han),
    ("wqy-microhei", Script::Han),
    ("wqy-zenhei", Script::Han),
    ("nanumgothic", Script::Hangul),
    ("notosanskr-regular", Script::Hangul),
    ("applesdgothicneo", Script::Hangul),
];

/// The CJK fonts the system has: the first that draws the hanzi and the first that draws the Hangul, each read once (a font
/// with both is read once), and whether one with the hanzi was among them.
pub struct Fonts {
    /// Each font's bytes, the face to use, and what it draws, in the order to put them behind the window's own.
    pub fonts: Vec<(Vec<u8>, u32, Script)>,
    /// Whether one of the fonts has the hanzi.
    pub hanzi: bool,
}

/// None of the system's fonts: what a window without any CJK font on the machine is told it has.
#[cfg(test)]
pub(crate) fn no_fonts() -> Fonts {
    Fonts { fonts: Vec::new(), hanzi: false }
}

pub(super) fn find() -> Fonts {
    let known = KNOWN.iter().map(|&(path, face, script)| (PathBuf::from(path), face, script));
    // Only the fonts the machine has are candidates: a Windows path that is not here is not the hanzi font either.
    let (picked, hanzi) = pick(known.chain(scan()).filter(|(path, _, _)| path.is_file()));
    let fonts = picked.into_iter().filter_map(|(path, face, script)| read_font(&path).map(|bytes| (bytes, face, script))).collect();
    Fonts { fonts, hanzi }
}

/// The bytes of a font the system has, none where the file is not there or too big to read.
fn read_font(path: &Path) -> Option<Vec<u8>> {
    let size = std::fs::metadata(path).ok()?.len();
    (size <= MAX_FONT).then(|| std::fs::read(path).ok())?
}

/// The first candidates for each script, best first: the walk stops when both are had, and a font wanted for neither is not
/// read.
fn pick<I: Iterator<Item = (PathBuf, u32, Script)>>(candidates: I) -> (Vec<(PathBuf, u32, Script)>, bool) {
    let mut picked: Vec<(PathBuf, u32, Script)> = Vec::new();
    let mut want_hanzi = true;
    let mut want_hangul = true;
    let mut hanzi = false;
    for (path, face, script) in candidates {
        if !(want_hanzi && script.has(Script::Han)) && !(want_hangul && script.has(Script::Hangul)) {
            continue;
        }
        if want_hanzi && script.has(Script::Han) {
            want_hanzi = false;
            hanzi = true;
        }
        if want_hangul && script.has(Script::Hangul) {
            want_hangul = false;
        }
        picked.push((path, face, script));
        if !want_hanzi && !want_hangul {
            break;
        }
    }
    (picked, hanzi)
}

/// The fonts the folders hold whose names say what they draw, preferred ones first.
fn scan() -> Vec<(PathBuf, u32, Script)> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut found: Vec<(usize, PathBuf, Script)> = Vec::new();
    for folder in FOLDERS {
        let folder =
            if folder.starts_with('/') { PathBuf::from(folder) } else { home.iter().map(|h| h.join(folder)).next().unwrap_or_default() };
        look(&folder, 4, &mut found);
    }
    found.sort();
    // A collection of Noto CJK has the Simplified face third; any other file has its one.
    found.into_iter().map(|(_, path, script)| (path.clone(), if is_collection(&path) { 2 } else { 0 }, script)).collect()
}

fn is_collection(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    name.contains("notosanscjk") && name.ends_with(".ttc")
}

fn look(folder: &Path, depth: u32, found: &mut Vec<(usize, PathBuf, Script)>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth > 0 {
                look(&path, depth - 1, found);
            }
            continue;
        }
        let name = path.file_stem().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        let font = path.extension().is_some_and(|e| matches!(e.to_string_lossy().to_lowercase().as_str(), "ttf" | "ttc" | "otf"));
        if let Some((rank, (_, script))) = NAMES.iter().enumerate().find(|(_, (n, _))| name == *n).filter(|_| font) {
            found.push((rank, path, *script));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_are_looked_through_a_few_levels_for_fonts_named_as_cjk_ones_best_first() {
        let root = std::env::temp_dir().join(format!("bimcraft-fonts-{}", std::process::id()));
        let files = [
            "truetype/wqy/wqy-microhei.ttc",
            "opentype/noto/NotoSansCJK-Regular.ttc",
            "opentype/noto/NotoSansCJK-Regular.txt",
            "latin/DejaVuSans.ttf",
            "a/b/c/d/e/f/NotoSansCJK-Regular.ttc",
            "truetype/nanum/NanumGothic.ttf",
        ];
        for file in files {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"font").unwrap();
        }
        let mut found = Vec::new();
        look(&root, 4, &mut found);
        found.sort();
        let names: Vec<String> =
            found.iter().map(|(_, p, _)| p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/")).collect();
        assert_eq!(
            names,
            ["opentype/noto/NotoSansCJK-Regular.ttc", "truetype/wqy/wqy-microhei.ttc", "truetype/nanum/NanumGothic.ttf"],
            "Noto first, WenQuanYi and Nanum after; no text file, no Latin font, nothing too deep"
        );
        assert_eq!(found.iter().map(|(_, _, s)| *s).collect::<Vec<_>>(), [Script::Both, Script::Han, Script::Hangul]);
        assert!(is_collection(&found[0].1) && !is_collection(&found[1].1), "only Noto's collection has its Simplified face third");
        assert!(look_in_nothing());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A folder that is not there is no font, and no panic.
    fn look_in_nothing() -> bool {
        let mut found = Vec::new();
        look(Path::new("/nonexistent-folder-of-fonts"), 4, &mut found);
        found.is_empty()
    }

    #[test]
    fn the_first_hanzi_font_and_the_first_hangul_one_are_picked_and_one_with_both_is_taken_once() {
        let of = |name: &str, script: Script| (PathBuf::from(name), 0, script);
        // The Windows pair: the hanzi font and the Korean one behind it.
        let (picked, hanzi) =
            pick([of("msyh.ttc", Script::Han), of("simsun.ttc", Script::Han), of("malgun.ttf", Script::Hangul)].into_iter());
        assert_eq!(picked, [of("msyh.ttc", Script::Han), of("malgun.ttf", Script::Hangul)], "the second hanzi font is not read");
        assert!(hanzi);
        // A font with both serves both scripts, and is read once.
        let (picked, hanzi) = pick([of("NotoSansCJK-Regular.ttc", Script::Both)].into_iter());
        assert_eq!(picked, [of("NotoSansCJK-Regular.ttc", Script::Both)]);
        assert!(hanzi);
        // A font with the Hangul only says so: the Chinese choice is warned without.
        let (picked, hanzi) = pick([of("malgun.ttf", Script::Hangul)].into_iter());
        assert_eq!(picked, [of("malgun.ttf", Script::Hangul)]);
        assert!(!hanzi);
        // A font with the hanzi first does not stop the Hangul's being looked for.
        let (picked, hanzi) = pick([of("wqy-microhei.ttc", Script::Han), of("NotoSansCJK-Regular.ttc", Script::Both)].into_iter());
        assert_eq!(picked, [of("wqy-microhei.ttc", Script::Han), of("NotoSansCJK-Regular.ttc", Script::Both)]);
        assert!(hanzi);
    }
}
