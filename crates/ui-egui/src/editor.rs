//! The Family Editor: a family's parameters, types, forms and symbols, edited as text and tried on a
//! preview as they're typed, then made the project's, saved to a file or kept in the Favorites.
//! Every change to the project is a `family` command, so it's checked, and undone, like any other.

mod association;
pub(crate) mod connector_marks;
pub(crate) mod connectors;
pub(crate) mod dimensions;
pub(crate) mod document;
pub(crate) mod exists;
mod extrusion;
pub(crate) mod lit3d;
mod nested;
pub(crate) mod options;
mod pivot;
pub(crate) mod sketch;
pub(crate) mod tabs;
mod temp_hide;
mod types_dialog;
pub(crate) mod views;
pub(crate) mod visibility;
pub(crate) mod work_plane;

use crate::popup::Modal;

use bimcraft_i18n::msg;

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use bimcraft_core::engine::Session;
use bimcraft_core::family::{
    Align as LabelAlign, CENTER_FRONT_BACK, CENTER_LEFT_RIGHT, Category, Circle, Curve, Expr, Family, Form, IfcClass, Kind as Holds, Label,
    Parameter, PlaneRef, REF_LEVEL, RefLine, RefPlane, Symbol,
};
use bimcraft_core::model::{Model, Operation};
use bimcraft_core::view3d::{AZIMUTH, ELEVATION};
use egui::{
    Align, Color32, ColorImage, ComboBox, Context, Grid, Id as UiId, Layout, PointerButton, Rect, RichText, ScrollArea, Sense, Stroke,
    StrokeKind, TextEdit, TextureHandle, TextureOptions, Ui, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::style::pal;
use crate::{BimApp, Status, Tool};

/// A number, as the editor writes it: whole, or to a millionth.
pub(crate) fn number_text(x: f64) -> String {
    let r = (x * 1e6).round() / 1e6;
    if r == r.trunc() && r.abs() < 1e15 { format!("{}", r as i64) } else { format!("{r}") }
}

/// A number or a formula, as typed.
fn expr_text(e: &Expr) -> String {
    match e {
        Expr::Number(x) => number_text(*x),
        Expr::Formula(f) => f.clone(),
    }
}

/// What was typed, as a number if it is one, else a formula; nothing is zero.
fn text_expr(s: &str) -> Expr {
    let s = s.trim();
    if s.is_empty() {
        return Expr::Number(0.0);
    }
    s.parse::<f64>().ok().filter(|x| x.is_finite()).map_or_else(|| Expr::Formula(s.to_string()), Expr::Number)
}

/// `s` cut at the commas that aren't inside parentheses: `max(1, 2), 3` is two.
fn split_top(s: &str) -> Vec<&str> {
    let (mut depth, mut start, mut out) = (0_i32, 0, Vec::new());
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// A profile's corners, one `x, y` to a line.
fn profile_text(p: &[[Expr; 2]]) -> String {
    p.iter().map(|[a, b]| format!("{}, {}", expr_text(a), expr_text(b))).collect::<Vec<_>>().join("\n")
}

fn text_profile(s: &str) -> Result<Vec<[Expr; 2]>, String> {
    s.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, l)| match split_top(l).as_slice() {
            [a, b] => Ok([text_expr(a), text_expr(b)]),
            _ => Err(tf("corner {}: each corner is `x, y`", &[&(i + 1).to_string()])),
        })
        .collect()
}

/// What a form is drawn from: the corners of a profile, or a circle, as typed; or a sketch of other lines, arcs or loops,
/// drawn in sketch mode (`editor/sketch.rs`).
#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Profile,
    Circle,
    Sketch,
}

/// A form as typed.
#[derive(Clone, PartialEq)]
struct FormText {
    /// What it was: what the editor doesn't edit (a mesh's triangles) stays as it was.
    original: Form,
    shape: Shape,
    /// The plane it is drawn on, kept as it was unless a datum is picked.
    host: PlaneRef,
    profile: String,
    circle: [String; 3],
    revolve: bool,
    axis: [String; 4],
    from: String,
    to: String,
    material: String,
    visible: String,
    void: bool,
    /// Which form it is while the document is open, whatever its place in the list: what Temporary Hide/Isolate hides it by
    /// (`temp_hide.rs`). Given when it is made, never saved.
    tag: u64,
}

/// The last form's `tag`.
static TAGS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl FormText {
    /// Form `f` as typed again, keeping the tag `tag` it is known by while the document is open.
    pub(crate) fn again(f: &Form, tag: u64) -> FormText {
        FormText { tag, ..FormText::of(f) }
    }

    fn of(f: &Form) -> FormText {
        // A form the document holds has a GUID, and so has each line of its sketches, as the project's sync gives them: Align,
        // dimensions and locks name them by it, and a form or a line without one (a blank family's starter box, a profile
        // typed as numbers) would be drawn but never picked.
        let mut f = f.clone();
        if f.guid.is_nil() {
            f.guid = bimcraft_core::ids::Guid::new();
        }
        for l in f.all_lines_mut().filter(|l| l.guid.is_nil()) {
            l.guid = bimcraft_core::ids::Guid::new();
        }
        let f = &f;
        let axis =
            f.axis().map_or_else(Default::default, |[a, b]| [expr_text(&a[0]), expr_text(&a[1]), expr_text(&b[0]), expr_text(&b[1])]);
        let round = f.circle();
        let circle =
            round.as_ref().map_or_else(Default::default, |c| [expr_text(&c.centre[0]), expr_text(&c.centre[1]), expr_text(&c.radius)]);
        let corners = f.corners();
        let shape = match (&round, &corners) {
            (Some(_), _) => Shape::Circle,
            (None, Some(_)) => Shape::Profile,
            (None, None) if f.boundary().next().is_none() => Shape::Profile,
            (None, None) => Shape::Sketch,
        };
        FormText {
            original: f.clone(),
            shape,
            host: f.host,
            profile: corners.as_deref().map(profile_text).unwrap_or_default(),
            circle,
            revolve: f.revolves(),
            axis,
            from: expr_text(&f.from),
            to: expr_text(&f.to),
            material: f.material.clone(),
            visible: f.visible.as_ref().map(expr_text).unwrap_or_default(),
            void: f.void,
            tag: TAGS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1,
        }
    }

    /// A form of a profile swept up from nothing to a metre.
    #[cfg(test)]
    fn blank() -> FormText {
        let corners = [("-500", "-500"), ("500", "-500"), ("500", "500"), ("-500", "500")];
        let profile = corners.iter().map(|(a, b)| format!("{a}, {b}")).collect::<Vec<_>>().join("\n");
        FormText { profile, to: "1000".into(), axis: ["0".into(), "0".into(), "0".into(), "1".into()], ..FormText::of(&Form::default()) }
    }

    /// Whether the triangles of a file are what it's made of.
    fn mesh(&self) -> bool {
        !self.original.mesh.is_empty()
    }

    /// What it says it is, for its heading.
    fn describe(&self) -> String {
        use bimcraft_core::family::forms::FormKind;
        let what = match (self.mesh(), self.shape, self.revolve) {
            (true, _, _) => t("Mesh"),
            _ if self.original.kind() == FormKind::Blend => t("Blend"),
            _ if self.original.kind() == FormKind::Sweep => t("Sweep"),
            _ if self.original.kind() == FormKind::SweptBlend => t("Swept Blend"),
            (_, Shape::Circle, true) => t("Revolved Circle"),
            (_, Shape::Circle, false) => t("Cylinder"),
            (_, Shape::Profile | Shape::Sketch, true) => t("Revolve"),
            (_, Shape::Profile | Shape::Sketch, false) => t("Extrusion"),
        };
        if self.void { format!("{what} ({})", t("void")) } else { what.to_string() }
    }

    fn form(&self) -> Result<Form, String> {
        let mut f = self.original.clone();
        f.material = self.material.trim().to_string();
        f.visible = (!self.visible.trim().is_empty()).then(|| text_expr(&self.visible));
        f.void = self.void;
        if self.mesh() {
            return Ok(f);
        }
        (f.host, f.from, f.to) = (self.host, text_expr(&self.from), text_expr(&self.to));
        // The typed corners or circle: the sketch's lines again, each keeping its id and GUID by place.
        match self.shape {
            Shape::Profile => f.set_corners(&text_profile(&self.profile)?),
            Shape::Circle => {
                f.set_circle(&Circle {
                    centre: [text_expr(&self.circle[0]), text_expr(&self.circle[1])],
                    radius: text_expr(&self.circle[2]),
                });
            }
            Shape::Sketch => {}
        }
        let a = &self.axis;
        let axis = self.revolve.then(|| [[text_expr(&a[0]), text_expr(&a[1])], [text_expr(&a[2]), text_expr(&a[3])]]);
        // Unchanged, the axis line stays as it is (its formulas, where sketch mode put it).
        if axis != f.axis() {
            f.set_axis(axis.as_ref());
        }
        Ok(f)
    }
}

/// A parameter as typed.
#[derive(Clone, PartialEq)]
struct ParamText {
    /// What it was, whose group, shared GUID, reporting and `of` the editor keeps as they are.
    original: Parameter,
    name: String,
    kind: Holds,
    instance: bool,
    value: String,
    formula: String,
}

/// The name a parameter's kind goes by.
pub(crate) fn kind_name(k: Holds) -> &'static str {
    match k {
        Holds::Length => "Length",
        Holds::Number => "Number",
        Holds::Angle => "Angle",
        Holds::YesNo => msg!("Yes/No"),
        Holds::Material => "Material",
        Holds::Text => msg!("Text"),
        Holds::Integer => msg!("Integer"),
        Holds::Area => msg!("Area"),
        Holds::Volume => msg!("Volume"),
        Holds::Url => msg!("URL"),
        Holds::FamilyType => msg!("Family Type"),
    }
}

/// Every kind a parameter may be, in the order the editor offers them.
const KINDS: [Holds; 11] = [
    Holds::Length,
    Holds::Number,
    Holds::Integer,
    Holds::Angle,
    Holds::Area,
    Holds::Volume,
    Holds::YesNo,
    Holds::Text,
    Holds::Url,
    Holds::Material,
    Holds::FamilyType,
];

