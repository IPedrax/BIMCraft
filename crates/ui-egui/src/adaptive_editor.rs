//! Direct editing of the canonical adaptive dependency model, without a graph canvas: the Properties beside the modelling view
//! (`adaptive_view.rs`), and the list of every element, the test placement and what the evaluation says.
use crate::lang::{t, tf};
use bimcraft_core::adaptive::{self, Definition, Evaluated, Frame, Host, Operation, Placement};
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
            "measurement" => choice(ui, key, s, &["parameter", "normalized_length", "distance"]),
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
        "adaptive" => json!({"type":"adaptive","number":1,"frame":Frame::default()}),
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
            if let Operation::Point { host: Host::Adaptive { number, frame: default }, .. } = &node.operation {
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
        choice(ui, "Host relationship", &mut kind, &["world", "adaptive", "frame", "plane", "curve", "face", "edge", "vertex"]);
        if kind != before {
            v["host"] = host(&kind);
        }
    }
    let mut transform = None;
    let mut hosted_frame = None;
    if kind == "point" {
        if let Ok(mut typed) = serde_json::from_value::<Operation>(v.clone()) {
            if let Operation::Point { host, transform: t } = &mut typed {
                if let Host::Adaptive { frame: f, .. } | Host::World { frame: f } = host {
                    frame(ui, f);
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
