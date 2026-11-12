//! Direct editing of the canonical adaptive dependency model, without a graph canvas: the Properties beside the modelling view
//! (`adaptive_view.rs`), and the list of every element, the test placement and what the evaluation says.
use crate::lang::{t, tf};
use bimcraft_core::adaptive::{self, Constrained, Definition, Evaluated, Frame, Host, Operation, OrientsTo, Placement, PointType, Show};
use bimcraft_core::family::{Expr, Kind as Holds, Val};
use bimcraft_i18n::msg;
use egui::{ComboBox, DragValue, RichText, TextEdit, Ui, WidgetInfo, WidgetType};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Where a point is, mm.
fn position(ui: &mut Ui, value: &mut Frame) {
    ui.horizontal(|ui| {
        ui.label(t("Position (mm)"));
        for v in &mut value.origin {
            ui.add(DragValue::new(v).speed(5.));
        }
    });
}
fn frame(ui: &mut Ui, value: &mut Frame) {
    ui.horizontal(|ui| {
        ui.label(t("Position (mm)"));
        for v in &mut value.origin {
            ui.add(DragValue::new(v).speed(5.));
        }
    });
    let mut delta = [0.; 3];
    ui.horizontal(|ui| {
        ui.label(t("Rotate local X / Y / Z (degrees)"));
        for v in &mut delta {
            ui.add(DragValue::new(v).speed(0.5));
        }
    });
    if delta != [0.; 3]
        && let Ok(next) = value.transformed([0.; 3], delta)
    {
        *value = next;
    }
    ui.collapsing(t("Exact frame axes"), |ui| {
        for (name, axis) in [("X", &mut value.x), ("Y", &mut value.y), ("Z", &mut value.z)] {
            ui.horizontal(|ui| {
                ui.label(name);
                for v in axis {
                    ui.add(DragValue::new(v).speed(0.01));
                }
            });
        }
    });
    if let Err(error) = value.checked() {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
}
fn choice(ui: &mut Ui, key: &str, value: &mut String, options: &[&str]) {
    ComboBox::from_id_salt(key).selected_text(value.as_str()).show_ui(ui, |ui| {
        for option in options {
            ui.selectable_value(value, (*option).into(), *option);
        }
    });
}
// Only fields applicable to the chosen operation and host are present in this property tree.
fn properties(ui: &mut Ui, key: &str, value: &mut Value) {
    ui.push_id(key, |ui| match value {
        Value::Object(map) => {
            for (k, v) in map {
                if k == "type" {
                    continue;
                }
                ui.horizontal_top(|ui| {
                    ui.label(k.as_str());
                    ui.vertical(|ui| properties(ui, k, v));
                });
            }
        }
        Value::Array(items) => {
            for (index, v) in items.iter_mut().enumerate() {
                properties(ui, &format!("{key}[{index}]"), v);
            }
            if key == "points" || key == "profiles" {
                ui.horizontal(|ui| {
                    if ui.small_button(t("Add reference")).clicked() {
                        items.push(json!(""));
                    }
                    if ui.small_button(t("Remove last")).clicked() {
                        items.pop();
                    }
                });
            }
        }
        Value::String(s) => match key {
            "orientation" => {
                choice(ui, key, s, &["interpolate_normals", "project_interpolated_normals", "world_vertical", "project_world_z"])
            }
            "measurement" => choice(
                ui,
                key,
                s,
                &[
                    "non_normalized_curve_parameter",
                    "normalized_curve_parameter",
                    "segment_length",
                    "normalized_segment_length",
                    "chord_length",
                    "angle",
                ],
            ),
            "plane" => choice(ui, key, s, &["xy", "xz", "yz"]),
            "axis_a" | "axis_b" => choice(ui, key, s, &["x", "y", "z"]),
            _ => {
                ui.text_edit_singleline(s);
            }
        },
        Value::Number(n) => {
            let mut text = n.to_string();
            if ui.text_edit_singleline(&mut text).changed() {
                *value = serde_json::from_str::<serde_json::Number>(&text).ok().map(Value::Number).unwrap_or(Value::String(text));
            }
        }
        Value::Bool(b) => {
            ui.checkbox(b, key);
        }
        Value::Null => {
            if ui.small_button(t("Set")).clicked() {
                *value = if key == "direction" { json!([0, 0, 1]) } else { json!(1) };
            }
        }
    });
}
fn operation(kind: &str) -> Value {
    match kind {
        "point" => json!({"kind":"point","host":{"type":"frame","point":"p1"},"transform":{"offset":[0,0,0],"rotation":[0,0,0]}}),
        "line" => json!({"kind":"line","a":"p1","b":"p2"}),
        "polyline" => json!({"kind":"polyline","points":["p1","p2","p3","p4"],"closed":true}),
        "bezier" => json!({"kind":"bezier","points":["p1","p2","p3","p4"]}),
        "patch" => json!({"kind":"patch","chain":"outline"}),
        "extrude" => json!({"kind":"extrude","chain":"outline","distance":100,"direction":null}),
        "loft" => json!({"kind":"loft","profiles":["outline","profile2"]}),
        _ => json!({"kind":"intersection","a":"p1","b":"p2","axis_a":"z","axis_b":"z","forward_only":false}),
    }
}
fn host(kind: &str) -> Value {
    match kind {
        "world" => json!({"type":"world","frame":Frame::default()}),
        "adaptive" => json!({"type":"adaptive","number":1,"frame":Frame::default(),"orients":"instance"}),
        "handle" => json!({"type":"handle","frame":Frame::default(),"orients":"instance","constrained":"none"}),
        "plane" => json!({"type":"plane","point":"p1","plane":"xy"}),
        "curve" => {
            json!({"type":"curve","curve":"reference","location":{"measurement":"parameter","value":0.5,"from_end":false},"orientation":"project_interpolated_normals"})
        }
        "face" => json!({"type":"face","form":"panel","topology":"select evaluated mesh token below","face":0,"barycentric":[1,0,0]}),
        "edge" => json!({"type":"edge","form":"panel","topology":"select evaluated mesh token below","edge":0,"parameter":0.5}),
        "vertex" => json!({"type":"vertex","form":"panel","topology":"select evaluated mesh token below","vertex":0}),
        _ => json!({"type":"frame","point":"p1"}),
    }
}
/// Every element of the family, with its properties, and the test placement; Associate Family Parameter, when an "=" of one
/// of them was pressed.
pub(crate) fn show(
    ui: &mut Ui,
    definition: &mut Option<Definition>,
    values: &BTreeMap<String, Val>,
    params: &[(String, Holds)],
) -> Option<Associating> {
    if definition.is_none() {
        ui.label(t("An adaptive family's points are placed where each instance is clicked, as Revit's Generic Model Adaptive."));
        if ui.button(t("Make this family adaptive")).clicked() {
            *definition = Some(Definition::default());
        }
        if ui.button(t("Create four-point adaptive panel")).clicked() {
            *definition = Some(adaptive::four_point_panel());
        }
        return None;
    }
    let d = definition.as_mut()?;
    let mut asked = None;
    ui.label(t("Lengths: mm. Rotations: degrees, local intrinsic X then Y then Z. Offsets use host axes."));
    ui.collapsing(t("Simulate placement frames"), |ui| {
        ui.text_edit_singleline(&mut d.preview.context);
        for node in &d.nodes {
            if let Operation::Point { host: Host::Adaptive { number, frame: default, .. }, .. } = &node.operation {
                ui.push_id(number, |ui| {
                    ui.collapsing(tf("Placement {} - {}", &[&number.to_string(), &node.name]), |ui| {
                        let f = d.preview.frames.entry(*number).or_insert(*default);
                        frame(ui, f);
                    });
                });
            }
        }
        if ui.button(t("Reset test placement to definition frames")).clicked() {
            d.preview = Placement::default();
        }
    });
    let mut remove = None;
    for (i, node) in d.nodes.iter_mut().enumerate() {
        ui.push_id(node.id.clone(), |ui| {
            egui::CollapsingHeader::new(format!("{} - {}", node.id, node.name)).show(ui, |ui| {
                let (deleted, a) = node_properties(ui, node, values, params);
                if deleted {
                    remove = Some(i);
                }
                if a.is_some() {
                    asked = a;
                }
            });
        });
    }
    if let Some(i) = remove {
        d.nodes.remove(i);
    }
    ui.horizontal(|ui| {
        for kind in ["point", "line", "polyline", "patch", "extrude", "loft", "intersection"] {
            if ui.button(tf("Add {}", &[kind])).clicked() {
                let mut index = d.nodes.len() + 1;
                while d.nodes.iter().any(|n| n.id == format!("entity{index}")) {
                    index += 1;
                }
                if let Ok(op) = serde_json::from_value(operation(kind)) {
                    d.nodes.push(adaptive::Node {
                        id: format!("entity{index}"),
                        name: kind.into(),
                        operation: op,
                        appearance: Default::default(),
                    });
                }
            }
        }
    });
    let report = d.evaluate(values, &d.preview);
    ui.separator();
    ui.label(RichText::new(t("Evaluation and host references")).strong());
    for id in &report.order {
        ui.collapsing(tf("{}: valid", &[id]), |ui| {
            if let Some(Evaluated::Point { frame }) = report.values.get(id) {
                ui.label(tf("Origin (mm): {}", &[&format!("{:?}", frame.origin)]));
                ui.label(format!("X: {:?}  Y: {:?}  Z: {:?}", frame.x, frame.y, frame.z));
            }
            if let Some(Evaluated::Form { mesh, .. }) = report.values.get(id) {
                ui.label(tf("Topology: {} ({} triangles)", &[&mesh.topology(), &mesh.faces.len().to_string()]));
                ui.label(t("Face = triangle index + barycentric coordinates; edge/vertex = native mesh index."));
            }
            if let Some(node) = d.nodes.iter().find(|n| &n.id == id) {
                ui.label(tf("Depends on: {}", &[&node.dependencies().join(", ")]));
            }
        });
    }
    diagnostics(ui, &report);
    asked
}

/// What the evaluation says went wrong, or warns of.
pub(crate) fn diagnostics(ui: &mut Ui, report: &adaptive::Evaluation) {
    for diagnostic in &report.diagnostics {
        ui.colored_label(
            if diagnostic.warning { ui.visuals().warn_fg_color } else { ui.visuals().error_fg_color },
            format!("{} [{}]: {} ({})", diagnostic.node, diagnostic.code, diagnostic.message, diagnostic.path.join(" > ")),
        );
    }
}

/// Revit's labels for Orients to, in the order its list has them.
pub(crate) const ORIENTS: [(OrientsTo, &str); 6] = [
    (OrientsTo::Instance, msg!("Instance (xyz)")),
    (OrientsTo::InstanceZThenHost, msg!("Instance (z) then Host (xy)")),
    (OrientsTo::Host, msg!("Host (xyz)")),
    (OrientsTo::HostAndLoopSystem, msg!("Host and Loop System (xyz)")),
    (OrientsTo::Global, msg!("Global (xyz)")),
    (OrientsTo::GlobalZThenHost, msg!("Global (z) then Host (xy)")),
];
const SHOWS: [(Show, &str); 3] =
    [(Show::Never, msg!("Never")), (Show::WhenSelected, msg!("When Selected")), (Show::Always, msg!("Always"))];
const CONSTRAINTS: [(Constrained, &str); 7] = [
    (Constrained::None, msg!("None")),
    (Constrained::PlaneYz, msg!("Center (Left/Right)")),
    (Constrained::PlaneZx, msg!("Center (Front/Back)")),
    (Constrained::PlaneXy, msg!("Ref. Level")),
    (Constrained::AxisX, msg!("X axis")),
    (Constrained::AxisY, msg!("Y axis")),
    (Constrained::AxisZ, msg!("Z axis")),
];

/// A row of Properties: the label, and a drop-down of `options` named by it; whether it changed.
fn pick<T: Copy + PartialEq>(ui: &mut Ui, label: &str, value: &mut T, options: &[(T, &str)]) -> bool {
    let before = *value;
    let name = ui.label(t(label));
    let shown = options.iter().find(|(v, _)| v == value).map(|(_, l)| t(l)).unwrap_or_default();
    // As wide as the group's value column.
    let column = ui.ctx().data(|w| w.get_temp::<f32>(egui::Id::new("family-group-column"))).unwrap_or(160.0);
    let list = ComboBox::from_id_salt(label).selected_text(shown).width(column - 8.0).truncate().show_ui(ui, |ui| {
        for (v, l) in options {
            ui.selectable_value(value, *v, t(l));
        }
    });
    // Named by its label, as a screen reader reads it.
    list.response.labelled_by(name.id);
    ui.end_row();
    *value != before
}

/// A value in an element's Properties that Revit's "=" ties to a family parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    /// A point's offset from its host along x, y or z, mm (z: off its work plane, along the normal, Revit's Offset).
    Offset(usize),
    /// A point's turn about its x, y or z, degrees (z: about its normal, Revit's Rotation Angle).
    Rotation(usize),
    /// Where a point is along the line or the edge it is on, as its measurement says.
    Along,
    /// Whether it shows when the family is in a project.
    Visible,
    /// What a form is made of.
    Material,
    /// A row of a form of a family's own (one that isn't adaptive), its place among the family's forms the `id`.
    Form(Slot),
    /// A row of a symbol of a family's own, its place among the family's symbols the `id`.
    Symbol(Slot),
}

