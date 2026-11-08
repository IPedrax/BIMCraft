//! F7a: a family's dimensions in its plans and elevations. Aligned Dimension (DI) between reference planes, the Options Bar's
//! Label (T19) with Create Parameter… and the over-constrained refusal, the padlocks and EQ of a selected dimension, Align (AL)
//! and its padlock, a labelled plane dragged changing its parameter, Family Types' Lock (T21), "Constraints are not
//! satisfied" with Remove Constraints or Cancel, and a reload whose dimensions measure to nothing (T14).
use super::family_views::{click_family, editor, open, view};
use super::*;
use bimcraft_core::family::views::Which;
use bimcraft_core::family::{DimRef, Extent};
use egui_kittest::kittest::NodeT;
use std::collections::BTreeMap;

const LEFT: &str = "11111111-1111-4111-8111-000000000001";
const RIGHT: &str = "11111111-1111-4111-8111-000000000002";
const BAR: &str = "22222222-2222-4222-8222-000000000001";

/// A box family: Width 1000 (Small 600, Large 1400), Left and Right planes at ±500, and a bar along x on Center
/// (Left/Right) from -400 to 400, 200 deep; `dims`, its dimensions as JSON.
fn family(dims: &str) -> String {
    format!(
        r#"family {{"name": "Box", "category": "generic", "parameters": [{{"name": "Width", "value": 1000}}], "types": {{"Small": {{"Width": 600}}, "Large": {{"Width": 1400}}}}, "ref_planes": [{{"guid": "{LEFT}", "name": "Left", "start": [-500, -2000, 0], "end": [-500, 2000, 0], "up": [0, 0, 1]}}, {{"guid": "{RIGHT}", "name": "Right", "start": [500, -2000, 0], "end": [500, 2000, 0], "up": [0, 0, 1]}}], "forms": [{{"guid": "{BAR}", "host": {{"plane": "9e3c0a10-1f0b-4d8c-8a5e-0000000000a3"}}, "profile": [[-100, 0], [100, 0], [100, 50], [-100, 50]], "from": -400, "to": 400}}], "dimensions": [{dims}]}}"#
    )
}

const WIDTH: &str = r#"{"refs": [{"plane": "11111111-1111-4111-8111-000000000001"}, {"plane": "11111111-1111-4111-8111-000000000002"}], "at": [0, -700, 0], "label": "Width"}"#;
const EQ: &str = r#"{"refs": [{"plane": "11111111-1111-4111-8111-000000000001"}, {"plane": "9e3c0a10-1f0b-4d8c-8a5e-0000000000a3"}, {"plane": "11111111-1111-4111-8111-000000000002"}], "at": [0, -1000, 0]}"#;

fn g(s: &str) -> bimcraft_core::ids::Guid {
    serde_json::from_value(json!(s)).unwrap()
}

/// The box open in its document, its floor plan shown.
fn in_plan(dims: &str) -> Harness<'static, BimApp> {
    let (mut h, _) = open(&family(dims), "Box");
    view(&mut h, Some(Which::Plan));
    h
}

fn ed_mut<'a>(h: &'a mut Harness<'_, BimApp>) -> &'a mut crate::editor::FamilyEditor {
    h.state_mut().family_editor.as_mut().unwrap()
}

/// Where plane `name` is, along its normal, as the type previewed flexes the draft.
fn offset(h: &Harness<BimApp>, guid: &str) -> f64 {
    let ed = editor(h);
    let f = ed.draft_family().unwrap();
    let flexed = f.flexed(&ed.values()).unwrap();
    let p = flexed.planes.iter().find(|p| p.guid == g(guid)).unwrap();
    let z = p.frame().z;
    p.start[0] * z[0] + p.start[1] * z[1] + p.start[2] * z[2]
}

fn status(h: &Harness<BimApp>) -> String {
    h.state().status.text.clone()
}

/// A click `by` pixels from family point `p` on screen.
fn click_off(h: &mut Harness<BimApp>, p: [f64; 3], by: egui::Vec2) {
    let at = editor(h).views.screen_of(p).expect("a view shown") + by;
    click_at(h, at);
}

