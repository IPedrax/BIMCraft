//! Work planes in the 3D view: the view control bar's Work Plane sets what the view sketches on (a level from the list, a
//! surface or three points picked on the model, each with an offset), and the Floor, Ceiling, Roof and Shaft tools sketch a
//! new one there when it lies level.

use super::*;
use bimcraft_core::walls::Plane;

fn frame(h: &Harness<BimApp>) -> bimcraft_core::workplane::Frame {
    let m = &h.state().session.model;
    m.work_plane(m.default_3d()).resolve(m).unwrap()
}

fn plane(h: &Harness<BimApp>) -> Plane {
    frame(h).over_plan().unwrap()
}

/// The box's walls: half their thickness, and how high their tops are.
fn walls(h: &Harness<BimApp>) -> (f64, f64) {
    let m = &h.state().session.model;
    let w = m.wall(ids(h, "wall")[0]).unwrap();
    (m.thickness(w) / 2.0, m.top_z(w))
}

/// Pick Points, clicking a few points off each of `points` on the model: each snaps to the corner there.
fn pick_points(h: &mut Harness<BimApp>, points: [[f64; 3]; 3]) {
    press(h, "Pick Points");
    assert!(h.state().work_plane.as_ref().is_some_and(|d| d.picking.is_some()), "the dialog makes way for the picks");
    for p in points {
        let at = h.state().screen3(p) + vec2(3.0, -2.0);
        click_at(h, at);
    }
    assert!(h.state().work_plane.as_ref().is_some_and(|d| d.picking.is_none()), "back, with the points in it");
}

/// Each sketched-flat tool refused on a plane that isn't level, saying it must be horizontal, and nothing sketched.
fn refused_off_level(h: &mut Harness<BimApp>) {
    for name in ["Floor", "Ceiling"] {
        tool(h, name);
        assert!(h.state().sketch.is_none(), "{name}");
        assert_eq!(h.state().tool, Tool::Modify);
        assert!(h.state().status.error && h.state().status.text.contains("must be horizontal"), "{}", h.state().status.text);
    }
}

/// Where a point of the work plane shows in the 3D view.
fn on_plane(h: &Harness<BimApp>, p: P2) -> Pos2 {
    let z = plane(h).at(p);
    h.state().screen3([p.0, p.1, z])
}

/// A rectangle drawn with the Draw panel's Rectangle, corner to corner on the work plane, and the sketch finished.
fn rectangle(h: &mut Harness<BimApp>, a: P2, b: P2) {
    press(h, "Rectangle");
    let at = on_plane(h, a);
    click_at(h, at);
    let at = on_plane(h, b);
    click_at(h, at);
    assert_eq!(h.state().sketch.as_ref().map(|s| s.lines.len()), Some(4), "{}", h.state().status.text);
    press(h, "Finish Edit Mode");
    assert!(h.state().sketch.is_none(), "{}", h.state().status.text);
}

#[test]
fn in_3d_the_floor_tool_sketches_a_new_floor_on_the_work_plane_set_to_a_level_and_an_offset() {
    let mut h = app(BOX);
    press(&mut h, "Default 3D View");
    press(&mut h, "Work Plane");
    type_into(&mut h, "Offset", "500");
    press(&mut h, "OK");
    assert_eq!(plane(&h), Plane::flat(500.0));
    tool(&mut h, "Floor");
    assert_eq!(h.state().sheet, Sheet::View3D, "it sketches where it is: {}", h.state().status.text);
    assert!(matches!(h.state().sketch.as_ref().map(|s| s.of), Some(crate::sketch::Of::Floor(_))));
    rectangle(&mut h, P2(2000.0, 2000.0), P2(4000.0, 3000.0));
    let floors = ids(&h, "floor");
    assert_eq!(floors.len(), 1);
    assert_eq!(field(&h, floors[0], "offset"), json!(500.0));
    assert_eq!(field(&h, floors[0], "area_m2"), json!(2.0));
    assert_eq!(h.state().selection, floors, "selected, as a finished sketch is");
}

#[test]
fn three_points_picked_snap_to_corners_and_a_sloped_plane_is_set_though_floors_and_ceilings_need_a_level_one() {
    let mut h = app(BOX);
    press(&mut h, "Default 3D View");
    let (half, top) = walls(&h);
    let (w, e, s, n) = (-half, 8000.0 + half, -half, 6000.0 + half);
    // The south wall's foot and the north-east corner's top: a plane sloping up to the north-east.
    press(&mut h, "Work Plane");
    pick_points(&mut h, [[w, s, 0.0], [e, s, 0.0], [e, n, top]]);
    assert!(h.state().selection.is_empty(), "a pick is no selection");
    press(&mut h, "OK");
    let p = plane(&h);
    assert!((p.at(P2(e, n)) - top).abs() < 1e-6 && p.at(P2(w, s)).abs() < 1e-6, "on the corners themselves: {p:?}");
    refused_off_level(&mut h);
    // The tops of three corners: level, at their height; a floor is sketched on it.
    press(&mut h, "Work Plane");
    pick_points(&mut h, [[w, s, top], [e, s, top], [e, n, top]]);
    press(&mut h, "OK");
    assert_eq!(plane(&h), Plane::flat(top), "{}", h.state().status.text);
    tool(&mut h, "Floor");
    rectangle(&mut h, P2(1000.0, 1000.0), P2(3000.0, 3000.0));
    let f = ids(&h, "floor")[0];
    assert_eq!(h.state().session.model.sketch_plane(f), Some(Plane::flat(top)), "its top on the plane, level");
}

