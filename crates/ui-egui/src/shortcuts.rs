//! Keyboard shortcuts customization, management, loading from file, and modal dialog.

use std::collections::BTreeMap;
use std::path::Path;

use bimcraft_core::geom::P2;
use bimcraft_core::snap::Kind as Snap;
use egui::{Align, ComboBox, Context, Grid, Id as UiId, Layout, Modal, RichText, ScrollArea, TextEdit, vec2};

use crate::graphics;
use crate::lang::{Lang, t, tf, translated_in};
use crate::pal;
use crate::{BimApp, Once, Tab, Tool, phasing};

/// A shortcuts file is a few lines; one over this isn't one.
const MAX_FILE: u64 = 64 << 10;

/// All available actions that can be bound to shortcuts in BIMCraft.
pub const ACTIONS: &[&str] = &[
    "Modify",
    "3D View",
    "Wall",
    "Door",
    "Window",
    "Component",
    "Floor",
    "Floor: Structural",
    "Roof",
    "Roof by Extrusion",
    "Ceiling",
    "Shaft Opening",
    "Stair",
    "Ramp",
    "Railing",
    "Room",
    "Column",
    "Structural Column",
    "Beam",
    "Isolated Footing",
    "Wall Footing",
    "Grid",
    "Level",
    "Aligned Dimension",
    "Text",
    "Spot Elevation",
    "Tag by Category",
    "Section",
    "Elevation",
    "Measure",
    "Align",
    "Trim/Extend to Corner",
    "Split",
    "Offset",
    "Demolish",
    "Match Properties",
    "Create Similar",
    "Paint",
    "Move",
    "Copy",
    "Rotate",
    "Mirror Pick Axis",
    "Mirror Draw Axis",
    "Zoom to Fit",
    "Thin Lines",
    "Properties",
    "Project Browser",
    "Select All Instances",
    "Keyboard Shortcuts",
    "Visibility/Graphics",
    "Hide Element",
    "Hide Category",
    "Temporary Hide Element",
    "Temporary Hide Category",
    "Temporary Isolate Element",
    "Temporary Isolate Category",
    "Reset Temporary Hide/Isolate",
    "Reveal Hidden Elements",
    "Unhide Elements",
    "Unhide Category",
    "Override Graphics by Element",
    "Purge Unused",
    "Link Model",
    "Duplicate View",
    "Filters",
    "View Templates",
    "Phasing",
    "Design Options",
    "Wireframe",
    "Hidden Line",
    "Shaded",
    "Project Units",
    "Delete",
    "Show Mass",
    "Mass",
    "Mass Floors",
    "Wall by Face",
    "Floor by Face",
    "Roof by Face",
    "Update to Face",
    "Pipe",
    "Conduit",
    "Cable Tray",
    "Wire",
    "Plumbing Fixture",
    "Pipe Fitting",
    "Pipe Accessory",
    "Electrical Equipment",
    "Device",
    "Lighting Fixture",
    "Air Terminal",
    "Earthing Electrode",
    "Inspection Box",
    "Data Outlet",
    "Voice Outlet",
    "Rack",
    "Patch Panel",
    "Snap to Endpoints",
    "Snap to Midpoints",
    "Snap to Intersections",
    "Snap to Centres",
    "Snap to Perpendiculars",
    "Snap to Nearest",
    "Snaps Off",
    "Turn Snap Override Off",
];

/// State of the Keyboard Shortcuts management modal.
#[derive(Clone, Debug, Default)]
pub struct ShortcutsDialog {
    pub search: String,
    pub editing_action: Option<String>,
    pub edit_keys: String,
    pub adding: bool,
    pub new_keys: String,
    pub new_action: String,
    pub error: Option<String>,
    pub message: Option<String>,
}

/// A shortcut key: two letters or digits, which is all the typed buffer holds.
fn is_key(k: &str) -> bool {
    k.len() == 2 && k.chars().all(|c| c.is_ascii_alphanumeric())
}

/// The action a description names: its whole name, in English or in any of the interface's languages, in any case.
pub fn resolve_action(desc: &str) -> Option<&'static str> {
    let d = desc.trim().to_lowercase();
    ACTIONS
        .iter()
        .copied()
        .find(|act| act.to_lowercase() == d || Lang::ALL.iter().any(|l| translated_in(*l, act).is_some_and(|w| w.to_lowercase() == d)))
}

