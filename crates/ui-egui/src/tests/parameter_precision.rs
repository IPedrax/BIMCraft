use super::*;

fn press(h: &mut Harness<BimApp>, label: &str) {
    h.query_all_by_label(label).last().unwrap_or_else(|| panic!("no {label}")).click();
    h.run();
}

const FAMILY: &str = r#"
family {"name":"Precision Block","category":"generic","parameters":[{"name":"Gap","value":3},{"name":"Width","value":55},{"name":"Height","value":500,"instance":true}],"types":{"A":{}},"forms":[{"profile":[[0,0],["Width",0],["Width",500],[0,500]],"to":"Height"}]}
component {"family":"Precision Block","at":[3000,2000]}
"#;

fn family_dialog(units: &str) -> Harness<'static, BimApp> {
    let mut h = app(&format!("{FAMILY}\nunits {{\"length\":\"{units}\"}}"));
    let id = ids(&h, "component")[0];
    h.state_mut().selection = vec![id];
    h.run();
    press(&mut h, "Edit Type");
    h
}

fn family_value(h: &Harness<BimApp>, name: &str) -> f64 {
    match h.state().session.model.families["Precision Block"].values("A", &Default::default()).unwrap()[name] {
        bimcraft_core::family::Val::Number(x) => x,
        _ => panic!("expected a numeric parameter"),
    }
}

#[test]
fn precision_family_noop_apply_keeps_stored_lengths() {
    for units in ["m", "mm", "ft", "ft-in", "in"] {
        let mut h = family_dialog(units);
        press(&mut h, "Apply");
        assert_eq!(family_value(&h, "Gap"), 3.0, "{units}: untouched Gap");
        assert_eq!(family_value(&h, "Width"), 55.0, "{units}: untouched Width");
    }
}

#[test]
fn precision_fractional_lengths_survive_unit_changes_noop_history_and_reopen() {
    let original = 3.000123456789;
    let mut h = app(&FAMILY.replace("\"value\":3}", "\"value\":3.000123456789}"));
    h.state_mut().selection = ids(&h, "component");
    for unit in ["ft-in", "m", "in", "mm"] {
        h.state_mut().session.execute("units", &json!({"length":unit})).unwrap();
        h.run();
        let before = Arc::clone(&h.state().session.model);
        press(&mut h, "Edit Type");
        press(&mut h, "Apply");
        press(&mut h, "OK");
        assert!(Arc::ptr_eq(&before, &h.state().session.model), "no-op in {unit}");
        press(&mut h, "Edit Type");
        type_into(&mut h, "Width", "1 1/8\"");
        press(&mut h, "Apply");
        press(&mut h, "Apply");
        press(&mut h, "OK");
        assert_eq!(family_value(&h, "Gap"), original);
        assert_eq!(family_value(&h, "Width"), 28.575);
        h.state_mut().session.execute("undo", &json!({})).unwrap();
        assert_eq!(family_value(&h, "Width"), 55.0, "one Undo in {unit}");
        h.state_mut().session.execute("redo", &json!({})).unwrap();
        assert_eq!(family_value(&h, "Width"), 28.575);
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("fractional.bimc");
        h.state_mut().session.execute("save", &json!({"path":path})).unwrap();
        let mut reopened = Session::new();
        reopened.execute("open", &json!({"path":path})).unwrap();
        let values = reopened.model.families["Precision Block"].values("A", &Default::default()).unwrap();
        assert_eq!(values["Gap"], bimcraft_core::family::Val::Number(original));
        assert_eq!(values["Width"], bimcraft_core::family::Val::Number(28.575));
        h.state_mut().session.execute("undo", &json!({})).unwrap();
        assert_eq!(family_value(&h, "Width"), 55.0);
    }
}

