//! The Properties palette, laid out as Revit's: the Type Selector, the filter with Edit Type,
//! the parameters grouped under Constraints, Dimensions, Identity Data and Other, and Apply. It
//! shows the selected element, else what the tool places, else the view. Every edit is a
//! command (`modify`, or `type` from the Type Properties dialog), so it is checked, rolled back
//! when it breaks a rule, and undone like any other change.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use bimcraft_core::element_json;
use bimcraft_core::family::{Kind as Holds, Val};
use bimcraft_core::model::{Element, Function, Guid, Id, Layer, Location, Model, PARAMETER_CATEGORIES, ParameterKind, Pattern};
use bimcraft_core::schedule::{self, Field, Filter, Op, Sort};
use bimcraft_core::snap::Kind as Snap;
use bimcraft_core::texture::{Kind as TextureKind, Texture};
use bimcraft_core::units::Units;
use egui::{
    Align, Align2, Button, Checkbox, ComboBox, Context, FontId, Frame, Grid, Id as UiId, Key, Layout, Margin, Modal, Popup, Rect, RichText,
    ScrollArea, Sense, Shape, Stroke, StrokeKind, TextEdit, TextureHandle, Ui, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::{Value, json};

use crate::icons::{self, Icon};
use crate::lang::{self, t, tf};
use crate::style::pal;
use crate::{BimApp, Sheet, Status, Tool};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Text,
    /// A number with no unit: an angle, a scale, a size on paper.
    Number,
    /// A length or an elevation, in the project's units.
    Length,
    /// `[x, y]`, lengths in the project's units, typed as `x, y`.
    Point,
    /// `[x, y]` on paper, in mm.
    PaperPoint,
    /// A mass's top outline: corners as `x, y`, parted by `;`, or blank for none.
    Outline,
    Flag,
    Level,
    /// A level, shown by name; read only.
    LevelName,
    /// A view, shown by name; read only.
    ViewName,
    /// A level, or none for a wall of unconnected height.
    TopLevel,
    /// A wall's height: typed when its top is unconnected, else what its levels make it.
    Height,
    /// A length, or nothing, which its hint names when blank (a plan's top at the level above, a
    /// view with no far clip): typed, or cleared.
    Limit(&'static str),
    /// Whether a view is cropped: on crops it to what it shows (`crop`), off uncrops it.
    Crop,
    /// Derived from the rest; read only.
    Shown,
    /// A length (or several), read only.
    LengthShown,
    /// A length some elements have and others don't (an extrusion roof's start and end): typed, and
    /// no row without one.
    LengthIf,
    /// An area in m², read only: shown in m² or ft².
    AreaShown,
    /// A length in m, read only.
    MetresShown,
    /// A dimension's values: lengths, or an angle's degrees.
    Values,
    /// A wall's location line: a drop-down.
    LocationLine,
    /// A detail line's line style: a drop-down of the project's.
    LineStyle,
    /// A filled region's type: a drop-down of the project's.
    RegionType,
    /// A filled region's boundary: a line style, or invisible lines (none).
    RegionLines,
    /// Which line of a run's section its heights are: a drop-down.
    Justification,
    /// Whether a roof or a floor is there (a wall's attachment); read only.
    Attached,
}

use Kind::*;

/// A parameter: its group, its field as `elements` names it (a field of a field after a dot, as
/// `view_range.cut`), its label, its editor.
type Row = (&'static str, &'static str, &'static str, Kind);

/// Field `key` of element `e`'s JSON, through the dots, shown as `kind`. The JSON leaves a number
/// out at 0 (a floor's offset), so a length or a number it hasn't got is 0; a `null` it has is
/// none (a section's far clip, which sees all there is).
fn field_of(e: &Value, key: &str, kind: Kind) -> Value {
    match key.split('.').try_fold(e, |v, k| v.get(k)) {
        Some(v) => v.clone(),
        None if matches!(kind, Length | Number) => json!(0.0),
        None => Value::Null,
    }
}

/// `{key: val}` for `modify`, nested through the dots: modify merges into fields that are objects.
fn nested(key: &str, val: Value) -> Value {
    key.rsplit('.').fold(val, |v, k| json!({ k: v }))
}

fn rows(category: &str) -> &'static [Row] {
    match category {
        "wall" => &[
            ("Constraints", "location", "Location Line", LocationLine),
            ("Constraints", "level", "Base Constraint", Level),
            ("Constraints", "base_offset", "Base Offset", Length),
            ("Constraints", "attach_base", "Base is Attached", Attached),
            ("Constraints", "top_level", "Top Constraint", TopLevel),
            ("Constraints", "height", "Unconnected Height", Height),
            ("Constraints", "attach_top", "Top is Attached", Attached),
            ("Dimensions", "length", "Length", LengthShown),
            ("Dimensions", "radius", "Radius", LengthShown),
            ("Other", "start", "Start", Point),
            ("Other", "end", "End", Point),
        ],
        "door" => &[
            ("Constraints", "level", "Level", LevelName),
            ("Identity Data", "mark", "Mark", Text),
            ("Other", "host", "Host Wall", Shown),
            ("Other", "offset", "Offset", Length),
            ("Other", "flip_facing", "Flip Facing", Flag),
            ("Other", "flip_hand", "Flip Hand", Flag),
        ],
        "window" => &[
            ("Constraints", "level", "Level", LevelName),
            ("Constraints", "sill", "Sill Height", Length),
            ("Identity Data", "mark", "Mark", Text),
            ("Other", "host", "Host Wall", Shown),
            ("Other", "offset", "Offset", Length),
            ("Other", "flip_facing", "Flip Facing", Flag),
        ],
        "room" => &[
            ("Constraints", "level", "Level", Level),
            ("Dimensions", "area_m2", "Area", AreaShown),
            ("Dimensions", "perimeter_m", "Perimeter", MetresShown),
            ("Identity Data", "number", "Number", Text),
            ("Identity Data", "name", "Name", Text),
            ("Other", "point", "Location", Point),
        ],
        "floor" => &[
            ("Constraints", "level", "Level", Level),
            ("Constraints", "offset", "Height Offset From Level", Length),
            ("Dimensions", "slope", "Slope (°)", Number),
            ("Dimensions", "area_m2", "Area", AreaShown),
            ("Dimensions", "thickness", "Thickness", LengthShown),
            ("Structural", "structural", "Structural", Flag),
        ],
        "ceiling" => &[
            ("Constraints", "level", "Level", Level),
            ("Constraints", "offset", "Height Offset From Level", Length),
            ("Dimensions", "area_m2", "Area", AreaShown),
            ("Dimensions", "thickness", "Thickness", LengthShown),
        ],
        "shaft" => &[
            ("Constraints", "level", "Base Constraint", Level),
            ("Constraints", "base_offset", "Base Offset", Length),
            ("Constraints", "top_level", "Top Constraint", TopLevel),
            ("Constraints", "height", "Unconnected Height", Height),
            ("Dimensions", "area_m2", "Area", AreaShown),
        ],
        "mass" => &[
            ("Constraints", "level", "Base Constraint", Level),
            ("Constraints", "base_offset", "Base Offset", Length),
            ("Constraints", "top_level", "Top Constraint", TopLevel),
            ("Constraints", "height", "Unconnected Height", Height),
            ("Dimensions", "top", "Top Outline", Outline),
            ("Dimensions", "area_m2", "Area", AreaShown),
            ("Dimensions", "volume_m3", "Volume (m³)", Shown),
            ("Dimensions", "gross_floor_area_m2", "Gross Floor Area", AreaShown),
            ("Identity Data", "name", "Name", Text),
        ],
        "level" => &[("Constraints", "elevation", "Elevation", Length), ("Identity Data", "name", "Name", Text)],
        "grid" => &[("Identity Data", "name", "Name", Text), ("Other", "start", "Start", Point), ("Other", "end", "End", Point)],
        "dimension" => {
            &[("Dimensions", "values", "Value", Values), ("Other", "kind", "Kind", Shown), ("Other", "at", "Line Through", Point)]
        }
        "text" => &[
            ("Text", "text", "Text", Text),
            ("Graphics", "size", "Text Size (mm)", Number),
            ("Other", "angle", "Rotation (°)", Number),
            ("Other", "at", "Location", Point),
        ],
        "line" => &[("Graphics", "style", "Line Style", LineStyle), ("Dimensions", "length", "Length", LengthShown)],
        "region" => &[
            ("Graphics", "type", "Region Type", RegionType),
            ("Graphics", "line_style", "Line Style", RegionLines),
            ("Dimensions", "area_m2", "Area", AreaShown),
        ],
        "spot" => {
            &[("Dimensions", "elevation", "Elevation", LengthShown), ("Other", "host", "Host", Shown), ("Other", "at", "Location", Point)]
        }
        "schedule" => &[("Identity Data", "name", "View Name", Text), ("Other", "of", "Category", Shown)],
        "view3d" => &[
            ("Identity Data", "name", "View Name", Text),
            ("Camera", "azimuth", "Azimuth (°)", Number),
            ("Camera", "elevation", "Elevation (°)", Number),
        ],
        "tag" => {
            &[("Graphics", "leader", "Leader Line", Flag), ("Other", "host", "Tagged Element", Shown), ("Other", "offset", "Offset", Point)]
        }
        "sheet" => &[
            ("Identity Data", "number", "Sheet Number", Text),
            ("Identity Data", "name", "Sheet Name", Text),
            ("Other", "size", "Size", Text),
        ],
        "viewport" => &[
            ("Identity Data", "title", "View Name", Shown),
            ("Graphics", "scale", "View Scale (1 : n)", Number),
            ("Other", "at", "Location", PaperPoint),
        ],
        "component" => &[
            ("Constraints", "level", "Level", Level),
            ("Constraints", "offset", "Height Offset From Level", Length),
            ("Other", "at", "Location", Point),
            ("Other", "angle", "Rotation (°)", Number),
        ],
        "column" => &[
            ("Constraints", "level", "Base Level", Level),
            ("Constraints", "base_offset", "Base Offset", Length),
            ("Constraints", "top_level", "Top Level", TopLevel),
            ("Constraints", "height", "Unconnected Height", Height),
            ("Other", "at", "Location", Point),
            ("Other", "angle", "Rotation (°)", Number),
        ],
        "structuralcolumn" | "footing" => &[
            ("Constraints", "level", "Base Level", Level),
            ("Constraints", "base_offset", "Base Offset", Length),
            ("Constraints", "top_level", "Top Level", TopLevel),
            ("Constraints", "height", "Unconnected Height", Height),
            ("Other", "at", "Location", Point),
            ("Other", "angle", "Rotation (°)", Number),
        ],
        run if bimcraft_core::model::RUN_CATEGORIES.contains(&run) => &[
            ("Constraints", "level", "Reference Level", Level),
            ("Constraints", "offset", "Start Level Offset", Length),
            ("Constraints", "end_offset", "End Level Offset", Limit("Same as start")),
            ("Constraints", "justify", "Z Justification", Justification),
            ("Dimensions", "length", "Length", LengthShown),
            ("Identity Data", "system", "System", Text),
            ("Other", "start", "Start", Point),
            ("Other", "end", "End", Point),
        ],
        "stair" => &[
            ("Constraints", "level", "Base Level", Level),
            ("Constraints", "top_level", "Top Level", TopLevel),
            ("Constraints", "height", "Unconnected Height", Height),
            ("Dimensions", "width", "Actual Run Width", Length),
            ("Dimensions", "riser", "Maximum Riser Height", Length),
            ("Dimensions", "tread", "Actual Tread Depth", Length),
            ("Dimensions", "risers", "Actual Number of Risers", Shown),
            ("Dimensions", "riser_height", "Actual Riser Height", LengthShown),
            ("Other", "start", "Start", Point),
        ],
        "ramp" => &[
            ("Constraints", "level", "Base Level", Level),
            ("Constraints", "top_level", "Top Level", TopLevel),
            ("Constraints", "height", "Unconnected Height", Height),
            ("Dimensions", "width", "Width", Length),
            ("Dimensions", "slope", "Max Slope (%)", Number),
            ("Dimensions", "length", "Length", LengthShown),
            ("Other", "start", "Start", Point),
        ],
        "railing" => &[
            ("Constraints", "level", "Level", Level),
            ("Constraints", "base_offset", "Base Offset", Length),
            ("Constraints", "host", "Host", Shown),
            ("Constraints", "side", "Side", Shown),
            ("Dimensions", "height", "Height", LengthShown),
        ],
        "roof" => &[
            ("Constraints", "level", "Base Level", Level),
            ("Constraints", "offset", "Base Offset", Length),
            ("Constraints", "extrusion.from", "Extrusion Start", LengthIf),
            ("Constraints", "extrusion.to", "Extrusion End", LengthIf),
            ("Dimensions", "slope", "Slope (°)", Number),
            ("Dimensions", "ridge", "Max Ridge Height", LengthShown),
            ("Dimensions", "area_m2", "Area", AreaShown),
            ("Other", "gable", "Gable Ends", Flag),
        ],
        "section" | "elevation" => &[
            ("Identity Data", "name", "View Name", Text),
            ("Extents", "crop", "Crop View", Crop),
            ("Extents", "depth", "Far Clip Offset", Limit("No Clip")),
            ("Other", "start", "Start", Point),
            ("Other", "end", "End", Point),
        ],
        "link" => &[
            ("Identity Data", "name", "Name", Text),
            ("Identity Data", "path", "File", Shown),
            ("Identity Data", "loaded", "Loaded", Shown),
            ("Other", "at", "Origin", Point),
            ("Other", "angle", "Angle", Number),
        ],
        _ => &[],
    }
}

/// A floor plan's parameters, read from and written to its level.
const PLAN: &[Row] = &[
    ("Identity Data", "name", "View Name", Text),
    ("Extents", "id", "Associated Level", LevelName),
    ("Extents", "elevation", "Level Elevation", Length),
    ("Extents", "crop", "Crop View", Crop),
    ("View Range", "view_range.top", "Top", Limit("Level Above")),
    ("View Range", "view_range.cut", "Cut Plane", Length),
    ("View Range", "view_range.bottom", "Bottom", Length),
    ("View Range", "view_range.depth", "View Depth", Length),
];

/// A plan view's parameters, read from and written to the plan view itself.
const PLAN_VIEW: &[Row] = &[
    ("Identity Data", "name", "View Name", Text),
    ("Extents", "level", "Associated Level", LevelName),
    ("Extents", "crop", "Crop View", Crop),
    ("View Range", "view_range.top", "Top", Limit("Level Above")),
    ("View Range", "view_range.cut", "Cut Plane", Length),
    ("View Range", "view_range.bottom", "Bottom", Length),
    ("View Range", "view_range.depth", "View Depth", Length),
];

/// A dependent view's: its view range and graphics are its parent's, so it shows the parent, and a crop of its own.
const PLAN_DEPENDENT: &[Row] = &[
    ("Identity Data", "name", "View Name", Text),
    ("Extents", "level", "Associated Level", LevelName),
    ("Extents", "parent", "Parent View", ViewName),
    ("Extents", "crop", "Crop View", Crop),
];

/// Type `name` of `category` as the project's JSON holds it: `types` is the project's `Types` as JSON.
fn type_json<'a>(types: &'a Value, category: &str, name: &str) -> &'a Value {
    let of = match category {
        "structuralcolumn" => &types["structural_column"],
        run if bimcraft_core::model::RUN_CATEGORIES.contains(&run) => &types["runs"][category],
        other => &types[other],
    };
    &of[name]
}

