//! In the family's {3D}, Mirror - Pick Axis, Offset and Align pick references in the picture: reference planes (the datums
//! too), reference lines and the forms' edges as drawn.

use super::family_drag_move::{EDGE, g, pick, status};
use super::family_modify_3d::{in_3d, tool};
use super::family_selection::command;
use super::*;
use crate::editor::modify::Tool as M;
use crate::editor::select_box::Picked;

/// The Movers family in its {3D}, zoomed out to show Edge (x 1000) and the reference line (y 1200).
fn zoomed() -> Harness<'static, BimApp> {
    let mut h = in_3d();
    if let Some(c) = h.state_mut().active_family_mut().and_then(|f| f.orbit.camera.as_mut()) {
        c.scale *= 0.5;
    }
    for _ in 0..4 {
        h.run();
    }
    h
}

/// A click in {3D} a few pixels off where family point `p` shows.
fn click_off(h: &mut Harness<BimApp>, p: [f64; 3]) {
    let at = h.state().family_screen3(p).expect("{3D} drawn") + egui::vec2(3.0, 2.0);
    click_at(h, at);
}

/// Form `i`'s sketch's extent: least and most x, least and most y.
fn extent(h: &Harness<BimApp>, i: usize) -> [f64; 4] {
    let pts = super::family_drag_move::points(h, i);
    let f = |k: usize, most: bool| {
        pts.iter().map(|p| p[k]).fold(if most { f64::NEG_INFINITY } else { f64::INFINITY }, if most { f64::max } else { f64::min })
    };
    [f(0, false), f(0, true), f(1, false), f(1, true)]
}

fn about(a: [f64; 4], b: [f64; 4]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-3)
}

/// Mirror - Pick Axis in hand in {3D} on box A, not copying.
fn mirroring() -> Harness<'static, BimApp> {
    let mut h = zoomed();
    pick(&mut h, &[Picked::Form(0)]);
    command(&mut h, "Mirror - Pick Axis");
    super::family_drag_move::ed(&mut h).views.modify_options.mirror_copy = false;
    assert_eq!(tool(&h), Some(M::Mirror { draw: false, start: None }), "{}", status(&h));
    h
}

#[test]
fn mirror_pick_axis_in_3d_takes_a_plane_a_reference_line_or_a_forms_edge() {
    // Edge (x 1000), by the line it was drawn as: box A from x 100 to 500 goes to 1500 to 1900.
    let mut h = mirroring();
    click_off(&mut h, [1000.0, 600.0, 0.0]);
    assert!(about(extent(&h, 0), [1500.0, 1900.0, 100.0, 500.0]), "{:?}: {}", extent(&h, 0), status(&h));
    // The reference line along y 1200: from y 100 to 500 to 1900 to 2300.
    let mut h = mirroring();
    click_off(&mut h, [0.0, 1200.0, 0.0]);
    assert!(about(extent(&h, 0), [100.0, 500.0, 1900.0, 2300.0]), "{:?}: {}", extent(&h, 0), status(&h));
    // Box B's east edge on its top (x -600), seen square on to Ref. Level: from x 100 to 500 to -1700 to -1300.
    let mut h = mirroring();
    click_off(&mut h, [-600.0, 300.0, 300.0]);
    assert!(about(extent(&h, 0), [-1700.0, -1300.0, 100.0, 500.0]), "{:?}: {}", extent(&h, 0), status(&h));
}

#[test]
fn mirror_pick_axis_in_3d_refuses_a_line_along_the_planes_normal_and_a_plane_parallel_to_it() {
    use super::family_drag_move::ed;
    // An upright reference line, on Center (Front/Back): square to Ref. Level, it is a point on it.
    let mut h = mirroring();
    ed(&mut h).lines.push(bimcraft_core::family::RefLine {
        guid: bimcraft_core::ids::Guid::new(),
        start: [1400.0, 0.0, 0.0],
        end: [1400.0, 0.0, 900.0],
        host: bimcraft_core::family::PlaneRef::Plane(bimcraft_core::family::CENTER_FRONT_BACK),
        ..Default::default()
    });
    h.run();
    let before = extent(&h, 0);
    click_off(&mut h, [1400.0, 0.0, 450.0]);
    assert!(about(extent(&h, 0), before), "nothing mirrored");
    assert!(status(&h).contains("square to the work plane"), "{}", status(&h));
    // A plane level with Ref. Level, 600 over it: it crosses the plane nowhere.
    let mut h = mirroring();
    let shelf: bimcraft_core::family::RefPlane = serde_json::from_value(serde_json::json!({
        "guid": bimcraft_core::ids::Guid::new(), "name": "Shelf", "start": [-500, 800, 600], "end": [500, 800, 600], "up": [0, 1, 0]
    }))
    .unwrap();
    ed(&mut h).planes.push(shelf);
    h.run();
    click_off(&mut h, [0.0, 800.0, 600.0]);
    assert!(about(extent(&h, 0), before), "nothing mirrored");
    assert!(status(&h).contains("parallel to the work plane"), "{}", status(&h));
    // A line lying on the plane is an axis: the reference line, as above.
    click_off(&mut h, [0.0, 1200.0, 0.0]);
    assert!(about(extent(&h, 0), [100.0, 500.0, 1900.0, 2300.0]), "{:?}: {}", extent(&h, 0), status(&h));
}

