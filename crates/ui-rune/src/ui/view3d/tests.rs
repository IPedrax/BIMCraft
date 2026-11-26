//! A 3D view opened in the window, driven headless: its canvas, the picture the thread makes coming in through the poll, the wheel.

use std::time::{Duration, Instant};

use rune::{Env, Runtime, WheelEvent};

use crate::app::App;
use crate::ui::{poll, view};
use crate::views::ViewKind;

fn window() -> Runtime<App> {
    let mut rt = Runtime::new(App::sample(), view, (1280.0, 800.0), Env::new(true, 1.0));
    rt.poll = Some(poll);
    for _ in 0..3 {
        rt.frame(0.0);
    }
    rt
}

fn until_made(rt: &mut Runtime<App>) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while rt.state.orbit.frame.is_none() || rt.state.views_busy() {
        assert!(Instant::now() < deadline, "the 3D view was not made");
        rt.frame(0.016);
        std::thread::sleep(Duration::from_millis(2));
    }
    for _ in 0..2 {
        rt.frame(0.016);
    }
}

#[test]
fn a_3d_view_has_a_canvas_of_its_own_and_its_picture_comes_in_through_the_poll() {
    let mut rt = window();
    rt.update(|s, _| {
        s.open_view(ViewKind::View3D(None));
    });
    rt.frame(0.0);
    assert!(rt.find("view3d").is_some() && rt.find("plan").is_none() && rt.find("view-pending").is_none());
    until_made(&mut rt);
    assert!(rt.find("view3d-picture").is_some() && rt.find("view3d-world").is_some() && rt.find("view3d-note").is_none());
    assert!(rt.state.orbit.revision >= 1 && rt.state.orbit.error.is_none());
}

#[test]
fn the_wheel_zooms_the_3d_view_and_the_exact_picture_follows() {
    let mut rt = window();
    rt.update(|s, _| {
        s.open_view(ViewKind::View3D(None));
    });
    until_made(&mut rt);
    let (rect, _) = rt.presented("view3d").expect("the canvas");
    let before = rt.state.orbit.camera.map_or(0.0, |c| c.scale);
    rt.wheel_event(WheelEvent::new(0.0, 200.0).at(rect[0] + 300.0, rect[1] + 200.0));
    rt.frame(0.0);
    assert!(rt.state.orbit.camera.map_or(0.0, |c| c.scale) > before, "wheel forward zooms in");
    until_made(&mut rt);
    assert!((rt.state.orbit.live_scale.get() - 1.0).abs() < 1e-6, "the exact picture came in for the camera");
}