#[test]
fn aligned_dimension_picks_two_planes_and_a_label_from_the_options_bar_flexes_them() {
    let mut h = in_plan("");
    press(&mut h, "Aligned Dimension");
    click_family(&mut h, [-500.0, 600.0, 0.0]);
    click_family(&mut h, [500.0, 600.0, 0.0]);
    click_family(&mut h, [200.0, -700.0, 0.0]);
    let dims = editor(&h).dimensions.clone();
    assert_eq!(dims.len(), 1, "placed by a click on nothing");
    assert_eq!(dims[0].refs, vec![DimRef::Plane(g(LEFT)), DimRef::Plane(g(RIGHT))]);
    assert_eq!(dims[0].view, bimcraft_core::family::DimView::Plan);
    // Escape leaves the tool; a click on its line selects it, and the Options Bar labels it.
    key(&mut h, Key::Escape);
    assert!(editor(&h).views.dimensioning.is_none());
    click_family(&mut h, [200.0, -700.0, 0.0]);
    assert_eq!(editor(&h).selected_dimension(), Some(dims[0].guid));
    assert!(h.query_all_by_label("Modify | Dimensions").count() > 0, "its contextual tab");
    h.get_by_label("Label").click();
    h.run();
    press_near(&mut h, "Width", "Label");
    assert_eq!(editor(&h).dimensions[0].label.as_deref(), Some("Width"));
    // Flexed through its types: Large first (the types' order), then Small.
    assert!((offset(&h, RIGHT) - offset(&h, LEFT) - 1400.0).abs() < 1e-6);
    ed_mut(&mut h).preview_type("Small");
    h.run();
    assert!((offset(&h, RIGHT) - offset(&h, LEFT) - 600.0).abs() < 1e-6);
    // The draft loads into the project with it.
    press(&mut h, "Load into Project");
    press(&mut h, "Overwrite the existing version");
    assert_eq!(h.state().session.model.families["Box"].dimensions.len(), 1);
}

