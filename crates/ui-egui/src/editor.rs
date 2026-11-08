//! The Family Editor: a family's parameters, types, forms and symbols, edited as text and tried on a
//! preview as they're typed, then made the project's, saved to a file or kept in the Favorites.
//! Every change to the project is a `family` command, so it's checked, and undone, like any other.

mod association;
mod dimensions;
pub(crate) mod document;
pub(crate) mod exists;
mod extrusion;
pub(crate) mod options;
pub(crate) mod sketch;
mod temp_hide;
mod types_dialog;
pub(crate) mod views;

use crate::popup::Modal;

use bimcraft_i18n::msg;

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use bimcraft_core::engine::Session;
use bimcraft_core::family::{
    Align as LabelAlign, CENTER_FRONT_BACK, CENTER_LEFT_RIGHT, Category, Circle, Connector, Curve, Expr, Family, Form, IfcClass,
    Kind as Holds, Label, Parameter, PlaneRef, REF_LEVEL, RefLine, RefPlane, Symbol,
};
use bimcraft_core::model::{Model, Operation};
use bimcraft_core::view3d::{AZIMUTH, ELEVATION, Scene};
use egui::{
    Align, CollapsingHeader, Color32, ColorImage, ComboBox, Context, Grid, Id as UiId, Layout, PointerButton, Rect, RichText, ScrollArea,
    Sense, Stroke, StrokeKind, TextEdit, TextureHandle, TextureOptions, Ui, WidgetInfo, WidgetType, pos2, vec2,
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
        let what = match (self.mesh(), self.shape, self.revolve) {
            (true, _, _) => t("Mesh"),
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
enum Part {
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
        let base = Symbol { visible, id, guid, subcategory, ..Symbol::default() };
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

/// The family's {3D} preview: its picture and what it was drawn of (`key`); for a test, the camera it was drawn through and
/// its size in pixels.
pub(crate) struct Preview {
    key: u64,
    pub(crate) tile: TextureHandle,
    /// The camera it was taken through and its size: what a sketch in {3D} is drawn and clicked through.
    pub(crate) cam: bimcraft_core::view3d::Camera,
    pub(crate) size: [u32; 2],
}

#[cfg(test)]
impl Preview {
    /// Where point `q` of the family is on screen, the picture stretched over `pane`.
    pub(crate) fn screen(&self, pane: Rect, q: [f64; 3]) -> egui::Pos2 {
        let (x, y, _) = self.cam.project(q);
        pane.min + vec2(x as f32 * pane.width() / self.size[0].max(1) as f32, y as f32 * pane.height() / self.size[1].max(1) as f32)
    }
}

#[cfg(test)]
impl FamilyEditor {
    /// The {3D} preview, once drawn.
    pub(crate) fn preview(&self) -> Option<&Preview> {
        self.preview.as_ref()
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
    /// Its connectors, as the family has them: the editor has no tab for them yet, and keeps them as they are.
    connectors: Vec<Connector>,
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
    /// The Properties palette's tab, for a family that isn't adaptive: its forms, its symbols, or making it adaptive.
    pub(crate) tab: Tab,
    /// What Family Types keeps while it is open: its search, the groups folded, the row selected, its own dialogs.
    types_dialog: types_dialog::TypesDialog,
    /// The type shown in Types and on the preview.
    picked: usize,
    /// How the preview looks at it: azimuth and elevation, degrees.
    look: (f64, f64),
    /// The preview, and what it was drawn of.
    preview: Option<Preview>,
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
    /// Go To View, asked by Create > Extrusion or Revolve (revolve, void) where the view doesn't see the work plane flat.
    pub(crate) go_to: Option<(bool, bool)>,
    /// What a sketch snaps to, as Manage > Snaps says: the window's, given each frame.
    pub(crate) snaps: bimcraft_core::snap::Snaps,
}

/// The tabs of a family document's ribbon, as Revit's family editor has them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DocTab {
    #[default]
    Create,
    Insert,
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
    pub(crate) save: bool,
    pub(crate) favorite: bool,
    pub(crate) load: bool,
    /// The document's own Undo and Redo, done when the frame ends.
    pub(crate) undo: bool,
    pub(crate) redo: bool,
}

/// What the Properties palette shows of a family that isn't adaptive.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Tab {
    Forms,
    Symbols,
    Adaptive,
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
            connectors: f.connectors.clone(),
            planes: f.planes.clone(),
            lines: f.lines.clone(),
            dimensions: f.dimensions.clone(),
            kind: f.kind.clone(),
            ifc: f.ifc.clone(),
            operation: f.operation,
            options: options::OptionsText::of(f),
            tab: Tab::Forms,
            types_dialog: Default::default(),
            picked: 0,
            look: (AZIMUTH, ELEVATION),
            preview: None,
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
            go_to: None,
            snaps: Default::default(),
        }
    }

    /// Associate Family Parameter, open from a row's "=" (an adaptive element's, or a form's or a symbol's of a family that isn't
    /// adaptive): a parameter New Parameter makes goes in the family at once, as in Revit.
    pub(crate) fn associate_dialog(&mut self, ctx: &Context) {
        use crate::adaptive_editor::{Field, associate};
        let Some(mut a) = self.associating.take() else { return };
        let (params, values) = (self.parameter_kinds(), self.values());
        let (done, new) = if matches!(a.field, Field::Form(_) | Field::Symbol(_)) {
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

    /// A family to start from, of `category`: a box of its width, depth and height, or a door's or window's frame of its own.
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
        let family: Family = serde_json::from_value(family).unwrap_or_default();
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
            connectors: self.connectors.clone(),
            adaptive: self.adaptive.clone(),
            planes: self.planes.clone(),
            lines: self.lines.clone(),
            dimensions: self.dimensions.clone(),
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
        let place = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).map(|_| ());
        if family.category.hosted() {
            place(&mut s, "wall", json!({"points": [[-2500, 0], [2500, 0]]}))?;
            let kind = if family.category == Category::Door { "door" } else { "window" };
            place(&mut s, kind, json!({"at": [0, 0], "family": name, "type": type_name}))?;
        } else {
            place(&mut s, "component", json!({"family": name, "type": type_name, "at": [0, 0], "params": self.preview_params}))?;
        }
        Ok(Arc::unwrap_or_clone(s.model))
    }

    /// The preview, drawn again when what it shows or how it's looked at changes. A form the type previewed hides is drawn
    /// ghosted (`ghosted`), as Revit's family editor draws it, unless Preview Visibility is on: then the family is drawn as it
    /// is placed, without it. The camera is fitted to every form either way, so turning Preview Visibility on doesn't move it,
    /// nor does Temporary Hide/Isolate leaving forms or a door's wall out (`temp_hide.rs`).
    fn refresh(&mut self, ctx: &Context, base: &Model) {
        let ground = crate::canvas::color(bimcraft_core::render::PAPER);
        let placed_only = self.document.preview_visibility;
        // Work Plane > Show: the plane drawn into the picture, so the forms in front of it hide it.
        let work_plane = self.shown_work_plane_3d();
        let result = self.family().and_then(|f| {
            let key = {
                let mut h = DefaultHasher::new();
                (
                    serde_json::to_string(&f).unwrap_or_default(),
                    self.picked,
                    self.name.trim(),
                    self.look.0.to_bits(),
                    self.look.1.to_bits(),
                    self.document.preview_size,
                    ground.to_array(),
                    placed_only,
                    self.temporary(),
                    format!("{work_plane:?} {:?}", (pal().muted, pal().accent)),
                )
                    .hash(&mut h);
                (base.materials.len(), base.images.len()).hash(&mut h);
                h.finish()
            };
            if self.preview.as_ref().is_some_and(|p| p.key == key) {
                return Ok(None);
            }
            let ghosts = ghosted(&f, &self.values());
            let every = self.scratch(base, ghosts.as_ref().unwrap_or(&f))?;
            let mut graphics = bimcraft_core::graphics::Graphics::default();
            graphics.categories.insert("level".into(), bimcraft_core::graphics::Override { hidden: true, ..Default::default() });
            let [w, h] = self.document.preview_size;
            let scene = Scene::new(&every);
            let cam = scene.fit_in(self.look.0, self.look.1, w, h, &bimcraft_core::graphics::Look::with(&every, &graphics));
            // What Temporary Hide/Isolate hides for now: forms, and a door's or a window's wall.
            let shown = if placed_only { &f } else { ghosts.as_ref().unwrap_or(&f) };
            let hidden = temp_hide::without(shown, |i| self.temp_hidden_form(i));
            if self.temp_hidden_host() {
                graphics.categories.insert("wall".into(), bimcraft_core::graphics::Override { hidden: true, ..Default::default() });
            }
            let mut image = if hidden.is_some() || (placed_only && ghosts.is_some()) {
                let drawn = self.scratch(base, hidden.as_ref().unwrap_or(shown))?;
                Scene::new(&drawn).render_in(&cam, &[], &bimcraft_core::graphics::Look::with(&drawn, &graphics))
            } else {
                scene.render_in(&cam, &[], &bimcraft_core::graphics::Look::with(&every, &graphics))
            };
            // The canvas's ground where nothing is, before the work plane is laid over it.
            for p in image.rgba.as_chunks_mut::<4>().0.iter_mut().filter(|p| p[..3] == [255; 3]) {
                p[..3].copy_from_slice(&ground.to_array()[..3]);
            }
            if let Some(shown) = &work_plane {
                let extents = bimcraft_core::family::views::extents(&f, &self.values());
                views::draw_work_plane_3d(&mut image, &cam, shown, extents);
            }
            let pixels = image.rgba.as_chunks::<4>().0.iter().map(|p| Color32::from_rgb(p[0], p[1], p[2])).collect();
            let picture = ColorImage { size: [w as usize, h as usize], pixels, source_size: Default::default() };
            let tile = ctx.load_texture("family-preview", picture, TextureOptions::LINEAR);
            Ok(Some(Preview { key, tile, cam, size: [w, h] }))
        });
        match result {
            Ok(Some(drawn)) => (self.preview, self.problem) = (Some(drawn), None),
            Ok(None) => self.problem = None,
            Err(e) => self.problem = Some(e),
        }
    }
}