#[test]
fn precision_instance_units_and_angle_keep_upstream_display_and_parsing() {
    use egui_kittest::kittest::NodeT;
    let mut h = app(&format!("{FAMILY}\nunits {{\"length\":\"m\"}}"));
    let id = ids(&h, "component")[0];
    h.state_mut().selection = vec![id];
    h.run();
    assert_eq!(h.get_by_role_and_label(Role::TextInput, "Height").accesskit_node().value(), Some("0.50 m".into()));
    assert_eq!(h.get_by_role_and_label(Role::TextInput, "Rotation").accesskit_node().value(), Some("0°".into()));
    type_into(&mut h, "Rotation", "22.5°");
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, id, "angle"), json!(22.5));
    let original_params = field(&h, id, "params");
    type_into(&mut h, "Height", "1/8\"");
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, id, "params")["Height"], json!(3.175));
    let before = Arc::clone(&h.state().session.model);
    press(&mut h, "Apply");
    assert!(Arc::ptr_eq(&before, &h.state().session.model));
    h.state_mut().session.execute("undo", &json!({})).unwrap();
    assert_eq!(field(&h, id, "params"), original_params);
    assert_eq!(field(&h, id, "angle"), json!(22.5));
    h.state_mut().session.execute("redo", &json!({})).unwrap();
    assert_eq!(field(&h, id, "params")["Height"], json!(3.175));
}

#[test]
fn precision_family_typed_value_survives_repeat_apply() {
    let mut h = family_dialog("m");
    type_into(&mut h, "Gap", "0.003");
    press(&mut h, "Apply");
    assert_eq!(family_value(&h, "Gap"), 3.0, "first Apply");
    press(&mut h, "Apply");
    assert_eq!(family_value(&h, "Gap"), 3.0, "second Apply");
}

#[test]
fn precision_wall_noop_apply_keeps_total_and_layers() {
    let mut h = app(
        r#"type {"category":"wall","name":"Precision Wall","layers":[{"function":"finish","thickness":3},{"function":"structure","thickness":55}]}
wall {"points":[[0,0],[4000,0]],"type":"Precision Wall"}
units {"length":"m"}"#,
    );
    h.state_mut().selection = ids(&h, "wall");
    h.run();
    press(&mut h, "Edit Type");
    press(&mut h, "Apply");
    let wall = &h.state().session.model.types.wall["Precision Wall"];
    assert_eq!(wall.thickness, 58.0);
    assert_eq!(wall.layers.iter().map(|l| l.thickness).collect::<Vec<_>>(), vec![3.0, 55.0]);
}

#[test]
fn precision_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_PRECISION_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (profile, name) in [(crate::style::Profile::Rev, "rev"), (crate::style::Profile::BimCraft, "bimcraft")] {
        for (theme, light_canvas, look) in
            [(egui::Theme::Light, false, "light"), (egui::Theme::Dark, false, "dark"), (egui::Theme::Dark, true, "light-canvas")]
        {
            let mut h = harness(&format!("{FAMILY}\nunits {{\"length\":\"m\"}}"), 0.25, profile);
            h.state_mut().selection = ids(&h, "component");
            h.set_size(vec2(1600.0, 1000.0));
            h.ctx.set_theme(theme);
            h.state_mut().theme_wanted =
                Some(if theme == egui::Theme::Dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
            h.state_mut().light_canvas = light_canvas;
            h.run();
            press(&mut h, "Edit Type");
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("{name}-{look}.png"))).unwrap();
        }
    }
}

#[test]
fn precision_family_other_field_cancel_history_and_reopen() {
    let mut h = family_dialog("m");
    type_into(&mut h, "Width", "0.125");
    press(&mut h, "Cancel");
    assert_eq!(family_value(&h, "Width"), 55.0);
    press(&mut h, "Edit Type");
    type_into(&mut h, "Width", "125 mm");
    press(&mut h, "Apply");
    assert_eq!(family_value(&h, "Gap"), 3.0);
    assert_eq!(family_value(&h, "Width"), 125.0);
    press(&mut h, "OK");
    h.state_mut().run("undo", json!({}));
    assert_eq!(family_value(&h, "Gap"), 3.0);
    assert_eq!(family_value(&h, "Width"), 55.0, "one Undo reverses the edit despite repeated Apply/OK");
    h.state_mut().run("redo", json!({}));
    assert_eq!(family_value(&h, "Width"), 125.0);
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("precision.bimc");
    h.state_mut().session.execute("save", &json!({"path":path})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("open", &json!({"path":path})).unwrap();
    assert_eq!(reopened.model.families["Precision Block"].types["A"]["Gap"], json!(3.0));
    assert_eq!(reopened.model.families["Precision Block"].types["A"]["Width"], json!(125.0));
}

#[test]
fn precision_untouched_family_value_keeps_submicron_source_precision() {
    let mut h = app(&format!("{}\nunits {{\"length\":\"m\"}}", FAMILY.replace("\"value\":3}", "\"value\":3.000123456789}")));
    h.state_mut().selection = ids(&h, "component");
    h.run();
    press(&mut h, "Edit Type");
    type_into(&mut h, "Width", "60 mm");
    press(&mut h, "Apply");
    assert_eq!(family_value(&h, "Gap"), 3.000123456789);
    press(&mut h, "Duplicate…");
    type_into(&mut h, "New type", "B");
    press(&mut h, "Create");
    press(&mut h, "OK");
    assert_eq!(h.state().session.model.families["Precision Block"].types["B"]["Gap"], json!(3.000123456789));
}

#[test]
fn precision_instance_fields_are_change_only_controls() {
    let mut h = family_dialog("m");
    press(&mut h, "Cancel");
    let id = ids(&h, "component")[0];
    type_into(&mut h, "Height", "0.055");
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, id, "params")["Height"], json!(55.0));
    press(&mut h, "Apply");
    assert_eq!(field(&h, id, "params")["Height"], json!(55.0));
    type_into(&mut h, "Height", "3 mm");
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, id, "params")["Height"], json!(3.0));
    type_into(&mut h, "Height", "125 mm");
    key(&mut h, Key::Escape);
    assert_eq!(field(&h, id, "params")["Height"], json!(3.0));
}

