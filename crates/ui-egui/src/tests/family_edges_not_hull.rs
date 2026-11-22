//! FV-2: what Pick Lines takes of a form in a plan is an edge the view draws it with, as a click and a box select it by
//! (FE-20): an L-shaped form's inner edges, never the chord its hull draws across the empty corner.
use super::family_drag_move::status;
use super::family_selection::ELL;
use super::family_views::{click_family, editor, open, view};
use super::*;
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
