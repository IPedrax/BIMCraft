//! The two lists the Start dialog shows, in the files egui keeps beside `settings.json` (so the two apps share them): the projects
//! opened or saved last, newest first (`recent.json`), and the templates New starts a project from (`templates.json`). Each file is
//! `{"files": [path, …]}`. A file that is missing, too big, damaged or not what is expected is an empty list, and an entry that is not
//! a path is left out: a list never stops the app.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// How many recent files are kept and listed.
pub const MAX_RECENT: usize = 10;
/// How many templates the dialog lists (egui's New lists the first ten; more are kept in the file).
pub const MAX_TEMPLATES: usize = 10;
/// How many entries a file is read for: a list is a few hundred bytes an entry, and one past this is not a person's.
const MAX_KEPT: usize = 200;
/// A file over this is not ours.
const MAX_FILE: u64 = 256 << 10;

/// Where the recent files are kept, given the settings file: beside it.
pub fn recent_file(settings: &Path) -> PathBuf {
    settings.with_file_name("recent.json")
}

/// Where the templates are kept, given the settings file: beside it.
pub fn templates_file(settings: &Path) -> PathBuf {
    settings.with_file_name("templates.json")
}

/// The first `most` paths of the file's list, in the order it keeps them.
pub fn read(path: &Path, most: usize) -> Vec<PathBuf> {
    let Ok(file) = std::fs::File::open(path) else { return Vec::new() };
    let mut text = String::new();
    if file.take(MAX_FILE + 1).read_to_string(&mut text).is_err() || text.len() as u64 > MAX_FILE {
        return Vec::new();
    }
    let json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let files = json.get("files").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    files.iter().filter_map(Value::as_str).filter(|p| !p.is_empty()).take(most.min(MAX_KEPT)).map(PathBuf::from).collect()
}

/// `files` written to `path`, whole or not at all: its folder made if need be.
pub fn write(path: &Path, files: &[PathBuf]) -> Result<(), String> {
    let names: Vec<_> = files.iter().map(|p| p.to_string_lossy()).collect();
    let text = serde_json::to_string_pretty(&json!({ "files": names })).map_err(|e| e.to_string())?;
    if let Some(folder) = path.parent().filter(|f| !f.as_os_str().is_empty()) {
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    }
    let part = path.with_extension("json.part");
    std::fs::write(&part, text).map_err(|e| format!("{}: {e}", part.display()))?;
    std::fs::rename(&part, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// `path` as the lists keep it: the same file reached another way (a relative path) is the same entry.
fn key(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The recent files with `path` just opened or saved put first, once, and at most [`MAX_RECENT`] kept.
pub fn pushed(list: &[PathBuf], path: &Path) -> Vec<PathBuf> {
    let path = key(path);
    let mut out = vec![path.clone()];
    out.extend(list.iter().filter(|p| key(p) != path).cloned());
    out.truncate(MAX_RECENT);
    out
}

/// What a list shows for each path: its file name, with its folder for a name two share.
pub fn labels(paths: &[PathBuf]) -> Vec<String> {
    let name = |p: &PathBuf| p.file_name().map_or_else(|| p.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
    paths
        .iter()
        .map(|p| {
            let shared = paths.iter().filter(|o| name(o) == name(p)).count() > 1;
            match p.parent().filter(|_| shared) {
                Some(folder) => format!("{} ({})", name(p), folder.display()),
                None => name(p),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("rune-recent-{}-{name}", std::process::id()))
    }

    #[test]
    fn a_list_is_written_whole_and_read_back_in_order() {
        let path = scratch("a").join("recent.json");
        let files = vec![PathBuf::from("/b/house.bimc"), PathBuf::from("/a/tower.bimc")];
        write(&path, &files).expect("written");
        assert_eq!(read(&path, 10), files);
        assert_eq!(read(&path, 1), &files[..1], "only the first few are read");
        let _ = std::fs::remove_dir_all(path.parent().expect("a folder"));
    }

    #[test]
    fn what_is_missing_damaged_or_not_a_path_is_an_empty_list_or_left_out() {
        assert!(read(&scratch("missing.json"), 10).is_empty());
        let path = scratch("damaged.json");
        for text in ["", "not json", "[1, 2]", r#"{"files": 3}"#] {
            std::fs::write(&path, text).expect("a file");
            assert!(read(&path, 10).is_empty(), "{text:?}");
        }
        std::fs::write(&path, r#"{"files": ["/a.bimc", 4, null, "", "/b.bimc"]}"#).expect("a file");
        assert_eq!(read(&path, 10), [PathBuf::from("/a.bimc"), PathBuf::from("/b.bimc")]);
        std::fs::write(&path, vec![b' '; (MAX_FILE + 1) as usize]).expect("a big file");
        assert!(read(&path, 10).is_empty(), "a file over the limit is not ours");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_file_opened_again_goes_first_once_and_the_list_is_cut() {
        let list: Vec<PathBuf> = (0..MAX_RECENT).map(|i| PathBuf::from(format!("/p/{i}.bimc"))).collect();
        let again = pushed(&list, Path::new("/p/3.bimc"));
        assert_eq!(again.len(), MAX_RECENT);
        assert_eq!(again[0], key(Path::new("/p/3.bimc")), "kept as the lists keep a path");
        assert_eq!(again.iter().filter(|p| key(p) == again[0]).count(), 1);
        let new = pushed(&list, Path::new("/p/new.bimc"));
        assert_eq!((new.len(), new.last()), (MAX_RECENT, Some(&PathBuf::from("/p/8.bimc"))), "the oldest falls off");
    }

    #[test]
    fn a_name_two_files_share_is_said_with_its_folder() {
        let paths = [PathBuf::from("/a/house.bimc"), PathBuf::from("/b/house.bimc"), PathBuf::from("/a/tower.bimc")];
        assert_eq!(labels(&paths), ["house.bimc (/a)", "house.bimc (/b)", "tower.bimc"]);
    }
}
