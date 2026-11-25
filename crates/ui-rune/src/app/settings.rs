//! The settings' state and what their handlers do, a slice of `App` (the dialog is `ui/settings.rs`, the file is `settings.rs`): the
//! language, the theme, the profile, the project's units and the window's size. What is chosen is kept in the user's `settings.json`
//! at once; the window's size once it has held still for a moment, since a drag on its edge is a frame for each pixel.

use std::path::PathBuf;

use bimcraft_core::units::Units;
use serde_json::json;

use super::App;
use crate::lang;
use crate::profile::Profile;
use crate::settings::{self, LANGUAGES, Settings, Theme, Window};

/// How long the window must hold still, seconds, before its size is kept.
const STILL: f64 = 0.5;

/// The select of the Settings dialog whose choices are listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Language,
    Units,
    Theme,
    Profile,
}

/// The Settings dialog, open.
#[derive(Default)]
pub struct SettingsDialog {
    pub open: Option<Choice>,
    /// What stopped the last change: the core's refusal of the units, or the file that could not be written.
    pub error: Option<String>,
}

/// What `settings.json` holds of ours, where it is, and how the window was last seen.
#[derive(Default)]
pub struct Prefs {
    pub kept: Settings,
    pub path: Option<PathBuf>,
    /// The window's size now and since when, until it has held still long enough to be kept.
    seen: Option<([f32; 2], f64)>,
}

impl App {
    /// Keeps the settings in `path`: reads what it holds and puts it to use (the language, and the profile unless the arguments asked
    /// for one), and from now on writes what changes. A missing or damaged file is the defaults.
    pub fn use_settings(&mut self, path: PathBuf, profile_asked: bool) {
        let kept = settings::load(&path);
        if let Some(code) = &kept.language {
            lang::set(code);
        }
        if let (Some(profile), false) = (kept.profile, profile_asked) {
            self.profile = profile;
        }
        self.prefs.kept = kept;
        self.prefs.path = Some(path);
    }

    /// Whether the interface is drawn dark.
    pub fn dark(&self) -> bool {
        self.prefs.kept.theme.dark()
    }

    /// Writes what is kept, if there is a file to write; a failure is said in the Settings dialog, else in the status bar.
    fn keep(&mut self) {
        let Some(path) = self.prefs.path.clone() else { return };
        if let Err(why) = settings::save(&path, &self.prefs.kept) {
            match self.dialogs.settings.as_mut() {
                Some(d) => d.error = Some(why),
                None => self.message = Some(why),
            }
        }
    }

    /// Ctrl+, : opens Settings, or closes it when it is open.
    pub fn toggle_settings(&mut self) {
        if self.dialogs.settings.take().is_some() {
            return;
        }
        self.close_menu();
        self.dialogs.clear();
        self.dialogs.settings = Some(SettingsDialog::default());
    }

    /// Opens the choices of one select of the Settings dialog, or closes them when they are open.
    pub fn toggle_choice(&mut self, which: Choice) {
        if let Some(d) = self.dialogs.settings.as_mut() {
            d.open = if d.open == Some(which) { None } else { Some(which) };
        }
    }

    pub fn close_choice(&mut self) {
        if let Some(d) = self.dialogs.settings.as_mut() {
            d.open = None;
        }
    }

    fn chose(&mut self) {
        if let Some(d) = self.dialogs.settings.as_mut() {
            (d.open, d.error) = (None, None);
        }
    }

    /// The interface speaks this language (a code of `settings::LANGUAGES`) from now on, and the file keeps it. False for any other.
    pub fn set_language(&mut self, code: &str) -> bool {
        let Some((code, _)) = LANGUAGES.iter().find(|(c, _)| *c == code) else { return false };
        lang::set(code);
        self.prefs.kept.language = Some((*code).to_string());
        self.chose();
        self.keep();
        true
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.prefs.kept.theme = theme;
        self.chose();
        self.keep();
    }

    pub fn set_profile(&mut self, profile: Profile) {
        self.profile = profile;
        self.prefs.kept.profile = Some(profile);
        self.chose();
        self.keep();
    }

    /// The project shows lengths in this unit (`Units::name`: mm, cm, m, in, ft, ft-in), through the core's `units` command, which is
    /// the project's own and so goes with it into its file. The core's refusal is said in the dialog.
    pub fn set_units(&mut self, name: &str) -> bool {
        if Units::from_name(name).is_none() {
            return false;
        }
        match self.execute("units", &json!({ "length": name })) {
            Ok(_) => {
                self.chose();
                true
            }
            Err(why) => {
                if let Some(d) = self.dialogs.settings.as_mut() {
                    (d.open, d.error) = (None, Some(why));
                }
                false
            }
        }
    }

    /// The window is `w` x `h` points now (the layout of the window's root says it).
    pub fn window_seen(&mut self, w: f32, h: f32) {
        let size = [w.round(), h.round()];
        if !Window::believed(size) || self.prefs.seen.is_some_and(|(seen, _)| seen == size) {
            return;
        }
        self.prefs.seen = Some((size, self.now));
    }

    /// Seconds until the window's size is due to be kept, when it differs from the one kept.
    pub fn window_due(&self) -> Option<f64> {
        let (size, since) = self.prefs.seen?;
        (self.prefs.path.is_some() && self.prefs.kept.window.size != Some(size)).then(|| (since + STILL - self.now).max(0.0))
    }

    /// Keeps the window's size once it has held still for a moment; true when it did.
    pub fn save_window_if_still(&mut self) -> bool {
        let Some((size, since)) = self.prefs.seen else { return false };
        if self.prefs.kept.window.size == Some(size) || self.now - since < STILL {
            return false;
        }
        self.prefs.kept.window.size = Some(size);
        self.keep();
        true
    }
}

#[cfg(test)]
mod tests;