/// The (key, description) pairs a shortcuts file holds, as written: a JSON object of key to description, Revit's
/// `<Shortcut CommandName=".." Shortcuts="WA#W1"/>` lines, or lines like `WA - Wall`, `WA = Wall`, `WA: Wall`.
/// [`BimApp::load_shortcuts_from_file`] checks them.
pub fn parse_shortcuts_file(content: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    if let Ok(serde_json::Value::Object(obj)) = serde_json::from_str(content) {
        for (key, val) in obj {
            if let Some(desc) = val.as_str() {
                entries.push((key.to_ascii_uppercase(), desc.to_string()));
            }
        }
        return entries;
    }
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.contains("<Shortcut") {
            let attr = |name: &str| {
                let pattern = format!("{name}=\"");
                line.split(pattern.as_str()).nth(1).and_then(|s| s.split('"').next())
            };
            if let (Some(cmd), Some(keys)) = (attr("CommandName").or_else(|| attr("CommandId")), attr("Shortcuts")) {
                entries.extend(keys.split('#').map(|k| (k.trim().to_ascii_uppercase(), cmd.to_string())));
            }
            continue;
        }
        let sep = [':', '=', '-', '\u{2013}'].into_iter().find_map(|c| line.find(c).map(|idx| (idx, c.len_utf8())));
        if let Some((pos, char_len)) = sep {
            let key = line[..pos].trim().trim_matches('*').trim().to_ascii_uppercase();
            entries.push((key, line[pos + char_len..].trim().to_string()));
        }
    }
    entries
}

/// Checks whether `key` is already assigned to a different action in default or custom shortcuts.
pub fn find_conflict(key: &str, for_action: &str, app: &BimApp) -> Option<String> {
    let key = key.trim().to_ascii_uppercase();
    if let Some(act) = app.custom_shortcuts.get(&key)
        && act != for_action
    {
        return Some(act.clone());
    }
    for (d_key, d_act) in crate::shortcuts() {
        if d_key == key && d_act != for_action && !app.custom_shortcuts.contains_key(d_key) {
            return Some(d_act.to_string());
        }
    }
    None
}

impl BimApp {
    /// Adds the shortcuts of a file's `parsed` pairs to the custom ones: how many were added, and how many skipped
    /// (not a two-character key, not an action BIMCraft has, or a key another action already has).
    fn add_shortcuts(&mut self, parsed: Vec<(String, String)>) -> (usize, usize) {
        let (mut added, mut skipped) = (0, 0);
        for (key, desc) in parsed {
            match resolve_action(&desc) {
                Some(action) if is_key(&key) && find_conflict(&key, action, self).is_none() => {
                    self.custom_shortcuts.insert(key, action.to_string());
                    added += 1;
                }
                _ => skipped += 1,
            }
        }
        (added, skipped)
    }

    /// Loads keyboard shortcuts from a file on disk (markdown, txt, json, xml).
    pub(crate) fn load_shortcuts_from_file(&mut self, path: &Path) {
        let read = match bimcraft_core::disk::len(path) {
            Ok(len) if len <= MAX_FILE => {
                bimcraft_core::disk::read_to_string(path).map_err(|e| tf("Failed to read file: {}", &[&e.to_string()]))
            }
            Ok(_) => Err(t("That is too big to be a shortcuts file.").to_string()),
            Err(e) => Err(tf("Failed to read file: {}", &[&e.to_string()])),
        };
        let (error, message) = match read {
            Err(e) => (Some(e), None),
            Ok(text) => {
                let (added, skipped) = self.add_shortcuts(parse_shortcuts_file(&text));
                let loaded = tf("Loaded {} shortcuts from file.", &[&added.to_string()]);
                let note = if skipped == 0 { loaded } else { tf("{} Skipped {}.", &[&loaded, &skipped.to_string()]) };
                (None, Some(note))
            }
        };
        self.shortcuts_dialog.error = error;
        self.shortcuts_dialog.message = message;
    }

    /// Project Browser (NP): shown, or hidden.
    pub(crate) fn toggle_browser(&mut self) {
        let p = crate::dock::Palette::Browser;
        self.show_palette(p, !self.docking.shown(p));
    }

