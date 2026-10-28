//! BIMCraft's desktop front end, in egui: a thin shell over the core's [`Session`]. Panels read
//! the session's model and change it only through [`Session::execute`], so the desktop, the CLI,
//! scripts and agents share one set of commands, rules and undo history. The layout is Revit's:
//! the Quick Access Toolbar over the ribbon's tabs, a contextual Modify tab for the tool or the
//! selection, the Options Bar, Properties over the Project Browser on the left, views in tabs
//! (plans, the 3D view, schedules), and the tool's prompt in the status bar.

mod browser;
mod canvas;
mod clouds;
mod context;
pub mod control;
#[cfg(not(target_arch = "wasm32"))]
mod dialog;
#[cfg(target_arch = "wasm32")]
#[path = "web/dialog.rs"]
mod dialog;
mod dock;
mod draw;
mod editor;
mod favorites;
mod fillpatterns;
mod footer;
mod gpu;
mod graphics;
mod groups;
pub mod icons;
mod inquiry;
pub mod lang;
mod linestyles;
mod lineweights;
mod links;
mod mep;
mod objectstyles;
mod orbit;
mod paper;
mod phasing;
mod properties;
mod purge;
pub mod recent;
mod renames;
pub mod revit;
mod ribbon;
mod section;
mod section_box;
pub mod settings;
pub mod shortcuts;
mod sketch;
mod start;
mod structure;
mod style;
#[cfg(test)]
mod tests;
mod titlebar;
#[cfg(not(target_arch = "wasm32"))]
mod updates;
#[cfg(target_arch = "wasm32")]
#[path = "web/updates.rs"]
mod updates;
pub mod window;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bimcraft_core::engine::{self, Session};
use bimcraft_core::geom::P2;
use bimcraft_core::model::{CEILING_HEIGHT, DimKind, Element, Id, Justify, Location, Model, RAMP_WIDTH, Reference, STAIR_WIDTH};
use bimcraft_core::snap::{Kind as Snap, Snaps};
use bimcraft_core::units::Units;
use bimcraft_core::{element_json, render, schedule, stairs};
use egui::containers::menu::{MenuButton, SubMenuButton};
use egui::{
    Align, Align2, Area, CentralPanel, Color32, ComboBox, Context, CornerRadius, Event, FontId, Frame, Grid, Id as UiId, Key,
    KeyboardShortcut, Layout, Margin, Modal, Modifiers, Order, Panel, Popup, Pos2, Rect, Response, RichText, ScrollArea, Sense, Stroke,
    StrokeKind, TextEdit, Ui, ViewportCommand, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::{Value, json};

use bimcraft_core::language::Language;
mod autosave;
use icons::Icon;
use lang::{Lang, t, tf};
use ribbon::panel;
use style::pal;

/// A view: what the middle of the window shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sheet {
    /// A level's floor plan.
    Plan(Id),
    /// A level's reflected ceiling plan.
    Ceiling(Id),
    /// The default 3D view, `{3D}`.
    View3D,
    /// A section's or an elevation's view.
    Section(Id),
    /// A sheet: paper with a title block, and views placed on it.
    Paper(Id),
    /// One of the schedules every project has, by its category (`schedule::VIEWS`): the project's
    /// own once it's changed.
    Schedule(&'static str),
    /// Another schedule of the project's, by id.
    Table(Id),
}

/// What clicks on the plan do.
#[derive(Clone, Debug, PartialEq)]
pub enum Tool {
    /// Select the element under the cursor.
    Modify,
    /// The points of the wall chain clicked so far.
    Wall(Vec<P2>),
    Door,
    Window,
    Room,
    /// The Options Bar asks for the new level's elevation.
    Level,
    /// The grid's start, once clicked.
    Grid(Option<P2>),
    /// Move the selection: its base point, once clicked.
    Move(Option<P2>),
    /// Copy the selection: its base point, once clicked.
    Copy(Option<P2>),
    /// Rotate the selection about `centre`: the start of the angle, once clicked; with `place`, the
    /// next click moves the centre.
    Rotate {
        centre: P2,
        start: Option<P2>,
        place: bool,
    },
    /// Mirror the selection across a line picked, or with `draw`, a line drawn from `start`.
    Mirror {
        draw: bool,
        start: Option<P2>,
    },
    /// Line an element up with a reference line, once picked.
    Align(Option<(P2, P2)>),
    /// Trim or extend two walls to a corner: the first wall, and where it was picked.
    Trim(Option<(Id, P2)>),
    /// Trim/Extend Single or Multiple Elements: the boundary, once picked; then each line clicked goes to it (with `many`,
    /// until Esc; else one, and the boundary is picked again).
    TrimTo {
        boundary: Option<Id>,
        many: bool,
    },
    /// Add Vertex, or with `delete` Delete Vertex: on each line clicked, where it was clicked.
    Vertex {
        delete: bool,
    },
    /// Split the wall picked.
    Split,
    /// Copy (or move) the wall picked to the cursor's side, by the Options Bar's distance.
    Offset,
    /// Take the element picked down in the phase of the view on screen, or, if it comes down in it already, back up.
    Demolish,
    /// Draw the edge clicked in the Options Bar's line style, in this view only, as Revit's Linework does.
    Linework,
    /// Place Detail Group: the Options Bar's group type where clicked.
    DetailGroup,
    /// Repeating Detail: the Options Bar's group along a line, its start once clicked.
    RepeatingDetail(Option<P2>),
    /// Lay the material in the Options Bar over the face clicked in the 3D view, as Revit's Paint does.
    Paint,
    /// Take the paint off the face clicked in the 3D view.
    Unpaint,
    /// An aligned dimension: the references picked so far. A click away from any places it.
    Dimension(Vec<Reference>),
    /// An angular dimension: the lines picked so far, two that meet; then a click where its arc goes.
    Angular(Vec<Reference>),
    /// A radial dimension: the round column picked, then a click toward where its radius goes.
    Radial(Option<Reference>),
    /// A text note where clicked, saying what the Options Bar says.
    Text,
    /// A spot elevation where clicked.
    Spot,
    /// A ruler laid between two points: the first, once clicked.
    Measure(Option<P2>),
    /// Tag the door or window clicked.
    Tag,
    /// A section: its start, once clicked.
    Section(Option<P2>),
    /// An elevation, looking at the side of the building clicked beside.
    Elevation,
    /// A column where clicked.
    Column,
    /// A run (a beam, a pipe, a conduit…, by category, as `beam` and `pipe` name them): its first point, once clicked.
    Run(&'static str, Option<P2>),
    /// A structural column or an isolated footing (`structuralcolumn`, `footing`) where clicked.
    Stand(&'static str),
    /// A family's type where clicked: the one picked, turned by Space.
    Component,
    /// A fixture or a device of one tab's kinds (`mep::FIXTURES`, by key) where clicked: the family picked among the
    /// families of those kinds, turned by Space.
    Fixture(&'static str),
    /// A stair: the points clicked, its start and where its first run ends; the next click, which
    /// way its next run turns from the landing.
    Stair(Vec<P2>),
    /// A ramp, clicked as a stair is.
    Ramp(Vec<P2>),
    /// A railing's path, as clicked so far; with none, a click on a stair or a ramp puts one down
    /// each side.
    Railing(Vec<P2>),
    /// A roof over the walls of the plan's level, on the level above: Revit's Roof by Footprint,
    /// with every wall picked at once.
    Roof,
    /// A roof by extrusion, on the level above: its line's start and end, as clicked; the next
    /// click, how far across it's swept.
    ExtrusionRoof(Vec<P2>),
    /// A dormer where a roof's slope is clicked.
    Dormer,
    /// The roof footprint edge clicked stands upright, a gable end, or slopes again.
    GableEdge,
    /// Attach the selected walls' tops (or, as the Options Bar says, their bases) to the roof or
    /// floor picked.
    Attach,
    /// A floor under the walls of the plan's level.
    Floor,
    /// A ceiling in the room clicked, out to its walls' faces.
    Ceiling,
    /// A shaft opening: its first corner, once clicked.
    Shaft(Option<P2>),
    /// A mass: the corners of its footprint, as clicked so far.
    Mass(Vec<P2>),
    /// A detail line in the Options Bar's line style: its points, as clicked so far.
    DetailLine(Vec<P2>),
    /// The selected floor's slope arrow: its tail, once clicked.
    SlopeArrow(Option<P2>),
    /// Sketch mode's Draw panel: the points clicked so far of the shape it draws (Line: since the last loop closed).
    SketchLine(Vec<P2>),
    /// Sketch mode's Pick Walls: a line along each wall clicked.
    SketchPick,
}

/// Why the last command failed, shown in a box over the view.
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub text: String,
    pub error: bool,
}

/// A snap override typed as a shortcut, for the next point picked: Revit's SE, SM, SI, SC, SP and
/// SN (that snap alone), and SO (none).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Once {
    Only(Snap),
    Off,
}

/// The ribbon's tabs. `Context` is the contextual Modify tab of the tool or the selection.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Tab {
    Architecture,
    Structure,
    Plumbing,
    Electrical,
    Lightning,
    Data,
    Insert,
    Annotate,
    Massing,
    View,
    Manage,
    Modify,
    Context,
}

/// A step that would drop unsaved changes, waiting on the user.
#[derive(Clone, Debug, PartialEq)]
enum Pending {
    Recover(bimcraft_core::autosave::Backup),
    New,
    Open,
    /// A file the system asked to open ([`OsEvent::Open`]).
    OpenPath(PathBuf),
    Close,
    /// Close, so the installer can replace the program, and start it again updated.
    Update,
}

/// What the system asks of the app from outside its window. macOS sends a file Finder, the Dock or
/// Open With opens, and Quit from the Dock, as Apple events rather than as arguments; the front end
/// hands them over through [`BimApp::os_events`].
#[derive(Clone, Debug, PartialEq)]
pub enum OsEvent {
    Open(PathBuf),
    Quit,
}

/// The schedules: the category `schedule` takes, and the view's name.
pub(crate) const SCHEDULES: [(&str, &str); 6] = schedule::VIEWS;

/// Ribbon buttons that pick tools: icon, label, tooltip, tool.
type Tools<'a> = &'a [(Icon, &'a str, &'a str, Tool)];

/// The Modify panel's small buttons: icon, label, tooltip, the tool (none for Delete), enabled.
type Buttons<'a> = &'a [(Icon, &'a str, &'a str, Option<Tool>, bool)];

/// The desktop's own two-letter shortcuts, which [`BimApp::shortcut`] takes before the commands'
/// aliases, and what Keyboard Shortcuts (KS) calls them.
const SHORTCUTS: [(&str, &str); 50] = [
    ("3D", "3D View"),
    ("MD", "Modify"),
    ("SB", "Floor: Structural"),
    ("MM", "Mirror Pick Axis"),
    ("DM", "Mirror Draw Axis"),
    ("AL", "Align"),
    ("OF", "Offset"),
    ("DW", "Demolish"),
    ("ZF", "Zoom to Fit"),
    ("ZE", "Zoom to Fit"),
    ("ZA", "Zoom to Fit"),
    ("TL", "Thin Lines"),
    ("CS", "Create Similar"),
    ("PP", "Properties"),
    ("VP", "Properties"),
    ("NP", "Project Browser"),
    ("SA", "Select All Instances"),
    ("SE", "Snap to Endpoints"),
    ("SM", "Snap to Midpoints"),
    ("SI", "Snap to Intersections"),
    ("SC", "Snap to Centres"),
    ("SP", "Snap to Perpendiculars"),
    ("SQ", "Snap to Quadrants"),
    ("ST", "Snap to Tangents"),
    ("SN", "Snap to Nearest"),
    ("SO", "Snaps Off"),
    ("SS", "Turn Snap Override Off"),
    ("VV", "Visibility/Graphics"),
    ("VG", "Visibility/Graphics"),
    ("EH", "Hide Element"),
    ("VH", "Hide Category"),
    ("HH", "Temporary Hide Element"),
    ("HC", "Temporary Hide Category"),
    ("HI", "Temporary Isolate Element"),
    ("IC", "Temporary Isolate Category"),
    ("HR", "Reset Temporary Hide/Isolate"),
    ("RH", "Reveal Hidden Elements"),
    ("EU", "Unhide Elements"),
    ("UE", "Unhide Elements"),
    ("VU", "Unhide Category"),
    ("EO", "Override Graphics by Element"),
    ("FT", "Filters"),
    ("VT", "View Templates"),
    ("PH", "Phasing"),
    ("DO", "Design Options"),
    ("WF", "Wireframe"),
    ("HL", "Hidden Line"),
    ("SD", "Shaded"),
    ("KS", "Keyboard Shortcuts"),
    ("BX", "Section Box"),
];

/// Every two-letter shortcut and what it does: the desktop's own, then the commands' aliases.
fn shortcuts() -> Vec<(&'static str, &'static str)> {
    let aliases = engine::commands().iter().flat_map(|c| c.aliases.iter().map(move |a| (*a, c.label)));
    SHORTCUTS
        .into_iter()
        .chain(mep::FIXTURES.iter().map(|f| (f.letters, f.label)))
        .chain(aliases.filter(|(a, _)| !SHORTCUTS.iter().any(|(s, _)| s == a)))
        .collect()
}

/// What an IFC import brought in, and the first few things it left out (the core's reasons, in
/// English).
fn import_note(file: &str, reply: &Value) -> String {
    let counts: Vec<String> = reply["elements"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(cat, n)| match n.as_u64().unwrap_or(0) {
            1 => format!("1 {}", lang::category(cat, false).to_lowercase()),
            n => format!("{n} {}", lang::category(cat, true).to_lowercase()),
        })
        .collect();
    let mut text = tf("Imported {}: {}.", &[file, &counts.join(", ")]);
    let skipped: Vec<&str> = reply["skipped"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if !skipped.is_empty() {
        let more = match skipped.len().saturating_sub(3) {
            0 => String::new(),
            n => tf("; and {} more", &[&n.to_string()]),
        };
        text += &tf(" Left out: {}{}.", &[&skipped.iter().take(3).copied().collect::<Vec<_>>().join("; "), &more]);
    }
    if let Some([x, y]) = reply["moved"].as_array().map(Vec::as_slice) {
        text += &tf(" Moved {}, {} mm to bring it near the origin.", &[&x.to_string(), &y.to_string()]);
    }
    text
}

pub struct BimApp {
    pub session: Session,
    /// The system's requests since the last frame, read once a frame. None where it sends none.
    pub os_events: Option<Box<dyn FnMut() -> Vec<OsEvent>>>,
    /// The window draws its own title bar (`titlebar.rs`): the desktop's `main` says so where the system lets it (Windows and
    /// Linux), unless `system_title_bar`, the user's ask for the system's (Settings > User Interface, kept in `settings.json`).
    pub own_title_bar: bool,
    pub system_title_bar: bool,
    /// The active view, which takes the tools: a tab of `views`, or one of `floating`.
    pub sheet: Sheet,
    pub tool: Tool,
    /// The selected elements, in the order they were picked.
    pub selection: Vec<Id>,
    pub status: Status,
    /// The interface's language: English unless told otherwise (the app asks the environment).
    pub lang: Lang,
    /// Where a font with Chinese glyphs comes from, when the window is put in Chinese.
    font_finder: lang::FontFinder,
    /// The views docked as tabs, and the one of them the views' area shows: the active view, unless that floats.
    views: Vec<Sheet>,
    front: Sheet,
    /// Views floating in windows of their own.
    floating: Vec<dock::FloatView>,
    /// A view other than the active one is being drawn: it takes no clicks.
    passive: bool,
    /// Where the palettes are, and what is being dragged.
    docking: dock::Docking,
    dragging: Option<dock::Drag>,
    /// What a drag may land on, as drawn this frame: each stack's title (the palette it shows, and whether it floats),
    /// the views' tab bar and its tabs, and the docked view's drawing, along whose edges palettes dock; and the pointer, last
    /// seen.
    drop_headers: Vec<(dock::Palette, Rect, bool)>,
    tab_bar: Rect,
    tab_rects: Vec<(Sheet, Rect)>,
    work: Rect,
    pointer: Pos2,
    /// Where a floating window's title was taken from, from its corner.
    grab: egui::Vec2,
    /// The ribbon folded to its tabs (a tab's name double-clicked), and its tools shown under them for now (one clicked
    /// while folded); where its tabs end.
    ribbon_min: bool,
    ribbon_peek: bool,
    /// What High Contrast Point Clouds fills a view's scans with: the user's (Settings > Graphics), crimson at first.
    scan_contrast: [u8; 3],
    ribbon_bottom: Pos2,
    /// The Compact layout (File > Settings): the bars and palettes flush to the edges, not floating cards.
    compact: bool,
    /// File > Settings, while open, at its page; the settings file, when the app was given one, and what it holds; the
    /// theme it asks for, put in force on the next frame.
    settings_page: Option<settings::Page>,
    settings_path: Option<PathBuf>,
    settings_kept: Option<settings::Settings>,
    /// The window's size, kept and brought within its screen (`window.rs`).
    window: window::Tracker,
    theme_wanted: Option<egui::ThemePreference>,
    /// Keep drawing backgrounds light even when the interface is dark.
    light_canvas: bool,
    tab: Tab,
    /// Where the ribbon goes back to when the contextual tab goes.
    back: Tab,
    /// The type each placing tool uses, by category; the core's default when missing.
    types: BTreeMap<&'static str, String>,
    /// The family a door or window type picked belongs to (none: one of the project's own); by category.
    opening_families: BTreeMap<&'static str, String>,
    /// The Wall tool's top: `None` as the core does it (up to the next level up), `Some(None)`
    /// unconnected at `wall_height`, `Some(Some(level))` up to that level.
    wall_top: Option<Option<Id>>,
    wall_height: String,
    /// The Column tool's top, as `wall_top` and `wall_height` are the Wall tool's.
    column_top: Option<Option<Id>>,
    column_height: String,
    /// The Floor tool makes structural floors; the Beam tool's justification and height over the level; the footing tools'
    /// thickness and the height of their tops.
    floor_structural: bool,
    run_justify: Justify,
    beam_offset: String,
    /// The Options Bar's justification and offset of the run tools not on screen, by category, and whose are.
    run_kept: BTreeMap<&'static str, (Justify, String)>,
    run_loaded: &'static str,
    footing_thickness: String,
    footing_top: String,
    /// The Stair tool's width; the Roof tool's slope, overhang and gable ends; the Roof by
    /// Extrusion tool's profile, a shed's rather than a gable's; the Dormer tool's width and height,
    /// and whether its front has a window.
    stair_width: String,
    roof_slope: String,
    /// The scale being typed in the view control bar's Scale menu.
    scale_text: String,
    roof_overhang: String,
    roof_gable: bool,
    roof_shed: bool,
    dormer_width: String,
    dormer_height: String,
    dormer_window: bool,
    /// Stairs and ramps made with a railing down each side; the Ramp tool's top, width and slope.
    railings: bool,
    ramp_top: Option<Option<Id>>,
    ramp_height: String,
    ramp_width: String,
    ramp_slope: String,
    /// How high the Ceiling tool's ceilings are; the Slope Arrow's slope; the Shaft tool's top.
    ceiling_height: String,
    floor_slope: String,
    shaft_top: Option<Option<Id>>,
    shaft_height: String,
    /// The Mass tool's top, as the Shaft tool's.
    mass_top: Option<Option<Id>>,
    mass_height: String,
    /// Each click ends a wall and starts the next; off, two clicks make one wall.
    chain: bool,
    /// The line the Wall tool's walls stand on.
    wall_location: Location,
    /// Attach Top/Base attaches bases.
    attach_base: bool,
    /// Thin Lines: every line one pixel wide.
    thin: bool,
    /// Move and Copy keep to horizontal and vertical.
    constrain: bool,
    /// Copy places copy after copy from the same base point.
    multiple: bool,
    /// The Options Bar's Rotate angle, Copy for Rotate and for Mirror, and Offset distance and Copy.
    angle: String,
    rotate_copy: bool,
    mirror_copy: bool,
    offset: String,
    offset_copy: bool,
    /// Tags placed with a leader.
    tag_leader: bool,
    /// The field the Tag tool's tags show; blank for the category's usual one.
    tag_label: String,
    /// What the Text tool writes.
    note_text: String,
    /// The line style the Detail Line tool draws in; blank for the project's Thin Lines.
    line_style: String,
    /// The Linework tool's line style: a style's name, or `<Invisible lines>` or `<By Category>` (in English).
    linework_style: String,
    /// The detail group type Place Detail Group places.
    group_type: String,
    /// The Repeating Detail tool's spacing, as typed.
    repeat_spacing: String,
    /// The Detail Line tool's Chain: each click ends a line and starts the next; off, two clicks make one line.
    line_chain: bool,
    /// The shape each Draw panel draws, and the Options Bar's Sides, Radius and Offset.
    pub(crate) drawing: draw::Drawing,
    /// The project units the Options Bar's lengths were last written in.
    units: Units,
    /// A length typed while picking points: the listening dimension.
    pub(crate) typed_length: String,
    /// What each view hides or reveals for now (not the project's), and the model it is drawn from.
    temporary: HashMap<Sheet, footer::Temp>,
    shadow: Option<footer::Shadow>,
    /// The project, and what the views draw of it with its links built in: see [`BimApp::view_model`].
    seen: Option<(Arc<Model>, Arc<Model>)>,
    /// Link Model was asked for: its file dialog opens on the next frame, so a shortcut typed alone never waits on one.
    link_dialog: bool,
    /// Insert > Point Cloud was pressed: its file dialog opens this frame.
    cloud_dialog: bool,
    /// Bringing scan files in: the box, while it's open.
    cloud_box: Option<clouds::CloudBox>,
    /// A selected element's temporary dimension being typed: the element, which of its dimensions (a wall has one, a
    /// door or window two: from its wall's start, from its end), the length so far.
    dim_edit: Option<(Id, usize, String)>,
    /// A selected wall's angle being typed: the wall, whether it turns about its start (else its
    /// end), the angle so far.
    angle_edit: Option<(Id, bool, String)>,
    /// The snaps in force (Manage > Snaps), and whether their dialog is open.
    snaps: Snaps,
    show_snaps: bool,
    /// An override of them for the next point picked, typed as a shortcut.
    snap_once: Option<Once>,
    /// The Measure tool's last ruler: where it starts and ends.
    measured: Option<(P2, P2)>,
    /// Tab pressed over the plan, taken from egui's focus keys: cycle what's under the cursor.
    tab_cycle: bool,
    /// What a window handed over that `pace` kept back for the frames after the one it came in.
    held: Vec<Event>,
    /// The Keyboard Shortcuts dialog is open (KS); the Manage tab drops its Project Units menu open
    /// the next time it shows (UN).
    show_shortcuts: bool,
    pub custom_shortcuts: BTreeMap<String, String>,
    pub shortcuts_dialog: shortcuts::ShortcutsDialog,
    open_units: bool,
    /// Where the Options Bar was drawn last frame, for a view's navigation bar to keep clear of it (nothing when it isn't there).
    options_rect: Rect,
    canvas: canvas::Canvas,
    orbit: orbit::Orbit,
    /// The graphics card the 3D view may be drawn on (Settings > Performance).
    card: gpu::Card,
    cut: section::Cut,
    paper: paper::Papers,
    /// The model as last opened or saved: any other is unsaved.
    saved: Arc<Model>,
    autosave: autosave::State,
    /// The palette field being typed in: element, field, text so far.
    edit: Option<(Id, String, String)>,
    /// The Level tool's elevation and name, as typed in the Options Bar.
    new_level: (String, String),
    /// The last letters typed over the plan, for two-letter shortcuts.
    typed: String,
    pending: Option<Pending>,
    type_dialog: Option<properties::TypeDialog>,
    materials_dialog: Option<properties::MaterialsDialog>,
    family_dialog: Option<properties::FamilyDialog>,
    /// The Favorites window, while it's open; where the favorites file is, when the app was told (else the usual place).
    favorites_dialog: Option<favorites::FavoritesDialog>,
    /// The Family Editor, while it's open.
    family_editor: Option<editor::FamilyEditor>,
    favorites_path: Option<std::path::PathBuf>,
    /// The files File > Open Recent lists, and where they are kept (see [`BimApp::keep_recent_in`]; none until the app is told).
    recent: recent::Recent,
    recent_path: Option<std::path::PathBuf>,
    /// The start page: up when the window opens on no project, and from the quick access bar's Home.
    home: start::Home,
    parameters_dialog: Option<properties::ParametersDialog>,
    schedule_dialog: Option<properties::ScheduleDialog>,
    /// Visibility/Graphics, Filters and View Templates, while they're open.
    vg_dialog: Option<graphics::VgDialog>,
    filters_dialog: Option<graphics::FiltersDialog>,
    templates_dialog: Option<graphics::TemplatesDialog>,
    /// Phasing and Design Options, while they're open, and the design option (`set: option`) new
    /// elements are made in: none is the main model.
    phasing_dialog: Option<phasing::PhasingDialog>,
    /// Manage > Line Weights, while open.
    line_weights_dialog: Option<lineweights::LineWeightsDialog>,
    /// Manage > Object Styles, while open.
    object_styles_dialog: Option<objectstyles::ObjectStylesDialog>,
    /// Manage > Line Styles, while open.
    line_styles_dialog: Option<linestyles::LineStylesDialog>,
    fill_patterns_dialog: Option<fillpatterns::FillPatternsDialog>,
    /// Sketch mode: the lines of an outline being drawn, until Finish or Cancel.
    pub sketch: Option<sketch::Sketch>,
    /// The project the window is on (its `uid`, which a project keeps for life): one put in the session's place meanwhile is
    /// another, and everything held of the first (a sketch, a dialog's draft, the views open) is of nothing.
    project: u64,
    /// How wide the ribbon's Mode panel was last drawn, to stand it at the ribbon's right end.
    mode_width: f32,
    options_dialog: Option<phasing::OptionsDialog>,
    purge_dialog: Option<purge::PurgeDialog>,
    /// Manage > Select by ID and IDs of Selection, while open.
    select_by_id: Option<inquiry::SelectById>,
    ids_of_selection: Option<inquiry::IdsOfSelection>,
    /// What was last put on the clipboard.
    pub copied: Option<String>,
    active_option: Option<String>,
    new_schedule: Option<properties::NewSchedule>,
    /// The material the Paint tool lays; the first of the library's while it isn't one it has.
    paint_material: String,
    /// The family and type the Component tool places, as picked, and how far it's turned, degrees.
    component: Option<(String, String)>,
    component_angle: f64,
    /// The Properties palette's folded groups.
    collapsed: BTreeSet<&'static str>,
    /// Where views show, last frame.
    view_rect: Rect,
    /// Where the last view drawn showed, whether it was the active view or not.
    pane: Rect,
    /// What the Project Browser keeps to, and its search.
    browser_tab: browser::Tab,
    browser_find: String,
    /// Unsaved changes were dealt with: the window may close.
    closing: bool,
    title: String,
    styled: bool,
    /// The updater, which the desktop app attaches: a window starts without one, and so never phones home.
    #[cfg(not(target_arch = "wasm32"))]
    pub updates: Option<bimcraft_update::Updater>,
    /// In a browser there is no updater: the page is the version the site serves.
    #[cfg(target_arch = "wasm32")]
    pub updates: Option<std::convert::Infallible>,
    /// The reader of Revit files, which the desktop app attaches (`bimcraft-rvt` is Apache-2.0 only, so this crate never depends on
    /// it): a window starts without one, so it offers no `.rvt` and a test never reads a real one.
    pub revit: Option<revit::Importer>,
    /// The control port, when the window was started with one: agents work on this project through it.
    control: Option<control::Control>,
    /// The Revit file being read, on a thread of its own.
    revit_job: Option<revit::Job>,
    /// Files to open at the next frame: the command line's, and what the system asked for while a Revit file was being read.
    opens: Vec<PathBuf>,
    /// The file dialog that is open, if one is: it is on a thread of its own, and answers in a later frame.
    dialog: Option<dialog::Dialog>,
    /// What the last Revit import left out and brought in, for as long as its project is the one open.
    revit_report: Option<revit::Report>,
}

/// The Options Bar's Height: up to a level above the plan's `level` (by default the next one up),
/// or Unconnected, with a field for the height. `top` and `height` are the tool's own.
fn height_options(ui: &mut Ui, m: &Model, level: Id, top: &mut Option<Option<Id>>, height: &mut String, salt: &str) {
    let z = m.elevation(level);
    let above: Vec<(Id, &str)> = m.levels().into_iter().filter(|(_, l)| l.elevation > z).map(|(id, l)| (id, l.name.as_str())).collect();
    let now = top.unwrap_or_else(|| above.first().map(|(id, _)| *id));
    let current = now.and_then(|id| m.level(id)).map_or(t("Unconnected"), |l| l.name.as_str());
    let label = ui.label(t("Height:"));
    let combo = ComboBox::from_id_salt(salt).selected_text(current).width(130.0).show_ui(ui, |ui| {
        for (id, name) in &above {
            if ui.selectable_label(now == Some(*id), *name).clicked() {
                *top = Some(Some(*id));
            }
        }
        if ui.selectable_label(now.is_none(), t("Unconnected")).clicked() {
            *top = Some(None);
        }
    });
    combo.response.labelled_by(label.id);
    if now.is_none() {
        ui.add(TextEdit::singleline(height).desired_width(64.0).hint_text(m.units.name())).labelled_by(label.id);
    }
}

/// The params for the top an Options Bar set from the plan of `level`: up to a level, or
/// unconnected at a height.
fn top_params(m: &Model, level: Id, top: Option<Option<Id>>, height: &str) -> Result<Value, String> {
    match top.unwrap_or_else(|| m.level_above(level).and_then(|g| m.elements.id(g))) {
        Some(t) => Ok(json!({"top_level": t})),
        None => m.units.parse(height).map(|h| json!({"height": h})).ok_or_else(|| tf("`{}` is not a height", &[height])),
    }
}

/// A ribbon button: an icon over its label, tinted while its tool is on; as wide as its label, which breaks over two lines
/// when one would make it wider than `WIDE`.
fn big_button(ui: &mut Ui, icon: Icon, label: &str, active: bool, tip: &str) -> Response {
    const WIDE: f32 = 78.0;
    if ribbon::listing() {
        return list_row(ui, icon, label, active, tip);
    }
    if let Some(r) = ribbon::handed_over(ui) {
        return r;
    }
    let (label, tip) = (t(label), t(tip));
    let font = FontId::proportional(12.0);
    let wide = |s: &str| ui.painter().layout_no_wrap(s.to_string(), font.clone(), pal().text).size().x;
    let one = wide(label);
    // The space nearest the middle, for a label too wide for one line.
    let lines: Vec<String> = match label.char_indices().filter(|(_, c)| *c == ' ').min_by_key(|(i, _)| i.abs_diff(label.len() / 2)) {
        Some((at, _)) if one + 12.0 > WIDE => vec![label[..at].to_string(), label[at + 1..].to_string()],
        _ => vec![label.to_string()],
    };
    let width = lines.iter().map(|l| wide(l)).fold(0.0, f32::max) + 12.0;
    let (rect, r) = ui.allocate_exact_size(vec2(width.max(54.0), 60.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, active, label));
    let (fill, stroke, ink) = match (active, r.hovered() && enabled) {
        (true, _) => (pal().select_fill, Stroke::new(1.0, pal().accent), pal().accent_ink),
        (false, true) => (pal().hover_fill, Stroke::new(1.0, pal().hover), pal().icon),
        (false, false) => (Color32::TRANSPARENT, Stroke::NONE, if enabled { pal().icon } else { pal().icon.gamma_multiply(0.35) }),
    };
    // Keyboard focus shows as a ring, whatever else the button shows.
    let stroke = if r.has_focus() { style::focus_ring() } else { stroke };
    let p = ui.painter();
    p.rect(rect, 6.0, fill, stroke, StrokeKind::Inside);
    icons::paint(p, icon, Rect::from_center_size(rect.center_top() + vec2(0.0, 18.0), vec2(26.0, 26.0)), ink);
    let text = match (active, enabled) {
        (true, _) => pal().accent_ink,
        (false, true) => pal().text,
        (false, false) => pal().muted.gamma_multiply(0.6),
    };
    for (i, line) in lines.iter().enumerate() {
        p.text(rect.center_top() + vec2(0.0, 40.0 + 13.0 * i as f32), Align2::CENTER_CENTER, line, font.clone(), text);
    }
    r.on_hover_text(tip)
}

/// A small ribbon button: an icon alone, as in Revit's Modify panel; its name in its tooltip.
fn small_button(ui: &mut Ui, icon: Icon, label: &str, active: bool, tip: &str) -> Response {
    if ribbon::listing() {
        return list_row(ui, icon, label, active, tip);
    }
    if let Some(r) = ribbon::handed_over(ui) {
        return r;
    }
    let (label, tip) = (t(label), t(tip));
    let (rect, r) = ui.allocate_exact_size(vec2(30.0, 25.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, active, label));
    let (fill, stroke, ink) = match (active, r.hovered() && enabled) {
        (true, _) => (pal().select_fill, Stroke::new(1.0, pal().accent), pal().accent_ink),
        (false, true) => (pal().hover_fill, Stroke::new(1.0, pal().hover), pal().icon),
        (false, false) => (Color32::TRANSPARENT, Stroke::NONE, if enabled { pal().icon } else { pal().icon.gamma_multiply(0.35) }),
    };
    let stroke = if r.has_focus() { style::focus_ring() } else { stroke };
    ui.painter().rect(rect, 6.0, fill, stroke, StrokeKind::Inside);
    icons::paint(ui.painter(), icon, Rect::from_center_size(rect.center(), vec2(19.0, 19.0)), ink);
    r.on_hover_text(tip)
}

/// A ribbon button in its panel's list: a row of its icon and its name.
fn list_row(ui: &mut Ui, icon: Icon, label: &str, active: bool, tip: &str) -> Response {
    let (label, tip) = (t(label), t(tip));
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width().max(200.0), 30.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, active, label));
    let fill = match (active, r.hovered() && enabled) {
        (true, _) => pal().select_fill,
        (false, true) => pal().hover_fill,
        (false, false) => Color32::TRANSPARENT,
    };
    let p = ui.painter();
    p.rect_filled(rect, 6.0, fill);
    if r.has_focus() {
        p.rect_stroke(rect, 6.0, style::focus_ring(), StrokeKind::Inside);
    }
    let dim = |c: Color32| if enabled { c } else { c.gamma_multiply(0.4) };
    let ink = if active { pal().accent_ink } else { pal().icon };
    icons::paint(p, icon, Rect::from_center_size(rect.left_center() + vec2(18.0, 0.0), vec2(20.0, 20.0)), dim(ink));
    let text = if active { pal().accent_ink } else { pal().text };
    p.text(rect.left_center() + vec2(38.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(13.0), dim(text));
    r.on_hover_text(tip)
}

/// Buttons stacked three high in a panel, as Revit's Settings panel stacks them; in the panel's list they are rows like any other.
pub(crate) fn stacked(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        if !ribbon::listing() {
            ui.spacing_mut().item_spacing.y = 2.0;
        }
        add(ui);
    });
}