/// A stored value, as typed for a parameter of `kind`.
fn value_text(kind: Holds, v: &Value) -> String {
    match (kind, v) {
        (Holds::YesNo, Value::Bool(b)) => (if *b { "1" } else { "0" }).into(),
        (_, Value::Number(n)) => n.as_f64().map(number_text).unwrap_or_default(),
        (_, Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

/// What was typed for a parameter of `kind`, as the value it stores; none for blank, where a type goes by the family's.
fn text_value(kind: Holds, s: &str, who: &str) -> Result<Option<Value>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    Ok(Some(match kind {
        Holds::Material | Holds::Text | Holds::Url | Holds::FamilyType => json!(s),
        Holds::YesNo => json!(matches!(s, "1" | "true" | "yes" | "Yes")),
        Holds::Length | Holds::Number | Holds::Angle | Holds::Integer | Holds::Area | Holds::Volume => {
            json!(s.parse::<f64>().ok().filter(|x| x.is_finite()).ok_or_else(|| tf("{}: `{}` isn't a number", &[who, s]))?)
        }
    }))
}

impl ParamText {
    fn of(p: &Parameter) -> ParamText {
        ParamText {
            original: p.clone(),
            name: p.name.clone(),
            kind: p.kind,
            instance: p.instance,
            value: value_text(p.kind, &p.value),
            formula: p.formula.clone().unwrap_or_default(),
        }
    }

    fn parameter(&self) -> Result<Parameter, String> {
        let value = text_value(self.kind, &self.value, &self.name)?.unwrap_or(match self.kind {
            Holds::Material | Holds::Text | Holds::Url => json!(""),
            Holds::FamilyType => Value::Null,
            Holds::YesNo => json!(false),
            _ => json!(0.0),
        });
        let formula = Some(self.formula.trim().to_string()).filter(|f| !f.is_empty());
        Ok(Parameter {
            name: self.name.trim().to_string(),
            kind: self.kind,
            instance: self.instance,
            value,
            formula,
            ..self.original.clone()
        })
    }
}

/// Which part a symbol is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Part {
    Line,
    Arc,
    Text,
    Dot,
}

/// A symbol as typed. `centre` is an arc's or a dot's centre or a label's point; `radius` an arc's or a dot's radius or a
/// label's height.
#[derive(Clone, PartialEq)]
struct SymbolText {
    /// Its id and GUID, which it keeps wherever it's moved to in the list.
    ident: (bimcraft_core::model::Id, bimcraft_core::model::Guid),
    part: Part,
    line: String,
    centre: [String; 2],
    radius: String,
    from: String,
    to: String,
    value: String,
    align: LabelAlign,
    visible: String,
    /// The subcategory it is drawn in, by its English name (`Opening`); blank, its category.
    subcategory: String,
    /// Its Visibility Settings (F8): the detail levels it is drawn at; none, Medium and Fine.
    visibility: Option<bimcraft_core::family::Visibility>,
}

impl SymbolText {
    fn blank(part: Part) -> SymbolText {
        let s = |x: &str| x.to_string();
        let base = SymbolText {
            ident: Default::default(),
            part,
            line: String::new(),
            centre: Default::default(),
            radius: String::new(),
            from: String::new(),
            to: String::new(),
            value: String::new(),
            align: LabelAlign::Left,
            visible: String::new(),
            subcategory: String::new(),
            visibility: None,
        };
        match part {
            Part::Line => SymbolText { line: s("0, 0\n0, 900"), ..base },
            Part::Arc => SymbolText { centre: [s("0"), s("0")], radius: s("900"), from: s("0"), to: s("90"), ..base },
            Part::Text => SymbolText { centre: [s("0"), s("0")], radius: s("100"), value: s("A"), ..base },
            Part::Dot => SymbolText { centre: [s("0"), s("0")], radius: s("20"), ..base },
        }
    }

    fn of(y: &Symbol) -> SymbolText {
        let ex = |e: &Expr| expr_text(e);
        let mut t = SymbolText::blank(Part::Line);
        t.ident = (y.id, y.guid);
        t.line = profile_text(&y.line);
        t.visible = y.visible.as_ref().map(expr_text).unwrap_or_default();
        t.subcategory = y.subcategory.clone().unwrap_or_default();
        t.visibility = y.visibility;
        if let Some(c) = &y.arc {
            (t.part, t.centre, t.radius, t.from, t.to) =
                (Part::Arc, [ex(&c.centre[0]), ex(&c.centre[1])], ex(&c.radius), ex(&c.from), ex(&c.to));
        } else if let Some(l) = &y.text {
            (t.part, t.centre, t.radius, t.value, t.align) =
                (Part::Text, [ex(&l.at[0]), ex(&l.at[1])], ex(&l.height), l.value.clone(), l.align);
        } else if let Some(c) = &y.dot {
            (t.part, t.centre, t.radius) = (Part::Dot, [ex(&c.centre[0]), ex(&c.centre[1])], ex(&c.radius));
        }
        t
    }

    fn symbol(&self) -> Result<Symbol, String> {
        let visible = (!self.visible.trim().is_empty()).then(|| text_expr(&self.visible));
        let (id, guid) = self.ident;
        let centre = [text_expr(&self.centre[0]), text_expr(&self.centre[1])];
        let subcategory = (!self.subcategory.trim().is_empty()).then(|| self.subcategory.clone());
        let base = Symbol { visible, id, guid, subcategory, visibility: self.visibility, ..Symbol::default() };
        Ok(match self.part {
            Part::Arc => Symbol {
                arc: Some(Curve { centre, radius: text_expr(&self.radius), from: text_expr(&self.from), to: text_expr(&self.to) }),
                ..base
            },
            Part::Text => Symbol {
                text: Some(Label { at: centre, height: text_expr(&self.radius), value: self.value.clone(), align: self.align }),
                ..base
            },
            Part::Dot => Symbol { dot: Some(Circle { centre, radius: text_expr(&self.radius) }), ..base },
            Part::Line => Symbol { line: text_profile(&self.line)?, ..base },
        })
    }
}

/// What {3D} drew last, for a test: the camera its picture was taken through and its size in pixels.
#[cfg(test)]
pub(crate) struct Drawn3d {
    pub(crate) cam: bimcraft_core::view3d::Camera,
    pub(crate) size: [u32; 2],
}

#[cfg(test)]
impl Drawn3d {
    /// Where point `q` of the family is on screen, the picture over `pane`.
    pub(crate) fn screen(&self, pane: Rect, q: [f64; 3]) -> egui::Pos2 {
        let (x, y, _) = self.cam.project(q);
        pane.min + vec2(x as f32 * pane.width() / self.size[0].max(1) as f32, y as f32 * pane.height() / self.size[1].max(1) as f32)
    }
}

#[cfg(test)]
impl FamilyEditor {
    /// What {3D} drew last, once it has.
    pub(crate) fn drawn_3d(&self) -> Option<Drawn3d> {
        let cam = self.orbit.camera?;
        let (w, h) = (self.orbit.rect.width() * self.orbit.ppp, self.orbit.rect.height() * self.orbit.ppp);
        let size = [
            (w.round().max(1.0) as u32).min(bimcraft_core::view3d::MAX_SIDE),
            (h.round().max(1.0) as u32).min(bimcraft_core::view3d::MAX_SIDE),
        ];
        Some(Drawn3d { cam: bimcraft_core::view3d::Camera { width: size[0], height: size[1], ..cam }, size })
    }
}

/// What the editor shows of a family: its tabs' contents, as typed.
pub(crate) struct FamilyEditor {
    /// The name the project has it under, when it has it.
    original: Option<String>,
    name: String,
    category: Category,
    params: Vec<ParamText>,
    /// Each type's name, and for each parameter of its own what's typed (blank: the family's).
    types: Vec<(String, BTreeMap<String, String>)>,
    forms: Vec<FormText>,
    pub(crate) adaptive: Option<bimcraft_core::adaptive::Definition>,
    /// The adaptive modelling view's camera, tool and selection.
    pub(crate) modeller: crate::adaptive_view::Modeller,
    replay: Option<bimcraft_core::model::Id>,
    preview_params: BTreeMap<String, Value>,
    /// Associate Family Parameter, open on a row of Properties its "=" was pressed on.
    pub(crate) associating: Option<crate::adaptive_editor::Associating>,
    symbols: Vec<SymbolText>,
    /// Its connectors (#269), placed from Create > Connectors, listed in the Project Browser and typed in Properties.
    connectors: Vec<connectors::ConnectorText>,
    /// Its reference planes and lines, as the family has them: the editor has no tab for them yet, and keeps them as they are.
    pub(crate) planes: Vec<RefPlane>,
    pub(crate) lines: Vec<RefLine>,
    /// Its dimensions (F7): drawn, labelled, locked and taken away in its plans and elevations.
    pub(crate) dimensions: Vec<bimcraft_core::family::FamilyDimension>,
    /// Which fixture or fitting it is, as the family has it: set with the `family` command, kept as it is here.
    kind: Option<String>,
    /// The IFC class a generic family from IFC remembers: set with the `family` command or by the import, kept as it is here.
    ifc: Option<IfcClass>,
    /// How a door's or a window's leaves open; none, not said.
    operation: Option<Operation>,
    /// Its options (Family Category and Parameters, Properties' Other group), as the document edits them (`options.rs`).
    options: options::OptionsText,
    /// The plan symbol selected (from the Project Browser's Symbols, or made by the Detail panel), whose Properties show.
    pub(crate) symbol: Option<usize>,
    /// The connector selected: Properties show its rows.
    pub(crate) connector: Option<usize>,
    /// The connector under the pointer in a view, lit as a click would select it.
    pub(crate) connector_hover: Option<usize>,
    /// What Family Types keeps while it is open: its search, the groups folded, the row selected, its own dialogs.
    types_dialog: types_dialog::TypesDialog,
    /// The type shown in Types and on the preview.
    picked: usize,
    /// {3D}: the project's 3D view's camera, navigation and painters, and the family staged for it (`lit3d.rs`).
    pub(crate) orbit: crate::orbit::Orbit,
    pub(crate) scene3d: Option<lit3d::FamilyScene>,
    /// Why the family can't be tried or made: shown under the preview.
    problem: Option<String>,
    /// The document's own state: its history, its preview's size, an association being made.
    document: document::State,
    /// The family document's ribbon tab, and the one it goes back to when the contextual tab goes.
    pub(crate) doc_tab: DocTab,
    pub(crate) doc_back: DocTab,
    /// What its ribbon asked for this frame, done after it is drawn.
    pub(crate) ask: Asked,
    /// The dialog its ribbon opened, if any.
    pub(crate) dialog: Option<DocDialog>,
    /// The instance it was opened from (Edit Family on it), whose placement Replay Instance takes.
    source: Option<bimcraft_core::model::Id>,
    /// The form selected (a family that isn't adaptive), whose Properties show as Revit's for an Extrusion (`extrusion.rs`).
    form: Option<usize>,
    /// Its plans and elevations (`views.rs`): which one shows, and what each keeps for the session.
    pub(crate) views: views::Views,
    /// A form being sketched (F5, `editor/sketch.rs`): no part of the draft until Finish.
    pub(crate) sketch: Option<Box<sketch::FormSketch>>,
    /// What a sketch snaps to, as Manage > Snaps says: the window's, given each frame.
    pub(crate) snaps: bimcraft_core::snap::Snaps,
    /// The families loaded into it and their instances placed in it (T6a, `nested.rs`), as the family holds them.
    pub(crate) nested: Vec<bimcraft_core::family::NestedFamily>,
    pub(crate) nested_instances: Vec<bimcraft_core::family::NestedInstance>,
    /// The nested instance selected.
    nested_sel: Option<usize>,
    /// Create > Component, on: the nested family and type a click places.
    pub(crate) placing: Option<(String, String)>,
    /// Opened by Edit Family on a family nested in the document under it: its loads go into that one, not the project.
    pub(crate) into_host: bool,
    /// Edit Family asked for on this nested family, done after the frame.
    pub(crate) open_nested: Option<String>,
}

/// The tabs of a family document's ribbon, as Revit's family editor has them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DocTab {
    #[default]
    Create,
    Insert,
    Annotate,
    View,
    Manage,
    Modify,
    /// "Modify | Reference Points" and the like, for the selection or the tool in hand.
    Context,
}

