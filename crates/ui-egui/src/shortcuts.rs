//! Keyboard shortcuts: the action registry, the bindings, keyboard input, the management dialog,
//! and the text, JSON and Markdown files the user can read and write.
//!
//! A binding is either a chord (`Ctrl+Shift+D`, `F8`) or a two-key sequence (`WA`, `MV`). Chords
//! are matched on the physical key, so `Ctrl+D` works on every layout; sequences are read from
//! physical keys too, which is what makes `WA` work on a Russian layout.

use crate::popup::Modal;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

use bimcraft_core::geom::P2;
use bimcraft_core::snap::Kind as Snap;
use egui::{Align, ComboBox, Context, Event, Id as UiId, Key, Layout, Modifiers, RichText, ScrollArea, TextEdit, vec2};
use serde_json::{Value, json};

use crate::graphics;
use crate::lang::{Lang, t, tf, translated_in};
use crate::pal;
use crate::{BimApp, Once, Pending, Tab, Tool, phasing};

/// The largest shortcuts file read: 4096 bindings of a line of prose each still fit.
const MAX_FILE: u64 = 256 << 10;
/// The most lines a shortcuts file may have.
const MAX_LINES: usize = 10_000;
/// The most bindings a profile may hold.
pub const MAX_BINDINGS: usize = 4096;
/// The most bindings one action may have.
pub const MAX_PER_ACTION: usize = 32;

/// An action's stable id: `cmd.move`, `ui.file.save_as`, `legacy.<name>`.
pub type ActionId = Arc<str>;

/// A physical key with its modifiers. `ctrl` is the platform's primary modifier (Ctrl or Cmd).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyChord {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

impl KeyChord {
    pub fn new(key: Key, ctrl: bool, shift: bool, alt: bool, meta: bool) -> Self {
        KeyChord { key, ctrl, shift, alt, meta }
    }

    /// The chord a key event makes. `Ctrl` is the primary modifier: Ctrl on Windows/Linux, Cmd
    /// on macOS. A literal `Meta` is Command on macOS and the Windows/Super key elsewhere.
    pub fn from_event(key: Key, modifiers: Modifiers) -> Self {
        KeyChord { key, ctrl: modifiers.command, shift: modifiers.shift, alt: modifiers.alt, meta: modifiers.mac_cmd && !modifiers.command }
    }
}

/// One keyboard binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Binding {
    Chord(KeyChord),
    Sequence([Key; 2]),
    /// Three keys, for a user who wants `BMM` beside Revit's two-letter `BM`.
    Sequence3([Key; 3]),
}

/// What kind of thing an action is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActionKind {
    Tool,
    Dialog,
    Immediate,
    Input,
    Internal,
}

impl ActionKind {
    pub fn name(self) -> &'static str {
        match self {
            ActionKind::Tool => "Tool",
            ActionKind::Dialog => "Dialog",
            ActionKind::Immediate => "Immediate",
            ActionKind::Input => "Input",
            ActionKind::Internal => "Internal",
        }
    }

    pub fn bindable(self) -> bool {
        self != ActionKind::Internal && self != ActionKind::Input
    }
}

/// A group of actions in the dialog and in the Markdown help.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    File,
    Edit,
    Architecture,
    Structure,
    View,
    Manage,
    Palettes,
    Snaps,
    Mep,
    Annotate,
    Other,
}

impl Category {
    pub const ALL: [Category; 11] = [
        Category::File,
        Category::Edit,
        Category::Architecture,
        Category::Structure,
        Category::View,
        Category::Manage,
        Category::Palettes,
        Category::Snaps,
        Category::Mep,
        Category::Annotate,
        Category::Other,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::File => "File",
            Category::Edit => "Edit",
            Category::Architecture => "Architecture",
            Category::Structure => "Structure",
            Category::View => "Views",
            Category::Manage => "Manage",
            Category::Palettes => "Palettes",
            Category::Snaps => "Snaps",
            Category::Mep => "MEP",
            Category::Annotate => "Annotations",
            Category::Other => "Other",
        }
    }
}

/// The built-in starting sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    BimCraft,
    Revit,
    Archicad,
    Empty,
    Custom,
}

impl Profile {
    pub const CHOICES: [Profile; 4] = [Profile::BimCraft, Profile::Revit, Profile::Archicad, Profile::Empty];

    pub fn code(self) -> &'static str {
        match self {
            Profile::BimCraft => "bimcraft",
            Profile::Revit => "revit",
            Profile::Archicad => "archicad",
            Profile::Empty => "empty",
            Profile::Custom => "custom",
        }
    }

    pub fn from_code(code: &str) -> Option<Profile> {
        match code.to_ascii_lowercase().as_str() {
            "bimcraft" => Some(Profile::BimCraft),
            "revit" => Some(Profile::Revit),
            "archicad" => Some(Profile::Archicad),
            "empty" => Some(Profile::Empty),
            "custom" => Some(Profile::Custom),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Profile::BimCraft => "BIMCraft",
            Profile::Revit => "Revit-like",
            Profile::Archicad => "Archicad-like",
            Profile::Empty => "Empty",
            Profile::Custom => "Custom",
        }
    }
}

/// An action the user can bind, or one the dialog shows with a reason.
#[derive(Clone)]
pub struct ActionDef {
    pub id: ActionId,
    /// The English name; `t()` localizes it.
    pub label: &'static str,
    pub category: Category,
    pub kind: ActionKind,
    /// The sequence bindings a starting profile gives it.
    pub defaults: Vec<&'static str>,
    pub menu: Option<&'static str>,
    /// The name the old executor knows it by.
    pub legacy: Option<&'static str>,
}

/// The actions, in the order the dialog lists them when nothing else sorts them.
pub struct Registry {
    actions: Vec<ActionDef>,
    by_id: HashMap<ActionId, usize>,
    by_label: HashMap<String, ActionId>,
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();

/// The action registry: the window's own actions first, then the core's commands, then any legacy
/// action the old names still reach.
pub fn registry() -> &'static Registry {
    REGISTRY.get_or_init(build_registry)
}

impl Registry {
    pub fn actions(&self) -> &[ActionDef] {
        &self.actions
    }

    pub fn get(&self, id: &str) -> Option<&ActionDef> {
        self.by_id.get(id).map(|i| &self.actions[*i])
    }

    /// The action a name means: its id, its English label, or a localized label. Ids win.
    pub fn resolve(&self, name: &str) -> Option<ActionId> {
        let name = name.trim();
        if let Some(id) = self.by_id.get_key_value(name) {
            return Some(id.0.clone());
        }
        if let Some(id) = self.by_label.get(&name.to_lowercase()) {
            return Some(id.clone());
        }
        let lower = name.to_lowercase();
        for l in Lang::ALL {
            if let Some(id) =
                self.actions.iter().find(|a| translated_in(l, a.label).is_some_and(|w| w.to_lowercase() == lower)).map(|a| a.id.clone())
            {
                return Some(id);
            }
        }
        None
    }

    pub fn by_label(&self, label: &str) -> Option<&ActionDef> {
        self.by_label.get(&label.to_lowercase()).and_then(|id| self.get(id))
    }

    fn index(&mut self) {
        self.by_id.clear();
        self.by_label.clear();
        for (i, a) in self.actions.iter().enumerate() {
            debug_assert!(!self.by_id.contains_key(&a.id), "duplicate action id {}", a.id);
            self.by_id.insert(a.id.clone(), i);
            self.by_label.entry(a.label.to_lowercase()).or_insert_with(|| a.id.clone());
        }
    }
}

/// A command's action kind: only the ones a shortcut can really run are bindable.
const TOOL_COMMANDS: &[&str] = &[
    "wall",
    "door",
    "window",
    "floor",
    "ceiling",
    "shaft",
    "stair",
    "ramp",
    "railing",
    "room",
    "room_separator",
    "level",
    "grid",
    "column",
    "footing",
    "beam",
    "pipe",
    "duct",
    "conduit",
    "cable_tray",
    "wire",
    "component",
    "mass",
    "section",
    "elevation",
    "text",
    "spot",
    "dimension",
    "paint",
    "linework",
    "detail_line",
    "detail_group",
    "repeating_detail",
    "move",
    "copy",
    "rotate",
    "mirror",
    "align",
    "offset",
    "corner",
    "split",
    "trim_extend",
    "filled_region",
    "sketch",
    "point_cloud",
    "title_block",
];

const DIALOG_COMMANDS: &[&str] = &[
    "link",
    "purge",
    "units",
    "graphics",
    "graphic_display_options",
    "duplicate_view",
    "filters_placeholder",
    "reinforce",
    "material",
    "favorite",
    "parameter",
    "family",
    "line_style",
    "line_pattern",
    "fill_pattern",
    "object_styles",
    "line_weights",
    "phase",
    "phase_filter",
    "option",
    "template",
    "filter",
    "work_plane",
    "section_box",
    "view_template_placeholder",
];

const IMMEDIATE_COMMANDS: &[&str] = &[
    "new",
    "open",
    "save",
    "undo",
    "redo",
    "show_mass",
    "mass_floors",
    "wall_by_face",
    "floor_by_face",
    "roof_by_face",
    "update_by_face",
    "join_polyline",
    "explode",
    "create_group",
    "ungroup",
    "delete",
    "match_properties",
    "select_by_id",
    "ids_of",
    "purge",
    "reinforce",
    "title_block",
];

/// The window's own actions, which are not core commands.
const UI_ACTIONS: &[(&str, &str, Category, ActionKind, &str)] = &[
    ("ui.file.save_as", "Save As", Category::File, ActionKind::Immediate, "Save As"),
    ("ui.file.export_pdf", "Export PDF", Category::File, ActionKind::Immediate, "Export PDF"),
    ("ui.settings", "Settings", Category::File, ActionKind::Dialog, "Settings"),
    ("ui.settings.shortcuts", "Keyboard Shortcuts", Category::File, ActionKind::Dialog, "Keyboard Shortcuts"),
    ("ui.find", "Find", Category::Edit, ActionKind::Dialog, "Find"),
    ("ui.edit.modify", "Modify", Category::Edit, ActionKind::Tool, "Modify"),
    ("ui.edit.select_all_instances", "Select All Instances", Category::Edit, ActionKind::Immediate, "Select All Instances"),
    ("ui.tool.floor_structural", "Floor: Structural", Category::Structure, ActionKind::Tool, "Floor: Structural"),
    ("ui.tool.mirror_pick", "Mirror Pick Axis", Category::Edit, ActionKind::Tool, "Mirror Pick Axis"),
    ("ui.tool.mirror_draw", "Mirror Draw Axis", Category::Edit, ActionKind::Tool, "Mirror Draw Axis"),
    ("ui.view.3d", "3D View", Category::View, ActionKind::Immediate, "3D View"),
    ("ui.view.plan", "Floor Plan", Category::View, ActionKind::Immediate, "Floor Plan"),
    ("ui.view.home", "Home", Category::View, ActionKind::Immediate, "Home"),
    ("ui.view.zoom_fit", "Zoom to Fit", Category::View, ActionKind::Immediate, "Zoom to Fit"),
    ("ui.view.thin_lines", "Thin Lines", Category::View, ActionKind::Immediate, "Thin Lines"),
    ("ui.view.alignment_guides", "Alignment Guides", Category::View, ActionKind::Immediate, "Alignment Guides"),
    ("ui.view.orthogonal_guides", "Orthogonal Guides", Category::View, ActionKind::Immediate, "Orthogonal Guides"),
    ("ui.view.hide_element", "Hide Element", Category::View, ActionKind::Immediate, "Hide Element"),
    ("ui.view.hide_category", "Hide Category", Category::View, ActionKind::Immediate, "Hide Category"),
    ("ui.view.temp_hide_element", "Temporary Hide Element", Category::View, ActionKind::Immediate, "Temporary Hide Element"),
    ("ui.view.temp_hide_category", "Temporary Hide Category", Category::View, ActionKind::Immediate, "Temporary Hide Category"),
    ("ui.view.temp_isolate_element", "Temporary Isolate Element", Category::View, ActionKind::Immediate, "Temporary Isolate Element"),
    ("ui.view.temp_isolate_category", "Temporary Isolate Category", Category::View, ActionKind::Immediate, "Temporary Isolate Category"),
    ("ui.view.temp_reset", "Reset Temporary Hide/Isolate", Category::View, ActionKind::Immediate, "Reset Temporary Hide/Isolate"),
    ("ui.view.reveal_hidden", "Reveal Hidden Elements", Category::View, ActionKind::Immediate, "Reveal Hidden Elements"),
    ("ui.view.unhide_elements", "Unhide Elements", Category::View, ActionKind::Immediate, "Unhide Elements"),
    ("ui.view.unhide_category", "Unhide Category", Category::View, ActionKind::Immediate, "Unhide Category"),
    ("ui.view.override_graphics", "Override Graphics by Element", Category::View, ActionKind::Immediate, "Override Graphics by Element"),
    ("ui.view.wireframe", "Wireframe", Category::View, ActionKind::Immediate, "Wireframe"),
    ("ui.view.hidden_line", "Hidden Line", Category::View, ActionKind::Immediate, "Hidden Line"),
    ("ui.view.shaded", "Shaded", Category::View, ActionKind::Immediate, "Shaded"),
    ("ui.view.filters", "Filters", Category::View, ActionKind::Dialog, "Filters"),
    ("ui.view.templates", "View Templates", Category::View, ActionKind::Dialog, "View Templates"),
    ("ui.manage.phasing", "Phasing", Category::Manage, ActionKind::Dialog, "Phasing"),
    ("ui.manage.design_options", "Design Options", Category::Manage, ActionKind::Dialog, "Design Options"),
    ("ui.palette.properties", "Properties", Category::Palettes, ActionKind::Immediate, "Properties"),
    ("ui.palette.browser", "Project Browser", Category::Palettes, ActionKind::Immediate, "Project Browser"),
    ("ui.assistant", "Assistant", Category::Palettes, ActionKind::Immediate, "Assistant"),
    ("ui.title_blocks", "Title Blocks", Category::Annotate, ActionKind::Dialog, "Title Blocks"),
    ("ui.snap.endpoints", "Snap to Endpoints", Category::Snaps, ActionKind::Immediate, "Snap to Endpoints"),
    ("ui.snap.midpoints", "Snap to Midpoints", Category::Snaps, ActionKind::Immediate, "Snap to Midpoints"),
    ("ui.snap.intersections", "Snap to Intersections", Category::Snaps, ActionKind::Immediate, "Snap to Intersections"),
    ("ui.snap.centres", "Snap to Centres", Category::Snaps, ActionKind::Immediate, "Snap to Centres"),
    ("ui.snap.perpendiculars", "Snap to Perpendiculars", Category::Snaps, ActionKind::Immediate, "Snap to Perpendiculars"),
    ("ui.snap.nearest", "Snap to Nearest", Category::Snaps, ActionKind::Immediate, "Snap to Nearest"),
    ("ui.snap.off", "Snaps Off", Category::Snaps, ActionKind::Immediate, "Snaps Off"),
    ("ui.snap.reset", "Turn Snap Override Off", Category::Snaps, ActionKind::Immediate, "Turn Snap Override Off"),
];

