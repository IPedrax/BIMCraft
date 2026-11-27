//! Shortcuts run: what a typed code (`WA`, `ZF`, `3D`) or a chord (`Ctrl+0`, `F3`) does in this window. The table is the core's
//! (`shortcuts`), the same egui matches; each entry is done through the methods the buttons use (`set_tool`, `open_command`, `zoom_fit`,
//! `open_view`), so a shortcut and its button cannot differ. An entry this window cannot do yet says so in the status bar, and does
//! nothing else.

use bimcraft_core::engine;
use bimcraft_core::shortcuts::Target;

use super::App;
use crate::keys::Step;
use crate::lang::{t, tf};
use crate::tools;
use crate::views::ViewKind;

impl App {
    /// A plain letter or digit typed over the window: true when it belongs to a shortcut (it waits for the next letter, or it ran
    /// one), so the window's own keys leave it. With a dialog open the letters are the dialog's.
    pub fn type_key(&mut self, c: char) -> bool {
        if self.dialogs.any() {
            return false;
        }
        match self.typed.push(c, self.now) {
            Step::Waiting => true,
            Step::Run(target) => {
                self.run_target(target);
                true
            }
            Step::Nothing => false,
        }
    }

    /// Drops the letters waiting for a second (Escape): true when there were some.
    pub fn drop_typed(&mut self) -> bool {
        let some = !self.typed.is_empty();
        self.typed.clear();
        some
    }

    /// Drops the letters that have waited too long: true when it did, so the status bar says so.
    pub fn expire_typed(&mut self) -> bool {
        self.typed.expire(self.now)
    }

    /// Seconds until the letters waiting are dropped.
    pub fn typed_in(&self) -> Option<f64> {
        self.typed.due(self.now)
    }

    /// Does what a shortcut names. True when it did; false when this window cannot yet, which the status bar says.
    pub fn run_target(&mut self, target: Target) -> bool {
        match target {
            Target::Command(id) => self.run_shortcut_command(id),
            Target::Desktop(label) => self.run_desktop(label),
        }
    }

    /// Does what a chord's action id names (`cmd.undo`, `ui.view.3d`): the chords that need a file dialog are the window's, in `ui`.
    pub fn run_action(&mut self, action: &str) -> bool {
        match action {
            "ui.find" => {
                self.toggle_find();
                true
            }
            "ui.settings" => {
                self.toggle_settings();
                true
            }
            "ui.view.zoom_fit" => {
                self.zoom_fit();
                true
            }
            "ui.view.3d" => self.run_desktop("3D View"),
            "ui.view.plan" => {
                if let Some(level) = self.level {
                    self.open_view(ViewKind::Plan(level));
                }
                true
            }
            _ => match action.strip_prefix("cmd.") {
                Some(id) => self.run_shortcut_command(id),
                None => self.unavailable(action),
            },
        }
    }

    /// A command's shortcut: the three that act at once, a tool put in hand, or the command's form.
    fn run_shortcut_command(&mut self, id: &str) -> bool {
        match id {
            "undo" => {
                self.undo();
                true
            }
            "redo" => {
                self.redo();
                true
            }
            "delete" => {
                self.delete_selection();
                true
            }
            _ => {
                if let Some(tool) = tools::id_of(id) {
                    // The shortcut picks the tool: typing it again leaves it in hand, as Find does.
                    if self.tool != Some(tool) {
                        self.set_tool(tool);
                    }
                    return true;
                }
                if self.open_command(id) {
                    return true;
                }
                let label = engine::find(id).map_or(id, |c| c.label);
                self.unavailable(label)
            }
        }
    }

    /// One of the desktop's own actions, by its English label.
    fn run_desktop(&mut self, label: &str) -> bool {
        match label {
            "3D View" => {
                self.open_named("3d");
                true
            }
            "Modify" => {
                // Back to selecting: the tool, the points it had and Zoom In Region are put down.
                (self.region, self.tool, self.landing) = (false, None, None);
                self.picked.clear();
                true
            }
            "Zoom to Fit" => {
                self.zoom_fit();
                true
            }
            "Create Similar" => self.create_similar(),
            "Select All Instances" => self.select_instances(),
            _ => self.unavailable(label),
        }
    }

    /// Create Similar (CS): the tool that makes what is selected, when the palette has one for its category.
    fn create_similar(&mut self) -> bool {
        let category = match self.selection.as_slice() {
            [id] => self.session.model.elements.get(*id).map(|e| e.category()),
            _ => None,
        };
        match category.and_then(tools::id_of) {
            Some(tool) => {
                if self.tool != Some(tool) {
                    self.set_tool(tool);
                }
                true
            }
            None => {
                self.message = Some(t("Create Similar: select one element that has a tool").to_string());
                false
            }
        }
    }

    /// Select All Instances (SA): the elements of the selected one's category and type.
    fn select_instances(&mut self) -> bool {
        let model = std::sync::Arc::clone(&self.session.model);
        let [id] = self.selection.as_slice() else {
            self.message = Some(t("Select All Instances: select one element").to_string());
            return false;
        };
        let Some(like) = model.elements.get(*id) else { return false };
        let category = like.category();
        let kind = |id| bimcraft_core::element_json(&model, id).get("type").and_then(|v| v.as_str()).map(str::to_string);
        let wanted = kind(*id);
        let ids: Vec<_> =
            model.elements.iter().filter(|(_, e)| e.category() == category).map(|(i, _)| *i).filter(|i| kind(*i) == wanted).collect();
        self.select(ids);
        true
    }

    /// This window cannot do `what` yet: the status bar says so.
    fn unavailable(&mut self, what: &str) -> bool {
        self.message = Some(tf("{}: not in this window yet", &[t(what)]));
        false
    }
}

#[cfg(test)]
mod tests;