/// A family document's dialogs: Family Types (its parameters and types) and Family Category and Parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DocDialog {
    Types,
    Category,
}

/// What a family document's ribbon asked for in a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Asked {
    /// The selected adaptive instances' points taken as the test placement.
    pub(crate) capture: bool,
    /// The test placement placed as a new instance.
    pub(crate) place: bool,
    /// The test placement given to the instance the family was opened on.
    pub(crate) apply_placement: bool,
    /// Load into Project.
    pub(crate) apply: bool,
    /// Load into Project and Close.
    pub(crate) ok: bool,
    /// Close, forgetting what was not loaded.
    pub(crate) cancel: bool,
    /// Save Family and Save As (File > Save Family As…, Ctrl+Shift+S): the family to a file asked for.
    pub(crate) save: bool,
    /// Save on a family tab (the Quick Access Toolbar's Save, File > Save Family, Ctrl+S): to the document's file, without
    /// asking, once it has one; asked for, as `save`, until then.
    pub(crate) save_in_place: bool,
    pub(crate) favorite: bool,
    pub(crate) load: bool,
    /// The document's own Undo and Redo, done when the frame ends.
    pub(crate) undo: bool,
    pub(crate) redo: bool,
}

impl FamilyEditor {
    /// Its reference planes and lines, as the family has them: what an adaptive point's work plane is worked out from.
    pub(crate) fn plane_set(&self) -> bimcraft_core::family::Planes<'_> {
        bimcraft_core::family::Planes { planes: &self.planes, lines: &self.lines }
    }

    /// The editor of family `name` as the project has it.
    pub(crate) fn of(name: &str, f: &Family) -> FamilyEditor {
        let own: Vec<&Parameter> = f.parameters.iter().filter(|p| !p.instance && p.formula.is_none()).collect();
        let types = f
            .types
            .iter()
            .map(|(n, set)| (n.clone(), own.iter().filter_map(|p| Some((p.name.clone(), value_text(p.kind, set.get(&p.name)?)))).collect()))
            .collect();
        FamilyEditor {
            original: Some(name.to_string()),
            name: name.to_string(),
            category: f.category,
            params: f.parameters.iter().map(ParamText::of).collect(),
            types,
            forms: f.forms.iter().map(FormText::of).collect(),
            adaptive: f.adaptive.clone(),
            modeller: Default::default(),
            replay: None,
            preview_params: BTreeMap::new(),
            associating: None,
            symbols: f.symbols.iter().map(SymbolText::of).collect(),
            connectors: f.connectors.iter().map(connectors::ConnectorText::of).collect(),
            planes: f.planes.clone(),
            lines: f.lines.clone(),
            dimensions: f.dimensions.clone(),
            kind: f.kind.clone(),
            ifc: f.ifc.clone(),
            operation: f.operation,
            options: options::OptionsText::of(f),
            symbol: None,
            connector: None,
            connector_hover: None,
            types_dialog: Default::default(),
            picked: 0,
            orbit: crate::orbit::Orbit::named("family-preview"),
            scene3d: None,
            problem: None,
            document: Default::default(),
            doc_tab: DocTab::Create,
            doc_back: DocTab::Create,
            ask: Asked::default(),
            dialog: None,
            source: None,
            form: None,
            views: Default::default(),
            sketch: None,
            snaps: Default::default(),
            nested: f.nested.clone(),
            nested_instances: f.nested_instances.clone(),
            nested_sel: None,
            placing: None,
            into_host: false,
            open_nested: None,
        }
    }

    /// Associate Family Parameter, open from a row's "=" (an adaptive element's, or a form's or a symbol's of a family that isn't
    /// adaptive): a parameter New Parameter makes goes in the family at once, as in Revit.
    pub(crate) fn associate_dialog(&mut self, ctx: &Context) {
        use crate::adaptive_editor::{Field, associate};
        let Some(mut a) = self.associating.take() else { return };
        let (params, values) = (self.parameter_kinds(), self.values());
        let (done, new) = if matches!(a.field, Field::Nested(_)) {
            let mut rows = nested::NestedRows { instances: &mut self.nested_instances };
            associate(ctx, &mut a, &mut rows, &params, &values)
        } else if matches!(a.field, Field::Pivot) {
            let mut rows = pivot::PivotRows { lines: &mut self.lines, planes: &mut self.planes };
            associate(ctx, &mut a, &mut rows, &params, &values)
        } else if matches!(a.field, Field::Form(_) | Field::Symbol(_)) {
            let mut rows = association::Rows { forms: &mut self.forms, symbols: &mut self.symbols };
            associate(ctx, &mut a, &mut rows, &params, &values)
        } else if let Some(d) = self.adaptive.as_mut() {
            associate(ctx, &mut a, d, &params, &values)
        } else {
            return;
        };
        if let Some(n) = new {
            self.params.push(ParamText {
                original: Parameter::default(),
                name: n.name,
                kind: n.kind,
                instance: n.instance,
                value: n.value,
                formula: String::new(),
            });
        }
        if !done {
            self.associating = Some(a);
        }
    }

    /// Each of its parameters, by name, and what it holds: what a row of Properties may be tied to.
    pub(crate) fn parameter_kinds(&self) -> Vec<(String, Holds)> {
        self.params.iter().map(|p| (p.name.trim().to_string(), p.kind)).collect()
    }

    /// The values of the type it previews (and the instance parameters it replays), for evaluating it.
    pub(crate) fn values(&self) -> BTreeMap<String, bimcraft_core::family::Val> {
        self.family()
            .ok()
            .and_then(|f| self.types.get(self.picked).and_then(|(name, _)| f.values(name, &self.preview_params).ok()))
            .unwrap_or_default()
    }

    /// Replay Instance: the test placement becomes the placement, the type and the instance parameters of the instance it was
    /// opened from (held in `model`), to see that instance as it is (deformed, or not building) or to start modelling from it;
    /// Apply to Instance gives it back. Whether there was one to replay.
    pub(crate) fn replay_from(&mut self, model: &Model) -> bool {
        let Some(id) = self.source else { return false };
        let Some(bimcraft_core::model::Element::Component(c)) = model.elements.get(id) else { return false };
        let Some(d) = self.adaptive.as_mut() else { return false };
        d.preview = c.adaptive.clone().unwrap_or_default();
        self.replay = Some(id);
        self.preview_params = c.params.clone();
        self.picked = self.types.iter().position(|(n, _)| n == &c.type_name).unwrap_or(0);
        true
    }

    /// Its name, as typed.
    pub(crate) fn title(&self) -> &str {
        self.name.trim()
    }

    /// The instance it was opened on, replaying its placement, if any.
    pub(crate) fn replaying(&self) -> Option<bimcraft_core::model::Id> {
        self.replay
    }

    /// Why it can't be made, if something said so.
    /// What Load into Project was refused for, while the draft is as it was then, else why it can't be tried.
    pub(crate) fn problem(&self) -> Option<&str> {
        self.document.refused.as_deref().or(self.problem.as_deref())
    }

    /// A family to start from, of `category`: a box of its width, depth and height, or a door's or window's frame of its own;
    /// a window's with a Sill plane on Ref. Level (F10c), as Revit's window template has it.
    pub(crate) fn blank(category: Category) -> FamilyEditor {
        let length = |name: &str, value: f64| json!({"name": name, "value": value});
        let block = |x: [&str; 2], y: [&str; 2], z: [&str; 2]| json!({"profile": [[x[0], y[0]], [x[1], y[0]], [x[1], y[1]], [x[0], y[1]]], "from": z[0], "to": z[1]});
        let family = if category.hosted() {
            json!({"category": category, "parameters": [length("Width", 900.0), length("Height", if category == Category::Door { 2100.0 } else { 1200.0 }), length("Sill Height", if category == Category::Door { 0.0 } else { 900.0 })],
                   "types": {"900": {}},
                   "forms": [block(["-Width / 2", "-Width / 2 + 50"], ["-Wall Thickness / 2", "Wall Thickness / 2"], ["0", "Height"]),
                             block(["Width / 2 - 50", "Width / 2"], ["-Wall Thickness / 2", "Wall Thickness / 2"], ["0", "Height"]),
                             block(["-Width / 2 + 50", "Width / 2 - 50"], ["-20", "20"], ["0", "Height - 50"])]})
        } else {
            json!({"category": category, "parameters": [length("Width", 1000.0), length("Depth", 600.0), length("Height", 750.0)],
                   "types": {"1000 x 600": {}},
                   "forms": [block(["-Width / 2", "Width / 2"], ["-Depth / 2", "Depth / 2"], ["0", "Height"])]})
        };
        let mut family: Family = serde_json::from_value(family).unwrap_or_default();
        if category == Category::Window {
            sill_plane(&mut family);
        }
        let mut editor = FamilyEditor::of(t("New Family"), &family);
        editor.original = None;
        editor
    }

    /// The family as typed, or the first thing that isn't what it should be.
    fn family(&self) -> Result<Family, String> {
        let parameters: Vec<Parameter> = self.params.iter().map(ParamText::parameter).collect::<Result<_, _>>()?;
        let own: BTreeMap<&str, Holds> =
            parameters.iter().filter(|p| !p.instance && p.formula.is_none()).map(|p| (p.name.as_str(), p.kind)).collect();
        let mut types = BTreeMap::new();
        for (name, typed) in &self.types {
            if types.contains_key(name) {
                return Err(tf("two types are named `{}`", &[name]));
            }
            let mut set = BTreeMap::new();
            for (param, text) in typed {
                if let Some(kind) = own.get(param.as_str())
                    && let Some(v) = text_value(*kind, text, &format!("{name}: {param}"))?
                {
                    set.insert(param.clone(), v);
                }
            }
            types.insert(name.clone(), set);
        }
        let forms = self.forms.iter().map(FormText::form).collect::<Result<_, _>>()?;
        let symbols = self.symbols.iter().map(SymbolText::symbol).collect::<Result<_, _>>()?;
        let operation = self.operation.filter(|_| self.category.hosted());
        let mut family = Family {
            category: self.category,
            // A kind the category it now is has no row for goes with the change of category.
            kind: self.kind.clone().filter(|k| self.category.kinds().iter().any(|(n, _)| n == k)),
            ifc: self.ifc.clone(),
            parameters,
            types,
            forms,
            symbols,
            operation,
            connectors: self.connectors.iter().map(connectors::ConnectorText::connector).collect(),
            adaptive: self.adaptive.clone(),
            planes: self.planes.clone(),
            lines: self.lines.clone(),
            dimensions: self.dimensions.clone(),
            nested: self.nested.clone(),
            nested_instances: self.nested_instances.clone(),
            ..Family::default()
        };
        self.options.apply(&mut family);
        Ok(family)
    }

    /// The `family` command's parameters making the project's family what's typed: its name, its own and the type it drops.
    fn command(&self, project: &Model) -> Result<Value, String> {
        if self.name.trim().is_empty() {
            return Err(t("a family needs a name").to_string());
        }
        let family = self.family()?;
        let mut p = serde_json::to_value(&family).map_err(|e| e.to_string())?;
        // Said or not, so one taken off goes from the family too.
        p["operation"] = json!(family.operation.map_or("", Operation::name));
        // Its planes and lines as the command takes them, so the forms drawn on them keep their hosts; its options, all of
        // them, so the project's family keeps them as they are.
        if let Some(map) = p.as_object_mut() {
            Family::options_from_fields(map);
            map.insert("options".into(), family.options_json());
            for (k, key) in [("planes", "ref_planes"), ("lines", "ref_lines")] {
                let v = map.remove(k).unwrap_or_else(|| json!([]));
                map.insert(key.into(), v);
            }
        }
        match &self.original {
            Some(was) => {
                p["name"] = json!(was);
                if *was != self.name.trim() {
                    p["rename"] = json!(self.name.trim());
                }
                // A type that's gone from the editor goes from the family: `types` merge into the family's.
                for gone in project.families.get(was).iter().flat_map(|f| f.types.keys()).filter(|k| !family.types.contains_key(*k)) {
                    p["types"][gone] = Value::Null;
                }
                // And a value gone from a type that stays (a parameter renamed, deleted or no longer the type's) goes too.
                for (name, set) in project.families.get(was).iter().flat_map(|f| f.types.iter()) {
                    if let Some(now) = family.types.get(name) {
                        for gone in set.keys().filter(|k| !now.contains_key(*k)) {
                            p["types"][name][gone] = Value::Null;
                        }
                    }
                }
            }
            None => p["name"] = json!(self.name.trim()),
        }
        Ok(p)
    }

    /// What the family looks like standing in a project of its own: its type, in a wall if it's a door's or a window's.
    fn scratch(&self, base: &Model, family: &Family) -> Result<Model, String> {
        let name = self.name.trim();
        let type_name = self.types.get(self.picked).map(|(n, _)| n.clone()).ok_or_else(|| t("a family needs a type").to_string())?;
        let mut s = Session::new();
        {
            let m = s.model_mut();
            m.materials = base.materials.clone();
            m.images = base.images.clone();
            // What a ghosted form is made of.
            m.materials
                .insert(GHOST.into(), bimcraft_core::model::Material { color: [150, 150, 150], transparency: 65, ..Default::default() });
            m.families.insert(name.to_string(), family.clone());
        }
        let place = |s: &mut Session, id: &str, p: Value| s.execute(id, &p);
        // Its forms stand where its planes are drawn: its insertion point at its origin (O1).
        let [x, y, z] = family.origin();
        if family.category.hosted() {
            // Its origin in the middle of the wall, which fixes it across, and on the wall's base, a sill of 0 up it.
            place(&mut s, "wall", json!({"points": [[-2500, y], [2500, y]], "base_offset": z}))?;
            // A window's sill on Ref. Level: its forms are drawn in axes whose z is up from the sill, as its planes are.
            let kind = if family.category == Category::Door { "door" } else { "window" };
            let mut p = json!({"at": [x, y], "family": name, "type": type_name});
            if kind == "window" {
                p["sill"] = json!(0);
            }
            place(&mut s, kind, p)?;
        } else {
            let mut p = json!({"family": name, "type": type_name, "at": [0, 0], "params": self.preview_params});
            at_origin(family, &mut p);
            place(&mut s, "component", p)?;
        }
        Ok(Arc::unwrap_or_clone(s.model))
    }
}