    /// Executes an action triggered by a shortcut key sequence.
    pub(crate) fn execute_shortcut_action(&mut self, action: &str) -> bool {
        match action {
            "Modify" => self.set_tool(Tool::Modify),
            "3D View" => self.open_sheet(crate::Sheet::View3D),
            "Wall" => self.set_tool(Tool::Wall(Vec::new())),
            "Door" => self.set_tool(Tool::Door),
            "Window" => self.set_tool(Tool::Window),
            "Component" => self.set_tool(Tool::Component),
            "Room" => self.set_tool(Tool::Room),
            "Level" => self.set_tool(Tool::Level),
            "Grid" => self.set_tool(Tool::Grid(None)),
            "Column" => self.set_tool(Tool::Column),
            "Structural Column" => self.set_tool(Tool::Stand("structuralcolumn")),
            "Floor" => self.set_tool(Tool::Floor),
            "Floor: Structural" => self.structural_floor(),
            "Roof" => self.set_tool(Tool::Roof),
            "Roof by Extrusion" => self.set_tool(Tool::ExtrusionRoof(Vec::new())),
            "Ceiling" => self.set_tool(Tool::Ceiling),
            "Shaft Opening" => self.set_tool(Tool::Shaft(None)),
            "Stair" => self.set_tool(Tool::Stair(Vec::new())),
            "Ramp" => self.set_tool(Tool::Ramp(Vec::new())),
            "Railing" => self.set_tool(Tool::Railing(Vec::new())),
            "Mass" => self.set_tool(Tool::Mass(Vec::new())),
            "Aligned Dimension" => self.set_tool(Tool::Dimension(Vec::new())),
            "Text" => self.set_tool(Tool::Text),
            "Spot Elevation" => self.set_tool(Tool::Spot),
            "Tag by Category" => self.set_tool(Tool::Tag),
            "Section" => self.set_tool(Tool::Section(None)),
            "Elevation" => self.set_tool(Tool::Elevation),
            "Measure" => self.set_tool(Tool::Measure(None)),
            "Align" => self.set_tool(Tool::Align(None)),
            "Trim/Extend to Corner" => self.set_tool(Tool::Trim(None)),
            "Split" => self.set_tool(Tool::Split),
            "Offset" => self.set_tool(Tool::Offset),
            "Demolish" => self.set_tool(Tool::Demolish),
            "Paint" => self.set_tool(Tool::Paint),
            "Move" => self.set_tool(Tool::Move(None)),
            "Copy" => self.set_tool(Tool::Copy(None)),
            "Rotate" => self.set_tool(Tool::Rotate { centre: P2(0.0, 0.0), start: None, place: false }),
            "Mirror Pick Axis" => self.set_tool(Tool::Mirror { draw: false, start: None }),
            "Mirror Draw Axis" => self.set_tool(Tool::Mirror { draw: true, start: None }),
            "Zoom to Fit" => self.fit(),
            "Thin Lines" => self.thin = !self.thin,
            "Create Similar" => self.create_similar(),
            "Properties" => self.toggle_properties(),
            "Project Browser" => self.toggle_browser(),
            "Select All Instances" => self.select_all_instances(),
            "Keyboard Shortcuts" => self.show_shortcuts = true,
            "Visibility/Graphics" => self.open_graphics(0),
            "Hide Element" => self.hide_selection(false),
            "Hide Category" => self.hide_selection(true),
            "Temporary Hide Element" => self.temp_hide(false),
            "Temporary Hide Category" => self.temp_hide(true),
            "Temporary Isolate Element" => self.temp_isolate(false),
            "Temporary Isolate Category" => self.temp_isolate(true),
            "Reset Temporary Hide/Isolate" => self.temp_reset(),
            "Reveal Hidden Elements" => self.temp_reveal(),
            "Unhide Elements" => {
                if self.hidden_selection().0.is_empty() {
                    self.open_graphics(3);
                } else {
                    self.unhide_selection(false);
                }
            }
            "Unhide Category" => self.unhide_selection(true),
            "Override Graphics by Element" => self.override_selection(),
            "Purge Unused" => self.open_purge(),
            "Link Model" => self.link_dialog = true,
            "Duplicate View" => self.duplicate_plan("duplicate"),
            "Filters" => self.filters_dialog = Some(graphics::FiltersDialog::default()),
            "View Templates" => self.templates_dialog = Some(graphics::TemplatesDialog::default()),
            "Phasing" => self.phasing_dialog = Some(phasing::PhasingDialog::default()),
            "Design Options" => self.options_dialog = Some(phasing::OptionsDialog::default()),
            "Wireframe" => self.set_visual_style(bimcraft_core::graphics::VisualStyle::Wireframe),
            "Hidden Line" => self.set_visual_style(bimcraft_core::graphics::VisualStyle::HiddenLine),
            "Shaded" => self.set_visual_style(bimcraft_core::graphics::VisualStyle::Shaded),
            "Project Units" => {
                self.tab = Tab::Manage;
                self.open_units = true;
            }
            "Delete" => self.delete(),
            // Revit's snap overrides, for the next point picked.
            "Snap to Endpoints" => self.snap_once = Some(Once::Only(Snap::Endpoint)),
            "Snap to Midpoints" => self.snap_once = Some(Once::Only(Snap::Midpoint)),
            "Snap to Intersections" => self.snap_once = Some(Once::Only(Snap::Intersection)),
            "Snap to Centres" => self.snap_once = Some(Once::Only(Snap::Centre)),
            "Snap to Perpendiculars" => self.snap_once = Some(Once::Only(Snap::Perpendicular)),
            "Snap to Nearest" => self.snap_once = Some(Once::Only(Snap::Nearest)),
            "Snaps Off" => self.snap_once = Some(Once::Off),
            "Turn Snap Override Off" => self.snap_once = None,
            "Show Mass" => self.show_mass(),
            "Mass Floors" => self.by_mass("mass_floors"),
            "Wall by Face" => self.by_mass("wall_by_face"),
            "Floor by Face" => self.by_mass("floor_by_face"),
            "Roof by Face" => self.by_mass("roof_by_face"),
            "Update to Face" => self.by_mass("update_by_face"),
            "Match Properties" => self.match_properties(),
            "Beam" => self.set_tool(Tool::Run("beam", None)),
            "Pipe" => self.set_tool(Tool::Run("pipe", None)),
            "Conduit" => self.set_tool(Tool::Run("conduit", None)),
            "Cable Tray" => self.set_tool(Tool::Run("cabletray", None)),
            "Wire" => self.set_tool(Tool::Run("wire", None)),
            "Plumbing Fixture" => self.set_tool(Tool::Fixture("plumbing")),
            "Pipe Fitting" => self.set_tool(Tool::Fixture("fitting")),
            "Pipe Accessory" => self.set_tool(Tool::Fixture("accessory")),
            "Electrical Equipment" => self.set_tool(Tool::Fixture("equipment")),
            "Device" => self.set_tool(Tool::Fixture("device")),
            "Lighting Fixture" => self.set_tool(Tool::Fixture("lighting")),
            "Air Terminal" => self.set_tool(Tool::Fixture("airterminal")),
            "Earthing Electrode" => self.set_tool(Tool::Fixture("earthing")),
            "Inspection Box" => self.set_tool(Tool::Fixture("inspection")),
            "Data Outlet" => self.set_tool(Tool::Fixture("dataoutlet")),
            "Voice Outlet" => self.set_tool(Tool::Fixture("voiceoutlet")),
            "Rack" => self.set_tool(Tool::Fixture("rack")),
            "Patch Panel" => self.set_tool(Tool::Fixture("patchpanel")),
            "Isolated Footing" => self.set_tool(Tool::Stand("footing")),
            "Wall Footing" => self.set_tool(Tool::Run("stripfooting", None)),
            _ => return false,
        }
        true
    }

