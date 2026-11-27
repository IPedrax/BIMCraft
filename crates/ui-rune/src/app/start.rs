//! The Start dialog's state and what its handlers do, a slice of `App` (the dialog is `ui/start.rs`, the lists `recent.rs`): New, Open,
//! the sample house, the recent files and the templates New starts from. What it opens goes through the core's `open` and `new`
//! (`App::execute`), so a refusal is said in the dialog, and a project opened or saved is put first in the recent files, the file egui
//! keeps beside `settings.json`.

use std::path::{Path, PathBuf};

use serde_json::json;

use super::App;
use crate::project::{self, Source};
use crate::recent;

/// The most characters of a refusal the dialog shows.
const MAX_REFUSAL: usize = 2_000;

/// The Start dialog, open: the lists as they were when it opened, and what stopped the last thing it did.
#[derive(Default)]
pub struct StartDialog {
    pub recent: Vec<PathBuf>,
    pub templates: Vec<PathBuf>,
    pub error: Option<String>,
}

impl App {
    /// Opens the Start dialog with the recent files and the templates the settings' folder keeps.
    pub fn open_start(&mut self) {
        let (recent, templates) = match &self.prefs.path {
            Some(settings) => (
                recent::read(&recent::recent_file(settings), recent::MAX_RECENT),
                recent::read(&recent::templates_file(settings), recent::MAX_TEMPLATES),
            ),
            None => (Vec::new(), Vec::new()),
        };
        self.close_menu();
        self.dialogs.clear();
        self.dialogs.start = Some(StartDialog { recent, templates, error: None });
    }

    /// New: an empty project, with nothing to undo.
    pub fn start_new(&mut self) -> bool {
        match self.execute("new", &json!({})) {
            Ok(_) => {
                self.dialogs.clear();
                true
            }
            Err(why) => self.start_refused(why),
        }
    }

    /// The sample house, as the program starts on it.
    pub fn start_sample(&mut self) -> bool {
        let (session, why) = project::load(&Source::Sample);
        if let Some(why) = why {
            return self.start_refused(why);
        }
        self.session = session;
        (self.fitted, self.moved) = (false, false);
        self.history.clear();
        self.picked.clear();
        self.selection.clear();
        self.settle();
        self.message = None;
        self.dialogs.clear();
        true
    }

    /// Opens a recent file or a template (a `.bimt` comes in as a new, unsaved project; the core's `open` clears its file).
    pub fn start_open(&mut self, path: &Path) -> bool {
        match self.execute("open", &json!({"path": path.to_string_lossy()})) {
            Ok(_) => {
                if self.session.path.is_some() {
                    self.note_recent(path);
                }
                self.dialogs.clear();
                true
            }
            Err(why) => self.start_refused(why),
        }
    }

    /// `path` put first in the recent files, in the file beside the settings; a file that cannot be written is said in the status bar.
    pub(super) fn note_recent(&mut self, path: &Path) {
        let Some(settings) = self.prefs.path.as_ref() else { return };
        let file = recent::recent_file(settings);
        let list = recent::pushed(&recent::read(&file, recent::MAX_RECENT), path);
        if let Err(why) = recent::write(&file, &list) {
            self.message = Some(why);
        }
    }

    /// The core's refusal, said in the dialog.
    fn start_refused(&mut self, why: String) -> bool {
        if let Some(d) = self.dialogs.start.as_mut() {
            d.error = Some(crate::forms::cut(&why, MAX_REFUSAL));
        }
        false
    }
}

#[cfg(test)]
mod tests;