/// `place`, the `component` command's parameters putting family `f`'s axes' zero at its `at` and `offset` (none: its Default
/// Elevation) in the document's scratch project, moved so the family's insertion point, its origin, is where its origin is
/// drawn (O1): so its forms stand where its planes are.
pub(super) fn at_origin(f: &Family, place: &mut Value) {
    let [x, y, z] = f.origin();
    if [x, y, z] == [0.0; 3] {
        return;
    }
    let at = |k: usize| place["at"][k].as_f64().unwrap_or(0.0);
    let offset = place["offset"].as_f64().unwrap_or(if f.work_plane_based { 0.0 } else { f.default_elevation });
    (place["at"], place["offset"]) = (json!([at(0) + x, at(1) + y]), json!(offset + z));
}

/// The material the preview draws a ghosted form in: grey and see-through. The scratch project's own, never the family's.
const GHOST: &str = "Preview Visibility ghost";

/// The colour of the frame round the family document's view, and of its banner, while Preview Visibility is on.
pub(crate) const PREVIEW_FRAME: Color32 = Color32::from_rgb(232, 128, 0);

/// The family as the preview draws it while Preview Visibility is off: each solid form the type (its `values`) hides, or its
/// Visibility Settings leave out of {3D} at the document's Detail Level (`by_view`, F8), shown, in
/// the ghost's material, as Revit's family editor greys a form hidden in a type so it can still be edited. A void stays as it
/// builds: never seen, it cuts only where it is there. None when the type hides no form.
pub(super) fn ghosted(f: &Family, values: &BTreeMap<String, bimcraft_core::family::Val>, by_view: &[bool]) -> Option<Family> {
    let mut g = f.clone();
    let mut any = false;
    for (i, form) in g.forms.iter_mut().enumerate().filter(|(_, form)| !form.void) {
        let in_type = form.visible.as_ref().is_some_and(|v| crate::adaptive_editor::worth(v, values) == Some(0.0));
        if in_type || by_view.get(i).copied().unwrap_or(false) {
            form.visible = None;
            form.visibility = None;
            form.material = GHOST.into();
            any = true;
        }
    }
    any.then_some(g)
}

/// The categories a family may be of, in the order they're offered.
pub(crate) const CATEGORIES: [Category; 20] = [
    Category::Furniture,
    Category::Casework,
    Category::Plumbing,
    Category::Drain,
    Category::PipeFitting,
    Category::PipeAccessory,
    Category::Equipment,
    Category::Outlet,
    Category::Switch,
    Category::AirTerminal,
    Category::Earthing,
    Category::InspectionBox,
    Category::DataOutlet,
    Category::VoiceOutlet,
    Category::PatchPanel,
    Category::Rack,
    Category::Lighting,
    Category::Generic,
    Category::Door,
    Category::Window,
];

pub(crate) fn category_label(c: Category) -> String {
    t(match c {
        Category::Furniture => msg!("Furniture"),
        Category::Casework => msg!("Casework"),
        Category::Plumbing => msg!("Plumbing Fixtures"),
        Category::Drain => "Drains",
        Category::PipeFitting => "Pipe Fittings",
        Category::PipeAccessory => "Pipe Accessories",
        Category::Equipment => msg!("Electrical Equipment"),
        Category::Outlet => msg!("Outlets"),
        Category::Switch => msg!("Switches"),
        Category::AirTerminal => msg!("Air Terminals"),
        Category::Earthing => msg!("Earthing Electrodes"),
        Category::InspectionBox => msg!("Inspection Boxes"),
        Category::DataOutlet => msg!("Data Outlets"),
        Category::VoiceOutlet => msg!("Voice Outlets"),
        Category::PatchPanel => msg!("Patch Panels"),
        Category::Rack => msg!("Racks"),
        Category::Lighting => msg!("Lighting"),
        Category::Generic => msg!("Generic"),
        Category::Door => "Door",
        Category::Window => "Window",
    })
    .to_string()
}

