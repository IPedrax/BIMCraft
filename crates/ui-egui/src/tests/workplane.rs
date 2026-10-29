//! Work planes in the 3D view: the view control bar's Work Plane sets what the view sketches on (a level with an offset, or
//! three points typed or picked on the model), and the Floor, Ceiling, Roof and Shaft tools sketch a new one there.

use super::*;
use bimcraft_core::walls::Plane;

fn plane(h: &Harness<BimApp>) -> Plane {
    let m = &h.state().session.model;
    m.work_plane(m.default_3d()).resolve(m).unwrap()
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
fn a_work_plane_through_three_typed_points_may_slope_but_floors_and_ceilings_need_it_horizontal() {
    let mut h = app(BOX);
    press(&mut h, "Default 3D View");
    press(&mut h, "Work Plane");
    pick(&mut h, Role::RadioButton, "Through three points");
    // Rising 1000 mm over 4000 mm toward the north.
    for (k, p) in [[0, 0, 300], [4000, 0, 300], [0, 4000, 1300]].iter().enumerate() {
        for (axis, v) in ["X", "Y", "Z"].iter().zip(p) {
            type_into(&mut h, &format!("Point {} {axis}", k + 1), &v.to_string());
        }
    }
    press(&mut h, "OK");
    assert!((plane(&h).at(P2(2000.0, 2000.0)) - 800.0).abs() < 1e-6, "{}", h.state().status.text);
    // Each refused, saying the plane must be horizontal, and nothing sketched.
    for name in ["Floor", "Ceiling"] {
        tool(&mut h, name);
        assert!(h.state().sketch.is_none(), "{name}");
        assert_eq!(h.state().tool, Tool::Modify);
        assert!(h.state().status.error && h.state().status.text.contains("must be horizontal"), "{}", h.state().status.text);
    }
    // Three points at one height are a level plane: a floor is sketched on it.
    press(&mut h, "Work Plane");
    for (k, p) in [[0, 0, 750], [4000, 0, 750], [0, 4000, 750]].iter().enumerate() {
        for (axis, v) in ["X", "Y", "Z"].iter().zip(p) {
            type_into(&mut h, &format!("Point {} {axis}", k + 1), &v.to_string());
        }
    }
    press(&mut h, "OK");
    tool(&mut h, "Floor");
    rectangle(&mut h, P2(1000.0, 1000.0), P2(3000.0, 3000.0));
    let f = ids(&h, "floor")[0];
    assert_eq!(field(&h, f, "offset"), json!(750.0));
    assert_eq!(field(&h, f, "slope").as_f64().unwrap_or(0.0), 0.0, "level");
}

#[test]
fn a_work_plane_picked_on_the_model_lies_through_the_three_points_clicked() {
    let mut h = app(BOX);
    press(&mut h, "Default 3D View");
    let m = &h.state().session.model;
    let top = m.wall(ids(&h, "wall")[0]).map(|w| m.top_z(w)).unwrap();
    press(&mut h, "Work Plane");
    press(&mut h, "Pick in View");
    assert!(h.state().work_plane.as_ref().is_some_and(|d| d.picking.is_some()), "the dialog makes way for the picks");
    // The tops of the south, east and north walls.
    for p in [[4000.0, 0.0, top], [8000.0, 3000.0, top], [4000.0, 6000.0, top]] {
        let at = h.state().screen3(p);
        click_at(&mut h, at);
    }
    assert!(h.state().selection.is_empty(), "a pick is no selection");
    assert!(h.state().work_plane.as_ref().is_some_and(|d| d.picking.is_none() && d.by_points), "back, with the points in it");
    press(&mut h, "OK");
    let p = plane(&h);
    assert!(h.state().session.model.work_plane(h.state().session.model.default_3d()).points.is_some());
    assert!((p.at(P2(4000.0, 3000.0)) - top).abs() < 30.0 && p.g.0.abs() < 0.02 && p.g.1.abs() < 0.02, "{p:?} at {top}");
}