/// The chords the BIMCraft profile starts with. The two-letter sequences come from the desktop's
/// own table and the commands' aliases.
const DEFAULT_CHORDS: &[(&str, &[&str])] = &[
    ("cmd.save", &["Ctrl+S"]),
    ("ui.file.save_as", &["Ctrl+Shift+S"]),
    ("cmd.open", &["Ctrl+O"]),
    ("cmd.new", &["Ctrl+N"]),
    ("ui.find", &["Ctrl+K"]),
    ("ui.settings", &["Ctrl+,"]),
    ("ui.settings.shortcuts", &["Ctrl+Shift+K"]),
    ("ui.palette.properties", &["Ctrl+1"]),
    ("ui.palette.browser", &["Ctrl+2"]),
    ("ui.file.export_pdf", &["Ctrl+P"]),
    ("ui.view.home", &["Ctrl+U"]),
    ("cmd.undo", &["Ctrl+Z"]),
    ("cmd.redo", &["Ctrl+Shift+Z", "Ctrl+Y"]),
    ("cmd.delete", &["Delete"]),
    ("cmd.copy", &["Ctrl+Shift+D"]),
    ("cmd.rotate", &["Ctrl+R"]),
    ("cmd.mirror", &["Ctrl+M"]),
    ("ui.tool.mirror_draw", &["Ctrl+Shift+M"]),
    ("cmd.offset", &["Ctrl+Shift+O"]),
    ("cmd.corner", &["Ctrl+T"]),
    ("cmd.split", &["Ctrl+Shift+T"]),
    ("cmd.align", &["Ctrl+Shift+A"]),
    ("ui.view.zoom_fit", &["Ctrl+0"]),
    ("ui.view.3d", &["F3"]),
    ("ui.view.plan", &["F2"]),
    ("ui.view.alignment_guides", &["F8"]),
];

/// The Archicad-like profile's extra chords, and its `Ctrl+D` for Move.
const ARCHICAD_CHORDS: &[(&str, &[&str])] = &[("cmd.move", &["Ctrl+D"])];

/// Two-letter bindings the Revit-style table does not have but the command catalog names.
const EXTRA_SEQUENCES: &[(&str, &str)] = &[
    ("DU", "Duct"),
    ("FI", "Isolated Footing"),
    ("FW", "Wall Footing"),
    ("BM", "Beam"),
    ("PI", "Pipe"),
    ("CN", "Conduit"),
    ("CT", "Cable Tray"),
    ("DC", "Down Conductor"),
    ("GC", "Earthing Conductor"),
    ("CB", "Cabling"),
    ("DL", "Detail Line"),
    ("FR", "Filled Region"),
    ("GP", "Create Group"),
    ("UG", "Ungroup"),
    ("LW", "Linework"),
    ("PT", "Paint"),
    ("PJ", "Join into Polyline"),
    ("EX", "Explode Polyline"),
    ("DE", "Delete"),
    ("UN", "Project Units"),
    ("GD", "Graphic Display Options"),
    ("PU", "Purge Unused"),
    ("LK", "Link Model"),
    ("DP", "Duplicate View"),
    ("CR", "Section"),
    ("TG", "Tag by Category"),
    ("MA", "Match Properties"),
];

fn build_registry() -> Registry {
    let mut actions: Vec<ActionDef> = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut push = |a: ActionDef, actions: &mut Vec<ActionDef>| {
        if seen_ids.insert(a.id.to_string()) {
            actions.push(a);
        }
    };

    for &(id, label, category, kind, legacy) in UI_ACTIONS {
        push(ActionDef { id: Arc::from(id), label, category, kind, defaults: Vec::new(), menu: None, legacy: Some(legacy) }, &mut actions);
    }

    for c in bimcraft_core::engine::commands() {
        let mut kind = if TOOL_COMMANDS.contains(&c.id) {
            ActionKind::Tool
        } else if DIALOG_COMMANDS.contains(&c.id) {
            ActionKind::Dialog
        } else if IMMEDIATE_COMMANDS.contains(&c.id) || !c.aliases.is_empty() {
            ActionKind::Immediate
        } else {
            ActionKind::Input
        };
        // A few commands need parameters and have no UI that gathers them: they are shown but not
        // offered. Keeping them `Input` is the dialog's way of saying why.
        if c.params.starts_with('{') && c.menu.is_none() && c.aliases.is_empty() {
            kind = if kind == ActionKind::Tool { ActionKind::Tool } else { ActionKind::Input };
        }
        let id: Arc<str> = Arc::from(format!("cmd.{}", c.id));
        push(
            ActionDef {
                id,
                label: c.label,
                category: command_category(c.id, c.menu),
                kind,
                defaults: c.aliases.to_vec(),
                menu: c.menu,
                legacy: Some(c.label),
            },
            &mut actions,
        );
    }

    // Every name the old dialog offered must still resolve: a name that is no command becomes a
    // legacy action with the old executor behind it.
    for &label in ACTIONS {
        if actions.iter().any(|a| a.label == label) {
            continue;
        }
        let slug = label.to_ascii_lowercase().replace(|c: char| !c.is_ascii_alphanumeric(), "_");
        let id: Arc<str> = Arc::from(format!("legacy.{slug}"));
        push(
            ActionDef {
                id,
                label,
                category: Category::Other,
                kind: ActionKind::Immediate,
                defaults: Vec::new(),
                menu: None,
                legacy: Some(label),
            },
            &mut actions,
        );
    }

    // The desktop's own table names a few actions the old dialog did not list (Alignment Guides,
    // the quadrant and tangent snaps): they are actions too, and their sequences must resolve.
    for &(_, label) in crate::SHORTCUTS.iter() {
        if actions.iter().any(|a| a.label == label) {
            continue;
        }
        let slug = label.to_ascii_lowercase().replace(|c: char| !c.is_ascii_alphanumeric(), "_");
        let id: Arc<str> = Arc::from(format!("legacy.{slug}"));
        push(
            ActionDef {
                id,
                label,
                category: Category::Other,
                kind: ActionKind::Immediate,
                defaults: Vec::new(),
                menu: None,
                legacy: Some(label),
            },
            &mut actions,
        );
    }
    for f in crate::mep::FIXTURES {
        if actions.iter().any(|a| a.label == f.label) {
            continue;
        }
        let slug = f.label.to_ascii_lowercase().replace(|c: char| !c.is_ascii_alphanumeric(), "_");
        let id: Arc<str> = Arc::from(format!("legacy.{slug}"));
        push(
            ActionDef {
                id,
                label: f.label,
                category: Category::Mep,
                kind: ActionKind::Immediate,
                defaults: Vec::new(),
                menu: None,
                legacy: Some(f.label),
            },
            &mut actions,
        );
    }

    // The starting sequences: the desktop's own table, the MEP fixtures, then the extras.
    let mut defaults: HashMap<String, Vec<&'static str>> = HashMap::new();
    for (keys, label) in crate::SHORTCUTS.iter().map(|(k, l)| (*k, *l)) {
        defaults.entry(label.to_string()).or_default().push(keys);
    }
    for f in crate::mep::FIXTURES {
        defaults.entry(f.label.to_string()).or_default().push(f.letters);
    }
    for (keys, label) in EXTRA_SEQUENCES {
        defaults.entry(label.to_string()).or_default().push(keys);
    }
    for a in &mut actions {
        if let Some(keys) = defaults.remove(a.label) {
            a.defaults.extend(keys);
        }
    }

    let mut registry = Registry { actions, by_id: HashMap::new(), by_label: HashMap::new() };
    registry.index();
    registry
}

fn command_category(id: &str, menu: Option<&str>) -> Category {
    if let Some(menu) = menu {
        if menu.starts_with("Architecture") {
            return Category::Architecture;
        }
        if menu.starts_with("Manage") {
            return Category::Manage;
        }
    }
    match id {
        "new"
        | "open"
        | "save"
        | "insert"
        | "link"
        | "reload"
        | "recover"
        | "export_crosscheck"
        | "compare_crosscheck"
        | "export_fill_patterns"
        | "import_cad"
        | "reload_cad"
        | "point_cloud" => Category::File,
        "undo" | "redo" | "modify" | "move" | "copy" | "rotate" | "mirror" | "align" | "offset" | "corner" | "split" | "trim_extend"
        | "delete" | "paint" | "linework" | "match_properties" | "fillet" | "join_polyline" | "explode" | "vertex" | "convert_spline"
        | "create_group" | "ungroup" | "detail_line" | "detail_group" | "repeating_detail" | "flip" | "select_by_id" | "ids_of"
        | "filter_selection" => Category::Edit,
        "wall" | "door" | "window" | "floor" | "ceiling" | "roof" | "stair" | "ramp" | "railing" | "room" | "room_separator" | "shaft"
        | "mass" | "show_mass" | "mass_floors" | "wall_by_face" | "floor_by_face" | "roof_by_face" | "update_by_face" | "component"
        | "dormer" | "sketch" => Category::Architecture,
        "column" | "structural_column" | "beam" | "footing" | "rebar" | "reinforce" | "reinforcement_preset" | "strip_footing" => {
            Category::Structure
        }
        "section"
        | "elevation"
        | "sheet"
        | "place"
        | "crop"
        | "duplicate_view"
        | "scale"
        | "detail"
        | "visual_style"
        | "graphics"
        | "section_box"
        | "perspective"
        | "graphic_display_options"
        | "camera"
        | "work_plane"
        | "filter"
        | "template"
        | "phase"
        | "phase_filter"
        | "option"
        | "title_block" => Category::View,
        "material"
        | "favorite"
        | "parameter"
        | "family"
        | "line_pattern"
        | "line_style"
        | "fill_pattern"
        | "make_fill_pattern"
        | "import_fill_patterns"
        | "filled_region_type"
        | "group_type"
        | "delete_type"
        | "rename_type"
        | "purge"
        | "line_weights"
        | "object_styles"
        | "units"
        | "room_area"
        | "georeference"
        | "language"
        | "standard"
        | "inspect"
        | "render"
        | "schedule"
        | "library"
        | "shared_parameters" => Category::Manage,
        "pipe"
        | "duct"
        | "conduit"
        | "cable_tray"
        | "wire"
        | "down_conductor"
        | "ground_conductor"
        | "cabling"
        | "voltage"
        | "distribution_system"
        | "wire_type"
        | "circuit"
        | "conduit_fill"
        | "drain_size"
        | "vent_size"
        | "hydrant_size"
        | "load_points"
        | "dr_check"
        | "dps_check"
        | "demand"
        | "water_size"
        | "reservoir_size"
        | "storm_size"
        | "network"
        | "panel"
        | "panel_schedule"
        | "demand_factors" => Category::Mep,
        "dimension"
        | "dimension_3d"
        | "dimension_constraint"
        | "dimension_text"
        | "dimension_type"
        | "text"
        | "text_type"
        | "spot"
        | "spot_slope"
        | "tag" => Category::Annotate,
        _ => Category::Other,
    }
}

/// All available action names, in the old dialog's shape, kept for callers that still list them.
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
    "Room Separator",
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

/// The action a description names: its id, English label, localized label, or old name.
pub fn resolve_action(desc: &str) -> Option<ActionId> {
    registry().resolve(desc)
}

/// Whether `k` is a two-character sequence the old dialog would have taken.
#[cfg(test)]
pub(crate) fn is_key(k: &str) -> bool {
    k.len() == 2 && k.chars().all(|c| c.is_ascii_alphanumeric())
}

/// The Cyrillic letters the Russian layout puts on the US punctuation keys and letters, keyed by
/// the letter: `в` is the physical `D`, `а` the physical `F`, and so on.
const CYRILLIC: &[(char, Key)] = &[
    ('й', Key::Q),
    ('ц', Key::W),
    ('у', Key::E),
    ('к', Key::R),
    ('е', Key::T),
    ('н', Key::Y),
    ('г', Key::U),
    ('ш', Key::I),
    ('щ', Key::O),
    ('з', Key::P),
    ('х', Key::OpenBracket),
    ('ъ', Key::CloseBracket),
    ('ф', Key::A),
    ('ы', Key::S),
    ('в', Key::D),
    ('а', Key::F),
    ('п', Key::G),
    ('р', Key::H),
    ('о', Key::J),
    ('л', Key::K),
    ('д', Key::L),
    ('ж', Key::Semicolon),
    ('э', Key::Quote),
    ('я', Key::Z),
    ('ч', Key::X),
    ('с', Key::C),
    ('м', Key::V),
    ('и', Key::B),
    ('т', Key::N),
    ('ь', Key::M),
    ('б', Key::Comma),
    ('ю', Key::Period),
    ('ё', Key::Backtick),
];

/// The Cyrillic letter a physical key carries on the Russian layout, for the `(Ctrl+В)` hint.
fn cyrillic_of(key: Key) -> Option<char> {
    CYRILLIC.iter().find(|(_, k)| *k == key).map(|(c, _)| *c)
}

fn key_of_char(c: char) -> Option<Key> {
    if c.is_ascii_alphabetic() {
        return Key::from_name(&c.to_ascii_uppercase().to_string());
    }
    if c.is_ascii_digit() {
        return Key::from_name(&c.to_string());
    }
    let lower = c.to_lowercase().next().unwrap_or(c);
    if let Some((_, key)) = CYRILLIC.iter().find(|(cyr, _)| *cyr == lower) {
        return Some(*key);
    }
    match c {
        ',' => Some(Key::Comma),
        '.' => Some(Key::Period),
        '/' => Some(Key::Slash),
        ';' => Some(Key::Semicolon),
        '\'' => Some(Key::Quote),
        '`' => Some(Key::Backtick),
        '-' => Some(Key::Minus),
        '=' => Some(Key::Equals),
        '[' => Some(Key::OpenBracket),
        ']' => Some(Key::CloseBracket),
        '\\' => Some(Key::Backslash),
        '+' => Some(Key::Plus),
        _ => Key::from_name(&c.to_string()),
    }
}