fn layer_text(h: &mut Harness<BimApp>, index: usize, text: &str) {
    use egui_kittest::kittest::NodeT;
    let id = h.query_all_by_role_and_label(Role::TextInput, "Thickness").nth(index).unwrap().accesskit_node().id();
    h.get_by(|n| n.id() == id).focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by(|n| n.id() == id).type_text(text);
    h.run();
}

#[test]
fn precision_wall_layer_edit_keeps_other_layers_and_repeated_apply() {
    let mut h = app(
        r#"type {"category":"wall","name":"Precision Wall","layers":[{"function":"finish","thickness":3.000123456789},{"function":"structure","thickness":55}]}
wall {"points":[[0,0],[4000,0]],"type":"Precision Wall"}
units {"length":"m"}"#,
    );
    h.state_mut().selection = ids(&h, "wall");
    h.run();
    press(&mut h, "Edit Type");
    layer_text(&mut h, 1, "0.125");
    press(&mut h, "Apply");
    let wall = &h.state().session.model.types.wall["Precision Wall"];
    assert_eq!(wall.layers[0].thickness, 3.000123456789);
    assert_eq!(wall.layers[1].thickness, 125.0);
    press(&mut h, "Apply");
    let wall = &h.state().session.model.types.wall["Precision Wall"];
    assert_eq!(wall.layers[0].thickness, 3.000123456789);
    assert_eq!(wall.layers[1].thickness, 125.0);
    press(&mut h, "OK");
    h.state_mut().run("undo", json!({}));
    assert_eq!(h.state().session.model.types.wall["Precision Wall"].layers[1].thickness, 55.0);
    h.state_mut().run("redo", json!({}));
    press(&mut h, "Edit Type");
    layer_text(&mut h, 0, "3 mm");
    press(&mut h, "OK");
    assert_eq!(h.state().session.model.types.wall["Precision Wall"].thickness, 128.0);
}

#[test]
fn precision_room_type_offsets_survive_other_field_edit() {
    let mut h = app(&format!(
        r#"{BOX}
type {{"category":"room","name":"Precise Room","floor":"Generic - 200mm","floor_offset":3.000123456789,"ceiling":"Generic - 20mm","ceiling_offset":2500.055}}
room {{"point":[1000,1000],"type":"Precise Room"}}
units {{"length":"m"}}"#
    ));
    h.state_mut().selection = ids(&h, "room");
    h.run();
    press(&mut h, "Edit Type");
    type_into(&mut h, "Ceiling Height", "2.75 m");
    press(&mut h, "Apply");
    let room = &h.state().session.model.types.room["Precise Room"];
    assert_eq!(room.floor_offset, 3.000123456789);
    assert_eq!(room.ceiling_offset, 2750.0);
    press(&mut h, "Apply");
    assert_eq!(h.state().session.model.types.room["Precise Room"].floor_offset, 3.000123456789);
}

