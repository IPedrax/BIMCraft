//! Fly mode (FY, the wing on the 3D view's bar): W A S D fly the 3D view, Q and E raise and lower it, the mouse turns it about
//! the eye, and Esc leaves it. It is the 3D view's only.

use super::*;

fn hold(h: &mut Harness<BimApp>, key: Key, pressed: bool) {
    h.event(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
}

/// `n` frames.
fn frames(h: &mut Harness<BimApp>, n: usize) {
    for _ in 0..n {
        h.step();
    }
}

fn near(a: [f64; 3], b: [f64; 3], within: f64) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < within)
}

fn flying(h: &Harness<BimApp>) -> bool {
    h.state().orbit.fly.is_some()
}

#[test]
fn fy_flies_the_3d_view_in_perspective_and_esc_lands_it() {
    let mut h = app(BOX);
    press(&mut h, "Default 3D View");
    assert_eq!(h.state().orbit.camera.and_then(|c| c.lens), None, "the default 3D view begins parallel");
    shortcut(&mut h, "FY");
    assert!(flying(&h), "{:?}", h.state().status);
    // A parallel view has no eye to fly: it is seen in perspective.
    h.run();
    let start = h.state().orbit.camera.unwrap();
    assert!(start.lens.is_some(), "{start:?}");
    // It says so over the view, with the keys.
    assert!(h.query_by_label("Fly mode").is_some());
    assert!(h.query_by_label_contains("Esc leaves fly mode").is_some());
    let eye = start.eye().unwrap();
    let looks = start.toward(start.target).map(|v| -v);
    // W: forward, where it looks; the eye and the target go together, so it sees as far.
    hold(&mut h, Key::W, true);
    frames(&mut h, 5);
    hold(&mut h, Key::W, false);
    h.run();
    let ahead = h.state().orbit.camera.unwrap();
    let moved = std::array::from_fn::<f64, 3, _>(|k| ahead.eye().unwrap()[k] - eye[k]);
    let along: f64 = moved.iter().zip(looks).map(|(m, l)| m * l).sum::<f64>() / looks.iter().map(|l| l * l).sum::<f64>().sqrt();
    assert!(along > 50.0, "flew {moved:?} looking along {looks:?}");
    assert!((ahead.eye_distance().unwrap() - start.eye_distance().unwrap()).abs() < 1e-6);
    assert_eq!((ahead.azimuth, ahead.elevation), (start.azimuth, start.elevation), "W doesn't turn it");
    // Q rises, E falls, straight up and down.
    let z = ahead.eye().unwrap()[2];
    hold(&mut h, Key::Q, true);
    frames(&mut h, 5);
    hold(&mut h, Key::Q, false);
    h.run();
    let risen = h.state().orbit.camera.unwrap();
    assert!(risen.eye().unwrap()[2] > z + 50.0, "{z} to {:?}", risen.eye());
    assert!(near([risen.eye().unwrap()[0], risen.eye().unwrap()[1], 0.0], [ahead.eye().unwrap()[0], ahead.eye().unwrap()[1], 0.0], 1e-6));
    hold(&mut h, Key::E, true);
    frames(&mut h, 5);
    hold(&mut h, Key::E, false);
    h.run();
    assert!(h.state().orbit.camera.unwrap().eye().unwrap()[2] < risen.eye().unwrap()[2] - 50.0);
    // The mouse turns it about the eye, which stays where it is.
    let before = h.state().orbit.camera.unwrap();
    h.event(Event::MouseMoved(vec2(100.0, 0.0)));
    h.run();
    let turned = h.state().orbit.camera.unwrap();
    assert!((turned.azimuth - before.azimuth).abs() > 5.0, "{} to {}", before.azimuth, turned.azimuth);
    assert!(near(turned.eye().unwrap(), before.eye().unwrap(), 1e-3), "{:?} to {:?}", before.eye(), turned.eye());
    // The letters it flies with aren't typed as anything else: still in Modify, nothing made.
    assert_eq!(h.state().tool, Tool::Modify);
    // Esc lands it where it flew to, and the words go.
    key(&mut h, Key::Escape);
    assert!(!flying(&h));
    h.run();
    assert!(h.query_by_label_contains("Esc leaves fly mode").is_none());
    let landed = h.state().orbit.camera.unwrap();
    assert!(near(landed.eye().unwrap(), turned.eye().unwrap(), 1e-3));
    // W after it does nothing to the view.
    hold(&mut h, Key::W, true);
    frames(&mut h, 3);
    hold(&mut h, Key::W, false);
    h.run();
    assert!(near(h.state().orbit.camera.unwrap().eye().unwrap(), landed.eye().unwrap(), 1e-6));
}