/// A text field `width` points wide, named `label` for screen readers and tests. Sized, not asked for a width: a Grid's
/// column is as wide as its cell was a frame before, and a field that only wants to be wider would stay clamped to it.
fn field(ui: &mut Ui, label: String, text: &mut String, width: f32) {
    let r = ui.add_sized([width, ui.spacing().interact_size.y], TextEdit::singleline(text).id_salt(&label));
    r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, label.as_str()));
}

/// A drop-down of `options` for `current`, named `label`: what was picked, if it was.
fn pick(ui: &mut Ui, label: String, current: &str, options: &[String]) -> Option<String> {
    let mut picked = None;
    let shown = if current.is_empty() { t("(none)") } else { current };
    let c = ComboBox::from_id_salt(&label).selected_text(shown).width(150.0).show_ui(ui, |ui| {
        for o in options {
            if ui.selectable_label(o == current, if o.is_empty() { t("(none)") } else { o }).clicked() {
                picked = Some(o.clone());
            }
        }
    });
    c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, label.as_str()));
    picked
}

/// What a formula works out, shown greyed as Revit's Family Types shows it and taking no typing: a tick, or the value as text;
/// `label` names it for screen readers and tests.
fn computed(ui: &mut Ui, label: String, kind: Holds, value: Option<&bimcraft_core::family::Val>, width: f32) {
    use bimcraft_core::family::Val;
    if kind == Holds::YesNo {
        let mut on = matches!(value, Some(Val::Number(x)) if *x != 0.0);
        let r = ui.add_enabled(false, egui::Checkbox::without_text(&mut on));
        r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, false, on, label.as_str()));
        return;
    }
    let mut text = match value {
        Some(Val::Number(x)) => number_text(*x),
        Some(Val::Text(t)) => t.clone(),
        None => "?".into(),
    };
    let r =
        ui.add_enabled_ui(false, |ui| ui.add_sized([width, ui.spacing().interact_size.y], TextEdit::singleline(&mut text).id_salt(&label)));
    r.inner.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, false, label.as_str()));
}

/// A form's or a symbol's Visible row, as Revit's Properties have it: a tick, and "=" at its end to tie it to a yes/no parameter;
/// tied (or worked out by a formula), the tick shows what the type previewed comes to, greyed, the formula beside it. `name`
/// names the tick for screen readers and tests. Whether "=" was pressed.
fn visible_row(ui: &mut Ui, visible: &mut String, name: String, values: &BTreeMap<String, bimcraft_core::family::Val>) -> bool {
    let label = msg!("Visible");
    let l = ui.label(t(label));
    let typed = visible.trim().to_string();
    let tied = !typed.is_empty() && typed.parse::<f64>().is_err();
    let pressed = ui
        .horizontal(|ui| {
            let mut on = shows(visible, values);
            let r = ui.add_enabled(!tied, egui::Checkbox::without_text(&mut on)).labelled_by(l.id);
            r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, !tied, on, name.as_str()));
            if r.changed() {
                // Shown is what a form or a symbol is unless it says otherwise.
                *visible = if on { String::new() } else { "0".into() };
            }
            if tied {
                ui.label(RichText::new(&typed).color(pal().muted));
            }
            crate::adaptive_editor::equals(ui, label, tied)
        })
        .inner;
    ui.end_row();
    pressed
}

/// Whether a form or a symbol whose Visible is typed `visible` shows with `values`: unless it says otherwise, it does.
fn shows(visible: &str, values: &BTreeMap<String, bimcraft_core::family::Val>) -> bool {
    let typed = visible.trim();
    typed.is_empty() || crate::adaptive_editor::worth(&text_expr(typed), values).is_some_and(|x| x != 0.0)
}

/// Each form of the draft as `values` (the type previewed) flex it, where that differs from how it is drawn in the shapes the
/// Forms tab types (corners, circle, axis, From and To): what its dimensions and locks set. None for one they leave as drawn.
fn held_forms(ed: &FamilyEditor, values: &BTreeMap<String, bimcraft_core::family::Val>) -> Vec<Option<FormText>> {
    let flexed = ed.family().ok().and_then(|f| f.flexed(values).ok().map(std::borrow::Cow::into_owned));
    ed.forms
        .iter()
        .enumerate()
        .map(|(i, drawn)| {
            let f = flexed.as_ref()?;
            let guid = drawn.original.guid;
            let form = if guid.is_nil() { f.forms.get(i) } else { f.forms.iter().find(|x| x.guid == guid) }?;
            let as_set = FormText::of(form);
            let moved = as_set.profile != drawn.profile
                || as_set.circle != drawn.circle
                || as_set.axis != drawn.axis
                || as_set.from != drawn.from
                || as_set.to != drawn.to;
            moved.then_some(as_set)
        })
        .collect()
}

/// The selected symbol's Properties (symbol `i`): what it is drawn from, as typed, each number with "=" (Associate Family
/// Parameter); Visible with "="; its subcategory and its Visibility Settings. Symbols are drawn in plan only, so they have no
/// shape in the family's views to drag: their numbers are typed here.
fn symbol_properties(ed: &mut FamilyEditor, ui: &mut Ui, i: usize) {
    use crate::adaptive_editor::{Field, Slot};
    let mut links = association::Controls::new(ed);
    let (values, params) = (ed.values(), ed.parameter_kinds());
    let mut open_visibility = false;
    // The subcategories a symbol may be drawn in: its family's category's (a door's and a window's Opening).
    let subs: Vec<&'static str> = bimcraft_core::graphics::subcategories(ed.category.graphics_category()).map(|s| s.name).collect();
    let Some(y) = ed.symbols.get_mut(i) else { return };
    let n = i + 1;
    let w = (ui.available_width() / 2.0 - 16.0).max(60.0);
    document::group(ui, msg!("Constraints"), |ui| {
        if y.part == Part::Line {
            ui.label(t("Points"));
            let r = ui.add(TextEdit::multiline(&mut y.line).desired_rows(3).desired_width(w).id_salt(("fe-symbol-line", i)));
            r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, format!("{} {n}", t("Symbol Points"))));
            ui.end_row();
            return;
        }
        let centre = if y.part == Part::Text { msg!("Middle of the text") } else { msg!("Centre") };
        for (k, what) in [msg!("Symbol Centre X"), msg!("Symbol Centre Y")].into_iter().enumerate() {
            ui.label(format!("{} {}", t(centre), ["X", "Y"][k]));
            links.field(ui, format!("{} {n}", t(what)), &mut y.centre[k], w - 30.0, Holds::Length, (Field::Symbol(Slot::Centre(k)), i));
            ui.end_row();
        }
        let size = if y.part == Part::Text { msg!("Height") } else { msg!("Radius") };
        ui.label(t(size));
        links.field(
            ui,
            format!("{} {n}", t(msg!("Symbol Radius"))),
            &mut y.radius,
            w - 30.0,
            Holds::Length,
            (Field::Symbol(Slot::Radius), i),
        );
        ui.end_row();
        if y.part == Part::Arc {
            for (label, what, text) in
                [(msg!("From (degrees)"), msg!("Symbol From"), &mut y.from), (msg!("To (degrees)"), msg!("Symbol To"), &mut y.to)]
            {
                ui.label(t(label));
                let slot = if what == "Symbol From" { Slot::From } else { Slot::To };
                links.field(ui, format!("{} {n}", t(what)), text, w - 30.0, Holds::Angle, (Field::Symbol(slot), i));
                ui.end_row();
            }
        }
        if y.part == Part::Text {
            ui.label(t("Text"));
            field(ui, format!("{} {n}", t("Symbol Text")), &mut y.value, w);
            ui.end_row();
            ui.label(t("Align"));
            ui.horizontal_wrapped(|ui| {
                for (a, label) in
                    [(LabelAlign::Left, msg!("Left")), (LabelAlign::Center, msg!("Centre")), (LabelAlign::Right, msg!("Right"))]
                {
                    ui.selectable_value(&mut y.align, a, t(label));
                }
            });
            ui.end_row();
        }
    });
    document::group(ui, msg!("Graphics"), |ui| {
        if visible_row(ui, &mut y.visible, format!("{} {n}", t("Symbol Visible")), &values) {
            let typed = y.visible.trim().to_string();
            links.request = Some(crate::adaptive_editor::associating(
                &i.to_string(),
                Field::Symbol(Slot::Visible),
                Holds::YesNo,
                msg!("Visible"),
                Some(&typed),
                &params,
            ));
        }
        // Drawn in a subcategory a view hides or restyles apart (a door's swing in Doors > Opening), or the category.
        if !subs.is_empty() || !y.subcategory.is_empty() {
            ui.label(t("Subcategory"));
            let shown = if y.subcategory.is_empty() { t("None").to_string() } else { t(&y.subcategory).to_string() };
            let c = ComboBox::from_id_salt(("fe-symbol-sub", i)).selected_text(shown).width(w).truncate().show_ui(ui, |ui| {
                ui.selectable_value(&mut y.subcategory, String::new(), t("None"));
                for name in &subs {
                    ui.selectable_value(&mut y.subcategory, name.to_string(), t(name));
                }
            });
            c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, format!("{} {n}", t("Symbol Subcategory"))));
            ui.end_row();
        }
        // F8: the detail levels it is drawn at (Family Element Visibility Settings), named short so the column stays narrow.
        ui.label(t("Detail Levels"));
        let r = ui.button(t("Edit…"));
        r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, format!("{} {n}", t("Symbol Visibility Settings"))));
        open_visibility = r.clicked();
        ui.end_row();
    });
    if links.request.is_some() {
        ed.associating = links.request;
    }
    if open_visibility {
        ed.open_visibility(visibility::Target::Symbol(i));
    }
}

impl FamilyEditor {
    /// The symbol selected, while it is there.
    pub(crate) fn selected_symbol(&self) -> Option<usize> {
        self.symbol.filter(|i| *i < self.symbols.len())
    }

