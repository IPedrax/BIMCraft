//! The Family Editor: a family's parameters, types, forms and symbols, edited as text and tried on a
//! preview as they're typed, then made the project's, saved to a file or kept in the Favorites.
//! Every change to the project is a `family` command, so it's checked, and undone, like any other.

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use bimcraft_core::engine::Session;
use bimcraft_core::family::{Category, Circle, Curve, Expr, Family, Form, Kind as Holds, Parameter, Plane, Symbol};
use bimcraft_core::model::Model;
use bimcraft_core::view3d::{AZIMUTH, ELEVATION, Scene};
use egui::{
    Align, CollapsingHeader, Color32, ColorImage, ComboBox, Context, Grid, Id as UiId, Layout, Modal, PointerButton, Rect, RichText,
    ScrollArea, Sense, Stroke, StrokeKind, TextEdit, TextureHandle, TextureOptions, Ui, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::style::pal;
use crate::{BimApp, Status, Tool};

/// A number, as the editor writes it: whole, or to a millionth.
fn number_text(x: f64) -> String {
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

/// What a form is drawn from: the corners of a profile, or a circle.
#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Profile,
    Circle,
}

/// A form as typed.
#[derive(Clone)]
struct FormText {
    /// What it was: what the editor doesn't edit (a mesh's triangles) stays as it was.
    original: Form,
    shape: Shape,
    plane: Plane,
    profile: String,
    circle: [String; 3],
    revolve: bool,
    axis: [String; 4],
    from: String,
    to: String,
    material: String,
    visible: String,
    void: bool,
}

impl FormText {
    fn of(f: &Form) -> FormText {
        let axis = f
            .axis
            .as_ref()
            .map_or_else(Default::default, |[a, b]| [expr_text(&a[0]), expr_text(&a[1]), expr_text(&b[0]), expr_text(&b[1])]);
        let circle =
            f.circle.as_ref().map_or_else(Default::default, |c| [expr_text(&c.centre[0]), expr_text(&c.centre[1]), expr_text(&c.radius)]);
        FormText {
            original: f.clone(),
            shape: if f.circle.is_some() { Shape::Circle } else { Shape::Profile },
            plane: f.plane,
            profile: profile_text(&f.profile),
            circle,
            revolve: f.axis.is_some(),
            axis,
            from: expr_text(&f.from),
            to: expr_text(&f.to),
            material: f.material.clone(),
            visible: f.visible.as_ref().map(expr_text).unwrap_or_default(),
            void: f.void,
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
            (_, Shape::Profile, true) => t("Revolve"),
            (_, Shape::Profile, false) => t("Extrusion"),
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
        (f.plane, f.from, f.to) = (self.plane, text_expr(&self.from), text_expr(&self.to));
        (f.profile, f.circle) = (Vec::new(), None);
        match self.shape {
            Shape::Profile => f.profile = text_profile(&self.profile)?,
            Shape::Circle => {
                f.circle =
                    Some(Circle { centre: [text_expr(&self.circle[0]), text_expr(&self.circle[1])], radius: text_expr(&self.circle[2]) });
            }
        }
        let a = &self.axis;
        f.axis = self.revolve.then(|| [[text_expr(&a[0]), text_expr(&a[1])], [text_expr(&a[2]), text_expr(&a[3])]]);
        Ok(f)
    }
}

/// A parameter as typed.
#[derive(Clone)]
struct ParamText {
    name: String,
    kind: Holds,
    instance: bool,
    value: String,
    formula: String,
}

/// The name a parameter's kind goes by.
fn kind_name(k: Holds) -> &'static str {
    match k {
        Holds::Length => "Length",
        Holds::Number => "Number",
        Holds::Angle => "Angle",
        Holds::YesNo => "Yes/No",
        Holds::Material => "Material",
    }
}

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
        Holds::Material => json!(s),
        Holds::YesNo => json!(matches!(s, "1" | "true" | "yes" | "Yes")),
        Holds::Length | Holds::Number | Holds::Angle => {
            json!(s.parse::<f64>().ok().filter(|x| x.is_finite()).ok_or_else(|| tf("{}: `{}` isn't a number", &[who, s]))?)
        }
    }))
}

impl ParamText {
    fn of(p: &Parameter) -> ParamText {
        ParamText {
            name: p.name.clone(),
            kind: p.kind,
            instance: p.instance,
            value: value_text(p.kind, &p.value),
            formula: p.formula.clone().unwrap_or_default(),
        }
    }

