//! T6a, nested families in the family document: a family file loaded into a family (Insert > Load Family), placed by
//! Component with a click in its floor plan, its width associated with the host's through "=", loaded into the project; then
//! Edit Family on it, changed in its own document and loaded back into the host, Family Already Exists asking how.
use super::family_views::{editor, view};
use super::*;
use bimcraft_core::family::Expr;
use bimcraft_core::family::views::Which;

const EXISTS: &str = "Family Already Exists";
const VERSION: &str = "Overwrite the existing version";
const VALUES: &str = "Overwrite the existing version and its parameter values";

/// A cabinet of two types, Small (600) and Large (1200), with a plinth, in a project.
const CABINET: &str = r#"family {"name": "Cabinet", "category": "furniture", "parameters": [{"name": "Width", "value": 600}], "types": {"Small": {"Width": 600}, "Large": {"Width": 1200}}, "forms": [{"profile": [[-50, -50], [50, -50], [50, 50], [-50, 50]], "to": 20}]}"#;

/// A box family's file, `W` by `D` by `H`, in `dir`.
fn box_file(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("Box.json");
    let family = json!({"name": "Box", "category": "generic",
        "parameters": [{"name": "W", "value": 300}, {"name": "D", "value": 200}, {"name": "H", "value": 400}],
        "types": {"B": {}}, "forms": [{"profile": [[0, 0], ["W", 0], ["W", "D"], [0, "D"]], "to": "H"}]});
    std::fs::write(&path, family.to_string()).unwrap();
    path
}

/// "=" on the selected instance's W, scrolled to in Properties first.
fn associate_w(h: &mut Harness<BimApp>) {
    let label = "Associate Family Parameter: W";
    h.get_by_label(label).scroll_to_me();
    h.run();
    press(h, label);
}

/// The cabinet open in its document, the box loaded into it, placed at (300, 200) in the floor plan.
fn placed(dir: &std::path::Path) -> Harness<'static, BimApp> {
    let mut h = app(CABINET);
    h.set_size(vec2(1600., 1000.));
    h.state_mut().edit_family("Cabinet");
    h.run();
    h.state_mut().nest_family_file(&box_file(dir));
    h.run();
    assert_eq!(editor(&h).nested.len(), 1, "the box is loaded into the cabinet");
    assert!(editor(&h).nested_instances.is_empty(), "and nothing is placed yet");
    view(&mut h, Some(Which::Plan));
    press(&mut h, "Component");
    assert!(editor(&h).placing.is_some(), "Component places the box");
    assert!(h.query_all_by_label("Modify | Place Component").count() > 0, "its contextual tab");
    let at = editor(&h).views.screen_of([300.0, 200.0, 0.0]).expect("the floor plan shows");
    click_at(&mut h, at);
    h.run();
    h
}

#[test]
fn a_family_loaded_into_a_family_is_placed_associated_and_loaded_into_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = placed(dir.path());
    let ed = editor(&h);
    assert_eq!(ed.nested_instances.len(), 1, "a click placed one");
    let inst = &ed.nested_instances[0];
    let near = |e: &Expr, want: f64| matches!(e, Expr::Number(x) if (x - want).abs() < 2.0);
    assert!(near(&inst.at[0], 300.0) && near(&inst.at[1], 200.0), "where the click was: {:?}", inst.at);
    assert_eq!(ed.selected_nested(), Some(0), "and it is selected");
    assert!(ed.placing.is_none(), "the tool is done");
    assert!(h.query_all_by_label("Modify | Nested Instance").count() > 0, "the instance's contextual tab");
    assert!(h.query_by_label("Associate Family Parameter: W").is_some(), "its Properties show its family's parameters");
    // "=" on its W: Associate Family Parameter ties it to the cabinet's Width.
    associate_w(&mut h);
    press(&mut h, "Width");
    press(&mut h, "OK");
    assert_eq!(editor(&h).nested_instances[0].params.get("W"), Some(&Expr::Formula("Width".into())));
    // Into the project: the cabinet's Large type carries a box 1200 wide.
    press(&mut h, "Load into Project");
    assert!(h.query_by_label(EXISTS).is_some(), "the project has the cabinet defined otherwise");
    press(&mut h, VERSION);
    let f = &h.state().session.model.families["Cabinet"];
    assert_eq!(f.nested_instances.len(), 1);
    let v = f.values("Large", &Default::default()).unwrap();
    let x = f.pieces(&v).unwrap().iter().flat_map(|p| p.tris.iter().flatten()).map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
    assert!((x - 1500.0).abs() < 3.0, "the box reaches 300 + 1200: {x}");
    // Delete takes the instance away; the family it placed stays loaded.
    h.state_mut().active_family_mut().unwrap().select_nested(Some(0));
    h.run();
    press(&mut h, "Delete");
    assert!(editor(&h).nested_instances.is_empty());
    assert_eq!(editor(&h).nested.len(), 1);
}