/// A type's parameters, as its Type Properties list them: its field, its label.
fn type_rows(category: &str) -> &'static [(&'static str, &'static str)] {
    match category {
        "wall" => &[("thickness", "Width")],
        "floor" => &[("thickness", "Default Thickness")],
        "door" => &[("width", "Width"), ("height", "Height")],
        "window" => &[("width", "Width"), ("height", "Height"), ("sill", "Default Sill Height")],
        "column" | "structuralcolumn" | "footing" => &[("width", "Width"), ("depth", "Depth")],
        run if bimcraft_core::model::RUN_CATEGORIES.contains(&run) => &[("width", "Width"), ("depth", "Depth")],
        "roof" => &[("thickness", "Thickness")],
        "ceiling" => &[("thickness", "Thickness"), ("grid", "Tile Size")],
        "railing" => &[("height", "Height"), ("spacing", "Baluster Spacing")],
        _ => &[],
    }
}

fn icon(category: &str) -> Icon {
    match category {
        "wall" => Icon::Wall,
        "door" => Icon::Door,
        "window" => Icon::Window,
        "room" => Icon::Room,
        "level" => Icon::Level,
        "grid" => Icon::Grid,
        "dimension" => Icon::Dimension,
        "section" => Icon::Section,
        "sheet" => Icon::Sheet,
        "viewport" => Icon::PlaceView,
        "elevation" => Icon::Elevation,
        "tag" => Icon::Tag,
        "schedule" => Icon::Schedule,
        "3d view" => Icon::View3D,
        "column" | "structuralcolumn" => Icon::Column,
        "beam" => Icon::Beam,
        "footing" => Icon::Footing,
        "stripfooting" => Icon::WallFooting,
        "pipe" => Icon::Pipe,
        "conduit" => Icon::Conduit,
        "cabletray" => Icon::CableTray,
        "wire" => Icon::Wire,
        "downconductor" => Icon::DownConductor,
        "groundconductor" => Icon::GroundConductor,
        "cable" => Icon::Cabling,
        "component" => Icon::Component,
        "stair" => Icon::Stair,
        "roof" => Icon::Roof,
        "floor" => Icon::Floor,
        "ceiling" | "ceiling plan" => Icon::Ceiling,
        "shaft" => Icon::Shaft,
        "mass" => Icon::Mass,
        "ramp" => Icon::Ramp,
        "railing" => Icon::Railing,
        "text" => Icon::Text,
        "line" => Icon::Line,
        "region" => Icon::FilledRegion,
        "spot" => Icon::Spot,
        "view3d" => Icon::View3D,
        _ => Icon::Plan,
    }
}

/// A value as the palette and schedules show it: numbers to the hundredth.
pub(crate) fn text(v: &Value) -> String {
    match v {
        Value::Number(n) => n.as_f64().map_or_else(String::new, |x| format!("{}", (x * 100.0).round() / 100.0 + 0.0)),
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(text).collect::<Vec<_>>().join(", "),
        Value::Bool(b) => (if *b { "Yes" } else { "No" }).into(),
        Value::Null => String::new(),
        Value::Object(_) => v.to_string(),
    }
}

pub(crate) fn number(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|x| x.is_finite())
}

/// A value of `kind` as the palette shows it, lengths and areas in `units`.
fn shown(kind: Kind, v: &Value, units: Units) -> String {
    let each = |f: &dyn Fn(f64) -> String| match v {
        Value::Array(a) => a.iter().filter_map(Value::as_f64).map(f).collect::<Vec<_>>().join(", "),
        v => v.as_f64().map_or_else(String::new, f),
    };
    match kind {
        Outline => match v {
            Value::Array(corners) => {
                let corner = |p: &Value| Some(format!("{}, {}", units.length(p.get(0)?.as_f64()?), units.length(p.get(1)?.as_f64()?)));
                corners.iter().filter_map(corner).collect::<Vec<_>>().join("; ")
            }
            _ => String::new(),
        },
        Length | LengthIf | Height | Limit(_) | Point | LengthShown | Values => each(&|x| units.length(x)),
        MetresShown => each(&|x| units.length(x * 1000.0)),
        AreaShown => each(&|x| {
            let (area, unit) = units.area(x * 1e6);
            format!("{area:.2} {unit}")
        }),
        _ => text(v),
    }
}

/// What was typed into a field of `kind`, as the value `modify` takes: lengths in `units`.
fn parse(kind: Kind, s: &str, units: Units) -> Result<Value, String> {
    let length = |typed: &str| units.parse(typed).ok_or_else(|| tf("`{}` is not a length", &[typed.trim()]));
    match kind {
        Number => number(s).map(|x| json!(x)).ok_or_else(|| tf("`{}` is not a number", &[s])),
        Length | LengthIf | Height => length(s).map(|x| json!(x)),
        Limit(_) if s.trim().is_empty() => Ok(Value::Null),
        Limit(_) => length(s).map(|x| json!(x)),
        Outline if s.trim().is_empty() => Ok(Value::Null),
        Outline => s.split(';').map(|corner| parse(Point, corner, units)).collect::<Result<Vec<_>, _>>().map(Value::Array),
        // Lengths with feet and inches hold spaces: a comma parts x from y.
        Point => match s.split(',').collect::<Vec<_>>()[..] {
            [x, y] => Ok(json!([length(x)?, length(y)?])),
            _ => Err(tf("`{}` is not a point; type x, y", &[s])),
        },
        PaperPoint => match s.split([',', ' ']).filter(|t| !t.trim().is_empty()).collect::<Vec<_>>()[..] {
            [x, y] if number(x).is_some() && number(y).is_some() => Ok(json!([number(x), number(y)])),
            _ => Err(tf("`{}` is not a point; type x, y in mm", &[s])),
        },
        _ => Ok(json!(s)),
    }
}

/// A drop-down of `options`, named by `label`; the one picked, if it changed.
pub(crate) fn combo(ui: &mut Ui, salt: (Id, &str), label: UiId, current: &str, options: &[String]) -> Option<String> {
    let mut picked = None;
    let c = ComboBox::from_id_salt(salt).selected_text(current).width(ui.available_width()).show_ui(ui, |ui| {
        for o in options {
            if ui.selectable_label(o == current, o).clicked() && o != current {
                picked = Some(o.clone());
            }
        }
    });
    c.response.labelled_by(label);
    picked
}

/// A parameter group's header: a bar across the palette that folds the group, as Revit's do.
/// Whether it was clicked.
pub(crate) fn group_header(ui: &mut Ui, group: &str, open: bool) -> bool {
    let (r, response) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::CollapsingHeader, true, open, group));
    let p = ui.painter();
    p.rect_filled(r, 0.0, if response.hovered() { pal().hover_fill } else { pal().bar });
    p.text(r.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, group, FontId::proportional(13.0), pal().text);
    // A chevron on the right, pointing down while the group is open.
    let c = pos2(r.right() - 12.0, r.center().y);
    let chevron =
        if open { [vec2(-4.0, -2.0), vec2(0.0, 2.0), vec2(4.0, -2.0)] } else { [vec2(-2.0, -4.0), vec2(2.0, 0.0), vec2(-2.0, 4.0)] };
    p.add(Shape::line(chevron.iter().map(|d| c + *d).collect(), Stroke::new(1.5, pal().icon)));
    if response.has_focus() {
        p.rect_stroke(r, 0.0, Stroke::new(2.0, pal().accent), StrokeKind::Inside);
    }
    response.clicked()
}

/// What the palette shows.
#[derive(Clone, Copy, PartialEq)]
enum Subject {
    Element(Id),
    /// Several selected elements, all of a category.
    Elements(&'static str),
    /// Several selected elements of different categories: how many.
    Mixed(usize),
    /// The elements a tool places, of a category.
    New(&'static str),
    Plan(Id),
    CeilingPlan,
    View3D,
    Schedule,
}

/// A type's layer as typed: what it does, what it's made of, how thick.
type LayerRow = (Function, String, String);

/// The categories whose types have layers: a wall's from its exterior in, the others' from the top down.
const LAYERED: [&str; 4] = ["wall", "floor", "roof", "ceiling"];

/// Each pattern as the Materials window names it.
const PATTERNS: [(Pattern, &str); 10] = [
    (Pattern::None, "None"),
    (Pattern::Solid, "Solid Fill"),
    (Pattern::Diagonal, "Diagonal"),
    (Pattern::Crosshatch, "Crosshatch"),
    (Pattern::Horizontal, "Horizontal"),
    (Pattern::Vertical, "Vertical"),
    (Pattern::Grid, "Square Grid"),
    (Pattern::Brick, "Brick"),
    (Pattern::Concrete, "Concrete"),
    (Pattern::Insulation, "Insulation"),
];

/// A kind of texture, as the Materials window names it.
pub(crate) fn texture_name(k: bimcraft_core::texture::Kind) -> &'static str {
    use bimcraft_core::texture::Kind::*;
    match k {
        Brick => "Brick",
        Block => "Block",
        Boards => "Boards",
        Concrete => "Concrete",
        Plaster => "Plaster",
        Tile => "Tile",
        Roof => "Roof Tiles",
        Fabric => "Fabric",
        Metal => "Metal",
        Insulation => "Insulation",
        Image => "Picture",
    }
}

fn pattern_name(p: Pattern) -> &'static str {
    PATTERNS.iter().find(|(q, _)| *q == p).map_or("None", |(_, n)| n)
}

/// The Materials window: the library's materials, the one picked to edit, its name as typed (a
/// rename once typed in), its colour as picked (applied once the mouse lets go).
pub(crate) struct MaterialsDialog {
    picked: String,
    name: String,
    color: Option<[u8; 3]>,
    /// The texture's fields as typed (a tile's width and height, the angle, the offset across and up), for the material and
    /// texture they were made from.
    texture: Option<(String, Texture, [String; 5])>,
    /// The swatch drawn of the texture, and what it was drawn of.
    swatch: Option<(String, TextureHandle)>,
}

impl MaterialsDialog {
    pub(crate) fn new(m: &Model) -> MaterialsDialog {
        let picked = m.materials.keys().next().cloned().unwrap_or_default();
        MaterialsDialog { name: picked.clone(), picked, color: None, texture: None, swatch: None }
    }
}

/// What a layer does, as the layer editor names it.
const FUNCTIONS: [(Function, &str); 5] = [
    (Function::Structure, "Structure"),
    (Function::Substrate, "Substrate"),
    (Function::Thermal, "Thermal"),
    (Function::Membrane, "Membrane"),
    (Function::Finish, "Finish"),
];

fn function_name(f: Function) -> &'static str {
    FUNCTIONS.iter().find(|(g, _)| *g == f).map_or("Structure", |(_, n)| n)
}

/// The Type Properties dialog: a type of a category, and its parameters as typed; a layered
/// type's layers, too.
pub(crate) struct TypeDialog {
    category: &'static str,
    name: String,
    /// The elements that were on the type when the dialog opened; OK puts them on the type shown.
    elements: Vec<Id>,
    fields: Vec<(&'static str, &'static str, String)>,
    /// The name being typed for a duplicate.
    duplicate: Option<String>,
    /// A layered type's layers, exterior or top first, as typed, and as they were: changed, they
    /// make it.
    layers: Vec<LayerRow>,
    layers_were: Vec<LayerRow>,
    /// A run type's section: whether it is round (then it has a diameter and no depth) and what it is made of ("" for nothing).
    shape: Option<(bool, String)>,
}

impl TypeDialog {
    pub(crate) fn new(m: &Model, category: &'static str, name: String, elements: Vec<Id>) -> TypeDialog {
        let (fields, layers) = (TypeDialog::fields(m, category, &name), TypeDialog::layers(m, category, &name));
        let shape = bimcraft_core::model::RUN_CATEGORIES.contains(&category).then(|| {
            let t = m.types.runs.get(category).and_then(|t| t.get(&name));
            (t.is_some_and(|t| t.round), t.map(|t| t.material.clone()).unwrap_or_default())
        });
        TypeDialog { category, name, elements, fields, duplicate: None, layers_were: layers.clone(), layers, shape }
    }

    /// The dialog showing type `name` as the model has it now.
    fn show(&mut self, m: &Model, name: String) {
        *self = TypeDialog::new(m, self.category, name, std::mem::take(&mut self.elements));
    }

    /// A type's sizes, as typed: lengths in the project's units.
    fn fields(m: &Model, category: &str, name: &str) -> Vec<(&'static str, &'static str, String)> {
        let types = serde_json::to_value(&m.types).unwrap_or_default();
        type_rows(category).iter().map(|(k, l)| (*k, *l, shown(Length, &type_json(&types, category, name)[k], m.units))).collect()
    }

    /// A layered type's layers, as typed: one structure layer as thick as it, without any.
    fn layers(m: &Model, category: &str, name: &str) -> Vec<LayerRow> {
        let Some((.., thickness, layers)) = m.layered_types().find(|(c, n, ..)| *c == category && *n == name) else { return Vec::new() };
        let typed = |l: &Layer| (l.function, l.material.clone(), m.units.length(l.thickness));
        match layers {
            [] => vec![(Function::Structure, String::new(), m.units.length(thickness))],
            layers => layers.iter().map(typed).collect(),
        }
    }

