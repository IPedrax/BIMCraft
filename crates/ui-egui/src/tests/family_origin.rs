//! O1: Defines Origin on a reference plane's Properties, and the insertion point's mark in the family's views. Ticked on a plane,
//! the plane of its direction that defined the origin lets go; cleared, the datum takes it back; the forms stay where the planes
//! are drawn. An adaptive family, which its points place, has no insertion point. With `BIMCRAFT_O1_DUMP` set to a folder,
//! `the_insertion_point_is_marked_in_plans_elevations_and_3d` saves the window there (`o1-*.png`) to look at.
use super::family_views::{around, click_family, editor, open, view};
use super::*;
use bimcraft_core::family::views::{Side, Which};

/// A box east and behind the origin, and planes of its own: Edge, upright west of it (square to x), and Shelf, level 500 up.
const FAMILY: &str = r#"family {"name": "Box", "category": "generic", "types": {"T": {}}, "forms": [{"profile": [[500, 300], [1000, 300], [1000, 600], [500, 600]], "to": 2000}], "ref_planes": [{"name": "Edge", "start": [-400, -2000, 0], "end": [-400, 2000, 0], "up": [0, 0, 1]}, {"name": "Shelf", "start": [-2000, 0, 500], "end": [2000, 0, 500], "up": [0, 1, 0]}]}"#;

fn flags(h: &Harness<BimApp>) -> Vec<(String, bool)> {
    editor(h).planes.iter().map(|p| (p.name.clone(), p.defines_origin)).collect()
}

#[test]
fn defines_origin_moves_the_insertion_point_and_leaves_the_forms_where_the_planes_are() {
    let (mut h, pictures) = open(FAMILY, "Box");
    assert_eq!(editor(&h).origin_mark(), Some([0.0; 3]), "the datums' crossing");
    view(&mut h, Some(Which::Plan));
    let before = around(&h, &pictures, [750.0, 450.0, 0.0], 3);
    // Edge selected in the plan: its Properties' Defines Origin, ticked.
    click_family(&mut h, [-400.0, -1000.0, 0.0]);
    assert_eq!(editor(&h).selected_plane().map(|g| editor(&h).planes.iter().any(|p| p.guid == g && p.name == "Edge")), Some(true));
    pick(&mut h, Role::CheckBox, "Defines Origin");
    let on: Vec<(String, bool)> = flags(&h).into_iter().filter(|(_, on)| *on).collect();
    assert_eq!(
        on,
        [("Ref. Level".to_string(), true), ("Center (Front/Back)".to_string(), true), ("Edge".to_string(), true)],
        "Center (Left/Right) let go of x"
    );
    assert_eq!(editor(&h).origin_mark(), Some([-400.0, 0.0, 0.0]));
    // The box is drawn where it was, by its planes: the family is placed by its origin in the document's scratch project.
    assert_eq!(around(&h, &pictures, [750.0, 450.0, 0.0], 3), before, "the box stays where it is drawn");
    // Shelf, in an elevation, takes z.
    view(&mut h, Some(Which::Elevation(Side::Front)));
    let front = around(&h, &pictures, [750.0, 0.0, 1000.0], 3);
    click_family(&mut h, [-1500.0, 0.0, 500.0]);
    pick(&mut h, Role::CheckBox, "Defines Origin");
    assert_eq!(editor(&h).origin_mark(), Some([-400.0, 0.0, 500.0]));
    assert_eq!(around(&h, &pictures, [750.0, 0.0, 1000.0], 3), front);
    // Cleared, Ref. Level takes z back.
    pick(&mut h, Role::CheckBox, "Defines Origin");
    assert_eq!(editor(&h).origin_mark(), Some([-400.0, 0.0, 0.0]));
    assert!(flags(&h).contains(&("Ref. Level".to_string(), true)));
    // A datum ticked stays ticked until another plane takes it: its box can't be cleared.
    click_family(&mut h, [-1500.0, 0.0, 0.0]);
    assert_eq!(editor(&h).selected_plane(), Some(bimcraft_core::family::REF_LEVEL));
    use egui_kittest::kittest::NodeT;
    let boxes: Vec<bool> = h
        .get_all_by_label("Defines Origin")
        .filter(|n| n.accesskit_node().role() == Role::CheckBox)
        .map(|n| n.accesskit_node().is_disabled())
        .collect();
    assert_eq!(boxes, [true]);
    // Loaded into the project, the family holds it, and stands by it there.
    press(&mut h, "Load into Project");
    press(&mut h, "Overwrite the existing version");
    let f = &h.state().session.model.families["Box"];
    assert_eq!(f.origin(), [-400.0, 0.0, 0.0]);
}

#[test]
fn an_adaptive_family_has_no_insertion_point_to_mark() {
    let (mut h, _) = open(FAMILY, "Box");
    h.state_mut().active_family_mut().unwrap().adaptive = Some(bimcraft_core::adaptive::four_point_panel());
    assert_eq!(editor(&h).origin_mark(), None);
    view(&mut h, Some(Which::Plan));
    click_family(&mut h, [-400.0, -1000.0, 0.0]);
    assert!(h.query_by_label("Defines Origin").is_none(), "no Defines Origin for an adaptive family");
}

/// The window rendered with wgpu, saved as `o1-<name>.png` where `BIMCRAFT_O1_DUMP` says.
fn dump(h: &mut Harness<BimApp>, folder: &std::path::Path, name: &str) {
    // The pointer off what is shown, on the status bar.
    h.event(egui::Event::PointerMoved(egui::pos2(1000.0, 990.0)));
    h.run();
    h.render().unwrap().save(folder.join(format!("o1-{name}.png"))).unwrap();
}

#[test]
fn the_insertion_point_is_marked_in_plans_elevations_and_3d() {
    let Ok(folder) = std::env::var("BIMCRAFT_O1_DUMP") else { return };
    let folder = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&folder).unwrap();
    let mut s = Session::new();
    s.script(FAMILY).unwrap();
    let mut h = Harness::builder()
        .with_size([1600.0, 1000.0])
        .with_max_steps(32)
        .build_ui_state(|ui, app: &mut BimApp| app.show(ui), BimApp::new(s));
    h.run();
    h.state_mut().edit_family("Box");
    h.run();
    view(&mut h, Some(Which::Plan));
    click_family(&mut h, [-400.0, -1000.0, 0.0]);
    pick(&mut h, Role::CheckBox, "Defines Origin");
    dump(&mut h, &folder, "plan-properties");
    view(&mut h, Some(Which::Elevation(Side::Front)));
    click_family(&mut h, [-1500.0, 0.0, 500.0]);
    pick(&mut h, Role::CheckBox, "Defines Origin");
    dump(&mut h, &folder, "front-elevation");
    view(&mut h, None);
    dump(&mut h, &folder, "3d");
    h.ctx.set_theme(egui::Theme::Dark);
    view(&mut h, Some(Which::Plan));
    dump(&mut h, &folder, "plan-dark");
}
