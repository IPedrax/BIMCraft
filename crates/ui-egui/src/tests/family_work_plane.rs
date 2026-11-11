//! T12: Revit's Work Plane dialog in the family document. Set opens it on the view's work plane (Current Work Plane, Show,
//! Dissociate greyed); Name chooses one of the family's planes, Pick a plane a trace and Pick a line a reference line clicked
//! after OK; a plane the view doesn't show flat asks Go To View, whose Open View shows a view that does with the plane set.
use super::family_views::{click_family, editor, open, view};
use super::*;
use bimcraft_core::family::views::{Side, Which};
use bimcraft_core::family::{CENTER_FRONT_BACK, CENTER_LEFT_RIGHT, PlaneRef, REF_LEVEL};
use egui_kittest::kittest::NodeT;

/// A box family with a reference line rising on Center (Front/Back), west of the origin, and one on Ref. Level in front.
const FAMILY: &str = r#"family {"name": "Box", "category": "generic", "types": {"T": {}}, "forms": [{"profile": [[500, 300], [1000, 300], [1000, 600], [500, 600]], "to": 2000}], "ref_planes": [{"name": "Edge", "start": [-400, -2000, 0], "end": [-400, 2000, 0], "up": [0, 0, 1]}], "ref_lines": [{"start": [-1300, -1000, 0], "end": [-700, -1000, 0]}, {"start": [-1300, 0, 300], "end": [-700, 0, 1100], "host": {"plane": "9e3c0a10-1f0b-4d8c-8a5e-0000000000a2"}}]}"#;

fn dialog<'a>(h: &'a Harness<'_, BimApp>) -> Option<&'a crate::editor::work_plane::Dialog> {
    editor(h).views.dialog.as_ref()
}

/// The Create tab's Work Plane > Set, on the ribbon (the topmost of the labels).
pub(super) fn set(h: &mut Harness<BimApp>) {
    let at = h.get_all_by_label("Set").map(|n| n.rect()).min_by(|a, b| a.top().total_cmp(&b.top())).expect("Set").center();
    click_at(h, at);
    h.run();
}

/// The dialog's button `label`, where the ribbon has one too (the lowest of the labels).
fn in_dialog(h: &mut Harness<BimApp>, label: &str) {
    let at = h.get_all_by_label(label).map(|n| n.rect()).max_by(|a, b| a.top().total_cmp(&b.top())).expect(label).center();
    click_at(h, at);
    h.run();
}

/// Name in the open dialog made `plane`.
fn choose(h: &mut Harness<BimApp>, plane: PlaneRef) {
    if let Some(d) = h.state_mut().family_editor.as_mut().and_then(|ed| ed.views.dialog.as_mut()) {
        d.chosen = plane;
    }
    h.run();
}