/// Which row of a family's own form or symbol "=" ties: its numbers, whether it shows, what it is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    From,
    To,
    Visible,
    Material,
    /// A circle's (or a symbol's) centre, x or y; a form's circle's radius is its third.
    Centre(usize),
    /// A revolve's axis: its two points' x and y.
    Axis(usize),
    /// A symbol's radius or text height.
    Radius,
}

/// What Associate Family Parameter ties a row of: an adaptive definition's element, or a family's own forms.
pub(crate) trait Tied {
    /// `a`'s choice made: the row tied to the parameter chosen or, with <None>, untied, keeping what it came to.
    fn tie(&mut self, a: &Associating, values: &BTreeMap<String, Val>);
    /// What the row `a` is open on holds now, as the Family Types dialog types a value: a new parameter starts from it.
    fn value_now(&mut self, a: &Associating, values: &BTreeMap<String, Val>) -> String;
}

impl Tied for Definition {
    fn tie(&mut self, a: &Associating, values: &BTreeMap<String, Val>) {
        tie(self, a, values);
    }

    fn value_now(&mut self, a: &Associating, values: &BTreeMap<String, Val>) -> String {
        value_now(self, a, values)
    }
}

/// Associate Family Parameter, open on `field` of element `id` (its row is `label`, holding a `kind`): the parameter chosen
/// (none: <None>) and, while New Parameter's Parameter Properties are filled in, the new one's name and whether each instance
/// has its own.
#[derive(Clone, Debug)]
pub(crate) struct Associating {
    pub(crate) id: String,
    pub(crate) field: Field,
    pub(crate) kind: Holds,
    pub(crate) label: String,
    pub(crate) chosen: Option<String>,
    pub(crate) new: Option<(String, bool)>,
}

