//! The owner's Quality of Life list, as a user meets it: what wasn't another test's already.

use super::*;
use egui::accesskit::Role;

/// Two rooms' worth of wall to measure across: `BOX`'s corners are at z 0 and 3000.
fn three_d(script: &str) -> Harness<'static, BimApp> {
    let mut h = app(script);
    press(&mut h, "Default 3D View");
    h
}

#[test]
fn the_finish_of_a_sketch_is_green_and_its_cancel_red_in_every_theme() {
    use crate::style::{AMOLED, DARK, DRACULA, LIGHT, MIDNIGHT};
    for p in [LIGHT, DARK, DRACULA, MIDNIGHT, AMOLED] {
        let (green, red) = (p.context, p.error);
        assert!(green.g() > green.r() && green.g() > green.b(), "{green:?}");
        assert!(red.r() > red.g() && red.r() > red.b(), "{red:?}");
    }
    let mut h = app(BOX);
    h.state_mut().set_tool(Tool::Floor);
    h.run();
    assert!(h.query_by_label("Finish Edit Mode").is_some() && h.query_by_label("Cancel Edit Mode").is_some());
    press(&mut h, "Cancel Edit Mode");
    assert!(h.state().sketch.is_none(), "they still do what they did");
}

#[test]
fn the_ai_button_beside_settings_lets_agents_in_and_shuts_them_out() {
    let mut h = app(BOX);
    assert!(h.state().control.is_none());
    h.get_by_role_and_label(Role::CheckBox, "AI agents").click();
    h.run();
    assert!(h.state().control.is_some() && h.state().agents.listen, "the port is open, and kept");
    h.get_by_role_and_label(Role::CheckBox, "AI agents").click();
    h.run();
    assert!(h.state().control.is_none() && !h.state().agents.listen, "and closed");
}

#[test]
fn the_type_selector_s_list_is_no_wider_than_the_selector() {
    let mut h = app(BOX);
    let wall = find(&h, |e| matches!(e, Element::Wall(_)));
    h.state_mut().selection = vec![wall];
    h.run();
    let selector = h.get_by_label("Type Selector").rect();
    press(&mut h, "Type Selector");
    let names: Vec<String> = h.state().session.model.types.wall.keys().cloned().collect();
    assert!(!names.is_empty());
    for n in &names {
        let row =
            h.query_all_by_label(n).map(|r| r.rect()).find(|r| r.top() > selector.bottom() - 1.0 || r.bottom() < selector.top() + 1.0);
        let row = row.unwrap_or_else(|| panic!("`{n}` isn't listed"));
        assert!(row.right() <= selector.right() + 0.5, "{n}: {row:?} past the selector's {selector:?}");
    }
}

#[test]
fn a_wall_type_s_sample_shows_each_of_its_layers_by_its_share() {
    let mut h = app(BOX);
    h.state_mut().run(
        "type",
        json!({"category": "wall", "name": "Layered", "thickness": 200.0, "layers": [
            {"function": "finish", "material": "", "thickness": 50.0},
            {"function": "structure", "material": "", "thickness": 150.0}
        ]}),
    );
    h.run();
    let bands = crate::properties::layer_bands(&h.state().session.model, "Layered");
    assert_eq!(bands.iter().map(|b| b.0).collect::<Vec<_>>(), [0.25, 0.75], "{}", h.state().status.text);
}

#[test]
fn a_line_style_s_list_shows_samples_and_a_row_picks_its_style() {
    let mut h = app(BOX);
    h.state_mut().set_tool(Tool::DetailLine(Vec::new()));
    h.run();
    press(&mut h, "Line Style");
    let other = h.state().session.model.line_styles.keys().find(|n| **n != h.state().line_style).cloned().unwrap();
    press(&mut h, &other);
    assert_eq!(h.state().line_style, other);
}

#[test]
fn the_measure_tool_in_3d_reads_between_two_corners_at_different_heights_whatever_the_work_plane() {
    let mut h = three_d(BOX);
    h.state_mut().set_tool(Tool::Measure(None));
    h.run();
    // From the south wall's outer corner at the ground to the top of the same wall's far end: not on any one level.
    let (a, b): ([f64; 3], [f64; 3]) = ([-100.0, -100.0, 0.0], [8100.0, -100.0, 3000.0]);
    let at = h.state().screen3(a);
    click_at(&mut h, at);
    let to = h.state().screen3(b);
    click_at(&mut h, to);
    let chain = h.state().orbit.chain.clone();
    assert_eq!(chain.len(), 2, "{chain:?}");
    let long = bimcraft_core::dims3d::reading(chain[0], chain[1]).0;
    assert!((long - 8200.0f64.hypot(3000.0)).abs() < 1.0, "{long} along {chain:?}");
    // Esc takes the chain away, then the tool; the camera stayed.
    key(&mut h, Key::Escape);
    assert!(h.state().orbit.chain.is_empty() && matches!(h.state().tool, Tool::Measure(_)));
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Modify);
}