/// The canonical name of a key, for a file and the dialog.
pub fn key_name(key: Key) -> &'static str {
    match key {
        Key::Comma => ",",
        Key::Period => ".",
        Key::Slash => "/",
        Key::Semicolon => ";",
        Key::Quote => "'",
        Key::Backtick => "`",
        Key::Minus => "-",
        Key::Equals => "=",
        Key::OpenBracket => "[",
        Key::CloseBracket => "]",
        Key::Backslash => "\\",
        Key::Plus => "Plus",
        Key::Space => "Space",
        Key::Enter => "Enter",
        Key::Escape => "Escape",
        _ => key.name(),
    }
}

fn modifier_count(chord: KeyChord) -> usize {
    usize::from(chord.ctrl) + usize::from(chord.shift) + usize::from(chord.alt) + usize::from(chord.meta)
}

/// The canonical form: `Ctrl+Shift+Alt+Meta+Key`, and sequences without a `+`.
pub fn display_binding(b: Binding) -> String {
    match b {
        Binding::Sequence([a, z]) => format!("{}{}", key_name(a), key_name(z)),
        Binding::Sequence3([a, z, c]) => format!("{}{}{}", key_name(a), key_name(z), key_name(c)),
        Binding::Chord(c) => {
            let mut s = String::new();
            if c.ctrl {
                s.push_str("Ctrl+");
            }
            if c.shift {
                s.push_str("Shift+");
            }
            if c.alt {
                s.push_str("Alt+");
            }
            if c.meta {
                s.push_str("Meta+");
            }
            s.push_str(key_name(c.key));
            s
        }
    }
}

/// The canonical form with the Russian layout's letter after it, as `Ctrl+D (Ctrl+В)`.
pub fn display_binding_localized(b: Binding, lang: Lang) -> String {
    let canonical = display_binding(b);
    // The interface has no Russian catalog yet, so the hint is shown whenever it can help: the
    // canonical key stays first, and the ЙЦУКЕН letter follows in parentheses.
    let _ = lang;
    match b {
        Binding::Chord(c) => match cyrillic_of(c.key) {
            Some(letter) => {
                let mut hint = String::new();
                if c.ctrl {
                    hint.push_str("Ctrl+");
                }
                if c.shift {
                    hint.push_str("Shift+");
                }
                if c.alt {
                    hint.push_str("Alt+");
                }
                if c.meta {
                    hint.push_str("Meta+");
                }
                hint.push(letter.to_uppercase().next().unwrap_or(letter));
                format!("{canonical} ({hint})")
            }
            None => canonical,
        },
        Binding::Sequence([a, z]) => match (cyrillic_of(a), cyrillic_of(z)) {
            (Some(a), Some(z)) => format!("{canonical} ({}{})", a.to_uppercase().next().unwrap_or(a), z.to_uppercase().next().unwrap_or(z)),
            _ => canonical,
        },
        Binding::Sequence3([a, z, c]) => match (cyrillic_of(a), cyrillic_of(z), cyrillic_of(c)) {
            (Some(a), Some(z), Some(c)) => format!(
                "{canonical} ({}{}{})",
                a.to_uppercase().next().unwrap_or(a),
                z.to_uppercase().next().unwrap_or(z),
                c.to_uppercase().next().unwrap_or(c)
            ),
            _ => canonical,
        },
    }
}

/// A binding as a file writes it: `Ctrl+D`, `WA`, `Plus`.
pub fn parse_binding(text: &str) -> Result<Binding, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(t("An empty binding").to_string());
    }
    if text.contains('+') {
        let parts: Vec<&str> = text.split('+').map(str::trim).collect();
        if parts.iter().any(|p| p.is_empty()) {
            return Err(tf("Not a key: \"{}\"", &[text]).to_string());
        }
        let (key, mods) = parts.split_last().ok_or_else(|| t("An empty binding").to_string())?;
        let (mut ctrl, mut shift, mut alt, mut meta) = (false, false, false, false);
        for m in mods {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "primary" => ctrl = true,
                "shift" => shift = true,
                "alt" | "option" => alt = true,
                "meta" | "cmd" | "command" | "super" | "win" => meta = true,
                _ => return Err(tf("Unknown modifier: \"{}\"", &[m]).to_string()),
            }
        }
        let key = if key.chars().count() == 1 { key_of_char(key.chars().next().unwrap_or('\0')) } else { Key::from_name(key) }
            .ok_or_else(|| tf("Unknown key: \"{}\"", &[text]).to_string())?;
        if matches!(
            key,
            Key::ControlLeft
                | Key::ControlRight
                | Key::ShiftLeft
                | Key::ShiftRight
                | Key::AltLeft
                | Key::AltRight
                | Key::SuperLeft
                | Key::SuperRight
        ) {
            return Err(t("A modifier alone is not a binding").to_string());
        }
        return Ok(Binding::Chord(KeyChord::new(key, ctrl, shift, alt, meta)));
    }

    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let count = compact.chars().count();
    // A named key (`F2`, `Esc`, `PageUp`) wins over its length: only what is no key's name can
    // be a sequence.
    if (count == 2 || count == 3) && Key::from_name(&compact).is_none() {
        let keys: Option<Vec<Key>> = compact.chars().map(key_of_char).collect();
        if let Some(keys) = keys
            && keys.iter().all(|k| is_sequence_key(*k))
        {
            return match keys.as_slice() {
                [a, z] => Ok(Binding::Sequence([*a, *z])),
                [a, z, c] => Ok(Binding::Sequence3([*a, *z, *c])),
                _ => Err(tf("Unknown key: \"{}\"", &[text]).to_string()),
            };
        }
    }
    let key = if count == 1 { key_of_char(compact.chars().next().unwrap_or('\0')) } else { Key::from_name(&compact) }
        .ok_or_else(|| tf("Unknown key: \"{}\"", &[text]).to_string())?;
    if matches!(
        key,
        Key::ControlLeft
            | Key::ControlRight
            | Key::ShiftLeft
            | Key::ShiftRight
            | Key::AltLeft
            | Key::AltRight
            | Key::SuperLeft
            | Key::SuperRight
    ) {
        return Err(t("A modifier alone is not a binding").to_string());
    }
    if is_sequence_key(key) && modifier_count(KeyChord::new(key, false, false, false, false)) == 0 {
        return Err(tf("\"{}\" needs a modifier or a second key", &[text]).to_string());
    }
    Ok(Binding::Chord(KeyChord::new(key, false, false, false, false)))
}

pub(crate) fn is_sequence_key(key: Key) -> bool {
    let name = key.name();
    name.len() == 1 && name.as_bytes()[0].is_ascii_alphanumeric()
}

/// Whether a binding may be assigned: the reserved keys are refused, and a system shortcut is
/// accepted with a warning.
pub fn check_assignable(b: Binding) -> Result<Option<&'static str>, String> {
    if let Binding::Chord(c) = b {
        let bare = modifier_count(c) == 0;
        let reserved = matches!(
            c.key,
            Key::Escape
                | Key::Enter
                | Key::Tab
                | Key::Backspace
                | Key::Space
                | Key::ArrowUp
                | Key::ArrowDown
                | Key::ArrowLeft
                | Key::ArrowRight
        );
        if bare && reserved {
            return Err(tf("\"{}\" is reserved by the application", &[key_name(c.key)]).to_string());
        }
        if is_sequence_key(c.key) && bare {
            return Err(t("A letter or digit needs a modifier or a second key").to_string());
        }
        return Ok(system_warning(c));
    }
    Ok(None)
}

/// A warning for a chord the system may take before the window sees it.
pub fn system_warning(c: KeyChord) -> Option<&'static str> {
    let key = c.key;
    if c.alt && key == Key::F4 {
        return Some("The system may use this: Alt+F4 closes the window");
    }
    if c.alt && key == Key::Tab {
        return Some("The system may use this: Alt+Tab switches windows");
    }
    if c.ctrl && c.alt {
        return Some("AltGr may be taken by the system or the layout");
    }
    if c.ctrl && c.shift && key == Key::Escape {
        return Some("The system may use this: Ctrl+Shift+Esc opens the task manager");
    }
    if c.ctrl && key == Key::Escape {
        return Some("The system may use this: Ctrl+Esc opens the system menu");
    }
    if (c.ctrl || c.meta)
        && matches!(
            key,
            Key::F1 | Key::F2 | Key::F3 | Key::F4 | Key::F5 | Key::F6 | Key::F7 | Key::F8 | Key::F9 | Key::F10 | Key::F11 | Key::F12
        )
    {
        return Some("Some function keys are taken by the system or the window manager");
    }
    None
}

/// Whether a binding names a letter or digit alone, which sequences need to keep.
#[cfg(test)]
fn bare_letter(b: Binding) -> bool {
    matches!(b, Binding::Chord(c) if modifier_count(c) == 0 && is_sequence_key(c.key))
}

/// A profile's bindings, as action ids to canonical strings.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub version: u32,
    pub profile: Profile,
    /// Action id to canonical binding strings, in the order the action has them.
    pub bindings: Vec<(String, Vec<String>)>,
}

impl Config {
    pub fn defaults(profile: Profile) -> Config {
        Shortcuts::defaults(profile).to_config()
    }

    pub fn to_json(&self) -> Value {
        json!({
            "version": self.version,
            "profile": self.profile.code(),
            "bindings": self
                .bindings
                .iter()
                .map(|(action, keys)| json!({"action": action, "keys": keys}))
                .collect::<Vec<_>>(),
        })
    }

    /// Reads the v2 object; a legacy object (`{"WA": "Wall"}`) is migrated over `base`.
    pub fn from_json(v: &Value, base: &Config) -> Config {
        let Some(obj) = v.as_object() else { return base.clone() };
        if let Some(version) = obj.get("version").and_then(Value::as_u64)
            && version != 2
        {
            return base.clone();
        }
        if obj.get("version").and_then(Value::as_u64) == Some(2) {
            let profile = obj.get("profile").and_then(Value::as_str).and_then(Profile::from_code).unwrap_or(Profile::Custom);
            let mut bindings = Vec::new();
            if let Some(entries) = obj.get("bindings").and_then(Value::as_array) {
                for entry in entries.iter().take(crate::settings::MAX_ACTIONS) {
                    let Some(action) = entry.get("action").and_then(Value::as_str) else { continue };
                    let keys = entry
                        .get("keys")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).take(MAX_PER_ACTION).collect())
                        .unwrap_or_default();
                    bindings.push((action.to_string(), keys));
                }
            }
            return Config { version: 2, profile, bindings };
        }
        // The old format: key -> English action name. Each becomes a sequence over the base profile.
        if obj.is_empty() {
            return base.clone();
        }
        let mut migrated = Shortcuts::from_config(base);
        for (key, action) in obj.iter().take(crate::settings::MAX_SHORTCUTS) {
            let Some(action) = action.as_str() else { continue };
            let Some(id) = resolve_action(action) else { continue };
            let Ok(binding) = parse_binding(key) else { continue };
            if !matches!(binding, Binding::Sequence(_)) {
                continue;
            }
            migrated.remove_binding_everywhere(binding);
            let _ = migrated.add_binding(&id, binding, true);
        }
        migrated.profile = Profile::Custom;
        migrated.to_config()
    }
}

impl Default for Config {
    fn default() -> Config {
        Config::defaults(Profile::BimCraft)
    }
}

/// The profile the user is running.
#[derive(Clone, Debug)]
pub struct Shortcuts {
    pub profile: Profile,
    by_action: BTreeMap<ActionId, Vec<Binding>>,
    by_chord: HashMap<KeyChord, ActionId>,
    by_sequence: HashMap<[Key; 2], ActionId>,
    by_sequence3: HashMap<[Key; 3], ActionId>,
    sequence: Vec<Key>,
    sequence_at: Option<Instant>,
}

impl Default for Shortcuts {
    fn default() -> Self {
        Shortcuts::defaults(Profile::BimCraft)
    }
}

impl Shortcuts {
    /// The starting bindings of `profile`, from the registry's sequences and the profile's chords.
    pub fn defaults(profile: Profile) -> Shortcuts {
        let chords: &[(&str, &[&str])] = match profile {
            Profile::BimCraft | Profile::Archicad => DEFAULT_CHORDS,
            Profile::Revit => &DEFAULT_CHORDS[..10],
            Profile::Empty | Profile::Custom => &[],
        };
        let mut s = Shortcuts {
            profile,
            by_action: BTreeMap::new(),
            by_chord: HashMap::new(),
            by_sequence: HashMap::new(),
            by_sequence3: HashMap::new(),
            sequence: Vec::new(),
            sequence_at: None,
        };
        for a in registry().actions() {
            if !a.kind.bindable() {
                continue;
            }
            for spec in &a.defaults {
                if let Ok(b) = parse_binding(spec) {
                    let _ = s.add_binding(&a.id, b, true);
                }
            }
        }
        if profile != Profile::Empty && profile != Profile::Custom {
            for (action, specs) in chords {
                if let Some(id) = registry().resolve(action) {
                    for spec in *specs {
                        if let Ok(b) = parse_binding(spec) {
                            s.remove_binding_everywhere(b);
                            let _ = s.add_binding(&id, b, true);
                        }
                    }
                }
            }
        }
        if profile == Profile::Archicad {
            for (action, specs) in ARCHICAD_CHORDS {
                if let Some(id) = registry().resolve(action) {
                    for spec in *specs {
                        if let Ok(b) = parse_binding(spec) {
                            s.remove_binding_everywhere(b);
                            let _ = s.add_binding(&id, b, true);
                        }
                    }
                }
            }
        }
        s
    }

    /// A session with no bindings.
    pub fn empty(profile: Profile) -> Shortcuts {
        Shortcuts {
            profile,
            by_action: BTreeMap::new(),
            by_chord: HashMap::new(),
            by_sequence: HashMap::new(),
            by_sequence3: HashMap::new(),
            sequence: Vec::new(),
            sequence_at: None,
        }
    }

    pub fn bindings(&self, id: &str) -> &[Binding] {
        self.by_action.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn actions(&self) -> impl Iterator<Item = (&ActionId, &Vec<Binding>)> {
        self.by_action.iter()
    }

    pub fn action_for_chord(&self, chord: &KeyChord) -> Option<&ActionId> {
        self.by_chord.get(chord)
    }

    pub fn action_for_sequence(&self, keys: [Key; 2]) -> Option<&ActionId> {
        self.by_sequence.get(&keys)
    }

    /// The action a two- or three-character text names, for the family document and the tests.
    pub fn action_for_text(&self, text: &str) -> Option<ActionId> {
        let keys: Option<Vec<Key>> =
            text.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| key_of_char(c.to_ascii_uppercase())).collect();
        match keys?.as_slice() {
            [a, z] => self.by_sequence.get(&[*a, *z]).cloned(),
            [a, z, c] => self.by_sequence3.get(&[*a, *z, *c]).cloned(),
            _ => None,
        }
    }