#[test]
fn precision_typed_lengths_are_exact_in_metric_and_imperial_projects() {
    for (unit, typed, expected) in [
        ("m", "0.055", 55.0),
        ("mm", "3.125", 3.125),
        ("cm", "0.3", 3.0),
        ("ft", "0.125", 38.1),
        ("in", "1/8\"", 3.175),
        ("ft-in", "0'-0 1/8\"", 3.175),
    ] {
        let mut h = family_dialog(unit);
        type_into(&mut h, "Gap", typed);
        press(&mut h, "Apply");
        press(&mut h, "Apply");
        assert_eq!(family_value(&h, "Gap"), expected, "{unit}: {typed}");
        assert_eq!(family_value(&h, "Width"), 55.0, "untouched in {unit}");
    }
}

#[test]
fn precision_invalid_type_input_does_not_commit_other_fields() {
    let mut h = family_dialog("m");
    let before = Arc::clone(&h.state().session.model);
    type_into(&mut h, "Gap", "not a length");
    type_into(&mut h, "Width", "125 mm");
    press(&mut h, "Apply");
    assert!(h.state().family_dialog.is_some());
    assert!(Arc::ptr_eq(&before, &h.state().session.model));
    press(&mut h, "Cancel");
    assert_eq!(family_value(&h, "Width"), 55.0);
}

#[test]
fn precision_layer_reorder_and_duplicate_keep_original_values() {
    let mut h = app(
        r#"type {"category":"wall","name":"Precision Wall","layers":[{"function":"finish","thickness":3.000123456789},{"function":"structure","thickness":55}]}
wall {"points":[[0,0],[4000,0]],"type":"Precision Wall"}
units {"length":"m"}"#,
    );
    h.state_mut().selection = ids(&h, "wall");
    h.run();
    press(&mut h, "Edit Type");
    h.query_all_by_label("In").next().unwrap().click();
    h.run();
    press(&mut h, "Apply");
    let layers = &h.state().session.model.types.wall["Precision Wall"].layers;
    assert_eq!(layers[0].thickness, 55.0);
    assert_eq!(layers[1].thickness, 3.000123456789);
    press(&mut h, "Duplicate…");
    type_into(&mut h, "New type", "Precision Copy");
    press(&mut h, "Create");
    press(&mut h, "OK");
    let layers = &h.state().session.model.types.wall["Precision Copy"].layers;
    assert_eq!(layers[0].thickness, 55.0);
    assert_eq!(layers[1].thickness, 3.000123456789);
}

#[test]
fn precision_project_type_parameter_is_a_change_only_control() {
    let mut h = app(r#"wall {"points":[[0,0],[4000,0]]}
parameter {"name":"Clearance","kind":"length","scope":"type","categories":["wall"]}
type_parameter {"category":"wall","type":"Generic - 200mm","values":{"Clearance":3.000123456789}}
units {"length":"m"}"#);
    h.state_mut().selection = ids(&h, "wall");
    h.run();
    press(&mut h, "Edit Type");
    press(&mut h, "Apply");
    let read = |h: &Harness<BimApp>| h.state().session.model.parameters["Clearance"].type_values.values().next().unwrap().as_f64().unwrap();
    assert_eq!(read(&h), 3.000123456789);
    type_into(&mut h, "Clearance", "0.055");
    press(&mut h, "Apply");
    press(&mut h, "Apply");
    assert_eq!(read(&h), 55.0);
}

#[test]
fn precision_project_decimals_keep_their_display_and_untouched_values() {
    use egui_kittest::kittest::NodeT;
    for decimals in [2, 3, 4, 5] {
        let mut h = app(&format!("{FAMILY}\nunits {{\"length\":\"m\",\"decimals\":{decimals}}}"));
        h.state_mut().selection = ids(&h, "component");
        h.run();
        press(&mut h, "Edit Type");
        let expected = h.state().session.model.field(55.0);
        let expected_precision = h.state().session.model.decimals;
        assert_eq!(h.get_by_role_and_label(Role::TextInput, "Width").accesskit_node().value(), Some(expected));
        type_into(&mut h, "Gap", "4 mm");
        press(&mut h, "Apply");
        press(&mut h, "Apply");
        assert_eq!(family_value(&h, "Gap"), 4.0);
        assert_eq!(family_value(&h, "Width"), 55.0);
        assert_eq!(h.state().session.model.decimals, expected_precision);
    }
}