    /// The `type` command's parameters for the type as typed, or why they aren't lengths. Changed
    /// layers make the type, and how thick it is; `copy` sends them anyway, as a duplicate has them.
    fn params(&self, name: &str, units: Units, copy: bool) -> Result<Value, String> {
        let mut p = json!({"category": self.category, "name": name});
        let layered = copy || self.layers != self.layers_were;
        if layered {
            let layers = self.layers.iter().map(|(function, material, typed)| {
                let thickness = units.parse(typed).ok_or_else(|| tf("{}: `{}` is not a length", &[t("Thickness"), typed]))?;
                Ok(json!({"function": function, "material": material.trim(), "thickness": thickness}))
            });
            p["layers"] = Value::Array(layers.collect::<Result<_, String>>()?);
        }
        let round = self.shape.as_ref().is_some_and(|(round, _)| *round);
        for (key, label, typed) in self.fields.iter().filter(|(key, ..)| !(layered && *key == "thickness") && !(round && *key == "depth")) {
            p[*key] = json!(units.parse(typed).ok_or_else(|| tf("{}: `{}` is not a length", &[t(label), typed]))?);
        }
        if let Some((round, material)) = &self.shape {
            p["round"] = json!(round);
            p["material"] = json!(material);
        }
        Ok(p)
    }
}

impl BimApp {
    fn subject(&self) -> Subject {
        let m = &self.session.model;
        match (self.selection.as_slice(), &self.tool, self.sheet) {
            (&[id], _, _) => Subject::Element(id),
            ([_, _, ..], _, _) => {
                let categories: BTreeSet<&'static str> =
                    self.selection.iter().filter_map(|id| m.elements.get(id)).map(|e| e.category()).collect();
                match categories.first() {
                    Some(c) if categories.len() == 1 => Subject::Elements(c),
                    _ => Subject::Mixed(self.selection.len()),
                }
            }
            ([], Tool::Wall(_), _) => Subject::New("wall"),
            ([], Tool::Door, _) => Subject::New("door"),
            ([], Tool::Window, _) => Subject::New("window"),
            ([], Tool::Room, _) => Subject::New("room"),
            ([], Tool::Level, _) => Subject::New("level"),
            ([], Tool::Grid(_), _) => Subject::New("grid"),
            ([], Tool::Dimension(_) | Tool::Angular(_) | Tool::Radial(_), _) => Subject::New("dimension"),
            ([], Tool::Text, _) => Subject::New("text"),
            ([], Tool::DetailLine(_), _) => Subject::New("line"),
            ([], Tool::Spot, _) => Subject::New("spot"),
            ([], Tool::Tag, _) => Subject::New("tag"),
            ([], Tool::Section(_), _) => Subject::New("section"),
            ([], Tool::Elevation, _) => Subject::New("elevation"),
            ([], Tool::Column, _) => Subject::New("column"),
            ([], Tool::Run(category, _) | Tool::Stand(category), _) => Subject::New(category),
            ([], Tool::Component | Tool::Fixture(_), _) => Subject::New("component"),
            ([], Tool::Stair(_), _) => Subject::New("stair"),
            ([], Tool::Ramp(_), _) => Subject::New("ramp"),
            ([], Tool::Railing(_), _) => Subject::New("railing"),
            ([], Tool::Roof | Tool::ExtrusionRoof(_), _) => Subject::New("roof"),
            ([], Tool::Dormer, _) => Subject::New("wall"),
            ([], Tool::Floor, _) => Subject::New("floor"),
            ([], Tool::Ceiling, _) => Subject::New("ceiling"),
            ([], Tool::Shaft(_), _) => Subject::New("shaft"),
            ([], Tool::Mass(_), _) => Subject::New("mass"),
            // A section's or an elevation's view shows its own parameters.
            ([], _, Sheet::Section(id) | Sheet::Paper(id)) => Subject::Element(id),
            // Modify, and the tools that pick walls rather than place anything: the view.
            ([], _, Sheet::Plan(level)) => Subject::Plan(level),
            ([], _, Sheet::Ceiling(_)) => Subject::CeilingPlan,
            ([], _, Sheet::View3D) => Subject::View3D,
            ([], _, Sheet::Schedule(_) | Sheet::Table(_)) => Subject::Schedule,
        }
    }

    /// The type the selected elements share, if they do.
    fn shared_type(&self, m: &Model) -> Option<String> {
        let names: BTreeSet<String> =
            self.selection.iter().filter_map(|id| element_json(m, *id)["type"].as_str().map(String::from)).collect();
        if names.len() == 1 { names.into_iter().next() } else { None }
    }

    /// The family and type of the selected components, while they share them; with none selected,
    /// the Component tool's.
    pub(crate) fn component_type(&self) -> Option<(String, String)> {
        let m = &self.session.model;
        if self.selection.is_empty() {
            return self.placing();
        }
        let types: BTreeSet<(String, String)> = self
            .selection
            .iter()
            .map(|id| match m.elements.get(id) {
                Some(Element::Component(c)) => Some((c.family.clone(), c.type_name.clone())),
                _ => None,
            })
            .collect::<Option<_>>()?;
        if types.len() == 1 { types.into_iter().next() } else { None }
    }

    /// The type Edit Type opens: its category, its name, and the selected elements on it.
    pub(crate) fn type_subject(&self) -> Option<(&'static str, String, Vec<Id>)> {
        let m = &self.session.model;
        let category = match self.subject() {
            Subject::Element(id) => m.elements.get(id)?.category(),
            Subject::Elements(category) | Subject::New(category) => category,
            Subject::Mixed(_) | Subject::Plan(_) | Subject::CeilingPlan | Subject::View3D | Subject::Schedule => return None,
        };
        if category == "component" {
            return self.component_type().map(|(_, ty)| (category, ty, self.selection.clone()));
        }
        // A door or window of a family: that family's type.
        if let Some((_, ty)) = self.opening_pair(m, self.subject(), category) {
            return Some((category, ty, self.selection.clone()));
        }
        let name = match self.selection.is_empty() {
            false => self.shared_type(m),
            true => self.types.get(category).cloned().or_else(|| m.default_type(category).map(String::from)),
        };
        Some((category, name?, self.selection.clone()))
    }

    /// The family and type a door or window is of: the selected ones', while they share them; with none selected, what the tool places.
    /// None for one of the project's own types.
    fn opening_pair(&self, m: &Model, subject: Subject, category: &str) -> Option<(String, String)> {
        let of = |id: &Id| {
            m.elements.get(id).filter(|e| e.category() == category).and_then(|e| e.family().map(|(f, t)| (f.to_string(), t.to_string())))
        };
        match subject {
            Subject::Element(id) => of(&id),
            Subject::Elements(_) => {
                let pairs: BTreeSet<Option<(String, String)>> = self.selection.iter().map(of).collect();
                if pairs.len() == 1 { pairs.into_iter().next().flatten() } else { None }
            }
            Subject::New(c) if c == category => {
                let (family, ty) = self.opening_pick(category);
                family.zip(ty)
            }
            _ => None,
        }
    }

    /// The family and type the Door or Window tool places: the ones picked, while the project still has them, else the project's usual
    /// door or window of a family, else its own default type.
    pub(crate) fn opening_pick(&self, category: &str) -> (Option<String>, Option<String>) {
        let m = &self.session.model;
        if let Some(ty) = self.types.get(category) {
            match self.opening_families.get(category) {
                Some(f) if m.families.get(f).is_some_and(|x| x.types.contains_key(ty)) => return (Some(f.clone()), Some(ty.clone())),
                None if m.type_names(category).contains(&ty.as_str()) => return (None, Some(ty.clone())),
                _ => {}
            }
        }
        match m.default_family(category) {
            Some((f, ty)) => (Some(f.to_string()), Some(ty.to_string())),
            None => (None, m.default_type(category).map(String::from)),
        }
    }

    pub(crate) fn edit_type(&mut self) {
        if let Some((category, name, elements)) = self.type_subject() {
            let pair = if category == "component" {
                self.component_type()
            } else {
                self.opening_pair(&self.session.model, self.subject(), category)
            };
            match pair {
                Some((family, ty)) => self.family_dialog = Some(FamilyDialog::new(&self.session.model, family, ty, elements)),
                None => self.type_dialog = Some(TypeDialog::new(&self.session.model, category, name, elements)),
            }
        }
    }

