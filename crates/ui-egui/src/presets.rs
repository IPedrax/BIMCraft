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
use crate::dock::{Docking, Palette, Policy};
use crate::lang::t;
use crate::style::{Profile, pal};

/// The most presets kept, and the longest name one has.
pub const MOST: usize = 32;
const NAME: usize = 64;
/// The most tools a palette leaves out, and the longest name one has.
const HIDDEN: usize = 256;
const TOOL: usize = 64;
/// The most panel keys a file may hide, and the longest key one has (`tab:<tab>/<panel>`).
pub const MOST_PANELS: usize = 512;
pub const PANEL_KEY: usize = 160;

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
    names_from_json(v, HIDDEN, TOOL)
}

/// The panels a settings file (or a preset) hides: keys of `tab:<tab>/<panel>`, no longer than [`PANEL_KEY`], at most
/// [`MOST_PANELS`] of them. Anything else is left out.
pub fn panels_from_json(v: &Value) -> BTreeSet<String> {
    names_from_json(v, MOST_PANELS, PANEL_KEY).into_iter().filter(|k| k.starts_with("tab:") && k.contains('/')).collect()
}

fn names_from_json(v: &Value, most: usize, longest: usize) -> BTreeSet<String> {
    let names = v.as_array().into_iter().flatten().filter_map(Value::as_str);
    names.filter(|n| !n.is_empty() && n.chars().count() <= longest).take(most).map(String::from).collect()
}

/// A layout the user keeps by a name: where the palettes are and which are closed, the panels and tools the window shows, its
/// profile, the project's scale and the layout.
#[derive(Clone, Debug, PartialEq)]
pub struct Preset {
    pub name: String,
    pub docking: Docking,
    /// The ribbon panels taken out of the window (`tab:<tab>/<panel>`).
    pub panels_hidden: BTreeSet<String>,
    /// The tools left out of the palette and the ribbon.
    pub hidden: BTreeSet<String>,
    /// The interface the preset was kept in; None for one from before profiles were part of a preset: applying it leaves the
    /// window's profile alone.
    pub profile: Option<Profile>,
    pub scale: Scale,
    pub compact: bool,
}

impl Preset {
    pub fn to_json(&self) -> Value {
        let mut v = json!({
            "name": self.name,
            "palettes": self.docking.to_json(&Palette::known()),
            "panels_hidden": self.panels_hidden,
            "tools_hidden": self.hidden,
            "scale": self.scale.code(),
            "layout": if self.compact { "compact" } else { "floating" },
        });
        if let Some(profile) = self.profile {
            v["profile"] = json!(profile.code());
        }
        v
    }

    fn from_json(v: &Value) -> Option<Preset> {
        let name = v["name"].as_str().map(str::trim).filter(|n| !n.is_empty() && n.chars().count() <= NAME)?;
        Some(Preset {
            name: name.to_string(),
            // A preset keeps exactly the palettes it was saved with: one added since stays closed.
            docking: Docking::from_json(&v["palettes"], Policy::Preset, &Palette::known()),
            panels_hidden: panels_from_json(&v["panels_hidden"]),
            hidden: hidden_from_json(&v["tools_hidden"]),
            profile: v["profile"].as_str().and_then(Profile::from_code),
            scale: v["scale"].as_str().and_then(Scale::from_code).unwrap_or_default(),
            compact: v["layout"] == "compact",
        })
    }

    /// Its line in Settings: the profile it is kept in (the window's while it has none), how many palettes it shows, how many
    /// panels it hides, and its scale.
    pub fn summary(&self, window: Profile) -> String {
        let palettes = Palette::known().into_iter().filter(|p| self.docking.shown(*p)).count();
        let profile = self.profile.unwrap_or(window).name();
        crate::lang::tf(
            "{} · {} palettes · {} panels hidden · {}",
            &[profile, &palettes.to_string(), &self.panels_hidden.len().to_string(), self.scale.name()],
        )
    }

