//! A font of the system's with Chinese glyphs, for the window to draw hanzi with: egui's own have none, and a CJK font is too big
//! to bundle. The places each system keeps its Chinese fonts are tried in turn, then the folders of the fonts a Linux desktop
//! has installed are looked through.

use std::path::{Path, PathBuf};

/// The most of a font file that is read: Noto Sans CJK as a collection is about 20 MB.
const MAX_FONT: u64 = 100 * 1024 * 1024;

/// Fonts by where each system keeps them, with the face of the file to use (Noto Sans CJK's collection has Japanese, Korean,
/// Simplified, Traditional and Hong Kong, in that order: Simplified is the third).
const KNOWN: &[(&str, u32)] = &[
    (r"C:\Windows\Fonts\msyh.ttc", 0),
    (r"C:\Windows\Fonts\msyh.ttf", 0),
    (r"C:\Windows\Fonts\simsun.ttc", 0),
    (r"C:\Windows\Fonts\simhei.ttf", 0),
    ("/System/Library/Fonts/PingFang.ttc", 0),
    ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0),
    ("/System/Library/Fonts/STHeiti Light.ttc", 0),
    ("/Library/Fonts/Arial Unicode.ttf", 0),
    ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 2),
    ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 2),
    ("/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc", 2),
    ("/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc", 2),
    ("/usr/share/fonts/OTF/NotoSansCJK-Regular.ttc", 2),
    ("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc", 0),
    ("/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc", 0),
    ("/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf", 0),
];

/// Folders of a Linux desktop's fonts, looked through (a few levels down) when none of the known places has one.
const FOLDERS: &[&str] = &["/usr/share/fonts", "/usr/local/share/fonts", ".local/share/fonts", ".fonts"];

/// The file names that are Chinese-capable fonts, in the order they are preferred: Noto's CJK collection, Source Han Sans, the
/// WenQuanYi fonts.
const NAMES: &[&str] = &["notosanscjk-regular", "sourcehansanssc-regular", "wqy-microhei", "wqy-zenhei"];

/// The font bytes and the face to use, from the first font the system has.
pub(super) fn find() -> Option<(Vec<u8>, u32)> {
    let known = KNOWN.iter().map(|&(path, face)| (PathBuf::from(path), face));
    let (path, face) = known.chain(scan()).find(|(path, _)| path.is_file())?;
    let size = std::fs::metadata(&path).ok()?.len();
    (size <= MAX_FONT).then(|| std::fs::read(&path).ok().map(|bytes| (bytes, face)))?
}

/// The fonts the folders hold whose names say they have Chinese, preferred ones first.
fn scan() -> Vec<(PathBuf, u32)> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut found: Vec<(usize, PathBuf)> = Vec::new();
    for folder in FOLDERS {
        let folder =
            if folder.starts_with('/') { PathBuf::from(folder) } else { home.iter().map(|h| h.join(folder)).next().unwrap_or_default() };
        look(&folder, 4, &mut found);
    }
    found.sort();
    // A collection of Noto CJK has the Simplified face third; any other file has its one.
    found.into_iter().map(|(_, path)| (path.clone(), if is_collection(&path) { 2 } else { 0 })).collect()
}

fn is_collection(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    name.contains("notosanscjk") && name.ends_with(".ttc")
}

fn look(folder: &Path, depth: u32, found: &mut Vec<(usize, PathBuf)>) {
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
        if let Some(rank) = NAMES.iter().position(|n| name == *n).filter(|_| font) {
            found.push((rank, path));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_are_looked_through_a_few_levels_for_fonts_named_as_chinese_ones_best_first() {
        let root = std::env::temp_dir().join(format!("bimcraft-fonts-{}", std::process::id()));
        let files = [
            "truetype/wqy/wqy-microhei.ttc",
            "opentype/noto/NotoSansCJK-Regular.ttc",
            "opentype/noto/NotoSansCJK-Regular.txt",
            "latin/DejaVuSans.ttf",
            "a/b/c/d/e/f/NotoSansCJK-Regular.ttc",
        ];
        for file in files {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"font").unwrap();
        }
        let mut found = Vec::new();
        look(&root, 4, &mut found);
        found.sort();
        let names: Vec<String> = found.iter().map(|(_, p)| p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/")).collect();
        assert_eq!(
            names,
            ["opentype/noto/NotoSansCJK-Regular.ttc", "truetype/wqy/wqy-microhei.ttc"],
            "Noto first; no text file, no Latin font, nothing too deep"
        );
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
}