    fn parameter(&self) -> Result<Parameter, String> {
        let value = text_value(self.kind, &self.value, &self.name)?.unwrap_or(match self.kind {
            Holds::Material => json!(""),
            Holds::YesNo => json!(false),
            _ => json!(0.0),
        });
        let formula = Some(self.formula.trim().to_string()).filter(|f| !f.is_empty());
        Ok(Parameter { name: self.name.trim().to_string(), kind: self.kind, instance: self.instance, value, formula })
    }
}

/// A symbol as typed.
#[derive(Clone)]
struct SymbolText {
    /// Its id and GUID, which it keeps wherever it's moved to in the list.
    ident: (bimcraft_core::model::Id, bimcraft_core::model::Guid),
    arc: bool,
    line: String,
    centre: [String; 2],
    radius: String,
    from: String,
    to: String,
    visible: String,
}

impl SymbolText {
    fn of(y: &Symbol) -> SymbolText {
        let (centre, radius, from, to) = y.arc.as_ref().map_or_else(
            || (Default::default(), String::new(), String::new(), String::new()),
            |c| ([expr_text(&c.centre[0]), expr_text(&c.centre[1])], expr_text(&c.radius), expr_text(&c.from), expr_text(&c.to)),
        );
        SymbolText {
            ident: (y.id, y.guid),
            arc: y.arc.is_some(),
            line: profile_text(&y.line),
            centre,
            radius,
            from,
            to,
            visible: y.visible.as_ref().map(expr_text).unwrap_or_default(),
        }
    }

    fn symbol(&self) -> Result<Symbol, String> {
        let visible = (!self.visible.trim().is_empty()).then(|| text_expr(&self.visible));
        let (id, guid) = self.ident;
        Ok(if self.arc {
            let centre = [text_expr(&self.centre[0]), text_expr(&self.centre[1])];
            Symbol {
                arc: Some(Curve { centre, radius: text_expr(&self.radius), from: text_expr(&self.from), to: text_expr(&self.to) }),
                visible,
                id,
                guid,
                ..Symbol::default()
            }
        } else {
            Symbol { line: text_profile(&self.line)?, visible, id, guid, ..Symbol::default() }
        })
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
    symbols: Vec<SymbolText>,
    tab: Tab,
    /// The type shown in Types and on the preview.
    picked: usize,
    /// How the preview looks at it: azimuth and elevation, degrees.
    look: (f64, f64),
    /// The preview, and what it was drawn of.
    preview: Option<(u64, TextureHandle)>,
    /// Why the family can't be tried or made: shown under the preview.
    problem: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Parameters,
    Types,
    Forms,
    Symbols,
}

impl FamilyEditor {
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
            symbols: f.symbols.iter().map(SymbolText::of).collect(),
            tab: Tab::Parameters,
            picked: 0,
            look: (AZIMUTH, ELEVATION),
            preview: None,
            problem: None,
        }
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
        Ok(Family { category: self.category, parameters, types, forms, symbols })
    }

    /// The `family` command's parameters making the project's family what's typed: its name, its own and the type it drops.
    fn command(&self, project: &Model) -> Result<Value, String> {
        if self.name.trim().is_empty() {
            return Err(t("a family needs a name").to_string());
        }
        let family = self.family()?;
        let mut p = serde_json::to_value(&family).map_err(|e| e.to_string())?;
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
            m.families.insert(name.to_string(), family.clone());
        }
        let place = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).map(|_| ());
        if family.category.hosted() {
            place(&mut s, "wall", json!({"points": [[-2500, 0], [2500, 0]]}))?;
            let kind = if family.category == Category::Door { "door" } else { "window" };
            place(&mut s, kind, json!({"at": [0, 0], "family": name, "type": type_name}))?;
        } else {
            place(&mut s, "component", json!({"family": name, "type": type_name, "at": [0, 0]}))?;
        }
        Ok(Arc::unwrap_or_clone(s.model))
    }

    /// The preview, drawn again when what it shows or how it's looked at changes.
    fn refresh(&mut self, ctx: &Context, base: &Model) {
        let result = self.family().and_then(|f| {
            let key = {
                let mut h = DefaultHasher::new();
                (
                    serde_json::to_string(&f).unwrap_or_default(),
                    self.picked,
                    self.name.trim(),
                    self.look.0.to_bits(),
                    self.look.1.to_bits(),
                )
                    .hash(&mut h);
                (base.materials.len(), base.images.len()).hash(&mut h);
                h.finish()
            };
            if self.preview.as_ref().is_some_and(|(k, _)| *k == key) {
                return Ok(None);
            }
            let model = self.scratch(base, &f)?;
            let scene = Scene::new(&model);
            let (w, h) = (360_u32, 300_u32);
            let cam = scene.fit(self.look.0, self.look.1, w, h);
            let image = scene.render(&cam, &[]);
            let ground = Color32::from_rgb(pal().panel.r(), pal().panel.g(), pal().panel.b());
            let pixels = image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| if p[..3] == [255; 3] { ground } else { Color32::from_rgb(p[0], p[1], p[2]) })
                .collect();
            let picture = ColorImage { size: [w as usize, h as usize], pixels, source_size: Default::default() };
            Ok(Some((key, ctx.load_texture("family-preview", picture, TextureOptions::LINEAR))))
        });
        match result {
            Ok(Some(drawn)) => (self.preview, self.problem) = (Some(drawn), None),
            Ok(None) => self.problem = None,
            Err(e) => self.problem = Some(e),
        }
    }
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
        Category::Furniture => "Furniture",
        Category::Casework => "Casework",
        Category::Plumbing => "Plumbing Fixtures",
        Category::Drain => "Drains",
        Category::PipeFitting => "Pipe Fittings",
        Category::PipeAccessory => "Pipe Accessories",
        Category::Equipment => "Electrical Equipment",
        Category::Outlet => "Outlets",
        Category::Switch => "Switches",
        Category::AirTerminal => "Air Terminals",
        Category::Earthing => "Earthing Electrodes",
        Category::InspectionBox => "Inspection Boxes",
        Category::DataOutlet => "Data Outlets",
        Category::VoiceOutlet => "Voice Outlets",
        Category::PatchPanel => "Patch Panels",
        Category::Rack => "Racks",
        Category::Lighting => "Lighting",
        Category::Generic => "Generic",
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