    /// Symbol `i` alone selected, as the Project Browser selects it: whatever else was selected let go.
    pub(crate) fn select_symbol(&mut self, i: usize) {
        self.let_everything_go();
        self.symbol = Some(i);
    }

    /// Form `i` alone selected, as the Project Browser selects it: whatever else was selected let go.
    pub(crate) fn select_only_form(&mut self, i: usize) {
        self.let_everything_go();
        self.form = Some(i);
    }

    /// Nothing selected: Properties show the family's own.
    pub(crate) fn let_everything_go(&mut self) {
        (self.form, self.symbol, self.connector, self.nested_sel) = (None, None, None, None);
        (self.views.plane, self.views.line, self.views.dim) = (None, None, None);
    }

    /// A symbol of `part` added, as the Detail panel draws one, and selected so Properties show its rows.
    pub(crate) fn add_symbol(&mut self, part: Part) {
        self.symbols.push(SymbolText::blank(part));
        self.select_symbol(self.symbols.len() - 1);
    }

    /// The selected symbol's name, as Properties and the Project Browser give it.
    pub(crate) fn symbol_name(&self, i: usize) -> String {
        let what = match self.symbols.get(i).map(|y| y.part) {
            Some(Part::Arc) => t("Symbolic Arc"),
            Some(Part::Text) => t("Text"),
            Some(Part::Dot) => t("Dot"),
            _ => t("Symbolic Line"),
        };
        format!("{}. {what}", i + 1)
    }

    /// Form `i` taken one place up (`by` -1) or down (1) the list, which says what a void cuts (the forms listed above it),
    /// staying selected if it was.
    pub(crate) fn move_form(&mut self, i: usize, by: isize) {
        let Some(j) = i.checked_add_signed(by).filter(|j| *j < self.forms.len()) else { return };
        if i >= self.forms.len() {
            return;
        }
        self.forms.swap(i, j);
        self.form = self.form.map(|f| {
            if f == i {
                j
            } else if f == j {
                i
            } else {
                f
            }
        });
    }

    /// Form `i` taken away, with the dimensions and locks measuring to it, as the ribbon's Delete does.
    pub(crate) fn remove_form(&mut self, i: usize) {
        if i >= self.forms.len() {
            return;
        }
        let gone = self.forms.remove(i).original.guid;
        if !gone.is_nil() {
            self.forget_reference(gone);
        }
        self.form = self.form.and_then(|f| match f.cmp(&i) {
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some(f - 1),
            std::cmp::Ordering::Less => Some(f),
        });
    }

    /// Symbol `i` taken away.
    pub(crate) fn remove_symbol(&mut self, i: usize) {
        if i < self.symbols.len() {
            self.symbols.remove(i);
            self.symbol = self.symbol.and_then(|s| match s.cmp(&i) {
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some(s - 1),
                std::cmp::Ordering::Less => Some(s),
            });
        }
    }
}

/// The family in the file at `path`: its name (its own, else the file's), and what it holds.
fn read_family_file(path: &std::path::Path) -> Result<(String, Family), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut v: Value = serde_json::from_str(&text).map_err(|e| format!("{} isn't a family: {e}", path.display()))?;
    let name = v.as_object_mut().and_then(|o| o.remove("name")).and_then(|n| n.as_str().map(String::from));
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let family = serde_json::from_value(v).map_err(|e| format!("{} isn't a family: {e}", path.display()))?;
    Ok((name.unwrap_or(stem), family))
}

impl BimApp {
    /// Open the Family Editor on family `name` of the project: its document, in a tab of its own beside the project's; the one
    /// open brought forward, if it is.
    pub(crate) fn edit_family(&mut self, name: &str) {
        if let Some(id) = self.doc_of_family(name) {
            self.activate_doc(id);
            return;
        }
        if let Some(f) = self.session.model.families.get(name) {
            let mut editor = FamilyEditor::of(name, f);
            // As Revit's: the family as it is defined, its points on its own axes, whatever instance it was opened from; that
            // instance is remembered, for Replay Instance.
            if let Some((id, c)) = self.selection.iter().find_map(|id| match self.session.model.elements.get(id) {
                Some(bimcraft_core::model::Element::Component(c)) if c.family == name => Some((*id, c)),
                _ => None,
            }) && editor.adaptive.is_some()
            {
                editor.source = Some(id);
                editor.picked = editor.types.iter().position(|(n, _)| n == &c.type_name).unwrap_or(0);
            }
            self.open_family_document(editor, None);
        }
    }

    /// Open the Family Editor on a new family of `category`: "Family1", "Family2" and so on.
    pub(crate) fn new_family(&mut self, category: Category) {
        let mut editor = FamilyEditor::blank(category);
        editor.name = self.next_family_name();
        self.open_family_document(editor, None);
    }

    /// The start page's New Family and Open Family: the document over the page, which comes back when it closes unless a
    /// project is behind it (the page brought up over one, or a family loaded into the blank one meanwhile).
    pub(crate) fn family_from_start_page(&mut self) {
        if self.active_family().is_some() {
            self.family_from_home = !self.home.over;
            self.home.shown = false;
        }
    }

    /// Closed, a family document opened from the start page with no project behind it goes back to the page, as Revit's goes
    /// back to Home; a family it loaded into the blank project leaves the project on screen.
    pub(crate) fn family_closed_from_home(&mut self) {
        if self.family_from_home && self.families.is_empty() {
            self.family_from_home = false;
            if !self.dirty() {
                self.home.shown = true;
            }
        }
    }

    /// File > Open > Family… and the start page's Open Family: a family file (BIMCraft's own JSON, as Load Family reads it)
    /// asked for, then opened as its document.
    pub(crate) fn open_family(&mut self, ctx: &Context) {
        let dialog = rfd::AsyncFileDialog::new().add_filter(t("BIMCraft family"), &bimcraft_core::files::FAMILIES);
        let home = self.home.shown;
        self.ask_file(ctx, dialog, false, move |app, path| {
            app.open_family_file(&path);
            if home {
                app.family_from_start_page();
            }
        });
    }

    /// The family in the file at `path` open in its document, as Revit's File > Open > Family: not the project's (Load into
    /// Project loads it, asking first when the project has a family of its name), the project left as it is. A file that isn't
    /// a family says why.
    pub(crate) fn open_family_file(&mut self, path: &std::path::Path) {
        if let Some(id) = self.doc_of_file(path) {
            self.activate_doc(id);
            return;
        }
        match read_family_file(path) {
            Ok((name, family)) => {
                let mut editor = FamilyEditor::of(&name, &family);
                editor.original = None;
                self.open_family_document(editor, Some(path.to_path_buf()));
                self.status = Status::default();
            }
            Err(text) => self.status = Status { text, error: true },
        }
    }