#[test]
fn a_measured_chain_round_a_wall_s_face_closes_on_its_first_point_and_reads_its_area() {
    let mut h = three_d(BOX);
    h.state_mut().set_tool(Tool::Measure(None));
    h.run();
    // The south wall's outer face, corner to corner, and back to the first: in one plane.
    let face = [[-100.0, -100.0, 0.0], [8100.0, -100.0, 0.0], [8100.0, -100.0, 3000.0], [-100.0, -100.0, 3000.0], [-100.0, -100.0, 0.0]];
    for q in face {
        let at = h.state().screen3(q);
        click_at(&mut h, at);
    }
    assert_eq!(h.state().orbit.chain_done, Some(true), "closed: {:?}", h.state().orbit.chain);
    let (total, area) = bimcraft_core::dims3d::chain_reading(&h.state().orbit.chain, true);
    assert!((total - 2.0 * (8200.0 + 3000.0)).abs() < 2.0, "{total}");
    assert!((area.unwrap_or_default() - 8200.0 * 3000.0).abs() < 8200.0 * 3000.0 * 1e-3, "{area:?}");
    // The next click begins another chain.
    let at = h.state().screen3(face[1]);
    click_at(&mut h, at);
    assert_eq!((h.state().orbit.chain.len(), h.state().orbit.chain_done), (1, None));
}

#[test]
fn over_nothing_the_measure_tool_says_what_it_needs() {
    let mut h = three_d(BOX);
    h.state_mut().set_tool(Tool::Measure(None));
    h.run();
    let corner = h.state().orbit.rect.min + vec2(4.0, 4.0);
    click_at(&mut h, corner);
    assert!(h.state().orbit.chain.is_empty());
    assert!(h.state().status.error, "{}", h.state().status.text);
}

const TWO_WALLS: &str = r#"wall {"points": [[0, 0], [0, 4000]]}
wall {"points": [[2000, 0], [2000, 4000]]}
dimension {"refs": [[0, 2000], [2000, 2000]], "at": [1000, -1000]}"#;

fn type_in(h: &mut Harness<BimApp>, label: &str, value: &str) {
    h.get_by_label(label).click();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text(value.into()));
    h.run();
    key(h, Key::Enter);
}

fn all(h: &Harness<BimApp>, category: &str) -> Vec<Id> {
    let m = &h.state().session.model;
    m.elements.iter().filter(|(_, e)| e.category() == category).map(|(id, _)| *id).collect()
}

#[test]
fn with_the_near_wall_selected_its_dimension_s_number_moves_that_wall() {
    let mut h = app(TWO_WALLS);
    let walls = all(&h, "wall");
    h.state_mut().selection = vec![walls[0]];
    h.run();
    type_in(&mut h, "Dimension 2000", "1500");
    let m = &h.state().session.model;
    assert_eq!(m.wall(walls[0]).unwrap().start, P2(500.0, 0.0), "{}", h.state().status.text);
    assert_eq!(m.wall(walls[1]).unwrap().start, P2(2000.0, 0.0), "the other stays");
}

#[test]
fn with_the_dimension_and_the_far_wall_selected_its_number_moves_that_wall() {
    let mut h = app(TWO_WALLS);
    let (walls, dim) = (all(&h, "wall"), all(&h, "dimension")[0]);
    h.state_mut().selection = vec![dim, walls[1]];
    h.run();
    type_in(&mut h, "Dimension 2000", "3000");
    assert_eq!(h.state().session.model.wall(walls[1]).unwrap().start, P2(3000.0, 0.0), "{}", h.state().status.text);
}