fn parameters_tab(ed: &mut FamilyEditor, ui: &mut Ui, materials: &[String]) {
    if ed.category.hosted() {
        let note = t("A door or a window needs lengths named Width and Height; its formulas can read Wall Thickness, the wall's.");
        ui.label(RichText::new(note).color(pal().muted));
    }
    let mut remove = None;
    Grid::new("fe-params").num_columns(6).striped(true).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
        for h in ["Name", "Kind", "Instance", "Value", "Formula", ""] {
            ui.label(RichText::new(t(h)).color(pal().muted));
        }
        ui.end_row();
        for (i, p) in ed.params.iter_mut().enumerate() {
            let n = i + 1;
            field(ui, format!("{} {n}", t("Parameter Name")), &mut p.name, 130.0);
            let kinds = [Holds::Length, Holds::Number, Holds::Angle, Holds::YesNo, Holds::Material];
            ComboBox::from_id_salt(("fe-param-kind", i)).selected_text(t(kind_name(p.kind))).width(84.0).show_ui(ui, |ui| {
                for k in kinds {
                    ui.selectable_value(&mut p.kind, k, t(kind_name(k)));
                }
            });
            let r = ui.checkbox(&mut p.instance, "");
            r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, p.instance, format!("{} {n}", t("Parameter Instance"))));
            match p.kind {
                Holds::YesNo => {
                    let mut on = p.value == "1";
                    let r = ui.checkbox(&mut on, "");
                    r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, format!("{} {n}", t("Parameter Value"))));
                    p.value = (if on { "1" } else { "0" }).into();
                }
                Holds::Material => {
                    let mut options = vec![String::new()];
                    options.extend(materials.iter().cloned());
                    if let Some(m) = pick(ui, format!("{} {n}", t("Parameter Value")), &p.value, &options) {
                        p.value = m;
                    }
                }
                _ => field(ui, format!("{} {n}", t("Parameter Value")), &mut p.value, 90.0),
            }
            field(ui, format!("{} {n}", t("Parameter Formula")), &mut p.formula, 150.0);
            if ui.small_button(t("Delete")).clicked() {
                remove = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = remove {
        ed.params.remove(i);
    }
    if ui.button(t("Add Parameter")).clicked() {
        let name = (1..).map(|k| format!("{} {k}", t("Parameter"))).find(|n| !ed.params.iter().any(|p| p.name == *n)).unwrap_or_default();
        ed.params.push(ParamText { name, kind: Holds::Length, instance: false, value: "0".into(), formula: String::new() });
    }
}