/// A button stacked three high in a panel (see [`stacked`]): its icon and its name in a row.
pub(crate) fn stacked_button(ui: &mut Ui, icon: Icon, label: &str, active: bool, tip: &str) -> Response {
    if ribbon::listing() {
        return list_row(ui, icon, label, active, tip);
    }
    if let Some(r) = ribbon::handed_over(ui) {
        return r;
    }
    let (label, tip) = (t(label), t(tip));
    let font = FontId::proportional(12.0);
    let width = ui.painter().layout_no_wrap(label.to_string(), font.clone(), pal().text).size().x + 28.0;
    let (rect, r) = ui.allocate_exact_size(vec2(width, 16.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, active, label));
    let fill = match (active, r.hovered() && enabled) {
        (true, _) => pal().select_fill,
        (false, true) => pal().hover_fill,
        (false, false) => Color32::TRANSPARENT,
    };
    let dim = |c: Color32| if enabled { c } else { c.gamma_multiply(0.4) };
    let (ink, text) = if active { (pal().accent_ink, pal().accent_ink) } else { (pal().icon, pal().text) };
    ui.painter().rect_filled(rect, 3.0, fill);
    if r.has_focus() {
        ui.painter().rect_stroke(rect, 3.0, style::focus_ring(), StrokeKind::Inside);
    }
    icons::paint(ui.painter(), icon, Rect::from_center_size(rect.left_center() + vec2(10.0, 0.0), vec2(14.0, 14.0)), dim(ink));
    ui.painter().text(rect.left_center() + vec2(22.0, 0.0), Align2::LEFT_CENTER, label, font, dim(text));
    r.on_hover_text(tip)
}

/// A small icon button, named for screen readers and tests.
fn icon_button(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    let label = t(label);
    let (rect, r) = ui.allocate_exact_size(vec2(26.0, 24.0), Sense::click());
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    let (ink, hover) = style::icon_inks();
    if r.hovered() {
        ui.painter().rect_filled(rect, 6.0, hover);
    }
    if r.has_focus() {
        ui.painter().rect_stroke(rect, 6.0, style::focus_ring(), StrokeKind::Inside);
    }
    icons::paint(ui.painter(), icon, Rect::from_center_size(rect.center(), vec2(18.0, 18.0)), ink);
    r.on_hover_text(label)
}

/// An undo step as Undo's list names it: what it made (Create Walls) or took away (Delete Door), or else its command.
fn step_name(st: &engine::Step) -> String {
    let one = |list: &[(&str, usize)]| match list {
        [(c, n)] => Some(lang::category(c, *n > 1)),
        _ => None,
    };
    let named = match (st.made.as_slice(), st.removed.as_slice()) {
        (made, []) if !made.is_empty() => one(made).map(|c| tf("Create {}", &[&c])),
        ([], removed) if !removed.is_empty() => one(removed).map(|c| tf("Delete {}", &[&c])),
        _ => None,
    };
    named.unwrap_or_else(|| t(st.label).to_string())
}

/// A tab, of the ribbon or of a view. Contextual tabs are green, as Revit's are.
fn tab_button(ui: &mut Ui, label: &str, active: bool, contextual: bool) -> Response {
    tab_button_with(ui, label, active, contextual, Sense::click(), 0.0)
}

/// A tab that senses what `sense` says (a view's drags too), `room` px wider on its right for what's drawn in it (a view's ×).
fn tab_button_with(ui: &mut Ui, label: &str, active: bool, contextual: bool, sense: Sense, room: f32) -> Response {
    let label = t(label);
    let font = if active { style::semibold(13.0) } else { FontId::proportional(13.0) };
    let size = ui.painter().layout_no_wrap(label.to_string(), font.clone(), pal().text).size();
    let (rect, r) = ui.allocate_exact_size(vec2(size.x + 24.0 + room, 24.0), sense);
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, active, label));
    // A pill: the open tab in the accent's soft tint, a contextual one solid green when open and tinted green when not.
    let (fill, stroke, ink) = match (contextual, active, r.hovered()) {
        (true, true, _) => (pal().context, Stroke::NONE, pal().on_context),
        (true, false, _) => (pal().context_fill, Stroke::new(1.0, pal().context.gamma_multiply(0.5)), pal().context),
        (false, true, _) => (pal().select_fill, Stroke::NONE, pal().accent_ink),
        (false, false, true) => (pal().hover_fill, Stroke::NONE, pal().text),
        (false, false, false) => (Color32::TRANSPARENT, Stroke::NONE, pal().text),
    };
    let p = ui.painter();
    let radius = CornerRadius::same(6);
    p.rect(rect, radius, fill, stroke, StrokeKind::Inside);
    if r.has_focus() {
        p.rect_stroke(rect, radius, style::focus_ring(), StrokeKind::Inside);
    }
    p.text(rect.center() - vec2(room / 2.0, 0.0), Align2::CENTER_CENTER, label, font, ink);
    r
}

impl BimApp {
    pub fn new(session: Session) -> BimApp {
        let first = Sheet::Plan(session.model.levels().first().map_or(0, |(id, _)| *id));
        let units = session.model.units;
        BimApp {
            saved: Arc::clone(&session.model),
            project: session.model.uid,
            autosave: autosave::State::default(),
            session,
            os_events: None,
            own_title_bar: false,
            system_title_bar: false,
            sheet: first,
            tool: Tool::Modify,
            selection: Vec::new(),
            status: Status::default(),
            views: vec![first],
            front: first,
            floating: Vec::new(),
            passive: false,
            docking: dock::Docking::default(),
            dragging: None,
            drop_headers: Vec::new(),
            tab_bar: Rect::NOTHING,
            tab_rects: Vec::new(),
            work: Rect::ZERO,
            pointer: Pos2::ZERO,
            grab: egui::Vec2::ZERO,
            ribbon_min: false,
            compact: false,
            ribbon_peek: false,
            scan_contrast: footer::HIGH_CONTRAST,
            ribbon_bottom: Pos2::ZERO,
            settings_page: None,
            settings_path: None,
            settings_kept: None,
            window: window::Tracker::default(),
            theme_wanted: None,
            light_canvas: false,
            tab: Tab::Architecture,
            back: Tab::Architecture,
            types: BTreeMap::new(),
            opening_families: BTreeMap::new(),
            wall_top: None,
            wall_height: units.length(3000.0),
            column_top: None,
            column_height: units.length(3000.0),
            floor_structural: false,
            run_justify: Justify::Top,
            beam_offset: units.length(0.0),
            run_kept: BTreeMap::new(),
            run_loaded: "beam",
            footing_thickness: units.length(400.0),
            footing_top: units.length(0.0),
            stair_width: units.length(STAIR_WIDTH),
            roof_slope: "30".into(),
            scale_text: String::new(),
            roof_overhang: units.length(500.0),
            roof_gable: false,
            roof_shed: false,
            dormer_width: units.length(1600.0),
            dormer_height: units.length(1200.0),
            dormer_window: true,
            railings: true,
            ramp_top: Some(None),
            ramp_height: units.length(600.0),
            ramp_width: units.length(RAMP_WIDTH),
            ramp_slope: "8.33".into(),
            ceiling_height: units.length(CEILING_HEIGHT),
            floor_slope: "5".into(),
            shaft_top: None,
            shaft_height: units.length(3000.0),
            mass_top: None,
            mass_height: units.length(3000.0),
            chain: true,
            wall_location: Location::Centre,
            attach_base: false,
            thin: false,
            constrain: false,
            multiple: false,
            angle: String::new(),
            rotate_copy: false,
            mirror_copy: true,
            offset: units.length(1000.0),
            offset_copy: true,
            tag_leader: false,
            tag_label: String::new(),
            note_text: String::new(),
            line_style: String::new(),
            linework_style: String::new(),
            group_type: String::new(),
            repeat_spacing: "200".into(),
            line_chain: true,
            drawing: draw::Drawing::default(),
            units,
            lang: Lang::English,
            font_finder: lang::chinese_font,
            typed_length: String::new(),
            dim_edit: None,
            temporary: HashMap::new(),
            shadow: None,
            seen: None,
            link_dialog: false,
            cloud_dialog: false,
            cloud_box: None,
            tab_cycle: false,
            held: Vec::new(),
            show_shortcuts: false,
            custom_shortcuts: BTreeMap::new(),
            shortcuts_dialog: shortcuts::ShortcutsDialog::default(),
            angle_edit: None,
            snaps: Snaps::default(),
            show_snaps: false,
            snap_once: None,
            measured: None,
            open_units: false,
            options_rect: Rect::NOTHING,
            canvas: canvas::Canvas::default(),
            orbit: orbit::Orbit::default(),
            card: gpu::Card::default(),
            cut: section::Cut::default(),
            paper: paper::Papers::default(),
            edit: None,
            new_level: (String::new(), String::new()),
            typed: String::new(),
            pending: None,
            type_dialog: None,
            materials_dialog: None,
            family_dialog: None,
            favorites_dialog: None,
            family_editor: None,
            favorites_path: None,
            recent: recent::Recent::default(),
            recent_path: None,
            home: start::Home::default(),
            paint_material: String::new(),
            parameters_dialog: None,
            schedule_dialog: None,
            vg_dialog: None,
            filters_dialog: None,
            templates_dialog: None,
            phasing_dialog: None,
            line_weights_dialog: None,
            object_styles_dialog: None,
            line_styles_dialog: None,
            fill_patterns_dialog: None,
            sketch: None,
            mode_width: 0.0,
            options_dialog: None,
            purge_dialog: None,
            select_by_id: None,
            ids_of_selection: None,
            copied: None,
            active_option: None,
            new_schedule: None,
            component: None,
            component_angle: 0.0,
            collapsed: BTreeSet::new(),
            view_rect: Rect::ZERO,
            pane: Rect::ZERO,
            browser_tab: browser::Tab::All,
            browser_find: String::new(),
            closing: false,
            title: String::new(),
            styled: false,
            updates: None,
            revit: None,
            control: None,
            revit_job: None,
            opens: Vec::new(),
            dialog: None,
            revit_report: None,
        }
    }

    /// Whether the model changed since it was last opened or saved.
    pub fn dirty(&self) -> bool {
        !Arc::ptr_eq(&self.saved, &self.session.model)
    }

    /// Where model point `p` shows on screen, on the plan drawn last.
    pub fn screen(&self, p: P2) -> Pos2 {
        self.canvas.screen(p)
    }

    /// Run a command; when it fails, say why.
    fn run(&mut self, id: &str, params: Value) -> Option<Value> {
        self.made_in();
        match self.session.execute(id, &params) {
            Ok(v) => {
                self.status = Status::default();
                // The core renames and drops the option with its set.
                self.active_option = self.session.option.clone();
                Some(v)
            }
            Err(e) => {
                self.status = Status { text: e, error: true };
                None
            }
        }
    }