#[test]
fn set_opens_the_work_plane_dialog_naming_the_views_plane_and_a_plane_by_name_goes_through_go_to_view() {
    let (mut h, _) = open(FAMILY, "Box");
    view(&mut h, Some(Which::Elevation(Side::Front)));
    set(&mut h);
    assert!(dialog(&h).is_some());
    for label in ["Current Work Plane", "Specify a new Work Plane", "Reference Plane : Center (Front/Back)", "Pick a plane"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    assert!(h.get_by_label("Dissociate").accesskit_node().is_disabled(), "a family's work plane is always one of its planes");
    // Show from the dialog: the same toggle as the ribbon's; the dialog stays.
    in_dialog(&mut h, "Show");
    assert!(editor(&h).views.show_work_plane && dialog(&h).is_some());
    // Center (Left/Right) is seen edge-on in Front: Go To View, listing Left, Right and {3D}.
    choose(&mut h, PlaneRef::Plane(CENTER_LEFT_RIGHT));
    press(&mut h, "OK");
    let go = editor(&h).views.go_to.clone().expect("Go To View");
    assert_eq!(go.views, vec![Some(Which::Elevation(Side::Left)), Some(Which::Elevation(Side::Right)), None]);
    assert_eq!(editor(&h).views.work_plane(), PlaneRef::Plane(CENTER_FRONT_BACK), "nothing set yet");
    // Cancel sets nothing.
    press(&mut h, "Cancel");
    assert!(editor(&h).views.go_to.is_none());
    assert_eq!(editor(&h).views.work_plane(), PlaneRef::Plane(CENTER_FRONT_BACK));
    // Again, Right chosen and opened: shown, with the plane set there; Front keeps its own.
    set(&mut h);
    choose(&mut h, PlaneRef::Plane(CENTER_LEFT_RIGHT));
    press(&mut h, "OK");
    press(&mut h, "Elevation: Right");
    press(&mut h, "Open View");
    assert_eq!(editor(&h).views.shown, Some(Which::Elevation(Side::Right)));
    assert_eq!(editor(&h).views.work_plane(), PlaneRef::Plane(CENTER_LEFT_RIGHT));
    view(&mut h, Some(Which::Elevation(Side::Front)));
    assert_eq!(editor(&h).views.work_plane(), PlaneRef::Plane(CENTER_FRONT_BACK));
    // A plane the view shows flat is set at once: Ref. Level in plan.
    view(&mut h, Some(Which::Plan));
    set(&mut h);
    let edge = editor(&h).planes.iter().find(|p| p.name == "Edge").map(|p| p.guid).unwrap();
    choose(&mut h, PlaneRef::Plane(edge));
    press(&mut h, "OK");
    assert!(editor(&h).views.go_to.is_some(), "Edge stands upright: edge-on in plan");
    press(&mut h, "Cancel");
    set(&mut h);
    choose(&mut h, PlaneRef::Plane(REF_LEVEL));
    press(&mut h, "OK");
    assert!(editor(&h).views.go_to.is_none() && dialog(&h).is_none());
}

#[test]
fn pick_a_line_takes_the_plane_a_reference_line_was_sketched_in_and_escape_while_picking_brings_the_dialog_back() {
    let (mut h, _) = open(FAMILY, "Box");
    view(&mut h, Some(Which::Elevation(Side::Left)));
    assert_eq!(editor(&h).views.work_plane(), PlaneRef::Plane(CENTER_LEFT_RIGHT));
    set(&mut h);
    pick(&mut h, Role::RadioButton, "Pick a line and use the work plane it was sketched in");
    press(&mut h, "OK");
    assert_eq!(editor(&h).views.picking.map(|p| p.line), Some(true));
    assert!(dialog(&h).is_none(), "the dialog makes way for the click");
    // Escape: the dialog back, on Pick a line.
    key(&mut h, Key::Escape);
    assert!(editor(&h).views.picking.is_none());
    assert_eq!(dialog(&h).map(|d| d.by), Some(crate::editor::work_plane::By::Line));
    press(&mut h, "OK");
    // A click on nothing says what to click and waits on.
    click_family(&mut h, [0.0, 3000.0, 3000.0]);
    assert!(editor(&h).views.picking.is_some(), "still picking");
    // The rising line, seen edge-on from the left: drawn on Center (Front/Back), which Left sees edge-on too.
    click_family(&mut h, [-1000.0, 0.0, 700.0]);
    assert!(editor(&h).views.picking.is_none());
    let go = editor(&h).views.go_to.clone().expect("Go To View");
    assert_eq!(go.plane, PlaneRef::Plane(CENTER_FRONT_BACK));
    assert_eq!(go.views, vec![Some(Which::Elevation(Side::Back)), Some(Which::Elevation(Side::Front)), None]);
    press(&mut h, "Elevation: Front");
    press(&mut h, "Open View");
    assert_eq!(
        (editor(&h).views.shown, editor(&h).views.work_plane()),
        (Some(Which::Elevation(Side::Front)), PlaneRef::Plane(CENTER_FRONT_BACK))
    );
}

#[test]
fn in_3d_the_dialog_takes_a_plane_by_name_at_once_and_its_picks_are_greyed() {
    let (mut h, _) = open(FAMILY, "Box");
    assert_eq!(editor(&h).views.shown, None, "{{3D}}");
    set(&mut h);
    assert!(h.get_by_label("Pick a plane").accesskit_node().is_disabled(), "{{3D}} draws no planes to click");
    choose(&mut h, PlaneRef::Plane(CENTER_LEFT_RIGHT));
    press(&mut h, "OK");
    assert!(editor(&h).views.go_to.is_none(), "{{3D}} takes any plane");
    assert_eq!(editor(&h).views.work_plane(), PlaneRef::Plane(CENTER_LEFT_RIGHT));
}

/// The dialog, Go To View and a plane picked and shown, rendered for a look (`BIMCRAFT_T12_DUMP`: a folder).
#[test]
fn work_plane_dialog_pictures() {
    let Ok(folder) = std::env::var("BIMCRAFT_T12_DUMP") else { return };
    let folder = std::path::Path::new(&folder);
    let mut s = Session::new();
    s.script(FAMILY).unwrap();
    let mut h = Harness::builder()
        .with_size([1600.0, 1000.0])
        .with_max_steps(32)
        .build_ui_state(|ui, app: &mut BimApp| app.show(ui), BimApp::new(s));
    h.run();
    h.state_mut().edit_family("Box");
    h.run();
    view(&mut h, Some(Which::Elevation(Side::Front)));
    set(&mut h);
    h.run();
    h.render().unwrap().save(folder.join("t12-family-dialog.png")).unwrap();
    choose(&mut h, PlaneRef::Plane(CENTER_LEFT_RIGHT));
    press(&mut h, "OK");
    h.run();
    h.render().unwrap().save(folder.join("t12-go-to-view.png")).unwrap();
    press(&mut h, "Open View");
    if !editor(&h).views.show_work_plane {
        press(&mut h, "Show");
    }
    // Pick a plane, Edge's trace in Left... Edge faces Left: shown flat, tinted all over. Front shows it edge-on, its trace.
    view(&mut h, Some(Which::Plan));
    set(&mut h);
    pick(&mut h, Role::RadioButton, "Pick a plane");
    press(&mut h, "OK");
    click_family(&mut h, [-400.0, 800.0, 0.0]);
    press(&mut h, "Open View");
    h.run();
    h.render().unwrap().save(folder.join("t12-family-picked-plane.png")).unwrap();
}