#[test]
fn a_work_plane_on_a_picked_wall_face_stands_upright_offset_out_from_it_and_floors_refuse_it() {
    let mut h = app(BOX);
    press(&mut h, "Default 3D View");
    let (half, _) = walls(&h);
    press(&mut h, "Work Plane");
    pick(&mut h, Role::RadioButton, "Pick a surface");
    press(&mut h, "OK");
    assert!(h.state().work_plane.as_ref().and_then(|d| d.error.as_ref()).is_some_and(|e| e.contains("surface")), "nothing picked yet");
    press(&mut h, "Pick Surface");
    // The south wall's outside face, its middle.
    let at = h.state().screen3([4000.0, -half, 1500.0]);
    click_at(&mut h, at);
    assert!(h.state().work_plane.as_ref().is_some_and(|d| d.picking.is_none() && d.surface.is_some()), "back, with the face");
    type_into(&mut h, "Offset", "200");
    press(&mut h, "OK");
    let f = frame(&h);
    assert!(f.normal[1] < -1.0 + 1e-9, "facing south, out of the wall: {f:?}");
    assert!((f.origin[1] - (-half - 200.0)).abs() < 1e-6, "200 mm out from the face: {f:?}");
    assert!(h.state().work_plane_name().contains("90°"), "{}", h.state().work_plane_name());
    refused_off_level(&mut h);
    // Its patch is on it, where it was picked: a click opens the dialog, on the surface.
    let patch = h.state().work_plane_patch().expect("drawn in the 3D view");
    click_at(&mut h, patch);
    assert_eq!(h.state().work_plane.as_ref().map(|d| d.by), Some(crate::workplane::By::Surface));
}

#[test]
fn a_line_sketched_in_3d_shows_where_it_goes_from_the_last_point_before_the_click() {
    let mut h = app(BOX);
    press(&mut h, "Default 3D View");
    tool(&mut h, "Floor");
    let at = on_plane(&h, P2(1000.0, 1000.0));
    click_at(&mut h, at);
    let shown = h.state().sketch3d_preview(P2(3000.0, 1000.0));
    assert_eq!(shown, vec![vec![P2(1000.0, 1000.0), P2(3000.0, 1000.0)]], "the line to come, from the point clicked");
}

#[test]
fn the_work_plane_is_set_from_its_patch_in_the_view_the_right_click_menu_and_the_ribbon() {
    let mut h = app(BOX);
    press(&mut h, "Default 3D View");
    // The patch on the plane itself, as ArchiCAD's: a click on it opens the dialog.
    let patch = h.state().work_plane_patch().expect("drawn in the 3D view");
    click_at(&mut h, patch);
    assert!(h.state().work_plane.is_some(), "the patch opens the dialog");
    assert!(h.state().selection.is_empty());
    press(&mut h, "Cancel");
    assert!(h.state().work_plane.is_none());
    // The right-click menu.
    let at = h.state().orbit.rect.min + vec2(40.0, 40.0);
    right_click_at(&mut h, at);
    choose_item(&mut h, "Set Work Plane");
    assert!(h.state().work_plane.is_some(), "the menu opens the dialog");
    press(&mut h, "Cancel");
    // The Architecture tab's Set, beside the levels and grids (in the Datum panel's list where the window folds it).
    press(&mut h, "Architecture");
    from_list(&mut h, "Datum", "Set");
    assert!(h.state().work_plane.is_some(), "the ribbon opens the dialog");
}

#[test]
fn in_a_plan_the_floor_tool_sketches_at_once_and_walls_outline_puts_the_walls_outline_in_the_sketch() {
    let mut h = app(BOX);
    tool(&mut h, "Floor");
    assert!(matches!(h.state().sketch.as_ref().map(|s| s.of), Some(crate::sketch::Of::Floor(_))), "straight into sketch mode");
    assert!(h.query_by_label("Modify | Create Floor Boundary").is_some());
    press(&mut h, "Walls' Outline");
    assert_eq!(h.state().sketch.as_ref().map(|s| s.lines.len()), Some(4), "the box's four sides, to edit before Finish");
    press(&mut h, "Finish Edit Mode");
    let floors = ids(&h, "floor");
    assert_eq!(floors.len(), 1, "{}", h.state().status.text);
    assert_eq!(field(&h, floors[0], "area_m2"), json!(48.0));
}
