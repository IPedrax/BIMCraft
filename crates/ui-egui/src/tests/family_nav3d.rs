//! The family's {3D} drawn and moved as the project's 3D view is (`orbit.rs`): the wheel zooms toward the point under the
//! cursor, the middle button pans, Shift and the middle button orbit, a primary drag orbits about the point under the pointer,
//! the triad and the ViewCube turn it, and the 3D mouse moves it; on the card as on the processor.

use super::preview_visibility::{Pictures, cabinet};
use super::*;
use crate::orbit::Orbit;
use crate::spacemouse::SpaceMouse;
use bimcraft_core::triad::{self, End};
use bimcraft_core::view3d::Camera;

/// The cabinet's first box's front, and its second's, in the family's axes.
const PLINTH: [f64; 3] = [500.0, 0.0, 250.0];
const TOP: [f64; 3] = [2500.0, 0.0, 300.0];

fn camera(h: &Harness<BimApp>) -> Camera {
    h.state().active_family().and_then(|ed| ed.orbit.camera).expect("{3D} drawn")
}

fn at(h: &Harness<BimApp>, p: [f64; 3]) -> Pos2 {
    h.state().family_screen3(p).expect("{3D} drawn")
}

/// A drag with `button` and `modifiers` between points on screen, in steps.
fn drag_with(h: &mut Harness<BimApp>, a: Pos2, b: Pos2, button: PointerButton, modifiers: Modifiers) {
    h.event(Event::ModifiersChanged(modifiers));
    h.hover_at(a);
    h.run();
    h.event(Event::PointerButton { pos: a, button, pressed: true, modifiers });
    h.run();
    for t in [0.25, 0.5, 0.75, 1.0] {
        h.event(Event::PointerMoved(a + (b - a) * t));
        h.run();
    }
    h.event(Event::PointerButton { pos: b, button, pressed: false, modifiers });
    h.run();
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run();
}

fn opened() -> (Harness<'static, BimApp>, Pictures) {
    let (mut h, pictures) = cabinet();
    // Off the view, so nothing is lit under the pointer.
    h.hover_at(pos2(2.0, 2.0));
    h.run();
    (h, pictures)
}

#[test]
fn the_wheel_zooms_toward_the_point_under_the_cursor() {
    let (mut h, _) = opened();
    let (before, cursor) = (camera(&h), at(&h, PLINTH));
    h.hover_at(cursor);
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, 120.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.run();
    let after = camera(&h);
    assert!(after.scale > before.scale * 1.2, "closer: {} from {}", after.scale, before.scale);
    assert!(at(&h, PLINTH).distance(cursor) < 3.0, "the plinth stays under the cursor: {:?} from {cursor:?}", at(&h, PLINTH));
}

#[test]
fn the_middle_button_pans_and_shift_and_the_middle_button_orbit() {
    let (mut h, _) = opened();
    let (before, from) = (camera(&h), at(&h, PLINTH));
    drag_with(&mut h, from, from + vec2(80.0, 40.0), PointerButton::Middle, Modifiers::NONE);
    let panned = camera(&h);
    assert_eq!((panned.azimuth, panned.elevation, panned.scale), (before.azimuth, before.elevation, before.scale), "only moved");
    assert!(at(&h, PLINTH).distance(from + vec2(80.0, 40.0)) < 3.0, "the plinth follows the hand: {:?}", at(&h, PLINTH));
    let from = at(&h, PLINTH);
    drag_with(&mut h, from, from + vec2(100.0, 0.0), PointerButton::Middle, Modifiers::SHIFT);
    let turned = camera(&h);
    assert!((turned.azimuth - panned.azimuth).abs() > 20.0, "turned: {} from {}", turned.azimuth, panned.azimuth);
}