/// A parameter New Parameter adds to the family: its name, what it holds, whether each instance has its own, and its value
/// as the Family Types dialog types it.
pub(crate) struct NewParameter {
    pub(crate) name: String,
    pub(crate) kind: Holds,
    pub(crate) instance: bool,
    pub(crate) value: String,
}

/// The largest number a row takes, mm or degrees.
const LIMIT: f64 = 1e7;

/// The name of what a parameter holds, as Revit's Parameter type says it.
fn holds(kind: Holds) -> &'static str {
    crate::editor::kind_name(kind)
}

/// What `e` comes to with the type's `values`; none for a formula that does not work out.
pub(crate) fn worth(e: &Expr, values: &BTreeMap<String, Val>) -> Option<f64> {
    match e {
        Expr::Number(x) => Some(*x),
        Expr::Formula(f) => bimcraft_core::family::eval(f, &mut |n| match values.get(n) {
            Some(Val::Number(x)) => Ok(*x),
            _ => Err(format!("`{n}` is not a number")),
        })
        .ok(),
    }
}

/// The expression `field` of `node` is; none for Material (a name) and for a field the node has not.
fn expr_mut(node: &mut adaptive::Node, field: Field) -> Option<&mut Expr> {
    // A point is not visible in a project unless it is said to be, a form is.
    let shown = node.is_form() || node.look().visible;
    match (field, &mut node.operation) {
        (Field::Offset(i), Operation::Point { transform, .. }) => transform.offset.get_mut(i),
        (Field::Rotation(i), Operation::Point { transform, .. }) => transform.rotation.get_mut(i),
        (Field::Along, Operation::Point { host: Host::Curve { location, .. }, .. }) => Some(&mut location.value),
        (Field::Along, Operation::Point { host: Host::Edge { parameter, .. }, .. }) => Some(parameter),
        (Field::Visible, _) => Some(node.appearance.visible.get_or_insert(Expr::Number(if shown { 1. } else { 0. }))),
        _ => None,
    }
}