    pub fn binding_owner(&self, b: Binding) -> Option<&ActionId> {
        match b {
            Binding::Chord(c) => self.by_chord.get(&c),
            Binding::Sequence(k) => self.by_sequence.get(&k),
            Binding::Sequence3(k) => self.by_sequence3.get(&k),
        }
    }

    fn insert_binding(&mut self, id: &ActionId, b: Binding) {
        match b {
            Binding::Chord(c) => {
                self.by_chord.insert(c, id.clone());
            }
            Binding::Sequence(k) => {
                self.by_sequence.insert(k, id.clone());
            }
            Binding::Sequence3(k) => {
                self.by_sequence3.insert(k, id.clone());
            }
        }
    }

    fn remove_binding_everywhere(&mut self, b: Binding) {
        match b {
            Binding::Chord(c) => {
                self.by_chord.remove(&c);
            }
            Binding::Sequence(k) => {
                self.by_sequence.remove(&k);
            }
            Binding::Sequence3(k) => {
                self.by_sequence3.remove(&k);
            }
        }
        for list in self.by_action.values_mut() {
            list.retain(|x| *x != b);
        }
    }

    /// Adds a binding to `id`. A binding another action has is refused unless `replace`, in which
    /// case it is taken from the other action.
    pub fn add_binding(&mut self, id: &str, b: Binding, replace: bool) -> Result<(), ActionId> {
        if let Some(owner) = self.binding_owner(b)
            && owner.as_ref() != id
        {
            if !replace {
                return Err(owner.clone());
            }
            let owner = owner.clone();
            if let Some(list) = self.by_action.get_mut(&owner) {
                list.retain(|x| *x != b);
            }
        }
        let id: ActionId = self.by_action.keys().find(|k| k.as_ref() == id).cloned().unwrap_or_else(|| Arc::from(id));
        let list = self.by_action.entry(id.clone()).or_default();
        if !list.contains(&b) {
            if list.len() >= MAX_PER_ACTION {
                return Err(Arc::from(""));
            }
            list.push(b);
        }
        self.insert_binding(&id, b);
        Ok(())
    }

    pub fn remove_binding(&mut self, id: &str, b: Binding) {
        if let Some(list) = self.by_action.get_mut(id) {
            list.retain(|x| *x != b);
        }
        if self.binding_owner(b).is_some_and(|owner| owner.as_ref() == id) {
            match b {
                Binding::Chord(c) => {
                    self.by_chord.remove(&c);
                }
                Binding::Sequence(k) => {
                    self.by_sequence.remove(&k);
                }
                Binding::Sequence3(k) => {
                    self.by_sequence3.remove(&k);
                }
            }
        }
    }

    /// Removes every binding of `id`; its defaults do not come back by themselves.
    pub fn remove_all(&mut self, id: &str) {
        let list = self.by_action.remove(id).unwrap_or_default();
        for b in list {
            if self.binding_owner(b).is_some_and(|owner| owner.as_ref() == id) {
                match b {
                    Binding::Chord(c) => {
                        self.by_chord.remove(&c);
                    }
                    Binding::Sequence(k) => {
                        self.by_sequence.remove(&k);
                    }
                    Binding::Sequence3(k) => {
                        self.by_sequence3.remove(&k);
                    }
                }
            }
        }
    }

    /// Gives `id` the bindings the active starting profile gives it.
    pub fn reset_action(&mut self, id: &str, profile: Profile) {
        self.remove_all(id);
        for b in defaults_for_action(profile, id) {
            let _ = self.add_binding(id, b, true);
        }
    }

    pub fn reset_all(&mut self, profile: Profile) {
        *self = Shortcuts::defaults(profile);
    }

    /// How `id`'s bindings compare with the starting profile's.
    pub fn source(&self, id: &str) -> &'static str {
        if registry().get(id).is_some_and(|def| !def.kind.bindable()) {
            return "cantchanged";
        }
        let defaults = defaults_for_action(self.profile, id);
        let now = self.bindings(id);
        if now.is_empty() {
            if defaults.is_empty() { "default" } else { "deleted" }
        } else if now == defaults.as_slice() {
            "default"
        } else if defaults.is_empty() {
            "custom"
        } else {
            "changed"
        }
    }

    pub fn to_config(&self) -> Config {
        Config {
            version: 2,
            profile: self.profile,
            bindings: self
                .by_action
                .iter()
                .filter(|(_, keys)| !keys.is_empty())
                .map(|(id, keys)| (id.to_string(), keys.iter().map(|b| display_binding(*b)).collect()))
                .collect(),
        }
    }

    pub fn from_config(config: &Config) -> Shortcuts {
        if config.version != 2 {
            return Shortcuts::defaults(Profile::BimCraft);
        }
        let mut s = Shortcuts::empty(config.profile);
        for (action, keys) in &config.bindings {
            if registry().get(action).is_none() {
                continue;
            }
            for key in keys.iter().take(MAX_PER_ACTION) {
                let Ok(b) = parse_binding(key) else { continue };
                let _ = s.add_binding(action, b, true);
            }
        }
        s
    }

    /// Starts a two-key sequence with `key`; a complete pair returns the action it names.
    pub fn push_sequence(&mut self, key: Key) -> Option<ActionId> {
        if self.sequence_at.is_some_and(|at| at.elapsed() > Duration::from_secs(2)) {
            self.sequence.clear();
        }
        self.sequence_at = Some(Instant::now());
        if self.sequence.len() >= 3 {
            self.sequence.clear();
        }
        self.sequence.push(key);
        match self.sequence.len() {
            2 => {
                let pair = [self.sequence[0], self.sequence[1]];
                if let Some(action) = self.by_sequence.get(&pair) {
                    let action = action.clone();
                    self.sequence.clear();
                    return Some(action);
                }
                // A three-key binding may still follow (`BMM`): keep the pair only when one starts so.
                let starts_three = self.by_sequence3.keys().any(|k| k[0] == pair[0] && k[1] == pair[1]);
                if !starts_three {
                    self.sequence.clear();
                }
                None
            }
            3 => {
                let keys = [self.sequence[0], self.sequence[1], self.sequence[2]];
                self.sequence.clear();
                self.by_sequence3.get(&keys).cloned()
            }
            _ => None,
        }
    }

    pub fn clear_sequence(&mut self) {
        self.sequence.clear();
        self.sequence_at = None;
    }

    /// The first `n` bindings, as the dialog, Find and tooltips show them.
    pub fn hints(&self, id: &str, n: usize) -> Vec<String> {
        let mut out: Vec<String> = self.bindings(id).iter().take(n).map(|b| display_binding(*b)).collect();
        let total = self.bindings(id).len();
        if total > n {
            out.push(format!("+{}", total - n));
        }
        out
    }
}

/// The starting bindings of one action under `profile`.
pub fn defaults_for_action(profile: Profile, id: &str) -> Vec<Binding> {
    let Some(def) = registry().get(id) else { return Vec::new() };
    if !def.kind.bindable() || profile == Profile::Empty || profile == Profile::Custom {
        return Vec::new();
    }
    let mut out: Vec<Binding> = def.defaults.iter().filter_map(|s| parse_binding(s).ok()).collect();
    let chord_sets: &[&[(&str, &[&str])]] = match profile {
        Profile::BimCraft => &[DEFAULT_CHORDS],
        Profile::Archicad => &[DEFAULT_CHORDS, ARCHICAD_CHORDS],
        Profile::Revit => &[&DEFAULT_CHORDS[..10]],
        Profile::Empty | Profile::Custom => &[],
    };
    for set in chord_sets {
        for (action, specs) in *set {
            if *action == id || registry().resolve(action).as_deref() == Some(id) {
                for spec in *specs {
                    if let Ok(b) = parse_binding(spec)
                        && !out.contains(&b)
                    {
                        out.push(b);
                    }
                }
            }
        }
    }
    out
}

/// The live bindings, for the ribbon tooltips that are drawn without the app at hand.
static LIVE: OnceLock<RwLock<Option<Shortcuts>>> = OnceLock::new();

fn live() -> &'static RwLock<Option<Shortcuts>> {
    LIVE.get_or_init(|| RwLock::new(None))
}

/// Publishes the window's bindings for the tooltips.
pub fn publish(shortcuts: &Shortcuts) {
    if let Ok(mut live) = live().write() {
        *live = Some(shortcuts.clone());
    }
}

/// A tooltip with the action's bindings after it: `Move (Ctrl+D, MV)`.
pub fn tip_with_shortcuts(label: &str, tip: &str) -> String {
    let keys = hint_for_label(label);
    if keys.is_empty() { tip.to_string() } else { format!("{tip} ({})", keys.join(", ")) }
}

/// The first bindings of the action an English label names, for a menu drawn without the app at hand.
pub fn hint_for_label(label: &str) -> Vec<String> {
    let def = registry().by_label(label).or_else(|| match label {
        "Shortcuts" => registry().get("ui.settings.shortcuts"),
        "Home" => registry().get("ui.view.home"),
        _ => None,
    });
    let Some(def) = def else { return Vec::new() };
    live().read().ok().and_then(|live| live.as_ref().map(|s| s.hints(&def.id, 2))).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// Files

/// What kind of export is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportMode {
    All,
    Changes,
}

/// One problem in a shortcuts file, with the line it was found on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseIssue {
    pub line: usize,
    pub message: String,
}

#[derive(Clone, Debug, Default)]
pub struct ParsedProfile {
    pub version: u32,
    pub profile: Option<Profile>,
    pub bindings: Vec<(ActionId, Binding)>,
    pub warnings: Vec<ParseIssue>,
    pub errors: Vec<ParseIssue>,
}

/// Reads a shortcuts file: the v2 text, old Revit XML lines, `KEY - Name`, or JSON.
pub fn parse_shortcuts(text: &str) -> ParsedProfile {
    let text = text.trim_start_matches('\u{feff}');
    let mut out = ParsedProfile::default();
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() > MAX_LINES {
        out.errors.push(ParseIssue { line: 1, message: t("The file has too many lines").to_string() });
        return out;
    }
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        return parse_json_file(&value, out);
    }
    let mut owner: HashMap<Binding, ActionId> = HashMap::new();
    let mut version_seen = false;
    for (index, raw) in lines.iter().enumerate() {
        let line_no = index + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.contains("<Shortcut") {
            let attr = |name: &str| {
                let pattern = format!("{name}=\"");
                line.split(pattern.as_str()).nth(1).and_then(|s| s.split('"').next())
            };
            if let (Some(cmd), Some(keys)) = (attr("CommandName").or_else(|| attr("CommandId")), attr("Shortcuts")) {
                let Some(id) = resolve_action(cmd) else {
                    out.errors.push(ParseIssue { line: line_no, message: tf("Unknown action: \"{}\"", &[cmd]) });
                    continue;
                };
                for key in keys.split('#') {
                    match parse_binding(key) {
                        Ok(b) if check_assignable(b).is_ok() => add_parsed(&mut out, &mut owner, id.clone(), b, line_no),
                        Ok(_) => out.errors.push(ParseIssue { line: line_no, message: tf("Reserved key: \"{}\"", &[key]) }),
                        Err(e) => out.errors.push(ParseIssue { line: line_no, message: e }),
                    }
                }
            }
            continue;
        }
        if let Some((name, value)) = line.split_once('=').map(|(a, b)| (a.trim(), b.trim()))
            && name.eq_ignore_ascii_case("version")
        {
            let version = value.parse::<u32>().unwrap_or(0);
            version_seen = true;
            out.version = version;
            if version != 2 {
                out.errors.push(ParseIssue { line: line_no, message: tf("Version {} is not supported", &[&version.to_string()]) });
                out.bindings.clear();
                return out;
            }
            continue;
        }
        if let Some((name, value)) = line.split_once('=').map(|(a, b)| (a.trim(), b.trim()))
            && name.eq_ignore_ascii_case("profile")
        {
            out.profile = Profile::from_code(value);
            continue;
        }
        // `action = binding`, `action: binding`, or the old `KEY - Name`.
        let split = line.find('=').map(|i| (i, 1)).or_else(|| line.find(':').map(|i| (i, 1))).or_else(|| line.find(" - ").map(|i| (i, 3)));
        let Some((at, width)) = split else {
            out.errors.push(ParseIssue { line: line_no, message: tf("Not a binding: \"{}\"", &[line]) });
            continue;
        };
        let (left, right) = (line[..at].trim(), line[at + width..].trim());
        let action = resolve_action(left);
        if let Some(id) = action {
            match parse_binding(right) {
                Ok(b) => match check_assignable(b) {
                    Ok(Some(warning)) => {
                        out.warnings.push(ParseIssue { line: line_no, message: warning.to_string() });
                        add_parsed(&mut out, &mut owner, id, b, line_no);
                    }
                    Ok(None) => add_parsed(&mut out, &mut owner, id, b, line_no),
                    Err(e) => out.errors.push(ParseIssue { line: line_no, message: e }),
                },
                Err(e) => out.errors.push(ParseIssue { line: line_no, message: e }),
            }
        } else if let (Ok(b), Some(id)) = (parse_binding(left), resolve_action(right)) {
            // The old line `WA = Wall`: left is the key, right the action.
            add_parsed(&mut out, &mut owner, id, b, line_no);
        } else if width == 3 {
            out.errors.push(ParseIssue { line: line_no, message: tf("Unknown action: \"{}\"", &[right]) });
        } else {
            out.errors.push(ParseIssue { line: line_no, message: tf("Unknown action: \"{}\"", &[left]) });
        }
    }
    if !version_seen && !out.bindings.is_empty() {
        out.version = 2;
    }
    out
}

fn add_parsed(out: &mut ParsedProfile, owner: &mut HashMap<Binding, ActionId>, id: ActionId, b: Binding, line: usize) {
    if let Some(other) = owner.get(&b)
        && other != &id
    {
        out.errors.push(ParseIssue { line, message: tf("\"{}\" is already assigned to \"{}\"", &[&display_binding(b), other]) });
        return;
    }
    if out.bindings.len() >= MAX_BINDINGS {
        out.errors.push(ParseIssue { line, message: t("The file has too many bindings").to_string() });
        return;
    }
    owner.insert(b, id.clone());
    out.bindings.push((id, b));
}