    /// Renders the Keyboard Shortcuts modal dialog with options to load from file, edit, or add shortcuts.
    pub(crate) fn shortcuts_window(&mut self, ctx: &Context) {
        if !self.show_shortcuts {
            return;
        }

        let mut close = false;
        let mut load_asked = false;

        let r = Modal::new(UiId::new("shortcuts")).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(t("Keyboard Shortcuts")).heading());
            });
            ui.add_space(4.0);

            // Controls toolbar: Search, Load File, Add Shortcut, Restore Defaults
            ui.horizontal(|ui| {
                ui.add(TextEdit::singleline(&mut self.shortcuts_dialog.search).hint_text(t("Search…")).desired_width(180.0));

                if ui.button(t("Load File...")).clicked() {
                    load_asked = true;
                }

                let add_label = if self.shortcuts_dialog.adding { t("Cancel") } else { t("Add Shortcut") };
                if ui.button(add_label).clicked() {
                    self.shortcuts_dialog.adding = !self.shortcuts_dialog.adding;
                    self.shortcuts_dialog.error = None;
                    self.shortcuts_dialog.message = None;
                    if self.shortcuts_dialog.new_action.is_empty() {
                        self.shortcuts_dialog.new_action = ACTIONS.first().copied().unwrap_or("Wall").to_string();
                    }
                }

                if !self.custom_shortcuts.is_empty() && ui.button(t("Restore Defaults")).clicked() {
                    self.custom_shortcuts.clear();
                    self.shortcuts_dialog.error = None;
                    self.shortcuts_dialog.message = Some(t("Restore Defaults").to_string());
                }
            });

            // Form to add a new keyboard shortcut (with duplicate checking)
            if self.shortcuts_dialog.adding {
                ui.add_space(6.0);
                ui.group(|ui| {
                    ui.label(RichText::new(t("Add Shortcut")).strong());
                    ui.horizontal(|ui| {
                        ui.label(t("Shortcut Keys:"));
                        let r = ui.add(TextEdit::singleline(&mut self.shortcuts_dialog.new_keys).desired_width(60.0));
                        if r.changed() {
                            self.shortcuts_dialog.new_keys = self.shortcuts_dialog.new_keys.to_ascii_uppercase();
                        }

                        ui.label(t("Command / Tool:"));
                        let current_action = self.shortcuts_dialog.new_action.clone();
                        ComboBox::from_id_salt("new_shortcut_action").selected_text(t(&current_action)).show_ui(ui, |ui| {
                            for &act in ACTIONS {
                                ui.selectable_value(&mut self.shortcuts_dialog.new_action, act.to_string(), t(act));
                            }
                        });

                        if ui.button(t("Add")).clicked() {
                            let key = self.shortcuts_dialog.new_keys.trim().to_ascii_uppercase();
                            let action = self.shortcuts_dialog.new_action.trim().to_string();

                            if !is_key(&key) {
                                self.shortcuts_dialog.error = Some(t("Shortcut keys are two characters, letters or numbers.").to_string());
                                self.shortcuts_dialog.message = None;
                            } else if action.is_empty() {
                                self.shortcuts_dialog.error = Some(t("Select an action to assign.").to_string());
                                self.shortcuts_dialog.message = None;
                            } else if let Some(conflict) = find_conflict(&key, &action, self) {
                                // Verification: already exists!
                                self.shortcuts_dialog.error = Some(tf("The shortcut '{}' already exists for '{}'.", &[&key, t(&conflict)]));
                                self.shortcuts_dialog.message = None;
                            } else {
                                self.custom_shortcuts.insert(key.clone(), action.clone());
                                self.shortcuts_dialog.message = Some(tf("Shortcut '{}' added for '{}'.", &[&key, t(&action)]));
                                self.shortcuts_dialog.error = None;
                                self.shortcuts_dialog.new_keys.clear();
                                self.shortcuts_dialog.adding = false;
                            }
                        }
                    });
                });
            }

            // Notification message / error banner
            if let Some(err) = &self.shortcuts_dialog.error {
                ui.add_space(4.0);
                ui.label(RichText::new(err).color(pal().error));
            } else if let Some(msg) = &self.shortcuts_dialog.message {
                ui.add_space(4.0);
                ui.label(RichText::new(msg).color(pal().accent_ink));
            }

            ui.add_space(6.0);

            // Build map of action -> list of shortcut keys
            let mut by_action: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (letters, name) in crate::shortcuts() {
                if !self.custom_shortcuts.contains_key(letters) {
                    by_action.entry(name.to_string()).or_default().push(letters.to_string());
                }
            }
            for (key, act) in &self.custom_shortcuts {
                by_action.entry(act.clone()).or_default().push(key.clone());
            }

            let search_term = self.shortcuts_dialog.search.trim().to_lowercase();
            let mut rows: Vec<(String, Vec<String>)> = by_action
                .into_iter()
                .filter(|(act, keys)| {
                    if search_term.is_empty() {
                        return true;
                    }
                    act.to_lowercase().contains(&search_term)
                        || t(act).to_lowercase().contains(&search_term)
                        || keys.iter().any(|k| k.to_lowercase().contains(&search_term))
                })
                .collect();
            rows.sort_by(|a, b| t(&a.0).cmp(t(&b.0)));

            let mut save_edit = None;
            let mut cancel_edit = false;

            ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                Grid::new("shortcuts_table").num_columns(3).striped(true).spacing(vec2(16.0, 6.0)).show(ui, |ui| {
                    for (action, keys) in rows {
                        let is_editing = self.shortcuts_dialog.editing_action.as_deref() == Some(&action);

                        // Column 1: Action label
                        ui.label(RichText::new(t(&action)).strong());

                        // Column 2: Keys display or editor
                        if is_editing {
                            let r = ui.add(TextEdit::singleline(&mut self.shortcuts_dialog.edit_keys).desired_width(100.0));
                            if r.changed() {
                                self.shortcuts_dialog.edit_keys = self.shortcuts_dialog.edit_keys.to_ascii_uppercase();
                            }
                        } else {
                            ui.monospace(keys.join(", "));
                        }

                        // Column 3: Edit / Save / Cancel button
                        ui.horizontal(|ui| {
                            if is_editing {
                                if ui.button(t("Save")).clicked() {
                                    save_edit = Some(action.clone());
                                }
                                if ui.button(t("Cancel")).clicked() {
                                    cancel_edit = true;
                                }
                            } else if ui.button(t("Edit")).clicked() {
                                self.shortcuts_dialog.editing_action = Some(action.clone());
                                self.shortcuts_dialog.edit_keys = keys.first().cloned().unwrap_or_default();
                                self.shortcuts_dialog.error = None;
                                self.shortcuts_dialog.message = None;
                            }
                        });

                        ui.end_row();
                    }
                });
            });

            if cancel_edit {
                self.shortcuts_dialog.editing_action = None;
                self.shortcuts_dialog.error = None;
            }

            if let Some(action) = save_edit {
                let new_key = self.shortcuts_dialog.edit_keys.trim().to_ascii_uppercase();
                if !is_key(&new_key) {
                    self.shortcuts_dialog.error = Some(t("Shortcut keys are two characters, letters or numbers.").to_string());
                } else if let Some(conflict) = find_conflict(&new_key, &action, self) {
                    self.shortcuts_dialog.error = Some(tf("The shortcut '{}' already exists for '{}'.", &[&new_key, t(&conflict)]));
                } else {
                    // Remove any previous keys assigned specifically to this action in custom
                    self.custom_shortcuts.retain(|_, v| *v != action);
                    self.custom_shortcuts.insert(new_key.clone(), action.clone());
                    self.shortcuts_dialog.message = Some(tf("Shortcut '{}' updated.", &[&new_key]));
                    self.shortcuts_dialog.error = None;
                    self.shortcuts_dialog.editing_action = None;
                }
            }

            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = ui.button(t("Close")).clicked());
        });

        if load_asked {
            let dialog =
                rfd::AsyncFileDialog::new().add_filter("Shortcut files (*.md, *.txt, *.json, *.xml)", &["md", "txt", "json", "xml"]);
            self.ask_file(ctx, dialog, false, |app, path| {
                app.load_shortcuts_from_file(&path);
            });
        }

        if close || r.should_close() {
            self.show_shortcuts = false;
            self.shortcuts_dialog.editing_action = None;
            self.shortcuts_dialog.adding = false;
            self.shortcuts_dialog.error = None;
            self.shortcuts_dialog.message = None;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use bimcraft_core::engine::Session;

    #[test]
    fn a_description_is_an_action_by_its_whole_name_in_any_language() {
        for (desc, action) in [
            ("Wall", "Wall"),
            ("  floor ", "Floor"),
            ("Parede", "Wall"),
            ("Filters", "Filters"),
            ("Floor: Structural", "Floor: Structural"),
        ] {
            assert_eq!(resolve_action(desc), Some(action), "{desc}");
        }
        // What a substring match took for a tool: a word inside a longer one, or a sentence that mentions one.
        for desc in ["ou", "Patch", "Filter", "the wall of the house", "Walls and doors", "", "x"] {
            assert_eq!(resolve_action(desc), None, "{desc}");
        }
    }

    #[test]
    fn a_file_binds_only_two_character_keys_to_actions_it_names_and_that_are_free() {
        let mut app = BimApp::new(Session::new());
        let text = "ZW - Wall\n**ZZ** \u{2013} Parede\nSB - Floor\nFT = Wall\nQ - Door\nABC - Door\nOU - ou\nZY: Wall with a long note\n";
        assert_eq!(app.add_shortcuts(parse_shortcuts_file(text)), (2, 6));
        // SB and FT are Floor: Structural and Filters already; the others are no key or no action.
        assert_eq!(app.custom_shortcuts.get("ZW").map(String::as_str), Some("Wall"));
        assert_eq!(app.custom_shortcuts.get("ZZ").map(String::as_str), Some("Wall"));
        assert_eq!(app.custom_shortcuts.len(), 2);
        // The same file again adds the same two and nothing else.
        assert_eq!(app.add_shortcuts(parse_shortcuts_file(text)), (2, 6));
        assert_eq!(app.custom_shortcuts.len(), 2);
    }

    #[test]
    fn json_and_revit_xml_lines_are_read_too() {
        let json = parse_shortcuts_file(r#"{"zw": "Wall", "zd": "Door", "bad": 5}"#);
        assert_eq!(json, [("ZD".to_string(), "Door".to_string()), ("ZW".to_string(), "Wall".to_string())]);
        let xml = parse_shortcuts_file(r#"<Shortcut CommandName="Wall" CommandId="ID_OBJECTS_WALL" Shortcuts="ZW#Z1" />"#);
        assert_eq!(xml, [("ZW".to_string(), "Wall".to_string()), ("Z1".to_string(), "Wall".to_string())]);
    }

    #[test]
    fn a_file_over_the_cap_or_missing_is_refused_with_a_message() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = BimApp::new(Session::new());
        let big = dir.path().join("big.txt");
        std::fs::write(&big, format!("ZW - Wall\n{}", " ".repeat(MAX_FILE as usize))).unwrap();
        app.load_shortcuts_from_file(&big);
        assert!(app.shortcuts_dialog.error.is_some() && app.custom_shortcuts.is_empty());
        app.load_shortcuts_from_file(&dir.path().join("nope.txt"));
        assert!(app.shortcuts_dialog.error.is_some() && app.custom_shortcuts.is_empty());
        let small = dir.path().join("keys.txt");
        std::fs::write(&small, "ZW - Wall\nSB - Floor\n").unwrap();
        app.load_shortcuts_from_file(&small);
        assert!(app.shortcuts_dialog.message.is_some() && app.shortcuts_dialog.error.is_none());
        assert_eq!(app.custom_shortcuts.len(), 1, "ZW added, SB (Floor: Structural) skipped");
    }

    #[test]
    fn duplicate_check_detects_existing_shortcuts() {
        let app = BimApp::new(Session::new());
        // "WA" already exists for Wall
        let conflict = find_conflict("WA", "Door", &app);
        assert_eq!(conflict.as_deref(), Some("Wall"));

        // "WA" for Wall is not a conflict with itself
        let no_conflict = find_conflict("WA", "Wall", &app);
        assert!(no_conflict.is_none());

        // Unused key "ZZ" has no conflict
        let no_conflict_new = find_conflict("ZZ", "Wall", &app);
        assert!(no_conflict_new.is_none());
    }

    #[test]
    fn custom_shortcut_execution_works() {
        let mut app = BimApp::new(Session::new());
        app.custom_shortcuts.insert("ZZ".to_string(), "Wall".to_string());
        app.typed = "ZZ".to_string();
        assert!(app.shortcut());
        assert!(matches!(app.tool, Tool::Wall(_)));
    }

    #[test]
    fn shortcut_3d_opens_view_3d() {
        let mut app = BimApp::new(Session::new());
        assert_ne!(app.sheet, crate::Sheet::View3D);
        app.typed = "3D".to_string();
        assert!(app.shortcut());
        assert_eq!(app.sheet, crate::Sheet::View3D);
    }

    #[test]
    fn every_action_the_dialog_offers_does_something() {
        for act in ACTIONS {
            let mut app = BimApp::new(Session::new());
            assert!(app.execute_shortcut_action(act), "{act}");
        }
    }
}