fn types_tab(ed: &mut FamilyEditor, ui: &mut Ui, materials: &[String]) {
    ed.picked = ed.picked.min(ed.types.len().saturating_sub(1));
    ui.horizontal(|ui| {
        let names: Vec<String> = ed.types.iter().map(|(n, _)| n.clone()).collect();
        let now = names.get(ed.picked).cloned().unwrap_or_default();
        if let Some(n) = pick(ui, t("Type").to_string(), &now, &names)
            && let Some(i) = names.iter().position(|x| *x == n)
        {
            ed.picked = i;
        }
        if ui.button(t("Add Type")).clicked() {
            let name = (1..).map(|k| format!("{} {k}", t("Type"))).find(|n| !names.contains(n)).unwrap_or_default();
            ed.types.push((name, BTreeMap::new()));
            ed.picked = ed.types.len() - 1;
        }
        if ui.button(t("Duplicate Type")).clicked()
            && let Some((n, set)) = ed.types.get(ed.picked).cloned()
        {
            let name = (2..).map(|k| format!("{n} {k}")).find(|x| !names.contains(x)).unwrap_or_default();
            ed.types.push((name, set));
            ed.picked = ed.types.len() - 1;
        }
        if ui.add_enabled(ed.types.len() > 1, egui::Button::new(t("Delete Type"))).clicked() {
            ed.types.remove(ed.picked);
        }
    });
    let own: Vec<(String, Holds)> =
        ed.params.iter().filter(|p| !p.instance && p.formula.trim().is_empty()).map(|p| (p.name.clone(), p.kind)).collect();
    let Some((name, set)) = ed.types.get_mut(ed.picked) else { return };
    ui.add_space(4.0);
    Grid::new("fe-type").num_columns(2).spacing(vec2(12.0, 5.0)).show(ui, |ui| {
        ui.label(t("Type Name"));
        field(ui, t("Type Name").to_string(), name, 200.0);
        ui.end_row();
        for (param, kind) in &own {
            ui.label(param);
            let text = set.entry(param.clone()).or_default();
            let label = format!("{} {param}", t("Type Value"));
            match kind {
                Holds::YesNo => {
                    // Blank is the family's; a tick or a clear is the type's own.
                    let mut on = text == "1";
                    let r = ui.checkbox(&mut on, "");
                    r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, label.as_str()));
                    if r.changed() {
                        *text = (if on { "1" } else { "0" }).into();
                    }
                }
                Holds::Material => {
                    let mut options = vec![String::new()];
                    options.extend(materials.iter().cloned());
                    if let Some(m) = pick(ui, label, text, &options) {
                        *text = m;
                    }
                }
                _ => field(ui, label, text, 120.0),
            }
            ui.end_row();
        }
    });
    ui.label(RichText::new(t("A blank value is the family's own, as the Parameters tab has it.")).color(pal().muted));
}