fn parse_json_file(value: &Value, mut out: ParsedProfile) -> ParsedProfile {
    if value.get("version").and_then(Value::as_u64) == Some(2) {
        out.version = 2;
        out.profile = value.get("profile").and_then(Value::as_str).and_then(Profile::from_code);
        let mut owner = HashMap::new();
        if let Some(entries) = value.get("bindings").and_then(Value::as_array) {
            for (i, entry) in entries.iter().enumerate() {
                let Some(action) = entry.get("action").and_then(Value::as_str) else { continue };
                let Some(id) = resolve_action(action) else {
                    out.errors.push(ParseIssue { line: i + 1, message: tf("Unknown action: \"{}\"", &[action]) });
                    continue;
                };
                for key in entry.get("keys").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                    match parse_binding(key) {
                        Ok(b) => add_parsed(&mut out, &mut owner, id.clone(), b, i + 1),
                        Err(e) => out.errors.push(ParseIssue { line: i + 1, message: e }),
                    }
                }
            }
        }
        return out;
    }
    if let Some(obj) = value.as_object() {
        let mut owner = HashMap::new();
        for (i, (key, action)) in obj.iter().enumerate() {
            let Some(action) = action.as_str() else { continue };
            let Some(id) = resolve_action(action) else { continue };
            if let Ok(b) = parse_binding(key) {
                add_parsed(&mut out, &mut owner, id, b, i + 1);
            }
        }
        out.version = 2;
        return out;
    }
    out.errors.push(ParseIssue { line: 1, message: t("Not a shortcuts file").to_string() });
    out
}

/// The v2 text: `<action> = <binding>`, one line per binding, actions by id.
pub fn serialize_shortcuts(shortcuts: &Shortcuts, mode: ExportMode) -> String {
    let mut text = String::from("# BIMCraft keyboard shortcuts\n# version: 2\nversion = 2\n");
    text.push_str(&format!("profile = {}\n\n", shortcuts.profile.code()));
    for (id, keys) in shortcuts.actions() {
        if keys.is_empty() {
            continue;
        }
        if mode == ExportMode::Changes && shortcuts.source(id) == "default" {
            continue;
        }
        let label = registry().get(id).map(|a| a.label).unwrap_or(id);
        text.push_str(&format!("# --- {} ({}) ---\n", t(label), id));
        for b in keys {
            text.push_str(&format!("{} = {}\n", id, display_binding(*b)));
        }
        text.push('\n');
    }
    text
}

/// Opens a file with whatever the system has associated with its kind: MarkText for `.md`, the
/// editor for `.txt`. A browser would show a local file poorly, so `ctx.open_url` is not used here.
fn open_path(path: &Path) -> Result<(), String> {
    use std::process::Command;
    #[cfg(target_os = "windows")]
    let spawned = Command::new("cmd").args(["/C", "start", ""]).arg(path).spawn();
    #[cfg(target_os = "macos")]
    let spawned = Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let spawned = Command::new("xdg-open").arg(path).spawn().or_else(|_| Command::new("gio").arg("open").arg(path).spawn());
    spawned.map(|_| ()).map_err(|e| e.to_string())
}

/// Writes a file through a temporary beside it, so a crash never leaves half of one.
pub fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let staged = path.with_extension(format!("{}.new", path.extension().and_then(|e| e.to_str()).unwrap_or("tmp")));
    std::fs::write(&staged, text).map_err(|e| e.to_string())?;
    std::fs::rename(&staged, path).map_err(|e| e.to_string())
}

/// Markdown help options.
#[derive(Clone, Debug)]
pub struct MarkdownOptions {
    pub lang: Lang,
    pub include_empty: bool,
    pub include_date: bool,
}

impl Default for MarkdownOptions {
    fn default() -> Self {
        MarkdownOptions { lang: Lang::English, include_empty: true, include_date: true }
    }
}

fn localized(lang: Lang, english: &str) -> String {
    translated_in(lang, english).unwrap_or(english).to_string()
}

fn escape_cell(text: &str) -> String {
    text.replace('|', "\\|")
}

/// The Markdown cheat sheet: every action, its live and starting bindings, commands without keys,
/// the reserved combinations, and any conflicts.
pub fn markdown(shortcuts: &Shortcuts, options: &MarkdownOptions) -> String {
    let lang = options.lang;
    let mut text = String::from("# BIMCraft — ");
    text.push_str(&localized(lang, "Keyboard Shortcuts"));
    text.push_str("\n\n");
    text.push_str(&format!(
        "{}: {} · {}: {} · {}: {}",
        localized(lang, "Profile"),
        shortcuts.profile.name(),
        localized(lang, "Version"),
        2,
        localized(lang, "Platform"),
        std::env::consts::OS
    ));
    text.push('\n');
    if options.include_date {
        let date = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        text.push_str(&format!("{}: {} (Unix time)\n", localized(lang, "Generated"), date));
    }
    text.push_str("\n## ");
    text.push_str(&localized(lang, "How to read"));
    text.push_str("\n\n- **");
    text.push_str(&localized(lang, "Active"));
    text.push_str("** — ");
    text.push_str(&localized(lang, "every combination that works now, including yours."));
    text.push_str("\n- `Ctrl+D` / `Ctrl+В` — ");
    text.push_str(&localized(lang, "one physical key; the canonical form is shown first."));
    text.push_str("\n\n## ");
    text.push_str(&localized(lang, "Contents"));
    text.push_str("\n\n");
    for c in Category::ALL {
        text.push_str(&format!("- [{}](#{})\n", localized(lang, c.name()), anchor(&localized(lang, c.name()))));
    }
    text.push_str(&format!(
        "- [{}](#{})\n",
        localized(lang, "Reserved and system combinations"),
        anchor(&localized(lang, "Reserved and system combinations"))
    ));
    text.push('\n');

    let mut actions: Vec<&ActionDef> = registry().actions().iter().collect();
    actions.sort_by(|a, b| a.category.cmp(&b.category).then_with(|| localized(lang, a.label).cmp(&localized(lang, b.label))));

    for category in Category::ALL {
        let rows: Vec<&&ActionDef> = actions.iter().filter(|a| a.category == category).collect();
        if rows.is_empty() {
            continue;
        }
        text.push_str(&format!("## {}\n\n", localized(lang, category.name())));
        text.push_str(&format!("| {} | {} |\n", localized(lang, "Command"), localized(lang, "Active")));
        text.push_str("|---|---|\n");
        for def in rows {
            if !options.include_empty && shortcuts.bindings(&def.id).is_empty() {
                continue;
            }
            let active = binding_list(shortcuts, &def.id, lang);
            text.push_str(&format!("| {} | {} |\n", escape_cell(&localized(lang, def.label)), active));
        }
        text.push('\n');
    }

    text.push_str(&format!("## {}\n\n", localized(lang, "Reserved and system combinations")));
    text.push_str(&format!("| {} | {} | {} |\n", localized(lang, "Combination"), localized(lang, "Status"), localized(lang, "Reason")));
    text.push_str("|---|---|---|\n");
    for (keys, status, why) in [
        ("Esc", "reserved by the application", "cancel, close"),
        ("Enter", "reserved by the application", "finish input"),
        ("Tab", "reserved by the application", "cycle or focus"),
        ("Backspace", "reserved by the application", "delete the last character or step"),
        ("Space", "reserved by the application", "the tool's contextual action"),
        ("Alt+Tab", "taken by the system", "switch windows"),
        ("Alt+F4", "taken by the system", "close the window"),
        ("Cmd+Tab", "taken by the system (macOS)", "switch applications"),
        ("Ctrl+Alt+…", "may be taken", "AltGr on some layouts"),
    ] {
        text.push_str(&format!("| `{}` | {} | {} |\n", keys, localized(lang, status), localized(lang, why)));
    }
    text.push('\n');
    text
}

fn anchor(text: &str) -> String {
    text.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect::<String>().trim_matches('-').to_string()
}

fn binding_list(shortcuts: &Shortcuts, id: &str, lang: Lang) -> String {
    binding_list_of(shortcuts.bindings(id), lang)
}

fn binding_list_of(bindings: &[Binding], lang: Lang) -> String {
    if bindings.is_empty() {
        return "—".to_string();
    }
    bindings.iter().map(|b| format!("`{}`", display_binding_localized(*b, lang))).collect::<Vec<_>>().join(", ")
}

fn source_name(source: &str) -> &'static str {
    match source {
        "custom" => "custom",
        "changed" => "changed",
        "deleted" => "deleted",
        "cantchanged" => "CantChanged",
        _ => "default",
    }
}

// ---------------------------------------------------------------------------------------------
// The dialog

/// What the dialog's filter shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    Assigned,
    Unassigned,
    Changed,
    Custom,
    CantChanged,
}

/// The column the table is sorted by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortColumn {
    Command,
    Category,
    Bindings,
    Source,
    Actions,
}

impl Filter {
    pub const ALL: [Filter; 6] = [Filter::All, Filter::Assigned, Filter::Unassigned, Filter::Changed, Filter::Custom, Filter::CantChanged];

    fn name(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Assigned => "Assigned",
            Filter::Unassigned => "Without keys",
            Filter::Changed => "Changed",
            Filter::Custom => "Custom",
            Filter::CantChanged => "CantChanged",
        }
    }
}

/// A binding being captured in the dialog.
#[derive(Clone, Debug)]
pub struct Capture {
    pub action: ActionId,
    pub current: Option<Binding>,
    pub keys: Vec<Key>,
    pub conflict: Option<(Binding, ActionId)>,
    pub error: Option<String>,
    pub warning: Option<String>,
}

/// A file read for import, waiting for the user's confirmation.
#[derive(Clone, Debug)]
pub struct ImportPreview {
    pub path: String,
    pub parsed: ParsedProfile,
    pub replace: bool,
    pub backup: Option<String>,
}

/// The Keyboard Shortcuts dialog.
pub struct ShortcutsDialog {
    pub search: String,
    pub filter: Filter,
    pub category: Option<Category>,
    pub sort: SortColumn,
    pub ascending: bool,
    pub profile_pick: Profile,
    pub pending: Shortcuts,
    pub original: Shortcuts,
    pub capture: Option<Capture>,
    pub import: Option<ImportPreview>,
    pub message: Option<String>,
    pub error: Option<String>,
    /// The file just written, shown as a link under the message.
    pub saved_path: Option<PathBuf>,
    pub confirm_discard: bool,
    pub confirm_reset_all: bool,
    pub show_help: bool,
}

impl Default for ShortcutsDialog {
    fn default() -> Self {
        let s = Shortcuts::default();
        ShortcutsDialog {
            search: String::new(),
            filter: Filter::All,
            category: None,
            sort: SortColumn::Category,
            ascending: true,
            profile_pick: s.profile,
            pending: s.clone(),
            original: s,
            capture: None,
            import: None,
            message: None,
            error: None,
            saved_path: None,
            confirm_discard: false,
            confirm_reset_all: false,
            show_help: false,
        }
    }
}

/// A column heading: its name on the left, and when it is the sorted column an arrow at its right.
/// A click on either part asks to sort by this column (or turn the order round). The returned
/// rectangle is the heading's cell, for the separator drawn between it and the next one.
fn sort_header(ui: &mut egui::Ui, width: f32, label: &str, on: bool, ascending: bool) -> (bool, egui::Rect) {
    let mut clicked = false;
    let cell = ui.allocate_ui_with_layout(vec2(width, 22.0), Layout::left_to_right(Align::Center), |ui| {
        ui.set_width(width);
        let text = if on { RichText::new(label).strong().color(pal().accent_ink) } else { RichText::new(label).strong() };
        if ui.add(egui::Button::new(text).frame(false)).clicked() {
            clicked = true;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if on
                && ui
                    .add(egui::Button::new(RichText::new(if ascending { "↑" } else { "↓" }).small().color(pal().accent_ink)).frame(false))
                    .on_hover_text(t(if ascending { "Ascending; click to reverse" } else { "Descending; click to reverse" }))
                    .clicked()
            {
                clicked = true;
            }
        });
    });
    (clicked, cell.response.rect)
}

/// One binding chip: clicking its text edits it, clicking the small `×` near its right edge
/// removes it. A read-only chip has no `×` and takes no clicks.
fn binding_chip(ui: &mut egui::Ui, text: &str, editable: bool) -> (bool, bool) {
    let font = egui::FontId::proportional(12.0);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font.clone(), pal().text);
    let pad = 12.0;
    let width = if editable { galley.size().x + 8.0 + pad * 2.0 } else { galley.size().x + 16.0 };
    let sense = if editable { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(width, 20.0), sense);
    let cross_x = rect.right() - pad;
    let pointer = ui.ctx().pointer_hover_pos();
    let over_cross = editable && response.hovered() && pointer.is_some_and(|p| p.x >= cross_x - 7.0 && p.x <= rect.right());
    let response = response.on_hover_text(t(if editable {
        if over_cross { "Remove this binding" } else { "Edit this binding" }
    } else {
        "This action cannot be changed"
    }));
    let fill = if editable && response.hovered() { pal().hover_fill } else { pal().surface };
    ui.painter().rect(rect, 5.0, fill, egui::Stroke::new(1.0, pal().border), egui::StrokeKind::Inside);
    ui.painter().text(egui::pos2(rect.left() + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, text, font, pal().text);
    if editable {
        let centre = egui::pos2(cross_x, rect.center().y);
        let arm = 3.0;
        let ink = if over_cross { pal().error } else { pal().muted };
        ui.painter().line_segment([centre + vec2(-arm, -arm), centre + vec2(arm, arm)], egui::Stroke::new(1.2, ink));
        ui.painter().line_segment([centre + vec2(-arm, arm), centre + vec2(arm, -arm)], egui::Stroke::new(1.2, ink));
    }
    let clicked = response.clicked();
    (editable && clicked && !over_cross, editable && clicked && over_cross)
}

impl BimApp {
    /// Project Browser (NP): shown, or hidden.
    pub(crate) fn toggle_browser(&mut self) {
        let p = crate::dock::Palette::Browser;
        self.show_palette(p, !self.docking.shown(p));
    }

    pub(crate) fn open_shortcuts(&mut self) {
        if !self.show_shortcuts {
            self.shortcuts_dialog =
                ShortcutsDialog { pending: self.shortcuts.clone(), original: self.shortcuts.clone(), ..ShortcutsDialog::default() };
            self.shortcuts_dialog.profile_pick = self.shortcuts.profile;
        }
        self.show_shortcuts = true;
    }

