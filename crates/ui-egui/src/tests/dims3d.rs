//! Dimensions in the 3D view: the Aligned Dimension tool (DI) picks two corners or edges of the model and then where its
//! line goes; the dimension is the view's, drawn over its picture on either painter, and stays as the view turns.

use super::*;
use bimcraft_core::dims3d;
use bimcraft_core::model::Dimension3D;
use bimcraft_core::view3d::P3;

/// The 3D view of the box, with the Aligned Dimension tool on: in Rev's profile, or with `own` BIMCraft's.
fn dimensioning_in(own: bool) -> Harness<'static, BimApp> {
    let mut h = if own { bimcraft(BOX) } else { app(BOX) };
    shortcut(&mut h, "3D");
    assert_eq!(h.state().sheet, Sheet::View3D);
    shortcut(&mut h, "DI");
    assert!(matches!(h.state().tool, Tool::Dimension(..)), "{:?}", h.state().tool);
    h
}

fn dimensioning() -> Harness<'static, BimApp> {
    dimensioning_in(false)
}

/// Clicks a point off each of `points` on the model, in order: a corner there is snapped to (another of the model's within a
/// few points, where walls join, would be nearer a click further off).
fn clicks(h: &mut Harness<BimApp>, points: &[P3]) {
    for p in points {
        let at = h.state().screen3(*p) + vec2(1.0, 0.5);
        click_at(h, at);
    }
}

/// The 3D dimensions of the model, by id.
fn dims(h: &Harness<BimApp>) -> Vec<(Id, Dimension3D)> {
    let m = &h.state().session.model;
    m.elements.iter().filter_map(|(id, e)| if let Element::Dimension3D(d) = e { Some((*id, d.clone())) } else { None }).collect()
}

#[test]
fn an_aligned_dimension_between_two_corners_in_the_3d_view_reports_its_length() {
    for own in [false, true] {
        let mut h = dimensioning_in(own);
        // The south wall's outer top corners, snapped to; then where its line goes, under the wall.
        clicks(&mut h, &[[-100.0, -100.0, 3000.0], [8100.0, -100.0, 3000.0]]);
        assert!(dims(&h).is_empty(), "two picks, and its line still to place");
        let at = h.state().screen3([4000.0, -100.0, 0.0]) + vec2(0.0, 40.0);
        click_at(&mut h, at);
        let made = dims(&h);
        assert_eq!(made.len(), 1, "{}", h.state().status.text);
        let (id, d) = &made[0];
        assert_eq!(d.refs.map(|r| r.at), [[-100.0, -100.0, 3000.0], [8100.0, -100.0, 3000.0]], "on the corners");
        assert_eq!(field(&h, *id, "length"), json!(8200.0));
        assert!(!h.state().status.error && h.state().status.text.contains("8200"), "{}", h.state().status.text);
        assert_eq!(h.state().session.model.elements.id(d.view), h.state().session.model.default_3d(), "the view's");
    }
}

#[test]
fn between_two_parallel_edges_it_measures_square_to_them() {
    let mut h = dimensioning();
    // The middles of the south and north walls' outer top edges: no corner near.
    clicks(&mut h, &[[4000.0, -100.0, 3000.0], [4000.0, 6100.0, 3000.0]]);
    let at = h.state().screen3([9000.0, 3000.0, 3000.0]);
    click_at(&mut h, at);
    let made = dims(&h);
    assert_eq!(made.len(), 1, "{}", h.state().status.text);
    assert!(made[0].1.refs.iter().all(|r| r.edge.is_some()), "on the edges: {:?}", made[0].1.refs);
    assert_eq!(field(&h, made[0].0, "length"), json!(6200.0));
}

#[test]
fn a_dimension_in_the_3d_view_stays_as_the_view_orbits_and_is_picked_where_it_is_drawn() {
    let mut h = dimensioning();
    clicks(&mut h, &[[-100.0, -100.0, 3000.0], [8100.0, -100.0, 3000.0]]);
    let at = h.state().screen3([4000.0, -100.0, 0.0]) + vec2(0.0, 40.0);
    click_at(&mut h, at);
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Modify);
    let (id, d) = dims(&h).pop().expect("placed");
    // Turned round the background's target.
    let before = h.state().orbit.camera.unwrap();
    let corner = h.state().orbit.rect.min + vec2(6.0, 6.0);
    drag_screen(&mut h, corner, corner + vec2(80.0, 30.0), PointerButton::Primary);
    assert!((h.state().orbit.camera.unwrap().azimuth - before.azimuth).abs() > 10.0, "it turned");
    assert_eq!(dims(&h), vec![(id, d.clone())], "the same dimension, where it was in the model");
    // A click on its line, where the turned view draws it, picks it.
    let (a, b) = dims3d::line(&d);
    let along = [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) / 3.0);
    let on = h.state().screen3(along);
    click_at(&mut h, on);
    assert_eq!(h.state().selection, vec![id], "{}", h.state().status.text);
}
