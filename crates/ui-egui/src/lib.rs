//! BIMCraft's desktop front end, in egui: a thin shell over the core's [`Session`]. Panels read
//! the session's model and change it only through [`Session::execute`], so the desktop, the CLI,
//! scripts and agents share one set of commands, rules and undo history. The layout is Revit's:
//! the Quick Access Toolbar over the ribbon's tabs, a contextual Modify tab for the tool or the
//! selection, the Options Bar, Properties over the Project Browser on the left, views in tabs
//! (plans, the 3D view, schedules), and the tool's prompt in the status bar.

use crate::popup::Modal;

use bimcraft_i18n::msg;

mod adaptive_editor;
mod adaptive_place;
mod adaptive_view;
mod assistant;
mod background_grid;
pub mod bench;
mod browser;
mod cad;
mod camera;
mod canvas;
mod classification;
mod clipboard;
mod clouds;
mod codes;
mod context;
pub mod control;
mod coordinates;
pub mod cowork;
mod curtain;
mod dialog;
mod dimtypes;
mod display;
mod dock;
mod draw;
mod editor;
mod electrical;
mod export;
mod favorites;
mod fillpatterns;
mod filter;
mod find;
mod finishes;
mod fly;
mod footer;
mod gpu;
mod halftone;
mod plugin_form;
mod plugin_panels;
mod printsets;
mod profile;
mod publisher;
mod stairgrips;
mod stairtype;
mod viewrange;
pub use gpu::limits_for;
mod asset_manager;
mod graphics;
mod gridtypes;
mod groups;
mod guide_refs;
pub mod icons;
mod image_grips;
mod inquiry;
pub mod lang;
mod linestyles;
mod lineweights;
mod links;
mod manage_links;
mod markdown;
mod materials;
mod mep;
mod mesh;
mod minimap;
mod motion;
mod notes;
mod objectstyles;
mod opening;
mod orbit;
pub mod pacing;
mod panelsched;
mod paper;
mod parameters;
mod phasing;
mod plugins;
mod popup;
mod presets;
pub mod project_templates;
mod projectinfo;
mod properties;
mod purge;
mod pyaddins;
mod rebar_detail;
pub mod recent;
mod reinforce;
mod renames;
mod retained;
pub mod revit;
mod ribbon;
mod scale;
mod script;
mod section;
mod section_box;
mod section_worker;
pub mod settings;
mod shape;
pub mod shortcuts;
mod sketch;
mod sketch3d;
pub mod spacemouse;
mod spots;
mod standards;
mod start;
mod steel;
mod structure;
mod style;
mod swap;
mod templates;
#[cfg(test)]
mod tests;
mod titlebar;
mod titleblocks;
mod triad;
mod typeprops;
mod underlay;
mod updates;
mod view_worker;
pub mod window;
mod workplane;
mod zoom;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use bimcraft_core::dims::Placing as DimMode;
use bimcraft_core::engine::{self, Session};
use bimcraft_core::files;
use bimcraft_core::geom::P2;
use bimcraft_core::model::{CEILING_HEIGHT, DimKind, Element, Id, Justify, LeaderKind, Location, Model, Reference, RoomArea, STAIR_WIDTH};
use bimcraft_core::snap::{Kind as Snap, Snaps};
use bimcraft_core::units::Units;
use bimcraft_core::{element_json, render, schedule, stairs};
use egui::containers::menu::{MenuButton, SubMenuButton};
use egui::{
    Align, Align2, Area, CentralPanel, Color32, ComboBox, Context, CornerRadius, Event, FontId, Frame, Grid, Id as UiId, Key,
    KeyboardShortcut, Layout, Margin, Modifiers, Order, Panel, Popup, Pos2, Rect, Response, RichText, ScrollArea, Sense, Stroke,
    StrokeKind, TextEdit, Ui, UiBuilder, ViewportCommand, WidgetInfo, WidgetType, pos2, vec2,
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
    /// A view of an open family document: its {3D} (none), or a plan or an elevation (`editor/tabs.rs`).
    Family(editor::tabs::DocId, Option<bimcraft_core::family::views::Which>),
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
    /// Room separation lines, one a click from the point before: the last point clicked, while the chain goes on.
    RoomSeparator(Option<P2>),
    /// The Options Bar asks for the new level's elevation.
    Level,
    /// The grid's start, once clicked.
    Grid(Option<P2>),
    /// A reference plane's start, once clicked: two clicks draw an upright plane through them.
    ReferencePlane(Option<P2>),
    /// Move the selection: its base point, once clicked.
    Move(Option<P2>),
    /// Place grid lines on a curtain wall: hover to see where, click to place, Esc to stop.
    CurtainGrid {
        wall: Id,
        way: bimcraft_core::curtain::GridWay,
    },
    /// Take stretches of a curtain wall's grid lines off, or put them back: hover to see which, click, Esc to stop.
    CurtainSegment {
        wall: Id,
    },
    /// Copy the selection: its base point, once clicked.
    Copy(Option<P2>),
    /// Paste from Clipboard: what's pasted follows the cursor until a click puts it down (`clipboard.rs`).
    Paste,
    /// Aligned to Picked Level: a click on a level line in a section or an elevation pastes onto it.
    PickLevel,
    /// Rotate the selection about `centre`: the start of the angle, once clicked; with `place`, the
    /// next click moves the centre.
    Rotate {
        centre: P2,
        start: Option<P2>,
        place: bool,
    },
    /// Rotate Project North: the whole model about `centre` (the Project Base Point at first), as Rotate turns a
    /// selection, and every location's angle back by as much, so nothing moves in the world.
    RotateNorth {
        centre: P2,
        start: Option<P2>,
        place: bool,
    },
    /// Specify Coordinates at a Point: the point the shared coordinates typed in the Location window are at, picked in the
    /// view (snapped) while the window steps aside.
    SpecifyPoint,
    /// The rectangle an export is cut to, drawn in the view while the Export dialog steps aside: its first corner, once
    /// clicked, in the view's own coordinates.
    ExportRegion(Option<P2>),
    /// Acquire Coordinates: the link or CAD drawing clicked gives the project its shared coordinates.
    Acquire,
    /// Scale the selection's annotation, CAD drawings, images and PDFs: how (`scale.rs`), then the point that stays or two
    /// points and the length between them.
    Scale(scale::Step),
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
    /// Place Model Group: the Options Bar's model group type where clicked, on the view's level.
    ModelGroup,
    /// Repeating Detail: the Options Bar's group along a line, its start once clicked.
    RepeatingDetail(Option<P2>),
    /// Lay the material in the Options Bar over the face clicked in the 3D view, as Revit's Paint does.
    Paint,
    /// Take the paint off the face clicked in the 3D view.
    Unpaint,
    /// An aligned or linear dimension, laid as its mode says: the references and points picked so far. A click away from
    /// any places it.
    Dimension(DimMode, Vec<Reference>),
    /// An angular dimension: the lines picked so far, two that meet; then a click where its arc goes.
    Angular(Vec<Reference>),
    /// A radial, diameter or arc length dimension: what round was picked (an arc length's, then two points on its arc, if
    /// it measures part of it), then a click toward where it goes.
    Round(DimKind, Vec<Reference>),
    /// A text note where clicked, saying what the Options Bar says.
    Text,
    /// A spot elevation where clicked.
    Spot,
    /// A spot slope where clicked: how steeply the floor's or roof's top falls there.
    SpotSlope,
    /// A spot coordinate: the point it reads, once clicked; the next click ends its leader.
    SpotCoordinate(Option<P2>),
    /// A ruler laid between two points: the first, once clicked.
    Measure(Option<P2>),
    /// Tag the door or window clicked.
    Tag,
    /// A section: its start, once clicked.
    Section(Option<P2>),
    /// A detail view's fragment: its first corner, once clicked, on a plan, a section or an elevation.
    DetailView(Option<P2>),
    /// An elevation, looking at the side of the building clicked beside.
    Elevation,
    /// A camera: the point its eye stands at, once clicked; then its target. The eye and target heights
    /// are the Options Bar's over the plan's level (Revit's Camera tool).
    Camera(Option<P2>),
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
    /// Shape Editing's Modify Sub Elements: a point of the selected floor's shape, a corner, or both ends of an edge picked,
    /// to the Options Bar's elevation.
    ModifySubElements,
    /// Shape Editing's Add Point: a point on the selected floor's top at the Options Bar's elevation.
    AddPoint,
    /// Shape Editing's Add Split Line: a crease across the selected floor's top; its first end, once clicked.
    AddSplitLine(Option<P2>),
    /// Sketch mode's Draw panel: the points clicked so far of the shape it draws (Line: since the last loop closed).
    SketchLine(Vec<P2>),
    /// Sketch mode's Pick Walls: a line along each wall clicked.
    SketchPick,
    /// Fill Patterns' New Pattern > Custom: the tile's corners, the first once clicked (the lines are picked already).
    PatternCorner(Option<P2>),
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
    Collaborate,
    View,
    Manage,
    /// The user's Python scripts: the Player, and the buttons made of them.
    PyAddins,
    Modify,
    Context,
    /// A tab a plugin declared, by its place in [`BimApp::plugin_tabs`].
    Plugin(usize),
}

/// A key for the tab, for the frame data egui keeps between frames: a plain sort in reading order.
fn tab_key(tab: Tab) -> u64 {
    match tab {
        Tab::Architecture => 0,
        Tab::Structure => 1,
        Tab::Plumbing => 2,
        Tab::Electrical => 3,
        Tab::Lightning => 4,
        Tab::Data => 5,
        Tab::Insert => 6,
        Tab::Annotate => 7,
        Tab::Massing => 8,
        Tab::View => 9,
        Tab::Manage => 10,
        Tab::Modify => 11,
        Tab::Context => 12,
        Tab::Collaborate => 13,
        Tab::PyAddins => 14,
        Tab::Plugin(i) => 100 + i as u64,
    }
}

/// A step that would drop unsaved changes, waiting on the user.
#[derive(Clone, Debug, PartialEq)]
enum Pending {
    Recover(bimcraft_core::autosave::Backup),
    /// An empty project.
    New,
    /// A new project from the template at the path (`project_templates.rs`).
    Template(PathBuf),
    Open,
    /// The sample house, as a project nobody has saved ([`bimcraft_core::sample::SAMPLE`]).
    Sample,
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
pub(crate) const SCHEDULES: [(&str, &str); 7] = schedule::VIEWS;

/// Ribbon buttons that pick tools: icon, label, tooltip, tool.
type Tools<'a> = &'a [(Icon, &'a str, &'a str, Tool)];

/// The Modify panel's small buttons: icon, label, tooltip, the tool (none for Delete), enabled.
type Buttons<'a> = &'a [(Icon, &'a str, &'a str, Option<Tool>, bool)];

/// The desktop's own two-letter shortcuts, which [`BimApp::shortcut`] takes before the commands'
/// aliases, and what Keyboard Shortcuts (KS) calls them.
const SHORTCUTS: [(&str, &str); 70] = [
    ("3D", "3D View"),
    ("MD", "Modify"),
    ("SB", "Floor: Structural"),
    ("RF", "Reinforce"),
    ("MM", msg!("Mirror Pick Axis")),
    ("DM", msg!("Mirror Draw Axis")),
    ("AL", msg!("Align")),
    ("OF", "Offset"),
    ("DW", "Demolish"),
    ("ZF", "Zoom to Fit"),
    ("ZE", "Zoom to Fit"),
    ("ZA", "Zoom to Fit"),
    ("TL", "Thin Lines"),
    ("CS", msg!("Create Similar")),
    ("PP", "Properties"),
    ("VP", "Properties"),
    ("NP", "Project Browser"),
    ("SA", "Select All Instances"),
    ("AG", msg!("Alignment Guides")),
    ("OG", msg!("Orthogonal Guides")),
    ("SE", msg!("Snap to Endpoints")),
    ("SM", msg!("Snap to Midpoints")),
    ("SI", msg!("Snap to Intersections")),
    ("SC", msg!("Snap to Centres")),
    ("SP", msg!("Snap to Perpendiculars")),
    ("SQ", msg!("Snap to Quadrants")),
    ("ST", msg!("Snap to Tangents")),
    ("SN", msg!("Snap to Nearest")),
    ("SO", msg!("Snaps Off")),
    ("SS", msg!("Turn Snap Override Off")),
    ("VV", "Visibility/Graphics"),
    ("VG", "Visibility/Graphics"),
    ("EH", "Hide Element"),
    ("VH", "Hide Category"),
    ("HH", msg!("Temporary Hide Element")),
    ("HC", msg!("Temporary Hide Category")),
    ("HI", msg!("Temporary Isolate Element")),
    ("IC", msg!("Temporary Isolate Category")),
    ("HR", msg!("Reset Temporary Hide/Isolate")),
    ("RH", "Reveal Hidden Elements"),
    ("EU", msg!("Unhide Elements")),
    ("UE", "Unhide Elements"),
    ("VU", "Unhide Category"),
    ("EO", msg!("Override Graphics by Element")),
    ("FT", "Filters"),
    ("VT", "View Templates"),
    ("PH", "Phasing"),
    ("DO", "Design Options"),
    ("WF", "Wireframe"),
    ("HL", "Hidden Line"),
    ("SD", "Shaded"),
    ("KS", "Keyboard Shortcuts"),
    // Revit has none for Manage > Location; BIMCraft's window of it.
    ("LO", msg!("Location")),
    ("AI", msg!("Assistant")),
    ("CW", msg!("Co-Work")),
    ("BX", "Section Box"),
    ("GD", "Graphic Display Options"),
    // Revit's fly has none; the 3D view's fly mode.
    ("FY", msg!("Fly Mode")),
    // Revit has none for its title blocks; no command or tool takes TB.
    ("TB", "Title Blocks"),
    // Revit has no building codes, and none of these for its stairs; letters it doesn't use.
    ("BC", "Building Codes"),
    ("CK", "Code Check"),
    ("TS", "Convert to Sketch"),
    ("SY", "Separate Storeys"),
    // Revit has no PyAddins: letters free in both, Reload as Load Scripts and Stop as eXit Script.
    ("PY", "Player"),
    ("RS", "Run Script"),
    ("LS", "Reload"),
    ("NS", "New Script"),
    ("XS", "Stop"),
    // Revit has no Classification Manager; CI is free there and here.
    ("CI", "Classification Manager"),
    // Revit has no Underlay Settings of this kind (its Underlay is a plan's range of levels); UD is free there and here.
    ("UD", "Underlay Settings"),
];

/// How far an arrow key nudges the selection on screen, in pixels, before [`nudge_step`] rounds it to a length.
const NUDGE_PX: f64 = 10.0;

/// The length an arrow key nudges by in a view of `scale` pixels a millimetre: [`NUDGE_PX`] pixels' worth, rounded down to 1, 2
/// or 5 times a power of ten, so a step reads as a round number at any zoom; a millimetre at the least.
pub(crate) fn nudge_step(scale: f64) -> f64 {
    let raw = NUDGE_PX / scale;
    if !raw.is_finite() || raw <= 1.0 {
        return 1.0;
    }
    let power = 10f64.powf(raw.log10().floor());
    let lead = raw / power;
    power
        * if lead >= 5.0 {
            5.0
        } else if lead >= 2.0 {
            2.0
        } else {
            1.0
        }
}

/// Removes the key events a shortcut took, so no other handler runs the same command. Matching is
/// on the event's logical key, its physical key and its exact modifiers.
pub(crate) fn consume_key_events(ctx: &Context, handled: &[(Key, Option<Key>, Modifiers)]) {
    if handled.is_empty() {
        return;
    }
    ctx.input_mut(|i| {
        i.events.retain(|e| {
            !matches!(
                e,
                Event::Key { key, physical_key, pressed: true, repeat: false, modifiers, .. }
                    if handled.iter().any(|(k, p, m)| k == key && p == physical_key && m == modifiers)
            )
        })
    });
}

/// How long a letter typed without a modifier waits for the next one of its sequence (W for Wall): past it the letter is
/// forgotten, so a stray key never completes a pair much later.
pub(crate) const SEQUENCE_WAIT: f64 = 2.0;

/// The letters and digits typed this frame without Ctrl, Alt or Cmd, taken out of the frame's events, upper case: a
/// sequence is two or three of them, typed as Revit's are (`WA` for Wall), with or without Shift. They are read by their
/// physical key, so a Russian layout types the same sequences; the family document's own keys come this way. A key held
/// with Ctrl is no part of a sequence: it is the combination Keyboard Shortcuts gave it, or nothing at all.
pub(crate) fn sequence_letters(ctx: &Context) -> Vec<char> {
    ctx.input_mut(|i| {
        let events = std::mem::take(&mut i.events);
        let mut out = Vec::new();
        let mut kept = Vec::with_capacity(events.len());
        for event in events {
            match &event {
                Event::Key { key, physical_key, pressed: true, repeat: false, modifiers, .. }
                    if !modifiers.ctrl && !modifiers.alt && !modifiers.mac_cmd && !modifiers.command =>
                {
                    let name = physical_key.unwrap_or(*key).name();
                    let mut chars = name.chars();
                    match (chars.next(), chars.next()) {
                        (Some(c), None) if c.is_ascii_alphanumeric() => out.push(c.to_ascii_uppercase()),
                        _ => kept.push(event),
                    }
                }
                _ => kept.push(event),
            }
        }
        i.events = kept;
        out
    })
}

/// Every two-letter shortcut and what it does, as the registry's starting sequences. Tests use it
/// to check that every listed pair resolves.
#[cfg(test)]
fn shortcuts() -> Vec<(&'static str, &'static str)> {
    let mut out: Vec<(&'static str, &'static str)> = Vec::new();
    for a in crate::shortcuts::registry().actions() {
        for k in &a.defaults {
            if !(k.len() == 2 && k.chars().all(|c| c.is_ascii_alphanumeric())) {
                continue;
            }
            // CL is Revit's for two different actions: the dialog lists both, and the context picks one.
            if *k != "CL" && out.iter().any(|(seen, _)| seen == k) {
                continue;
            }
            out.push((k, a.label));
        }
    }
    out
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

/// Elements' JSON (`element_json`) by id, and the model it was found in.
type KeptJson = (Weak<Model>, HashMap<Id, Arc<Value>>);

pub struct BimApp {
    pub session: Session,
    /// A scenario being run on the window (`bimcraft --bench`): it drives the window and times its frames.
    pub bench: Option<bench::Bench>,
    /// The window's own pacing of its frames where the system's vsync doesn't (Windows: `pacing.rs`). None where it doesn't
    /// pace them (other systems, the tests).
    pub frames: Option<pacing::Pacer>,
    /// The system's requests since the last frame, read once a frame. None where it sends none.
    pub os_events: Option<Box<dyn FnMut() -> Vec<OsEvent>>>,
    /// The window draws its own title bar (`titlebar.rs`): the desktop's `main` says so where the system lets it (Windows and
    /// Linux), unless `system_title_bar`, the user's ask for the system's (Settings > User Interface, kept in `settings.json`).
    pub own_title_bar: bool,
    pub system_title_bar: bool,
    /// Manage > Classification Manager, while it's open.
    classification: Option<classification::ClassificationManager>,
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
    /// How the views' area is laid out: the tabbed view, columns and rows, or a tile of equal cells.
    arrange: dock::Arrange,
    /// The whole area the views take this frame, between the palettes and under their tab bar.
    views_rect: Rect,
    /// The views' area as this frame lays it out: the windows off the tab bar, and the pane the tabs show.
    placements: Vec<(Sheet, Rect)>,
    tab_pane: Rect,
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
    /// How far down the view the tabs reach where they float over it (else the view's top): what floats over the view
    /// (the Options Bar) and what docks along it start below.
    tabs_foot: f32,
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
    background_grid_dialog: Option<background_grid::GridDialog>,
    /// The underlay of the view on screen, made and kept on the card (`underlay.rs`).
    pub(crate) underlay: underlay::Layer,
    /// Underlay Settings, open for this view.
    pub(crate) underlay_dialog: Option<Id>,
    /// Underlay Settings' Move or Rotate, picking in the view.
    pub(crate) underlay_edit: Option<underlay::Edit>,
    /// A colour being picked in Underlay Settings, set once the hand lets go.
    pub(crate) underlay_color: Option<[u8; 3]>,
    /// What Underlay Settings' last change was refused for.
    pub(crate) underlay_error: Option<String>,
    /// Global grid snapping preference, independent of view graphics and templates.
    grid_snapping: bool,
    /// How guides and the live drawing's aids are drawn (Manage > Snaps): the refined look, or the classic one to compare it with.
    guide_style: style::GuideStyle,
    /// Whether Save and Save As write the project zipped (File > Settings > Files, the File menu): off, plain JSON.
    pub(crate) compressed_save: bool,
    /// Whether the 3D view's section box shows what it cuts filled (Settings > Performance): off, its faces are left hollow,
    /// which spares cutting the model each time the box moves.
    pub(crate) cut_fills_3d: bool,
    /// Select Elements by Face, in a family document's plans and elevations (`editor/selection.rs`): on, a
    /// face selects what it is of; off, only an edge does. The user's, kept in the settings file.
    pub(crate) select_by_face: bool,
    /// Drag Elements on Selection, beside it: on, a drag from the face of a form or a nested instance that isn't
    /// selected is that element's and selects it, where a face selects; off (at first), that face is a selection box's to
    /// begin in. The user's, kept in the settings file.
    pub(crate) drag_on_selection: bool,
    /// The origin marker on a plan, and the view's name in its corner (File > Settings): working aids, drawn on the screen and
    /// never in a drawing that is printed or exported.
    show_origin: bool,
    show_view_name: bool,
    ribbon_bottom: Pos2,
    /// The Compact layout (File > Settings): the bars and palettes flush to the edges, not floating cards.
    compact: bool,
    /// Which interface the window wears (File > Settings): BIMCraft's own, or Rev.
    profile: style::Profile,
    /// A preset's colours when dark, and the user's accent (Settings > User Interface > Theme).
    scheme: style::Scheme,
    accent: Option<[u8; 3]>,
    /// The user's own panel, canvas and text colours over any scheme (the same page).
    panel_colour: Option<[u8; 3]>,
    canvas_colour: Option<[u8; 3]>,
    text_colour: Option<[u8; 3]>,
    /// The category the Properties palette's filter narrows a selection of several to, and the selection it was picked for: a new
    /// selection drops it (`BimApp::palette_ids`).
    palette_filter: Option<(&'static str, Vec<Id>)>,
    /// What `element_json` says of the elements the palette has shown, of the model it was found in (`BimApp::elements_json`):
    /// the palette reads it every frame, and a component's room costs its level's room boundaries.
    palette_json: RefCell<Option<KeptJson>>,
    /// File > Settings, while open, at its page; the settings file, when the app was given one, and what it holds; the
    /// theme it asks for, put in force on the next frame.
    settings_page: Option<settings::Page>,
    /// Find (Ctrl+K), while it's open: what is typed in it.
    find: Option<String>,
    /// The project's scale, the tools and panels the user took out, whether Edit Tools is on, and the kept presets
    /// (`presets.rs`); with what Settings' preset name field holds and why the last save was refused.
    scale: presets::Scale,
    tools_hidden: BTreeSet<String>,
    /// The ribbon panels the user took out (`tab:<tab>/<panel>`): their tools stay in Find and in Edit Tools.
    panels_hidden: BTreeSet<String>,
    /// The panel keys the window has drawn, which the preset editor lists under their tabs.
    panel_keys: BTreeSet<String>,
    editing_tools: bool,
    presets: Vec<presets::Preset>,
    /// The preset last applied, which the title bar's chip names and marks: none until one is used.
    active_preset: Option<String>,
    /// The preset being edited, if its dialog is open: its draft is not the window until the preset is used.
    preset_editor: Option<presets::PresetEditor>,
    preset_name: String,
    preset_note: Option<String>,
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
    /// Pick Walls' Options Bar: whether a boundary line goes on a wall's core face (else its finish) and how far out
    /// from the face it lies, in the project's units. They last while the sketch is open.
    pub(crate) pick_core: bool,
    pub(crate) pick_offset: String,
    /// Pick Walls' line on the other side of its wall, as Space flips it: its face mirrored across the wall.
    pub(crate) pick_flip: bool,
    /// Walls' Outline: the line of the walls the contour follows (their centre line, their interior or their exterior
    /// face), which component of a wall that face is (its finish or its core) and how far out from it the line lies.
    pub(crate) outline_side: crate::sketch::OutlineSide,
    pub(crate) outline_core: bool,
    pub(crate) outline_offset: String,
    /// A centre-line outline offset to the outside of the loop, as Space turns it; by default it goes inside.
    pub(crate) outline_flip: bool,
    /// Walls' Outline asked for and its Options Bar open: the contour is made when DRAW is pressed there.
    pub(crate) outline_pending: bool,
    /// The walls picked for Walls' Outline: DRAW follows these alone, and with none it follows every wall of the level.
    pub(crate) outline_walls: BTreeSet<Id>,
    /// The sketch Line chain's first and last line indices while Offset draws it: corners are trimmed to meet.
    pub(crate) sketch_line_chain: Option<(usize, usize)>,
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
    /// The Stair tool's winders: how many treads a turn is made of, none for a landing.
    stair_winders: u32,
    /// How the Stair tool draws: along a line, straight or in arcs; a spiral about a centre; or riser by riser.
    stair_shape: StairShape,
    /// Whether the segment the Stair tool draws next is a landing, not a flight of steps.
    stair_landing: bool,
    /// What each segment drawn so far is: a landing, an arc, and how wide it was asked to be.
    stair_kinds: Vec<(bool, bool, Option<f64>)>,
    /// The Stair tool's top (none: the level above), its height when unconnected, and whether it stands on every level on
    /// the way.
    stair_top: Option<Option<Id>>,
    stair_height: String,
    stair_multistorey: bool,
    /// Whether the Stair tool keeps one tread depth the whole stair through (on as it starts), or lets each flight's
    /// treads stretch to end where it is drawn.
    pub(crate) stair_lock_going: bool,
    /// Whether the Stair tool draws a stair from its top down: the first click is where it arrives.
    pub(crate) stair_from_top: bool,
    /// The Stair tool's riser height and tread depth as typed: empty, the type's rules or the building code's.
    pub(crate) stair_riser: String,
    pub(crate) stair_tread: String,
    /// The nearest stair inside the rules to the one being drawn, while it is outside them: its riser, tread and width,
    /// which Tab takes.
    pub(crate) stair_offer: std::cell::Cell<Option<(f64, f64, f64)>>,
    /// Which way the Stair tool's spiral winds.
    stair_clockwise: bool,
    /// The Stair tool's location line, the line of the stair the clicks are on, and how far the stair stands from it.
    stair_line: bimcraft_core::model::StairLine,
    stair_offset: String,
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
    /// How the Ramp tool draws: along straight stretches or arcs, or round a centre; whether the next stretch is a landing;
    /// what each stretch clicked was (a landing, an arc, its slope); and which way a circular one winds.
    ramp_shape: RampShape,
    ramp_landing: bool,
    ramp_kinds: Vec<(bool, bool, f64)>,
    ramp_clockwise: bool,
    /// The line the Ramp tool draws along (its left edge, its middle or its right edge), and the unit its slope is typed in.
    pub(crate) ramp_line: bimcraft_core::model::StairLine,
    pub(crate) slope_unit: SlopeUnit,
    /// How the Ceiling tool's ceilings are; the Slope Arrow's slope; the Shaft tool's top.
    ceiling_height: String,
    floor_slope: String,
    /// Add Point's elevation: above the floor's top as it would be without its shape, or with `shape_absolute`, above its
    /// level; empty, the top's height where it's clicked.
    shape_z: String,
    /// What the Options Bar's elevations count from.
    pub(crate) shape_base: crate::shape::Base,
    /// Add Point's mode, as Revit's: along the surface as it is, `shape_offset` over it; else Absolute, at `shape_z` above
    /// the floor's level.
    pub(crate) shape_along: bool,
    pub(crate) shape_offset: String,
    /// The selected floor's shape point a drag under Modify Sub Elements holds, where it was.
    shape_grip: Option<P2>,
    /// The selected floor as a grip's drag would leave it, plan or 3D, while it's dragged: its creases drawn over it.
    pub(crate) shape_preview: Option<bimcraft_core::model::Floor>,
    /// What Modify Sub Elements has selected of the shaped floor's shape.
    pub(crate) shape_picked: Vec<crate::shape::Sub>,
    /// A shape point's height label clicked into a field: the point, and what's typed.
    pub(crate) shape_label_edit: Option<(P2, String)>,
    /// Where the plan drew the shape's height labels, last frame: a click there is the label's, not the tool's.
    pub(crate) shape_label_rects: Vec<egui::Rect>,
    shaft_top: Option<Option<Id>>,
    shaft_height: String,
    /// The Mass tool's top, as the Shaft tool's.
    mass_top: Option<Option<Id>>,
    mass_height: String,
    /// The Camera tool's eye and target heights over the plan's level, and its lens, as typed.
    camera_eye_height: String,
    camera_target_height: String,
    camera_lens: String,
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
    /// Notes written with a leader: the Text tool's first click is the point it reaches, kept here with its view.
    pub(crate) note_leader: bool,
    /// How that leader runs: straight, in two segments or curved.
    pub(crate) note_leader_kind: LeaderKind,
    pub(crate) note_arrow: Option<(Id, P2)>,
    /// The note being written on the canvas.
    note_edit: Option<notes::NoteEdit>,
    /// Text Type Properties, while it's open.
    text_type_dialog: Option<notes::TextTypeDialog>,
    dimension_type_dialog: Option<dimtypes::DimensionTypeDialog>,
    /// Spot Coordinate Type Properties, while it's open.
    spot_type_dialog: Option<spots::SpotTypeDialog>,
    /// The spot coordinate whose leader's end is being dragged.
    spot_grip: Option<Id>,
    /// The selected image's corner being dragged: the image, and which corner.
    pub(crate) image_grip: Option<(Id, usize)>,
    /// Insert > PDF's page picker, while a PDF of several pages waits for its page.
    pub(crate) pdf_page_box: Option<mesh::PdfPageBox>,
    curtain_editor: Option<curtain::CurtainEditor>,
    /// Go To View, while Edit Profile begun in a plan asks for a view that looks at the wall.
    go_to_view: Option<profile::GoToView>,
    /// The one part of the selected curtain wall Tab has gone to: what Properties and Delete then act on.
    curtain_part: Option<(Id, bimcraft_core::curtain::Address)>,
    /// With a mullion's whole grid picked, the other places on it.
    curtain_more: Vec<bimcraft_core::curtain::Address>,
    /// Where the Curtain Grid tool would place a line: the wall, how far, and the way.
    curtain_hover: Option<(Id, f64, bimcraft_core::curtain::GridWay)>,
    /// The part of the selected curtain wall a Tab would go to next, under the cursor.
    curtain_aim: Option<(Id, bimcraft_core::curtain::Address)>,
    /// The stretch of a grid line Add/Remove Segments would take off or put back.
    curtain_segment_hover: Option<(Id, bimcraft_core::curtain::Address)>,
    /// The layout of the curtain wall a frame works with, kept while the model is the same.
    curtain_laid: curtain::Laid,
    /// The model the malformed curtain walls were last looked for in, those it had, and those being asked about.
    curtain_seen: Option<Arc<Model>>,
    curtain_known: std::collections::BTreeSet<Id>,
    curtain_asking: std::collections::BTreeSet<Id>,
    grid_type_dialog: Option<gridtypes::GridTypeDialog>,
    dimension_text_dialog: Option<dimtypes::DimensionTextDialog>,
    /// The line style the Detail Line tool draws in; blank for the project's Thin Lines.
    line_style: String,
    /// The Linework tool's line style: a style's name, or `<Invisible lines>` or `<By Category>` (in English).
    linework_style: String,
    /// The detail group type Place Detail Group places.
    group_type: String,
    /// The model group type Place Model Group places, and whether what it places is flexible.
    model_group_type: String,
    group_flexible: bool,
    /// Create Group's question for a selection that can't make one group: what each kind would take of it.
    group_split: Option<bimcraft_core::groups::Split>,
    /// Edit Group's Add or Remove, picking.
    group_pick: Option<groups::GroupPick>,
    /// A command refused because it would change a group from inside, while the window asks what to do with it.
    group_conflict: Option<groups::Conflict>,
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
    /// Linked 3D View, while it's on: where the 3D view was before it floated beside the plan.
    linked_3d: Option<section_box::Was>,
    /// The project, and what the views draw of it with its links built in: see [`BimApp::view_model`].
    seen: Option<(Arc<Model>, Arc<Model>)>,
    /// Link Model was asked for: its file dialog opens on the next frame, so a shortcut typed alone never waits on one.
    link_dialog: bool,
    /// Insert > Point Cloud was pressed: its file dialog opens this frame.
    cloud_dialog: bool,
    /// Bringing scan files in: the box, while it's open.
    cloud_box: Option<clouds::CloudBox>,
    /// Insert > Import CAD (false) or Link CAD (true) was pressed: its file dialog opens this frame.
    cad_dialog: Option<bool>,
    /// Bringing a CAD drawing in: the box, while it's open.
    cad_box: Option<cad::CadBox>,
    /// Link Model's box (its positioning), while it's open.
    pub(crate) link_box: Option<links::LinkBox>,
    /// A selected element's temporary dimension being typed: the element, which of its dimensions (a wall has one, a
    /// door or window two: from its wall's start, from its end), the length so far.
    dim_edit: Option<(Id, usize, String)>,
    /// The pointer is over a dimension's number, padlock, EQ or text button: the canvas leaves the click to it.
    over_dimension_control: bool,
    /// A number field was left by Esc: the key's own Esc handling must not clear the selection then.
    dim_edit_escaped: bool,
    /// Edit Dimension References, while it's on: the dimension, its references being picked, and where its line goes.
    dimension_refs_edit: Option<canvas::DimensionRefsEdit>,
    /// The dimension segment picked out of a chain, by Tab and a click: the dimension and which segment.
    dimension_segment: Option<(Id, usize)>,
    /// The segment Tab is stepping over now, highlighted until a click picks it.
    dimension_segment_hover: Option<(Id, usize)>,
    /// The reference of an element Tab is stepping over while a dimension is picked or its references are edited.
    dimension_reference_hover: Option<(Id, Reference)>,
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
    /// Tab pressed over the plan, taken from egui's focus keys: step the numbers a shape is typed by, or before its first
    /// point cycle what's under the cursor.
    tab_cycle: bool,
    /// Shift+Tab pressed over the plan: cycle the lines of the wall under the pointer, mid-chain or before the first point.
    face_cycle: bool,
    /// What a window handed over that `pace` kept back for the frames after the one it came in.
    held: Vec<Event>,
    /// The Keyboard Shortcuts dialog is open (KS); the Manage tab drops its Project Units menu open
    /// the next time it shows (UN).
    show_shortcuts: bool,
    /// The action bindings: several per action, chords and Revit-style sequences.
    pub shortcuts: shortcuts::Shortcuts,
    pub shortcuts_dialog: shortcuts::ShortcutsDialog,
    /// The user's named snapshots of the bindings, kept in `settings.json`; built-in presets are generated.
    pub shortcut_presets: Vec<shortcuts::ShortcutPreset>,
    /// The preset the current set came from: `builtin:<code>` or a user's name.
    pub active_shortcut_preset: Option<String>,
    open_units: bool,
    /// Where the Options Bar was drawn last frame, for a view's navigation bar to keep clear of it (nothing when it isn't there).
    options_rect: Rect,
    canvas: canvas::Canvas,
    orbit: orbit::Orbit,
    /// Which 3D view the 3D tab shows: a view's id, or none for the default `{3D}` (made when it is first changed). Set by
    /// opening a camera view (`open_3d`), as Revit's 3D views each have their own tab.
    active_3d: Option<Id>,
    /// The graphics card the 3D view may be drawn on (Settings > Performance).
    card: gpu::Card,
    cut: section::Cut,
    minimap: minimap::Minimap,
    paper: paper::Papers,
    /// The model as last opened or saved: any other is unsaved.
    saved: Arc<Model>,
    autosave: autosave::State,
    /// The palette field being typed in: element, field, text so far.
    edit: Option<(Id, String, String)>,
    /// The Level tool's elevation and name, as typed in the Options Bar.
    new_level: (String, String),
    /// The letters typed without a modifier, for the two- and three-key sequences (`WA` for Wall): one waits for the next
    /// ([`SEQUENCE_WAIT`]).
    typed: String,
    /// When the one letter in `typed` was typed, by the window's clock.
    typed_at: f64,
    pending: Option<Pending>,
    /// What Delete was asked to take while it asks whether rooms' floors and ceilings go with them.
    deleting_rooms: Option<Vec<Id>>,
    type_dialog: Option<properties::TypeDialog>,
    materials_dialog: Option<materials::MaterialsDialog>,
    family_dialog: Option<properties::FamilyDialog>,
    /// The Favorites window, while it's open; where the favorites file is, when the app was told (else the usual place).
    favorites_dialog: Option<favorites::FavoritesDialog>,
    /// The family documents open, each a tab or more beside the project's (`editor/tabs.rs`), and the next one's id.
    families: Vec<editor::tabs::FamilyDoc>,
    next_doc: u32,
    /// A family document being closed, asked about first while it has unsaved changes.
    doc_closing: Option<editor::tabs::Closing>,
    /// File > New, Open, Close or quitting, its family documents closed: the project's question next.
    after_documents: Option<Pending>,
    /// File > New > Family…'s dialog, while it is open: the category picked.
    pub(crate) new_family_dialog: Option<bimcraft_core::family::Category>,
    /// The family document was opened from the start page with no project behind it: closed, the start page comes back.
    pub(crate) family_from_home: bool,
    /// A family being loaded into the project, waiting on "Family Already Exists" (`editor::exists`).
    family_loading: Option<editor::exists::Loading>,
    /// What the last family load decided: how the project's family was overwritten, and its shared nested families.
    pub(crate) family_decided: Option<editor::exists::Decided>,
    favorites_path: Option<std::path::PathBuf>,
    /// The files File > Open Recent lists, and where they are kept (see [`BimApp::keep_recent_in`]; none until the app is told).
    recent: recent::Recent,
    recent_path: Option<std::path::PathBuf>,
    /// The templates New lists, and where they are kept (see [`BimApp::keep_templates_in`]; none until the app is told).
    templates: project_templates::Templates,
    templates_path: Option<std::path::PathBuf>,
    /// The New Project popup is up (`project_templates.rs`).
    new_project_popup: bool,
    /// The template picked in Settings > Templates.
    template_picked: Option<usize>,
    /// The start page: up when the window opens on no project, and from the quick access bar's Home.
    home: start::Home,
    parameter_manager: Option<parameters::ParameterManager>,
    schedule_dialog: Option<properties::ScheduleDialog>,
    /// Visibility/Graphics, Filters and View Templates, while they're open.
    vg_dialog: Option<graphics::VgDialog>,
    filters_dialog: Option<graphics::FiltersDialog>,
    templates_dialog: Option<templates::TemplatesDialog>,
    /// Phasing and Design Options, while they're open, and the design option (`set: option`) new
    /// elements are made in: none is the main model.
    phasing_dialog: Option<phasing::PhasingDialog>,
    /// Graphic Display Options (GD), while open.
    display_dialog: Option<display::DisplayDialog>,
    /// View Range (a plan's Properties, Edit…), while open.
    view_range_dialog: Option<viewrange::RangeDialog>,
    /// Manage > Annotation Settings > Halftone/Underlay, while open.
    halftone_dialog: Option<halftone::HalftoneDialog>,
    project_info_dialog: Option<projectinfo::ProjectInfoDialog>,
    /// Structure > Reinforce, while open, and what was typed in it for each kind of host.
    reinforce_dialog: Option<reinforce::ReinforceDialog>,
    steel_dialog: Option<steel::SteelDialog>,
    detail_dialog: Option<rebar_detail::DetailDialog>,
    reinforce_kept: BTreeMap<reinforce::Form, BTreeMap<&'static str, String>>,
    /// The slot picked in the panel schedule on screen.
    panel_slot: Option<u32>,
    /// Manage > Line Weights, while open.
    line_weights_dialog: Option<lineweights::LineWeightsDialog>,
    /// Manage > Object Styles, while open.
    object_styles_dialog: Option<objectstyles::ObjectStylesDialog>,
    /// Manage > Line Styles, while open.
    line_styles_dialog: Option<linestyles::LineStylesDialog>,
    /// View > Title Blocks, while open.
    title_blocks_dialog: Option<titleblocks::TitleBlocksDialog>,
    fill_patterns_dialog: Option<fillpatterns::FillPatternsDialog>,
    /// Sketch mode: the lines of an outline being drawn, until Finish or Cancel.
    pub sketch: Option<sketch::Sketch>,
    /// The project the window is on (its `uid`, which a project keeps for life): one put in the session's place meanwhile is
    /// another, and everything held of the first (a sketch, a dialog's draft, the views open) is of nothing.
    project: u64,
    /// How wide the ribbon's Mode panel was last drawn, to stand it at the ribbon's right end.
    mode_width: f32,
    /// Manage > Technical Standards, while open.
    standards_dialog: Option<standards::StandardsDialog>,
    /// Manage > Building Codes, while open.
    codes_dialog: Option<codes::CodesDialog>,
    /// The stair whose findings against the building code are shown in a window.
    code_findings: Option<Id>,
    /// The plugins of the user's folder, the tabs they put on the ribbon, and the Plugins window while it is open.
    pub(crate) plugins: bimcraft_plugin::Plugins,
    pub(crate) plugin_tabs: Vec<bimcraft_plugin::RibbonTab>,
    pub(crate) plugins_dialog: bool,
    /// A report a plugin's function asked the window to show, while it is open.
    pub(crate) plugin_report: Option<plugins::PluginReport>,
    /// A plugin's function whose path the user is choosing in the system's dialog: its command, and the parameters with
    /// the path left to set. Running a function is one click on the ribbon, and a function that needs a file asks for
    /// it there.
    pub(crate) plugin_pick: Option<(String, serde_json::Map<String, serde_json::Value>)>,
    /// The values and the answer of each plugin's panel, by the palette's name (`plugin:<plugin id>:<panel id>`): kept while
    /// the window is open, and never written to the project or the settings file.
    pub(crate) plugin_panel_states: std::collections::BTreeMap<String, crate::plugin_panels::PluginPanelState>,
    /// The form a plugin function's `params` make, by the command's id: its values stay while the window is open.
    pub(crate) plugin_forms: std::collections::BTreeMap<String, plugin_form::PluginForm>,
    /// The command whose form is open in a dialog.
    pub(crate) plugin_form_open: Option<String>,
    /// The folder being read on a thread of its own, so the first frame isn't kept waiting by it.
    plugins_job: Option<std::sync::mpsc::Receiver<bimcraft_plugin::Plugins>>,
    /// Manage > Coordinates, while it is open.
    coordinates_dialog: Option<coordinates::CoordinatesDialog>,
    /// How the Survey Point goes when it's dragged, moved or typed in Properties: clipped (the point alone, the world staying)
    /// or unclipped (the shared coordinates with it), as Revit's clip says.
    pub(crate) survey_moves: bimcraft_core::coordinates::Move,
    options_dialog: Option<phasing::OptionsDialog>,
    purge_dialog: Option<purge::PurgeDialog>,
    pub(crate) manage_links: Option<manage_links::ManageLinks>,
    pub(crate) asset_manager: Option<asset_manager::AssetManager>,
    /// Manage > Select by ID and IDs of Selection, while open.
    select_by_id: Option<inquiry::SelectById>,
    /// Manage > Run Script, while open.
    run_script: Option<script::RunScript>,
    /// The Work Plane dialog's draft, while it's open or picking its points in the 3D view.
    work_plane: Option<workplane::Draft>,
    /// The Work Plane dialog's Show (T12): the 3D view's work plane drawn as a grid whether or not it is being set.
    pub(crate) show_work_plane: bool,
    ids_of_selection: Option<inquiry::IdsOfSelection>,
    selection_filter: Option<filter::SelectionFilter>,
    /// What was last put on the clipboard.
    pub copied: Option<String>,
    /// What Copy to Clipboard last kept, for a paste the system clipboard has nothing of BIMCraft's for.
    pub(crate) clip_text: Option<Arc<str>>,
    /// The clipboard Paste from Clipboard or Aligned to Picked Level places, while its tool is on.
    pub(crate) pasting: Option<clipboard::Pasting>,
    /// What the right-click menu (or a family tab's Find) asked of the clipboard, for the next frame.
    pub(crate) clip_request: Option<clipboard::Request>,
    /// What Copy to Clipboard last kept on a family tab (`editor/clipboard.rs`): the window's, so it outlives views and tabs.
    pub(crate) family_clip: Option<editor::clipboard::Held>,
    /// A paste the ribbon asked the system clipboard's text for, when, and on which tab (a family document's, or none: the
    /// project's).
    pub(crate) paste_asked: Option<(clipboard::PasteMode, f64, Option<editor::tabs::DocId>)>,
    /// Aligned to Selected Levels' or Selected Views' list, while it's open.
    pub(crate) paste_choose: Option<clipboard::Choose>,
    /// Ctrl+V's question in a 3D view, while it's asked: the clipboard it would paste.
    pub(crate) paste_where: Option<clipboard::Pasting>,
    /// Scale's popups: how it scales, the percentage and the length typed.
    pub(crate) scaling: scale::Options,
    active_option: Option<String>,
    new_schedule: Option<properties::NewSchedule>,
    /// The material the Paint tool lays; the first of the library's while it isn't one it has.
    paint_material: String,
    /// The family and type the Component tool places, as picked, and how far it's turned, degrees.
    component: Option<(String, String)>,
    component_angle: f64,
    /// Hinge hand for this run of the Door tool. Facing follows the pointer.
    door_flip_hand: bool,
    /// Face for this run of the Window tool, over the one the pointer's side picks.
    window_flip_facing: bool,
    /// An adaptive component's points clicked so far, in the project, before the last one places it.
    pub(crate) adaptive_clicks: Vec<[f64; 3]>,
    /// An adaptive instance's grip being dragged in a plan: the instance, and its point's number.
    pub(crate) adaptive_grip: Option<(Id, u32)>,
    /// The Component tool's Placement Plane for a work plane-based family: a reference plane of the project, by GUID; none,
    /// the view's level.
    placement_plane: Option<bimcraft_core::ids::Guid>,
    /// The Component tool's Place on Face for a work plane-based family: a click places it on the face under it (in plan a
    /// floor's or a roof's top, in a ceiling plan a ceiling's underside, in the 3D view any face that hosts).
    pub(crate) place_on_face: bool,
    /// The Component tool's Rotate after placement: each component placed is selected and Rotate starts about its point.
    rotate_after: bool,
    /// The Properties palette's folded groups.
    collapsed: BTreeSet<&'static str>,
    /// The names of Properties' groups a project's parameters make, kept for `collapsed`.
    collapse_keys: Vec<&'static str>,
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
    pub updates: Option<bimcraft_update::Updater>,
    /// The Assistant palette's: the user's provider, model and key, and the chat, which the desktop app gives a network (`assistant::State::attach`): a window starts without one.
    pub assistant: assistant::State,
    /// The 3D mouse's reader, which the desktop app attaches: a window starts without one, so a test never opens a device.
    pub space_mouse: Option<spacemouse::SpaceMouse>,
    /// The reader of Revit files, which the desktop app attaches (`bimcraft-rvt` is Apache-2.0 only, so this crate never depends on
    /// it): a window starts without one, so it offers no `.rvt` and a test never reads a real one.
    pub revit: Option<revit::Importer>,
    /// The control port, when the window was started with one or Settings opened it: agents work on this project through it.
    control: Option<control::Control>,
    /// File > Settings > Agents: whether the window opens the port, on which, and whether agents may edit or only read.
    agents: control::Agents,
    /// Co-Work: the session the window is in, what Settings keeps of it, and its dialog while open.
    cowork: Option<cowork::Live>,
    pub(crate) cowork_prefs: cowork::Prefs,
    cowork_dialog: Option<cowork::Dialog>,
    /// What the window keeps of the others in the session and of where it is itself.
    cowork_seen: cowork::Seen,
    /// The session's shared AI chat, as this window takes part in it.
    cowork_ai: cowork::Ai,
    /// A host with guests in that was asked to close: asking whether to end the session (false), or told to (true).
    cowork_quit: Option<bool>,
    /// The user's Python scripts: the folder, the Player's forms, the run going on.
    pub(crate) pyaddins: pyaddins::PyAddins,
    /// The Revit file being read, on a thread of its own.
    revit_job: Option<revit::Job>,
    /// A big project being read on a thread of its own (`opening.rs`).
    opening: Option<opening::Opening>,
    /// Whether the machine has started to swap, and the banner that says so (`swap.rs`).
    swapping: swap::Swapping,
    /// Files to open at the next frame: the command line's, and what the system asked for while a Revit file was being read.
    opens: Vec<PathBuf>,
    /// The file dialog that is open, if one is: it is on a thread of its own, and answers in a later frame.
    dialog: Option<dialog::Dialog>,
    /// What the last Revit import left out and brought in, for as long as its project is the one open.
    revit_report: Option<revit::Report>,
    /// File > Export…, while it is open.
    export_dialog: Option<export::Dialog>,
    /// Print Sets, while it is open.
    print_sets: Option<printsets::Dialog>,
    /// The Publisher, while it is open.
    publisher: Option<publisher::Dialog>,
    /// The name asked for a print set of the views picked in the Project Browser.
    name_set: Option<printsets::NameSet>,
    /// The views and sheets picked in the Project Browser (Ctrl and Shift pick more than one), and the one a Shift-click runs from.
    browser_picked: Vec<browser::Pick>,
    browser_anchor: Option<browser::Pick>,
    /// Expand All or Collapse All, for the next frame's groups.
    browser_fold: Option<bool>,
    /// The export being written, on a thread of its own.
    export_job: Option<export::worker::Running>,
    /// The format the last export was to, and each format's settings, for the next export while the window is open.
    export_format: Option<String>,
    export_last: std::collections::BTreeMap<String, bimcraft_core::export::Options>,
    /// What prints: the system's spooler, or in a test one that only remembers.
    spooler: std::sync::Arc<dyn export::print::Spooler>,
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

/// How a slope is typed and shown: percent (rise over run), degrees, or a grade (1 : the run for a rise of one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SlopeUnit {
    Percent,
    Degrees,
    Grade,
}

impl SlopeUnit {
    pub(crate) const ALL: [SlopeUnit; 3] = [SlopeUnit::Percent, SlopeUnit::Degrees, SlopeUnit::Grade];

    pub(crate) fn label(self) -> &'static str {
        match self {
            SlopeUnit::Percent => msg!("Percent (%)"),
            SlopeUnit::Degrees => msg!("Degrees (°)"),
            SlopeUnit::Grade => msg!("Grade (1:x)"),
        }
    }

    /// A slope of `percent`, as this unit writes it.
    pub(crate) fn show(self, percent: f64) -> String {
        let trim = |x: f64, places: usize| {
            let s = format!("{x:.places$}");
            if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
        };
        match self {
            SlopeUnit::Percent => trim(percent, 2),
            SlopeUnit::Degrees => trim(bimcraft_core::ramps::degrees(percent), 2),
            SlopeUnit::Grade => format!("1:{}", trim(bimcraft_core::ramps::grade(percent), 2)),
        }
    }

    /// What is typed in this unit, as percent: a grade as `1:12`, `1/12` or `12`.
    pub(crate) fn read(self, typed: &str) -> Option<f64> {
        let s = typed.trim().trim_end_matches(['%', '°']).trim();
        let p = match self {
            SlopeUnit::Percent => crate::properties::number(s)?,
            SlopeUnit::Degrees => crate::properties::number(s).filter(|d| *d > 0.0 && *d < 89.0)?.to_radians().tan() * 100.0,
            SlopeUnit::Grade => {
                let run = match s.split_once([':', '/']) {
                    Some((rise, run)) => crate::properties::number(run)? / crate::properties::number(rise).filter(|r| *r > 0.0)?,
                    None => crate::properties::number(s)?,
                };
                if run > 0.0 { 100.0 / run } else { return None }
            }
        };
        (p.is_finite() && p > 0.0).then_some(p)
    }
}

/// How the Ramp tool draws a ramp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RampShape {
    /// Along a line, the stretch to the next click straight.
    Straight,
    /// Along a line, the stretch to the next click an arc leaving the one before smoothly.
    Arc,
    /// Round a centre, as far as it needs to reach its top.
    Circular,
}

/// How the Stair tool draws a stair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StairShape {
    /// Along a line, the stretch to the next click straight.
    Straight,
    /// Along a line, the stretch to the next click an arc leaving the one before smoothly.
    Arc,
    /// About a centre.
    Spiral,
    /// Riser by riser.
    Risers,
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
    let tip = crate::shortcuts::tip_with_shortcuts(label, tip);
    ribbon_button(ui, icon, label, active, &tip, true, None)
}