/// One form's fields.
fn form_fields(ui: &mut Ui, f: &mut FormText, i: usize, options: &[String]) {
    let n = i + 1;
    let planes = [(Plane::Xy, "Plane XY"), (Plane::Xz, "Plane XZ"), (Plane::Yz, "Plane YZ")];
    Grid::new(("fe-form-grid", i)).num_columns(2).spacing(vec2(12.0, 5.0)).show(ui, |ui| {
        if f.mesh() {
            ui.label(t("Mesh"));
            ui.label(RichText::new(tf("{} triangles, from a file", &[&f.original.mesh.len().to_string()])).color(pal().muted));
            ui.end_row();
        } else {
            ui.label(t("Plane"));
            ComboBox::from_id_salt(("fe-plane", i))
                .selected_text(t(planes.iter().find(|(p, _)| *p == f.plane).map_or("Plane XY", |(_, l)| l)))
                .width(110.0)
                .show_ui(ui, |ui| {
                    for (p, label) in planes {
                        ui.selectable_value(&mut f.plane, p, t(label));
                    }
                });
            ui.end_row();
            ui.label(t("Shape"));
            ComboBox::from_id_salt(("fe-shape", i))
                .selected_text(if f.shape == Shape::Profile { t("Profile") } else { t("Circle") })
                .width(110.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut f.shape, Shape::Profile, t("Profile"));
                    ui.selectable_value(&mut f.shape, Shape::Circle, t("Circle"));
                });
            ui.end_row();
            if f.shape == Shape::Profile {
                ui.label(t("Corners"));
                let r = ui.add(TextEdit::multiline(&mut f.profile).desired_rows(4).desired_width(300.0).id_salt(("fe-profile", i)));
                r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, format!("{} {n}", t("Form Corners"))));
            } else {
                ui.label(t("Circle centre and radius"));
                ui.horizontal(|ui| {
                    for (k, what) in ["Form Circle X", "Form Circle Y", "Form Circle Radius"].into_iter().enumerate() {
                        field(ui, format!("{} {n}", t(what)), &mut f.circle[k], 80.0);
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
                    for (k, what) in ["Form Axis X1", "Form Axis Y1", "Form Axis X2", "Form Axis Y2"].into_iter().enumerate() {
                        field(ui, format!("{} {n}", t(what)), &mut f.axis[k], 60.0);
                    }
                });
                ui.end_row();
            }
            ui.label(if f.revolve { t("From (degrees)") } else { t("From") });
            field(ui, format!("{} {n}", t("Form From")), &mut f.from, 200.0);
            ui.end_row();
            ui.label(if f.revolve { t("To (degrees)") } else { t("To") });
            field(ui, format!("{} {n}", t("Form To")), &mut f.to, 200.0);
            ui.end_row();
        }
        ui.label(t("Material"));
        if let Some(m) = pick(ui, format!("{} {n}", t("Form Material")), &f.material, options) {
            f.material = m;
        }
        ui.end_row();
        ui.label(t("Visible when"));
        field(ui, format!("{} {n}", t("Form Visible")), &mut f.visible, 200.0);
        ui.end_row();
        ui.label(t("Void"));
        let r = ui.checkbox(&mut f.void, t("cuts the forms listed above it"));
        r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, f.void, format!("{} {n}", t("Form Void"))));
        ui.end_row();
    });
}

