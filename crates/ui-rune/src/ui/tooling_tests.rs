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

/// A wall of its own in the free canvas, between two canvas points, made through the core: its id. The walls of the house may be held
/// by dimensions, which refuse to move, so a test that moves one makes its own.
fn lone_wall(rt: &mut Runtime<App>, a: (f32, f32), b: (f32, f32)) -> u64 {
    let (w, h) = rt.state.size;
    let view = rt.state.camera.view(w, h);
    let (p, q) = (view.mm(a), view.mm(b));
    let round = |v: f64| (v / 10.0).round() * 10.0;
    let points = serde_json::json!([[round(p.0), round(p.1)], [round(q.0), round(q.1)]]);
    rt.update(move |s, _| {
        let level = s.level;
        drop(s.session.execute("wall", &serde_json::json!({"points": points, "level": level})));
        s.redraw();
    });
    rt.state.session.model.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id).max().expect("a wall")
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
    // An element that mounts later fades in, and a press ignores it until it has: the frames let the bar settle.
    settle(&mut rt);
    assert!(rt.find("draw-bar").is_some(), "the Wall tool takes a shape");
    let buttons: Vec<_> = rt.elements().into_iter().filter(|e| e.role == "button").filter_map(|e| e.key).collect();
    for key in ["shape-line", "shape-rectangle", "shape-circle", "shape-centrearc", "shape-tangentarc", "shape-fillet"] {
        assert!(buttons.iter().any(|k| k == key), "{key} is a button to a screen reader: {buttons:?}");
    }
    let rectangle = rt.find("shape-rectangle").expect("the Rectangle button");
    tap(&mut rt, rectangle);
    let picked = rt.element_at(rectangle.0, rectangle.1).map(|p| format!("{p:?}"));
    assert_eq!(rt.state.tooling.shape, Shape::Rectangle, "a press at {rectangle:?} on {picked:?}: {:?}", rt.presented("shape-rectangle"));
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

#[test]
fn a_modify_tool_of_the_ribbon_moves_the_selection_with_two_taps_on_the_plan() {
    let mut app = App::sample();
    app.profile = crate::profile::Profile::Rev;
    let mut rt = Runtime::new(app, view, (1280.0, 800.0), Env::new(true, 1.0));
    rt.on_key = Some(super::on_key);
    for _ in 0..3 {
        rt.frame(0.0);
    }
    rt.update(|s, _| s.zoom_by(0.25));
    let wall = lone_wall(&mut rt, (100.0, 400.0), (250.0, 400.0));
    let start = |rt: &Runtime<App>| rt.state.session.model.wall(wall).map(|w| w.start).expect("a wall");
    let before = start(&rt);
    rt.update(move |s, _| s.select(vec![wall]));
    frames(&mut rt);
    let tool = rt.find("tool-move").expect("Move in the ribbon");
    tap(&mut rt, tool);
    assert_eq!(rt.state.tool, Some("move"));
    tap_plan(&mut rt, 100.0, 500.0);
    tap_plan(&mut rt, 300.0, 500.0);
    assert!(
        start(&rt).dist(before) > 1000.0,
        "the wall moved by what two taps are apart: {:?} -> {:?}; {:?}, picked {:?}",
        before,
        start(&rt),
        rt.state.message,
        rt.state.picked
    );
    assert!(rt.state.tool.is_none(), "and Move is done with: {:?}", rt.state.message);
}

/// Frames enough for a menu to open and its animation to settle.
fn settle(rt: &mut Runtime<App>) {
    rt.frame(0.0);
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
}

