//! The 3D view as a working view: a drag orbits round the point under the cursor and the wheel zooms toward it, and the drawing
//! tools (walls, beams and braces, pipes, conduits and trays, columns, components) take their points on the face under the cursor,
//! snapped as a plan snaps; over empty space each says what it needs. In both profiles, on the processor's picture and, where the
//! machine has a graphics adapter, on the card's.

use super::*;
use crate::gpu::{Card, headless};
use bimcraft_core::view3d::P3;

/// The 3D view of `script` in each window it is driven in: BIMCraft's and Rev's, drawn on the processor and on the card (where
/// there is one: a runner without a graphics adapter says so and drives the processor's alone), each named for its failures.
fn views(script: &str) -> Vec<(String, Harness<'static, BimApp>)> {
    let mut out = Vec::new();
    for rev in [false, true] {
        for card in [false, true] {
            let mut h = if rev { app(script) } else { bimcraft(script) };
            if card {
                let Some(gpu) = headless() else {
                    eprintln!("no graphics adapter here: the card's 3D view is not driven");
                    continue;
                };
                h.state_mut().card = Card::with(gpu);
            }
            shortcut(&mut h, "3D");
            assert_eq!(h.state().sheet, Sheet::View3D);
            assert_eq!(h.state().orbit.on_card, card, "{:?}", h.state().card.note);
            let name = format!("{} on the {}", if rev { "Rev" } else { "BIMCraft" }, if card { "card" } else { "processor" });
            out.push((name, h));
        }
    }
    out
}

/// A point on the south wall's outer face: `BOX`'s walls are 200 mm thick about their lines.
const FACE: P3 = [5000.0, -100.0, 1500.0];

#[test]
fn a_drag_orbits_round_the_point_under_the_cursor_which_stays_where_it_is_on_screen() {
    for (name, mut h) in views(BOX) {
        let before = h.state().orbit.camera.unwrap();
        let at = h.state().screen3(FACE);
        drag_screen(&mut h, at, at + vec2(90.0, 40.0), PointerButton::Primary);
        let cam = h.state().orbit.camera.unwrap();
        assert!((cam.azimuth - before.azimuth).abs() > 10.0 && (cam.elevation - before.elevation).abs() > 5.0, "{name}: it turned");
        let now = h.state().screen3(FACE);
        assert!(now.distance(at) < 0.5, "{name}: the face under the cursor went from {at:?} to {now:?}");
        // From the background, round the target as ever.
        let corner = h.state().orbit.rect.min + vec2(6.0, 6.0);
        drag_screen(&mut h, corner, corner + vec2(60.0, 0.0), PointerButton::Primary);
        let turned = h.state().orbit.camera.unwrap();
        assert!(turned.azimuth != cam.azimuth && turned.target == cam.target, "{name}");
    }
}