/// A big ribbon button, or its row in a list, whose icon and name are in `own` rather than the theme's ink: Finish Edit Mode's
/// green and Cancel Edit Mode's red.
fn inked_button(ui: &mut Ui, icon: Icon, label: &str, active: bool, tip: &str, own: Option<Color32>) -> Response {
    let tip = crate::shortcuts::tip_with_shortcuts(label, tip);
    ribbon_button(ui, icon, label, active, &tip, true, own)
}

/// A ribbon button whose label is a plugin's data: it isn't translated, as its manifest wrote it, and it has no icon
/// (icons come later, from a limited set of glyphs in the manifest).
fn plugin_button(ui: &mut Ui, label: &str, tip: &str) -> Response {
    const WIDE: f32 = 78.0;
    if ribbon::left_out(label) {
        return ui.allocate_response(egui::Vec2::ZERO, Sense::hover());
    }
    if ribbon::listing() {
        return plugin_row(ui, label, tip);
    }
    if let Some(r) = ribbon::handed_over(ui) {
        return r;
    }
    let (label, tip) = (label.to_string(), t(tip));
    let font = FontId::proportional(12.0);
    let wide = |s: &str| ui.painter().layout_no_wrap(s.to_string(), font.clone(), pal().text).size().x;
    let one = wide(&label);
    let lines: Vec<String> = match label.char_indices().filter(|(_, c)| *c == ' ').min_by_key(|(i, _)| i.abs_diff(label.len() / 2)) {
        Some((at, _)) if one + 12.0 > WIDE => vec![label[..at].to_string(), label[at + 1..].to_string()],
        _ => vec![label.clone()],
    };
    let width = lines.iter().map(|l| wide(l)).fold(0.0, f32::max) + 12.0;
    let (rect, r) = ui.allocate_exact_size(vec2(width.max(54.0), 60.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, false, &label));
    let (fill, stroke, ink) = match r.hovered() && enabled {
        true => (pal().hover_fill, Stroke::new(1.0, pal().hover), pal().text),
        false => (Color32::TRANSPARENT, Stroke::NONE, if enabled { pal().text } else { pal().muted.gamma_multiply(0.6) }),
    };
    let stroke = if r.has_focus() { style::focus_ring() } else { stroke };
    ui.painter().rect(rect, 6.0, fill, stroke, StrokeKind::Inside);
    let top = rect.center_top() + vec2(0.0, 30.0 - 6.5 * (lines.len().saturating_sub(1)) as f32);
    for (i, line) in lines.iter().enumerate() {
        ui.painter().text(top + vec2(0.0, 13.0 * i as f32), Align2::CENTER_CENTER, line, font.clone(), ink);
    }
    r.on_hover_text(tip)
}

/// A plugin's button in its panel's list: its row is text alone, as its ribbon button has no icon.
fn plugin_row(ui: &mut Ui, label: &str, tip: &str) -> Response {
    let width = if ribbon::in_palette() { ui.available_width() } else { ui.available_width().max(200.0) };
    let (rect, r) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, false, label));
    let (fill, ink) = match r.hovered() && enabled {
        true => (pal().hover_fill, pal().text),
        false => (Color32::TRANSPARENT, if enabled { pal().text } else { pal().muted.gamma_multiply(0.6) }),
    };
    let stroke = if r.has_focus() { style::focus_ring() } else { Stroke::NONE };
    let p = ui.painter();
    p.rect(rect, 4.0, fill, stroke, StrokeKind::Inside);
    p.text(rect.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(12.5), ink);
    r.on_hover_text(t(tip))
}

fn ribbon_button(ui: &mut Ui, icon: Icon, label: &str, active: bool, tip: &str, translate: bool, own: Option<Color32>) -> Response {
    const WIDE: f32 = 78.0;
    if ribbon::left_out(label) {
        return ui.allocate_response(egui::Vec2::ZERO, Sense::hover());
    }
    let name = label;
    if ribbon::listing() {
        return list_row_in(ui, icon, label, active, tip, translate, own);
    }
    if let Some(r) = ribbon::handed_over(ui) {
        return r;
    }
    let (label, tip) = if translate { (t(label).to_string(), t(tip)) } else { (label.to_string(), t(tip)) };
    let font = FontId::proportional(12.0);
    let wide = |s: &str| ui.painter().layout_no_wrap(s.to_string(), font.clone(), pal().text).size().x;
    let one = wide(&label);
    // The space nearest the middle, for a label too wide for one line.
    let lines: Vec<String> = match label.char_indices().filter(|(_, c)| *c == ' ').min_by_key(|(i, _)| i.abs_diff(label.len() / 2)) {
        Some((at, _)) if one + 12.0 > WIDE => vec![label[..at].to_string(), label[at + 1..].to_string()],
        _ => vec![label.to_string()],
    };
    let width = lines.iter().map(|l| wide(l)).fold(0.0, f32::max) + 12.0;
    let (rect, r) = ui.allocate_exact_size(vec2(width.max(54.0), 60.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, active, &label));
    let (fill, stroke, mut ink) = match (active, r.hovered() && enabled) {
        (true, _) => (pal().select_fill, Stroke::new(1.0, pal().accent), pal().accent_ink),
        (false, true) => (pal().hover_fill, Stroke::new(1.0, pal().hover), pal().icon),
        (false, false) => (Color32::TRANSPARENT, Stroke::NONE, if enabled { pal().icon } else { pal().icon.gamma_multiply(0.35) }),
    };
    if let Some(own) = own.filter(|_| enabled) {
        ink = own;
    }
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
    let text = own.filter(|_| enabled).unwrap_or(text);
    for (i, line) in lines.iter().enumerate() {
        p.text(rect.center_top() + vec2(0.0, 40.0 + 13.0 * i as f32), Align2::CENTER_CENTER, line, font.clone(), text);
    }
    if ribbon::editing_tools() {
        return edit_tool(ui, r, rect, name);
    }
    r.on_hover_text(tip)
}

/// A small ribbon button: an icon alone, as in Revit's Modify panel; its name in its tooltip.
pub(crate) fn small_button(ui: &mut Ui, icon: Icon, label: &str, active: bool, tip: &str) -> Response {
    if ribbon::left_out(label) {
        return ui.allocate_response(egui::Vec2::ZERO, Sense::hover());
    }
    let name = label;
    if ribbon::listing() {
        return list_row(ui, icon, label, active, tip);
    }
    if let Some(r) = ribbon::handed_over(ui) {
        return r;
    }
    let (label, tip) = (t(label), t(tip));
    let tip = crate::shortcuts::tip_with_shortcuts(name, tip);
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
    if ribbon::editing_tools() {
        return edit_tool(ui, r, rect, name);
    }
    r.on_hover_text(tip)
}

/// A ribbon button in its panel's list: a row of its icon and its name.
fn list_row(ui: &mut Ui, icon: Icon, label: &str, active: bool, tip: &str) -> Response {
    list_row_in(ui, icon, label, active, tip, true, None)
}

/// [`list_row`], its icon and name in `own` where it has an ink of its own ([`inked_button`]).
fn list_row_in(ui: &mut Ui, icon: Icon, label: &str, active: bool, tip: &str, translate: bool, own: Option<Color32>) -> Response {
    if ribbon::left_out(label) {
        return ui.allocate_response(egui::Vec2::ZERO, Sense::hover());
    }
    let name = label;
    let (label, tip) = if translate { (t(label).to_string(), t(tip)) } else { (label.to_string(), t(tip)) };
    // A panel's list is at least a menu wide; the Tools palette's rows are as wide as its column.
    let width = if ribbon::in_palette() { ui.available_width() } else { ui.available_width().max(200.0) };
    let (rect, r) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, active, &label));
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
    let ink = own.unwrap_or(if active { pal().accent_ink } else { pal().icon });
    icons::paint(p, icon, Rect::from_center_size(rect.left_center() + vec2(18.0, 0.0), vec2(20.0, 20.0)), dim(ink));
    let text = own.unwrap_or(if active { pal().accent_ink } else { pal().text });
    p.text(rect.left_center() + vec2(38.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(13.0), dim(text));
    if ribbon::editing_tools() {
        return edit_tool(ui, r, rect, name);
    }
    r.on_hover_text(tip)
}

/// In Edit Tools a tool's button or row is a box to tick, what the ribbon or the Tools palette shows: a click ticks or clears it and
/// takes no tool. `name` is its English name, which the settings file keeps.
fn edit_tool(ui: &Ui, r: Response, rect: Rect, name: &str) -> Response {
    let shown = !ribbon::tool_hidden(name);
    let label = t(name);
    r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, shown, label));
    let p = ui.painter();
    if !shown {
        p.rect_filled(rect, 6.0, pal().panel.gamma_multiply(0.7));
    }
    let tick = Rect::from_min_size(pos2(rect.right() - 15.0, rect.top() + 3.0), vec2(12.0, 12.0));
    p.rect(tick, 2.0, if shown { pal().accent } else { pal().surface }, Stroke::new(1.0, pal().accent_ink), StrokeKind::Inside);
    if shown {
        let ends = vec![tick.left_center() + vec2(2.5, 0.0), tick.center_bottom() - vec2(1.0, 3.0), tick.right_top() + vec2(-2.5, 3.0)];
        p.line(ends, Stroke::new(1.5, Color32::WHITE));
    }
    if r.clicked() {
        ribbon::toggle(name);
    }
    ui.interact(rect, r.id.with("edit-tool"), Sense::hover()).on_hover_text(label)
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
    if ribbon::left_out(label) {
        return ui.allocate_response(egui::Vec2::ZERO, Sense::hover());
    }
    let name = label;
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
    if ribbon::editing_tools() {
        return edit_tool(ui, r, rect, name);
    }
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

/// The app bar's AI button: lit while it is `on`, a checkbox to a screen reader, what a click turns on or off.
fn toggle_button(ui: &mut Ui, icon: Icon, label: &str, on: bool, tip: &str) -> Response {
    let (rect, r) = ui.allocate_exact_size(vec2(26.0, 24.0), Sense::click());
    r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, label));
    let (ink, hover) = style::icon_inks();
    if on {
        ui.painter().rect_filled(rect, 6.0, pal().accent);
    } else if r.hovered() {
        ui.painter().rect_filled(rect, 6.0, hover);
    }
    if r.has_focus() {
        ui.painter().rect_stroke(rect, 6.0, style::focus_ring(), StrokeKind::Inside);
    }
    icons::paint(ui.painter(), icon, Rect::from_center_size(rect.center(), vec2(18.0, 18.0)), if on { Color32::WHITE } else { ink });
    r.on_hover_text(tip)
}

/// An undo step as Undo's list names it: what it made (Create Walls) or took away (Delete Door), or else its command.
fn step_name(st: &engine::Step) -> String {
    named(&st.label, &st.made, &st.removed)
}

/// A step named by its label and what it made and took away: Create Walls, Delete Door, or its command.
fn named<C: AsRef<str>>(label: &str, made: &[(C, usize)], removed: &[(C, usize)]) -> String {
    let one = |list: &[(C, usize)]| match list {
        [(c, n)] => Some(lang::category(c.as_ref(), *n > 1)),
        _ => None,
    };
    if let Some(asked) = assistant::step_label(label) {
        return asked;
    }
    let named = match (made, removed) {
        (made, []) if !made.is_empty() => one(made).map(|c| tf("Create {}", &[&c])),
        ([], removed) if !removed.is_empty() => one(removed).map(|c| tf("Delete {}", &[&c])),
        _ => None,
    };
    named.unwrap_or_else(|| t(label).to_string())
}

/// A tab, of the ribbon or of a view. Contextual tabs are green, as Revit's are.
fn tab_button(ui: &mut Ui, label: &str, active: bool, contextual: bool) -> Response {
    tab_button_with(ui, label, active, contextual, Sense::click(), (0.0, 0.0))
}

/// A tab that senses what `sense` says (a view's drags too), `room` px wider on its right for what's drawn in it (a view's ×).
/// `room`: points kept clear left of the label (an icon's) and right of it (a close button's).
fn tab_button_with(ui: &mut Ui, label: &str, active: bool, contextual: bool, sense: Sense, (lead, room): (f32, f32)) -> Response {
    let label = t(label);
    let font = if active { style::semibold(13.0) } else { FontId::proportional(13.0) };
    let size = ui.painter().layout_no_wrap(label.to_string(), font.clone(), pal().text).size();
    let (rect, r) = ui.allocate_exact_size(vec2(size.x + 24.0 + lead + room, 24.0), sense);
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
    p.text(rect.center() + vec2((lead - room) / 2.0, 0.0), Align2::CENTER_CENTER, label, font, ink);
    r
}

/// How tall a card floating over a view is (its tabs, its view control bar), points.
const FLOATING_CARD: f32 = 30.0;

/// The ribbon's tabs, in order, with the icon the Tools palette's rail gives each (the contextual tab comes after them).
const TABS: [(Tab, &str, Icon); 14] = [
    (Tab::Architecture, msg!("Architecture"), Icon::Wall),
    (Tab::Structure, msg!("Structure"), Icon::Beam),
    (Tab::Plumbing, msg!("Plumbing"), Icon::Pipe),
    (Tab::Electrical, msg!("Electrical"), Icon::Luminaire),
    (Tab::Lightning, msg!("Lightning Protection"), Icon::DownConductor),
    (Tab::Data, msg!("Data"), Icon::DataOutlet),
    (Tab::Insert, msg!("Insert"), Icon::ImportCad),
    (Tab::Annotate, msg!("Annotate"), Icon::Dimension),
    (Tab::Massing, msg!("Massing"), Icon::Mass),
    (Tab::Collaborate, msg!("Collaborate"), Icon::Cowork),
    (Tab::View, msg!("View"), Icon::View3D),
    (Tab::Manage, msg!("Manage"), Icon::Settings),
    (Tab::PyAddins, "PyAddins", Icon::Script),
    (Tab::Modify, "Modify", Icon::Modify),
];

/// The panels each built-in tab draws, for the preset editor: a panel's key is `tab:<tab>/<panel>` (`ТЗ-019`). `ribbon::panel`
/// keeps what it draws, and these make the list complete before a tab is opened. A panel a tool, a script or a plugin adds still
/// comes from the drawing; `Tab::Modify` lists the panels it always draws, the tool in hand adding the rest.
const PANELS: [(Tab, &[&str]); 14] = [
    (Tab::Architecture, &["Select", "Build", "Circulation", "Room & Area", "Opening", "Datum"]),
    (Tab::Structure, &["Select", "Structure", "Reinforcement", "Quantities", "Foundation"]),
    (Tab::Plumbing, &["Select", "Plumbing & Piping"]),
    (Tab::Electrical, &["Select", "Electrical", "Symbols", "Conduit & Wire"]),
    (Tab::Lightning, &["Select", "Capture & Descent", "Earthing"]),
    (Tab::Data, &["Select", "Outlets & Equipment", "Cabling"]),
    (Tab::Insert, &["Link", "Import"]),
    (Tab::Annotate, &["Select", "Dimension", "Detail", "Region", "Text", "Measure", "Tag"]),
    (Tab::Massing, &["Conceptual Mass", "Model by Face"]),
    (Tab::Collaborate, &["Work Together"]),
    (Tab::View, &["Graphics", "Create", "Sheet Composition", "Windows"]),
    (Tab::Manage, &["Settings", "Inquiry", "Phasing", "Design Options", "Macros"]),
    (Tab::PyAddins, &["Scripts", "Run"]),
    (Tab::Modify, &["Select", "Properties", "Clipboard"]),
];

/// A tab on the Tools palette's rail: its icon, lit while it is open; its name for screen readers and in its tooltip.
fn rail_button(ui: &mut Ui, icon: Icon, label: &str, open: bool) -> Response {
    let label = t(label);
    let (rect, r) = ui.allocate_exact_size(vec2(32.0, 30.0), Sense::click());
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, open, label));
    let (fill, ink) = match (open, r.hovered()) {
        (true, _) => (pal().select_fill, pal().accent_ink),
        (false, true) => (pal().hover_fill, pal().icon),
        (false, false) => (Color32::TRANSPARENT, pal().icon),
    };
    let p = ui.painter();
    p.rect_filled(rect, 6.0, fill);
    if r.has_focus() {
        p.rect_stroke(rect, 6.0, style::focus_ring(), StrokeKind::Inside);
    }
    icons::paint(p, icon, Rect::from_center_size(rect.center(), vec2(19.0, 19.0)), ink);
    r.on_hover_text(label)
}

/// The ribbon's name for the Wall tool drawing a curtain wall type.
const CURTAIN_TOOL: &str = msg!("Curtain Wall");

