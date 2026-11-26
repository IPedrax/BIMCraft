//! Export of the view shown, through the core's `export` command (the export engine: PDF of a sheet or a plan, PNG, SVG and the rest
//! of `export_formats`). The window asks for a file with the system's dialog and runs the command with the path; the form for the
//! command's other parameters (page, scale, colour, print sets) is the generic command form of the dialogs, which this crate does not
//! have yet, so a minimal path: the view shown, as the format asks, to the file.

use std::path::PathBuf;

use serde_json::{Value, json};

use super::App;
use crate::lang::{t, tf};
use crate::views::{self, ViewKind};

impl App {
    /// The view shown as `export`'s `view` names it: an element's id for a plan, a section, a sheet, a table or a named 3D view; the
    /// name for a schedule every project has and the default 3D view. None for a ceiling plan (its id is the level's, which `export`
    /// would take for the floor plan) and for no view.
    pub fn export_target(&self) -> Option<Value> {
        match self.view {
            ViewKind::Plan(id) | ViewKind::Section(id) | ViewKind::Sheet(id) | ViewKind::Table(id) | ViewKind::View3D(Some(id)) => {
                Some(json!(id))
            }
            ViewKind::Schedule(_) | ViewKind::View3D(None) => Some(json!(views::name(&self.session.model, self.view))),
            ViewKind::Ceiling(_) | ViewKind::Nothing => None,
        }
    }

    /// A file name for the view shown as `format`: its name, what a file name cannot hold made `_`, and the braces of `{3D}` left out.
    pub fn export_file_name(&self, format: &str) -> String {
        let name: String = views::name(&self.session.model, self.view)
            .chars()
            .filter(|c| !matches!(c, '{' | '}'))
            .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '_' } else { c })
            .collect();
        let name = name.trim();
        format!("{}.{format}", if name.is_empty() { "View" } else { name })
    }

    /// Writes the view shown to `path` as `format`. A refusal, or a failure the command reports, is said in the status bar and changes
    /// nothing; so is what was written.
    pub fn export_view(&mut self, format: &str, path: PathBuf) -> bool {
        let Some(view) = self.export_target() else {
            self.message = Some(t("This view cannot be exported yet.").to_string());
            return false;
        };
        match self.session.execute("export", &json!({"format": format, "view": view, "path": path})) {
            Ok(reply) => match reply["failed"].as_array().and_then(|f| f.first()) {
                Some(failure) => {
                    self.message = Some(failure["error"].as_str().unwrap_or("the export failed").to_string());
                    false
                }
                None => {
                    self.message = Some(tf("Written to {}", &[&path.display().to_string()]));
                    true
                }
            },
            Err(why) => {
                self.message = Some(why);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests;
