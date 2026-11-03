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

#[test]
fn which_way_a_door_opens_is_picked_from_four_pictures_in_both_profiles() {
    for rev in [false, true] {
        let script = format!("{BOX}\ndoor {{\"at\": [2000, 0]}}");
        let mut h = if rev { app(&script) } else { bimcraft(&script) };
        let door = ids(&h, "door")[0];
        h.state_mut().selection = vec![door];
        h.run();
        let flips = |h: &Harness<BimApp>| {
            let on = |key: &str| field(h, door, key).as_bool().unwrap_or(false);
            (on("flip_hand"), on("flip_facing"))
        };
        h.get_by_role_and_label(Role::RadioButton, "Flipped both").click();
        h.run();
        assert_eq!(flips(&h), (true, true), "rev {rev}");
        h.get_by_role_and_label(Role::RadioButton, "Flipped facing").click();
        h.run();
        assert_eq!(flips(&h), (false, true), "rev {rev}");
        h.get_by_role_and_label(Role::RadioButton, "As placed").click();
        h.run();
        assert_eq!(flips(&h), (false, false), "rev {rev}");
    }
}

#[test]
fn a_rooms_colour_on_plans_is_picked_from_swatches_in_both_profiles() {
    for rev in [false, true] {
        let script = format!("{BOX}\nroom {{\"point\": [4000, 3000]}}");
        let mut h = if rev { app(&script) } else { bimcraft(&script) };
        let room = ids(&h, "room")[0];
        h.state_mut().selection = vec![room];
        h.run();
        h.get_by_role_and_label(Role::RadioButton, "Sage").click();
        h.run();
        assert_eq!(field(&h, room, "colour"), json!([214, 229, 210]), "rev {rev}");
        h.get_by_role_and_label(Role::RadioButton, "No fill").click();
        h.run();
        assert!(field(&h, room, "colour").is_null(), "rev {rev}");
    }
}