/// Revit's "=" at the end of a row: Associate Family Parameter for `label`, lit while the row is tied. Whether it was pressed.
pub(crate) fn equals(ui: &mut Ui, label: &str, tied: bool) -> bool {
    let name = tf("Associate Family Parameter: {}", &[t(label)]);
    let r = ui.add(egui::Button::new("=").small().selected(tied)).on_hover_text(t("Associate Family Parameter"));
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &name));
    r.clicked()
}

/// The width of a row's field beside its "=".
fn field_width(ui: &Ui) -> f32 {
    let column = ui.ctx().data(|w| w.get_temp::<f32>(egui::Id::new("family-group-column"))).unwrap_or(160.0);
    (column - 34.0).max(40.0)
}

/// A row of Properties holding a number, with Revit's "=" at its end when it is `editable`; tied to a parameter, it shows what
/// the parameter comes to, greyed, and takes no typing. Whether "=" was pressed.
fn number_row(ui: &mut Ui, label: &str, value: &mut Expr, values: &BTreeMap<String, Val>, editable: bool) -> bool {
    let l = ui.label(t(label));
    let width = field_width(ui);
    let tied = matches!(value, Expr::Formula(_));
    let pressed = ui
        .horizontal(|ui| {
            if tied || !editable {
                let mut shown = worth(value, values).map(crate::editor::number_text).unwrap_or_else(|| "?".into());
                ui.add_enabled(false, TextEdit::singleline(&mut shown).desired_width(width)).labelled_by(l.id);
            } else {
                // What is typed stays as typed while the field has the focus, so "2." is not read back as "2".
                let key = ui.make_persistent_id(("number-row", label));
                let mut text = ui
                    .data(|w| w.get_temp::<String>(key))
                    .unwrap_or_else(|| worth(value, values).map(crate::editor::number_text).unwrap_or_default());
                let r = ui.add(TextEdit::singleline(&mut text).desired_width(width)).labelled_by(l.id);
                if r.changed()
                    && let Ok(x) = text.trim().parse::<f64>()
                    && x.is_finite()
                    && x.abs() <= LIMIT
                {
                    *value = Expr::Number(x);
                }
                if r.has_focus() {
                    ui.data_mut(|w| w.insert_temp(key, text));
                } else {
                    ui.data_mut(|w| w.remove::<String>(key));
                }
            }
            editable && equals(ui, label, tied)
        })
        .inner;
    ui.end_row();
    pressed
}