#[test]
fn a_selected_angular_dimension_s_number_turns_its_second_line() {
    let mut h = app(r#"wall {"points": [[0, 0], [4000, 0]]}
wall {"points": [[0, 0], [0, 4000]]}"#);
    let walls = all(&h, "wall");
    let made = h.state_mut().run("dimension", json!({"kind": "angular", "refs": [{"id": walls[0]}, {"id": walls[1]}], "at": [1000, 1000]}));
    let dim = made.unwrap()["id"].as_u64().unwrap();
    h.state_mut().selection = vec![dim];
    h.run();
    type_in(&mut h, "Dimension 90°", "60");
    let Some(Element::Dimension(d)) = h.state().session.model.elements.get(dim) else { panic!("the dimension") };
    let now = h.state().session.model.dimension_values(d)[0];
    assert!((now - 60.0).abs() < 1e-3, "{now}: {}", h.state().status.text);
}

#[test]
fn a_selected_wall_s_temporary_dimension_across_it_moves_it_from_its_neighbour() {
    let mut h = app(r#"wall {"points": [[0, 0], [0, 4000]]}
wall {"points": [[2000, 0], [2000, 4000]]}"#);
    let walls = all(&h, "wall");
    h.state_mut().selection = vec![walls[1]];
    h.run();
    type_in(&mut h, "Temporary dimension 2000", "3000");
    assert_eq!(h.state().session.model.wall(walls[1]).unwrap().start, P2(3000.0, 0.0), "{}", h.state().status.text);
}

/// A level above, a wall to it, a column to it, and a roof over them: to drag and attach in the 3D view.
const STOREY: &str = r#"level {"name": "Upper", "elevation": 3000}
wall {"points": [[0, 0], [8000, 0]], "level": "Level 1", "top_level": "Upper"}
column {"at": [4000, 3000], "level": "Level 1", "top_level": "Upper"}
roof {"boundary": [[-500, -500], [8500, -500], [8500, 6500], [-500, 6500]], "level": "Upper", "slope": 20}"#;

fn handle_of(h: &Harness<BimApp>, id: Id, which: bimcraft_core::handles::Handle) -> Pos2 {
    let (_, q) = bimcraft_core::handles::of(&h.state().session.model, id).into_iter().find(|(k, _)| *k == which).expect("that handle");
    h.state().screen3(q)
}

#[test]
fn in_3d_a_wall_s_top_arrow_and_end_grip_drag_without_turning_the_view_or_losing_the_selection() {
    use bimcraft_core::handles::Handle;
    let mut h = three_d(STOREY);
    let wall = all(&h, "wall")[0];
    h.state_mut().selection = vec![wall];
    h.run();
    let before = h.state().orbit.camera;
    let was = h.state().session.model.wall(wall).cloned().unwrap();
    let top = handle_of(&h, wall, Handle::Top);
    drag_screen(&mut h, top, top + vec2(0.0, -40.0), PointerButton::Primary);
    let w = h.state().session.model.wall(wall).cloned().unwrap();
    assert!(w.top_offset > 0.0, "raised: {} ({})", w.top_offset, h.state().status.text);
    assert_eq!(h.state().orbit.camera, before, "the handle took the drag");
    assert_eq!(h.state().selection, [wall]);
    let end = handle_of(&h, wall, Handle::End);
    drag_screen(&mut h, end, end + vec2(60.0, 0.0), PointerButton::Primary);
    let w = h.state().session.model.wall(wall).cloned().unwrap();
    assert_ne!(w.end, was.end, "its end went with the hand");
    assert_eq!(w.start, was.start, "its start stayed");
    // A click on a handle picks nothing behind it.
    let top = handle_of(&h, wall, Handle::Top);
    click_at(&mut h, top);
    assert_eq!(h.state().selection, [wall]);
}

#[test]
fn in_3d_attach_top_takes_the_roof_clicked_for_a_wall_and_for_a_column_which_loses_its_arrow() {
    use bimcraft_core::handles::Handle;
    for category in ["wall", "column"] {
        let mut h = three_d(STOREY);
        let id = all(&h, category)[0];
        h.state_mut().selection = vec![id];
        h.run();
        press(&mut h, if category == "wall" { "Modify | Walls" } else { "Modify | Columns" });
        press(&mut h, "Attach Top/Base");
        assert_eq!(h.state().tool, Tool::Attach, "{}", h.state().status.text);
        // Up through the roof's middle, as a hand looks for it: a click on what isn't a roof or a floor takes nothing.
        for k in 0..60 {
            let at = h.state().screen3([4000.0, 3000.0, 3000.0 + f64::from(k) * 100.0]);
            click_at(&mut h, at);
            if h.state().tool == Tool::Modify {
                break;
            }
        }
        let v = element_json(&h.state().session.model, id);
        assert!(!v["attach_top"].is_null(), "{category}: {v}");
        assert_eq!(h.state().tool, Tool::Modify);
        let kinds: Vec<Handle> = bimcraft_core::handles::of(&h.state().session.model, id).into_iter().map(|x| x.0).collect();
        assert!(!kinds.contains(&Handle::Top), "{category}: {kinds:?}");
        press(&mut h, "Detach Top/Base");
        let v = element_json(&h.state().session.model, id);
        assert!(v["attach_top"].is_null(), "{category}: {v}");
    }
}

#[test]
fn in_3d_a_column_s_top_arrow_and_place_drag_as_a_wall_s_do() {
    use bimcraft_core::handles::Handle;
    let mut h = three_d(STOREY);
    let column = all(&h, "column")[0];
    h.state_mut().selection = vec![column];
    h.run();
    let span = |h: &Harness<BimApp>| h.state().session.model.elements.get(column).map(|e| h.state().session.model.column_span(e)).unwrap();
    let top = handle_of(&h, column, Handle::Top);
    drag_screen(&mut h, top, top + vec2(0.0, -40.0), PointerButton::Primary);
    assert!(span(&h).1 > 3000.0, "raised: {:?} ({})", span(&h), h.state().status.text);
    let place = handle_of(&h, column, Handle::Place);
    drag_screen(&mut h, place, place + vec2(50.0, 0.0), PointerButton::Primary);
    let at = h.state().session.model.elements.get(column).and_then(Element::column).map(|c| c.at).unwrap();
    assert_ne!(at, P2(4000.0, 3000.0), "it went with the hand");
}

/// As in the owner's picture: a wall along the top, one down from its east end, a door on that one, and a dimension from the top
/// wall's face down to the door.
fn l_door(up: bool, door_first: bool) -> String {
    let walls =
        if up { r#"wall {"points": [[0, -9000], [0, 0], [-6000, 0]]}"# } else { r#"wall {"points": [[-6000, 0], [0, 0], [0, -9000]]}"# };
    // Door first, picked a little off its wall's centre line, which runs through the door's own centre too.
    let refs = if door_first { "[[30, -7000], [-3000, 0]]" } else { "[[-3000, 0], [0, -7000]]" };
    format!("{walls}\ndoor {{\"at\": [0, -7000]}}\ndimension {{\"refs\": {refs}, \"at\": [4000, -3000]}}")
}

/// The distance the door stands from the top wall's inner face, and from the bottom end of its wall.
fn door_at(h: &Harness<BimApp>) -> f64 {
    let door = all(h, "door")[0];
    let Some(Element::Door(o)) = h.state().session.model.elements.get(door) else { panic!("the door") };
    let w = h.state().session.model.wall(o.host).unwrap();
    let axis = bimcraft_core::walls::Axis::of(w);
    axis.at(o.offset, 0.0).1
}

#[test]
fn every_number_beside_a_selected_door_moves_it_so_that_number_reads_what_was_typed() {
    for (up, door_first) in [(false, false), (true, false), (false, true), (true, true)] {
        number_beside_a_door(&l_door(up, door_first), &format!("drawn up {up}, door first {door_first}"));
    }
}

fn number_beside_a_door(script: &str, case: &str) {
    eprintln!("CASE {case}\n{script}");
    let h = app(script);
    let door = all(&h, "door")[0];
    let dim = all(&h, "dimension")[0];
    let Some(Element::Dimension(d)) = h.state().session.model.elements.get(dim) else { panic!("the dimension") };
    let was = h.state().session.model.dimension_values(d)[0];
    let y0 = door_at(&h);
    // The permanent dimension, the door selected: 500 longer, so the door goes 500 further down.
    let mut h = app(script);
    h.state_mut().selection = vec![door];
    h.run();
    let label = format!("Dimension {}", h.state().session.model.units.length(was));
    type_in(&mut h, &label, &format!("{}", was + 500.0));
    let Some(Element::Dimension(d)) = h.state().session.model.elements.get(dim) else { panic!("the dimension") };
    assert!(
        (h.state().session.model.dimension_values(d)[0] - (was + 500.0)).abs() < 0.5,
        "{case}: {} now {:?}, door from {y0} to {}: {} {:?}",
        was,
        h.state().session.model.dimension_values(d),
        door_at(&h),
        h.state().status.text,
        h.state().session.steps(false).iter().map(|st| st.label).collect::<Vec<_>>()
    );
    assert!((door_at(&h) - (y0 - 500.0)).abs() < 0.5, "{case}: down, not up: {} from {y0}", door_at(&h));
    // Its temporary dimensions, from the face of the top wall and to its wall's end: whichever is typed reads what was typed.
    let spans = |h: &Harness<BimApp>| {
        let m = &h.state().session.model;
        let Some(Element::Door(o)) = m.elements.get(door) else { panic!("the door") };
        let (start, end) = m.opening_span(o).unwrap();
        [o.offset - start, end - o.offset].map(|x| (x * 100.0).round() / 100.0)
    };
    for n in 0..2 {
        let mut h = app(script);
        h.state_mut().selection = vec![door];
        h.run();
        let now = spans(&h)[n];
        let label = format!("Temporary dimension {}", h.state().session.model.units.length(now));
        type_in(&mut h, &label, &format!("{}", now - 300.0));
        assert!((spans(&h)[n] - (now - 300.0)).abs() < 0.5, "{case}: typed {} into {now}: now {:?}", now - 300.0, spans(&h));
    }
}

#[test]
fn every_segment_of_a_string_along_the_top_reads_what_is_typed_into_it() {
    for west in [false, true] {
        for select_door in [false, true] {
            let walls = if west {
                r#"wall {"points": [[0, -9000], [0, 0], [-6000, 0]]}"#
            } else {
                r#"wall {"points": [[-6000, 0], [0, 0], [0, -9000]]}"#
            };
            // The two doors along the top wall, then the side wall's centre line.
            let side = if west { 100002 } else { 100003 };
            let script = format!(
                "{walls}\ndoor {{\"at\": [-4000, 0]}}\ndoor {{\"at\": [-1500, 0]}}\ndimension {{\"refs\": [{{\"id\": 100004}}, {{\"id\": 100005}}, {{\"id\": {side}}}], \"at\": [-3000, 1500]}}"
            );
            for segment in 0..2 {
                let case = format!("drawn west {west}, door selected {select_door}, segment {segment}");
                let mut h = app(&script);
                let (dim, door) = (all(&h, "dimension")[0], all(&h, "door")[0]);
                let values = |h: &Harness<BimApp>| match h.state().session.model.elements.get(dim) {
                    Some(Element::Dimension(d)) => h.state().session.model.dimension_values(d),
                    _ => Vec::new(),
                };
                let was = values(&h);
                // The first door is at one end of the first segment only.
                if select_door && segment == 1 {
                    continue;
                }
                h.state_mut().selection = vec![if select_door { door } else { dim }];
                h.run();
                let label = format!("Dimension {}", h.state().session.model.units.length(was[segment]));
                type_in(&mut h, &label, &format!("{}", was[segment] + 300.0));
                let now = values(&h);
                assert!((now[segment] - (was[segment] + 300.0)).abs() < 0.5, "{case}: {was:?} to {now:?}: {}", h.state().status.text);
            }
        }
    }
}

#[test]
fn a_box_in_3d_takes_the_levels_it_takes_in_as_it_does_walls() {
    let mut h = three_d(BOX);
    let levels = all(&h, "level");
    let rect = h.state().orbit.rect;
    // Round the whole view, from the left: every level whole inside, the walls too.
    drag_screen(&mut h, rect.min + vec2(2.0, 2.0), rect.max - vec2(2.0, 2.0), PointerButton::Primary);
    let picked = h.state().selection.clone();
    assert!(levels.iter().all(|l| picked.contains(l)), "{levels:?} in {picked:?}");
    assert!(all(&h, "wall").iter().all(|w| picked.contains(w)));
}

#[test]
fn trim_to_a_corner_works_in_the_3d_view_as_in_a_plan() {
    // Two walls that cross, one each way: trimmed to a corner, the parts clicked are kept.
    let mut h = three_d(
        r#"wall {"points": [[0, 0], [8000, 0]]}
wall {"points": [[6000, -2000], [6000, 4000]]}"#,
    );
    let walls = all(&h, "wall");
    h.state_mut().set_tool(Tool::Trim(None));
    h.run();
    let a = h.state().screen3([2000.0, -100.0, 1500.0]);
    click_at(&mut h, a);
    assert!(matches!(h.state().tool, Tool::Trim(Some(_))), "{}", h.state().status.text);
    let b = h.state().screen3([6000.0, 2000.0, 1500.0]);
    click_at(&mut h, b);
    let m = &h.state().session.model;
    let (w0, w1) = (m.wall(walls[0]).unwrap(), m.wall(walls[1]).unwrap());
    assert_eq!(w0.end, P2(6000.0, 0.0), "{}: {w0:?}", h.state().status.text);
    assert_eq!(w1.start, P2(6000.0, 0.0), "{w1:?}");
}

#[test]
fn in_3d_the_measure_tool_says_how_its_chain_works() {
    let mut h = three_d(BOX);
    h.state_mut().set_tool(Tool::Measure(None));
    h.run();
    assert!(h.state().prompt().contains("Click the first point again to close it"), "{}", h.state().prompt());
}