/// A click in {3D} right where family point `p` shows.
fn click_on(h: &mut Harness<BimApp>, p: [f64; 3]) {
    let at = h.state().family_screen3(p).expect("{3D} drawn");
    click_at(h, at);
}

/// Offset in hand in {3D}, its distance 300, moving (no copy).
fn offsetting() -> Harness<'static, BimApp> {
    let mut h = zoomed();
    command(&mut h, "Offset");
    assert_eq!(tool(&h), Some(M::Offset), "{}", status(&h));
    super::family_drag_move::ed(&mut h).views.modify_options.offset_copy = false;
    h
}

#[test]
fn offset_in_3d_takes_a_plane_or_a_reference_line_toward_the_side_clicked() {
    use super::family_drag_move::{LINE, family};
    // Edge (x 1000), clicked just east of the line it was drawn as: 300 east.
    let mut h = offsetting();
    click_on(&mut h, [1010.0, 600.0, 0.0]);
    let edge = family(&h).planes.iter().find(|p| p.guid == g(EDGE)).unwrap().clone();
    assert!((edge.start[0] - 1300.0).abs() < 1e-6 && (edge.end[0] - 1300.0).abs() < 1e-6, "{edge:?}: {}", status(&h));
    // The reference line (y 1200), clicked just south of it: 300 south.
    let mut h = offsetting();
    click_on(&mut h, [0.0, 1190.0, 0.0]);
    let line = family(&h).lines.iter().find(|l| l.guid == g(LINE)).unwrap().clone();
    assert!((line.start[1] - 900.0).abs() < 1e-6 && (line.end[1] - 900.0).abs() < 1e-6, "{line:?}: {}", status(&h));
    // With Copy, a new plane 300 off, Edge where it was.
    let mut h = offsetting();
    super::family_drag_move::ed(&mut h).views.modify_options.offset_copy = true;
    let planes = family(&h).planes.len();
    click_on(&mut h, [1010.0, 600.0, 0.0]);
    assert_eq!(family(&h).planes.len(), planes + 1, "{}", status(&h));
    assert!(family(&h).planes.iter().any(|p| (p.start[0] - 1300.0).abs() < 1e-6), "a copy at x 1300");
}

#[test]
fn offset_in_3d_refuses_a_forms_edge_saying_what_it_takes() {
    let mut h = offsetting();
    let before = extent(&h, 0);
    click_off(&mut h, [500.0, 300.0, 300.0]);
    assert!(about(extent(&h, 0), before));
    assert!(status(&h).contains("Offset takes reference planes and reference lines"), "{}", status(&h));
}

/// Align in hand in {3D}.
fn aligning() -> Harness<'static, BimApp> {
    let mut h = zoomed();
    command(&mut h, "Align");
    assert!(super::family_views::editor(&h).views.aligning.is_some(), "Align in hand in {{3D}}: {}", status(&h));
    h
}

#[test]
fn align_in_3d_moves_what_is_picked_second_onto_the_reference_and_its_padlock_locks_them() {
    use super::family_views::editor;
    let mut h = aligning();
    assert!(h.state().prompt().contains("reference to align to"), "{}", h.state().prompt());
    // Edge (x 1000), then box A's east side by its top edge: the side goes onto Edge, the lines joined to it stretched to
    // it, as a plan's Align moves it; box A runs from x 100 to 1000.
    click_off(&mut h, [1000.0, 600.0, 0.0]);
    assert!(h.state().prompt().contains("moves onto it"), "{}", h.state().prompt());
    click_off(&mut h, [500.0, 300.0, 300.0]);
    assert!(about(extent(&h, 0), [100.0, 1000.0, 100.0, 500.0]), "{:?}: {}", extent(&h, 0), status(&h));
    assert_eq!(editor(&h).steps(), 1, "one step");
    // Its padlock, open where the second click was: a click locks the two.
    let dims = editor(&h).dimensions.len();
    let lock = editor(&h).views.align_lock_3d.as_ref().map(|l| l.2).expect("a padlock offered");
    let (dx, dy) = crate::editor::modify::PADLOCK_3D;
    let at = h.state().family_screen3(lock).expect("on screen") + egui::vec2(dx, dy);
    click_at(&mut h, at);
    assert_eq!(editor(&h).dimensions.len(), dims + 1, "{}", status(&h));
    let d = editor(&h).dimensions.last().unwrap().clone();
    assert!(d.alignment && d.is_locked(0) && d.view == bimcraft_core::family::constraints::DimView::Plan, "{d:?}");
}