/// A Yes/No row of Properties, with Revit's "=" at its end; tied, it shows what the parameter comes to, greyed. Whether "="
/// was pressed.
fn yes_no_row(ui: &mut Ui, label: &str, value: &mut Option<Expr>, shown: bool, values: &BTreeMap<String, Val>) -> bool {
    let l = ui.label(t(label));
    let tied = matches!(value, Some(Expr::Formula(_)));
    let pressed = ui
        .horizontal(|ui| {
            let mut on = value.as_ref().and_then(|e| worth(e, values)).map_or(shown, |x| x != 0.);
            let r = ui.add_enabled(!tied, egui::Checkbox::without_text(&mut on)).labelled_by(l.id);
            if r.changed() {
                *value = Some(Expr::Number(if on { 1. } else { 0. }));
            }
            ui.add_space((field_width(ui) - r.rect.width()).max(0.0));
            equals(ui, label, tied)
        })
        .inner;
    ui.end_row();
    pressed
}

/// A form's Material row, with Revit's "=" at its end; tied to a material parameter it shows the material that has, greyed.
fn material_row(ui: &mut Ui, material: &mut String, values: &BTreeMap<String, Val>, params: &[(String, Holds)]) -> bool {
    let label = msg!("Material");
    let l = ui.label(t(label));
    let width = field_width(ui);
    let tied = params.iter().any(|(n, k)| *k == Holds::Material && n == material);
    let pressed = ui
        .horizontal(|ui| {
            if tied {
                let mut shown = match values.get(material.as_str()) {
                    Some(Val::Text(m)) => m.clone(),
                    _ => String::new(),
                };
                ui.add_enabled(false, TextEdit::singleline(&mut shown).desired_width(width)).labelled_by(l.id);
            } else {
                ui.add(TextEdit::singleline(material).desired_width(width)).labelled_by(l.id);
            }
            equals(ui, label, tied)
        })
        .inner;
    ui.end_row();
    pressed
}

/// Associate Family Parameter, opening on `field` of `id`: tied already to one of `params`, that one is chosen.
pub(crate) fn associating(id: &str, field: Field, kind: Holds, label: &str, now: Option<&str>, params: &[(String, Holds)]) -> Associating {
    let chosen = now.filter(|n| params.iter().any(|(p, k)| p == n && *k == kind)).map(String::from);
    Associating { id: id.to_string(), field, kind, label: label.to_string(), chosen, new: None }
}

/// The name a measurement's value goes by in Properties, as Revit's.
fn measured(m: adaptive::Measurement) -> (&'static str, Holds) {
    use adaptive::Measurement as M;
    match m {
        M::NonNormalizedCurveParameter => (msg!("Non-Normalized Curve Parameter"), Holds::Length),
        M::NormalizedCurveParameter => (msg!("Normalized Curve Parameter"), Holds::Number),
        M::SegmentLength => (msg!("Segment Length"), Holds::Length),
        M::NormalizedSegmentLength => (msg!("Normalized Segment Length"), Holds::Number),
        M::ChordLength => (msg!("Chord Length"), Holds::Length),
        M::Angle => (msg!("Angle"), Holds::Angle),
    }
}

