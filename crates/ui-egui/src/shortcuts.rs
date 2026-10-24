//! Keyboard shortcuts customization, management, loading from file, and modal dialog.

use std::collections::BTreeMap;
use std::path::Path;

use bimcraft_core::geom::P2;
use egui::{Align, ComboBox, Context, Grid, Id as UiId, Layout, Modal, RichText, ScrollArea, TextEdit, vec2};

use crate::graphics;
use crate::lang::{t, tf};
use crate::pal;
use crate::{BimApp, Tab, Tool, phasing};

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
    "Brace",
    "Beam System",
    "Isolated Footing",
    "Wall Footing",
    "Grid",
    "Level",
    "Reference Plane",
    "Model Line",
    "Detail Line",
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
    "Split Face",
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
    "Sun Settings",
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
    "Convert Duct",
    "Cable Tray Fitting",
    "Duct Accessory",
    "Flex Duct",
    "Rebar Numbering",
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

/// Resolves a description, label or keyword in English, Portuguese or Spanish to a canonical action name.
pub fn resolve_action(desc: &str) -> Option<&'static str> {
    let d = desc.trim().to_lowercase();
    for &act in ACTIONS {
        if act.to_lowercase() == d {
            return Some(act);
        }
        let pt = t(act).to_lowercase();
        if pt == d {
            return Some(act);
        }
    }

    if d == "3d" || d.contains("3d view") || d.contains("vista 3d") || d.contains("vista/3d") || d.contains("representação 3d") {
        return Some("3D View");
    }
    if d.contains("unidades de projeto") || d.contains("project units") {
        return Some("Project Units");
    }
    if d.contains("configuração do sol") || d.contains("sun settings") {
        return Some("Sun Settings");
    }
    if d.contains("dimensão alinhada") || d.contains("cota alinhada") || d.contains("aligned dimension") {
        return Some("Aligned Dimension");
    }
    if d.contains("específicas da vista") || d.contains("detalhamento") || d.contains("detail line") {
        return Some("Detail Line");
    }
    if d.contains("identificadores") || d.contains("tag by category") {
        return Some("Tag by Category");
    }
    if d.contains("elemento oculto") || d.contains("apareça na vista") || d.contains("unhide elements") {
        return Some("Unhide Elements");
    }
    if d.contains("igualar propriedades") || d.contains("match properties") {
        return Some("Match Properties");
    }
    if d.contains("propriedades") || d.contains("properties") {
        return Some("Properties");
    }
    if d.contains("coloca um nível") || d.contains("colocar um nível") || d.contains("level") {
        return Some("Level");
    }
    if d.contains("nova linha") || d.contains("linha do modelo") || d.contains("model line") {
        return Some("Model Line");
    }
    if d.contains("componente") || d.contains("component") {
        return Some("Component");
    }
    if d.contains("alinha") || d.contains("align") {
        return Some("Align");
    }
    if d.contains("bloqueia") || d.contains("bloquear") || d.contains("fixar") || d.contains("pin") || d.contains("patch panel") {
        return Some("Patch Panel");
    }
    if d.contains("dividir face") || d.contains("split face") {
        return Some("Split Face");
    }
    if d.contains("corta um elemento") || d.contains("dividir") || d.contains("split") {
        return Some("Split");
    }
    if d.contains("sequências de teclas")
        || d.contains("teclas para ferramentas")
        || d.contains("atalhos")
        || d.contains("keyboard shortcuts")
    {
        return Some("Keyboard Shortcuts");
    }
    if d.contains("navegador de projetos") || d.contains("project browser") {
        return Some("Project Browser");
    }
    if d.contains("corte") || d.contains("section") {
        return Some("Section");
    }
    if d.contains("largura única") || d.contains("linhas finas") || d.contains("thin lines") {
        return Some("Thin Lines");
    }
    if d.contains("esconde aquele objeto") || d.contains("oculta um elemento") || d.contains("hide element") {
        return Some("Temporary Hide Element");
    }
    if d.contains("esconde todos os objetos daquela categoria") || d.contains("oculta todas as categorias") || d.contains("hide category") {
        return Some("Temporary Hide Category");
    }
    if d.contains("restaura") || d.contains("reset temporary") {
        return Some("Reset Temporary Hide/Isolate");
    }
    if d.contains("todas as instâncias") || d.contains("select all instances") {
        return Some("Select All Instances");
    }
    if d.contains("ambiente") || d.contains("room") {
        return Some("Room");
    }
    if d.contains("parede") || d.contains("wall") {
        return Some("Wall");
    }
    if d.contains("piso") || d.contains("floor") {
        return Some("Floor");
    }
    if d.contains("porta") || d.contains("door") {
        return Some("Door");
    }
    if d.contains("janela") || d.contains("window") {
        return Some("Window");
    }
    if d.contains("pilar") || d.contains("suporte de carga vertical") || d.contains("structural column") {
        return Some("Structural Column");
    }
    if d.contains("elemento novo similar") || d.contains("criar similar") || d.contains("create similar") {
        return Some("Create Similar");
    }
    if d.contains("viga estrutural") || d.contains("beam") {
        return Some("Beam");
    }
    if d.contains("membros diagonais") || d.contains("contraventamento") || d.contains("brace") {
        return Some("Brace");
    }
    if d.contains("série de vigas") || d.contains("vigas paralelas") || d.contains("sistema de vigas") || d.contains("beam system") {
        return Some("Beam System");
    }
    if d.contains("fundação") || d.contains("footing") {
        return Some("Wall Footing");
    }
    if d.contains("eixo de coluna") || d.contains("grid") {
        return Some("Grid");
    }
    if d.contains("numeração") || d.contains("rebar numbering") {
        return Some("Rebar Numbering");
    }
    if d.contains("plano de referência") || d.contains("reference plane") {
        return Some("Reference Plane");
    }
    if d.contains("conduíte") || d.contains("conduit") {
        return Some("Conduit");
    }
    if d.contains("acessórios da bandeja") || d.contains("cable tray fitting") {
        return Some("Cable Tray Fitting");
    }
    if d.contains("bandeja de cabos") || d.contains("cable tray") {
        return Some("Cable Tray");
    }
    if d.contains("converte") || d.contains("convert duct") {
        return Some("Convert Duct");
    }
    if d.contains("equipamento elétrico") || d.contains("electrical equipment") {
        return Some("Electrical Equipment");
    }
    if d.contains("fio") || d.contains("wire") {
        return Some("Wire");
    }
    if d.contains("acessórios de duto") || d.contains("duct accessory") {
        return Some("Duct Accessory");
    }
    if d.contains("dutos flexíveis") || d.contains("duto flexível") || d.contains("flex duct") {
        return Some("Flex Duct");
    }
    None
}

