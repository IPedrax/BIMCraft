//! T11: a form selected in the family document shows its instance Properties as Revit's Extrusion does (Constraints, Graphics,
//! Materials and Finishes, Identity Data), its Depth in the Options Bar, and Edit Work Plane on its contextual tab.
use super::*;
use bimcraft_core::family::{Expr, Family, PlaneRef};
use egui_kittest::kittest::NodeT;

/// A family with an extrusion from 10 to 60 on Ref. Level, a plane Up 300 over it, and a length, a material and a yes/no
/// parameter, opened as a document with the extrusion selected from the Project Browser's Forms list.
fn selected() -> Harness<'static, BimApp> {
    let mut h = app("");
    h.set_size(vec2(1600., 1000.));
    let material = h.state().session.model.materials.keys().next().unwrap().clone();
    let made = h.state_mut().run(
        "family",
        json!({"name": "Plinth", "types": {"A": {}},
               "parameters": [{"name": "Height", "value": 900}, {"name": "Finish", "kind": "material", "value": material},
                              {"name": "Show", "kind": "yesno", "value": true}],
               "ref_planes": [{"name": "Up", "start": [0, 0, 300], "end": [1000, 0, 300], "up": [0, 1, 0]}],
               "forms": [{"profile": [[0, 0], [100, 0], [100, 50], [0, 50]], "from": 10, "to": 60, "material": material}]}),
    );
    assert!(made.is_some());
    h.state_mut().edit_family("Plinth");
    h.run();
    press(&mut h, "1. Extrusion");
    h
}

/// The draft as it stands, built.
fn draft(h: &Harness<BimApp>) -> Family {
    h.state().family_editor.as_ref().unwrap().draft_family().unwrap()
}

/// The draft's built triangles: their lowest and highest z.
fn heights(h: &Harness<BimApp>) -> (f64, f64) {
    let f = draft(h);
    let values = f.values("A", &Default::default()).unwrap();
    let zs: Vec<f64> = f.pieces(&values).unwrap().iter().flat_map(|p| p.tris.iter().flatten().map(|q| q[2])).collect();
    (zs.iter().copied().fold(f64::INFINITY, f64::min), zs.iter().copied().fold(f64::NEG_INFINITY, f64::max))
}

/// Focus taken off the field typed in, so the edit becomes a step of the document's history.
fn leave(h: &mut Harness<BimApp>) {
    h.get_by_label("Close Family").focus();
    h.run();
    h.run();
}

#[test]
fn a_selected_form_shows_revits_extrusion_properties() {
    let h = selected();
    let material = h.state().session.model.materials.keys().next().unwrap().clone();
    assert_eq!(h.get_by_label("Extrusion End").value().as_deref(), Some("60"));
    assert_eq!(h.get_by_label("Extrusion Start").value().as_deref(), Some("10"));
    assert!(h.query_by_label("Level : Ref. Level").is_some(), "the Work Plane names its host as Revit does");
    assert!(h.query_by_label("Work Plane").is_some());
    assert!(h.query_by_label("Visible").is_some());
    assert_eq!(h.get_by_label("Material").value().as_deref(), Some(material.as_str()));
    assert_eq!(h.get_by_label("Solid/Void").value().as_deref(), Some("Solid"));
    for group in ["Constraints", "Graphics", "Materials and Finishes", "Identity Data"] {
        assert!(h.query_by_label(group).is_some(), "{group}");
    }
    // The contextual tab and the Options Bar's Depth come with it.
    assert!(h.query_all_by_label("Modify | Form").count() > 0);
    assert_eq!(h.get_by_label("Depth").value().as_deref(), Some("50"));
    // The family's own are not shown under it, as Revit's Properties show only what is selected.
    assert!(h.query_by_label("Family name").is_none());
}

#[test]
fn editing_extrusion_end_changes_the_form_and_undo_puts_it_back() {
    let mut h = selected();
    type_into(&mut h, "Extrusion End", "200");
    leave(&mut h);
    assert_eq!(draft(&h).forms[0].to, Expr::Number(200.));
    assert_eq!(heights(&h), (10., 200.));
    press(&mut h, "Undo");
    assert_eq!(draft(&h).forms[0].to, Expr::Number(60.));
    assert_eq!(h.get_by_label("Extrusion End").value().as_deref(), Some("60"));
    press(&mut h, "Redo");
    assert_eq!(draft(&h).forms[0].to, Expr::Number(200.));
    assert!(h.state().session.model.families["Plinth"].forms[0].to == Expr::Number(60.), "the project's family is as it was");
}