#[test]
fn create_parameter_labels_a_dimension_and_a_label_that_adds_nothing_is_over_constrained() {
    let mut h = in_plan(&format!("{WIDTH}, {}", WIDTH.replace("-700", "-1200").replace(r#", "label": "Width""#, "")));
    // The second dimension, Left to Right again: a label on it would say what Width says already.
    click_family(&mut h, [200.0, -1200.0, 0.0]);
    let second = editor(&h).dimensions[1].guid;
    assert_eq!(editor(&h).selected_dimension(), Some(second));
    h.get_by_label("Label").click();
    h.run();
    press_near(&mut h, "Create Parameter…", "Label");
    type_into(&mut h, "Name", "Span");
    press_near(&mut h, "OK", "Name");
    assert!(editor(&h).views.creating.is_some(), "refused, its dialog stays: {}", status(&h));
    assert!(h.query_by_label_contains("over-constrained").is_some(), "T19's words");
    assert!(editor(&h).dimensions[1].label.is_none() && !editor(&h).draft_family().unwrap().parameters.iter().any(|p| p.name == "Span"));
    // As a reporting parameter it measures and holds nothing: made, and its value is what it measures.
    pick(&mut h, Role::CheckBox, "Reporting Parameter");
    press_near(&mut h, "OK", "Name");
    assert!(editor(&h).views.creating.is_none());
    assert_eq!(editor(&h).dimensions[1].label.as_deref(), Some("Span"));
    let ed = editor(&h);
    assert_eq!(ed.values().get("Span"), Some(&bimcraft_core::family::Val::Number(1400.0)));
}

#[test]
fn a_selected_strings_padlocks_and_eq_constrain_it_and_one_too_many_is_refused() {
    let mut h = in_plan(EQ);
    click_family(&mut h, [-250.0, -1000.0, 0.0]);
    let d = editor(&h).dimensions[0].guid;
    assert_eq!(editor(&h).selected_dimension(), Some(d));
    // EQ, over the string: Left and Right go to ±500 about the centre (they are already).
    click_off(&mut h, [0.0, -1000.0, 0.0], vec2(0.0, -30.0));
    assert!(editor(&h).dimensions[0].equal, "EQ on");
    // The first segment's padlock, under it: locked at its length.
    click_off(&mut h, [-250.0, -1000.0, 0.0], vec2(0.0, 16.0));
    assert_eq!(editor(&h).dimensions[0].locked, vec![bimcraft_core::model::Lock { segment: 0, length: 500.0 }]);
    // The second's too would say again what EQ and the first say: refused.
    click_off(&mut h, [250.0, -1000.0, 0.0], vec2(0.0, 16.0));
    assert_eq!(editor(&h).dimensions[0].locked.len(), 1);
    assert!(status(&h).contains("over-constrained"), "{}", status(&h));
    // Unlocked again, and EQ off; then Delete takes it away.
    click_off(&mut h, [-250.0, -1000.0, 0.0], vec2(0.0, 16.0));
    click_off(&mut h, [0.0, -1000.0, 0.0], vec2(0.0, -30.0));
    assert!(editor(&h).dimensions[0].locked.is_empty() && !editor(&h).dimensions[0].equal);
    key(&mut h, Key::Delete);
    assert!(editor(&h).dimensions.is_empty());
}

#[test]
fn align_moves_an_extrusions_end_onto_a_plane_and_its_padlock_locks_them() {
    let mut h = in_plan("");
    shortcut(&mut h, "AL");
    assert!(editor(&h).views.aligning.is_some());
    click_family(&mut h, [500.0, 600.0, 0.0]);
    click_family(&mut h, [400.0, 0.0, 0.0]);
    let draft = editor(&h).draft_family().unwrap();
    assert_eq!(draft.forms[0].to, bimcraft_core::family::Expr::Number(500.0), "the bar's end onto Right");
    assert!(editor(&h).views.align_lock.is_some(), "its padlock, open");
    click_family(&mut h, [400.0, 0.0, 0.0]);
    let lock = editor(&h).dimensions.last().cloned().expect("locked");
    assert!(lock.alignment && lock.refs == vec![DimRef::Plane(g(RIGHT)), DimRef::Form(g(BAR), Extent::End)]);
    // Locked, the bar's end follows Right when Width flexes it.
    let d = editor(&h).draft_family().unwrap();
    let mut with_width = d.clone();
    with_width.dimensions.push(serde_json::from_str(WIDTH).unwrap());
    let v = with_width.values("Large", &BTreeMap::new()).unwrap();
    let flexed = with_width.flexed(&v).unwrap();
    let bimcraft_core::family::Expr::Number(to) = flexed.forms[0].to else { panic!("a number") };
    assert!((to - 700.0).abs() < 1e-6, "{to}");
    // Its Properties grey the end it sets.
    assert!(editor(&h).driven_ends(0)[1].is_some());
}

#[test]
fn dragging_a_labelled_plane_changes_its_parameter_and_a_locked_one_says_the_constraints_hold_it() {
    let mut h = in_plan(&format!("{WIDTH}, {}", EQ.replace("}]", r#"}], "equal": true"#)));
    // Large: Right at 700 (Left at -700: the least change from ±500).
    assert!((offset(&h, RIGHT) - 700.0).abs() < 1e-6);
    click_family(&mut h, [700.0, 600.0, 0.0]);
    assert_eq!(editor(&h).selected_plane(), Some(g(RIGHT)));
    let (a, b) = (editor(&h).views.screen_of([700.0, 600.0, 0.0]).unwrap(), editor(&h).views.screen_of([900.0, 600.0, 0.0]).unwrap());
    drag_screen(&mut h, a, b, PointerButton::Primary);
    let width = editor(&h).draft_family().unwrap().types["Large"]["Width"].as_f64().unwrap();
    assert!((width - 1800.0).abs() < 1.0, "Width follows the drag, EQ about the centre: {width}");
    assert!((offset(&h, LEFT) + 900.0).abs() < 1.0, "Left goes out as far, EQ");
    // Family Types' Lock locks it (T21): the drag is refused, "Constraints are not satisfied".
    press(&mut h, "Family Types");
    assert!(!h.get_by_label("Lock Width").accesskit_node().is_disabled());
    h.get_by_label("Lock Width").click();
    h.run();
    press(&mut h, "OK");
    assert!(editor(&h).dimensions[0].is_locked(0));
    let right = offset(&h, RIGHT);
    let (a, b) =
        (editor(&h).views.screen_of([right, 600.0, 0.0]).unwrap(), editor(&h).views.screen_of([right + 200.0, 600.0, 0.0]).unwrap());
    drag_screen(&mut h, a, b, PointerButton::Primary);
    assert!(h.query_by_label("Constraints are not satisfied").is_some(), "asked");
    press(&mut h, "Cancel");
    assert!(h.query_by_label("Constraints are not satisfied").is_none());
    assert_eq!(offset(&h, RIGHT), right, "as it was");
}

#[test]
fn a_type_its_dimensions_cant_hold_asks_and_remove_constraints_takes_the_label_off() {
    let mut h = in_plan(WIDTH);
    press(&mut h, "Family Types");
    type_into(&mut h, "Parameter Value Width", "0");
    press(&mut h, "Apply");
    assert!(h.query_by_label("Constraints are not satisfied").is_some());
    assert!(editor(&h).dimensions[0].label.is_some());
    press(&mut h, "Remove Constraints");
    assert!(h.query_by_label("Constraints are not satisfied").is_none());
    assert!(editor(&h).dimensions[0].label.is_none(), "unlabelled");
    assert!(editor(&h).dialog.is_some(), "Family Types stays open after Apply");
}

#[test]
fn a_reload_whose_dimensions_measure_to_nothing_asks_delete_dimensions_or_cancel() {
    let mut s = Session::new();
    s.script(&family(WIDTH)).unwrap();
    let mut h = Harness::builder().with_size([1600.0, 1000.0]).build_ui_state(|ui, app: &mut BimApp| app.show(ui), BimApp::new(s));
    h.run();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("Box.json");
    assert!(h.state_mut().run("family", json!({"name": "Box", "save": file.to_string_lossy()})).is_some());
    // The file's version has no Left: Width measures to nothing.
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    v["planes"].as_array_mut().unwrap().retain(|p| p["guid"] != json!(LEFT));
    v["types"]["Medium"] = json!({"Width": 900});
    std::fs::write(&file, v.to_string()).unwrap();
    // Cancel loads nothing.
    h.state_mut().load_family_file(&file);
    h.run();
    assert!(h.query_by_label("Invalid Dimensions").is_some());
    press(&mut h, "Cancel");
    assert!(h.state().session.model.families["Box"].planes.iter().any(|p| p.guid == g(LEFT)));
    // Delete Dimension(s), then Family Already Exists: loaded without it.
    h.state_mut().load_family_file(&file);
    h.run();
    press(&mut h, "Delete Dimension(s)");
    press(&mut h, "Overwrite the existing version");
    let f = &h.state().session.model.families["Box"];
    assert!(f.dimensions.is_empty() && f.types.contains_key("Medium"), "{:?}", f.dimensions);
}

/// A box's floor plan with Width and Depth labelled and EQ, Width selected (its padlocks shown), then the same flexed to its
/// other type, saved where `BIMCRAFT_F7_DUMP` says (only then).
#[test]
fn family_dimensions_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_F7_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    const FRONT: &str = "11111111-1111-4111-8111-000000000003";
    const BACK: &str = "11111111-1111-4111-8111-000000000004";
    let script = format!(
        r#"family {{"name": "Box", "category": "generic", "parameters": [{{"name": "Width", "value": 1000}}, {{"name": "Depth", "value": 600}}], "types": {{"Large": {{"Width": 1400, "Depth": 900}}, "Small": {{"Width": 800, "Depth": 500}}}}, "ref_planes": [{{"guid": "{LEFT}", "name": "Left", "start": [-500, -2000, 0], "end": [-500, 2000, 0], "up": [0, 0, 1]}}, {{"guid": "{RIGHT}", "name": "Right", "start": [500, -2000, 0], "end": [500, 2000, 0], "up": [0, 0, 1]}}, {{"guid": "{FRONT}", "name": "Front", "start": [-2000, -300, 0], "end": [2000, -300, 0], "up": [0, 0, 1]}}, {{"guid": "{BACK}", "name": "Back", "start": [-2000, 300, 0], "end": [2000, 300, 0], "up": [0, 0, 1]}}], "forms": [{{"guid": "{BAR}", "host": {{"plane": "9e3c0a10-1f0b-4d8c-8a5e-0000000000a3"}}, "profile": [[-150, 0], [150, 0], [150, 400], [-150, 400]], "from": -500, "to": 500}}], "dimensions": [{WIDTH}, {}, {{"refs": [{{"plane": "{FRONT}"}}, {{"plane": "{BACK}"}}], "at": [-900, 0, 0], "label": "Depth"}}, {{"refs": [{{"plane": "{FRONT}"}}, {{"plane": "9e3c0a10-1f0b-4d8c-8a5e-0000000000a2"}}, {{"plane": "{BACK}"}}], "at": [-1200, 0, 0], "equal": true}}, {{"refs": [{{"plane": "{LEFT}"}}, {{"form": "{BAR}", "end": "start"}}], "at": [-500, 0, 0], "locked": [{{"segment": 0, "length": 0}}], "alignment": true}}, {{"refs": [{{"plane": "{RIGHT}"}}, {{"form": "{BAR}", "end": "end"}}], "at": [500, 0, 0], "locked": [{{"segment": 0, "length": 0}}], "alignment": true}}]}}"#,
        EQ.replace("}]", r#"}], "equal": true"#)
    );
    let mut h = app(&script);
    h.set_size(vec2(1600.0, 1000.0));
    h.state_mut().edit_family("Box");
    h.run();
    view(&mut h, Some(Which::Plan));
    click_family(&mut h, [300.0, -700.0, 0.0]);
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("f7a-plan-large.png")).unwrap();
    ed_mut(&mut h).preview_type("Small");
    h.run();
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("f7a-plan-small.png")).unwrap();
}
