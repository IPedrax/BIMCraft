//! The tools through the window, headless: Rune's own `Runtime` on the app's real state and view, taps on the plan, the keys and the
//! Draw panel's shape bar. The plan is zoomed out first, so there is free canvas round the house where the points can go.

use bimcraft_core::draw::Shape;
use bimcraft_core::model::Element;
use rune::{Env, Key, KeyEvent, Mods, Runtime};

use super::view;
use crate::app::App;

fn window() -> Runtime<App> {
    let mut rt = Runtime::new(App::sample(), view, (1280.0, 800.0), Env::new(true, 1.0));
    rt.on_key = Some(super::on_key);
    rt.poll = Some(super::poll);
    for _ in 0..3 {
        rt.frame(0.0);
    }
    rt.update(|s, _| s.zoom_by(0.25));
    for _ in 0..2 {
        rt.frame(0.0);
    }
    rt
}

fn frames(rt: &mut Runtime<App>) {
    for _ in 0..2 {
        rt.frame(0.0);
    }
}

/// A point of the plan canvas as a point of the window.
fn spot(rt: &Runtime<App>, x: f32, y: f32) -> (f32, f32) {
    let (ox, oy) = rt.presented("plan").map_or((0.0, 0.0), |(r, _)| (r[0], r[1]));
    (ox + x, oy + y)
}

fn tap(rt: &mut Runtime<App>, at: (f32, f32)) {
    rt.pointer_down(at.0, at.1);
    rt.pointer_up(at.0, at.1);
    frames(rt);
}

fn tap_plan(rt: &mut Runtime<App>, x: f32, y: f32) {
    let at = spot(rt, x, y);
    tap(rt, at);
}

fn key(rt: &mut Runtime<App>, key: Key) {
    rt.key(KeyEvent { key, mods: Mods::default() });
    frames(rt);
}

fn walls(rt: &Runtime<App>) -> usize {
    rt.state.session.model.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).count()
}

#[test]
fn enter_finishes_a_path_and_backspace_gives_a_point_back_through_the_window() {
    let mut rt = window();
    rt.update(|s, _| s.set_tool("railing"));
    frames(&mut rt);
    let before = rt.state.session.model.elements.len();
    for (x, y) in [(100.0, 560.0), (300.0, 560.0), (300.0, 640.0)] {
        tap_plan(&mut rt, x, y);
    }
    assert_eq!(rt.state.picked.len(), 3, "the plan's taps are the tool's points");
    key(&mut rt, Key::Backspace);
    assert_eq!(rt.state.picked.len(), 2, "Backspace gives the last one back");
    tap_plan(&mut rt, 300.0, 640.0);
    key(&mut rt, Key::Enter);
    assert!(rt.state.picked.is_empty(), "Enter finishes the path: {:?}", rt.state.message);
    assert_eq!(rt.state.session.model.elements.len(), before + 1, "and the core made the railing");
    key(&mut rt, Key::Enter);
    assert_eq!(rt.state.session.model.elements.len(), before + 1, "Enter with nothing picked does nothing");
}

#[test]
fn the_wall_tool_has_a_shape_bar_that_picks_a_shape_and_shows_what_the_next_click_makes() {
    let mut rt = window();
    assert!(rt.find("draw-bar").is_none() && rt.find("plan-ghost").is_none(), "with no tool there is no bar");
    rt.update(|s, _| s.set_tool("wall"));
    frames(&mut rt);
    assert!(rt.find("draw-bar").is_some(), "the Wall tool takes a shape");
    let buttons: Vec<_> = rt.elements().into_iter().filter(|e| e.role == "button").filter_map(|e| e.key).collect();
    for key in ["shape-line", "shape-rectangle", "shape-circle", "shape-centrearc"] {
        assert!(buttons.iter().any(|k| k == key), "{key} is a button to a screen reader: {buttons:?}");
    }
    let rectangle = rt.find("shape-rectangle").expect("the Rectangle button");
    tap(&mut rt, rectangle);
    assert_eq!(rt.state.tooling.shape, Shape::Rectangle);
    let before = walls(&rt);
    tap_plan(&mut rt, 150.0, 560.0);
    let at = spot(&rt, 450.0, 640.0);
    rt.pointer_move(at.0, at.1);
    frames(&mut rt);
    assert!(rt.find("plan-ghost").is_some(), "the rectangle the next click makes is shown");
    tap(&mut rt, at);
    assert_eq!(walls(&rt), before + 4, "{:?}", rt.state.message);
    assert!(rt.find("plan-ghost").is_none(), "and the ghost goes with it");
    rt.update(|s, _| s.set_tool("beam"));
    frames(&mut rt);
    assert!(rt.find("draw-bar").is_none(), "a beam takes no shape: the bar goes");
}
