//! The navigation triad in the 3D view's corner: the x, y and z axes turning with the camera, whose ends turn it to look from that
//! side (top, bottom, front, back, left, right), a second click on an end to the opposite side, and with Alt held set the work plane
//! square to the axis (or, on empty space, back to the level). Clicks on it never reach the model behind it. Every test runs in both
//! profiles, Rev's (`app`) and BIMCraft's (`bimcraft`).

use super::*;
use crate::gpu::{Card, headless};
use bimcraft_core::triad::{self, ENDS, End, Side};
use bimcraft_core::view3d::Camera;

/// The 3D view open, in the profile (Rev's if `rev`).
fn three_d(rev: bool) -> Harness<'static, BimApp> {
    let mut h = if rev { app(BOX) } else { bimcraft(BOX) };
    h.state_mut().open_sheet(Sheet::View3D);
    h.run();
    h
}

fn camera(h: &Harness<BimApp>) -> Camera {
    h.state().orbit.camera.expect("the 3D view has shown")
}

/// Where the camera looks from: its azimuth and elevation.
fn looking(h: &Harness<BimApp>) -> (f64, f64) {
    let c = camera(h);
    (c.azimuth, c.elevation)
}

/// The camera as the 3D view first has it, for the next click to start from.
fn home(h: &mut Harness<BimApp>) {
    h.state_mut().orbit.camera = None;
    h.run();
}

/// A primary click at `pos` with `modifiers` held.
fn click_holding(h: &mut Harness<BimApp>, pos: Pos2, modifiers: Modifiers) {
    h.hover_at(pos);
    h.run();
    h.event(Event::ModifiersChanged(modifiers));
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers });
    }
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run();
}

fn end_pos(h: &Harness<BimApp>, end: End) -> Pos2 {
    h.state().triad_at(end).expect("the triad shows in the 3D view")
}

fn click_end(h: &mut Harness<BimApp>, end: End) {
    let at = end_pos(h, end);
    click_at(h, at);
}

/// What the picture of the model shows at a point of the screen, in the 3D view as it is now.
fn model_at(h: &Harness<BimApp>, pos: Pos2) -> Option<Id> {
    let o = &h.state().orbit;
    let m = &h.state().session.model;
    let scene = bimcraft_core::view3d::Scene::new(m);
    let image = scene.render_in(&camera(h), &[], &bimcraft_core::graphics::Look::of_3d(m));
    let at = (pos - o.rect.min) * o.ppp;
    image.pick(f64::from(at.x), f64::from(at.y))
}

/// The work plane of the default 3D view.
fn plane(h: &Harness<BimApp>) -> bimcraft_core::workplane::Frame {
    let m = &h.state().session.model;
    m.work_plane(m.default_3d()).resolve(m).unwrap()
}

#[test]
fn the_triad_shows_in_the_3d_views_corner_beside_the_navigation_bar_in_both_profiles() {
    for rev in [true, false] {
        let h = three_d(rev);
        let view = h.state().orbit.rect;
        let rect = h.state().triad_rect(view).expect("a corner for it");
        assert!(view.contains_rect(rect), "rev {rev}: {rect:?} in {view:?}");
        let nav = h.state().nav_corner(view);
        assert!(rect.right() < nav.x && (rect.top() - nav.y).abs() < 1.0, "rev {rev}: beside the navigation bar, clear of it");
        for end in ENDS {
            assert!(rect.contains(end_pos(&h, end)), "rev {rev}: {end:?} is in the triad");
        }
    }
}

#[test]
fn the_triad_turns_with_the_camera() {
    for rev in [true, false] {
        let mut h = three_d(rev);
        let before = end_pos(&h, End::PlusX);
        let cam = camera(&h);
        h.state_mut().orbit.camera = Some(Camera { azimuth: cam.azimuth + 70.0, ..cam });
        h.run();
        let after = end_pos(&h, End::PlusX);
        assert!(before.distance(after) > 10.0, "rev {rev}: x moved from {before:?} to {after:?}");
        // Where the core's layout puts it, scaled to the corner.
        let placed = triad::layout(&camera(&h));
        let x = placed.iter().find(|p| p.end == End::PlusX).unwrap();
        let rect = h.state().triad_rect(h.state().orbit.rect).unwrap();
        assert!((after.x > rect.center().x) == (x.x > 0.0), "rev {rev}: {after:?} {x:?}");
    }
}