    pub(crate) fn properties(&mut self, ui: &mut Ui) {
        let model = Arc::clone(&self.session.model);
        let subject = self.subject();
        self.type_selector(ui, &model, subject);
        let (filter, ids, rows) = match subject {
            Subject::Element(id) => match model.elements.get(id) {
                Some(e) => (format!("{} (1)", lang::category(e.category(), true)), vec![id], rows(e.category())),
                None => return,
            },
            Subject::Elements(category) => {
                (format!("{} ({})", lang::category(category, true), self.selection.len()), self.selection.clone(), rows(category))
            }
            Subject::Mixed(n) => (tf("Multiple Categories ({})", &[&n.to_string()]), Vec::new(), &[][..]),
            Subject::New(category) => (tf("New {}", &[&lang::category(category, true)]), Vec::new(), &[][..]),
            Subject::Plan(level) => {
                let rows = match self.session.model.plan_view(level) {
                    Some(v) if v.parent.is_some() => PLAN_DEPENDENT,
                    Some(_) => PLAN_VIEW,
                    None => PLAN,
                };
                (self.view_title(Sheet::Plan(level)), vec![level], rows)
            }
            Subject::CeilingPlan | Subject::View3D | Subject::Schedule => (self.view_title(self.sheet), Vec::new(), &[][..]),
        };
        ui.horizontal(|ui| {
            ui.label(filter);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add_enabled(self.type_subject().is_some(), Button::new(t("Edit Type"))).clicked() {
                    self.edit_type();
                }
            });
        });
        ui.add_space(4.0);
        if !ids.is_empty() {
            self.parameters(ui, &model, &ids, rows);
            self.component_parameters(ui, &model, &ids);
            self.project_parameters(ui, &model, &ids);
            self.element_phasing(ui, &model, &ids);
        }
        if subject == Subject::Schedule {
            self.schedule_parameters(ui);
        }
        // A 3D view on a sheet: where it looks from, too.
        if let Subject::Element(id) = subject
            && let Some(bimcraft_core::model::Element::Viewport(vp)) = model.elements.get(id)
            && let Some(bimcraft_core::model::Element::View3D(_)) = model.elements.get(vp.view)
        {
            self.parameters(ui, &model, &[model.elements.id(vp.view).unwrap_or_default()], self::rows("view3d"));
        }
        // A view's graphics: the template it follows, and its overrides.
        let view_shown = matches!(subject, Subject::Plan(_) | Subject::CeilingPlan | Subject::View3D)
            || (matches!(subject, Subject::Element(_)) && self.selection.is_empty() && matches!(self.sheet, Sheet::Section(_)));
        if view_shown {
            self.graphics_properties(ui);
            self.phasing_properties(ui);
        }
        ui.add_space(8.0);
        // Fields commit on Enter or when they lose the keyboard, which a click here does.
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let _ = ui.button(t("Apply"));
        });
    }

    /// The Type Selector: the category and the type of what the palette shows, a drop-down of
    /// the category's types.
    fn type_selector(&mut self, ui: &mut Ui, model: &Model, subject: Subject) {
        let (category, current) = match subject {
            Subject::Element(id) => match model.elements.get(id) {
                Some(e) => (e.category(), element_json(model, id)["type"].as_str().map(String::from)),
                None => return,
            },
            Subject::Elements(category) => (category, self.shared_type(model)),
            Subject::Mixed(_) => ("multiple categories", None),
            Subject::New(category) => {
                (category, self.types.get(category).cloned().or_else(|| model.default_type(category).map(String::from)))
            }
            Subject::Plan(_) => ("floor plan", None),
            Subject::CeilingPlan => ("ceiling plan", None),
            Subject::View3D => ("3d view", None),
            Subject::Schedule => ("schedule", None),
        };
        // A component's: its family over its type; every family's types to pick from. A door's or a window's: the project's own
        // types, then those of its families, each as `Family: Type`.
        let hosted = match category {
            "door" => Some(bimcraft_core::family::Category::Door),
            "window" => Some(bimcraft_core::family::Category::Window),
            _ => None,
        };
        let pairs: Vec<(String, String)> = model
            .families
            .iter()
            .filter(|(_, x)| match (category, &self.tool) {
                ("component", Tool::Fixture(key)) => crate::mep::kinds(key).contains(&x.category),
                ("component", _) => !x.category.hosted(),
                _ => hosted == Some(x.category),
            })
            .flat_map(|(f, x)| x.types.keys().map(move |t| (f.clone(), t.clone())))
            .collect();
        let label = |(f, t): &(String, String)| format!("{f}: {t}");
        let picked_pair =
            if category == "component" { self.component_type() } else { hosted.and_then(|_| self.opening_pair(model, subject, category)) };
        let current = match &picked_pair {
            Some(pair) => Some(label(pair)),
            None if category == "component" => None,
            None => current,
        };
        let names: Vec<String> = if category == "component" {
            pairs.iter().map(label).collect()
        } else {
            model.type_names(category).into_iter().map(String::from).chain(pairs.iter().map(label)).collect()
        };
        let enabled = !names.is_empty();
        let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 50.0), if enabled { Sense::click() } else { Sense::hover() });
        r.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, enabled, t("Type Selector")));
        let p = ui.painter();
        p.rect(
            rect,
            3.0,
            pal().surface,
            Stroke::new(1.0, if r.hovered() && enabled { pal().hover } else { pal().border }),
            StrokeKind::Inside,
        );
        let swatch = Rect::from_min_size(rect.min + vec2(5.0, 5.0), vec2(40.0, 40.0));
        p.rect_filled(swatch, 2.0, pal().bar);
        icons::paint(p, icon(category), swatch.shrink(8.0), pal().icon);
        let x = swatch.right() + 10.0;
        let family = match subject {
            Subject::Plan(_) => t("Floor Plan").to_string(),
            Subject::CeilingPlan => t("Ceiling Plan").to_string(),
            Subject::View3D => t("3D View").to_string(),
            Subject::Mixed(_) => t("Multiple Categories").to_string(),
            _ => lang::category(category, false),
        };
        let (family, below) = match &picked_pair {
            Some((f, t)) => (f.clone(), Some(t.clone())),
            None => (family, current.clone()),
        };
        let first = if below.is_some() { rect.top() + 16.0 } else { rect.center().y };
        p.text(egui::pos2(x, first), Align2::LEFT_CENTER, family, FontId::proportional(13.0), pal().text);
        if let Some(t) = &below {
            p.text(egui::pos2(x, rect.top() + 34.0), Align2::LEFT_CENTER, t, FontId::proportional(12.5), pal().muted);
        }
        if enabled {
            let c = egui::pos2(rect.right() - 14.0, rect.center().y);
            p.add(Shape::convex_polygon(vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)], pal().icon, Stroke::NONE));
        }
        if r.has_focus() {
            p.rect_stroke(rect, 3.0, Stroke::new(2.0, pal().accent), StrokeKind::Inside);
        }
        let mut picked = None;
        Popup::menu(&r).width(rect.width()).show(|ui| {
            for n in &names {
                if ui.selectable_label(current.as_ref() == Some(n), n).clicked() {
                    picked = Some(n.clone());
                }
            }
        });
        let pair = picked.as_ref().and_then(|n| pairs.iter().find(|p| label(p) == *n)).cloned();
        match (picked, subject) {
            (Some(n), Subject::Element(_) | Subject::Elements(_)) if current.as_ref() != Some(&n) && pair.is_some() => {
                if let Some((f, t)) = pair {
                    self.run("modify", json!({"ids": self.selection, "family": f, "type": t}));
                }
            }
            (Some(_), Subject::New("component")) => self.component = pair,
            (Some(_), Subject::New(category @ ("door" | "window"))) if pair.is_some() => {
                if let Some((f, t)) = pair {
                    self.opening_families.insert(category, f);
                    self.types.insert(category, t);
                }
            }
            (Some(n), Subject::Element(_) | Subject::Elements(_)) if current.as_ref() != Some(&n) => {
                // One of the project's own types takes a door or window off the family it was of.
                let mut p = json!({"ids": self.selection, "type": n});
                if hosted.is_some() {
                    p["family"] = Value::Null;
                }
                self.run("modify", p);
            }
            (Some(n), Subject::New(category)) => {
                self.opening_families.remove(category);
                self.types.insert(category, n);
            }
            _ => {}
        }
    }

    /// The element's parameters, group by group.
    /// The elements' parameters, group by group: what they share shows, what varies is blank.
    /// An edit changes all of them, in one step.
    fn parameters(&mut self, ui: &mut Ui, model: &Model, ids: &[Id], rows: &[Row]) {
        let Some(&id) = ids.first() else { return };
        let all: Vec<Value> = ids.iter().map(|id| element_json(model, *id)).collect();
        let first = element_json(model, id);
        let v: serde_json::Map<String, Value> = rows
            .iter()
            .map(|r| (r.1, r.3))
            .chain([("top_level", Shown), ("enclosed", Shown)])
            .map(|(k, kind)| {
                let one = field_of(&first, k, kind);
                let shared = all.iter().all(|e| field_of(e, k, kind) == one);
                (k.to_string(), if shared { one } else { Value::Null })
            })
            .collect();
        let v = Value::Object(v);
        let levels: Vec<String> = model.levels().iter().map(|(_, l)| l.name.clone()).collect();
        // What an element points at is in its JSON as a GUID, or as an id for the one beside it.
        let id_of = |v: &Value| v.as_u64().or_else(|| v.as_str().and_then(Guid::parse).and_then(|g| model.elements.id(g)));
        let level_name = |v: &Value| id_of(v).and_then(|l| model.level(l)).map_or_else(String::new, |l| l.name.clone());
        let mut groups: Vec<&'static str> = Vec::new();
        for (g, ..) in rows {
            if !groups.contains(g) {
                groups.push(g);
            }
        }
        let mut change: Option<(&str, Result<Value, String>)> = None;
        let mut crop = None;
        let units = model.units;
        // A note on a sheet sits in paper mm.
        let paper = matches!(model.elements.get(id), Some(Element::Text(n)) if model.sheet(n.view).is_some());
        for group in groups {
            let open = !self.collapsed.contains(group);
            if group_header(ui, t(group), open) && !self.collapsed.remove(group) {
                self.collapsed.insert(group);
            }
            if !open {
                continue;
            }
            Frame::new().inner_margin(Margin { left: 8, right: 0, top: 2, bottom: 6 }).show(ui, |ui| {
                // Half the palette each, as Revit's columns: a long label wraps rather than pushing
                // its field past the palette's edge.
                let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                Grid::new(("properties", id, group)).num_columns(2).striped(true).min_col_width(half).max_col_width(half).show(ui, |ui| {
                    for &(_, key, label, kind) in rows.iter().filter(|r| r.0 == group) {
                        let val = &v[key];
                        // A length one element hasn't got (a straight wall's radius): no row.
                        if ids.len() == 1 && matches!(kind, LengthShown | LengthIf) && val.is_null() {
                            continue;
                        }
                        let kind = match kind {
                            Height if !v["top_level"].is_null() => LengthShown,
                            Point if paper => PaperPoint,
                            kind => kind,
                        };
                        let label = t(label);
                        let name = ui.label(label);
                        match kind {
                            Shown => {
                                ui.label(RichText::new(text(val)).color(pal().muted));
                            }
                            // An angle's degrees; else lengths.
                            Values if v["kind"] == "angular" => {
                                ui.label(RichText::new(format!("{}°", text(val))).color(pal().muted));
                            }
                            LengthShown | AreaShown | MetresShown | Values => {
                                ui.label(RichText::new(shown(kind, val, units)).color(pal().muted));
                            }
                            LevelName => {
                                ui.label(RichText::new(level_name(val)).color(pal().muted));
                            }
                            ViewName => {
                                let name = id_of(val).and_then(|v| model.view_name(v)).unwrap_or_default();
                                ui.label(RichText::new(name).color(pal().muted));
                            }
                            Crop => {
                                let mut on = !val.is_null();
                                let r = ui.checkbox(&mut on, "");
                                r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, label));
                                if r.labelled_by(name.id).changed() {
                                    crop = Some(on);
                                }
                            }
                            Flag => {
                                let mut on = val.as_bool().unwrap_or(false);
                                let r = ui.checkbox(&mut on, "");
                                // A checkbox without text names itself "" for screen readers: give it its row's name.
                                r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, label));
                                if r.labelled_by(name.id).changed() {
                                    change = Some((key, Ok(json!(on))));
                                }
                            }
                            Level => {
                                if let Some(l) = combo(ui, (id, key), name.id, &level_name(val), &levels) {
                                    change = Some((key, Ok(json!(l))));
                                }
                            }
                            TopLevel => {
                                let none = t("Unconnected");
                                let options: Vec<String> = std::iter::once(none.to_string()).chain(levels.iter().cloned()).collect();
                                let current = if val.is_null() { none.to_string() } else { level_name(val) };
                                if let Some(l) = combo(ui, (id, key), name.id, &current, &options) {
                                    change = Some((key, Ok(if l == none { Value::Null } else { json!(l) })));
                                }
                            }
                            // Shown by its label; a centre line by default, which the wall's JSON leaves out.
                            LocationLine => {
                                let now = val.as_str().and_then(Location::from_name).unwrap_or_default();
                                let options: Vec<String> = Location::ALL.iter().map(|l| t(l.label()).to_string()).collect();
                                if let Some(l) = combo(ui, (id, key), name.id, t(now.label()), &options)
                                    && let Some(loc) = Location::ALL.iter().find(|loc| t(loc.label()) == l)
                                {
                                    change = Some((key, Ok(json!(loc.name()))));
                                }
                            }
                            LineStyle => {
                                let options: Vec<String> = model.line_styles.keys().cloned().collect();
                                if let Some(s) = combo(ui, (id, key), name.id, val.as_str().unwrap_or_default(), &options) {
                                    change = Some((key, Ok(json!(s))));
                                }
                            }
                            RegionType => {
                                let options: Vec<String> = model.region_types.keys().cloned().collect();
                                if let Some(s) = combo(ui, (id, key), name.id, val.as_str().unwrap_or_default(), &options) {
                                    change = Some((key, Ok(json!(s))));
                                }
                            }
                            // Invisible lines first, then the styles; picked, none is null.
                            RegionLines => {
                                let invisible = t("<Invisible lines>").to_string();
                                let options: Vec<String> =
                                    std::iter::once(invisible.clone()).chain(model.line_styles.keys().cloned()).collect();
                                let now = val.as_str().map_or(invisible.clone(), str::to_string);
                                if let Some(s) = combo(ui, (id, key), name.id, &now, &options) {
                                    change = Some((key, Ok(if s == invisible { Value::Null } else { json!(s) })));
                                }
                            }
                            Justification => {
                                let names = [("top", "Top"), ("centre", "Centre"), ("bottom", "Bottom")];
                                let now = names.iter().find(|(k, _)| val.as_str() == Some(k)).map_or("Centre", |(_, n)| n);
                                let options: Vec<String> = names.iter().map(|(_, n)| t(n).to_string()).collect();
                                if let Some(j) = combo(ui, (id, key), name.id, t(now), &options)
                                    && let Some((k, _)) = names.iter().find(|(_, n)| t(n) == j)
                                {
                                    change = Some((key, Ok(json!(k))));
                                }
                            }
                            // Revit's greyed checkbox: Attach Top/Base and Detach Top/Base change it.
                            Attached => {
                                let mut on = !val.is_null();
                                let r = ui.add_enabled(false, Checkbox::new(&mut on, ""));
                                r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, false, on, label));
                                r.labelled_by(name.id);
                            }
                            Text | Number | Length | LengthIf | Point | PaperPoint | Outline | Height | Limit(_) => {
                                // Blank across a selection may be values that differ, not nothing.
                                let hint = match kind {
                                    Limit(hint) if ids.len() == 1 => t(hint),
                                    _ => "",
                                };
                                if let Some(typed) = self.field(ui, id, key, shown(kind, val, units), name.id, hint) {
                                    change = Some((key, parse(kind, &typed, units)));
                                }
                            }
                        }
                        ui.end_row();
                    }
                });
            });
        }
        if ids.len() == 1 && v["enclosed"] == json!(false) {
            ui.label(RichText::new(t("Not enclosed: the walls around it don't close.")).color(pal().error));
        }
        // A view's crop: on to all it shows, or off.
        if let (Some(on), &[view]) = (crop, ids) {
            self.run("crop", if on { json!({"view": view}) } else { json!({"view": view, "region": null}) });
        }
        match change {
            Some((key, Ok(val))) => {
                let mut p = nested(key, val);
                p["ids"] = json!(ids);
                self.run("modify", p);
            }
            Some((key, Err(e))) => self.status = Status { text: format!("{key}: {e}"), error: true },
            None => {}
        }
    }

    /// A text field that edits as the user types and commits once, when they press Enter or
    /// click away (Escape drops the edit): the text typed, if it changed.
    pub(crate) fn field(&mut self, ui: &mut Ui, id: Id, key: &str, current: String, label: UiId, hint: &str) -> Option<String> {
        let wid = ui.make_persistent_id(("field", id, key));
        let focused = ui.memory(|m| m.has_focus(wid));
        let mut typed = match &self.edit {
            Some((i, k, t)) if focused && *i == id && k == key => t.clone(),
            _ => current.clone(),
        };
        let r = ui.add(TextEdit::singleline(&mut typed).id(wid).desired_width(f32::INFINITY).hint_text(hint)).labelled_by(label);
        if r.has_focus() {
            self.edit = Some((id, key.to_string(), typed));
            return None;
        }
        if r.lost_focus() {
            self.edit = None;
            if typed != current && !ui.input(|i| i.key_pressed(Key::Escape)) {
                return Some(typed);
            }
        }
        None
    }

    /// The selected components' instance parameters, while they're of one family: as its
    /// parameters name them, lengths and numbers under Dimensions, materials under Materials and
    /// Finishes, yes or no under Other; what a formula works out, shown. An edit changes them all.
    fn component_parameters(&mut self, ui: &mut Ui, model: &Model, ids: &[Id]) {
        let families: BTreeSet<&str> = ids.iter().filter_map(|id| model.elements.get(id)?.family().map(|(f, _)| f)).collect();
        let families: Vec<&str> = families.into_iter().collect();
        let (Some(&id), [name]) = (ids.first(), families.as_slice()) else { return };
        let Some(f) = model.families.get(*name).filter(|_| ids.iter().all(|i| model.elements.get(i).is_some_and(|e| e.family().is_some())))
        else {
            return;
        };
        let values: Vec<Value> = ids.iter().map(|i| element_json(model, *i)["values"].clone()).collect();
        let shared = |k: &str| {
            let v = values.first().map_or(&Value::Null, |v| &v[k]);
            if values.iter().all(|x| &x[k] == v) { v.clone() } else { Value::Null }
        };
        let (units, materials): (Units, Vec<String>) = (model.units, model.materials.keys().cloned().collect());
        let mut change: Option<(String, Result<Value, String>)> = None;
        for (group, kinds) in [
            ("Dimensions", &[Holds::Length, Holds::Number, Holds::Angle][..]),
            ("Materials and Finishes", &[Holds::Material][..]),
            ("Other", &[Holds::YesNo][..]),
        ] {
            let params: Vec<_> = f.parameters.iter().filter(|p| p.instance && kinds.contains(&p.kind)).collect();
            if params.is_empty() {
                continue;
            }
            let open = !self.collapsed.contains(group);
            if group_header(ui, t(group), open) && !self.collapsed.remove(group) {
                self.collapsed.insert(group);
            }
            if !open {
                continue;
            }
            Frame::new().inner_margin(Margin { left: 8, right: 0, top: 2, bottom: 6 }).show(ui, |ui| {
                Grid::new(("component", id, group)).num_columns(2).striped(true).min_col_width(124.0).show(ui, |ui| {
                    for p in params {
                        let val = shared(&p.name);
                        let name = ui.label(&p.name);
                        let kind = if p.kind == Holds::Length { Length } else { Number };
                        match p.kind {
                            _ if p.formula.is_some() => {
                                ui.label(RichText::new(shown(kind, &val, units)).color(pal().muted));
                            }
                            Holds::YesNo => {
                                let mut on = val.as_f64().is_some_and(|x| x != 0.0);
                                let r = ui.checkbox(&mut on, "");
                                r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, &p.name));
                                if r.labelled_by(name.id).changed() {
                                    change = Some((p.name.clone(), Ok(json!(on))));
                                }
                            }
                            Holds::Material => {
                                if let Some(m) = combo(ui, (id, &p.name), name.id, val.as_str().unwrap_or_default(), &materials) {
                                    change = Some((p.name.clone(), Ok(json!(m))));
                                }
                            }
                            Holds::Length | Holds::Number | Holds::Angle => {
                                if let Some(typed) = self.field(ui, id, &p.name, shown(kind, &val, units), name.id, "") {
                                    change = Some((p.name.clone(), parse(kind, &typed, units)));
                                }
                            }
                        }
                        ui.end_row();
                    }
                });
            });
        }
        match change {
            Some((key, Ok(val))) => {
                self.run("modify", json!({"ids": ids, "params": {key: val}}));
            }
            Some((key, Err(e))) => self.status = Status { text: format!("{key}: {e}"), error: true },
            None => {}
        }
    }

    /// The selected elements' values of the project's parameters for their category, in a group of
    /// their own: a field for each, a checkbox for yes or no. An edit sets them all; emptied, a
    /// field takes their values away.
    fn project_parameters(&mut self, ui: &mut Ui, model: &Model, ids: &[Id]) {
        let cats: BTreeSet<&str> = ids.iter().filter_map(|id| model.elements.get(id)).map(Element::category).collect();
        let (Some(&id), Some(&cat)) = (ids.first(), cats.first()) else { return };
        let params: Vec<(&String, ParameterKind)> = model
            .parameters
            .iter()
            .filter(|(_, p)| cats.len() == 1 && p.categories.iter().any(|c| c == cat))
            .map(|(n, p)| (n, p.kind))
            .collect();
        if params.is_empty() {
            return;
        }
        // Blank across a selection may be values that differ, not none.
        let value = |name: &str| -> Value {
            let of = |i: &Id| model.elements.get(i).and_then(Element::params).and_then(|p| p.get(name)).cloned().unwrap_or(Value::Null);
            let first = of(&id);
            if ids.iter().all(|i| of(i) == first) { first } else { Value::Null }
        };
        let group = "Project Parameters";
        let open = !self.collapsed.contains(group);
        if group_header(ui, t(group), open) && !self.collapsed.remove(group) {
            self.collapsed.insert(group);
        }
        if !open {
            return;
        }
        let units = model.units;
        let mut change: Option<(String, Result<Value, String>)> = None;
        Frame::new().inner_margin(Margin { left: 8, right: 0, top: 2, bottom: 6 }).show(ui, |ui| {
            let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
            Grid::new(("project-parameters", id)).num_columns(2).striped(true).min_col_width(half).max_col_width(half).show(ui, |ui| {
                for (name, kind) in &params {
                    let val = value(name);
                    let label = ui.label(name.as_str());
                    let field = match kind {
                        ParameterKind::YesNo => {
                            let mut on = val.as_bool().unwrap_or(false);
                            let r = ui.checkbox(&mut on, "");
                            r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, name.as_str()));
                            if r.labelled_by(label.id).changed() {
                                change = Some((name.to_string(), Ok(json!(on))));
                            }
                            ui.end_row();
                            continue;
                        }
                        ParameterKind::Length => Length,
                        ParameterKind::Number => Number,
                        ParameterKind::Text => Text,
                    };
                    if let Some(typed) = self.field(ui, id, &format!("params.{name}"), shown(field, &val, units), label.id, "") {
                        let v = if typed.trim().is_empty() { Ok(Value::Null) } else { parse(field, &typed, units) };
                        change = Some((name.to_string(), v));
                    }
                    ui.end_row();
                }
            });
        });
        match change {
            Some((name, Ok(v))) => {
                self.run("modify", json!({"ids": ids, "params": {name: v}}));
            }
            Some((name, Err(e))) => self.status = Status { text: format!("{name}: {e}"), error: true },
            None => {}
        }
    }

    /// A schedule view's own parameters, as Revit's palette has them: its name and category, and
    /// its fields, filters and sorting, each Edit… opening Schedule Properties on its tab.
    fn schedule_parameters(&mut self, ui: &mut Ui) {
        let Some((made, sc)) = self.shown_schedule() else { return };
        let mut open = None;
        let mut rename = None;
        for group in ["Identity Data", "Other"] {
            let unfolded = !self.collapsed.contains(group);
            if group_header(ui, t(group), unfolded) && !self.collapsed.remove(group) {
                self.collapsed.insert(group);
            }
            if !unfolded {
                continue;
            }
            Frame::new().inner_margin(Margin { left: 8, right: 0, top: 2, bottom: 6 }).show(ui, |ui| {
                let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                Grid::new(("schedule-properties", group)).num_columns(2).striped(true).min_col_width(half).max_col_width(half).show(
                    ui,
                    |ui| {
                        if group == "Identity Data" {
                            let label = ui.label(t("View Name"));
                            match made {
                                Some(id) => {
                                    if let Some(typed) = self.field(ui, id, "name", sc.name.clone(), label.id, "") {
                                        rename = Some((id, typed));
                                    }
                                }
                                // Every project's, as it is: named as the project's once it's changed.
                                None => {
                                    ui.label(RichText::new(&sc.name).color(pal().muted));
                                }
                            }
                            ui.end_row();
                            return;
                        }
                        ui.label(t("Category"));
                        ui.label(RichText::new(lang::category(&sc.of, true)).color(pal().muted));
                        ui.end_row();
                        for (tab, label) in SCHEDULE_TABS.iter().enumerate() {
                            ui.label(t(label));
                            let r = ui.button(t("Edit…"));
                            // Named for what it edits, for screen readers.
                            r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, tf("Edit {}", &[t(label)])));
                            if r.clicked() {
                                open = Some(tab);
                            }
                            ui.end_row();
                        }
                    },
                );
            });
        }
        // Named otherwise, one every project has is just the project's: its tab follows it.
        if let Some((id, name)) = rename
            && self.run("modify", json!({"id": id, "name": name.trim()})).is_some()
            && let Sheet::Schedule(_) = self.sheet
            && schedule::builtin(Some(name.trim()), None).is_none()
        {
            let was = self.sheet;
            self.views.iter_mut().filter(|v| **v == was).for_each(|v| *v = Sheet::Table(id));
            self.floating.iter_mut().filter(|f| f.sheet == was).for_each(|f| f.sheet = Sheet::Table(id));
            self.sheet = Sheet::Table(id);
        }
        if let Some(tab) = open {
            self.schedule_dialog = Some(ScheduleDialog { tab, ..ScheduleDialog::default() });
        }
    }

    /// The Type Properties dialog, while it's open: the type's parameters, Duplicate, and OK,
    /// Cancel and Apply. Changing a type changes every element of it.
    pub(crate) fn type_properties(&mut self, ctx: &Context) {
        let Some(mut d) = self.type_dialog.take() else { return };
        let model = Arc::clone(&self.session.model);
        let names: Vec<String> = model.type_names(d.category).into_iter().map(String::from).collect();
        let (mut ok, mut cancel, mut apply, mut create, mut switch) = (false, false, false, false, None);
        let r = Modal::new(UiId::new("type-properties")).show(ctx, |ui| {
            // Wider for a type's layers.
            ui.set_width(if LAYERED.contains(&d.category) { 560.0 } else { 400.0 });
            ui.label(RichText::new(t("Type Properties")).heading());
            ui.add_space(6.0);
            Grid::new("type-head").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                ui.label(t("Family:"));
                ui.label(lang::category(d.category, false));
                ui.end_row();
                let l = ui.label(t("Type:"));
                ui.horizontal(|ui| {
                    let c = ComboBox::from_id_salt("type-name").selected_text(&d.name).width(200.0).show_ui(ui, |ui| {
                        for n in &names {
                            if ui.selectable_label(*n == d.name, n).clicked() {
                                switch = Some(n.clone());
                            }
                        }
                    });
                    c.response.labelled_by(l.id);
                    if d.duplicate.is_none() && ui.button(t("Duplicate…")).clicked() {
                        d.duplicate = Some(format!("{} 2", d.name));
                    }
                });
                ui.end_row();
                if let Some(new) = &mut d.duplicate {
                    let l = ui.label(t("New type"));
                    ui.horizontal(|ui| {
                        ui.add(TextEdit::singleline(new).desired_width(200.0)).labelled_by(l.id);
                        create = ui.button(t("Create")).clicked();
                    });
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.label(RichText::new(t("Type Parameters")).strong());
            Grid::new("type-parameters").num_columns(2).striped(true).min_col_width(150.0).show(ui, |ui| {
                ui.label(RichText::new(t("Parameter")).color(pal().muted));
                ui.label(RichText::new(t("Value")).color(pal().muted));
                ui.end_row();
                let round = d.shape.as_ref().is_some_and(|(round, _)| *round);
                for (key, label, typed) in &mut d.fields {
                    // A round section has a diameter and no depth.
                    if round && *key == "depth" {
                        continue;
                    }
                    let l = ui.label(t(if round && *key == "width" { "Diameter" } else { label }));
                    ui.add(TextEdit::singleline(typed).desired_width(160.0)).labelled_by(l.id);
                    ui.end_row();
                }
                if let Some((round, material)) = &mut d.shape {
                    ui.label("");
                    ui.checkbox(round, t("Round"));
                    ui.end_row();
                    let l = ui.label(t("Material"));
                    let names: Vec<String> = model.materials.keys().cloned().collect();
                    if let Some(n) = combo(ui, (0, "type-material"), l.id, material, &names) {
                        *material = n;
                    }
                    ui.end_row();
                }
            });
            if LAYERED.contains(&d.category) {
                ui.add_space(6.0);
                let materials: Vec<String> = model.materials.keys().cloned().collect();
                let (out, inward) = if d.category == "wall" { ("Out", "In") } else { ("Up", "Down") };
                layer_editor(ui, &mut d.layers, model.units, &materials, (out, inward));
            }
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                apply = ui.button(t("Apply")).clicked();
                cancel = ui.button(t("Cancel")).clicked();
                ok = ui.button(t("OK")).clicked();
            });
        });
        if let Some(n) = switch {
            d.show(&model, n);
        }
        if create && let Some(new) = d.duplicate.clone() {
            let layered = !model.type_layers(d.category, &d.name).is_empty();
            let made = match d.params(new.trim(), model.units, layered) {
                Ok(mut p) => {
                    // A duplicate is round, and made of a material, as the type it copies is.
                    let types = serde_json::to_value(&model.types).unwrap_or_default();
                    let from = type_json(&types, d.category, &d.name);
                    if d.shape.is_none() {
                        for key in ["round", "material"] {
                            if !from[key].is_null() {
                                p[key] = from[key].clone();
                            }
                        }
                    }
                    self.run("type", p).is_some()
                }
                Err(e) => {
                    self.status = Status { text: e, error: true };
                    false
                }
            };
            if made {
                d.show(&self.session.model, new.trim().to_string());
            }
        }
        if apply || ok {
            let applied = match d.params(&d.name, model.units, false) {
                Ok(p) => self.run("type", p).is_some(),
                Err(e) => {
                    self.status = Status { text: e, error: true };
                    false
                }
            };
            ok &= applied;
            if applied {
                let name = d.name.clone();
                d.show(&Arc::clone(&self.session.model), name);
            }
        }
        if ok {
            let m = &self.session.model;
            if d.elements.is_empty() {
                self.types.insert(d.category, d.name.clone());
            } else if d.elements.iter().any(|id| element_json(m, *id)["type"].as_str() != Some(d.name.as_str())) {
                self.run("modify", json!({"ids": d.elements, "type": d.name}));
            }
        }
        if !(ok || cancel || r.should_close()) {
            self.type_dialog = Some(d);
        }
    }
}