#[test]
fn align_in_3d_says_why_where_it_takes_nothing() {
    // The reference twice.
    let mut h = aligning();
    click_off(&mut h, [1000.0, 600.0, 0.0]);
    click_off(&mut h, [1000.0, 600.0, 0.0]);
    assert!(status(&h).contains("That is the reference"), "{}", status(&h));
    // An upright edge has no face square to Ref. Level through it but two, and runs along what the plane faces.
    let mut h = aligning();
    click_off(&mut h, [1000.0, 600.0, 0.0]);
    let before = extent(&h, 0);
    click_on(&mut h, [500.0, 100.0, 150.0]);
    assert!(about(extent(&h, 0), before), "nothing moved");
    assert!(status(&h).contains("square to the work plane"), "{}", status(&h));
}

/// Mirror - Pick Axis over Edge, Offset over the reference line and Align's padlock, in {3D} in the three looks, when
/// `BIMCRAFT_MODIFY_3D_REFERENCES_DUMP` names a folder.
#[test]
fn references_in_3d_screenshots() {
    use super::family_drag_move::family_with;
    let Ok(dir) = std::env::var("BIMCRAFT_MODIFY_3D_REFERENCES_DUMP") else { return };
    std::fs::create_dir_all(&dir).unwrap();
    for (look, dark, light_canvas) in [("light", false, false), ("dark", true, false), ("dark-ui-light-canvas", true, true)] {
        for name in ["mirror", "offset", "align"] {
            // The ribbon's harness, whose renderer draws.
            let mut h = app(&family_with(""));
            h.set_size(egui::vec2(1400.0, 900.0));
            h.state_mut().edit_family("Movers");
            h.ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
            h.state_mut().light_canvas = light_canvas;
            for _ in 0..4 {
                h.run();
            }
            super::family_views::view(&mut h, None);
            for _ in 0..4 {
                h.run();
            }
            if let Some(c) = h.state_mut().active_family_mut().and_then(|f| f.orbit.camera.as_mut()) {
                c.scale *= 0.5;
            }
            for _ in 0..2 {
                h.run();
            }
            let hover = match name {
                "mirror" => {
                    pick(&mut h, &[Picked::Form(0)]);
                    command(&mut h, "Mirror - Pick Axis");
                    [1000.0, 600.0, 0.0]
                }
                "offset" => {
                    command(&mut h, "Offset");
                    [0.0, 1210.0, 0.0]
                }
                _ => {
                    command(&mut h, "Align");
                    click_off(&mut h, [1000.0, 600.0, 0.0]);
                    click_off(&mut h, [500.0, 300.0, 300.0]);
                    [500.0, 300.0, 300.0]
                }
            };
            let at = h.state().family_screen3(hover).expect("{3D} drawn") + egui::vec2(3.0, 2.0);
            h.hover_at(at);
            for _ in 0..3 {
                h.run();
            }
            h.render().unwrap().save(std::path::Path::new(&dir).join(format!("modify-3d-references-{name}-{look}.png"))).unwrap();
        }
    }
}

/// A reference plane of the family's own through `start` and `end`, `up` the other way it runs, put straight into the
/// editor; its GUID.
fn plane(h: &mut Harness<BimApp>, start: [f64; 3], end: [f64; 3], up: [f64; 3]) -> bimcraft_core::ids::Guid {
    let guid = bimcraft_core::ids::Guid::new();
    let p: bimcraft_core::family::RefPlane =
        serde_json::from_value(serde_json::json!({"guid": guid, "start": start, "end": end, "up": up})).unwrap();
    super::family_drag_move::ed(h).planes.push(p);
    h.run();
    guid
}

#[test]
fn offset_in_3d_reads_the_side_beside_a_plane_drawn_up_off_the_work_plane() {
    use super::family_drag_move::family;
    // A plane x 300 drawn upright, from z 0 to 1600: clicked 10 east of it at z 800, it goes 300 east.
    let mut h = offsetting();
    let g = plane(&mut h, [300.0, -500.0, 0.0], [300.0, -500.0, 1600.0], [0.0, 1.0, 0.0]);
    click_on(&mut h, [310.0, -500.0, 800.0]);
    let p = family(&h).planes.iter().find(|p| p.guid == g).unwrap().clone();
    assert!((p.start[0] - 600.0).abs() < 1e-6, "{p:?}: {}", status(&h));
}

