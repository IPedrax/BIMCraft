//! The files File > Open Recent lists: the projects and IFC files opened or saved last, newest first, kept in a file of their own in
//! the user's configuration folder, beside the favorites, not in a project (which travels, and which somebody else opens).

use std::path::{Path, PathBuf};

use egui::containers::menu::SubMenuButton;
use egui::{Button, Context, Ui};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::{BimApp, Pending, Status};

/// How many files are kept.
pub const MAX_RECENT: usize = 10;
/// The file is a few hundred bytes. One over this isn't ours, and is ignored.
const MAX_FILE: u64 = 64 << 10;
/// What the list's file is called.
const FILE: &str = "recent.json";

/// Where the list is kept, given where the favorites are (`favorites::default_path` in the core, which finds the user's own
/// configuration folder): beside them, in a file of their own.
pub fn beside(favorites: &Path) -> PathBuf {
    favorites.with_file_name(FILE)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recent {
    paths: Vec<PathBuf>,
}

impl Recent {
    /// The list in the file at `path`. Whatever is missing, damaged or not what's expected is left out: a settings file never
    /// stops the app.
    pub fn load(path: &Path) -> Recent {
        let text =
            std::fs::metadata(path).ok().filter(|m| m.is_file() && m.len() <= MAX_FILE).and_then(|_| std::fs::read_to_string(path).ok());
        let j: Value = text.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
        let mut recent = Recent::default();
        // Oldest first, so that pushing each leaves the newest on top.
        for p in j.get("files").and_then(Value::as_array).into_iter().flatten().rev() {
            if let Some(p) = p.as_str().filter(|p| !p.is_empty()) {
                recent.push(Path::new(p));
            }
        }
        recent
    }

    /// Written to `path`, whole or not at all: its folder made if need be.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let files: Vec<_> = self.paths.iter().map(|p| p.to_string_lossy()).collect();
        let text = serde_json::to_string_pretty(&json!({ "files": files })).map_err(|e| e.to_string())?;
        if let Some(folder) = path.parent().filter(|f| !f.as_os_str().is_empty()) {
            std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        }
        let part = path.with_extension("json.part");
        std::fs::write(&part, text).map_err(|e| format!("{}: {e}", part.display()))?;
        std::fs::rename(&part, path).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The files, newest first.
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// `path` as the newest, once: where it was is forgotten, and the oldest go once there are [`MAX_RECENT`].
    pub fn push(&mut self, path: &Path) {
        // The same file reached another way (a relative path, a link) is the same entry when the system says so.
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        self.paths.retain(|p| *p != path);
        self.paths.insert(0, path);
        self.paths.truncate(MAX_RECENT);
    }

    pub fn remove(&mut self, path: &Path) {
        self.paths.retain(|p| p != path);
    }

    pub fn clear(&mut self) {
        self.paths.clear();
    }

    /// What the menu shows for each file: its name, with the folder it is in for a name two of them share.
    pub fn labels(&self) -> Vec<String> {
        let name = |p: &Path| p.file_name().map_or_else(|| p.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
        self.paths
            .iter()
            .map(|p| {
                let n = name(p);
                if self.paths.iter().filter(|q| name(q) == n).count() < 2 {
                    return n;
                }
                match p.parent().and_then(Path::file_name) {
                    Some(folder) => format!("{n} ({})", folder.to_string_lossy()),
                    None => n,
                }
            })
            .collect()
    }
}

impl BimApp {
    /// Keep the list of recent files in the file at `path`, starting from what it holds. Until the app is told where, it keeps none:
    /// a window made in a test never reads or writes the user's own.
    pub fn keep_recent_in(&mut self, path: PathBuf) {
        self.recent = Recent::load(&path);
        self.recent_path = Some(path);
    }

    /// `path` is the file just opened or saved: the newest of the list.
    pub fn note_recent(&mut self, path: &Path) {
        if self.recent_path.is_some() {
            self.recent.push(path);
            self.keep_recent();
        }
    }

    /// Write the list down. A failure is said, unless the status already says something.
    fn keep_recent(&mut self) {
        let Some(path) = &self.recent_path else { return };
        if let Err(e) = self.recent.save(path)
            && self.status.text.is_empty()
        {
            self.status = Status { text: tf("Couldn't save the setting: {}", &[&e]), error: true };
        }
    }

    /// File > Open Recent: the files opened or saved last, newest first, and a way to forget them.
    pub(crate) fn recent_menu(&mut self, ui: &mut Ui, ctx: &Context) {
        if self.recent.paths().is_empty() {
            ui.add_enabled(false, Button::new(t("Open Recent")));
            return;
        }
        let files: Vec<(String, PathBuf)> = self.recent.labels().into_iter().zip(self.recent.paths().iter().cloned()).collect();
        SubMenuButton::new(t("Open Recent")).ui(ui, |ui| {
            for (label, path) in files {
                if ui.button(label).on_hover_text(path.to_string_lossy()).clicked() {
                    self.guard(Pending::OpenPath(path), ctx);
                }
            }
            ui.separator();
            if ui.button(t("Clear Recent Files")).clicked() {
                self.recent.clear();
                self.keep_recent();
            }
        });
    }

    /// A file that was asked for and couldn't be opened, and isn't there any more, is forgotten.
    pub(crate) fn forget_recent_if_gone(&mut self, path: &Path) {
        if self.recent_path.is_some() && !path.exists() {
            let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
            self.recent.remove(&path);
            self.keep_recent();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("bimcraft-recent-{}-{name}.json", std::process::id()))
    }

    /// What `push` keeps for a path: on Windows `/a.bimcraft` is `C:\a.bimcraft`.
    fn abs(path: &str) -> PathBuf {
        std::path::absolute(path).unwrap()
    }

    #[test]
    fn the_newest_file_comes_first_once_and_only_so_many_are_kept() {
        let mut r = Recent::default();
        for n in 0..MAX_RECENT + 3 {
            r.push(Path::new(&format!("/projects/p{n}.bimcraft")));
        }
        assert_eq!(r.paths().len(), MAX_RECENT);
        assert_eq!(r.paths()[0], abs("/projects/p12.bimcraft"));
        // Opened again, a file moves to the top instead of appearing twice.
        r.push(Path::new("/projects/p5.bimcraft"));
        assert_eq!(r.paths().len(), MAX_RECENT);
        assert_eq!(r.paths()[0], abs("/projects/p5.bimcraft"));
        assert_eq!(r.paths().iter().filter(|p| **p == abs("/projects/p5.bimcraft")).count(), 1);
        r.remove(&abs("/projects/p5.bimcraft"));
        assert!(!r.paths().contains(&abs("/projects/p5.bimcraft")));
        r.clear();
        assert!(r.paths().is_empty());
    }

    #[test]
    fn a_name_two_files_share_shows_its_folder() {
        let mut r = Recent::default();
        for p in ["/a/house.bimcraft", "/b/house.bimcraft", "/a/barn.bimcraft"] {
            r.push(Path::new(p));
        }
        assert_eq!(r.labels(), ["barn.bimcraft", "house.bimcraft (b)", "house.bimcraft (a)"]);
    }

    #[test]
    fn the_list_is_kept_in_a_file_and_read_back_in_order() {
        let file = scratch("kept");
        let mut r = Recent::default();
        for p in ["/a.bimcraft", "/b.ifc", "/c.bimcraft"] {
            r.push(Path::new(p));
        }
        r.save(&file).unwrap();
        assert_eq!(Recent::load(&file), r);
        let _ = std::fs::remove_file(file);
    }

    #[test]
    fn a_missing_damaged_or_oversized_file_is_an_empty_list_and_odd_entries_are_dropped() {
        let file = scratch("odd");
        let _ = std::fs::remove_file(&file);
        assert_eq!(Recent::load(&file), Recent::default(), "no file");
        std::fs::write(&file, "{not json").unwrap();
        assert_eq!(Recent::load(&file), Recent::default(), "damaged");
        std::fs::write(&file, r#"{"files": ["/a.bimcraft", 7, null, "", "/b.bimcraft"]}"#).unwrap();
        assert_eq!(Recent::load(&file).paths(), [abs("/a.bimcraft"), abs("/b.bimcraft")]);
        let many: Vec<String> = (0..MAX_RECENT * 3).map(|n| format!("/p{n}.bimcraft")).collect();
        std::fs::write(&file, json!({ "files": many }).to_string()).unwrap();
        assert_eq!(Recent::load(&file).paths().len(), MAX_RECENT, "a file that lists more is cut");
        std::fs::write(&file, " ".repeat(MAX_FILE as usize + 1)).unwrap();
        assert_eq!(Recent::load(&file), Recent::default(), "oversized");
        let _ = std::fs::remove_file(file);
    }

    #[test]
    fn it_is_kept_beside_the_favorites() {
        assert_eq!(beside(Path::new("/home/u/.config/bimcraft/favorites.json")), Path::new("/home/u/.config/bimcraft/recent.json"));
    }
}