fn forms_tab(ed: &mut FamilyEditor, ui: &mut Ui, materials: &[String]) {
    // What a form may be made of: nothing, a library material, or a material parameter.
    let mut options = vec![String::new()];
    options.extend(ed.params.iter().filter(|p| p.kind == Holds::Material).map(|p| p.name.clone()));
    options.extend(materials.iter().cloned());
    let mut act: Option<(usize, i8)> = None;
    for i in 0..ed.forms.len() {
        let heading = format!("{}. {}", i + 1, ed.forms[i].describe());
        CollapsingHeader::new(heading).id_salt(("fe-form", i)).default_open(ed.forms.len() == 1).show(ui, |ui| {
            form_fields(ui, &mut ed.forms[i], i, &options);
            ui.horizontal(|ui| {
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
            ed.forms.push(FormText { plane: Plane::Xz, profile, revolve: true, to: "360".into(), ..FormText::blank() });
        }
        if ui.button(t("Add Void")).clicked() {
            ed.forms.push(FormText { void: true, ..FormText::blank() });
        }
    });
}

fn symbols_tab(ed: &mut FamilyEditor, ui: &mut Ui) {
    if !ed.category.hosted() {
        ui.label(RichText::new(t("Symbols are the lines a door's or a window's plan draws; this family isn't one.")).color(pal().muted));
        return;
    }
    let mut remove = None;
    for (i, y) in ed.symbols.iter_mut().enumerate() {
        let n = i + 1;
        let heading = format!("{n}. {}", if y.arc { t("Arc") } else { t("Line") });
        CollapsingHeader::new(heading).id_salt(("fe-symbol", i)).default_open(true).show(ui, |ui| {
            Grid::new(("fe-symbol-grid", i)).num_columns(2).spacing(vec2(12.0, 5.0)).show(ui, |ui| {
                if y.arc {
                    ui.label(t("Centre"));
                    ui.horizontal(|ui| {
                        for (k, what) in ["Symbol Centre X", "Symbol Centre Y"].into_iter().enumerate() {
                            field(ui, format!("{} {n}", t(what)), &mut y.centre[k], 170.0);
                        }
                    });
                    ui.end_row();
                    for (label, what, text) in [
                        ("Radius", "Symbol Radius", &mut y.radius),
                        ("From (degrees)", "Symbol From", &mut y.from),
                        ("To (degrees)", "Symbol To", &mut y.to),
                    ] {
                        ui.label(t(label));
                        field(ui, format!("{} {n}", t(what)), text, 200.0);
                        ui.end_row();
                    }
                } else {
                    ui.label(t("Points"));
                    let r = ui.add(TextEdit::multiline(&mut y.line).desired_rows(3).desired_width(300.0).id_salt(("fe-symbol-line", i)));
                    r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, format!("{} {n}", t("Symbol Points"))));
                    ui.end_row();
                }
                ui.label(t("Visible when"));
                field(ui, format!("{} {n}", t("Symbol Visible")), &mut y.visible, 200.0);
                ui.end_row();
            });
            if ui.button(t("Delete")).clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        ed.symbols.remove(i);
    }
    ui.horizontal(|ui| {
        if ui.button(t("Add Line")).clicked() {
            ed.symbols.push(SymbolText {
                ident: Default::default(),
                arc: false,
                line: "0, 0\n0, 900".into(),
                centre: Default::default(),
                radius: String::new(),
                from: String::new(),
                to: String::new(),
                visible: String::new(),
            });
        }
        if ui.button(t("Add Arc")).clicked() {
            ed.symbols.push(SymbolText {
                ident: Default::default(),
                arc: true,
                line: String::new(),
                centre: ["0".into(), "0".into()],
                radius: "900".into(),
                from: "0".into(),
                to: "90".into(),
                visible: String::new(),
            });
        }
    });
}

/// The family in the file at `path`: its name (its own, else the file's), and what it holds.
fn read_family_file(path: &std::path::Path) -> Result<(String, Family), String> {
    let text = bimcraft_core::disk::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
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
            self.family_editor = Some(FamilyEditor::of(name, f));
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

    /// The Family Editor, while it's open.
    pub(crate) fn family_editor_window(&mut self, ctx: &Context) {
        let Some(mut ed) = self.family_editor.take() else { return };
        let base = Arc::clone(&self.session.model);
        ed.refresh(ctx, &base);
        let materials: Vec<String> = base.materials.keys().cloned().collect();
        let (mut apply, mut ok, mut cancel, mut save, mut favorite, mut load) = (false, false, false, false, false, false);
        let r = Modal::new(UiId::new("family-editor")).show(ctx, |ui| {
            ui.set_width(1000.0);
            ui.label(RichText::new(t("Family Editor")).heading());
            ui.add_space(4.0);
            Grid::new("fe-head").num_columns(4).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                let l = ui.label(t("Family name"));
                ui.add_sized([220.0, ui.spacing().interact_size.y], TextEdit::singleline(&mut ed.name)).labelled_by(l.id);
                let l = ui.label(t("Category"));
                let c = ComboBox::from_id_salt("fe-category").selected_text(category_label(ed.category)).width(150.0).show_ui(ui, |ui| {
                    for c in CATEGORIES {
                        if ui.selectable_label(ed.category == c, category_label(c)).clicked() {
                            ed.category = c;
                        }
                    }
                });
                c.response.labelled_by(l.id);
                ui.end_row();
            });
            ui.add_space(6.0);
            // What the tabs leave of the screen, to the buttons below: a small one scrolls them.
            let room = (ctx.content_rect().height() - 330.0).clamp(160.0, 470.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(620.0);
                    ui.horizontal(|ui| {
                        for (tab, label) in
                            [(Tab::Parameters, "Parameters"), (Tab::Types, "Types"), (Tab::Forms, "Forms"), (Tab::Symbols, "Symbols")]
                        {
                            if ui.selectable_label(ed.tab == tab, t(label)).clicked() {
                                ed.tab = tab;
                            }
                        }
                    });
                    ui.separator();
                    ScrollArea::vertical().max_height(room).min_scrolled_height(room).auto_shrink([false, false]).show(ui, |ui| {
                        match ed.tab {
                            Tab::Parameters => parameters_tab(&mut ed, ui, &materials),
                            Tab::Types => types_tab(&mut ed, ui, &materials),
                            Tab::Forms => forms_tab(&mut ed, ui, &materials),
                            Tab::Symbols => symbols_tab(&mut ed, ui),
                        }
                    });
                });
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.set_width(340.0);
                    ui.label(RichText::new(t("Preview")).strong());
                    let (rect, response) = ui.allocate_exact_size(vec2(340.0, 283.0), Sense::drag());
                    if let Some((_, tile)) = &ed.preview {
                        ui.painter().image(tile.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.0, pal().border), StrokeKind::Inside);
                    if response.dragged_by(PointerButton::Primary) {
                        let d = response.drag_delta();
                        ed.look = (ed.look.0 - f64::from(d.x) * 0.5, (ed.look.1 + f64::from(d.y) * 0.5).clamp(-89.0, 89.0));
                    }
                    ui.label(RichText::new(t("Drag the picture to turn it.")).color(pal().muted));
                    let names: Vec<String> = ed.types.iter().map(|(n, _)| n.clone()).collect();
                    let now = names.get(ed.picked).cloned().unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.label(t("Previewing"));
                        if let Some(n) = pick(ui, t("Preview Type").to_string(), &now, &names)
                            && let Some(i) = names.iter().position(|x| *x == n)
                        {
                            ed.picked = i;
                        }
                    });
                    if let Some(p) = &ed.problem {
                        ui.label(RichText::new(p).color(pal().error));
                    }
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                load = ui.button(t("Load Family…")).clicked();
                save = ui.button(t("Save Family…")).clicked();
                favorite = ui.button(t("Add to Favorites")).clicked();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ok = ui.button(t("OK")).clicked();
                    cancel = ui.button(t("Cancel")).clicked();
                    apply = ui.button(t("Apply")).clicked();
                });
            });
        });
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
        // Made the project's: Apply and OK do it, and so do saving to a file or to the Favorites, which keep what's in the project.
        let mut made = false;
        if apply || ok || save || favorite {
            match ed.command(&base) {
                Ok(p) => {
                    if self.run("family", p).is_some() {
                        ed.original = Some(ed.name.trim().to_string());
                        made = true;
                    } else {
                        // Said in the window; the box over the view's corner would lie on its buttons.
                        ed.problem = Some(std::mem::take(&mut self.status).text);
                    }
                }
                Err(e) => ed.problem = Some(e),
            }
        }
        let name = ed.name.trim().to_string();
        if made && save {
            let dialog = rfd::AsyncFileDialog::new().add_filter(t("BIMCraft family"), &["json"]).set_file_name(format!("{name}.json"));
            let family = name.clone();
            self.ask_file(ctx, dialog, true, move |app, path| {
                app.run("family", json!({"name": family, "save": path.to_string_lossy()}));
            });
        }
        if made && favorite {
            let p = self.favorite_params("save", "family", &name);
            if self.run("favorite", p).is_some() {
                self.status = Status { text: tf("Saved {} to your Favorites", &[&name]), error: false };
            }
        }
        if !(cancel || (ok && made) || r.should_close()) {
            self.family_editor = Some(ed);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

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
        let m = Model::new("x");
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
        (text.revolve, text.plane, text.to) = (true, Plane::Xz, "360".into());
        let f = text.form().unwrap();
        assert!(f.axis.is_some() && f.plane == Plane::Xz && f.to == Expr::Number(360.0) && f.profile.len() == 4);
        assert_eq!(FormText::of(&f).describe(), "Revolve");
        text.shape = Shape::Circle;
        text.circle = ["0".into(), "0".into(), "R".into()];
        let f = text.form().unwrap();
        assert!(f.profile.is_empty() && f.circle.as_ref().is_some_and(|c| c.radius == Expr::Formula("R".into())));
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
        let p = Parameter { name: "Legs".into(), kind: Holds::YesNo, instance: true, value: json!(true), formula: None };
        assert_eq!(ParamText::of(&p).parameter().unwrap(), p);
        let area = Parameter {
            name: "Area".into(),
            kind: Holds::Number,
            instance: false,
            value: json!(0.0),
            formula: Some("Width * Depth".into()),
        };
        assert_eq!(ParamText::of(&area).parameter().unwrap(), area);
        assert_eq!(kind_name(Holds::YesNo), "Yes/No");
        // The rest of an editor's tabs list: all of them there.
        let tabs: BTreeSet<&str> = ["Parameters", "Types", "Forms", "Symbols"].into_iter().collect();
        assert_eq!(tabs.len(), 4);
    }
}