#[test]
fn a_right_click_on_the_plan_opens_the_menu_at_the_pointer_and_an_item_does_its_work_and_closes_it() {
    let mut rt = window();
    let (x, y) = spot(&rt, 300.0, 400.0);
    assert!(rt.find("plan-menu-menu").is_none(), "closed, nothing is built");
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    settle(&mut rt);
    assert!(rt.state.tooling.menu.is_some(), "the right click opened it");
    let menu = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("plan-menu-menu")).expect("the menu is drawn");
    assert!(menu.rect[0] >= x - 1.0 && menu.rect[1] >= y - 1.0, "it opens at the pointer: {:?} for ({x}, {y})", menu.rect);
    // Nothing selected, no tool: Cancel, a separator, then Zoom to Fit, Zoom In and Zoom Out.
    let zoom_in = rt.find("plan-menu-item3").expect("Zoom In");
    let before = rt.state.camera.scale;
    tap(&mut rt, zoom_in);
    settle(&mut rt);
    assert!(rt.state.camera.scale > before, "Zoom In zoomed");
    assert!(rt.state.tooling.menu.is_none() && rt.find("plan-menu-menu").is_none(), "and the menu closed");
}

#[test]
fn escape_closes_the_menu_without_doing_anything() {
    let mut rt = window();
    rt.on_key = Some(super::on_key);
    let (x, y) = spot(&rt, 300.0, 400.0);
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    settle(&mut rt);
    assert!(rt.state.tooling.menu.is_some());
    let before = rt.state.camera;
    rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    settle(&mut rt);
    assert!(rt.state.tooling.menu.is_none(), "Escape closes it");
    assert_eq!(rt.state.camera, before);
}

#[test]
fn dragging_an_end_grip_of_the_selected_wall_moves_that_end_through_the_window() {
    let mut rt = window();
    let wall = lone_wall(&mut rt, (150.0, 560.0), (350.0, 560.0));
    rt.update(move |s, _| s.select(vec![wall]));
    settle(&mut rt);
    assert!(rt.find("plan-grips").is_some(), "the wall selected alone has its grips");
    let ends = |rt: &Runtime<App>| crate::grips::ends(&rt.state.session.model, wall).expect("a wall's ends");
    let (start, end) = ends(&rt);
    let (w, h) = rt.state.size;
    let (x, y) = rt.state.camera.view(w, h).px(end);
    let at = spot(&rt, x, y);
    rt.pointer_down(at.0, at.1);
    let to = (at.0 + 60.0, at.1 + 40.0);
    rt.pointer_move(to.0, to.1);
    frames(&mut rt);
    assert!(
        rt.state.gripping(),
        "the press on the end took the grip: at {at:?} for the end {end:?}, selection {:?}, shown {}, tool {:?}, cursor {:?}, picked {:?}, message {:?}",
        rt.state.selection,
        rt.state.grips_shown().is_some(),
        rt.state.tool,
        rt.state.cursor,
        rt.state.picked,
        rt.state.message
    );
    assert!(rt.find("plan-grip-tag").is_some(), "and its reading shows");
    assert!(rt.state.selection == vec![wall] && rt.state.rubber.is_none(), "it was no click and no box");
    rt.pointer_up(to.0, to.1);
    frames(&mut rt);
    assert!(!rt.state.gripping());
    let (after_start, after_end) = ends(&rt);
    assert!(after_end.dist(end) > 500.0 && after_start.dist(start) < 1.0, "only the dragged end moved: {after_end:?} from {end:?}");
}

#[test]
fn the_selected_wall_shows_its_temporary_dimension_and_a_tool_in_hand_takes_it_away() {
    let mut rt = Runtime::new(App::sample(), view, (1280.0, 800.0), Env::new(true, 1.0));
    for _ in 0..3 {
        rt.frame(0.0);
    }
    let wall = rt
        .state
        .session
        .model
        .elements
        .iter()
        .find_map(|(id, e)| matches!(e, Element::Wall(w) if w.arc == 0.0 && w.start.dist(w.end) > 3000.0).then_some(*id))
        .expect("a long straight wall");
    assert!(rt.find("plan-dims").is_none(), "nothing selected, no dimension");
    rt.update(|s, _| s.select(vec![wall]));
    settle(&mut rt);
    assert!(rt.find("plan-dims").is_some() && rt.find("plan-dim-0").is_some(), "the wall's length has a line and a number to type into");
    rt.update(|s, _| s.set_tool("beam"));
    settle(&mut rt);
    assert!(rt.find("plan-dims").is_none() && rt.find("plan-dim-0").is_none(), "a tool in hand takes them away");
}