impl BimApp {
    /// The Materials window: Manage > Materials, Revit's Material Browser. Each change is a
    /// `material` command, undone like any other.
    pub(crate) fn materials_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.materials_dialog.take() else { return };
        let model = Arc::clone(&self.session.model);
        let (mut close, mut run, mut picked): (bool, Option<Value>, Option<String>) = (false, None, None);
        // Save the material to the Favorites (true), or open them (false), once the window is done.
        let mut favorite: Option<bool> = None;
        let mut typed_wrong: Option<String> = None;
        // The material that asked for a picture for its texture.
        let mut picture: Option<String> = None;
        let released = ctx.input(|i| !i.pointer.any_down());
        let r = Modal::new(UiId::new("materials")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(RichText::new(t("Materials")).heading());
            ui.add_space(6.0);
            Grid::new("material-head").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                let l = ui.label(t("Material:"));
                ui.horizontal(|ui| {
                    let c = ComboBox::from_id_salt("material-name").selected_text(&d.picked).width(200.0).show_ui(ui, |ui| {
                        for name in model.materials.keys() {
                            if ui.selectable_label(*name == d.picked, name).clicked() {
                                picked = Some(name.clone());
                            }
                        }
                    });
                    c.response.labelled_by(l.id);
                    if ui.button(t("New")).clicked() {
                        let fresh =
                            (1..).map(|n| format!("{} {n}", t("Material"))).find(|n| !model.materials.contains_key(n)).unwrap_or_default();
                        run = Some(json!({"name": fresh}));
                        picked = Some(fresh);
                    }
                    if ui.add_enabled(model.materials.contains_key(&d.picked), Button::new(t("Delete"))).clicked() {
                        run = Some(json!({"name": d.picked, "delete": true}));
                        picked = model.materials.keys().find(|n| **n != d.picked).cloned();
                    }
                });
                ui.end_row();
            });
            if let Some(m) = model.materials.get(&d.picked) {
                ui.add_space(6.0);
                Grid::new("material").num_columns(2).striped(true).spacing(vec2(12.0, 6.0)).min_col_width(130.0).show(ui, |ui| {
                    let l = ui.label(t("Name"));
                    let r = ui.add(TextEdit::singleline(&mut d.name).desired_width(200.0)).labelled_by(l.id);
                    if r.lost_focus() && !d.name.trim().is_empty() && d.name.trim() != d.picked {
                        run = Some(json!({"name": d.picked, "rename": d.name.trim()}));
                        picked = Some(d.name.trim().to_string());
                    }
                    ui.end_row();
                    let l = ui.label(t("Color"));
                    let colour = d.color.get_or_insert(m.color);
                    ui.color_edit_button_srgb(colour).labelled_by(l.id);
                    if released && *colour != m.color {
                        run = Some(json!({"name": d.picked, "color": *colour}));
                    }
                    ui.end_row();
                    for (key, label, now) in [("cut", "Cut Pattern", m.cut), ("surface", "Surface Pattern", m.surface)] {
                        let l = ui.label(t(label));
                        let c = ComboBox::from_id_salt(("material-pattern", key)).selected_text(t(pattern_name(now))).width(160.0).show_ui(
                            ui,
                            |ui| {
                                for (p, name) in PATTERNS {
                                    if ui.selectable_label(p == now, t(name)).clicked() {
                                        run = Some(json!({"name": d.picked, key: p}));
                                    }
                                }
                            },
                        );
                        c.response.labelled_by(l.id);
                        ui.end_row();
                    }
                    // Its texture: a pattern drawn in code or a picture, over its colour in the 3D view; a tile of it this
                    // big, turned and moved.
                    let l = ui.label(t("Texture"));
                    let now = m.texture.as_ref().map(|x| x.kind);
                    let shown = now.map_or(t("None"), |k| t(texture_name(k)));
                    let c = ComboBox::from_id_salt("material-texture").selected_text(shown).width(160.0).show_ui(ui, |ui| {
                        if ui.selectable_label(now.is_none(), t("None")).clicked() {
                            run = Some(json!({"name": d.picked, "texture": null}));
                        }
                        for k in TextureKind::ALL.into_iter().filter(|k| *k != TextureKind::Image) {
                            if ui.selectable_label(now == Some(k), t(texture_name(k))).clicked() {
                                run = Some(json!({"name": d.picked, "texture": {"kind": k}}));
                            }
                        }
                        if ui.selectable_label(now == Some(TextureKind::Image), t("Picture…")).clicked() {
                            picture = Some(d.picked.clone());
                        }
                    });
                    c.response.labelled_by(l.id);
                    ui.end_row();
                    if let Some(x) = &m.texture {
                        let units = model.units;
                        if !d.texture.as_ref().is_some_and(|(n, old, _)| *n == d.picked && old == x) {
                            let typed = [
                                units.length(x.size[0]),
                                units.length(x.size[1]),
                                format!("{}", x.angle),
                                units.length(x.offset[0]),
                                units.length(x.offset[1]),
                            ];
                            d.texture = Some((d.picked.clone(), x.clone(), typed));
                        }
                        if let Some((_, _, typed)) = &mut d.texture {
                            let rows = [
                                ("Tile Width", 0),
                                ("Tile Height", 1),
                                ("Texture Angle", 2),
                                ("Texture Offset Across", 3),
                                ("Texture Offset Up", 4),
                            ];
                            for (label, k) in rows {
                                let l = ui.label(t(label));
                                let hint = if k == 2 { "°" } else { units.name() };
                                let r = ui.add(TextEdit::singleline(&mut typed[k]).desired_width(120.0).hint_text(hint)).labelled_by(l.id);
                                if r.lost_focus() {
                                    let length = |i: usize| units.parse(&typed[i]);
                                    match (length(0), length(1), number(&typed[2]), length(3), length(4)) {
                                        (Some(w), Some(h), Some(a), Some(ox), Some(oy)) => {
                                            if [w, h, a, ox, oy] != [x.size[0], x.size[1], x.angle, x.offset[0], x.offset[1]] {
                                                run = Some(
                                                    json!({"name": d.picked, "texture": {"size": [w, h], "angle": a, "offset": [ox, oy]}}),
                                                );
                                            }
                                        }
                                        _ => typed_wrong = Some(tf("`{}` is not a size, an angle or an offset", &[&typed[k]])),
                                    }
                                }
                                ui.end_row();
                            }
                        }
                    }
                });
                // A tile of its texture, or its colour; and the Favorites it can be kept in.
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let key = format!("{:?}", (m.color, &m.texture));
                    if d.swatch.as_ref().map(|(k, _)| k) != Some(&key) {
                        d.swatch = crate::favorites::swatch(ui.ctx(), "material-swatch", m, &model.images, 64).map(|h| (key, h));
                    }
                    crate::favorites::paint_swatch(ui, 64.0, m.color, d.swatch.as_ref().map(|(_, h)| h));
                    ui.vertical(|ui| {
                        if ui.button(t("Save to Favorites")).clicked() {
                            favorite = Some(true);
                        }
                        if ui.button(t("Favorites…")).clicked() {
                            favorite = Some(false);
                        }
                    });
                });
            }
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = ui.button(t("Close")).clicked());
        });
        if let Some(text) = typed_wrong {
            self.status = Status { text, error: true };
        }
        match favorite {
            Some(true) => {
                let p = self.favorite_params("save", "material", &d.picked);
                if self.run("favorite", p).is_some() {
                    self.status = Status { text: tf("Saved {} to your Favorites", &[&d.picked]), error: false };
                }
            }
            Some(false) => self.open_favorites(),
            None => {}
        }
        if let Some(p) = run {
            self.run("material", p);
        }
        if let Some(material) = picture {
            let dialog = rfd::AsyncFileDialog::new().add_filter(t("Pictures"), &["png", "jpg", "jpeg"]);
            self.ask_file(ctx, dialog, false, move |app, path| {
                app.run("material", json!({"name": material, "texture": {"kind": "image", "path": path.to_string_lossy()}}));
            });
        }
        if let Some(n) = picked {
            (d.name, d.picked, d.color) = (n.clone(), n, None);
        } else if !self.session.model.materials.contains_key(&d.picked) {
            d = MaterialsDialog::new(&self.session.model);
        } else if d.color.is_some_and(|c| self.session.model.materials.get(&d.picked).is_some_and(|m| m.color == c)) {
            d.color = None;
        }
        if !(close || r.should_close()) {
            self.materials_dialog = Some(d);
        }
    }
}