/// An element's properties as Revit's palette groups them, each number, Yes/No and material with Revit's "=" to tie it to one
/// of the family's `params`. A point: Constraints (its Work Plane, where it is along its line, and how it is turned as `values`
/// evaluate it; its offsets and rotations are in its relationship, `node_properties`), Adaptive Component (Point, Number, Show Placement Number, Orients to, Constrained),
/// Graphics (Show Reference Planes, Visible) and Identity Data (Name). A form: Materials and Finishes, Graphics, Identity Data.
/// Associate Family Parameter, when an "=" was pressed.
pub(crate) fn point_properties(
    ui: &mut Ui,
    d: &mut Definition,
    planes: bimcraft_core::family::Planes,
    id: &str,
    values: &BTreeMap<String, Val>,
    params: &[(String, Holds)],
) -> Option<Associating> {
    use crate::editor::document::group;
    let mut asked = None;
    if let Some(node) = d.nodes.iter_mut().find(|n| n.id == id && n.is_form()) {
        if !node.appearance.void {
            group(ui, msg!("Materials and Finishes"), |ui| {
                if material_row(ui, &mut node.appearance.material, values, params) {
                    let now = node.appearance.material.clone();
                    asked = Some(associating(id, Field::Material, Holds::Material, msg!("Material"), Some(&now), params));
                }
            });
        }
        group(ui, msg!("Graphics"), |ui| {
            if yes_no_row(ui, msg!("Visible"), &mut node.appearance.visible, true, values) {
                let now = match &node.appearance.visible {
                    Some(Expr::Formula(f)) => Some(f.clone()),
                    _ => None,
                };
                asked = Some(associating(id, Field::Visible, Holds::YesNo, msg!("Visible"), now.as_deref(), params));
            }
        });
        group(ui, msg!("Identity Data"), |ui| {
            ui.label(t("Solid/Void"));
            ui.label(t(if node.appearance.void { "Void" } else { "Solid" }));
            ui.end_row();
        });
        return asked;
    }
    let kind = d.point_type(id)?;
    // Its axes where it is now (an instance replayed may turn them): the angles about X, then Y, then Z from the family's.
    let angles = match d.evaluate(values, &d.preview).values.get(id) {
        Some(Evaluated::Point { frame }) => Some(frame.rotations()),
        _ => None,
    };
    let work_plane = d
        .work_plane(id)
        .and_then(|w| crate::editor::document::work_planes(d, planes).into_iter().find(|(p, _)| *p == w).map(|(_, label)| label));
    if let Some(node) = d.nodes.iter_mut().find(|n| n.id == id) {
        group(ui, msg!("Constraints"), |ui| {
            if let Some(label) = &work_plane {
                ui.label(t("Work Plane"));
                ui.label(label.as_str());
                ui.end_row();
            }
            let along = match &node.operation {
                Operation::Point { host: Host::Curve { location, .. }, .. } => Some(measured(location.measurement)),
                Operation::Point { host: Host::Edge { .. }, .. } => Some((msg!("Normalized Curve Parameter"), Holds::Number)),
                _ => None,
            };
            if let Some((label, kind)) = along
                && let Some(e) = expr_mut(node, Field::Along)
                && number_row(ui, label, e, values, true)
            {
                asked = Some(associating(id, Field::Along, kind, label, formula(e), params));
            }
            if let Some(angles) = angles {
                for (label, a) in [(msg!("Rotation X"), angles[0]), (msg!("Rotation Y"), angles[1]), (msg!("Rotation Z"), angles[2])] {
                    ui.label(t(label));
                    ui.label(format!("{:.2}°", if a.abs() < 0.005 { 0.0 } else { a }));
                    ui.end_row();
                }
            }
        });
    }
    let mut problem = None;
    group(ui, msg!("Adaptive Component"), |ui| {
        let mut chosen = kind;
        let kinds = [
            (PointType::Reference, msg!("Reference Point")),
            (PointType::Placement, msg!("Placement Point (Adaptive)")),
            (PointType::ShapeHandle, msg!("Shape Handle Point (Adaptive)")),
        ];
        if pick(ui, msg!("Point"), &mut chosen, &kinds)
            && let Err(e) = d.set_point_type(id, chosen)
        {
            problem = Some(e);
        }
        if let Some(number) = d.number(id) {
            let mut n = number;
            let count = d.next_number() - 1;
            let l = ui.label(t("Number"));
            ui.add(DragValue::new(&mut n).range(1..=count.max(1))).labelled_by(l.id);
            ui.end_row();
            if n != number {
                let _ = d.set_number(id, n);
            }
        }
        let Some(node) = d.nodes.iter_mut().find(|n| n.id == id) else { return };
        let look = node.look();
        if matches!(node.operation, Operation::Point { host: Host::Adaptive { .. }, .. }) {
            let mut number = look.number;
            if pick(ui, msg!("Show Placement Number"), &mut number, &SHOWS) {
                node.appearance.show_number = Some(number);
            }
        }
        if let Operation::Point { host: Host::Adaptive { orients, .. } | Host::Handle { orients, .. }, .. } = &mut node.operation {
            pick(ui, msg!("Orients to"), orients, &ORIENTS);
        }
        if let Operation::Point { host: Host::Handle { constrained, .. }, .. } = &mut node.operation {
            pick(ui, msg!("Constrained"), constrained, &CONSTRAINTS);
        }
    });
    let node = d.nodes.iter_mut().find(|n| n.id == id)?;
    let look = node.look();
    group(ui, msg!("Graphics"), |ui| {
        let mut planes = look.planes;
        if pick(ui, msg!("Show Reference Planes"), &mut planes, &SHOWS) {
            node.appearance.show_planes = Some(planes);
        }
        if yes_no_row(ui, msg!("Visible"), &mut node.appearance.visible, look.visible, values) {
            let now = match &node.appearance.visible {
                Some(Expr::Formula(f)) => Some(f.clone()),
                _ => None,
            };
            asked = Some(associating(id, Field::Visible, Holds::YesNo, msg!("Visible"), now.as_deref(), params));
        }
    });
    group(ui, msg!("Identity Data"), |ui| {
        let l = ui.label(t("Name"));
        ui.add(TextEdit::singleline(&mut node.name).desired_width(f32::INFINITY)).labelled_by(l.id);
        ui.end_row();
    });
    if let Some(e) = problem {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
    asked
}

/// `a`'s choice made: the row tied to the parameter chosen or, with <None>, untied, keeping what it came to.
fn tie(d: &mut Definition, a: &Associating, values: &BTreeMap<String, Val>) {
    let Some(node) = d.nodes.iter_mut().find(|n| n.id == a.id) else { return };
    if a.field == Field::Material {
        let now = &node.appearance.material;
        node.appearance.material = match (&a.chosen, values.get(now.as_str())) {
            (Some(p), _) => p.clone(),
            (None, Some(Val::Text(m))) => m.clone(),
            (None, _) => now.clone(),
        };
        return;
    }
    let Some(e) = expr_mut(node, a.field) else { return };
    *e = match &a.chosen {
        Some(p) => Expr::Formula(p.clone()),
        None => Expr::Number(worth(e, values).unwrap_or(0.)),
    };
}

/// What the row `a` is open on holds now, as the Family Types dialog types a value: a new parameter starts from it.
fn value_now(d: &mut Definition, a: &Associating, values: &BTreeMap<String, Val>) -> String {
    let Some(node) = d.nodes.iter_mut().find(|n| n.id == a.id) else { return String::new() };
    if a.field == Field::Material {
        return node.appearance.material.clone();
    }
    expr_mut(node, a.field).and_then(|e| worth(e, values)).map(crate::editor::number_text).unwrap_or_default()
}

/// Revit's Associate Family Parameter for `a`: the family's parameters holding what its row does (of `params`), <None>, and New
/// Parameter, which asks a name and whether it is a type's or each instance's (Parameter Properties) and adds it. OK ties the
/// row to the one chosen, or with <None> unties it. Whether it is done, and a parameter to add.
pub(crate) fn associate(
    ctx: &egui::Context,
    a: &mut Associating,
    d: &mut dyn Tied,
    params: &[(String, Holds)],
    values: &BTreeMap<String, Val>,
) -> (bool, Option<NewParameter>) {
    let (mut done, mut add, mut ok, mut cancel) = (false, None, false, false);
    let title = if a.new.is_some() { t("Parameter Properties") } else { t("Associate Family Parameter") };
    let r = crate::popup::Modal::new(egui::Id::new("associate-family-parameter"), title).width(380.0).show(ctx, |ui| {
        crate::popup::body(ui, |ui| {
            if let Some((name, instance)) = &mut a.new {
                egui::Grid::new("parameter-properties").num_columns(2).spacing(egui::vec2(12.0, 6.0)).show(ui, |ui| {
                    let l = ui.label(t("Name:"));
                    ui.add_sized([220.0, ui.spacing().interact_size.y], TextEdit::singleline(name)).labelled_by(l.id);
                    ui.end_row();
                    ui.label("");
                    ui.horizontal(|ui| {
                        ui.radio_value(instance, false, t("Type"));
                        ui.radio_value(instance, true, t("Instance"));
                    });
                    ui.end_row();
                    ui.label(t("Type of Parameter:"));
                    ui.label(t(holds(a.kind)));
                    ui.end_row();
                });
                let name = name.trim();
                if params.iter().any(|(p, _)| p == name) {
                    ui.colored_label(ui.visuals().error_fg_color, tf("The family has a parameter named {} already.", &[name]));
                }
            } else {
                ui.label(tf("Family parameter: {}", &[t(&a.label)]));
                ui.label(tf("Parameter type: {}", &[t(holds(a.kind))]));
                ui.add_space(6.0);
                ui.label(t("Existing family parameters of compatible type:"));
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    if ui.selectable_label(a.chosen.is_none(), t("<None>")).clicked() {
                        a.chosen = None;
                    }
                    for (name, _) in params.iter().filter(|(_, k)| *k == a.kind) {
                        if ui.selectable_label(a.chosen.as_ref() == Some(name), name.as_str()).clicked() {
                            a.chosen = Some(name.clone());
                        }
                    }
                });
                if ui.button(t("New Parameter")).clicked() {
                    a.new = Some((String::new(), false));
                }
            }
        });
        crate::popup::footer(ui, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                cancel = ui.button(t("Cancel")).clicked();
                ok = ui.button(t("OK")).clicked();
            });
        });
    });
    match a.new.take() {
        // Parameter Properties: OK adds the parameter, chosen; Cancel goes back to the list.
        Some((typed, instance)) => {
            let name = typed.trim().to_string();
            if ok && !name.is_empty() && !params.iter().any(|(p, _)| *p == name) {
                add = Some(NewParameter { name: name.clone(), kind: a.kind, instance, value: d.value_now(a, values) });
                a.chosen = Some(name);
            } else if !(cancel || r.should_close()) {
                a.new = Some((typed, instance));
            }
        }
        None if ok => {
            d.tie(a, values);
            done = true;
        }
        None => done = cancel || r.should_close(),
    }
    (done, add)
}

