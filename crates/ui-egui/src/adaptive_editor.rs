//! Direct editing of the canonical adaptive dependency model, without a graph canvas: the Properties beside the modelling view
//! (`adaptive_view.rs`), and the list of every element, the test placement and what the evaluation says.
use crate::lang::{t, tf};
use bimcraft_core::adaptive::{self, Constrained, Definition, Evaluated, Frame, Host, Operation, OrientsTo, Placement, PointType, Show};
use bimcraft_core::family::{Expr, Val};
use bimcraft_i18n::msg;
use egui::{ComboBox, DragValue, RichText, Ui};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn expression(ui: &mut Ui, label: &str, value: &mut Expr) {
    let mut text = match value {
        Expr::Number(n) => n.to_string(),
        Expr::Formula(s) => s.clone(),
    };
    ui.horizontal(|ui| {
        ui.label(t(label));
        if ui.text_edit_singleline(&mut text).changed() {
            *value = text.parse().map(Expr::Number).unwrap_or(Expr::Formula(text));
        }
    });
}
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
pub(crate) fn show(ui: &mut Ui, definition: &mut Option<Definition>, values: &BTreeMap<String, Val>) {
    if definition.is_none() {
        ui.label(t("An adaptive family's points are placed where each instance is clicked, as Revit's Generic Model Adaptive."));
        if ui.button(t("Make this family adaptive")).clicked() {
            *definition = Some(Definition::default());
        }
        if ui.button(t("Create four-point adaptive panel")).clicked() {
            *definition = Some(adaptive::four_point_panel());
        }
        return;
    }
    let Some(d) = definition else { return };
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
                if node_properties(ui, node) {
                    remove = Some(i);
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

/// A point's properties as Revit's palette groups them: Constraints (how it is turned, as `values` evaluate it), Adaptive
/// Component (Point, Number, Show Placement Number, Orients to, Constrained), Graphics (Show Reference Planes, Visible) and
/// Identity Data (Name). Nothing for an element that is not a point.
pub(crate) fn point_properties(ui: &mut Ui, d: &mut Definition, id: &str, values: &BTreeMap<String, Val>) {
    use crate::editor::document::group;
    let Some(kind) = d.point_type(id) else { return };
    // Its axes where it is now (an instance replayed may turn them): the angles about X, then Y, then Z from the family's.
    if let Some(Evaluated::Point { frame }) = d.evaluate(values, &d.preview).values.get(id) {
        let angles = frame.rotations();
        let work_plane = d
            .work_plane(id)
            .and_then(|w| crate::editor::document::work_planes(d).into_iter().find(|(p, _)| *p == w).map(|(_, label)| label));
        group(ui, msg!("Constraints"), |ui| {
            if let Some(label) = &work_plane {
                ui.label(t("Work Plane"));
                ui.label(label.as_str());
                ui.end_row();
            }
            for (label, a) in [(msg!("Rotation X"), angles[0]), (msg!("Rotation Y"), angles[1]), (msg!("Rotation Z"), angles[2])] {
                ui.label(t(label));
                ui.label(format!("{:.2}°", if a.abs() < 0.005 { 0.0 } else { a }));
                ui.end_row();
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
    let Some(node) = d.nodes.iter_mut().find(|n| n.id == id) else { return };
    let look = node.look();
    group(ui, msg!("Graphics"), |ui| {
        let mut planes = look.planes;
        if pick(ui, msg!("Show Reference Planes"), &mut planes, &SHOWS) {
            node.appearance.show_planes = Some(planes);
        }
        let l = ui.label(t("Visible"));
        let mut visible = look.visible;
        if ui.checkbox(&mut visible, "").labelled_by(l.id).changed() {
            node.appearance.visible = Some(Expr::Number(if visible { 1. } else { 0. }));
        }
        ui.end_row();
    });
    group(ui, msg!("Identity Data"), |ui| {
        let l = ui.label(t("Name"));
        ui.add(egui::TextEdit::singleline(&mut node.name).desired_width(f32::INFINITY)).labelled_by(l.id);
        ui.end_row();
    });
    if let Some(e) = problem {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
}

/// One element's properties: its name, its relationship and what that takes, and a form's appearance. Whether Delete was
/// pressed.
pub(crate) fn node_properties(ui: &mut Ui, node: &mut adaptive::Node) -> bool {
    let mut delete = false;
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
                for i in 0..3 {
                    expression(ui, [msg!("OffsetX (mm)"), msg!("OffsetY (mm)"), msg!("OffsetZ (mm)")][i], &mut t.offset[i]);
                }
                for i in 0..3 {
                    expression(ui, [msg!("RotX (degrees)"), msg!("RotY (degrees)"), msg!("RotZ (degrees)")][i], &mut t.rotation[i]);
                }
                transform = serde_json::to_value(t).ok();
            }
            v = serde_json::to_value(typed).unwrap_or(v);
        }
        if let Some(map) = v.as_object_mut() {
            map.remove("transform");
        }
        if let Some(map) = v["host"].as_object_mut() {
            hosted_frame = map.remove("frame");
            for key in ["orients", "constrained", "datum"] {
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
        if !node.appearance.void {
            ui.horizontal(|ui| {
                ui.label(t("Material or material parameter"));
                ui.text_edit_singleline(&mut node.appearance.material);
            });
        }
        let mut visible = node.appearance.visible.clone().unwrap_or(Expr::Number(1.));
        expression(ui, msg!("Visible (formula)"), &mut visible);
        node.appearance.visible = Some(visible);
    }
    delete
}
