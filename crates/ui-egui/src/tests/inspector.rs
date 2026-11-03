//! The Inspector's pictures, the same in both profiles: a type's layers drawn to their thicknesses, a wall's location line on them.

use super::*;

#[test]
fn a_walls_layers_are_drawn_and_a_click_on_them_sets_its_location_line_in_both_profiles() {
    for rev in [false, true] {
        let mut h = if rev { app(BOX) } else { bimcraft(BOX) };
        let wall = ids(&h, "wall")[0];
        h.state_mut().selection = vec![wall];
        h.run();
        // At its exterior face, the line the face is.
        let picture = h.get_by_label("Wall layers").rect();
        click_at(&mut h, picture.left_center() + vec2(1.0, 0.0));
        assert_eq!(field(&h, wall, "location"), json!("exterior-finish"), "rev {rev}");
        // In its middle, the centre line, which the wall's JSON leaves out.
        click_at(&mut h, picture.center());
        assert!(field(&h, wall, "location").as_str().is_none_or(|l| l == "centre"), "rev {rev}: {}", field(&h, wall, "location"));
    }
}