#[test]
fn edit_family_on_a_nested_family_loads_it_back_into_the_host_asking_family_already_exists() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = placed(dir.path());
    // Edit Family, on the selected instance's contextual tab: its own document, over the cabinet's.
    press(&mut h, "Edit Family");
    assert_eq!(editor(&h).title(), "Box", "the box's document");
    assert!(editor(&h).into_host);
    // Its D made 250 in Family Types.
    press(&mut h, "Family Types");
    type_into(&mut h, "Parameter Value D", "250");
    press(&mut h, "OK");
    // Load into Family goes into the cabinet, asking: Overwrite the existing version keeps the cabinet's value of D.
    press(&mut h, "Load into Family");
    assert!(h.query_by_label(EXISTS).is_some(), "the cabinet has the box defined otherwise");
    assert!(h.query_all_by_label_contains("already in this family").count() > 0, "said of the family, not the project");
    press(&mut h, VERSION);
    assert_eq!(editor(&h).title(), "Box", "Load into Family leaves it open");
    let project = h.state().session.model.to_json().unwrap();
    // Load into Family and Close, with its values: back to the cabinet, whose box now has D 250.
    press(&mut h, "Load into Family and Close");
    assert!(h.query_by_label(EXISTS).is_some());
    press(&mut h, VALUES);
    assert_eq!(editor(&h).title(), "Cabinet", "back in the cabinet");
    let d = |h: &Harness<BimApp>| editor(h).nested[0].family.types["B"].get("D").and_then(Value::as_f64);
    assert_eq!(d(&h), Some(250.0), "type B's D");
    assert_eq!(editor(&h).nested_instances.len(), 1, "its instance stays");
    assert_eq!(h.state().session.model.to_json().unwrap(), project, "the project is untouched until the cabinet is loaded");
    // Cancel on a load into the host changes nothing; Close Family asks, and Close Without Loading goes back without loading.
    h.state_mut().active_family_mut().unwrap().select_nested(Some(0));
    h.run();
    press(&mut h, "Edit Family");
    press(&mut h, "Family Types");
    type_into(&mut h, "Parameter Value D", "999");
    press(&mut h, "OK");
    press(&mut h, "Load into Family and Close");
    press(&mut h, "Cancel");
    assert_eq!(editor(&h).title(), "Box", "Cancel: it stays open");
    press(&mut h, "Close Family");
    assert!(h.query_by_label("Save changes to Box?").is_some(), "the box has changes not loaded");
    press(&mut h, "Close Without Loading");
    assert_eq!(editor(&h).title(), "Cabinet");
    assert_eq!(d(&h), Some(250.0), "closed without loading: the cabinet's box as it was");
}

/// The cabinet's floor plan with the box placed and selected, its Properties showing, and the box's own document opened over
/// it by Edit Family, saved where `BIMCRAFT_T6_DUMP` says (only then).
#[test]
fn nested_families_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_T6_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut h = placed(dir.path());
    h.state_mut().active_family_mut().unwrap().select_nested(Some(0));
    h.run();
    associate_w(&mut h);
    press(&mut h, "Width");
    press(&mut h, "OK");
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("t6-plan-placed.png")).unwrap();
    key(&mut h, Key::Escape);
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("t6-plan.png")).unwrap();
    h.state_mut().active_family_mut().unwrap().select_nested(Some(0));
    h.run();
    view(&mut h, None);
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("t6-3d.png")).unwrap();
    press(&mut h, "Edit Family");
    press(&mut h, "Family Types");
    type_into(&mut h, "Parameter Value D", "250");
    press(&mut h, "OK");
    press(&mut h, "Load into Project");
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("t6-exists-dialog.png")).unwrap();
}
