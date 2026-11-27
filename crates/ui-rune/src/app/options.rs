//! The Options Bar's slice of `App`: the settings the tool in hand reads (`options::Settings`), the text being typed into its fields,
//! which list is open, and what joins a click's params. The machine (`tooling.rs`) makes the params of a click; `feed_options` adds what
//! the bar says to them before they go to the core, and a field that is no length refuses the click, as in the egui window.

use rune::TextEdit;

use super::App;
use super::properties::Field;
use crate::options::{self, Settings};
use crate::tooling::{Chain, Spec, Step};

pub struct Bar {
    pub settings: Settings,
    /// The typed fields of the tool in hand's bar.
    pub fields: Vec<Field>,
    /// The list open under the bar, by its key.
    pub open: Option<&'static str>,
    /// The tool the fields were made for.
    pub tool: Option<&'static str>,
}

impl Bar {
    pub fn new() -> Bar {
        Bar { settings: Settings::default(), fields: Vec::new(), open: None, tool: None }
    }
}

impl Default for Bar {
    fn default() -> Bar {
        Bar::new()
    }
}

impl App {
    /// What was typed into the fields goes back into the settings, which a click reads.
    fn bar_store(&mut self) {
        for f in &self.bar.fields {
            self.bar.settings.texts.insert(f.key, f.edit.text.clone());
        }
    }

    /// The tool in hand is not the one the bar was made for: its fields are made (from what was typed, or the defaults in the project's
    /// units), and the palette shows the type it places.
    pub(super) fn sync_bar(&mut self) {
        if self.tool == self.bar.tool {
            return;
        }
        self.bar_store();
        self.forget_typed();
        self.bar.tool = self.tool;
        self.bar.open = None;
        let units = self.session.model.units;
        let keys = self.tool.map(options::field_keys).unwrap_or_default();
        self.bar.fields = keys.into_iter().map(|key| Field { key, edit: TextEdit::new(self.bar.settings.text(key, units)) }).collect();
        self.refresh_panel();
    }

    /// Opens the list `key` of the bar, or closes it when it is the one open.
    pub fn bar_open(&mut self, key: &'static str) {
        self.bar.open = if self.bar.open == Some(key) { None } else { Some(key) };
    }

    /// Option `i` of the bar's list `key` was picked. True when it is one the bar has.
    pub fn bar_choose(&mut self, key: &str, i: usize) -> bool {
        self.bar.open = None;
        let Some(level) = self.level else { return false };
        self.bar.settings.choose(&self.session.model, level, key, i)
    }

    /// Space with a door or a window in hand: the next one is placed the other way, with its other hinge (a door) or its other face (a window).
    pub fn flip_opening(&mut self) -> bool {
        if !matches!(self.tool, Some("door" | "window")) {
            return false;
        }
        self.bar.settings.flip = !self.bar.settings.flip;
        true
    }

    /// A check box of the bar was set.
    pub fn bar_check(&mut self, key: &str, on: bool) -> bool {
        self.bar.settings.check(key, on)
    }

    /// The type the Type Selector's list item `i` is becomes what the tool in hand places.
    pub(super) fn pick_for_tool(&mut self, category: &'static str, i: usize) -> bool {
        let Some(pick) = crate::props::apply::picked(&self.palette.panel, i) else { return false };
        self.bar.settings.picks.insert(category, pick);
        self.refresh_panel();
        true
    }

    /// What a click of `spec` makes, with the bar's settings joined to its params: the type, the heights, the lines. A field that is no
    /// length refuses the click, and says which. A chain the bar turned off ends after each segment.
    pub(super) fn feed_options(&mut self, spec: &Spec, step: Step) -> Step {
        let Step::Run { mut commands, keep } = step else { return step };
        self.bar_store();
        if let Some(level) = self.level {
            for (name, params) in commands.iter_mut() {
                // A command of the tool's own takes the settings; another it makes (Pick Faces' `wall_by_face`) takes the type only.
                // Fillet Arc's `fillet` rounds walls that are there: it has no type to give.
                if *name == "fillet" {
                    continue;
                }
                if *name != spec.command {
                    self.bar.settings.feed_type(spec.id, &self.session.model, params);
                    continue;
                }
                let near = 2.0 * f64::from(super::REACH) / self.camera.scale;
                if let Err(why) = self.bar.settings.feed(spec.id, &self.session.model, level, params, near) {
                    return Step::Refuse(why);
                }
            }
        }
        let keep = match keep {
            Chain::Whole | Chain::Last if !self.bar.settings.chains(spec.id) => Chain::No,
            other => other,
        };
        Step::Run { commands, keep }
    }
}

#[cfg(test)]
#[path = "options_tests.rs"]
mod tests;
