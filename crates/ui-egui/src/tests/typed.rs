//! Numbers typed while drawing: a line's or a wall's length, and with Tab its angle, at the cursor; the same in both profiles.

use super::*;

/// A detail line's tool in hand, a point clicked and the cursor out at `to` from it: what the cursor says then.
fn line_under_the_cursor(to: P2) -> Harness<'static, BimApp> {
    let mut h = app("");
    press(&mut h, "Annotate");
    press(&mut h, "Detail Line");
    click(&mut h, P2(0.0, 0.0));
    h.hover_at(h.state().screen(to));
    h.run();
    h
}

/// What the cursor says as a shape is drawn from `base` toward `to`, as the line, wall and polyline tools show it: the
/// text `shape_dimensions` puts on screen.
fn cursor_text(h: &Harness<BimApp>, base: P2, to: P2) -> String {
    let app = h.state();
    let texts = app.shape_texts(&app.session.model, &[base], to);
    assert_eq!(texts.len(), 1, "one number at the cursor: {texts:?}");
    texts[0].1.clone()
}

#[test]
fn a_lines_cursor_says_which_of_its_length_and_angle_enter_takes() {
    let mut h = line_under_the_cursor(P2(3000.0, 0.0));
    let before = cursor_text(&h, P2(0.0, 0.0), P2(3000.0, 0.0));
    assert!(before.starts_with("3000 mm"), "the length so far: {before:?}");
    assert!(!before.contains('['), "and the length is what Enter takes first: {before:?}");
    key(&mut h, Key::Tab);
    let after = cursor_text(&h, P2(0.0, 0.0), P2(3000.0, 0.0));
    assert!(after.contains("[0°]"), "Tab puts the angle in brackets, the one Enter takes now: {after:?}");
    key(&mut h, Key::Tab);
    let back = cursor_text(&h, P2(0.0, 0.0), P2(3000.0, 0.0));
    assert!(!back.contains('['), "Tab again is the length's: {back:?}");
}

/// The same as seen: the label by the cursor changes when Tab moves it to the angle. It did not: the line's preview drew
/// its own reading, which knew nothing of Tab, so the angle could be typed but nothing said so.
#[test]
fn tab_changes_what_a_detail_lines_cursor_shows() {
    let at = P2(3000.0, 0.0);
    let mut h = line_under_the_cursor(at);
    let Ok(before) = h.render() else { return }; // no renderer on this machine
    key(&mut h, Key::Tab);
    let Ok(after) = h.render() else { return };
    // The label's own box, above and right of the cursor: nothing else in it moves, so a moved focus ring in the ribbon
    // cannot pass for the angle.
    let ppp = h.ctx.pixels_per_point();
    let cursor = h.state().screen(at);
    let (x0, y0) = (((cursor.x + 4.0) * ppp) as u32, ((cursor.y - 36.0) * ppp) as u32);
    let (x1, y1) = (((cursor.x + 340.0) * ppp) as u32, ((cursor.y - 8.0) * ppp) as u32);
    let (w, ht) = (before.width(), before.height());
    assert!(x1 <= w && y1 <= ht, "the label is on screen: {cursor:?} in {w}x{ht}");
    let differs = (y0..y1).any(|y| (x0..x1).any(|x| before.get_pixel(x, y) != after.get_pixel(x, y)));
    assert!(differs, "Tab must change what the cursor says");
}

#[test]
fn a_walls_angle_is_typed_after_its_length_with_tab_in_both_profiles() {
    for rev in [false, true] {
        let mut h = if rev { app("") } else { bimcraft("") };
        tool(&mut h, "Wall");
        click(&mut h, P2(0.0, 0.0));
        h.event(Event::Text("3000".into()));
        h.run();
        key(&mut h, Key::Tab);
        h.event(Event::Text("90".into()));
        h.run();
        key(&mut h, Key::Enter);
        key(&mut h, Key::Escape);
        let w = walls(&h);
        assert_eq!(w.len(), 1, "rev {rev}");
        let (a, b) = w[0];
        assert!(a.dist(P2(0.0, 0.0)) < 1e-6 && b.dist(P2(0.0, 3000.0)) < 1e-6, "rev {rev}: {a:?} {b:?}");
    }
}