/// A type's layers, exterior or top first: what each does, what it's made of (from the library),
/// how thick; a new one on the outside, or on top, and each moved, or taken away.
fn layer_editor(ui: &mut Ui, layers: &mut Vec<LayerRow>, units: Units, materials: &[String], (out, inward): (&str, &str)) {
    ui.label(RichText::new(t("Layers")).strong());
    let mut action: Option<(usize, i8)> = None;
    Grid::new("type-layers").num_columns(4).striped(true).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
        let function = ui.label(RichText::new(t("Function")).color(pal().muted));
        let material = ui.label(RichText::new(t("Material")).color(pal().muted));
        let thickness = ui.label(RichText::new(t("Thickness")).color(pal().muted));
        ui.label("");
        ui.end_row();
        let last = layers.len().saturating_sub(1);
        for (k, (f, mat, typed)) in layers.iter_mut().enumerate() {
            let c = ComboBox::from_id_salt(("layer-function", k)).selected_text(t(function_name(*f))).width(110.0).show_ui(ui, |ui| {
                for (g, n) in FUNCTIONS {
                    if ui.selectable_label(*f == g, t(n)).clicked() {
                        *f = g;
                    }
                }
            });
            c.response.labelled_by(function.id);
            let none = t("(none)");
            let shown = if mat.is_empty() { none } else { mat.as_str() };
            let c = ComboBox::from_id_salt(("layer-material", k)).selected_text(shown).width(150.0).show_ui(ui, |ui| {
                if ui.selectable_label(mat.is_empty(), none).clicked() {
                    mat.clear();
                }
                for n in materials {
                    if ui.selectable_label(mat == n, n).clicked() {
                        mat.clone_from(n);
                    }
                }
            });
            c.response.labelled_by(material.id);
            ui.add_sized([70.0, 20.0], TextEdit::singleline(typed).hint_text(units.name())).labelled_by(thickness.id);
            ui.horizontal(|ui| {
                if ui.add_enabled(k > 0, Button::new(t(out)).small()).clicked() {
                    action = Some((k, -1));
                }
                if ui.add_enabled(k < last, Button::new(t(inward)).small()).clicked() {
                    action = Some((k, 1));
                }
                if ui.add_enabled(last > 0, Button::new(t("Delete")).small()).clicked() {
                    action = Some((k, 0));
                }
            });
            ui.end_row();
        }
    });
    match action {
        Some((k, 0)) if k < layers.len() => {
            layers.remove(k);
        }
        Some((k, -1)) if k > 0 => layers.swap(k, k - 1),
        Some((k, _)) if k + 1 < layers.len() => layers.swap(k, k + 1),
        _ => {}
    }
    // A finish 10 mm thick, on the exterior side.
    if ui.button(t("Insert Layer")).clicked() {
        layers.insert(0, (Function::Finish, String::new(), units.length(10.0)));
    }
}

/// Type Properties for a family's type: its type parameters as typed (a length in the project's
/// units, a material's name, yes or no), what its formulas work out shown; Duplicate, OK, Cancel
/// and Apply, each a `family` command.
pub(crate) struct FamilyDialog {
    family: String,
    name: String,
    /// The components that were on the type when the dialog opened; OK puts them on the type shown.
    elements: Vec<Id>,
    /// Each type parameter: its name, what it holds, as typed, and whether a formula works it out.
    fields: Vec<(String, Holds, String, bool)>,
    /// The name being typed for a duplicate.
    duplicate: Option<String>,
}

impl FamilyDialog {
    pub(crate) fn new(m: &Model, family: String, name: String, elements: Vec<Id>) -> FamilyDialog {
        let fields = m
            .families
            .get(&family)
            .map(|f| {
                let values = f.values(&name, &Default::default()).unwrap_or_default();
                let typed = |p: &bimcraft_core::family::Parameter| match values.get(&p.name) {
                    Some(Val::Number(x)) if p.kind == Holds::Length => m.units.length(*x),
                    Some(Val::Number(x)) if p.kind == Holds::YesNo => (if *x != 0.0 { "1" } else { "0" }).to_string(),
                    Some(Val::Number(x)) => format!("{}", (x * 1e6).round() / 1e6),
                    Some(Val::Text(t)) => t.clone(),
                    None => String::new(),
                };
                f.parameters.iter().filter(|p| !p.instance).map(|p| (p.name.clone(), p.kind, typed(p), p.formula.is_some())).collect()
            })
            .unwrap_or_default();
        FamilyDialog { family, name, elements, fields, duplicate: None }
    }

    /// The dialog showing type `name` as the model has it now.
    fn show(&mut self, m: &Model, name: String) {
        *self = FamilyDialog::new(m, self.family.clone(), name, std::mem::take(&mut self.elements));
    }

    /// The `family` command's parameters setting type `name` to the values as typed, or why they
    /// aren't what they hold.
    fn params(&self, name: &str, units: Units) -> Result<Value, String> {
        let mut set = serde_json::Map::new();
        for (key, kind, typed, _) in self.fields.iter().filter(|f| !f.3) {
            let v = match kind {
                Holds::Length => json!(units.parse(typed).ok_or_else(|| tf("{}: `{}` is not a length", &[key, typed]))?),
                Holds::Number | Holds::Angle => json!(number(typed).ok_or_else(|| tf("{}: `{}` is not a number", &[key, typed]))?),
                Holds::YesNo => json!(typed == "1"),
                Holds::Material => json!(typed),
            };
            set.insert(key.clone(), v);
        }
        Ok(json!({"name": self.family, "types": {name: set}}))
    }
}

impl BimApp {
    /// Type Properties for a family's type, while it's open.
    pub(crate) fn family_type_properties(&mut self, ctx: &Context) {
        let Some(mut d) = self.family_dialog.take() else { return };
        let model = Arc::clone(&self.session.model);
        let names: Vec<String> = model.families.get(&d.family).map(|f| f.types.keys().cloned().collect()).unwrap_or_default();
        let materials: Vec<String> = model.materials.keys().cloned().collect();
        let (mut ok, mut cancel, mut apply, mut create, mut switch) = (false, false, false, false, None);
        let r = Modal::new(UiId::new("family-type-properties")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(RichText::new(t("Type Properties")).heading());
            ui.add_space(6.0);
            Grid::new("family-type-head").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                ui.label(t("Family:"));
                ui.label(&d.family);
                ui.end_row();
                let l = ui.label(t("Type:"));
                ui.horizontal(|ui| {
                    let c = ComboBox::from_id_salt("family-type-name").selected_text(&d.name).width(200.0).show_ui(ui, |ui| {
                        for n in &names {
                            if ui.selectable_label(*n == d.name, n).clicked() {
                                switch = Some(n.clone());
                            }
                        }
                    });
                    c.response.labelled_by(l.id);
                    if d.duplicate.is_none() && ui.button(t("Duplicate…")).clicked() {
                        d.duplicate = Some(format!("{} 2", d.name));
                    }
                });
                ui.end_row();
                if let Some(new) = &mut d.duplicate {
                    let l = ui.label(t("New type"));
                    ui.horizontal(|ui| {
                        ui.add(TextEdit::singleline(new).desired_width(200.0)).labelled_by(l.id);
                        create = ui.button(t("Create")).clicked();
                    });
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.label(RichText::new(t("Type Parameters")).strong());
            Grid::new("family-type-parameters").num_columns(2).striped(true).min_col_width(150.0).show(ui, |ui| {
                ui.label(RichText::new(t("Parameter")).color(pal().muted));
                ui.label(RichText::new(t("Value")).color(pal().muted));
                ui.end_row();
                for (key, kind, typed, computed) in &mut d.fields {
                    let l = ui.label(key.as_str());
                    match kind {
                        _ if *computed => {
                            ui.label(RichText::new(typed.as_str()).color(pal().muted));
                        }
                        Holds::YesNo => {
                            let mut on = typed == "1";
                            let r = ui.checkbox(&mut on, "");
                            r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, key.as_str()));
                            if r.labelled_by(l.id).changed() {
                                *typed = (if on { "1" } else { "0" }).to_string();
                            }
                        }
                        Holds::Material => {
                            if let Some(m) = combo(ui, (0, key.as_str()), l.id, typed, &materials) {
                                *typed = m;
                            }
                        }
                        _ => {
                            ui.add(TextEdit::singleline(typed).desired_width(160.0)).labelled_by(l.id);
                        }
                    }
                    ui.end_row();
                }
            });
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                apply = ui.button(t("Apply")).clicked();
                cancel = ui.button(t("Cancel")).clicked();
                ok = ui.button(t("OK")).clicked();
            });
        });
        if let Some(n) = switch {
            d.show(&model, n);
        }
        if create && let Some(new) = d.duplicate.clone() {
            let made = match d.params(new.trim(), model.units) {
                Ok(p) => self.run("family", p).is_some(),
                Err(e) => {
                    self.status = Status { text: e, error: true };
                    false
                }
            };
            if made {
                d.show(&self.session.model, new.trim().to_string());
            }
        }
        if apply || ok {
            let applied = match d.params(&d.name, model.units) {
                Ok(p) => self.run("family", p).is_some(),
                Err(e) => {
                    self.status = Status { text: e, error: true };
                    false
                }
            };
            ok &= applied;
            if applied {
                let name = d.name.clone();
                d.show(&Arc::clone(&self.session.model), name);
            }
        }
        if ok {
            let m = &self.session.model;
            if d.elements.is_empty() {
                self.component = Some((d.family.clone(), d.name.clone()));
            } else if d.elements.iter().any(|id| element_json(m, *id)["type"].as_str() != Some(d.name.as_str())) {
                self.run("modify", json!({"ids": d.elements, "type": d.name}));
            }
        }
        if !(ok || cancel || r.should_close()) {
            self.family_dialog = Some(d);
        }
    }
}

/// The angles a direction snaps to multiples of, degrees: none, or every one of these.
const ANGLE_STEPS: [f64; 6] = [0.0, 5.0, 15.0, 30.0, 45.0, 90.0];

