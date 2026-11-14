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
fn a_room_types_colour_floor_and_ceiling_are_set_in_its_type_properties_in_both_profiles() {
    for rev in [false, true] {
        let script = format!("{BOX}\nroom {{\"point\": [4000, 3000]}}");
        let mut h = if rev { app(&script) } else { bimcraft(&script) };
        let room = ids(&h, "room")[0];
        h.state_mut().selection = vec![room];
        h.run();
        // The colour is the type's: the palette has no swatches of a room's own.
        assert!(h.query_by_role_and_label(Role::RadioButton, "Sage").is_none(), "rev {rev}");
        press(&mut h, "Edit Type");
        h.get_by_role_and_label(Role::RadioButton, "Sage").click();
        h.run();
        // The floor and the ceiling its rooms have, as its lists pick them.
        let rows = h.state_mut().type_dialog.as_mut().and_then(|d| d.room.as_mut()).unwrap();
        (rows.floor, rows.ceiling) = ("Generic - 200mm".into(), "Generic - 20mm".into());
        h.run();
        press(&mut h, "OK");
        let m = &h.state().session.model;
        let t = &m.types.room["Standard"];
        assert_eq!(
            (t.colour, t.floor.as_deref(), t.ceiling.as_deref()),
            (Some([214, 229, 210]), Some("Generic - 200mm"), Some("Generic - 20mm"))
        );
        assert_eq!((ids(&h, "floor").len(), ids(&h, "ceiling").len()), (1, 1), "rev {rev}: the room's own");
        // Without a fill and a floor again.
        press(&mut h, "Edit Type");
        let rows = h.state_mut().type_dialog.as_mut().and_then(|d| d.room.as_mut()).unwrap();
        assert!(rows.fill && rows.floor == "Generic - 200mm");
        (rows.fill, rows.floor) = (false, String::new());
        h.run();
        press(&mut h, "OK");
        let t = &h.state().session.model.types.room["Standard"];
        assert_eq!((t.colour, t.floor.as_deref()), (None, None), "rev {rev}");
        assert_eq!((ids(&h, "floor").len(), ids(&h, "ceiling").len()), (0, 1), "rev {rev}");
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

#[test]
fn a_walls_top_offset_is_a_row_under_its_top_constraint_in_both_profiles() {
    for rev in [false, true] {
        let script = format!("{BOX}\nwall {{\"points\": [[0, -2000], [4000, -2000]], \"height\": 2700}}");
        let mut h = if rev { app(&script) } else { bimcraft(&script) };
        let (connected, free) = (ids(&h, "wall")[0], ids(&h, "wall")[4]);
        let value = |h: &Harness<BimApp>| h.get_by_role_and_label(Role::TextInput, "Top Offset").value().unwrap_or_default();
        // To Level 2, it shows 0 where the wall's JSON leaves the offset out, and sets one through `modify`.
        h.state_mut().selection = vec![connected];
        h.run();
        assert_eq!(value(&h), "0", "rev {rev}");
        type_into(&mut h, "Top Offset", "300");
        key(&mut h, Key::Enter);
        assert_eq!((field(&h, connected, "top_offset"), field(&h, connected, "height")), (json!(300.0), json!(3300.0)), "rev {rev}");
        assert_eq!(h.state().session.history().last(), Some(&"Modify"), "rev {rev}");
        assert_eq!(value(&h), "300", "rev {rev}");
        // A negative one takes the top down; one that takes it to the base is refused, the wall staying as it was.
        type_into(&mut h, "Top Offset", "-3000");
        key(&mut h, Key::Enter);
        assert!(h.state().status.error && h.state().status.text.contains("tall"), "rev {rev}: {}", h.state().status.text);
        assert_eq!(field(&h, connected, "top_offset"), json!(300.0), "rev {rev}");
        // An unconnected wall has no top constraint to offset from: no row.
        h.state_mut().selection = vec![free];
        h.run();
        assert!(h.query_by_role_and_label(Role::TextInput, "Top Offset").is_none(), "rev {rev}");
    }
}

/// The 900 mm Single Door family draws its open leaf between the two jambs' inner faces (800 mm).
fn preview_leaf(h: &Harness<BimApp>, hinge: P2, tip: P2) -> bool {
    let (a, b) = (h.state().screen(hinge), h.state().screen(tip));
    h.output().shapes.iter().any(|s| {
        matches!(&s.shape, egui::Shape::Path(p)
        if p.points.len() == 2 && p.points[0].distance(a) < 1.0 && p.points[1].distance(b) < 1.0)
    })
}

#[test]
fn door_space_and_pointer_choose_the_leaf_shown_before_and_after_a_click() {
    // The south wall runs east; the north wall runs west. The same world-side pointer must
    // therefore choose opposite facing flags. Neither hovering nor pressing Space places a door.
    for (wall_y, along) in [(0.0, 1.0), (6000.0, -1.0)] {
        let mut h = app(BOX);
        tool(&mut h, "Door");
        let x = 3000.0;
        let point_at = |side: f64| P2(x, wall_y + side * 60.0);
        let leaf = |hand: f64, side: f64| {
            let hinge_x = x + hand * along * 400.0;
            (P2(hinge_x, wall_y + side * 100.0), P2(hinge_x, wall_y + side * 900.0))
        };
        for (press_space, hand, side) in [(false, -1.0, 1.0), (true, 1.0, 1.0), (false, 1.0, -1.0)] {
            h.hover_at(h.state().screen(point_at(side)));
            h.run();
            if press_space {
                key(&mut h, Key::Space);
            }
            let (hinge, tip) = leaf(hand, side);
            assert!(preview_leaf(&h, hinge, tip), "preview on wall {wall_y}: {hinge:?} to {tip:?}");
        }
        assert!(ids(&h, "door").is_empty());
        click(&mut h, point_at(-1.0));
        let door = ids(&h, "door")[0];
        let model = &h.state().session.model;
        let (hinge, tip) = leaf(1.0, -1.0);
        assert!(model.opening_marks(&model.elements[&door]).iter().any(|mark| matches!(mark,
            (bimcraft_core::family::Mark::Line(p), _) if p.len() == 2 && p[0].dist(hinge) < 1.0 && p[1].dist(tip) < 1.0)));
    }
}

#[test]
fn door_space_cycles_the_opens_choices_with_one_undo_each_and_reopens() {
    let mut h = app(&format!("{BOX}\ndoor {{\"at\":[2000,0]}}"));
    click(&mut h, P2(2000.0, 0.0));
    let door = ids(&h, "door")[0];
    assert_eq!(h.state().selection, [door]);
    let flips = |s: &Session| {
        let Element::Door(o) = &s.model.elements[&door] else { panic!("door") };
        (o.flip_hand, o.flip_facing)
    };
    let states = [(false, false), (true, false), (false, true), (true, true)];
    for &want in states.iter().cycle().skip(1).take(4) {
        key(&mut h, Key::Space);
        assert_eq!(flips(&h.state().session), want);
        assert_eq!(h.state().selection, [door]);
    }
    for &want in states.iter().rev() {
        h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
        h.run();
        assert_eq!(flips(&h.state().session), want);
    }
    h.key_press_modifiers(Modifiers::COMMAND, Key::Y);
    h.run();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("door.bimcraft");
    h.state_mut().session.execute("save", &json!({"path":path})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("open", &json!({"path":path})).unwrap();
    assert_eq!(flips(&reopened), (true, false));
}

#[test]
fn door_space_leaves_typing_other_operations_and_multiple_doors_alone() {
    let mut h = app(&format!("{BOX}\ndoor {{\"at\":[2000,0]}}\ndoor {{\"at\":[6000,0],\"family\":\"Sliding Door\"}}"));
    let [single, sliding] = ids(&h, "door")[..] else { panic!("two doors") };
    click(&mut h, P2(2000.0, 0.0));
    let before = h.state().session.model.clone();
    h.key_press_modifiers(Modifiers::SHIFT, Key::Space);
    h.run();
    h.get_by_role_and_label(Role::TextInput, "Mark").focus();
    h.run();
    h.event(Event::Text(" ".into()));
    key(&mut h, Key::Space);
    assert!(h.get_by_role_and_label(Role::TextInput, "Mark").value().unwrap().contains(' '));
    assert!(std::sync::Arc::ptr_eq(&before, &h.state().session.model));
    // Discard the uncommitted field edit and select the sliding door, then both doors.
    key(&mut h, Key::Escape);
    click(&mut h, P2(6000.0, 0.0));
    assert_eq!(h.state().selection, [sliding]);
    let before = h.state().session.model.clone();
    key(&mut h, Key::Space);
    click_with(&mut h, P2(2000.0, 0.0), Modifiers::COMMAND);
    assert_eq!(h.state().selection, [sliding, single]);
    key(&mut h, Key::Space);
    assert!(std::sync::Arc::ptr_eq(&before, &h.state().session.model));
    // A wall keeps its original Space shortcut.
    click(&mut h, P2(4000.0, 6000.0));
    let wall = h.state().selection[0];
    let end = field(&h, wall, "end");
    key(&mut h, Key::Space);
    assert_eq!(field(&h, wall, "start"), end);
}

#[test]
fn door_space_placement_uses_the_physical_wall_centre_and_escape_discards_the_pending_hand() {
    let mut h = app(&format!("{BOX}\nwall {{\"points\":[[1000,2500],[7000,2500]],\"location\":\"exterior-finish\"}}"));
    tool(&mut h, "Door");
    let before = h.state().session.model.clone();
    key(&mut h, Key::Space);
    key(&mut h, Key::Escape);
    assert!(std::sync::Arc::ptr_eq(&before, &h.state().session.model));
    tool(&mut h, "Door");
    // Both clicks round to the same Y for placement, but fall on opposite sides of the physical centre at 2400.
    click(&mut h, P2(2500.0, 2402.0));
    click(&mut h, P2(5500.0, 2398.0));
    let doors = ids(&h, "door");
    assert_eq!(doors.len(), 2);
    let states: Vec<_> = doors
        .iter()
        .map(|id| {
            let Element::Door(o) = &h.state().session.model.elements[id] else { panic!("door") };
            (o.flip_hand, o.flip_facing)
        })
        .collect();
    assert_eq!(states, [(false, false), (false, true)]);
}