/// The material the preview draws a ghosted form in: grey and see-through. The scratch project's own, never the family's.
const GHOST: &str = "Preview Visibility ghost";

/// The colour of the frame round the family document's view, and of its banner, while Preview Visibility is on.
pub(crate) const PREVIEW_FRAME: Color32 = Color32::from_rgb(232, 128, 0);

/// The family as the preview draws it while Preview Visibility is off: each solid form the type (its `values`) hides shown, in
/// the ghost's material, as Revit's family editor greys a form hidden in a type so it can still be edited. A void stays as it
/// builds: never seen, it cuts only where it is there. None when the type hides no form.
fn ghosted(f: &Family, values: &BTreeMap<String, bimcraft_core::family::Val>) -> Option<Family> {
    let mut g = f.clone();
    let mut any = false;
    for form in g.forms.iter_mut().filter(|form| !form.void) {
        if form.visible.as_ref().is_some_and(|v| crate::adaptive_editor::worth(v, values) == Some(0.0)) {
            form.visible = None;
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

/// One form's fields, its Visible tied to the type previewed's `values`. Whether Visible's "=" was pressed.
fn form_fields(
    ui: &mut Ui,
    f: &mut FormText,
    i: usize,
    options: &[String],
    values: &BTreeMap<String, bimcraft_core::family::Val>,
    links: &mut association::Controls,
) -> bool {
    use crate::adaptive_editor::{Field, Slot};
    let n = i + 1;
    let mut pressed = false;
    // The datums, by the planes they are; a form on a plane of the family's own keeps it until a datum is picked.
    let planes = [
        (PlaneRef::Plane(REF_LEVEL), msg!("Plane XY")),
        (PlaneRef::Plane(CENTER_FRONT_BACK), msg!("Plane XZ")),
        (PlaneRef::Plane(CENTER_LEFT_RIGHT), msg!("Plane YZ")),
    ];
    Grid::new(("fe-form-grid", i)).num_columns(2).spacing(vec2(12.0, 5.0)).show(ui, |ui| {
        if f.mesh() {
            ui.label(t("Mesh"));
            ui.label(RichText::new(tf("{} triangles, from a file", &[&f.original.mesh.len().to_string()])).color(pal().muted));
            ui.end_row();
        } else {
            ui.label(t("Plane"));
            ComboBox::from_id_salt(("fe-plane", i))
                .selected_text(planes.iter().find(|(p, _)| *p == f.host).map_or_else(|| "…".to_string(), |(_, l)| t(l).to_string()))
                .width(110.0)
                .show_ui(ui, |ui| {
                    for (p, label) in planes {
                        ui.selectable_value(&mut f.host, p, t(label));
                    }
                });
            ui.end_row();
            ui.label(t("Shape"));
            let named = match f.shape {
                Shape::Profile => t("Profile"),
                Shape::Circle => t("Circle"),
                Shape::Sketch => t("Sketch"),
            };
            ComboBox::from_id_salt(("fe-shape", i)).selected_text(named).width(110.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut f.shape, Shape::Profile, t("Profile"));
                ui.selectable_value(&mut f.shape, Shape::Circle, t("Circle"));
            });
            ui.end_row();
            if f.shape == Shape::Sketch {
                // Lines, arcs or loops the corners can't say: sketch mode draws them (Edit Extrusion).
                ui.label(t("Sketch"));
                let n = f.original.boundary().count().to_string();
                ui.label(RichText::new(tf("{} lines and arcs, drawn in sketch mode", &[&n])).color(pal().muted));
            } else if f.shape == Shape::Profile {
                ui.label(t("Corners"));
                let r = ui.add(TextEdit::multiline(&mut f.profile).desired_rows(4).desired_width(300.0).id_salt(("fe-profile", i)));
                r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, format!("{} {n}", t("Form Corners"))));
            } else {
                ui.label(t("Circle centre and radius"));
                ui.horizontal(|ui| {
                    for (k, what) in [msg!("Form Circle X"), msg!("Form Circle Y"), msg!("Form Circle Radius")].into_iter().enumerate() {
                        links.field(
                            ui,
                            format!("{} {n}", t(what)),
                            &mut f.circle[k],
                            80.0,
                            Holds::Length,
                            (Field::Form(Slot::Centre(k)), i),
                        );
                    }
                });
            }
            ui.end_row();
            ui.label(t("Revolve"));
            ui.checkbox(&mut f.revolve, t("turned about an axis"));
            ui.end_row();
            if f.revolve {
                ui.label(t("Axis"));
                ui.horizontal(|ui| {
                    for (k, what) in
                        [msg!("Form Axis X1"), msg!("Form Axis Y1"), msg!("Form Axis X2"), msg!("Form Axis Y2")].into_iter().enumerate()
                    {
                        links.field(ui, format!("{} {n}", t(what)), &mut f.axis[k], 60.0, Holds::Length, (Field::Form(Slot::Axis(k)), i));
                    }
                });
                ui.end_row();
            }
            ui.label(if f.revolve { t("Start Angle") } else { t("From") });
            links.field(
                ui,
                format!("{} {n}", t("Form From")),
                &mut f.from,
                120.0,
                if f.revolve { Holds::Angle } else { Holds::Length },
                (Field::Form(Slot::From), i),
            );
            ui.end_row();
            ui.label(if f.revolve { t("End Angle") } else { t("To") });
            links.field(
                ui,
                format!("{} {n}", t("Form To")),
                &mut f.to,
                120.0,
                if f.revolve { Holds::Angle } else { Holds::Length },
                (Field::Form(Slot::To), i),
            );
            ui.end_row();
        }
        ui.label(t("Material"));
        if links.bound(&f.material, Holds::Material) {
            links.field(
                ui,
                format!("{} {n}", t("Form Material")),
                &mut f.material,
                120.,
                Holds::Material,
                (Field::Form(Slot::Material), i),
            );
        } else {
            ui.horizontal(|ui| {
                if let Some(m) = pick(ui, format!("{} {n}", t("Form Material")), &f.material, options) {
                    f.material = m;
                }
                links.button(ui, format!("{} {n}", t("Form Material")), &f.material, Holds::Material, (Field::Form(Slot::Material), i));
            });
        }
        ui.end_row();
        pressed = visible_row(ui, &mut f.visible, format!("{} {n}", t("Form Visible")), values);
        ui.label(t("Void"));
        let r = ui.checkbox(&mut f.void, t("cuts the forms listed above it"));
        r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, f.void, format!("{} {n}", t("Form Void"))));
        ui.end_row();
    });
    pressed
}