#[test]
fn the_wing_on_the_3d_views_bar_flies_it_and_lands_it() {
    let mut h = app(BOX);
    press(&mut h, "Default 3D View");
    press(&mut h, "Fly Mode");
    assert!(flying(&h));
    press(&mut h, "Fly Mode");
    assert!(!flying(&h));
}

#[test]
fn fly_mode_is_the_3d_views_only_and_leaving_the_view_lands_it() {
    let mut h = app(BOX);
    let plan = h.state().sheet;
    shortcut(&mut h, "FY");
    assert!(!flying(&h));
    assert!(h.state().status.error && h.state().status.text.contains("3D view"), "{:?}", h.state().status);
    // The plan has no wing.
    assert!(h.query_by_label("Fly Mode").is_none());
    press(&mut h, "Default 3D View");
    shortcut(&mut h, "FY");
    assert!(flying(&h));
    h.state_mut().open_sheet(plan);
    h.run();
    assert!(!flying(&h));
    // W in the plan moves nothing and makes nothing.
    let before = h.state().session.model.elements.len();
    hold(&mut h, Key::W, true);
    frames(&mut h, 3);
    hold(&mut h, Key::W, false);
    h.run();
    assert_eq!(h.state().session.model.elements.len(), before);
}

#[test]
fn a_camera_view_keeps_where_it_flew_once_it_lands_in_one_step() {
    let mut h = app(BOX);
    let r = h.state_mut().run("camera", json!({"eye": [-3000.0, 3000.0, 1600.0], "target": [4000.0, 3000.0, 1600.0], "lens": 35.0}));
    let id = r.and_then(|r| r["id"].as_u64()).unwrap();
    h.state_mut().open_3d(Some(id));
    h.run();
    let undo = h.state().session.history().len();
    shortcut(&mut h, "FY");
    let lens = field(&h, id, "camera")["lens"].clone();
    hold(&mut h, Key::W, true);
    frames(&mut h, 5);
    hold(&mut h, Key::W, false);
    frames(&mut h, 3);
    // Flying writes nothing meanwhile.
    assert_eq!(h.state().session.history().len(), undo);
    key(&mut h, Key::Escape);
    h.run();
    let eye: Vec<f64> = field(&h, id, "camera")["eye"].as_array().unwrap().iter().filter_map(|v| v.as_f64()).collect();
    assert!(eye[0] > -3000.0 + 50.0 && (eye[1] - 3000.0).abs() < 1.0, "{eye:?}");
    assert_eq!(field(&h, id, "camera")["lens"], lens);
    assert_eq!(h.state().session.history().len(), undo + 1, "the flight is one step to undo");
}

/// Fly mode as the window shows it, for a person to look at: `BIMCRAFT_FLY_DUMP=<folder>` writes the 3D view flying, with the
/// banner over it and the wing lit on its bar, in each look and both profiles.
#[test]
fn fly_mode_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_FLY_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (profile, mut h) in [("rev", app(BOX)), ("bimcraft", bimcraft(BOX))] {
        h.set_size(vec2(1400.0, 900.0));
        h.state_mut().open_3d(None);
        h.run();
        shortcut(&mut h, "FY");
        for (look, theme, light_canvas) in
            [("light", egui::Theme::Light, false), ("dark", egui::Theme::Dark, false), ("dark-light-canvas", egui::Theme::Dark, true)]
        {
            h.state_mut().light_canvas = light_canvas;
            h.ctx.set_theme(theme);
            h.run();
            h.run();
            let name = format!("fly-{profile}-{look}.png");
            h.render().unwrap().save(std::path::Path::new(&folder).join(name)).unwrap();
        }
    }
}

#[test]
fn the_wheel_sets_how_fast_it_flies_and_zooms_nothing() {
    let mut h = app(BOX);
    press(&mut h, "Default 3D View");
    shortcut(&mut h, "FY");
    h.run();
    let (speed, cam) = (h.state().orbit.fly.unwrap().speed, h.state().orbit.camera.unwrap());
    let shown = crate::fly::speed_text(h.state().session.model.units, speed);
    assert!(h.query_by_label(&shown).is_some(), "{shown}");
    h.hover_at(h.state().orbit.rect.center());
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, 100.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    frames(&mut h, 10);
    let faster = h.state().orbit.fly.unwrap().speed;
    assert!(faster > speed * 1.4, "{speed} to {faster}");
    assert_eq!(h.state().orbit.camera.unwrap().scale, cam.scale, "the wheel doesn't zoom while it flies");
    assert!(shown.ends_with("m/s"), "{shown}");
}
