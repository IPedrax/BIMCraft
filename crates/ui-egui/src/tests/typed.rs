//! Numbers typed while drawing: a line's or a wall's length, and with Tab its angle, at the cursor; the same in both profiles.

use super::*;

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