    fn apply_shortcuts(&mut self) {
        self.shortcuts = self.shortcuts_dialog.pending.clone();
        self.shortcuts_dialog.original = self.shortcuts.clone();
        self.shortcuts_dialog.message = Some(t("Keyboard shortcuts applied.").to_string());
        self.shortcuts_dialog.error = None;
        self.shortcuts_dialog.saved_path = None;
        publish(&self.shortcuts);
    }

    /// Executes an action by its id.
    pub(crate) fn execute_action(&mut self, ctx: &Context, id: &str) -> bool {
        let Some(def) = registry().get(id) else { return false };
        match id {
            "ui.file.save_as" => {
                self.save_as(ctx, None);
                true
            }
            "ui.file.export_pdf" => {
                self.export(ctx, "PDF", "pdf");
                true
            }
            "ui.settings" => {
                self.settings_page = Some(crate::settings::Page::Interface);
                true
            }
            "ui.assistant" => {
                self.toggle_assistant();
                true
            }
            "ui.settings.shortcuts" => {
                self.open_shortcuts();
                true
            }
            "ui.find" => {
                self.find = Some(String::new());
                true
            }
            "ui.view.home" => {
                self.toggle_home();
                true
            }
            "ui.view.alignment_guides" | "ui.view.orthogonal_guides" => {
                self.toggle_alignment_guides();
                true
            }
            "ui.view.plan" => {
                self.open_sheet(crate::Sheet::Plan(self.plan_key().unwrap_or_default()));
                true
            }
            "ui.title_blocks" => {
                self.open_title_blocks(None);
                true
            }
            _ if id.starts_with("cmd.") => self.execute_command(ctx, &id[4..]),
            _ => {
                let Some(legacy) = def.legacy else { return false };
                self.execute_shortcut_action(legacy)
            }
        }
    }

    fn execute_command(&mut self, ctx: &Context, id: &str) -> bool {
        match id {
            "new" => {
                self.guard(Pending::New, ctx);
                true
            }
            "open" => {
                self.guard(Pending::Open, ctx);
                true
            }
            "save" => {
                self.save(ctx, None);
                true
            }
            "undo" => {
                self.run("undo", json!({}));
                true
            }
            "redo" => {
                self.run("redo", json!({}));
                true
            }
            "reinforce" => {
                self.open_reinforce();
                true
            }
            "title_block" => {
                self.open_title_blocks(None);
                true
            }
            "graphic_display_options" => {
                self.open_display();
                true
            }
            "section_box" => {
                self.toggle_section_box();
                true
            }
            "column" | "structural_column" if self.tab == Tab::Structure || (self.tab == Tab::Context && self.back == Tab::Structure) => {
                self.set_tool(Tool::Stand("structuralcolumn"));
                true
            }
            "column" | "structural_column" => {
                self.set_tool(Tool::Column);
                true
            }
            _ => {
                let label = bimcraft_core::engine::find(id).map(|c| c.label);
                let Some(label) = label else { return false };
                if self.execute_shortcut_action(label) {
                    return true;
                }
                // Commands with no parameters and no dialog can run where a shortcut reaches
                // them. Anything else stays a grey row in the dialog rather than a surprise.
                false
            }
        }
    }