impl BimApp {
    /// Manage > Snaps: what a picked point sticks to, and the angles a direction steps by. A change
    /// is in force at once.
    pub(crate) fn snaps_window(&mut self, ctx: &Context) {
        if !self.show_snaps {
            return;
        }
        let mut close = false;
        let r = Modal::new(UiId::new("snaps")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(RichText::new(t("Snaps")).heading());
            ui.add_space(6.0);
            ui.label(RichText::new(t("Object Snaps")).strong());
            let s = &mut self.snaps;
            let kinds: [(&mut bool, Snap, &str); 6] = [
                (&mut s.endpoint, Snap::Endpoint, "The ends of walls, grids and what else is drawn, and the points picked so far"),
                (&mut s.midpoint, Snap::Midpoint, "The middle of a wall, along its arc"),
                (&mut s.intersection, Snap::Intersection, "Where two walls or grids cross"),
                (&mut s.centre, Snap::Centre, "The centre of a curved wall, a column or a component"),
                (&mut s.perpendicular, Snap::Perpendicular, "The foot of the perpendicular from the last point to a wall or grid"),
                (&mut s.nearest, Snap::Nearest, "A wall's centre line, so a wall drawn on it meets it"),
            ];
            for (on, kind, tip) in kinds {
                ui.checkbox(on, t(kind.name())).on_hover_text(t(tip));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let label = ui.label(t("Angle steps"));
                let now = if s.angle >= 1.0 { format!("{}°", s.angle) } else { t("Off").to_string() };
                let c = ComboBox::from_id_salt("snap-angle").selected_text(now).width(110.0).show_ui(ui, |ui| {
                    for step in ANGLE_STEPS {
                        let name = if step >= 1.0 { format!("{step}°") } else { t("Off").to_string() };
                        ui.selectable_value(&mut s.angle, step, name);
                    }
                });
                c.response.labelled_by(label.id).on_hover_text(t(
                    "A direction from the last point sticks to multiples of this angle, from east and from the line before; a dashed guide shows it",
                ));
            });
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = ui.button(t("Close")).clicked());
        });
        if close || r.should_close() {
            self.show_snaps = false;
        }
    }
}

impl BimApp {
    /// The rows of a filter editor, as Schedule Properties' Filter tab and the Filters dialog have
    /// them: each a field of `names`, how to compare it, and a value, with Remove, and Add Filter
    /// under them. `kind_of` says what a field holds, and `key` names the element whose field is
    /// being typed in. Whether `filters` changed.
    pub(crate) fn filter_rows(
        &mut self,
        ui: &mut Ui,
        key: Id,
        filters: &mut Vec<Filter>,
        names: &[String],
        kind_of: &dyn Fn(&str) -> schedule::Kind,
        units: Units,
    ) -> bool {
        let mut edited = false;
        Grid::new("schedule-filters").num_columns(5).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
            let mut gone = None;
            for (i, f) in filters.iter_mut().enumerate() {
                let kind = kind_of(&f.field);
                // Each row's caption names its field, its comparison and its value.
                let caption = ui.label(t(if i == 0 { "Filter by:" } else { "And:" }));
                let c = ComboBox::from_id_salt(("filter-field", i)).selected_text(t(&f.field)).width(170.0).show_ui(ui, |ui| {
                    for n in names {
                        if ui.selectable_label(*n == f.field, t(n)).clicked() && *n != f.field {
                            (f.field, f.op, f.value) = (n.clone(), Op::HasValue, Value::Null);
                            edited = true;
                        }
                    }
                });
                c.response.labelled_by(caption.id);
                let now = OPS.iter().find(|(op, _)| *op == f.op).map_or("equals", |(_, n)| n);
                let c = ComboBox::from_id_salt(("filter-op", i)).selected_text(t(now)).width(190.0).show_ui(ui, |ui| {
                    for (op, label) in ops(kind) {
                        if ui.selectable_label(op == f.op, t(label)).clicked() && op != f.op {
                            f.op = op;
                            let fits = match kind {
                                schedule::Kind::Text => f.value.is_string(),
                                schedule::Kind::YesNo => f.value.is_boolean(),
                                _ => f.value.is_number(),
                            };
                            if !matches!(op, Op::HasValue | Op::HasNoValue) && !fits {
                                f.value = blank(kind);
                            }
                            edited = true;
                        }
                    }
                });
                c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, t("Comparison")));
                match (f.op, kind) {
                    (Op::HasValue | Op::HasNoValue, _) => {
                        ui.label("");
                    }
                    (_, schedule::Kind::YesNo) => {
                        let now = if f.value.as_bool().unwrap_or(true) { "Yes" } else { "No" };
                        let c = ComboBox::from_id_salt(("filter-yes", i)).selected_text(t(now)).width(120.0).show_ui(ui, |ui| {
                            for (b, label) in [(true, "Yes"), (false, "No")] {
                                if ui.selectable_label(f.value.as_bool() == Some(b), t(label)).clicked() {
                                    f.value = json!(b);
                                    edited = true;
                                }
                            }
                        });
                        c.response.labelled_by(caption.id);
                    }
                    (_, kind) => {
                        let now = match (kind, f.value.as_f64()) {
                            (schedule::Kind::Length, Some(x)) => units.length(x),
                            (schedule::Kind::Area, Some(x)) => format!("{}", (units.area(x * 1e6).0 * 100.0).round() / 100.0),
                            _ => text(&f.value),
                        };
                        // A field of its own width: a grid cell would only be as wide as the last frame's.
                        let height = ui.spacing().interact_size.y;
                        let typed = ui
                            .allocate_ui(vec2(170.0, height), |ui| self.field(ui, key, &format!("filter.{i}"), now, caption.id, ""))
                            .inner;
                        if let Some(typed) = typed {
                            match filter_value(kind, &typed, units) {
                                Ok(v) => {
                                    f.value = v;
                                    edited = true;
                                }
                                Err(e) => self.status = Status { text: e, error: true },
                            }
                        }
                    }
                }
                if ui.button(t("Remove")).clicked() {
                    gone = Some(i);
                }
                ui.end_row();
            }
            if let Some(i) = gone {
                filters.remove(i);
                edited = true;
            }
        });
        if ui.button(t("Add Filter")).clicked()
            && let Some(first) = names.first()
        {
            filters.push(Filter { field: first.clone(), op: Op::HasValue, value: Value::Null });
            edited = true;
        }
        edited
    }
}

/// What a project parameter holds, as Project Parameters names it.
const KINDS: [(ParameterKind, &str); 4] =
    [(ParameterKind::Text, "Text"), (ParameterKind::Number, "Number"), (ParameterKind::Length, "Length"), (ParameterKind::YesNo, "Yes/No")];

/// Manage > Project Parameters: the project's parameters, the one picked to edit, and its name as
/// typed (a rename once typed in).
pub(crate) struct ParametersDialog {
    picked: String,
    name: String,
}

impl ParametersDialog {
    pub(crate) fn new(m: &Model) -> ParametersDialog {
        let picked = m.parameters.keys().next().cloned().unwrap_or_default();
        ParametersDialog { name: picked.clone(), picked }
    }
}

/// View > Schedules > Schedule/Quantities: the category picked, and the name typed.
#[derive(Default)]
pub(crate) struct NewSchedule {
    category: Option<&'static str>,
    name: String,
}

/// Schedule Properties' tabs.
const SCHEDULE_TABS: [&str; 3] = ["Fields", "Filter", "Sorting/Grouping"];

/// Schedule Properties, for the schedule on screen: the tab showing, the field picked in each list
/// of the Fields tab, and a calculated field being made (its name, kind and formula).
#[derive(Default)]
pub(crate) struct ScheduleDialog {
    tab: usize,
    available: Option<String>,
    scheduled: Option<usize>,
    calculated: Option<(String, schedule::Kind, String)>,
}

/// What a calculated field comes to, as Schedule Properties names it.
const RESULTS: [(schedule::Kind, &str); 4] = [
    (schedule::Kind::Number, "Number"),
    (schedule::Kind::Length, "Length"),
    (schedule::Kind::Area, "Area"),
    (schedule::Kind::YesNo, "Yes/No"),
];

/// A filter's comparisons, as Schedule Properties says them.
const OPS: [(Op, &str); 14] = [
    (Op::Equals, "equals"),
    (Op::NotEquals, "does not equal"),
    (Op::Greater, "is greater than"),
    (Op::GreaterOrEqual, "is greater than or equal to"),
    (Op::Less, "is less than"),
    (Op::LessOrEqual, "is less than or equal to"),
    (Op::Contains, "contains"),
    (Op::NotContains, "does not contain"),
    (Op::BeginsWith, "begins with"),
    (Op::NotBeginsWith, "does not begin with"),
    (Op::EndsWith, "ends with"),
    (Op::NotEndsWith, "does not end with"),
    (Op::HasValue, "has a value"),
    (Op::HasNoValue, "has no value"),
];

/// The comparisons a field holding `kind` takes: text all of them, numbers by size, yes or no
/// whether it is.
fn ops(kind: schedule::Kind) -> Vec<(Op, &'static str)> {
    let fits = |op: Op| match kind {
        schedule::Kind::Text => true,
        schedule::Kind::YesNo => matches!(op, Op::Equals | Op::NotEquals | Op::HasValue | Op::HasNoValue),
        _ => !matches!(op, Op::Contains | Op::NotContains | Op::BeginsWith | Op::NotBeginsWith | Op::EndsWith | Op::NotEndsWith),
    };
    OPS.into_iter().filter(|(op, _)| fits(*op)).collect()
}

/// A filter's value as typed, for a field holding `kind`: lengths in `units`, areas in m² or ft².
fn filter_value(kind: schedule::Kind, typed: &str, units: Units) -> Result<Value, String> {
    match kind {
        schedule::Kind::Text => Ok(json!(typed.trim())),
        schedule::Kind::Length => units.parse(typed).map(|x| json!(x)).ok_or_else(|| tf("`{}` is not a length", &[typed.trim()])),
        // Typed as the schedule shows it; kept in m².
        schedule::Kind::Area => {
            number(typed).map(|x| json!(x / units.area(1e6).0)).ok_or_else(|| tf("`{}` is not a number", &[typed.trim()]))
        }
        _ => number(typed).map(|x| json!(x)).ok_or_else(|| tf("`{}` is not a number", &[typed.trim()])),
    }
}

/// A value of a field holding `kind` that a filter can compare with, when it has none yet.
fn blank(kind: schedule::Kind) -> Value {
    match kind {
        schedule::Kind::Text => json!(""),
        schedule::Kind::YesNo => json!(true),
        _ => json!(0.0),
    }
}