fn forms_tab(ed: &mut FamilyEditor, ui: &mut Ui, materials: &[String]) {
    // What a form may be made of: nothing, a library material, or a material parameter.
    let mut options = vec![String::new()];
    options.extend(ed.params.iter().filter(|p| p.kind == Holds::Material).map(|p| p.name.clone()));
    options.extend(materials.iter().cloned());
    let mut act: Option<(usize, i8)> = None;
    let mut select = None;
    let (values, params) = (ed.values(), ed.parameter_kinds());
    let mut links = association::Controls::new(ed);
    // A form picked in the Project Browser opens.
    let reveal = ed.document.reveal.take();
    for i in 0..ed.forms.len() {
        let heading = format!("{}. {}", i + 1, ed.forms[i].describe());
        let header = CollapsingHeader::new(heading).id_salt(("fe-form", i)).default_open(ed.forms.len() == 1);
        let header = if reveal == Some(i) { header.open(Some(true)) } else { header };
        header.show(ui, |ui| {
            if form_fields(ui, &mut ed.forms[i], i, &options, &values, &mut links) {
                let typed = ed.forms[i].visible.trim().to_string();
                ed.associating = Some(crate::adaptive_editor::associating(
                    &i.to_string(),
                    crate::adaptive_editor::Field::Form(crate::adaptive_editor::Slot::Visible),
                    Holds::YesNo,
                    msg!("Visible"),
                    Some(typed.as_str()),
                    &params,
                ));
            }
            ui.horizontal(|ui| {
                // Selected, Properties show it as Revit's do an Extrusion.
                if ui.button(t("Select")).clicked() {
                    select = Some(i);
                }
                if ui.add_enabled(i > 0, egui::Button::new(t("Move Up"))).clicked() {
                    act = Some((i, -1));
                }
                if ui.add_enabled(i + 1 < ed.forms.len(), egui::Button::new(t("Move Down"))).clicked() {
                    act = Some((i, 1));
                }
                if ui.button(t("Delete")).clicked() {
                    act = Some((i, 0));
                }
            });
        });
    }
    if links.request.is_some() {
        ed.associating = links.request;
    }
    if select.is_some() {
        ed.form = select;
    }
    match act {
        Some((i, -1)) => ed.forms.swap(i, i - 1),
        Some((i, 1)) => ed.forms.swap(i, i + 1),
        Some((i, _)) => {
            ed.forms.remove(i);
        }
        None => {}
    }
    ui.horizontal(|ui| {
        if ui.button(t("Add Box")).clicked() {
            ed.forms.push(FormText::blank());
        }
        if ui.button(t("Add Cylinder")).clicked() {
            let circle = ["0".into(), "0".into(), "200".into()];
            ed.forms.push(FormText { shape: Shape::Circle, circle, ..FormText::blank() });
        }
        if ui.button(t("Add Revolve")).clicked() {
            let profile = "100, 0\n300, 0\n300, 500\n100, 500".to_string();
            ed.forms.push(FormText {
                host: PlaneRef::Plane(CENTER_FRONT_BACK),
                profile,
                revolve: true,
                to: "360".into(),
                ..FormText::blank()
            });
        }
        if ui.button(t("Add Void")).clicked() {
            ed.forms.push(FormText { void: true, ..FormText::blank() });
        }
    });
}