    /// The contextual tab's name while a tool is on or something is selected.
    fn context(&self) -> Option<String> {
        if let Some(sk) = &self.sketch {
            return Some(format!("{} | {}", t("Modify"), t(sk.title())));
        }
        let place = match self.tool {
            Tool::Modify
            | Tool::Move(_)
            | Tool::Copy(_)
            | Tool::Rotate { .. }
            | Tool::Mirror { .. }
            | Tool::Align(_)
            | Tool::Trim(_)
            | Tool::TrimTo { .. }
            | Tool::Vertex { .. }
            | Tool::Split
            | Tool::Offset
            | Tool::Demolish
            | Tool::Linework
            | Tool::Attach
            | Tool::GableEdge
            | Tool::SlopeArrow(_)
            | Tool::SketchLine(_)
            | Tool::SketchPick => None,
            Tool::Wall(_) => Some("Place Wall"),
            Tool::Paint => Some("Paint"),
            Tool::Unpaint => Some("Remove Paint"),
            Tool::Door => Some("Place Door"),
            Tool::Window => Some("Place Window"),
            Tool::Room => Some("Place Room"),
            Tool::Level => Some("Place Level"),
            Tool::Grid(_) => Some("Place Grid"),
            Tool::Dimension(_) | Tool::Angular(_) | Tool::Radial(_) => Some("Place Dimensions"),
            Tool::Text => Some("Place Text"),
            Tool::DetailGroup => Some("Place Detail Group"),
            Tool::RepeatingDetail(_) => Some("Place Repeating Detail"),
            Tool::DetailLine(_) if self.drawing.insulation.is_some() => Some("Place Insulation"),
            Tool::DetailLine(_) if self.drawing.cloud.is_some() => Some("Place Revision Cloud"),
            Tool::DetailLine(_) => Some("Place Detail Lines"),
            Tool::Spot => Some("Spot Elevation"),
            Tool::Measure(_) => Some("Measure"),
            Tool::Tag => Some("Tag"),
            Tool::Section(_) => Some("Section"),
            Tool::Elevation => Some("Elevation"),
            Tool::Column => Some("Place Column"),
            Tool::Run(..) | Tool::Stand(_) => structure::title(&self.tool),
            Tool::Component => Some("Place Component"),
            Tool::Fixture(key) => mep::fixture(key).map(|f| f.title),
            Tool::Stair(_) => Some("Create Stair"),
            Tool::Ramp(_) => Some("Create Ramp"),
            Tool::Railing(_) => Some("Create Railing Path"),
            Tool::Roof => Some("Create Roof Footprint"),
            Tool::ExtrusionRoof(_) => Some("Create Extrusion Roof"),
            Tool::Dormer => Some("Dormer"),
            Tool::Floor => Some("Create Floor"),
            Tool::Ceiling => Some("Place Ceiling"),
            Tool::Shaft(_) => Some("Create Shaft Opening"),
            Tool::Mass(_) => Some("Create Mass"),
        };
        if let Some(what) = place {
            return Some(format!("{} | {}", t("Modify"), t(what)));
        }
        let m = &self.session.model;
        let categories: BTreeSet<&str> = self.selection.iter().filter_map(|id| m.elements.get(id)).map(|e| e.category()).collect();
        match categories.len() {
            0 => None,
            1 => categories.first().map(|c| format!("{} | {}", t("Modify"), lang::category(c, true))),
            _ => Some(format!("{} | {}", t("Modify"), t("Multi-Select"))),
        }
    }

    /// Bring the contextual tab up, as Revit does for a tool or a selection.
    fn raise_context(&mut self) {
        if self.tab != Tab::Context {
            self.back = self.tab;
            self.tab = Tab::Context;
        }
    }

    fn view_name(&self, sheet: Sheet) -> String {
        match sheet {
            Sheet::Plan(key) | Sheet::Ceiling(key) => self.session.model.view_name(key).map_or_else(|| "Floor Plan".into(), String::from),
            Sheet::View3D => "{3D}".into(),
            Sheet::Section(id) => self.session.model.section(id).map_or_else(|| "Section".into(), |v| v.name.clone()),
            Sheet::Paper(id) => self.session.model.sheet(id).map_or_else(|| "Sheet".into(), |s| format!("{} - {}", s.number, s.name)),
            Sheet::Schedule(cat) => match schedule::standard(&self.session.model, cat) {
                (Some(_), sc) => sc.name,
                (None, _) => t(SCHEDULES.iter().find(|(c, _)| *c == cat).map_or(cat, |(_, name)| name)).to_string(),
            },
            Sheet::Table(id) => match self.session.model.elements.get(id) {
                Some(Element::Schedule(sc)) => sc.name.clone(),
                _ => "Schedule".into(),
            },
        }
    }

    /// The view as the title bar names it: `Floor Plan: Level 1`.
    fn view_title(&self, sheet: Sheet) -> String {
        let kind = match sheet {
            Sheet::Plan(_) => "Floor Plan",
            Sheet::Ceiling(_) => "Ceiling Plan",
            Sheet::View3D => "3D View",
            Sheet::Section(id) if matches!(self.session.model.elements.get(id), Some(Element::Elevation(_))) => "Elevation",
            Sheet::Section(_) => "Section",
            Sheet::Paper(_) => "Sheet",
            Sheet::Schedule(_) | Sheet::Table(_) => "Schedule",
        };
        format!("{}: {}", t(kind), self.view_name(sheet))
    }

    /// The whole window.
    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        // What `pace` held is for the next frame, which a window with nothing new to say won't run.
        if !self.held.is_empty() {
            ctx.request_repaint();
        }
        if !self.styled {
            style::apply(&ctx);
            self.styled = true;
        }
        if let Some(theme) = self.theme_wanted.take() {
            ctx.set_theme(theme);
        }
        style::follow(&ctx, style::Look { light_canvas: self.light_canvas, compact: self.compact });
        // Chinese needs a font with its glyphs, which the system may not have: then the window stays in English and says why.
        if self.lang == Lang::Chinese && !lang::use_chinese_font(&ctx, self.font_finder) {
            self.lang = Lang::English;
            self.status = updates::failed(t(lang::NO_CHINESE_FONT).to_string());
        }
        lang::set(self.lang);
        // What agents asked of the project comes before the tidying, which puts right what a command left stale.
        self.answer_agents(&ctx);
        // A project put in the session's place under the window (the control port turns an agent's New and Open away, but a
        // script or an embedder may do them) starts it over, as File > New does: its sketch and dialogs were of the old one, and
        // it gets a recovery copy of its own. (The old one's stays until that is written: nobody was asked about its changes.)
        if self.session.model.uid != self.project {
            self.reopened();
            self.autosave_document();
        }
        self.tidy();
        self.poll_updates(&ctx);
        self.poll_revit(&ctx);
        self.poll_autosave(&ctx);
        self.keys(&ctx);
        self.os_requests(&ctx);
        self.dialog_answered();
        self.watch_window(&ctx);
        let view = self.view_title(self.sheet);
        let title = format!("{}{} · {view} · BIMCraft", self.session.model.name, if self.dirty() { "*" } else { "" });
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.closing && (self.installing() || self.dirty()) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            // An update being put in place holds the window: it closes it when it's in.
            if !self.installing() {
                self.pending = Some(Pending::Close);
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.dirty() && !self.installing() {
            self.retire_autosave();
        }
        if self.home.shown {
            if self.own_bar() {
                self.start_title_bar(ui);
            }
            self.start_page(ui);
            self.drag_and_drop(&ctx);
            self.warning(&ctx);
            self.dialogs(&ctx);
            self.keep_settings(&ctx);
            if self.own_bar() {
                titlebar::frame(ui);
            }
            return;
        }
        // The view's ground under the cards: what shows in the gaps round them is the view going on beneath.
        ui.painter().rect_filled(ui.max_rect(), 0.0, self.backdrop());
        let app_bar = Frame::new().fill(pal().app_bar).inner_margin(Margin::symmetric(8, 5));
        Panel::top("quick-access").frame(app_bar).show_separator_line(false).show(ui, |ui| {
            // Where the window draws its own title bar, this is it: its ground moves the window, its corner closes it.
            let bar = ui.max_rect().expand2(vec2(8.0, 5.0));
            let own = self.own_bar();
            if own {
                titlebar::drag_area(ui, bar);
            }
            style::on_app_bar(|| self.quick_access(ui, &view));
            if own {
                titlebar::caption_buttons(ui, bar);
            }
        });
        let card = style::card([true, true, true, false]);
        Panel::top("ribbon").frame(card).show_separator_line(false).show(ui, |ui| self.ribbon(ui));
        if self.has_update_banner() {
            Panel::top("update").frame(Frame::NONE).show_separator_line(false).show(ui, |ui| self.update_banner(ui));
        }
        let status = style::card([true, true, false, true]).inner_margin(Margin::symmetric(10, 2));
        Panel::bottom("status").frame(status).show_separator_line(false).show(ui, |ui| self.status_bar(ui));
        self.docks(ui);
        CentralPanel::default().frame(Frame::NONE).show(ui, |ui| {
            let g = style::gap().max(4);
            Panel::top("view-tabs")
                .frame(Frame::new().inner_margin(Margin { left: g, right: g, top: g, bottom: 4 }))
                .show_separator_line(false)
                .show(ui, |ui| {
                    self.view_tabs(ui);
                });
            let front = self.front;
            self.view_pane(ui, front);
            self.work = self.pane;
        });
        self.options_bar(&ctx);
        if self.ribbon_min && self.ribbon_peek {
            self.ribbon_peek(&ctx);
        }
        self.palette_windows(&ctx);
        self.view_windows(&ctx);
        self.drag_and_drop(&ctx);
        self.warning(&ctx);
        self.dialogs(&ctx);
        self.keep_settings(&ctx);
        self.track_sketch();
        if self.own_bar() {
            titlebar::frame(ui);
        }
    }

    /// The start page's title bar, where the window draws its own: the app bar's colour, the app's name, and the caption buttons.
    fn start_title_bar(&mut self, ui: &mut Ui) {
        Panel::top("start-title-bar").frame(Frame::new().fill(pal().app_bar)).exact_size(34.0).show_separator_line(false).show(ui, |ui| {
            let bar = ui.max_rect();
            titlebar::drag_area(ui, bar);
            ui.painter().text(
                bar.left_center() + vec2(14.0, 0.0),
                Align2::LEFT_CENTER,
                "BIMCraft",
                FontId::proportional(12.5),
                pal().app_bar_muted,
            );
            titlebar::caption_buttons(ui, bar);
        });
    }

    /// What the active view is drawn on: paper for a drawing, the desk round a sheet, a schedule's ground.
    fn backdrop(&self) -> Color32 {
        match self.sheet {
            Sheet::Paper(_) => pal().desk,
            Sheet::Schedule(_) | Sheet::Table(_) => pal().surface,
            _ => canvas::color(render::PAPER),
        }
    }

    /// A view under its view control bar, as the active view draws it.
    fn view_body(&mut self, ui: &mut Ui) {
        self.view_control_bar(ui);
        self.view_rect = ui.available_rect_before_wrap();
        self.pane = self.view_rect;
        // The plan sets the cursor each frame it shows.
        self.canvas.cursor = None;
        match self.sheet {
            Sheet::Plan(key) | Sheet::Ceiling(key) if self.session.model.plan_level(key).is_some() => self.canvas(ui, key),
            Sheet::Plan(_) | Sheet::Ceiling(_) => {
                ui.centered_and_justified(|ui| ui.label(t("This project has no levels. Add one with the Level tool.")));
            }
            Sheet::View3D => self.view3d(ui),
            Sheet::Section(id) => self.section_view(ui, id),
            Sheet::Paper(id) => self.paper_view(ui, id),
            Sheet::Schedule(_) | Sheet::Table(_) => self.schedule(ui),
        }
        self.temp_frame(ui);
    }

    /// Properties (PP): shown, or hidden.
    fn toggle_properties(&mut self) {
        let p = dock::Palette::Properties;
        self.show_palette(p, !self.docking.shown(p));
    }

    /// Follow what the last command removed: the selection, views of levels that are gone, and
    /// the contextual tab once its tool or selection is.
    fn tidy(&mut self) {
        // The Options Bar's lengths, rewritten in the project's units when they change.
        let (from, to) = (self.units, self.session.model.units);
        if from != to {
            let fields = [
                &mut self.wall_height,
                &mut self.column_height,
                &mut self.stair_width,
                &mut self.roof_overhang,
                &mut self.offset,
                &mut self.ceiling_height,
                &mut self.ramp_height,
                &mut self.ramp_width,
                &mut self.shaft_height,
                &mut self.mass_height,
            ];
            for field in fields {
                if let Some(mm) = from.parse(field) {
                    *field = to.length(mm);
                }
            }
            self.units = to;
        }
        let m = &self.session.model;
        self.selection.retain(|id| m.elements.contains_key(id));
        self.active_option = self.active_option.take().filter(|o| bimcraft_core::phasing::option_exists(m, o));
        let exists = |s: &Sheet| match s {
            Sheet::Plan(key) | Sheet::Ceiling(key) => m.plan_level(*key).is_some(),
            Sheet::Section(id) => m.section(*id).is_some(),
            Sheet::Paper(id) => m.sheet(*id).is_some(),
            Sheet::View3D | Sheet::Schedule(_) => true,
            Sheet::Table(id) => matches!(m.elements.get(id), Some(Element::Schedule(_))),
        };
        self.views.retain(exists);
        self.floating.retain(|f| exists(&f.sheet));
        if !exists(&self.sheet) {
            let floated = self.floating.last().map(|f| f.sheet);
            self.sheet = self.views.last().copied().or(floated).unwrap_or(Sheet::Plan(m.levels().first().map_or(0, |(id, _)| *id)));
        }
        let floats = self.floating.iter().any(|f| f.sheet == self.sheet);
        // The tab bar is never empty: the last view floating comes back to it.
        if floats && self.views.is_empty() {
            let sheet = self.sheet;
            self.floating.retain(|f| f.sheet != sheet);
        } else if !floats && !self.views.contains(&self.sheet) {
            self.views.push(self.sheet);
        }
        if !self.floating.iter().any(|f| f.sheet == self.sheet) {
            self.front = self.sheet;
        }
        if !self.views.contains(&self.front) {
            self.front = self.views.last().copied().unwrap_or(self.sheet);
        }
        if self.tab == Tab::Context && self.context().is_none() {
            self.tab = self.back;
        }
    }