#[test]
fn a_datum_is_picked_in_3d_as_an_axis_and_offset_refuses_it() {
    // Center (Left/Right), drawn along y at x 0, south of the boxes: box A from x 100 to 500 goes to -500 to -100.
    let mut h = mirroring();
    click_off(&mut h, [0.0, -800.0, 0.0]);
    assert!(about(extent(&h, 0), [-500.0, -100.0, 100.0, 500.0]), "{:?}: {}", extent(&h, 0), status(&h));
    let mut h = offsetting();
    click_off(&mut h, [0.0, -800.0, 0.0]);
    assert!(status(&h).contains("A datum stays where it is"), "{}", status(&h));
}

#[test]
fn align_in_3d_takes_a_plane_drawn_along_the_normal_and_refuses_one_level_with_the_work_plane() {
    use super::family_drag_move::family;
    // A plane x 300 drawn upright: Edge (x 1000) aligned onto it.
    let mut h = aligning();
    plane(&mut h, [300.0, -500.0, 0.0], [300.0, -500.0, 1600.0], [0.0, 1.0, 0.0]);
    click_on(&mut h, [310.0, -500.0, 800.0]);
    click_off(&mut h, [1000.0, 600.0, 0.0]);
    let edge = family(&h).planes.iter().find(|p| p.guid == g(EDGE)).unwrap().clone();
    assert!((edge.start[0] - 300.0).abs() < 1e-6, "{edge:?}: {}", status(&h));
    // A plane level with Ref. Level, 600 over it: no plan sees it edge on.
    let mut h = aligning();
    plane(&mut h, [-500.0, 800.0, 600.0], [500.0, 800.0, 600.0], [0.0, 1.0, 0.0]);
    click_off(&mut h, [0.0, 800.0, 600.0]);
    assert!(status(&h).contains("square to the work plane"), "{}", status(&h));
}

/// Box A's top (300 high) aligned onto a plane level 600 high, in {3D} with work plane `work` (none: Ref. Level); the
/// padlock's view, if one is offered, then clicked.
fn top_onto_a_shelf(work: Option<[[f64; 3]; 3]>) -> (Harness<'static, BimApp>, Option<bimcraft_core::family::constraints::DimView>) {
    use super::family_drag_move::ed;
    let mut h = zoomed();
    plane(&mut h, [-500.0, 800.0, 600.0], [500.0, 800.0, 600.0], [0.0, 1.0, 0.0]);
    if let Some([s, e, u]) = work {
        let w = plane(&mut h, s, e, u);
        ed(&mut h).views.set_work_plane(bimcraft_core::family::PlaneRef::Plane(w));
        h.run();
    }
    command(&mut h, "Align");
    click_off(&mut h, [0.0, 800.0, 600.0]);
    // Box A's south edge on its top, running along x.
    click_off(&mut h, [300.0, 100.0, 300.0]);
    let to = serde_json::to_value(&super::family_drag_move::family(&h).forms[0]).unwrap()["to"].clone();
    assert_eq!(to.as_f64(), Some(600.0), "{to}: {}", status(&h));
    let view = super::family_views::editor(&h).views.align_lock_3d.map(|l| l.3);
    if let Some(at) = super::family_views::editor(&h).views.align_lock_3d.map(|l| l.2) {
        let (dx, dy) = crate::editor::modify::PADLOCK_3D;
        let p = h.state().family_screen3(at).expect("on screen") + egui::vec2(dx, dy);
        click_at(&mut h, p);
    }
    (h, view)
}

#[test]
fn align_in_3d_locks_with_the_view_that_sees_the_work_plane_flat_and_offers_no_padlock_on_an_oblique_one() {
    use super::family_views::editor;
    use bimcraft_core::family::constraints::DimView;
    // Center (Front/Back), upright through the origin facing front: the Front elevation's lock.
    let (h, view) = top_onto_a_shelf(Some([[-1000.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [0.0, 0.0, 1.0]]));
    assert_eq!(view, Some(DimView::Front));
    let d = editor(&h).dimensions.last().unwrap().clone();
    assert!(d.alignment && d.is_locked(0) && d.view == DimView::Front, "{d:?}");
    // An upright plane on the diagonal, which no plan or elevation sees flat: aligned, and no padlock.
    let (h, view) = top_onto_a_shelf(Some([[-1000.0, -1000.0, 0.0], [1000.0, 1000.0, 0.0], [0.0, 0.0, 1.0]]));
    assert_eq!(view, None);
    assert!(editor(&h).dimensions.iter().all(|d| !d.alignment), "no lock made");
}
