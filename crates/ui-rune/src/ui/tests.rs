//! The window driven as a person drives it, headless: Rune's own `Runtime` on the app's real state and view, with a pointer, a
//! wheel and presses at window points, and the elements read back (`Runtime::elements`).

use rune::{Env, Runtime, WheelEvent};

use super::view;
use crate::app::App;

fn window() -> Runtime<App> {
    let mut rt = Runtime::new(App::sample(), view, (1280.0, 800.0), Env::new(true, 1.0));
    for _ in 0..3 {
        rt.frame(0.0);
    }
    rt
}

/// Where the plan canvas is in the window, points.
fn plan_origin(rt: &Runtime<App>) -> Option<(f32, f32)> {
    rt.presented("plan").map(|(rect, _)| (rect[0], rect[1]))
}

/// A point of the window over something the plan draws, found by looking over the canvas.
fn over_something(rt: &Runtime<App>) -> Option<(f32, f32)> {
    let (ox, oy) = plan_origin(rt)?;
    let (w, h) = rt.state.size;
    (0..(h as u32))
        .step_by(6)
        .flat_map(|y| (0..(w as u32)).step_by(6).map(move |x| (x as f32, y as f32)))
        .find_map(|(x, y)| rt.state.pick(x, y).map(|_| (ox + x, oy + y)))
}

fn tap(rt: &mut Runtime<App>, at: (f32, f32)) {
    rt.pointer_down(at.0, at.1);
    rt.pointer_up(at.0, at.1);
    for _ in 0..2 {
        rt.frame(0.0);
    }
}

fn text_of(rt: &Runtime<App>, key: &str) -> Option<String> {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).and_then(|e| e.text)
}

#[test]
fn the_plan_is_laid_out_and_the_core_draws_it_for_the_size_it_got() {
    let rt = window();
    let (_, w) = rt.presented("plan").map(|(r, _)| ((r[0], r[1]), r[2])).unwrap_or_default();
    assert!(w > 600.0 && w < 1280.0, "the panel takes its share: the plan is {w} wide");
    assert!((rt.state.size.0 - w).abs() < 1.0, "the app knows the size the layout gave: {:?}", rt.state.size);
    assert!(rt.state.drawing.as_ref().is_some_and(|d| !d.prims.is_empty()));
}

#[test]
fn a_pointer_over_the_plan_hovers_what_is_under_it_and_the_status_bar_says_so() {
    let mut rt = window();
    let at = over_something(&rt).expect("something is drawn");
    rt.pointer_move(at.0, at.1);
    rt.frame(0.0);
    assert!(rt.state.hovered.is_some(), "hover picks");
    let line = text_of(&rt, "status-line").unwrap_or_default();
    assert!(line.contains(rt.state.level_name.as_str()) && line.contains(" mm"), "the bar says where: {line}");
}

#[test]
fn a_tap_selects_and_the_panel_lists_its_properties_and_delete_goes_through_the_engine() {
    let mut rt = window();
    let at = over_something(&rt).expect("something is drawn");
    tap(&mut rt, at);
    let id = *rt.state.selection.first().expect("a tap on a drawn element selects it");
    assert!(!rt.state.props.is_empty());
    assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("rows")), "the panel draws the rows");

    let button = rt.find("delete").expect("the Delete button");
    tap(&mut rt, button);
    assert!(rt.state.session.model.elements.get(id).is_none(), "the engine deleted it");
    assert!(rt.state.selection.is_empty());

    let undo = rt.find("undo").expect("the Undo button");
    tap(&mut rt, undo);
    assert!(rt.state.session.model.elements.get(id).is_some(), "undo gave it back");
}

#[test]
fn the_wheel_zooms_about_the_pointer() {
    let mut rt = window();
    let (ox, oy) = plan_origin(&rt).expect("the plan");
    let (x, y) = (ox + 300.0, oy + 200.0);
    let before = rt.state.camera.scale;
    rt.wheel_event(WheelEvent::new(0.0, 200.0).at(x, y));
    rt.frame(0.0);
    assert!(rt.state.camera.scale > before, "wheel forward zooms in: {before} -> {}", rt.state.camera.scale);
}

#[test]
fn the_buttons_are_buttons_to_a_screen_reader() {
    let rt = window();
    let buttons: Vec<_> = rt.elements().into_iter().filter(|e| e.role == "button").filter_map(|e| e.key).collect();
    for key in ["delete", "undo", "redo"] {
        assert!(buttons.iter().any(|k| k == key), "{key} is a button: {buttons:?}");
    }
}

#[test]
fn a_tool_is_picked_from_the_palette_and_the_status_bar_says_so() {
    let mut rt = window();
    let wall = rt.find("tool-wall").expect("the Wall tool in the palette");
    tap(&mut rt, wall);
    assert_eq!(rt.state.tool, Some("wall"));
    assert!(text_of(&rt, "status-line").unwrap_or_default().contains("Tool: Wall"));
    assert!(
        rt.elements().iter().any(|e| e.key.as_deref() == Some("tool-wall") && e.role == "button"),
        "a tool is a button to a screen reader"
    );
}