#[test]
fn a_primary_drag_orbits_about_the_point_under_the_pointer_where_it_began() {
    let (mut h, _) = opened();
    let (before, from) = (camera(&h), at(&h, TOP));
    drag_with(&mut h, from, from + vec2(-90.0, 30.0), PointerButton::Primary, Modifiers::NONE);
    let after = camera(&h);
    assert!((after.azimuth - before.azimuth).abs() > 20.0 && after.elevation != before.elevation, "{after:?}");
    assert!(at(&h, TOP).distance(from) < 6.0, "the top's front stays where the drag began: {:?} from {from:?}", at(&h, TOP));
    assert_eq!(h.state().active_family().unwrap().selected_form(), None, "a drag selects nothing");
}

#[test]
fn the_triad_and_the_viewcube_turn_the_family_s_3d() {
    let (mut h, _) = opened();
    let pane = h.state().view_rect;
    let corner = Orbit::cube_rect(pane).left_top();
    let top = h.state().active_family().unwrap().orbit.triad_at(End::PlusZ, corner).expect("the triad shows");
    assert!(pane.contains(top) && !Orbit::cube_rect(pane).contains(top), "beside the ViewCube: {top:?}");
    click_at(&mut h, top);
    assert!(triad::looks_from(&camera(&h), End::PlusZ), "seen from the top: {:?}", camera(&h));
    let faces = Orbit::cube_faces(&camera(&h), pane);
    let front = faces.iter().find(|(_, label, ..)| *label == "FRONT").map(|(poly, ..)| poly.clone());
    // From the top the front face is seen edge on: turn back to the default view first, with the house.
    let home = Orbit::home_rect(pane).center();
    if front.is_none() {
        click_at(&mut h, home);
    }
    let faces = Orbit::cube_faces(&camera(&h), pane);
    let front = faces.iter().find(|(_, label, ..)| *label == "FRONT").expect("the front face shows");
    let centre = front.0.iter().fold(Pos2::ZERO, |a, q| a + q.to_vec2() / 4.0);
    click_at(&mut h, centre);
    let now = camera(&h);
    assert_eq!((now.azimuth, now.elevation), (-90.0, 0.0), "seen from the front");
    assert_eq!(h.state().active_family().unwrap().selected_form(), None, "the cube's clicks select nothing");
    click_at(&mut h, home);
    let now = camera(&h);
    assert_eq!((now.azimuth, now.elevation), (bimcraft_core::view3d::AZIMUTH, bimcraft_core::view3d::ELEVATION), "home again");
}

#[test]
fn the_3d_mouse_turns_the_family_s_3d() {
    let (mut h, _) = opened();
    let start = camera(&h);
    h.state_mut().space_mouse = Some(SpaceMouse::held([0.0, 0.0, 0.0, 0.0, 0.0, 0.8]));
    for _ in 0..3 {
        h.step();
    }
    let spun = camera(&h);
    assert!((spun.azimuth - start.azimuth).abs() > 5.0, "{spun:?} from {start:?}");
    h.state_mut().space_mouse = None;
    h.run();
}

mod gpu {
    use super::*;

    /// The cabinet's family open in a window with a card, and the project's 3D view drawn on it before.
    fn on_card() -> Option<Harness<'static, BimApp>> {
        let gpu = crate::gpu::headless()?;
        let mut s = Session::new();
        s.script(&format!("{BOX}\n{}", super::super::preview_visibility::CABINET)).unwrap();
        let mut app = BimApp::new(s);
        app.card = crate::gpu::Card::with(gpu);
        let mut h =
            Harness::builder().with_size([1600.0, 900.0]).with_max_steps(64).build_ui_state(|ui, app: &mut BimApp| app.show(ui), app);
        h.run();
        press(&mut h, "Default 3D View");
        h.run();
        assert!(h.state().orbit.on_card, "the project's 3D view on the card");
        h.state_mut().edit_family("Cabinet");
        h.run();
        h.hover_at(pos2(2.0, 2.0));
        h.run();
        Some(h)
    }

    #[test]
    fn the_family_s_3d_is_drawn_on_the_card_picked_through_its_mesh_and_moved_by_the_hand() {
        let Some(mut h) = on_card() else { return };
        let ed = h.state().active_family().unwrap();
        assert!(ed.orbit.on_card, "{{3D}} on the card");
        // The project's mesh is kept on the card beside the family's.
        let project = h.state().orbit.mesh.as_ref().and_then(|(_, m)| m.as_ref().ok()).cloned().expect("the project's mesh");
        assert!(h.state().card.gpu.as_ref().unwrap().parks(&project), "the project's mesh parked");
        let at = at(&h, PLINTH);
        click_at(&mut h, at);
        assert_eq!(h.state().active_family().unwrap().selected_form(), Some(0), "a click on the card's picture picks the first box");
        let before = camera(&h);
        h.hover_at(at);
        h.event(Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, 120.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        h.run();
        assert!(camera(&h).scale > before.scale * 1.2, "the wheel zooms on the card too");
    }
}