#[test]
fn equals_ties_extrusion_end_to_a_parameter_and_the_row_greys_out() {
    let mut h = selected();
    press(&mut h, "Associate Family Parameter: Extrusion End");
    assert!(h.query_by_label("Finish").is_none(), "only lengths are offered");
    press(&mut h, "Height");
    press(&mut h, "OK");
    assert_eq!(draft(&h).forms[0].to, Expr::Formula("Height".into()));
    let end = h.get_by_label("Extrusion End");
    assert!(end.accesskit_node().is_disabled(), "tied, the row takes no typing");
    assert_eq!(end.value().as_deref(), Some("900"), "it shows what Height comes to");
    // Depth is worked out from it, and can't be typed while End is tied.
    let depth = h.get_by_label("Depth");
    assert!(depth.accesskit_node().is_disabled());
    assert_eq!(depth.value().as_deref(), Some("890"));
    // Material ties to a material parameter the same way.
    // Properties scroll: the row is brought into view before its "=" is pressed.
    h.get_by_label("Associate Family Parameter: Material").scroll_to_me();
    h.run();
    press(&mut h, "Associate Family Parameter: Material");
    press(&mut h, "Finish");
    press(&mut h, "OK");
    assert_eq!(draft(&h).forms[0].material, "Finish");
    assert!(h.get_by_label("Material").accesskit_node().is_disabled());
}

#[test]
fn depth_in_the_options_bar_moves_extrusion_end() {
    let mut h = selected();
    type_into(&mut h, "Depth", "100");
    leave(&mut h);
    let f = draft(&h);
    assert_eq!((f.forms[0].from.clone(), f.forms[0].to.clone()), (Expr::Number(10.), Expr::Number(110.)), "Start kept, End moved");
    assert_eq!(h.get_by_label("Extrusion End").value().as_deref(), Some("110"));
    press(&mut h, "Undo");
    assert_eq!(draft(&h).forms[0].to, Expr::Number(60.));
}

#[test]
fn edit_work_plane_moves_the_form_onto_another_plane() {
    let mut h = selected();
    assert_eq!(heights(&h), (10., 60.));
    press(&mut h, "Edit Work Plane");
    // The Work Plane dialog (T12), on the form's plane; Name chooses Up.
    assert!(h.query_by_label("Current Work Plane").is_some(), "the dialog");
    let up = draft(&h).planes.iter().find(|p| p.name == "Up").unwrap().guid;
    if let Some(d) = h.state_mut().family_editor.as_mut().and_then(|ed| ed.views.dialog.as_mut()) {
        assert_eq!(d.chosen, PlaneRef::default(), "Name starts on the form's own plane");
        d.chosen = PlaneRef::Plane(up);
    }
    press(&mut h, "OK");
    assert!(h.state().family_editor.as_ref().unwrap().views.dialog.is_none());
    let f = draft(&h);
    let up = f.planes.iter().find(|p| p.name == "Up").unwrap().guid;
    assert_eq!(f.forms[0].host, PlaneRef::Plane(up));
    assert_eq!(heights(&h), (310., 360.), "it moved with the plane, as Revit's Edit Work Plane does");
    assert!(h.query_by_label("Reference Plane : Up").is_some(), "Properties' Work Plane names it");
    // The form keeps its identity, and the project is untouched until Load into Project.
    assert_eq!(f.forms[0].guid, h.state().session.model.families["Plinth"].forms[0].guid);
    assert_eq!(h.state().session.model.families["Plinth"].forms[0].host, PlaneRef::default());
    press(&mut h, "Undo");
    assert_eq!(draft(&h).forms[0].host, PlaneRef::default());
    press(&mut h, "Redo");
    press(&mut h, "Load into Project");
    press(&mut h, "Overwrite the existing version");
    assert_eq!(h.state().session.model.families["Plinth"].forms[0].host, PlaneRef::Plane(up));
}

#[test]
fn solid_void_switches_the_form_to_a_void() {
    let mut h = selected();
    choose(&mut h, "Solid/Void", "Void");
    assert!(draft(&h).forms[0].void);
    assert_eq!(h.get_by_label("Solid/Void").value().as_deref(), Some("Void"));
    press(&mut h, "Undo");
    assert!(!draft(&h).forms[0].void);
}

#[test]
fn escape_or_a_click_in_the_view_lets_the_form_go() {
    let mut h = selected();
    h.key_press(Key::Escape);
    h.run();
    assert!(h.query_by_label("Extrusion End").is_none());
    assert!(h.query_by_label("Family name").is_some(), "nothing selected, Properties show the family's own");
    // Selected again, from the browser, and let go by a click in the view on nothing (one on a form selects it, T11).
    press(&mut h, "1. Extrusion");
    assert!(h.query_by_label("Extrusion End").is_some());
    let nothing = h.state().pane.min + vec2(40.0, 120.0);
    click_at(&mut h, nothing);
    assert!(h.query_by_label("Extrusion End").is_none());
}