impl BimApp {
    /// Manage > Project Parameters: each project parameter, what it holds and the categories it's
    /// for, as Revit's dialog has them. Each change is a `parameter` command, undone like any other.
    pub(crate) fn parameters_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.parameters_dialog.take() else { return };
        let model = Arc::clone(&self.session.model);
        let (mut close, mut run, mut picked): (bool, Option<Value>, Option<String>) = (false, None, None);
        let r = Modal::new(UiId::new("project-parameters")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.label(RichText::new(t("Project Parameters")).heading());
            ui.add_space(6.0);
            Grid::new("parameter-head").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                let l = ui.label(t("Parameter:"));
                ui.horizontal(|ui| {
                    let c = ComboBox::from_id_salt("parameter-name").selected_text(&d.picked).width(200.0).show_ui(ui, |ui| {
                        for name in model.parameters.keys() {
                            if ui.selectable_label(*name == d.picked, name).clicked() {
                                picked = Some(name.clone());
                            }
                        }
                    });
                    c.response.labelled_by(l.id);
                    if ui.button(t("New")).clicked() {
                        let fresh = (1..)
                            .map(|n| format!("{} {n}", t("Parameter")))
                            .find(|n| !model.parameters.contains_key(n))
                            .unwrap_or_default();
                        run = Some(json!({"name": fresh, "categories": ["wall"]}));
                        picked = Some(fresh);
                    }
                    if ui.add_enabled(model.parameters.contains_key(&d.picked), Button::new(t("Delete"))).clicked() {
                        run = Some(json!({"name": d.picked, "delete": true}));
                        picked = model.parameters.keys().find(|n| **n != d.picked).cloned().or_else(|| Some(String::new()));
                    }
                });
                ui.end_row();
            });
            match model.parameters.get(&d.picked) {
                Some(p) => {
                    ui.add_space(6.0);
                    Grid::new("parameter").num_columns(2).striped(true).spacing(vec2(12.0, 6.0)).min_col_width(130.0).show(ui, |ui| {
                        let l = ui.label(t("Name"));
                        let r = ui.add(TextEdit::singleline(&mut d.name).desired_width(200.0)).labelled_by(l.id);
                        if r.lost_focus() && !d.name.trim().is_empty() && d.name.trim() != d.picked {
                            run = Some(json!({"name": d.picked, "rename": d.name.trim()}));
                            picked = Some(d.name.trim().to_string());
                        }
                        ui.end_row();
                        let l = ui.label(t("Type of Parameter"));
                        let now = KINDS.iter().find(|(k, _)| *k == p.kind).map_or("Text", |(_, n)| n);
                        let c = ComboBox::from_id_salt("parameter-kind").selected_text(t(now)).width(160.0).show_ui(ui, |ui| {
                            for (k, label) in KINDS {
                                if ui.selectable_label(k == p.kind, t(label)).clicked() && k != p.kind {
                                    run = Some(json!({"name": d.picked, "kind": k}));
                                }
                            }
                        });
                        c.response.labelled_by(l.id);
                        ui.end_row();
                    });
                    ui.add_space(6.0);
                    ui.label(RichText::new(t("Categories")).strong());
                    Grid::new("parameter-categories").num_columns(3).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
                        for (i, cat) in PARAMETER_CATEGORIES.iter().enumerate() {
                            let mut on = p.categories.iter().any(|c| c == cat);
                            if ui.checkbox(&mut on, lang::category(cat, true)).changed() {
                                let mut list: Vec<&str> = p.categories.iter().map(String::as_str).filter(|c| c != cat).collect();
                                if on {
                                    list.push(cat);
                                }
                                run = Some(json!({"name": d.picked, "categories": list}));
                            }
                            if i % 3 == 2 {
                                ui.end_row();
                            }
                        }
                    });
                }
                None => {
                    ui.label(RichText::new(t("No project parameters yet: New makes one.")).color(pal().muted));
                }
            }
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = ui.button(t("Close")).clicked());
        });
        if let Some(p) = run
            && self.run("parameter", p).is_none()
        {
            picked = None;
        }
        if let Some(n) = picked {
            (d.name, d.picked) = (n.clone(), n);
        } else if !self.session.model.parameters.contains_key(&d.picked) {
            d = ParametersDialog::new(&self.session.model);
        }
        if !(close || r.should_close()) {
            self.parameters_dialog = Some(d);
        }
    }

    /// View > Schedules > Schedule/Quantities: a category and a name, and OK makes the schedule,
    /// opens it, and opens its properties on Fields.
    pub(crate) fn new_schedule_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.new_schedule.take() else { return };
        let (mut ok, mut cancel) = (false, false);
        let model = Arc::clone(&self.session.model);
        let r = Modal::new(UiId::new("new-schedule")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(t("New Schedule")).heading());
            ui.add_space(6.0);
            ui.label(t("Category:"));
            ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                for cat in schedule::categories() {
                    if ui.selectable_label(d.category == Some(cat), lang::category(cat, true)).clicked() {
                        d.category = Some(cat);
                        // Named after it, as one of its kind there isn't yet.
                        let base = tf("{} Schedule", &[&lang::category(cat, false)]);
                        d.name = std::iter::once(base.clone())
                            .chain((2..).map(|n| format!("{base} {n}")))
                            .find(|n| {
                                schedule::builtin(Some(n), None).is_none()
                                    && !model.elements.keys().any(|id| model.view_name(*id) == Some(n.as_str()))
                            })
                            .unwrap_or(base);
                    }
                }
            });
            ui.add_space(6.0);
            let l = ui.label(t("Name:"));
            ui.add(TextEdit::singleline(&mut d.name).desired_width(300.0)).labelled_by(l.id);
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                cancel = ui.button(t("Cancel")).clicked();
                ok = ui.add_enabled(d.category.is_some() && !d.name.trim().is_empty(), Button::new(t("OK"))).clicked();
            });
        });
        if ok && let Some(cat) = d.category {
            let name = d.name.trim().to_string();
            if let Some(made) = self.run("schedule", json!({"name": name, "category": cat})) {
                let sheet = match (schedule::builtin(Some(&name), None), made["id"].as_u64()) {
                    (Some((cat, _)), _) => crate::SCHEDULES.iter().find(|(c, _)| *c == cat).map(|(c, _)| Sheet::Schedule(c)),
                    (None, Some(id)) => Some(Sheet::Table(id)),
                    (None, None) => None,
                };
                if let Some(sheet) = sheet {
                    self.open_sheet(sheet);
                    self.schedule_dialog = Some(ScheduleDialog::default());
                }
                return;
            }
        }
        // Refused, it stays open, the status saying why, to be named again.
        if !(cancel || r.should_close()) {
            self.new_schedule = Some(d);
        }
    }

    /// Schedule Properties, for the schedule on screen: its Fields (its category's, and calculated
    /// ones, in order, their headings and totals), its Filter and its Sorting/Grouping, as Revit's
    /// tabs have them. Each change is a `schedule` command, undone like any other.
    pub(crate) fn schedule_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.schedule_dialog.take() else { return };
        let Some((made, sc)) = self.shown_schedule() else { return };
        let model = Arc::clone(&self.session.model);
        let units = model.units;
        let cat = sc.category();
        let available = schedule::fields(&model, &cat);
        let mut fields: Vec<Field> = if sc.fields.is_empty() { schedule::defaults(&cat) } else { sc.fields.clone() };
        let kind_of = |fields: &[Field], name: &str| -> schedule::Kind {
            match fields.iter().find(|f| f.name == name && f.formula.is_some()) {
                Some(f) => f.kind.unwrap_or_default(),
                None => available.iter().find(|(n, _)| n == name).map_or(schedule::Kind::Text, |(_, k)| *k),
            }
        };
        // Every field there is to filter and sort by: its category's, then the calculated ones.
        let names: Vec<String> =
            available.iter().map(|(n, _)| n.clone()).chain(fields.iter().filter(|f| f.formula.is_some()).map(|f| f.name.clone())).collect();
        let key = made.unwrap_or(0);
        let mut change: Option<Value> = None;
        let mut close = false;
        let r = Modal::new(UiId::new("schedule-properties")).show(ctx, |ui| {
            ui.set_width(620.0);
            ui.label(RichText::new(tf("Schedule Properties: {}", &[&sc.name])).heading());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (i, label) in SCHEDULE_TABS.iter().enumerate() {
                    if ui.selectable_label(d.tab == i, t(label)).clicked() {
                        d.tab = i;
                    }
                }
            });
            ui.separator();
            match d.tab {
                0 => {
                    let shown: Vec<String> = fields.iter().map(|f| f.name.clone()).collect();
                    // Revit's three columns: the fields there are, what moves them, the fields shown.
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(220.0);
                            ui.label(RichText::new(t("Available fields:")).strong());
                            ui.push_id("available", |ui| {
                                ScrollArea::vertical().max_height(220.0).min_scrolled_height(220.0).show(ui, |ui| {
                                    ui.set_width(210.0);
                                    for (name, _) in available.iter().filter(|(n, _)| !shown.contains(n)) {
                                        if ui.selectable_label(d.available.as_ref() == Some(name), t(name)).clicked() {
                                            d.available = Some(name.clone());
                                        }
                                    }
                                });
                            });
                        });
                        ui.vertical(|ui| {
                            ui.set_width(140.0);
                            ui.add_space(28.0);
                            let can_add = d.available.as_ref().is_some_and(|n| !shown.contains(n));
                            if ui.add_enabled(can_add, Button::new(t("Add")).min_size(vec2(130.0, 0.0))).clicked()
                                && let Some(name) = d.available.take()
                            {
                                fields.push(Field { name, ..Field::default() });
                                d.scheduled = Some(fields.len() - 1);
                                change = Some(json!({"fields": fields}));
                            }
                            let picked = d.scheduled.filter(|i| *i < fields.len());
                            if ui
                                .add_enabled(picked.is_some() && fields.len() > 1, Button::new(t("Remove")).min_size(vec2(130.0, 0.0)))
                                .clicked()
                                && let Some(i) = picked
                            {
                                fields.remove(i);
                                d.scheduled = None;
                                change = Some(json!({"fields": fields}));
                            }
                            ui.add_space(12.0);
                            if ui.add(Button::new(t("Calculated Value…")).min_size(vec2(130.0, 0.0))).clicked() {
                                d.calculated = Some((String::new(), schedule::Kind::Number, String::new()));
                            }
                        });
                        ui.vertical(|ui| {
                            ui.set_width(220.0);
                            ui.label(RichText::new(t("Scheduled fields (in order):")).strong());
                            ui.push_id("scheduled", |ui| {
                                ScrollArea::vertical().max_height(190.0).min_scrolled_height(190.0).show(ui, |ui| {
                                    ui.set_width(210.0);
                                    for (i, f) in fields.iter().enumerate() {
                                        if ui.selectable_label(d.scheduled == Some(i), t(&f.name)).clicked() {
                                            d.scheduled = Some(i);
                                        }
                                    }
                                });
                            });
                            ui.horizontal(|ui| {
                                let picked = d.scheduled.filter(|i| *i < fields.len());
                                if ui.add_enabled(picked.is_some_and(|i| i > 0), Button::new(t("Move Up"))).clicked()
                                    && let Some(i) = picked
                                {
                                    fields.swap(i, i - 1);
                                    d.scheduled = Some(i - 1);
                                    change = Some(json!({"fields": fields}));
                                }
                                if ui.add_enabled(picked.is_some_and(|i| i + 1 < fields.len()), Button::new(t("Move Down"))).clicked()
                                    && let Some(i) = picked
                                {
                                    fields.swap(i, i + 1);
                                    d.scheduled = Some(i + 1);
                                    change = Some(json!({"fields": fields}));
                                }
                            });
                        });
                    });
                    // The field picked: its heading, its totals; a calculated one's formula and kind.
                    if let Some(i) = d.scheduled.filter(|i| *i < fields.len()) {
                        ui.add_space(6.0);
                        let kind = kind_of(&fields, &fields[i].name);
                        Grid::new("schedule-field").num_columns(2).spacing(vec2(12.0, 6.0)).min_col_width(130.0).show(ui, |ui| {
                            let l = ui.label(t("Heading"));
                            let now = fields[i].heading.clone().unwrap_or_else(|| t(&fields[i].name).to_string());
                            if let Some(typed) = self.field(ui, key, &format!("heading.{i}"), now, l.id, "") {
                                let typed = typed.trim();
                                fields[i].heading = (!typed.is_empty() && typed != fields[i].name).then(|| typed.to_string());
                                change = Some(json!({"fields": fields}));
                            }
                            ui.end_row();
                            if matches!(kind, schedule::Kind::Number | schedule::Kind::Length | schedule::Kind::Area) {
                                let l = ui.label(t("Calculate totals"));
                                let mut on = fields[i].total;
                                let r = ui.checkbox(&mut on, "");
                                r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, t("Calculate totals")));
                                if r.labelled_by(l.id).changed() {
                                    fields[i].total = on;
                                    change = Some(json!({"fields": fields}));
                                }
                                ui.end_row();
                            }
                            if let Some(formula) = fields[i].formula.clone() {
                                let l = ui.label(t("Formula"));
                                if let Some(typed) = self.field(ui, key, &format!("formula.{i}"), formula, l.id, "") {
                                    fields[i].formula = Some(typed.trim().to_string());
                                    change = Some(json!({"fields": fields}));
                                }
                                ui.end_row();
                            }
                        });
                    }
                    // A calculated value being made: its name, what it comes to, its formula.
                    let (mut add, mut drop) = (false, false);
                    if let Some((name, kind, formula)) = &mut d.calculated {
                        ui.add_space(6.0);
                        ui.label(RichText::new(t("Calculated Value")).strong());
                        Grid::new("calculated").num_columns(2).spacing(vec2(12.0, 6.0)).min_col_width(130.0).show(ui, |ui| {
                            let l = ui.label(t("Name"));
                            ui.add(TextEdit::singleline(name).desired_width(240.0)).labelled_by(l.id);
                            ui.end_row();
                            let l = ui.label(t("Type"));
                            let now = RESULTS.iter().find(|(k, _)| k == kind).map_or("Number", |(_, n)| n);
                            let c = ComboBox::from_id_salt("calculated-kind").selected_text(t(now)).width(160.0).show_ui(ui, |ui| {
                                for (k, label) in RESULTS {
                                    ui.selectable_value(kind, k, t(label));
                                }
                            });
                            c.response.labelled_by(l.id);
                            ui.end_row();
                            let l = ui.label(t("Formula"));
                            ui.add(TextEdit::singleline(formula).desired_width(240.0).hint_text("Area * 120")).labelled_by(l.id);
                            ui.end_row();
                        });
                        ui.horizontal(|ui| {
                            add = ui.add_enabled(!name.trim().is_empty() && !formula.trim().is_empty(), Button::new(t("OK"))).clicked();
                            drop = ui.button(t("Cancel")).clicked();
                        });
                    }
                    if drop {
                        d.calculated = None;
                    }
                    if add && let Some((name, kind, formula)) = d.calculated.clone() {
                        let mut with = fields.clone();
                        with.push(Field {
                            name: name.trim().to_string(),
                            formula: Some(formula.trim().to_string()),
                            kind: Some(kind),
                            ..Field::default()
                        });
                        change = Some(json!({"fields": with}));
                    }
                }
                1 => {
                    let mut filters: Vec<Filter> = sc.filters.clone();
                    if self.filter_rows(ui, key, &mut filters, &names, &|n| kind_of(&fields, n), units) {
                        change = Some(json!({"filters": filters}));
                    }
                }
                _ => {
                    let mut sort: Vec<Sort> = sc.sort.clone();
                    let mut edited = false;
                    Grid::new("schedule-sort").num_columns(4).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
                        let mut gone = None;
                        for (i, s) in sort.iter_mut().enumerate() {
                            let caption = ui.label(t(if i == 0 { "Sort by:" } else { "Then by:" }));
                            let c = ComboBox::from_id_salt(("sort-field", i)).selected_text(t(&s.field)).width(170.0).show_ui(ui, |ui| {
                                for n in &names {
                                    if ui.selectable_label(*n == s.field, t(n)).clicked() && *n != s.field {
                                        s.field = n.clone();
                                        edited = true;
                                    }
                                }
                            });
                            c.response.labelled_by(caption.id);
                            let now = if s.descending { "Descending" } else { "Ascending" };
                            let c = ComboBox::from_id_salt(("sort-way", i)).selected_text(t(now)).width(120.0).show_ui(ui, |ui| {
                                for (down, label) in [(false, "Ascending"), (true, "Descending")] {
                                    if ui.selectable_label(s.descending == down, t(label)).clicked() && s.descending != down {
                                        s.descending = down;
                                        edited = true;
                                    }
                                }
                            });
                            c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, t("Order")));
                            if ui.button(t("Remove")).clicked() {
                                gone = Some(i);
                            }
                            ui.end_row();
                        }
                        if let Some(i) = gone {
                            sort.remove(i);
                            edited = true;
                        }
                    });
                    if ui.button(t("Add Sort")).clicked()
                        && let Some(first) = fields.first()
                    {
                        sort.push(Sort { field: first.name.clone(), descending: false });
                        edited = true;
                    }
                    if edited {
                        change = Some(json!({"sort": sort}));
                    }
                    ui.add_space(6.0);
                    let mut every = !sc.grouped;
                    if ui.checkbox(&mut every, t("Itemize every instance")).changed() {
                        change = Some(json!({"grouped": !every}));
                    }
                }
            }
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = ui.button(t("Close")).clicked());
        });
        if let Some(mut p) = change {
            p["name"] = json!(sc.name);
            let done = self.run("schedule", p).is_some();
            if done && d.calculated.is_some() && d.tab == 0 {
                d.calculated = None;
            }
        }
        if !(close || r.should_close()) {
            self.schedule_dialog = Some(d);
        }
    }

    /// Keyboard Shortcuts (KS): each shortcut by what it does, as Revit's dialog lists them.
    pub(crate) fn shortcuts_window(&mut self, ctx: &Context) {
        if !self.show_shortcuts {
            return;
        }
        let mut by_name: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (letters, name) in crate::shortcuts() {
            by_name.entry(t(name)).or_default().push(letters);
        }
        let rows: Vec<(&str, Vec<&str>)> = by_name.into_iter().collect();
        let mut close = false;
        let r = Modal::new(UiId::new("shortcuts")).show(ctx, |ui| {
            ui.label(RichText::new(t("Keyboard Shortcuts")).heading());
            ui.add_space(6.0);
            ScrollArea::vertical().max_height(480.0).show(ui, |ui| {
                // In two columns, each running down, as a reference card.
                ui.horizontal_top(|ui| {
                    for (k, part) in rows.chunks(rows.len().div_ceil(2).max(1)).enumerate() {
                        Grid::new(("shortcuts", k)).num_columns(2).striped(true).spacing(vec2(24.0, 4.0)).show(ui, |ui| {
                            for (name, letters) in part {
                                ui.label(*name);
                                ui.monospace(letters.join(", "));
                                ui.end_row();
                            }
                        });
                        ui.add_space(24.0);
                    }
                });
            });
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = ui.button(t("Close")).clicked());
        });
        if close || r.should_close() {
            self.show_shortcuts = false;
        }
    }
}