#[test]
fn clicking_the_z_end_gives_the_top_view_and_clicking_it_again_the_bottom_view() {
    for rev in [true, false] {
        let mut h = three_d(rev);
        let before = camera(&h);
        click_end(&mut h, End::PlusZ);
        assert_eq!(looking(&h), Side::Top.angles(), "rev {rev}: the top view");
        let top = camera(&h);
        assert_eq!((top.target, top.scale, top.width, top.height), (before.target, before.scale, before.width, before.height));
        assert!(triad::looks_from(&top, End::PlusZ));
        click_end(&mut h, End::PlusZ);
        assert_eq!(looking(&h), Side::Bottom.angles(), "rev {rev}: a second click, the bottom view");
        click_end(&mut h, End::PlusZ);
        assert_eq!(looking(&h), Side::Top.angles(), "rev {rev}: and a third, the top again");
        // The z end of the other sign is the bottom at once.
        home(&mut h);
        click_end(&mut h, End::MinusZ);
        assert_eq!(looking(&h), Side::Bottom.angles(), "rev {rev}");
    }
}

#[test]
fn each_end_turns_the_camera_to_its_side_and_a_second_click_to_the_opposite_one() {
    for rev in [true, false] {
        let mut h = three_d(rev);
        for end in ENDS {
            home(&mut h);
            click_end(&mut h, end);
            assert_eq!(looking(&h), end.side().angles(), "rev {rev}: {end:?}");
            click_end(&mut h, end);
            assert_eq!(looking(&h), end.opposite().side().angles(), "rev {rev}: {end:?} again");
        }
    }
}

#[test]
fn hovering_an_end_names_the_view_a_click_turns_to_and_with_alt_the_work_plane_it_would_set() {
    for rev in [true, false] {
        let mut h = three_d(rev);
        assert_eq!(h.state().triad_label(), None, "rev {rev}: nothing under the pointer");
        for (end, name) in [
            (End::PlusZ, "Top"),
            (End::MinusZ, "Bottom"),
            (End::MinusY, "Front"),
            (End::PlusY, "Back"),
            (End::PlusX, "Right"),
            (End::MinusX, "Left"),
        ] {
            home(&mut h);
            let at = end_pos(&h, end);
            h.hover_at(at);
            h.run();
            assert_eq!(h.state().triad_label().as_deref(), Some(name), "rev {rev}: {end:?}");
        }
        // Already on top, the z end names where the next click goes.
        click_end(&mut h, End::PlusZ);
        let at = end_pos(&h, End::PlusZ);
        h.hover_at(at);
        h.run();
        assert_eq!(h.state().triad_label().as_deref(), Some("Bottom"), "rev {rev}");
        // With Alt held, the work plane.
        h.event(Event::ModifiersChanged(Modifiers::ALT));
        h.run();
        assert_eq!(h.state().triad_label().as_deref(), Some("Work plane square to the Z axis"), "rev {rev}");
        // On the triad's empty space, only Alt has something to say.
        let empty = h.state().triad_empty().expect("a place with no end");
        h.hover_at(empty);
        h.run();
        assert_eq!(h.state().triad_label().as_deref(), Some("Work plane back to the level"), "rev {rev}");
        h.event(Event::ModifiersChanged(Modifiers::NONE));
        h.run();
        assert_eq!(h.state().triad_label(), None, "rev {rev}");
        // Off the triad, nothing.
        h.hover_at(h.state().orbit.rect.left_top() + vec2(20.0, 120.0));
        h.run();
        assert_eq!(h.state().triad_label(), None, "rev {rev}");
    }
}

