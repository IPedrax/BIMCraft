//! The Inspector's pictures, the same in both profiles: a type's layers drawn to their thicknesses, a wall's location line on them,
//! which way a door opens, a room's colour, a floor's slope.

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

#[test]
fn a_floors_slope_is_drawn_at_its_angle_and_stepped_on_its_picture_in_both_profiles() {
    for rev in [false, true] {
        let script = format!("{BOX}\nfloor {{\"slope\": 2, \"arrow\": [[0, 3000], [8000, 3000]]}}\nfloor {{\"offset\": 3000}}");
        let mut h = if rev { app(&script) } else { bimcraft(&script) };
        let [sloped, flat] = ids(&h, "floor")[..] else { panic!("two floors") };
        h.state_mut().selection = vec![sloped];
        h.run();
        // Its section at its angle, which the picture says.
        assert!(h.query_by_label("Floor slope: 2° (3.49%)").is_some(), "rev {rev}");
        let slope = |h: &Harness<BimApp>| field(h, sloped, "slope").as_f64().unwrap_or(0.0);
        // A step sets it, through the command the Slope field runs.
        press(&mut h, "1%");
        assert!((slope(&h).to_radians().tan() - 0.01).abs() < 1e-9, "rev {rev}: {}", slope(&h));
        press(&mut h, "Flat");
        assert_eq!(slope(&h), 0.0, "rev {rev}");
        // One that falls toward its arrow's head keeps falling.
        h.state_mut().session.execute("modify", &json!({"id": sloped, "slope": -3})).unwrap();
        h.run();
        press(&mut h, "2%");
        assert!((slope(&h).to_radians().tan() + 0.02).abs() < 1e-9, "rev {rev}: {}", slope(&h));
        // With no slope arrow it is flat, and the picture starts the tool that draws one.
        h.state_mut().selection = vec![flat];
        h.run();
        assert!(h.query_by_label("1%").is_none(), "rev {rev}");
        press(&mut h, "Draw a slope arrow");
        assert!(matches!(h.state().tool, Tool::SlopeArrow(None)), "rev {rev}");
    }
}

#[test]
fn what_an_element_is_tied_to_is_a_link_that_selects_it_in_both_profiles() {
    for rev in [false, true] {
        let script = format!("{BOX}\ndoor {{\"at\": [2000, 0]}}\nroof {{}}");
        let mut h = if rev { app(&script) } else { bimcraft(&script) };
        let (walls, door, roof) = (ids(&h, "wall"), ids(&h, "door")[0], ids(&h, "roof")[0]);
        let (south, east, west) = (walls[0], walls[1], walls[3]);
        let session = &mut h.state_mut().session;
        session.execute("modify", &json!({"ids": [south], "attach_top": roof})).unwrap();
        let d = session.execute("dimension", &json!({"refs": [{"id": west}, {"id": east}], "at": [4000, 3000]})).unwrap()["id"].clone();
        session.execute("dimension_constraint", &json!({"id": d, "locked": [0]})).unwrap();
        // By the link's own action: a wall's rows run below the palette's fold.
        let follow = |h: &mut Harness<BimApp>, link: String| {
            h.get_by_role_and_label(Role::Link, &link).click_accesskit();
            h.run();
            h.state().selection.clone()
        };
        h.state_mut().selection = vec![door];
        h.run();
        // The door's host; the wall's openings and the roof its top is attached to; the walls attached to the roof.
        assert_eq!(follow(&mut h, format!("Wall {south}")), [south], "rev {rev}");
        assert_eq!(follow(&mut h, format!("Roof {roof}")), [roof], "rev {rev}");
        assert_eq!(follow(&mut h, format!("Wall {south}")), [south], "rev {rev}");
        assert_eq!(follow(&mut h, format!("Door {door}")), [door], "rev {rev}");
        // The locked dimension that holds a wall.
        h.state_mut().selection = vec![west];
        h.run();
        assert_eq!(follow(&mut h, format!("Dimension {d}")), [d.as_u64().unwrap_or_default()], "rev {rev}");
    }
}