    fn keys(&mut self, ctx: &Context) {
        // A dialog takes the keys while it's open.
        if self.type_dialog.is_some()
            || self.materials_dialog.is_some()
            || self.family_dialog.is_some()
            || self.favorites_dialog.is_some()
            || self.family_editor.is_some()
            || self.parameters_dialog.is_some()
            || self.schedule_dialog.is_some()
            || self.vg_dialog.is_some()
            || self.filters_dialog.is_some()
            || self.templates_dialog.is_some()
            || self.phasing_dialog.is_some()
            || self.line_weights_dialog.is_some()
            || self.object_styles_dialog.is_some()
            || self.line_styles_dialog.is_some()
            || self.fill_patterns_dialog.is_some()
            || self.options_dialog.is_some()
            || self.purge_dialog.is_some()
            || self.select_by_id.is_some()
            || self.ids_of_selection.is_some()
            || self.settings_page.is_some()
            || self.new_schedule.is_some()
            || self.show_shortcuts
            || self.show_snaps
            || self.pending.is_some()
            || self.autosave.show_recovery
            || self.installing()
            || self.importing()
            || self.report_open()
            // An open menu takes the keys: Esc closes it, not the tool.
            || egui::Popup::is_any_open(ctx)
        {
            return;
        }
        let pressed = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::S) {
            self.save_as(ctx, None);
        } else if pressed(Modifiers::COMMAND, Key::S) {
            self.save(ctx, None);
        }
        if pressed(Modifiers::COMMAND, Key::O) {
            self.guard(Pending::Open, ctx);
        }
        if pressed(Modifiers::COMMAND, Key::N) {
            self.guard(Pending::New, ctx);
        }
        if pressed(Modifiers::COMMAND, Key::Comma) {
            self.settings_page = Some(settings::Page::Interface);
        }
        if pressed(Modifiers::COMMAND, Key::Num1) {
            self.toggle_properties();
        }
        if pressed(Modifiers::COMMAND, Key::P) {
            self.export(ctx, "PDF", "pdf");
        }
        // The rest belongs to a text field while one has the keyboard.
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if pressed(Modifiers::COMMAND, Key::D) {
            self.toggle_home();
        }
        if self.sketch.is_some() {
            // While sketching, Ctrl+Z takes back a line; the model's steps wait until Finish or Cancel.
            if pressed(Modifiers::COMMAND, Key::Z) {
                self.unsketch();
            }
        } else if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || pressed(Modifiers::COMMAND, Key::Y) {
            self.run("redo", json!({}));
        } else if pressed(Modifiers::COMMAND, Key::Z) {
            self.run("undo", json!({}));
        }
        if pressed(Modifiers::NONE, Key::Escape) {
            self.escape();
        }
        // Enter places a typed length, else builds the wall chain.
        if pressed(Modifiers::NONE, Key::Enter)
            && let Some(level) = self.plan_level()
        {
            match (&self.tool, self.typed_length.is_empty() && self.drawing.other.is_empty()) {
                (Tool::Railing(_), true) => self.finish_railing(level),
                (Tool::Mass(_), true) => self.finish_mass(level),
                (Tool::DetailLine(_), true) => self.finish_detail_line(level),
                (Tool::SketchLine(_), true) if self.shape().open_ended() => self.finish_open_shape(),
                (_, true) => self.finish_wall(),
                (_, false) => self.place_typed(level),
            }
            // A snap override typed was for that point.
            self.snap_once = None;
        }
        if !self.typed_length.is_empty() && pressed(Modifiers::NONE, Key::Backspace) {
            self.typed_length.pop();
        }
        if pressed(Modifiers::NONE, Key::Delete) {
            self.delete();
        }
        if self.tool == Tool::Modify && pressed(Modifiers::NONE, Key::Space) {
            self.flip();
        }
        if matches!(self.tool, Tool::Component | Tool::Fixture(_)) && pressed(Modifiers::NONE, Key::Space) {
            self.component_angle = (self.component_angle + 90.0) % 360.0;
        }
        let typed: String = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    Event::Text(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect()
        });
        // A digit starts a length while a tool is picking its next point, and everything typed
        // after it (feet, inches, fractions, a unit) is the length's until Enter; letters before
        // one are shortcuts.
        let mut shortcut = String::new();
        for ch in typed.chars() {
            let listening = self.base().is_some() || self.filleting();
            if listening && (ch.is_ascii_digit() || ch == '.' || !self.typed_length.is_empty()) {
                self.typed_length.push(ch);
            } else {
                shortcut.push(ch);
            }
        }
        for ch in shortcut.chars().filter(|c| c.is_ascii_alphanumeric()) {
            self.typed.push(ch.to_ascii_uppercase());
            if self.typed.len() > 2 {
                self.typed.remove(0);
            }
            if self.typed.len() == 2 && self.shortcut() {
                self.typed.clear();
            }
        }
    }

    /// Revit's two-letter shortcuts: the desktop's own ([`SHORTCUTS`]), then the commands' aliases
    /// (WA, DR, CR…). Whether `typed` is one.
    fn shortcut(&mut self) -> bool {
        if let Some(action) = self.custom_shortcuts.get(&self.typed).cloned()
            && self.execute_shortcut_action(&action)
        {
            return true;
        }
        let tool = match self.typed.as_str() {
            "3D" => {
                self.open_sheet(Sheet::View3D);
                return true;
            }
            "MD" => Tool::Modify,
            "MM" => Tool::Mirror { draw: false, start: None },
            "DM" => Tool::Mirror { draw: true, start: None },
            "AL" => Tool::Align(None),
            "OF" => Tool::Offset,
            "DW" => Tool::Demolish,
            "LW" => Tool::Linework,
            "ZF" | "ZE" | "ZA" => {
                self.fit();
                return true;
            }
            "TL" => {
                self.thin = !self.thin;
                return true;
            }
            "CS" => {
                self.create_similar();
                return true;
            }
            "PP" | "VP" => {
                self.toggle_properties();
                return true;
            }
            "NP" => {
                self.toggle_browser();
                return true;
            }
            "SA" => {
                self.select_all_instances();
                return true;
            }
            "KS" => {
                self.show_shortcuts = true;
                return true;
            }
            // Floor: Structural.
            "SB" => {
                self.structural_floor();
                return true;
            }
            "VV" | "VG" => {
                self.open_graphics(0);
                return true;
            }
            "EH" | "VH" => {
                self.hide_selection(self.typed == "VH");
                return true;
            }
            "WF" | "HL" | "SD" => {
                let style = match self.typed.as_str() {
                    "WF" => bimcraft_core::graphics::VisualStyle::Wireframe,
                    "HL" => bimcraft_core::graphics::VisualStyle::HiddenLine,
                    _ => bimcraft_core::graphics::VisualStyle::Shaded,
                };
                self.set_visual_style(style);
                return true;
            }
            "HH" | "HC" | "HI" | "IC" | "HR" | "RH" => {
                match self.typed.as_str() {
                    "HH" => self.temp_hide(false),
                    "HC" => self.temp_hide(true),
                    "HI" => self.temp_isolate(false),
                    "IC" => self.temp_isolate(true),
                    "HR" => self.temp_reset(),
                    _ => self.temp_reveal(),
                }
                return true;
            }
            // What the selection hides comes back; with nothing of it hidden, the list of what the view hides.
            "PU" => {
                self.open_purge();
                return true;
            }
            "LK" => {
                self.link_dialog = true;
                return true;
            }
            "DP" => {
                self.duplicate_plan("duplicate");
                return true;
            }
            "BX" => {
                self.toggle_section_box();
                return true;
            }
            "EU" | "UE" => {
                if self.hidden_selection().0.is_empty() {
                    self.open_graphics(3);
                } else {
                    self.unhide_selection(false);
                }
                return true;
            }
            "VU" => {
                self.unhide_selection(true);
                return true;
            }
            "EO" => {
                self.override_selection();
                return true;
            }
            "FT" => {
                self.filters_dialog = Some(graphics::FiltersDialog::default());
                return true;
            }
            "VT" => {
                self.templates_dialog = Some(graphics::TemplatesDialog::default());
                return true;
            }
            "PH" => {
                self.phasing_dialog = Some(phasing::PhasingDialog::default());
                return true;
            }
            "DO" => {
                self.options_dialog = Some(phasing::OptionsDialog::default());
                return true;
            }
            // Revit's snap overrides: for the next point picked, that snap alone, or none.
            "SE" | "SM" | "SI" | "SC" | "SP" | "SQ" | "ST" | "SN" | "SO" | "SS" => {
                self.snap_once = match self.typed.as_str() {
                    "SQ" => Some(Once::Only(Snap::Quadrant)),
                    "ST" => Some(Once::Only(Snap::Tangent)),
                    "SE" => Some(Once::Only(Snap::Endpoint)),
                    "SM" => Some(Once::Only(Snap::Midpoint)),
                    "SI" => Some(Once::Only(Snap::Intersection)),
                    "SC" => Some(Once::Only(Snap::Centre)),
                    "SP" => Some(Once::Only(Snap::Perpendicular)),
                    "SN" => Some(Once::Only(Snap::Nearest)),
                    "SO" => Some(Once::Off),
                    _ => None,
                };
                return true;
            }
            code if mep::fixture_with(code).is_some() => Tool::Fixture(mep::fixture_with(code).map_or("", |f| f.key)),
            code if mep::run_with(code).is_some() => Tool::Run(mep::run_with(code).unwrap_or("beam"), None),
            code => match engine::find(code).map(|c| c.id) {
                Some("wall") => Tool::Wall(Vec::new()),
                Some("door") => Tool::Door,
                Some("window") => Tool::Window,
                Some("room") => Tool::Room,
                Some("level") => Tool::Level,
                Some("grid") => Tool::Grid(None),
                Some("move") => Tool::Move(None),
                Some("copy") => Tool::Copy(None),
                Some("rotate") => Tool::Rotate { centre: P2(0.0, 0.0), start: None, place: false },
                Some("corner") => Tool::Trim(None),
                Some("split") => Tool::Split,
                Some("linework") => Tool::Linework,
                Some("detail_group") => Tool::DetailGroup,
                Some("repeating_detail") => Tool::RepeatingDetail(None),
                Some("create_group") => {
                    self.create_group();
                    return true;
                }
                Some("ungroup") => {
                    self.ungroup();
                    return true;
                }
                Some("dimension") => Tool::Dimension(Vec::new()),
                Some("text") => Tool::Text,
                Some("detail_line") => {
                    (self.drawing.insulation, self.drawing.cloud) = (None, None);
                    Tool::DetailLine(Vec::new())
                }
                Some("spot") => Tool::Spot,
                Some("tag") => Tool::Tag,
                // Revit's CL is the structural column on its Structure tab, the architectural one elsewhere.
                Some("column") if self.tab == Tab::Structure || (self.tab == Tab::Context && self.back == Tab::Structure) => {
                    Tool::Stand("structuralcolumn")
                }
                Some("column") => Tool::Column,
                Some("footing") => Tool::Stand("footing"),
                Some("mass") => Tool::Mass(Vec::new()),
                Some("show_mass") => {
                    self.show_mass();
                    return true;
                }
                Some("filled_region") => {
                    if let Some(view) = self.plan_key() {
                        self.start_sketch(sketch::Of::Region { view, masking: false }, Vec::new());
                    }
                    return true;
                }
                Some(c @ ("mass_floors" | "wall_by_face" | "floor_by_face" | "roof_by_face" | "update_by_face")) => {
                    self.by_mass(c);
                    return true;
                }
                Some("component") => Tool::Component,
                Some("section") => Tool::Section(None),
                Some("measure") => Tool::Measure(None),
                Some("paint") => Tool::Paint,
                Some("match_properties") => {
                    self.match_properties();
                    return true;
                }
                Some("delete") => {
                    self.delete();
                    return true;
                }
                Some("fillet") => {
                    self.set_tool(Tool::DetailLine(Vec::new()));
                    self.set_shape(draw::Target::Detail, draw::Shape::Fillet);
                    return true;
                }
                Some("join_polyline") => {
                    self.join_polyline();
                    return true;
                }
                Some("explode") => {
                    self.explode_polyline();
                    return true;
                }
                // Revit's opens the Project Units dialog; BIMCraft's is the Manage tab's menu.
                Some("units") => {
                    self.tab = Tab::Manage;
                    self.open_units = true;
                    return true;
                }
                _ => return false,
            },
        };
        self.set_tool(tool);
        true
    }

    /// The snaps in force for the next point picked: Manage > Snaps, unless a shortcut (SE, SM, SI,
    /// SC, SP, SN, SO) has overridden them for it.
    pub(crate) fn snaps_now(&self) -> Snaps {
        match self.snap_once {
            Some(Once::Only(kind)) => Snaps::only(kind),
            Some(Once::Off) => Snaps::none(),
            None => self.snaps,
        }
    }

    /// What the status bar says of a snap override waiting for the next point.
    fn snap_note(&self) -> Option<String> {
        match self.snap_once? {
            Once::Only(kind) => Some(tf("Snap: {} (next point)", &[t(kind.name())])),
            Once::Off => Some(t("Snaps off (next point)").to_string()),
        }
    }

    /// Create Similar (CS): the tool that makes what's selected, one element, with its type picked.
    fn create_similar(&mut self) {
        let m = Arc::clone(&self.session.model);
        let picked = match self.selection.as_slice() {
            &[id] => m.elements.get(id).map(|e| (id, e)),
            _ => None,
        };
        let tool = match picked.map(|(_, e)| e) {
            Some(Element::Wall(_)) => Tool::Wall(Vec::new()),
            Some(Element::Door(_)) => Tool::Door,
            Some(Element::Window(_)) => Tool::Window,
            Some(Element::Floor(_)) => Tool::Floor,
            Some(Element::Ceiling(_)) => Tool::Ceiling,
            Some(Element::Shaft(_)) => Tool::Shaft(None),
            Some(Element::Mass(_)) => Tool::Mass(Vec::new()),
            Some(Element::Roof(r)) if r.extrusion.is_some() => Tool::ExtrusionRoof(Vec::new()),
            Some(Element::Roof(_)) => Tool::Roof,
            Some(Element::Column(_)) => Tool::Column,
            Some(Element::StructuralColumn(_)) => Tool::Stand("structuralcolumn"),
            Some(Element::Footing(_)) => Tool::Stand("footing"),
            Some(e) if e.run().is_some() => Tool::Run(e.category(), None),
            Some(Element::Component(c)) => {
                self.component = Some((c.family.clone(), c.type_name.clone()));
                Tool::Component
            }
            Some(Element::Stair(_)) => Tool::Stair(Vec::new()),
            Some(Element::Ramp(_)) => Tool::Ramp(Vec::new()),
            Some(Element::Railing(_)) => Tool::Railing(Vec::new()),
            Some(Element::Room(_)) => Tool::Room,
            Some(Element::Level(_)) => Tool::Level,
            Some(Element::Grid(_)) => Tool::Grid(None),
            Some(Element::Dimension(d)) => match d.kind {
                DimKind::Aligned => Tool::Dimension(Vec::new()),
                DimKind::Angular => Tool::Angular(Vec::new()),
                DimKind::Radial => Tool::Radial(None),
            },
            Some(Element::Text(_)) => Tool::Text,
            Some(Element::DetailLine(l)) => {
                self.line_style.clone_from(&l.style);
                Tool::DetailLine(Vec::new())
            }
            Some(Element::Spot(_)) => Tool::Spot,
            Some(Element::Tag(_)) => Tool::Tag,
            Some(Element::Section(_)) => Tool::Section(None),
            Some(Element::Elevation(_)) => Tool::Elevation,
            // Nothing, several, or a sheet's viewport: nothing to place one like.
            _ => {
                self.status = Status { text: t("select one element to create one like it first").into(), error: true };
                return;
            }
        };
        if let Some((id, e)) = picked
            && let Some(name) = element_json(&m, id)["type"].as_str()
        {
            self.types.insert(e.category(), name.to_string());
        }
        self.set_tool(tool);
        // A structural floor's likeness is structural.
        self.floor_structural = matches!(picked.map(|(_, e)| e), Some(Element::Floor(f)) if f.structural);
    }

    /// Select All Instances (SA): every element the open view shows of a type selected, a
    /// component's type being its family's.
    fn select_all_instances(&mut self) {
        let m = Arc::clone(&self.session.model);
        let type_of = |id: &Id| {
            let e = element_json(&m, *id);
            e["type"].is_string().then(|| [e["category"].clone(), e["family"].clone(), e["type"].clone()])
        };
        let types: Vec<[Value; 3]> = self.selection.iter().filter_map(type_of).collect();
        if types.is_empty() {
            self.status = Status { text: t("select an element that has a type first").into(), error: true };
            return;
        }
        let found = self.shown().into_iter().filter(|id| type_of(id).is_some_and(|k| types.contains(&k))).collect();
        self.pick_up(found, Modifiers::NONE);
    }

    /// Match Properties (MA): with one element selected, its type is the one the next element drawn
    /// of its category takes; with several, the `match_properties` command gives the first one's type
    /// to the others of its category.
    pub(crate) fn match_properties(&mut self) {
        let Some(&first) = self.selection.first() else {
            self.status = Status { text: t("Select an element first to match its properties").into(), error: true };
            return;
        };
        let src = element_json(&self.session.model, first);
        let Some(typ) = src["type"].as_str().filter(|t| !t.is_empty()) else {
            self.status = Status { text: t("The selected element has no type to match").into(), error: true };
            return;
        };
        if self.selection.len() == 1 {
            if let Some(el) = self.session.model.elements.get(first) {
                self.types.insert(el.category(), typ.to_string());
            }
            if let Some(f) = src["family"].as_str() {
                self.component = Some((f.to_string(), typ.to_string()));
            }
            self.status = Status {
                text: tf("Match Properties: source type '{}' selected. Select target elements and press MA again.", &[typ]),
                error: false,
            };
        } else if let Some(r) = self.run("match_properties", json!({"from": first, "ids": &self.selection[1..]})) {
            let n = r["modified"].as_array().map_or(0, Vec::len);
            self.status =
                Status { text: tf("Matched properties: applied type '{}' to {} element(s)", &[typ, &n.to_string()]), error: false };
        }
    }

    /// What the open view shows: what a plan draws, in its view range and crop region; what a
    /// section or an elevation cuts and what its picture shows past its line; in 3D, the whole
    /// model but for what is hidden. Nothing on a sheet or in a schedule has a type to select.
    fn shown(&mut self) -> BTreeSet<Id> {
        let m = Arc::clone(&self.session.model);
        let owners = |d: render::Drawing| d.owners.into_iter().flatten().collect::<BTreeSet<Id>>();
        // Fitted at any size: what a view shows doesn't depend on it.
        let (w, h) = (1600, 1000);
        match self.sheet {
            Sheet::Plan(level) => owners(render::plan(&m, level, w, h)),
            Sheet::Ceiling(level) => owners(render::ceiling_plan(&m, level, w, h)),
            Sheet::Section(id) => {
                let view = render::section_fit(&m, id, w, h);
                let scene = orbit::scene_of(&mut self.orbit.scene, &m);
                // ponytail: the picture leaves out what's hidden behind something nearer; test the
                // solids against the view's extent if SA should find those too.
                let seen = render::section_image(&m, scene, id, view, &[]).map(|i| i.shows()).unwrap_or_default();
                owners(render::section(&m, id, view)).into_iter().chain(seen).collect()
            }
            // The whole model, but for what the default 3D view hides.
            Sheet::View3D => {
                let look = bimcraft_core::graphics::Look::of_3d(&m);
                m.elements.keys().copied().filter(|id| look.shows(*id)).collect()
            }
            Sheet::Paper(_) | Sheet::Schedule(_) | Sheet::Table(_) => BTreeSet::new(),
        }
    }

    /// The material the Paint tool lays: the one picked while the library has it, else the library's first.
    pub(crate) fn painting(&self) -> Option<String> {
        let m = &self.session.model;
        Some(self.paint_material.clone()).filter(|n| m.materials.contains_key(n)).or_else(|| m.materials.keys().next().cloned())
    }

    fn set_tool(&mut self, mut tool: Tool) {
        // Trim/Extend to Corner with two lines or walls selected (or picked, sketching) joins them at once, keeping the
        // parts toward their middles; with any other selection it asks for the picks.
        if tool == Tool::Trim(None) && self.corner_selected() {
            return;
        }
        // Paint works in the 3D view: open it, as the tool asks.
        if matches!(tool, Tool::Paint | Tool::Unpaint) && self.sheet != Sheet::View3D {
            self.open_sheet(Sheet::View3D);
        }
        // Move, Copy, Rotate and Mirror take the selection; Align, Trim, Split and Offset pick walls, Demolish anything.
        let edits = matches!(tool, Tool::Move(_) | Tool::Copy(_) | Tool::Rotate { .. } | Tool::Mirror { .. });
        let picks = matches!(
            tool,
            Tool::Align(_)
                | Tool::Trim(_)
                | Tool::TrimTo { .. }
                | Tool::Vertex { .. }
                | Tool::Split
                | Tool::Offset
                | Tool::Demolish
                | Tool::Linework
        );
        if edits && self.selection.is_empty() {
            self.status = Status { text: t("select what to move, copy, rotate or mirror first").into(), error: true };
            return;
        }
        if tool == Tool::Attach && self.selected_walls().is_empty() {
            self.status = Status { text: t("select the walls to attach first").into(), error: true };
            return;
        }
        if matches!(tool, Tool::SlopeArrow(_)) && self.selected_floor().is_none() {
            self.status = Status { text: t("select the floor to slope first").into(), error: true };
            return;
        }
        if let Tool::Rotate { centre, .. } = &mut tool
            && let Some(middle) = self.middle()
        {
            *centre = middle;
        }
        if tool == Tool::Level {
            let top = self.session.model.levels().last().map_or(0.0, |(_, l)| l.elevation);
            self.new_level = (self.session.model.units.length(top + 3000.0), String::new());
        }
        if tool != Tool::Modify && tool != Tool::Attach && tool != Tool::GableEdge && !matches!(tool, Tool::SlopeArrow(_)) && !edits {
            self.selection.clear();
        }
        if tool != Tool::Modify && !picks {
            self.raise_context();
        }
        self.typed.clear();
        self.clear_typed();
        // Insulation and Revision Cloud are the Detail Line tool's; the lines Fillet Arc and Trim/Extend have picked were
        // picked for them, and are not for the tool that comes next.
        if !matches!(tool, Tool::DetailLine(_)) {
            (self.drawing.insulation, self.drawing.cloud) = (None, None);
        }
        self.forget_picks();
        // The Floor tool makes structural floors only when the Structure tab asks (`structural_floor`).
        self.floor_structural = false;
        if let Tool::Run(category, _) = &tool {
            self.keep_run_options(category);
        }
        if !matches!(tool, Tool::Measure(_)) {
            self.measured = None;
        }
        self.snap_once = None;
        self.tool = tool;
    }

    /// The walls among the selection.
    fn selected_walls(&self) -> Vec<Id> {
        self.selection.iter().copied().filter(|id| self.session.model.wall(*id).is_some()).collect()
    }

    /// The roof selected, when one roof is all that is.
    pub(crate) fn selected_roof(&self) -> Option<Id> {
        match self.selection.as_slice() {
            &[id] if matches!(self.session.model.elements.get(id), Some(Element::Roof(_))) => Some(id),
            _ => None,
        }
    }

    /// The floor selected, when one floor is all that is.
    pub(crate) fn selected_floor(&self) -> Option<Id> {
        match self.selection.as_slice() {
            &[id] if matches!(self.session.model.elements.get(id), Some(Element::Floor(_))) => Some(id),
            _ => None,
        }
    }

    /// The selected walls turned round about their location lines: Revit's flip arrows, or Space.
    fn flip(&mut self) {
        let walls = self.selected_walls();
        if !walls.is_empty() {
            self.run("flip", json!({"ids": walls}));
        }
    }

    /// The middle of what's selected: where Rotate turns it about until told otherwise.
    fn middle(&self) -> Option<P2> {
        let m = &self.session.model;
        let mut pts = Vec::new();
        for id in &self.selection {
            match m.elements.get(id) {
                Some(Element::Wall(w)) => pts.extend([w.start, w.end]),
                Some(Element::Grid(g)) => pts.extend([g.start, g.end]),
                Some(Element::Floor(f)) => pts.extend(&f.boundary),
                Some(Element::Room(r)) => pts.push(r.point),
                Some(Element::Door(_) | Element::Window(_)) => pts.extend(m.anchor(*id)),
                Some(Element::Dimension(d)) => pts.push(d.at),
                Some(Element::Section(v) | Element::Elevation(v)) => pts.extend([v.start, v.end]),
                Some(Element::Tag(t)) => pts.extend(m.anchor(t.host).map(|a| a + t.offset)),
                Some(Element::Column(c)) => pts.push(c.at),
                Some(Element::Component(c)) => pts.push(c.at),
                Some(e @ (Element::Stair(_) | Element::Ramp(_))) => {
                    pts.extend(stairs::layout(m, e).into_iter().flat_map(|l| l.walk.into_iter().map(|(p, _)| p)));
                }
                Some(Element::Railing(rl)) => pts.extend(stairs::railing(m, rl).into_iter().map(|(p, _)| p)),
                Some(Element::Roof(r)) => pts.extend(&r.boundary),
                _ => {}
            }
        }
        let first = *pts.first()?;
        let (lo, hi) = pts.iter().fold((first, first), |(lo, hi), p| (P2(lo.0.min(p.0), lo.1.min(p.1)), P2(hi.0.max(p.0), hi.1.max(p.1))));
        Some(P2((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0))
    }

    /// The point the tool measures its next one from, while it has one.
    fn base(&self) -> Option<P2> {
        match &self.tool {
            Tool::Wall(chain) => chain.last().copied(),
            Tool::Run(_, start) => *start,
            Tool::Grid(start) | Tool::Move(start) | Tool::Copy(start) | Tool::Section(start) | Tool::SlopeArrow(start) => *start,
            Tool::Stair(pts) | Tool::Ramp(pts) | Tool::Railing(pts) | Tool::Mass(pts) | Tool::DetailLine(pts) | Tool::SketchLine(pts) => {
                pts.last().copied()
            }
            _ => None,
        }
    }

    /// The level of the plan on screen, a floor plan or a ceiling plan (of a plan view, its level's).
    pub(crate) fn plan_level(&self) -> Option<Id> {
        self.plan_key().and_then(|key| self.session.model.plan_level(key))
    }

    /// The key of the plan on screen: its level's, or a plan view's.
    pub(crate) fn plan_key(&self) -> Option<Id> {
        match self.sheet {
            Sheet::Plan(key) | Sheet::Ceiling(key) => Some(key),
            _ => None,
        }
    }

    /// Escape: dismiss the error, drop the chain being drawn, else leave the tool, else clear the
    /// selection.
    fn escape(&mut self) {
        self.typed.clear();
        self.ribbon_peek = false;
        // A snap override waiting for its point goes first, leaving what's being drawn.
        if self.snap_once.take().is_some() {
            return;
        }
        self.clear_typed();
        self.status = Status::default();
        self.tool = match std::mem::replace(&mut self.tool, Tool::Modify) {
            Tool::Wall(chain) if !chain.is_empty() => Tool::Wall(Vec::new()),
            Tool::Grid(Some(_)) => Tool::Grid(None),
            Tool::Run(c, Some(_)) => Tool::Run(c, None),
            Tool::Mirror { draw, start: Some(_) } => Tool::Mirror { draw, start: None },
            Tool::Align(Some(_)) => Tool::Align(None),
            Tool::Trim(Some(_)) => Tool::Trim(None),
            Tool::TrimTo { boundary: Some(_), many } => Tool::TrimTo { boundary: None, many },
            Tool::Dimension(refs) if !refs.is_empty() => Tool::Dimension(Vec::new()),
            Tool::Angular(refs) if !refs.is_empty() => Tool::Angular(Vec::new()),
            Tool::Radial(Some(_)) => Tool::Radial(None),
            Tool::Section(Some(_)) => Tool::Section(None),
            Tool::Measure(Some(_)) => Tool::Measure(None),
            // The ruler laid goes first, then the tool.
            Tool::Measure(None) if self.measured.take().is_some() => Tool::Measure(None),
            Tool::Stair(pts) if !pts.is_empty() => Tool::Stair(Vec::new()),
            Tool::Ramp(pts) if !pts.is_empty() => Tool::Ramp(Vec::new()),
            Tool::Railing(pts) if !pts.is_empty() => Tool::Railing(Vec::new()),
            Tool::Mass(pts) if !pts.is_empty() => Tool::Mass(Vec::new()),
            Tool::DetailLine(pts) if !pts.is_empty() => Tool::DetailLine(Vec::new()),
            Tool::Shaft(Some(_)) => Tool::Shaft(None),
            Tool::SlopeArrow(Some(_)) => Tool::SlopeArrow(None),
            Tool::ExtrusionRoof(pts) if !pts.is_empty() => Tool::ExtrusionRoof(Vec::new()),
            Tool::SketchLine(pts) if !pts.is_empty() => Tool::SketchLine(Vec::new()),
            // Fillet Arc's lines picked so far go first, then the tool.
            Tool::DetailLine(pts) if !self.drawing.fillet.is_empty() => {
                self.drawing.fillet.clear();
                Tool::DetailLine(pts)
            }
            Tool::SketchLine(pts) if !self.drawing.fillet.is_empty() => {
                self.drawing.fillet.clear();
                Tool::SketchLine(pts)
            }
            // While sketching, Trim/Extend's first pick likewise.
            tool @ (Tool::Trim(_) | Tool::TrimTo { .. }) if self.drawing.trim.is_some() => {
                self.drawing.trim = None;
                tool
            }
            Tool::Modify => {
                match &mut self.sketch {
                    Some(sk) => sk.picked.clear(),
                    None => self.selection.clear(),
                }
                Tool::Modify
            }
            _ => Tool::Modify,
        };
    }

    fn delete(&mut self) {
        if self.sketch.is_some() {
            self.delete_sketch_lines();
            return;
        }
        if !self.selection.is_empty() {
            self.run("delete", json!({"ids": self.selection}));
        }
    }

    /// Zoom the view on screen by `factor` about its middle.
    fn zoom_view(&mut self, factor: f64) {
        match self.sheet {
            Sheet::Plan(id) | Sheet::Ceiling(id) | Sheet::Section(id) | Sheet::Paper(id) => self.canvas.zoom(id, factor),
            Sheet::View3D => {
                if let Some(c) = self.orbit.camera {
                    self.orbit.camera = Some(c.zoom(factor, (f64::from(c.width) / 2.0, f64::from(c.height) / 2.0)));
                }
            }
            Sheet::Schedule(_) | Sheet::Table(_) => {}
        }
    }

    fn fit(&mut self) {
        match self.sheet {
            Sheet::Plan(id) | Sheet::Ceiling(id) | Sheet::Section(id) | Sheet::Paper(id) => self.canvas.fit(id),
            Sheet::View3D => self.fit3d(),
            Sheet::Schedule(_) | Sheet::Table(_) => {}
        }
    }

    /// Open a view in a tab, or bring its tab forward.
    fn open_sheet(&mut self, sheet: Sheet) {
        if !self.views.contains(&sheet) && !self.floats(sheet) {
            self.views.push(sheet);
        }
        self.sheet = sheet;
        if !self.floats(sheet) {
            self.front = sheet;
        }
        // The selection is the model's, not the view's: it stays, whatever view comes up.
        self.clear_typed();
        self.snap_once = None;
        // A ruler belongs to the view it was laid in.
        self.measured = None;
        // The tools that pick points work in plans (Text in sections and on sheets too, Spot
        // Elevation in sections, Demolish in sections and 3D); any other view takes clicks to select.
        let works = match sheet {
            Sheet::Plan(_) | Sheet::Ceiling(_) => true,
            Sheet::Section(_) => matches!(self.tool, Tool::Text | Tool::Spot | Tool::Demolish),
            Sheet::View3D => matches!(self.tool, Tool::Demolish | Tool::Paint | Tool::Unpaint),
            Sheet::Paper(_) => self.tool == Tool::Text,
            _ => false,
        };
        // (A sketch is held in its plan: it takes the window back, with its tool, [`BimApp::track_sketch`].)
        if !works && self.tool != Tool::Level && self.sketch_plan().is_none() {
            self.tool = Tool::Modify;
        }
        match &mut self.tool {
            Tool::Wall(chain) => chain.clear(),
            Tool::Grid(start) | Tool::Run(_, start) => *start = None,
            Tool::Dimension(refs) | Tool::Angular(refs) => refs.clear(),
            Tool::Radial(r) => *r = None,
            Tool::Section(start) | Tool::Shaft(start) | Tool::Measure(start) => *start = None,
            Tool::Stair(pts)
            | Tool::Ramp(pts)
            | Tool::Railing(pts)
            | Tool::ExtrusionRoof(pts)
            | Tool::Mass(pts)
            | Tool::DetailLine(pts) => pts.clear(),
            Tool::Move(_) | Tool::Copy(_) | Tool::SlopeArrow(_) | Tool::GableEdge => self.tool = Tool::Modify,
            _ => {}
        }
    }

    /// The views on no sheet yet, as Place View lists them: plans, sections and elevations, the
    /// 3D view and the schedules; each as `place` takes it, by id, or by name while it isn't an
    /// element yet.
    fn unplaced_views(&self) -> Vec<(Value, String)> {
        let m = &self.session.model;
        let placed: BTreeSet<&str> = m
            .elements
            .values()
            .filter_map(|e| match e {
                Element::Viewport(v) => m.view_name(v.view),
                _ => None,
            })
            .collect();
        let plans = m.levels().into_iter().flat_map(|(id, l)| {
            let own = std::iter::once((json!(id), tf("Floor Plan: {}", &[&l.name]), l.name.as_str()));
            own.chain(m.plan_views(id).into_iter().map(|(v, pv)| (json!(v), tf("Floor Plan: {}", &[&pv.name]), pv.name.as_str())))
        });
        let cuts = m.elements.iter().filter_map(|(id, e)| match e {
            Element::Section(v) => Some((json!(id), tf("Section: {}", &[&v.name]), v.name.as_str())),
            Element::Elevation(v) => Some((json!(id), tf("Elevation: {}", &[&v.name]), v.name.as_str())),
            _ => None,
        });
        // The schedules every project has, by their English names, then the project's others.
        let builtin = SCHEDULES.iter().map(|(cat, name)| match schedule::standard(m, cat) {
            (Some(id), sc) => (json!(id), tf("Schedule: {}", &[&sc.name]), m.view_name(id).unwrap_or(name)),
            (None, _) => (json!(name), tf("Schedule: {}", &[t(name)]), *name),
        });
        let made = self.schedules().into_iter().map(|(id, name)| (json!(id), tf("Schedule: {}", &[name]), name));
        let others = std::iter::once((json!("{3D}"), t("3D View: {3D}").to_string(), "{3D}")).chain(builtin).chain(made);
        plans.chain(cuts).chain(others).filter(|(_, _, name)| !placed.contains(name)).map(|(v, label, _)| (v, label)).collect()
    }

    /// The project's schedules other than those every project has: their ids and names, by name.
    pub(crate) fn schedules(&self) -> Vec<(Id, &str)> {
        let m = &self.session.model;
        let mut out: Vec<(Id, &str)> = m
            .elements
            .iter()
            .filter_map(|(id, e)| match e {
                Element::Schedule(sc) if schedule::builtin(Some(&sc.name), None).is_none() => Some((*id, sc.name.as_str())),
                _ => None,
            })
            .collect();
        out.sort_by_key(|(_, name)| name.to_lowercase());
        out
    }

    /// Close a view's tab; the last one stays open.
    fn close_view(&mut self, sheet: Sheet) {
        if self.floats(sheet) {
            self.floating.retain(|f| f.sheet != sheet);
            if self.sheet == sheet {
                self.open_sheet(self.front);
            }
            return;
        }
        if self.views.len() < 2 {
            return;
        }
        self.views.retain(|v| *v != sheet);
        if self.sheet == sheet
            && let Some(&last) = self.views.last()
        {
            self.open_sheet(last);
        }
    }

    // ---------- files ----------

    /// Save the project (`.bimcraft`, or any other name) or export by extension: `.ifc`, or the
    /// view on screen as `.png` (a plan or the 3D view) or `.svg` (a plan). Whether it worked; the
    /// warning box says why not.
    pub fn save_to(&mut self, path: &Path) -> bool {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let file = path.to_string_lossy();
        if matches!(ext.as_str(), "png" | "svg") {
            let mut p = json!({"path": file, "width": 1600, "height": 1000});
            match (self.sheet, self.orbit.camera) {
                (Sheet::Plan(id) | Sheet::Section(id) | Sheet::Paper(id), _) => p["view"] = json!(id),
                (Sheet::Ceiling(id), _) => (p["view"], p["ceiling"]) = (json!(id), json!(true)),
                (Sheet::View3D, camera) => {
                    p["view"] = json!("{3D}");
                    if let Some(c) = camera {
                        (p["azimuth"], p["elevation"]) = (json!(c.azimuth), json!(c.elevation));
                    }
                }
                (Sheet::Schedule(_) | Sheet::Table(_), _) => {}
            }
            return self.run("render", p).is_some();
        }
        let saved = self.run("save", json!({"path": file})).is_some();
        // Exports (IFC, the sheets' PDF) leave the project unsaved.
        if saved && !matches!(ext.as_str(), "ifc" | "pdf") {
            self.saved = Arc::clone(&self.session.model);
            self.note_recent(path);
            self.autosave_saved();
        }
        saved
    }

    /// Saves to the project's file, or asks where first. `then` is done once it is saved.
    fn save(&mut self, ctx: &Context, then: Option<Pending>) {
        match self.session.path.clone() {
            Some(p) => {
                if self.save_to(&p)
                    && let Some(then) = then
                {
                    self.proceed(then, ctx);
                }
            }
            None => self.save_as(ctx, then),
        }
    }

    fn save_as(&mut self, ctx: &Context, then: Option<Pending>) {
        let later = ctx.clone();
        self.ask_path(ctx, "BIMCraft project", "bimcraft", move |app, p| {
            if app.save_to(&p)
                && let Some(then) = then
            {
                app.proceed(then, &later);
            }
        });
    }

    fn export(&mut self, ctx: &Context, what: &str, ext: &str) {
        self.ask_path(ctx, what, ext, |app, p| {
            app.save_to(&p);
        });
    }

    /// Asks, in the system's file dialog, where to save, and calls `then` with the path, ending in `.ext`, in a later frame.
    fn ask_path(&mut self, ctx: &Context, what: &str, ext: &str, then: impl FnOnce(&mut BimApp, PathBuf) + 'static) {
        let name = format!("{}.{ext}", self.session.model.name.replace(['/', '\\'], "-"));
        let ext = ext.to_string();
        let dialog = rfd::AsyncFileDialog::new().add_filter(t(what), &[&ext]).set_file_name(name);
        self.ask_file(ctx, dialog, true, move |app, p| {
            then(app, if p.extension().is_some_and(|e| e.eq_ignore_ascii_case(&ext)) { p } else { p.with_extension(&ext) });
        });
    }

    /// Open a project, or import an IFC file and say what came in and what was left out.
    pub(crate) fn open_path(&mut self, path: &Path) -> bool {
        if revit::is_revit(path) {
            return self.open_revit(path);
        }
        let Some(reply) = self.run("open", json!({"path": path.to_string_lossy()})) else {
            self.forget_recent_if_gone(path);
            // Even a damaged original can have a healthy recovery copy.
            if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ifc")) {
                self.inspect_recovery(path.to_path_buf());
            }
            return false;
        };
        self.retire_autosave();
        self.reopened();
        self.note_recent(path);
        self.autosave_document();
        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        if reply["format"].is_string() {
            self.status = Status { text: import_note(&name, &reply), error: false };
        } else if let Some(left_out) = reply["left_out"].as_array() {
            let count = reply["left_out_count"].as_u64().map_or(left_out.len(), |n| n as usize);
            // A newer BIMCraft's fields: what this one doesn't know, it can't keep.
            let text =
                tf("{} opened without {} fields this version doesn't know; saving it leaves them out.", &[&name, &count.to_string()]);
            self.status = Status { text, error: false };
        }
        true
    }

    /// Start over on the model the session holds now (after New and Open), keeping how the
    /// window is set up.
    fn reopened(&mut self) {
        let mut fresh = BimApp::new(std::mem::take(&mut self.session));
        fresh.status = std::mem::take(&mut self.status);
        fresh.os_events = self.os_events.take();
        (fresh.own_title_bar, fresh.system_title_bar) = (self.own_title_bar, self.system_title_bar);
        (fresh.thin, fresh.lang, fresh.snaps, fresh.ribbon_min) = (self.thin, self.lang, self.snaps, self.ribbon_min);
        fresh.compact = self.compact;
        fresh.font_finder = self.font_finder;
        fresh.scan_contrast = self.scan_contrast;
        fresh.light_canvas = self.light_canvas;
        fresh.styled = self.styled;
        fresh.theme_wanted = self.theme_wanted.take();
        fresh.docking = std::mem::take(&mut self.docking);
        (fresh.settings_path, fresh.settings_kept) = (self.settings_path.take(), self.settings_kept.take());
        fresh.window = std::mem::take(&mut self.window);
        (fresh.updates, fresh.revit) = (self.updates.take(), self.revit.take());
        fresh.control = self.control.take();
        fresh.opens = std::mem::take(&mut self.opens);
        (fresh.recent, fresh.recent_path) = (std::mem::take(&mut self.recent), self.recent_path.take());
        fresh.autosave = std::mem::take(&mut self.autosave);
        fresh.card = std::mem::take(&mut self.card);
        fresh.home = self.home.put_away();
        *self = fresh;
    }

    /// What the system asked for, as File > Open and closing the window do it: a file opened in turn
    /// (the last one wins), and Quit, each asking first about unsaved changes.
    fn os_requests(&mut self, ctx: &Context) {
        // An update being put in place holds the window, so what the system asks waits in its inbox until that has gone through
        // (the window closes) or failed.
        if self.installing() {
            return;
        }
        let events = self.os_events.as_mut().map(|poll| poll()).unwrap_or_default();
        for event in events {
            match event {
                OsEvent::Open(path) => self.opens.push(path),
                OsEvent::Quit => self.guard(Pending::Close, ctx),
            }
        }
        // A file dropped on the window opens as one the system asked for.
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        self.opens.extend(dropped);
        // A Revit file being read holds the files that come meanwhile, which are opened in turn once it is over: the last one wins.
        if !self.importing()
            && let Some(path) = self.opens.pop()
        {
            self.opens.clear();
            self.guard(Pending::OpenPath(path), ctx);
        }
    }

    /// Do `p` now, or first ask about unsaved changes.
    fn guard(&mut self, p: Pending, ctx: &Context) {
        if self.dirty() { self.pending = Some(p) } else { self.proceed(p, ctx) }
    }

    fn proceed(&mut self, p: Pending, ctx: &Context) {
        match p {
            Pending::Recover(backup) => self.recover_backup(backup),
            Pending::New => {
                // In the interface's language: its levels named in it.
                if self.run("new", json!({"language": self.lang.project().code()})).is_some() {
                    self.retire_autosave();
                    self.reopened();
                    self.autosave_document();
                }
            }
            Pending::OpenPath(path) => {
                self.open_path(&path);
            }
            Pending::Open => {
                let revit = self.revit.is_some();
                let name = t(if revit { "BIMCraft project, IFC or Revit" } else { "BIMCraft project or IFC" });
                let dialog = rfd::AsyncFileDialog::new().add_filter(name, &revit::open_extensions(revit));
                self.ask_file(ctx, dialog, false, |app, path| {
                    app.open_path(&path);
                });
            }
            Pending::Close => {
                self.retire_autosave();
                self.closing = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            Pending::Update => {
                self.retire_autosave();
                self.install_update();
            }
        }
    }

    // ---------- the top: Quick Access Toolbar, ribbon, Options Bar ----------

    fn quick_access(&mut self, ui: &mut Ui, view: &str) {
        let ctx = ui.ctx().clone();
        let row = ui
            .horizontal(|ui| {
                if icon_button(ui, Icon::Home, "Home").clicked() {
                    self.toggle_home();
                }
                let file = egui::Button::new(RichText::new(t("File")).color(pal().app_bar_text)).frame(false).min_size(vec2(44.0, 24.0));
                MenuButton::from_button(file).ui(ui, |ui| self.file_menu(ui, &ctx));
                if icon_button(ui, Icon::Open, "Open").clicked() {
                    self.guard(Pending::Open, &ctx);
                }
                if icon_button(ui, Icon::Save, "Save").clicked() {
                    self.save(&ctx, None);
                }
                ui.separator();
                ui.spacing_mut().item_spacing.x = 0.0;
                if icon_button(ui, Icon::Undo, "Undo").clicked() {
                    self.run("undo", json!({}));
                }
                self.steps_list(ui, false);
                ui.add_space(4.0);
                if icon_button(ui, Icon::Redo, "Redo").clicked() {
                    self.run("redo", json!({}));
                }
                self.steps_list(ui, true);
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.separator();
                if icon_button(ui, Icon::View3D, "Default 3D View").clicked() {
                    self.open_sheet(Sheet::View3D);
                }
                ui.separator();
                if icon_button(ui, Icon::Settings, "Settings").clicked() {
                    self.settings_page = Some(settings::Page::Interface);
                }
            })
            .response
            .rect;
        // The project and the view, centred where Revit's title bar has them: in the window's middle when they fit between the
        // buttons on the left and, where the window draws its own title bar, the caption buttons on the right; else as near it
        // as that room allows, and cut short where even it is too little.
        let gap = 12.0;
        let right = ui.max_rect().right() + if self.own_bar() { 8.0 - 3.0 * titlebar::BUTTON_WIDTH } else { 0.0 } - gap;
        let left = row.right() + gap;
        let mut title = egui::text::LayoutJob::simple_singleline(
            format!("{} · {view}", self.session.model.name),
            FontId::proportional(12.5),
            pal().app_bar_muted,
        );
        title.wrap = egui::text::TextWrapping::truncate_at_width((right - left).max(0.0));
        let galley = ui.painter().layout_job(title);
        let width = galley.size().x;
        let x = ui.max_rect().center().x.clamp(left + width / 2.0, (right - width / 2.0).max(left + width / 2.0));
        ui.painter().galley(pos2(x - width / 2.0, row.center().y - galley.size().y / 2.0), galley, pal().app_bar_muted);
    }

    /// The arrow beside Undo (or with `redo`, Redo): a list of the steps, the next first, as Revit's. Hovering one marks it
    /// and every step before it, and a click undoes (or redoes) them all.
    fn steps_list(&mut self, ui: &mut Ui, redo: bool) {
        let what = if redo { "Redo" } else { "Undo" };
        let names: Vec<String> = self.session.steps(redo).into_iter().map(step_name).collect();
        let (rect, arrow) = ui.allocate_exact_size(vec2(12.0, 22.0), Sense::click());
        let label = t(if redo { "Redo History" } else { "Undo History" });
        arrow.widget_info(|| WidgetInfo::labeled(WidgetType::Button, !names.is_empty(), label));
        let (icon, hover) = style::icon_inks();
        if arrow.hovered() && !names.is_empty() {
            ui.painter().rect_filled(rect, 3.0, hover);
        }
        let ink = if names.is_empty() { icon.gamma_multiply(0.4) } else { icon };
        let c = rect.center();
        ui.painter().add(egui::Shape::convex_polygon(
            vec![c + vec2(-3.0, -1.5), c + vec2(3.0, -1.5), c + vec2(0.0, 2.0)],
            ink,
            Stroke::NONE,
        ));
        if names.is_empty() {
            return;
        }
        let arrow = arrow.on_hover_text(label);
        let mut chosen = None;
        egui::Popup::menu(&arrow).width(240.0).show(|ui| {
            ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let rows: Vec<Response> = names
                    .iter()
                    .map(|name| {
                        let (_, row) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                        row.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
                        row
                    })
                    .collect();
                let reach = rows.iter().position(Response::hovered);
                for (i, (row, name)) in rows.iter().zip(&names).enumerate() {
                    if reach.is_some_and(|h| i <= h) {
                        ui.painter().rect_filled(row.rect, 0.0, pal().select_fill);
                    }
                    ui.painter().text(
                        row.rect.left_center() + vec2(8.0, 0.0),
                        Align2::LEFT_CENTER,
                        name,
                        FontId::proportional(13.0),
                        pal().text,
                    );
                    if row.clicked() {
                        chosen = Some(i + 1);
                    }
                }
                ui.separator();
                let n = reach.map_or(1, |h| h + 1);
                let foot = tf(if redo { "Redo {} steps" } else { "Undo {} steps" }, &[&n.to_string()]);
                let foot = if n == 1 { tf(if redo { "Redo {}" } else { "Undo {}" }, &[&names[0]]) } else { foot };
                ui.label(RichText::new(foot).color(pal().muted));
            });
        });
        if let Some(count) = chosen {
            self.run(&what.to_lowercase(), json!({"count": count}));
        }
    }

    fn ribbon(&mut self, ui: &mut Ui) {
        Frame::new().inner_margin(Margin { left: 6, right: 6, top: 5, bottom: 5 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                // The tabs, scrolling sideways (with the wheel) in a window too narrow for them; the fold button at the end.
                let room = ui.available_width() - 32.0;
                // The tab that has just been picked (or a contextual tab that has just come) is brought into view: the row
                // scrolls in a window too narrow for it, and its last tabs are out of sight.
                let seen = UiId::new("ribbon-tab-seen");
                let now = self.tab as u8;
                let changed = ui.ctx().data(|d| d.get_temp::<u8>(seen)) != Some(now);
                ui.ctx().data_mut(|d| d.insert_temp(seen, now));
                ScrollArea::horizontal()
                    .id_salt("ribbon-tabs")
                    .max_width(room)
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            for (tab, label) in [
                                (Tab::Architecture, "Architecture"),
                                (Tab::Structure, "Structure"),
                                (Tab::Plumbing, "Plumbing"),
                                (Tab::Electrical, "Electrical"),
                                (Tab::Lightning, "Lightning Protection"),
                                (Tab::Data, "Data"),
                                (Tab::Insert, "Insert"),
                                (Tab::Annotate, "Annotate"),
                                (Tab::Massing, "Massing"),
                                (Tab::View, "View"),
                                (Tab::Manage, "Manage"),
                                (Tab::Modify, "Modify"),
                            ] {
                                let r = tab_button(ui, label, self.tab == tab, false);
                                if changed && self.tab == tab {
                                    r.scroll_to_me(None);
                                }
                                self.ribbon_tab(&r, tab);
                            }
                            if let Some(name) = self.context() {
                                let r = tab_button(ui, &name, self.tab == Tab::Context, true);
                                if changed && self.tab == Tab::Context {
                                    r.scroll_to_me(None);
                                }
                                self.ribbon_tab(&r, Tab::Context);
                            }
                        });
                    });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (icon, label) =
                        if self.ribbon_min { (Icon::Expand, "Expand the Ribbon") } else { (Icon::Minimize, "Minimize the Ribbon") };
                    if icon_button(ui, icon, label).clicked() {
                        self.fold_ribbon(!self.ribbon_min);
                    }
                });
            });
        });
        let base = ui.min_rect();
        if !self.ribbon_min {
            ui.painter().hline(base.x_range(), base.bottom(), Stroke::new(1.0, pal().border));
        }
        self.ribbon_bottom = ui.min_rect().left_bottom();
        if !self.ribbon_min {
            self.ribbon_panels(ui);
        }
    }

    /// A ribbon tab's clicks: one picks it (and, while the ribbon is folded, shows its tools under the tabs, or hides
    /// them again), two fold the ribbon to its tabs or open it again, as Revit's do.
    fn ribbon_tab(&mut self, r: &Response, tab: Tab) {
        if r.double_clicked() {
            self.fold_ribbon(!self.ribbon_min);
        } else if r.clicked() {
            self.ribbon_peek = self.ribbon_min && !(self.ribbon_peek && self.tab == tab);
            self.tab = tab;
        }
    }

    fn fold_ribbon(&mut self, folded: bool) {
        self.ribbon_min = folded;
        self.ribbon_peek = false;
    }

    /// The folded ribbon's tools, under its tabs over everything else, until one is picked or a click lands elsewhere.
    fn ribbon_peek(&mut self, ctx: &Context) {
        let tool = self.tool.clone();
        let frame = Frame::popup(&ctx.global_style()).fill(pal().surface).inner_margin(0.0);
        let shown = Area::new(UiId::new("ribbon-peek"))
            .order(Order::Foreground)
            .fixed_pos(self.ribbon_bottom)
            .show(ctx, |ui| frame.show(ui, |ui| self.ribbon_panels(ui)).response.rect);
        let rect = shown.inner;
        let (released, at) = ctx.input(|i| (i.pointer.primary_released(), i.pointer.interact_pos()));
        let inside = at.is_some_and(|p| rect.contains(p));
        let on_tabs = at.is_some_and(|p| p.y < rect.top());
        // A tool picked, a button done (one that opens a menu keeps it open), or a click elsewhere.
        if self.tool != tool || (released && !Popup::is_any_open(ctx) && (inside || !on_tabs)) {
            self.ribbon_peek = false;
        }
    }

    /// The ribbon tab's panels of tools.
    fn ribbon_panels(&mut self, ui: &mut Ui) {
        Frame::new().inner_margin(Margin { left: 6, right: 6, top: 4, bottom: 4 }).show(ui, |ui| {
            // A tab wider than the window folds its last panels; one still too wide scrolls sideways, each tab where it was
            // left.
            let key = UiId::new(("ribbon-widths", self.tab as u8));
            let room = ui.available_width();
            egui::ScrollArea::horizontal().id_salt(self.tab as u8).auto_shrink([false, true]).show(ui, |ui| {
                ribbon::tab(ui, key, room, |ui| {
                    ui.horizontal(|ui| match self.tab {
                        Tab::Architecture => self.architecture_tab(ui),
                        Tab::Structure => self.structure_tab(ui),
                        Tab::Plumbing => self.plumbing_tab(ui),
                        Tab::Electrical => self.electrical_tab(ui),
                        Tab::Lightning => self.lightning_tab(ui),
                        Tab::Data => self.data_tab(ui),
                        Tab::Insert => self.insert_tab(ui),
                        Tab::Annotate => self.annotate_tab(ui),
                        Tab::Massing => self.massing_tab(ui),
                        Tab::View => self.view_tab(ui),
                        Tab::Manage => self.manage_tab(ui),
                        Tab::Modify | Tab::Context => self.modify_tab(ui),
                    });
                });
            });
        });
    }

    /// The application menu, under the File tab.
    fn file_menu(&mut self, ui: &mut Ui, ctx: &Context) {
        let item = |ui: &mut Ui, label: &str, keys: Option<KeyboardShortcut>| {
            let b = egui::Button::new(t(label));
            ui.add(match keys {
                Some(k) => b.shortcut_text(ctx.format_shortcut(&k)),
                None => b,
            })
            .clicked()
        };
        let cmd = |k: Key| Some(KeyboardShortcut::new(Modifiers::COMMAND, k));
        if item(ui, "New", cmd(Key::N)) {
            self.guard(Pending::New, ctx);
        }
        if item(ui, "Open…", cmd(Key::O)) {
            self.guard(Pending::Open, ctx);
        }
        self.recent_menu(ui, ctx);
        if item(ui, "Save", cmd(Key::S)) {
            self.save(ctx, None);
        }
        if item(ui, "Save As…", Some(KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::S))) {
            self.save_as(ctx, None);
        }
        if !self.autosave.candidates.is_empty() && item(ui, "Recover Autosave…", None) {
            self.autosave.show_recovery = true;
        }
        SubMenuButton::new(t("Export")).ui(ui, |ui| {
            if ui.button(t("IFC…")).clicked() {
                self.export(ctx, "IFC4", "ifc");
            }
            if ui.button(t("View as PNG…")).clicked() {
                self.export(ctx, "PNG image", "png");
            }
            if ui.button(t("Plan as SVG…")).clicked() {
                self.export(ctx, "SVG drawing", "svg");
            }
            if ui.button(t("Sheets as PDF…")).clicked() {
                self.export(ctx, "PDF", "pdf");
            }
        });
        self.report_menu(ui);
        self.update_menu(ui);
        ui.separator();
        if item(ui, "Settings…", Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma))) {
            self.settings_page = Some(settings::Page::Interface);
        }
        ui.separator();
        if item(ui, "Exit", None) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    /// Ribbon buttons that pick tools, under the panel's name.
    fn tools(&mut self, ui: &mut Ui, name: &str, tools: Tools) {
        panel(ui, name, |ui| {
            for (icon, label, tip, tool) in tools {
                let active = structure::same_tool(&self.tool, tool);
                if big_button(ui, *icon, label, active, tip).clicked() {
                    self.set_tool(tool.clone());
                }
            }
        });
    }

    fn massing_tab(&mut self, ui: &mut Ui) {
        let tip = "Mass (MS): click the corners of the footprint; Enter, or the first corner again, finishes; Esc cancels";
        let shown = !self.graphics_now().resolved(&self.session.model).categories.get("mass").is_some_and(|o| o.hidden);
        panel(ui, "Conceptual Mass", |ui| {
            if big_button(ui, Icon::Mass, "Mass", structure::same_tool(&self.tool, &Tool::Mass(Vec::new())), tip).clicked() {
                self.set_tool(Tool::Mass(Vec::new()));
            }
            let tip = "Show Mass (MH): hides the masses in this view, or shows them again";
            if big_button(ui, Icon::Visibility, "Show Mass", shown, tip).clicked() {
                self.show_mass();
            }
        });
        let selected = !self.selected_masses().is_empty();
        panel(ui, "Model by Face", |ui| {
            ui.add_enabled_ui(selected, |ui| {
                for (icon, label, tip, command) in [
                    (Icon::Floor, "Mass Floors", "Mass Floors (MF): a floor at each level the selected masses cross", "mass_floors"),
                    (
                        Icon::Wall,
                        "Wall by Face",
                        "Wall by Face (WB): a wall on each edge of the selected masses' footprints",
                        "wall_by_face",
                    ),
                    (Icon::Floor, "Floor by Face", "Floor by Face (FB): a floor on the selected masses' top faces", "floor_by_face"),
                    (Icon::Roof, "Roof by Face", "Roof by Face (RB): a roof on the selected masses' top faces", "roof_by_face"),
                    (
                        Icon::Align,
                        "Update to Face",
                        "Update to Face (UF): the walls, floors and roofs made by face from the selected masses follow them again",
                        "update_by_face",
                    ),
                ] {
                    if big_button(ui, icon, label, false, tip).clicked() {
                        self.by_mass(command);
                    }
                }
            });
        });
    }

    /// Show Mass (MH): the masses in the view on screen, hidden if they show and shown if they're hidden.
    fn show_mass(&mut self) {
        match self.graphic_view() {
            Some(view) => {
                self.run("show_mass", json!({"view": view}));
            }
            None => self.status = Status { text: t("This view has no graphics to override.").to_string(), error: true },
        }
    }

    /// The masses selected.
    fn selected_masses(&self) -> Vec<Id> {
        self.selection.iter().copied().filter(|id| matches!(self.session.model.elements.get(id), Some(Element::Mass(_)))).collect()
    }

    /// A Model by Face button or shortcut: `command` for each mass selected.
    fn by_mass(&mut self, command: &str) {
        let masses = self.selected_masses();
        if masses.is_empty() {
            self.status = Status { text: t("Select a mass first.").into(), error: true };
        }
        for id in masses {
            self.run(command, json!({"mass": id}));
        }
    }

    fn architecture_tab(&mut self, ui: &mut Ui) {
        self.tools(ui, "Select", &[(Icon::Modify, "Modify", "Modify (MD): select elements", Tool::Modify)]);
        self.tools(
            ui,
            "Build",
            &[
                (Icon::Wall, "Wall", "Wall (WA): each click builds a wall; Enter or Esc ends the chain", Tool::Wall(Vec::new())),
                (Icon::Door, "Door", "Door (DR): click a wall", Tool::Door),
                (Icon::Window, "Window", "Window (WN): click a wall", Tool::Window),
                (Icon::Column, "Column", "Column (CL): click where it stands", Tool::Column),
                (Icon::Component, "Component", "Component (CM): a family's type, where you click; Space turns it", Tool::Component),
                (Icon::Roof, "Roof", "Roof by Footprint: click inside the walls to roof them, on the level above", Tool::Roof),
                (
                    Icon::ExtrusionRoof,
                    "Roof by Extrusion",
                    "Roof by Extrusion: click a line's start and end, then how far across it's swept; its profile rises from the line",
                    Tool::ExtrusionRoof(Vec::new()),
                ),
                (Icon::Floor, "Floor", "Floor: click inside the walls to floor them", Tool::Floor),
                (Icon::Ceiling, "Ceiling", "Ceiling: click inside a room for a ceiling out to its walls", Tool::Ceiling),
            ],
        );
        self.tools(
            ui,
            "Circulation",
            &[
                (
                    Icon::Stair,
                    "Stair",
                    "Stair: click where it starts, then where its run ends; a landing turns it",
                    Tool::Stair(Vec::new()),
                ),
                (Icon::Ramp, "Ramp", "Ramp: click where it starts, then where its run ends; a landing turns it", Tool::Ramp(Vec::new())),
                (
                    Icon::Railing,
                    "Railing",
                    "Railing: click a stair or a ramp for one down each side, or click a path and press Enter",
                    Tool::Railing(Vec::new()),
                ),
            ],
        );
        self.tools(ui, "Room & Area", &[(Icon::Room, "Room", "Room (RM): click inside walls", Tool::Room)]);
        self.tools(
            ui,
            "Opening",
            &[
                (
                    Icon::Shaft,
                    "Shaft",
                    "Shaft Opening: click two opposite corners; it cuts the floors, ceilings and roofs it passes through",
                    Tool::Shaft(None),
                ),
                (Icon::Dormer, "Dormer", "Dormer: click a roof's slope where its front wall goes; the roof opens under it", Tool::Dormer),
            ],
        );
        self.tools(
            ui,
            "Datum",
            &[
                (Icon::Level, "Level", "Level (LL): a new level at an elevation", Tool::Level),
                (Icon::Grid, "Grid", "Grid (GR): click its two ends", Tool::Grid(None)),
            ],
        );
    }

    fn annotate_tab(&mut self, ui: &mut Ui) {
        self.tools(ui, "Select", &[(Icon::Modify, "Modify", "Modify (MD): select elements", Tool::Modify)]);
        self.tools(
            ui,
            "Dimension",
            &[
                (
                    Icon::Dimension,
                    "Aligned",
                    "Aligned Dimension (DI): pick parallel wall faces, wall ends, doors, windows or grids, then click where the \
                     dimension line goes",
                    Tool::Dimension(Vec::new()),
                ),
                (
                    Icon::Angular,
                    "Angular",
                    "Angular Dimension: pick two lines that meet, then click where the arc goes",
                    Tool::Angular(Vec::new()),
                ),
                (Icon::Radial, "Radial", "Radial Dimension: pick a round column, then click where the radius goes", Tool::Radial(None)),
                (Icon::Spot, "Spot Elevation", "Spot Elevation (EL): click a floor, a roof or a stair; in a section, a point", Tool::Spot),
            ],
        );
        self.detail_panel(ui);
        self.region_panel(ui);
        self.tools(
            ui,
            "Text",
            &[(Icon::Text, "Text", "Text (TX): type the note in the Options Bar, then click where it goes", Tool::Text)],
        );
        self.tools(
            ui,
            "Measure",
            &[(
                Icon::Measure,
                "Measure",
                "Measure (ME): click two points; their length and angle read beside the ruler between them",
                Tool::Measure(None),
            )],
        );
        panel(ui, "Tag", |ui| {
            let tip = "Tag by Category (TG): click an element to tag it with its type, a door's or window's mark, or the field in Label";
            if big_button(ui, Icon::Tag, "Tag by Category", self.tool == Tool::Tag, tip).clicked() {
                self.set_tool(Tool::Tag);
            }
            if big_button(ui, Icon::TagAll, "Tag All", false, "Tag every door and window in this view that has no tag").clicked() {
                self.tag_all();
            }
        });
    }

    /// Duplicate View (DP): a new plan view of the plan on screen (`mode` as the command says), opened as the kind of plan it
    /// was made from.
    pub(crate) fn duplicate_plan(&mut self, mode: &'static str) {
        match self.plan_key() {
            Some(key) => self.duplicate_view_of(key, mode),
            None => self.status = Status { text: t("Duplicate View: open a plan first").into(), error: true },
        }
    }

    /// A duplicate of plan `key`, opened as the kind of plan (floor or ceiling) on screen.
    pub(crate) fn duplicate_view_of(&mut self, key: Id, mode: &'static str) {
        if let Some(id) = self.run("duplicate_view", json!({"view": key, "mode": mode})).and_then(|r| r["id"].as_u64()) {
            self.open_sheet(if matches!(self.sheet, Sheet::Ceiling(_)) { Sheet::Ceiling(id) } else { Sheet::Plan(id) });
        }
    }

    /// Tag All: the doors and windows of the plan's level that have no tag yet.
    fn tag_all(&mut self) {
        if let Sheet::Plan(level) = self.sheet {
            self.run("tag", json!({"level": level, "leader": self.tag_leader}));
        }
    }

    fn view_tab(&mut self, ui: &mut Ui) {
        panel(ui, "Graphics", |ui| {
            let tip = "Visibility/Graphics (VV): what this view hides, and how it draws the rest, by category, filter and element";
            let on = self.graphic_view().is_some();
            if ui
                .add_enabled_ui(on, |ui| big_button(ui, Icon::Visibility, "Visibility/Graphics", self.vg_dialog.is_some(), tip))
                .inner
                .clicked()
            {
                self.open_graphics(0);
            }
            let tip = "Filters (FT): rules that pick out elements for a view to hide or draw another way";
            if big_button(ui, Icon::Filter, "Filters", self.filters_dialog.is_some(), tip).clicked() {
                self.filters_dialog = Some(graphics::FiltersDialog::default());
            }
            let tip = "View Templates (VT): a view's settings under a name, for other views to follow";
            if big_button(ui, Icon::Template, "View Templates", self.templates_dialog.is_some(), tip).clicked() {
                self.templates_dialog = Some(graphics::TemplatesDialog::default());
            }
            if big_button(ui, Icon::ThinLines, "Thin Lines", self.thin, "Thin Lines (TL): every line one pixel wide").clicked() {
                self.thin = !self.thin;
            }
            let dark = style::dark();
            let tip = "Choose a theme, including a dark interface with a light canvas";
            let r = big_button(ui, Icon::Theme, "Dark Mode", dark, tip);
            let at = r.rect.right_top() + vec2(-8.0, 9.0);
            ui.painter().add(egui::Shape::convex_polygon(
                vec![at + vec2(-3.0, -2.0), at + vec2(3.0, -2.0), at + vec2(0.0, 2.0)],
                pal().icon,
                Stroke::NONE,
            ));
            ribbon::choices(ui, &r, false, |ui| self.theme_choices(ui, true));
            // A plan's or a section's crop region: on to what it shows, or off.
            let view = match self.sheet {
                Sheet::Plan(id) | Sheet::Section(id) => Some(id),
                _ => None,
            };
            let cropped = view.is_some_and(|id| match self.session.model.elements.get(id) {
                Some(Element::Level(_) | Element::PlanView(_)) => self.session.model.crop_of(id).is_some(),
                Some(Element::Section(v) | Element::Elevation(v)) => v.crop.is_some(),
                _ => false,
            });
            let tip = "Crop View: show only the crop region; drag its edges by their grips";
            if ui.add_enabled_ui(view.is_some(), |ui| big_button(ui, Icon::Crop, "Crop View", cropped, tip)).inner.clicked()
                && let Some(id) = view
            {
                self.run("crop", if cropped { json!({"view": id, "region": null}) } else { json!({"view": id}) });
            }
        });
        panel(ui, "Create", |ui| {
            if big_button(ui, Icon::View3D, "3D View", self.sheet == Sheet::View3D, "Open the default 3D view").clicked() {
                self.open_sheet(Sheet::View3D);
            }
            let tip = "Duplicate View (DP): a new plan of this level with its own annotation; with detailing, it starts with this plan's; as a dependent, it follows this plan's range and graphics";
            let r = ui.add_enabled_ui(self.plan_key().is_some(), |ui| big_button(ui, Icon::Copy, "Duplicate View", false, tip)).inner;
            ribbon::choices(ui, &r, false, |ui| {
                for (label, mode) in
                    [("Duplicate", "duplicate"), ("Duplicate with Detailing", "detailing"), ("Duplicate as Dependent", "dependent")]
                {
                    if ui.button(t(label)).clicked() {
                        self.duplicate_plan(mode);
                    }
                }
            });
            let tip = "Section (CR): click its start, then its end; it looks to the left of the line";
            if big_button(ui, Icon::Section, "Section", matches!(self.tool, Tool::Section(_)), tip).clicked() {
                self.set_tool(Tool::Section(None));
            }
            let tip = "Elevation: click beside the building, on the side to look at";
            if big_button(ui, Icon::Elevation, "Elevation", self.tool == Tool::Elevation, tip).clicked() {
                self.set_tool(Tool::Elevation);
            }
            let r = big_button(ui, Icon::Schedule, "Schedules", false, "Make a schedule, or open one");
            ribbon::choices(ui, &r, false, |ui| {
                if ui.button(t("Schedule/Quantities…")).clicked() {
                    self.new_schedule = Some(properties::NewSchedule::default());
                }
                ui.separator();
                for (cat, name) in SCHEDULES {
                    if ui.button(t(name)).clicked() {
                        self.open_sheet(Sheet::Schedule(cat));
                    }
                }
                let made: Vec<(Id, String)> = self.schedules().into_iter().map(|(id, n)| (id, n.to_string())).collect();
                for (id, name) in made {
                    if ui.button(name).clicked() {
                        self.open_sheet(Sheet::Table(id));
                    }
                }
            });
        });
        panel(ui, "Sheet Composition", |ui| {
            if big_button(ui, Icon::Sheet, "Sheet", false, "A new sheet, with a title block").clicked()
                && let Some(id) = self.run("sheet", json!({})).and_then(|made| made["id"].as_u64())
            {
                self.open_sheet(Sheet::Paper(id));
            }
            let sheet = match self.sheet {
                Sheet::Paper(id) => Some(id),
                _ => None,
            };
            let tip = "Put a view on this sheet: one not on a sheet yet";
            let r = ui.add_enabled_ui(sheet.is_some(), |ui| big_button(ui, Icon::PlaceView, "Place View", false, tip)).inner;
            if let Some(sheet) = sheet {
                let views = self.unplaced_views();
                ribbon::choices(ui, &r, false, |ui| {
                    if views.is_empty() {
                        ui.label(t("Every view is on a sheet."));
                    }
                    for (view, label) in views {
                        if ui.button(label).clicked() {
                            let mut p = json!({"sheet": sheet, "view": view});
                            // The 3D view as it's turned on screen.
                            if let (Some("{3D}"), Some(c)) = (p["view"].as_str(), self.orbit.camera) {
                                (p["azimuth"], p["elevation"]) = (json!(c.azimuth), json!(c.elevation));
                            }
                            self.run("place", p);
                        }
                    }
                });
            }
        });
        panel(ui, "Windows", |ui| {
            if big_button(ui, Icon::CloseInactive, "Close Inactive", false, "Close every view but this one").clicked() {
                let active = self.sheet;
                self.views.retain(|v| *v == active);
                self.floating.retain(|f| f.sheet == active);
            }
            let tip = "Show or hide the palettes, and pick the interface's language";
            let r = big_button(ui, Icon::Interface, "User Interface", false, tip);
            ribbon::choices(ui, &r, false, |ui| {
                for p in dock::Palette::ALL {
                    let mut on = self.docking.shown(p);
                    if ui.checkbox(&mut on, t(p.name())).changed() {
                        self.show_palette(p, on);
                    }
                }
                ui.separator();
                ui.label(RichText::new(t("Language")).color(pal().muted));
                for l in Lang::ALL {
                    if ui.selectable_label(self.lang == l, l.label()).clicked() {
                        self.set_lang(ui.ctx(), l);
                    }
                }
            });
        });
    }

    /// Manage: the project's settings, as Revit's tab has them.
    /// Insert: what comes into the project from other files, as Revit's tab. Link: other projects, and laser scans.
    fn insert_tab(&mut self, ui: &mut Ui) {
        panel(ui, "Link", |ui| {
            let tip = "Link Model (LK): another project, a BIMCraft file or an IFC, shown in this one as a reference that can't be edited: halftone in the plans, the sections and the 3D view, and snapped to";
            if big_button(ui, Icon::Link, "Link Model", false, tip).clicked() {
                self.link_dialog = true;
            }
            let tip = "Point Cloud: laser scans (PTS, XYZ, PLY, LAS or E57) brought into the project, one file or a survey's stations as one scan, drawn in the 3D view, plans and sections and snapped to";
            if big_button(ui, Icon::PointCloud, "Point Cloud", self.cloud_box.is_some(), tip).clicked() {
                self.cloud_dialog = true;
            }
        });
    }

    fn manage_tab(&mut self, ui: &mut Ui) {
        panel(ui, "Settings", |ui| {
            let tip = "Materials: the project's library, each material's colour in 3D and the patterns its cut and its surfaces show";
            if big_button(ui, Icon::Materials, "Materials", self.materials_dialog.is_some(), tip).clicked() {
                self.materials_dialog = Some(properties::MaterialsDialog::new(&self.session.model));
            }
            let tip = "Favorites: the materials and families you keep for every project; add one to this project with a click";
            if big_button(ui, Icon::Favorite, "Favorites", self.favorites_dialog.is_some(), tip).clicked() {
                self.open_favorites();
            }
            let tip = "Project Parameters: values of your own each element of their categories holds, for schedules and IFC";
            if big_button(ui, Icon::Parameters, "Project Parameters", self.parameters_dialog.is_some(), tip).clicked() {
                self.parameters_dialog = Some(properties::ParametersDialog::new(&self.session.model));
            }
            // Pens and lines, stacked: Line Weights, Object Styles, Line Styles.
            stacked(ui, |ui| {
                let tip = "Line Weights: what each pen draws on paper, the model's at each scale and the annotation's at any";
                if stacked_button(ui, Icon::LineWeights, "Line Weights", self.line_weights_dialog.is_some(), tip).clicked() {
                    self.line_weights_dialog = Some(lineweights::LineWeightsDialog::new(&self.session.model));
                }
                let tip = "Object Styles: the pen each category draws in, seen and cut, in every view";
                if stacked_button(ui, Icon::ObjectStyles, "Object Styles", self.object_styles_dialog.is_some(), tip).clicked() {
                    self.object_styles_dialog = Some(objectstyles::ObjectStylesDialog::new(&self.session.model));
                }
                let tip = "Line Styles: the pen, colour and line pattern detail lines draw in, and the patterns' dashes, dots and spaces";
                if stacked_button(ui, Icon::ThinLines, "Line Styles", self.line_styles_dialog.is_some(), tip).clicked() {
                    self.line_styles_dialog = Some(linestyles::LineStylesDialog::new(&self.session.model));
                }
            });
            let tip =
                "Purge Unused (PU): take out the types, families and the rest nothing in the project uses; each category keeps a type";
            if big_button(ui, Icon::Delete, "Purge Unused", self.purge_dialog.is_some(), tip).clicked() {
                self.open_purge();
            }
            // Fills, snaps and units, stacked.
            stacked(ui, |ui| {
                let tip = "Fill Patterns: the patterns filled regions fill with, and the filled region types";
                if stacked_button(ui, Icon::FillPatterns, "Fill Patterns", self.fill_patterns_dialog.is_some(), tip).clicked() {
                    self.fill_patterns_dialog = Some(fillpatterns::FillPatternsDialog::new(&self.session.model));
                }
                let tip = "Snaps: what a picked point sticks to (ends, middles, crossings, centres, perpendiculars), and the angles it steps by; SE, SM, SI, SC, SP and SN take one alone for the next point, SO none";
                if stacked_button(ui, Icon::Snaps, "Snaps", self.show_snaps, tip).clicked() {
                    self.show_snaps = true;
                }
                let now = self.session.model.units;
                let tip = tf("Project Units (UN): how lengths show and read; now {}", &[&t(now.label()).to_lowercase()]);
                let r = stacked_button(ui, Icon::Units, "Project Units", false, &tip);
                let asked = self.open_units;
                let opened = ribbon::choices(ui, &r, asked, |ui| {
                    for u in Units::ALL {
                        if ui.selectable_label(u == now, t(u.label())).clicked() {
                            self.run("units", json!({"length": u.name()}));
                        }
                    }
                });
                self.open_units &= !opened;
            });
            let tip = "Keyboard Shortcuts (KS): customize, edit, and load shortcut keys";
            if big_button(ui, Icon::Keyboard, "Shortcuts", self.show_shortcuts, tip).clicked() {
                self.show_shortcuts = true;
            }
            // What the drawings say, and how new things are named.
            let now = self.session.model.language;
            let tip = tf("Project Language: what the drawings say and how new things are named; now {}", &[now.label()]);
            let r = big_button(ui, Icon::Text, "Project Language", false, &tip);
            ribbon::choices(ui, &r, false, |ui| {
                for l in Language::ALL {
                    if ui.selectable_label(l == now, l.label()).clicked() {
                        self.run("language", json!({"code": l.code()}));
                    }
                }
            });
        });
        panel(ui, "Links", |ui| {
            let tip = "Link Model (LK): another project, a BIMCraft file or an IFC, shown in this one as a reference that can't be edited: halftone in the plans, the sections and the 3D view, and snapped to";
            if big_button(ui, Icon::Link, "Link Model", false, tip).clicked() {
                self.link_dialog = true;
            }
        });
        panel(ui, "Family Editor", |ui| {
            let tip = "New Family: start a family of your own, of a category, to edit its parameters, types and forms";
            let r = big_button(ui, Icon::FamilyEditor, "New Family", false, tip);
            ribbon::choices(ui, &r, false, |ui| {
                for c in editor::CATEGORIES {
                    if ui.button(editor::category_label(c)).clicked() {
                        self.new_family(c);
                    }
                }
            });
            let tip = "Edit Family: the family of what is selected, or of the component being placed";
            let family = self.family_in_hand();
            if ui.add_enabled_ui(family.is_some(), |ui| big_button(ui, Icon::FamilyEditor, "Edit Family", false, tip)).inner.clicked()
                && let Some(f) = family
            {
                self.edit_family(&f);
            }
        });
        panel(ui, "Inquiry", |ui| {
            let tip = "Select by ID: select elements by their IDs, GUIDs or IFC GlobalIds, as many as you paste, and show them";
            if big_button(ui, Icon::SelectById, "Select by ID", self.select_by_id.is_some(), tip).clicked() {
                self.open_select_by_id();
            }
            let tip = self.selection_ids_tip();
            let some = !self.selection.is_empty();
            if ui
                .add_enabled_ui(some, |ui| big_button(ui, Icon::IdsOfSelection, "IDs of Selection", self.ids_of_selection.is_some(), &tip))
                .inner
                .clicked()
            {
                self.open_ids_of_selection();
            }
        });
        panel(ui, "Phasing", |ui| {
            let tip = "Phasing (PH): the project's phases in order, the phase filters that show them, and how a filter draws each";
            if big_button(ui, Icon::Phasing, "Phasing", self.phasing_dialog.is_some(), tip).clicked() {
                self.phasing_dialog = Some(phasing::PhasingDialog::default());
            }
        });
        panel(ui, "Design Options", |ui| {
            let tip = "Design Options (DO): sets of options for a part of the design, and the one new elements are made in";
            if big_button(ui, Icon::Options, "Design Options", self.options_dialog.is_some(), tip).clicked() {
                self.options_dialog = Some(phasing::OptionsDialog::default());
            }
        });
    }

    fn modify_tab(&mut self, ui: &mut Ui) {
        self.tools(ui, "Select", &[(Icon::Modify, "Modify", "Modify (MD): select elements", Tool::Modify)]);
        if self.sketch.is_some() {
            self.sketch_draw(ui);
            self.mode_panel(ui);
            return;
        }
        self.sketch_starters(ui);
        panel(ui, "Properties", |ui| {
            let shown = self.docking.shown(dock::Palette::Properties);
            if big_button(ui, Icon::Properties, "Properties", shown, "Properties (PP): show or hide the palette").clicked() {
                self.toggle_properties();
            }
            let typed = self.type_subject().is_some();
            if ui
                .add_enabled_ui(typed, |ui| big_button(ui, Icon::TypeProperties, "Type Properties", false, "Edit the type"))
                .inner
                .clicked()
            {
                self.edit_type();
            }
        });
        // Hide in View, and Override Graphics: what the selection does in the view on screen.
        if !self.selection.is_empty() && self.graphic_view().is_some() {
            panel(ui, "View", |ui| {
                let tip = "Hide Element (EH): leave the selected elements out of this view";
                if big_button(ui, Icon::Visibility, "Hide Element", false, tip).clicked() {
                    self.hide_selection(false);
                }
                let tip = "Hide Category (VH): leave every category the selection is of out of this view";
                if big_button(ui, Icon::Visibility, "Hide Category", false, tip).clicked() {
                    self.hide_selection(true);
                }
                let tip = "Override Graphics by Element (EO): draw the selected elements their own way in this view";
                if big_button(ui, Icon::Visibility, "Override Graphics", false, tip).clicked() {
                    self.override_selection();
                }
                // What can be picked while hidden is what Reveal Hidden Elements draws: its selection can be brought back.
                let (elements, categories) = self.hidden_selection();
                if !elements.is_empty() {
                    let tip = "Unhide Element (EU): bring back the selected elements this view hides";
                    if big_button(ui, Icon::Visibility, "Unhide Element", false, tip).clicked() {
                        self.unhide_selection(false);
                    }
                }
                if !categories.is_empty() {
                    let tip = "Unhide Category (VU): bring back every category the selection is of that this view hides";
                    if big_button(ui, Icon::Visibility, "Unhide Category", false, tip).clicked() {
                        self.unhide_selection(true);
                    }
                }
            });
        }
        // Revit's Modify panel: small buttons in rows. Those taking the selection need one.
        let some = !self.selection.is_empty();
        // The button lit while its tool is on (the two Mirrors told apart by how they take the axis).
        let on = |t: &Tool, other: &Tool| match (t, other) {
            (Tool::Mirror { draw: a, .. }, Tool::Mirror { draw: b, .. }) => a == b,
            _ => std::mem::discriminant(t) == std::mem::discriminant(other),
        };
        let rows: [Buttons; 2] = [
            &[
                (Icon::Align, "Align", "Align (AL): pick a reference line, then what to line up with it", Some(Tool::Align(None)), true),
                (Icon::Offset, "Offset", "Offset (OF): pick a wall or a line; a copy goes to the cursor's side", Some(Tool::Offset), true),
                (
                    Icon::MirrorPick,
                    "Mirror Pick Axis",
                    "Mirror, Pick Axis (MM): pick a wall or grid to mirror across",
                    Some(Tool::Mirror { draw: false, start: None }),
                    some,
                ),
                (
                    Icon::MirrorDraw,
                    "Mirror Draw Axis",
                    "Mirror, Draw Axis (DM): click the axis's two ends",
                    Some(Tool::Mirror { draw: true, start: None }),
                    some,
                ),
                (Icon::Split, "Split Element", "Split Element (SL): click a wall or a line where to split it", Some(Tool::Split), true),
                (
                    Icon::Linework,
                    "Linework",
                    "Linework (LW): click an edge to draw it in another line style, or hide it, in this view only",
                    Some(Tool::Linework),
                    true,
                ),
                (
                    Icon::TrimSingle,
                    "Trim/Extend Single Element",
                    "Trim/Extend Single Element: pick the line to trim or extend to, then the line to change, on the part to keep",
                    Some(Tool::TrimTo { boundary: None, many: false }),
                    true,
                ),
            ],
            &[
                (Icon::Move, "Move", "Move (MV): click a base point, then where it goes", Some(Tool::Move(None)), some),
                (Icon::Copy, "Copy", "Copy (CO): click a base point, then where the copy goes", Some(Tool::Copy(None)), some),
                (
                    Icon::Rotate,
                    "Rotate",
                    "Rotate (RO): click the angle's start, then its end",
                    Some(Tool::Rotate { centre: P2(0.0, 0.0), start: None, place: false }),
                    some,
                ),
                (
                    Icon::Trim,
                    "Trim/Extend to Corner",
                    "Trim/Extend to Corner (TR): pick two walls or two lines, on the parts to keep",
                    Some(Tool::Trim(None)),
                    true,
                ),
                (
                    Icon::TrimMultiple,
                    "Trim/Extend Multiple Elements",
                    "Trim/Extend Multiple Elements: pick the line to trim or extend to, then each line to change, on the part to keep",
                    Some(Tool::TrimTo { boundary: None, many: true }),
                    true,
                ),
                (Icon::Delete, "Delete", "Delete (DE) the selection", None, some),
            ],
        ];
        // The Wall tool's and the Detail Line tool's Draw panels: lines, rectangles, polygons, circles, arcs and the rest.
        match self.tool {
            Tool::Wall(_) => self.draw_panel(ui, draw::Target::Wall),
            Tool::DetailLine(_) => self.draw_panel(ui, draw::Target::Detail),
            _ => {}
        }
        // Placing a component: Revit's Mode panel, which loads a family from a file.
        if matches!(self.tool, Tool::Component | Tool::Fixture(_)) {
            panel(ui, "Mode", |ui| {
                let tip = "Load Family: a family saved as a .json file, BIMCraft's own";
                if big_button(ui, Icon::LoadFamily, "Load Family", false, tip).clicked() {
                    let dialog = rfd::AsyncFileDialog::new().add_filter(t("BIMCraft family"), &["json"]);
                    self.ask_file(ui.ctx(), dialog, false, |app, path| {
                        if let Some(r) = app.run("family", json!({"path": path.to_string_lossy()})) {
                            let name = r["name"].as_str().unwrap_or_default().to_string();
                            let first = r["family"]["types"].as_object().and_then(|t| t.keys().next().cloned()).unwrap_or_default();
                            app.component = Some((name, first));
                        }
                    });
                }
            });
        }
        // Detail lines selected, or sketch lines picked: Join into Polyline and Explode Polyline.
        let picked = self.sketch.as_ref().map(|sk| sk.picked.len());
        if (self.lines_selected() && self.tool == Tool::Modify) || (picked.is_some() && self.tool == Tool::Modify) {
            panel(ui, "Modify Lines", |ui| {
                let two = picked.unwrap_or(self.selection.len()) >= 2;
                let tip = "Join into Polyline (PJ): the lines selected, meeting end to end, become one polyline";
                if ui.add_enabled_ui(two, |ui| big_button(ui, Icon::JoinPolyline, "Join into Polyline", false, tip)).inner.clicked() {
                    self.join_polyline();
                }
                let tip = "Explode Polyline (EX): a polyline taken apart into a line or an arc for each segment";
                if big_button(ui, Icon::ExplodePolyline, "Explode Polyline", false, tip).clicked() {
                    self.explode_polyline();
                }
                self.vertex_buttons(ui);
            });
        }
        // Walls selected: Revit's Modify Wall panel.
        let walls = self.selected_walls();
        if !walls.is_empty() && matches!(self.tool, Tool::Modify | Tool::Attach) {
            panel(ui, "Modify Wall", |ui| {
                let tip = "Flip (Space): the walls' exterior sides go to the other side of their location lines";
                if big_button(ui, Icon::Flip, "Flip", false, tip).clicked() {
                    self.flip();
                }
                let tip = "Attach Top/Base: pick a roof or a floor for the walls' tops, or bases, to follow";
                if big_button(ui, Icon::Attach, "Attach Top/Base", self.tool == Tool::Attach, tip).clicked() {
                    self.set_tool(Tool::Attach);
                }
                if big_button(ui, Icon::Detach, "Detach Top/Base", false, "Detach Top/Base: the walls' tops and bases their own again")
                    .clicked()
                {
                    self.run("modify", json!({"ids": walls, "attach_top": null, "attach_base": null}));
                }
            });
        }
        // A roof selected: which of its edges stand upright.
        if self.selected_roof().is_some() && matches!(self.tool, Tool::Modify | Tool::GableEdge) {
            panel(ui, "Shape Editing", |ui| {
                let tip = "Gable Edge: click an edge of a roof's footprint to stand it upright, a gable end; click again to slope it";
                if big_button(ui, Icon::GableEdge, "Gable Edge", self.tool == Tool::GableEdge, tip).clicked() {
                    self.set_tool(Tool::GableEdge);
                }
            });
        }
        // A floor selected: its slope arrow.
        if self.selected_floor().is_some() && matches!(self.tool, Tool::Modify | Tool::SlopeArrow(_)) {
            panel(ui, "Shape Editing", |ui| {
                let tip = "Slope Arrow: click its tail, then its head; the floor rises toward the head at the Options Bar's slope";
                if big_button(ui, Icon::SlopeArrow, "Slope Arrow", matches!(self.tool, Tool::SlopeArrow(_)), tip).clicked() {
                    self.set_tool(Tool::SlopeArrow(None));
                }
            });
        }
        panel(ui, "Modify", |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for row in rows {
                    ui.horizontal(|ui| {
                        for (icon, label, tip, tool, enabled) in row {
                            let active = tool.as_ref().is_some_and(|t| on(t, &self.tool) && *t != Tool::Modify);
                            if ui.add_enabled_ui(*enabled, |ui| small_button(ui, *icon, label, active, tip)).inner.clicked() {
                                match tool {
                                    Some(t) => self.set_tool(t.clone()),
                                    None => self.delete(),
                                }
                            }
                        }
                    });
                }
            });
        });
        panel(ui, "Phasing", |ui| {
            let tip = "Demolish (DW): click an element to take it down in the view's phase; click it again to take it back";
            let on = self.graphic_view().is_some();
            if ui.add_enabled_ui(on, |ui| big_button(ui, Icon::Demolish, "Demolish", self.tool == Tool::Demolish, tip)).inner.clicked() {
                self.set_tool(Tool::Demolish);
            }
        });
        panel(ui, "Geometry", |ui| {
            let tip = "Paint (PT): lay a material, and its texture, over the face you click in the 3D view";
            if big_button(ui, Icon::Paint, "Paint", self.tool == Tool::Paint, tip).clicked() {
                self.set_tool(Tool::Paint);
            }
            let tip = "Remove Paint: take the paint off the face you click in the 3D view";
            if big_button(ui, Icon::Unpaint, "Remove Paint", self.tool == Tool::Unpaint, tip).clicked() {
                self.set_tool(Tool::Unpaint);
            }
        });
        self.mode_panel(ui);
    }

    /// The Options Bar: the contextual tab's name, then the tool's options.
    /// The Options Bar, while there's something on it (a contextual tab's name, a tool's options): a card floating over the
    /// view's top left, so the view never moves when it comes and goes, and taking the clicks that land on it.
    fn options_bar(&mut self, ctx: &Context) {
        if self.context().is_none() && matches!(self.tool, Tool::Modify) {
            self.options_rect = Rect::NOTHING;
            return;
        }
        let g = f32::from(style::gap());
        let view = self.view_rect;
        let frame = style::card([false; 4]).inner_margin(Margin::symmetric(10, 3));
        let shown = Area::new(UiId::new("options-bar")).order(Order::Middle).fixed_pos(view.left_top() + vec2(g, g)).show(ctx, |ui| {
            // Kept short of the view's zoom buttons on the right, as far as its fields wrap (a drop-down doesn't).
            ui.set_max_width((view.width() - 2.0 * g - 60.0).max(200.0));
            frame.show(ui, |ui| self.options(ui));
        });
        self.options_rect = shown.response.rect;
    }

    /// Where a view's navigation bar starts: at its top right, or under the Options Bar when that reaches over it.
    pub(crate) fn nav_corner(&self, view: Rect) -> Pos2 {
        let at = view.right_top() + vec2(-46.0, 14.0);
        let o = self.options_rect;
        if view.contains(o.left_top()) && o.right() > at.x - 6.0 && o.bottom() + 6.0 > at.y {
            return pos2(at.x, o.bottom() + 6.0);
        }
        at
    }

    fn options(&mut self, ui: &mut Ui) {
        // Wrapped: in a narrow view (the window's least) the bar takes a second line rather than run over the zoom buttons.
        ui.horizontal_wrapped(|ui| {
            ui.set_min_height(24.0);
            if let Some(name) = self.context() {
                // Painted: the tab already names it for screen readers.
                let font = FontId::proportional(13.0);
                let size = ui.painter().layout_no_wrap(name.clone(), font.clone(), pal().text).size();
                let (r, _) = ui.allocate_exact_size(size, Sense::hover());
                ui.painter().text(r.left_center(), Align2::LEFT_CENTER, name, font, pal().text);
                ui.separator();
            }
            if self.sketch.is_some() {
                self.sketch_options(ui);
                match self.tool {
                    Tool::SketchLine(_) => self.shape_options(ui),
                    Tool::Offset => self.offset_options(ui),
                    _ => {}
                }
                return;
            }
            match self.tool {
                Tool::Wall(_) => self.wall_options(ui),
                Tool::Linework => self.linework_options(ui),
                Tool::DetailGroup => self.group_options(ui),
                Tool::RepeatingDetail(_) => {
                    self.group_options(ui);
                    let l = ui.label(t("Spacing:"));
                    ui.add(egui::TextEdit::singleline(&mut self.repeat_spacing).desired_width(70.0)).labelled_by(l.id);
                }
                Tool::Paint => {
                    let label = ui.label(t("Material:"));
                    let names: Vec<String> = self.session.model.materials.keys().cloned().collect();
                    let now = self.painting().unwrap_or_default();
                    if let Some(n) = properties::combo(ui, (0, "paint-material"), label.id, &now, &names) {
                        self.paint_material = n;
                    }
                }
                Tool::Column => {
                    if let Some(level) = self.plan_level() {
                        height_options(ui, &self.session.model, level, &mut self.column_top, &mut self.column_height, "column-top");
                    }
                }
                Tool::Run(category, _) => self.run_options(ui, category),
                Tool::Stand(category) => self.stand_options(ui, category),
                Tool::Shaft(_) => {
                    if let Some(level) = self.plan_level() {
                        height_options(ui, &self.session.model, level, &mut self.shaft_top, &mut self.shaft_height, "shaft-top");
                    }
                }
                Tool::Mass(_) => {
                    if let Some(level) = self.plan_level() {
                        height_options(ui, &self.session.model, level, &mut self.mass_top, &mut self.mass_height, "mass-top");
                    }
                }
                Tool::Ceiling => {
                    let l = ui.label(t("Height Offset:"));
                    let hint = self.session.model.units.name();
                    ui.add(TextEdit::singleline(&mut self.ceiling_height).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                }
                Tool::SlopeArrow(_) => {
                    let l = ui.label(t("Slope:"));
                    ui.add(TextEdit::singleline(&mut self.floor_slope).desired_width(48.0).hint_text("°")).labelled_by(l.id);
                }
                Tool::Stair(_) => {
                    let l = ui.label(t("Width:"));
                    let hint = self.session.model.units.name();
                    ui.add(TextEdit::singleline(&mut self.stair_width).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                    ui.checkbox(&mut self.railings, t("Railings"));
                }
                Tool::Ramp(_) => {
                    if let Some(level) = self.plan_level() {
                        height_options(ui, &self.session.model, level, &mut self.ramp_top, &mut self.ramp_height, "ramp-top");
                    }
                    let hint = self.session.model.units.name();
                    let l = ui.label(t("Width:"));
                    ui.add(TextEdit::singleline(&mut self.ramp_width).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                    let l = ui.label(t("Slope:"));
                    ui.add(TextEdit::singleline(&mut self.ramp_slope).desired_width(48.0).hint_text("%")).labelled_by(l.id);
                    ui.checkbox(&mut self.railings, t("Railings"));
                }
                Tool::Roof => {
                    let l = ui.label(t("Slope:"));
                    ui.add(TextEdit::singleline(&mut self.roof_slope).desired_width(48.0).hint_text("°")).labelled_by(l.id);
                    let l = ui.label(t("Overhang:"));
                    let hint = self.session.model.units.name();
                    ui.add(TextEdit::singleline(&mut self.roof_overhang).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                    ui.checkbox(&mut self.roof_gable, t("Gable"));
                }
                Tool::ExtrusionRoof(_) => {
                    let l = ui.label(t("Slope:"));
                    ui.add(TextEdit::singleline(&mut self.roof_slope).desired_width(48.0).hint_text("°")).labelled_by(l.id);
                    ui.checkbox(&mut self.roof_shed, t("Shed"));
                }
                Tool::Dormer => {
                    let hint = self.session.model.units.name();
                    let l = ui.label(t("Width:"));
                    ui.add(TextEdit::singleline(&mut self.dormer_width).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                    let l = ui.label(t("Height:"));
                    ui.add(TextEdit::singleline(&mut self.dormer_height).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                    let l = ui.label(t("Slope:"));
                    ui.add(TextEdit::singleline(&mut self.roof_slope).desired_width(48.0).hint_text("°")).labelled_by(l.id);
                    ui.checkbox(&mut self.dormer_window, t("Window"));
                }
                Tool::Level => self.level_options(ui),
                Tool::Move(_) => {
                    ui.checkbox(&mut self.constrain, t("Constrain"));
                }
                Tool::Copy(_) => {
                    ui.checkbox(&mut self.constrain, t("Constrain"));
                    ui.checkbox(&mut self.multiple, t("Multiple"));
                }
                Tool::Rotate { .. } => self.rotate_options(ui),
                Tool::Mirror { .. } => {
                    ui.checkbox(&mut self.mirror_copy, t("Copy"));
                }
                Tool::Tag => {
                    ui.checkbox(&mut self.tag_leader, t("Leader"));
                    let l = ui.label(t("Label:"));
                    ui.add(TextEdit::singleline(&mut self.tag_label).desired_width(160.0).hint_text(t("a field, such as Type or Height")))
                        .labelled_by(l.id);
                }
                Tool::DetailLine(_) => self.line_style_options(ui),
                Tool::Text => {
                    let l = ui.label(t("Text:"));
                    ui.add(TextEdit::singleline(&mut self.note_text).desired_width(240.0).hint_text(t("what the note says")))
                        .labelled_by(l.id);
                }
                Tool::Attach => {
                    ui.label(t("Attach Wall:"));
                    if ui.selectable_label(!self.attach_base, t("Top")).clicked() {
                        self.attach_base = false;
                    }
                    if ui.selectable_label(self.attach_base, t("Base")).clicked() {
                        self.attach_base = true;
                    }
                }
                Tool::Offset => self.offset_options(ui),
                _ => {}
            }
        });
    }

    /// Offset's Options Bar: the distance, and Copy.
    fn offset_options(&mut self, ui: &mut Ui) {
        let l = ui.label(t("Offset:"));
        let hint = self.session.model.units.name();
        ui.add(TextEdit::singleline(&mut self.offset).desired_width(70.0).hint_text(hint)).labelled_by(l.id);
        ui.checkbox(&mut self.offset_copy, t("Copy"));
    }

    fn wall_options(&mut self, ui: &mut Ui) {
        let Some(level) = self.plan_level() else { return };
        height_options(ui, &self.session.model, level, &mut self.wall_top, &mut self.wall_height, "wall-top");
        let l = ui.label(t("Location Line:"));
        let now = self.wall_location;
        let c = ComboBox::from_id_salt("wall-location").selected_text(t(now.label())).width(150.0).show_ui(ui, |ui| {
            for loc in Location::ALL {
                if ui.selectable_label(loc == now, t(loc.label())).clicked() {
                    self.wall_location = loc;
                }
            }
        });
        c.response.labelled_by(l.id);
        if self.shape().chains() {
            ui.checkbox(&mut self.chain, t("Chain"));
        }
        self.shape_options(ui);
    }

    /// Rotate's Options Bar: an angle to type (Enter turns by it), Place Centre, and Copy.
    fn rotate_options(&mut self, ui: &mut Ui) {
        let l = ui.label(t("Angle:"));
        let r = ui.add(TextEdit::singleline(&mut self.angle).desired_width(60.0).hint_text("°")).labelled_by(l.id);
        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            match (self.angle.trim().parse::<f64>(), &self.tool) {
                (Ok(a), &Tool::Rotate { centre, .. }) if a.is_finite() => self.turn(centre, a),
                _ => self.status = Status { text: tf("rotate: `{}` is not an angle in degrees", &[&self.angle]), error: true },
            }
        }
        if ui.button(t("Place Centre")).clicked()
            && let Tool::Rotate { place, .. } = &mut self.tool
        {
            *place = true;
        }
        ui.checkbox(&mut self.rotate_copy, t("Copy"));
    }

    fn level_options(&mut self, ui: &mut Ui) {
        let l = ui.label(t("Elevation"));
        ui.add(TextEdit::singleline(&mut self.new_level.0).desired_width(80.0)).labelled_by(l.id);
        let l = ui.label(t("Name"));
        ui.add(TextEdit::singleline(&mut self.new_level.1).hint_text(t("the next free name")).desired_width(140.0)).labelled_by(l.id);
        if ui.button(t("Create")).clicked() {
            match self.session.model.units.parse(&self.new_level.0) {
                Some(elevation) => {
                    let mut p = json!({"elevation": elevation});
                    if !self.new_level.1.trim().is_empty() {
                        p["name"] = json!(self.new_level.1.trim());
                    }
                    if self.run("level", p).is_some() {
                        self.tool = Tool::Modify;
                    }
                }
                None => self.status = Status { text: tf("level: `{}` is not an elevation", &[&self.new_level.0]), error: true },
            }
        }
    }

    // ---------- the bottom: status bar, warnings ----------

    /// What the tool wants next, in the status bar as Revit puts it.
    fn prompt(&self) -> &'static str {
        if let Some(hint) = self.sketch_prompt() {
            return hint;
        }
        match (self.sheet, &self.tool) {
            (Sheet::View3D, Tool::Modify) => {
                return "Click to select. Drag to orbit (or Shift and the middle button), the middle button pans, the wheel zooms.";
            }
            (Sheet::Paper(_), Tool::Modify) => {
                return "Click a view on the sheet to select it, drag it to move it, double-click it to open it.";
            }
            (Sheet::Section(_), Tool::Modify) => {
                return "Click to select what the view cuts or sees; the middle button pans, the wheel zooms.";
            }
            (Sheet::View3D | Sheet::Section(_) | Sheet::Paper(_), Tool::Level) => {
                return "Type the new level's elevation in the Options Bar, then Create.";
            }
            (Sheet::Section(_) | Sheet::Paper(_), Tool::Text) => return "Type the note in the Options Bar, then click where it goes.",
            (Sheet::Section(_), Tool::Spot) => return "Click a point to mark its height.",
            // Demolish picks in these views as it does in a plan; Paint in the 3D view alone.
            (Sheet::View3D | Sheet::Section(_), Tool::Demolish) | (Sheet::View3D, Tool::Paint | Tool::Unpaint) => {}
            (Sheet::View3D | Sheet::Section(_) | Sheet::Paper(_), _) => {
                return "This tool works in plan views: open one from the Project Browser.";
            }
            _ => {}
        }
        if matches!(self.target(), Some(draw::Target::Detail | draw::Target::Wall))
            && let Some(prompt) = self.shape_prompt()
        {
            return prompt;
        }
        match &self.tool {
            Tool::Modify => "Click to select, Tab for what's underneath, Ctrl adds, Shift takes away; drag to box or to move.",
            Tool::Wall(chain) if chain.is_empty() => "Click to enter the wall's start point.",
            Tool::Wall(_) => "Click to build the wall to the next point. Enter or Esc ends the chain, Shift keeps it square.",
            Tool::Attach if self.attach_base => "Pick a roof or a floor: the selected walls' bases stand on it.",
            Tool::Attach => "Pick a roof or a floor: the selected walls' tops follow its underside.",
            Tool::Door => "Click a wall to place the door.",
            Tool::Window => "Click a wall to place the window.",
            Tool::Room => "Click inside walls to place a room; the outline shows the room you'd get.",
            Tool::Level => "Type the new level's elevation in the Options Bar, then Create.",
            Tool::Grid(None) => "Click to enter the grid's start point.",
            Tool::Grid(Some(_)) => "Click to enter the grid's end point.",
            Tool::Move(None) | Tool::Copy(None) => "Click a base point to measure from.",
            Tool::Move(Some(_)) => "Click where the base point goes, or type a distance and press Enter. Esc cancels.",
            Tool::Copy(Some(_)) => "Click where the copy's base point goes, or type a distance and press Enter. Esc cancels.",
            Tool::Rotate { place: true, .. } => "Click the centre of rotation.",
            Tool::Rotate { start: None, .. } => "Click the start of the angle, or type an angle in the Options Bar.",
            Tool::Rotate { start: Some(_), .. } => "Click the end of the angle. Esc cancels.",
            Tool::Mirror { draw: false, .. } => "Pick a wall or grid to mirror across.",
            Tool::Mirror { draw: true, start: None } => "Click the start of the mirror axis.",
            Tool::Mirror { draw: true, start: Some(_) } => "Click the end of the mirror axis.",
            Tool::Align(None) => "Pick a reference line: a grid, or a wall's face or centre line.",
            Tool::Align(Some(_)) => "Pick the wall or grid to line up with the reference.",
            Tool::Trim(None) => "Pick the first wall or line, on the part to keep.",
            Tool::Trim(Some(_)) => "Pick the second wall or line, on the part to keep.",
            Tool::TrimTo { boundary: None, .. } => "Pick the line to trim or extend to: a line, a wall or a grid.",
            Tool::TrimTo { many: false, .. } => "Click the line to trim or extend to it, on the part to keep.",
            Tool::TrimTo { many: true, .. } => "Click each line to trim or extend to it, on the part to keep; Esc picks another boundary.",
            Tool::Vertex { delete: false } => "Click a polyline or a spline where a vertex goes.",
            Tool::Vertex { delete: true } => "Click a polyline's corner or a spline's point to take it out.",
            Tool::Split => "Click a wall or a line where to split it.",
            Tool::Offset => "Point to a wall or a line: the offset goes to the cursor's side. Click to place it.",
            Tool::Demolish => "Click an element to take it down in the view's phase; click it again to take it back.",
            Tool::Linework => "Click an edge to draw it in the line style in the Options Bar; <By Category> puts it back.",
            Tool::DetailGroup => "Click where the group's origin goes.",
            Tool::RepeatingDetail(None) => "Click where the repeating detail starts.",
            Tool::RepeatingDetail(Some(_)) => "Click where it ends: the group repeats along the line every spacing.",
            Tool::Paint => "Click a face to paint it with the material in the Options Bar; the 3D view orbits as ever.",
            Tool::Unpaint => "Click a painted face to take its paint off.",
            Tool::Dimension(refs) if refs.is_empty() => "Pick a wall face, centre line or end, a door, a window or a grid to measure from.",
            Tool::Dimension(refs) if refs.len() == 1 => "Pick the next reference, parallel to the first.",
            Tool::Dimension(_) => "Pick more references, or click away from them where the dimension line goes. Esc cancels.",
            Tool::Measure(None) => "Click the ruler's start: points snap as they do drawing.",
            Tool::Measure(Some(_)) => "Click its end: its length and angle read beside it. Esc cancels.",
            Tool::Tag => "Click an element to tag it with its type, a door's or window's with its mark, or with the field in Label.",
            Tool::Angular(refs) if refs.len() < 2 => "Pick a wall, a grid or another line; then one that meets it.",
            Tool::Angular(_) => "Click where the arc goes: the angle on that side. Esc cancels.",
            Tool::Radial(None) => "Pick a round column.",
            Tool::Radial(Some(_)) => "Click where the radius goes. Esc cancels.",
            Tool::Text => "Type the note in the Options Bar, then click where it goes: its top left corner.",
            Tool::Spot => "Click a floor, a roof, a stair or a wall: the spot elevation reads the height of its top there.",
            Tool::Section(None) => "Click the section's start: it looks to the left of the line you draw.",
            Tool::Section(Some(_)) => "Click the section's end.",
            Tool::Elevation => "Click beside the building: the elevation looks at the side you click by.",
            Tool::Column => "Click to place a column, as high as the Options Bar says.",
            Tool::Run(..) | Tool::Stand(_) => structure::hint(&self.tool).unwrap_or_default(),
            Tool::Component | Tool::Fixture(_) => "Click where the component goes; Space turns it a quarter turn.",
            Tool::Stair(pts) if pts.is_empty() => "Click where the stair starts: the middle of its first riser.",
            Tool::Stair(pts) if pts.len() == 1 => {
                "Click where its run ends: past its last riser, one run to the level above; short of it, a landing. Esc cancels."
            }
            Tool::Ramp(pts) if pts.is_empty() => "Click where the ramp starts: the middle of its foot.",
            Tool::Ramp(pts) if pts.len() == 1 => {
                "Click where its run ends: past its top, one run, as high as the Options Bar says; short of it, a landing. Esc cancels."
            }
            Tool::Stair(_) | Tool::Ramp(_) => "Click which way the next run goes from the landing: on, left, right, or back beside it.",
            Tool::Railing(pts) if pts.is_empty() => "Click a stair or a ramp for a railing down each side, or the start of a path.",
            Tool::Railing(_) => "Click the path's next point. Enter finishes, Esc cancels.",
            Tool::Roof => "Click inside the walls to roof them, on the level above: the overhang runs past their faces.",
            Tool::ExtrusionRoof(pts) if pts.is_empty() => "Click the start of the line the roof's profile runs along.",
            Tool::ExtrusionRoof(pts) if pts.len() == 1 => "Click the line's end: the profile rises from it at the Options Bar's slope.",
            Tool::ExtrusionRoof(_) => "Click how far across the roof is swept, to either side of the line. Esc cancels.",
            Tool::Dormer => "Click a roof's slope where the dormer's front wall goes, across the slope.",
            Tool::GableEdge => "Click an edge of a roof's footprint: it stands upright, a gable end; click it again to slope it.",
            Tool::Floor => "Click inside the walls to floor them: the slab runs under their outline, its top at the level.",
            Tool::Ceiling => "Click inside a room: a ceiling out to its walls' faces, as high as the Options Bar says.",
            Tool::Shaft(None) => "Click a corner of the shaft opening.",
            Tool::Shaft(Some(_)) => "Click the opposite corner: the shaft cuts the floors, ceilings and roofs up to its top. Esc cancels.",
            Tool::Mass(pts) if pts.is_empty() => "Click the first corner of the mass's footprint.",
            Tool::Mass(pts) if pts.len() < 3 => "Click the footprint's next corner. Esc cancels.",
            Tool::Mass(_) => "Click the next corner; Enter, or the first corner again, finishes the mass. Esc cancels.",
            Tool::DetailLine(pts) if pts.is_empty() => "Click where the detail line starts.",
            Tool::DetailLine(_) => "Click its next point; Enter finishes it, the first point again closes it. Esc cancels.",
            Tool::SlopeArrow(None) => "Click the slope arrow's tail: where the floor's top stays at its height offset.",
            Tool::SlopeArrow(Some(_)) => "Click its head: the floor rises toward it at the Options Bar's slope. Esc cancels.",
            Tool::SketchLine(_) | Tool::SketchPick => "Click where the line starts.",
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(t(self.prompt())).color(pal().muted));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // How many elements are selected, by the selection filter, as Revit shows it.
                ui.label(format!(":{}", self.selection.len()));
                let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                icons::paint(ui.painter(), Icon::Filter, r, pal().icon);
                if let Some(p) = self.canvas.cursor {
                    let u = self.session.model.units;
                    ui.monospace(format!("x {}  y {}", u.with_unit(p.0), u.with_unit(p.1)));
                }
                if let Some(note) = self.snap_note() {
                    ui.label(RichText::new(note).color(pal().accent_ink));
                }
                if self.dirty() {
                    ui.label(RichText::new(t("Unsaved changes")).color(pal().muted));
                }
                self.option_selector(ui);
            });
        });
    }

    /// Why the last command failed, in a box over the view's corner until the next one works.
    fn warning(&mut self, ctx: &Context) {
        if self.status.text.is_empty() {
            return;
        }
        let (title, color) = if self.status.error { (t("Error"), pal().error) } else { (t("Note"), pal().accent_ink) };
        let at = self.view_rect.right_bottom() - vec2(16.0, 16.0);
        Area::new(UiId::new("warning")).order(Order::Foreground).fixed_pos(at).pivot(Align2::RIGHT_BOTTOM).show(ctx, |ui| {
            Frame::popup(ui.style()).fill(pal().surface).stroke(Stroke::new(1.0, color)).show(ui, |ui| {
                ui.set_max_width(380.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(title).strong().color(color));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, Icon::Close, "Dismiss").clicked() {
                            self.status = Status::default();
                        }
                    });
                });
                ui.label(&self.status.text);
            });
        });
    }

    // ---------- the palettes: Properties, Project Browser (`dock.rs` docks them) ----------

    /// The family and type the Component tool places: the one picked, while its family has it, else
    /// the first there is. A fixture tool's are of its kinds only.
    pub(crate) fn placing(&self) -> Option<(String, String)> {
        let m = &self.session.model;
        let fits = |f: &bimcraft_core::family::Family| match self.tool {
            Tool::Fixture(key) => mep::kinds(key).contains(&f.category),
            _ => true,
        };
        self.component
            .clone()
            .filter(|(f, t)| m.families.get(f).is_some_and(|f| fits(f) && f.types.contains_key(t)))
            .or_else(|| m.families.iter().filter(|(_, f)| fits(f)).find_map(|(n, f)| f.types.keys().next().map(|t| (n.clone(), t.clone()))))
    }

    // ---------- the middle: view tabs, schedules ----------

    /// The docked views' tabs: a click opens one, a drag moves it along the bar or, let go off it, floats it.
    fn view_tabs(&mut self, ui: &mut Ui) {
        self.tab_rects.clear();
        let mut closed = None;
        let bar = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for sheet in self.views.clone() {
                // A ceiling plan's tab says so: its level's plan has the level's name too.
                let name = if matches!(sheet, Sheet::Ceiling(_)) { self.view_title(sheet) } else { self.view_name(sheet) };
                let many = self.views.len() > 1;
                let r = tab_button_with(ui, &name, sheet == self.front, false, Sense::click_and_drag(), if many { 18.0 } else { 0.0 });
                self.tab_rects.push((sheet, r.rect));
                // The × is part of the tab, at its right end.
                if many {
                    let x = Rect::from_center_size(pos2(r.rect.right() - 16.0, r.rect.center().y), vec2(16.0, 16.0));
                    let label = tf("Close {}", &[&name]);
                    let c = ui.interact(x, ui.id().with(("close view", sheet)), Sense::click());
                    c.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
                    if c.hovered() {
                        ui.painter().rect_filled(x, 3.0, pal().hover_fill);
                    }
                    let ink = if c.hovered() || sheet == self.front { pal().text } else { pal().muted };
                    let d = 3.5;
                    for (a, b) in [(vec2(-d, -d), vec2(d, d)), (vec2(-d, d), vec2(d, -d))] {
                        ui.painter().line_segment([x.center() + a, x.center() + b], Stroke::new(1.2, ink));
                    }
                    if c.clicked() {
                        closed = Some(sheet);
                    }
                }
                if r.clicked() {
                    self.open_sheet(sheet);
                }
                if r.drag_started() {
                    self.dragging = Some(dock::Drag::View(sheet));
                }
                r.context_menu(|ui| {
                    if ui.add_enabled(many, egui::Button::new(t("Float"))).clicked() {
                        let at = self.view_rect.left_top() + vec2(60.0, 60.0);
                        self.float_view(sheet, at);
                    }
                    if ui.add_enabled(many, egui::Button::new(t("Close"))).clicked() {
                        self.close_view(sheet);
                    }
                });
                ui.add_space(2.0);
            }
        });
        if let Some(sheet) = closed {
            self.close_view(sheet);
        }
        self.tab_bar = bar.response.rect.with_max_x(ui.max_rect().right());
    }

    /// The schedule on screen: the project's, by id, and as it is (one every project has may be
    /// none of the project's yet).
    pub(crate) fn shown_schedule(&self) -> Option<(Option<Id>, bimcraft_core::model::Schedule)> {
        let m = &self.session.model;
        match self.sheet {
            Sheet::Schedule(cat) => Some(schedule::standard(m, cat)),
            Sheet::Table(id) => match m.elements.get(id) {
                Some(Element::Schedule(sc)) => Some((Some(id), sc.clone())),
                _ => None,
            },
            _ => None,
        }
    }

    /// The schedule on screen as a table: its headings, its rows, its totals under a rule.
    fn schedule(&mut self, ui: &mut Ui) {
        let Some((_, sc)) = self.shown_schedule() else { return };
        // The right-click menu, on the schedule and on each of its cells.
        let groups = self.context_items();
        let mut chosen = None;
        Frame::new().fill(pal().surface).inner_margin(16.0).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            let ground = ui.interact(ui.max_rect(), ui.id().with("schedule menu"), egui::Sense::click());
            context::menu(&ground, &groups, &mut chosen);
            let name = self.view_name(self.sheet);
            ui.label(RichText::new(&name).heading());
            ui.add_space(8.0);
            match schedule::table(&self.session.model, &sc) {
                Err(e) => {
                    ui.label(e);
                }
                Ok(t) if t.rows.is_empty() => {
                    ui.label(RichText::new(tf("No rows yet: the {} is empty.", &[&name.to_lowercase()])).color(pal().muted));
                }
                Ok(table) => {
                    let units = self.session.model.units;
                    let said = |c: &schedule::Column, v: &Value| {
                        let text = schedule::shown(units, c.kind, v);
                        if v.is_boolean() { t(&text).to_string() } else { text }
                    };
                    // A row is highlighted while what it lists is selected; a click on it picks that.
                    let selected: Vec<bool> = table.ids.iter().map(|ids| ids.iter().any(|id| self.selection.contains(id))).collect();
                    // Striped, as a grid is; the heading is row 0.
                    let shaded = selected.clone();
                    let row_color = move |row: usize, style: &egui::Style| match row.checked_sub(1).and_then(|r| shaded.get(r)) {
                        Some(true) => Some(style.visuals.selection.bg_fill.gamma_multiply(0.5)),
                        _ => (row % 2 == 1).then_some(style.visuals.faint_bg_color),
                    };
                    let mut picked = None;
                    ScrollArea::both().show(ui, |ui| {
                        Grid::new(("schedule", &sc.name)).with_row_color(row_color).spacing(vec2(28.0, 6.0)).show(ui, |ui| {
                            for c in &table.columns {
                                let heading = schedule::Column { heading: t(&c.heading).to_string(), kind: c.kind };
                                ui.label(RichText::new(schedule::heading(units, &heading)).strong());
                            }
                            ui.end_row();
                            for ((row, ids), &selected) in table.rows.iter().zip(&table.ids).zip(&selected) {
                                for (v, c) in row.iter().zip(&table.columns) {
                                    let said = said(c, v);
                                    let text = RichText::new(&said);
                                    let text = if v.is_number() { text.monospace() } else { text };
                                    let r = ui.add(egui::Label::new(text).sense(egui::Sense::click()));
                                    // Selected or not, for screen readers, as a selectable label says.
                                    r.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, selected, &said));
                                    if r.clicked() {
                                        picked = Some((ids.clone(), ui.input(|i| i.modifiers)));
                                    }
                                    context::menu(&r, &groups, &mut chosen);
                                }
                                ui.end_row();
                            }
                            if let Some(totals) = &table.totals {
                                for (i, (v, c)) in totals.iter().zip(&table.columns).enumerate() {
                                    match v {
                                        Value::Null if i == 0 => ui.label(RichText::new(t("Total")).strong()),
                                        v => ui.monospace(RichText::new(said(c, v)).strong()),
                                    };
                                }
                                ui.end_row();
                            }
                        });
                    });
                    if let Some((ids, mods)) = picked {
                        self.pick_up(ids, mods);
                    }
                }
            }
        });
        if let Some(action) = chosen {
            self.act(action);
        }
    }

    fn dialogs(&mut self, ctx: &Context) {
        self.type_properties(ctx);
        self.family_type_properties(ctx);
        self.materials_window(ctx);
        self.favorites_window(ctx);
        self.family_editor_window(ctx);
        self.snaps_window(ctx);
        self.parameters_window(ctx);
        self.new_schedule_window(ctx);
        self.schedule_window(ctx);
        self.graphics_window(ctx);
        self.filters_window(ctx);
        self.templates_window(ctx);
        self.phasing_window(ctx);
        self.line_weights_window(ctx);
        self.object_styles_window(ctx);
        self.line_styles_window(ctx);
        self.fill_patterns_window(ctx);
        self.options_window(ctx);
        self.purge_window(ctx);
        if std::mem::take(&mut self.link_dialog) {
            self.link_model_dialog(ctx);
        }
        if std::mem::take(&mut self.cloud_dialog) {
            self.point_cloud_dialog(ctx);
        }
        self.point_cloud_window(ctx);
        self.select_by_id_window(ctx);
        self.ids_of_selection_window(ctx);
        self.shortcuts_window(ctx);
        self.settings_window(ctx);
        self.installing_box(ctx);
        self.importing_box(ctx);
        self.report_window(ctx);
        if self.pending.is_none() {
            self.recovery_window(ctx);
        }
        let Some(pending) = self.pending.clone() else { return };
        #[derive(PartialEq)]
        enum Choice {
            Save,
            Discard,
            Cancel,
        }
        let mut choice = None;
        let name = self.session.model.name.clone();
        let r = Modal::new(UiId::new("unsaved")).show(ctx, |ui| {
            ui.label(RichText::new(tf("Save changes to {}?", &[&name])).strong());
            ui.label(t("If you don't, the changes since it was last saved are lost."));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for (label, c) in [("Save", Choice::Save), ("Don't Save", Choice::Discard), ("Cancel", Choice::Cancel)] {
                    if ui.button(t(label)).clicked() {
                        choice = Some(c);
                    }
                }
            });
        });
        if choice.is_some() || r.should_close() {
            self.pending = None;
        }
        match choice {
            Some(Choice::Save) => self.save(ctx, Some(pending)),
            Some(Choice::Discard) => self.proceed(pending, ctx),
            _ => {}
        }
    }
}

