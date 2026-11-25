//! A sheet opened in the window, driven headless: its canvas, the picture the thread paints coming in through the poll, and the wheel.

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

fn until_painted(rt: &mut Runtime<App>) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while rt.state.paper.frame.is_none() || rt.state.views_busy() {
        assert!(Instant::now() < deadline, "the sheet was not painted");
        rt.frame(0.016);
        std::thread::sleep(Duration::from_millis(2));
    }
    for _ in 0..2 {
        rt.frame(0.016);
    }
}

fn first_sheet(rt: &Runtime<App>) -> ViewKind {
    rt.state
        .views
        .groups
        .iter()
        .find(|g| g.label == "Sheets (all)")
        .and_then(|g| g.entries.first())
        .map(|e| e.kind)
        .expect("the sample has sheets")
}

#[test]
fn a_sheet_has_a_canvas_of_its_own_and_its_picture_comes_in_through_the_poll() {
    let mut rt = window();
    let sheet = first_sheet(&rt);
    rt.update(|s, _| {
        s.open_view(sheet);
    });
    rt.frame(0.0);
    assert!(rt.find("sheet").is_some() && rt.find("plan").is_none() && rt.find("view-pending").is_none() && rt.find("section").is_none());
    until_painted(&mut rt);
    assert!(rt.find("sheet-picture").is_some() && rt.find("sheet-world").is_some());
    assert!(rt.find("sheet-note").is_none(), "the note goes once the picture is there");
    assert!(rt.state.paper.revision >= 1 && rt.state.paper.error.is_none());
}

#[test]
fn the_wheel_zooms_the_sheet_and_the_exact_picture_follows() {
    let mut rt = window();
    let sheet = first_sheet(&rt);
    rt.update(|s, _| {
        s.open_view(sheet);
    });
    until_painted(&mut rt);
    let (rect, _) = rt.presented("sheet").expect("the canvas");
    let before = rt.state.paper.camera.scale;
    rt.wheel_event(WheelEvent::new(0.0, 200.0).at(rect[0] + 300.0, rect[1] + 200.0));
    rt.frame(0.0);
    assert!(rt.state.paper.camera.scale > before, "wheel forward zooms in");
    until_painted(&mut rt);
    assert!((rt.state.paper.live_scale.get() - 1.0).abs() < 1e-6, "the exact picture came in for the camera");
}
