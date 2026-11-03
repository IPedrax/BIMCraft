//! The 3D view as a working view: a drag orbits round the point under the cursor and the wheel zooms toward it. In both
//! profiles, on the processor's picture and, where the machine has a graphics adapter, on the card's.

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