impl BimApp {
    /// Cut a window's `events` for one frame after the first button press or release, and hold the
    /// rest, in order, for the frames after (true while any is held). egui takes where a click was,
    /// and what lay under it, from the pointer's last position in the frame, and counts two clicks in
    /// a frame as one: a hand that moves on between one frame and the next, as in fast drawing,
    /// would click somewhere else, or once.
    pub(crate) fn pace(&mut self, events: &mut Vec<Event>) -> bool {
        let mut all = std::mem::take(&mut self.held);
        all.append(events);
        if let Some(cut) = all.iter().position(|e| matches!(e, Event::PointerButton { .. })) {
            self.held = all.split_off(cut + 1);
        }
        *events = all;
        !self.held.is_empty()
    }
}

impl eframe::App for BimApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }

    /// Tab over the plan cycles through what's under the cursor, as in Revit (or moves between a shape's numbers),
    /// rather than moving the keyboard between widgets.
    fn raw_input_hook(&mut self, ctx: &Context, raw: &mut egui::RawInput) {
        self.pace(&mut raw.events);
        if self.canvas.hovering() && !ctx.egui_wants_keyboard_input() {
            self.tab_cycle |= raw.events.iter().any(|e| matches!(e, Event::Key { key: Key::Tab, pressed: true, .. }));
            raw.events.retain(|e| !matches!(e, Event::Key { key: Key::Tab, .. }));
        }
    }
}