impl BimApp {
    pub fn new(session: Session) -> BimApp {
        let first = Sheet::Plan(session.model.levels().first().map_or(0, |(id, _)| *id));
        let units = session.model.units;
        let app = BimApp {
            saved: Arc::clone(&session.model),
            project: session.model.uid,
            autosave: autosave::State::default(),
            session,
            os_events: None,
            own_title_bar: false,
            system_title_bar: false,
            classification: None,
            sheet: first,
            tool: Tool::Modify,
            selection: Vec::new(),
            status: Status::default(),
            views: vec![first],
            front: first,
            floating: Vec::new(),
            arrange: dock::Arrange::Tabs,
            views_rect: Rect::NOTHING,
            placements: Vec::new(),
            tab_pane: Rect::NOTHING,
            passive: false,
            docking: dock::Docking::default(),
            dragging: None,
            drop_headers: Vec::new(),
            tab_bar: Rect::NOTHING,
            tabs_foot: f32::NEG_INFINITY,
            tab_rects: Vec::new(),
            work: Rect::ZERO,
            pointer: Pos2::ZERO,
            grab: egui::Vec2::ZERO,
            ribbon_min: false,
            compact: false,
            profile: style::Profile::default(),
            scheme: style::Scheme::default(),
            accent: None,
            panel_colour: None,
            canvas_colour: None,
            text_colour: None,
            palette_filter: None,
            palette_json: RefCell::new(None),
            ribbon_peek: false,
            scan_contrast: footer::HIGH_CONTRAST,
            background_grid_dialog: None,
            underlay: Default::default(),
            underlay_dialog: None,
            underlay_edit: None,
            underlay_color: None,
            underlay_error: None,
            grid_snapping: false,
            guide_style: style::GuideStyle::default(),
            compressed_save: false,
            cut_fills_3d: true,
            select_by_face: true,
            drag_on_selection: false,
            show_origin: true,
            show_view_name: true,
            ribbon_bottom: Pos2::ZERO,
            settings_page: None,
            find: None,
            scale: presets::Scale::default(),
            tools_hidden: BTreeSet::new(),
            panels_hidden: BTreeSet::new(),
            panel_keys: BTreeSet::new(),
            editing_tools: false,
            presets: Vec::new(),
            active_preset: None,
            preset_editor: None,
            preset_name: String::new(),
            preset_note: None,
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
            pick_core: true,
            pick_offset: String::new(),
            pick_flip: false,
            outline_side: crate::sketch::OutlineSide::Interior,
            outline_core: true,
            outline_offset: String::new(),
            outline_flip: false,
            outline_pending: false,
            outline_walls: BTreeSet::new(),
            sketch_line_chain: None,
            run_justify: Justify::Top,
            beam_offset: units.length(0.0),
            run_kept: BTreeMap::new(),
            run_loaded: "beam",
            footing_thickness: units.length(400.0),
            footing_top: units.length(0.0),
            stair_width: units.length(STAIR_WIDTH),
            stair_winders: 0,
            stair_shape: StairShape::Straight,
            stair_landing: false,
            stair_kinds: Vec::new(),
            stair_clockwise: false,
            stair_top: None,
            stair_height: units.length(bimcraft_core::model::DEFAULT_HEIGHT),
            stair_multistorey: false,
            stair_lock_going: true,
            stair_from_top: false,
            stair_riser: String::new(),
            stair_tread: String::new(),
            stair_offer: std::cell::Cell::new(None),
            stair_line: Default::default(),
            stair_offset: units.length(0.0),
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
            ramp_width: units.length(bimcraft_core::codes::library_rules::<bimcraft_core::codes::checks::RampRules>().width),
            ramp_slope: "8.33".into(),
            ramp_shape: RampShape::Straight,
            ramp_landing: false,
            ramp_kinds: Vec::new(),
            ramp_clockwise: false,
            ramp_line: Default::default(),
            slope_unit: SlopeUnit::Percent,
            ceiling_height: units.length(CEILING_HEIGHT),
            floor_slope: "5".into(),
            shape_z: String::new(),
            shape_base: crate::shape::Base::default(),
            shape_along: true,
            shape_offset: "0".into(),
            shape_grip: None,
            shape_preview: None,
            shape_label_edit: None,
            shape_picked: Vec::new(),
            shape_label_rects: Vec::new(),
            shaft_top: None,
            shaft_height: units.length(3000.0),
            mass_top: None,
            mass_height: units.length(3000.0),
            camera_eye_height: units.length(1600.0),
            camera_target_height: units.length(1200.0),
            camera_lens: "35".into(),
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
            note_leader: false,
            note_leader_kind: LeaderKind::Straight,
            note_arrow: None,
            note_edit: None,
            text_type_dialog: None,
            dimension_type_dialog: None,
            spot_type_dialog: None,
            spot_grip: None,
            image_grip: None,
            pdf_page_box: None,
            curtain_editor: None,
            go_to_view: None,
            curtain_part: None,
            curtain_more: Vec::new(),
            curtain_hover: None,
            curtain_aim: None,
            curtain_segment_hover: None,
            curtain_laid: Default::default(),
            curtain_seen: None,
            curtain_known: Default::default(),
            curtain_asking: Default::default(),
            grid_type_dialog: None,
            dimension_text_dialog: None,
            line_style: String::new(),
            linework_style: String::new(),
            group_type: String::new(),
            model_group_type: String::new(),
            group_flexible: false,
            group_split: None,
            group_pick: None,
            group_conflict: None,
            repeat_spacing: "200".into(),
            line_chain: true,
            drawing: draw::Drawing::default(),
            units,
            lang: Lang::English,
            font_finder: lang::cjk_fonts,
            typed_length: String::new(),
            dim_edit: None,
            over_dimension_control: false,
            dim_edit_escaped: false,
            dimension_refs_edit: None,
            dimension_segment: None,
            dimension_segment_hover: None,
            dimension_reference_hover: None,
            temporary: HashMap::new(),
            shadow: None,
            linked_3d: None,
            seen: None,
            link_dialog: false,
            cloud_dialog: false,
            cloud_box: None,
            cad_dialog: None,
            cad_box: None,
            link_box: None,
            tab_cycle: false,
            face_cycle: false,
            held: Vec::new(),
            show_shortcuts: false,
            shortcuts: shortcuts::Shortcuts::default(),
            shortcuts_dialog: shortcuts::ShortcutsDialog::default(),
            shortcut_presets: Vec::new(),
            active_shortcut_preset: None,
            angle_edit: None,
            snaps: Snaps::default(),
            show_snaps: false,
            snap_once: None,
            measured: None,
            open_units: false,
            options_rect: Rect::NOTHING,
            canvas: canvas::Canvas::default(),
            bench: None,
            frames: None,
            orbit: orbit::Orbit::default(),
            active_3d: None,
            card: gpu::Card::default(),
            cut: section::Cut::default(),
            minimap: minimap::Minimap::default(),
            paper: paper::Papers::default(),
            edit: None,
            new_level: (String::new(), String::new()),
            typed: String::new(),
            typed_at: 0.0,
            pending: None,
            deleting_rooms: None,
            type_dialog: None,
            materials_dialog: None,
            family_dialog: None,
            favorites_dialog: None,
            families: Vec::new(),
            next_doc: 1,
            doc_closing: None,
            after_documents: None,
            new_family_dialog: None,
            family_from_home: false,
            family_loading: None,
            family_decided: None,
            favorites_path: None,
            recent: recent::Recent::default(),
            recent_path: None,
            templates: project_templates::Templates::default(),
            templates_path: None,
            new_project_popup: false,
            template_picked: None,
            home: start::Home::default(),
            paint_material: String::new(),
            parameter_manager: None,
            schedule_dialog: None,
            vg_dialog: None,
            filters_dialog: None,
            templates_dialog: None,
            phasing_dialog: None,
            display_dialog: None,
            view_range_dialog: None,
            halftone_dialog: None,
            project_info_dialog: None,
            reinforce_dialog: None,
            steel_dialog: None,
            detail_dialog: None,
            reinforce_kept: BTreeMap::new(),
            panel_slot: None,
            line_weights_dialog: None,
            object_styles_dialog: None,
            line_styles_dialog: None,
            title_blocks_dialog: None,
            fill_patterns_dialog: None,
            sketch: None,
            mode_width: 0.0,
            standards_dialog: None,
            codes_dialog: None,
            code_findings: None,
            plugins: bimcraft_plugin::Plugins::empty(),
            plugin_tabs: Vec::new(),
            plugins_dialog: false,
            plugin_report: None,
            plugin_pick: None,
            plugin_panel_states: std::collections::BTreeMap::new(),
            plugin_forms: std::collections::BTreeMap::new(),
            plugin_form_open: None,
            plugins_job: None,
            coordinates_dialog: None,
            survey_moves: bimcraft_core::coordinates::Move::Point,
            options_dialog: None,
            purge_dialog: None,
            manage_links: None,
            asset_manager: None,
            select_by_id: None,
            run_script: None,
            work_plane: None,
            show_work_plane: false,
            ids_of_selection: None,
            selection_filter: None,
            copied: None,
            clip_text: None,
            pasting: None,
            paste_asked: None,
            clip_request: None,
            family_clip: None,
            paste_choose: None,
            paste_where: None,
            scaling: scale::Options { percent: "100".into(), ..Default::default() },
            active_option: None,
            new_schedule: None,
            component: None,
            component_angle: 0.0,
            door_flip_hand: false,
            window_flip_facing: false,
            adaptive_clicks: Vec::new(),
            adaptive_grip: None,
            placement_plane: None,
            place_on_face: false,
            rotate_after: false,
            collapsed: BTreeSet::new(),
            collapse_keys: Vec::new(),
            view_rect: Rect::ZERO,
            pane: Rect::ZERO,
            browser_tab: browser::Tab::All,
            browser_find: String::new(),
            closing: false,
            title: String::new(),
            styled: false,
            updates: None,
            assistant: assistant::State::default(),
            space_mouse: None,
            revit: None,
            control: None,
            agents: control::Agents::default(),
            cowork: None,
            cowork_prefs: cowork::Prefs::default(),
            cowork_dialog: None,
            cowork_seen: cowork::Seen::default(),
            cowork_ai: cowork::Ai::default(),
            cowork_quit: None,
            pyaddins: pyaddins::PyAddins::default(),
            revit_job: None,
            opening: None,
            swapping: swap::Swapping::default(),
            export_dialog: None,
            print_sets: None,
            publisher: None,
            name_set: None,
            browser_picked: Vec::new(),
            browser_anchor: None,
            browser_fold: None,
            export_job: None,
            export_format: None,
            export_last: Default::default(),
            spooler: std::sync::Arc::new(export::print::System),
            opens: Vec::new(),
            dialog: None,
            revit_report: None,
        };
        shortcuts::publish(&app.shortcuts);
        // A new window has no plugin palettes until its plugins are read: the registry is the thread's, and a test may
        // have left one there.
        crate::dock::set_plugin_palettes(&[]);
        app
    }

    /// Whether the model changed since it was last opened or saved.
    pub fn dirty(&self) -> bool {
        !Arc::ptr_eq(&self.saved, &self.session.model)
    }

    /// Where model point `p` shows on screen, on the plan drawn last.
    pub fn screen(&self, p: P2) -> Pos2 {
        self.canvas.screen(p)
    }

    /// What comes from outside the window, done once a pass: agents' requests, the Co-Work session's, the assistant's replies.
    /// `logic` calls it, which runs while the window is minimised too, so a host keeps answering its guests; `show` calls it as
    /// well, for the tests that drive `show` alone (in the window it finds nothing left by then).
    pub(crate) fn pump(&mut self, ctx: &Context) {
        // What agents asked of the project comes before the tidying, which puts right what a command left stale.
        self.answer_agents(ctx);
        self.poll_pyaddins(ctx);
        self.pump_cowork(ctx);
        // A project put in the session's place under the window (the control port turns an agent's New and Open away, but a
        // script or an embedder may do them, and joining a Co-Work session puts the host's there) starts it over, as File > New
        // does: its sketch and dialogs were of the old one, and it gets a recovery copy of its own. (The old one's stays until
        // that is written: nobody was asked about its changes.)
        if self.session.model.uid != self.project {
            self.reopened();
            self.autosave_document();
        }
        // After that check: a reply that came as the project was replaced is not for the new one (`State::project_replaced`).
        self.poll_assistant(ctx);
    }

    /// Run a command; when it fails, say why.
    fn run(&mut self, id: &str, params: Value) -> Option<Value> {
        match self.exec(id, &params) {
            Ok(v) => {
                self.status = Status::default();
                Some(v)
            }
            // A group conflict is asked about in its own dialog (`group_conflict_window`), not said again in the corner.
            Err(e) if self.group_conflict.is_some() && bimcraft_core::groups::is_conflict(&e) => None,
            Err(e) => {
                self.status = Status { text: e, error: true };
                None
            }
        }
    }

    /// Run a command on the project, in the view's phase and option: the one way the interface changes it, so that a Co-Work
    /// guest's commands can go to the host (COWORK-PLAN.md). A test reads the sources for any other.
    pub(crate) fn exec(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        self.made_in();
        let result = match &mut self.cowork {
            Some(cowork::Live::Guest(joined)) => joined.exec(&mut self.session, id, params),
            _ => self.session.execute(id, params),
        };
        if let (Err(e), true) = (&result, matches!(id, "undo" | "redo")) {
            self.light_refused(e);
        }
        // A mark names what stood when it was made: undo or redo can take that away or move it.
        if matches!(id, "undo" | "redo") && result.is_ok() {
            self.canvas.guides_reset();
        }
        // A change that would break a group from inside: the window asks what to do (Ungroup, Create New Type, Cancel).
        if let Err(e) = &result {
            self.conflict_refused(id, params, e);
        }
        // The core renames and drops the option with its set.
        self.active_option = self.session.option.clone();
        result
    }

    /// Several commands as one undo step named `label`, all or nothing: what each gave, or the error.
    pub(crate) fn exec_batch(&mut self, label: &str, commands: &[(&str, Value)]) -> Result<Vec<Value>, String> {
        self.made_in();
        let result = match &mut self.cowork {
            Some(cowork::Live::Guest(joined)) => joined.batch(&mut self.session, label, commands),
            _ => self.session.transaction(label, |s| commands.iter().map(|(c, p)| s.execute(c, p)).collect()),
        };
        self.active_option = self.session.option.clone();
        result
    }

