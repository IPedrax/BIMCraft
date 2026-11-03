//! What BIMCraft's Tools palette offers, and the layouts a user keeps: the project's scale (Sketch, Studio, Team), which leaves out
//! the tools a project doesn't need yet; the tools the user took out of the palette (its Edit Tools); and presets, each of these with
//! where the palettes are and the layout, under a name. All of it the window's, kept in `settings.json`, never in a project. The same in both profiles: Rev's ribbon leaves
//! out and edits its buttons as BIMCraft's Tools palette does its rows (`ribbon::tool_rules`).

use std::collections::BTreeSet;

use bimcraft_i18n::msg;
use egui::containers::menu::MenuButton;
use egui::{Button, RichText, TextEdit, Ui, vec2};
use serde_json::{Value, json};

use crate::BimApp;
use crate::dock::Docking;
use crate::lang::t;
use crate::style::pal;

/// The most presets kept, and the longest name one has.
pub const MOST: usize = 32;
const NAME: usize = 64;
/// The most tools a palette leaves out, and the longest name one has.
const HIDDEN: usize = 256;
const TOOL: usize = 64;

/// How much of BIMCraft a project needs: what the Tools palette offers grows with it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scale {
    Sketch,
    Studio,
    /// Everything, as the window was before there were scales: the default, so nobody loses a tool they had.
    #[default]
    Team,
}

impl Scale {
    pub const ALL: [Scale; 3] = [Scale::Sketch, Scale::Studio, Scale::Team];

    pub fn name(self) -> &'static str {
        match self {
            Scale::Sketch => msg!("Sketch"),
            Scale::Studio => msg!("Studio"),
            Scale::Team => msg!("Team"),
        }
    }

    /// What the settings file keeps.
    pub fn code(self) -> &'static str {
        match self {
            Scale::Sketch => "sketch",
            Scale::Studio => "studio",
            Scale::Team => "team",
        }
    }

    pub fn from_code(code: &str) -> Option<Scale> {
        Scale::ALL.into_iter().find(|s| s.code() == code)
    }

    /// Whether the Tools palette leaves out the panel or the tool named `name` (its English name) at this scale: a team's (design
    /// options, links, inquiry) below Team, and the documents' (sheets, phasing, view templates) in Sketch too.
    pub fn hides(self, name: &str) -> bool {
        const TEAM: [&str; 4] = ["Design Options", "Links", "Link", "Inquiry"];
        const STUDIO: [&str; 3] = ["Sheet Composition", "Phasing", "View Templates"];
        match self {
            Scale::Team => false,
            Scale::Studio => TEAM.contains(&name),
            Scale::Sketch => TEAM.contains(&name) || STUDIO.contains(&name),
        }
    }
}

/// The tools a settings file leaves out of the palette: names a tool's length, as many as there could be.
pub fn hidden_from_json(v: &Value) -> BTreeSet<String> {
    let names = v.as_array().into_iter().flatten().filter_map(Value::as_str);
    names.filter(|n| !n.is_empty() && n.chars().count() <= TOOL).take(HIDDEN).map(String::from).collect()
}

/// A layout the user keeps by a name.
#[derive(Clone, Debug, PartialEq)]
pub struct Preset {
    pub name: String,
    pub docking: Docking,
    pub hidden: BTreeSet<String>,
    pub scale: Scale,
    pub compact: bool,
}

impl Preset {
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "palettes": self.docking.to_json(),
            "tools_hidden": self.hidden,
            "scale": self.scale.code(),
            "layout": if self.compact { "compact" } else { "floating" },
        })
    }

    fn from_json(v: &Value) -> Option<Preset> {
        let name = v["name"].as_str().map(str::trim).filter(|n| !n.is_empty() && n.chars().count() <= NAME)?;
        Some(Preset {
            name: name.to_string(),
            docking: Docking::from_json(&v["palettes"]),
            hidden: hidden_from_json(&v["tools_hidden"]),
            scale: v["scale"].as_str().and_then(Scale::from_code).unwrap_or_default(),
            compact: v["layout"] == "compact",
        })
    }
}

/// The presets a settings file keeps, as hostile as any file: each needs a name, a name is kept once, `MOST` at most.
pub fn presets_from_json(v: &Value) -> Vec<Preset> {
    let mut out: Vec<Preset> = Vec::new();
    for p in v.as_array().into_iter().flatten().filter_map(Preset::from_json) {
        if out.len() < MOST && !out.iter().any(|q| q.name == p.name) {
            out.push(p);
        }
    }
    out
}

impl BimApp {
    /// Puts the window as `p` keeps it.
    pub(crate) fn apply_preset(&mut self, p: &Preset) {
        (self.docking, self.tools_hidden, self.scale, self.compact) = (p.docking.clone(), p.hidden.clone(), p.scale, p.compact);
    }