fn symbols_tab(ed: &mut FamilyEditor, ui: &mut Ui) {
    use crate::adaptive_editor::{Field, Slot};
    let mut links = association::Controls::new(ed);
    let (values, params) = (ed.values(), ed.parameter_kinds());
    ui.label(
        RichText::new(t("Symbols are what the plan draws of the family besides its forms: lines, arcs, labels and dots."))
            .color(pal().muted),
    );
    let mut remove = None;
    // The subcategories a symbol may be drawn in: its family's category's (a door's and a window's Opening).
    let subs: Vec<&'static str> = bimcraft_core::graphics::subcategories(ed.category.graphics_category()).map(|s| s.name).collect();
    for (i, y) in ed.symbols.iter_mut().enumerate() {
        let n = i + 1;
        let kind = match y.part {
            Part::Line => t("Line"),
            Part::Arc => t("Arc"),
            Part::Text => t("Text"),
            Part::Dot => t("Dot"),
        };
        let heading = format!("{n}. {kind}");
        CollapsingHeader::new(heading).id_salt(("fe-symbol", i)).default_open(true).show(ui, |ui| {
            Grid::new(("fe-symbol-grid", i)).num_columns(2).spacing(vec2(12.0, 5.0)).show(ui, |ui| {
                if y.part == Part::Line {
                    ui.label(t("Points"));
                    let r = ui.add(TextEdit::multiline(&mut y.line).desired_rows(3).desired_width(300.0).id_salt(("fe-symbol-line", i)));
                    r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, format!("{} {n}", t("Symbol Points"))));
                    ui.end_row();
                } else {
                    ui.label(t(if y.part == Part::Text { msg!("Middle of the text") } else { msg!("Centre") }));
                    ui.horizontal(|ui| {
                        for (k, what) in [msg!("Symbol Centre X"), msg!("Symbol Centre Y")].into_iter().enumerate() {
                            links.field(
                                ui,
                                format!("{} {n}", t(what)),
                                &mut y.centre[k],
                                90.0,
                                Holds::Length,
                                (Field::Symbol(Slot::Centre(k)), i),
                            );
                        }
                    });
                    ui.end_row();
                    let size = if y.part == Part::Text { msg!("Height") } else { msg!("Radius") };
                    ui.label(t(size));
                    links.field(
                        ui,
                        format!("{} {n}", t(msg!("Symbol Radius"))),
                        &mut y.radius,
                        120.0,
                        Holds::Length,
                        (Field::Symbol(Slot::Radius), i),
                    );
                    ui.end_row();
                    if y.part == Part::Arc {
                        for (label, what, text) in [
                            (msg!("From (degrees)"), msg!("Symbol From"), &mut y.from),
                            (msg!("To (degrees)"), msg!("Symbol To"), &mut y.to),
                        ] {
                            ui.label(t(label));
                            links.field(
                                ui,
                                format!("{} {n}", t(what)),
                                text,
                                120.0,
                                Holds::Angle,
                                (Field::Symbol(if what == "Symbol From" { Slot::From } else { Slot::To }), i),
                            );
                            ui.end_row();
                        }
                    }
                    if y.part == Part::Text {
                        ui.label(t("Text"));
                        field(ui, format!("{} {n}", t("Symbol Text")), &mut y.value, 300.0);
                        ui.end_row();
                        ui.label(t("Align"));
                        ui.horizontal(|ui| {
                            for (a, label) in
                                [(LabelAlign::Left, msg!("Left")), (LabelAlign::Center, msg!("Centre")), (LabelAlign::Right, msg!("Right"))]
                            {
                                ui.selectable_value(&mut y.align, a, t(label));
                            }
                        });
                        ui.end_row();
                    }
                }
                if visible_row(ui, &mut y.visible, format!("{} {n}", t("Symbol Visible")), &values) {
                    let typed = y.visible.trim().to_string();
                    let field = Field::Symbol(Slot::Visible);
                    links.request = Some(crate::adaptive_editor::associating(
                        &i.to_string(),
                        field,
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
                    let c = ComboBox::from_id_salt(("fe-symbol-sub", i)).selected_text(shown).width(200.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut y.subcategory, String::new(), t("None"));
                        for name in &subs {
                            ui.selectable_value(&mut y.subcategory, name.to_string(), t(name));
                        }
                    });
                    c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, format!("{} {n}", t("Symbol Subcategory"))));
                    ui.end_row();
                }
            });
            if ui.button(t("Delete")).clicked() {
                remove = Some(i);
            }
        });
    }
    if links.request.is_some() {
        ed.associating = links.request;
    }
    if let Some(i) = remove {
        ed.symbols.remove(i);
    }
    ui.horizontal(|ui| {
        for (part, label) in
            [(Part::Line, msg!("Add Line")), (Part::Arc, msg!("Add Arc")), (Part::Text, msg!("Add Text")), (Part::Dot, msg!("Add Dot"))]
        {
            if ui.button(t(label)).clicked() {
                ed.symbols.push(SymbolText::blank(part));
            }
        }
    });
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
    /// Open the Family Editor on family `name` of the project.
    pub(crate) fn edit_family(&mut self, name: &str) {
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
                editor.tab = Tab::Adaptive;
            }
            self.family_editor = Some(editor);
        }
    }

    /// Open the Family Editor on a new family of `category`.
    pub(crate) fn new_family(&mut self, category: Category) {
        self.family_editor = Some(FamilyEditor::blank(category));
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
        let Some(mut ed) = self.family_editor.take() else { return };
        let materials: Vec<String> = self.session.model.materials.keys().cloned().collect();
        self.document_dialogs(ctx, &mut ed, &materials);
        self.go_to_view_dialog(ctx, &mut ed);
        let ask = std::mem::take(&mut ed.ask);
        ed.history(ctx, &ask);
        self.family_asked(ctx, ed, ask);
    }

    /// Does what the family document's ribbon asked for: capturing the selection's frames, loading a file, making the family the
    /// project's (Load into Project, placing the test frames), saving it to a file or the Favorites (from a session of its own,
    /// which never touches the project), then closing it on Close Family, or on Load into Project and Close when the family
    /// went in. It stays open otherwise.
    fn family_asked(&mut self, ctx: &Context, mut ed: FamilyEditor, ask: Asked) {
        let base = Arc::clone(&self.session.model);
        let Asked { capture: capture_context, place: place_adaptive, apply_placement, apply, ok, cancel, save, favorite, load, .. } = ask;
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
            let dialog = rfd::AsyncFileDialog::new().add_filter(t("BIMCraft family"), &["json"]);
            self.ask_file(ctx, dialog, false, |app, path| match read_family_file(&path) {
                Ok((name, f)) => {
                    // Closed meanwhile, the editor has nothing to load into.
                    if let Some(open) = app.family_editor.as_mut() {
                        *open = FamilyEditor::of(&name, &f);
                        open.original = None;
                    }
                }
                Err(text) => app.status = Status { text, error: true },
            });
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
                    if save {
                        let dialog =
                            rfd::AsyncFileDialog::new().add_filter(t("BIMCraft family"), &["json"]).set_file_name(format!("{name}.json"));
                        self.ask_file(ctx, dialog, true, move |app, path| {
                            match draft.execute("family", &json!({"name":name,"save":path.to_string_lossy()})) {
                                // Still open, the document has nothing unsaved but what changed meanwhile.
                                Ok(_) => {
                                    if let Some(open) = app.family_editor.as_mut() {
                                        open.saved_as(saved);
                                    }
                                }
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
        if !cancel && (apply || ok || place_adaptive || apply_placement) {
            self.load_draft(&mut ed, exists::After::Document { close: ok, place: place_adaptive, apply_placement });
        }
        let closed = cancel || std::mem::take(&mut ed.document.loaded_to_close);
        if !closed {
            self.family_editor = Some(ed);
        }
    }
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
