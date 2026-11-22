//! What Pick Lines and Mirror - Pick Axis take of a form in a plan is an edge the view draws it with, as a click and a box
//! select it by: an L-shaped form's inner edges, never the chord its hull draws across the empty corner.
use super::family_drag_move::status;
use super::family_selection::{ELL, command};
use super::family_views::{click_family, editor, open, view};
use super::*;
use crate::editor::select_box::Picked;
use bimcraft_core::family::views::Which;

fn ell() -> Harness<'static, BimApp> {
    let (mut h, _) = open(ELL, "Ell");
    view(&mut h, Some(Which::Plan));
    h
}

#[test]
fn pick_lines_takes_an_inner_edge_of_an_l_and_not_its_hulls_chord() {
    let mut h = ell();
    for label in ["Extrusion", "Pick Lines"] {
        h.get_by_role_and_label(Role::Button, label).click();
        h.run();
    }
    assert!(editor(&h).sketching());
    // The middle of the hull's chord, over the empty corner: no line there to pick.
    click_family(&mut h, [900.0, 900.0, 0.0]);
    assert!(editor(&h).sketch.as_ref().unwrap().sketch.lines.is_empty(), "the hull's chord: {}", status(&h));
    // The inner edge along y 600, from the corner to the east side: that line.
    click_family(&mut h, [900.0, 600.0, 0.0]);
    let lines = &editor(&h).sketch.as_ref().unwrap().sketch.lines;
    assert_eq!(lines.len(), 1, "{}", status(&h));
    let (a, b) = (lines[0].start(), lines[0].end());
    let mut xs = [a.0, b.0];
    xs.sort_by(f64::total_cmp);
    assert!((a.1 - 600.0).abs() < 1e-6 && (b.1 - 600.0).abs() < 1e-6, "{a:?} {b:?}");
    assert!((xs[0] - 600.0).abs() < 1e-6 && (xs[1] - 1200.0).abs() < 1e-6, "{a:?} {b:?}");
}

#[test]
fn mirror_pick_axis_takes_an_inner_edge_of_an_l_and_not_its_hulls_chord() {
    let mut h = ell();
    h.state_mut().active_family_mut().unwrap().selection.apply(&[Picked::Form(0)], crate::editor::select_box::How::Replace);
    h.run();
    command(&mut h, "Mirror - Pick Axis");
    assert!(h.state().prompt().contains("Pick Axis"), "{}", h.state().prompt());
    // The hull's chord: no axis there, nothing mirrored.
    click_family(&mut h, [900.0, 900.0, 0.0]);
    assert_eq!(editor(&h).draft_family().unwrap().forms.len(), 1, "the hull's chord: {}", status(&h));
    // The inner edge along y 600: a copy mirrored across it, from y 0 to 1000 (200..1200 → 0..1000).
    click_family(&mut h, [900.0, 600.0, 0.0]);
    let f = editor(&h).draft_family().unwrap();
    assert_eq!(f.forms.len(), 2, "{}", status(&h));
    let v = f.values("T", &std::collections::BTreeMap::new()).unwrap();
    let b = f.shapes(&v).into_iter().find(|s| s.index == 1).unwrap();
    assert!((b.min[1] - 0.0).abs() < 1e-6 && (b.max[1] - 1000.0).abs() < 1e-6, "{b:?}");
}