#[test]
fn alt_and_a_click_on_an_end_sets_the_work_plane_square_to_that_axis_and_on_empty_space_back_to_the_level() {
    for rev in [true, false] {
        let mut h = three_d(rev);
        assert_eq!(plane(&h), bimcraft_core::workplane::Frame::flat(0.0), "rev {rev}: the level to begin with");
        for (end, normal) in
            [(End::PlusX, [1.0, 0.0, 0.0]), (End::MinusY, [0.0, -1.0, 0.0]), (End::PlusZ, [0.0, 0.0, 1.0]), (End::MinusZ, [0.0, 0.0, 1.0])]
        {
            home(&mut h);
            let target = camera(&h).target.map(f64::round);
            let at = end_pos(&h, end);
            click_holding(&mut h, at, Modifiers::ALT);
            let frame = plane(&h);
            assert_eq!(frame.normal, normal, "rev {rev}: {end:?}: {}", h.state().status.text);
            // Through the target's place along the axis the plane is square to, to the millimetre.
            assert!((frame.origin[end.axis()] - target[end.axis()]).abs() < 1e-9, "rev {rev}: {end:?}: {:?} {target:?}", frame.origin);
            assert_eq!(h.state().status.text, format!("Work plane square to the {} axis", end.axis_name()), "rev {rev}");
            assert!(!h.state().status.error);
            // The view looks as it did: a work plane click doesn't turn the camera.
            assert_eq!(looking(&h), (-45.0, 30.0), "rev {rev}: {end:?}");
        }
        // It is the model's, with its undo.
        let m = &h.state().session.model;
        assert!(m.default_3d().is_some());
        assert!(m.work_plane(m.default_3d()).surface.is_some(), "rev {rev}: a surface plane");
        // Empty space of the triad, Alt held: back to the level.
        let empty = h.state().triad_empty().expect("a place with no end");
        click_holding(&mut h, empty, Modifiers::ALT);
        assert_eq!(plane(&h), bimcraft_core::workplane::Frame::flat(0.0), "rev {rev}");
        let m = &h.state().session.model;
        assert!(m.work_plane(m.default_3d()).surface.is_none() && m.work_plane(m.default_3d()).level.is_some(), "rev {rev}");
        assert_eq!(h.state().status.text, "Work plane back to the level", "rev {rev}");
        // Again: nothing to go back from, and it says so rather than making an undo step.
        let steps = h.state().session.history().len();
        click_holding(&mut h, empty, Modifiers::ALT);
        assert_eq!(h.state().session.history().len(), steps, "rev {rev}");
        assert_eq!(h.state().status.text, "The work plane is on the level already", "rev {rev}");
    }
}

#[test]
fn a_plain_click_on_empty_space_in_the_triad_does_nothing_and_alt_and_a_click_on_an_end_does_not_turn_the_camera() {
    for rev in [true, false] {
        let mut h = three_d(rev);
        let before = camera(&h);
        let empty = h.state().triad_empty().expect("a place with no end");
        click_at(&mut h, empty);
        assert_eq!(camera(&h), before, "rev {rev}");
        assert_eq!(plane(&h), bimcraft_core::workplane::Frame::flat(0.0), "rev {rev}");
        assert!(h.state().session.model.default_3d().is_none() || h.state().status.text.is_empty(), "rev {rev}");
    }
}