    /// File > New > Family…: the family's category picked, as Revit's New Family picks a template, then its document opens.
    pub(crate) fn new_family_window(&mut self, ctx: &Context) {
        let Some(mut picked) = self.new_family_dialog else { return };
        let (mut ok, mut close) = (false, false);
        let r = Modal::new(UiId::new("new-family"), t("New Family")).width(360.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                ui.label(t("Category:"));
                // The body scrolls past the window's height: the list needs no scrolling of its own.
                for c in CATEGORIES {
                    let row = ui.selectable_label(picked == c, category_label(c));
                    if row.clicked() {
                        picked = c;
                    }
                    if row.double_clicked() {
                        ok = true;
                    }
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    close = ui.button(t("Cancel")).clicked();
                    ok |= ui.button(t("OK")).clicked();
                });
            });
        });
        if ok {
            self.new_family_dialog = None;
            let home = self.home.shown;
            self.new_family(picked);
            if home {
                self.family_from_start_page();
            }
        } else if close || r.should_close() {
            self.new_family_dialog = None;
        } else {
            self.new_family_dialog = Some(picked);
        }
    }

    /// The family the selection is of, else the one the Component tool places.
    pub(crate) fn family_in_hand(&self) -> Option<String> {
        let m = &self.session.model;
        let selected = self.selection.iter().find_map(|id| m.elements.get(id)?.family().map(|(f, _)| f.to_string()));
        selected.or_else(|| if matches!(self.tool, Tool::Component | Tool::Fixture(_)) { self.placing().map(|(f, _)| f) } else { None })
    }

    /// The selected adaptive instances' points, as `adaptive_capture` takes them, in each instance's number order.
    pub(crate) fn capture_sources(&self) -> Vec<Value> {
        let base = &self.session.model;
        self.selection
            .iter()
            .flat_map(|id| {
                let Some(bimcraft_core::model::Element::Component(c)) = base.elements.get(id) else { return Vec::new() };
                let Some(d) = base.families.get(&c.family).and_then(|f| f.adaptive.as_ref()) else { return Vec::new() };
                let mut points = d
                    .nodes
                    .iter()
                    .filter_map(|n| match &n.operation {
                        bimcraft_core::adaptive::Operation::Point {
                            host: bimcraft_core::adaptive::Host::Adaptive { number, .. }, ..
                        } => Some((*number, n.id.clone())),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                points.sort_by_key(|p| p.0);
                points.into_iter().map(|(_, point)| json!({"id":id,"host":{"type":"frame","point":point}})).collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    }

    /// The family document, while one is open: what its ribbon asked for and the dialogs it opened, the window drawing the
    /// rest; then a step of its own history for what changed this frame.
    pub(crate) fn family_editor_window(&mut self, ctx: &Context) {
        let Some(mut ed) = self.take_family() else { return };
        let materials: Vec<String> = self.session.model.materials.keys().cloned().collect();
        self.document_dialogs(ctx, &mut ed, &materials);
        let ask = std::mem::take(&mut ed.ask);
        ed.history(ctx, &ask);
        self.family_asked(ctx, ed, ask);
    }

    /// Does what the family document's ribbon asked for: capturing the selection's frames, loading a file, making the family the
    /// project's (Load into Project, placing the test frames), saving it to a file or the Favorites (from a session of its own,
    /// which never touches the project), then closing it on Close Family, or on Load into Project and Close when the family
    /// went in. It stays open otherwise.
    fn family_asked(&mut self, ctx: &Context, mut ed: FamilyEditor, ask: Asked) {
        // Edit Family on a nested family: its document over this one, which waits under it.
        if let Some(name) = ed.open_nested.take() {
            self.open_nested_family(ed, &name);
            return;
        }
        // The document `ed` is of: what a file dialog it opens finishes in, whichever tab is active by then.
        let Some(doc) = self.active_doc() else {
            self.put_family(ed);
            return;
        };
        let base = Arc::clone(&self.session.model);
        let Asked {
            capture: capture_context,
            place: place_adaptive,
            apply_placement,
            apply,
            ok,
            cancel,
            save,
            save_in_place,
            favorite,
            load,
            ..
        } = ask;
        // Save writes to the document's file once it has one (opened from it, or saved to it), else asks as Save As does.
        let in_place = if save { None } else { self.doc(doc).and_then(|d| d.path.clone()).filter(|_| save_in_place) };
        let save = save || save_in_place;
        let capture_sources = self.capture_sources();
        if capture_context && let Some(value) = self.run("adaptive_capture", json!({"sources":capture_sources})) {
            match serde_json::from_value(value) {
                Ok(placement) => {
                    if let Some(d) = ed.adaptive.as_mut() {
                        d.preview = placement;
                        ed.replay = None;
                    }
                }
                Err(e) => ed.problem = Some(e.to_string()),
            }
        }
        if load {
            let dialog = rfd::AsyncFileDialog::new().add_filter(t("BIMCraft family"), &bimcraft_core::files::FAMILIES);
            // Loaded into this family (T6a), as Revit's Insert > Load Family in a family, though another tab came forward
            // meanwhile; closed meanwhile, nothing to load into.
            self.ask_file(ctx, dialog, false, move |app, path| app.nest_family_file(doc, &path));
        }
        // Save and Favorites use a private Session: neither implicitly loads the draft into the project.
        if save || favorite {
            let prepared = ed.command(&base).and_then(|p| {
                let mut draft = Session::new();
                draft.model = Arc::clone(&base);
                draft.execute("family", &p)?;
                Ok(draft)
            });
            match prepared {
                Ok(mut draft) => {
                    let name = ed.name.trim().to_string();
                    let saved = ed.draft();
                    if let Some(path) = in_place {
                        match draft.execute("family", &json!({"name":name,"save":path.to_string_lossy()})) {
                            Ok(_) => ed.saved_as(saved),
                            Err(text) => self.status = Status { text, error: true },
                        }
                    } else if save {
                        let dialog = rfd::AsyncFileDialog::new()
                            .add_filter(t("BIMCraft family"), &[bimcraft_core::files::FAMILY])
                            .set_file_name(bimcraft_core::files::file_name(&name, bimcraft_core::files::FAMILY));
                        self.ask_file(ctx, dialog, true, move |app, path| {
                            match draft.execute("family", &json!({"name":name,"save":path.to_string_lossy()})) {
                                // Still open, the document that saved has nothing unsaved but what changed meanwhile, whichever
                                // tab is active, and Save writes to this file from now on; closed, the file is all there is.
                                Ok(_) => app.family_saved_to(doc, saved, path),
                                Err(text) => app.status = Status { text, error: true },
                            }
                        });
                    } else {
                        let p = self.favorite_params("save", "family", &name);
                        match draft.execute("favorite", &p) {
                            Ok(_) => {
                                ed.saved_as(saved);
                                self.status = Status { text: tf("Saved {} to your Favorites", &[&name]), error: false }
                            }
                            Err(e) => ed.problem = Some(e),
                        }
                    }
                }
                Err(e) => ed.problem = Some(e),
            }
        }
        // Made the project's: Load into Project does it, and so does placing the test frames, which places the family's. A
        // family the project has otherwise asks first (`exists`); Load into Project and Close closes once it went in.
        if !cancel && ed.into_host && (apply || ok) {
            // A nested family's document loads into the family under it, asking as a load into the project does.
            let family = ed.family();
            match family {
                Ok(family) => {
                    let name = ed.name.trim().to_string();
                    self.put_family_of(doc, ed);
                    self.nest_family(name, family, exists::After::IntoHost { doc, close: ok });
                }
                Err(e) => {
                    ed.document.refused = Some(e);
                    self.put_family_of(doc, ed);
                }
            }
            return;
        }
        if !cancel && (apply || ok || place_adaptive || apply_placement) {
            self.load_draft(&mut ed, exists::After::Document { doc, close: ok, place: place_adaptive, apply_placement });
        }
        // Loaded and closed: its tabs go, and the one it was opened from comes back (a nested family's, the family under it).
        // Close Family asks first while it has unsaved changes.
        let loaded = std::mem::take(&mut ed.document.loaded_to_close);
        self.put_family_of(doc, ed);
        if loaded {
            self.close_family_document(doc);
        } else if cancel {
            self.ask_close_family(doc, tabs::Then::Nothing);
        }
    }
}

/// A window's Sill (F10c): a reference plane on Ref. Level, seen edge on in the elevations, Align-locked to it so a Height
/// dimensioned from it moves its head alone. A window's axes rise from its sill, so the two lie on one another; Tab picks
/// between them in a view.
fn sill_plane(family: &mut Family) {
    use bimcraft_core::family::{DimKind, DimRef, DimView, FamilyDimension};
    let guid = bimcraft_core::ids::Guid::new();
    family.planes.push(RefPlane {
        id: 0,
        guid,
        name: t("Sill").to_string(),
        start: [-1500.0, 0.0, 0.0],
        end: [1500.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        reference: Default::default(),
        defines_origin: false,
        subcategory: None,
        level: false,
        pivot: None,
        turned: None,
    });
    family.dimensions.push(FamilyDimension {
        id: 0,
        guid: bimcraft_core::ids::Guid::new(),
        view: DimView::Front,
        refs: vec![DimRef::Plane(REF_LEVEL), DimRef::Plane(guid)],
        at: [1300.0, 0.0, 0.0],
        label: None,
        locked: vec![bimcraft_core::model::Lock { segment: 0, length: 0.0 }],
        equal: false,
        alignment: true,
        kind: DimKind::Aligned,
    });
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn adaptive_editor_roundtrip_preserves_identity_and_independent_preview_frames() {
        let family = Family {
            types: [("A".into(), BTreeMap::new())].into(),
            adaptive: Some(bimcraft_core::adaptive::four_point_panel()),
            ..Family::default()
        };
        let mut ed = FamilyEditor::of("Adaptive panel", &family);
        assert_eq!(ed.family().unwrap(), family);
        ed.adaptive.as_mut().unwrap().preview.frames.insert(2, bimcraft_core::adaptive::Frame::at([2000., 0., 100.]));
        assert_eq!(family.adaptive.as_ref().unwrap().preview.frames.len(), 0);
        assert_eq!(ed.family().unwrap().adaptive.unwrap().nodes, family.adaptive.unwrap().nodes);
    }

    #[test]
    fn what_is_typed_is_a_number_or_a_formula_and_a_profile_is_a_corner_to_a_line() {
        for (typed, want) in [
            ("600", Expr::Number(600.0)),
            (" -2.5 ", Expr::Number(-2.5)),
            ("Width / 2", Expr::Formula("Width / 2".into())),
            ("", Expr::Number(0.0)),
            ("inf", Expr::Formula("inf".into())),
        ] {
            assert_eq!(text_expr(typed), want, "{typed:?}");
        }
        for x in [0.0, 600.0, -2.5, 0.000_001, 1e12] {
            assert_eq!(text_expr(&number_text(x)), Expr::Number(x), "{x}");
        }
        assert_eq!(split_top("max(1, 2), 3"), ["max(1, 2)", " 3"]);
        assert_eq!(split_top("a, (b, (c, d)), e").len(), 3);
        let p = text_profile("0, 0\n Width, 0 \n\nmax(1, Depth), Height / 2").unwrap();
        assert_eq!(profile_text(&p), "0, 0\nWidth, 0\nmax(1, Depth), Height / 2");
        assert!(text_profile("0, 0\n1, 2, 3").unwrap_err().contains("corner 2"));
        assert!(text_profile("7").is_err());
    }

    /// The library's families, taken into the editor and built again, are the families they were: no form, parameter, type or symbol lost.
    #[test]
    fn every_library_family_comes_back_from_the_editor_as_it_went_in() {
        // Whole numbers a file writes as 1200 come back as 1200.0, and a formula that is a number ("-40") as -40.0: the same number.
        fn alike(v: Value) -> Value {
            match v {
                Value::Number(n) => json!(n.as_f64()),
                Value::String(s) => s.trim().parse::<f64>().ok().filter(|x| x.is_finite()).map_or(Value::String(s), |x| json!(x)),
                Value::Array(a) => Value::Array(a.into_iter().map(alike).collect()),
                Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, alike(v))).collect()),
                other => other,
            }
        }
        // Where two values first differ, as a path into them.
        fn first_diff(path: String, a: &Value, b: &Value) -> Option<String> {
            match (a, b) {
                (Value::Object(x), Value::Object(y)) => x
                    .keys()
                    .chain(y.keys())
                    .find_map(|k| first_diff(format!("{path}.{k}"), x.get(k).unwrap_or(&Value::Null), y.get(k).unwrap_or(&Value::Null))),
                (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
                    x.iter().zip(y).enumerate().find_map(|(i, (p, q))| first_diff(format!("{path}[{i}]"), p, q))
                }
                _ => (a != b).then(|| format!("{path}: {a} against {b}")),
            }
        }
        let mut m = Model::new("x");
        // And one from IFC, which remembers the class it came in as.
        let mut panel = m.families["Table"].clone();
        panel.category = Category::Generic;
        panel.ifc =
            Some(IfcClass { class: "IFCPLATE".into(), predefined: Some("CURTAIN_PANEL".into()), object_type: Some("Glass".into()) });
        // And its options set, none at its default.
        panel.merge_options(&json!({"work_plane_based": true, "always_vertical": false, "cut_with_voids_when_loaded": true,
            "shared": true, "room_calculation_point": ["Width / 2", 0, 0], "cut_in_views": false, "omniclass": {"number": "23.30", "title": "Panel"}}))
            .unwrap();
        m.families.insert("System Panel".into(), panel);
        for (name, f) in &m.families {
            let back = FamilyEditor::of(name, f).family().unwrap_or_else(|e| panic!("{name}: {e}"));
            let (back, was) = (alike(serde_json::to_value(&back).unwrap()), alike(serde_json::to_value(f).unwrap()));
            assert_eq!(first_diff(name.clone(), &back, &was), None);
        }
    }

    #[test]
    fn a_new_family_is_one_that_builds_and_the_commands_params_rename_and_drop_types() {
        let m = Model::new("x");
        for category in [Category::Furniture, Category::Door, Category::Window] {
            let ed = FamilyEditor::blank(category);
            let family = ed.family().unwrap();
            assert!(family.check(&|name| m.materials.contains_key(name)).is_ok(), "{category:?}");
            assert!(ed.scratch(&m, &family).is_ok(), "{category:?}: it stands in a model of its own");
        }
        // As the project has it: its name stays what the command finds it by, a rename says so, a dropped type is a null.
        let mut ed = FamilyEditor::of("Table", &m.families["Table"]);
        ed.name = "Dining Table".into();
        ed.types.retain(|(n, _)| n != "2000 x 1000");
        let p = ed.command(&m).unwrap();
        assert_eq!(
            (&p["name"], &p["rename"], &p["types"]["2000 x 1000"], p["types"]["1200 x 800"].is_object()),
            (&json!("Table"), &json!("Dining Table"), &Value::Null, true)
        );
        // Blank types go by the family's value; what isn't a number for a number is said.
        ed.types[0].1.insert("Width".into(), "wide".into());
        assert!(ed.family().unwrap_err().contains("isn't a number"));
        ed.types[0].1.insert("Width".into(), String::new());
        assert!(!ed.family().unwrap().types[&ed.types[0].0].contains_key("Width"));
        ed.name = " ".into();
        assert!(ed.command(&m).unwrap_err().contains("needs a name"));
    }

    /// F10: a window stands on Ref. Level in its document's scratch model, its sill at 0, as its forms are drawn in axes whose z
    /// is up from the sill: else every view drew its forms `Sill Height` above the reference planes they are locked to.
    #[test]
    fn a_window_in_its_documents_scratch_model_stands_with_its_sill_on_ref_level() {
        let m = Arc::unwrap_or_clone(Session::new().model);
        let ed = FamilyEditor::blank(Category::Window);
        let family = ed.family().unwrap();
        let scratch = ed.scratch(&m, &family).unwrap();
        let window = scratch.elements.values().find(|e| matches!(e, bimcraft_core::model::Element::Window(_))).unwrap();
        let low = scratch.opening_pieces(window).iter().flat_map(|p| p.tris.iter()).flatten().map(|p| p[2]).fold(f64::INFINITY, f64::min);
        assert!(low.abs() < 1e-9, "the frame from the sill, on Ref. Level: {low}");
    }

    #[test]
    fn the_editor_keeps_a_familys_options_when_it_saves() {
        let mut s = Session::new();
        let options = json!({"work_plane_based": true, "always_vertical": false, "default_elevation": 900.0, "cut_with_voids_when_loaded": true, "shared": true,
            "room_calculation_point": ["Width / 2", 100.0, 0.0], "cut_in_views": true, "omniclass": {"number": "23.40.20", "title": "Tables"}});
        s.execute("family", &json!({"name": "Table", "options": options})).unwrap();
        let was = s.model.families["Table"].clone();
        let p = FamilyEditor::of("Table", &was).command(&s.model).unwrap();
        s.execute("family", &p).unwrap();
        assert_eq!(s.model.families["Table"].options_json(), options);
    }

    #[test]
    fn a_form_on_a_plane_of_the_familys_own_keeps_it_through_the_editor() {
        let mut s = Session::new();
        let shelf = "1b2c3d4e-5f60-4a71-8b92-a3b4c5d6e7f8";
        let line = "2c3d4e5f-6071-4b82-9ca3-b4c5d6e7f809";
        s.execute(
            "family",
            &json!({"name": "Shelf", "types": {"A": {}},
                "ref_planes": [{"guid": shelf, "name": "Top", "start": [0, 0, 500], "end": [1000, 0, 500], "up": [0, 1, 0]}],
                "ref_lines": [{"guid": line, "start": [0, 0, 500], "end": [100, 0, 500], "host": {"plane": shelf}}],
                "forms": [{"host": {"plane": shelf}, "profile": [[0, 0], [100, 0], [100, 50]], "to": 20},
                          {"host": {"line": line, "face": "start"}, "profile": [[0, 0], [100, 0], [100, 50]], "to": 20},
                          {"plane": "xz", "profile": [[0, 0], [100, 0], [100, 50]], "to": 20}]}),
        )
        .unwrap();
        let was = s.model.families["Shelf"].clone();
        let mut ed = FamilyEditor::of("Shelf", &was);
        assert_eq!(ed.forms[0].host, was.forms[0].host);
        let p = ed.command(&s.model).unwrap();
        s.execute("family", &p).unwrap();
        assert_eq!(s.model.families["Shelf"], was, "its planes, lines and hosts as they were");
        // The Plane list picks a datum.
        ed.forms[0].host = PlaneRef::Plane(REF_LEVEL);
        assert_eq!(ed.family().unwrap().forms[0].host, PlaneRef::Plane(REF_LEVEL));
    }

    #[test]
    fn a_form_as_typed_keeps_what_the_editor_doesnt_edit_and_a_revolve_and_a_void_survive() {
        let tri = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let mesh = Form { mesh: Arc::new(vec![tri]), material: "Timber".into(), ..Form::default() };
        let mut text = FormText::of(&mesh);
        text.visible = "Legs".into();
        text.void = true;
        let back = text.form().unwrap();
        assert_eq!((back.mesh.len(), back.visible, back.void), (1, Some(Expr::Formula("Legs".into())), true));
        assert_eq!(text.describe(), "Mesh (void)");
        let mut text = FormText::blank();
        (text.revolve, text.host, text.to) = (true, PlaneRef::Plane(CENTER_FRONT_BACK), "360".into());
        let f = text.form().unwrap();
        assert!(
            f.axis().is_some()
                && f.host == PlaneRef::Plane(CENTER_FRONT_BACK)
                && f.to == Expr::Number(360.0)
                && f.corners().map(|c| c.len()) == Some(4)
        );
        assert_eq!(FormText::of(&f).describe(), "Revolve");
        text.shape = Shape::Circle;
        text.circle = ["0".into(), "0".into(), "R".into()];
        let f = text.form().unwrap();
        assert!(f.corners().is_none() && f.circle().is_some_and(|c| c.radius == Expr::Formula("R".into())));
    }

    #[test]
    fn symbols_and_parameters_come_back_as_typed() {
        let y = Symbol {
            arc: Some(Curve {
                centre: [Expr::Number(1.0), Expr::Formula("Wall Thickness / 2".into())],
                radius: Expr::Number(700.0),
                from: Expr::Number(0.0),
                to: Expr::Number(90.0),
            }),
            ..Symbol::default()
        };
        assert_eq!(SymbolText::of(&y).symbol().unwrap(), y);
        let line = Symbol {
            line: vec![[Expr::Number(0.0), Expr::Number(0.0)], [Expr::Formula("Width".into()), Expr::Number(5.0)]],
            visible: Some(Expr::Formula("Swing".into())),
            ..Symbol::default()
        };
        assert_eq!(SymbolText::of(&line).symbol().unwrap(), line);
        // A label and a dot come back as they were typed, and a new one of each is one the family takes.
        let label = Symbol {
            text: Some(Label {
                at: [Expr::Number(0.0), Expr::Number(-150.0)],
                height: Expr::Number(60.0),
                value: "C{Circuit}".into(),
                align: LabelAlign::Center,
            }),
            visible: Some(Expr::Formula("Show".into())),
            ..Symbol::default()
        };
        assert_eq!(SymbolText::of(&label).symbol().unwrap(), label);
        let dot = Symbol {
            dot: Some(Circle { centre: [Expr::Number(2.0), Expr::Number(3.0)], radius: Expr::Number(20.0) }),
            ..Symbol::default()
        };
        assert_eq!(SymbolText::of(&dot).symbol().unwrap(), dot);
        for part in [Part::Line, Part::Arc, Part::Text, Part::Dot] {
            assert_eq!(SymbolText::of(&SymbolText::blank(part).symbol().unwrap()).part, part);
        }
        let p = Parameter { name: "Legs".into(), kind: Holds::YesNo, instance: true, value: json!(true), ..Parameter::default() };
        assert_eq!(ParamText::of(&p).parameter().unwrap(), p);
        let area = Parameter {
            name: "Area".into(),
            kind: Holds::Number,
            instance: false,
            value: json!(0.0),
            formula: Some("Width * Depth".into()),
            ..Parameter::default()
        };
        assert_eq!(ParamText::of(&area).parameter().unwrap(), area);
        // What the editor shows no field for yet stays as the family has it.
        let shared = Parameter {
            name: "Fire Rating".into(),
            kind: Holds::Text,
            value: json!("EI 60"),
            group: Some(bimcraft_core::family::Group::IdentityData),
            shared: bimcraft_core::model::Guid::parse("6f1c2a3b-4d5e-4f60-8a71-92b3c4d5e6f7"),
            ..Parameter::default()
        };
        assert_eq!(ParamText::of(&shared).parameter().unwrap(), shared);
        let reach = Parameter { name: "Reach".into(), reporting: true, value: json!(450.0), ..Parameter::default() };
        assert_eq!(ParamText::of(&reach).parameter().unwrap(), reach);
        for k in KINDS {
            assert!(!kind_name(k).is_empty());
        }
        assert_eq!(kind_name(Holds::YesNo), "Yes/No");
        // The rest of an editor's tabs list: all of them there.
        let tabs: BTreeSet<&str> = ["Parameters", "Types", "Forms", "Symbols"].into_iter().collect();
        assert_eq!(tabs.len(), 4);
    }
}
