//! A section opened in the window, driven headless: the canvas comes with the browser's press, the picture comes in when the thread
//! has made it, and the wheel moves it.

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

/// Frames until the section's thread has answered and its picture is in.
fn until_made(rt: &mut Runtime<App>) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while rt.state.cut.frame.is_none() || rt.state.views_busy() {
        assert!(Instant::now() < deadline, "the section was not made");
        rt.frame(0.016);
        std::thread::sleep(Duration::from_millis(2));
    }
    for _ in 0..2 {
        rt.frame(0.016);
    }
}

fn first_section(rt: &Runtime<App>) -> ViewKind {
    rt.state
        .views
        .groups
        .iter()
        .find(|g| g.label == "Sections (Building Section)")
        .and_then(|g| g.entries.first())
        .map(|e| e.kind)
        .expect("the sample has a section")
}

#[test]
fn a_section_has_a_canvas_of_its_own_and_its_picture_comes_in_through_the_poll() {
    let mut rt = window();
    let section = first_section(&rt);
    assert!(rt.find("section").is_none() && rt.find("plan").is_some());
    rt.update(|s, _| {
        s.open_view(section);
    });
    rt.frame(0.0);
    assert!(rt.find("section").is_some() && rt.find("plan").is_none() && rt.find("view-pending").is_none(), "the section's own canvas");
    assert!(rt.find("section-note").is_some() || rt.state.cut.frame.is_some(), "it says it is being made, until it is");
    until_made(&mut rt);
    assert!(rt.find("section-picture").is_some() && rt.find("section-world").is_some());
    assert!(rt.find("section-note").is_none(), "the note goes once the picture is there");
    assert!(rt.state.cut.revision >= 1 && rt.state.cut.error.is_none());
}

#[test]
fn the_wheel_zooms_the_section_about_the_pointer_and_the_exact_picture_follows() {
    let mut rt = window();
    let section = first_section(&rt);
    rt.update(|s, _| {
        s.open_view(section);
    });
    until_made(&mut rt);
    let (rect, _) = rt.presented("section").expect("the canvas");
    let before = rt.state.cut.camera.scale;
    rt.wheel_event(WheelEvent::new(0.0, 200.0).at(rect[0] + 300.0, rect[1] + 200.0));
    rt.frame(0.0);
    assert!(rt.state.cut.camera.scale > before, "wheel forward zooms in");
    until_made(&mut rt);
    assert!((rt.state.cut.live_scale.get() - 1.0).abs() < 1e-6, "the exact picture came in for the camera");
    assert!((rt.state.cut.frame.as_ref().map_or(0.0, |f| f.view.scale) - rt.state.cut.camera.scale).abs() < 1e-9);
}

#[test]
fn a_press_on_a_section_row_in_the_browser_opens_it() {
    let mut rt = window();
    let section = first_section(&rt);
    let key = section.key();
    let tap = |rt: &mut Runtime<App>, at: (f32, f32)| {
        rt.pointer_down(at.0, at.1);
        rt.pointer_up(at.0, at.1);
        // Time passes, so nothing is still on its way to where it will stand.
        for _ in 0..3 {
            rt.frame(0.3);
        }
    };
    // The row may be below what the list shows: the groups above it fold first, as a person folds them, until it is on screen.
    for label in ["Floor Plans", "Ceiling Plans", "3D Views", "Elevations (Building Elevation)"] {
        let at = rt.find(&key).expect("the section's row");
        if rt.contains(&key, at.0, at.1) {
            break;
        }
        let head = rt.find(&format!("group-{label}")).expect("the group");
        tap(&mut rt, head);
    }
    let at = rt.find(&key).expect("the section's row");
    assert!(rt.contains(&key, at.0, at.1), "the row is on screen: {at:?} in {:?}", rt.presented("browser"));
    tap(&mut rt, at);
    assert_eq!(rt.state.view, section, "the press on {at:?} opened it");
    assert!(rt.find("section").is_some());
}