    /// The standard view of the preset's profile: its layout (`Docking::default` for BIMCraft, `Docking::revit` for Rev, and the
    /// window's while the preset has no profile), no hidden panels or tools, Team and the floating layout. The name and the profile
    /// are the preset's own: `window` is only what an old preset without one borrows.
    pub(crate) fn reset_to_standard(&mut self, window: Profile) {
        let profile = self.profile.unwrap_or(window);
        self.docking = match profile {
            Profile::BimCraft => Docking::default(),
            Profile::Rev => Docking::revit(),
        };
        self.panels_hidden.clear();
        self.hidden.clear();
        self.scale = Scale::Team;
        self.compact = false;
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

/// A preset being edited: its saved name and the draft the dialog changes. The window is not touched until the preset is used.
#[derive(Clone)]
pub struct PresetEditor {
    pub name: String,
    pub draft: Preset,
}

impl BimApp {
    /// Edit Tools on or off. Done writes the window's arrangement into the preset the window wears, if one is chosen: Edit Tools is
    /// where a preset's palette layout, panels and tools are arranged, and Done is what keeps them.
    pub(crate) fn edit_tools(&mut self, on: bool) {
        self.editing_tools = on;
        if on {
            return;
        }
        let name = self.active_preset.clone().filter(|name| self.presets.iter().any(|p| p.name == *name));
        match name {
            Some(name) => {
                let _ = self.save_preset(&name);
            }
            None => {
                self.active_preset = None;
                self.status = crate::Status {
                    text: t("No preset is selected: choose one in Project scale to keep the layout").to_string(),
                    error: false,
                };
            }
        }
    }

    /// Puts the window as `p` keeps it: its profile first (the layout is drawn by it), then the palettes, the panels, the tools,
    /// the scale and the layout.
    pub(crate) fn apply_preset(&mut self, p: &Preset) {
        if let Some(profile) = p.profile {
            self.profile = profile;
        }
        self.docking = p.docking.clone();
        self.panels_hidden = p.panels_hidden.clone();
        self.tools_hidden = p.hidden.clone();
        self.scale = p.scale;
        self.compact = p.compact;
        self.active_preset = Some(p.name.clone());
    }

    /// The preset as the window is now, under `name`: what Update from Current Layout and Save Preset keep.
    fn preset_now(&self, name: &str) -> Preset {
        Preset {
            name: name.to_string(),
            docking: self.docking.clone(),
            panels_hidden: self.panels_hidden.clone(),
            hidden: self.tools_hidden.clone(),
            profile: Some(self.profile),
            scale: self.scale,
            compact: self.compact,
        }
    }

    /// Keeps the layout now under `name`, in place of a preset of that name: true when it was kept.
    fn save_preset(&mut self, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > NAME {
            self.preset_note = Some(t("A preset needs a name, of at most 64 letters").to_string());
            return false;
        }
        let p = self.preset_now(name);
        let kept_name = p.name.clone();
        match self.presets.iter().position(|q| q.name == p.name) {
            Some(i) => self.presets[i] = p,
            None if self.presets.len() < MOST => self.presets.push(p),
            None => {
                self.preset_note = Some(t("That is as many presets as are kept: delete one first").to_string());
                return false;
            }
        }
        self.preset_note = None;
        // The preset is the window as it is now, so it is the one the window wears: the title bar's chip says so.
        self.active_preset = Some(kept_name);
        true
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
                    let active = self.active_preset.as_deref() == Some(p.name.as_str());
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        // The preset applied is ticked; the others keep the tick's room.
                        ui.label(if active { RichText::new("✓").color(pal().accent) } else { RichText::new(" ") });
                        if ui.selectable_label(active, &p.name).clicked() {
                            self.apply_preset(&p);
                            ui.close();
                        }
                    });
                }
            }
            ui.separator();
            let edit = if self.editing_tools { msg!("Done Editing Tools") } else { msg!("Edit Tools") };
            if ui.button(t(edit)).clicked() {
                self.edit_tools(!self.editing_tools);
                ui.close();
            }
            if ui.button(t("Manage Presets…")).clicked() {
                self.settings_page = Some(crate::settings::Page::Interface);
                ui.close();
            }
        });
    }

    /// Settings > User Interface's presets: the project's scale, each preset to use, edit or delete, the layout now kept under a
    /// name, and every tool and panel back.
    pub(crate) fn presets_section(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(t("Project scale:"));
            for s in Scale::ALL {
                ui.radio_value(&mut self.scale, s, t(s.name()));
            }
        });
        ui.label(
            RichText::new(t("Each preset keeps its own palettes, panels, tools, scale and profile: save one, then Edit it"))
                .small()
                .color(pal().muted),
        );
        ui.label(
            RichText::new(t("Select a preset, arrange the window in Edit Tools, then Done keeps it in that preset"))
                .small()
                .color(pal().muted),
        );
        let window = self.profile;
        let mut gone = None;
        let mut opening = None;
        for (i, p) in self.presets.clone().iter().enumerate() {
            ui.horizontal(|ui| {
                let active = self.active_preset.as_deref() == Some(p.name.as_str());
                ui.label(if active { RichText::new("●").color(pal().accent) } else { RichText::new("○").color(pal().muted) });
                ui.label(&p.name);
                if ui.button(t("Use")).clicked() {
                    self.apply_preset(p);
                }
                if ui.button(t("Edit")).clicked() {
                    opening = Some(PresetEditor { name: p.name.clone(), draft: p.clone() });
                }
                if ui.button(t("Delete")).clicked() {
                    gone = Some(i);
                }
            });
            ui.label(RichText::new(p.summary(window)).small().color(pal().muted));
        }
        if let Some(editor) = opening {
            self.preset_editor = Some(editor);
        }
        if let Some(i) = gone {
            if self.active_preset.as_deref() == Some(self.presets[i].name.as_str()) {
                self.active_preset = None;
            }
            self.presets.remove(i);
        }
        ui.horizontal(|ui| {
            let label = ui.label(t("Preset name:"));
            ui.add(TextEdit::singleline(&mut self.preset_name).desired_width(180.0)).labelled_by(label.id);
            if ui
                .button(t("Save Preset"))
                .on_hover_text(t(
                    "The palettes, the panels, the tools shown, the profile, the project scale and the layout, kept under this name",
                ))
                .clicked()
            {
                let name = self.preset_name.clone();
                if self.save_preset(&name) {
                    // A name kept would make the next Save replace this preset: the next starts blank, a new one.
                    self.preset_name.clear();
                }
            }
        });
        ui.label(RichText::new(t("A new name makes a new preset; the same name replaces it")).small().color(pal().muted));
        if let Some(note) = &self.preset_note {
            ui.label(RichText::new(note).color(pal().error));
        }
        ui.horizontal(|ui| {
            if !self.tools_hidden.is_empty() && ui.button(t("Show Every Tool")).clicked() {
                self.tools_hidden.clear();
            }
            if !self.panels_hidden.is_empty() && ui.button(t("Show Every Panel")).clicked() {
                self.panels_hidden.clear();
            }
        });
    }

    /// The preset editor: what the preset keeps, for the user to change without touching the window. Cancel (or the corner's
    /// close) leaves the preset as it was; OK keeps the draft, which the next Use puts on the window.
    pub(crate) fn preset_editor_window(&mut self, ctx: &egui::Context) {
        let Some(editor) = self.preset_editor.clone() else { return };
        let mut draft = editor.draft.clone();
        let title = crate::lang::tf("Preset: {}", &[&editor.name]);
        let mut close = false;
        let mut ok = false;
        let mut reset = false;
        let mut update = false;
        let r = crate::popup::Modal::new(egui::Id::new("preset-editor"), title).width(560.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(RichText::new(t("This preset keeps its own palettes, panels and profile")).color(pal().muted));
                ui.add_space(4.0);
                crate::settings::heading(ui, msg!("Profile"));
                ui.horizontal(|ui| {
                    for profile in Profile::ALL {
                        // A preset from before profiles were kept in them has none: neither is picked until the user picks one.
                        if ui.radio(draft.profile == Some(profile), profile.name()).clicked() {
                            draft.profile = Some(profile);
                        }
                    }
                });
                crate::settings::heading(ui, msg!("Project scale"));
                ui.horizontal(|ui| {
                    for scale in Scale::ALL {
                        ui.radio_value(&mut draft.scale, scale, t(scale.name()));
                    }
                });
                crate::settings::heading(ui, msg!("Palettes"));
                for p in Palette::known() {
                    let mut on = draft.docking.shown(p);
                    if ui.checkbox(&mut on, t(p.name())).changed() {
                        draft.docking.show(p, on);
                    }
                }
                crate::settings::heading(ui, msg!("Panels"));
                ui.label(RichText::new(t("Tick the panels this preset shows; Find still has their tools")).small().color(pal().muted));
                self.panel_tree(ui, &mut draft.panels_hidden);
                crate::settings::heading(ui, msg!("Tools"));
                ui.label(crate::lang::tf("{} hidden; edit them in Edit Tools after Use", &[&draft.hidden.len().to_string()]));
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if ui.button(t("Cancel")).clicked() {
                        close = true;
                    }
                    if ui.button(t("OK")).clicked() {
                        ok = true;
                    }
                    if ui
                        .button(t("Reset to Standard View"))
                        .on_hover_text(t("Reset the preset to the standard view, keeping its profile"))
                        .clicked()
                    {
                        reset = true;
                    }
                    if ui.button(t("Update from Current Layout")).clicked() {
                        update = true;
                    }
                });
            });
        });
        if update {
            draft = self.preset_now(&editor.name);
        }
        if reset {
            // The profile is the user's choice in this dialog and stays: the layout is the standard one for it.
            draft.reset_to_standard(self.profile);
        }
        if let Some(mut kept) = self.preset_editor.take() {
            if ok {
                kept.draft = draft;
                if let Some(slot) = self.presets.iter_mut().find(|p| p.name == kept.name) {
                    *slot = kept.draft;
                }
            } else if !close && !r.should_close() {
                kept.draft = draft;
                self.preset_editor = Some(kept);
            }
        }
    }

    /// The Panels section of the preset editor: each tab, and under it its panels as boxes to tick (ticked shows the panel). The
    /// keys are drawn grouped by tab, as the window has met them; a plugin's tab and panels are listed as its manifest names them.
    fn panel_tree(&self, ui: &mut Ui, hidden: &mut BTreeSet<String>) {
        let known = self.known_panel_keys();
        let mut keys: BTreeSet<String> = self.panel_keys.iter().chain(hidden.iter()).chain(known.iter()).cloned().collect();
        keys.retain(|k| k.starts_with("tab:") && k.contains('/'));
        let mut tabs: std::collections::BTreeMap<String, BTreeSet<String>> = std::collections::BTreeMap::new();
        for key in keys {
            let Some((tab, panel)) = key["tab:".len()..].split_once('/') else { continue };
            tabs.entry(tab.to_string()).or_default().insert(panel.to_string());
        }
        if tabs.is_empty() {
            ui.label(RichText::new(t("The window has not drawn a panel yet")).small().color(pal().muted));
            return;
        }
        for (tab, panels) in tabs {
            let label = match tab.strip_prefix("plugin:") {
                Some(name) => name.to_string(),
                None => t(tab.strip_prefix("family:").unwrap_or(&tab)).to_string(),
            };
            ui.label(RichText::new(label).strong());
            ui.indent(("preset-panels", &tab), |ui| {
                for panel in panels {
                    let key = format!("tab:{tab}/{panel}");
                    let mut on = !hidden.contains(&key);
                    if ui.checkbox(&mut on, t(&panel)).changed() {
                        if on {
                            hidden.remove(&key);
                        } else {
                            hidden.insert(key);
                        }
                    }
                }
            });
        }
    }

    /// The body of a tab every panel of which is hidden: says so, and brings them all back with a button.
    pub(crate) fn empty_tab_notice(&mut self, ui: &mut Ui, tab: &str) {
        ui.label(RichText::new(t("Every panel of this tab is hidden.")).color(pal().muted));
        if ui.button(t("Show the panels")).clicked() {
            let prefix = format!("tab:{tab}/");
            self.panels_hidden.retain(|key| !key.starts_with(&prefix));
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
            panels_hidden: BTreeSet::from(["tab:View/Phasing".to_string()]),
            hidden: BTreeSet::from(["Door".to_string()]),
            profile: Some(Profile::Rev),
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
