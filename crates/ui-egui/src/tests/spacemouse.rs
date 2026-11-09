//! The 3D mouse moves the active view when it is a 3D view, and only it: plans, sections and sheets it leaves alone, and a 3D
//! view drawn beside the active one too.

use super::*;
use crate::spacemouse::SpaceMouse;

/// `n` frames, the cap held as it is: a held cap asks for frame after frame, so the harness never runs out of them.
fn frames(h: &mut Harness<BimApp>, n: usize) {
    for _ in 0..n {
        h.step();
    }
}

/// How far `to` is turned from `from` about the vertical, degrees, -180 to 180.
fn turned(from: f64, to: f64) -> f64 {
    (to - from + 180.0).rem_euclid(360.0) - 180.0
}

#[test]
fn a_held_cap_turns_and_zooms_the_active_3d_view_and_the_wheel_waits() {
    let mut h = app(BOX);
    press(&mut h, "Default 3D View");
    let start = h.state().orbit.camera.unwrap();
    // Twisted: the model spins about the vertical, and a wheel turn meanwhile (3DxWare's, from the tilt) doesn't zoom.
    h.state_mut().space_mouse = Some(SpaceMouse::held([0.0, 0.0, 0.0, 0.0, 0.0, 0.8]));
    let mid = h.state().orbit.rect.center();
    h.hover_at(mid);
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, 120.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    frames(&mut h, 3);
    let spun = h.state().orbit.camera.unwrap();
    let by = turned(start.azimuth, spun.azimuth);
    assert!(by > 5.0 && by < 30.0 && spun.elevation == start.elevation, "{by} {spun:?}");
    assert_eq!(spun.scale, start.scale, "the wheel waited");
    // Pulled toward the user: closer.
    h.state_mut().space_mouse = Some(SpaceMouse::held([0.0, 0.8, 0.0, 0.0, 0.0, 0.0]));
    frames(&mut h, 3);
    assert!(h.state().orbit.camera.unwrap().scale > spun.scale * 1.05);
}

#[test]
fn beside_an_active_plan_the_cap_moves_neither_the_plan_nor_the_3d_view() {
    let mut h = app(BOX);
    let plan = h.state().sheet;
    let Sheet::Plan(level) = plan else { panic!("{plan:?}") };
    press(&mut h, "Default 3D View");
    h.state_mut().float_view(Sheet::View3D, pos2(700.0, 400.0));
    h.run();
    h.state_mut().open_sheet(plan);
    h.run();
    assert_eq!(h.state().sheet, plan, "the plan is the active view, the 3D view floats beside it");
    let (start, view) = (h.state().orbit.camera, h.state().canvas.current_view(level));
    assert!(start.is_some(), "the 3D view is drawn beside the plan");
    h.state_mut().space_mouse = Some(SpaceMouse::held([0.5, 0.5, 0.5, 0.5, 0.0, 0.8]));
    frames(&mut h, 3);
    assert_eq!(h.state().orbit.camera, start, "the 3D view beside the active one stays as it was");
    assert_eq!(h.state().canvas.current_view(level), view, "the plan stays as it was");
}

#[test]
fn of_several_3d_views_the_cap_moves_the_active_one_and_keeps_only_its_camera() {
    let mut h = app(BOX);
    let mut made = Vec::new();
    for eye in [[-1000.0, 3000.0, 1600.0], [4000.0, -2000.0, 1600.0]] {
        let r = h.state_mut().run("camera", json!({"eye": eye, "target": [4000.0, 3000.0, 1200.0], "lens": 35.0}));
        made.extend(r.and_then(|r| r["id"].as_u64()));
    }
    let &[a, b] = made.as_slice() else { panic!("{made:?}") };
    let (eye_a, eye_b) = (field(&h, a, "camera")["eye"].clone(), field(&h, b, "camera")["eye"].clone());
    h.state_mut().open_3d(Some(b));
    h.run();
    assert_eq!((h.state().sheet, h.state().active_3d()), (Sheet::View3D, Some(b)));
    h.state_mut().space_mouse = Some(SpaceMouse::held([0.0, 0.0, 0.0, 0.0, 0.0, 0.8]));
    frames(&mut h, 3);
    // Let go: the view shown keeps where the cap took it; the other camera view is as it was.
    h.state_mut().space_mouse = Some(SpaceMouse::held([0.0; 6]));
    h.run();
    assert_ne!(field(&h, b, "camera")["eye"], eye_b, "the active camera view is written back where the cap left it");
    assert_eq!(field(&h, a, "camera")["eye"], eye_a, "the camera view not shown stays");
}

#[test]
fn opening_a_project_keeps_the_3d_mouse() {
    let mut h = app(BOX);
    h.state_mut().space_mouse = Some(SpaceMouse::held([0.0, 0.0, 0.0, 0.0, 0.0, 0.8]));
    // New and Open start the window over on the model opened (`reopened`): the reader is the window's, and goes along.
    h.state_mut().reopened();
    let mouse = h.state_mut().space_mouse.take();
    assert!(mouse.is_some(), "the window lost its 3D mouse when the project was opened");
    press(&mut h, "Default 3D View");
    let start = h.state().orbit.camera.unwrap();
    h.state_mut().space_mouse = mouse;
    frames(&mut h, 3);
    assert!(turned(start.azimuth, h.state().orbit.camera.unwrap().azimuth) > 5.0, "the cap turns the 3D view after the open");
}

#[test]
fn without_a_3d_view_on_screen_the_cap_moves_nothing() {
    let mut h = app(BOX);
    let Sheet::Plan(level) = h.state().sheet else { panic!() };
    let view = h.state().canvas.current_view(level);
    h.state_mut().space_mouse = Some(SpaceMouse::held([0.8, 0.8, 0.8, 0.8, 0.8, 0.8]));
    frames(&mut h, 3);
    assert_eq!(h.state().canvas.current_view(level), view);
    assert_eq!(h.state().orbit.camera, None, "no 3D view was drawn");
}