/// The ViewCube drawn alone as `cam` turns it, with the pointer at `hover`: the colours at `at`. (The family's {3D} keeps its
/// pictures in a renderer that draws none, so the cube is drawn on a harness of its own.)
fn cube_ink(cam: Camera, hover: Option<Pos2>, at: &[Pos2]) -> Vec<[u8; 4]> {
    let rect = egui::Rect::from_min_size(Pos2::ZERO, vec2(240.0, 140.0));
    let mut h = Harness::builder().with_size([240.0, 140.0]).build_ui(move |ui| Orbit::draw_cube(ui.painter(), &cam, rect, hover));
    h.run();
    let image = h.render().unwrap();
    at.iter().map(|p| image.get_pixel(p.x.round() as u32, p.y.round() as u32).0).collect()
}

/// The cube with the pointer on a face, on an edge and on a corner, from the default view and from the front, four times
/// its size: saved where `BIMCRAFT_VIEWCUBE_DUMP` says (only then).
#[test]
fn viewcube_zones_screenshots() {
    let Ok(dir) = std::env::var("BIMCRAFT_VIEWCUBE_DUMP") else { return };
    std::fs::create_dir_all(&dir).unwrap();
    let (h, _) = opened();
    let home = Camera { azimuth: bimcraft_core::view3d::AZIMUTH, elevation: bimcraft_core::view3d::ELEVATION, ..camera(&h) };
    let front = Camera { azimuth: -90.0, elevation: 0.0, ..home };
    let rect = egui::Rect::from_min_size(Pos2::ZERO, vec2(120.0, 110.0));
    for (view, cam) in [("home", home), ("front", front)] {
        for (name, zone) in [("none", None), ("face", Some([0, -1, 0])), ("edge", Some([1, -1, 0])), ("corner", Some([1, -1, 1]))] {
            let hover = zone.and_then(|z| Orbit::cube_zone_at(&cam, rect, z).first().copied());
            let mut h = Harness::builder().with_size([120.0, 110.0]).with_pixels_per_point(4.0).build_ui(move |ui| {
                ui.painter().rect_filled(ui.max_rect().expand(8.0), 0.0, egui::Color32::WHITE);
                Orbit::draw_cube(ui.painter(), &cam, rect, hover);
            });
            h.run();
            h.render().unwrap().save(std::path::Path::new(&dir).join(format!("viewcube-{view}-{name}.png"))).unwrap();
        }
    }
}