#[test]
fn in_perspective_the_wheel_zooms_toward_the_point_under_the_cursor() {
    for (name, mut h) in views(BOX) {
        h.state_mut().run("perspective", json!({}));
        h.run();
        let before = h.state().orbit.camera.unwrap();
        assert!(before.lens.is_some(), "{name}: in perspective");
        let at = h.state().screen3(FACE);
        h.hover_at(at);
        h.event(Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, 120.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        h.run();
        let cam = h.state().orbit.camera.unwrap();
        let k = cam.scale / before.scale;
        assert!(k > 1.2, "{name}: zoomed in");
        let now = h.state().screen3(FACE);
        assert!(now.distance(at) < 0.5, "{name}: the face under the cursor went from {at:?} to {now:?}");
        // The eye closed in on the face by as much as it zoomed: through it, it would never go.
        let far = |c: bimcraft_core::view3d::Camera| c.eye().map(|e| (0..3).map(|i| (e[i] - FACE[i]).powi(2)).sum::<f64>().sqrt());
        let (d0, d1) = (far(before).unwrap(), far(cam).unwrap());
        assert!((d0 / d1 - k).abs() < 1e-6 * k, "{name}: {d0} mm from the face, then {d1}, zoomed by {k}");
    }
}

#[test]
fn the_wall_tool_and_a_component_draw_on_a_floors_top_in_the_3d_view_and_stand_on_it() {
    let script = format!("{BOX}\nfloor {{\"offset\": 500}}");
    for (name, mut h) in views(&script) {
        let level = h.state().session.model.levels()[0].0;
        shortcut(&mut h, "WA");
        // A wall's face stands up: nothing to draw on, and the status line says so.
        let at = h.state().screen3(FACE);
        click_at(&mut h, at);
        assert!(h.state().status.error && matches!(&h.state().tool, Tool::Wall(c) if c.is_empty()), "{name}: {:?}", h.state().tool);
        for p in [[1500.0, 5000.0, 500.0], [4500.0, 5000.0, 500.0]] {
            let at = h.state().screen3(p);
            click_at(&mut h, at);
        }
        key(&mut h, Key::Escape);
        let m = &h.state().session.model;
        let walls = m.elements.values().filter_map(|e| if let Element::Wall(w) = e { Some(w) } else { None });
        // Not one of the box's, which all start on its corners.
        let made: Vec<_> = walls.filter(|w| w.start.0 > 0.0 && w.start.0 < 8000.0).collect();
        let w = made.first().unwrap_or_else(|| panic!("{name}: no wall: {}", h.state().status.text));
        assert_eq!((w.start, w.end, made.len()), (P2(1500.0, 5000.0), P2(4500.0, 5000.0), 1), "{name}");
        assert_eq!((m.elements.id(w.level), w.base_offset, m.own_base(w)), (Some(level), 500.0, 500.0), "{name}: on the floor's top");
        // A component on it too: on the floor's level, 500 up.
        key(&mut h, Key::Escape);
        shortcut(&mut h, "CM");
        let at = h.state().screen3([3000.0, 4000.0, 500.0]);
        click_at(&mut h, at);
        let m = &h.state().session.model;
        let placed: Vec<_> = m.elements.values().filter_map(|e| if let Element::Component(c) = e { Some(c) } else { None }).collect();
        let c = placed.first().unwrap_or_else(|| panic!("{name}: no component: {}", h.state().status.text));
        assert_eq!((c.at, m.elements.id(c.level), c.offset), (P2(3000.0, 4000.0), Some(level), 500.0), "{name}");
    }
}

/// The runs of `category` in the model, as they are.
fn runs(h: &Harness<BimApp>, category: &str) -> Vec<bimcraft_core::model::Run> {
    let m = &h.state().session.model;
    m.elements.values().filter(|e| e.category() == category).filter_map(|e| e.run().cloned()).collect()
}

#[test]
fn a_beam_runs_between_two_points_on_a_floors_top_in_the_3d_view_and_the_next_snaps_to_its_end() {
    let script = format!("{BOX}\nfloor {{\"offset\": 500}}");
    for (name, mut h) in views(&script) {
        let level = h.state().session.model.levels()[0].0;
        shortcut(&mut h, "BM");
        assert!(matches!(h.state().tool, Tool::Run("beam", None)), "{name}: {:?}", h.state().tool);
        // Over the south wall, where the view sees the floor.
        for p in [[1500.0, 5000.0, 500.0], [4500.0, 5000.0, 500.0]] {
            let at = h.state().screen3(p);
            click_at(&mut h, at);
        }
        key(&mut h, Key::Escape);
        let made = runs(&h, "beam");
        let b = made.first().unwrap_or_else(|| panic!("{name}: no beam: {}", h.state().status.text));
        assert_eq!((b.start, b.end, made.len()), (P2(1500.0, 5000.0), P2(4500.0, 5000.0), 1), "{name}");
        let m = &h.state().session.model;
        assert_eq!((m.elements.id(b.level), b.offset, b.end_offset), (Some(level), 500.0, None), "{name}: its top on the floor's");
        // The next one's start, a few points off the first one's end, snaps to it as a plan's does.
        let at = h.state().screen3([4500.0, 5000.0, 500.0]) + vec2(4.0, -3.0);
        click_at(&mut h, at);
        let at = h.state().screen3([2500.0, 4000.0, 500.0]);
        click_at(&mut h, at);
        let made = runs(&h, "beam");
        assert_eq!(made.len(), 2, "{name}: {}", h.state().status.text);
        assert_eq!((made[1].start, made[1].end), (P2(4500.0, 5000.0), P2(2500.0, 4000.0)), "{name}");
    }
}

#[test]
fn a_brace_from_a_floors_top_to_a_walls_top_climbs_from_one_face_to_the_other() {
    let script = format!("{BOX}\nfloor {{\"offset\": 500}}");
    for (name, mut h) in views(&script) {
        let m = &h.state().session.model;
        let (level, top) = (m.levels()[0].0, m.top_z(m.wall(ids(&h, "wall")[0]).unwrap()));
        shortcut(&mut h, "BM");
        // From the floor's top to the north wall's, its end on the wall's centre line.
        for p in [[3000.0, 4500.0, 500.0], [3000.0, 6000.0, top]] {
            let at = h.state().screen3(p);
            click_at(&mut h, at);
        }
        let made = runs(&h, "beam");
        let b = made.first().unwrap_or_else(|| panic!("{name}: no brace: {}", h.state().status.text));
        assert_eq!((b.start, b.end), (P2(3000.0, 4500.0), P2(3000.0, 6000.0)), "{name}");
        let m = &h.state().session.model;
        assert_eq!(
            (m.elements.id(b.level), b.offset, b.end_offset),
            (Some(level), 500.0, Some(top)),
            "{name}: from 500 up to the wall's top"
        );
    }
}

#[test]
fn pipes_conduits_and_trays_run_on_a_floors_top_in_the_3d_view() {
    let script = format!("{BOX}\nfloor {{\"offset\": 500}}");
    for (name, mut h) in views(&script) {
        for (k, category) in ["pipe", "conduit", "cabletray"].into_iter().enumerate() {
            h.state_mut().set_tool(Tool::Run(category, None));
            let y = 4000.0 + 500.0 * k as f64;
            for p in [[1000.0, y, 500.0], [3500.0, y, 500.0]] {
                let at = h.state().screen3(p);
                click_at(&mut h, at);
            }
            key(&mut h, Key::Escape);
            let made = runs(&h, category);
            assert_eq!(made.len(), 1, "{name}: {category}: {}", h.state().status.text);
            assert_eq!((made[0].start, made[0].end, made[0].offset), (P2(1000.0, y), P2(3500.0, y), 500.0), "{name}: {category}");
        }
    }
}

#[test]
fn a_column_and_a_structural_column_stand_on_the_face_they_are_placed_on() {
    let script = format!("{BOX}\nfloor {{\"offset\": 500}}");
    for (name, mut h) in views(&script) {
        let level = h.state().session.model.levels()[0].0;
        for (tool, category, at) in [
            (Tool::Column, "column", [3000.0, 4000.0, 500.0]),
            (Tool::Stand("structuralcolumn"), "structuralcolumn", [2000.0, 4500.0, 500.0]),
        ] {
            h.state_mut().set_tool(tool);
            let on = h.state().screen3(at);
            click_at(&mut h, on);
            let m = &h.state().session.model;
            let placed: Vec<_> = m
                .elements
                .values()
                .filter(|e| e.category() == category)
                .filter_map(|e| match e {
                    Element::Column(c) | Element::StructuralColumn(c) => Some(c),
                    _ => None,
                })
                .collect();
            let c = placed.first().unwrap_or_else(|| panic!("{name}: no {category}: {}", h.state().status.text));
            assert_eq!((c.at, m.elements.id(c.level), c.base_offset), (P2(at[0], at[1]), Some(level), 500.0), "{name}: {category}");
        }
    }
}

#[test]
fn a_floor_is_sketched_in_the_3d_view_on_a_level_face_picked_for_the_work_plane() {
    let script = format!("{BOX}\nfloor {{\"offset\": 500}}");
    let mut h = app(&script);
    press(&mut h, "Default 3D View");
    press(&mut h, "Work Plane");
    pick(&mut h, Role::RadioButton, "Pick a plane");
    press(&mut h, "OK");
    // The floor's top, a level face, where the view sees it over the south wall: the click sets it.
    let at = h.state().screen3([3000.0, 4500.0, 500.0]);
    click_at(&mut h, at);
    assert!(h.state().work_plane.is_none(), "set by the click: {}", h.state().status.text);
    tool(&mut h, "Floor");
    assert!(matches!(h.state().sketch.as_ref().map(|s| s.of), Some(crate::sketch::Of::Floor(_))), "{}", h.state().status.text);
    press(&mut h, "Rectangle");
    for p in [[2000.0, 2000.0, 500.0], [4000.0, 3000.0, 500.0]] {
        let at = h.state().screen3(p);
        click_at(&mut h, at);
    }
    press(&mut h, "Finish Edit Mode");
    assert!(h.state().sketch.is_none(), "{}", h.state().status.text);
    let floors = ids(&h, "floor");
    let new = *floors.last().unwrap();
    assert_eq!((floors.len(), field(&h, new, "offset"), field(&h, new, "area_m2")), (2, json!(500.0), json!(2.0)), "on the floor's top");
}

#[test]
fn over_empty_space_each_tool_says_what_it_needs_in_the_3d_view() {
    let mut h = app(BOX);
    shortcut(&mut h, "3D");
    // Looking level at the model: the ground is edge-on, and nothing is at the view's top corner.
    let cam = h.state().orbit.camera.unwrap();
    h.state_mut().orbit.camera = Some(bimcraft_core::view3d::Camera { elevation: 0.0, ..cam });
    h.run();
    // Under the view's tabs and the Options Bar, which float over its top left corner.
    let empty = h.state().orbit.rect.min + vec2(30.0, 110.0);
    let walls = ids(&h, "wall").len();
    let before = h.state().session.model.elements.len();
    for (tool, says) in [
        (Tool::Wall(Vec::new()), "Place Wall"),
        (Tool::Run("beam", None), "Place Beam"),
        (Tool::Run("pipe", None), "Place Pipe"),
        (Tool::Run("conduit", None), "Place Conduit"),
        (Tool::Run("cabletray", None), "Place Cable Tray"),
        (Tool::Column, "Place Column"),
        (Tool::Stand("structuralcolumn"), "Place Structural Column"),
        (Tool::Stand("footing"), "Place Isolated Footing"),
        (Tool::Component, "Place Component"),
    ] {
        h.state_mut().set_tool(tool.clone());
        h.state_mut().status = crate::Status::default();
        click_at(&mut h, empty);
        let s = &h.state().status;
        assert!(s.error && s.text.contains(says) && s.text.contains("level face"), "{tool:?}: {}", s.text);
    }
    // Floors and ceilings sketch on the work plane, which a level view sees edge-on.
    for name in ["Floor", "Ceiling"] {
        tool(&mut h, name);
        click_at(&mut h, empty);
        let s = &h.state().status;
        assert!(s.error && s.text.contains("edge-on"), "{name}: {}", s.text);
        press(&mut h, "Cancel Edit Mode");
    }
    assert_eq!((ids(&h, "wall").len(), h.state().session.model.elements.len()), (walls, before), "nothing made");
}