    /// A script as one undo step, `Script`: on a guest's window the host runs it.
    pub(crate) fn exec_script(&mut self, text: &str) -> Result<Vec<Value>, String> {
        self.made_in();
        let result = match &mut self.cowork {
            Some(cowork::Live::Guest(joined)) => joined.script(&mut self.session, text),
            _ => self.session.script_atomic(text),
        };
        self.active_option = self.session.option.clone();
        result
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
            | Tool::RotateNorth { .. }
            | Tool::Scale(_)
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
            | Tool::ModifySubElements
            | Tool::AddPoint
            | Tool::AddSplitLine(_)
            | Tool::SketchLine(_)
            | Tool::SketchPick
            | Tool::PatternCorner(_)
            | Tool::SpecifyPoint
            | Tool::ExportRegion(_)
            | Tool::Paste
            | Tool::PickLevel
            | Tool::Acquire => None,
            Tool::CurtainGrid { .. } => Some(msg!("Place Curtain Grid")),
            Tool::CurtainSegment { .. } => Some(msg!("Add/Remove Segments")),
            Tool::Wall(_) => Some(msg!("Place Wall")),
            Tool::Paint => Some("Paint"),
            Tool::Unpaint => Some("Remove Paint"),
            Tool::Door => Some(msg!("Place Door")),
            Tool::Window => Some(msg!("Place Window")),
            Tool::Room => Some(msg!("Place Room")),
            Tool::RoomSeparator(_) => Some(msg!("Place Room Separation")),
            Tool::Level => Some(msg!("Place Level")),
            Tool::Grid(_) => Some(msg!("Place Grid")),
            Tool::ReferencePlane(_) => Some(msg!("Place Reference Plane")),
            Tool::Dimension(..) | Tool::Angular(_) | Tool::Round(..) => Some(msg!("Place Dimensions")),
            Tool::Text => Some(msg!("Place Text")),
            Tool::DetailGroup => Some(msg!("Place Detail Group")),
            Tool::ModelGroup => Some(msg!("Place Model Group")),
            Tool::RepeatingDetail(_) => Some(msg!("Place Repeating Detail")),
            Tool::DetailLine(_) if self.drawing.insulation.is_some() => Some(msg!("Place Insulation")),
            Tool::DetailLine(_) if self.drawing.cloud.is_some() => Some(msg!("Place Revision Cloud")),
            Tool::DetailLine(_) => Some(msg!("Place Detail Lines")),
            Tool::Spot => Some("Spot Elevation"),
            Tool::SpotSlope => Some(msg!("Spot Slope")),
            Tool::SpotCoordinate(_) => Some("Spot Coordinate"),
            Tool::Measure(_) => Some("Measure"),
            Tool::Tag => Some("Tag"),
            Tool::Section(_) => Some("Section"),
            Tool::DetailView(_) => Some(msg!("Callout")),
            Tool::Elevation => Some("Elevation"),
            Tool::Camera(_) => Some("Camera"),
            Tool::Column => Some(msg!("Place Column")),
            Tool::Run(..) | Tool::Stand(_) => structure::title(&self.tool),
            Tool::Component => Some(msg!("Place Component")),
            Tool::Fixture(key) => mep::fixture(key).map(|f| f.title),
            Tool::Stair(_) => Some(msg!("Create Stair")),
            Tool::Ramp(_) => Some(msg!("Create Ramp")),
            Tool::Railing(_) => Some(msg!("Create Railing Path")),
            Tool::Roof => Some(msg!("Create Roof Footprint")),
            Tool::ExtrusionRoof(_) => Some(msg!("Create Extrusion Roof")),
            Tool::Dormer => Some("Dormer"),
            Tool::Floor => Some(msg!("Create Floor")),
            Tool::Ceiling => Some(msg!("Place Ceiling")),
            Tool::Shaft(_) => Some(msg!("Create Shaft Opening")),
            Tool::Mass(_) => Some(msg!("Create Mass")),
        };
        if let Some(what) = place {
            return Some(format!("{} | {}", t("Modify"), t(what)));
        }
        let m = &self.session.model;
        // A component goes by its family's category, as Revit's Modify | Furniture.
        let categories: BTreeSet<String> = self
            .selection
            .iter()
            .filter_map(|id| m.elements.get(id))
            .map(|e| {
                match e {
                    Element::Component(c) => m.families.get(&c.family).map(|f| editor::category_label(f.category)),
                    _ => None,
                }
                .unwrap_or_else(|| lang::category(e.category(), true))
            })
            .collect();
        match categories.len() {
            0 => None,
            1 => categories.first().map(|c| format!("{} | {}", t("Modify"), c)),
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
            Sheet::View3D => {
                self.active_3d().and_then(|id| self.session.model.view_name(id).map(String::from)).unwrap_or_else(|| "{3D}".into())
            }
            Sheet::Section(id) => self.session.model.view_name(id).map_or_else(|| "Section".into(), String::from),
            Sheet::Paper(id) => self.session.model.sheet(id).map_or_else(|| "Sheet".into(), |s| format!("{} - {}", s.number, s.name)),
            Sheet::Schedule(cat) => match schedule::standard(&self.session.model, cat) {
                (Some(_), sc) => sc.name,
                (None, _) => t(SCHEDULES.iter().find(|(c, _)| *c == cat).map_or(cat, |(_, name)| name)).to_string(),
            },
            Sheet::Table(id) => match self.session.model.elements.get(id) {
                Some(Element::Schedule(sc)) => sc.name.clone(),
                Some(Element::PanelSchedule(ps)) => ps.name.clone(),
                Some(Element::RebarDetail(rd)) => rd.name.clone(),
                _ => "Schedule".into(),
            },
            Sheet::Family(..) => self.family_tab_name(sheet),
        }
    }

    /// The view as the title bar names it: `Floor Plan: Level 1`.
    fn view_title(&self, sheet: Sheet) -> String {
        let kind = match sheet {
            Sheet::Plan(id) if matches!(self.session.model.elements.get(id), Some(Element::DetailView(_))) => msg!("Callout"),
            Sheet::Plan(_) => "Floor Plan",
            Sheet::Ceiling(_) => "Ceiling Plan",
            Sheet::View3D => "3D View",
            Sheet::Section(id) if matches!(self.session.model.elements.get(id), Some(Element::Elevation(_))) => "Elevation",
            Sheet::Section(id) if matches!(self.session.model.elements.get(id), Some(Element::DetailView(_))) => msg!("Callout"),
            Sheet::Section(_) => "Section",
            Sheet::Paper(_) => "Sheet",
            Sheet::Table(id) if matches!(self.session.model.elements.get(id), Some(Element::PanelSchedule(_))) => "Panel Schedule",
            Sheet::Table(id) if matches!(self.session.model.elements.get(id), Some(Element::RebarDetail(_))) => "Reinforcement Detail",
            Sheet::Schedule(_) | Sheet::Table(_) => "Schedule",
            Sheet::Family(id, view) => {
                return self.family_of(id).map(|ed| format!("{} - {}", ed.title(), editor::views::view_title(view))).unwrap_or_default();
            }
        };
        format!("{}: {}", t(kind), self.view_name(sheet))
    }

    /// The project file's name without its extension, as the title bars show it: `House`. A project never saved has none.
    pub(crate) fn project_file_stem(&self) -> Option<String> {
        let name = self.session.path.as_deref()?.file_stem()?;
        Some(name.to_string_lossy().into_owned())
    }

    /// The whole window, as the system's title bar and the task switcher say it.
    pub(crate) fn window_title(&self, view: &str) -> String {
        let file = self.project_file_stem().map(|f| format!("{f} · ")).unwrap_or_default();
        let star = if self.dirty() { "*" } else { "" };
        format!("{file}{}{star} · {view} · {}BIMCraft", self.session.model.name, self.cowork_title())
    }

    /// What the window's own title bar says: the file, the project and the view, or the project and the view when the file's name
    /// would take the room of either.
    pub(crate) fn bar_title(&self, view: &str, room: f32, ctx: &Context) -> String {
        let with_file = match self.project_file_stem() {
            Some(file) => format!("{file} · {} · {view}", self.session.model.name),
            None => return format!("{} · {view}", self.session.model.name),
        };
        let width = ctx.fonts_mut(|f| f.layout_no_wrap(with_file.clone(), FontId::proportional(12.5), Color32::WHITE)).size().x;
        if width <= room { with_file } else { format!("{} · {view}", self.session.model.name) }
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
        let look = style::Look {
            light_canvas: self.light_canvas,
            compact: self.compact,
            profile: self.profile,
            scheme: self.scheme,
            accent: self.accent,
            panel_colour: self.panel_colour,
            canvas_colour: self.canvas_colour,
            text_colour: self.text_colour,
        };
        style::follow(&ctx, look);
        ribbon::tool_rules(self.tools_hidden.clone(), self.scale, self.editing_tools);
        ribbon::panel_rules(self.panels_hidden.clone());
        // The fonts with the hanzi and the Hangul go behind the window's own whatever language it speaks: content — a floor's
        // name from a Korean file — is not the interface's. Chinese needs its glyphs, which the system may not have: then the
        // window stays in English and says why.
        let hanzi = lang::use_cjk_font(&ctx, self.font_finder);
        if self.lang.is_chinese() && !hanzi {
            self.lang = Lang::English;
            self.status = updates::failed(t(lang::NO_CHINESE_FONT).to_string());
        }
        lang::set(self.lang);
        self.pump(&ctx);
        self.tidy();
        self.poll_updates(&ctx);
        self.poll_revit(&ctx);
        self.poll_opening(&ctx);
        self.poll_swap(&ctx);
        self.poll_export(&ctx);
        self.poll_plugins(&ctx);
        self.poll_autosave(&ctx);
        self.fly_frame(&ctx);
        self.keys(&ctx);
        self.family_keys(&ctx);
        self.os_requests(&ctx);
        self.dialog_answered();
        self.watch_window(&ctx);
        let view = self.view_title(self.sheet);
        let title = match self.family_title() {
            Some(family) => format!("{family} · BIMCraft"),
            None => self.window_title(&view),
        };
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        let documents = self.families.iter().any(|d| d.ed.as_ref().is_some_and(editor::FamilyEditor::unsaved));
        // A host whose guests are in is asked first: closing ends the session for them.
        let others_in = matches!(&self.cowork, Some(cowork::Live::Host(h)) if !h.people().is_empty());
        let held = others_in && self.cowork_quit != Some(true);
        if ctx.input(|i| i.viewport().close_requested()) && !self.closing && held {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.cowork_quit = Some(false);
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.closing && !held && (self.installing() || self.dirty() || documents) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            // An update being put in place holds the window: it closes it when it's in. The family documents are asked about
            // first.
            if !self.installing() {
                if documents {
                    self.pending_after_documents(Pending::Close);
                } else {
                    self.pending = Some(Pending::Close);
                }
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && !held && !self.dirty() && !self.installing() && !documents {
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
            self.find_window(&ctx);
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
        // The ribbon is Rev's: BIMCraft's tools are a palette (`Palette::Tools`), which Rev never shows. A family document (an
        // adaptive family open) has a ribbon, palettes and a view of its own, as Revit's family editor, in either profile.
        let family = self.family_doc();
        if style::rev() || family {
            let card = style::card([true, true, true, false]);
            Panel::top("ribbon")
                .frame(card)
                .show_separator_line(false)
                .show(ui, |ui| if family { self.family_ribbon(ui) } else { self.ribbon(ui) });
        }
        if self.has_update_banner() {
            Panel::top("update").frame(Frame::NONE).show_separator_line(false).show(ui, |ui| self.update_banner(ui));
        }
        if self.has_swap_banner() {
            Panel::top("swap").frame(Frame::NONE).show_separator_line(false).show(ui, |ui| self.swap_banner(ui));
        }
        let status = style::card([true, true, false, true]).inner_margin(Margin::symmetric(10, 2));
        Panel::bottom("status").frame(status).show_separator_line(false).show(ui, |ui| self.status_bar(ui));
        self.docks(ui);
        CentralPanel::default().frame(Frame::NONE).show(ui, |ui| {
            if family {
                self.family_central(ui);
                return;
            }
            let g = style::gap().max(4);
            let front = self.front;
            // A schedule's table keeps its rows clear of the tabs.
            let table = matches!(self.sheet, Sheet::Schedule(_) | Sheet::Table(_));
            let arranged = self.arrange != dock::Arrange::Tabs;
            if arranged {
                // Every part is a window of its own with a header: the snapped columns and rows beside, and the tabbed pane in
                // the middle whose header is the tabs. They are all drawn below, over the views' area.
                self.tabs_foot = f32::NEG_INFINITY;
                self.views_rect = ui.available_rect_before_wrap();
                self.arrange_views();
            } else if style::compact() || table {
                Panel::top("view-tabs")
                    .frame(Frame::new().inner_margin(Margin { left: g, right: g, top: g, bottom: 4 }))
                    .show_separator_line(false)
                    .show(ui, |ui| {
                        self.view_tabs(ui);
                    });
                self.tabs_foot = f32::NEG_INFINITY;
                // The views' area is what the tab bar leaves; columns and cells are counted inside it.
                self.views_rect = ui.available_rect_before_wrap();
                self.arrange_views();
                self.view_pane(ui, front);
            } else {
                // A drawn view fills the pane, and its tabs float over its top left corner on a card of their own: drawn after
                // it, so they are over it for the hand too.
                let pane = ui.available_rect_before_wrap();
                self.views_rect = pane;
                ui.scope_builder(UiBuilder::new().max_rect(pane), |ui| self.view_pane(ui, front));
                let card = style::card([true, false, true, false]).inner_margin(Margin::same(2));
                // A child that takes no room of the pane's: over the view, not after it.
                let mut over = ui.new_child(UiBuilder::new().id_salt("view-tabs").max_rect(pane));
                self.tabs_foot = card.show(&mut over, |ui| self.view_tabs(ui)).response.rect.bottom();
                self.arrange_views();
            }
            // The palettes dock round the active view: its cell, column or row, or the pane its tab shows.
            self.work = if arranged { self.active_rect() } else { self.pane.with_min_y(self.pane.top().max(self.tabs_foot)) };
            if arranged {
                // The Options Bar and the navigation bar follow the active cell, not the pane behind it.
                self.view_rect = self.work;
            }
        });
        self.options_bar(&ctx);
        self.edge_palette(&ctx);
        if self.ribbon_min && self.ribbon_peek {
            self.ribbon_peek(&ctx);
        }
        self.palette_windows(&ctx);
        self.view_windows(&ctx);
        self.drag_and_drop(&ctx);
        self.warning(&ctx);
        self.dialogs(&ctx);
        self.find_window(&ctx);
        // What Edit Tools ticked or cleared, on the ribbon or in the Tools palette.
        for name in ribbon::take_toggled() {
            if !self.tools_hidden.remove(&name) {
                self.tools_hidden.insert(name);
            }
        }
        for key in ribbon::take_toggled_panels() {
            if !self.panels_hidden.remove(&key) {
                self.panels_hidden.insert(key);
            }
        }
        // The panels drawn since the last frame: what the preset editor lists.
        self.panel_keys.extend(ribbon::take_seen_panels());
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

    /// A view and its view control bar, as the active view draws it: the bar floating over its foot, or (Compact) under it.
    fn view_body(&mut self, ui: &mut Ui) {
        let floating = !style::compact();
        if !floating {
            self.view_control_bar(ui);
        }
        // What the floating tabs and view control bar take of the view's height: a tab and a bar each, and a gap round each;
        // in Split and Tile the tabs stand above the views' area, so only the bar floats.
        let cards = if self.arrange == dock::Arrange::Tabs { 2.0 } else { 1.0 };
        self.canvas.room = if floating { cards * (FLOATING_CARD + f32::from(style::gap())) } else { 0.0 };
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
            // A family document's views are its own (`family_central`).
            Sheet::Family(..) => {}
        }
        if floating {
            self.view_control_bar(ui);
        }
        self.temp_frame(ui);
        self.edit_group_frame(ui);
    }

    /// Properties (PP): shown, or hidden.
    fn toggle_properties(&mut self) {
        let p = dock::Palette::Properties;
        self.show_palette(p, !self.docking.shown(p));
    }

    /// Toggle temporary alignment guides (smart snapping) on or off.
    pub(crate) fn toggle_alignment_guides(&mut self) {
        self.snaps.alignment_guides = !self.snaps.alignment_guides;
        let text = if self.snaps.alignment_guides { t("Alignment guides on") } else { t("Alignment guides off") };
        self.status = Status { text: text.to_string(), error: false };
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
                &mut self.stair_offset,
                &mut self.stair_height,
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
                    *field = to.field(mm);
                }
            }
            self.units = to;
        }
        let m = &self.session.model;
        // What's selected is the model's elements, and Site's points (by their library ids).
        self.selection.retain(|id| m.elements.contains_key(id) || bimcraft_core::coordinates::Point::of(m, *id).is_some());
        let selected = self.selection.as_slice();
        self.curtain_part = self.curtain_part.filter(|(id, at)| selected == [*id] && self.curtain_laid.of(m, *id).has(*at));
        if self.curtain_part.is_none() {
            self.curtain_more.clear();
        }
        if matches!(self.tool, Tool::CurtainGrid { wall, .. } | Tool::CurtainSegment { wall } if !m.wall(wall).is_some_and(|w| bimcraft_core::curtain::pattern(m, w).is_some()))
        {
            self.tool = Tool::Modify;
        }
        self.active_option = self.active_option.take().filter(|o| bimcraft_core::phasing::option_exists(m, o));
        let docs: Vec<editor::tabs::DocId> = self.families.iter().map(|d| d.id).collect();
        let exists = |s: &Sheet| match s {
            Sheet::Family(id, _) => docs.contains(id),
            Sheet::Plan(key) | Sheet::Ceiling(key) => m.plan_level(*key).is_some(),
            Sheet::Section(id) => m.section_geometry(*id).is_some(),
            Sheet::Paper(id) => m.sheet(*id).is_some(),
            Sheet::View3D | Sheet::Schedule(_) => true,
            Sheet::Table(id) => {
                matches!(m.elements.get(id), Some(Element::Schedule(_) | Element::PanelSchedule(_) | Element::RebarDetail(_)))
            }
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
        // A split without a window on an edge, or fewer than two views, is the tabbed view again.
        self.settle_arrange();
        if self.tab == Tab::Context && self.context().is_none() {
            self.tab = self.back;
        }
    }

    /// Whether a dialog, Find or a menu has the keys: the window's shortcuts wait.
    pub(crate) fn keys_taken(&self, ctx: &Context) -> bool {
        self.type_dialog.is_some()
            || self.text_type_dialog.is_some()
            || self.dimension_type_dialog.is_some()
            || self.spot_type_dialog.is_some()
            || self.curtain_editor.is_some()
            || self.go_to_view.is_some()
            || !self.curtain_asking.is_empty()
            || self.grid_type_dialog.is_some()
            || self.dimension_text_dialog.is_some()
            || self.materials_dialog.is_some()
            || self.family_dialog.is_some()
            || self.favorites_dialog.is_some()
            || self.parameter_manager.is_some()
            || self.schedule_dialog.is_some()
            || self.vg_dialog.is_some()
            || self.filters_dialog.is_some()
            || self.templates_dialog.is_some()
            || self.phasing_dialog.is_some()
            || self.display_dialog.is_some()
            || self.view_range_dialog.is_some()
            || self.halftone_dialog.is_some()
            || self.project_info_dialog.is_some()
            || self.reinforce_dialog.is_some()
            || self.steel_dialog.is_some()
            || self.detail_dialog.is_some()
            || self.line_weights_dialog.is_some()
            || self.coordinates_dialog.as_ref().is_some_and(|d| !d.picking())
            || self.object_styles_dialog.is_some()
            || self.line_styles_dialog.is_some()
            || self.fill_patterns_dialog.as_ref().is_some_and(|d| d.picking.is_none())
            || self.options_dialog.is_some()
            || self.purge_dialog.is_some()
            || self.manage_links.is_some()
            || self.asset_manager.is_some()
            || self.pdf_page_box.is_some()
            || self.select_by_id.is_some()
            || self.run_script.is_some()
            || self.work_plane.as_ref().is_some_and(|d| d.picking.is_none())
            || self.ids_of_selection.is_some()
            || self.selection_filter.is_some()
            || self.settings_page.is_some()
            || self.plugins_dialog
            || self.plugin_form_open.is_some()
            || self.find.is_some()
            || self.new_schedule.is_some()
            || self.show_shortcuts
            || self.show_snaps
            || self.pending.is_some()
            || self.autosave.show_recovery
            || self.installing()
            || self.importing()
            || self.report_open()
            || self.export_dialog.as_ref().is_some_and(|d| !d.picking)
            // An open menu takes the keys: Esc closes it, not the tool.
            || egui::Popup::is_any_open(ctx)
    }

    fn keys(&mut self, ctx: &Context) {
        // A note being written takes the keys, but for a field's that has them; another tool finishes it.
        if self.note_edit.is_some() {
            if !matches!(self.tool, Tool::Text | Tool::Modify) {
                self.finish_note();
            } else if !ctx.egui_wants_keyboard_input() && self.text_type_dialog.is_none() {
                self.note_keys(ctx);
                return;
            }
        }
        // A dialog takes the keys while it's open, and a family document's tab has its own (`family_keys`).
        if self.keys_taken(ctx) || self.active_family().is_some() {
            return;
        }
        let pressed = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        // Space in Walls' Outline's Offset field turns a centre-line offset from the inside to the outside, instead of
        // typing a space.
        if self.outline_pending
            && self.outline_side == crate::sketch::OutlineSide::Axis
            && ctx.memory(|m| m.has_focus(egui::Id::new("outline-offset")))
            && pressed(Modifiers::NONE, Key::Space)
        {
            self.outline_flip = !self.outline_flip;
            ctx.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Text(t) if t.trim().is_empty())));
            return;
        }
        // Space in Pick Walls' Offset field flips the line to the other face of its wall.
        if matches!(self.tool, Tool::SketchPick)
            && ctx.memory(|m| m.has_focus(egui::Id::new("pick-offset")))
            && pressed(Modifiers::NONE, Key::Space)
        {
            self.pick_flip = !self.pick_flip;
            ctx.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Text(t) if t.trim().is_empty())));
            return;
        }
        // Space in the Offset field flips the side the offset is drawn on, instead of typing a space: the preview and
        // the line the last click makes follow it.
        if self.target().is_some()
            && self.shape().takes_offset()
            && ctx.memory(|m| m.has_focus(egui::Id::new("draw-offset")))
            && pressed(Modifiers::NONE, Key::Space)
        {
            self.drawing.offset_flip = !self.drawing.offset_flip;
            ctx.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Text(t) if t.trim().is_empty())));
            return;
        }
        if ctx.input(|i| i.pointer.any_click()) {
            self.typed.clear();
        }
        let events: Vec<Event> = ctx.input(|i| i.events.clone());
        // A text field keeps its letters, but Save, Save As, Open, New, Find and Settings may be
        // pressed there: they do not take text away.
        if ctx.egui_wants_keyboard_input() {
            self.typed.clear();
            self.global_shortcuts(ctx, &events);
            return;
        }
        // Chords first: an exact match, on the physical key, so Ctrl+D and Ctrl+Shift+D are two
        // different bindings and a Russian layout still gives Ctrl+D. Ctrl is never part of a sequence:
        // a combination runs only when Keyboard Shortcuts assigned it.
        let mut handled: Vec<(Key, Option<Key>, Modifiers)> = Vec::new();
        for event in &events {
            let Event::Key { key, physical_key, pressed: true, repeat: false, modifiers, .. } = event else { continue };
            let physical = physical_key.unwrap_or(*key);
            // While sketching, Ctrl+Z takes back a line; the model's Undo waits for Finish.
            if self.sketch.is_some() && modifiers.command && !modifiers.shift && !modifiers.alt && physical == Key::Z {
                handled.push((*key, *physical_key, *modifiers));
                self.typed.clear();
                self.unsketch();
                continue;
            }
            let chord = shortcuts::KeyChord::from_event(physical, *modifiers);
            if let Some(action) = self.shortcuts.action_for_chord(&chord).cloned() {
                handled.push((*key, *physical_key, *modifiers));
                self.typed.clear();
                // Mid-chain, undo is the chain's: a typed number goes first, as in a field; then the point and the segment
                // built to it, never a step of the model behind it. A chain has no redo.
                if action.as_ref() == "cmd.undo" && self.chain_active() {
                    if self.typed_length.is_empty() && self.drawing.other.is_empty() {
                        self.undo_chain_point();
                    } else {
                        self.clear_typed();
                    }
                    continue;
                }
                if action.as_ref() == "cmd.redo" && self.chain_active() {
                    continue;
                }
                // A whole-selected field in hand takes Delete first: the number goes, not the selection
                // behind it. The field is cleared here, as the contextual keys would, and the event is
                // consumed so no second handler deletes an element.
                if action.as_ref() == "cmd.delete" && self.drawing.selected && modifiers.is_none() && physical == Key::Delete {
                    self.typed_length.clear();
                    self.drawing.selected = false;
                    handled.push((*key, *physical_key, *modifiers));
                    continue;
                }
                self.run_shortcut(ctx, &action);
            }
        }
        consume_key_events(ctx, &handled);

        // Ctrl+A with a reading field in hand selects that field for overwrite, as a text field does: it is taken before
        // the two-letter shortcuts, so it neither waits for a second letter nor reaches the canvas's Select All.
        if self.drawing.armed && self.line_angle_field() && pressed(Modifiers::COMMAND, Key::A) {
            self.drawing.selected = true;
            self.typed.clear();
        }
        // Ctrl+C, Ctrl+X and Ctrl+V: the selection to the clipboard, and back.
        if self.sketch.is_none() {
            self.clipboard_events(ctx);
        }
        if self.orbit.placement_drag_active() && (pressed(Modifiers::SHIFT, Key::Escape) || pressed(Modifiers::NONE, Key::Escape)) {
            self.orbit.cancel_placement_drag();
            self.status = Status::default();
            return;
        }
        // Contextual keys of a tool: Esc, Enter, Tab, Backspace, Space and the arrows are reserved.
        if pressed(Modifiers::NONE, Key::Escape) {
            self.typed.clear();
            // A custom fill pattern picked is brought back, as Underlay Settings' Move or Rotate is stopped.
            if !self.cancel_pattern_pick() && !self.cancel_underlay_edit() {
                self.escape();
            }
        }
        // Enter ends the 3D view's measured chain where it is.
        if self.sheet == Sheet::View3D
            && matches!(self.tool, Tool::Measure(_))
            && self.orbit.chain.len() >= 2
            && self.orbit.chain_done.is_none()
            && pressed(Modifiers::NONE, Key::Enter)
        {
            self.orbit.chain_done = Some(false);
        }
        // Enter confirms Edit Dimension References; else it places a typed length, else builds the wall chain.
        let mut enter = pressed(Modifiers::NONE, Key::Enter);
        // Scale's length is typed in its own popup, which takes its Enter.
        if matches!(self.tool, Tool::Scale(_)) {
            enter = false;
        }
        if enter && let Sheet::Section(view) = self.sheet {
            self.vertical_enter(view);
            enter = false;
        }
        if enter && self.dimension_refs_edit.is_some() {
            self.commit_dimension_refs();
            enter = false;
        }
        if enter && let Some(level) = self.plan_level() {
            match (&self.tool, self.typed_length.is_empty() && self.drawing.other.is_empty()) {
                (&Tool::Rotate { centre, .. }, false) if let Some(angle) = self.typed_angle() => {
                    self.typed_length.clear();
                    self.turn(centre, angle);
                }
                (&Tool::RotateNorth { centre, .. }, false) if let Some(angle) = self.typed_angle() => {
                    self.typed_length.clear();
                    self.rotate_project_north(Some(angle), Some(centre));
                }
                _ if self.picking_pattern_lines() => self.finish_pattern_lines(),
                (Tool::Railing(_), true) => self.finish_railing(level),
                (Tool::Ramp(_), true) => self.finish_ramp_path(level),
                (Tool::Stair(_), true) if self.stair_shape == StairShape::Risers => self.finish_stair_sketch(level),
                (Tool::Stair(_), true) if self.stair_shape != StairShape::Spiral => self.finish_stair_path(level),
                (Tool::Mass(_), true) => self.finish_mass(level),
                (Tool::DetailLine(_), true) => self.finish_detail_line(level),
                (Tool::SketchLine(_), true) if self.shape().open_ended() => self.finish_open_shape(),
                (_, true) => self.finish_wall(),
                (_, false) => self.place_typed(level),
            }
            // A snap override typed was for that point.
            self.snap_once = None;
        }
        // Backspace, read once, goes to the first of these that wants it; with none, it deletes the selection as Delete
        // does, since a Mac's keyboard has no Delete key (its "delete" is Backspace).
        let mut backspace = pressed(Modifiers::NONE, Key::Backspace);
        if backspace && self.drawing.selected {
            // A whole-selected field is cleared by Backspace, not clipped a digit at a time: the number goes and the field
            // catches up with the pointer again.
            self.typed_length.clear();
            self.drawing.selected = false;
            backspace = false;
        } else if backspace && !self.typed_length.is_empty() {
            self.typed_length.pop();
            backspace = false;
        }
        // Backspace in a wall or detail line chain takes its last point back, and the segment built to it, so the model and
        // the chain stay one drawing.
        if backspace && self.typed_length.is_empty() && self.drawing.other.is_empty() && self.chain_active() {
            self.undo_chain_point();
            backspace = false;
        }
        // Backspace while a dimension is being picked takes its last reference out.
        if backspace
            && self.typed_length.is_empty()
            && self.dim_edit.is_none()
            && self.dimension_refs_edit.is_none()
            && let Tool::Dimension(_, refs) = &mut self.tool
        {
            refs.pop();
            backspace = false;
        }
        // A dimension being picked or edited keeps Backspace, as it always has, whatever it did.
        if matches!(self.tool, Tool::Dimension(..)) || self.dim_edit.is_some() || self.dimension_refs_edit.is_some() {
            backspace = false;
        }
        if pressed(Modifiers::NONE, Key::Delete) || backspace {
            // While a field is whole-selected, Delete empties that field rather than deleting the selection.
            if self.drawing.selected {
                self.typed_length.clear();
                self.drawing.selected = false;
            } else {
                self.delete();
            }
        }
        // The arrow keys nudge the selection, as Revit's do.
        self.nudge_keys(ctx);
        // consume_key also matches Alt/Shift and key repeats. Only an unmodified downstroke turns a wall,
        // a door or a window, so holding Shift or Space cannot fill the Undo history.
        let plain_space =
            |e: &Event| matches!(e, Event::Key { key: Key::Space, pressed: true, repeat: false, modifiers: Modifiers::NONE, .. });
        // Space on a component or a column taken up again, dragged or alone in the selection: a quarter turn, as Revit's.
        if ctx.input(|i| i.events.iter().any(plain_space)) && self.space_turn() {
            pressed(Modifiers::NONE, Key::Space);
        }
        if matches!(self.tool, Tool::Door | Tool::Window | Tool::Modify)
            && ctx.input(|i| i.events.iter().any(plain_space))
            && self.opening_space()
        {
            pressed(Modifiers::NONE, Key::Space);
        }
        // Space while rotating: the next click places the centre, as Revit's Space does.
        if let Tool::Rotate { place, start, .. } | Tool::RotateNorth { place, start, .. } = &mut self.tool
            && pressed(Modifiers::NONE, Key::Space)
        {
            *place = true;
            *start = None;
        }
        // Space while Walls' Outline follows a centre line: its offset goes from the loop's inside to its outside.
        if self.outline_pending && self.outline_side == crate::sketch::OutlineSide::Axis && pressed(Modifiers::NONE, Key::Space) {
            self.outline_flip = !self.outline_flip;
        }
        // Space while Pick Walls is on: its line goes to the other face of the wall.
        if matches!(self.tool, Tool::SketchPick) && pressed(Modifiers::NONE, Key::Space) {
            self.pick_flip = !self.pick_flip;
        }
        // Space while a Draw tool is on: its Offset goes to the other side, in the preview and in what the click makes.
        if self.target().is_some() && self.shape().takes_offset() && pressed(Modifiers::NONE, Key::Space) {
            self.drawing.offset_flip = !self.drawing.offset_flip;
        }
        // Space steps the Dimension tool's lay: Auto, Aligned, Horizontal, Vertical.
        if let Tool::Dimension(mode, _) = &mut self.tool
            && pressed(Modifiers::NONE, Key::Space)
        {
            *mode = mode.next();
        }
        if matches!(self.tool, Tool::Component | Tool::Fixture(_)) && pressed(Modifiers::NONE, Key::Space) {
            self.component_angle = (self.component_angle + 90.0) % 360.0;
        }
        // F8 is the one function key for the guides. F11 is free: the system may use it for full screen.
        if pressed(Modifiers::NONE, Key::F8) {
            self.toggle_alignment_guides();
        }
        // Q marks the point or edge under the pointer as a temporary snap reference, before the first point too.
        // Q also starts the Quantities sequence (QT): it joins the sequence buffer, so the pin and the
        // shortcut can share the key.
        if pressed(Modifiers::NONE, Key::Q) && matches!(self.sheet, Sheet::Plan(_) | Sheet::Ceiling(_)) {
            self.pin_under_cursor();
            self.sequence_letter(ctx, 'Q');
            ctx.request_repaint();
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
        // after it (feet, inches, fractions, a unit) is the length's until Enter. Letters typed
        // without Ctrl are no shortcut.
        for ch in typed.chars() {
            let listening = self.base().is_some() || self.filleting() || self.rotating();
            // A bearing west of east is negative, so the angle field takes its sign; Rotate always has.
            let negative_angle = self.drawing.field == 1 && self.line_angle_field();
            let sign = ch == '-' && (self.rotating() || negative_angle) && self.typed_length.is_empty();
            if listening && (ch.is_ascii_digit() || ch == '.' || sign || !self.typed_length.is_empty()) {
                // The field `Tab` armed holds what is selected, so the first digit replaces it instead of appending.
                if self.drawing.selected {
                    self.typed_length.clear();
                    self.drawing.selected = false;
                }
                self.drawing.armed = true;
                self.typed_length.push(ch);
            }
        }

        // A sequence is typed on the keyboard's row without Ctrl (W and A for Wall), as Revit's are; a tool taking a
        // typed length keeps the digits and the letters after them, so a length never becomes a sequence.
        let listening = self.base().is_some() || self.filleting() || self.rotating();
        self.sequences(ctx, listening);
        self.sequence_waited(ctx);
    }

    /// The sequence keys typed this frame: one that starts a two- or three-key binding waits for the next, and one that
    /// starts none is dropped. Only Ctrl-free keys come here, so a combination runs solely as Keyboard Shortcuts
    /// assigned it; `listening` is true while a tool takes a typed length, when a digit is the length's and no sequence's.
    fn sequences(&mut self, ctx: &Context, listening: bool) {
        let taking = !self.typed_length.is_empty();
        let letters = ctx.input_mut(|i| {
            let events = std::mem::take(&mut i.events);
            let mut out = Vec::new();
            let mut kept = Vec::with_capacity(events.len());
            for event in events {
                let letter = match &event {
                    Event::Key { key, physical_key, pressed: true, repeat: false, modifiers, .. }
                        if !modifiers.ctrl && !modifiers.alt && !modifiers.mac_cmd && !modifiers.command =>
                    {
                        let physical = physical_key.unwrap_or(*key);
                        crate::shortcuts::is_sequence_key(physical).then_some(physical)
                    }
                    _ => None,
                };
                let Some(physical) = letter else {
                    kept.push(event);
                    continue;
                };
                // A digit is a typed length while a tool takes one, and a letter after one is the length's too.
                let digit = physical.name().chars().next().is_some_and(|c| c.is_ascii_digit());
                if taking || (listening && digit) {
                    kept.push(event);
                    continue;
                }
                out.push(physical);
            }
            i.events = kept;
            out
        });
        for physical in letters {
            let Some(letter) = physical.name().chars().next() else { continue };
            self.sequence_letter(ctx, letter);
        }
    }

    /// A sequence key typed: with the one before it, a shortcut; if the two are no binding, the first is dropped and the
    /// second waits in its place. A plain letter that no sequence names does nothing at all.
    fn sequence_letter(&mut self, ctx: &Context, letter: char) {
        self.typed.push(letter);
        self.typed_at = ctx.input(|i| i.time);
        loop {
            // A letter that begins no binding is no part of a sequence, and is dropped at once.
            let first_started = self.typed.chars().next().and_then(crate::shortcuts::key_of_char);
            if first_started.is_some_and(|k| !self.shortcuts.sequence_starts_with(k)) {
                self.typed.remove(0);
                continue;
            }
            if self.typed.len() < 2 {
                return;
            }
            if let Some(action) = self.shortcuts.action_for_text(&self.typed) {
                self.typed.clear();
                self.run_shortcut(ctx, &action);
                return;
            }
            // A pair that starts a three-key binding waits for its third letter.
            if self.typed.len() == 2 && self.shortcuts.text_starts_three(&self.typed) {
                return;
            }
            // The first letter began no binding: it is dropped, and the one after it waits in its place.
            self.typed.remove(0);
        }
    }

    /// A letter left waiting for its second is forgotten after [`SEQUENCE_WAIT`], so a stray key never completes a pair
    /// much later.
    fn sequence_waited(&mut self, ctx: &Context) {
        if self.typed.is_empty() {
            return;
        }
        let now = ctx.input(|i| i.time);
        let left = SEQUENCE_WAIT - (now - self.typed_at);
        if left > 0.0 {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(left));
            return;
        }
        self.typed.clear();
    }

    /// Runs the action a shortcut names, saying so in the status bar when it has nothing to do here.
    fn run_shortcut(&mut self, ctx: &Context, action: &shortcuts::ActionId) {
        if !self.execute_action(ctx, action) {
            self.status = Status { text: t("That shortcut has nothing to do here").to_string(), error: true };
        }
    }

    /// The arrow keys move what Modify has selected a step across the view, as Revit's nudge does: right, left, up or down on
    /// screen in a plan however it is turned, along its line or up in a section or an elevation. A step is [`NUDGE_PX`] pixels
    /// at the view's zoom ([`nudge_step`]), and Shift makes it ten. Each press is one Move, so Undo takes them back one by one,
    /// and Move's rules hold: doors and windows slide along their walls, joined walls stretch, pinned elements stay.
    fn nudge_keys(&mut self, ctx: &Context) {
        if !matches!(self.tool, Tool::Modify) || self.selection.is_empty() || self.sketch.is_some() {
            return;
        }
        let key = match self.sheet {
            Sheet::Plan(k) | Sheet::Ceiling(k) | Sheet::Section(k) => k,
            _ => return,
        };
        let Some(view) = self.canvas.view(key) else { return };
        // Steps right and up on screen. Shift's first: a plain arrow's pattern matches one pressed with Shift too.
        let mut steps = P2(0.0, 0.0);
        for (k, dir) in [
            (Key::ArrowRight, P2(1.0, 0.0)),
            (Key::ArrowLeft, P2(-1.0, 0.0)),
            (Key::ArrowUp, P2(0.0, 1.0)),
            (Key::ArrowDown, P2(0.0, -1.0)),
        ] {
            for (m, n) in [(Modifiers::SHIFT, 10.0), (Modifiers::NONE, 1.0)] {
                let count = ctx.input_mut(|i| i.count_and_consume_key(m, k));
                steps = steps + dir * (n * count as f64);
            }
        }
        if steps.length() == 0.0 {
            return;
        }
        // Screen right and up as the model's directions: a plan oriented to true north is turned on screen.
        let o = view.mm((0.0, 0.0));
        let (right, up) = ((view.mm((1.0, 0.0)) - o).unit(), (view.mm((0.0, -1.0)) - o).unit());
        let by = (right * steps.0 + up * steps.1) * nudge_step(view.scale);
        if !by.is_finite() {
            return;
        }
        let by = [(by.0 * 1e3).round() / 1e3 + 0.0, (by.1 * 1e3).round() / 1e3 + 0.0];
        let ids = self.selection.clone();
        match self.sheet {
            Sheet::Section(view) => self.run("move", json!({"view": view, "ids": ids, "by": by})),
            _ => self.run("move", json!({"ids": ids, "by": by})),
        };
    }

    /// The shortcuts a text field leaves alone: Save, Save As, Open, New, Find, Settings, Export PDF and Properties.
    fn global_shortcuts(&mut self, ctx: &Context, events: &[Event]) {
        const ALLOWED: &[&str] = &[
            "cmd.save",
            "ui.file.save_as",
            "cmd.open",
            "cmd.new",
            "ui.find",
            "ui.settings",
            "ui.settings.shortcuts",
            "ui.file.export_pdf",
            "ui.palette.properties",
        ];
        let mut handled: Vec<(Key, Option<Key>, Modifiers)> = Vec::new();
        for event in events {
            let Event::Key { key, physical_key, pressed: true, repeat: false, modifiers, .. } = event else { continue };
            let physical = physical_key.unwrap_or(*key);
            let chord = shortcuts::KeyChord::from_event(physical, *modifiers);
            let Some(action) = self.shortcuts.action_for_chord(&chord).cloned() else { continue };
            if !ALLOWED.contains(&action.as_ref()) {
                continue;
            }
            handled.push((*key, *physical_key, *modifiers));
            self.typed.clear();
            self.execute_action(ctx, &action);
        }
        consume_key_events(ctx, &handled);
    }

    /// Revit's two-key shortcuts, kept for the family document and for tests. The window's own
    /// input path reads physical keys in [`BimApp::keys`].
    #[cfg(test)]
    fn shortcut(&mut self) -> bool {
        let Some(action) = self.shortcuts.action_for_text(&self.typed) else { return false };
        let ctx = Context::default();
        self.execute_action(&ctx, &action)
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
            Some(Element::RoomSeparator(_)) => Tool::RoomSeparator(None),
            Some(Element::Level(_)) => Tool::Level,
            Some(Element::Grid(_)) => Tool::Grid(None),
            Some(Element::RefPlane(_)) => Tool::ReferencePlane(None),
            Some(Element::Dimension(d)) => match d.kind {
                DimKind::Aligned => Tool::Dimension(DimMode::Aligned, Vec::new()),
                DimKind::Linear if d.orient == Some(bimcraft_core::model::Orient::Vertical) => {
                    Tool::Dimension(DimMode::Vertical, Vec::new())
                }
                DimKind::Linear => Tool::Dimension(DimMode::Horizontal, Vec::new()),
                DimKind::Angular => Tool::Angular(Vec::new()),
                k @ (DimKind::Radial | DimKind::Diameter | DimKind::ArcLength) => Tool::Round(k, Vec::new()),
            },
            Some(Element::Text(_)) => Tool::Text,
            Some(Element::DetailLine(l)) => {
                self.line_style.clone_from(&l.style);
                Tool::DetailLine(Vec::new())
            }
            Some(Element::Spot(sp)) if sp.slope => Tool::SpotSlope,
            Some(Element::Spot(sp)) if sp.coordinate => Tool::SpotCoordinate(None),
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

    /// SA retains the visible-in-view scope; the menus also offer the entire project.
    fn select_all_instances(&mut self) {
        if !self
            .selection
            .iter()
            .filter_map(|id| self.session.model.elements.get(*id))
            .any(|e| bimcraft_core::selection::instance_type(e).is_some())
        {
            self.status = Status { text: t("select an element that has a type first").into(), error: true };
            return;
        }
        self.select_instances(json!({"ids": self.selection}), context::InstanceScope::View);
    }

    /// Both menus query the same core matching logic. View candidates come from the
    /// active view, including temporary visibility; the project scope omits that limit.
    fn select_instances(&mut self, mut params: Value, scope: context::InstanceScope) {
        if scope == context::InstanceScope::View {
            // The linked view model may draw linked ids; project selection contains only local instances.
            params["within"] =
                json!(self.shown().into_iter().filter(|id| self.session.model.elements.contains_key(id)).collect::<Vec<_>>());
        }
        if let Some(reply) = self.run("select_instances", params) {
            let found = reply["ids"].as_array().map(|ids| ids.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
            self.pick_up(found, Modifiers::NONE);
        }
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
        let m = self.view_model();
        let owners = |d: render::Drawing| d.owners.into_iter().flatten().collect::<BTreeSet<Id>>();
        // Fitted at any size: what a view shows doesn't depend on it.
        let (w, h) = (1600, 1000);
        match self.sheet {
            Sheet::Plan(level) => owners(render::plan(&m, level, w, h)),
            Sheet::Ceiling(level) => owners(render::ceiling_plan(&m, level, w, h)),
            Sheet::Section(id) => {
                let view = render::section_fit(&m, id, w, h);
                // ponytail: the picture leaves out what's hidden behind something nearer; test the
                // solids against the view's extent if SA should find those too.
                let seen = render::section_picture(&m, id, view, &[]).map(|i| i.shows()).unwrap_or_default();
                owners(render::section(&m, id, view)).into_iter().chain(seen).collect()
            }
            // The whole model, but for what the 3D view open hides.
            Sheet::View3D => {
                let look = bimcraft_core::graphics::Look::of_3d_view(&m, self.active_3d());
                m.elements.keys().copied().filter(|id| look.shows(*id)).collect()
            }
            Sheet::Paper(_) | Sheet::Schedule(_) | Sheet::Table(_) | Sheet::Family(..) => BTreeSet::new(),
        }
    }

    /// The material the Paint tool lays: the one picked while the library has it, else the library's first.
    pub(crate) fn painting(&self) -> Option<String> {
        let m = &self.session.model;
        Some(self.paint_material.clone()).filter(|n| m.materials.contains_key(n)).or_else(|| m.materials.keys().next().cloned())
    }

    fn set_tool(&mut self, mut tool: Tool) {
        self.adaptive_clicks.clear();
        if !matches!(tool, Tool::Paste | Tool::PickLevel) {
            self.pasting = None;
        }
        self.note_arrow = None;
        // Another tool takes the Options Bar: Walls' Outline's own closes, putting no outline in the sketch. The Modify
        // arrow is the one Walls' Outline itself uses, so choosing it leaves the mode on.
        self.sketch_line_chain = None;
        if tool != Tool::Modify {
            self.leave_walls_outline();
        }
        // Trim/Extend to Corner with two lines or walls selected (or picked, sketching) joins them at once, keeping the
        // parts toward their middles; with any other selection it asks for the picks.
        if tool == Tool::Trim(None) && self.corner_selected() {
            return;
        }
        // Paint works in the 3D view: open it, as the tool asks.
        if matches!(tool, Tool::Paint | Tool::Unpaint) && self.sheet != Sheet::View3D {
            self.open_sheet(Sheet::View3D);
        }
        // Scale asks first whether there's anything here it takes, and then how.
        if let Tool::Scale(_) = tool {
            if let Some(why) = self.scale_refused() {
                self.status = Status { text: why, error: true };
                return;
            }
            tool = Tool::Scale(scale::Step::Choose);
            (self.scaling.error, self.scaling.focused) = (None, false);
        }
        // Move, Copy, Rotate, Scale and Mirror take the selection; Align, Trim, Split and Offset pick walls, Demolish anything.
        let edits = matches!(tool, Tool::Move(_) | Tool::Copy(_) | Tool::Rotate { .. } | Tool::Scale(_) | Tool::Mirror { .. });
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
        if tool == Tool::Attach && self.selected_attachable().is_empty() {
            self.status = Status { text: t("select the walls or columns to attach first").into(), error: true };
            return;
        }
        if matches!(tool, Tool::SlopeArrow(_)) && self.selected_floor().is_none() {
            self.status = Status { text: t("select the floor to slope first").into(), error: true };
            return;
        }
        if matches!(tool, Tool::ModifySubElements | Tool::AddPoint | Tool::AddSplitLine(_)) && self.selected_shapeable().is_none() {
            self.status = Status { text: t("select the floor or flat roof to shape first").into(), error: true };
            return;
        }
        // Modify Sub Elements starts with nothing of the shape picked, its Elevation blank until something is.
        if tool == Tool::ModifySubElements && self.tool != Tool::ModifySubElements {
            self.shape_picked.clear();
            self.shape_z.clear();
        }
        if let Tool::Rotate { centre, .. } = &mut tool
            && let Some(middle) = self.middle()
        {
            *centre = middle;
        }
        if let Tool::RotateNorth { centre, .. } = &mut tool {
            let b = self.session.model.coordinates.base;
            *centre = P2(b[0], b[1]);
        }
        if tool == Tool::Level {
            let top = self.session.model.levels().last().map_or(0.0, |(_, l)| l.elevation);
            self.new_level = (self.session.model.field(top + 3000.0), String::new());
        }
        let shaping = matches!(tool, Tool::SlopeArrow(_) | Tool::ModifySubElements | Tool::AddPoint | Tool::AddSplitLine(_));
        let placing_grid = matches!(tool, Tool::CurtainGrid { .. } | Tool::CurtainSegment { .. });
        if tool != Tool::Modify && tool != Tool::Attach && tool != Tool::GableEdge && !shaping && !edits && !placing_grid {
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
            (self.orbit.chain, self.orbit.chain_done) = (Vec::new(), None);
        }
        self.snap_once = None;
        self.door_flip_hand = false;
        self.window_flip_facing = false;
        self.tool = tool;
        // Floor sketches its outline at once, as Revit's does (Walls' Outline puts the walls' in the sketch).
        if self.tool == Tool::Floor
            && self.sketch.is_none()
            && matches!(self.sheet, Sheet::Plan(_) | Sheet::Ceiling(_))
            && let Some(level) = self.plan_level()
        {
            self.start_sketch(crate::sketch::Of::Floor(level), Vec::new());
            return;
        }
        // In the 3D view the tools that sketch an outline sketch it on the work plane at once.
        if self.sheet == Sheet::View3D
            && self.sketch.is_none()
            && matches!(self.tool, Tool::Floor | Tool::Ceiling | Tool::Roof | Tool::Shaft(_))
        {
            self.sketch_on_work_plane();
        }
    }

    /// The walls among the selection.
    fn selected_walls(&self) -> Vec<Id> {
        self.selection.iter().copied().filter(|id| self.session.model.wall(*id).is_some()).collect()
    }

    /// A click of Trim/Extend to Corner or Trim/Extend Single or Multiple Elements on `hovered`, at plan point `at` (the side kept),
    /// in a plan or the 3D view alike.
    pub(crate) fn trim_click(&mut self, hovered: Option<Id>, at: P2) {
        match (self.tool.clone(), hovered) {
            (Tool::Trim(None), Some(a)) => self.tool = Tool::Trim(Some((a, at))),
            (Tool::Trim(Some((a, keep))), Some(b)) if b != a => {
                self.run("corner", json!({"ids": [a, b], "keep": [[keep.0, keep.1], [at.0, at.1]]}));
                self.tool = Tool::Trim(None);
            }
            (Tool::TrimTo { boundary: None, many }, Some(b)) => self.tool = Tool::TrimTo { boundary: Some(b), many },
            (Tool::TrimTo { boundary: Some(b), many }, Some(id)) if id != b => {
                self.run("trim_extend", json!({"boundary": b, "ids": [id], "keep": [[at.0, at.1]]}));
                if !many {
                    self.tool = Tool::TrimTo { boundary: None, many };
                }
            }
            _ => {}
        }
    }

    /// What's selected that Attach Top/Base takes: the walls and columns.
    pub(crate) fn selected_attachable(&self) -> Vec<Id> {
        let m = &self.session.model;
        let can = |id: &Id| matches!(m.elements.get(*id), Some(Element::Wall(_) | Element::Column(_) | Element::StructuralColumn(_)));
        self.selection.iter().copied().filter(can).collect()
    }

    /// Attach Top/Base's pick, in a plan or the 3D view: the selected walls and columns onto roof or floor `id`, then Modify, them
    /// still selected.
    pub(crate) fn attach_to(&mut self, id: Id) {
        let field = if self.attach_base { "attach_base" } else { "attach_top" };
        let mut p = json!({"ids": self.selected_attachable()});
        p[field] = json!(id);
        if self.run("modify", p).is_some() {
            self.tool = Tool::Modify;
        }
    }

    /// The roof selected, when one roof is all that is.
    pub(crate) fn selected_roof(&self) -> Option<Id> {
        match self.selection.as_slice() {
            &[id] if matches!(self.session.model.elements.get(id), Some(Element::Roof(_))) => Some(id),
            _ => None,
        }
    }

    /// Shape Editing's own tools for floor or flat roof `id`: small, two by two as the Modify panel's are, so the panels
    /// after them keep their room.
    fn shape_tools(&mut self, ui: &mut egui::Ui, id: Id) {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.horizontal(|ui| {
                let tip =
                    msg!("Modify Sub Elements: select a point, a corner or an edge of the top, then type its height on the Options Bar or its label");
                let active = self.tool == Tool::ModifySubElements;
                if small_button(ui, Icon::ModifySubElements, "Modify Sub Elements", active, tip).clicked() {
                    self.set_tool(Tool::ModifySubElements);
                }
                let tip = msg!("Add Point: click on the top to raise or lower it there to the Options Bar's elevation");
                if small_button(ui, Icon::AddPoint, "Add Point", self.tool == Tool::AddPoint, tip).clicked() {
                    self.set_tool(Tool::AddPoint);
                }
                // Convert Lines, as Revit's: on while Modify Sub Elements has folding lines selected.
                let tip = msg!("Convert Lines: make the selected folding lines split lines, so the top keeps folding along them");
                let can = self.tool == Tool::ModifySubElements && !self.shape_picked_folds().is_empty();
                if ui.add_enabled_ui(can, |ui| small_button(ui, Icon::ConvertLines, msg!("Convert Lines"), false, tip)).inner.clicked() {
                    self.shape_convert_lines();
                }
            });
            ui.horizontal(|ui| {
                let tip = msg!("Add Split Line: click two points; the top folds along the line between them");
                let active = matches!(self.tool, Tool::AddSplitLine(_));
                if small_button(ui, Icon::AddSplitLine, "Add Split Line", active, tip).clicked() {
                    self.set_tool(Tool::AddSplitLine(None));
                }
                let tip = msg!("Reset Shape: take the points and split lines off, the top flat again");
                if small_button(ui, Icon::ResetShape, "Reset Shape", false, tip).clicked() {
                    self.run("reset_shape", serde_json::json!({"id": id}));
                    self.tool = Tool::Modify;
                }
            });
        });
    }

    /// Shape Editing's Elevation Base on the Options Bar, as Revit's: what its elevations count from.
    fn elevation_base(&mut self, ui: &mut Ui) {
        let l = ui.label(t("Elevation Base:"));
        let now = self.shape_base;
        egui::ComboBox::from_id_salt("shape-elevation-base")
            .selected_text(t(now.name()))
            .show_ui(ui, |ui| {
                for b in crate::shape::Base::ALL {
                    ui.selectable_value(&mut self.shape_base, b, t(b.name()));
                }
            })
            .response
            .labelled_by(l.id);
        if self.shape_base != now {
            self.shape_refresh();
        }
    }

    /// What Shape Editing works on, when it's all that's selected: a floor, or a flat roof by footprint.
    pub(crate) fn selected_shapeable(&self) -> Option<Id> {
        match self.selection.as_slice() {
            &[id] if self.session.model.shapeable(id).is_ok() => Some(id),
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

    /// While placing a swinging door Space changes only its pending hand; while placing a window it changes the
    /// pending face. With one door selected, step through the four Opens tiles in order with the same instance edit
    /// as a tile click.
    fn opening_space(&mut self) -> bool {
        match self.tool {
            Tool::Door if self.plan_level().is_some() => {
                if !self.picked_door_swings() {
                    return false;
                }
                self.door_flip_hand = !self.door_flip_hand;
            }
            Tool::Window if self.plan_level().is_some() => self.window_flip_facing = !self.window_flip_facing,
            Tool::Modify if self.sketch.is_none() => {
                let ids = self.flippable();
                if ids.is_empty() {
                    return false;
                }
                self.run("flip", json!({"ids": ids, "cycle": "auto"}));
            }
            _ => return false,
        }
        true
    }

    /// Whether the Door tool's picked type is a swing door, whose hinge Space can turn.
    fn picked_door_swings(&self) -> bool {
        let (family, ty) = self.opening_pick("door");
        ty.as_deref().is_some_and(|t| self.session.model.door_flips(family.as_deref(), t).hand)
    }

    /// Whether every selected element is an opening that Space can turn, so the status can say what Space does.
    fn selection_flips(&self) -> bool {
        let m = &self.session.model;
        !self.selection.is_empty()
            && self.selection.iter().all(|id| m.elements.get(*id).and_then(|e| m.opening_flips(e)).is_some_and(|flips| flips.any()))
    }

    /// Whether the selection is only doors with no Opens positions, so Space has no swing to turn.
    fn selection_has_no_swing(&self) -> bool {
        let m = &self.session.model;
        !self.selection.is_empty()
            && self
                .selection
                .iter()
                .all(|id| matches!(m.elements.get(*id), Some(Element::Door(o)) if !m.door_flips(o.family.as_deref(), &o.type_name).any()))
    }

    /// The selected elements Space and Flip turn over: walls, stairs, and doors and windows whose type flips.
    fn flippable(&self) -> Vec<Id> {
        let m = &self.session.model;
        self.selection
            .iter()
            .copied()
            .filter(|id| {
                m.wall(*id).is_some()
                    || matches!(m.elements.get(*id), Some(Element::Stair(_) | Element::Ramp(_)))
                    || m.elements.get(*id).and_then(|e| m.opening_flips(e)).is_some_and(|flips| flips.any())
            })
            .collect()
    }

    /// Space in Modify on a selection with nothing to flip: a quarter turn counter-clockwise, as Revit's Space gives a component
    /// taken up again. While the selection is dragged it turns as it goes, about the one element's own point (a component's or
    /// a column's), else about the point it was taken by, and the drag goes on with it turned. Not dragged, one component or
    /// column selected turns about its own point. Whether it turned: else Space is left to the flips.
    fn space_turn(&mut self) -> bool {
        if self.tool != Tool::Modify
            || self.selection.is_empty()
            || self.sketch.is_some()
            || !matches!(self.sheet, Sheet::Plan(_) | Sheet::Ceiling(_))
            || !self.flippable().is_empty()
        {
            return false;
        }
        let m = &self.session.model;
        let own = match self.selection.as_slice() {
            &[id] => match m.elements.get(id) {
                Some(Element::Component(c)) if c.adaptive.is_none() => Some(c.at),
                Some(Element::Column(c)) => Some(c.at),
                _ => None,
            },
            _ => None,
        };
        let Some(about) = own.or_else(|| self.canvas.moving_from()) else { return false };
        let ids = self.selection.clone();
        self.run("rotate", json!({"ids": ids, "about": [about.0, about.1], "angle": 90.0})).is_some()
    }

    /// The selected walls, stairs and openings turned over, in one Undo step: Revit's flip arrows, or Space. Stairs selected climb
    /// the other way.
    fn flip(&mut self) {
        let ids = self.flippable();
        if !ids.is_empty() {
            self.run("flip", json!({"ids": ids, "cycle": "auto"}));
        }
    }

    /// The stairs in the selection.
    fn selected_stairs(&self) -> Vec<Id> {
        let m = &self.session.model;
        self.selection.iter().copied().filter(|id| matches!(m.elements.get(*id), Some(Element::Stair(_)))).collect()
    }

    /// Code Check (CK) of stair `id`: the rules it breaks in a window, or that it meets them all.
    fn code_check(&mut self, id: Id) {
        let m = &self.session.model;
        let outside = matches!(m.elements.get(id), Some(Element::Stair(st)) if !bimcraft_core::codes::stair_findings(m, st).is_empty());
        if outside {
            self.code_findings = Some(id);
        } else {
            self.status = Status { text: t("This stair meets every rule of the project's building code.").to_string(), error: false };
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
                // A camera stands on the plan at its eye: turning it turns it about that point unless Place Centre says otherwise.
                Some(Element::View3D(v)) => pts.extend(v.camera.map(|c| P2(c.eye[0], c.eye[1]))),
                _ => {}
            }
        }
        let first = *pts.first()?;
        let (lo, hi) = pts.iter().fold((first, first), |(lo, hi), p| (P2(lo.0.min(p.0), lo.1.min(p.1)), P2(hi.0.max(p.0), hi.1.max(p.1))));
        Some(P2((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0))
    }

    /// The point the tool measures its next one from, while it has one.
    /// Rotate or Rotate Project North waiting for its angle (not its centre): digits typed over the view are the angle.
    pub(crate) fn rotating(&self) -> bool {
        matches!(self.tool, Tool::Rotate { place: false, .. } | Tool::RotateNorth { place: false, .. })
    }

    /// The angle typed over the view while rotating, degrees counter-clockwise, if it reads as one.
    pub(crate) fn typed_angle(&self) -> Option<f64> {
        let t = self.typed_length.trim().trim_end_matches('°');
        self.rotating().then(|| t.parse::<f64>().ok()).flatten().filter(|a| a.is_finite() && a.abs() <= 360.0)
    }

    fn base(&self) -> Option<P2> {
        match &self.tool {
            Tool::Wall(chain) => chain.last().copied(),
            Tool::Run(_, start) => *start,
            Tool::Grid(start)
            | Tool::ReferencePlane(start)
            | Tool::RoomSeparator(start)
            | Tool::Move(start)
            | Tool::Copy(start)
            | Tool::Section(start)
            | Tool::SlopeArrow(start)
            | Tool::AddSplitLine(start) => *start,
            Tool::Scale(scale::Step::Second(a)) => Some(*a),
            Tool::Stair(pts) | Tool::Ramp(pts) | Tool::Railing(pts) | Tool::Mass(pts) | Tool::DetailLine(pts) | Tool::SketchLine(pts) => {
                pts.last().copied()
            }
            _ => None,
        }
    }

    /// A wall or detail line chain with a point already placed.
    fn chain_active(&self) -> bool {
        matches!(self.tool, Tool::Wall(_) | Tool::DetailLine(_)) && self.base().is_some()
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
        // A number being typed, or a field armed with Tab, goes first, and the field back to Length: the point it is for
        // is still wanted, and a stale locked length must not linger. The next Esc drops the chain.
        if !self.typed_length.trim().is_empty() || !self.drawing.other.trim().is_empty() || self.drawing.armed {
            self.clear_typed();
            return;
        }
        // Esc drops the drawing's marked references with the points they were marked for.
        self.canvas.guides_reset();
        // Walls' Outline's Options Bar clears its walls first: the mode itself stays on.
        if self.outline_pending {
            self.outline_walls.clear();
            self.selection.clear();
            self.status = Status::default();
            return;
        }
        // A note's leader point clicked goes first, the Text tool staying on.
        if self.note_arrow.take().is_some() {
            self.status = Status::default();
            return;
        }
        // A form selected in a family document goes first.
        if let Some(ed) = self.active_family_mut()
            && ed.let_form_go()
        {
            return;
        }
        // Edit Dimension References goes next: the dimension stays as it was.
        if self.dimension_refs_edit.take().is_some() {
            return;
        }
        // Edit Group's Add or Remove picking stops, the edit going on.
        if self.group_pick.take().is_some() {
            return;
        }
        // A dimension segment picked goes, leaving its dimension as it was.
        if self.dimension_segment.take().is_some() || self.dimension_segment_hover.take().is_some() {
            return;
        }
        self.dimension_reference_hover = None;
        // A number field left by Esc keeps the selection it was typed for.
        if std::mem::take(&mut self.dim_edit_escaped) || self.dim_edit.take().is_some() {
            return;
        }
        // Pick in View goes back to its dialog, and so does Specify Coordinates' pick.
        if self.stop_work_plane_pick() || self.stop_specify_pick() {
            return;
        }
        self.typed.clear();
        self.ribbon_peek = false;
        self.sketch_line_chain = None;
        // A snap override waiting for its point goes first, leaving what's being drawn.
        if self.snap_once.take().is_some() {
            return;
        }
        // An adaptive instance's points clicked go next, leaving the tool.
        if !self.adaptive_clicks.is_empty() {
            self.adaptive_clicks.clear();
            self.status = Status::default();
            return;
        }
        self.clear_typed();
        self.status = Status::default();
        self.tool = match std::mem::replace(&mut self.tool, Tool::Modify) {
            Tool::Wall(chain) if !chain.is_empty() => Tool::Wall(Vec::new()),
            Tool::Grid(Some(_)) => Tool::Grid(None),
            Tool::ReferencePlane(Some(_)) => Tool::ReferencePlane(None),
            Tool::RoomSeparator(Some(_)) => Tool::RoomSeparator(None),
            Tool::Run(c, Some(_)) => Tool::Run(c, None),
            Tool::Mirror { draw, start: Some(_) } => Tool::Mirror { draw, start: None },
            Tool::Align(Some(_)) => {
                self.drawing.align_reference = None;
                Tool::Align(None)
            }
            Tool::Trim(Some(_)) => Tool::Trim(None),
            Tool::TrimTo { boundary: Some(_), many } => Tool::TrimTo { boundary: None, many },
            Tool::Dimension(mode, refs) if !refs.is_empty() => Tool::Dimension(mode, Vec::new()),
            Tool::Angular(refs) if !refs.is_empty() => Tool::Angular(Vec::new()),
            Tool::Round(k, picked) if !picked.is_empty() => Tool::Round(k, Vec::new()),
            Tool::Section(Some(_)) => Tool::Section(None),
            Tool::DetailView(Some(_)) => Tool::DetailView(None),
            Tool::Measure(Some(_)) => Tool::Measure(None),
            // The ruler begun or laid goes first, then the tool, in a plan and in the 3D view alike.
            Tool::Measure(None) if !std::mem::take(&mut self.orbit.chain).is_empty() => {
                self.orbit.chain_done = None;
                Tool::Measure(None)
            }
            Tool::Measure(None) if self.measured.take().is_some() => Tool::Measure(None),
            Tool::Stair(pts) if !pts.is_empty() => Tool::Stair(Vec::new()),
            Tool::Ramp(pts) if !pts.is_empty() => {
                self.ramp_kinds.clear();
                Tool::Ramp(Vec::new())
            }
            Tool::Railing(pts) if !pts.is_empty() => Tool::Railing(Vec::new()),
            Tool::Mass(pts) if !pts.is_empty() => Tool::Mass(Vec::new()),
            Tool::DetailLine(pts) if !pts.is_empty() => Tool::DetailLine(Vec::new()),
            Tool::Shaft(Some(_)) => Tool::Shaft(None),
            Tool::SlopeArrow(Some(_)) => Tool::SlopeArrow(None),
            Tool::AddSplitLine(Some(_)) => Tool::AddSplitLine(None),
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
        // A dimension segment picked out of a chain goes, not the whole dimension.
        if let Some((id, segment)) = self.dimension_segment.take() {
            self.dimension_segment_hover = None;
            if self.run("dimension_segment", json!({"id": id, "segment": segment})).is_some() {
                self.selection = if self.session.model.elements.get(id).is_some() { vec![id] } else { Vec::new() };
            }
            return;
        }
        // With one part of a curtain wall picked, that part, and never the wall.
        if let Some((id, at)) = self.curtain_part {
            self.delete_curtain_part(id, at);
            return;
        }
        // Under Modify Sub Elements, what of the shape is selected.
        if self.shape_delete() {
            return;
        }
        if self.sketch.is_some() {
            self.delete_sketch_lines();
            return;
        }
        self.delete_elements(self.selection.clone());
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
            Sheet::Family(_, None) => {
                if let Some(o) = self.active_family_mut().map(|ed| &mut ed.orbit)
                    && let Some(c) = o.camera
                {
                    o.camera = Some(c.zoom(factor, (f64::from(c.width) / 2.0, f64::from(c.height) / 2.0)));
                }
            }
            Sheet::Schedule(_) | Sheet::Table(_) | Sheet::Family(..) => {}
        }
    }

    fn fit(&mut self) {
        match self.sheet {
            Sheet::Plan(id) | Sheet::Ceiling(id) | Sheet::Section(id) | Sheet::Paper(id) => self.canvas.fit(id),
            Sheet::View3D => self.fit3d(),
            // A family's view, whichever is shown (`editor/zoom.rs`).
            Sheet::Family(..) => {
                if let Some(ed) = self.active_family_mut() {
                    ed.zoom_to_fit();
                }
            }
            Sheet::Schedule(_) | Sheet::Table(_) => {}
        }
    }

    /// The 3D view the 3D tab shows: one opened on it (a camera view, or `{3D}` itself), else the default `{3D}` — none
    /// while the project has neither.
    pub(crate) fn active_3d(&self) -> Option<Id> {
        let m = &self.session.model;
        self.active_3d.filter(|id| matches!(m.elements.get(id), Some(Element::View3D(_)))).or_else(|| m.default_3d())
    }

    /// Whether nothing is being made for what the window shows: no plan or 3D scene on a worker, nothing waited for.
    pub(crate) fn settled(&self) -> bool {
        !self.canvas.drawing.busy() && self.orbit.waiting.is_none() && !self.orbit.busy() && !self.opening_busy()
    }

    /// The first plan in a tab, as the Project Browser opens it.
    pub(crate) fn open_first_plan(&mut self) {
        if let Some((id, _)) = self.session.model.levels().first() {
            self.open_sheet(Sheet::Plan(*id));
        }
    }

    /// Open a 3D view in the 3D tab: the default `{3D}` (none), or a camera view by id. Its camera stands the view where
    /// it looks from the next time it is drawn.
    pub(crate) fn open_3d(&mut self, id: Option<Id>) {
        self.active_3d = id;
        self.orbit.camera = None;
        self.orbit.built = None;
        self.open_sheet(Sheet::View3D);
    }

    /// Open a view in a tab, or bring its tab forward.
    fn open_sheet(&mut self, sheet: Sheet) {
        let changed_vertical_plane = self.sheet != sheet && (matches!(self.sheet, Sheet::Section(_)) || matches!(sheet, Sheet::Section(_)));
        // A family document's view: its tab after the document's others (after the project's and the documents' before it),
        // and the document showing it. The project's tool, selection and views are left as they are.
        if let Sheet::Family(id, view) = sheet {
            if !self.views.contains(&sheet) {
                let at = self.views.iter().position(|s| matches!(s, Sheet::Family(d, _) if *d > id)).unwrap_or(self.views.len());
                self.views.insert(at, sheet);
            }
            (self.sheet, self.front) = (sheet, sheet);
            if let Some(ed) = self.family_of_mut(id) {
                ed.views.shown = view;
                // What changed while another tab was in front goes from its selection.
                ed.prune_selection();
            }
            return;
        }
        if !matches!(sheet, Sheet::Section(_)) {
            self.cut.suspend();
        }
        if !matches!(sheet, Sheet::View3D) {
            self.orbit.suspend();
            if let Some(gpu) = &mut self.card.gpu {
                gpu.suspend_upload();
            }
        }
        self.canvas.drawing.suspend();
        if self.sheet != sheet {
            self.underlay.suspend();
            // Move or Rotate was picking in the view left.
            self.underlay_edit = None;
        }
        if !self.views.contains(&sheet) && !self.floats(sheet) {
            self.views.push(sheet);
        }
        self.sheet = sheet;
        if !self.floats(sheet) {
            self.front = sheet;
        }
        // The selection is the model's, not the view's: it stays, whatever view comes up.
        self.clear_typed();
        if changed_vertical_plane
            && matches!(self.tool, Tool::Dimension(..) | Tool::Move(_) | Tool::DetailLine(_) | Tool::Trim(_) | Tool::TrimTo { .. })
        {
            self.tool = Tool::Modify;
            self.dimension_reference_hover = None;
        }
        self.snap_once = None;
        // A ruler belongs to the view it was laid in.
        self.measured = None;
        // The tools that pick points work in plans (Text in sections and on sheets too, Spot
        // Elevation in sections, Demolish in sections and 3D); any other view takes clicks to select.
        let works = match sheet {
            Sheet::Plan(_) | Sheet::Ceiling(_) => true,
            Sheet::Section(_) => {
                matches!(self.tool, Tool::Text | Tool::Spot | Tool::Demolish | Tool::CurtainGrid { .. } | Tool::CurtainSegment { .. })
            }
            Sheet::View3D => {
                matches!(self.tool, Tool::Demolish | Tool::Paint | Tool::Unpaint | Tool::CurtainGrid { .. } | Tool::CurtainSegment { .. })
            }
            Sheet::Paper(_) => self.tool == Tool::Text,
            _ => false,
        };
        // (A sketch is held in its plan: it takes the window back, with its tool, [`BimApp::track_sketch`].)
        if !works && self.tool != Tool::Level && self.sketch_plan().is_none() {
            self.tool = Tool::Modify;
        }
        match &mut self.tool {
            Tool::Wall(chain) => chain.clear(),
            Tool::Grid(start) | Tool::ReferencePlane(start) | Tool::RoomSeparator(start) | Tool::Run(_, start) => *start = None,
            Tool::Dimension(_, refs) | Tool::Angular(refs) => refs.clear(),
            Tool::Round(_, picked) => picked.clear(),
            Tool::Section(start) | Tool::Shaft(start) | Tool::Measure(start) | Tool::Camera(start) => *start = None,
            Tool::Stair(pts)
            | Tool::Ramp(pts)
            | Tool::Railing(pts)
            | Tool::ExtrusionRoof(pts)
            | Tool::Mass(pts)
            | Tool::DetailLine(pts) => pts.clear(),
            Tool::Move(_)
            | Tool::Copy(_)
            | Tool::Scale(_)
            | Tool::SlopeArrow(_)
            | Tool::ModifySubElements
            | Tool::AddPoint
            | Tool::AddSplitLine(_)
            | Tool::GableEdge => self.tool = Tool::Modify,
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
        let boards = self.panel_schedules().into_iter().map(|(id, name)| (json!(id), tf("Panel Schedule: {}", &[name]), name));
        let others = std::iter::once((json!("{3D}"), t("3D View: {3D}").to_string(), "{3D}")).chain(builtin).chain(made).chain(boards);
        plans.chain(cuts).chain(others).filter(|(_, _, name)| !placed.contains(name)).map(|(v, label, _)| (v, label)).collect()
    }

    /// The project's schedules other than those every project has: their ids and names, by name.
    pub(crate) fn schedules(&self) -> Vec<(Id, &str)> {
        let m = &self.session.model;
        let mut out: Vec<(Id, &str)> = m
            .rare("schedule")
            .into_iter()
            .filter_map(|id| match m.elements.get(id) {
                Some(Element::Schedule(sc)) if schedule::builtin(Some(&sc.name), None).is_none() => Some((id, sc.name.as_str())),
                _ => None,
            })
            .collect();
        out.sort_by_key(|(_, name)| name.to_lowercase());
        out
    }

    /// Close a view's tab; the last one stays open.
    fn close_view(&mut self, sheet: Sheet) {
        if matches!(sheet, Sheet::Family(..)) {
            self.close_family_tab(sheet);
            return;
        }
        if self.floats(sheet) {
            self.floating.retain(|f| f.sheet != sheet);
            if self.sheet == sheet {
                self.open_sheet(self.front);
            }
            self.settle_arrange();
            return;
        }
        // The project keeps a tab of its own.
        if self.views.iter().filter(|v| !matches!(v, Sheet::Family(..))).count() < 2 {
            return;
        }
        self.views.retain(|v| *v != sheet);
        if self.sheet == sheet
            && let Some(&last) = self.views.last()
        {
            self.open_sheet(last);
        }
        self.settle_arrange();
    }

    // ---------- files ----------

    /// Save the project (`.bimc`, or any other name) or export by extension: `.ifc`, the 3D scene as `.glb`, or the
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
                (Sheet::Schedule(_) | Sheet::Table(_) | Sheet::Family(..), _) => {}
            }
            return self.run("render", p).is_some();
        }
        // A project as the user keeps them (Settings > Files): zipped, or plain JSON; an export takes no such choice.
        let mut p = json!({"path": file});
        if files::is_project(path) {
            p["compress"] = json!(self.compressed_save);
        }
        let saved = self.run("save", p).is_some();
        // A template saved is listed for New, and the project's own file is still the one it saves to.
        if saved && ext == files::TEMPLATE {
            self.note_template(path);
            let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            self.status = Status { text: tf("Saved the template {}; New lists it.", &[&name]), error: false };
            return true;
        }
        // Exports (IFC, glTF, the sheets' PDF) leave the project unsaved.
        if saved && !matches!(ext.as_str(), "ifc" | "glb" | "pdf") {
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
        self.ask_path(ctx, msg!("BIMCraft project"), &files::SAVED, move |app, p| {
            // A template saved leaves the project unsaved, so what waited on the save waits on.
            if app.save_to(&p)
                && !app.dirty()
                && let Some(then) = then
            {
                app.proceed(then, &later);
            }
        });
    }

    /// Asks, in the system's file dialog, where to save, and calls `then` with the path in a later frame: the first of `exts` is
    /// what it offers and what a path with none of them gets.
    pub(crate) fn ask_path(&mut self, ctx: &Context, what: &str, exts: &[&str], then: impl FnOnce(&mut BimApp, PathBuf) + 'static) {
        let name = files::file_name(&self.session.model.name, exts.first().copied().unwrap_or_default());
        let exts: Vec<String> = exts.iter().map(|e| e.to_string()).collect();
        let dialog = rfd::AsyncFileDialog::new().add_filter(t(what), &exts).set_file_name(name);
        self.ask_file(ctx, dialog, true, move |app, p| then(app, files::named(p, &exts)));
    }

    /// Open a project, or import an IFC file and say what came in and what was left out.
    pub(crate) fn open_path(&mut self, path: &Path) -> bool {
        if revit::is_revit(path) {
            return self.open_revit(path);
        }
        // A family opens in its own document, the project left as it is.
        if files::is_family(path) {
            self.open_family_file(path);
            if self.home.shown {
                self.family_from_start_page();
            }
            return self.active_family().is_some();
        }
        self.open_project_file(path, false)
    }

    /// Open a project or an IFC file, or with `template` start a new project from one: its content with no file.
    pub(crate) fn open_project_file(&mut self, path: &Path, template: bool) -> bool {
        // A big file is read on a thread of its own, the window drawing meanwhile, with a box and Cancel (`opening.rs`). A Co-Work
        // guest's commands go to the host, so it opens as it always did.
        let big = std::fs::metadata(path).is_ok_and(|m| m.len() >= opening::IN_BACKGROUND);
        if big && !matches!(self.cowork, Some(cowork::Live::Guest(_))) {
            return self.open_in_background(path, template);
        }
        let mut p = json!({"path": path.to_string_lossy()});
        if template {
            p["as_template"] = json!(true);
        }
        let Some(reply) = self.run("open", p) else {
            self.open_failed(path);
            return false;
        };
        self.opened(path, &reply);
        true
    }

    /// What a failed open leaves: the file forgotten from the recent ones if it's gone, and its recovery copy looked at.
    pub(crate) fn open_failed(&mut self, path: &Path) {
        self.forget_recent_if_gone(path);
        // Even a damaged original can have a healthy recovery copy.
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ifc")) {
            self.inspect_recovery(path.to_path_buf());
        }
    }

    /// The window on a project just opened from `path`, `reply` being what `open` said.
    pub(crate) fn opened(&mut self, path: &Path, reply: &Value) {
        self.retire_autosave();
        self.reopened();
        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        // A template is let go once read: it is listed among the templates, not the recent projects, and the project is new.
        if reply["template"] == json!(true) {
            self.note_template(path);
            self.status = Status { text: tf("A new project from the template {}: Save asks where to put it.", &[&name]), error: false };
        } else {
            self.note_recent(path);
        }
        self.autosave_document();
        // What the open had to mend: a line pattern a file names but doesn't hold (issue 566), or a formula saying a name
        // its family lacks.
        let warnings: Vec<&str> = reply["warnings"].as_array().map_or_else(Vec::new, |a| a.iter().filter_map(Value::as_str).collect());
        if !warnings.is_empty() {
            let text = tf("{} opened with warnings:\n{}", &[&name, &warnings.join("\n")]);
            self.status = Status { text, error: false };
        } else if reply["format"].is_string() {
            self.status = Status { text: import_note(&name, reply), error: false };
        } else if let Some(left_out) = reply["left_out"].as_array() {
            let count = reply["left_out_count"].as_u64().map_or(left_out.len(), |n| n as usize);
            // A newer BIMCraft's fields: what this one doesn't know, it can't keep.
            let text =
                tf("{} opened without {} fields this version doesn't know; saving it leaves them out.", &[&name, &count.to_string()]);
            self.status = Status { text, error: false };
        }
    }

    /// The sample house, the model `bimcraft --sample` builds, as a project with no file: Save asks where to put it. Built
    /// all or nothing (a line that fails leaves the project as it was), and its steps are not undo steps, as with `--sample`.
    fn open_sample(&mut self) {
        match self.session.script_atomic(bimcraft_core::sample::SAMPLE) {
            Ok(_) => {
                let model = (*self.session.model).clone();
                self.session.reset(model, None);
                self.retire_autosave();
                self.reopened();
                self.autosave_document();
            }
            Err(e) => self.status = Status { text: e, error: true },
        }
    }

    /// Start over on the model the session holds now (after New and Open), keeping how the
    /// window is set up.
    fn reopened(&mut self) {
        let mut fresh = BimApp::new(std::mem::take(&mut self.session));
        fresh.status = std::mem::take(&mut self.status);
        fresh.os_events = self.os_events.take();
        (fresh.own_title_bar, fresh.system_title_bar) = (self.own_title_bar, self.system_title_bar);
        (fresh.thin, fresh.lang, fresh.snaps, fresh.ribbon_min) = (self.thin, self.lang, self.snaps, self.ribbon_min);
        (fresh.compact, fresh.profile, fresh.scale) = (self.compact, self.profile, self.scale);
        (fresh.tools_hidden, fresh.presets) = (std::mem::take(&mut self.tools_hidden), std::mem::take(&mut self.presets));
        fresh.active_preset = self.active_preset.take();
        (fresh.panels_hidden, fresh.panel_keys) = (std::mem::take(&mut self.panels_hidden), std::mem::take(&mut self.panel_keys));
        fresh.font_finder = self.font_finder;
        fresh.scan_contrast = self.scan_contrast;
        fresh.grid_snapping = self.grid_snapping;
        fresh.guide_style = self.guide_style;
        fresh.compressed_save = self.compressed_save;
        fresh.cut_fills_3d = self.cut_fills_3d;
        (fresh.select_by_face, fresh.drag_on_selection) = (self.select_by_face, self.drag_on_selection);
        (fresh.show_origin, fresh.show_view_name) = (self.show_origin, self.show_view_name);
        fresh.shortcuts = std::mem::take(&mut self.shortcuts);
        crate::shortcuts::publish(&fresh.shortcuts);
        (fresh.light_canvas, fresh.scheme, fresh.accent) = (self.light_canvas, self.scheme, self.accent);
        (fresh.panel_colour, fresh.canvas_colour, fresh.text_colour) = (self.panel_colour, self.canvas_colour, self.text_colour);
        fresh.styled = self.styled;
        fresh.theme_wanted = self.theme_wanted.take();
        fresh.docking = std::mem::take(&mut self.docking);
        (fresh.settings_path, fresh.settings_kept) = (self.settings_path.take(), self.settings_kept.take());
        fresh.window = std::mem::take(&mut self.window);
        (fresh.updates, fresh.revit) = (self.updates.take(), self.revit.take());
        // The assistant's network and the user's choices are the window's, not the project's: its conversation ends with the project.
        fresh.assistant = std::mem::take(&mut self.assistant);
        fresh.assistant.project_replaced();
        // The 3D mouse's reader runs for the window, not the project: without it the cap would move nothing once a project is open.
        fresh.space_mouse = self.space_mouse.take();
        fresh.control = self.control.take();
        fresh.agents = std::mem::take(&mut self.agents);
        fresh.cowork_prefs = std::mem::take(&mut self.cowork_prefs);
        fresh.cowork = fresh.cowork_kept(self.cowork.take());
        fresh.cowork_ai = std::mem::take(&mut self.cowork_ai);
        fresh.opens = std::mem::take(&mut self.opens);
        (fresh.bench, fresh.opening) = (self.bench.take(), self.opening.take());
        fresh.swapping = std::mem::take(&mut self.swapping);
        (fresh.recent, fresh.recent_path) = (std::mem::take(&mut self.recent), self.recent_path.take());
        (fresh.templates, fresh.templates_path) = (std::mem::take(&mut self.templates), self.templates_path.take());
        fresh.autosave = std::mem::take(&mut self.autosave);
        fresh.card = std::mem::take(&mut self.card);
        fresh.pyaddins = self.pyaddins.carried();
        fresh.home = self.home.put_away();
        // A new or opened project keeps the plugins and their tabs: they are the user's folder, not the project's, and the
        // session's external commands were already registered. Re-registering puts them back if the project replaced them.
        (fresh.plugins, fresh.plugin_tabs) = (std::mem::take(&mut self.plugins), std::mem::take(&mut self.plugin_tabs));
        fresh.plugins_job = self.plugins_job.take();
        fresh.plugins_dialog = self.plugins_dialog;
        fresh.plugin_panel_states = std::mem::take(&mut self.plugin_panel_states);
        fresh.plugin_forms = std::mem::take(&mut self.plugin_forms);
        fresh.plugin_form_open = self.plugin_form_open.take();
        fresh.plugins.register(&mut fresh.session);
        fresh.plugin_tabs = fresh.plugins.tabs();
        // A new window starts with no plugin palettes, but this one keeps them: they are the user's folder, as the tabs are.
        crate::dock::set_plugin_palettes(&fresh.plugins.panels());
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
        let mut dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        // Over the Assistant palette, a file is for the message.
        self.drop_on_assistant(ctx, &mut dropped);
        self.opens.extend(dropped);
        // A Revit file being read holds the files that come meanwhile, which are opened in turn once it is over: the last one wins.
        if !self.importing()
            && let Some(path) = self.opens.pop()
        {
            self.opens.clear();
            // A family replaces nothing, so there is nothing to ask about first.
            if files::is_family(&path) {
                self.open_path(&path);
            } else {
                self.guard(Pending::OpenPath(path), ctx);
            }
        }
    }

    /// Do `p` now, or first ask about unsaved changes.
    fn guard(&mut self, p: Pending, ctx: &Context) {
        // The family documents first, each asked about while it has unsaved changes; then the project.
        if !self.families.is_empty() {
            self.pending_after_documents(p);
            return;
        }
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
            Pending::Template(path) => {
                self.open_template(&path);
            }
            Pending::Sample => self.open_sample(),
            Pending::OpenPath(path) => {
                self.open_path(&path);
            }
            Pending::Open => {
                let revit = self.revit.is_some();
                let name = t(if revit { msg!("BIMCraft project, IFC or Revit") } else { "BIMCraft project or IFC" });
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
                // On a family tab, the family document's Save, as Revit saves the active document: never the project.
                if icon_button(ui, Icon::Save, "Save").clicked() {
                    if self.active_family().is_some() {
                        self.family_save(false);
                    } else {
                        self.save(&ctx, None);
                    }
                }
                ui.separator();
                ui.spacing_mut().item_spacing.x = 0.0;
                // A family document's Undo and Redo are its own: the project's steps are not offered while it is open.
                let family = self.family_doc();
                if icon_button(ui, Icon::Undo, "Undo").clicked() {
                    if family {
                        self.family_undo();
                    } else {
                        self.run("undo", json!({}));
                    }
                }
                if !family {
                    self.steps_list(ui, false);
                }
                ui.add_space(4.0);
                if icon_button(ui, Icon::Redo, "Redo").clicked() {
                    if family {
                        self.family_redo();
                    } else {
                        self.run("redo", json!({}));
                    }
                }
                if !family {
                    self.steps_list(ui, true);
                }
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.separator();
                if icon_button(ui, Icon::View3D, "Default 3D View").clicked() {
                    self.open_3d(None);
                }
                ui.separator();
                if icon_button(ui, Icon::Settings, "Settings").clicked() {
                    self.settings_page = Some(settings::Page::Interface);
                }
                // Close beside the gear, and the Find field narrower for it: the title's room stays as it was.
                ui.add_space(-6.0);
                self.agents_toggle(ui);
                // Find and the project's scale, in both profiles: BIMCraft's Find a field's width, Rev's an icon among the others.
                ui.separator();
                let find = if style::rev() { icon_button(ui, Icon::Search, "Find") } else { find::button(ui) };
                if find.clicked() {
                    self.find = Some(String::new());
                }
                self.scale_chip(ui);
            })
            .response
            .rect;
        // The project and the view, centred where Revit's title bar has them: in the window's middle when they fit between the
        // buttons on the left and, where the window draws its own title bar, the caption buttons on the right; else as near it
        // as that room allows, and cut short where even it is too little.
        let gap = 12.0;
        // Which palettes show, in both profiles, at the bar's right end where an editor keeps its layout's buttons: a closed
        // palette comes back from here, the Tools palette too, which has no shortcut. The title's middle stays the window's.
        let end = ui.max_rect().right() + if self.own_bar() { 8.0 - 3.0 * titlebar::BUTTON_WIDTH } else { 0.0 } - 4.0;
        let at = Rect::from_min_size(pos2(end - 26.0, row.center().y - 12.0), vec2(26.0, 24.0));
        let palettes = ui.put(at, |ui: &mut Ui| icon_button(ui, Icon::Interface, msg!("Palettes")));
        egui::Popup::menu(&palettes).show(|ui| self.palette_switches(ui));
        let right = at.left() - gap;
        let left = row.right() + gap;
        let mut title = egui::text::LayoutJob::simple_singleline(
            self.family_title().unwrap_or_else(|| self.bar_title(view, (right - left).max(0.0), ui.ctx())),
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
        // In a Co-Work session, what this person's undo takes back: their own steps (a guest has them from the host).
        let names: Vec<String> = match &self.cowork {
            Some(cowork::Live::Guest(j)) => j.history(redo).iter().map(|s| named(&s.label, &s.made, &s.removed)).collect(),
            Some(cowork::Live::Host(h)) => self.session.own_steps(h.name(), redo).into_iter().map(step_name).collect(),
            None => self.session.steps(redo).into_iter().map(step_name).collect(),
        };
        let (rect, arrow) = ui.allocate_exact_size(vec2(12.0, 22.0), Sense::click());
        let label = t(if redo { msg!("Redo History") } else { msg!("Undo History") });
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
                let foot = tf(if redo { msg!("Redo {} steps") } else { msg!("Undo {} steps") }, &[&n.to_string()]);
                let foot = if n == 1 { tf(if redo { msg!("Redo {}") } else { msg!("Undo {}") }, &[&names[0]]) } else { foot };
                ui.label(RichText::new(foot).color(pal().muted));
            });
        });
        if let Some(count) = chosen {
            self.run(&what.to_lowercase(), json!({"count": count}));
        }
    }

    fn ribbon(&mut self, ui: &mut Ui) {
        Frame::new().inner_margin(Margin::symmetric(6, 5)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                // The tabs, scrolling sideways (with the wheel) in a window too narrow for them; the fold button at the end.
                let room = ui.available_width() - 32.0;
                // The tab that has just been picked (or a contextual tab that has just come) is brought into view: the row
                // scrolls in a window too narrow for it, and its last tabs are out of sight.
                let seen = UiId::new("ribbon-tab-seen");
                let now = tab_key(self.tab);
                let changed = ui.ctx().data(|d| d.get_temp::<u64>(seen)) != Some(now);
                ui.ctx().data_mut(|d| d.insert_temp(seen, now));
                let plugin_tabs: Vec<String> = self.plugin_tabs.iter().map(|p| self.plugin_tab_name(p)).collect();
                ScrollArea::horizontal()
                    .id_salt("ribbon-tabs")
                    .max_width(room)
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            for (tab, label, _) in TABS {
                                let r = tab_button(ui, label, self.tab == tab, false);
                                if changed && self.tab == tab {
                                    r.scroll_to_me(None);
                                }
                                self.ribbon_tab(&r, tab);
                            }
                            // The plugins' tabs come after BIMCraft's, each as its manifest declared it.
                            for (i, label) in plugin_tabs.iter().enumerate() {
                                let tab = Tab::Plugin(i);
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
                    let (icon, label) = if self.ribbon_min {
                        (Icon::Expand, msg!("Expand the Ribbon"))
                    } else {
                        (Icon::Minimize, msg!("Minimize the Ribbon"))
                    };
                    if icon_button(ui, icon, label).clicked() {
                        self.fold_ribbon(!self.ribbon_min);
                    }
                    // Edit Tools on: the ribbon's buttons are boxes to tick until this, which keeps the layout in the selected preset.
                    if self.editing_tools
                        && ui.button(t("Done")).on_hover_text(t("Done keeps the tools, panels and layout in the selected preset")).clicked()
                    {
                        self.edit_tools(false);
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

    /// The panels of `tab`: on the ribbon side by side, in the Tools palette as rows.
    fn tab_content(&mut self, ui: &mut Ui, tab: Tab) {
        // The panels' keys: `tab:<tab>/<panel>`, a plugin's `tab:plugin:<name>/<panel>`, a contextual tab's `tab:Modify/…`.
        let name = self.tab_name(tab);
        ribbon::set_tab(&name);
        ribbon::begin_tab();
        match tab {
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
            Tab::Collaborate => self.collaborate_tab(ui),
            Tab::Manage => self.manage_tab(ui),
            Tab::PyAddins => self.pyaddins_tab(ui),
            Tab::Modify | Tab::Context => self.modify_tab(ui),
            Tab::Plugin(i) => self.plugin_tab(ui, i),
        }
        if ribbon::drawn_panels() == 0 && ribbon::hidden_panels() > 0 {
            self.empty_tab_notice(ui, &name);
        }
    }

    /// The name a tab's panels' keys carry: its English name, `plugin:<name>` for a plugin's tab, `Modify` for the contextual
    /// tab, which the same panels draw.
    fn tab_name(&self, tab: Tab) -> String {
        match tab {
            // The manifest's name, never translated: a key must read the same in every language.
            Tab::Plugin(i) => {
                let name = self.plugin_tabs.get(i).and_then(|t| t.name.clone()).unwrap_or_else(|| "Plugins".to_string());
                format!("plugin:{name}")
            }
            Tab::Modify | Tab::Context => "Modify".to_string(),
            tab => TABS.iter().find(|(t, ..)| *t == tab).map_or("", |(_, label, _)| *label).to_string(),
        }
    }

    /// The panel keys the preset editor lists: every built-in tab's panels, whether it has been opened or not, and every plugin
    /// tab's declared panels. A tool's or a script's own panels join them from what the window has drawn (`panel_keys`).
    pub(crate) fn known_panel_keys(&self) -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        for (tab, panels) in PANELS {
            let name = self.tab_name(tab);
            for panel in panels {
                keys.insert(format!("tab:{name}/{panel}"));
            }
        }
        for tab in &self.plugin_tabs {
            let name = tab.name.clone().unwrap_or_else(|| "Plugins".to_string());
            for panel in &tab.panels {
                keys.insert(format!("tab:plugin:{name}/{}", panel.name));
            }
        }
        keys
    }

    /// BIMCraft's Tools palette, in place of the ribbon: a rail of the ribbon's tabs down its left, and beside it, as rows under their
    /// panels' names, the Select panel, the tool in hand's or the selection's (the contextual tab's, named for it) and the open tab's.
    pub(crate) fn tool_column(&mut self, ui: &mut Ui) {
        let open = if self.tab == Tab::Context { self.back } else { self.tab };
        // The Select panel belongs to the open tab: its key, and the tab's panels' keys, follow it.
        ribbon::set_tab(&self.tab_name(open));
        // The rail and the rows side by side, each in a rect of its own: what is wider than the rows' is cut, never widens the column.
        let all = ui.available_rect_before_wrap();
        let rail = all.with_max_x(all.left() + 36.0).shrink2(vec2(2.0, 4.0));
        let rows = all.with_min_x(all.left() + 40.0);
        ui.painter().vline(rows.left() - 2.0, all.y_range().shrink(6.0), Stroke::new(1.0, pal().border));
        ui.scope_builder(UiBuilder::new().max_rect(rail), |ui| {
            // A palette too short for every tab scrolls them, as the rows beside them scroll theirs.
            ScrollArea::vertical().id_salt("tool-rail").auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (tab, label, icon) in TABS {
                    if rail_button(ui, icon, label, tab == open).clicked() {
                        self.tab = tab;
                    }
                }
                // The plugins' tabs come after BIMCraft's, scrolling with them.
                let plugin_tabs: Vec<String> = self.plugin_tabs.iter().map(|p| self.plugin_tab_name(p)).collect();
                for (i, label) in plugin_tabs.iter().enumerate() {
                    if rail_button(ui, Icon::Code, label, Tab::Plugin(i) == open).clicked() {
                        self.tab = Tab::Plugin(i);
                    }
                }
            });
        });
        ui.scope_builder(UiBuilder::new().max_rect(rows), |ui| {
            // Edit Profile can be reached near the bottom of this column. Its draft opens with the editing tools visible.
            let scroll = if self.editing_roof_profile() { "roof-profile-tools" } else { "tool-column" };
            ScrollArea::both().id_salt(scroll).auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(rows.width() - 12.0);
                let edit = if self.editing_tools { msg!("Done") } else { msg!("Edit Tools") };
                let tip = msg!(
                    "Tick the tools and panels this window shows; Done keeps them, and the palette layout, in the selected preset; Find still has the others"
                );
                if ui.button(t(edit)).on_hover_text(t(tip)).clicked() {
                    self.edit_tools(!self.editing_tools);
                }
                ribbon::rows(ui, |ui| self.select_panel(ui));
                ribbon::skip_select(true);
                let context = self.context();
                if let Some(name) = &context {
                    ui.add_space(8.0);
                    ui.label(RichText::new(name).color(pal().context).strong());
                    ribbon::set_tab("Modify");
                    ribbon::rows(ui, |ui| self.modify_tab(ui));
                }
                // The Modify tab is what the contextual rows already are.
                if open != Tab::Modify || context.is_none() {
                    ui.add_space(8.0);
                    // Painted: the rail's button already names it for screen readers.
                    let (label, plugin) = match open {
                        Tab::Plugin(i) => (self.plugin_tabs.get(i).map(|t| self.plugin_tab_name(t)).unwrap_or_default(), true),
                        tab => (TABS.iter().find(|(t, ..)| *t == tab).map_or("", |(_, label, _)| *label).to_string(), false),
                    };
                    let label = if plugin { label } else { t(&label).to_string() };
                    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
                    ui.painter().text(r.left_center(), Align2::LEFT_CENTER, label, style::semibold(13.0), pal().text);
                    ribbon::rows(ui, |ui| self.tab_content(ui, open));
                }
                ribbon::skip_select(false);
            });
        });
    }

    /// The ribbon tab's panels of tools.
    fn ribbon_panels(&mut self, ui: &mut Ui) {
        Frame::new().inner_margin(Margin::symmetric(6, 4)).show(ui, |ui| {
            // A tab wider than the window folds its last panels; one still too wide scrolls sideways, each tab where it was
            // left.
            let key = UiId::new(("ribbon-widths", tab_key(self.tab)));
            let room = ui.available_width();
            egui::ScrollArea::horizontal().id_salt(tab_key(self.tab)).auto_shrink([false, true]).show(ui, |ui| {
                ribbon::tab(ui, key, room, |ui| {
                    ui.horizontal(|ui| self.tab_content(ui, self.tab));
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
        // As Revit's: New and Open, each of a project or a family.
        SubMenuButton::new(t("New")).ui(ui, |ui| {
            if item(ui, msg!("Project"), cmd(Key::N)) {
                self.new_project();
            }
            if item(ui, msg!("Family…"), None) {
                self.new_family_dialog = Some(bimcraft_core::family::Category::Generic);
            }
        });
        SubMenuButton::new(t("Open")).ui(ui, |ui| {
            if item(ui, msg!("Project…"), cmd(Key::O)) {
                self.guard(Pending::Open, ctx);
            }
            if item(ui, msg!("Family…"), None) {
                self.open_family(ctx);
            }
        });
        self.recent_menu(ui, ctx);
        // On a family tab, Save and Save As are the family document's, and say so: the project is never saved from there.
        let save_as = Some(KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::S));
        if self.active_family().is_some() {
            if item(ui, msg!("Save Family"), cmd(Key::S)) {
                self.family_save(false);
            }
            if item(ui, msg!("Save Family As…"), save_as) {
                self.family_save(true);
            }
        } else {
            if item(ui, "Save", cmd(Key::S)) {
                self.save(ctx, None);
            }
            // As Revit's: Save As a project, or as a template New starts projects from.
            SubMenuButton::new(t("Save As")).ui(ui, |ui| {
                if item(ui, msg!("Project…"), save_as) {
                    self.save_as(ctx, None);
                }
                if item(ui, msg!("Template…"), None) {
                    self.save_as_template(ctx);
                }
            });
            // As in Settings > Files: kept for every save after.
            ui.checkbox(&mut self.compressed_save, t("Compressed save"))
                .on_hover_text(t("⚠ A compressed project is much smaller, but saving and opening it take longer."));
        }
        if !self.autosave.candidates.is_empty() && item(ui, msg!("Recover Autosave…"), None) {
            self.autosave.show_recovery = true;
        }
        SubMenuButton::new(t("Export")).ui(ui, |ui| {
            let shortcut = ctx.format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::P));
            if ui.add(egui::Button::new(t("Export…")).shortcut_text(shortcut)).clicked() {
                self.open_export(None);
            }
            if ui.button(t("Publisher…")).clicked() {
                self.open_publisher();
            }
            if ui.button(t("Print Set…")).clicked() {
                self.open_print_sets(None);
            }
        });
        self.report_menu(ui);
        self.update_menu(ui);
        ui.separator();
        if item(ui, msg!("Settings…"), Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma))) {
            self.settings_page = Some(settings::Page::Interface);
        }
        ui.separator();
        if item(ui, msg!("Exit"), None) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    /// Ribbon buttons that pick tools, under the panel's name.
    /// The Select panel each tab starts with: Modify, and Revit's Filter while something is selected, to keep some of it by
    /// category or by the values of a parameter they all have.
    fn select_panel(&mut self, ui: &mut Ui) {
        // The Tools palette draws it once, over the tabs it shows.
        if ribbon::skipping_select() {
            return;
        }
        panel(ui, msg!("Select"), |ui| {
            if big_button(ui, Icon::Modify, "Modify", structure::same_tool(&self.tool, &Tool::Modify), msg!("Modify (MD): select elements"))
                .clicked()
            {
                self.set_tool(Tool::Modify);
            }
            if !self.selection.is_empty() {
                let tip = msg!("Filter: keep some of the selection, by category or by the values of a parameter they all have");
                if big_button(ui, Icon::Filter, "Filter", self.selection_filter.is_some(), tip).clicked() {
                    self.open_filter();
                }
            }
        });
        // Edit Group's Add, Remove, Finish and Cancel, beside Select on every tab while a group is edited.
        self.edit_group_panel(ui);
    }

    fn tools(&mut self, ui: &mut Ui, name: &str, tools: Tools) {
        panel(ui, name, |ui| {
            for (icon, label, tip, tool) in tools {
                // In the panel's list and the palette only: the ribbon has no width to spare for it, and the Wall tool's Type
                // Selector offers the same types.
                if *label == CURTAIN_TOOL && !ribbon::listing() {
                    continue;
                }
                // Wall and Curtain Wall are one tool, told apart by the type it draws.
                let curtain_type =
                    self.types.get("wall").is_some_and(|n| self.session.model.types.wall.get(n).is_some_and(|t| t.curtain.is_some()));
                let wall = matches!(tool, Tool::Wall(_));
                let active = structure::same_tool(&self.tool, tool) && (!wall || curtain_type == (*label == CURTAIN_TOOL));
                if big_button(ui, *icon, label, active, tip).clicked() {
                    if wall && curtain_type != (*label == CURTAIN_TOOL) {
                        let m = &self.session.model;
                        let name = if *label == CURTAIN_TOOL {
                            m.types.wall.iter().find(|(_, t)| t.curtain.is_some()).map(|(n, _)| n.clone())
                        } else {
                            m.default_type("wall").map(String::from)
                        };
                        if let Some(name) = name {
                            self.types.insert("wall", name);
                        }
                    }
                    self.set_tool(tool.clone());
                }
            }
        });
    }

    fn massing_tab(&mut self, ui: &mut Ui) {
        let tip = msg!("Mass (MS): click the corners of the footprint; Enter, or the first corner again, finishes; Esc cancels");
        let shown = !self.graphics_now().resolved(&self.session.model).categories.get("mass").is_some_and(|o| o.hidden);
        panel(ui, msg!("Conceptual Mass"), |ui| {
            if big_button(ui, Icon::Mass, "Mass", structure::same_tool(&self.tool, &Tool::Mass(Vec::new())), tip).clicked() {
                self.set_tool(Tool::Mass(Vec::new()));
            }
            let tip = msg!("Show Mass (MH): hides the masses in this view, or shows them again");
            if big_button(ui, Icon::Visibility, "Show Mass", shown, tip).clicked() {
                self.show_mass();
            }
        });
        let selected = !self.selected_masses().is_empty();
        panel(ui, msg!("Model by Face"), |ui| {
            ui.add_enabled_ui(selected, |ui| {
                for (icon, label, tip, command) in [
                    (Icon::Floor, "Mass Floors", msg!("Mass Floors (MF): a floor at each level the selected masses cross"), "mass_floors"),
                    (
                        Icon::Wall,
                        "Wall by Face",
                        msg!("Wall by Face (WB): a wall on each edge of the selected masses' footprints"),
                        "wall_by_face",
                    ),
                    (Icon::Floor, "Floor by Face", msg!("Floor by Face (FB): a floor on the selected masses' top faces"), "floor_by_face"),
                    (Icon::Roof, "Roof by Face", msg!("Roof by Face (RB): a roof on the selected masses' top faces"), "roof_by_face"),
                    (
                        Icon::Align,
                        "Update to Face",
                        msg!("Update to Face (UF): the walls, floors and roofs made by face from the selected masses follow them again"),
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
        self.select_panel(ui);
        self.tools(
            ui,
            msg!("Build"),
            &[
                (Icon::Wall, "Wall", msg!("Wall (WA): each click builds a wall; Enter or Esc ends the chain"), Tool::Wall(Vec::new())),
                (
                    Icon::Wall,
                    CURTAIN_TOOL,
                    msg!("Curtain Wall: a wall divided by its type's pattern into panels and mullions"),
                    Tool::Wall(Vec::new()),
                ),
                (Icon::Door, "Door", msg!("Door (DR): click a wall"), Tool::Door),
                (Icon::Window, "Window", msg!("Window (WN): click a wall"), Tool::Window),
                (Icon::Column, "Column", msg!("Column (CL): click where it stands"), Tool::Column),
                (Icon::Component, "Component", msg!("Component (CM): a family's type, where you click; Space turns it"), Tool::Component),
                (Icon::Roof, "Roof", msg!("Roof by Footprint: click inside the walls to roof them, on the level above"), Tool::Roof),
                (
                    Icon::ExtrusionRoof,
                    msg!("Roof by Extrusion"),
                    msg!(
                        "Roof by Extrusion: click a line's start and end, then how far across it's swept; its profile rises from the line"
                    ),
                    Tool::ExtrusionRoof(Vec::new()),
                ),
                (Icon::Floor, "Floor", msg!("Floor: click inside the walls to floor them"), Tool::Floor),
                (Icon::Ceiling, "Ceiling", msg!("Ceiling: click inside a room for a ceiling out to its walls"), Tool::Ceiling),
            ],
        );
        self.tools(
            ui,
            msg!("Circulation"),
            &[
                (
                    Icon::Stair,
                    "Stair",
                    msg!("Stair: click where the stair starts, then where each flight or landing ends"),
                    Tool::Stair(Vec::new()),
                ),
                (
                    Icon::Ramp,
                    "Ramp",
                    msg!("Ramp: click where it starts, then where its run ends; a landing turns it"),
                    Tool::Ramp(Vec::new()),
                ),
                (
                    Icon::Railing,
                    "Railing",
                    msg!("Railing: click a stair or a ramp for one down each side, or click a path and press Enter"),
                    Tool::Railing(Vec::new()),
                ),
            ],
        );
        self.tools(
            ui,
            msg!("Room & Area"),
            &[
                (Icon::Room, "Room", msg!("Room (RM): click inside walls"), Tool::Room),
                (
                    Icon::RoomSeparator,
                    msg!("Room Separator"),
                    msg!("Room Separator: lines that bound rooms as walls do, to split an open space; click from point to point"),
                    Tool::RoomSeparator(None),
                ),
            ],
        );
        self.tools(
            ui,
            msg!("Opening"),
            &[
                (
                    Icon::Shaft,
                    msg!("Shaft"),
                    msg!("Shaft Opening: click two opposite corners; it cuts the floors, ceilings and roofs it passes through"),
                    Tool::Shaft(None),
                ),
                (
                    Icon::Dormer,
                    "Dormer",
                    msg!("Dormer: click a roof's slope where its front wall goes; the roof opens under it"),
                    Tool::Dormer,
                ),
            ],
        );
        // Datum, and the Work Plane's Set and Ref Plane beside its levels and grids, as Revit's Work Plane panel has them.
        let datum: Tools = &[
            (Icon::Level, "Level", msg!("Level (LL): a new level at an elevation"), Tool::Level),
            (Icon::Grid, "Grid", msg!("Grid (GR): click its two ends"), Tool::Grid(None)),
        ];
        panel(ui, msg!("Datum"), |ui| {
            for (icon, label, tip, tool) in datum {
                let active = structure::same_tool(&self.tool, tool);
                if big_button(ui, *icon, label, active, tip).clicked() {
                    self.set_tool(tool.clone());
                }
            }
            self.work_plane_set(ui);
            let plane = Tool::ReferencePlane(None);
            let tip = msg!("Reference Plane (RP): click its two ends; work plane-based components are placed on it and move with it");
            if big_button(ui, Icon::RefPlane, msg!("Ref Plane"), structure::same_tool(&self.tool, &plane), tip).clicked() {
                self.set_tool(plane);
            }
        });
    }

    /// Annotate > Dimension, as Revit's: Aligned and Linear big, the others small beside them, in two rows.
    fn dimension_panel(&mut self, ui: &mut Ui) {
        let big: [(Icon, &str, &str, Tool); 2] = [
            (
                Icon::Dimension,
                msg!("Aligned"),
                msg!(
                    "Aligned Dimension (DI): pick parallel wall faces, wall ends, doors, windows or grids, or endpoints, then click \
                     where the dimension line goes"
                ),
                Tool::Dimension(DimMode::Aligned, Vec::new()),
            ),
            (
                Icon::Linear,
                msg!("Linear"),
                msg!(
                    "Linear Dimension: pick endpoints (wall ends and corners, line ends, grid ends), then click where it goes: \
                     horizontal or vertical as the cursor lies"
                ),
                Tool::Dimension(DimMode::Auto, Vec::new()),
            ),
        ];
        let small: [(Icon, &str, &str, Tool); 7] = [
            (
                Icon::Angular,
                msg!("Angular"),
                msg!("Angular Dimension: pick two lines that meet, then click where the arc goes"),
                Tool::Angular(Vec::new()),
            ),
            (
                Icon::Radial,
                msg!("Radial"),
                msg!("Radial Dimension: pick a round column, a curved wall, a circle or an arc, then click where the radius goes"),
                Tool::Round(DimKind::Radial, Vec::new()),
            ),
            (
                Icon::Diameter,
                msg!("Diameter"),
                msg!("Diameter Dimension: pick a round column, a curved wall, a circle or an arc, then click where it goes"),
                Tool::Round(DimKind::Diameter, Vec::new()),
            ),
            (
                Icon::ArcLength,
                msg!("Arc Length"),
                msg!("Arc Length Dimension: pick a curved wall or an arc, then click where the dimension's arc goes"),
                Tool::Round(DimKind::ArcLength, Vec::new()),
            ),
            (
                Icon::Spot,
                "Spot Elevation",
                msg!("Spot Elevation (EL): click a floor, a roof or a stair; in a section, a point"),
                Tool::Spot,
            ),
            (
                Icon::SpotSlope,
                msg!("Spot Slope"),
                msg!("Spot Slope: click a floor or a roof in a plan; an arrow points downhill with how steeply its top falls"),
                Tool::SpotSlope,
            ),
            (
                Icon::SpotCoordinate,
                "Spot Coordinate",
                msg!(
                    "Spot Coordinate (PC): click a point in a plan, then where its leader ends; it reads north, east and the elevation as its type says"
                ),
                Tool::SpotCoordinate(None),
            ),
        ];
        panel(ui, "Dimension", |ui| {
            for (icon, label, tip, tool) in &big {
                if big_button(ui, *icon, label, structure::same_tool(&self.tool, tool), tip).clicked() {
                    self.set_tool(tool.clone());
                }
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for row in small.chunks(4) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 1.0;
                        for (icon, label, tip, tool) in row {
                            if small_button(ui, *icon, label, structure::same_tool(&self.tool, tool), tip).clicked() {
                                self.set_tool(tool.clone());
                            }
                        }
                    });
                }
            });
        });
    }

    fn annotate_tab(&mut self, ui: &mut Ui) {
        self.select_panel(ui);
        self.dimension_panel(ui);
        self.detail_panel(ui);
        self.region_panel(ui);
        self.tools(
            ui,
            "Text",
            &[(Icon::Text, "Text", msg!("Text (TX): type the note in the Options Bar, then click where it goes"), Tool::Text)],
        );
        self.tools(
            ui,
            "Measure",
            &[(
                Icon::Measure,
                "Measure",
                msg!("Measure (ME): click two points; their length and angle read beside the ruler between them. In the 3D view, any two points of the model, whatever the work plane"),
                Tool::Measure(None),
            )],
        );
        panel(ui, "Tag", |ui| {
            let tip =
                msg!("Tag by Category (TG): click an element to tag it with its type, a door's or window's mark, or the field in Label");
            if big_button(ui, Icon::Tag, "Tag by Category", self.tool == Tool::Tag, tip).clicked() {
                self.set_tool(Tool::Tag);
            }
            if big_button(ui, Icon::TagAll, "Tag All", false, "Tag every door and window in this view that has no tag").clicked() {
                self.tag_all();
            }
        });
    }

    /// A Callout of the fragment from `a` to `b` on `parent`: its view, and the boundary and mark on the parent.
    pub(crate) fn make_callout(&mut self, parent: Id, a: P2, b: P2) -> bool {
        let Some(scale) = self.drawing.detail_scale.trim().parse::<f64>().ok().filter(|n| (1.0..=50.0).contains(n)) else {
            self.status =
                Status { text: tf("callout: `{}` is not a scale from 1 to 50", &[self.drawing.detail_scale.trim()]), error: true };
            return false;
        };
        let (lo, hi) = (P2(a.0.min(b.0), a.1.min(b.1)), P2(a.0.max(b.0), a.1.max(b.1)));
        let mut p = json!({"parent": parent, "region": [[lo.0, lo.1], [hi.0, hi.1]], "scale": scale});
        if !self.drawing.detail_name.trim().is_empty() {
            p["name"] = json!(self.drawing.detail_name.trim());
        }
        if !self.drawing.detail_number.trim().is_empty() {
            p["number"] = json!(self.drawing.detail_number.trim());
        }
        match self.run("detail_view", p) {
            Some(made) => {
                let name = made["name"].as_str().unwrap_or_default().to_string();
                self.status = Status {
                    text: tf("Callout `{}` made: open it from the Project Browser, or double-click its mark", &[&name]),
                    error: false,
                };
                true
            }
            None => false,
        }
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
            let tip = msg!("Visibility/Graphics (VV): what this view hides, and how it draws the rest, by category, filter and element");
            let on = self.graphic_view().is_some();
            if ui
                .add_enabled_ui(on, |ui| big_button(ui, Icon::Visibility, "Visibility/Graphics", self.vg_dialog.is_some(), tip))
                .inner
                .clicked()
            {
                self.open_graphics(0);
            }
            let tip = msg!("Filters (FT): rules that pick out elements for a view to hide or draw another way");
            if big_button(ui, Icon::Filter, "Filters", self.filters_dialog.is_some(), tip).clicked() {
                self.filters_dialog = Some(graphics::FiltersDialog::default());
            }
            let tip = msg!("View Templates (VT): a view's settings under a name, for other views to follow");
            if big_button(ui, Icon::Template, "View Templates", self.templates_dialog.is_some(), tip).clicked() {
                self.open_templates();
            }
            if small_button(ui, Icon::ThinLines, "Thin Lines", self.thin, "Thin Lines (TL): every line one pixel wide").clicked() {
                self.thin = !self.thin;
            }
            let dark = style::dark();
            let tip = msg!("Choose a theme, including a dark interface with a light canvas");
            let r = small_button(ui, Icon::Theme, "Dark Mode", dark, tip);
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
            let tip = msg!("Crop View: show only the crop region; drag its edges by their grips");
            if ui.add_enabled_ui(view.is_some(), |ui| big_button(ui, Icon::Crop, "Crop View", cropped, tip)).inner.clicked()
                && let Some(id) = view
            {
                self.run("crop", if cropped { json!({"view": id, "region": null}) } else { json!({"view": id}) });
            }
        });
        panel(ui, "Create", |ui| {
            if big_button(ui, Icon::View3D, "3D View", self.sheet == Sheet::View3D, "Open the default 3D view").clicked() {
                self.open_3d(None);
            }
            let tip = msg!(
                "Duplicate View (DP): a new plan of this level with its own annotation; with detailing, it starts with this plan's; as a dependent, it follows this plan's range and graphics"
            );
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
            let tip = msg!(
                "Callout: click two opposite corners of a fragment on a plan, a section or an elevation; a larger view of it is made at the Options Bar's scale, and its callout and boundary stay on this view"
            );
            let can_callout = match self.sheet {
                Sheet::Plan(id) | Sheet::Section(id) => !matches!(self.session.model.elements.get(id), Some(Element::DetailView(_))),
                _ => false,
            };
            if ui
                .add_enabled_ui(can_callout, |ui| {
                    big_button(ui, Icon::DetailView, "Callout", matches!(self.tool, Tool::DetailView(_)), tip)
                })
                .inner
                .clicked()
            {
                self.set_tool(Tool::DetailView(None));
            }
            let tip = msg!("Section (CR): click its start, then its end; it looks to the left of the line");
            if big_button(ui, Icon::Section, "Section", matches!(self.tool, Tool::Section(_)), tip).clicked() {
                self.set_tool(Tool::Section(None));
            }
            let tip = msg!("Elevation: click a wall, a line or a slab's edge to look at, the project's or a link's");
            if big_button(ui, Icon::Elevation, "Elevation", self.tool == Tool::Elevation, tip).clicked() {
                self.set_tool(Tool::Elevation);
            }
            let tip = msg!("Camera: click where its eye stands, then what it looks at: a 3D view in perspective from there");
            if big_button(ui, Icon::Camera, "Camera", matches!(self.tool, Tool::Camera(_)), tip).clicked() {
                self.set_tool(Tool::Camera(None));
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
                let made: Vec<(Id, String)> =
                    self.schedules().into_iter().chain(self.panel_schedules()).map(|(id, n)| (id, n.to_string())).collect();
                for (id, name) in made {
                    if ui.button(name).clicked() {
                        self.open_sheet(Sheet::Table(id));
                    }
                }
            });
        });
        panel(ui, msg!("Sheet Composition"), |ui| {
            if big_button(ui, Icon::Sheet, "Sheet", false, "A new sheet, with a title block").clicked()
                && let Some(id) = self.run("sheet", json!({})).and_then(|made| made["id"].as_u64())
            {
                self.open_sheet(Sheet::Paper(id));
            }
            let tip = msg!("Title Blocks (TB): your own, read from a DXF or typed, with fields each sheet or the whole project fills in");
            if big_button(ui, Icon::Sheet, "Title Blocks", self.title_blocks_dialog.is_some(), tip).clicked() {
                self.open_title_blocks(None);
            }
            let sheet = match self.sheet {
                Sheet::Paper(id) => Some(id),
                _ => None,
            };
            let tip = msg!("Put a view on this sheet: one not on a sheet yet");
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
                self.arrange = dock::Arrange::Tabs;
            }
            let tiled = self.arrange == dock::Arrange::Tile;
            let tip = msg!("Tile Views: every open view in an equal cell, four of them in the four corners");
            let tile = ui.add_enabled_ui(self.project_views() >= 2, |ui| big_button(ui, Icon::Tile, "Tile Views", tiled, tip));
            if tile.inner.clicked() {
                self.tile_views();
            }
            let float_or_split = !self.floating.is_empty() || self.arrange != dock::Arrange::Tabs;
            let tip = msg!("Dock All Views: every floating view back onto the tab bar, in one view under the tabs");
            let dock = ui.add_enabled_ui(float_or_split, |ui| big_button(ui, Icon::DockAll, "Dock All Views", false, tip));
            if dock.inner.clicked() {
                self.dock_all_views();
            }
            let tip = msg!("Show or hide the palettes, and pick the interface's language");
            let r = big_button(ui, Icon::Interface, "User Interface", false, tip);
            ribbon::choices(ui, &r, false, |ui| {
                for p in dock::Palette::known() {
                    let mut on = self.docking.shown(p);
                    if ui.checkbox(&mut on, p.title()).changed() {
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
            // Last, so a narrow window hands it to the panel's list before User Interface (File > Export has it too).
            if big_button(ui, Icon::Sheet, "Publisher", self.publisher.is_some(), "Open publishing sets and export their print sets")
                .clicked()
            {
                self.open_publisher();
            }
        });
    }

    /// Manage: the project's settings, as Revit's tab has them.
    /// Insert: what comes into the project from other files, as Revit's tab. Link: other projects, and laser scans.
    fn insert_tab(&mut self, ui: &mut Ui) {
        panel(ui, msg!("Link"), |ui| {
            let tip = msg!(
                "Link Model (LK): another project, a BIMCraft file or an IFC, shown in this one as a reference that can't be edited: halftone in the plans, the sections and the 3D view, and snapped to"
            );
            if big_button(ui, Icon::Link, "Link Model", false, tip).clicked() {
                self.link_dialog = true;
            }
            let tip = msg!(
                "Point Cloud: laser scans (PTS, XYZ, PLY, LAS or E57) brought into the project, one file or a survey's stations as one scan, drawn in the 3D view, plans and sections and snapped to"
            );
            if big_button(ui, Icon::PointCloud, "Point Cloud", self.cloud_box.is_some(), tip).clicked() {
                self.cloud_dialog = true;
            }
            let tip = msg!(
                "Link CAD: a DWG or DXF drawing shown in the plans of a level, read again from its file on Reload and when the project opens"
            );
            if big_button(ui, Icon::LinkCad, "Link CAD", self.cad_box.as_ref().is_some_and(|b| b.linked()), tip).clicked() {
                self.cad_dialog = Some(true);
            }
            let tip = msg!(
                "Manage Links: every linked model, IFC, CAD drawing, point cloud and 3D model, its path and status and how many instances place it; Reload, Reload From and Unload"
            );
            if big_button(ui, Icon::Link, "Manage Links", self.manage_links.is_some(), tip).clicked() {
                self.open_manage_links();
            }
        });
        panel(ui, msg!("Import"), |ui| {
            let tip = msg!(
                "Import CAD: a DWG or DXF drawing brought into the project, drawn under the model in the plans of a level, its layers, colours and linetypes kept, and snapped to"
            );
            if big_button(ui, Icon::ImportCad, "Import CAD", self.cad_box.as_ref().is_some_and(|b| !b.linked()), tip).clicked() {
                self.cad_dialog = Some(false);
            }
            let tip = msg!(
                "Import 3D Model: an OBJ or glTF file's objects brought into the project as Generic Model families, each placed where the file has it on this plan's level, with its materials' colours"
            );
            if big_button(ui, Icon::Component, "Import 3D Model", false, tip).clicked() {
                self.import_mesh_dialog(ui.ctx());
            }
            let tip = msg!(
                "Image: a PNG or JPEG picture placed under the model in the plans of this plan's level, moved and sized as an element"
            );
            let r = big_button(ui, Icon::Component, "Image", false, tip);
            ribbon::choices(ui, &r, false, |ui| {
                if ui.button(t("Import Image…")).clicked() {
                    self.image_dialog(ui.ctx(), false, false);
                }
                if ui.button(t("Link Image…")).clicked() {
                    self.image_dialog(ui.ctx(), true, false);
                }
            });
            let tip = msg!(
                "PDF: a PDF's first page placed under the model in the plans of this plan's level, at its paper's size at the plan's scale, so a drawing printed at that scale lies true"
            );
            let r = big_button(ui, Icon::Sheet, "PDF", false, tip);
            ribbon::choices(ui, &r, false, |ui| {
                if ui.button(t("Import PDF…")).clicked() {
                    self.image_dialog(ui.ctx(), false, true);
                }
                if ui.button(t("Link PDF…")).clicked() {
                    self.image_dialog(ui.ctx(), true, true);
                }
            });
        });
    }

    /// Collaborate: Co-Work, others working on this project live.
    fn collaborate_tab(&mut self, ui: &mut Ui) {
        panel(ui, msg!("Work Together"), |ui| {
            let tip =
                msg!("Co-Work (CW): host this project for others on BIMCraft, or join someone's session, and see each other's edits live");
            if big_button(ui, Icon::Cowork, "Co-Work", self.cowork_dialog.is_some(), tip).clicked() {
                self.open_cowork();
            }
        });
    }

    fn manage_tab(&mut self, ui: &mut Ui) {
        panel(ui, "Settings", |ui| {
            let tip = msg!("Materials: the project's library, each material's colour in 3D and the patterns its cut and its surfaces show");
            if big_button(ui, Icon::Materials, "Materials", self.materials_dialog.is_some(), tip).clicked() {
                self.materials_dialog = Some(materials::MaterialsDialog::new(&self.session.model, self.favorites_path.as_ref()));
            }
            let tip = msg!("Favorites: the materials and families you keep for every project; add one to this project with a click");
            if big_button(ui, Icon::Favorite, "Favorites", self.favorites_dialog.is_some(), tip).clicked() {
                self.open_favorites();
            }
            let tip =
                msg!("Plugins: the WASM extensions in your plugin folder, what became of each, their log, and switching them on and off");
            if big_button(ui, Icon::Code, "Plugins", self.plugins_dialog, tip).clicked() {
                self.plugins_dialog = true;
            }
            // The project's information, its parameters and Purge Unused, stacked, as Revit's Settings panel has them.
            stacked(ui, |ui| {
                let tip =
                    msg!("Project Information: the project's name, number, client, address, status and issue date, and its parameters");
                if stacked_button(ui, Icon::ProjectInfo, "Project Information", self.project_info_dialog.is_some(), tip).clicked() {
                    self.open_project_info();
                }
                let tip = msg!(
                    "Project Parameters: project and shared parameters in one place, their categories, dictionaries, import and export"
                );
                if stacked_button(ui, Icon::Parameters, "Project Parameters", self.parameter_manager.is_some(), tip).clicked() {
                    self.parameter_manager = Some(parameters::ParameterManager::new(&self.session.model));
                }
                let tip = msg!(
                    "Purge Unused (PU): take out the types, families and the rest nothing in the project uses; each category keeps a type"
                );
                if stacked_button(ui, Icon::Delete, "Purge Unused", self.purge_dialog.is_some(), tip).clicked() {
                    self.open_purge();
                }
            });
            // What annotation draws with: pens, line styles and patterns, fill patterns; one button that lists them.
            let open = self.line_weights_dialog.is_some()
                || self.line_styles_dialog.is_some()
                || self.fill_patterns_dialog.is_some()
                || self.asset_manager.is_some()
                || self.halftone_dialog.is_some();
            let tip =
                msg!("Annotation Settings: line weights, line styles, line patterns and fill patterns, halftone, and the Asset Manager");
            let r = big_button(ui, Icon::AnnotationSettings, "Annotation Settings", open, tip);
            ribbon::choices(ui, &r, false, |ui| {
                // As wide as its longest name, not the window.
                ui.set_width(200.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let tip = msg!("Line Weights: what each pen draws on paper, the model's at each scale and the annotation's at any");
                if list_row(ui, Icon::LineWeights, "Line Weights", false, tip).clicked() {
                    self.line_weights_dialog = Some(lineweights::LineWeightsDialog::new(&self.session.model));
                }
                let tip = msg!("Line Styles: the pen, colour and line pattern detail lines draw in");
                if list_row(ui, Icon::ThinLines, "Line Styles", false, tip).clicked() {
                    self.line_styles_dialog = Some(linestyles::LineStylesDialog::new(&self.session.model));
                }
                let tip = msg!("Line Patterns: the dashes, dots, spaces and shapes lines are drawn with");
                if list_row(ui, Icon::LinePatterns, "Line Patterns", false, tip).clicked() {
                    self.line_styles_dialog = Some(linestyles::LineStylesDialog::patterns(&self.session.model));
                }
                let tip = msg!("Fill Patterns: the patterns filled regions and materials fill with, simple or made from lines drawn");
                if list_row(ui, Icon::FillPatterns, "Fill Patterns", false, tip).clicked() {
                    self.fill_patterns_dialog = Some(fillpatterns::FillPatternsDialog::new(&self.session.model));
                }
                let tip = msg!(
                    "Halftone/Underlay: how far toward the paper halftone draws, and so a plan's underlay between its bottom and its view depth"
                );
                if list_row(ui, Icon::Halftone, "Halftone/Underlay", false, tip).clicked() {
                    self.open_halftone();
                }
                let tip = msg!(
                    "Asset Manager: the project's materials, fill types and patterns, line patterns and styles and pictures, where each is used, and their import and export as a .bima file"
                );
                if list_row(ui, Icon::Materials, "Asset Manager", false, tip).clicked() {
                    self.open_asset_manager();
                }
            });
            let now_std = self.session.model.standard();
            let tip = tf("Technical Standards: configure drawing standards (NBR, ISO, ANSI, EU); now {}", &[t(now_std.short_name())]);
            if big_button(ui, Icon::Settings, "Technical Standards", self.standards_dialog.is_some(), &tip).clicked() {
                self.standards_dialog = Some(standards::StandardsDialog::new(&self.session.model));
            }
            let tip = tf(
                "Building Codes: the rules stairs and railings are held to (NBR, IBC, DIN, JIS), which the project may change; now {}",
                &[t(&self.session.model.building_code().name)],
            );
            if big_button(ui, Icon::Stair, "Building Codes", self.codes_dialog.is_some(), &tip).clicked() {
                self.codes_dialog = Some(codes::CodesDialog::new(&self.session.model));
            }
            let tip = msg!(
                "Electrical Settings: the voltages, distribution systems and wire types circuits and panels refer to; sections in mm²"
            );
            // The settings are the first sections of the Building Codes' Electrical tab.
            let open = self.codes_dialog.as_ref().is_some_and(codes::CodesDialog::on_electrical_settings);
            if big_button(ui, Icon::Wire, "Electrical Settings", open, tip).clicked() {
                self.codes_dialog = Some(codes::CodesDialog::electrical_settings(&self.session.model));
            }
            // Object styles, snaps and units, stacked.
            stacked(ui, |ui| {
                let tip = msg!("Object Styles: the pen each category draws in, seen and cut, in every view");
                if stacked_button(ui, Icon::ObjectStyles, "Object Styles", self.object_styles_dialog.is_some(), tip).clicked() {
                    self.object_styles_dialog = Some(objectstyles::ObjectStylesDialog::new(&self.session.model));
                }
                let tip = msg!(
                    "Snaps: what a picked point sticks to (ends, middles, crossings, centres, perpendiculars), and the angles it steps by; SE, SM, SI, SC, SP and SN take one alone for the next point, SO none"
                );
                if stacked_button(ui, Icon::Snaps, "Snaps", self.show_snaps, tip).clicked() {
                    self.show_snaps = true;
                }
                let now = self.session.model.units;
                let tip = tf("Project Units (UN): how lengths show and read; now {}", &[&t(now.label()).to_lowercase()]);
                let r = stacked_button(ui, Icon::Units, msg!("Project Units"), false, &tip);
                let asked = self.open_units;
                let places = self.session.model.decimals_shown();
                let opened = ribbon::choices(ui, &r, asked, |ui| {
                    for u in Units::ALL {
                        if ui.selectable_label(u == now, t(u.label())).clicked() {
                            self.run("units", json!({"length": u.name()}));
                        }
                    }
                    // The places lengths show, each as it writes a metre; inches and feet-inches show sixteenths.
                    if let Some(range) = now.decimals() {
                        ui.separator();
                        ui.weak(t("Decimal places"));
                        for d in range {
                            if ui.selectable_label(Some(d) == places, now.with_unit_to(1000.0, Some(d))).clicked() {
                                self.run("units", json!({"decimals": d}));
                            }
                        }
                    }
                });
                self.open_units &= !opened;
            });
            let tip = msg!("Keyboard Shortcuts (KS): customize, edit, and load shortcut keys");
            if big_button(ui, Icon::Keyboard, "Shortcuts", self.show_shortcuts, tip).clicked() {
                self.open_shortcuts();
            }
            // Where a room's area is measured, as Revit's Room Area Computation.
            let now = self.session.model.room_area;
            let tip = tf("Room Area Computation: where room areas are measured; now {}", &[&t(now.label()).to_lowercase()]);
            let r = big_button(ui, Icon::Settings, "Room Area Computation", false, &tip);
            ribbon::choices(ui, &r, false, |ui| {
                for a in RoomArea::ALL {
                    if ui.selectable_label(a == now, t(a.label())).clicked() {
                        self.run("room_area", json!({"at": a.name()}));
                    }
                }
            });
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
            // New and seldom wanted: the first of Settings' tools handed over when the window is narrow.
            let tip = msg!(
                "Classification Manager (CI): a classification system's tables and parameters, codes picked by hand for the selection, or proposed by the AI assistant"
            );
            if big_button(ui, Icon::Classify, "Classification Manager", self.classification.is_some(), tip).clicked() {
                self.open_classification();
            }
        });
        // Revit's Project Location: where the project stands (a model is linked from Insert > Link).
        panel(ui, msg!("Project Location"), |ui| {
            let tip = msg!(
                "Location (LO): where the project stands in the world: its locations, the Project Base Point and the Survey Point, the map system, managing them, and conversions"
            );
            if big_button(ui, Icon::Coordinates, "Location", self.coordinates_dialog.is_some(), tip).clicked() {
                self.open_location();
            }
        });
        panel(ui, msg!("Inquiry"), |ui| {
            let tip = msg!("Select by ID: select elements by their IDs, GUIDs or IFC GlobalIds, as many as you paste, and show them");
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
            let tip = msg!("Phasing (PH): the project's phases in order, the phase filters that show them, and how a filter draws each");
            if big_button(ui, Icon::Phasing, "Phasing", self.phasing_dialog.is_some(), tip).clicked() {
                self.phasing_dialog = Some(phasing::PhasingDialog::default());
            }
        });
        panel(ui, "Design Options", |ui| {
            let tip = msg!("Design Options (DO): sets of options for a part of the design, and the one new elements are made in");
            if big_button(ui, Icon::Options, "Design Options", self.options_dialog.is_some(), tip).clicked() {
                self.options_dialog = Some(phasing::OptionsDialog::default());
            }
        });
        panel(ui, msg!("Macros"), |ui| {
            let tip =
                msg!("Run Script: commands, one a line as agents and the command line write them, tried first and run as one undo step");
            if big_button(ui, Icon::Code, "Run Script", self.run_script.is_some(), tip).clicked() {
                self.open_run_script();
            }
        });
    }

    /// Whether the selection is images and PDFs placed in plans, and nothing else.
    pub(crate) fn images_selected(&self) -> bool {
        let m = &self.session.model;
        !self.selection.is_empty() && self.selection.iter().all(|id| matches!(m.elements.get(*id), Some(Element::Image(_))))
    }

    fn modify_tab(&mut self, ui: &mut Ui) {
        self.select_panel(ui);
        if self.sketch.is_some() {
            self.sketch_draw(ui);
            self.ramp_sketch_panel(ui);
            self.walls_outline_panel(ui);
            self.sketch_modify_panel(ui);
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
        self.clipboard_panel(ui);
        // Create Group, and with a group or a member selected, Revit's Group panel.
        self.group_panels(ui);
        // A dimension selected: its references are picked again from the Modify tab, as from its right-click menu.
        let dimension = matches!(self.selection.as_slice(), [id]
            if matches!(self.session.model.elements.get(*id), Some(Element::Dimension(d)) if !d.alignment));
        if dimension {
            panel(ui, msg!("Dimension"), |ui| {
                let active = self.dimension_refs_edit.is_some();
                let tip = msg!(
                    "Edit Dimension References: click a picked reference to take it out, another element to add it; click away places the line. Enter confirms, Esc cancels"
                );
                if big_button(ui, Icon::Dimension, "Edit Dimension References", active, tip).clicked() {
                    if active {
                        self.commit_dimension_refs();
                    } else {
                        self.start_dimension_refs_edit();
                    }
                }
            });
        }
        // Hide in View, and Override Graphics: what the selection does in the view on screen.
        // Kept whole while the ribbon makes room: what it offers depends on the selection (Unhide only for what's hidden).
        if !self.selection.is_empty() && self.graphic_view().is_some() {
            ribbon::kept(ui, "View", |ui| {
                let tip = msg!("Hide Element (EH): leave the selected elements out of this view");
                if big_button(ui, Icon::Visibility, "Hide Element", false, tip).clicked() {
                    self.hide_selection(false);
                }
                let tip = msg!("Hide Category (VH): leave every category the selection is of out of this view");
                if big_button(ui, Icon::Visibility, "Hide Category", false, tip).clicked() {
                    self.hide_selection(true);
                }
                let tip = msg!("Override Graphics by Element (EO): draw the selected elements their own way in this view");
                if big_button(ui, Icon::Visibility, "Override Graphics", false, tip).clicked() {
                    self.override_selection();
                }
                // What can be picked while hidden is what Reveal Hidden Elements draws: its selection can be brought back.
                let (elements, categories) = self.hidden_selection();
                if !elements.is_empty() {
                    let tip = msg!("Unhide Element (EU): bring back the selected elements this view hides");
                    if big_button(ui, Icon::Visibility, "Unhide Element", false, tip).clicked() {
                        self.unhide_selection(false);
                    }
                }
                if !categories.is_empty() {
                    let tip = msg!("Unhide Category (VU): bring back every category the selection is of that this view hides");
                    if big_button(ui, Icon::Visibility, "Unhide Category", false, tip).clicked() {
                        self.unhide_selection(true);
                    }
                }
            });
        }
        // Revit's Modify panel: small buttons in rows. Those taking the selection need one.
        let some = !self.selection.is_empty();
        // Images and PDFs alone: what they take, moving, copying, mirroring and deleting, and nothing that trims or paints.
        let images = some && self.images_selected();
        // The button lit while its tool is on (the two Mirrors told apart by how they take the axis).
        let on = |t: &Tool, other: &Tool| match (t, other) {
            (Tool::Mirror { draw: a, .. }, Tool::Mirror { draw: b, .. }) => a == b,
            _ => std::mem::discriminant(t) == std::mem::discriminant(other),
        };
        let rows: [Buttons; 2] = [
            &[
                (
                    Icon::Align,
                    "Align",
                    msg!("Align (AL): pick a reference line, then what to line up with it"),
                    Some(Tool::Align(None)),
                    true,
                ),
                (
                    Icon::Offset,
                    "Offset",
                    msg!("Offset (OF): pick a wall or a line; a copy goes to the cursor's side"),
                    Some(Tool::Offset),
                    true,
                ),
                (
                    Icon::MirrorPick,
                    "Mirror Pick Axis",
                    msg!("Mirror, Pick Axis (MM): pick a wall or grid to mirror across"),
                    Some(Tool::Mirror { draw: false, start: None }),
                    some,
                ),
                (
                    Icon::MirrorDraw,
                    "Mirror Draw Axis",
                    msg!("Mirror, Draw Axis (DM): click the axis's two ends"),
                    Some(Tool::Mirror { draw: true, start: None }),
                    some,
                ),
                (
                    Icon::Split,
                    msg!("Split Element"),
                    msg!("Split Element (SL): click a wall or a line where to split it"),
                    Some(Tool::Split),
                    true,
                ),
                (
                    Icon::Linework,
                    msg!("Linework"),
                    msg!("Linework (LW): click an edge to draw it in another line style, or hide it, in this view only"),
                    Some(Tool::Linework),
                    true,
                ),
                (
                    Icon::TrimSingle,
                    msg!("Trim/Extend Single Element"),
                    msg!("Trim/Extend Single Element: pick the line to trim or extend to, then the line to change, on the part to keep"),
                    Some(Tool::TrimTo { boundary: None, many: false }),
                    true,
                ),
            ],
            &[
                (Icon::Move, "Move", msg!("Move (MV): click a base point, then where it goes"), Some(Tool::Move(None)), some),
                (Icon::Copy, "Copy", msg!("Copy (CO): click a base point, then where the copy goes"), Some(Tool::Copy(None)), some),
                (
                    Icon::Rotate,
                    "Rotate",
                    msg!("Rotate (RO): click the angle's start, then its end"),
                    Some(Tool::Rotate { centre: P2(0.0, 0.0), start: None, place: false }),
                    some,
                ),
                (
                    Icon::Scale,
                    "Scale",
                    msg!("Scale (RE): annotation, CAD drawings, images and PDFs, by a percentage or by a length between two points"),
                    Some(Tool::Scale(scale::Step::Choose)),
                    some,
                ),
                (
                    Icon::Trim,
                    "Trim/Extend to Corner",
                    msg!("Trim/Extend to Corner (TR): pick two walls or two lines, on the parts to keep"),
                    Some(Tool::Trim(None)),
                    true,
                ),
                (
                    Icon::TrimMultiple,
                    msg!("Trim/Extend Multiple Elements"),
                    msg!(
                        "Trim/Extend Multiple Elements: pick the line to trim or extend to, then each line to change, on the part to keep"
                    ),
                    Some(Tool::TrimTo { boundary: None, many: true }),
                    true,
                ),
                (Icon::Delete, "Delete", msg!("Delete (DE) the selection"), None, some),
            ],
        ];
        // The Wall tool's and the Detail Line tool's Draw panels: lines, rectangles, polygons, circles, arcs and the rest.
        match self.tool {
            Tool::Wall(_) => self.draw_panel(ui, draw::Target::Wall),
            Tool::DetailLine(_) => self.draw_panel(ui, draw::Target::Detail),
            _ => {}
        }
        // Detail lines selected, or sketch lines picked: Join into Polyline and Explode Polyline.
        let picked = self.sketch.as_ref().map(|sk| sk.picked.len());
        if (self.lines_selected() && self.tool == Tool::Modify) || (picked.is_some() && self.tool == Tool::Modify) {
            panel(ui, msg!("Modify Lines"), |ui| {
                let two = picked.unwrap_or(self.selection.len()) >= 2;
                let tip = msg!("Join into Polyline (PJ): the lines selected, meeting end to end, become one polyline");
                if ui.add_enabled_ui(two, |ui| big_button(ui, Icon::JoinPolyline, "Join into Polyline", false, tip)).inner.clicked() {
                    self.join_polyline();
                }
                let tip = msg!("Explode Polyline (EX): a polyline taken apart into a line or an arc for each segment");
                if big_button(ui, Icon::ExplodePolyline, "Explode Polyline", false, tip).clicked() {
                    self.explode_polyline();
                }
                self.vertex_buttons(ui);
            });
        }
        // The Stair tool: how its line is drawn, and whether the next stretch of it is steps or a landing.
        if matches!(self.tool, Tool::Stair(_)) {
            panel(ui, msg!("Draw"), |ui| {
                let shapes = [
                    (StairShape::Straight, Icon::Line, "Straight", msg!("Straight: the next stretch of the stair runs in a straight line")),
                    (
                        StairShape::Arc,
                        Icon::TangentArc,
                        "Arc",
                        msg!("Arc: the next stretch curves, carrying on smoothly from the one before it"),
                    ),
                    (
                        StairShape::Spiral,
                        Icon::Circle,
                        "Spiral",
                        msg!("Spiral: click the centre first, then where the stair starts. It winds round the centre."),
                    ),
                    (
                        StairShape::Risers,
                        Icon::Stair,
                        "By Risers",
                        msg!("By Risers: draw each riser yourself with two clicks, left end first. Press Enter when you are done."),
                    ),
                ];
                for (shape, icon, label, tip) in shapes {
                    if big_button(ui, icon, label, self.stair_shape == shape, tip).clicked() && self.stair_shape != shape {
                        // A line goes on from straight to arc; a spiral or a stair by risers starts again.
                        let line = |s: StairShape| matches!(s, StairShape::Straight | StairShape::Arc);
                        if !(line(shape) && line(self.stair_shape)) {
                            self.tool = Tool::Stair(Vec::new());
                            self.stair_kinds.clear();
                        }
                        self.stair_shape = shape;
                    }
                }
            });
            if matches!(self.stair_shape, StairShape::Straight | StairShape::Arc) {
                panel(ui, msg!("Segment"), |ui| {
                    let tip = msg!("Steps: the next stretch you draw is a flight of steps");
                    if big_button(ui, Icon::Stair, "Steps", !self.stair_landing, tip).clicked() {
                        self.stair_landing = false;
                    }
                    let tip = msg!("Landing: the next stretch you draw is a landing, as long as you draw it");
                    if big_button(ui, Icon::Landing, "Landing", self.stair_landing, tip).clicked() {
                        self.stair_landing = true;
                    }
                    let tip = msg!(
                        "Lock Tread Depth: keep every tread the same depth. Turn it off to let each flight stretch or squeeze its treads, within the rules, so it ends exactly where you click."
                    );
                    if big_button(ui, Icon::TreadLock, "Lock Tread Depth", self.stair_lock_going, tip).clicked() {
                        self.stair_lock_going = !self.stair_lock_going;
                    }
                    let tip = msg!(
                        "Draw from Top: start where the stair arrives upstairs and draw it down. Useful when the top landing has to sit in an exact spot."
                    );
                    if big_button(ui, Icon::StairFromTop, "Draw from Top", self.stair_from_top, tip).clicked() {
                        self.stair_from_top = !self.stair_from_top;
                    }
                });
            }
        }
        // The Ramp tool: how its line is drawn, or a sketch; and whether the next stretch is a flight or a landing.
        if matches!(self.tool, Tool::Ramp(_)) {
            panel(ui, msg!("Draw"), |ui| {
                let shapes = [
                    (RampShape::Straight, Icon::Line, "Straight", msg!("Straight: the next stretch of the ramp runs in a straight line")),
                    (
                        RampShape::Arc,
                        Icon::TangentArc,
                        "Arc",
                        msg!("Arc: the next stretch curves, carrying on smoothly from the one before it"),
                    ),
                    (
                        RampShape::Circular,
                        Icon::Circle,
                        "Circular",
                        msg!("Circular: click the centre first, then the middle of the ramp's foot. It winds round the centre to its top."),
                    ),
                ];
                for (shape, icon, label, tip) in shapes {
                    if big_button(ui, icon, label, self.ramp_shape == shape, tip).clicked() && self.ramp_shape != shape {
                        let line = |s: RampShape| matches!(s, RampShape::Straight | RampShape::Arc);
                        if !(line(shape) && line(self.ramp_shape)) {
                            self.tool = Tool::Ramp(Vec::new());
                            self.ramp_kinds.clear();
                        }
                        self.ramp_shape = shape;
                    }
                }
                let tip = msg!(
                    "Sketch: draw the ramp's two sides as Boundary lines, then its start line, its end line and two lines round each landing as Run lines"
                );
                if big_button(ui, Icon::Line, "Sketch", false, tip).clicked()
                    && let Some(level) = self.plan_level()
                {
                    self.ramp_kinds.clear();
                    self.start_sketch(crate::sketch::Of::Ramp(level), Vec::new());
                }
            });
            if self.ramp_shape != RampShape::Circular {
                panel(ui, msg!("Segment"), |ui| {
                    let tip = msg!("Flight: the next stretch you draw slopes, at the Options Bar's slope");
                    if big_button(ui, Icon::Ramp, "Flight", !self.ramp_landing, tip).clicked() {
                        self.ramp_landing = false;
                    }
                    let tip = msg!("Landing: the next stretch you draw is a flat landing, as long as you draw it");
                    if big_button(ui, Icon::Landing, "Landing", self.ramp_landing, tip).clicked() {
                        self.ramp_landing = true;
                    }
                });
            }
        }
        // Ramps selected: Flip, and a sketched one's Edit Sketch.
        let ramps: Vec<Id> =
            (self.selection.iter().copied()).filter(|id| matches!(self.session.model.elements.get(*id), Some(Element::Ramp(_)))).collect();
        if !ramps.is_empty() && self.tool == Tool::Modify {
            panel(ui, msg!("Modify Ramp"), |ui| {
                let tip = msg!("Flip (Space): make the ramp climb the other way, so its top becomes its foot");
                if big_button(ui, Icon::Flip, "Flip", false, tip).clicked() {
                    self.flip();
                }
                if let [id] = ramps.as_slice()
                    && let Some(Element::Ramp(r)) = self.session.model.elements.get(*id)
                    && !r.sketch.is_empty()
                {
                    let lines = r.sketch.clone();
                    let tip = msg!("Edit Sketch: change the ramp's sides, its start and end lines and its landings' lines");
                    if big_button(ui, Icon::Line, "Edit Sketch", false, tip).clicked() {
                        let id = *id;
                        self.start_sketch(crate::sketch::Of::RampEdit(id), lines);
                    }
                }
            });
        }
        // Stairs selected: which way they climb, and what their building code says of them.
        let stairs = self.selected_stairs();
        if !stairs.is_empty() && self.tool == Tool::Modify {
            panel(ui, msg!("Modify Stair"), |ui| {
                let tip = msg!("Flip (Space): make the stair climb the other way, so its top becomes its foot");
                if big_button(ui, Icon::Flip, "Flip", false, tip).clicked() {
                    self.flip();
                }
                let tip = msg!("Convert to Sketch: turn every riser into a line you can drag by its ends, to match an odd shape");
                if let [id] = stairs.as_slice()
                    && matches!(self.session.model.elements.get(*id), Some(Element::Stair(st)) if st.sketch.is_empty())
                    && big_button(ui, Icon::StairSketch, "Convert to Sketch", false, tip).clicked()
                {
                    self.run("stair_sketch", json!({"id": id}));
                }
                let tip = msg!("Convert Back: make this sketched stair the stair it was before it was converted");
                if let [id] = stairs.as_slice()
                    && matches!(self.session.model.elements.get(*id), Some(Element::Stair(st)) if st.unsketched.is_some())
                    && big_button(ui, Icon::Stair, "Convert Back", false, tip).clicked()
                {
                    self.run("stair_sketch", json!({"id": id, "back": true}));
                }
                // Connect Levels: the stair climbs on to a level picked, standing again on each level on the way.
                if let [id] = stairs.as_slice()
                    && let Some(Element::Stair(st)) = self.session.model.elements.get(*id)
                {
                    let m = &self.session.model;
                    let base = m.elevation(st.level);
                    let above: Vec<(String, bimcraft_core::model::Guid)> = (m.levels().into_iter())
                        .filter(|(_, l)| l.elevation > base + 1e-6)
                        .filter_map(|(lid, l)| Some((l.name.clone(), m.elements.guid(lid)?)))
                        .collect();
                    let (multi, top) = (st.multistorey, st.top_level);
                    let tip = msg!("Connect Levels: carry the stair on up to a level you pick, repeating it on every level on the way");
                    let r = big_button(ui, Icon::StairLevels, "Connect Levels", multi, tip);
                    let mut pick: Option<Value> = None;
                    egui::Popup::menu(&r).show(|ui| {
                        ui.set_min_width(160.0);
                        for (name, guid) in &above {
                            if ui.selectable_label(multi && top == Some(*guid), name).clicked() {
                                pick = Some(json!({"ids": [id], "top_level": name, "multistorey": true}));
                            }
                        }
                        if multi {
                            ui.separator();
                            if ui.button(t("Disconnect Levels")).clicked() {
                                let next = above.first().map(|(n, _)| n.clone());
                                pick = Some(json!({"ids": [id], "multistorey": false, "top_level": next}));
                            }
                        }
                    });
                    if let Some(p) = pick {
                        self.run("modify", p);
                    }
                    let tip = msg!("Separate Storeys: make each storey its own stair, so you can change one without the others");
                    if multi && big_button(ui, Icon::StairLevels, "Separate Storeys", false, tip).clicked() {
                        self.run("stair_storeys", json!({"id": id}));
                    }
                }
                let tip = msg!("Code Check: see which building code rules this stair breaks, and what to change");
                if let [id] = stairs.as_slice()
                    && big_button(ui, Icon::StairCheck, "Code Check", self.code_findings.is_some(), tip).clicked()
                {
                    self.code_check(*id);
                }
            });
        }
        self.curtain_grid_panel(ui);
        // Walls selected: Revit's Modify Wall panel.
        let walls = self.selected_walls();
        let attachable = self.selected_attachable();
        if !attachable.is_empty() && matches!(self.tool, Tool::Modify | Tool::Attach) {
            let title = if walls.is_empty() { msg!("Modify Column") } else { msg!("Modify Wall") };
            // With one wall's Mode panel beside it, it makes room first, keeping Flip: a narrow ribbon lists Attach and Detach
            // before it takes the Modify panel's tools, as the Mode panel would otherwise make it.
            let mode = matches!(walls.as_slice(), [id] if profile::profile_editable(&self.session.model, *id));
            let body = |ui: &mut Ui| {
                if !walls.is_empty() {
                    let tip = msg!("Flip (Space): the walls' exterior sides go to the other side of their location lines");
                    if big_button(ui, Icon::Flip, "Flip", false, tip).clicked() {
                        self.flip();
                    }
                }
                let tip = msg!("Attach Top/Base: pick a roof or a floor for the tops, or bases, of the walls and columns to follow");
                if big_button(ui, Icon::Attach, "Attach Top/Base", self.tool == Tool::Attach, tip).clicked() {
                    self.set_tool(Tool::Attach);
                }
                let tip = msg!("Detach Top/Base: the tops and bases of the walls and columns their own again");
                if big_button(ui, Icon::Detach, "Detach Top/Base", false, tip).clicked() {
                    self.run("modify", json!({"ids": attachable, "attach_top": null, "attach_base": null}));
                }
            };
            if mode { ribbon::yielding(ui, title, body) } else { panel(ui, title, body) }
        }
        // One straight, upright wall: Revit's Mode panel of Modify | Walls, its shape in elevation sketched again.
        if let [id] = self.selected_walls().as_slice()
            && self.tool == Tool::Modify
            && profile::profile_editable(&self.session.model, *id)
        {
            let id = *id;
            panel(ui, msg!("Mode"), |ui| {
                let tip =
                    msg!("Edit Profile: sketch the wall's outline, and any holes, on its face in an elevation, a section or the 3D view");
                let on = self.go_to_view.is_some() || matches!(self.sketch.as_ref().map(|sk| sk.of), Some(sketch::Of::Profile(_)));
                if big_button(ui, Icon::EditProfile, msg!("Edit Profile"), on, tip).clicked() {
                    self.edit_profile(id);
                }
                let shaped = self.session.model.wall(id).is_some_and(|w| !w.profile.is_empty());
                let tip = msg!("Reset Profile: the wall's rectangle back, from its base to its top");
                if shaped && big_button(ui, Icon::ResetProfile, msg!("Reset Profile"), false, tip).clicked() {
                    self.run("modify", json!({"id": id, "profile": []}));
                }
            });
        }
        // Work plane-based components selected: Revit's Work Plane panel, Edit Work Plane (T12).
        let hosted = self.work_plane_components();
        if !hosted.is_empty() && self.tool == Tool::Modify {
            panel(ui, msg!("Work Plane"), |ui| {
                let tip = msg!(
                    "Edit Work Plane: the components onto a level, a reference plane or a face picked (the Work Plane dialog), keeping their Offset from Host"
                );
                if big_button(ui, Icon::WorkPlane, msg!("Edit Work Plane"), self.work_plane.is_some(), tip).clicked() {
                    self.open_component_work_plane(hosted.clone());
                }
            });
        }
        // A roof selected: which of its edges stand upright; a flat one's shape too.
        let shaping = matches!(self.tool, Tool::ModifySubElements | Tool::AddPoint | Tool::AddSplitLine(_));
        if let Some(roof) = self.selected_roof()
            && (matches!(self.tool, Tool::Modify | Tool::GableEdge) || shaping)
        {
            panel(ui, msg!("Shape Editing"), |ui| {
                let tip = msg!("Gable Edge: click an edge of a roof's footprint to stand it upright, a gable end; click again to slope it");
                if big_button(ui, Icon::GableEdge, "Gable Edge", self.tool == Tool::GableEdge, tip).clicked() {
                    self.set_tool(Tool::GableEdge);
                }
                if self.selected_shapeable().is_some() {
                    self.shape_tools(ui, roof);
                }
            });
        }
        // A floor selected: its slope arrow, or its shape's points and split lines.
        if let Some(floor) = self.selected_floor()
            && matches!(self.tool, Tool::Modify | Tool::SlopeArrow(_) | Tool::ModifySubElements | Tool::AddPoint | Tool::AddSplitLine(_))
        {
            panel(ui, "Shape Editing", |ui| {
                let tip = msg!("Slope Arrow: click its tail, then its head; the floor rises toward the head at the Options Bar's slope");
                if big_button(ui, Icon::SlopeArrow, "Slope Arrow", matches!(self.tool, Tool::SlopeArrow(_)), tip).clicked() {
                    self.set_tool(Tool::SlopeArrow(None));
                }
                self.shape_tools(ui, floor);
            });
        }
        panel(ui, "Modify", |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for row in rows {
                    ui.horizontal(|ui| {
                        let taken = |tool: &Option<Tool>| {
                            !images || matches!(tool, None | Some(Tool::Move(_) | Tool::Copy(_) | Tool::Scale(_) | Tool::Mirror { .. }))
                        };
                        for (icon, label, tip, tool, enabled) in row.iter().filter(|b| taken(&b.3)) {
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
        if images {
            self.mode_panel(ui);
            return;
        }
        panel(ui, "Phasing", |ui| {
            let tip = msg!("Demolish (DW): click an element to take it down in the view's phase; click it again to take it back");
            let on = self.graphic_view().is_some();
            if ui.add_enabled_ui(on, |ui| big_button(ui, Icon::Demolish, "Demolish", self.tool == Tool::Demolish, tip)).inner.clicked() {
                self.set_tool(Tool::Demolish);
            }
        });
        panel(ui, msg!("Geometry"), |ui| {
            let tip = msg!("Paint (PT): lay a material, and its texture, over the face you click in the 3D view");
            if big_button(ui, Icon::Paint, "Paint", self.tool == Tool::Paint, tip).clicked() {
                self.set_tool(Tool::Paint);
            }
            let tip = msg!("Remove Paint: take the paint off the face you click in the 3D view");
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
        if self.family_doc() {
            if !self.family_has_options() {
                self.options_rect = Rect::NOTHING;
                return;
            }
            let g = f32::from(style::gap());
            let view = self.view_rect;
            let top = view.top().max(self.tabs_foot - g + 4.0);
            let frame = style::card([false; 4]).inner_margin(Margin::symmetric(10, 3));
            let shown = Area::new(UiId::new("family-options-bar"))
                .order(Order::Middle)
                .fixed_pos(pos2(view.left() + g, top + g))
                .show(ctx, |ui| frame.show(ui, |ui| self.family_options(ui)).inner);
            self.options_rect = if shown.inner { shown.response.rect } else { Rect::NOTHING };
            return;
        }
        // Nothing to set: Modify, and a paste being placed.
        if self.context().is_none() && matches!(self.tool, Tool::Modify | Tool::Paste | Tool::PickLevel) && self.note_edit.is_none() {
            self.options_rect = Rect::NOTHING;
            return;
        }
        let g = f32::from(style::gap());
        let view = self.view_rect;
        let frame = style::card([false; 4]).inner_margin(Margin::symmetric(10, 3));
        // Below the tabs where they float over the view.
        let top = view.top().max(self.tabs_foot - g + 4.0);
        let shown = Area::new(UiId::new("options-bar")).order(Order::Middle).fixed_pos(pos2(view.left() + g, top + g)).show(ctx, |ui| {
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
            if self.note_edit.is_some() {
                self.note_options(ui);
                return;
            }
            if self.sketch.is_some() {
                let pending = self.outline_pending;
                self.sketch_options(ui);
                // Walls' Outline's own Options Bar stands alone until its DRAW.
                if !pending {
                    match self.tool {
                        Tool::SketchLine(_) => self.shape_options(ui),
                        Tool::Offset => self.offset_options(ui),
                        _ => {}
                    }
                }
                return;
            }
            match self.tool {
                Tool::Wall(_) => self.wall_options(ui),
                Tool::Linework => self.linework_options(ui),
                Tool::DetailGroup => self.group_options(ui),
                Tool::ModelGroup => self.model_group_options(ui),
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
                Tool::Camera(_) => {
                    let hint = self.session.model.units.name();
                    let l = ui.label(t("Eye Height:"));
                    ui.add(TextEdit::singleline(&mut self.camera_eye_height).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                    let l = ui.label(t("Target Height:"));
                    ui.add(TextEdit::singleline(&mut self.camera_target_height).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                    let l = ui.label(t("Lens:"));
                    ui.add(TextEdit::singleline(&mut self.camera_lens).desired_width(48.0).hint_text("mm")).labelled_by(l.id);
                }
                Tool::SlopeArrow(_) => {
                    let l = ui.label(t("Slope:"));
                    ui.add(TextEdit::singleline(&mut self.floor_slope).desired_width(48.0).hint_text("°")).labelled_by(l.id);
                }
                // Add Point along the surface as it is, an offset over it; or Absolute, an elevation above the level.
                Tool::AddPoint => {
                    ui.radio_value(&mut self.shape_along, true, t("Along Surface"));
                    ui.radio_value(&mut self.shape_along, false, t("Absolute"));
                    let hint = self.session.model.units.name();
                    if self.shape_along {
                        let l = ui.label(t("Offset from Surface:"));
                        ui.add(TextEdit::singleline(&mut self.shape_offset).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                    } else {
                        let l = ui.label(t("Elevation:"));
                        ui.add(TextEdit::singleline(&mut self.shape_z).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                        self.elevation_base(ui);
                    }
                }
                Tool::ModifySubElements => {
                    let l = ui.label(t("Elevation:"));
                    let hint = self.session.model.units.name();
                    let field = ui.add(TextEdit::singleline(&mut self.shape_z).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                    // Under Modify Sub Elements, Enter sets the selection to it.
                    if self.tool == Tool::ModifySubElements && field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        self.shape_apply();
                    }
                    self.elevation_base(ui);
                }
                Tool::Stair(_) => {
                    use bimcraft_core::model::StairLine;
                    if let Some(level) = self.plan_level() {
                        height_options(ui, &self.session.model, level, &mut self.stair_top, &mut self.stair_height, "stair-top");
                        // Up past the level above, it can stand on each level on the way.
                        let m = &self.session.model;
                        let top = self.stair_top.unwrap_or(None).map(|t| m.elevation(t));
                        let above = m.level_above(level).map(|g| m.elevation(g));
                        if let (Some(top), Some(above)) = (top, above)
                            && top > above + 1.0
                        {
                            ui.checkbox(&mut self.stair_multistorey, t("Multistorey")).on_hover_text(t(
                                "Repeat the stair on every level on the way up, each storey with as many risers as its height needs",
                            ));
                        }
                    }
                    let lines = [(StairLine::Left, "Run: Left"), (StairLine::Centre, "Run: Center"), (StairLine::Right, "Run: Right")];
                    let l = ui.label(t("Location Line:"));
                    let now = lines.iter().find(|(k, _)| *k == self.stair_line).map_or("", |(_, s)| s);
                    ComboBox::from_id_salt("stair-location-line")
                        .selected_text(t(now))
                        .show_ui(ui, |ui| {
                            for (k, s) in lines {
                                ui.selectable_value(&mut self.stair_line, k, t(s));
                            }
                        })
                        .response
                        .labelled_by(l.id)
                        .on_hover_text(t("The line of the stair your clicks follow. It stays put when you change the width."));
                    let l = ui.label(t("Offset:"));
                    let hint = self.session.model.units.name();
                    ui.add(TextEdit::singleline(&mut self.stair_offset).desired_width(56.0).hint_text(hint)).labelled_by(l.id);
                    let l = ui.label(t("Width:"));
                    let hint = self.session.model.units.name();
                    ui.add(TextEdit::singleline(&mut self.stair_width).desired_width(64.0).hint_text(hint))
                        .labelled_by(l.id)
                        .on_hover_text(t("How wide the stair is. Change it between clicks to give the next flight or landing its own width."));
                    // The riser and the tread: as typed, to match a drawing exactly; empty, as the rules say.
                    let m = &self.session.model;
                    let kind = self.types.get("stair").cloned().or_else(|| m.default_type("stair").map(String::from)).unwrap_or_default();
                    let rules = bimcraft_core::codes::limits_of(m, &kind).rules;
                    let l = ui.label(t("Riser:"));
                    ui.add(TextEdit::singleline(&mut self.stair_riser).desired_width(56.0).hint_text(m.length(rules.riser)))
                        .labelled_by(l.id)
                        .on_hover_text(t("The most a riser may be: the stair takes as many risers as it needs to stay under it. Leave it empty to follow the rules."));
                    let l = ui.label(t("Tread:"));
                    ui.add(TextEdit::singleline(&mut self.stair_tread).desired_width(56.0).hint_text(m.length(rules.going)))
                        .labelled_by(l.id)
                        .on_hover_text(t("How deep each tread is. Leave it empty to follow the rules."));
                    let l = ui.label(t("Winders:"));
                    let tip = t("How many winder treads turn a corner where two flights meet without a landing. 0 puts a landing there.");
                    ui.add(egui::DragValue::new(&mut self.stair_winders).range(0..=30)).labelled_by(l.id).on_hover_text(tip);
                    if self.stair_winders > 0 && !self.session.model.stair_rules().winders {
                        let code = t(&self.session.model.building_code().name).to_string();
                        ui.label(RichText::new(tf("Against {}", &[&code])).color(codes::OUTSIDE)).on_hover_text(t(
                            "This building code does not allow winders. The stair is still built, and marked outside the rules.",
                        ));
                    }
                    if self.stair_shape == StairShape::Spiral {
                        ui.checkbox(&mut self.stair_clockwise, t("Clockwise"));
                    }
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
                    ui.add(TextEdit::singleline(&mut self.ramp_slope).desired_width(56.0)).labelled_by(l.id).on_hover_text(t(
                        "The slope of the flight being drawn: change it between clicks for each flight its own",
                    ));
                    let was = self.slope_unit;
                    egui::ComboBox::from_id_salt("ramp-slope-unit").selected_text(t(self.slope_unit.label())).show_ui(ui, |ui| {
                        for u in SlopeUnit::ALL {
                            ui.selectable_value(&mut self.slope_unit, u, t(u.label()));
                        }
                    });
                    // The slope typed, written again in the unit picked.
                    if self.slope_unit != was
                        && let Some(p) = was.read(&self.ramp_slope)
                    {
                        self.ramp_slope = self.slope_unit.show(p);
                    }
                    if self.ramp_shape != RampShape::Circular {
                        let l = ui.label(t("Location Line:"));
                        use bimcraft_core::model::StairLine;
                        let lines = [(StairLine::Left, "Run: Left"), (StairLine::Centre, "Run: Center"), (StairLine::Right, "Run: Right")];
                        let now = lines.iter().find(|(x, _)| *x == self.ramp_line).map_or("", |(_, n)| n);
                        egui::ComboBox::from_id_salt("ramp-line")
                            .selected_text(t(now))
                            .show_ui(ui, |ui| {
                                for (x, n) in lines {
                                    ui.selectable_value(&mut self.ramp_line, x, t(n));
                                }
                            })
                            .response
                            .labelled_by(l.id);
                    }
                    if self.ramp_shape == RampShape::Circular {
                        ui.checkbox(&mut self.ramp_clockwise, t("Clockwise"));
                    }
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
                Tool::RotateNorth { .. } => self.rotate_north_options(ui),
                Tool::SpecifyPoint => {
                    if ui.button(t("Back to Coordinates")).clicked() {
                        self.stop_specify_pick();
                    }
                }
                Tool::ExportRegion(_) => {
                    if ui.button(t("Back to Export")).clicked() {
                        self.set_tool(Tool::Modify);
                    }
                }
                Tool::Mirror { .. } => {
                    ui.checkbox(&mut self.mirror_copy, t("Copy"));
                }
                Tool::Tag => {
                    ui.checkbox(&mut self.tag_leader, t("Leader"));
                    let l = ui.label(t("Label:"));
                    ui.add(TextEdit::singleline(&mut self.tag_label).desired_width(160.0).hint_text(t("a field, such as Type or Height")))
                        .labelled_by(l.id);
                }
                Tool::DetailView(_) => {
                    let l = ui.label(t("Scale 1:"));
                    ui.add(egui::TextEdit::singleline(&mut self.drawing.detail_scale).desired_width(48.0)).labelled_by(l.id);
                    let l = ui.label(t("Name:"));
                    ui.add(egui::TextEdit::singleline(&mut self.drawing.detail_name).desired_width(180.0)).labelled_by(l.id);
                    let l = ui.label(t("Number:"));
                    ui.add(egui::TextEdit::singleline(&mut self.drawing.detail_number).desired_width(70.0)).labelled_by(l.id);
                }
                Tool::DetailLine(_) => self.line_style_options(ui),
                // Between endpoints: as the cursor lies, aligned, horizontal or vertical (Space steps them).
                Tool::Dimension(mode, _) => {
                    ui.label(t("Place:"));
                    for (m, label) in [
                        (DimMode::Auto, msg!("Auto")),
                        (DimMode::Aligned, msg!("Aligned")),
                        (DimMode::Horizontal, msg!("Horizontal")),
                        (DimMode::Vertical, msg!("Vertical")),
                    ] {
                        if ui.selectable_label(mode == m, t(label)).clicked()
                            && let Tool::Dimension(now, _) = &mut self.tool
                        {
                            *now = m;
                        }
                    }
                }
                Tool::Text => {
                    let l = ui.label(t("Text:"));
                    ui.add(TextEdit::singleline(&mut self.note_text).desired_width(240.0).hint_text(t("what the note says")))
                        .labelled_by(l.id);
                    if ui.checkbox(&mut self.note_leader, t("Leader")).changed() {
                        self.note_arrow = None;
                    }
                    if self.note_leader {
                        notes::leader_type(ui, &mut self.note_leader_kind);
                    }
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
                Tool::Component => self.component_options(ui),
                _ => {}
            }
        });
    }

    /// The reference planes of the project a work plane-based family may be placed on, by name (unnamed ones by id).
    fn placement_planes(&self) -> Vec<(bimcraft_core::ids::Guid, String)> {
        let m = &self.session.model;
        m.rare("refplane")
            .into_iter()
            .filter_map(|id| match m.elements.get(id) {
                Some(bimcraft_core::model::Element::RefPlane(p)) => {
                    let name = if p.name.is_empty() { id.to_string() } else { p.name.clone() };
                    Some((m.guid(id), format!("{} : {name}", t("Reference Plane"))))
                }
                _ => None,
            })
            .collect()
    }

    /// Whether the family the Component tool places is work plane-based.
    fn placing_on_work_plane(&self) -> bool {
        self.placing().is_some_and(|(f, _)| self.session.model.families.get(&f).is_some_and(|f| f.work_plane_based))
    }

    /// The Component tool's Options Bar, for a work plane-based family: Place on Work Plane, its Placement Plane (the view's
    /// level or a reference plane of the project) and Rotate after placement.
    fn component_options(&mut self, ui: &mut Ui) {
        if !self.placing_on_work_plane() {
            ui.checkbox(&mut self.rotate_after, t("Rotate after placement"));
            return;
        }
        if ui.selectable_label(!self.place_on_face, t("Place on Work Plane")).clicked() {
            self.place_on_face = false;
        }
        if ui.selectable_label(self.place_on_face, t("Place on Face")).clicked() {
            self.place_on_face = true;
        }
        if self.place_on_face {
            ui.checkbox(&mut self.rotate_after, t("Rotate after placement"));
            return;
        }
        let level = self.plan_level().and_then(|l| self.session.model.level(l).map(|l| l.name.clone())).unwrap_or_default();
        let on_level = format!("{} : {level}", t("Level"));
        let planes = self.placement_planes();
        let now = self.placement_plane.and_then(|g| planes.iter().find(|(p, _)| *p == g)).map_or(on_level.clone(), |(_, n)| n.clone());
        let names: Vec<String> = std::iter::once(on_level).chain(planes.iter().map(|(_, n)| n.clone())).collect();
        let l = ui.label(t("Placement Plane:"));
        if let Some(n) = properties::combo(ui, (0, "placement-plane"), l.id, &now, &names) {
            self.placement_plane = planes.iter().find(|(_, name)| *name == n).map(|(g, _)| *g);
        }
        ui.checkbox(&mut self.rotate_after, t("Rotate after placement"));
    }

    /// A component placed with `params` (and its Placement Plane, for a work plane-based family placed in plan at height
    /// `z`): with Rotate after placement, it is selected and Rotate starts about its point.
    pub(crate) fn place_component(&mut self, mut params: Value, z: f64) {
        if self.placing_on_work_plane() && self.place_on_face {
            // On the face under the point: a ceiling plan's ceiling, a plan's floor or roof.
            let at = params["at"].as_array().and_then(|a| Some(P2(a.first()?.as_f64()?, a.get(1)?.as_f64()?)));
            let ceiling = matches!(self.sheet, Sheet::Ceiling(_));
            let found = self.plan_level().zip(at).and_then(|(l, q)| self.session.model.face_in_plan(l, q, ceiling));
            let Some((id, face)) = found else {
                let text =
                    if ceiling { t("There's no ceiling here to place it on.") } else { t("There's no floor or roof here to place it on.") };
                self.status = Status { text: text.into(), error: true };
                return;
            };
            params["host"] = json!({"face": {"element": id, "face": face.name()}});
            if let Some(o) = params.as_object_mut() {
                o.remove("offset");
                o.remove("level");
            }
            self.place_made(params);
            return;
        }
        if self.placing_on_work_plane()
            && let Some(g) = self.placement_plane.filter(|g| self.placement_planes().iter().any(|(p, _)| p == g))
        {
            // On the plane: a plan point taken straight onto it; an upright plane's, along its normal from the view's height.
            let upright = matches!(self.session.model.elements.get(g), Some(bimcraft_core::model::Element::RefPlane(p)) if p.upright());
            if upright && let Some([x, y]) = params["at"].as_array().map(|a| [a[0].clone(), a[1].clone()]) {
                params["at"] = json!([x, y, z]);
            }
            params["host"] = json!({"plane": g});
            if let Some(o) = params.as_object_mut() {
                o.remove("offset");
            }
        }
        self.place_made(params);
    }

    /// Place on Face in the 3D view: the component on face `face` of element `id`, at point `at` there.
    pub(crate) fn place_component_on_face(&mut self, id: Id, face: bimcraft_core::model::FaceRole, at: [f64; 3]) {
        let Some((family, ty)) = self.placing() else { return };
        let params = json!({"family": family, "type": ty, "at": at.map(|v| (v * 1e3).round() / 1e3 + 0.0),
                            "angle": self.component_angle, "host": {"face": {"element": id, "face": face.name()}}});
        self.place_made(params);
    }

    /// `component` run with `params`; with Rotate after placement, what it made is selected and Rotate starts about it.
    fn place_made(&mut self, params: Value) {
        let at = params["at"].as_array().and_then(|a| Some(P2(a.first()?.as_f64()?, a.get(1)?.as_f64()?)));
        let Some(made) = self.run("component", params) else { return };
        if self.rotate_after
            && let (Some(id), Some(at)) = (made["id"].as_u64(), at)
        {
            self.selection = vec![id];
            self.set_tool(Tool::Rotate { centre: at, start: None, place: false });
            if let Tool::Rotate { centre, .. } = &mut self.tool {
                *centre = at;
            }
        }
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

    /// Rotate Project North's Options Bar: an angle to type (Enter turns by it), Align with True North (by the active
    /// location's angle), and Place Centre.
    fn rotate_north_options(&mut self, ui: &mut Ui) {
        let l = ui.label(t("Angle:"));
        let r = ui.add(TextEdit::singleline(&mut self.angle).desired_width(60.0).hint_text("°")).labelled_by(l.id);
        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            match (self.angle.trim().parse::<f64>(), &self.tool) {
                (Ok(a), &Tool::RotateNorth { centre, .. }) if a.is_finite() => self.rotate_project_north(Some(a), Some(centre)),
                _ => self.status = Status { text: tf("rotate: `{}` is not an angle in degrees", &[&self.angle]), error: true },
            }
        }
        let angle = self.session.model.site().rotation;
        let align = tf("Align with True North ({}°)", &[&format!("{angle:.3}")]);
        if ui.add_enabled(angle.abs() > 1e-9, egui::Button::new(align)).clicked()
            && let &Tool::RotateNorth { centre, .. } = &self.tool
        {
            self.rotate_project_north(None, Some(centre));
        }
        if ui.button(t("Place Centre")).clicked()
            && let Tool::RotateNorth { place, .. } = &mut self.tool
        {
            *place = true;
        }
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
        if let Some(hint) = self.family_prompt() {
            return hint;
        }
        if let Some(hint) = self.sketch_prompt() {
            return hint;
        }
        match self.group_pick() {
            Some(groups::GroupPick::Add) if self.tool == Tool::Modify => {
                return msg!("Add to Group: click a greyed element, or drag a box, to add it to the group. Esc or Add again stops.");
            }
            Some(groups::GroupPick::Remove) if self.tool == Tool::Modify => {
                return msg!("Remove from Group: click a member, or drag a box, to take it out of the group. Esc or Remove again stops.");
            }
            _ if self.editing_group().is_some() && self.tool == Tool::Modify => {
                return msg!(
                    "Edit Group: only the group's members can be picked and changed; the rest is greyed. Finish Edit Group keeps it."
                );
            }
            _ => {}
        }
        if self.picking_pattern_lines() {
            return msg!("Select fill pattern lines then press Enter");
        }
        match self.tool {
            Tool::PatternCorner(None) => return msg!("Click on Bottom Left Corner"),
            Tool::PatternCorner(Some(_)) => return msg!("Click on Top Right Corner"),
            _ => {}
        }
        match (self.sheet, &self.tool) {
            (Sheet::View3D, Tool::Modify) => {
                return msg!(
                    "Click to select, drag a box to select several. Shift and the middle button orbit, the middle button pans, the wheel zooms. Alt (Option on a Mac) and the left button stand in for the middle one."
                );
            }
            (Sheet::Paper(_), Tool::Modify) => {
                return msg!("Click a view on the sheet to select it, drag it to move it, double-click it to open it.");
            }
            (Sheet::Section(_), Tool::Modify) => {
                return msg!("Click to select what the view cuts or sees; the middle button pans, the wheel zooms.");
            }
            (Sheet::View3D | Sheet::Section(_) | Sheet::Paper(_), Tool::Level) => {
                return msg!("Type the new level's elevation in the Options Bar, then Create.");
            }
            (Sheet::Section(_) | Sheet::Paper(_), Tool::Text) => {
                return msg!("Type the note in the Options Bar, then click where it goes.");
            }
            (Sheet::Section(_), Tool::Spot) => return msg!("Click a point to mark its height."),
            (Sheet::Section(_), Tool::Dimension(..)) => {
                return msg!("Pick levels, perpendicular grids or detail lines in this view, then click to place the dimension.");
            }
            (Sheet::Section(_), Tool::Move(_)) => {
                return msg!(
                    "Pick a base and destination in this view. Move supports its annotations, straight-wall openings and unhosted components."
                );
            }
            (Sheet::Section(_), Tool::DetailLine(_) | Tool::Trim(_) | Tool::TrimTo { .. }) => {}
            // Demolish picks in these views as it does in a plan; Paint in the 3D view alone.
            (Sheet::View3D | Sheet::Section(_), Tool::Demolish) | (Sheet::View3D, Tool::Paint | Tool::Unpaint) => {}
            // Shape Editing works on the floor's top in the 3D view too.
            (Sheet::View3D, Tool::ModifySubElements) => {
                return msg!(
                    "Click a point, a corner or an edge to select it (Shift adds), then type its height or drag its arrows; Delete removes it."
                );
            }
            (Sheet::View3D, Tool::Measure(_)) => {
                return msg!(
                    "Click points along what to measure: each length and the total read beside them. Click the first point again to close it and read its area; Enter ends the chain, Esc clears it."
                );
            }
            (Sheet::View3D, Tool::AddPoint | Tool::AddSplitLine(_) | Tool::Attach | Tool::Trim(_) | Tool::TrimTo { .. }) => {}
            (Sheet::View3D, Tool::Paste) => return msg!("Click on the work plane where the pasted elements go. Esc cancels."),
            // A paste follows the cursor in these views as in a plan; a section's level lines are picked.
            (Sheet::Section(_) | Sheet::Paper(_), Tool::Paste) | (Sheet::Section(_), Tool::PickLevel) => {}
            (Sheet::View3D | Sheet::Section(_) | Sheet::Paper(_), _) => {
                return msg!("This tool works in plan views: open one from the Project Browser.");
            }
            _ => {}
        }
        if matches!(self.target(), Some(draw::Target::Detail | draw::Target::Wall))
            && let Some(prompt) = self.shape_prompt()
        {
            return prompt;
        }
        if self.dimension_refs_edit.is_some() {
            return msg!(
                "Click a face, an axis or a corner to add it, the same one again to take it out; Tab steps over the references of the element under the cursor. Enter or the button confirms, Esc cancels."
            );
        }
        match &self.tool {
            Tool::Modify if self.selection_flips() => {
                msg!("Space flips the hand or facing; the flip controls on the plan do the same.")
            }
            Tool::Modify if self.selection_has_no_swing() => msg!("This door has no swing to flip."),
            Tool::Modify => msg!("Click to select, Tab for what's underneath, Ctrl adds, Shift takes away; drag to box or to move."),
            Tool::Wall(chain) if chain.is_empty() => msg!("Click to enter the wall's start point; Q pins a guide there."),
            Tool::Wall(_) => msg!(
                "Click the next point; Enter or Esc ends the chain, Shift keeps it square. Tab steps length and angle, or the wall's lines; Backspace or Ctrl+Z takes back the last point and wall; Q pins a guide."
            ),
            Tool::Attach if self.attach_base => msg!("Pick a roof or a floor: the bases of what's selected stand on it."),
            Tool::Attach => msg!("Pick a roof or a floor: the tops of what's selected follow its underside."),
            Tool::Door if !self.picked_door_swings() => msg!("This door has no swing to flip. Click a wall to place it."),
            Tool::Door => {
                msg!("Click a wall to place the door. Space flips the hinge; the pointer's side of the wall sets the swing.")
            }
            Tool::Window => {
                msg!("Click a wall to place the window. Space flips its face; the pointer's side sets which face is out.")
            }
            Tool::Room => msg!("Click inside walls to place a room; the outline shows the room you'd get."),
            Tool::RoomSeparator(None) => msg!("Click the separation line's start."),
            Tool::RoomSeparator(Some(_)) => msg!("Click the line's next point; each click places a line. Esc ends the chain."),
            Tool::Level => "Type the new level's elevation in the Options Bar, then Create.",
            Tool::Grid(None) => msg!("Click to enter the grid's start point."),
            Tool::Grid(Some(_)) => msg!("Click to enter the grid's end point."),
            Tool::ReferencePlane(None) => msg!("Click to enter the reference plane's start point."),
            Tool::ReferencePlane(Some(_)) => msg!("Click to enter the reference plane's end point."),
            Tool::CurtainGrid { way: bimcraft_core::curtain::GridWay::Vertical, .. } => {
                msg!("Click on the curtain wall to place a vertical grid. Esc ends.")
            }
            Tool::CurtainGrid { .. } => {
                msg!("Click on the curtain wall in a section, an elevation or the 3D view to place a horizontal grid. Esc ends.")
            }
            Tool::CurtainSegment { .. } => {
                msg!("Click a stretch of a grid line in a section, an elevation or the 3D view to take it off or put it back. Esc ends.")
            }
            Tool::Move(None) | Tool::Copy(None) => msg!("Click a base point to measure from."),
            Tool::Paste => msg!("Click where the pasted elements go. Esc cancels."),
            Tool::PickLevel => msg!("Click a level line in this section or elevation: the pasted elements go onto it. Esc cancels."),
            Tool::Move(Some(_)) => msg!("Click where the base point goes, or type a distance and press Enter. Esc cancels."),
            Tool::Copy(Some(_)) => msg!("Click where the copy's base point goes, or type a distance and press Enter. Esc cancels."),
            Tool::Rotate { place: true, .. } => msg!("Click the centre of rotation."),
            Tool::Rotate { start: None, .. } => msg!("Click the start of the angle, or type an angle and press Enter."),
            Tool::Rotate { start: Some(_), .. } => msg!("Click the end of the angle. Esc cancels."),
            Tool::RotateNorth { place: true, .. } => msg!("Click the centre the model turns about."),
            Tool::Acquire => msg!("Click a linked model or a CAD drawing: this project takes its shared coordinates. Esc cancels."),
            Tool::SpotCoordinate(None) => msg!("Click the point the spot coordinate reads; it snaps."),
            Tool::SpotCoordinate(Some(_)) => msg!("Click where its leader ends. Esc cancels."),
            Tool::ExportRegion(None) => msg!("Click a corner of the region to export. Esc goes back to Export."),
            Tool::ExportRegion(Some(_)) => msg!("Click the region's opposite corner. Esc goes back to Export."),
            Tool::SpecifyPoint => {
                msg!(
                    "Click the point the shared coordinates typed are at: it snaps, and a wall's corner is its bottom, a slab's its top. Esc goes back."
                )
            }
            Tool::RotateNorth { start: None, .. } => {
                msg!("Rotate Project North: click the start of the angle, or type an angle and press Enter. It turns the whole model.")
            }
            Tool::RotateNorth { start: Some(_), .. } => {
                msg!("Click the end of the angle: the model turns, and true north with it, so nothing moves in the world. Esc cancels.")
            }
            Tool::Scale(scale::Step::Choose) => msg!("Scale: choose by a percentage or by a length between two points, then OK."),
            Tool::Scale(scale::Step::Base(_)) => msg!("Click the point that stays: the selection grows or shrinks about it. Esc cancels."),
            Tool::Scale(scale::Step::First) => msg!("Click the first point, the one that stays. Esc cancels."),
            Tool::Scale(scale::Step::Second(_)) => msg!("Click the second point: then type the length between them. Esc cancels."),
            Tool::Scale(scale::Step::Length(..)) => msg!("Type the new length between the two points and press Enter. Esc cancels."),
            Tool::Mirror { draw: false, .. } => msg!("Pick a wall or grid to mirror across."),
            Tool::Mirror { draw: true, start: None } => msg!("Click the start of the mirror axis."),
            Tool::Mirror { draw: true, start: Some(_) } => msg!("Click the end of the mirror axis."),
            Tool::Align(None) => msg!("Pick a reference edge or centre line; the line under the cursor is highlighted."),
            Tool::Align(Some(_)) => msg!("Pick the element's highlighted edge to move and turn onto the reference."),
            Tool::Trim(None) => msg!("Pick the first wall or line, on the part to keep."),
            Tool::Trim(Some(_)) => msg!("Pick the second wall or line, on the part to keep."),
            Tool::TrimTo { boundary: None, .. } => msg!("Pick the line to trim or extend to: a line, a wall or a grid."),
            Tool::TrimTo { many: false, .. } => msg!("Click the line to trim or extend to it, on the part to keep."),
            Tool::TrimTo { many: true, .. } => {
                msg!("Click each line to trim or extend to it, on the part to keep; Esc picks another boundary.")
            }
            Tool::Vertex { delete: false } => msg!("Click a polyline or a spline where a vertex goes."),
            Tool::Vertex { delete: true } => msg!("Click a polyline's corner or a spline's point to take it out."),
            Tool::Split => msg!("Click a wall or a line where to split it."),
            Tool::Offset => msg!("Point to a wall or a line: the offset goes to the cursor's side. Click to place it."),
            Tool::Demolish => msg!("Click an element to take it down in the view's phase; click it again to take it back."),
            Tool::Linework => msg!("Click an edge to draw it in the line style in the Options Bar; <By Category> puts it back."),
            Tool::DetailGroup | Tool::ModelGroup => msg!("Click where the group's origin goes."),
            Tool::RepeatingDetail(None) => msg!("Click where the repeating detail starts."),
            Tool::RepeatingDetail(Some(_)) => msg!("Click where it ends: the group repeats along the line every spacing."),
            Tool::Paint => msg!("Click a face to paint it with the material in the Options Bar; the 3D view orbits as ever."),
            Tool::Unpaint => msg!("Click a painted face to take its paint off."),
            Tool::Dimension(_, refs) if refs.is_empty() => {
                msg!("Pick a wall face, centre line or end, a door, a window, a grid, or an endpoint to measure from.")
            }
            Tool::Dimension(_, refs) if refs.len() == 1 => msg!("Pick the next reference or endpoint."),
            Tool::Dimension(..) => msg!(
                "Pick more references, or click away from them where the dimension line goes; Tab steps over the faces, axes and \
                 corners of the element under the cursor. A picked reference clicked again is taken out, Backspace takes out the \
                 last, Esc starts over. Space steps Auto, Aligned, Horizontal, Vertical."
            ),
            Tool::Measure(None) => msg!("Click the ruler's start: points snap as they do drawing."),
            Tool::Measure(Some(_)) => msg!("Click its end: its length and angle read beside it. Esc cancels."),
            Tool::Tag => msg!("Click an element to tag it with its type, a door's or window's with its mark, or with the field in Label."),
            Tool::Angular(refs) if refs.len() < 2 => msg!("Pick a wall, a grid or another line; then one that meets it."),
            Tool::Angular(_) => msg!("Click where the arc goes: the angle on that side. Esc cancels."),
            Tool::Round(DimKind::ArcLength, picked) if picked.is_empty() => msg!("Pick a curved wall or an arc."),
            Tool::Round(_, picked) if picked.is_empty() => msg!("Pick a round column, a curved wall, a circle or an arc."),
            Tool::Round(DimKind::ArcLength, picked) if picked.len() == 2 => msg!("Pick the second point on the arc. Esc cancels."),
            Tool::Round(DimKind::ArcLength, _) => {
                msg!("Click where the dimension's arc goes, or pick two points on the arc to measure between. Esc cancels.")
            }
            Tool::Round(DimKind::Diameter, _) => msg!("Click where the diameter goes. Esc cancels."),
            Tool::Round(..) => msg!("Click where the radius goes. Esc cancels."),
            Tool::Text => msg!("Type the note in the Options Bar, then click where it goes: its top left corner."),
            Tool::Spot => msg!("Click a floor, a roof, a stair or a wall: the spot elevation reads the height of its top there."),
            Tool::SpotSlope => msg!("Click a floor or a roof: the spot slope reads how steeply its top falls there, and which way."),
            Tool::Section(None) => msg!("Click the section's start: it looks to the left of the line you draw."),
            Tool::Section(Some(_)) => msg!("Click the section's end."),
            Tool::Elevation => msg!("Click a straight wall, a line or a slab's edge for the elevation to look at."),
            Tool::Camera(None) => msg!("Click where the camera's eye stands: it looks at the point you click next."),
            Tool::Camera(Some(_)) => msg!("Click what the camera looks at; its eye and target heights are the Options Bar's."),
            Tool::Column => msg!("Click to place a column, as high as the Options Bar says."),
            Tool::Run(..) | Tool::Stand(_) => structure::hint(&self.tool).unwrap_or_default(),
            Tool::Component | Tool::Fixture(_) => msg!("Click where the component goes; Space turns it a quarter turn."),
            Tool::Stair(pts) if self.stair_shape == StairShape::Risers && pts.len() % 2 == 0 => {
                msg!("Click the left end of the next riser, going up. Enter makes the stair of the risers drawn; Esc cancels.")
            }
            Tool::Stair(_) if self.stair_shape == StairShape::Risers => msg!("Click the right end of this riser."),
            Tool::Stair(pts) if pts.is_empty() && self.stair_shape == StairShape::Spiral => {
                msg!("Click the centre the spiral stair winds about.")
            }
            Tool::Stair(_) if self.stair_shape == StairShape::Spiral => {
                msg!("Click where it starts: the middle of its first riser. Esc cancels.")
            }
            Tool::Stair(pts) if pts.is_empty() => {
                msg!("Click where the stair starts. It is drawn along the line you click, a flight or a landing at a time.")
            }
            Tool::Stair(_) if self.stair_landing => {
                msg!(
                    "Click where this landing ends. Steps, in the ribbon, goes on with a flight; a double click or Enter finishes the stair."
                )
            }
            Tool::Stair(_) => {
                msg!(
                    "Click where this flight ends: past the top, the stair is done. Landing, in the ribbon, draws a landing next; a double click or Enter finishes the stair as drawn, its top under its top level by what is not drawn."
                )
            }
            Tool::Ramp(pts) if pts.is_empty() && self.ramp_shape == RampShape::Circular => {
                msg!("Click the centre the ramp winds round.")
            }
            Tool::Ramp(_) if self.ramp_shape == RampShape::Circular => {
                msg!("Click the middle of its foot: it winds round the centre from there until it reaches its top.")
            }
            Tool::Ramp(pts) if pts.is_empty() => msg!("Click where the ramp starts: the middle of its foot."),
            Tool::Ramp(_) if self.ramp_landing => {
                msg!(
                    "Click where this landing ends. Flight, in the ribbon, goes on with a flight; a double click or Enter finishes the ramp."
                )
            }
            Tool::Ramp(_) => {
                msg!(
                    "Click where this flight ends, at the Options Bar's slope: past the top, the ramp is done. Landing, in the ribbon, draws a landing next; a double click or Enter finishes the ramp as drawn."
                )
            }
            Tool::Railing(pts) if pts.is_empty() => msg!("Click a stair or a ramp for a railing down each side, or the start of a path."),
            Tool::Railing(_) => msg!("Click the path's next point. Enter finishes, Esc cancels."),
            Tool::Roof => msg!("Click inside the walls to roof them, on the level above: the overhang runs past their faces."),
            Tool::ExtrusionRoof(pts) if pts.is_empty() => msg!("Click the start of the line the roof's profile runs along."),
            Tool::ExtrusionRoof(pts) if pts.len() == 1 => {
                msg!("Click the line's end: the profile rises from it at the Options Bar's slope.")
            }
            Tool::ExtrusionRoof(_) => msg!("Click how far across the roof is swept, to either side of the line. Esc cancels."),
            Tool::Dormer => msg!("Click a roof's slope where the dormer's front wall goes, across the slope."),
            Tool::GableEdge => msg!("Click an edge of a roof's footprint: it stands upright, a gable end; click it again to slope it."),
            Tool::Floor => msg!("Click inside the walls to floor them: the slab runs under their outline, its top at the level."),
            Tool::Ceiling => msg!("Click inside a room: a ceiling out to its walls' faces, as high as the Options Bar says."),
            Tool::DetailView(None) => msg!("Click one corner of the fragment to call out."),
            Tool::DetailView(Some(_)) => msg!("Click the fragment's opposite corner; the callout is made at the Options Bar's scale."),
            Tool::Shaft(None) => msg!("Click a corner of the shaft opening."),
            Tool::Shaft(Some(_)) => {
                msg!("Click the opposite corner: the shaft cuts the floors, ceilings and roofs up to its top. Esc cancels.")
            }
            Tool::Mass(pts) if pts.is_empty() => msg!("Click the first corner of the mass's footprint."),
            Tool::Mass(pts) if pts.len() < 3 => msg!("Click the footprint's next corner. Esc cancels."),
            Tool::Mass(_) => msg!("Click the next corner; Enter, or the first corner again, finishes the mass. Esc cancels."),
            Tool::DetailLine(pts) if pts.is_empty() => msg!("Click where the detail line starts; Q pins a guide there."),
            Tool::DetailLine(_) => msg!(
                "Click its next point; Enter finishes it, the first point again closes it; Tab steps length and angle; Backspace or Ctrl+Z takes back the last point and line; Q pins a guide. Esc cancels."
            ),
            Tool::SlopeArrow(None) => msg!("Click the slope arrow's tail: where the floor's top stays at its height offset."),
            Tool::SlopeArrow(Some(_)) => msg!("Click its head: the floor rises toward it at the Options Bar's slope. Esc cancels."),
            Tool::ModifySubElements => {
                msg!(
                    "Click a point, a corner or an edge to select it (Shift adds), then type its height; drag a point to move it; Delete removes it."
                )
            }
            Tool::AddPoint => {
                msg!("Click on the top: a point there, the Options Bar's offset over the surface, or at its Absolute elevation.")
            }
            Tool::AddSplitLine(None) => msg!("Click where the split line starts: a point, a corner or an edge of the top."),
            Tool::AddSplitLine(Some(_)) => msg!("Click where it ends: the top folds along it. Esc cancels."),
            Tool::SketchLine(_) | Tool::SketchPick => msg!("Click where the line starts."),
            Tool::PatternCorner(_) => msg!("Click on Bottom Left Corner"),
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            // The indicators at the right first, then the prompt in what is left, cut short with an ellipsis rather than
            // run under them.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // How many elements are selected, by the selection filter: on a family's tab, of the
                // family document's own selection. A click on it filters the selection.
                let selected = self.active_family().map_or(self.selection.len(), |ed| ed.selected_count());
                ui.label(format!(":{selected}"));
                self.export_progress(ui);
                let (r, funnel) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
                icons::paint(ui.painter(), Icon::Filter, r, pal().icon);
                funnel.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, t("Filter Selection")));
                if funnel.on_hover_text(t("Filter Selection")).clicked() {
                    // The family document's Filter on its tab (FE-20), never the project's from there.
                    match self.active_family_mut() {
                        Some(ed) => ed.open_filter(),
                        None => self.open_filter(),
                    }
                }
                // Drag Elements on Selection and Select Elements by Face, left of it: a family
                // document's that isn't adaptive, greyed where they change nothing ({3D}, a sketch).
                if let Some(usable) = self.active_family().filter(|ed| ed.adaptive.is_none()).map(|ed| ed.picks_by_face()) {
                    let on = self.drag_on_selection;
                    if ui.add_enabled_ui(usable, |ui| editor::selection::drag_toggle(ui, on)).inner.clicked() {
                        self.drag_on_selection = !on;
                    }
                    let on = self.select_by_face;
                    if ui.add_enabled_ui(usable, |ui| editor::selection::face_toggle(ui, on)).inner.clicked() {
                        self.select_by_face = !on;
                    }
                }
                if let Some(p) = self.canvas.cursor {
                    let u = self.session.model.units;
                    // Where the cursor is in the world too, once the project's shared coordinates aren't its own.
                    if let Some(shared) = self.shared_readout(p) {
                        ui.monospace(RichText::new(shared).color(pal().accent_ink)).on_hover_text(t(
                            "The cursor in shared coordinates: the active location's east and north, in the map system's unit",
                        ));
                    }
                    ui.monospace(format!("x {}  y {}", u.with_unit(p.0), u.with_unit(p.1)));
                }
                if let Some(note) = self.snap_note() {
                    ui.label(RichText::new(note).color(pal().accent_ink));
                }
                // The active document's: an open family's draft, else the project.
                let unsaved = self.active_family().map_or_else(|| self.dirty(), |ed| ed.unsaved());
                if unsaved {
                    ui.label(RichText::new(t("Unsaved changes")).color(pal().muted));
                }
                self.option_selector(ui);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(t(self.prompt())).color(pal().muted)).truncate());
                });
            });
        });
    }

    /// Why the last command failed, in a box over the views' bottom right corner until the next one works. Its text can be
    /// copied, so a bug report can quote it (issue 566). It is a layer of the views' own order, so it is raised to the top of it
    /// every frame: the windows a split lays out are layers of that order too, and one of them clicked or dragged after the
    /// note came up would otherwise hide it. Modal dialogs and menus stand above it in orders of their own.
    fn warning(&mut self, ctx: &Context) {
        if self.status.text.is_empty() {
            return;
        }
        let (title, color) = if self.status.error { (t("Error"), pal().error) } else { (t("Note"), pal().accent_ink) };
        // The whole views' area in Split and Tile, not the one window drawn last; the view under the tabs otherwise.
        let view =
            if !self.home.shown && !self.family_doc() && self.arrange != dock::Arrange::Tabs { self.views_rect } else { self.view_rect };
        let at = view.right_bottom() - vec2(16.0, 16.0);
        let copied = egui::Id::new("warning copied");
        let mut was: Option<String> = ctx.data_mut(|d| d.get_temp::<String>(copied));
        Area::new(UiId::new("warning")).order(Order::Middle).fixed_pos(at).pivot(Align2::RIGHT_BOTTOM).show(ctx, |ui| {
            Frame::popup(ui.style()).fill(pal().surface).stroke(Stroke::new(1.0, color)).show(ui, |ui| {
                ui.set_max_width(380.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(title).strong().color(color));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, Icon::Close, "Dismiss").clicked() {
                            self.status = Status::default();
                        }
                        let said = was.as_deref() == Some(self.status.text.as_str());
                        if icon_button(ui, Icon::Copy, if said { "Copied" } else { "Copy" }).clicked() {
                            ctx.copy_text(self.status.text.clone());
                            was = Some(self.status.text.clone());
                            ctx.request_repaint();
                        }
                    });
                });
                ui.add(egui::Label::new(RichText::new(&self.status.text)).selectable(true));
            });
        });
        // A new message hasn't been copied: the button says Copy again.
        ctx.data_mut(|d| {
            if self.status.text.is_empty() {
                d.remove::<String>(copied);
            } else {
                d.insert_temp(copied, was.unwrap_or_default());
            }
        });
        ctx.move_to_top(egui::LayerId::new(Order::Middle, UiId::new("warning")));
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
        // A family opened from the start page with no project behind it: the blank project's tabs aren't shown, until something
        // is loaded into it.
        let alone = self.family_from_home && !self.families.is_empty() && !self.dirty();
        let bar = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for sheet in self.views.clone().into_iter().filter(|s| !alone || matches!(s, Sheet::Family(..))) {
                // A ceiling plan's tab says so: its level's plan has the level's name too.
                let name = if matches!(sheet, Sheet::Ceiling(_)) { self.view_title(sheet) } else { self.view_name(sheet) };
                // A family document's tabs: the family's icon, the document named over it; they neither float nor tile, and the
                // last of a document closes it.
                let family = match sheet {
                    Sheet::Family(id, _) => Some(self.family_of(id).map(|ed| ed.title().to_string()).unwrap_or_default()),
                    _ => None,
                };
                let many = self.views.len() > 1;
                let sense = if family.is_some() { Sense::click() } else { Sense::click_and_drag() };
                let lead = if family.is_some() { 18.0 } else { 0.0 };
                let r = tab_button_with(ui, &name, sheet == self.front, false, sense, (lead, if many { 18.0 } else { 0.0 }));
                let r = match &family {
                    Some(doc) => {
                        let icon = Rect::from_center_size(pos2(r.rect.left() + 14.0, r.rect.center().y), vec2(14.0, 14.0));
                        icons::paint(
                            ui.painter(),
                            Icon::FamilyEditor,
                            icon,
                            if sheet == self.front { pal().accent_ink } else { pal().icon },
                        );
                        r.on_hover_text(tf("Family document: {}", &[doc]))
                    }
                    None => r,
                };
                self.tab_rects.push((sheet, r.rect));
                // The × is part of the tab, at its right end.
                if many {
                    let x = Rect::from_center_size(pos2(r.rect.right() - 16.0, r.rect.center().y), vec2(16.0, 16.0));
                    // A family document's last tab closes the document: Close Family, as its × said before it had tabs.
                    let last = matches!(sheet, Sheet::Family(id, _) if self.doc_tabs(id).len() == 1);
                    let label = if last { t(msg!("Close Family")).to_string() } else { tf("Close {}", &[&name]) };
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
                    if ui.add_enabled(many && family.is_none(), egui::Button::new(t("Float"))).clicked() {
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
        if let Some(id) = self.showing_panel_schedule() {
            return self.panel_schedule_view(ui, id);
        }
        if let Some(id) = self.showing_rebar_detail() {
            return self.rebar_detail_view(ui, id);
        }
        let Some((id, sc)) = self.shown_schedule() else { return };
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
            match schedule::table_kept(&self.session.model, id, &sc) {
                Err(e) => {
                    ui.label(e);
                }
                Ok(t) if t.rows.is_empty() => {
                    ui.label(RichText::new(tf("No rows yet: the {} is empty.", &[&name.to_lowercase()])).color(pal().muted));
                }
                Ok(table) => {
                    let (units, decimals) = (self.session.model.units, self.session.model.decimals);
                    let said = |c: &schedule::Column, v: &Value| {
                        let text = schedule::shown_to(units, decimals, c.kind, v);
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
        self.delete_rooms_window(ctx);
        self.group_conflict_window(ctx);
        self.group_split_window(ctx);
        self.keep_to_group();
        self.family_type_properties(ctx);
        self.materials_window(ctx);
        self.favorites_window(ctx);
        self.family_editor_window(ctx);
        self.sync_family_tab();
        self.closing_window(ctx);
        if let Some(p) = self.after_documents.take() {
            self.guard(p, ctx);
        }
        self.new_family_window(ctx);
        self.family_load_dialog(ctx);
        self.family_closed_from_home();
        self.snaps_window(ctx);
        self.parameters_window(ctx);
        self.classification_window(ctx);
        self.new_schedule_window(ctx);
        self.schedule_window(ctx);
        self.graphics_window(ctx);
        self.filters_window(ctx);
        self.templates_window(ctx);
        self.phasing_window(ctx);
        self.display_window(ctx);
        self.view_range_window(ctx);
        self.halftone_window(ctx);
        self.project_info_window(ctx);
        self.reinforce_window(ctx);
        self.steel_window(ctx);
        self.rebar_detail_window(ctx);
        self.line_weights_window(ctx);
        self.object_styles_window(ctx);
        self.line_styles_window(ctx);
        self.title_blocks_window(ctx);
        self.fill_patterns_window(ctx);
        self.text_type_window(ctx);
        self.dimension_type_window(ctx);
        self.spot_type_window(ctx);
        self.curtain_editor_window(ctx);
        self.go_to_view_window(ctx);
        self.curtain_message(ctx);
        self.grid_type_window(ctx);
        self.dimension_text_window(ctx);
        self.standards_window(ctx);
        self.codes_window(ctx);
        self.findings_window(ctx);
        self.coordinates_window(ctx);
        self.options_window(ctx);
        self.manage_links_window(ctx);
        self.asset_manager_window(ctx);
        self.pdf_page_window(ctx);
        self.purge_window(ctx);
        self.cowork_window(ctx);
        self.lend_window(ctx);
        self.quit_window(ctx);
        self.knock_window(ctx);
        self.pyaddins_windows(ctx);
        if std::mem::take(&mut self.link_dialog) {
            self.link_model_dialog(ctx);
        }
        if std::mem::take(&mut self.cloud_dialog) {
            self.point_cloud_dialog(ctx);
        }
        self.point_cloud_window(ctx);
        if let Some(link) = self.cad_dialog.take() {
            self.import_cad_dialog(ctx, link);
        }
        self.import_cad_window(ctx);
        self.link_window(ctx);
        self.select_by_id_window(ctx);
        self.run_script_window(ctx);
        self.work_plane_window(ctx);
        self.ids_of_selection_window(ctx);
        self.paste_window(ctx);
        self.scale_window(ctx);
        self.selection_filter_window(ctx);
        self.shortcuts_window(ctx);
        // The preset editor is the dialog on top: Escape closes it alone, leaving the Settings window behind it open.
        if self.preset_editor.is_some() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.preset_editor = None;
        }
        self.settings_window(ctx);
        self.preset_editor_window(ctx);
        self.new_project_window(ctx);
        self.plugins_window(ctx);
        self.plugin_report_window(ctx);
        self.plugin_form_window(ctx);
        self.background_grid_window(ctx);
        self.underlay_window(ctx);
        self.installing_box(ctx);
        self.importing_box(ctx);
        self.opening_box(ctx);
        self.report_window(ctx);
        self.export_window(ctx);
        self.publisher_window(ctx);
        self.print_sets_window(ctx);
        self.name_set_window(ctx);
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
        let r = Modal::new(UiId::new("unsaved"), tf("Save changes to {}?", &[&name])).width(580.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(t("If you don't, the changes since it was last saved are lost."));
            });
            crate::popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    for (label, c) in [("Save", Choice::Save), (msg!("Don't Save"), Choice::Discard), ("Cancel", Choice::Cancel)] {
                        if ui.button(t(label)).clicked() {
                            choice = Some(c);
                        }
                    }
                });
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

impl BimApp {
    /// Where the window paces its own frames: the present mode for its card, set once, and the wait that keeps frames from
    /// coming closer than the monitor's refresh. Only a frame's first pass waits: a sizing pass egui asks for comes at once.
    fn pace_frame(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        let Some(pacer) = self.frames.as_mut() else { return };
        if ctx.current_pass_index() > 0 {
            return;
        }
        if !pacer.configured {
            if let Some(state) = frame.wgpu_render_state() {
                let mut config = state.surface_config;
                config.present_mode = pacing::present_for(state.adapter.get_info().backend, config.present_mode);
                frame.set_wgpu_surface_config(config);
            }
            pacer.configured = true;
        }
        let now = std::time::Instant::now();
        let every = pacer.every(now, || frame.winit_window()?.current_monitor()?.refresh_rate_millihertz());
        let wait = pacer.wait(now, every);
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }
}

impl eframe::App for BimApp {
    /// Runs before each pass, and while the window is hidden too: agents and a Co-Work session's guests are answered when the
    /// window is minimised.
    fn logic(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        self.pace_frame(ctx, frame);
        if let Some(mut bench) = self.bench.take() {
            // eframe counts the frame before's wait as its processor time.
            let slept = self.frames.as_ref().map_or(0.0, |p| p.slept.0.as_secs_f32());
            bench.step(self, ctx, (frame.info().cpu_usage.unwrap_or(0.0) - slept).max(0.0));
            self.bench = Some(bench);
        }
        self.pump(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }

    /// Tab over the plan cycles through what's under the cursor, as in Revit (or moves between a shape's numbers),
    /// rather than moving the keyboard between widgets.
    fn raw_input_hook(&mut self, ctx: &Context, raw: &mut egui::RawInput) {
        if let Some(bench) = self.bench.as_mut() {
            bench.input(raw);
        }
        self.pace(&mut raw.events);
        // Flying, the pointer is hidden (and on X11, which can't lock it, still roams the window): a click would act on
        // whatever it happens to be over.
        if self.orbit.fly.is_some() {
            raw.events.retain(|e| !matches!(e, Event::PointerButton { .. }));
        }
        // The family document's views take Tab themselves (F10c), the project's canvas being out of sight. Tab steps the
        // reading's fields; Shift+Tab steps the lines of the wall under the pointer, which a chain in hand would
        // otherwise never reach, `Tab` being the fields' from its first point on. Where no chain is drawn, it is Tab.
        if self.canvas.hovering() && self.active_family().is_none() && !ctx.egui_wants_keyboard_input() {
            for event in &raw.events {
                if let Event::Key { key: Key::Tab, pressed: true, modifiers, .. } = event {
                    if modifiers.shift {
                        self.face_cycle = true;
                    } else {
                        self.tab_cycle = true;
                    }
                }
            }
            raw.events.retain(|e| !matches!(e, Event::Key { key: Key::Tab, .. }));
        }
    }
}