    /// Executes an action by its old English name; kept for callers during the migration.
    pub(crate) fn execute_shortcut_action(&mut self, action: &str) -> bool {
        match action {
            "Modify" => self.set_tool(Tool::Modify),
            "3D View" => self.open_3d(None),
            "Wall" => self.set_tool(Tool::Wall(Vec::new())),
            "Door" => self.set_tool(Tool::Door),
            "Window" => self.set_tool(Tool::Window),
            "Component" => self.set_tool(Tool::Component),
            "Room" => self.set_tool(Tool::Room),
            "Room Separator" => self.set_tool(Tool::RoomSeparator(None)),
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
            "Aligned Dimension" => self.set_tool(Tool::Dimension(crate::DimMode::Aligned, Vec::new())),
            "Text" => self.set_tool(Tool::Text),
            "Spot Elevation" => self.set_tool(Tool::Spot),
            "Spot Slope" => self.set_tool(Tool::SpotSlope),
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
            "Mirror" => self.set_tool(Tool::Mirror { draw: false, start: None }),
            "Zoom to Fit" => self.fit(),
            "Thin Lines" => self.thin = !self.thin,
            "Create Similar" => self.create_similar(),
            "Properties" => self.toggle_properties(),
            "Project Browser" => self.toggle_browser(),
            "Select All Instances" => self.select_all_instances(),
            "Keyboard Shortcuts" => self.open_shortcuts(),
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
            "View Templates" => self.open_templates(),
            "Phasing" => self.phasing_dialog = Some(phasing::PhasingDialog::default()),
            "Design Options" => self.options_dialog = Some(phasing::OptionsDialog::default()),
            "Section Box" => self.toggle_section_box(),
            "Graphic Display Options" => self.open_display(),
            "Linework" => self.set_tool(Tool::Linework),
            "Create Group" => self.create_group(),
            "Ungroup" => self.ungroup(),
            "Join into Polyline" => self.join_polyline(),
            "Explode Polyline" => self.explode_polyline(),
            "Detail Line" => self.set_tool(Tool::DetailLine(Vec::new())),
            "Filled Region" => {
                if let Some(view) = self.plan_key() {
                    self.start_sketch(crate::sketch::Of::Region { view, masking: false }, Vec::new());
                }
            }
            "Down Conductor" => self.set_tool(Tool::Run("downconductor", None)),
            "Earthing Conductor" => self.set_tool(Tool::Run("groundconductor", None)),
            "Cabling" => self.set_tool(Tool::Run("cabling", None)),
            "Reinforce" => self.open_reinforce(),
            "Reference Plane" => self.set_tool(Tool::ReferencePlane(None)),
            "Wireframe" => self.set_visual_style(bimcraft_core::graphics::VisualStyle::Wireframe),
            "Hidden Line" => self.set_visual_style(bimcraft_core::graphics::VisualStyle::HiddenLine),
            "Shaded" => self.set_visual_style(bimcraft_core::graphics::VisualStyle::Shaded),
            "Project Units" => {
                self.tab = Tab::Manage;
                self.open_units = true;
            }
            "Delete" => self.delete(),
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
            "Duct" => self.set_tool(Tool::Run("duct", None)),
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

    /// Renders the Keyboard Shortcuts dialog.
    pub(crate) fn shortcuts_window(&mut self, ctx: &Context) {
        if !self.show_shortcuts {
            return;
        }
        let mut close = false;
        let mut import_asked = false;
        let mut export_asked = false;
        let mut md_asked = false;
        let mut apply = false;
        let mut discard = false;
        let mut escape_closes = false;

        let r = Modal::new(UiId::new("shortcuts"), t("Keyboard Shortcuts")).width(1120.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add(TextEdit::singleline(&mut self.shortcuts_dialog.search).hint_text(t("Search…")).desired_width(170.0));
                    ComboBox::from_id_salt("shortcut-filter").selected_text(t(self.shortcuts_dialog.filter.name())).show_ui(ui, |ui| {
                        for f in Filter::ALL {
                            ui.selectable_value(&mut self.shortcuts_dialog.filter, f, t(f.name()));
                        }
                    });
                    let category =
                        self.shortcuts_dialog.category.map_or_else(|| t("All categories").to_string(), |c| t(c.name()).to_string());
                    ComboBox::from_id_salt("shortcut-category").selected_text(category).show_ui(ui, |ui| {
                        if ui.selectable_label(self.shortcuts_dialog.category.is_none(), t("All categories")).clicked() {
                            self.shortcuts_dialog.category = None;
                        }
                        for c in Category::ALL {
                            if ui.selectable_label(self.shortcuts_dialog.category == Some(c), t(c.name())).clicked() {
                                self.shortcuts_dialog.category = Some(c);
                            }
                        }
                    });
                });
                // Profile choice changes the whole set, not the filter: it sits apart from them.
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t("Profile")).strong());
                    ComboBox::from_id_salt("shortcut-profile").selected_text(self.shortcuts_dialog.profile_pick.name()).show_ui(ui, |ui| {
                        for p in Profile::CHOICES {
                            ui.selectable_value(&mut self.shortcuts_dialog.profile_pick, p, p.name());
                        }
                    });
                    if ui.button(t("Apply Profile")).clicked() {
                        self.shortcuts_dialog.pending = Shortcuts::defaults(self.shortcuts_dialog.profile_pick);
                        self.shortcuts_dialog.message =
                            Some(tf("Profile “{}” applied to the pending set.", &[self.shortcuts_dialog.profile_pick.name()]));
                    }
                });
                ui.add_space(2.0);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t("Import…")).clicked() {
                        import_asked = true;
                    }
                    if ui.button(t("Export…")).clicked() {
                        export_asked = true;
                    }
                    if ui.button(t("Help (Markdown)…")).clicked() {
                        md_asked = true;
                    }
                    if ui.button(t("Reset All")).clicked() {
                        self.shortcuts_dialog.confirm_reset_all = true;
                    }
                });

                if let Some(err) = &self.shortcuts_dialog.error {
                    ui.add_space(4.0);
                    ui.label(RichText::new(err).color(pal().error));
                } else if let Some(msg) = self.shortcuts_dialog.message.clone() {
                    ui.add_space(4.0);
                    ui.label(RichText::new(msg).color(pal().accent_ink));
                    if let Some(path) = self.shortcuts_dialog.saved_path.clone() {
                        ui.horizontal(|ui| {
                            let link = ui.link(path.display().to_string()).on_hover_text(t("Open the saved file"));
                            if link.clicked()
                                && let Err(e) = open_path(&path)
                            {
                                self.shortcuts_dialog.error = Some(tf("Could not open the file: {}", &[&e]));
                            }
                        });
                    }
                }

                if self.shortcuts_dialog.confirm_reset_all {
                    ui.group(|ui| {
                        ui.label(t("Return every action to the starting profile?"));
                        ui.horizontal(|ui| {
                            if ui.button(t("Reset")).clicked() {
                                self.shortcuts_dialog.pending = Shortcuts::defaults(self.shortcuts_dialog.profile_pick);
                                self.shortcuts_dialog.confirm_reset_all = false;
                            }
                            if ui.button(t("Cancel")).clicked() {
                                self.shortcuts_dialog.confirm_reset_all = false;
                            }
                        });
                    });
                }

                if self.shortcuts_dialog.confirm_discard {
                    ui.group(|ui| {
                        ui.label(t("Discard the changes made here?"));
                        ui.horizontal(|ui| {
                            if ui.button(t("Discard")).clicked() {
                                discard = true;
                            }
                            if ui.button(t("Keep Editing")).clicked() {
                                self.shortcuts_dialog.confirm_discard = false;
                            }
                        });
                    });
                }

                if let Some(capture) = self.shortcuts_dialog.capture.clone() {
                    escape_closes = self.capture_ui(ui, ctx, capture);
                }

                if let Some(preview) = self.shortcuts_dialog.import.clone() {
                    self.import_ui(ui, preview);
                }

                ui.add_space(6.0);
                let full = (ui.available_width() - 26.0).max(600.0);
                let (command_w, category_w, source_w, actions_w, gap) = (260.0, 95.0, 85.0, 170.0, 8.0);
                let bindings_w = (full - command_w - category_w - source_w - actions_w - gap * 4.0).max(220.0);
                let mut picked_sort: Option<SortColumn> = None;
                let mut edges: Vec<f32> = Vec::new();
                let header = ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    let (clicked, rect) = sort_header(
                        ui,
                        command_w,
                        t("Command"),
                        self.shortcuts_dialog.sort == SortColumn::Command,
                        self.shortcuts_dialog.ascending,
                    );
                    if clicked {
                        picked_sort = Some(SortColumn::Command);
                    }
                    edges.push(rect.right() + gap / 2.0);
                    let (clicked, rect) = sort_header(
                        ui,
                        category_w,
                        t("Category"),
                        self.shortcuts_dialog.sort == SortColumn::Category,
                        self.shortcuts_dialog.ascending,
                    );
                    if clicked {
                        picked_sort = Some(SortColumn::Category);
                    }
                    edges.push(rect.right() + gap / 2.0);
                    let (clicked, rect) = sort_header(
                        ui,
                        bindings_w,
                        t("Key combinations"),
                        self.shortcuts_dialog.sort == SortColumn::Bindings,
                        self.shortcuts_dialog.ascending,
                    );
                    if clicked {
                        picked_sort = Some(SortColumn::Bindings);
                    }
                    edges.push(rect.right() + gap / 2.0);
                    let (clicked, rect) = sort_header(
                        ui,
                        source_w,
                        t("Source"),
                        self.shortcuts_dialog.sort == SortColumn::Source,
                        self.shortcuts_dialog.ascending,
                    );
                    if clicked {
                        picked_sort = Some(SortColumn::Source);
                    }
                    edges.push(rect.right() + gap / 2.0);
                    let (clicked, rect) = sort_header(
                        ui,
                        actions_w,
                        t("Actions"),
                        self.shortcuts_dialog.sort == SortColumn::Actions,
                        self.shortcuts_dialog.ascending,
                    );
                    if clicked {
                        picked_sort = Some(SortColumn::Actions);
                    }
                    edges.push(rect.right() + gap / 2.0);
                });
                let header_rect = header.response.rect;
                for x in edges.iter().take(4) {
                    ui.painter().line_segment(
                        [egui::pos2(*x, header_rect.top() + 2.0), egui::pos2(*x, header_rect.bottom() - 2.0)],
                        egui::Stroke::new(1.0, pal().border),
                    );
                }
                if let Some(column) = picked_sort {
                    if self.shortcuts_dialog.sort == column {
                        self.shortcuts_dialog.ascending = !self.shortcuts_dialog.ascending;
                    } else {
                        self.shortcuts_dialog.sort = column;
                        self.shortcuts_dialog.ascending = true;
                    }
                }
                ui.separator();
                let mut reset_action: Option<ActionId> = None;
                let mut remove_action: Option<ActionId> = None;
                let mut edit_binding: Option<(ActionId, Binding)> = None;
                let mut start_capture: Option<ActionId> = None;
                let term = self.shortcuts_dialog.search.trim().to_lowercase();
                let mut rows: Vec<&ActionDef> = registry()
                    .actions()
                    .iter()
                    .filter(|a| {
                        if let Some(c) = self.shortcuts_dialog.category
                            && a.category != c
                        {
                            return false;
                        }
                        let keys = self.shortcuts_dialog.pending.bindings(&a.id);
                        // A command that cannot be bound and has no binding is noise: it is not
                        // listed at all; CantChanged keeps only the ones that carry bindings.
                        if !a.kind.bindable() && keys.is_empty() {
                            return false;
                        }
                        match self.shortcuts_dialog.filter {
                            Filter::All => {}
                            Filter::Assigned if keys.is_empty() => return false,
                            Filter::Unassigned if !keys.is_empty() => return false,
                            Filter::Changed if self.shortcuts_dialog.pending.source(&a.id) != "changed" => return false,
                            Filter::Custom if self.shortcuts_dialog.pending.source(&a.id) != "custom" => return false,
                            Filter::CantChanged if self.shortcuts_dialog.pending.source(&a.id) != "cantchanged" => return false,
                            _ => {}
                        }
                        if term.is_empty() {
                            return true;
                        }
                        let label = t(a.label).to_lowercase();
                        a.label.to_lowercase().contains(&term)
                            || label.contains(&term)
                            || a.id.to_lowercase().contains(&term)
                            || keys.iter().any(|b| display_binding(*b).to_lowercase().contains(&term))
                    })
                    .collect();
                let sort = self.shortcuts_dialog.sort;
                let ascending = self.shortcuts_dialog.ascending;
                let pending = &self.shortcuts_dialog.pending;
                let binding_text =
                    |id: &str| pending.bindings(id).iter().map(|b| display_binding(*b)).collect::<Vec<_>>().join(", ").to_lowercase();
                rows.sort_by(|a, b| {
                    let ord = match sort {
                        SortColumn::Command => t(a.label).to_lowercase().cmp(&t(b.label).to_lowercase()),
                        SortColumn::Category => t(a.category.name()).to_lowercase().cmp(&t(b.category.name()).to_lowercase()),
                        SortColumn::Bindings => binding_text(&a.id).cmp(&binding_text(&b.id)),
                        SortColumn::Source => source_name(pending.source(&a.id)).cmp(source_name(pending.source(&b.id))),
                        SortColumn::Actions => (!a.kind.bindable()).cmp(&(!b.kind.bindable())),
                    };
                    let ord = if ascending { ord } else { ord.reverse() };
                    if ord == std::cmp::Ordering::Equal { t(a.label).to_lowercase().cmp(&t(b.label).to_lowercase()) } else { ord }
                });
                ScrollArea::vertical()
                    .id_salt("shortcuts-rows")
                    .auto_shrink([false, false])
                    .max_height(430.0)
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for def in rows {
                            let keys = self.shortcuts_dialog.pending.bindings(&def.id).to_vec();
                            let enabled = def.kind.bindable();
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                // The name wraps inside its fixed column: a long command never widens the row.
                                ui.allocate_ui_with_layout(vec2(command_w, 0.0), Layout::top_down(Align::Min), |ui| {
                                    ui.set_width(command_w);
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    ui.add(egui::Label::new(RichText::new(t(def.label)).strong()).wrap()).on_hover_text(def.id.as_ref());
                                });
                                ui.allocate_ui_with_layout(vec2(category_w, 0.0), Layout::top_down(Align::Min), |ui| {
                                    ui.set_width(category_w);
                                    ui.add(egui::Label::new(t(def.category.name())).wrap());
                                });
                                ui.allocate_ui_with_layout(vec2(bindings_w, 0.0), Layout::top_down(Align::Min), |ui| {
                                    ui.set_width(bindings_w);
                                    ui.horizontal_wrapped(|ui| {
                                        for b in &keys {
                                            let text = display_binding_localized(*b, self.lang);
                                            let (edit, remove) = binding_chip(ui, &text, enabled);
                                            if edit {
                                                edit_binding = Some((def.id.clone(), *b));
                                            }
                                            if remove {
                                                self.shortcuts_dialog.pending.remove_binding(&def.id, *b);
                                            }
                                        }
                                        if enabled && ui.small_button("+").on_hover_text(t("Add a binding")).clicked() {
                                            start_capture = Some(def.id.clone());
                                        }
                                    });
                                });
                                let source = self.shortcuts_dialog.pending.source(&def.id);
                                ui.allocate_ui_with_layout(vec2(source_w, 0.0), Layout::top_down(Align::Min), |ui| {
                                    ui.set_width(source_w);
                                    ui.add(
                                        egui::Label::new(RichText::new(t(source_name(source))).color(if source == "default" {
                                            pal().muted
                                        } else {
                                            pal().accent_ink
                                        }))
                                        .wrap(),
                                    );
                                });
                                ui.allocate_ui_with_layout(vec2(actions_w, 0.0), Layout::top_down(Align::Min), |ui| {
                                    ui.set_width(actions_w);
                                    ui.horizontal_wrapped(|ui| {
                                        if enabled && ui.small_button(t("Reset")).clicked() {
                                            reset_action = Some(def.id.clone());
                                        }
                                        if enabled && !keys.is_empty() && ui.small_button(t("Remove All")).clicked() {
                                            remove_action = Some(def.id.clone());
                                        }
                                    });
                                });
                            });
                            ui.add(egui::Separator::default().spacing(2.0));
                        }
                    });

                if let Some(id) = reset_action {
                    self.shortcuts_dialog.pending.reset_action(&id, self.shortcuts_dialog.pending.profile);
                }
                if let Some(id) = remove_action {
                    self.shortcuts_dialog.pending.remove_all(&id);
                }
                if let Some((id, b)) = edit_binding {
                    self.shortcuts_dialog.capture =
                        Some(Capture { action: id, current: Some(b), keys: Vec::new(), conflict: None, error: None, warning: None });
                }
                if let Some(id) = start_capture {
                    self.shortcuts_dialog.capture =
                        Some(Capture { action: id, current: None, keys: Vec::new(), conflict: None, error: None, warning: None });
                }

                ui.add_space(4.0);
                let assigned = registry().actions().iter().filter(|a| !self.shortcuts_dialog.pending.bindings(&a.id).is_empty()).count();
                ui.label(
                    RichText::new(tf("Assigned {} of {}", &[&assigned.to_string(), &registry().actions().len().to_string()]))
                        .color(pal().muted),
                );
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    if ui.button(t("Apply")).clicked() {
                        apply = true;
                    }
                    if ui.button(t("Cancel")).clicked() {
                        discard = true;
                    }
                    close = ui.button(t("Close")).clicked();
                });
            });
        });

        if import_asked {
            let dialog =
                rfd::AsyncFileDialog::new().add_filter("Shortcut files (*.bks, *.txt, *.json, *.xml)", &["bks", "txt", "json", "xml"]);
            self.ask_file(ctx, dialog, false, |app, path| app.preview_shortcuts_file(&path));
        }
        if export_asked {
            let dialog = rfd::AsyncFileDialog::new().set_file_name("bimcraft-shortcuts.txt");
            self.ask_file(ctx, dialog, true, |app, path| app.export_shortcuts_file(&path));
        }
        if md_asked {
            let dialog = rfd::AsyncFileDialog::new().set_file_name("bimcraft-shortcuts.md").add_filter("Markdown (*.md)", &["md"]);
            self.ask_file(ctx, dialog, true, |app, path| app.export_shortcuts_markdown(&path));
        }
        if apply {
            self.apply_shortcuts();
        }
        if escape_closes {
            self.show_shortcuts = false;
            self.shortcuts_dialog.pending = self.shortcuts.clone();
            self.shortcuts_dialog.capture = None;
            self.shortcuts_dialog.import = None;
            self.shortcuts_dialog.confirm_discard = false;
            self.shortcuts_dialog.confirm_reset_all = false;
        } else if (discard || close || r.should_close()) && !self.shortcuts_dialog.confirm_discard {
            let changed = self.shortcuts_dialog.pending.to_config() != self.shortcuts_dialog.original.to_config();
            if changed {
                self.shortcuts_dialog.confirm_discard = true;
            } else {
                self.show_shortcuts = false;
                self.shortcuts_dialog.capture = None;
                self.shortcuts_dialog.import = None;
            }
        } else if discard && self.shortcuts_dialog.confirm_discard {
            self.show_shortcuts = false;
            self.shortcuts_dialog.pending = self.shortcuts.clone();
            self.shortcuts_dialog.capture = None;
            self.shortcuts_dialog.import = None;
            self.shortcuts_dialog.confirm_discard = false;
        }
    }

    fn capture_ui(&mut self, ui: &mut egui::Ui, ctx: &Context, mut capture: Capture) -> bool {
        let events: Vec<Event> = ctx.input(|i| i.events.clone());
        let mut handled: Vec<(Key, Modifiers, bool)> = Vec::new();
        for event in &events {
            if let Event::Key { key, physical_key, pressed: true, repeat: false, modifiers, .. } = event {
                if matches!(
                    key,
                    Key::ControlLeft
                        | Key::ControlRight
                        | Key::ShiftLeft
                        | Key::ShiftRight
                        | Key::AltLeft
                        | Key::AltRight
                        | Key::SuperLeft
                        | Key::SuperRight
                ) {
                    continue;
                }
                let physical = physical_key.unwrap_or(*key);
                handled.push((*key, *modifiers, physical_key.is_some()));
                if *key == Key::Escape {
                    self.shortcuts_dialog.capture = None;
                    return true;
                }
                if *key == Key::Backspace {
                    capture.keys.clear();
                    capture.conflict = None;
                    capture.error = None;
                    continue;
                }
                if matches!(*key, Key::Tab | Key::Enter) {
                    continue;
                }
                let has_command = modifiers.ctrl || modifiers.alt || modifiers.command || modifiers.mac_cmd;
                if has_command || (is_sequence_key(physical) && modifiers.shift) {
                    let chord = KeyChord::from_event(physical, *modifiers);
                    self.finish_capture(&mut capture, Binding::Chord(chord));
                } else if is_sequence_key(physical) {
                    capture.keys.push(physical);
                    if capture.keys.len() == 2 {
                        let pair = [capture.keys[0], capture.keys[1]];
                        self.finish_capture(&mut capture, Binding::Sequence(pair));
                    }
                } else {
                    let chord = KeyChord::from_event(physical, *modifiers);
                    self.finish_capture(&mut capture, Binding::Chord(chord));
                }
                if self.shortcuts_dialog.capture.is_none() {
                    break;
                }
            }
        }
        if !handled.is_empty() {
            ctx.input_mut(|i| {
                i.events.retain(|e| {
                    !matches!(e, Event::Key { key, pressed: true, repeat: false, modifiers, .. }
                        if handled.iter().any(|(k, m, _)| k == key && m == modifiers))
                })
            });
        }
        // The capture's error, warning or conflict is drawn from the dialog's copy.
        if self.shortcuts_dialog.capture.is_some() {
            self.shortcuts_dialog.capture = Some(capture);
        }
        if let Some(capture) = self.shortcuts_dialog.capture.clone() {
            ui.group(|ui| {
                ui.label(RichText::new(t("Press a combination…")).strong());
                ui.label(t("A chord such as Ctrl+D, or two keys such as WA. Esc cancels; Backspace clears."));
                let shown = if capture.keys.is_empty() { capture.current.map(|b| display_binding_localized(b, self.lang)) } else { None };
                if let Some(shown) = shown {
                    ui.label(RichText::new(shown).monospace());
                }
                if !capture.keys.is_empty() {
                    ui.label(RichText::new(capture.keys.iter().map(|k| key_name(*k)).collect::<String>()).monospace());
                }
                if let Some(err) = &capture.error {
                    ui.label(RichText::new(err).color(pal().error));
                }
                if let Some(warn) = &capture.warning {
                    ui.label(RichText::new(warn).color(pal().accent_ink));
                }
                if let Some((binding, owner)) = &capture.conflict {
                    ui.label(
                        RichText::new(tf(
                            "“{}” is already assigned to “{}”.",
                            &[&display_binding(*binding), t(registry().get(owner).map(|a| a.label).unwrap_or("?"))],
                        ))
                        .color(pal().error),
                    );
                    ui.horizontal(|ui| {
                        if ui.button(t("Replace")).clicked() {
                            self.shortcuts_dialog.pending.remove_binding(owner, *binding);
                            if let Some(current) = capture.current {
                                self.shortcuts_dialog.pending.remove_binding(&capture.action, current);
                            }
                            let _ = self.shortcuts_dialog.pending.add_binding(&capture.action, *binding, true);
                            self.shortcuts_dialog.capture = None;
                        }
                        if ui.button(t("Cancel")).clicked() {
                            self.shortcuts_dialog.capture = None;
                        }
                    });
                } else {
                    ui.horizontal(|ui| {
                        if ui.button(t("Cancel")).clicked() {
                            self.shortcuts_dialog.capture = None;
                        }
                    });
                }
            });
        }
        false
    }

    fn finish_capture(&mut self, capture: &mut Capture, binding: Binding) {
        match check_assignable(binding) {
            Err(e) => capture.error = Some(e),
            Ok(warning) => {
                if let Some(owner) = self.shortcuts_dialog.pending.binding_owner(binding).cloned()
                    && owner != capture.action
                {
                    capture.conflict = Some((binding, owner));
                    capture.warning = warning.map(str::to_string);
                    return;
                }
                if let Some(current) = capture.current {
                    self.shortcuts_dialog.pending.remove_binding(&capture.action, current);
                }
                let _ = self.shortcuts_dialog.pending.add_binding(&capture.action, binding, true);
                self.shortcuts_dialog.message = Some(tf(
                    "“{}” assigned to “{}”.",
                    &[&display_binding(binding), t(registry().get(&capture.action).map(|a| a.label).unwrap_or("?"))],
                ));
                self.shortcuts_dialog.capture = None;
            }
        }
    }

    fn import_ui(&mut self, ui: &mut egui::Ui, preview: ImportPreview) {
        ui.group(|ui| {
            ui.label(RichText::new(tf("Import {}", &[&preview.path])).strong());
            ui.label(tf(
                "{} bindings read; {} errors.",
                &[&preview.parsed.bindings.len().to_string(), &preview.parsed.errors.len().to_string()],
            ));
            ui.horizontal(|ui| {
                let mut replace = preview.replace;
                ui.radio_value(&mut replace, true, t("Replace everything"));
                ui.radio_value(&mut replace, false, t("Add to the current set"));
                if replace != preview.replace
                    && let Some(p) = &mut self.shortcuts_dialog.import
                {
                    p.replace = replace;
                }
            });
            for issue in preview.parsed.errors.iter().take(50) {
                ui.label(RichText::new(format!("{}: {}", issue.line, issue.message)).color(pal().error).small());
            }
            if preview.parsed.errors.len() > 50 {
                ui.label(RichText::new(tf("…and {} more", &[&(preview.parsed.errors.len() - 50).to_string()])).small());
            }
            ui.horizontal(|ui| {
                if ui.button(t("Apply Import")).clicked() {
                    let preview = self.shortcuts_dialog.import.clone();
                    if let Some(preview) = preview {
                        let backup = self.backup_shortcuts();
                        let mut set = if preview.replace {
                            Shortcuts::empty(preview.parsed.profile.unwrap_or(Profile::Custom))
                        } else {
                            self.shortcuts_dialog.pending.clone()
                        };
                        let mut skipped = 0;
                        for (id, b) in &preview.parsed.bindings {
                            if set.add_binding(id, *b, preview.replace).is_err() {
                                skipped += 1;
                            }
                        }
                        self.shortcuts_dialog.pending = set;
                        self.shortcuts_dialog.import = None;
                        self.shortcuts_dialog.error = None;
                        self.shortcuts_dialog.message = Some(tf(
                            "Imported {} bindings ({} skipped).",
                            &[&preview.parsed.bindings.len().to_string(), &skipped.to_string()],
                        ));
                        if let Some(path) = backup {
                            self.shortcuts_dialog.message = Some(tf("Imported; the old profile is in {}.", &[&path]));
                        }
                    }
                }
                if ui.button(t("Cancel")).clicked() {
                    self.shortcuts_dialog.import = None;
                }
            });
        });
    }

    fn backup_shortcuts(&self) -> Option<String> {
        let path = self.settings_path.as_ref()?;
        let text = std::fs::read_to_string(path).ok()?;
        let backup = path.with_file_name("shortcuts.bak");
        std::fs::write(&backup, text).ok()?;
        Some(backup.display().to_string())
    }

    fn preview_shortcuts_file(&mut self, path: &Path) {
        self.shortcuts_dialog.saved_path = None;
        if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("md")) {
            self.shortcuts_dialog.error = Some(t("This is a help file to read. Use .bks, .txt or .json to import.").to_string());
            self.shortcuts_dialog.import = None;
            return;
        }
        let text = match std::fs::metadata(path) {
            Ok(m) if m.is_file() && m.len() <= MAX_FILE => std::fs::read_to_string(path).map_err(|e| e.to_string()),
            Ok(_) => Err(t("That is too big to be a shortcuts file.").to_string()),
            Err(e) => Err(e.to_string()),
        };
        match text {
            Err(e) => {
                self.shortcuts_dialog.error = Some(e);
                self.shortcuts_dialog.import = None;
            }
            Ok(text) => {
                let parsed = parse_shortcuts(&text);
                if parsed.bindings.is_empty() && !parsed.errors.is_empty() {
                    self.shortcuts_dialog.error = Some(t("The file has no usable bindings.").to_string());
                    self.shortcuts_dialog.import = None;
                    return;
                }
                self.shortcuts_dialog.error = None;
                self.shortcuts_dialog.import =
                    Some(ImportPreview { path: path.display().to_string(), parsed, replace: true, backup: None });
            }
        }
    }

    fn export_shortcuts_file(&mut self, path: &Path) {
        self.shortcuts_dialog.saved_path = None;
        let set = if self.show_shortcuts { self.shortcuts_dialog.pending.clone() } else { self.shortcuts.clone() };
        let text = match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase) {
            Some(e) if e == "md" => {
                self.shortcuts_dialog.error = Some(t("Markdown is a help file; use Help (Markdown)… to write it.").to_string());
                return;
            }
            Some(e) if e == "xml" => {
                self.shortcuts_dialog.error = Some(t("Revit XML import is supported; export it from Revit.").to_string());
                return;
            }
            _ => serialize_shortcuts(&set, ExportMode::All),
        };
        match write_atomic(path, &text) {
            Ok(()) => {
                self.shortcuts_dialog.error = None;
                self.shortcuts_dialog.message = Some(t("Exported.").to_string());
                self.shortcuts_dialog.saved_path = Some(path.to_path_buf());
            }
            Err(e) => self.shortcuts_dialog.error = Some(tf("Could not write the file: {}", &[&e])),
        }
    }

    fn export_shortcuts_markdown(&mut self, path: &Path) {
        self.shortcuts_dialog.saved_path = None;
        let options = MarkdownOptions { lang: self.lang, include_empty: true, include_date: true };
        let set = if self.show_shortcuts { self.shortcuts_dialog.pending.clone() } else { self.shortcuts.clone() };
        let text = markdown(&set, &options);
        match write_atomic(path, &text) {
            Ok(()) => {
                self.shortcuts_dialog.error = None;
                self.shortcuts_dialog.message = Some(t("Markdown help written.").to_string());
                self.shortcuts_dialog.saved_path = Some(path.to_path_buf());
            }
            Err(e) => self.shortcuts_dialog.error = Some(tf("Could not write the file: {}", &[&e])),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use bimcraft_core::engine::Session;

    #[test]
    fn chords_and_sequences_parse_in_the_canonical_form() {
        let ctrl_d = Binding::Chord(KeyChord::new(Key::D, true, false, false, false));
        assert_eq!(parse_binding("BMM"), Ok(Binding::Sequence3([Key::B, Key::M, Key::M])));
        assert_eq!(display_binding(Binding::Sequence3([Key::B, Key::M, Key::M])), "BMM");
        assert_eq!(parse_binding("Ctrl+D"), Ok(ctrl_d));
        assert_eq!(parse_binding("Ctrl+В"), Ok(ctrl_d));
        assert_eq!(parse_binding("Primary+d"), Ok(ctrl_d));
        assert_ne!(parse_binding("Ctrl+Shift+D").unwrap(), ctrl_d);
        assert_eq!(parse_binding("WA"), Ok(Binding::Sequence([Key::W, Key::A])));
        assert_eq!(parse_binding("WА"), Ok(Binding::Sequence([Key::W, Key::A])));
        assert_eq!(parse_binding("F2"), Ok(Binding::Chord(KeyChord::new(Key::F2, false, false, false, false))));
        assert!(parse_binding("Ctrl++").is_err());
        assert!(parse_binding("Ctrl+").is_err());
        assert!(parse_binding("Ctrl+№").is_err());
        assert!(parse_binding("D").is_err());
        assert_eq!(display_binding(ctrl_d), "Ctrl+D");
        assert_eq!(display_binding_localized(ctrl_d, Lang::English), "Ctrl+D (Ctrl+В)");
        assert_eq!(display_binding(Binding::Sequence([Key::W, Key::A])), "WA");
    }

    #[test]
    fn every_key_has_a_name_that_does_not_panic() {
        for key in Key::ALL {
            let _ = display_binding(Binding::Chord(KeyChord::new(*key, true, false, false, false)));
        }
    }

    #[test]
    fn reserved_keys_are_refused_and_system_keys_warn() {
        assert!(check_assignable(parse_binding("Esc").unwrap()).is_err());
        assert!(check_assignable(parse_binding("Space").unwrap()).is_err());
        assert!(check_assignable(parse_binding("Delete").unwrap()).is_ok());
        assert!(check_assignable(parse_binding("Alt+F4").unwrap()).is_ok());
        assert!(system_warning(parse_binding("Alt+F4").unwrap().chord().unwrap()).is_some());
        assert!(system_warning(parse_binding("Ctrl+Alt+E").unwrap().chord().unwrap()).is_some());
    }

    impl Binding {
        fn chord(self) -> Option<KeyChord> {
            match self {
                Binding::Chord(c) => Some(c),
                Binding::Sequence(_) | Binding::Sequence3(_) => None,
            }
        }
    }

    #[test]
    fn the_registry_covers_every_command_and_every_old_action() {
        let mut ids = HashSet::new();
        for a in registry().actions() {
            assert!(ids.insert(a.id.to_string()), "duplicate id {}", a.id);
        }
        for c in bimcraft_core::engine::commands() {
            assert!(registry().get(&format!("cmd.{}", c.id)).is_some(), "missing cmd.{}", c.id);
        }
        for label in ACTIONS {
            assert!(registry().by_label(label).is_some(), "missing action {label}");
        }
    }

    #[test]
    fn every_core_command_keeps_its_own_label_and_conduit_is_not_conduit_fill() {
        for c in bimcraft_core::engine::commands() {
            let action = registry().get(&format!("cmd.{}", c.id)).unwrap();
            assert_eq!(action.label, c.label, "cmd.{}", c.id);
        }
        let s = Shortcuts::defaults(Profile::BimCraft);
        assert_ne!(s.source("cmd.conduit"), "cantchanged", "Conduit is a tool");
        assert_eq!(s.source("cmd.conduit_fill"), "cantchanged", "Conduit Fill needs parameters");
    }

    #[test]
    fn the_bimcraft_profile_has_home_on_ctrl_u_ctrl_d_free_and_f8_for_the_guides() {
        let s = Shortcuts::defaults(Profile::BimCraft);
        assert_eq!(s.binding_owner(parse_binding("Ctrl+U").unwrap()).map(|s| s.to_string()), Some("ui.view.home".to_string()));
        assert!(s.binding_owner(parse_binding("Ctrl+D").unwrap()).is_none());
        let guides = registry().resolve("ui.view.alignment_guides").unwrap();
        assert!(s.bindings(&guides).contains(&parse_binding("F8").unwrap()));
        assert!(!s.bindings(&guides).contains(&parse_binding("F11").unwrap()));
        let archicad = Shortcuts::defaults(Profile::Archicad);
        assert_eq!(archicad.binding_owner(parse_binding("Ctrl+D").unwrap()).map(|s| s.to_string()), Some("cmd.move".to_string()));
    }

    #[test]
    fn an_action_keeps_several_bindings_and_one_binding_has_one_owner() {
        let mut s = Shortcuts::empty(Profile::Custom);
        let move_id = registry().resolve("cmd.move").unwrap();
        let copy_id = registry().resolve("cmd.copy").unwrap();
        s.add_binding(&move_id, parse_binding("Ctrl+D").unwrap(), true).unwrap();
        s.add_binding(&move_id, parse_binding("MV").unwrap(), true).unwrap();
        assert_eq!(s.bindings(&move_id).len(), 2);
        assert_eq!(s.add_binding(&copy_id, parse_binding("Ctrl+D").unwrap(), false), Err(move_id.clone()));
        s.add_binding(&copy_id, parse_binding("Ctrl+D").unwrap(), true).unwrap();
        assert_eq!(s.bindings(&move_id).len(), 1);
        assert_eq!(s.binding_owner(parse_binding("Ctrl+D").unwrap()), Some(&copy_id));
    }

    #[test]
    fn a_three_key_sequence_is_read_when_no_two_key_binding_starts_it() {
        let mut s = Shortcuts::empty(Profile::Custom);
        let id = registry().resolve("cmd.beam").unwrap();
        s.add_binding(&id, parse_binding("BMM").unwrap(), true).unwrap();
        assert!(s.push_sequence(Key::B).is_none());
        assert!(s.push_sequence(Key::M).is_none(), "the pair waits for a third key");
        assert_eq!(s.push_sequence(Key::M), Some(id));
    }

    #[test]
    fn version_two_files_round_trip_and_keep_the_order_of_bindings() {
        let mut s = Shortcuts::empty(Profile::Custom);
        let id = registry().resolve("cmd.move").unwrap();
        for key in ["Ctrl+D", "Ctrl+Shift+M", "MV"] {
            s.add_binding(&id, parse_binding(key).unwrap(), true).unwrap();
        }
        let text = serialize_shortcuts(&s, ExportMode::All);
        let parsed = parse_shortcuts(&text);
        assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
        let mut back = Shortcuts::empty(Profile::Custom);
        for (id, b) in parsed.bindings {
            back.add_binding(&id, b, true).unwrap();
        }
        assert_eq!(back.bindings(&id), s.bindings(&id));
        assert_eq!(serialize_shortcuts(&back, ExportMode::All), text);
    }

    #[test]
    fn legacy_json_xml_and_lines_are_read() {
        let json = parse_shortcuts(r#"{"ZW": "Wall", "ZD": "Door"}"#);
        assert_eq!(json.bindings.len(), 2);
        let xml = parse_shortcuts(r#"<Shortcut CommandName="Wall" CommandId="ID_OBJECTS_WALL" Shortcuts="ZW#Z1" />"#);
        assert_eq!(xml.bindings.len(), 2);
        let lines = parse_shortcuts("ZW - Wall\nZZ = Door\n");
        assert_eq!(lines.bindings.len(), 2);
    }

    #[test]
    fn bad_lines_are_reported_with_their_numbers_and_do_not_stop_the_rest() {
        let parsed = parse_shortcuts("cmd.move = Ctrl+D\nno.such = Ctrl+E\ncmd.copy = Ctrl+№\ncmd.delete = Esc\n");
        assert_eq!(parsed.bindings.len(), 1);
        assert_eq!(parsed.errors.len(), 3);
        assert_eq!(parsed.errors[0].line, 2);
        let bad_version = parse_shortcuts("version = 3\ncmd.move = Ctrl+D\n");
        assert!(bad_version.bindings.is_empty());
        assert_eq!(bad_version.errors.len(), 1);
    }

    #[test]
    fn a_conflict_in_a_file_keeps_the_first_and_reports_the_second() {
        let parsed = parse_shortcuts("cmd.move = Ctrl+D\ncmd.copy = Ctrl+D\n");
        assert_eq!(parsed.bindings.len(), 1);
        assert_eq!(parsed.bindings[0].0.to_string(), "cmd.move");
        assert!(parsed.errors.iter().any(|e| e.line == 2 && e.message.contains("Ctrl+D")));
    }

    #[test]
    fn markdown_lists_every_action_and_every_active_binding_in_two_columns() {
        let mut s = Shortcuts::defaults(Profile::BimCraft);
        let id = registry().resolve("cmd.beam").unwrap();
        s.add_binding(&id, parse_binding("Ctrl+Alt+B").unwrap(), true).unwrap();
        let md = markdown(&s, &MarkdownOptions { include_date: false, ..MarkdownOptions::default() });
        for a in registry().actions() {
            let label = escape_cell(&localized(Lang::English, a.label));
            assert!(md.contains(&label), "{} is missing", a.label);
        }
        assert!(md.contains("| Command | Active |"));
        assert!(md.contains("Ctrl+Alt+B"), "a user's own binding is in Active");
        assert!(md.contains("Ctrl+В"));
        assert!(md.contains("Reserved and system combinations"));
        let again = markdown(&s, &MarkdownOptions { include_date: false, ..MarkdownOptions::default() });
        assert_eq!(md, again);
    }

    #[test]
    fn the_help_file_is_not_an_import_and_markdown_is_a_file_to_read_only() {
        let _ = Session::new();
        assert_eq!(parse_shortcuts("# BIMCraft — keyboard shortcuts\n").bindings.len(), 0);
    }
}