#[test]
fn a_click_on_the_triad_never_reaches_the_model_behind_it() {
    for rev in [true, false] {
        let mut h = three_d(rev);
        // Close in on the south wall's outer face, so the wall fills the whole view, the triad's corner too.
        let cam = camera(&h);
        let close = Camera { target: [4000.0, 0.0, 1500.0], scale: cam.scale * 8.0, ..cam };
        h.state_mut().orbit.camera = Some(close);
        h.run();
        let wall = ids(&h, "wall")[0];
        let view = h.state().orbit.rect;
        // The control: a click elsewhere on the wall picks it, and a click on empty ground picks nothing.
        click_at(&mut h, view.center());
        assert_eq!(h.state().selection.len(), 1, "rev {rev}: the wall under the middle of the view: {:?}", h.state().selection);
        h.state_mut().selection.clear();
        h.run();
        // The triad's empty space and its ends: the model behind is not picked.
        let triad = h.state().triad_rect(view).unwrap();
        assert_eq!(model_at(&h, triad.center()), Some(wall), "rev {rev}: the wall is behind the triad");
        let empty = h.state().triad_empty().expect("a place with no end");
        click_at(&mut h, empty);
        assert!(h.state().selection.is_empty(), "rev {rev}: empty space: {:?}", h.state().selection);
        for end in ENDS {
            h.state_mut().selection.clear();
            let at = end_pos(&h, end);
            click_at(&mut h, at);
            assert!(!h.state().selection.contains(&wall), "rev {rev}: {end:?} selected the wall behind it");
            home(&mut h);
            h.state_mut().orbit.camera = Some(Camera { target: [4000.0, 0.0, 1500.0], scale: cam.scale * 8.0, ..camera(&h) });
            h.run();
        }
        // And hovering it lights nothing of the model.
        h.state_mut().selection.clear();
        h.hover_at(triad.center());
        h.run();
        assert!(h.state().selection.is_empty());
    }
}

#[test]
fn a_drag_that_begins_on_the_triad_does_not_orbit_the_view() {
    for rev in [true, false] {
        let mut h = three_d(rev);
        let before = camera(&h);
        let from = h.state().triad_empty().expect("a place with no end");
        drag_screen(&mut h, from, from + vec2(-120.0, 90.0), PointerButton::Primary);
        assert_eq!(camera(&h), before, "rev {rev}: the triad took the drag");
        // A drag elsewhere still orbits.
        let start = h.state().orbit.rect.center();
        orbit_screen(&mut h, start, start + vec2(60.0, 0.0));
        assert_ne!(camera(&h).azimuth, before.azimuth, "rev {rev}: the view still orbits");
    }
}

#[test]
fn the_triad_on_the_graphics_card_turns_the_view_the_same_way() {
    for rev in [true, false] {
        let Some(gpu) = headless() else {
            eprintln!("no graphics adapter here: the triad over the card's 3D view is not driven");
            return;
        };
        let mut h = if rev { app(BOX) } else { bimcraft(BOX) };
        h.state_mut().card = Card::with(gpu);
        h.state_mut().open_sheet(Sheet::View3D);
        h.run();
        assert!(h.state().orbit.on_card, "rev {rev}: the card drew it: {:?}", h.state().card.note);
        click_end(&mut h, End::PlusZ);
        assert_eq!(looking(&h), Side::Top.angles(), "rev {rev}");
        assert!(h.state().orbit.on_card, "rev {rev}: still the card's picture");
        click_end(&mut h, End::PlusZ);
        assert_eq!(looking(&h), Side::Bottom.angles(), "rev {rev}");
    }
}

#[test]
fn the_triad_turns_a_camera_views_camera_and_the_view_keeps_where_it_is_left() {
    for rev in [true, false] {
        let mut h = three_d(rev);
        h.state_mut().run("camera", json!({"eye": [-5000.0, -4000.0, 1600.0], "target": [4000.0, 3000.0, 1200.0]}));
        let view = find(&h, |e| matches!(e, Element::View3D(v) if v.camera.is_some()));
        h.state_mut().open_3d(Some(view));
        h.run();
        click_end(&mut h, End::PlusZ);
        h.run();
        let m = &h.state().session.model;
        let Some(Element::View3D(v)) = m.elements.get(view) else { panic!("the camera view") };
        let stored = v.camera.expect("a camera view");
        assert_eq!(stored.target, [4000.0, 3000.0, 1200.0], "rev {rev}: it still looks at the same point");
        assert!(stored.eye[2] > 1600.0 + 1000.0, "rev {rev}: the eye rose over the target: {:?}", stored.eye);
        assert!((stored.eye[0] - 4000.0).abs() < 1000.0, "rev {rev}: and stands nearly above it: {:?}", stored.eye);
    }
}