    /// Keeps the layout now under `name`, in place of a preset of that name.
    fn save_preset(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > NAME {
            self.preset_note = Some(t("A preset needs a name, of at most 64 letters").to_string());
            return;
        }
        let p = Preset {
            name: name.to_string(),
            docking: self.docking.clone(),
            hidden: self.tools_hidden.clone(),
            scale: self.scale,
            compact: self.compact,
        };
        match self.presets.iter().position(|q| q.name == p.name) {
            Some(i) => self.presets[i] = p,
            None if self.presets.len() < MOST => self.presets.push(p),
            None => {
                self.preset_note = Some(t("That is as many presets as are kept: delete one first").to_string());
                return;
            }
        }
        self.preset_note = None;
    }

    /// The chip on BIMCraft's title bar: the project's scale, and under it the scales and the presets, to switch at any time.
    pub(crate) fn scale_chip(&mut self, ui: &mut Ui) {
        let chip =
            Button::new(RichText::new(t(self.scale.name())).color(pal().app_bar_text)).fill(pal().app_bar_hover).min_size(vec2(64.0, 24.0));
        MenuButton::from_button(chip).ui(ui, |ui| {
            ui.label(RichText::new(t("Project scale")).strong());
            for s in Scale::ALL {
                if ui.radio(self.scale == s, t(s.name())).clicked() {
                    self.scale = s;
                    ui.close();
                }
            }
            if !self.presets.is_empty() {
                ui.separator();
                ui.label(RichText::new(t("Presets")).strong());
                for p in self.presets.clone() {
                    if ui.button(&p.name).clicked() {
                        self.apply_preset(&p);
                        ui.close();
                    }
                }
            }
            ui.separator();
            let edit = if self.editing_tools { msg!("Done Editing Tools") } else { msg!("Edit Tools") };
            if ui.button(t(edit)).clicked() {
                self.editing_tools = !self.editing_tools;
                ui.close();
            }
            if ui.button(t("Manage Presets…")).clicked() {
                self.settings_page = Some(crate::settings::Page::Interface);
                ui.close();
            }
        });
    }

    /// Settings > User Interface's presets: the project's scale, each preset to use or delete, the layout now kept under a name,
    /// and every tool back in the Tools palette.
    pub(crate) fn presets_section(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(t("Project scale:"));
            for s in Scale::ALL {
                ui.radio_value(&mut self.scale, s, t(s.name()));
            }
        });
        let mut gone = None;
        for (i, p) in self.presets.clone().iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(&p.name);
                if ui.button(t("Use")).clicked() {
                    self.apply_preset(p);
                }
                if ui.button(t("Delete")).clicked() {
                    gone = Some(i);
                }
            });
        }
        if let Some(i) = gone {
            self.presets.remove(i);
        }
        ui.horizontal(|ui| {
            let label = ui.label(t("Preset name:"));
            ui.add(TextEdit::singleline(&mut self.preset_name).desired_width(180.0)).labelled_by(label.id);
            if ui
                .button(t("Save Preset"))
                .on_hover_text(t("The palettes, the tools shown, the project scale and the layout, kept under this name"))
                .clicked()
            {
                let name = self.preset_name.clone();
                self.save_preset(&name);
            }
        });
        if let Some(note) = &self.preset_note {
            ui.label(RichText::new(note).color(pal().error));
        }
        if !self.tools_hidden.is_empty() && ui.button(t("Show Every Tool")).clicked() {
            self.tools_hidden.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_read_back_as_written_and_a_hostile_list_reads_as_sense() {
        let p = Preset {
            name: "Documentation".into(),
            docking: Docking::revit(),
            hidden: BTreeSet::from(["Door".to_string()]),
            scale: Scale::Team,
            compact: true,
        };
        assert_eq!(presets_from_json(&json!([p.to_json()])), std::slice::from_ref(&p));
        let long = "x".repeat(NAME + 1);
        let hostile = json!([{"name": ""}, {"name": long}, {"no": "name"}, p.to_json(), p.to_json(), {"name": "B", "scale": "huge", "tools_hidden": [1, "", "Wall"]}]);
        let read = presets_from_json(&hostile);
        assert_eq!(read.len(), 2, "unnamed, too long and twice-named are left out");
        assert_eq!((read[1].scale, read[1].hidden.iter().map(String::as_str).collect::<Vec<_>>()), (Scale::Team, vec!["Wall"]));
        let many = Value::Array((0..100).map(|i| json!({"name": format!("p{i}")})).collect());
        assert_eq!(presets_from_json(&many).len(), MOST);
    }

    #[test]
    fn each_scale_leaves_out_less_than_the_one_before() {
        for name in ["Design Options", "Links", "Link", "Inquiry", "Sheet Composition", "Phasing", "View Templates", "Wall"] {
            let (sketch, studio, team) = (Scale::Sketch.hides(name), Scale::Studio.hides(name), Scale::Team.hides(name));
            assert!(sketch >= studio && studio >= team && !team, "{name}");
        }
        assert!(!Scale::Sketch.hides("Wall"));
    }
}