#[test]
fn the_viewcube_s_edges_and_corners_look_from_between_its_faces() {
    let (mut h, _) = opened();
    let pane = h.state().view_rect;
    let home = Orbit::home_rect(pane).center();
    // From the default view the top, the front and the right show: the edges between them, and the corner of the three.
    for (zone, azimuth, elevation) in
        [([1, -1, 0], -45.0, 0.0), ([0, -1, 1], -90.0, 45.0), ([1, 0, 1], 0.0, 45.0), ([1, -1, 1], -45.0, 35.264_389_682_754_654)]
    {
        click_at(&mut h, home);
        let cam = camera(&h);
        let shown = Orbit::cube_zone_at(&cam, pane, zone);
        // An edge shows on the two faces it joins, a corner on three: a click on any of its parts is the same zone's.
        assert_eq!(shown.len(), zone.iter().filter(|c| **c != 0).count(), "{zone:?}");
        for at in shown {
            click_at(&mut h, home);
            assert_eq!(Orbit::cube_zone(&camera(&h), pane, at), Some(zone));
            click_at(&mut h, at);
            let now = camera(&h);
            assert!((now.azimuth - azimuth).abs() < 1e-9 && (now.elevation - elevation).abs() < 1e-9, "{zone:?}: {now:?}");
            assert_eq!(h.state().active_family().unwrap().selected_form(), None, "the cube's clicks select nothing");
        }
    }
    // A face's middle is still the face's.
    click_at(&mut h, home);
    let front = Orbit::cube_zone_at(&camera(&h), pane, [0, -1, 0]);
    assert_eq!(front.len(), 1);
    click_at(&mut h, front[0]);
    let now = camera(&h);
    assert_eq!((now.azimuth, now.elevation), (-90.0, 0.0), "seen from the front");
    // Seen from the front only that face shows, and its rim is the edges and the corners round it: its right strip is the
    // edge it shares with the right, its top right square the corner over it.
    for (zone, azimuth, elevation) in [([1, -1, 0], -45.0, 0.0), ([1, -1, 1], -45.0, 35.264_389_682_754_654), ([0, -1, -1], -90.0, -45.0)] {
        click_at(&mut h, home);
        click_at(&mut h, front[0]);
        let shown = Orbit::cube_zone_at(&camera(&h), pane, zone);
        assert_eq!(shown.len(), 1, "{zone:?}: on the front alone");
        click_at(&mut h, shown[0]);
        let now = camera(&h);
        assert!((now.azimuth - azimuth).abs() < 1e-9 && (now.elevation - elevation).abs() < 1e-9, "{zone:?}: {now:?}");
    }
}

#[test]
fn the_viewcube_lights_the_zone_under_the_pointer_on_every_face_that_shows_it() {
    let (h, _) = opened();
    let cam = Camera { azimuth: bimcraft_core::view3d::AZIMUTH, elevation: bimcraft_core::view3d::ELEVATION, ..camera(&h) };
    let rect = egui::Rect::from_min_size(Pos2::ZERO, vec2(240.0, 140.0));
    let edge = Orbit::cube_zone_at(&cam, rect, [1, -1, 0]);
    let corner = Orbit::cube_zone_at(&cam, rect, [1, -1, 1]);
    // The front's middle, under its name.
    let face = Orbit::cube_zone_at(&cam, rect, [0, -1, 0])[0] + vec2(0.0, 9.0);
    assert_eq!((edge.len(), corner.len(), Orbit::cube_zone(&cam, rect, face)), (2, 3, Some([0, -1, 0])));
    let probes: Vec<Pos2> = edge.iter().chain(&corner).copied().chain([face]).collect();
    // The pointer off the cube: nothing lit.
    let plain = cube_ink(cam, None, &probes);
    // Over the edge's strip on one face, its strip on the face beside lights too; the corner and the face's middle don't.
    let lit = cube_ink(cam, Some(edge[0]), &probes);
    assert_ne!(lit[0], plain[0], "the strip under the pointer");
    assert_ne!(lit[1], plain[1], "and the same edge's strip on the face beside");
    assert_eq!(lit[0], lit[1], "lit alike");
    assert_eq!(lit[2..], plain[2..], "the corner and the face stay as they were");
    // Over the corner, its three squares; over the face's middle, that alone.
    let lit = cube_ink(cam, Some(corner[1]), &probes);
    assert!(lit[2..5].iter().zip(&plain[2..5]).all(|(a, b)| a != b), "the corner's three squares");
    assert_eq!((&lit[..2], lit[5]), (&plain[..2], plain[5]));
    let lit = cube_ink(cam, Some(face), &probes);
    assert_ne!(lit[5], plain[5], "the face's middle");
    assert_eq!(lit[..5], plain[..5]);
}