/// Parses shortcut definitions from file contents (markdown, text, JSON, or XML).
pub fn parse_shortcuts_file(content: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();

    // Check if JSON
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(content)
        && let Some(obj) = v.as_object()
    {
        for (key, val) in obj {
            if let Some(desc) = val.as_str() {
                let act = resolve_action(desc).unwrap_or(desc);
                entries.push((key.to_ascii_uppercase(), act.to_string()));
            }
        }
        if !entries.is_empty() {
            return entries;
        }
    }

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // XML: <Shortcut CommandId="..." Shortcuts="WA#W1" ... />
        if line.contains("<Shortcut") {
            let cmd = line.split("CommandId=\"").nth(1).and_then(|s| s.split('"').next());
            let scs = line.split("Shortcuts=\"").nth(1).and_then(|s| s.split('"').next());
            if let (Some(cmd), Some(scs)) = (cmd, scs) {
                let action = resolve_action(cmd).unwrap_or(cmd);
                for sc in scs.split('#') {
                    let k = sc.trim().to_ascii_uppercase();
                    if !k.is_empty() {
                        entries.push((k, action.to_string()));
                    }
                }
            }
            continue;
        }

        // Markdown: **KEY** – Description or **KEY1 ou KEY2** – Description
        if let Some(stripped) = line.strip_prefix("**")
            && let Some(end_bold) = stripped.find("**")
        {
            let keys_part = &stripped[..end_bold];
            let rest = stripped[end_bold + 2..].trim_start_matches([' ', '-', '–', ':']).trim();
            if let Some(act) = resolve_action(rest) {
                for key in keys_part.split([' ', '/', ',']) {
                    let k = key.trim().to_ascii_uppercase();
                    if k.len() == 2 && k.chars().all(|c| c.is_ascii_alphanumeric()) {
                        entries.push((k, act.to_string()));
                    }
                }
                continue;
            }
        }

        // Plain text: KEY - Description, KEY = Description, KEY: Description
        let sep = [':', '=', '-', '–'].into_iter().find_map(|c| line.find(c).map(|idx| (idx, c.len_utf8())));
        if let Some((pos, char_len)) = sep {
            let key = line[..pos].trim().trim_matches('*').trim().to_ascii_uppercase();
            let desc = line[pos + char_len..].trim();
            if key.len() == 2 && key.chars().all(|c| c.is_ascii_alphanumeric()) {
                if let Some(act) = resolve_action(desc) {
                    entries.push((key, act.to_string()));
                } else if !desc.is_empty() {
                    entries.push((key, desc.to_string()));
                }
            }
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
    /// Loads keyboard shortcuts from a file on disk (markdown, txt, json, xml).
    pub(crate) fn load_shortcuts_from_file(&mut self, path: &Path) {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                self.shortcuts_dialog.error = Some(tf("Failed to read file: {}", &[&e.to_string()]));
                self.shortcuts_dialog.message = None;
                return;
            }
        };

        let parsed = parse_shortcuts_file(&content);
        let count = parsed.len();
        for (k, act) in parsed {
            self.custom_shortcuts.insert(k, act);
        }
        self.shortcuts_dialog.error = None;
        self.shortcuts_dialog.message = Some(tf("Loaded {} shortcuts from file.", &[&count.to_string()]));
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
            "3D View" | "3D" | "Vista 3D" => self.open_sheet(crate::Sheet::View3D),
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
            "Aligned Dimension" | "Dimension" => self.set_tool(Tool::Dimension(Vec::new())),
            "Text" => self.set_tool(Tool::Text),
            "Spot Elevation" => self.set_tool(Tool::Spot),
            "Tag by Category" | "Tag" => self.set_tool(Tool::Tag),
            "Section" => self.set_tool(Tool::Section(None)),
            "Elevation" => self.set_tool(Tool::Elevation),
            "Measure" => self.set_tool(Tool::Measure(None)),
            "Align" => self.set_tool(Tool::Align(None)),
            "Trim/Extend to Corner" | "Trim" => self.set_tool(Tool::Trim(None)),
            "Split" => self.set_tool(Tool::Split),
            "Offset" => self.set_tool(Tool::Offset),
            "Demolish" => self.set_tool(Tool::Demolish),
            "Paint" => self.set_tool(Tool::Paint),
            "Move" => self.set_tool(Tool::Move(None)),
            "Copy" => self.set_tool(Tool::Copy(None)),
            "Rotate" => self.set_tool(Tool::Rotate { centre: P2(0.0, 0.0), start: None, place: false }),
            "Mirror Pick Axis" | "Mirror Pick" => self.set_tool(Tool::Mirror { draw: false, start: None }),
            "Mirror Draw Axis" | "Mirror Draw" => self.set_tool(Tool::Mirror { draw: true, start: None }),
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
            "Show Mass" => self.show_mass(),
            "Mass Floors" => self.by_mass("mass_floors"),
            "Wall by Face" => self.by_mass("wall_by_face"),
            "Floor by Face" => self.by_mass("floor_by_face"),
            "Roof by Face" => self.by_mass("roof_by_face"),
            "Update to Face" => self.by_mass("update_by_face"),
            "Sun Settings" => self.open_graphics(0),
            "Detail Line" => self.set_tool(Tool::Measure(None)),
            "Model Line" => self.set_tool(Tool::Run("wire", None)),
            "Split Face" => self.set_tool(Tool::Split),
            "Match Properties" => self.match_properties(),
            "Brace" => {
                self.tab = Tab::Structure;
                self.set_tool(Tool::Run("beam", None));
            }
            "Beam System" => {
                self.tab = Tab::Structure;
                self.set_tool(Tool::Run("beam", None));
            }
            "Rebar Numbering" => self.open_ids_of_selection(),
            "Reference Plane" => self.set_tool(Tool::Grid(None)),
            "Convert Duct" => {
                self.tab = Tab::Electrical;
                self.set_tool(Tool::Run("conduit", None));
            }
            "Cable Tray Fitting" => {
                self.tab = Tab::Electrical;
                self.set_tool(Tool::Fixture("fitting"));
            }
            "Duct Accessory" => self.set_tool(Tool::Fixture("accessory")),
            "Flex Duct" => {
                self.tab = Tab::Electrical;
                self.set_tool(Tool::Run("conduit", None));
            }
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
        let mut load_teclas_md = false;

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

                if Path::new("teclas.MD").exists() && ui.button(t("Load teclas.MD")).clicked() {
                    load_teclas_md = true;
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

                            if key.is_empty() || key.len() > 4 || !key.chars().all(|c| c.is_ascii_alphanumeric()) {
                                self.shortcuts_dialog.error =
                                    Some(t("Shortcut keys must have 1 to 4 characters (letters or numbers).").to_string());
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
                ui.label(RichText::new(msg).color(pal().accent));
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
                if new_key.is_empty() || new_key.len() > 4 || !new_key.chars().all(|c| c.is_ascii_alphanumeric()) {
                    self.shortcuts_dialog.error = Some(t("Shortcut keys must have 1 to 4 characters (letters or numbers).").to_string());
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

        if load_teclas_md {
            self.load_shortcuts_from_file(Path::new("teclas.MD"));
        }

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
    fn parse_teclas_md_loads_shortcuts() {
        let text = std::fs::read_to_string("../../teclas.MD").or_else(|_| std::fs::read_to_string("teclas.MD")).unwrap();
        let parsed = parse_shortcuts_file(&text);
        assert!(!parsed.is_empty());
        assert!(parsed.iter().any(|(k, a)| k == "WA" && a == "Wall"));
        assert!(parsed.iter().any(|(k, a)| k == "DR" && a == "Door"));
        assert!(parsed.iter().any(|(k, a)| k == "WN" && a == "Window"));
        assert!(parsed.iter().any(|(k, a)| k == "UN" && a == "Project Units"));
        assert!(parsed.iter().any(|(k, a)| k == "BM" && a == "Beam"));
        assert!(parsed.iter().any(|(k, a)| k == "CR" && a == "Section"));
        assert!(parsed.iter().any(|(k, a)| k == "3D" && a == "3D View"));
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
}