/// The parameter `e` names, when it is a formula.
fn formula(e: &Expr) -> Option<&str> {
    match e {
        Expr::Formula(f) => Some(f),
        Expr::Number(_) => None,
    }
}

/// One element's properties: its name, its relationship and what that takes (a point's offsets and rotations each with Revit's
/// "=" to tie it to one of `params`), and a form's appearance. Whether Delete was pressed, and Associate Family Parameter when
/// an "=" was.
pub(crate) fn node_properties(
    ui: &mut Ui,
    node: &mut adaptive::Node,
    values: &BTreeMap<String, Val>,
    params: &[(String, Holds)],
) -> (bool, Option<Associating>) {
    let mut delete = false;
    let mut asked = None;
    ui.horizontal(|ui| {
        ui.label(t("Name"));
        ui.text_edit_singleline(&mut node.name);
        if ui.small_button(t("Delete")).clicked() {
            delete = true;
        }
    });
    ui.label(tf("Persistent ID: {}", &[&node.id]));
    let mut v = serde_json::to_value(&node.operation).unwrap_or(Value::Null);
    let mut kind = v["kind"].as_str().unwrap_or("point").to_string();
    let before = kind.clone();
    choice(ui, "Operation", &mut kind, &["point", "line", "polyline", "bezier", "intersection", "patch", "extrude", "loft"]);
    if kind != before {
        v = operation(&kind);
    }
    if kind == "point" {
        let mut kind = v["host"]["type"].as_str().unwrap_or("frame").to_string();
        let before = kind.clone();
        choice(ui, "Host relationship", &mut kind, &["world", "adaptive", "handle", "frame", "plane", "curve", "face", "edge", "vertex"]);
        if kind != before {
            v["host"] = host(&kind);
        }
    }
    let mut transform = None;
    let mut hosted_frame = None;
    // Shown as Revit's fields by point_properties, not as raw values here.
    let mut revit_fields = Vec::new();
    if kind == "point" {
        if let Ok(mut typed) = serde_json::from_value::<Operation>(v.clone()) {
            if let Operation::Point { host, transform: t } = &mut typed {
                // A point on the work plane, adaptive or not, has the family's axes, as in Revit: only where it is.
                if let Host::Adaptive { frame: f, .. } | Host::Handle { frame: f, .. } | Host::World { frame: f, .. } = host {
                    position(ui, f);
                }
                let rows = [
                    (Field::Offset(0), msg!("OffsetX (mm)"), Holds::Length),
                    (Field::Offset(1), msg!("OffsetY (mm)"), Holds::Length),
                    (Field::Offset(2), msg!("OffsetZ (mm)"), Holds::Length),
                    (Field::Rotation(0), msg!("RotX (degrees)"), Holds::Angle),
                    (Field::Rotation(1), msg!("RotY (degrees)"), Holds::Angle),
                    (Field::Rotation(2), msg!("RotZ (degrees)"), Holds::Angle),
                ];
                egui::Grid::new("point-transform").num_columns(2).show(ui, |ui| {
                    for (field, label, kind) in rows {
                        let e = match field {
                            Field::Offset(i) => t.offset.get_mut(i),
                            Field::Rotation(i) => t.rotation.get_mut(i),
                            _ => None,
                        };
                        if let Some(e) = e
                            && number_row(ui, label, e, values, true)
                        {
                            asked = Some(associating(&node.id, field, kind, label, formula(e), params));
                        }
                    }
                });
                transform = serde_json::to_value(t).ok();
            }
            v = serde_json::to_value(typed).unwrap_or(v);
        }
        if let Some(map) = v.as_object_mut() {
            map.remove("transform");
        }
        // A world point's `plane` is its Work Plane, shown by point_properties; a hosted point's `plane` (xy, xz, yz) is its
        // host's own, picked here.
        let world = v["host"]["type"] == "world";
        if let Some(map) = v["host"].as_object_mut() {
            hosted_frame = map.remove("frame");
            for key in ["orients", "constrained", if world { "plane" } else { "" }] {
                if let Some(v) = map.remove(key) {
                    revit_fields.push((key, v));
                }
            }
        }
    }
    if let Some(map) = v.as_object_mut() {
        map.remove("kind");
    }
    // Keep discriminator visible through the selectors, not an unrestricted duplicate field.
    properties(ui, "properties", &mut v);
    v["kind"] = json!(kind);
    if let Some(frame) = hosted_frame {
        v["host"]["frame"] = frame;
    }
    for (key, value) in revit_fields {
        v["host"][key] = value;
    }
    if let Some(transform) = transform {
        v["transform"] = transform;
    }
    match serde_json::from_value(v) {
        Ok(op) => node.operation = op,
        Err(e) => {
            ui.colored_label(ui.visuals().error_fg_color, e.to_string());
        }
    }
    if node.is_form() {
        ui.checkbox(&mut node.appearance.void, t("Void"));
    }
    (delete, asked)
}
