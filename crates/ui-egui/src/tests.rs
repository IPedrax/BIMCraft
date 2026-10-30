//! The app driven the way a user drives it: clicks on the plan at model points, the ribbon and
//! the browser by their labels, keys. Each test then reads the model the session ended up with,
//! or what the window shows.

use crate::DimMode;
use bimcraft_core::element_json;
use bimcraft_core::engine::Session;
use bimcraft_core::geom::P2;
use bimcraft_core::model::{DimKind, Element, Guid, Id};
use bimcraft_core::pen::Pen;
use bimcraft_core::sample::SAMPLE;
use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, TouchPhase, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::{BimApp, Sheet, Tab, Tool};

const BOX: &str = r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}"#;

fn app(script: &str) -> Harness<'static, BimApp> {
    app_at(script, 0.25)
}

/// The app with frames `step` seconds apart. Each queued event runs a frame, so a double click
/// (two clicks within 300 ms) needs short ones.
fn app_at(script: &str, step: f32) -> Harness<'static, BimApp> {
    let mut s = Session::new();
    s.script(script).unwrap();
    let mut h = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_max_steps(32)
        .with_step_dt(step)
        .build_ui_state(|ui, app: &mut BimApp| app.show(ui), BimApp::new(s));
    h.run();
    h
}

fn press(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_label(label).click();
    h.run();
}

/// A tool from the Architecture tab: a tool's contextual tab takes the ribbon, as in Revit.
fn tool(h: &mut Harness<BimApp>, label: &str) {
    press(h, "Architecture");
    press(h, label);
}

/// Replace a text field's contents, as a user selecting all and typing does.
fn type_into(h: &mut Harness<BimApp>, label: &str, text: &str) {
    h.get_by_label(label).focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by_label(label).type_text(text);
    h.run();
}

/// Of the widgets named `name`, a view tab, else the one highest up: a view tab over the browser's entry and over what a view
/// under the tabs draws, a ribbon tab over the tool of the same name.
fn topmost(h: &mut Harness<BimApp>, name: &str) {
    let tabs: Vec<egui::Rect> = h.state().tab_rects.iter().map(|(_, r)| *r).collect();
    let tab = h.get_all_by_label(name).map(|n| n.rect()).find(|r| tabs.iter().any(|t| t.contains(r.center())));
    let top = tab.map(|r| r.center());
    let top = top.or_else(|| h.get_all_by_label(name).min_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).map(|n| n.rect().center()));
    let pos = top.unwrap();
    h.hover_at(pos);
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    h.run();
}

/// A tool from its panel's list (the panel's name, `Build ▾`), which has it whether the panel is folded to fit the window or not:
/// the first row of that name under the list's button, not the ribbon's own nor one further down the window.
fn from_list(h: &mut Harness<BimApp>, panel: &str, tool: &str) {
    let name = format!("{panel} ▾");
    press(h, &name);
    let under = h.get_by_label(&name).rect().bottom();
    let row = h
        .get_all_by_label(tool)
        .map(|n| n.rect())
        .filter(|r| r.top() >= under)
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .map(|r| r.center());
    click_at(h, row.unwrap());
}

/// The ribbon's tab scrolled to its right end with the wheel, for a tab wider than the window.
fn scroll_ribbon_right(h: &mut Harness<BimApp>) {
    h.hover_at(pos2(400.0, 110.0));
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(-3000.0, 0.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.run();
}

/// A button named `name` beside the widget `beside`, when another window has one by that name too: the nearest.
fn press_near(h: &mut Harness<BimApp>, name: &str, beside: &str) {
    let from = h.get_by_label(beside).rect().center();
    let pos = h.get_all_by_label(name).map(|n| n.rect().center()).min_by(|a, b| a.distance(from).total_cmp(&b.distance(from)));
    click_at(h, pos.unwrap());
}

/// A view in the Project Browser, opened as screen readers do: it may lie below the browser's fold.
fn browse(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_label(label).click_accesskit();
    h.run();
}

/// A view's entry in the Project Browser while its tab is open too: the lower of the two.
fn browse_entry(h: &mut Harness<BimApp>, label: &str) {
    use egui_kittest::kittest::NodeT;
    let entry = h.get_all_by_label(label).max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).map(|n| n.accesskit_node().id());
    h.get_by(|n| Some(n.id()) == entry).click_accesskit();
    h.run();
}

/// A primary click on the plan at a model point.
fn click(h: &mut Harness<BimApp>, p: P2) {
    click_with(h, p, Modifiers::NONE);
}

/// A click holding modifier keys (Ctrl adds to the selection, Shift takes away).
fn click_with(h: &mut Harness<BimApp>, p: P2, modifiers: Modifiers) {
    let pos = h.state().screen(p);
    h.hover_at(pos);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers });
    }
    h.run();
}

/// A primary click at a point on screen.
fn click_at(h: &mut Harness<BimApp>, pos: Pos2) {
    h.hover_at(pos);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    h.run();
}

/// Two primary clicks at a model point, a frame apart: a double click, in a harness with short
/// frames ([`app_at`]).
fn double_click(h: &mut Harness<BimApp>, p: P2) {
    // Let the last clicks age first: a third click within 600 ms of two others is a triple.
    h.run_steps(15);
    let pos = h.state().screen(p);
    h.hover_at(pos);
    h.run();
    for pressed in [true, false, true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    h.run();
}

/// The primary button going down or up at `pos`.
fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

/// What a window hands the app for one frame, then the frames the app asks for to take what it held
/// back (`BimApp::pace`, as `raw_input_hook` runs it): a hand moves on between one frame and the next.
fn deliver(h: &mut Harness<BimApp>, mut events: Vec<Event>) {
    loop {
        let more = h.state_mut().pace(&mut events);
        h.input_mut().events.append(&mut events);
        h.step();
        if !more {
            break;
        }
    }
}

/// A press at `a`, a move halfway and a release at `b`, in three frames: a click made on the move
/// (a quick one in a harness with short frames, [`app_at`]).
fn flick(h: &mut Harness<BimApp>, a: Pos2, b: Pos2, modifiers: Modifiers) {
    let at = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers };
    deliver(h, vec![Event::PointerMoved(a), at(a, true)]);
    deliver(h, vec![Event::PointerMoved(a + (b - a) * 0.5)]);
    deliver(h, vec![Event::PointerMoved(b), at(b, false)]);
}

/// A drag with `button` between points on screen, in steps.
fn drag_screen(h: &mut Harness<BimApp>, a: Pos2, b: Pos2, button: PointerButton) {
    h.hover_at(a);
    h.run();
    h.event(Event::PointerButton { pos: a, button, pressed: true, modifiers: Modifiers::NONE });
    h.run();
    for t in [0.25, 0.5, 0.75, 1.0] {
        h.hover_at(a + (b - a) * t);
        h.run();
    }
    h.event(Event::PointerButton { pos: b, button, pressed: false, modifiers: Modifiers::NONE });
    h.run();
}

/// A primary drag on the plan between model points, in steps, as a hand makes it.
fn drag(h: &mut Harness<BimApp>, from: P2, to: P2) {
    let (a, b) = (h.state().screen(from), h.state().screen(to));
    h.hover_at(a);
    h.run();
    h.event(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run();
    for t in [0.25, 0.5, 0.75, 1.0] {
        h.hover_at(a + (b - a) * t);
        h.run();
    }
    h.event(Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run();
}

/// The id of the element `f` picks out.
/// Element `id`'s GUID: what views and other elements hold it by.
fn gid(h: &Harness<BimApp>, id: Id) -> Guid {
    h.state().session.model.guid(id)
}

fn find(h: &Harness<BimApp>, f: impl Fn(&Element) -> bool) -> Id {
    h.state().session.model.elements.iter().find(|(_, e)| f(e)).map(|(id, _)| *id).unwrap()
}

/// A click on the widget of `role` named `label`: of those named alike, the one that role picks out.
fn pick(h: &mut Harness<BimApp>, role: Role, label: &str) {
    // Alphabetical VG rows can start outside the table's viewport; scroll as a user would before clicking.
    if h.state().vg_dialog.is_some() {
        h.get_by_role_and_label(role, label).scroll_to_me();
        h.run();
    }
    h.get_by_role_and_label(role, label).click();
    h.run();
}

/// Replace the text of the field of `role` named `label`, and press Enter.
fn type_role(h: &mut Harness<BimApp>, role: Role, label: &str, text: &str) {
    h.get_by_role_and_label(role, label).focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.key_press(Key::Backspace);
    h.get_by_role_and_label(role, label).type_text(text);
    h.run();
    key(h, Key::Enter);
}

fn key(h: &mut Harness<BimApp>, k: Key) {
    h.key_press(k);
    h.run();
}

/// Letters typed over the view: a two-letter shortcut.
fn shortcut(h: &mut Harness<BimApp>, letters: &str) {
    h.event(Event::Text(letters.into()));
    h.run();
}

fn ids(h: &Harness<BimApp>, category: &str) -> Vec<Id> {
    h.state().session.model.elements.iter().filter(|(_, e)| e.category() == category).map(|(id, _)| *id).collect()
}

fn walls(h: &Harness<BimApp>) -> Vec<(P2, P2)> {
    h.state()
        .session
        .model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Wall(w) => Some((w.start, w.end)),
            _ => None,
        })
        .collect()
}

/// Element `id`'s `key`, as its JSON has it; what it points at, as the id of that element.
fn field(h: &Harness<BimApp>, id: Id, key: &str) -> serde_json::Value {
    let m = &h.state().session.model;
    let v = element_json(m, id)[key].clone();
    match serde_json::from_value::<Guid>(v.clone()) {
        Ok(g) if v.is_string() => m.elements.id(g).map_or(v, |id| json!(id)),
        _ => v,
    }
}

#[test]
fn clicking_the_plan_selects_the_element_drawn_nearest_the_cursor() {
    let mut h = app(SAMPLE);
    let door = find(&h, |e| matches!(e, Element::Door(o) if o.mark == "1"));
    click(&mut h, P2(2000.0, 0.0));
    assert_eq!(h.state().selection, [door]);
    assert!(h.query_by_label("Flip Facing").is_some(), "the palette shows the door's properties");
    assert!(h.query_by_label("Modify | Doors").is_some(), "the selection's contextual tab");
    click(&mut h, P2(3000.0, 1000.0));
    assert!(h.state().selection.is_empty(), "empty floor selects nothing");
}

#[test]
fn each_click_of_the_wall_tool_builds_a_wall_and_the_next_starts_where_it_ends() {
    let mut h = app("");
    tool(&mut h, "Wall");
    click(&mut h, P2(-3000.0, -3000.0));
    assert!(walls(&h).is_empty(), "one point is no wall");
    click(&mut h, P2(3000.0, -3000.0));
    assert_eq!(walls(&h), [(P2(-3000.0, -3000.0), P2(3000.0, -3000.0))], "the second click builds it");
    click(&mut h, P2(3000.0, 2000.0));
    assert_eq!(walls(&h), [(P2(-3000.0, -3000.0), P2(3000.0, -3000.0)), (P2(3000.0, -3000.0), P2(3000.0, 2000.0))]);
    assert_eq!(h.state().session.history(), ["Wall", "Wall"], "each wall its own undo step");
    key(&mut h, Key::Enter);
    assert_eq!(walls(&h).len(), 2, "Enter builds nothing more");
    assert!(matches!(&h.state().tool, Tool::Wall(pts) if pts.is_empty()), "it ends the chain; the tool stays on");
}

#[test]
fn escape_ends_the_chain_keeping_its_walls_and_clicking_its_start_closes_it() {
    let mut h = app("");
    tool(&mut h, "Wall");
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(4000.0, 0.0));
    key(&mut h, Key::Escape);
    assert_eq!(walls(&h), [(P2(0.0, 0.0), P2(4000.0, 0.0))], "what was built stays");
    assert!(matches!(&h.state().tool, Tool::Wall(pts) if pts.is_empty()));
    for p in [P2(0.0, 0.0), P2(0.0, 3000.0), P2(4000.0, 3000.0), P2(4000.0, 0.0), P2(0.0, 0.0)] {
        click(&mut h, p);
    }
    assert_eq!(walls(&h).len(), 5);
    assert!(matches!(&h.state().tool, Tool::Wall(pts) if pts.is_empty()), "back on its start, the chain closes");
    key(&mut h, Key::Escape);
    assert!(matches!(h.state().tool, Tool::Modify), "Escape with no chain leaves the tool");
}

#[test]
fn pace_cuts_the_events_of_a_frame_after_each_button_and_holds_the_rest_in_order() {
    let mut app = BimApp::new(Session::new());
    let (a, b) = (pos2(1.0, 1.0), pos2(9.0, 9.0));
    let mut events = vec![Event::PointerMoved(a), button(a, true), Event::PointerMoved(b), button(b, false), Event::PointerMoved(a)];
    assert!(app.pace(&mut events), "the rest is held");
    assert_eq!(events, [Event::PointerMoved(a), button(a, true)]);
    // What arrives meanwhile waits behind what is held.
    events = vec![Event::Text("w".into())];
    assert!(app.pace(&mut events));
    assert_eq!(events, [Event::PointerMoved(b), button(b, false)]);
    events = Vec::new();
    assert!(!app.pace(&mut events), "nothing left over");
    assert_eq!(events, [Event::PointerMoved(a), Event::Text("w".into())]);
}

#[test]
fn a_click_is_made_where_the_button_went_down_not_where_the_hand_had_got_to_by_the_frames_end() {
    let mut h = app("");
    tool(&mut h, "Wall");
    let (here, gone) = (h.state().screen(P2(1000.0, 1000.0)), h.state().screen(P2(5000.0, 3000.0)));
    // The button goes down and up at one spot; the hand is far off when the window gets round to the frame.
    deliver(&mut h, vec![Event::PointerMoved(here), button(here, true), button(here, false), Event::PointerMoved(gone)]);
    assert_eq!(h.state().tool, Tool::Wall(vec![P2(1000.0, 1000.0)]));
}

#[test]
fn a_press_belongs_to_what_lay_under_it_though_the_hand_has_left_it_by_the_frames_end() {
    let mut h = app("");
    tool(&mut h, "Wall");
    let (here, off) = (h.state().screen(P2(1000.0, 1000.0)), pos2(h.state().screen(P2(1000.0, 1000.0)).x, 2.0));
    // The hand goes up onto the ribbon in the breath after the button went down on the plan.
    deliver(&mut h, vec![Event::PointerMoved(here), button(here, true), Event::PointerMoved(off), button(off, false)]);
    assert_eq!(h.state().tool, Tool::Wall(vec![P2(1000.0, 1000.0)]));
}

#[test]
fn two_clicks_that_reach_the_app_in_one_frame_are_two_clicks() {
    let mut h = app("");
    tool(&mut h, "Wall");
    let (a, b) = (h.state().screen(P2(1000.0, 1000.0)), h.state().screen(P2(5000.0, 1000.0)));
    let clicks = [Event::PointerMoved(a), button(a, true), button(a, false), Event::PointerMoved(b), button(b, true), button(b, false)];
    deliver(&mut h, clicks.to_vec());
    assert_eq!(h.state().tool, Tool::Wall(vec![P2(1000.0, 1000.0), P2(5000.0, 1000.0)]));
}

#[test]
fn a_tool_takes_the_point_where_the_button_went_down_though_the_hand_travelled_before_it_came_up() {
    let mut h = app("");
    tool(&mut h, "Wall");
    // Fast drawing clicks on the move; egui calls a press and release more than six points apart a
    // drag, which a drawing tool would drop.
    drag(&mut h, P2(1000.0, 1000.0), P2(3000.0, 1000.0));
    assert_eq!(h.state().tool, Tool::Wall(vec![P2(1000.0, 1000.0)]));
    drag(&mut h, P2(5000.0, 1000.0), P2(5000.0, 3000.0));
    assert_eq!(h.state().tool, Tool::Wall(vec![P2(1000.0, 1000.0), P2(5000.0, 1000.0)]));
}

#[test]
fn a_tool_acts_where_the_button_went_down_in_sections_and_on_sheets_too() {
    let mut h = app_at(&format!("{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}\nsheet {{}}"), 0.05);
    press(&mut h, "Annotate");
    press(&mut h, "Text");
    type_into(&mut h, "Text:", "Tiled floor");
    browse(&mut h, "Section 1");
    drag(&mut h, P2(3000.0, 2500.0), P2(5000.0, 2500.0));
    h.get_by_label("A101 - Unnamed").click_accesskit();
    h.run();
    drag(&mut h, P2(50.0, 50.0), P2(120.0, 50.0));
    let notes = ids(&h, "text");
    assert_eq!(notes.len(), 2, "a note in the section and one on the sheet");
    assert_eq!((field(&h, notes[0], "at"), field(&h, notes[1], "at")), (json!([3000.0, 2500.0]), json!([50.0, 50.0])));
}

#[test]
fn a_click_selects_what_lay_under_the_button_though_the_hand_has_moved_on() {
    let mut h = app(SAMPLE);
    let door = find(&h, |e| matches!(e, Element::Door(o) if o.mark == "1"));
    let (on, off) = (h.state().screen(P2(2000.0, 0.0)), h.state().screen(P2(3000.0, 1000.0)));
    deliver(&mut h, vec![Event::PointerMoved(on), button(on, true), button(on, false), Event::PointerMoved(off)]);
    assert_eq!(h.state().selection, [door]);
}

#[test]
fn a_drag_lets_go_where_the_button_came_up_not_where_the_hand_had_got_to_by_the_frames_end() {
    let mut h = app(BOX);
    click(&mut h, P2(2000.0, 0.0));
    // The south wall's east end, dragged south, with the hand carrying on past where it let go.
    let (grip, half, to, gone) = (
        h.state().screen(P2(8000.0, 0.0)),
        h.state().screen(P2(8000.0, -500.0)),
        h.state().screen(P2(8000.0, -1000.0)),
        h.state().screen(P2(8000.0, -3000.0)),
    );
    deliver(&mut h, vec![Event::PointerMoved(grip), button(grip, true)]);
    deliver(&mut h, vec![Event::PointerMoved(half)]);
    deliver(&mut h, vec![Event::PointerMoved(to), button(to, false), Event::PointerMoved(gone)]);
    assert_eq!(walls(&h)[0], (P2(0.0, 0.0), P2(8000.0, -1000.0)));
}

#[test]
fn a_press_on_the_ribbon_is_no_click_on_the_plan() {
    let mut h = app("");
    tool(&mut h, "Wall");
    press(&mut h, "Annotate");
    assert_eq!(h.state().tool, Tool::Wall(Vec::new()), "the tab took the press: no point went down under it");
}

#[test]
fn the_app_asks_for_a_frame_while_it_holds_events_back() {
    // Else a click's release would sit held until the next event came.
    let mut h = app("");
    h.step();
    assert!(!h.ctx.has_requested_repaint(), "an idle frame asks for none");
    h.state_mut().held.push(Event::WindowFocused(true));
    h.step();
    assert!(h.ctx.has_requested_repaint(), "the held event needs its frame");
}

const COLUMNS: &str = "column {\"at\": [2000, 3000]}\ncolumn {\"at\": [6000, 3000]}";

#[test]
fn a_click_made_on_the_move_selects_in_modify_and_moves_nothing() {
    let mut h = app_at(COLUMNS, 0.05);
    let a = ids(&h, "column")[0];
    let before = Arc::clone(&h.state().session.model);
    let on = h.state().screen(P2(2000.0, 3000.0));
    flick(&mut h, on, on + vec2(90.0, 50.0), Modifiers::NONE);
    assert_eq!(h.state().selection, [a]);
    assert!(Arc::ptr_eq(&before, &h.state().session.model), "no command ran: nothing moved");
}

#[test]
fn a_slower_drag_in_modify_still_moves_what_it_began_on() {
    let mut h = app_at(COLUMNS, 0.05);
    let a = ids(&h, "column")[0];
    let (from, to) = (h.state().screen(P2(2000.0, 3000.0)), h.state().screen(P2(3000.0, 3000.0)));
    // Ten frames of 50 ms from the press to the release: the hand means to move it.
    deliver(&mut h, vec![Event::PointerMoved(from), button(from, true)]);
    for k in 1..=8 {
        deliver(&mut h, vec![Event::PointerMoved(from + (to - from) * (k as f32 / 8.0))]);
    }
    deliver(&mut h, vec![button(to, false)]);
    assert_eq!(field(&h, a, "at"), json!([3000.0, 3000.0]));
    assert_eq!(h.state().session.history().last(), Some(&"Move"));
}

#[test]
fn ctrl_and_a_click_made_on_the_move_adds_to_the_selection_it_found() {
    let mut h = app_at(COLUMNS, 0.05);
    let cols = ids(&h, "column");
    click(&mut h, P2(2000.0, 3000.0));
    assert_eq!(h.state().selection, [cols[0]]);
    let before = Arc::clone(&h.state().session.model);
    let on = h.state().screen(P2(6000.0, 3000.0));
    flick(&mut h, on, on + vec2(90.0, 50.0), Modifiers::COMMAND);
    assert_eq!(h.state().selection, [cols[0], cols[1]], "the first stays selected, the second joins it");
    assert!(Arc::ptr_eq(&before, &h.state().session.model), "nothing moved");
}

#[test]
fn a_quick_flick_from_empty_floor_is_a_click_there_not_a_selection_box() {
    // Two walls give the plan room for the box, which lies between them.
    let mut h = app_at(&format!("{COLUMNS}\nwall {{\"points\": [[0,0],[8000,0]]}}\nwall {{\"points\": [[0,6000],[8000,6000]]}}"), 0.05);
    let cols = ids(&h, "column");
    click(&mut h, P2(2000.0, 3000.0));
    assert_eq!(h.state().selection, [cols[0]]);
    // A window across both columns, if the hand meant one; a click on empty floor if it did not.
    let (from, to) = (h.state().screen(P2(1000.0, 4500.0)), h.state().screen(P2(7000.0, 1500.0)));
    flick(&mut h, from, to, Modifiers::NONE);
    assert!(h.state().selection.is_empty(), "{:?}", h.state().selection);
}

#[test]
fn a_flick_from_a_walls_end_grip_leaves_the_wall_as_it_was() {
    let mut h = app_at(r#"wall {"points": [[0,0],[8000,0]]}"#, 0.05);
    let wall = ids(&h, "wall")[0];
    click(&mut h, P2(4000.0, 0.0));
    let before = Arc::clone(&h.state().session.model);
    let (grip, off) = (h.state().screen(P2(8000.0, 0.0)), h.state().screen(P2(8000.0, -1500.0)));
    flick(&mut h, grip, off, Modifiers::NONE);
    assert!(Arc::ptr_eq(&before, &h.state().session.model), "the end did not move");
    assert_eq!(h.state().selection, [wall]);
}

#[test]
fn a_flick_from_a_crop_regions_grip_leaves_the_crop_as_it_was() {
    let mut h = app_at(BOX, 0.05);
    press(&mut h, "View");
    h.get_by_role_and_label(Role::Button, "Crop View").click();
    h.run();
    let crop = field(&h, 100000, "crop");
    let (lo, hi) = (&crop[0], &crop[1]);
    let east = P2(hi[0].as_f64().unwrap(), (lo[1].as_f64().unwrap() + hi[1].as_f64().unwrap()) / 2.0);
    let grip = h.state().screen(east);
    flick(&mut h, grip, grip - vec2(120.0, 0.0), Modifiers::NONE);
    assert_eq!(field(&h, 100000, "crop"), crop);
}

#[test]
fn a_flick_on_a_sheets_view_selects_it_and_does_not_move_it() {
    let mut h = app_at(BOX, 0.05);
    press(&mut h, "View");
    press(&mut h, "Sheet");
    press(&mut h, "View");
    press(&mut h, "Place View");
    press(&mut h, "Floor Plan: Level 1");
    let vp = ids(&h, "viewport")[0];
    let at = field(&h, vp, "at");
    let (x, y) = (at[0].as_f64().unwrap(), at[1].as_f64().unwrap());
    key(&mut h, Key::Escape);
    let on = h.state().screen(P2(x, y));
    flick(&mut h, on, on + vec2(60.0, 0.0), Modifiers::NONE);
    assert_eq!(field(&h, vp, "at"), json!([x, y]));
    assert_eq!(h.state().selection, [vp]);
}

#[test]
fn a_tool_acts_on_what_lay_under_the_press_not_under_where_the_hand_let_go() {
    let mut h = app_at(BOX, 0.05);
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 0.0)));
    shortcut(&mut h, "DW");
    assert_eq!(h.state().tool, Tool::Demolish);
    // Down on the south wall, up on empty floor inside the box.
    drag(&mut h, P2(4000.0, 0.0), P2(4000.0, 3000.0));
    assert_eq!(demolished(&h, wall), Some(json!("New Construction")));
}

#[test]
fn doors_and_windows_go_into_the_wall_clicked() {
    let mut h = app(BOX);
    let bottom = ids(&h, "wall")[0];
    tool(&mut h, "Door");
    click(&mut h, P2(2000.0, 40.0));
    let door = ids(&h, "door");
    assert_eq!(door.len(), 1);
    assert_eq!((field(&h, door[0], "host"), field(&h, door[0], "offset")), (json!(bottom), json!(2000.0)));
    tool(&mut h, "Window");
    click(&mut h, P2(4000.0, 3000.0));
    assert!(ids(&h, "window").is_empty());
    assert!(h.state().status.error && h.state().status.text.contains("no wall"), "{:?}", h.state().status.text);
    click(&mut h, P2(8000.0, 3000.0));
    assert_eq!(ids(&h, "window").len(), 1);
}

#[test]
fn rooms_go_where_clicked_and_their_area_follows_the_walls() {
    let mut h = app(BOX);
    tool(&mut h, "Room");
    click(&mut h, P2(2000.0, 3000.0));
    let room = ids(&h, "room")[0];
    assert_eq!(field(&h, room, "area_m2"), json!(48.0));
    // A partition clicked a little off the walls' centre lines, away from their middles: its ends
    // snap onto them, so it meets them and splits the room.
    tool(&mut h, "Wall");
    click(&mut h, P2(2040.0, 30.0));
    click(&mut h, P2(2040.0, 5970.0));
    key(&mut h, Key::Enter);
    assert_eq!(walls(&h).last(), Some(&(P2(2040.0, 0.0), P2(2040.0, 6000.0))));
    assert_eq!(field(&h, room, "area_m2"), json!(12.24));
}

#[test]
fn properties_edit_through_modify_and_undo_puts_them_back() {
    let mut h = app(SAMPLE);
    click(&mut h, P2(3000.0, 2000.0));
    let room = h.state().selection[0];
    assert_eq!(field(&h, room, "name"), json!("Living"));
    type_into(&mut h, "Name", "Lounge");
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, room, "name"), json!("Lounge"));
    assert_eq!(h.state().session.history().last(), Some(&"Modify"));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(field(&h, room, "name"), json!("Living"));
    press(&mut h, "Redo");
    assert_eq!(field(&h, room, "name"), json!("Lounge"));
}

#[test]
fn the_wheel_zooms_about_the_cursor_and_the_middle_button_pans() {
    let mut h = app(SAMPLE);
    let (p, q) = (P2(8000.0, 3500.0), P2(0.0, 0.0));
    let at = h.state().screen(p);
    let span = (h.state().screen(q) - at).length();
    h.hover_at(at);
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, 120.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.run();
    assert!((h.state().screen(p) - at).length() < 1.0, "the point under the cursor stays put");
    assert!((h.state().screen(q) - at).length() > span * 1.2, "zoomed in");
    let to = at + vec2(100.0, 50.0);
    h.event(Event::PointerButton { pos: at, button: PointerButton::Middle, pressed: true, modifiers: Modifiers::NONE });
    h.run();
    h.hover_at(to);
    h.run();
    h.event(Event::PointerButton { pos: to, button: PointerButton::Middle, pressed: false, modifiers: Modifiers::NONE });
    h.run();
    assert!((h.state().screen(p) - to).length() < 1.0, "the plan followed the drag");
    // Hostile wheels: the view stays usable.
    for dy in [1e30, -1e30] {
        h.event(Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, dy),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        h.run();
        let s = h.state().screen(p);
        assert!(s.x.is_finite() && s.y.is_finite());
    }
}

#[test]
fn the_browser_opens_level_plans_and_schedules() {
    let mut h = app(SAMPLE);
    for label in ["Views (all)", "Floor Plans", "Schedules/Quantities (all)"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    // Its floor plan, over its ceiling plan.
    topmost(&mut h, "Level 2");
    assert_eq!(h.state().sheet, Sheet::Plan(100001));
    // Below the browser's fold, past the elevations and sections: reached as screen readers do.
    h.get_by_label("Room Schedule").click_accesskit();
    h.run();
    assert_eq!(h.state().sheet, Sheet::Schedule("room"));
    assert!(h.query_by_label("Kitchen").is_some(), "a row of the room schedule");
}

#[test]
fn the_selection_stays_as_views_change_and_a_schedule_shows_and_picks_it() {
    let mut h = app(&format!(
        "{BOX}\nwall {{\"points\": [[4000,0],[4000,6000]]}}\nroom {{\"point\": [2000, 3000], \"name\": \"Hall\"}}\nroom {{\"point\": [6000, 3000], \"name\": \"Den\"}}"
    ));
    let hall = find(&h, |e| matches!(e, Element::Room(r) if r.name == "Hall"));
    let den = find(&h, |e| matches!(e, Element::Room(r) if r.name == "Den"));
    click(&mut h, P2(2000.0, 3000.0));
    assert_eq!(h.state().selection, [hall]);
    press(&mut h, "Default 3D View");
    assert_eq!(h.state().selection, [hall], "kept in the 3D view");
    browse(&mut h, "Room Schedule");
    assert_eq!(h.state().sheet, Sheet::Schedule("room"));
    assert_eq!(h.state().selection, [hall], "kept in a schedule");
    use egui_kittest::kittest::NodeT;
    let selected = |h: &Harness<BimApp>, name: &str| h.get_by_label(name).accesskit_node().toggled();
    use egui::accesskit::Toggled;
    assert_eq!((selected(&h, "Hall"), selected(&h, "Den")), (Some(Toggled::True), Some(Toggled::False)), "its row is highlighted");
    // A click on a row picks what it lists; Ctrl adds.
    h.get_by_label("Den").click();
    h.run();
    assert_eq!(h.state().selection, [den]);
    h.get_by_label("Hall").click_modifiers(Modifiers::COMMAND);
    h.run();
    assert_eq!(h.state().selection, [den, hall]);
    topmost(&mut h, "Level 1");
    assert_eq!(h.state().sheet, Sheet::Plan(100000));
    assert_eq!(h.state().selection, [den, hall], "and back in the plan");
}

#[test]
fn typed_shortcuts_pick_tools_like_revit() {
    let mut h = app(SAMPLE);
    h.event(Event::Text("W".into()));
    h.event(Event::Text("A".into()));
    h.run();
    assert!(matches!(h.state().tool, Tool::Wall(_)));
    h.event(Event::Text("dr".into()));
    h.run();
    assert!(matches!(h.state().tool, Tool::Door));
    h.event(Event::Text("DI".into()));
    h.run();
    assert!(matches!(h.state().tool, Tool::Dimension(..)));
    h.event(Event::Text("tg".into()));
    h.run();
    assert!(matches!(h.state().tool, Tool::Tag));
    shortcut(&mut h, "CR");
    assert!(matches!(h.state().tool, Tool::Section(None)));
    shortcut(&mut h, "DW");
    assert!(matches!(h.state().tool, Tool::Demolish));
    key(&mut h, Key::Escape);
    assert!(matches!(h.state().tool, Tool::Modify));
}

#[test]
fn create_similar_starts_the_selected_elements_tool_placing_its_type() {
    let mut h = app(&format!(
        "{BOX}\ndoor {{\"at\": [2000, 0], \"type\": \"Single 800 x 2100\"}}\n\
         component {{\"family\": \"Table\", \"type\": \"1200 x 800\", \"at\": [4000, 3000]}}"
    ));
    shortcut(&mut h, "CS");
    assert!(h.state().status.error, "nothing selected, nothing to create one like");
    // The narrower door: the Door tool, placing that type.
    click(&mut h, P2(2000.0, 0.0));
    shortcut(&mut h, "CS");
    assert!(matches!(h.state().tool, Tool::Door));
    click(&mut h, P2(6000.0, 6000.0));
    let doors = ids(&h, "door");
    assert_eq!(doors.len(), 2);
    assert_eq!(field(&h, doors[1], "type"), json!("Single 800 x 2100"));
    // A table, by the edge of its top: the Component tool, placing its family's type.
    key(&mut h, Key::Escape);
    click(&mut h, P2(4000.0, 2600.0));
    shortcut(&mut h, "CS");
    assert!(matches!(h.state().tool, Tool::Component));
    click(&mut h, P2(4000.0, 4800.0));
    let placed: Vec<(Value, Value)> = ids(&h, "component").into_iter().map(|id| (field(&h, id, "family"), field(&h, id, "type"))).collect();
    assert_eq!(placed, vec![(json!("Table"), json!("1200 x 800")); 2]);
}

#[test]
fn pp_shows_and_hides_the_properties_palette() {
    let mut h = app(BOX);
    assert!(h.query_by_label("Type Selector").is_some());
    shortcut(&mut h, "PP");
    assert!(h.query_by_label("Type Selector").is_none());
    assert!(h.query_by_label("Floor Plans").is_some(), "the Project Browser stays");
    shortcut(&mut h, "PP");
    assert!(h.query_by_label("Type Selector").is_some());
}

#[test]
fn zf_ze_and_za_zoom_to_fit() {
    let mut h = app(SAMPLE);
    let p = P2(0.0, 0.0);
    press(&mut h, "Zoom to Fit");
    let fitted = h.state().screen(p);
    for letters in ["ZF", "ZE", "ZA"] {
        h.hover_at(h.state().screen(P2(8000.0, 3500.0)));
        h.event(Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, 120.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        h.run();
        assert!((h.state().screen(p) - fitted).length() > 10.0, "zoomed in");
        shortcut(&mut h, letters);
        assert!((h.state().screen(p) - fitted).length() < 1.0, "{letters} fits the plan, as the navigation bar does");
    }
}

#[test]
fn sa_selects_every_element_of_the_selected_ones_type_the_view_shows() {
    let mut h = app_at(
        &format!(
            "{BOX}\ndoor {{\"at\": [2000, 0]}}\ndoor {{\"at\": [6000, 0]}}\ndoor {{\"at\": [2000, 6000]}}\ndoor {{\"at\": [6000, 6000]}}\n\
             door {{\"at\": [4000, 6000], \"type\": \"Single 800 x 2100\"}}\n\
             wall {{\"points\": [[0, 0], [8000, 0]], \"level\": \"Level 2\"}}\ndoor {{\"at\": [2000, 0], \"level\": \"Level 2\"}}\n\
             component {{\"family\": \"Desk\", \"type\": \"1200 x 600\", \"at\": [2000, 2000]}}\n\
             component {{\"family\": \"Desk\", \"type\": \"1200 x 600\", \"at\": [2000, 4500]}}\n\
             component {{\"family\": \"Wardrobe\", \"type\": \"1200 x 600\", \"at\": [5000, 2000]}}\n\
             section {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}"
        ),
        0.05,
    );
    shortcut(&mut h, "SA");
    assert!(h.state().status.error, "nothing selected, no type to select");
    // The doors as made: the south wall's two, the north wall's two, the narrower one, Level 2's.
    let doors = ids(&h, "door");
    // In Level 1's plan, the doors of the wider type on it: not the narrower one, nor Level 2's.
    click(&mut h, P2(2000.0, 0.0));
    shortcut(&mut h, "SA");
    assert_eq!(h.state().selection, doors[..4]);
    // A component's type is its family's: the desks, not the wardrobe of the same type name.
    key(&mut h, Key::Escape);
    click(&mut h, P2(2000.0, 2000.0));
    shortcut(&mut h, "SA");
    let desks: Vec<Id> = ids(&h, "component").into_iter().filter(|id| field(&h, *id, "family") == json!("Desk")).collect();
    assert_eq!(h.state().selection, desks);
    // A section shows what lies past its line: the north wall's doors, not the south wall's behind it.
    browse(&mut h, "Section 1");
    click(&mut h, P2(3000.0, 1000.0));
    assert_eq!(h.state().selection, [doors[2]], "the door the section shows there");
    shortcut(&mut h, "SA");
    assert_eq!(h.state().selection, doors[2..4]);
}

#[test]
fn ks_lists_every_shortcut_and_every_one_it_lists_works() {
    let mut h = app(BOX);
    shortcut(&mut h, "KS");
    for label in ["Create Similar", "CS", "Select All Instances", "SA", "CR", "WA", "KS", "ZF, ZE, ZA", "Demolish", "DW"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    shortcut(&mut h, "WA");
    assert!(matches!(h.state().tool, Tool::Modify), "the dialog takes the keys while it's open");
    press(&mut h, "Close");
    assert!(h.query_by_label("Create Similar").is_none());
    // UN, as Revit's opens Project Units: Manage's Project Units menu.
    shortcut(&mut h, "UN");
    press(&mut h, "Feet and inches");
    assert_eq!(h.state().session.model.units.name(), "ft-in");
    // Any two letters do something exactly when the dialog lists them, and it names each in every language.
    let listed: Vec<&str> = crate::shortcuts().iter().map(|(letters, _)| *letters).collect();
    // One key, one thing: a fixture's (the Plumbing Fixture's PX, once Explode's too) or an alias that a desktop key
    // takes first would be listed twice and reach only the first.
    let mut seen = std::collections::BTreeMap::new();
    for (letters, name) in crate::shortcuts() {
        if let Some(other) = seen.insert(letters, name) {
            assert_eq!(letters, "CL", "{letters} is both {other} and {name}");
        }
    }
    for a in 'A'..='Z' {
        for b in 'A'..='Z' {
            let mut app = BimApp::new(Session::new());
            app.typed = format!("{a}{b}");
            assert_eq!(app.shortcut(), listed.contains(&format!("{a}{b}").as_str()), "{a}{b}");
        }
    }
    use crate::lang::Lang;
    for (_, name) in crate::shortcuts() {
        for l in [Lang::Portuguese, Lang::Spanish] {
            assert!(crate::lang::translated_in(l, name).is_some(), "{name} in {l:?}");
        }
    }
}

#[test]
fn manage_tab_opens_keyboard_shortcuts_dialog() {
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Shortcuts");
    assert!(h.state().show_shortcuts);
    assert!(h.query_by_label("Load File...").is_some());
    assert!(h.query_by_label("Add Shortcut").is_some());
    press(&mut h, "Close");
    assert!(!h.state().show_shortcuts);
}

#[test]
fn typed_3d_opens_the_3d_view() {
    let mut h = app(BOX);
    assert_ne!(h.state().sheet, Sheet::View3D);
    shortcut(&mut h, "3D");
    assert_eq!(h.state().sheet, Sheet::View3D);
}

#[test]
fn ma_gives_the_walls_selected_after_the_first_its_type_in_one_undo_step() {
    let mut h = app(BOX);
    let walls = ids(&h, "wall");
    h.state_mut().run("modify", json!({"id": walls[0], "type": "Exterior - 300mm"}));
    let types = |h: &Harness<BimApp>| [walls[0], walls[1], walls[2]].map(|id| field(h, id, "type"));
    assert_eq!(types(&h), [json!("Exterior - 300mm"), json!("Generic - 200mm"), json!("Generic - 200mm")]);
    // One selected: its type becomes the one the next wall takes, and nothing changes.
    h.state_mut().selection = vec![walls[0]];
    shortcut(&mut h, "MA");
    assert_eq!(h.state().types.get("wall").map(String::as_str), Some("Exterior - 300mm"));
    assert_eq!(types(&h)[1], json!("Generic - 200mm"));
    // Several: the others of its category take the first's type, in one step.
    h.state_mut().selection = vec![walls[0], walls[1], walls[2]];
    shortcut(&mut h, "MA");
    assert!(h.state().status.text.contains("Matched properties") && !h.state().status.error, "{}", h.state().status.text);
    assert_eq!(types(&h), ["Exterior - 300mm"; 3].map(|t| json!(t)));
    h.state_mut().run("undo", json!({}));
    assert_eq!(types(&h), [json!("Exterior - 300mm"), json!("Generic - 200mm"), json!("Generic - 200mm")]);
    // Nothing selected: it says so.
    h.state_mut().selection.clear();
    shortcut(&mut h, "MA");
    assert!(h.state().status.error);
}

#[test]
fn keys_for_tools_bimcraft_lacks_are_bound_to_nothing_rather_than_to_another_tool() {
    for keys in ["SU", "LI", "SF", "BR", "BS", "RN", "RP", "CV", "TF", "DA", "FD"] {
        let mut app = BimApp::new(Session::new());
        app.typed = keys.into();
        assert!(!app.shortcut(), "{keys}");
    }
}

#[test]
fn the_level_and_grid_tools_add_datums() {
    let mut h = app("");
    tool(&mut h, "Level");
    assert_eq!(h.get_by_label("Elevation").value().as_deref(), Some("6000"), "the Options Bar proposes the next elevation");
    press(&mut h, "Create");
    let levels = h.state().session.model.levels().iter().map(|(_, l)| (l.name.clone(), l.elevation)).collect::<Vec<_>>();
    assert_eq!(levels.last(), Some(&("Level 3".to_string(), 6000.0)));
    topmost(&mut h, "Level 3");
    // Undoing the level leaves its plan open on nothing: the browser falls back to the lowest.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(h.state().sheet, Sheet::Plan(100000));
    tool(&mut h, "Grid");
    click(&mut h, P2(-3000.0, -3000.0));
    click(&mut h, P2(-3000.0, 3000.0));
    let grid = ids(&h, "grid");
    assert_eq!(grid.len(), 1);
    assert_eq!((field(&h, grid[0], "start"), field(&h, grid[0], "end")), (json!([-3000.0, -3000.0]), json!([-3000.0, 3000.0])));
}

#[test]
fn saving_writes_the_project_and_exports_by_extension() {
    let mut h = app(SAMPLE);
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    h.state_mut().session.execute("level", &json!({"elevation": 6000})).unwrap();
    assert!(h.state().dirty());
    let project = dir.join("house.bimcraft");
    assert!(h.state_mut().save_to(&project));
    assert!(!h.state().dirty() && h.state().session.path.as_deref() == Some(project.as_path()));
    for (name, starts) in [("house.ifc", &b"ISO-10303-21"[..]), ("plan.png", b"\x89PNG"), ("plan.svg", b"<svg")] {
        assert!(h.state_mut().save_to(&dir.join(name)), "{name}: {}", h.state().status.text);
        assert!(std::fs::read(dir.join(name)).unwrap().starts_with(starts), "{name}");
    }
    assert_eq!(h.state().session.path.as_deref(), Some(project.as_path()), "exports leave the project's path alone");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_asks_before_dropping_unsaved_changes() {
    let mut h = app("");
    h.state_mut().session.execute("wall", &json!({"points": [[0, 0], [4000, 0]]})).unwrap();
    // Menu items read out their shortcut too: "New Ctrl+N".
    let new = |h: &mut Harness<BimApp>| {
        press(h, "File");
        h.get_by_label_contains("New").click();
        h.run();
    };
    new(&mut h);
    press(&mut h, "Cancel");
    assert_eq!(walls(&h).len(), 1);
    new(&mut h);
    press(&mut h, "Don't Save");
    assert!(walls(&h).is_empty());
}

#[test]
fn the_ribbon_has_revit_tabs_and_a_contextual_tab_for_the_tool_or_selection() {
    let mut h = app(SAMPLE);
    for label in ["File", "Architecture", "View", "Wall"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    assert_eq!(h.query_all_by_label("Modify").count(), 2, "the Modify tab, and the Modify tool on Architecture");
    tool(&mut h, "Wall");
    assert!(h.query_by_label("Modify | Place Wall").is_some());
    assert!(h.query_by_label("Wall").is_none(), "the contextual tab took the ribbon");
    key(&mut h, Key::Escape);
    assert!(h.query_by_label("Modify | Place Wall").is_none(), "gone with the tool");
    assert!(h.query_by_label("Wall").is_some(), "back on Architecture");
    let walls_before = ids(&h, "wall").len();
    click(&mut h, P2(10000.0, 2000.0));
    assert!(h.query_by_label("Modify | Walls").is_some());
    press(&mut h, "Delete");
    assert_eq!(ids(&h, "wall").len(), walls_before - 1, "the contextual tab's Delete");
    press(&mut h, "View");
    for label in ["Thin Lines", "Schedules", "Close Inactive", "User Interface"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
}

#[test]
fn the_type_selector_and_edit_type_change_types_as_in_revit() {
    let mut h = app(BOX);
    let wall = ids(&h, "wall")[0];
    click(&mut h, P2(4000.0, 0.0));
    assert_eq!(h.state().selection, [wall]);
    press(&mut h, "Type Selector");
    press(&mut h, "Exterior - 300mm");
    assert_eq!(field(&h, wall, "type"), json!("Exterior - 300mm"));
    // Edit Type changes the type, so every wall of it.
    press(&mut h, "Edit Type");
    type_into(&mut h, "Width", "350");
    press(&mut h, "OK");
    assert_eq!(h.state().session.model.types.wall.get("Exterior - 300mm").map(|t| t.thickness), Some(350.0));
    // Duplicate makes a new type, and OK puts the selected wall on it.
    press(&mut h, "Edit Type");
    press(&mut h, "Duplicate…");
    type_into(&mut h, "New type", "Party - 250mm");
    press(&mut h, "Create");
    type_into(&mut h, "Width", "250");
    press(&mut h, "OK");
    assert_eq!(h.state().session.model.types.wall.get("Party - 250mm").map(|t| t.thickness), Some(250.0));
    assert_eq!(field(&h, wall, "type"), json!("Party - 250mm"));
    // With a tool on, the Type Selector picks the type of what it places.
    key(&mut h, Key::Escape);
    tool(&mut h, "Door");
    press(&mut h, "Type Selector");
    press(&mut h, "Single 800 x 2100");
    click(&mut h, P2(2000.0, 0.0));
    let door = ids(&h, "door");
    assert_eq!(field(&h, door[0], "type"), json!("Single 800 x 2100"));
}

#[test]
fn the_options_bar_sets_the_wall_height_and_chain() {
    let mut h = app("");
    tool(&mut h, "Wall");
    h.get_by_role_and_label(Role::ComboBox, "Height:").click();
    h.run();
    press(&mut h, "Unconnected");
    // Unconnected, a field for the height comes next to the drop-down.
    assert_eq!(h.get_by_role_and_label(Role::TextInput, "Height:").value().as_deref(), Some("3000"));
    h.get_by_role_and_label(Role::TextInput, "Height:").focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by_role_and_label(Role::TextInput, "Height:").type_text("2500");
    h.run();
    press(&mut h, "Chain");
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(4000.0, 0.0));
    let wall = ids(&h, "wall");
    assert_eq!(wall.len(), 1, "without Chain, two clicks make a wall");
    assert_eq!((field(&h, wall[0], "top_level"), field(&h, wall[0], "height")), (json!(null), json!(2500.0)));
    click(&mut h, P2(0.0, 3000.0));
    click(&mut h, P2(4000.0, 3000.0));
    assert_eq!(ids(&h, "wall").len(), 2, "and the next two another");
}

#[test]
fn views_open_in_tabs_that_switch_and_close() {
    let mut h = app(SAMPLE);
    topmost(&mut h, "Level 2");
    browse(&mut h, "Room Schedule");
    assert_eq!(h.state().sheet, Sheet::Schedule("room"));
    topmost(&mut h, "Level 1");
    assert_eq!(h.state().sheet, Sheet::Plan(100000));
    press(&mut h, "Close Room Schedule");
    assert!(h.query_by_label("Close Room Schedule").is_none());
    press(&mut h, "View");
    press(&mut h, "Close Inactive");
    assert!(h.query_by_label("Close Level 2").is_none(), "only the active view stays open");
    assert!(h.query_by_label("Close Level 1").is_none(), "and the last view can't close");
    assert_eq!(h.state().sheet, Sheet::Plan(100000));
}

#[test]
fn ctrl_adds_shift_takes_away_and_tab_reaches_what_is_underneath() {
    let mut h = app(SAMPLE);
    let door = find(&h, |e| matches!(e, Element::Door(o) if o.mark == "1"));
    let window = find(&h, |e| matches!(e, Element::Window(o) if o.mark == "1"));
    // The south wall: drawn clockwise, it runs west.
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(10000.0, 0.0) && w.end == P2(0.0, 0.0)));
    click(&mut h, P2(2000.0, 0.0));
    click_with(&mut h, P2(4300.0, 0.0), Modifiers::COMMAND);
    assert_eq!(h.state().selection, [door, window]);
    assert!(h.query_by_label("Modify | Multi-Select").is_some(), "a door and a window: two categories");
    click_with(&mut h, P2(2000.0, 0.0), Modifiers::SHIFT);
    assert_eq!(h.state().selection, [window]);
    // Over the door, Tab moves on to the wall it's in, and a click takes that.
    h.hover_at(h.state().screen(P2(2000.0, 0.0)));
    h.run();
    key(&mut h, Key::Tab);
    click(&mut h, P2(2000.0, 0.0));
    assert_eq!(h.state().selection, [wall]);
}

#[test]
fn dragging_across_empty_floor_selects_by_window_or_crossing() {
    let mut h = app(SAMPLE);
    let window = find(&h, |e| matches!(e, Element::Window(o) if o.type_name == "Fixed 600 x 900"));
    // The east wall: drawn clockwise, it runs south.
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(10000.0, 7000.0) && w.end == P2(10000.0, 0.0)));
    let tag = find(&h, |e| matches!(e, Element::Tag(t) if t.host == gid(&h, window)));
    // Left to right: what's wholly inside, the window and its tag. (From empty floor, clear of the
    // Bedroom's tag and the dimension at x = 10900.)
    drag(&mut h, P2(9400.0, 4200.0), P2(11400.0, 6000.0));
    assert_eq!(h.state().selection, [window, tag]);
    // Right to left: what it crosses too.
    drag(&mut h, P2(11400.0, 6000.0), P2(9400.0, 4200.0));
    let sel = &h.state().selection;
    assert!(sel.contains(&window) && sel.contains(&wall), "{sel:?}");
}

#[test]
fn dragging_a_wall_moves_it_and_the_walls_and_room_around_follow() {
    let mut h = app(&format!("{BOX}\nroom {{\"point\": [2000, 3000]}}"));
    let room = ids(&h, "room")[0];
    drag(&mut h, P2(8000.0, 3000.0), P2(9000.0, 3000.0));
    assert_eq!(walls(&h)[1], (P2(9000.0, 0.0), P2(9000.0, 6000.0)));
    assert_eq!(walls(&h)[0], (P2(0.0, 0.0), P2(9000.0, 0.0)), "the corner went along");
    assert_eq!(field(&h, room, "area_m2"), json!(54.0));
    assert_eq!(h.state().session.history().last(), Some(&"Move"));
}

#[test]
fn move_and_copy_take_a_base_point_and_a_destination() {
    let mut h = app(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}"));
    let door = ids(&h, "door")[0];
    click(&mut h, P2(2000.0, 0.0));
    press(&mut h, "Move");
    click(&mut h, P2(2000.0, 0.0));
    click(&mut h, P2(3000.0, 0.0));
    assert_eq!(field(&h, door, "at"), json!([3000.0, 0.0]));
    assert_eq!(h.state().selection, [door], "what moved stays selected");
    press(&mut h, "Copy");
    click(&mut h, P2(3000.0, 0.0));
    click(&mut h, P2(6000.0, 0.0));
    let doors = ids(&h, "door");
    assert_eq!(doors.len(), 2);
    assert_eq!(field(&h, doors[1], "at"), json!([6000.0, 0.0]));
    assert_eq!(h.state().selection, [doors[1]], "the copy is selected, as in Revit");
}

#[test]
fn typing_a_length_puts_the_next_point_that_far_toward_the_cursor() {
    let mut h = app("");
    tool(&mut h, "Wall");
    click(&mut h, P2(0.0, 0.0));
    // A cursor a degree off horizontal: the length goes along the horizontal.
    h.hover_at(h.state().screen(P2(1000.0, 20.0)));
    h.run();
    h.event(Event::Text("4500".into()));
    h.run();
    key(&mut h, Key::Enter);
    key(&mut h, Key::Enter);
    assert_eq!(walls(&h), [(P2(0.0, 0.0), P2(4500.0, 0.0))]);
}

#[test]
fn a_selected_walls_temporary_dimension_sets_its_length() {
    let mut h = app(BOX);
    click(&mut h, P2(4000.0, 0.0));
    h.get_by_label_contains("Temporary dimension").click();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text("9000".into()));
    h.run();
    key(&mut h, Key::Enter);
    assert_eq!(walls(&h)[0], (P2(0.0, 0.0), P2(9000.0, 0.0)));
    assert_eq!(walls(&h)[1].0, P2(9000.0, 0.0), "the wall joined at that end followed");
}

#[test]
fn a_selected_doors_temporary_dimensions_set_where_it_stands() {
    let mut h = app(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}"));
    let door = ids(&h, "door")[0];
    click(&mut h, P2(2000.0, 0.0));
    assert_eq!(h.state().selection, [door]);
    // From the inner face of each wall at the ends of its wall (200 thick, on their centre lines) to the middle of the door.
    assert!(h.query_by_label("Temporary dimension 1900").is_some());
    h.get_by_label("Temporary dimension 5900").click();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text("5000".into()));
    h.run();
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, door, "offset"), json!(2900.0), "5000 from the far wall's face");
    assert!(h.query_by_label("Temporary dimension 2800").is_some());
}

#[test]
fn a_doors_temporary_dimensions_stop_at_the_face_of_a_wall_crossing_its_wall() {
    let mut h =
        app(&format!("{BOX}\nwall {{\"points\": [[5000, 0], [5000, 6000]], \"type\": \"Interior - 100mm\"}}\ndoor {{\"at\": [2000, 0]}}"));
    let door = ids(&h, "door")[0];
    click(&mut h, P2(2000.0, 0.0));
    assert_eq!(h.state().selection, [door]);
    // 100 to 4950, the faces either side: 1900 and 2950.
    assert!(h.query_by_label("Temporary dimension 2950").is_some());
    retype_dimension(&mut h, "1900", "2400");
    assert_eq!(field(&h, door, "offset"), json!(2500.0), "2400 from the west wall's face");
    assert!(h.query_by_label("Temporary dimension 2450").is_some(), "the rest of the 4850 between the faces");
    // Past a face is off its stretch of wall: nothing moves.
    let steps = h.state().session.history().len();
    retype_dimension(&mut h, "2400", "6000");
    assert_eq!(h.state().session.history().len(), steps);
    assert!(h.state().status.error);
}

#[test]
fn a_door_placed_or_dragged_near_the_middle_between_the_faces_snaps_to_it() {
    let mut h = app(&format!("{BOX}\nwall {{\"points\": [[5000, 0], [5000, 6000]], \"type\": \"Interior - 100mm\"}}"));
    // Between the faces at 100 and 4950 the middle is 2525.
    shortcut(&mut h, "DR");
    click(&mut h, P2(2560.0, 0.0));
    click(&mut h, P2(1000.0, 6000.0));
    let doors = ids(&h, "door");
    assert_eq!(field(&h, doors[0], "offset"), json!(2525.0), "snapped to the middle");
    // The north wall runs west from 8000: x 1000 is 7000 along it, between faces at 3050 and 7900.
    assert_eq!(field(&h, doors[1], "offset"), json!(7000.0), "far from it, where it was put");
    // Dragged: to near the middle, x 2525, then on past it.
    key(&mut h, Key::Escape);
    click(&mut h, P2(1000.0, 6000.0));
    drag(&mut h, P2(1000.0, 6000.0), P2(2490.0, 6000.0));
    assert_eq!(field(&h, doors[1], "offset"), json!(5475.0));
    drag(&mut h, P2(2525.0, 6000.0), P2(3500.0, 6000.0));
    let offset = field(&h, doors[1], "offset").as_f64().unwrap();
    assert!((offset - 4500.0).abs() < 10.0, "free again, as far as a pixel lets the drag say: {offset}");
}

/// Click the number of a temporary dimension and type another.
fn retype_dimension(h: &mut Harness<BimApp>, was: &str, now: &str) {
    h.get_by_label(&format!("Temporary dimension {was}")).click();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text(now.into()));
    h.run();
    key(h, Key::Enter);
}

#[test]
fn a_selected_columns_temporary_dimensions_set_how_far_it_stands_from_the_walls() {
    let mut h = app(&format!("{BOX}\ncolumn {{\"at\": [3000, 2000]}}"));
    let column = ids(&h, "column")[0];
    click(&mut h, P2(3000.0, 2000.0));
    assert_eq!(h.state().selection, [column]);
    // To the walls east, north, west and south of it.
    for n in ["5000", "4000", "3000", "2000"] {
        assert!(h.query_by_label(&format!("Temporary dimension {n}")).is_some(), "{n}");
    }
    retype_dimension(&mut h, "3000", "1000");
    assert_eq!(field(&h, column, "at"), json!([1000.0, 2000.0]), "1000 from the west wall");
    assert_eq!(h.state().session.history().last(), Some(&"Move"));
    assert!(h.query_by_label("Temporary dimension 7000").is_some(), "the east one follows");
    // From the south wall instead: 2000 up goes to 500.
    retype_dimension(&mut h, "2000", "500");
    assert_eq!(field(&h, column, "at"), json!([1000.0, 500.0]));
    // What isn't a length moves nothing, and says so.
    let steps = h.state().session.history().len();
    retype_dimension(&mut h, "1000", "far");
    assert_eq!(h.state().session.history().len(), steps);
    assert!(h.state().status.error);
}

#[test]
fn a_selected_components_temporary_dimensions_set_how_far_it_stands_from_a_grid_or_wall() {
    let mut h = app(&format!(
        "{BOX}\ngrid {{\"start\": [1000, -500], \"end\": [1000, 6500]}}\n\
         component {{\"family\": \"Table\", \"type\": \"1600 x 900\", \"at\": [3000, 2500]}}"
    ));
    let table = ids(&h, "component")[0];
    // Its top is an outline: click on its east edge.
    click(&mut h, P2(3800.0, 2500.0));
    assert_eq!(h.state().selection, [table]);
    // East 5000 to the wall, north 3500, west 2000 to the grid, south 2500.
    for n in ["5000", "3500", "2000", "2500"] {
        assert!(h.query_by_label(&format!("Temporary dimension {n}")).is_some(), "{n}");
    }
    retype_dimension(&mut h, "5000", "4000");
    assert_eq!(field(&h, table, "at"), json!([4000.0, 2500.0]));
    assert!(h.query_by_label("Temporary dimension 3000").is_some(), "west is now 3000 from the grid");
}

#[test]
fn the_column_and_component_tools_show_how_far_a_click_would_stand_from_the_walls() {
    for (label, east, north, west, south) in [("Column", "5000", "4000", "3000", "2000"), ("Component", "5000", "3500", "3000", "2500")] {
        let mut h = app(BOX);
        tool(&mut h, label);
        let at = P2(3000.0, if label == "Column" { 2000.0 } else { 2500.0 });
        h.hover_at(h.state().screen(at));
        h.run();
        let text = painted_text(&h);
        for n in [east, north, west, south] {
            assert!(text.contains(&n.to_string()), "{label} {n}: {text:?}");
        }
        assert!(h.state().session.model.elements.values().all(|e| !matches!(e, Element::Column(_) | Element::Component(_))));
    }
}

#[test]
fn a_line_weight_is_a_pen_picked_from_a_list_and_a_hairline_is_a_pixel_on_screen() {
    let mut h = app(BOX);
    shortcut(&mut h, "VV");
    pick(&mut h, Role::Button, "Walls: Cut Lines");
    pick(&mut h, Role::CheckBox, "Cut: Line weight");
    assert_eq!(combo_value(&h, "Cut: Line weight"), "10 · 0.35 mm", "a cut wall's own pen to start with");
    choose(&mut h, "Cut: Line weight", "0 · Hairline");
    press(&mut h, "OK");
    press(&mut h, "OK");
    assert_eq!(plan_graphics(&h, 100000).categories["wall"].cut.weight, Some(Pen(0)));
    // On screen a line is its pen's pixels at the zoom, at least one: a hairline is one at any zoom; Thin Lines makes all one.
    use crate::canvas::on_screen;
    assert_eq!((on_screen(2.0, 3.0, false), on_screen(0.0, 3.0, false), on_screen(0.2, 1.0, false)), (6.0, 1.0, 1.0));
    assert_eq!(on_screen(2.0, 3.0, true), 1.0);
}

#[test]
fn a_project_in_feet_and_inches_offers_the_architects_and_engineers_scales() {
    let mut h = app(&format!("{BOX}\nunits {{\"length\": \"ft-in\"}}"));
    // A view with none of its own is at an eighth of an inch to the foot, and says so.
    assert!(painted_text(&h).contains(&"1/8\" = 1'-0\"".to_string()));
    assert_eq!(combo_value(&h, "View Scale"), "1/8\" = 1'-0\"");
    press(&mut h, "Scale");
    assert!(h.query_by_label("1 : 100").is_none());
    press(&mut h, "1\" = 20'-0\"");
    assert_eq!(plan_graphics(&h, 100000).scale, Some(240.0));
    choose(&mut h, "View Scale", "1/4\" = 1'-0\"");
    assert_eq!(plan_graphics(&h, 100000).scale, Some(48.0));
    // Typed as a drawing names it, or as 1 : n.
    press(&mut h, "Scale");
    type_into(&mut h, "Custom", "3/32\" = 1'-0\"");
    key(&mut h, Key::Enter);
    assert_eq!(plan_graphics(&h, 100000).scale, Some(128.0));
    type_role(&mut h, Role::TextInput, "Scale Value 1:", "1 : 300");
    assert_eq!(plan_graphics(&h, 100000).scale, Some(300.0));
    assert_eq!(combo_value(&h, "View Scale"), "1\" = 25'-0\"");
}

#[test]
fn the_view_control_bar_sets_a_views_scale_and_detail_level() {
    use bimcraft_core::graphics::Detail;
    let mut h = app(&format!("{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}"));
    let section = ids(&h, "section")[0];
    let g = |h: &Harness<BimApp>, id| plan_graphics(h, id);
    // A view with no scale of its own is at 1 : 100; every view starts at fine.
    let shown = painted_text(&h);
    assert!(shown.contains(&"1 : 100".to_string()) && shown.contains(&"Fine".to_string()), "{shown:?}");
    press(&mut h, "Scale");
    press(&mut h, "1 : 50");
    assert_eq!(g(&h, 100000).scale, Some(50.0));
    assert!(painted_text(&h).contains(&"1 : 50".to_string()));
    assert_eq!(h.state().session.history().last(), Some(&"View Scale"));
    pick(&mut h, Role::Button, "Detail Level");
    press(&mut h, "Coarse");
    assert_eq!(g(&h, 100000).detail, Detail::Coarse);
    assert!(painted_text(&h).contains(&"Coarse".to_string()));
    // Another scale, typed; what isn't one is said and changes nothing.
    press(&mut h, "Scale");
    type_into(&mut h, "Custom", "75");
    key(&mut h, Key::Enter);
    assert_eq!(g(&h, 100000).scale, Some(75.0));
    assert!(painted_text(&h).contains(&"1 : 75".to_string()));
    press(&mut h, "Scale");
    type_into(&mut h, "Custom", "big");
    key(&mut h, Key::Enter);
    assert!(h.state().status.error);
    assert_eq!(g(&h, 100000).scale, Some(75.0));
    // A view always has a scale: the menu has no Auto.
    press(&mut h, "Scale");
    assert!(h.query_by_label("Auto").is_none());
    press(&mut h, "1 : 100");
    assert_eq!(g(&h, 100000).scale, Some(100.0));
    // The section has its own, and the bar shows the one of the view on screen.
    browse(&mut h, "Section 1");
    assert_eq!(h.state().sheet, Sheet::Section(section));
    pick(&mut h, Role::Button, "Detail Level");
    press(&mut h, "Medium");
    assert_eq!((g(&h, section).detail, g(&h, 100000).detail), (Detail::Medium, Detail::Coarse));
    assert!(painted_text(&h).contains(&"Medium".to_string()));
}

#[test]
fn the_view_control_bar_sets_the_3d_views_visual_style_and_wf_hl_and_sd_do_too() {
    use bimcraft_core::graphics::{Look, VisualStyle};
    let mut h = app(BOX);
    let style = |h: &Harness<BimApp>| Look::of_3d(&h.state().session.model).style();
    press(&mut h, "Default 3D View");
    assert_eq!(h.state().sheet, Sheet::View3D);
    // Shaded until told otherwise; a 3D view has no scale or detail level.
    assert!(painted_text(&h).contains(&"Shaded".to_string()));
    assert!(h.query_by_label("Scale").is_none() && h.query_by_label("Detail Level").is_none());
    pick(&mut h, Role::Button, "Visual Style");
    press(&mut h, "Wireframe");
    assert_eq!(style(&h), VisualStyle::Wireframe);
    assert!(painted_text(&h).contains(&"Wireframe".to_string()));
    assert_eq!(h.state().session.history().last(), Some(&"Visual Style"));
    // Revit's letters: hidden line, shading, wireframe.
    for (letters, want) in [("HL", VisualStyle::HiddenLine), ("SD", VisualStyle::Shaded), ("WF", VisualStyle::Wireframe)] {
        shortcut(&mut h, letters);
        assert_eq!(style(&h), want, "{letters}");
    }
    // A plan has no visual style: the letters say so, and the 3D view keeps its own.
    h.state_mut().open_sheet(Sheet::Plan(100000));
    h.run();
    h.state_mut().status = crate::Status { text: String::new(), error: false };
    shortcut(&mut h, "HL");
    assert!(h.state().status.error);
    assert_eq!(style(&h), VisualStyle::Wireframe);
}

/// What the drop-down named `label` shows, as a screen reader reads it.
fn combo_value(h: &Harness<BimApp>, label: &str) -> String {
    h.get_by_role_and_label(Role::ComboBox, label).value().unwrap_or_default()
}

/// What the text field named `label` holds.
fn field_text(h: &Harness<BimApp>, label: &str) -> String {
    h.get_by_role_and_label(Role::TextInput, label).value().unwrap_or_default()
}

/// Open the drop-down named `label` and pick `option` from it, as screen readers do. A palette's foot, and a list's, can lie below
/// the window.
fn choose(h: &mut Harness<BimApp>, label: &str, option: &str) {
    h.get_by_role_and_label(Role::ComboBox, label).click_accesskit();
    h.run();
    h.get_by_label(option).click_accesskit();
    h.run();
    // Such a click leaves the list open, which a pointer's closes: the drop-down again shuts it.
    if egui::Popup::is_any_open(&h.ctx) {
        h.get_by_role_and_label(Role::ComboBox, label).click_accesskit();
        h.run();
    }
}

#[test]
fn properties_of_a_view_show_and_set_its_scale_detail_level_and_visual_style() {
    use bimcraft_core::graphics::{Detail, Look, VisualStyle};
    let mut h = app(&format!("{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}"));
    let section = ids(&h, "section")[0];
    let steps = |h: &Harness<BimApp>| h.state().session.history().len();
    // A plan: 1 : 100 and fine detail, until set; a plan has no visual style.
    assert_eq!((combo_value(&h, "View Scale"), combo_value(&h, "Detail Level")), ("1 : 100".into(), "Fine".into()));
    assert_eq!(field_text(&h, "Scale Value 1:"), "100");
    assert!(h.query_by_role_and_label(Role::ComboBox, "Visual Style").is_none());
    let before = steps(&h);
    choose(&mut h, "View Scale", "1 : 50");
    assert_eq!((plan_graphics(&h, 100000).scale, steps(&h)), (Some(50.0), before + 1), "one undo step");
    assert_eq!(h.state().session.history().last(), Some(&"View Scale"));
    assert_eq!(combo_value(&h, "View Scale"), "1 : 50");
    choose(&mut h, "Detail Level", "Coarse");
    assert_eq!((plan_graphics(&h, 100000).detail, steps(&h)), (Detail::Coarse, before + 2));
    assert_eq!(h.state().session.history().last(), Some(&"Detail Level"));
    assert_eq!(combo_value(&h, "Detail Level"), "Coarse");
    // The scale's value, typed: any number a scale may be; what isn't one is said and changes nothing.
    assert_eq!(field_text(&h, "Scale Value 1:"), "50");
    type_role(&mut h, Role::TextInput, "Scale Value 1:", "75");
    assert_eq!((plan_graphics(&h, 100000).scale, steps(&h)), (Some(75.0), before + 3));
    assert_eq!(combo_value(&h, "View Scale"), "1 : 75");
    type_role(&mut h, Role::TextInput, "Scale Value 1:", "big");
    assert!(h.state().status.error);
    assert_eq!((plan_graphics(&h, 100000).scale, steps(&h)), (Some(75.0), before + 3));
    // A view always has a scale: emptied, the field is refused as "big" is, and there's no Auto to choose.
    type_role(&mut h, Role::TextInput, "Scale Value 1:", "");
    assert!(h.state().status.error);
    assert_eq!((plan_graphics(&h, 100000).scale, steps(&h)), (Some(75.0), before + 3));
    h.get_by_role_and_label(Role::ComboBox, "View Scale").click_accesskit();
    h.run();
    assert!(h.query_by_label("1 : 20").is_some() && h.query_by_label("Auto").is_none());
    h.get_by_role_and_label(Role::ComboBox, "View Scale").click_accesskit();
    h.run();
    // A section has its own, and Properties shows the one on screen. Its palette is long: a tall window shows the foot of it.
    h.set_size(vec2(1280.0, 1600.0));
    browse(&mut h, "Section 1");
    assert_eq!(h.state().sheet, Sheet::Section(section));
    assert_eq!((combo_value(&h, "View Scale"), combo_value(&h, "Detail Level")), ("1 : 100".into(), "Fine".into()));
    choose(&mut h, "View Scale", "1 : 200");
    choose(&mut h, "Detail Level", "Medium");
    let g = bimcraft_core::graphics::graphics_of(&h.state().session.model, section).unwrap();
    assert_eq!((g.scale, g.detail), (Some(200.0), Detail::Medium));
    assert_eq!((plan_graphics(&h, 100000).scale, plan_graphics(&h, 100000).detail), (Some(75.0), Detail::Coarse));
    // The 3D view: a visual style, no scale or detail level.
    press(&mut h, "Default 3D View");
    assert_eq!(h.state().sheet, Sheet::View3D);
    assert!(h.query_by_role_and_label(Role::ComboBox, "View Scale").is_none());
    assert!(h.query_by_role_and_label(Role::ComboBox, "Detail Level").is_none());
    assert_eq!(combo_value(&h, "Visual Style"), "Shaded");
    choose(&mut h, "Visual Style", "Hidden Line");
    assert_eq!(Look::of_3d(&h.state().session.model).style(), VisualStyle::HiddenLine);
    assert_eq!(h.state().session.history().last(), Some(&"Visual Style"));
    assert_eq!(combo_value(&h, "Visual Style"), "Hidden Line");
    // Undone, it is shaded again, and so is the drop-down.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(combo_value(&h, "Visual Style"), "Shaded");
}

/// The text the last frame painted, whatever widget or painter put it there.
fn painted_text(h: &Harness<BimApp>) -> Vec<String> {
    fn walk(s: &egui::Shape, out: &mut Vec<String>) {
        match s {
            egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

#[test]
fn the_door_and_window_tools_sketch_what_a_click_would_put_in_the_wall() {
    for label in ["Door", "Window"] {
        let mut h = app(BOX);
        tool(&mut h, label);
        // Over the south wall, 3 m from its start: 2900 to the west wall's face and 4900 to the east wall's, where the
        // opening would be.
        h.hover_at(h.state().screen(P2(3000.0, 0.0)));
        h.run();
        let text = painted_text(&h);
        assert!(text.contains(&"2900".to_string()) && text.contains(&"4900".to_string()), "{label}: {text:?}");
        assert!(
            h.state().session.model.elements.values().all(|e| !matches!(e, Element::Door(_) | Element::Window(_))),
            "a sketch isn't an element"
        );
        // In the middle of the room there's no wall to put it in.
        h.hover_at(h.state().screen(P2(4000.0, 3000.0)));
        h.run();
        assert!(!painted_text(&h).contains(&"2900".to_string()), "{label}");
    }
}

#[test]
fn a_door_pointed_at_a_wall_shows_how_far_it_stands_from_the_ends() {
    use bimcraft_core::model::Element;
    let m = app(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}")).state().session.model.clone();
    let Some(Element::Door(o)) = m.elements.values().find(|e| matches!(e, Element::Door(_))) else { panic!("no door") };
    let ctx = egui::Context::default();
    let painter = egui::Painter::new(ctx, egui::LayerId::background(), egui::Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)));
    let v = bimcraft_core::render::View { scale: 0.06, cx: 4000.0, cy: 3000.0, width: 800, height: 600 };
    let spans: Vec<String> = crate::canvas::opening_dimensions(&painter, &m, &v, o).into_iter().map(|(_, text, _)| text).collect();
    assert_eq!(spans, ["1900", "5900"]);
}

#[test]
fn properties_edit_every_selected_element_in_one_step() {
    let mut h = app(SAMPLE);
    let one = find(&h, |e| matches!(e, Element::Door(o) if o.mark == "1"));
    let two = find(&h, |e| matches!(e, Element::Door(o) if o.mark == "2"));
    click(&mut h, P2(2000.0, 0.0));
    click_with(&mut h, P2(4500.0, 4000.0), Modifiers::COMMAND);
    assert!(h.query_by_label("Doors (2)").is_some());
    let steps = h.state().session.history().len();
    // The row is below the palette's fold in this window; screen readers click it all the same.
    h.get_by_label("Flip Hand").click_accesskit();
    h.run();
    assert_eq!((field(&h, one, "flip_hand"), field(&h, two, "flip_hand")), (json!(true), json!(true)));
    assert_eq!(h.state().session.history().len(), steps + 1);
}

#[test]
fn properties_show_0_where_the_json_leaves_a_number_out_and_blank_where_it_is_none() {
    let mut h = app(&format!("{BOX}\nfloor {{}}\nfloor {{\"offset\": 300}}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}"));
    let value = |h: &Harness<BimApp>, label: &str| h.get_by_role_and_label(Role::TextInput, label).value().unwrap_or_default();
    let floors = ids(&h, "floor");
    // A floor on its level, flat: its JSON leaves its offset and its slope out.
    h.state_mut().selection = floors[..1].to_vec();
    h.run();
    assert_eq!((value(&h, "Height Offset From Level"), value(&h, "Slope (°)")), ("0".into(), "0".into()));
    // Both floors: the slopes they share show, their offsets differ.
    h.state_mut().selection = floors;
    h.run();
    assert_eq!((value(&h, "Height Offset From Level"), value(&h, "Slope (°)")), (String::new(), "0".into()));
    // None is not 0: a section with no far clip sees all there is, a plan's top is the level above.
    h.state_mut().selection = ids(&h, "section");
    h.run();
    assert_eq!(value(&h, "Far Clip Offset"), "");
    h.state_mut().selection.clear();
    h.run();
    assert_eq!(value(&h, "Top"), "");
}

#[test]
fn clearing_a_sections_far_clip_offset_lets_it_see_all_there_is() {
    use egui_kittest::kittest::NodeT;
    let mut h = app(&format!(
        "{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000], \"depth\": 4000}}\nsection {{\"start\": [4000, -1000], \"end\": [4000, 7000]}}"
    ));
    // What the field holds, and the hint it shows while blank.
    let clip = |h: &Harness<BimApp>| {
        let n = h.get_by_role_and_label(Role::TextInput, "Far Clip Offset");
        (n.value().unwrap_or_default(), n.accesskit_node().placeholder().filter(|p| !p.is_empty()).map(String::from))
    };
    let sections = ids(&h, "section");
    h.state_mut().selection = sections[..1].to_vec();
    h.run();
    h.get_by_label("Far Clip Offset").focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    key(&mut h, Key::Backspace);
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, sections[0], "depth"), Value::Null);
    assert_eq!(clip(&h), (String::new(), Some("No Clip".into())), "blank, saying so");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(field(&h, sections[0], "depth"), json!(4000.0));
    // Two sections, one clipped: blank because they differ, so no hint that neither clips.
    h.state_mut().selection = sections;
    h.run();
    assert_eq!(clip(&h), (String::new(), None));
}

#[test]
fn the_palette_keeps_its_fields_and_apply_inside_it_whatever_their_labels() {
    use egui_kittest::kittest::{By, NodeT};
    let mut h = app(&format!("{BOX}\nfloor {{}}"));
    h.state_mut().selection = ids(&h, "floor");
    h.run();
    let panel = egui::PanelState::load(&h.ctx, egui::Id::new("dock-left")).map(|s| s.outer_rect).unwrap();
    assert_eq!(panel.width(), 290.0, "the palette at its default width");
    // Height Offset From Level is a long label.
    let fields = h.query_all(By::new().predicate(|n| matches!(n.role(), Role::TextInput | Role::ComboBox)));
    // The palette scrolls, and a floor's has rows enough to run past its foot: only its width is the question.
    for n in fields.filter(|n| n.rect().intersects(panel)).chain([h.get_by_label("Apply")]) {
        let r = n.rect();
        assert!(
            r.min.x >= panel.min.x && r.max.x <= panel.max.x,
            "{:?} at {r:?} runs out of the palette, {panel:?}",
            n.accesskit_node().label()
        );
    }
}

#[test]
fn rotate_turns_the_selection_about_its_middle() {
    let mut h = app(r#"wall {"points": [[0, 0], [4000, 0]]}"#);
    click(&mut h, P2(3000.0, 0.0));
    press(&mut h, "Rotate");
    // From the middle of the wall: the angle's start along it, its end a quarter turn round.
    click(&mut h, P2(4000.0, 0.0));
    click(&mut h, P2(2000.0, 1000.0));
    assert_eq!(walls(&h), [(P2(2000.0, -2000.0), P2(2000.0, 2000.0))]);
}

#[test]
fn mirror_pick_axis_copies_across_the_line_picked() {
    let mut h = app("wall {\"points\": [[0, 0], [4000, 0]]}\ngrid {\"start\": [6000, -3000], \"end\": [6000, 3000]}");
    click(&mut h, P2(2000.0, 0.0));
    press(&mut h, "Mirror Pick Axis");
    click(&mut h, P2(6000.0, 2000.0));
    // Copy is on, as in Revit; the copy runs back the other way, its exterior mirrored.
    assert_eq!(walls(&h), [(P2(0.0, 0.0), P2(4000.0, 0.0)), (P2(8000.0, 0.0), P2(12000.0, 0.0))]);
}

#[test]
fn align_lines_a_wall_face_up_with_a_grid() {
    let mut h = app("wall {\"points\": [[0, 500], [4000, 500]]}\ngrid {\"start\": [-1000, 0], \"end\": [5000, 0], \"name\": \"A\"}");
    topmost(&mut h, "Modify");
    press(&mut h, "Align");
    click(&mut h, P2(2000.0, 0.0));
    // The wall, picked by its lower face: that face goes onto the grid.
    click(&mut h, P2(2000.0, 420.0));
    assert_eq!(walls(&h), [(P2(0.0, 100.0), P2(4000.0, 100.0))]);
}

#[test]
fn trim_extend_to_corner_takes_two_picks() {
    let mut h = app("wall {\"points\": [[0, 0], [4000, 0]]}\nwall {\"points\": [[5000, 1000], [5000, 6000]]}");
    topmost(&mut h, "Modify");
    press(&mut h, "Trim/Extend to Corner");
    click(&mut h, P2(2000.0, 0.0));
    click(&mut h, P2(5000.0, 3000.0));
    assert_eq!(walls(&h), [(P2(0.0, 0.0), P2(5000.0, 0.0)), (P2(5000.0, 0.0), P2(5000.0, 6000.0))]);
}

#[test]
fn split_element_cuts_the_wall_clicked() {
    let mut h = app(BOX);
    topmost(&mut h, "Modify");
    press(&mut h, "Split Element");
    click(&mut h, P2(4000.0, 0.0));
    let w = walls(&h);
    assert_eq!((w.len(), w[0], w[4]), (5, (P2(0.0, 0.0), P2(4000.0, 0.0)), (P2(4000.0, 0.0), P2(8000.0, 0.0))));
}

#[test]
fn offset_copies_a_wall_to_the_cursors_side() {
    let mut h = app(r#"wall {"points": [[0, 0], [4000, 0]]}"#);
    topmost(&mut h, "Modify");
    press(&mut h, "Offset");
    click(&mut h, P2(2000.0, 60.0));
    assert_eq!(walls(&h), [(P2(0.0, 0.0), P2(4000.0, 0.0)), (P2(0.0, 1000.0), P2(4000.0, 1000.0))]);
}

#[test]
fn aligned_dimensions_pick_parallel_references_then_place_the_line() {
    let mut h = app(BOX);
    press(&mut h, "Annotate");
    press(&mut h, "Aligned");
    assert!(h.query_by_label("Modify | Place Dimensions").is_some());
    // The face nearest the cursor: the left wall's inner face, then the right wall's; then where the line goes.
    click(&mut h, P2(95.0, 3000.0));
    click(&mut h, P2(7905.0, 3000.0));
    click(&mut h, P2(4000.0, -1000.0));
    let dims = ids(&h, "dimension");
    assert_eq!((dims.len(), field(&h, dims[0], "values")), (1, json!([7800.0])));
    assert_eq!(h.state().tool, Tool::Dimension(DimMode::Aligned, Vec::new()), "the tool stays on for the next one");
    // A wall across the first isn't picked; centre lines are, where the cursor is on them.
    click(&mut h, P2(0.0, 3000.0));
    click(&mut h, P2(4000.0, 0.0));
    click(&mut h, P2(8000.0, 3000.0));
    click(&mut h, P2(4000.0, 3000.0));
    let dims = ids(&h, "dimension");
    assert_eq!((dims.len(), field(&h, dims[1], "values")), (2, json!([8000.0])));
    // Esc drops the references picked, then leaves the tool.
    click(&mut h, P2(0.0, 3000.0));
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Dimension(DimMode::Aligned, Vec::new()));
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Modify);
}

#[test]
fn tag_by_category_tags_the_door_clicked_and_tag_all_the_rest() {
    let mut h = app(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}\nwindow {{\"at\": [8000, 3000]}}"));
    press(&mut h, "Annotate");
    press(&mut h, "Tag by Category");
    assert!(h.query_by_label("Modify | Tag").is_some());
    click(&mut h, P2(2000.0, 0.0));
    let tags = ids(&h, "tag");
    assert_eq!((tags.len(), field(&h, tags[0], "text")), (1, json!("1")));
    // Any element: a wall names its type, or the field the Options Bar asks for; one without it says so and makes no tag.
    click(&mut h, P2(5000.0, 0.0));
    let tags_now = ids(&h, "tag");
    assert_eq!(tags_now.len(), 2);
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 0.0)));
    assert_eq!(field(&h, tags_now[1], "text"), field(&h, wall, "type"));
    type_into(&mut h, "Label:", "Height");
    click(&mut h, P2(5000.0, 0.0));
    let tags_now = ids(&h, "tag");
    assert_eq!((tags_now.len(), field(&h, tags_now[2], "text"), field(&h, tags_now[2], "label")), (3, json!("3000"), json!("Height")));
    type_into(&mut h, "Label:", "Colour");
    click(&mut h, P2(5000.0, 0.0));
    assert_eq!(ids(&h, "tag").len(), 3);
    assert!(h.state().status.error && h.state().status.text.contains("Colour"), "{:?}", h.state().status);
    type_into(&mut h, "Label:", "");
    // Tag All, back on the Annotate tab: the window too.
    press(&mut h, "Annotate");
    press(&mut h, "Tag All");
    assert_eq!(ids(&h, "tag").len(), 4);
    // A tag drags like anything else; it sat outside, the door swinging in.
    key(&mut h, Key::Escape);
    drag(&mut h, P2(2000.0, -500.0), P2(3000.0, -500.0));
    assert_eq!(field(&h, tags[0], "at"), json!([3000.0, -500.0]));
}

#[test]
fn the_default_3d_view_opens_from_the_toolbar_and_clicks_pick_what_it_shows() {
    let mut h = app(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}"));
    press(&mut h, "Default 3D View");
    assert_eq!(h.state().sheet, Sheet::View3D);
    assert!(h.query_by_label("3D View: {3D}").is_some(), "Properties shows the view");
    assert!(h.query_by_label("3D Views").is_some(), "the browser lists it");
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 0.0)));
    let door = find(&h, |e| matches!(e, Element::Door(_)));
    let at = h.state().screen3([5000.0, -100.0, 1500.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [wall], "the south wall's face");
    assert!(h.query_by_label("Modify | Walls").is_some());
    let at = h.state().screen3([2000.0, -25.0, 1000.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [door], "the door, through its opening");
    let corner = h.state().orbit.rect.min + vec2(4.0, 4.0);
    click_at(&mut h, corner);
    assert!(h.state().selection.is_empty(), "the background selects nothing");
}

#[test]
fn a_click_made_on_the_move_in_3d_picks_what_was_under_the_press_and_leaves_the_camera_alone() {
    let mut h = app_at(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}"), 0.05);
    press(&mut h, "Default 3D View");
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 0.0)));
    let before = h.state().orbit.camera;
    let on = h.state().screen3([5000.0, -100.0, 1500.0]);
    flick(&mut h, on, on + vec2(70.0, 30.0), Modifiers::NONE);
    assert_eq!(h.state().selection, [wall], "the south wall's face, where the button went down");
    assert_eq!(h.state().orbit.camera, before, "the view did not turn");
}

#[test]
fn in_3d_a_drag_orbits_the_middle_button_pans_and_the_wheel_zooms() {
    let mut h = app(BOX);
    press(&mut h, "Default 3D View");
    let cam = |h: &Harness<BimApp>| h.state().orbit.camera.unwrap();
    let (start, mid) = (cam(&h), h.state().orbit.rect.center());
    // 100 px to the right: the model turns 40° with it.
    drag_screen(&mut h, mid, mid + vec2(100.0, 0.0), PointerButton::Primary);
    let turned = cam(&h);
    assert!((turned.azimuth - (start.azimuth - 40.0)).abs() < 0.5 && turned.elevation == start.elevation, "{turned:?}");
    drag_screen(&mut h, mid, mid + vec2(0.0, 50.0), PointerButton::Middle);
    let panned = cam(&h);
    assert!(panned.target != turned.target && panned.azimuth == turned.azimuth, "{panned:?}");
    // Shift and the middle button orbit too, as in Revit: dragging down looks from higher up.
    h.event(Event::ModifiersChanged(Modifiers::SHIFT));
    drag_screen(&mut h, mid, mid + vec2(0.0, 50.0), PointerButton::Middle);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run();
    let raised = cam(&h);
    assert!(raised.elevation > panned.elevation + 15.0, "{raised:?}");
    h.hover_at(mid);
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, 120.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.run();
    let zoomed = cam(&h);
    assert!(zoomed.scale > raised.scale * 1.2, "{zoomed:?}");
    // Zoom to Fit: the whole model again, from where the camera is now.
    press(&mut h, "Zoom to Fit");
    assert!(cam(&h).scale < zoomed.scale && cam(&h).azimuth == zoomed.azimuth);
}

#[test]
fn the_section_tool_draws_a_section_whose_view_opens_from_the_browser_or_a_double_click() {
    let mut h = app_at(BOX, 0.05);
    press(&mut h, "View");
    press(&mut h, "Section");
    assert!(h.query_by_label("Modify | Section").is_some());
    // Drawn west to east across the box: it looks north.
    click(&mut h, P2(-1000.0, 3000.0));
    click(&mut h, P2(9000.0, 3000.0));
    let section = ids(&h, "section");
    assert_eq!((section.len(), field(&h, section[0], "looking")), (1, json!([0.0, 1.0])));
    assert!(h.query_by_label("Sections (Building Section)").is_some());
    browse(&mut h, "Section 1");
    assert_eq!(h.state().sheet, Sheet::Section(section[0]));
    // In the section, points run along its line from its start, and up: the west wall is cut at 1000.
    let west = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 6000.0)));
    click(&mut h, P2(1060.0, 1500.0));
    assert_eq!(h.state().selection, [west]);
    // Back on the plan, a double click on the section's line opens it again.
    topmost(&mut h, "Level 1");
    assert_eq!(h.state().sheet, Sheet::Plan(100000));
    double_click(&mut h, P2(-500.0, 3000.0));
    assert_eq!(h.state().sheet, Sheet::Section(section[0]));
}

#[test]
fn the_elevation_tool_looks_at_the_building_from_where_it_is_clicked() {
    let mut h = app(BOX);
    press(&mut h, "View");
    press(&mut h, "Elevation");
    click(&mut h, P2(4000.0, 7000.0));
    let made = ids(&h, "elevation");
    assert_eq!((field(&h, made[0], "name"), field(&h, made[0], "looking")), (json!("North"), json!([0.0, -1.0])));
    assert!(h.query_by_label("Elevations (Building Elevation)").is_some());
}

#[test]
fn sheets_take_views_from_the_view_tab_and_viewports_move_and_open() {
    let mut h = app_at(BOX, 0.05);
    press(&mut h, "View");
    press(&mut h, "Sheet");
    let sheets = ids(&h, "sheet");
    assert_eq!((sheets.len(), field(&h, sheets[0], "number")), (1, json!("A101")));
    assert_eq!(h.state().sheet, Sheet::Paper(sheets[0]));
    assert!(h.query_by_label("Sheets (all)").is_some());
    assert_eq!(h.query_all_by_label("A101 - Unnamed").count(), 2, "its entry in the browser, and its tab");
    // Place View offers the views on no sheet yet.
    press(&mut h, "View");
    press(&mut h, "Place View");
    press(&mut h, "Floor Plan: Level 1");
    let vp = ids(&h, "viewport");
    assert_eq!((vp.len(), field(&h, vp[0], "view")), (1, json!(100000)));
    // Points on a sheet are paper mm: the viewport, picked anywhere over it, drags 40 mm right.
    let at = field(&h, vp[0], "at");
    let (x, y) = (at[0].as_f64().unwrap(), at[1].as_f64().unwrap());
    drag(&mut h, P2(x, y), P2(x + 40.0, y));
    assert_eq!(field(&h, vp[0], "at"), json!([x + 40.0, y]));
    assert!(h.query_by_label("Modify | Viewports").is_some());
    // A double click on it opens its view.
    double_click(&mut h, P2(x + 40.0, y));
    assert_eq!(h.state().sheet, Sheet::Plan(100000));
}

#[test]
fn columns_go_where_clicked_up_to_the_options_bars_height() {
    let mut h = app(BOX);
    tool(&mut h, "Column");
    assert!(h.query_by_label("Modify | Place Column").is_some());
    click(&mut h, P2(4000.0, 3000.0));
    let col = ids(&h, "column");
    assert_eq!((col.len(), field(&h, col[0], "at"), field(&h, col[0], "top_level")), (1, json!([4000.0, 3000.0]), json!(100001)));
    // Unconnected, at a height typed: its own Height, not the Wall tool's.
    h.get_by_role_and_label(Role::ComboBox, "Height:").click();
    h.run();
    press(&mut h, "Unconnected");
    h.get_by_role_and_label(Role::TextInput, "Height:").focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by_role_and_label(Role::TextInput, "Height:").type_text("2500");
    h.run();
    click(&mut h, P2(2000.0, 3000.0));
    let col = ids(&h, "column");
    assert_eq!((field(&h, col[1], "top_level"), field(&h, col[1], "height")), (json!(null), json!(2500.0)));
    tool(&mut h, "Wall");
    assert!(h.query_by_role_and_label(Role::TextInput, "Height:").is_none(), "the Wall tool still goes up to the next level");
    // Selected, its palette.
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    click(&mut h, P2(4000.0, 3000.0));
    assert_eq!(h.state().selection, [col[0]]);
    assert!(h.query_by_label("Modify | Columns").is_some() && h.query_by_label("Top Level").is_some());
}

#[test]
fn stairs_start_where_clicked_and_climb_toward_the_second_click() {
    let mut h = app(BOX);
    tool(&mut h, "Stair");
    assert!(h.query_by_label("Modify | Create Stair").is_some());
    type_into(&mut h, "Width:", "1200");
    click(&mut h, P2(1000.0, 1000.0));
    assert!(ids(&h, "stair").is_empty(), "the first click starts it");
    // Past the end of a run of all its risers: one straight run.
    click(&mut h, P2(1000.0, 6000.0));
    let st = ids(&h, "stair");
    assert_eq!((st.len(), field(&h, st[0], "angle"), field(&h, st[0], "width")), (1, json!(90.0), json!(1200.0)));
    assert_eq!(
        (field(&h, st[0], "risers"), field(&h, st[0], "top_level"), field(&h, st[0], "landings")),
        (json!(17), json!(100001), Value::Null),
        "up to Level 2, in risers of 180 at most"
    );
    assert_eq!(ids(&h, "railing").len(), 2, "with its railings, as the Options Bar says");
    assert_eq!(h.state().tool, Tool::Stair(Vec::new()), "ready for the next one");
}

#[test]
fn a_stair_run_ends_where_clicked_and_the_next_turns_toward_the_third_click() {
    let mut h = app(BOX);
    tool(&mut h, "Stair");
    press(&mut h, "Railings");
    // 7 treads north: 8 risers, 9 to go; then to the right of the landing.
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(1000.0, 2960.0));
    assert!(ids(&h, "stair").is_empty(), "risers are left: the next run waits");
    click(&mut h, P2(3000.0, 3500.0));
    let st = ids(&h, "stair");
    assert_eq!((st.len(), field(&h, st[0], "landings")), (1, json!([{"at": 8.0, "turn": -90.0}])));
    assert!(ids(&h, "railing").is_empty(), "Railings off");
}

#[test]
fn ramps_climb_at_the_options_bars_slope_and_turn_at_landings() {
    let mut h = app(BOX);
    tool(&mut h, "Ramp");
    assert!(h.query_by_label("Modify | Create Ramp").is_some());
    h.get_by_role_and_label(Role::ComboBox, "Height:").click();
    h.run();
    press(&mut h, "Unconnected");
    h.get_by_role_and_label(Role::TextInput, "Height:").focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by_role_and_label(Role::TextInput, "Height:").type_text("600");
    h.run();
    type_into(&mut h, "Slope:", "10");
    // 360 mm up its first run, 3.6 m at 10%; then back beside it, to its left.
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(4600.0, 1000.0));
    assert!(ids(&h, "ramp").is_empty());
    click(&mut h, P2(1000.0, 2200.0));
    let r = ids(&h, "ramp");
    assert_eq!((r.len(), field(&h, r[0], "height"), field(&h, r[0], "length")), (1, json!(600.0), json!(6000.0)));
    assert_eq!(field(&h, r[0], "landings"), json!([{"at": 360.0, "turn": 180.0}]));
    assert_eq!(ids(&h, "railing").len(), 2, "with its railings");
}

#[test]
fn the_railing_tool_follows_a_stair_picked_or_a_path_clicked() {
    let mut h = app(&format!("{BOX}\nstair {{\"start\": [1000, 1000], \"end\": [1000, 2000]}}"));
    let stair = ids(&h, "stair")[0];
    tool(&mut h, "Railing");
    assert!(h.query_by_label("Modify | Create Railing Path").is_some());
    // The stair picked: a railing down each side.
    click(&mut h, P2(1000.0, 3000.0));
    let on: Vec<Id> = ids(&h, "railing");
    assert_eq!(on.len(), 2);
    assert!(on.iter().all(|r| field(&h, *r, "host") == json!(stair)));
    // A path clicked away from it, then Enter: one railing along it, on the plan's level.
    click(&mut h, P2(4000.0, 4000.0));
    click(&mut h, P2(7000.0, 4000.0));
    click(&mut h, P2(7000.0, 5500.0));
    key(&mut h, Key::Enter);
    let path = ids(&h, "railing")[2];
    assert_eq!(field(&h, path, "path"), json!([[4000.0, 4000.0], [7000.0, 4000.0], [7000.0, 5500.0]]));
}

#[test]
fn the_roof_tool_roofs_the_walls_of_the_plan_on_the_level_above() {
    let mut h = app(BOX);
    tool(&mut h, "Roof");
    assert!(h.query_by_label("Modify | Create Roof Footprint").is_some());
    type_into(&mut h, "Slope:", "40");
    press(&mut h, "Gable");
    click(&mut h, P2(4000.0, 3000.0));
    let roof = ids(&h, "roof");
    assert_eq!(roof.len(), 1);
    assert_eq!(
        (field(&h, roof[0], "level"), field(&h, roof[0], "slope"), field(&h, roof[0], "gable")),
        (json!(100001), json!(40.0), json!(true))
    );
    assert_eq!(field(&h, roof[0], "boundary")[0], json!([-600.0, -600.0]), "500 mm past the walls' faces");
    assert_eq!(h.state().selection, roof, "finished, the roof is selected");
    assert!(h.query_by_label("Modify | Roofs").is_some() && h.query_by_label("Slope (°)").is_some());
}

#[test]
fn the_roof_tool_leaves_a_courtyard_open_inside_a_loop_of_walls_standing_free() {
    let mut h = app(r#"wall {"points": [[0, 0], [16000, 0], [16000, 16000], [0, 16000]], "closed": true}
wall {"points": [[6000, 6000], [10000, 6000], [10000, 10000], [6000, 10000]], "closed": true}"#);
    tool(&mut h, "Roof");
    click(&mut h, P2(3000.0, 8000.0));
    let roof = ids(&h, "roof");
    assert_eq!(roof.len(), 1);
    let courtyard = json!([[[6600.0, 6600.0], [6600.0, 9400.0], [9400.0, 9400.0], [9400.0, 6600.0]]]);
    assert_eq!(field(&h, roof[0], "courtyards"), courtyard, "500 mm past the courtyard walls' faces");
}

#[test]
fn files_the_system_opens_open_as_from_the_menu_and_quit_asks_first() {
    // macOS hands the app a file Finder opened, and Quit from the Dock, as Apple events, not arguments.
    use crate::OsEvent;
    use std::{cell::RefCell, rc::Rc};
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-os-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (house, box_) = (dir.join("house.bimcraft"), dir.join("box.bimcraft"));
    for (script, path) in [(SAMPLE, &house), (BOX, &box_)] {
        let mut s = Session::new();
        s.script(script).unwrap();
        s.execute("save", &json!({"path": path})).unwrap();
    }
    let mut h = app(BOX);
    let inbox: Rc<RefCell<Vec<OsEvent>>> = Rc::default();
    let queue = Rc::clone(&inbox);
    h.state_mut().os_events = Some(Box::new(move || queue.borrow_mut().drain(..).collect()));
    let wall = |h: &mut Harness<BimApp>| h.state_mut().session.execute("wall", &json!({"points": [[0, 0], [0, 9000]]})).unwrap();
    wall(&mut h);
    inbox.borrow_mut().push(OsEvent::Open(house.clone()));
    h.run();
    assert_eq!(walls(&h).len(), 5, "unsaved changes: it asks first");
    press(&mut h, "Don't Save");
    assert_eq!(walls(&h).len(), 7);
    assert_eq!(h.state().session.path.as_deref(), Some(house.as_path()));
    inbox.borrow_mut().push(OsEvent::Open(box_.clone()));
    h.run();
    assert_eq!(walls(&h).len(), 4, "nothing unsaved, so straight away, the events still heard after the house opened");
    wall(&mut h);
    inbox.borrow_mut().push(OsEvent::Quit);
    h.run();
    assert!(h.query_by_label_contains("Save changes").is_some(), "Quit asks about unsaved changes, as closing the window does");
    press(&mut h, "Cancel");
    assert_eq!(walls(&h).len(), 5);
    let _ = std::fs::remove_dir_all(&dir);
}

/// File > Open Recent: a submenu's label reads out its arrow too, "Open Recent ⏵".
fn open_recent(h: &mut Harness<BimApp>) {
    h.get_by_label_contains("Open Recent").click();
    h.run();
}

/// Where a test keeps its recent files, and a folder with two projects to open.
fn recent_rig(tag: &str) -> (Harness<'static, BimApp>, std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-recent-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = app(SAMPLE);
    let file = dir.join("recent.json");
    h.state_mut().keep_recent_in(file.clone());
    (h, dir, file)
}

#[test]
fn a_window_told_of_no_file_keeps_no_recent_files_and_offers_none() {
    let mut h = app(SAMPLE);
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-recent-none-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    assert!(h.state_mut().save_to(&dir.join("a.bimcraft")));
    assert!(h.state().recent.paths().is_empty(), "so no test writes the user's own list");
    press(&mut h, "File");
    assert!(h.query_by_label("Open Recent").is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn saved_and_opened_projects_are_listed_in_open_recent_newest_first_and_exports_are_not() {
    let (mut h, dir, file) = recent_rig("list");
    for name in ["a.bimcraft", "b.bimcraft", "plan.png", "house.pdf", "house.ifc"] {
        assert!(h.state_mut().save_to(&dir.join(name)), "{name}");
    }
    // An IFC export leaves the project unsaved and is not a file to come back to.
    let names = |h: &Harness<BimApp>| h.state().recent.labels();
    assert_eq!(names(&h), ["b.bimcraft", "a.bimcraft"]);
    // Opening one moves it to the top; an IFC file that is opened (imported) is listed too.
    assert!(h.state_mut().open_path(&dir.join("a.bimcraft")));
    assert!(h.state_mut().open_path(&dir.join("house.ifc")));
    assert!(h.state_mut().open_path(&dir.join("a.bimcraft")));
    assert_eq!(names(&h), ["a.bimcraft", "house.ifc", "b.bimcraft"], "once each, though the window was started over each time");
    // The list is in the file, for the next time the app starts.
    let mut next = BimApp::new(Session::new());
    next.keep_recent_in(file);
    assert_eq!(next.recent.labels(), ["a.bimcraft", "house.ifc", "b.bimcraft"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn choosing_a_recent_file_opens_it_after_asking_about_unsaved_changes() {
    let (mut h, dir, _) = recent_rig("choose");
    assert!(h.state_mut().save_to(&dir.join("sample.bimcraft")));
    h.state_mut().session.execute("wall", &json!({"points": [[0, 0], [4000, 0]]})).unwrap();
    let walls_now = walls(&h).len();
    let choose = |h: &mut Harness<BimApp>| {
        press(h, "File");
        open_recent(h);
        press(h, "sample.bimcraft");
    };
    choose(&mut h);
    press(&mut h, "Cancel");
    assert_eq!(walls(&h).len(), walls_now, "cancelled: nothing opened");
    choose(&mut h);
    press(&mut h, "Don't Save");
    assert_eq!(walls(&h).len(), walls_now - 1, "the saved project, without the wall added since");
    assert_eq!(h.state().session.path.as_deref(), Some(dir.join("sample.bimcraft").as_path()));
    assert_eq!(h.state().recent.labels(), ["sample.bimcraft"], "the list survived starting the window over");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_recent_file_that_has_gone_is_said_and_dropped_and_clear_empties_the_list() {
    let (mut h, dir, file) = recent_rig("gone");
    for name in ["gone.bimcraft", "kept.bimcraft"] {
        assert!(h.state_mut().save_to(&dir.join(name)));
    }
    std::fs::remove_file(dir.join("gone.bimcraft")).unwrap();
    press(&mut h, "File");
    open_recent(&mut h);
    press(&mut h, "gone.bimcraft");
    assert!(h.state().status.error, "{}", h.state().status.text);
    assert_eq!(h.state().recent.labels(), ["kept.bimcraft"]);
    assert_eq!(listed_in(&file), 1, "and the file says so");
    press(&mut h, "File");
    open_recent(&mut h);
    press(&mut h, "Clear Recent Files");
    assert!(h.state().recent.paths().is_empty());
    assert_eq!(listed_in(&file), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

fn listed_in(file: &std::path::Path) -> usize {
    crate::recent::Recent::load(file).paths().len()
}

#[test]
fn opening_an_ifc_file_imports_it_and_says_what_came_in() {
    let mut h = app(SAMPLE);
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-ifc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ifc = dir.join("house.ifc");
    assert!(h.state_mut().save_to(&ifc));
    assert!(h.state_mut().open_path(&ifc));
    h.run();
    assert_eq!(ids(&h, "wall").len(), 7);
    assert_eq!(h.state().session.path, None, "an import is a new project: Save asks where");
    let note = h.state().status.text.clone();
    assert!(!h.state().status.error && note.contains("7 walls") && note.contains("house.ifc"), "{note}");
    assert!(h.query_by_label("Note").is_some(), "shown over the view until dismissed");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dark_mode_turns_the_window_and_the_drawing_dark() {
    use bimcraft_core::render::{PAPER, Rgb};
    let mut h = app(BOX);
    assert!(!crate::style::dark(), "light, as the system's theme is here");
    press(&mut h, "View");
    press(&mut h, "Dark Mode");
    assert!(!crate::style::dark(), "opening the menu leaves the current theme in place");
    pick(&mut h, Role::RadioButton, "Dark");
    assert!(crate::style::dark() && h.ctx.theme() == egui::Theme::Dark);
    // Ink light on a dark ground, the grid still blue.
    let (ground, ink, grid) = (crate::canvas::color(PAPER), crate::canvas::color(Rgb(25, 25, 25)), crate::canvas::color(Rgb(60, 100, 150)));
    assert!(ground.r() < 40 && ink.r() > 200 && grid.b() > grid.r() + 40, "{ground:?} {ink:?} {grid:?}");
    assert_eq!(crate::style::pal().surface, crate::style::DARK.surface);
    press(&mut h, "Dark Mode");
    pick(&mut h, Role::RadioButton, "Light");
    assert!(!crate::style::dark() && crate::canvas::color(PAPER) == egui::Color32::WHITE);
}

#[test]
fn dark_ui_with_light_canvas_is_shared_with_settings_and_survives_new_projects() {
    use bimcraft_core::render::{PAPER, Rgb};
    use egui_kittest::kittest::NodeT;
    let mut h = app(BOX);
    press(&mut h, "View");
    press(&mut h, "Dark Mode");
    pick(&mut h, Role::RadioButton, "Dark UI, Light Canvas");
    assert!(crate::style::dark() && h.ctx.theme() == egui::Theme::Dark);
    assert_eq!(crate::style::pal().surface, crate::style::DARK.surface);
    assert_eq!(crate::canvas::color(PAPER), egui::Color32::WHITE);
    assert_eq!(crate::canvas::color(Rgb(25, 25, 25)), egui::Color32::from_gray(25));
    assert!(h.query_by_label("Dark UI, Light Canvas").is_none(), "menu closes after choosing");
    press(&mut h, "Settings");
    assert_eq!(
        h.get_by_role_and_label(Role::RadioButton, "Dark UI, Light Canvas").accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True)
    );
    pick(&mut h, Role::RadioButton, "Dark");
    assert!(crate::canvas::color(PAPER).r() < 40);
    pick(&mut h, Role::RadioButton, "Dark UI, Light Canvas");
    press(&mut h, "Close");
    h.state_mut().reopened();
    h.run();
    assert!(crate::style::dark());
    assert_eq!(crate::canvas::color(PAPER), egui::Color32::WHITE);
    press(&mut h, "View");
    press(&mut h, "Dark Mode");
    pick(&mut h, Role::RadioButton, "As the System");
    assert_eq!(h.ctx.options(|o| o.theme_preference), egui::ThemePreference::System);
    assert!(!crate::style::dark());
}

#[test]
fn text_notes_go_where_clicked_in_plans_sections_and_sheets() {
    let mut h = app_at(&format!("{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}\nsheet {{}}"), 0.05);
    let (section, sheet) = (ids(&h, "section")[0], ids(&h, "sheet")[0]);
    press(&mut h, "Annotate");
    press(&mut h, "Text");
    assert!(h.query_by_label("Modify | Place Text").is_some());
    // With nothing typed in the Options Bar, a click opens the note on the canvas; left empty, it makes nothing.
    click(&mut h, P2(2000.0, 2000.0));
    assert!(h.state().note_edit.is_some());
    key(&mut h, Key::Escape);
    assert!(ids(&h, "text").is_empty() && h.state().note_edit.is_none());
    type_into(&mut h, "Text:", "Tiled floor");
    click(&mut h, P2(2000.0, 2000.0));
    let notes = ids(&h, "text");
    assert_eq!(
        (field(&h, notes[0], "text"), field(&h, notes[0], "at"), field(&h, notes[0], "view")),
        (json!("Tiled floor"), json!([2000.0, 2000.0]), json!(100000))
    );
    // The tool goes on working in a section, along it and up, and on a sheet, in paper mm.
    browse(&mut h, "Section 1");
    assert_eq!((h.state().sheet, &h.state().tool), (Sheet::Section(section), &Tool::Text));
    click(&mut h, P2(3000.0, 2500.0));
    h.get_by_label("A101 - Unnamed").click_accesskit();
    h.run();
    click(&mut h, P2(50.0, 50.0));
    let notes = ids(&h, "text");
    assert_eq!((field(&h, notes[1], "view"), field(&h, notes[1], "at")), (json!(section), json!([3000.0, 2500.0])));
    assert_eq!((field(&h, notes[2], "view"), field(&h, notes[2], "at")), (json!(sheet), json!([50.0, 50.0])));
}

#[test]
fn angular_and_radial_dimensions_and_spot_elevations_from_the_annotate_tab() {
    // Walls 3 to 6, a slanted one 7, a round column 8, the floor 9.
    let mut h = app(&format!(
        "{BOX}\nwall {{\"points\": [[0, 0], [3000, 3000]], \"type\": \"Interior - 100mm\"}}\ncolumn {{\"at\": [6000, 4000], \"type\": \"Round 300\"}}\nfloor {{}}"
    ));
    press(&mut h, "Annotate");
    press(&mut h, "Angular");
    assert!(h.query_by_label("Modify | Place Dimensions").is_some());
    // The south wall, the slanted one, then where the arc goes.
    click(&mut h, P2(4000.0, 0.0));
    click(&mut h, P2(1500.0, 1500.0));
    click(&mut h, P2(2500.0, 1000.0));
    let dims = ids(&h, "dimension");
    assert_eq!((field(&h, dims[0], "kind"), field(&h, dims[0], "values")), (json!("angular"), json!([45.0])));
    assert_eq!(h.state().tool, Tool::Angular(Vec::new()), "ready for the next one");
    press(&mut h, "Annotate");
    press(&mut h, "Radial");
    click(&mut h, P2(6150.0, 4000.0));
    click(&mut h, P2(6800.0, 4800.0));
    let dims = ids(&h, "dimension");
    assert_eq!((field(&h, dims[1], "kind"), field(&h, dims[1], "values")), (json!("radial"), json!([150.0])));
    press(&mut h, "Annotate");
    press(&mut h, "Spot Elevation");
    click(&mut h, P2(5000.0, 2000.0));
    let spots = ids(&h, "spot");
    assert_eq!((field(&h, spots[0], "host"), field(&h, spots[0], "elevation")), (json!(100008), json!(0.0)));
}

#[test]
fn crop_view_and_its_grips_frame_plans_and_sections() {
    let mut h = app(&format!("{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}"));
    let section = ids(&h, "section")[0];
    let crop_view = |h: &mut Harness<BimApp>| {
        // The ribbon's button; Properties has a Crop View row too.
        h.get_by_role_and_label(Role::Button, "Crop View").click();
        h.run();
    };
    press(&mut h, "View");
    crop_view(&mut h);
    // All the plan shows (the walls, the section's line), a metre past it.
    assert_eq!(field(&h, 100000, "crop"), json!([[-2000.0, -1100.0], [10000.0, 7100.0]]));
    // The east edge, by its grip, dragged west.
    drag(&mut h, P2(10000.0, 3000.0), P2(4000.0, 3000.0));
    assert_eq!(field(&h, 100000, "crop"), json!([[-2000.0, -1100.0], [4000.0, 7100.0]]));
    assert!(h.state().selection.is_empty(), "the grip took the drag, not a selection box");
    crop_view(&mut h);
    assert_eq!(field(&h, 100000, "crop"), Value::Null);
    // A section uncropped has no crop region to drag: it shows all there is along its line.
    browse(&mut h, "Section 1");
    assert_eq!(field(&h, section, "crop"), Value::Null);
    assert!(bimcraft_core::render::crop_region(&h.state().session.model, section).is_none());
    // Cropped, its top dragged down; its start, dragged along its line: what's on screen stays put.
    crop_view(&mut h);
    assert_eq!(field(&h, section, "crop"), json!([-600.0, 3600.0]));
    drag(&mut h, P2(5000.0, 3600.0), P2(5000.0, 2000.0));
    assert_eq!(field(&h, section, "crop"), json!([-600.0, 2000.0]));
    let before = h.state().screen(P2(2000.0, 700.0));
    drag(&mut h, P2(0.0, 700.0), P2(2000.0, 700.0));
    assert_eq!(field(&h, section, "start"), json!([1000.0, 3000.0]));
    assert!((h.state().screen(P2(0.0, 700.0)) - before).length() < 1.0);
    // The west wall now lies before its start: cropped out, nothing is there to pick.
    let west = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 6000.0)));
    h.get_by_label("Zoom to Fit").click();
    h.run();
    click(&mut h, P2(-1000.0, 1000.0));
    assert!(h.state().selection.is_empty(), "{:?}", h.state().selection);
    // Uncropped, the section shows it again, past its line's start.
    crop_view(&mut h);
    assert_eq!(field(&h, section, "crop"), Value::Null);
    h.get_by_label("Zoom to Fit").click();
    h.run();
    click(&mut h, P2(-1000.0, 1000.0));
    assert_eq!(h.state().selection, [west]);
}

#[test]
fn a_box_dragged_in_a_section_selects_as_in_a_plan() {
    let mut h = app(&format!("{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}"));
    browse(&mut h, "Section 1");
    let west = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 6000.0)));
    let north = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(8000.0, 6000.0)));
    // Left to right: what lies wholly inside, the west wall's cut and not the north wall seen past it.
    drag(&mut h, P2(600.0, -300.0), P2(1400.0, 3300.0));
    assert_eq!(h.state().selection, [west]);
    // Right to left: what it crosses too.
    key(&mut h, Key::Escape);
    drag(&mut h, P2(1400.0, 1400.0), P2(600.0, 1600.0));
    let mut both = vec![west, north];
    both.sort();
    assert_eq!(h.state().selection, both);
}

#[test]
fn a_plans_view_range_is_in_its_properties() {
    let mut h = app(BOX);
    assert!(h.query_by_label("View Range").is_some());
    type_into(&mut h, "Cut Plane", "1500");
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, 100000, "view_range"), json!({"top": null, "cut": 1500.0, "bottom": 0.0, "depth": 0.0}));
    type_into(&mut h, "Top", "2600");
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, 100000, "view_range")["top"], json!(2600.0));
    // Cleared, back to the level above.
    h.get_by_label("Top").focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    key(&mut h, Key::Backspace);
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, 100000, "view_range")["top"], Value::Null);
}

#[test]
fn place_view_puts_schedules_and_the_3d_view_on_sheets() {
    let mut h = app_at(&format!("{BOX}\nroom {{\"point\": [4000, 3000], \"name\": \"Hall\"}}"), 0.05);
    press(&mut h, "View");
    press(&mut h, "Sheet");
    for view in ["Schedule: Room Schedule", "3D View: {3D}"] {
        press(&mut h, "View");
        press(&mut h, "Place View");
        press(&mut h, view);
    }
    let (schedules, views) = (ids(&h, "schedule"), ids(&h, "view3d"));
    assert_eq!((schedules.len(), views.len(), ids(&h, "viewport").len()), (1, 1, 2));
    // Placed, they're no longer offered.
    press(&mut h, "View");
    press(&mut h, "Place View");
    assert!(h.query_by_label("Schedule: Room Schedule").is_none() && h.query_by_label("Schedule: Door Schedule").is_some());
    key(&mut h, Key::Escape);
    // A double click on the schedule opens it.
    let vp = ids(&h, "viewport")[0];
    let at = field(&h, vp, "at");
    double_click(&mut h, P2(at[0].as_f64().unwrap(), at[1].as_f64().unwrap()));
    assert_eq!(h.state().sheet, Sheet::Schedule("room"));
}

#[test]
fn project_units_show_and_read_lengths_in_feet_and_inches() {
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Project Units");
    press(&mut h, "Feet and inches");
    assert_eq!(h.state().session.model.units.name(), "ft-in");
    // The bottom wall, 8000 mm long, in Properties; a base offset typed in feet and inches.
    click(&mut h, P2(4000.0, 0.0));
    assert!(h.query_by_label("26'-2 15/16\"").is_some());
    type_into(&mut h, "Base Offset", "1' 6\"");
    key(&mut h, Key::Enter);
    let wall = h.state().selection[0];
    assert_eq!(field(&h, wall, "base_offset"), json!(457.2));
}

#[test]
fn lengths_typed_while_drawing_read_in_the_project_units() {
    let mut h = app("units {\"length\": \"ft-in\"}");
    tool(&mut h, "Wall");
    click(&mut h, P2(0.0, 0.0));
    h.hover_at(h.state().screen(P2(1000.0, 20.0)));
    h.run();
    // Feet, inches and a fraction: everything after the first digit is the length's, not a shortcut.
    h.event(Event::Text("10' 6 1/2\"".into()));
    h.run();
    assert_eq!(h.state().tool, Tool::Wall(vec![P2(0.0, 0.0)]), "no letter or mark started a tool");
    key(&mut h, Key::Enter);
    key(&mut h, Key::Enter);
    assert_eq!(walls(&h), [(P2(0.0, 0.0), P2(3213.1, 0.0))]);
    // A metre, typed with its unit, in a project in feet and inches.
    tool(&mut h, "Wall");
    click(&mut h, P2(0.0, 2000.0));
    h.hover_at(h.state().screen(P2(1000.0, 2000.0)));
    h.run();
    h.event(Event::Text("1m".into()));
    h.run();
    key(&mut h, Key::Enter);
    key(&mut h, Key::Enter);
    assert_eq!(walls(&h)[1], (P2(0.0, 2000.0), P2(1000.0, 2000.0)));
}

#[test]
fn the_interface_speaks_portuguese() {
    let mut h = app(BOX);
    press(&mut h, "View");
    press(&mut h, "User Interface");
    press(&mut h, "Português (Brasil)");
    for label in ["Arquitetura", "Anotar", "Vista", "Gerenciar", "Modificar", "Propriedades", "Plantas de piso", "Intervalo da vista"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    press(&mut h, "Arquitetura");
    press(&mut h, "Parede");
    assert!(h.query_by_label("Modificar | Inserir parede").is_some());
    // A new project, in the interface's language: its levels named in Portuguese.
    key(&mut h, Key::Escape);
    press(&mut h, "Arquivo");
    // Menu items read out their shortcut too: "Novo Ctrl+N".
    h.get_by_label_contains("Novo").click();
    h.run();
    let levels: Vec<String> = h.state().session.model.levels().iter().map(|(_, l)| l.name.clone()).collect();
    assert_eq!(levels, ["Nível 1", "Nível 2"]);
    assert!(h.query_by_label("Plantas de piso").is_some());
    assert!(h.query_all_by_label("Nível 1").count() >= 2, "in the browser, and as the view's tab");
}

#[test]
fn the_interface_speaks_spanish() {
    let mut h = app(BOX);
    press(&mut h, "View");
    press(&mut h, "User Interface");
    press(&mut h, "Español");
    for label in ["Arquitectura", "Anotar", "Vista", "Gestionar", "Modificar", "Propiedades", "Plantas", "Rango de vista"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    press(&mut h, "Arquitectura");
    press(&mut h, "Muro");
    assert!(h.query_by_label("Modificar | Colocar muro").is_some());
    key(&mut h, Key::Escape);
    press(&mut h, "Archivo");
    h.get_by_label_contains("Nuevo").click();
    h.run();
    let levels: Vec<String> = h.state().session.model.levels().iter().map(|(_, l)| l.name.clone()).collect();
    assert_eq!(levels, ["Nivel 1", "Nivel 2"]);
}

#[test]
fn nothing_a_translated_interface_shows_is_left_in_english() {
    use crate::lang::Lang;
    use egui_kittest::kittest::{By, NodeT};
    // A label in English that the table translates is a string the window forgot to translate: a
    // widget's name, or a plain label's text, which egui gives screen readers as its value.
    let english = |h: &Harness<BimApp>, l: Lang| -> Vec<String> {
        h.query_all(By::new())
            .filter_map(|n| n.accesskit_node().label().or_else(|| n.accesskit_node().value()))
            .filter(|s| crate::lang::translated_in(l, s).is_some_and(|x| x != s))
            .collect()
    };
    let extra = r#"parameter {"name": "Finish Height", "kind": "length", "categories": ["wall"]}
schedule {"name": "Areas", "category": "walls", "fields": ["Type", {"name": "Length", "total": true}, {"name": "Twice", "formula": "Length * 2"}], "filters": [{"field": "Length", "op": "greater", "value": 1}], "sort": ["Length"]}
phase {"name": "Existing", "rename": "Survey"}
phase {"name": "New Construction", "rename": "Works"}
phase_filter {"name": "Show All", "delete": true}
phase_filter {"name": "Show Complete", "delete": true}
phase_filter {"name": "Show New", "delete": true}
phase_filter {"name": "Show Previous + Demolished", "delete": true}
phase_filter {"name": "Show Previous + New", "delete": true}
phase_filter {"name": "Mine", "existing": "overridden"}
option {"set": "Studies", "options": ["Tall", "Low"]}
mass {"boundary": [[1000, 1000], [3000, 1000], [3000, 2000], [1000, 2000]], "name": "Block A"}"#;
    let mut h =
        app(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}\nroof {{}}\ncomponent {{\"family\": \"WC\", \"at\": [4000, 5850]}}\n{extra}"));
    // Wide enough for every tab's tools in the longest language: a narrower window hands some over to their panel's list.
    h.set_size(vec2(1800.0, 900.0));
    h.run();
    // A detail group to place and repeat, so the detail tools' Options Bars have one to name.
    let s = &mut h.state_mut().session;
    let line = s.execute("detail_line", &json!({"start": [1000, 1000], "end": [2000, 1000]})).unwrap();
    s.execute("create_group", &json!({"ids": [line["id"]]})).unwrap();
    h.run();
    // The probe finds what it looks for: in English, the same labels are all English.
    assert!(english(&h, Lang::Spanish).contains(&"Architecture".to_string()));
    for l in [Lang::Portuguese, Lang::Spanish, Lang::French, Lang::Chinese] {
        // A machine has the fonts it has: a window is told it has one for Chinese.
        crate::lang::assume_chinese_font(&h.ctx);
        h.state_mut().lang = l;
        h.run();
        let tr = |en: &'static str| crate::lang::translated_in(l, en).unwrap_or(en);
        let mut missed = Vec::new();
        for tab in ["Architecture", "Annotate", "Massing", "View", "Manage", "Modify"] {
            press(&mut h, tr(tab));
            missed.extend(english(&h, l));
        }
        // The Demolish tool on the Modify tab, with its prompt.
        press(&mut h, tr("Demolish"));
        missed.extend(english(&h, l));
        // A wall's Properties and its contextual tab; a door's; a roof's; the tools' Options Bars.
        let tools = ["Wall", "Roof", "Roof by Extrusion", "Floor", "Ceiling", "Shaft", "Dormer", "Stair", "Ramp", "Railing", "Component"]
            .map(|t| (P2(0.0, 0.0), Some(t)));
        // A wall, a door, a roof's edge, a WC's cistern.
        // (and a mass's outline).
        let picks = [
            (P2(5000.0, 0.0), None),
            (P2(2000.0, 0.0), None),
            (P2(-600.0, 3000.0), None),
            (P2(4000.0, 5670.0), None),
            (P2(2000.0, 1000.0), None),
        ];
        for (at, tool) in picks.into_iter().chain(tools) {
            key(&mut h, Key::Escape);
            if let Some(tool) = tool {
                press(&mut h, tr("Architecture"));
                press(&mut h, tr(tool));
            } else {
                click(&mut h, at);
            }
            missed.extend(english(&h, l));
        }
        // The Mass tool with its Options Bar and prompts, and the Massing tab with a mass selected.
        key(&mut h, Key::Escape);
        press(&mut h, tr("Massing"));
        press(&mut h, tr("Mass"));
        click(&mut h, P2(4000.0, 3000.0));
        missed.extend(english(&h, l));
        click(&mut h, P2(5000.0, 3000.0));
        missed.extend(english(&h, l));
        key(&mut h, Key::Escape);
        key(&mut h, Key::Escape);
        click(&mut h, P2(2000.0, 1000.0));
        press(&mut h, tr("Massing"));
        missed.extend(english(&h, l));
        // Project Parameters; New Schedule; a schedule, its properties' every tab.
        key(&mut h, Key::Escape);
        press(&mut h, tr("Manage"));
        press(&mut h, tr("Project Parameters"));
        missed.extend(english(&h, l));
        press(&mut h, tr("Close"));
        press(&mut h, tr("View"));
        press(&mut h, tr("Schedules"));
        press(&mut h, tr("Schedule/Quantities…"));
        pick(&mut h, Role::Button, &crate::lang::category("room", true));
        missed.extend(english(&h, l));
        press(&mut h, tr("Cancel"));
        browse_entry(&mut h, "Areas");
        missed.extend(english(&h, l));
        press(&mut h, &tr("Edit {}").replace("{}", tr("Fields")));
        pick(&mut h, Role::Button, tr("Length"));
        press(&mut h, tr("Calculated Value…"));
        missed.extend(english(&h, l));
        for tab in ["Filter", "Sorting/Grouping"] {
            pick(&mut h, Role::Button, tr(tab));
            missed.extend(english(&h, l));
        }
        press(&mut h, tr("Close"));
        // Keyboard Shortcuts.
        shortcut(&mut h, "KS");
        missed.extend(english(&h, l));
        press(&mut h, tr("Close"));
        // Settings, every page; a palette's Dock menu.
        press(&mut h, tr("Architecture"));
        h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
        h.run();
        missed.extend(english(&h, l));
        for page in ["Performance", "Graphics", "Files", "Updates"] {
            pick(&mut h, Role::Button, tr(page));
            missed.extend(english(&h, l));
        }
        press(&mut h, tr("Close"));
        press(&mut h, &tr("Dock {}").replace("{}", tr("Properties")));
        missed.extend(english(&h, l));
        key(&mut h, Key::Escape);
        // Snaps, and the Measure tool with its prompt.
        key(&mut h, Key::Escape);
        press(&mut h, tr("Manage"));
        press(&mut h, tr("Snaps"));
        missed.extend(english(&h, l));
        press(&mut h, tr("Close"));
        press(&mut h, tr("Annotate"));
        press(&mut h, tr("Measure"));
        missed.extend(english(&h, l));
        // A plan's Properties with its Graphics group; Visibility/Graphics on every tab, with an override
        // being edited; Filters; View Templates; and the Modify tab's View panel.
        key(&mut h, Key::Escape);
        topmost(&mut h, "Level 1");
        missed.extend(english(&h, l));
        shortcut(&mut h, "VV");
        missed.extend(english(&h, l));
        let over = format!("{}: {}", crate::lang::category("wall", true), tr("Cut Lines"));
        pick(&mut h, Role::Button, &over);
        missed.extend(english(&h, l));
        press(&mut h, tr("Cancel"));
        for tab in ["Annotation Categories", "Imported Categories", "Filters", "Elements", "Design Options"] {
            press(&mut h, tr(tab));
            missed.extend(english(&h, l));
        }
        press(&mut h, tr("Cancel"));
        // Phasing on every tab with a status being edited, and Design Options.
        shortcut(&mut h, "PH");
        missed.extend(english(&h, l));
        for tab in ["Phase Filters", "Graphic Overrides"] {
            press(&mut h, tr(tab));
            missed.extend(english(&h, l));
        }
        pick(&mut h, Role::CheckBox, &format!("{}: {}", tr("Cut"), tr("Line weight")));
        missed.extend(english(&h, l));
        press(&mut h, tr("Close"));
        shortcut(&mut h, "DO");
        missed.extend(english(&h, l));
        press(&mut h, tr("Close"));
        shortcut(&mut h, "FT");
        press(&mut h, tr("New"));
        press(&mut h, tr("Add Filter"));
        missed.extend(english(&h, l));
        press(&mut h, tr("Close"));
        shortcut(&mut h, "VT");
        missed.extend(english(&h, l));
        press(&mut h, tr("Cancel"));
        // The detail tools' Options Bars, with a group to place; Linework with its choices.
        for tool in ["Detail Group", "Repeating Detail", "Revision Cloud", "Insulation"] {
            key(&mut h, Key::Escape);
            press(&mut h, tr("Annotate"));
            press(&mut h, tr(tool));
            missed.extend(english(&h, l));
        }
        key(&mut h, Key::Escape);
        topmost(&mut h, tr("Modify"));
        press(&mut h, tr("Linework"));
        missed.extend(english(&h, l));
        key(&mut h, Key::Escape);
        // Manage > Fill Patterns on both tabs, with a pattern's families; Line Styles on both; Edit Type of a region being sketched.
        press(&mut h, tr("Manage"));
        press(&mut h, tr("Fill Patterns"));
        missed.extend(english(&h, l));
        topmost(&mut h, tr("Patterns"));
        press(&mut h, &tr("{}: Edit Lines").replace("{}", "Sand"));
        missed.extend(english(&h, l));
        press(&mut h, tr("Cancel"));
        press(&mut h, tr("Line Styles"));
        missed.extend(english(&h, l));
        topmost(&mut h, tr("Patterns"));
        missed.extend(english(&h, l));
        press(&mut h, tr("Cancel"));
        shortcut(&mut h, "FR");
        press(&mut h, tr("Edit Type"));
        missed.extend(english(&h, l));
        press(&mut h, tr("Cancel"));
        press(&mut h, tr("Cancel Edit Mode"));
        click(&mut h, P2(5000.0, 0.0));
        missed.extend(english(&h, l));
        key(&mut h, Key::Escape);
        // The names of the project's own patterns, types and styles are in the project's language, English here, whatever the
        // interface speaks (`language.rs` knows them, for a project that starts in another).
        let m = &h.state().session.model;
        let own: Vec<&String> =
            m.fill_patterns.keys().chain(m.line_patterns.keys()).chain(m.region_types.keys()).chain(m.line_styles.keys()).collect();
        missed.retain(|s| !own.contains(&s));
        missed.sort();
        missed.dedup();
        assert!(missed.is_empty(), "{l:?}: left in English: {missed:?}");
    }
}

#[test]
fn the_wall_tool_draws_arcs_and_walls_on_the_location_line_picked() {
    let mut h = app("");
    tool(&mut h, "Wall");
    // Revit's Draw panel: a Start-End-Radius Arc takes its start, its end, then a point it passes through.
    press(&mut h, "Start-End-Radius Arc");
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(4000.0, 0.0));
    click(&mut h, P2(2000.0, -2000.0));
    assert_eq!(walls(&h), [(P2(0.0, 0.0), P2(4000.0, 0.0))]);
    let arc = ids(&h, "wall")[0];
    assert!((field(&h, arc, "arc").as_f64().unwrap() - 180.0).abs() < 1e-6, "{}", field(&h, arc, "arc"));
    assert_eq!(h.state().tool, Tool::Wall(vec![P2(4000.0, 0.0)]), "chained, the next one starts where it ends");
    key(&mut h, Key::Escape);
    // Straight again, on the exterior finish face: the Options Bar's Location Line.
    press(&mut h, "Line");
    h.get_by_role_and_label(Role::ComboBox, "Location Line:").click();
    h.run();
    press(&mut h, "Finish Face: Exterior");
    click(&mut h, P2(0.0, 3000.0));
    click(&mut h, P2(4000.0, 3000.0));
    key(&mut h, Key::Enter);
    let line = ids(&h, "wall")[1];
    assert_eq!((field(&h, line, "location"), field(&h, line, "arc")), (json!("exterior-finish"), Value::Null));
}

#[test]
fn a_selected_wall_attaches_flips_and_takes_another_location_line() {
    use egui_kittest::kittest::NodeT;
    let mut h = app(&format!("{BOX}\nroof {{}}"));
    click(&mut h, P2(4000.0, 0.0));
    let wall = h.state().selection[0];
    // Put on its interior face (going east, its exterior is to its left, north), it stays where it is.
    h.get_by_role_and_label(Role::ComboBox, "Location Line").click();
    h.run();
    press(&mut h, "Finish Face: Interior");
    assert_eq!((field(&h, wall, "location"), field(&h, wall, "start")), (json!("interior-finish"), json!([0.0, -100.0])));
    // Attach Top/Base, then the roof, by its edge seen overhead: the wall's top follows it.
    press(&mut h, "Attach Top/Base");
    let roof = ids(&h, "roof")[0];
    click(&mut h, P2(4000.0, -600.0));
    assert_eq!(field(&h, wall, "attach_top"), json!(roof));
    assert_eq!(h.state().selection, [wall], "the wall stays selected");
    let attached = h.get_by_role_and_label(Role::CheckBox, "Top is Attached").accesskit_node().toggled();
    assert_eq!(attached, Some(egui::accesskit::Toggled::True));
    // Flipped from the same tab, it runs the other way; detached, its top is its own again.
    press(&mut h, "Flip");
    assert_eq!(walls(&h)[0], (P2(8000.0, -100.0), P2(0.0, -100.0)));
    press(&mut h, "Detach Top/Base");
    assert_eq!(field(&h, wall, "attach_top"), Value::Null);
}

#[test]
fn type_properties_build_a_wall_types_layers() {
    use egui_kittest::kittest::{By, NodeT};
    let mut h = app(BOX);
    click(&mut h, P2(4000.0, 0.0));
    press(&mut h, "Edit Type");
    // One structure layer as thick as the type; a finish inserted outside it, a brick 100 thick.
    press(&mut h, "Insert Layer");
    let nth = |h: &Harness<BimApp>, role: Role, label: &'static str, n: usize| {
        h.query_all(By::new().role(role).label(label)).nth(n).map(|n| n.accesskit_node().id()).unwrap()
    };
    // Its material from the library.
    let pick = nth(&h, Role::ComboBox, "Material", 0);
    h.get_by(|n| n.id() == pick).click();
    h.run();
    press(&mut h, "Brick");
    let thick = nth(&h, Role::TextInput, "Thickness", 0);
    h.get_by(|n| n.id() == thick).focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by(|n| n.id() == thick).type_text("100");
    h.run();
    press(&mut h, "OK");
    let t = &h.state().session.model.types.wall["Generic - 200mm"];
    assert_eq!((t.thickness, t.layers.len()), (300.0, 2));
    assert_eq!((t.layers[0].material.as_str(), t.layers[0].thickness, t.layers[1].thickness), ("Brick", 100.0, 200.0));
}

#[test]
fn duplicate_copies_a_types_layers() {
    let mut h = app(r#"wall {"points": [[0, 0], [8000, 0]], "type": "Exterior - 300mm"}"#);
    click(&mut h, P2(4000.0, 0.0));
    press(&mut h, "Edit Type");
    press(&mut h, "Duplicate…");
    type_into(&mut h, "New type", "Exterior copy");
    press(&mut h, "Create");
    press(&mut h, "OK");
    let types = &h.state().session.model.types.wall;
    // The same layers, each a copy with an id and a GUID of its own.
    let bare = |n: &str| {
        types[n].layers.iter().map(|l| bimcraft_core::model::Layer { id: 0, guid: Default::default(), ..l.clone() }).collect::<Vec<_>>()
    };
    assert_eq!(bare("Exterior copy"), bare("Exterior - 300mm"));
    assert!(types["Exterior copy"].layers.iter().zip(&types["Exterior - 300mm"].layers).all(|(a, b)| a.id != b.id && a.guid != b.guid));
}

#[test]
fn the_type_selectors_list_stays_in_the_window_and_scrolls() {
    // Issue 64: a component's list holds every family's types, taller than the window; it went off
    // the top and the bottom of it.
    let mut h = app(BOX);
    tool(&mut h, "Component");
    press(&mut h, "Type Selector");
    let list = h.ctx.memory(|m| m.areas().top_layer_id(egui::Order::Foreground).and_then(|l| m.area_rect(l.id)));
    let list = list.expect("the list is open");
    let window = h.ctx.content_rect();
    assert!(window.contains_rect(list), "{list:?} in {window:?}");
}

#[test]
fn the_component_tool_places_a_familys_type_that_space_turns() {
    let mut h = app(BOX);
    tool(&mut h, "Component");
    assert!(h.query_by_label("Modify | Place Component").is_some());
    press(&mut h, "Type Selector");
    press(&mut h, "Bed: 900 x 2000");
    // A quarter turn before it goes down, as Revit's Space gives.
    key(&mut h, Key::Space);
    click(&mut h, P2(4000.0, 3000.0));
    let c = ids(&h, "component");
    assert_eq!(c.len(), 1);
    assert_eq!(
        (field(&h, c[0], "family"), field(&h, c[0], "type"), field(&h, c[0], "angle"), field(&h, c[0], "at")),
        (json!("Bed"), json!("900 x 2000"), json!(90.0), json!([4000.0, 3000.0]))
    );
    // The Project Browser's families: a type of one, picked, is the one the tool places.
    key(&mut h, Key::Escape);
    for label in ["Families", "Table", "2000 x 1000"] {
        h.get_by_label(label).click_accesskit();
        h.run();
    }
    assert_eq!(h.state().tool, Tool::Component);
    click(&mut h, P2(4000.0, 1500.0));
    let c = ids(&h, "component");
    assert_eq!((field(&h, c[1], "family"), field(&h, c[1], "type")), (json!("Table"), json!("2000 x 1000")));
}

#[test]
fn rotate_turns_a_component_about_its_own_point() {
    let mut h = app(&format!("{BOX}\ncomponent {{\"family\": \"WC\", \"at\": [4000, 5850]}}"));
    let c = ids(&h, "component")[0];
    click(&mut h, P2(4000.0, 5670.0));
    assert_eq!(h.state().selection, [c]);
    press(&mut h, "Rotate");
    assert!(matches!(h.state().tool, Tool::Rotate { centre, .. } if centre == P2(4000.0, 5850.0)), "{:?}", h.state().tool);
}

#[test]
fn properties_and_type_properties_edit_a_components_parameters() {
    let stool = r#"family {"name": "Stool", "parameters": [{"name": "Seat", "instance": true, "value": 450}, {"name": "Size", "value": 400}], "types": {"A": {}}, "forms": [{"profile": [[0, 0], ["Size", 0], ["Size", "Size"], [0, "Size"]], "to": "Seat"}]}
component {"family": "Stool", "at": [3000, 2000]}"#;
    let mut h = app(&format!("{BOX}\n{stool}"));
    let c = ids(&h, "component")[0];
    // Picked by its outline.
    click(&mut h, P2(3200.0, 2000.0));
    assert_eq!(h.state().selection, [c]);
    assert!(h.query_by_label("Modify | Components").is_some());
    // Its instance parameter, in Properties.
    type_into(&mut h, "Seat", "500");
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, c, "params")["Seat"], json!(500.0));
    // Its type's, in Type Properties; Duplicate makes another, which OK puts it on.
    press(&mut h, "Edit Type");
    type_into(&mut h, "Size", "600");
    press(&mut h, "OK");
    assert_eq!(h.state().session.model.families["Stool"].types["A"]["Size"], json!(600.0));
    press(&mut h, "Edit Type");
    press(&mut h, "Duplicate…");
    type_into(&mut h, "New type", "B");
    press(&mut h, "Create");
    press(&mut h, "OK");
    assert_eq!(h.state().session.model.families["Stool"].types["B"]["Size"], json!(600.0));
    assert_eq!(field(&h, c, "type"), json!("B"));
}

#[test]
fn floors_go_under_the_walls_and_a_slope_arrow_slopes_them() {
    let mut h = app(BOX);
    tool(&mut h, "Floor");
    assert!(h.query_by_label("Modify | Create Floor").is_some());
    click(&mut h, P2(4000.0, 3000.0));
    let floor = ids(&h, "floor");
    assert_eq!((floor.len(), field(&h, floor[0], "area_m2")), (1, json!(48.0)));
    assert_eq!(h.state().selection, floor, "finished, the floor is selected");
    assert!(h.query_by_label("Modify | Floors").is_some() && h.query_by_label("Height Offset From Level").is_some());
    // Its slope arrow, from Shape Editing: its tail, then its head, at the Options Bar's slope.
    press(&mut h, "Slope Arrow");
    type_into(&mut h, "Slope:", "3");
    click(&mut h, P2(1000.0, 3000.0));
    click(&mut h, P2(7000.0, 3000.0));
    assert_eq!((field(&h, floor[0], "slope"), field(&h, floor[0], "arrow")), (json!(3.0), json!([[1000.0, 3000.0], [7000.0, 3000.0]])));
    assert_eq!((h.state().selection.as_slice(), &h.state().tool), (floor.as_slice(), &Tool::Modify), "the floor stays selected");
}

#[test]
fn the_floor_tool_leaves_a_courtyard_open_inside_a_loop_of_walls_standing_free() {
    let mut h = app(r#"wall {"points": [[0, 0], [16000, 0], [16000, 16000], [0, 16000]], "closed": true}
wall {"points": [[6000, 6000], [10000, 6000], [10000, 10000], [6000, 10000]], "closed": true}"#);
    tool(&mut h, "Floor");
    click(&mut h, P2(3000.0, 8000.0));
    let floor = ids(&h, "floor");
    assert_eq!(floor.len(), 1);
    let courtyard = json!([[[6000.0, 6000.0], [6000.0, 10000.0], [10000.0, 10000.0], [10000.0, 6000.0]]]);
    assert_eq!((field(&h, floor[0], "openings"), field(&h, floor[0], "area_m2")), (courtyard, json!(240.0)), "the loop's centre lines");
}

#[test]
fn ceilings_go_in_the_room_clicked_and_show_in_its_ceiling_plan() {
    use egui_kittest::kittest::NodeT;
    let mut h = app(BOX);
    tool(&mut h, "Ceiling");
    assert!(h.query_by_label("Modify | Place Ceiling").is_some());
    press(&mut h, "Type Selector");
    press(&mut h, "Tile 600 x 600");
    type_into(&mut h, "Height Offset:", "2400");
    click(&mut h, P2(4000.0, 3000.0));
    let c = ids(&h, "ceiling");
    // Out to the walls' faces, 100 mm in from their centre lines.
    assert_eq!((c.len(), field(&h, c[0], "offset"), field(&h, c[0], "area_m2")), (1, json!(2400.0), json!(45.24)));
    assert_eq!(field(&h, c[0], "type"), json!("Tile 600 x 600"));
    assert_eq!(h.state().tool, Tool::Ceiling, "ready for the next room");
    // The browser's Ceiling Plans open the level's reflected ceiling plan: its tiles pick the ceiling.
    key(&mut h, Key::Escape);
    let lowest = h.get_all_by_label("Level 1").max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).map(|n| n.accesskit_node().id());
    h.get_by(|n| Some(n.id()) == lowest).click_accesskit();
    h.run();
    assert_eq!(h.state().sheet, Sheet::Ceiling(100000));
    assert_eq!(h.get_all_by_label("Ceiling Plan: Level 1").count(), 2, "its tab and the palette name the view");
    click(&mut h, P2(4000.0, 1000.0));
    assert_eq!(h.state().selection, c, "a tile line through the ceiling's middle");
    assert!(h.query_by_label("Modify | Ceilings").is_some());
}

/// What the last frame painted as text: labels, and what the canvas writes beside the cursor.
fn painted(h: &Harness<BimApp>) -> Vec<String> {
    fn texts(s: &egui::Shape, out: &mut Vec<String>) {
        match s {
            egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
            egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| texts(&c.shape, &mut out));
    out
}

#[test]
fn the_ceiling_tool_stops_at_the_faces_of_a_loop_of_walls_in_the_room() {
    let mut h = app(r#"wall {"points": [[0, 0], [16000, 0], [16000, 16000], [0, 16000]], "closed": true}
wall {"points": [[6000, 6000], [10000, 6000], [10000, 10000], [6000, 10000]], "closed": true}"#);
    tool(&mut h, "Ceiling");
    // Before the click, the ring the ceiling would fill and its area, 15.8² less 4.2².
    h.hover_at(h.state().screen(P2(3000.0, 3000.0)));
    h.run();
    assert!(painted(&h).iter().any(|t| t == "232.00 m²"), "{:?}", painted(&h));
    click(&mut h, P2(3000.0, 3000.0));
    let c = ids(&h, "ceiling");
    assert_eq!(c.len(), 1);
    let island = json!([[[5900.0, 5900.0], [5900.0, 10100.0], [10100.0, 10100.0], [10100.0, 5900.0]]]);
    assert_eq!((field(&h, c[0], "openings"), field(&h, c[0], "area_m2")), (island, json!(232.0)), "out to the loop's faces");
}

#[test]
fn shaft_openings_take_two_corners_and_cut_the_floors_they_pass_through() {
    let mut h = app(&format!("{BOX}\nfloor {{\"level\": 100001, \"boundary\": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}}"));
    tool(&mut h, "Shaft");
    assert!(h.query_by_label("Modify | Create Shaft Opening").is_some());
    click(&mut h, P2(3000.0, 2000.0));
    assert!(ids(&h, "shaft").is_empty(), "the first click is a corner");
    click(&mut h, P2(4000.0, 4000.0));
    let shaft = ids(&h, "shaft");
    assert_eq!((shaft.len(), field(&h, shaft[0], "area_m2"), field(&h, shaft[0], "top_level")), (1, json!(2.0), json!(100001)));
    assert_eq!(field(&h, ids(&h, "floor")[0], "area_m2"), json!(46.0), "the floor above, less the hole");
    assert_eq!(h.state().tool, Tool::Shaft(None), "ready for the next one");
    // Selected by its cross, its palette.
    key(&mut h, Key::Escape);
    click(&mut h, P2(3500.0, 3000.0));
    assert_eq!(h.state().selection, shaft);
    assert!(h.query_by_label("Modify | Shaft Openings").is_some() && h.query_by_label("Top Constraint").is_some());
}

#[test]
fn roofs_by_extrusion_take_a_line_then_how_far_across() {
    let mut h = app(BOX);
    tool(&mut h, "Roof by Extrusion");
    assert!(h.query_by_label("Modify | Create Extrusion Roof").is_some());
    // Along the south wall, then across to the north one: a gable rising 30° to a ridge over the middle.
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(8000.0, 0.0));
    assert!(ids(&h, "roof").is_empty(), "two clicks are its line");
    click(&mut h, P2(4000.0, 6000.0));
    let roof = ids(&h, "roof");
    assert_eq!(roof.len(), 1);
    let x = field(&h, roof[0], "extrusion");
    assert_eq!((&x["line"], &x["from"], &x["to"]), (&json!([[0.0, 0.0], [8000.0, 0.0]]), &json!(0.0), &json!(6000.0)));
    let ridge = field(&h, roof[0], "ridge").as_f64().unwrap();
    assert!((ridge - (3000.0 + 4000.0 * 30f64.to_radians().tan())).abs() < 0.01, "on Level 2: {ridge}");
    assert_eq!(h.state().selection, roof, "finished, the roof is selected");
    assert!(h.query_by_label("Extrusion End").is_some());
    // A shed rises along the whole line; swept the other way, it lies to the line's right.
    tool(&mut h, "Roof by Extrusion");
    press(&mut h, "Shed");
    for p in [P2(0.0, 6000.0), P2(8000.0, 6000.0), P2(4000.0, 0.0)] {
        click(&mut h, p);
    }
    let shed = ids(&h, "roof")[1];
    let x = field(&h, shed, "extrusion");
    assert_eq!((x["profile"].as_array().map(Vec::len), &x["from"], &x["to"]), (Some(2), &json!(-6000.0), &json!(0.0)));
}

#[test]
fn dormers_go_where_a_roof_slope_is_clicked() {
    let mut h = app(&format!("{BOX}\nroof {{\"slope\": 45}}"));
    tool(&mut h, "Dormer");
    assert!(h.query_by_label("Modify | Dormer").is_some());
    type_into(&mut h, "Height:", "1000");
    click(&mut h, P2(4000.0, 800.0));
    let roofs = ids(&h, "roof");
    assert_eq!(roofs.len(), 2, "{}", h.state().status.text);
    assert_eq!(field(&h, roofs[1], "join"), json!(roofs[0]), "its roof runs into the one it stands on");
    assert_eq!((ids(&h, "wall").len(), ids(&h, "window").len()), (7, 1), "its front and sides, a window in its front");
    assert_eq!(h.state().tool, Tool::Dormer, "ready for the next one");
}

#[test]
fn gable_edge_stands_a_roof_edge_upright_and_slopes_it_again() {
    let mut h = app(&format!("{BOX}\nroof {{}}"));
    // The roof, by its edge overhead in Level 1's plan, then Shape Editing's Gable Edge.
    click(&mut h, P2(-600.0, 3000.0));
    let roof = ids(&h, "roof")[0];
    assert_eq!(h.state().selection, vec![roof]);
    press(&mut h, "Gable Edge");
    // Its east end and its west end, upright; the east one again, sloping.
    click(&mut h, P2(8600.0, 3000.0));
    click(&mut h, P2(-600.0, 3000.0));
    assert_eq!(field(&h, roof, "gables"), json!([1, 3]));
    click(&mut h, P2(8600.0, 3000.0));
    assert_eq!(field(&h, roof, "gables"), json!([3]));
    assert_eq!((h.state().selection.as_slice(), &h.state().tool), (&[roof][..], &Tool::GableEdge), "the roof stays selected");
    // A courtyard's edges are the roof's too, numbered on after the outline's.
    let court = r#"roof {"boundary": [[20000, 0], [30000, 0], [30000, 8000], [20000, 8000]], "courtyards": [[[23000, 3000], [27000, 3000], [27000, 5000], [23000, 5000]]]}"#;
    let mut h = app(&format!("{BOX}\n{court}"));
    click(&mut h, P2(20000.0, 4000.0));
    let roof = ids(&h, "roof")[0];
    press(&mut h, "Gable Edge");
    click(&mut h, P2(25000.0, 3000.0));
    assert_eq!(field(&h, roof, "gables"), json!([4]));
}

#[test]
fn the_materials_window_edits_the_projects_library() {
    use bimcraft_core::model::Pattern;
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Materials");
    // A new one, picked: its cut pattern, then its name.
    press(&mut h, "New");
    assert!(h.state().session.model.materials.contains_key("Material 1"));
    h.get_by_role_and_label(Role::ComboBox, "Cut Pattern").click();
    h.run();
    press(&mut h, "Diagonal");
    assert_eq!(h.state().session.model.materials["Material 1"].cut, Pattern::Diagonal);
    // How see-through it is, on its slider (a click on the slider's track, then its arrow keys).
    assert_eq!(h.state().session.model.materials["Material 1"].transparency, 0);
    h.get_by_label("Transparency").focus();
    h.run();
    key(&mut h, Key::ArrowRight);
    key(&mut h, Key::ArrowRight);
    assert_eq!(h.state().session.model.materials["Material 1"].transparency, 2);
    type_into(&mut h, "Name", "Oak");
    key(&mut h, Key::Enter);
    let m = &h.state().session.model.materials;
    assert!(m.contains_key("Oak") && !m.contains_key("Material 1"), "{:?}", m.keys().collect::<Vec<_>>());
    press(&mut h, "Delete");
    assert!(!h.state().session.model.materials.contains_key("Oak"));
    press(&mut h, "Close");
    assert!(h.query_by_label("Cut Pattern").is_none());
}

#[test]
fn a_floor_types_layers_go_from_its_top_down() {
    use egui_kittest::kittest::{By, NodeT};
    let mut h = app(BOX);
    tool(&mut h, "Floor");
    click(&mut h, P2(4000.0, 3000.0));
    press(&mut h, "Edit Type");
    // A layer on top of its structure: tiles, 10 mm.
    press(&mut h, "Insert Layer");
    let first = |h: &Harness<BimApp>, role: Role, label: &'static str| {
        h.query_all(By::new().role(role).label(label)).next().map(|n| n.accesskit_node().id()).unwrap()
    };
    let pick = first(&h, Role::ComboBox, "Material");
    h.get_by(|n| n.id() == pick).click();
    h.run();
    press(&mut h, "Ceramic tile");
    let thick = first(&h, Role::TextInput, "Thickness");
    h.get_by(|n| n.id() == thick).focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by(|n| n.id() == thick).type_text("10");
    h.run();
    assert!(h.get_all_by_label("Down").count() > 0 && h.query_by_label("In").is_none(), "a floor's layers move up and down");
    press(&mut h, "OK");
    let t = &h.state().session.model.types.floor["Generic - 200mm"];
    assert_eq!((t.thickness, t.layers.len()), (210.0, 2));
    assert_eq!((t.layers[0].material.as_str(), t.layers[0].thickness), ("Ceramic tile", 10.0));
}

#[test]
fn project_parameters_are_made_in_their_window_and_given_values_in_properties() {
    use bimcraft_core::model::ParameterKind;
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Project Parameters");
    assert!(h.query_by_label("No project parameters yet: New makes one.").is_some());
    press(&mut h, "New");
    assert_eq!(h.state().session.model.parameters["Parameter 1"].categories, ["wall"]);
    type_into(&mut h, "Name", "Finish Height");
    key(&mut h, Key::Enter);
    pick(&mut h, Role::CheckBox, "Doors");
    assert_eq!(h.state().session.model.parameters["Finish Height"].categories, ["door", "wall"]);
    h.get_by_role_and_label(Role::ComboBox, "Type of Parameter").click();
    h.run();
    press(&mut h, "Length");
    assert_eq!(h.state().session.model.parameters["Finish Height"].kind, ParameterKind::Length);
    press(&mut h, "Close");
    // The bottom wall's value, typed in the project's units; emptied, it has none.
    let wall = ids(&h, "wall")[0];
    click(&mut h, P2(4000.0, 0.0));
    type_role(&mut h, Role::TextInput, "Finish Height", "2400");
    assert_eq!(field(&h, wall, "params"), json!({"Finish Height": 2400.0}));
    type_role(&mut h, Role::TextInput, "Finish Height", "");
    assert_eq!(field(&h, wall, "params"), Value::Null);
    // Deleted, it's gone from the palette.
    key(&mut h, Key::Escape);
    press(&mut h, "Manage");
    press(&mut h, "Project Parameters");
    press(&mut h, "Delete");
    press(&mut h, "Close");
    assert!(h.state().session.model.parameters.is_empty());
    click(&mut h, P2(4000.0, 0.0));
    assert!(h.query_by_label("Finish Height").is_none() && h.query_by_label("Project Parameters").is_none(), "no group for it");
}

#[test]
fn a_new_schedule_takes_its_fields_filter_and_sorting_from_schedule_properties() {
    use bimcraft_core::schedule::{Filter, Op, Sort};
    let rooms = r#"wall {"points": [[5000,0],[5000,6000]], "type": "Interior - 100mm"}
room {"point": [6500, 3000], "name": "Kitchen"}
room {"point": [2000, 3000], "name": "Living"}"#;
    let mut h = app_at(&format!("{BOX}\n{rooms}"), 0.05);
    press(&mut h, "View");
    press(&mut h, "Schedules");
    press(&mut h, "Schedule/Quantities…");
    pick(&mut h, Role::Button, "Rooms");
    // Named after its category, as no view is yet.
    let named = {
        use egui_kittest::kittest::NodeT;
        h.get_by_role_and_label(Role::TextInput, "Name:").accesskit_node().value()
    };
    assert_eq!(named.as_deref(), Some("Room Schedule 2"));
    type_into(&mut h, "Name:", "Room Areas");
    press(&mut h, "OK");
    let sc = ids(&h, "schedule")[0];
    assert_eq!(h.state().sheet, Sheet::Table(sc));
    let schedule = |h: &Harness<BimApp>| match h.state().session.model.elements.get(sc) {
        Some(Element::Schedule(s)) => s.clone(),
        _ => panic!("schedule {sc} is gone"),
    };
    // Its fields: its perimeter taken away, its area totalled, and a cost worked out from it.
    pick(&mut h, Role::Button, "Perimeter");
    press(&mut h, "Remove");
    pick(&mut h, Role::Button, "Area");
    pick(&mut h, Role::CheckBox, "Calculate totals");
    press(&mut h, "Calculated Value…");
    type_role(&mut h, Role::TextInput, "Name", "Cost");
    type_role(&mut h, Role::TextInput, "Formula", "Area * 120");
    press(&mut h, "OK");
    let s = schedule(&h);
    let names: Vec<&str> = s.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["Number", "Name", "Level", "Area", "Cost"]);
    assert!(s.fields[3].total && s.fields[4].formula.as_deref() == Some("Area * 120"));
    // Its filter: the rooms over 20 m².
    pick(&mut h, Role::Button, "Filter");
    press(&mut h, "Add Filter");
    pick(&mut h, Role::ComboBox, "Filter by:");
    pick(&mut h, Role::Button, "Area");
    pick(&mut h, Role::ComboBox, "Comparison");
    press(&mut h, "is greater than");
    type_role(&mut h, Role::TextInput, "Filter by:", "20");
    assert_eq!(schedule(&h).filters, [Filter { field: "Area".into(), op: Op::Greater, value: json!(20.0) }]);
    // Sorted by its area, largest first, and a row for each.
    pick(&mut h, Role::Button, "Sorting/Grouping");
    press(&mut h, "Add Sort");
    pick(&mut h, Role::ComboBox, "Sort by:");
    pick(&mut h, Role::Button, "Area");
    pick(&mut h, Role::ComboBox, "Order");
    press(&mut h, "Descending");
    pick(&mut h, Role::CheckBox, "Itemize every instance");
    let s = schedule(&h);
    assert_eq!((s.sort, s.grouped), (vec![Sort { field: "Area".into(), descending: true }], true));
    press(&mut h, "Close");
    // The view: the room its filter keeps, its cost, and the totals.
    assert!(h.query_by_label("Living").is_some() && h.query_by_label("Kitchen").is_none());
    assert!(h.query_by_label("3600").is_some(), "Living's cost: 30 m² at 120");
    assert!(h.query_by_label("Total").is_some());
    // On a sheet, Place View offers it; a double click on it there opens it.
    press(&mut h, "View");
    press(&mut h, "Sheet");
    press(&mut h, "View");
    press(&mut h, "Place View");
    press(&mut h, "Schedule: Room Areas");
    let vp = ids(&h, "viewport")[0];
    let at = field(&h, vp, "at");
    double_click(&mut h, P2(at[0].as_f64().unwrap(), at[1].as_f64().unwrap()));
    assert_eq!(h.state().sheet, Sheet::Table(sc));
    // Its own name in the browser, under the schedules every project has.
    browse(&mut h, "Room Schedule");
    assert_eq!(h.state().sheet, Sheet::Schedule("room"));
    browse_entry(&mut h, "Room Areas");
    assert_eq!(h.state().sheet, Sheet::Table(sc));
}

#[test]
fn changing_a_schedule_every_project_has_makes_it_the_projects() {
    let mut h = app(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}\ndoor {{\"at\": [6000, 0], \"type\": \"Single 800 x 2100\"}}"));
    browse(&mut h, "Door Schedule");
    assert!(ids(&h, "schedule").is_empty(), "opened, it's still every project's");
    // Low in Properties, it may lie below the palette's fold: pressed as a screen reader does.
    h.get_by_label("Edit Sorting/Grouping").click_accesskit();
    h.run();
    press(&mut h, "Add Sort");
    pick(&mut h, Role::CheckBox, "Itemize every instance");
    press(&mut h, "Close");
    let sc = ids(&h, "schedule");
    assert_eq!(sc.len(), 1);
    assert_eq!(h.state().sheet, Sheet::Schedule("door"), "the same view, the project's now");
    match h.state().session.model.elements.get(sc[0]) {
        Some(Element::Schedule(s)) => assert!(s.grouped && s.sort.len() == 1 && s.name == "Door Schedule"),
        e => panic!("{e:?}"),
    }
    // Grouped by mark, every door its row still; by type, a row for each type.
    assert!(h.query_by_label("Single 800 x 2100").is_some() && h.query_by_label("Single 900 x 2100").is_some());
    // Renamed, it's a schedule of the project's like any other, and its tab stays on it.
    type_role(&mut h, Role::TextInput, "View Name", "Doors by Type");
    assert_eq!(h.state().sheet, Sheet::Table(sc[0]));
    assert!(h.query_by_label("Single 800 x 2100").is_some());
}

#[test]
fn a_new_schedule_named_as_another_view_stays_open_to_be_named_again() {
    let mut h = app(BOX);
    press(&mut h, "View");
    press(&mut h, "Schedules");
    press(&mut h, "Schedule/Quantities…");
    pick(&mut h, Role::Button, "Walls");
    type_into(&mut h, "Name:", "Level 1");
    press(&mut h, "OK");
    assert!(ids(&h, "schedule").is_empty());
    assert!(h.state().status.error, "it says why");
    type_into(&mut h, "Name:", "Wall Lengths");
    press(&mut h, "OK");
    assert_eq!(ids(&h, "schedule").len(), 1);
}

/// Move the cursor over the plan to a model point; what the tool's pick there would snap to.
fn hover(h: &mut Harness<BimApp>, p: P2) -> Option<bimcraft_core::snap::Found> {
    let at = h.state().screen(p);
    h.hover_at(at);
    h.run();
    h.state().canvas.found.get()
}

/// Degrees from `a` to `b`, counter-clockwise from east.
fn degrees(a: P2, b: P2) -> f64 {
    (b.1 - a.1).atan2(b.0 - a.0).to_degrees()
}

#[test]
fn walls_drawn_near_an_angle_guide_snap_onto_it_and_turn_exact_corners() {
    let mut h = app("");
    tool(&mut h, "Wall");
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(3470.0, 2020.0));
    key(&mut h, Key::Enter);
    let (a, b) = walls(&h)[0];
    assert!((degrees(a, b) - 30.0).abs() < 1e-3, "{a:?} {b:?}: 30.2° snaps onto 30°");
    // 20° is no multiple of 15°, so it's free; but 90° from it is exact all the same. Below the first, clear of the Options
    // Bar floating over the view's top.
    tool(&mut h, "Wall");
    click(&mut h, P2(0.0, -3000.0));
    click(&mut h, P2(3000.0, -1908.0));
    let (p, q) = match &h.state().tool {
        Tool::Wall(chain) => (chain[0], chain[1]),
        t => panic!("{t:?}"),
    };
    assert!((degrees(p, q) - 20.0).abs() < 0.2, "{p:?} {q:?}");
    let turn = (degrees(p, q) + 90.0).to_radians();
    click(&mut h, q + P2(turn.cos(), turn.sin()) * 2000.0 + P2(25.0, 10.0));
    key(&mut h, Key::Enter);
    let m = walls(&h);
    let (first, second) = (m[1], m[2]);
    assert_eq!(first.1, second.0);
    assert!((degrees(second.0, second.1) - degrees(first.0, first.1) - 90.0).abs() < 1e-2, "{first:?} {second:?}");
}

#[test]
fn the_cursor_snaps_to_ends_middles_and_perpendicular_feet_and_the_pick_says_which() {
    use bimcraft_core::snap::Kind;
    let mut h = app(BOX);
    tool(&mut h, "Wall");
    let kind = |h: &mut Harness<BimApp>, p: P2| hover(h, p).map(|f| (f.kind, f.at));
    assert_eq!(kind(&mut h, P2(8030.0, 20.0)), Some((Kind::Endpoint, P2(8000.0, 0.0))));
    assert_eq!(kind(&mut h, P2(4030.0, 20.0)), Some((Kind::Midpoint, P2(4000.0, 0.0))));
    assert_eq!(kind(&mut h, P2(2040.0, 30.0)), Some((Kind::Nearest, P2(2040.0, 0.0))));
    assert_eq!(kind(&mut h, P2(4020.0, 3000.0)), Some((Kind::Free, P2(4020.0, 3000.0))));
    // Not over a tool that picks points: nothing to say.
    key(&mut h, Key::Escape);
    // A second wall far off, so the view takes in where the first point goes.
    let mut h = app(r#"wall {"points": [[0,0],[6000,2000]]}
wall {"points": [[3000,5500],[4000,5500]]}"#);
    tool(&mut h, "Wall");
    click(&mut h, P2(1000.0, 4000.0));
    let f = hover(&mut h, P2(2130.0, 680.0)).unwrap();
    assert_eq!(f.kind, Kind::Perpendicular);
    assert!(f.at.dist(P2(2100.0, 700.0)) < 1e-3, "{:?}", f.at);
}

#[test]
fn manage_snaps_switches_each_snap_and_the_angle_steps() {
    use bimcraft_core::snap::Kind;
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Snaps");
    // The window takes the keys while it's open: a shortcut typed does nothing.
    assert!(h.state().show_snaps && h.query_by_label("Object Snaps").is_some());
    shortcut(&mut h, "WA");
    assert_eq!(h.state().tool, Tool::Modify);
    pick(&mut h, Role::CheckBox, "Midpoint");
    press(&mut h, "Close");
    assert!(!h.state().snaps.midpoint && h.state().snaps.endpoint);
    tool(&mut h, "Wall");
    assert_eq!(hover(&mut h, P2(4030.0, 20.0)).map(|f| f.kind), Some(Kind::Nearest), "no middle to stick to now");
    // The angle steps: off, a drawn wall goes where the cursor does.
    press(&mut h, "Manage");
    press(&mut h, "Snaps");
    pick(&mut h, Role::ComboBox, "Angle steps");
    press(&mut h, "Off");
    press(&mut h, "Close");
    assert_eq!(h.state().snaps.angle, 0.0);
    key(&mut h, Key::Escape);
    let mut h = app("");
    h.state_mut().snaps.angle = 0.0;
    tool(&mut h, "Wall");
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(3470.0, 2020.0));
    key(&mut h, Key::Enter);
    assert_eq!(walls(&h)[0].1, P2(3470.0, 2020.0));
}

#[test]
fn walls_drawn_near_alignment_guide_snap_to_other_walls_reference_points() {
    let mut h = app("");
    // First wall from (0, 0) to (5000, 0)
    tool(&mut h, "Wall");
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(5000.0, 0.0));
    key(&mut h, Key::Enter);

    // Second wall starting at (0, 3000)
    tool(&mut h, "Wall");
    click(&mut h, P2(0.0, 3000.0));
    // Click near X = 5000 (e.g. 5005, 3000) within tolerance
    click(&mut h, P2(5005.0, 3000.0));
    key(&mut h, Key::Enter);

    let w = walls(&h);
    assert_eq!(w.len(), 2);
    assert!(w[1].0.dist(P2(0.0, 3000.0)) < 1e-2);
    // The second wall's endpoint snapped to exactly X = 5000.0 aligned with the first wall's endpoint
    assert_eq!(w[1].1.0, 5000.0);
    assert!((w[1].1.1 - 3000.0).abs() < 1e-2);
}

#[test]
fn manage_snaps_toggles_alignment_guides() {
    let mut h = app("");
    assert!(h.state().snaps.alignment_guides);
    press(&mut h, "Manage");
    press(&mut h, "Snaps");
    assert!(h.query_by_label("Alignment Guides").is_some());
    pick(&mut h, Role::CheckBox, "Alignment Guides");
    press(&mut h, "Close");
    assert!(!h.state().snaps.alignment_guides);
}

#[test]
fn grids_drawn_near_alignment_guide_snap_to_other_grids_reference_points() {
    let mut h = app("");
    // First grid from (0, 0) to (0, 5000)
    tool(&mut h, "Grid");
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(0.0, 5000.0));

    // Second grid starting at (3000, 0)
    tool(&mut h, "Grid");
    click(&mut h, P2(3000.0, 0.0));
    // Second point near (3000, 5003) aligned with Y=5000
    click(&mut h, P2(3000.0, 5003.0));

    let grid = ids(&h, "grid");
    assert_eq!(grid.len(), 2);
    assert_eq!(field(&h, grid[1], "start"), json!([3000.0, 0.0]));
    assert_eq!(field(&h, grid[1], "end"), json!([3000.0, 5000.0]));
}

#[test]
fn shortcut_toggles_alignment_guides() {
    let mut h = app("");
    assert!(h.state().snaps.alignment_guides);
    key(&mut h, Key::F8);
    assert!(!h.state().snaps.alignment_guides);
    key(&mut h, Key::F8);
    assert!(h.state().snaps.alignment_guides);
    shortcut(&mut h, "AG");
    assert!(!h.state().snaps.alignment_guides);
    shortcut(&mut h, "OG");
    assert!(h.state().snaps.alignment_guides);
}

#[test]
fn the_measure_tool_lays_a_ruler_between_two_snapped_points_and_esc_takes_it_away() {
    let mut h = app(BOX);
    press(&mut h, "Annotate");
    press(&mut h, "Measure");
    assert_eq!(h.state().tool, Tool::Measure(None));
    click(&mut h, P2(0.0, 0.0));
    assert_eq!(h.state().tool, Tool::Measure(Some(P2(0.0, 0.0))));
    click(&mut h, P2(3000.0, 4000.0));
    assert_eq!(h.state().measured, Some((P2(0.0, 0.0), P2(3000.0, 4000.0))));
    assert_eq!(h.state().tool, Tool::Measure(None), "ready for another");
    key(&mut h, Key::Escape);
    assert_eq!((h.state().measured, h.state().tool.clone()), (None, Tool::Measure(None)));
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Modify);
    // ME is its shortcut, and a ruler left on show goes with the tool.
    shortcut(&mut h, "ME");
    assert_eq!(h.state().tool, Tool::Measure(None));
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(8000.0, 0.0));
    assert!(h.state().measured.is_some());
    tool(&mut h, "Wall");
    assert_eq!(h.state().measured, None);
}

#[test]
fn a_selected_walls_ends_are_grips_to_drag_and_ctrl_turns_it_about_the_other_end() {
    let mut h = app(BOX);
    click(&mut h, P2(2000.0, 0.0));
    drag(&mut h, P2(8000.0, 0.0), P2(8000.0, -1000.0));
    assert_eq!(walls(&h)[0], (P2(0.0, 0.0), P2(8000.0, -1000.0)));
    assert_eq!(walls(&h)[1].0, P2(8000.0, -1000.0), "the wall joined there followed");
    assert_eq!(h.state().selection, ids(&h, "wall")[..1], "still selected, to drag again");
    // Ctrl keeps its length: it turns about its start toward the cursor.
    let length = walls(&h)[0].0.dist(walls(&h)[0].1);
    h.event(Event::ModifiersChanged(Modifiers::COMMAND));
    h.run();
    drag(&mut h, P2(8000.0, -1000.0), P2(4000.0, 1500.0));
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run();
    let (a, b) = walls(&h)[0];
    assert_eq!(a, P2(0.0, 0.0));
    assert!((a.dist(b) - length).abs() < 1e-2, "{} {length}", a.dist(b));
    assert!((degrees(a, b) - degrees(P2(0.0, 0.0), P2(4000.0, 1500.0))).abs() < 1e-2, "{b:?}");
    // The start is a grip too, and the other end stays.
    drag(&mut h, P2(0.0, 0.0), P2(-500.0, 200.0));
    assert_eq!((walls(&h)[0].0, walls(&h)[0].1), (P2(-500.0, 200.0), b));
    // A drag that starts away from the ends moves the wall, as it did, not one end.
    let (a, b) = walls(&h)[0];
    drag(&mut h, (a + b) * 0.5, (a + b) * 0.5 + P2(0.0, 500.0));
    let (c, d) = walls(&h)[0];
    assert!(c.1 > a.1 + 400.0 && d.1 > b.1 + 400.0, "{a:?} {b:?} -> {c:?} {d:?}: both ends went up");
}

#[test]
fn a_selected_walls_angle_is_typed_to_turn_it_about_either_end() {
    let mut h = app(BOX);
    click(&mut h, P2(2000.0, 0.0));
    h.get_by_label_contains("Turn the wall about its start").click();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text("10".into()));
    h.run();
    key(&mut h, Key::Enter);
    // The wall joined at its end stays joined: the corner slides up the wall it met.
    let want = 8000.0 * 10f64.to_radians().tan();
    let (a, b) = walls(&h)[0];
    assert_eq!(a, P2(0.0, 0.0));
    assert!(b.0 == 8000.0 && (b.1 - want).abs() < 1e-2, "{b:?}");
    assert!((degrees(a, b) - 10.0).abs() < 1e-3);
    assert_eq!(walls(&h)[1].0, b, "the wall joined at that end followed");
    // About its end: the end stays, the start swings, level again.
    h.get_by_label_contains("Turn the wall about its end").click();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text("0".into()));
    h.run();
    key(&mut h, Key::Enter);
    let (a2, b2) = walls(&h)[0];
    assert_eq!(b2, b);
    assert!(a2.0 == 0.0 && (a2.1 - want).abs() < 1e-2 && degrees(a2, b2).abs() < 1e-3, "{a2:?}");
    // Not an angle: it says so, and nothing moves.
    h.get_by_label_contains("Turn the wall about its start").click();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text("steep".into()));
    h.run();
    key(&mut h, Key::Enter);
    assert!(h.state().status.error && h.state().status.text.contains("steep"), "{:?}", h.state().status.text);
    assert_eq!(walls(&h)[0], (a2, b2));
}

#[test]
fn a_wall_with_nothing_joined_turns_about_an_end_at_its_own_length() {
    let mut h = app(r#"wall {"points": [[0,0],[8000,0]]}
wall {"points": [[0,6000],[1000,6000]]}"#);
    click(&mut h, P2(2000.0, 0.0));
    h.get_by_label_contains("Turn the wall about its start").click();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text("30".into()));
    h.run();
    key(&mut h, Key::Enter);
    let (a, b) = walls(&h)[0];
    assert_eq!(a, P2(0.0, 0.0));
    assert!((degrees(a, b) - 30.0).abs() < 1e-6 && (a.dist(b) - 8000.0).abs() < 1e-2, "{b:?}");
}

#[test]
fn a_snap_shortcut_overrides_the_snaps_for_the_next_pick_only() {
    use crate::Once;
    use bimcraft_core::snap::Kind;
    let mut h = app(BOX);
    tool(&mut h, "Wall");
    let kind = |h: &mut Harness<BimApp>, p: P2| hover(h, p).map(|f| f.kind);
    // SE: ends alone; a middle beside the cursor is passed by.
    shortcut(&mut h, "SE");
    assert!(h.query_by_label("Snap: Endpoint (next point)").is_some(), "the status bar says so");
    assert_eq!(kind(&mut h, P2(4030.0, 20.0)), Some(Kind::Free));
    assert_eq!(kind(&mut h, P2(8030.0, 20.0)), Some(Kind::Endpoint));
    shortcut(&mut h, "SM");
    assert_eq!(kind(&mut h, P2(8030.0, 20.0)), Some(Kind::Free));
    assert_eq!(kind(&mut h, P2(4030.0, 20.0)), Some(Kind::Midpoint));
    // Each letter takes its own snap; SO takes none, SS puts them all back.
    for (letters, once) in [
        ("SE", Once::Only(Kind::Endpoint)),
        ("SM", Once::Only(Kind::Midpoint)),
        ("SI", Once::Only(Kind::Intersection)),
        ("SC", Once::Only(Kind::Centre)),
        ("SP", Once::Only(Kind::Perpendicular)),
        ("SN", Once::Only(Kind::Nearest)),
        ("SO", Once::Off),
    ] {
        shortcut(&mut h, letters);
        assert_eq!(h.state().snap_once, Some(once), "{letters}");
    }
    assert!(h.query_by_label("Snaps off (next point)").is_some());
    assert_eq!(kind(&mut h, P2(4030.0, 20.0)), Some(Kind::Free));
    shortcut(&mut h, "SS");
    assert_eq!(h.state().snap_once, None);
    assert_eq!(kind(&mut h, P2(4030.0, 20.0)), Some(Kind::Midpoint));
    // One pick: the click goes where it was, unsnapped, and the snaps are back.
    shortcut(&mut h, "SO");
    click(&mut h, P2(4030.0, 20.0));
    assert_eq!(h.state().tool, Tool::Wall(vec![P2(4030.0, 20.0)]));
    assert_eq!((h.state().snap_once, kind(&mut h, P2(8030.0, 20.0))), (None, Some(Kind::Endpoint)));
    // Esc takes it away first, leaving the chain; a second Esc drops that.
    shortcut(&mut h, "SO");
    key(&mut h, Key::Escape);
    assert_eq!(h.state().snap_once, None);
    assert_eq!(h.state().tool, Tool::Wall(vec![P2(4030.0, 20.0)]), "the chain stays");
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Wall(Vec::new()));
    // Another tool takes it away too.
    shortcut(&mut h, "SE");
    tool(&mut h, "Door");
    assert_eq!(h.state().snap_once, None);
    // And so does opening another view.
    shortcut(&mut h, "SE");
    browse(&mut h, "{3D}");
    assert_eq!(h.state().snap_once, None);
}

#[test]
fn a_snap_override_is_used_up_by_a_typed_length_and_by_enter_too() {
    let mut h = app(BOX);
    tool(&mut h, "Wall");
    click(&mut h, P2(1000.0, 1000.0));
    hover(&mut h, P2(4000.0, 1000.0));
    shortcut(&mut h, "SO");
    h.event(Event::Text("3000".into()));
    h.run();
    key(&mut h, Key::Enter);
    assert_eq!(h.state().tool, Tool::Wall(vec![P2(1000.0, 1000.0), P2(4000.0, 1000.0)]), "3000 mm along");
    assert_eq!(h.state().snap_once, None, "the typed point was the one");
    shortcut(&mut h, "SE");
    key(&mut h, Key::Enter);
    assert_eq!(h.state().snap_once, None, "so was the Enter that ended the chain");
}

#[test]
fn a_ruler_stays_in_the_view_it_was_laid_in() {
    let mut h = app(BOX);
    shortcut(&mut h, "ME");
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(6000.0, 4000.0));
    assert!(h.state().measured.is_some());
    browse(&mut h, "{3D}");
    assert_eq!(h.state().measured, None, "not carried to another view");
    shortcut(&mut h, "ME");
    assert_eq!(h.state().measured, None, "nor back from it");
}

#[test]
fn committing_a_walls_angle_unchanged_turns_nothing() {
    // 30.2044°, which the label reads as 30.2°.
    let mut h = app(r#"wall {"points": [[0,0],[3470,2020]]}"#);
    click(&mut h, P2(1735.0, 1010.0));
    let before = (walls(&h), h.state().session.history().len());
    h.get_by_label_contains("Turn the wall about its start").click();
    h.run();
    key(&mut h, Key::Enter);
    assert_eq!((walls(&h), h.state().session.history().len()), before, "nothing moved, and no undo step");
    // Typed over, it turns.
    h.get_by_label_contains("Turn the wall about its start").click();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text("30".into()));
    h.run();
    key(&mut h, Key::Enter);
    let (a, b) = walls(&h)[0];
    assert!((degrees(a, b) - 30.0).abs() < 1e-6, "{a:?} {b:?}");
}

#[test]
fn an_arcs_through_point_sticks_to_what_is_there_but_not_to_angle_guides() {
    use bimcraft_core::snap::Kind;
    let mut h = app(BOX);
    tool(&mut h, "Wall");
    press(&mut h, "Start-End-Radius Arc");
    click(&mut h, P2(1000.0, 3000.0));
    click(&mut h, P2(5000.0, 3000.0));
    // 90° up from its end is a guide for a line, not for the point an arc passes through.
    let f = hover(&mut h, P2(5030.0, 4500.0)).unwrap();
    assert_eq!((f.kind, f.at), (Kind::Free, P2(5030.0, 4500.0)));
}

#[test]
fn new_keeps_the_snaps_as_they_were_set() {
    let mut h = app(BOX);
    h.state_mut().snaps.angle = 5.0;
    h.state_mut().snaps.midpoint = false;
    h.key_press_modifiers(Modifiers::COMMAND, Key::N);
    h.run();
    assert!(h.state().session.model.elements.values().all(|e| e.category() == "level"), "a new project");
    assert_eq!((h.state().snaps.angle, h.state().snaps.midpoint), (5.0, false));
}

#[test]
fn a_typed_length_too_long_is_refused_and_a_far_point_does_not_stall_the_guide() {
    let mut h = app(BOX);
    tool(&mut h, "Wall");
    click(&mut h, P2(1000.0, 1000.0));
    hover(&mut h, P2(4000.0, 1000.0));
    // Past the largest coordinate: refused, the chain as it was.
    h.event(Event::Text("2000000000".into()));
    h.run();
    key(&mut h, Key::Enter);
    assert!(h.state().status.error, "{:?}", h.state().status.text);
    assert_eq!(h.state().tool, Tool::Wall(vec![P2(1000.0, 1000.0)]));
    // Within it, a point a million metres off: the dashed guide back from it is a screen long, not
    // a dash for every ten pixels of the way.
    h.event(Event::Text("900000000".into()));
    h.run();
    key(&mut h, Key::Enter);
    assert!(matches!(&h.state().tool, Tool::Wall(chain) if chain.len() == 2), "{:?}", h.state().tool);
    assert!(hover(&mut h, P2(3000.0, 1000.0)).is_some_and(|f| f.guide.is_some()), "its guide, drawn");
}

/// The graphics the plan of `level` holds.
fn plan_graphics(h: &Harness<BimApp>, level: Id) -> bimcraft_core::graphics::Graphics {
    bimcraft_core::graphics::graphics_of(&h.state().session.model, level).cloned().unwrap()
}

#[test]
fn visibility_graphics_hides_and_overrides_by_category_in_the_active_view_alone() {
    let mut h = app(&format!("{BOX}\nroom {{\"point\": [4000, 3000], \"name\": \"Living\"}}"));
    shortcut(&mut h, "VV");
    assert!(h.query_by_label("Visibility/Graphics Overrides for Floor Plan: Level 1").is_some());
    // Edited in the dialog, a draft: nothing is the model's until Apply or OK.
    pick(&mut h, Role::CheckBox, "Rooms");
    assert!(plan_graphics(&h, 100000).is_default());
    // Walls drawn their own way: a cut line weight, overriding it with a box and its start.
    pick(&mut h, Role::Button, "Walls: Cut Lines");
    pick(&mut h, Role::CheckBox, "Cut: Line weight");
    press(&mut h, "OK");
    pick(&mut h, Role::Button, "Walls: Projection Lines");
    pick(&mut h, Role::CheckBox, "Projection: Line color");
    press(&mut h, "OK");
    press(&mut h, "OK");
    let g = plan_graphics(&h, 100000);
    assert!(h.query_by_label("Visibility/Graphics Overrides for Floor Plan: Level 1").is_none(), "OK closes it");
    assert_eq!(g.categories["wall"].cut.weight, Some(Pen(10)), "a cut wall's own pen");
    assert_eq!(g.categories["wall"].projection.color, Some([40, 40, 40]));
    assert!(g.hides("room") && !g.hides("wall"));
    // Level 2's plan is its own view.
    assert!(plan_graphics(&h, 100001).is_default());
    // Cancel throws a draft away; unticking a box again takes the override back.
    shortcut(&mut h, "VG");
    pick(&mut h, Role::CheckBox, "Rooms");
    press(&mut h, "Cancel");
    assert!(plan_graphics(&h, 100000).hides("room"));
    shortcut(&mut h, "VV");
    pick(&mut h, Role::CheckBox, "Rooms");
    pick(&mut h, Role::Button, "Walls: Cut Lines");
    pick(&mut h, Role::CheckBox, "Cut: Line weight");
    press(&mut h, "OK");
    pick(&mut h, Role::Button, "Walls: Projection Lines");
    pick(&mut h, Role::CheckBox, "Projection: Line color");
    press(&mut h, "OK");
    press(&mut h, "OK");
    assert!(plan_graphics(&h, 100000).is_default(), "{:?}", plan_graphics(&h, 100000));
    // The annotation categories have a tab.
    shortcut(&mut h, "VV");
    press(&mut h, "Annotation Categories");
    assert!(h.query_by_role_and_label(Role::CheckBox, "Grids").is_some() && h.query_by_role_and_label(Role::CheckBox, "Walls").is_none());
    press(&mut h, "Cancel");
}

#[test]
fn visibility_graphics_is_a_table_each_cell_overriding_its_own_part() {
    let mut h = app(BOX);
    shortcut(&mut h, "VV");
    // Each column is a cell to override, with Halftone a box in the row and Detail Level the view's, for now.
    for cell in ["Projection Lines", "Projection Patterns", "Transparency", "Cut Lines", "Cut Patterns"] {
        assert!(h.query_by_role_and_label(Role::Button, &format!("Floors: {cell}")).is_some(), "{cell}");
    }
    assert!(painted_text(&h).contains(&"By View".to_string()));
    pick(&mut h, Role::CheckBox, "Floors: Halftone");
    // A cell's dialog changes only its own part; Cancel drops it.
    pick(&mut h, Role::Button, "Floors: Cut Patterns");
    assert!(h.query_by_label("Pattern Graphics").is_some() && h.query_by_role_and_label(Role::CheckBox, "Cut: Line weight").is_none());
    pick(&mut h, Role::CheckBox, "Cut: Fill color");
    press(&mut h, "Cancel");
    assert!(h.query_by_label("Pattern Graphics").is_none());
    pick(&mut h, Role::Button, "Floors: Transparency");
    assert!(h.query_by_label("Surface Transparency").is_some());
    press(&mut h, "Cancel");
    // The search narrows the rows; None hides what it found and no more.
    type_role(&mut h, Role::TextInput, "Category name search:", "wal");
    assert!(h.query_by_role_and_label(Role::CheckBox, "Floors").is_none());
    press(&mut h, "None");
    press(&mut h, "OK");
    let g = plan_graphics(&h, 100000);
    assert!(g.categories["floor"].halftone && g.categories["floor"].cut.fill.is_none());
    assert!(g.hides("wall") && !g.hides("floor") && !g.hides("door"));
    // Invert shows them again and hides the rest; All shows everything.
    shortcut(&mut h, "VV");
    press(&mut h, "Invert");
    press(&mut h, "All");
    press(&mut h, "OK");
    assert!(!plan_graphics(&h, 100000).hides("wall"));
    // Object Styles… stands in its place, for what isn't overridden, and gives it back as it was.
    shortcut(&mut h, "VV");
    pick(&mut h, Role::CheckBox, "Doors");
    press(&mut h, "Object Styles…");
    assert!(h.query_by_role_and_label(Role::ComboBox, "Walls: Cut").is_some());
    // It settles over a few frames, as a window opened from another does.
    for _ in 0..6 {
        h.step();
    }
    press(&mut h, "Cancel");
    assert!(h.state().object_styles_dialog.is_none());
    press(&mut h, "OK");
    assert!(h.state().vg_dialog.is_none());
    assert!(plan_graphics(&h, 100000).hides("door"), "the draft waited");
}

/// What Level 1's plan shows of the model it is drawn from, as the view control bar leaves it.
fn plan_shows(h: &mut Harness<BimApp>, id: Id) -> bool {
    let model = h.state_mut().view_model();
    bimcraft_core::graphics::Look::of(&model, 100000).shows(id)
}

#[test]
fn temporary_hide_and_isolate_change_what_a_view_shows_and_nothing_in_the_project() {
    let mut h = app(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}"));
    let (walls, door) = (ids(&h, "wall"), ids(&h, "door")[0]);
    let (model, steps) = (Arc::clone(&h.state().session.model), h.state().session.history().len());
    // Hide Element (HH): the south wall is gone from the view, and the selection with it.
    click(&mut h, P2(5000.0, 0.0));
    assert_eq!(h.state().selection, [walls[0]]);
    shortcut(&mut h, "HH");
    assert!(h.state().selection.is_empty());
    assert!(!plan_shows(&mut h, walls[0]) && plan_shows(&mut h, walls[1]));
    // Hide Category (HC), from the bar: every wall.
    click(&mut h, P2(8000.0, 3000.0));
    press(&mut h, "Temporary Hide/Isolate");
    press(&mut h, "Hide Category (HC)");
    assert!(walls.iter().all(|w| !plan_shows(&mut h, *w)) && plan_shows(&mut h, door));
    // Nothing of it is the project's: no new model, no undo step, nothing to save.
    assert!(Arc::ptr_eq(&model, &h.state().session.model) && h.state().session.history().len() == steps);
    assert!(!h.state().dirty());
    // Reset (HR) brings it all back.
    shortcut(&mut h, "HR");
    assert!(walls.iter().all(|w| plan_shows(&mut h, *w)));
    // Isolate Element (HI): only the door.
    click(&mut h, P2(2000.0, 0.0));
    assert_eq!(h.state().selection, [door]);
    shortcut(&mut h, "HI");
    assert!(plan_shows(&mut h, door) && walls.iter().all(|w| !plan_shows(&mut h, *w)));
    shortcut(&mut h, "HR");
    // Isolate Element picks the element, not its category: one wall of four, and nothing else.
    click(&mut h, P2(5000.0, 0.0));
    shortcut(&mut h, "HI");
    assert!(plan_shows(&mut h, walls[0]) && walls[1..].iter().all(|w| !plan_shows(&mut h, *w)) && !plan_shows(&mut h, door));
    shortcut(&mut h, "HR");
    // Isolate Category (IC): the walls, and not the door.
    click(&mut h, P2(8000.0, 3000.0));
    shortcut(&mut h, "IC");
    assert!(walls.iter().all(|w| plan_shows(&mut h, *w)) && !plan_shows(&mut h, door));
    // What is hidden for now is the view's: another view has all of it.
    h.state_mut().sheet = Sheet::View3D;
    let model = h.state_mut().view_model();
    assert!(bimcraft_core::graphics::Look::of_3d(&model).shows(door));
}

#[test]
fn the_temporary_hide_menu_greys_out_what_needs_a_selection_as_revits_does() {
    let mut h = app(BOX);
    let items = ["Hide Element (HH)", "Hide Category (HC)", "Isolate Element (HI)", "Isolate Category (IC)"];
    // Nothing selected: nothing to hide or isolate, and the menu says so before it is tried.
    press(&mut h, "Temporary Hide/Isolate");
    for label in items {
        assert!(is_disabled(&h, Role::Button, label), "{label}");
    }
    assert!(is_disabled(&h, Role::Button, "Reset Temporary Hide/Isolate (HR)"));
    key(&mut h, Key::Escape);
    // Something drawn selected: all four work.
    click(&mut h, P2(5000.0, 0.0));
    press(&mut h, "Temporary Hide/Isolate");
    for label in items {
        assert!(!is_disabled(&h, Role::Button, label), "{label}");
    }
    key(&mut h, Key::Escape);
}

/// Whether a frame in the colour Revit draws round a view that hides or reveals for now was painted.
fn framed(h: &Harness<BimApp>) -> bool {
    h.output().shapes.iter().any(|c| matches!(&c.shape, egui::Shape::Rect(r) if r.stroke.color == egui::Color32::from_rgb(0, 190, 220)))
}

/// Whether the bar's button named `label` is lit.
fn lit(h: &Harness<BimApp>, label: &str) -> bool {
    use egui_kittest::kittest::NodeT;
    h.get_by_role_and_label(Role::Button, label).accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

#[test]
fn isolating_leaves_out_the_annotation_the_selection_isnt_too() {
    let mut h = app(&format!("{BOX}\ngrid {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}"));
    let (walls, grid) = (ids(&h, "wall"), ids(&h, "grid")[0]);
    let seen = |h: &mut Harness<BimApp>, id: Id| {
        let model = h.state_mut().view_model();
        bimcraft_core::graphics::Look::of(&model, 100000).shows(id)
    };
    assert!(seen(&mut h, grid));
    // Isolate Category (IC) on a wall: the walls, and neither the grid nor anything else.
    click(&mut h, P2(5000.0, 0.0));
    shortcut(&mut h, "IC");
    assert!(walls.iter().all(|w| seen(&mut h, *w)) && !seen(&mut h, grid));
    shortcut(&mut h, "HR");
    assert!(seen(&mut h, grid));
    // So does Isolate Element (HI).
    click(&mut h, P2(5000.0, 0.0));
    shortcut(&mut h, "HI");
    assert!(seen(&mut h, walls[0]) && !seen(&mut h, grid) && !seen(&mut h, walls[1]));
}

#[test]
fn a_temporary_hide_the_views_template_forbids_is_dropped_and_says_why() {
    let mut h = app(BOX);
    let walls = ids(&h, "wall");
    h.state_mut().session.execute("template", &json!({"name": "T", "from": 100000})).unwrap();
    h.state_mut().session.execute("graphics", &json!({"view": 100000, "template": "T"})).unwrap();
    h.run();
    // The template sets which categories the view hides; hiding one for now asks the same of it.
    click(&mut h, P2(5000.0, 0.0));
    shortcut(&mut h, "HC");
    h.run();
    let status = &h.state().status;
    assert!(status.error && status.text.contains("`T`"), "{}", status.text);
    assert!(h.state().temporary().is_none(), "what can't be done is not kept");
    assert!(walls.iter().all(|w| plan_shows(&mut h, *w)));
    // An element is the view's own to hide, whatever its template says.
    click(&mut h, P2(5000.0, 0.0));
    shortcut(&mut h, "HH");
    assert!(!plan_shows(&mut h, walls[0]) && h.state().temporary().is_some());
    // And so is revealing what the view hides, and applying an element's hide to it: no category in either.
    h.state_mut().status = crate::Status { text: String::new(), error: false };
    shortcut(&mut h, "RH");
    assert!(!plan_shows(&mut h, walls[0]), "revealing shows what the view hides for good, and the wall is hidden for now");
    assert!(!h.state().status.error && lit(&h, "Reveal Hidden Elements"), "{}", h.state().status.text);
    shortcut(&mut h, "RH");
    press(&mut h, "Temporary Hide/Isolate");
    press(&mut h, "Apply Hide/Isolate to View");
    assert!(plan_graphics(&h, 100000).elements[&gid(&h, walls[0])].hidden, "{}", h.state().status.text);
}

#[test]
fn a_plan_and_a_section_draw_what_they_hide_for_now() {
    let mut h = app(&format!("{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}"));
    // The plan: the south wall can't be clicked once the view hides it, and can again after a reset.
    click(&mut h, P2(5000.0, 0.0));
    let south = h.state().selection.first().copied();
    assert_eq!(south, Some(ids(&h, "wall")[0]));
    shortcut(&mut h, "HH");
    h.run();
    click(&mut h, P2(5000.0, 0.0));
    assert_ne!(h.state().selection.first().copied(), south, "what the plan hides for now isn't there to click");
    shortcut(&mut h, "HR");
    h.run();
    click(&mut h, P2(5000.0, 0.0));
    assert_eq!(h.state().selection.first().copied(), south);
    browse(&mut h, "Section 1");
    // The west wall, where the section's line cuts it: 1000 along from its start, half way up.
    click(&mut h, P2(1000.0, 1500.0));
    let wall = h.state().selection.first().copied();
    assert!(
        wall.is_some_and(|w| h.state().session.model.elements.get(w).is_some_and(|e| e.category() == "wall")),
        "{:?}",
        h.state().selection
    );
    shortcut(&mut h, "HH");
    h.run();
    click(&mut h, P2(1000.0, 1500.0));
    assert_ne!(h.state().selection.first().copied(), wall, "what the section hides for now isn't there to click");
    shortcut(&mut h, "HR");
    h.run();
    click(&mut h, P2(1000.0, 1500.0));
    assert_eq!(h.state().selection.first().copied(), wall);
}

#[test]
fn a_3d_view_following_a_template_shows_its_visual_style_greyed_out() {
    let mut h = app(BOX);
    let run = |h: &mut Harness<BimApp>, id: &str, p: serde_json::Value| h.state_mut().session.execute(id, &p).unwrap();
    run(&mut h, "visual_style", json!({"style": "wireframe"}));
    run(&mut h, "template", json!({"name": "Wire", "from": "{3D}"}));
    // Its own is shaded again, but the template says wireframe.
    run(&mut h, "graphics", json!({"view": "{3D}", "reset": true, "template": "Wire"}));
    press(&mut h, "Default 3D View");
    assert_eq!(h.state().sheet, Sheet::View3D);
    assert!(painted_text(&h).contains(&"Wireframe".to_string()) && !painted_text(&h).contains(&"Shaded".to_string()));
    assert!(is_disabled(&h, Role::Button, "Visual Style") && is_disabled(&h, Role::ComboBox, "Visual Style"));
    assert_eq!(combo_value(&h, "Visual Style"), "Wireframe");
    // WF, HL and SD are refused the same way, and say why.
    shortcut(&mut h, "HL");
    assert!(h.state().status.error && h.state().status.text.contains("`Wire`"), "{}", h.state().status.text);
}

#[test]
fn a_view_that_hides_isolates_or_reveals_wears_a_frame_and_lights_the_bar_and_says_so() {
    let mut h = app(BOX);
    let note = |h: &Harness<BimApp>| painted_text(h).into_iter().find(|t| t.contains("(HR resets)") || t.contains("Revealing"));
    assert!(!framed(&h) && !lit(&h, "Temporary Hide/Isolate") && !lit(&h, "Reveal Hidden Elements") && note(&h).is_none());
    click(&mut h, P2(5000.0, 0.0));
    shortcut(&mut h, "HH");
    assert!(framed(&h) && lit(&h, "Temporary Hide/Isolate") && !lit(&h, "Reveal Hidden Elements"));
    assert_eq!(note(&h).as_deref(), Some("Temporary Hide/Isolate (HR resets)"));
    // Revealing too: still hiding, and it says so.
    shortcut(&mut h, "RH");
    assert!(framed(&h) && lit(&h, "Reveal Hidden Elements"));
    assert_eq!(note(&h).as_deref(), Some("Temporary Hide/Isolate (HR resets)"));
    shortcut(&mut h, "HR");
    assert!(!framed(&h) && !lit(&h, "Temporary Hide/Isolate") && !lit(&h, "Reveal Hidden Elements") && note(&h).is_none());
    // Only revealing: framed, and the button's Reset takes it off.
    shortcut(&mut h, "RH");
    assert!(framed(&h) && lit(&h, "Reveal Hidden Elements") && !lit(&h, "Temporary Hide/Isolate"));
    assert_eq!(note(&h).as_deref(), Some("Revealing hidden elements (HR resets)"));
    press(&mut h, "Temporary Hide/Isolate");
    assert!(!is_disabled(&h, Role::Button, "Reset Temporary Hide/Isolate (HR)"), "there is something to reset");
    assert!(is_disabled(&h, Role::Button, "Apply Hide/Isolate to View"), "but nothing to apply");
    press(&mut h, "Reset Temporary Hide/Isolate (HR)");
    assert!(!framed(&h) && h.state().temporary().is_none());
}

#[test]
fn temporary_hide_and_isolate_want_a_selection_and_a_view_with_graphics_and_belong_to_one_view() {
    let mut h = app(&format!("{BOX}\nsheet {{\"name\": \"Plans\"}}"));
    let (walls, sheet) = (ids(&h, "wall"), ids(&h, "sheet")[0]);
    // Nothing selected: each says so, and nothing is hidden, least of all by an isolate of nothing.
    for letters in ["HH", "HC", "HI", "IC"] {
        h.state_mut().status = crate::Status { text: String::new(), error: false };
        shortcut(&mut h, letters);
        assert!(h.state().status.error && h.state().temporary().is_none(), "{letters}");
        assert!(walls.iter().all(|w| plan_shows(&mut h, *w)), "{letters}");
    }
    // A sheet has no graphics to hide in or reveal.
    h.state_mut().open_sheet(Sheet::Paper(sheet));
    h.run();
    for letters in ["RH", "HR"] {
        shortcut(&mut h, letters);
        assert!(h.state().temporary().is_none(), "{letters}");
    }
    h.state_mut().selection = vec![walls[0]];
    shortcut(&mut h, "HH");
    assert!(h.state().temporary().is_none() && h.state().status.error, "a sheet has no graphics to override");
    // A plan draws walls, not sheets: what it doesn't draw isn't there to hide.
    h.state_mut().open_sheet(Sheet::Plan(100000));
    h.run();
    h.state_mut().status = crate::Status { text: String::new(), error: false };
    h.state_mut().selection = vec![sheet];
    shortcut(&mut h, "HI");
    assert!(h.state().temporary().is_none() && h.state().status.error, "a sheet isn't drawn in a plan");
    // What one view hides for now another that hides the same keeps shown until it does: each is its own.
    let shows = |h: &mut Harness<BimApp>, level: Id| {
        let model = h.state_mut().view_model();
        bimcraft_core::graphics::Look::of(&model, level).shows(walls[0])
    };
    for level in [100_000, 100_001] {
        h.state_mut().open_sheet(Sheet::Plan(level));
        h.run();
        assert!(shows(&mut h, level), "level {level} shows it before");
        h.state_mut().selection = vec![walls[0]];
        shortcut(&mut h, "HH");
        assert!(!shows(&mut h, level), "level {level} hides it now");
    }
    h.state_mut().open_sheet(Sheet::Plan(100000));
    h.run();
    assert!(!shows(&mut h, 100_000));
}

#[test]
fn the_3d_view_draws_what_it_hides_for_now_and_the_project_again_after_a_reset() {
    let mut h = app(BOX);
    press(&mut h, "Default 3D View");
    assert_eq!(h.state().sheet, Sheet::View3D);
    // The model the 3D view last drew: the project's own, until something is hidden for now.
    let project = |h: &Harness<BimApp>| h.state().orbit.scene.as_ref().is_some_and(|(m, _)| Arc::ptr_eq(m, &h.state().session.model));
    assert!(project(&h));
    h.state_mut().selection = vec![ids(&h, "wall")[0]];
    shortcut(&mut h, "HH");
    h.run();
    assert!(!project(&h), "the picture is of the view's copy");
    shortcut(&mut h, "HR");
    h.run();
    assert!(project(&h));
}

#[test]
fn reveal_hidden_elements_shows_what_the_view_hides_for_good_and_apply_makes_a_hide_stick() {
    let mut h = app(BOX);
    let walls = ids(&h, "wall");
    click(&mut h, P2(5000.0, 0.0));
    shortcut(&mut h, "EH");
    assert!(!plan_shows(&mut h, walls[0]));
    shortcut(&mut h, "RH");
    assert!(plan_shows(&mut h, walls[0]), "revealed");
    let model = h.state_mut().view_model();
    let magenta =
        bimcraft_core::graphics::graphics_of(&model, 100000).and_then(|g| g.elements.get(&gid(&h, walls[0]))).cloned().unwrap_or_default();
    assert_eq!((magenta.projection.color, magenta.cut.color), (Some([255, 0, 255]), Some([255, 0, 255])), "drawn in magenta, cut or seen");
    assert!(
        !plan_graphics(&h, 100000).elements.is_empty() && plan_graphics(&h, 100000).elements[&gid(&h, walls[0])].hidden,
        "the project still hides it"
    );
    shortcut(&mut h, "RH");
    assert!(!plan_shows(&mut h, walls[0]), "hidden again");
    // A category the view hides for good is revealed too: every one of its members, in magenta.
    click(&mut h, P2(8000.0, 3000.0));
    shortcut(&mut h, "VH");
    assert!(plan_graphics(&h, 100000).hides("wall") && !plan_shows(&mut h, walls[2]));
    shortcut(&mut h, "RH");
    let model = h.state_mut().view_model();
    let colours = |id: &Id| {
        let o = bimcraft_core::graphics::graphics_of(&model, 100000)
            .and_then(|g| g.elements.get(&model.guid(*id)))
            .cloned()
            .unwrap_or_default();
        (o.projection.color, o.cut.color)
    };
    assert!(walls.iter().all(|w| plan_shows(&mut h, *w) && colours(w) == (Some([255, 0, 255]), Some([255, 0, 255]))));
    shortcut(&mut h, "RH");
    shortcut(&mut h, "VV");
    pick(&mut h, Role::CheckBox, "Walls");
    press(&mut h, "OK");
    assert!(!plan_graphics(&h, 100000).hides("wall"));
    // Apply Hide/Isolate to View: a temporary hide of another wall becomes the view's own.
    click(&mut h, P2(8000.0, 3000.0));
    shortcut(&mut h, "HH");
    press(&mut h, "Temporary Hide/Isolate");
    press(&mut h, "Apply Hide/Isolate to View");
    assert!(plan_graphics(&h, 100000).elements[&gid(&h, walls[1])].hidden);
    assert!(h.state().temporary().is_none(), "nothing left to hide for now");
    assert!(h.state().dirty(), "it is a change to the project now");
    // A category hidden for now goes in with it.
    click(&mut h, P2(0.0, 3000.0));
    shortcut(&mut h, "HC");
    press(&mut h, "Temporary Hide/Isolate");
    press(&mut h, "Apply Hide/Isolate to View");
    assert!(plan_graphics(&h, 100000).hides("wall") && h.state().temporary().is_none());
}

#[test]
fn unhide_element_and_unhide_category_bring_back_what_the_selection_hides_while_revealing() {
    let mut h = app(BOX);
    let walls = ids(&h, "wall");
    // A wall hidden for good can't be picked, and offers nothing to unhide.
    click(&mut h, P2(4000.0, 0.0));
    shortcut(&mut h, "EH");
    assert!(h.query_by_label("Unhide Element").is_none());
    // Revealing draws it in magenta, and it can be picked: its panel then offers Unhide Element, not Unhide Category (its
    // category isn't hidden).
    shortcut(&mut h, "RH");
    click(&mut h, P2(4000.0, 0.0));
    assert_eq!(h.state().selection, [walls[0]]);
    assert!(h.query_by_label("Unhide Element").is_some() && h.query_by_label("Unhide Category").is_none());
    press(&mut h, "Unhide Element");
    assert!(plan_graphics(&h, 100000).elements.is_empty(), "no entry for what's visible and plain");
    assert_eq!(h.state().selection, [walls[0]], "it stays selected: it is still there");
    assert!(h.query_by_label("Unhide Element").is_none(), "nothing the selection hides is left");
    // EU on a selection that hides something unhides it, with no dialog; hidden again, and picked, then EU does it too.
    click(&mut h, P2(8000.0, 3000.0));
    shortcut(&mut h, "EH");
    click(&mut h, P2(8000.0, 3000.0));
    shortcut(&mut h, "EU");
    assert!(plan_graphics(&h, 100000).elements.is_empty());
    assert!(h.query_by_label("Visibility/Graphics Overrides for Level 1").is_none(), "the dialog stays shut");
    // A category hidden whole: Unhide Category (VU) brings back all its elements.
    shortcut(&mut h, "VH");
    assert!(plan_graphics(&h, 100000).hides("wall"));
    click(&mut h, P2(0.0, 3000.0));
    assert!(h.query_by_label("Unhide Category").is_some());
    press(&mut h, "Unhide Category");
    assert!(!plan_graphics(&h, 100000).hides("wall"));
    shortcut(&mut h, "VH");
    click(&mut h, P2(0.0, 3000.0));
    shortcut(&mut h, "VU");
    assert!(!plan_graphics(&h, 100000).hides("wall"));
    // With nothing the selection hides, VU says so and changes nothing.
    shortcut(&mut h, "VU");
    assert!(h.state().status.text.contains("Reveal Hidden Elements"), "{:?}", h.state().status);
    key(&mut h, Key::Escape);
}

#[test]
fn hide_element_hide_category_and_the_elements_tab_work_on_the_selection() {
    let mut h = app(BOX);
    let walls = ids(&h, "wall");
    click(&mut h, P2(4000.0, 0.0));
    shortcut(&mut h, "EH");
    assert!(h.state().selection.is_empty(), "what is hidden can't stay selected");
    let g = plan_graphics(&h, 100000);
    assert_eq!(g.elements.keys().copied().collect::<Vec<_>>(), [gid(&h, walls[0])]);
    assert!(g.elements[&gid(&h, walls[0])].hidden);
    click(&mut h, P2(8000.0, 3000.0));
    assert_eq!(h.state().selection, [walls[1]]);
    shortcut(&mut h, "VH");
    assert!(plan_graphics(&h, 100000).hides("wall"));
    // Unhide Elements: the Elements tab lists the one hidden; a box brings it back.
    shortcut(&mut h, "EU");
    assert!(h.query_by_role_and_label(Role::CheckBox, "Wall 100002").is_some());
    pick(&mut h, Role::CheckBox, "Wall 100002");
    press(&mut h, "OK");
    assert!(plan_graphics(&h, 100000).elements.is_empty(), "no entry for what's visible and plain");
    // The Modify tab's panel does the same, and Override Graphics adds the selection to the list.
    shortcut(&mut h, "VV");
    pick(&mut h, Role::CheckBox, "Walls");
    press(&mut h, "OK");
    click(&mut h, P2(0.0, 3000.0));
    press(&mut h, "Hide Element");
    assert!(plan_graphics(&h, 100000).elements[&gid(&h, walls[3])].hidden);
    // Override Graphics by Element (EO): the selection goes on the Elements tab, picked, to be drawn its own way.
    click(&mut h, P2(8000.0, 3000.0));
    shortcut(&mut h, "EO");
    assert!(h.query_by_role_and_label(Role::CheckBox, "Wall 100003").is_some(), "listed, though it has no override yet");
    pick(&mut h, Role::Button, "Wall 100003: Cut Patterns");
    pick(&mut h, Role::CheckBox, "Cut: Fill color");
    press(&mut h, "OK");
    press(&mut h, "OK");
    assert_eq!(plan_graphics(&h, 100000).elements[&gid(&h, walls[1])].cut.fill, Some([200, 200, 200]));
    key(&mut h, Key::Escape);
}

#[test]
fn filters_are_made_in_their_dialog_and_applied_by_a_view() {
    let mut h = app(BOX);
    shortcut(&mut h, "FT");
    assert!(h.query_by_label("No filters yet: New makes one.").is_some());
    press(&mut h, "New");
    assert_eq!(h.state().session.model.view_filters["Filter 1"].categories, ["wall"]);
    press(&mut h, "Add Filter");
    pick(&mut h, Role::ComboBox, "Filter by:");
    pick(&mut h, Role::Button, "Type");
    pick(&mut h, Role::ComboBox, "Comparison");
    press(&mut h, "equals");
    type_role(&mut h, Role::TextInput, "Filter by:", "Generic - 200mm");
    let f = h.state().session.model.view_filters["Filter 1"].clone();
    assert_eq!((f.rules[0].field.as_str(), f.rules[0].value.clone()), ("Type", json!("Generic - 200mm")));
    // Another category beside walls, and walls again.
    pick(&mut h, Role::CheckBox, "Columns");
    assert_eq!(h.state().session.model.view_filters["Filter 1"].categories, ["column", "wall"]);
    pick(&mut h, Role::CheckBox, "Columns");
    press(&mut h, "Close");
    // The view applies it: hidden walls, and a colour for the cut.
    shortcut(&mut h, "VV");
    press(&mut h, "Filters");
    pick(&mut h, Role::ComboBox, "Add filter:");
    press(&mut h, "Filter 1");
    pick(&mut h, Role::Button, "Filter 1: Cut Lines");
    pick(&mut h, Role::CheckBox, "Cut: Line color");
    press(&mut h, "OK");
    press(&mut h, "OK");
    let g = plan_graphics(&h, 100000);
    assert_eq!((g.filters.len(), g.filters[0].filter.as_str()), (1, "Filter 1"));
    assert_eq!(g.filters[0].over.cut.color, Some([40, 40, 40]));
    // Visible no more: unticked in the list.
    shortcut(&mut h, "VV");
    press(&mut h, "Filters");
    pick(&mut h, Role::CheckBox, "Filter 1");
    press(&mut h, "OK");
    assert!(plan_graphics(&h, 100000).filters[0].over.hidden);
    // Deleting the filter takes it from the view.
    shortcut(&mut h, "FT");
    press(&mut h, "Delete");
    assert!(plan_graphics(&h, 100000).filters.is_empty());
    press(&mut h, "Close");
}

/// Whether the widget of `role` named `label` is greyed out.
fn is_disabled(h: &Harness<BimApp>, role: Role, label: &str) -> bool {
    use egui_kittest::kittest::NodeT;
    h.get_by_role_and_label(role, label).accesskit_node().is_disabled()
}

#[test]
fn a_templates_controls_are_ticked_and_unticked_one_at_a_time() {
    use bimcraft_core::graphics::Group;
    let mut h = app(BOX);
    h.state_mut().session.execute("template", &json!({"name": "T", "from": 100000, "controls": ["categories"]})).unwrap();
    shortcut(&mut h, "VT");
    let controls = |h: &Harness<BimApp>| h.state().session.model.view_templates["T"].controls.iter().copied().collect::<Vec<_>>();
    assert_eq!(controls(&h), [Group::Categories]);
    // Each is ticked on its own, the others staying as they were; Apply (the dialog's, over the palette's) sends them.
    let apply = |h: &mut Harness<BimApp>| {
        h.get_all_by(|n| n.role() == Role::Button && n.label().as_deref() == Some("Apply")).last().unwrap().click();
        h.run();
    };
    pick(&mut h, Role::CheckBox, "Filters");
    apply(&mut h);
    assert_eq!(controls(&h), [Group::Categories, Group::Filters]);
    pick(&mut h, Role::CheckBox, "Categories");
    apply(&mut h);
    assert_eq!(controls(&h), [Group::Filters], "the others stay as they were");
    pick(&mut h, Role::CheckBox, "Filters");
    pick(&mut h, Role::CheckBox, "View Range");
    press(&mut h, "OK");
    assert_eq!(controls(&h), [Group::Range]);
}

#[test]
fn the_3d_view_leaves_out_what_its_graphics_hide() {
    let mut h = app(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}"));
    press(&mut h, "Default 3D View");
    let at = h.state().screen3([5000.0, -100.0, 1500.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection.len(), 1, "the south wall's face");
    key(&mut h, Key::Escape);
    shortcut(&mut h, "VV");
    assert!(h.query_by_label("Visibility/Graphics Overrides for 3D View: {3D}").is_some());
    pick(&mut h, Role::CheckBox, "Walls");
    press(&mut h, "OK");
    let id = h.state().session.model.default_3d().unwrap();
    assert!(bimcraft_core::graphics::graphics_of(&h.state().session.model, id).unwrap().hides("wall"));
    click_at(&mut h, at);
    assert!(h.state().selection.is_empty(), "no wall to pick");
}

#[test]
fn select_all_instances_in_3d_takes_what_the_view_shows_and_no_more() {
    let mut h = app(BOX);
    let walls = ids(&h, "wall");
    h.state_mut().session.execute("graphics", &json!({"view": "{3D}", "hide_elements": [walls[1], walls[2], walls[3]]})).unwrap();
    press(&mut h, "Default 3D View");
    let at = h.state().screen3([5000.0, -100.0, 1500.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [walls[0]]);
    shortcut(&mut h, "SA");
    assert_eq!(h.state().selection, [walls[0]], "the walls it can't see aren't selected with it");
    key(&mut h, Key::Escape);
}

#[test]
fn sheets_and_schedules_have_no_graphics_to_override() {
    let mut h = app(&format!("{BOX}\nsheet {{\"name\": \"Plans\"}}"));
    browse(&mut h, "A101 - Plans");
    shortcut(&mut h, "VV");
    assert!(h.query_by_label("Visibility/Graphics Overrides for Sheet: Plans").is_none());
    assert!(h.query_by_label("This view has no graphics to override.").is_some());
}

/// The value of parameter `key` that element `id` holds, if it holds one.
fn param(h: &Harness<BimApp>, id: Id, key: &str) -> Option<Value> {
    h.state().session.model.elements.get(id).and_then(Element::params).and_then(|p| p.get(key)).cloned()
}

/// A wall drawn with the Wall tool from `a` to `b`, the tool left again: its id.
fn draw_wall(h: &mut Harness<BimApp>, a: P2, b: P2) -> Id {
    tool(h, "Wall");
    click(h, a);
    click(h, b);
    key(h, Key::Enter);
    key(h, Key::Escape);
    ids(h, "wall").last().copied().unwrap()
}

/// A drop-down opened the way a screen reader does, for the ones lying below the palette's fold.
fn open_combo(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_role_and_label(Role::ComboBox, label).click_accesskit();
    h.run();
}

const MASSING: &str = r#"option {"set": "Massing", "options": ["Tall", "Low"]}"#;

#[test]
fn ph_and_do_open_phasing_and_design_options_and_so_do_the_manage_tabs_buttons() {
    let mut h = app(BOX);
    shortcut(&mut h, "PH");
    assert!(h.query_by_label("Phases, in order:").is_some());
    press(&mut h, "Close");
    shortcut(&mut h, "DO");
    assert!(h.query_by_label("No design option sets yet: New Set makes one.").is_some());
    press(&mut h, "Close");
    assert!(h.query_by_label("Active design option:").is_none(), "no sets of options, no active one to pick");
    // The Manage tab's buttons, in their panels' lists, as the window folds the tab's last panels to fit; the Properties palette
    // has a Phasing group of its own, below.
    press(&mut h, "Manage");
    from_list(&mut h, "Phasing", "Phasing");
    assert!(h.query_by_label("Phases, in order:").is_some());
    press(&mut h, "Close");
    from_list(&mut h, "Design Options", "Design Options");
    assert!(h.query_by_label("Option set:").is_some());
    press(&mut h, "Close");
    assert!(h.query_by_label("Option set:").is_none());
}

#[test]
fn a_ribbon_tab_wider_than_the_window_folds_its_last_panels_to_buttons_that_list_their_tools() {
    let mut h = app(BOX);
    h.set_size(vec2(1000.0, 800.0));
    h.run();
    let window = h.ctx.content_rect().right();
    press(&mut h, "Manage");
    assert!(h.query_by_label("Design Options").is_none(), "the last panel is folded at {window}");
    assert!(h.get_by_label("Design Options ▾").rect().right() <= window, "to one button, in the window");
    assert!(h.get_by_label("Materials").rect().left() >= 0.0, "and the first panel is as it was");
    // The folded panel lists its tools, and a tool there works as on the ribbon.
    press(&mut h, "Design Options ▾");
    press(&mut h, "Design Options");
    assert!(h.query_by_label("Option set:").is_some());
}

/// A tool that drops a list of choices down opens it from the list of a panel that has handed the tool over to its name (or folded
/// whole): a click on the row must leave the choices on screen, not close the list and with it the tool's own popup.
#[test]
fn a_tool_with_choices_of_its_own_opens_them_from_a_panels_list() {
    let mut shut = Vec::new();
    for (tab, panel, tool, choice) in [
        (Tab::Manage, "Settings", "Project Units", "Feet and inches"),
        (Tab::Manage, "Settings", "Project Language", "Español"),
        (Tab::Manage, "Family Editor", "New Family", "Furniture"),
        (Tab::View, "Create", "Schedules", "Schedule/Quantities…"),
        (Tab::View, "Create", "Duplicate View", "Duplicate with Detailing"),
        (Tab::View, "Windows", "User Interface", "Language"),
        (Tab::View, "Graphics", "Dark Mode", "Dark UI, Light Canvas"),
        (Tab::View, "Sheet Composition", "Place View", "Floor Plan: Level 1"),
    ] {
        let mut h = app(BOX);
        h.state_mut().tab = tab;
        h.run();
        if tool == "Place View" {
            // It places a view on a sheet that is open.
            press(&mut h, "Sheet");
        }
        from_list(&mut h, panel, tool);
        if h.query_by_label(choice).is_none() {
            shut.push(format!("{tool} (in {panel}'s list): `{choice}` is not on screen"));
        }
    }
    assert!(shut.is_empty(), "a click on the row must leave the tool's choices on screen: {shut:#?}");
    // And a choice picked there is made.
    let mut h = app(BOX);
    h.state_mut().tab = Tab::Manage;
    h.run();
    from_list(&mut h, "Settings", "Project Units");
    press(&mut h, "Feet and inches");
    assert_eq!(h.state().session.model.units, bimcraft_core::units::Units::FeetInches);
    assert!(!egui::Popup::is_any_open(&h.ctx), "the list and its choices close with it");
}

/// UN (Project Units) at the least a window can be: the tool is in its panel's list by then, which the shortcut opens, and the
/// first frame of the tab, which draws every tool, doesn't take the request for a button that is gone the next.
#[test]
fn the_units_shortcut_gets_to_the_choices_in_the_smallest_window_too() {
    let mut h = app(BOX);
    h.set_size(vec2(crate::window::MIN_SIZE[0], crate::window::MIN_SIZE[1]));
    h.run();
    shortcut(&mut h, "UN");
    assert!(egui::Popup::is_any_open(&h.ctx), "the panel's list is open");
    let row = h.get_by_label("Project Units").rect().center();
    click_at(&mut h, row);
    press(&mut h, "Feet and inches");
    assert_eq!(h.state().session.model.units, bimcraft_core::units::Units::FeetInches);
}

#[test]
fn a_panels_name_lists_its_tools() {
    let mut h = app(BOX);
    press(&mut h, "Build ▾");
    let lowest =
        |h: &Harness<BimApp>, name: &str| h.get_all_by_label(name).map(|n| n.rect().center()).max_by(|a, b| a.y.total_cmp(&b.y)).unwrap();
    // The list under the name, with the ribbon's own button above it.
    assert_eq!(h.get_all_by_label("Wall").count(), 2);
    let row = lowest(&h, "Wall");
    click_at(&mut h, row);
    assert!(matches!(h.state().tool, Tool::Wall(_)));
    // Its contextual tab has the ribbon now, and the list has closed.
    assert_eq!(h.query_all_by_label("Wall").count(), 0);
}

#[test]
fn a_ribbon_tab_too_wide_even_folded_scrolls_sideways_and_each_tab_keeps_its_place() {
    let mut h = app(BOX);
    h.set_size(vec2(520.0, 800.0));
    h.run();
    let window = h.ctx.content_rect().right();
    assert!(h.get_by_label("Datum ▾").rect().right() > window, "folded, Architecture is still wider than {window}");
    scroll_ribbon_right(&mut h);
    assert!(h.get_by_label("Datum ▾").rect().right() <= window, "at its end the last panel is in the window");
    // The tabs past the window's edge are out of reach at this size.
    h.state_mut().tab = crate::Tab::Manage;
    h.run();
    h.state_mut().tab = crate::Tab::Architecture;
    h.run();
    assert!(h.get_by_label("Datum ▾").rect().right() <= window, "and Architecture is where it was left");
}

#[test]
fn the_phasing_dialog_adds_renames_and_combines_phases() {
    use egui_kittest::kittest::NodeT;
    let mut h = app(BOX);
    let walls = ids(&h, "wall");
    let phases = |h: &Harness<BimApp>| h.state().session.model.phases.clone();
    shortcut(&mut h, "PH");
    // After the one picked.
    pick(&mut h, Role::Button, "Existing");
    type_into(&mut h, "Name:", "Demolition");
    press(&mut h, "Insert After");
    assert_eq!(phases(&h), ["Existing", "Demolition", "New Construction"]);
    // The new one is picked: renamed, it keeps the elements made in it.
    h.state_mut().session.execute("modify", &json!({"ids": [walls[0]], "params": {"Phase Created": "Demolition"}})).unwrap();
    type_into(&mut h, "Name:", "Interim");
    press(&mut h, "Rename");
    assert_eq!(phases(&h), ["Existing", "Interim", "New Construction"]);
    assert_eq!(param(&h, walls[0], "Phase Created"), Some(json!("Interim")));
    // Before it, and at the end.
    type_into(&mut h, "Name:", "Survey");
    press(&mut h, "Insert Before");
    type_into(&mut h, "Name:", "Handover");
    press(&mut h, "Add at End");
    assert_eq!(phases(&h), ["Existing", "Survey", "Interim", "New Construction", "Handover"]);
    // Combined into its neighbour, a phase leaves what was made in it there.
    pick(&mut h, Role::Button, "Interim");
    press(&mut h, "Combine with Previous");
    assert_eq!(phases(&h), ["Existing", "Survey", "New Construction", "Handover"]);
    assert_eq!(param(&h, walls[0], "Phase Created"), Some(json!("Survey")));
    // The first has no one before it; the last none after.
    pick(&mut h, Role::Button, "Existing");
    assert!(h.get_by_label("Combine with Previous").accesskit_node().is_disabled());
    pick(&mut h, Role::Button, "Handover");
    assert!(h.get_by_label("Combine with Next").accesskit_node().is_disabled());
    // A name the project has can't be added again.
    type_into(&mut h, "Name:", "Existing");
    assert!(h.get_by_label("Add at End").accesskit_node().is_disabled());
    press(&mut h, "Close");
}

#[test]
fn phase_filters_are_made_and_set_status_by_status_in_the_phasing_dialog() {
    use bimcraft_core::phasing::{Presentation, Status};
    let mut h = app(BOX);
    let filter = |h: &Harness<BimApp>, name: &str| h.state().session.model.phase_filters.get(name).cloned();
    shortcut(&mut h, "PH");
    press(&mut h, "Phase Filters");
    type_into(&mut h, "New filter:", "Survey Only");
    press(&mut h, "New Filter");
    let made = filter(&h, "Survey Only").unwrap();
    assert!(Status::ALL.iter().all(|s| made.of(*s) == Presentation::ByCategory), "a new one shows everything as its category does");
    // The new one is picked; each status has a drop-down.
    pick(&mut h, Role::ComboBox, "Existing");
    press(&mut h, "Overridden");
    pick(&mut h, Role::ComboBox, "Demolished");
    press(&mut h, "Not Displayed");
    let f = filter(&h, "Survey Only").unwrap();
    assert_eq!((f.existing, f.demolished, f.new), (Presentation::Overridden, Presentation::NotDisplayed, Presentation::ByCategory));
    // The ones a project starts with are listed, and a filter a view uses can't go.
    pick(&mut h, Role::ComboBox, "Phase filter:");
    press(&mut h, "Show New");
    h.state_mut().session.execute("graphics", &json!({"view": 100000, "phase_filter": "Show New"})).unwrap();
    press(&mut h, "Delete");
    assert!(filter(&h, "Show New").is_some() && h.state().status.error, "{:?}", h.state().status.text);
    pick(&mut h, Role::ComboBox, "Phase filter:");
    press(&mut h, "Survey Only");
    press(&mut h, "Delete");
    assert!(filter(&h, "Survey Only").is_none());
    press(&mut h, "Close");
}

#[test]
fn a_statuss_graphic_overrides_are_applied_as_one_command() {
    use bimcraft_core::phasing::Status;
    use egui_kittest::kittest::NodeT;
    let mut h = app(BOX);
    let over = |h: &Harness<BimApp>, s: Status| h.state().session.model.phase_overrides.get(&s).cloned().unwrap_or_default();
    let steps = h.state().session.history().len();
    shortcut(&mut h, "PH");
    press(&mut h, "Graphic Overrides");
    pick(&mut h, Role::Button, "Demolished");
    // Dashed red to begin with. The edit is a draft until Apply.
    pick(&mut h, Role::CheckBox, "Cut: Line weight");
    assert_eq!(over(&h, Status::Demolished).cut.weight, None);
    press(&mut h, "Apply Overrides");
    assert_eq!(over(&h, Status::Demolished).cut.weight, Some(Pen(10)));
    assert_eq!(over(&h, Status::Demolished).cut.dash, Some(bimcraft_core::graphics::Dash::Dashed), "the rest stays as it was");
    assert_eq!(h.state().session.history().len(), steps + 1, "one command");
    // Unticked again, the patch takes that one away and leaves the rest.
    pick(&mut h, Role::CheckBox, "Cut: Line weight");
    press(&mut h, "Apply Overrides");
    assert_eq!(over(&h, Status::Demolished).cut.weight, None);
    assert_eq!(over(&h, Status::Demolished).cut.dash, Some(bimcraft_core::graphics::Dash::Dashed));
    // A phase filter hides what it doesn't display; overrides draw, so no transparency to set.
    assert!(h.query_by_label("Transparency").is_none());
    // Another status, edited and dropped by moving on.
    pick(&mut h, Role::Button, "Temporary");
    pick(&mut h, Role::CheckBox, "Projection: Line color");
    pick(&mut h, Role::Button, "Demolished");
    pick(&mut h, Role::Button, "Temporary");
    assert!(over(&h, Status::Temporary).is_empty());
    assert!(h.get_by_label("Apply Overrides").accesskit_node().is_disabled(), "nothing to apply");
    press(&mut h, "Close");
}

#[test]
fn the_active_design_option_in_the_status_bar_is_what_new_walls_are_made_in() {
    let mut h = app(&format!("{BOX}\n{MASSING}"));
    let option = |h: &Harness<BimApp>, id: Id| param(h, id, "Design Option");
    // The main model first: a wall made there is in no option.
    let main = draw_wall(&mut h, P2(2000.0, 1500.0), P2(6000.0, 1500.0));
    assert!(!ids(&h, "wall")[..4].contains(&main), "a new wall");
    assert_eq!(option(&h, main), None);
    // The option picked is the one new walls are made in, and the view on screen shows it.
    pick(&mut h, Role::ComboBox, "Active design option:");
    press(&mut h, "Massing: Low");
    assert_eq!(h.state().active_option.as_deref(), Some("Massing: Low"));
    assert_eq!(plan_graphics(&h, 100000).options["Massing"], "Low");
    let low = draw_wall(&mut h, P2(2000.0, 3000.0), P2(6000.0, 3000.0));
    assert_eq!(option(&h, low), Some(json!("Massing: Low")));
    let look = bimcraft_core::graphics::Look::of(&h.state().session.model, 1);
    assert!(look.shows(low) && look.shows(main), "what was made in it is in the view");
    // Back to the main model: what is made now is in none.
    pick(&mut h, Role::ComboBox, "Active design option:");
    press(&mut h, "Main Model");
    let again = draw_wall(&mut h, P2(2000.0, 4500.0), P2(6000.0, 4500.0));
    assert_eq!(option(&h, again), None);
    let steps = ["Wall", "Design Options", "Wall", "Visibility/Graphics", "Wall", "Wall"];
    assert_eq!(h.state().session.history(), steps, "picking an option is no undo step, but showing it in the view is");
}

#[test]
fn what_is_drawn_in_a_view_with_a_phase_of_its_own_is_made_in_that_phase() {
    let mut h = app(BOX);
    let made = |h: &Harness<BimApp>, id: Id| param(h, id, "Phase Created");
    // A view with neither a phase nor a phase filter makes nothing phased: the first phase, no value.
    let plain = draw_wall(&mut h, P2(2000.0, 1500.0), P2(6000.0, 1500.0));
    assert_eq!(made(&h, plain), None);
    // A phase filter makes it phased; it shows the project's last phase, and walls are made in it.
    open_combo(&mut h, "Phase Filter");
    press(&mut h, "Show New");
    assert_eq!(plan_graphics(&h, 100000).phase_filter.as_deref(), Some("Show New"));
    let new = draw_wall(&mut h, P2(2000.0, 3000.0), P2(6000.0, 3000.0));
    assert_eq!(made(&h, new), Some(json!("New Construction")));
    // The first phase picked for the view: what is drawn there is made in it, which leaves no value.
    open_combo(&mut h, "Phase");
    press(&mut h, "Existing");
    assert_eq!(plan_graphics(&h, 100000).phase.as_deref(), Some("Existing"));
    let old = draw_wall(&mut h, P2(2000.0, 4500.0), P2(6000.0, 4500.0));
    assert_eq!(made(&h, old), None);
    // The last phase picked again is the view's own now, whatever the project gains after it.
    open_combo(&mut h, "Phase");
    press(&mut h, "New Construction");
    assert_eq!(plan_graphics(&h, 100000).phase.as_deref(), Some("New Construction"));
    // The filter takes the view back to no phasing; the phase stays its own.
    open_combo(&mut h, "Phase Filter");
    press(&mut h, "<None>");
    assert!(plan_graphics(&h, 100000).phase_filter.is_none());
}

#[test]
fn an_elements_phases_and_design_option_are_set_in_properties() {
    use egui_kittest::kittest::NodeT;
    let mut h = app(&format!("{BOX}\n{MASSING}"));
    let wall = ids(&h, "wall")[0];
    let shown = |h: &Harness<BimApp>, label: &str| h.get_by_role_and_label(Role::ComboBox, label).accesskit_node().value();
    click(&mut h, P2(4000.0, 0.0));
    assert_eq!(h.state().selection, [wall]);
    assert_eq!(shown(&h, "Phase Created").as_deref(), Some("Existing"), "no value is the first phase");
    open_combo(&mut h, "Phase Created");
    press(&mut h, "New Construction");
    assert_eq!(param(&h, wall, "Phase Created"), Some(json!("New Construction")));
    // Taken down in the phase it was made in: a temporary thing. None takes it back.
    open_combo(&mut h, "Phase Demolished");
    press(&mut h, "New Construction");
    assert_eq!(param(&h, wall, "Phase Demolished"), Some(json!("New Construction")));
    open_combo(&mut h, "Phase Demolished");
    press(&mut h, "<None>");
    assert_eq!(param(&h, wall, "Phase Demolished"), None);
    // Before it was made is refused, and says why.
    open_combo(&mut h, "Phase Demolished");
    press(&mut h, "Existing");
    assert!(h.state().status.error && param(&h, wall, "Phase Demolished").is_none(), "{:?}", h.state().status);
    // In an option of a set, and back in the main model.
    open_combo(&mut h, "Design Option");
    press(&mut h, "Massing: Tall");
    assert_eq!(param(&h, wall, "Design Option"), Some(json!("Massing: Tall")));
    open_combo(&mut h, "Design Option");
    press(&mut h, "Main Model");
    assert_eq!(param(&h, wall, "Design Option"), None);
    // Two walls that differ leave the field blank rather than say one of them; one pick sets both.
    click_with(&mut h, P2(8000.0, 3000.0), Modifiers::COMMAND);
    assert_eq!(h.state().selection.len(), 2);
    assert_eq!(shown(&h, "Phase Created").as_deref(), Some(""));
    open_combo(&mut h, "Phase Created");
    press(&mut h, "Existing");
    let both: Vec<Option<Value>> = h.state().selection.iter().map(|id| param(&h, *id, "Phase Created")).collect();
    assert_eq!(both, [Some(json!("Existing")), Some(json!("Existing"))]);
}

#[test]
fn the_design_options_dialog_makes_sets_and_options_and_edits_one() {
    let mut h = app(BOX);
    let walls = ids(&h, "wall");
    let set = |h: &Harness<BimApp>| h.state().session.model.option_sets.get("Massing").cloned();
    shortcut(&mut h, "DO");
    type_into(&mut h, "Name:", "Massing");
    press(&mut h, "New Set");
    assert_eq!(set(&h).map(|s| (s.options, s.primary)), Some((vec!["Option 1".to_string()], "Option 1".to_string())));
    // A second option, made the primary.
    type_into(&mut h, "Name:", "Option 2");
    press(&mut h, "Add Option");
    pick(&mut h, Role::Button, "Option 2");
    press(&mut h, "Make Primary");
    assert_eq!(set(&h).map(|s| s.primary).as_deref(), Some("Option 2"));
    // Renamed; an element in it follows.
    h.state_mut().session.execute("modify", &json!({"ids": [walls[0]], "params": {"Design Option": "Massing: Option 1"}})).unwrap();
    pick(&mut h, Role::Button, "Option 1");
    type_into(&mut h, "Name:", "Tall");
    press(&mut h, "Rename Option");
    assert_eq!(set(&h).map(|s| s.options), Some(vec!["Tall".to_string(), "Option 2".to_string()]));
    assert_eq!(param(&h, walls[0], "Design Option"), Some(json!("Massing: Tall")));
    // An option with elements in it can't go; one without can.
    pick(&mut h, Role::Button, "Tall");
    press(&mut h, "Delete Option");
    assert!(h.state().status.error && set(&h).is_some_and(|s| s.options.len() == 2), "{:?}", h.state().status.text);
    pick(&mut h, Role::Button, "Option 2 (primary)");
    type_into(&mut h, "Name:", "Low");
    press(&mut h, "Rename Option");
    // Edit Option: new elements are made in it, and the view shows it.
    pick(&mut h, Role::Button, "Tall");
    press(&mut h, "Edit Option");
    assert_eq!(h.state().active_option.as_deref(), Some("Massing: Tall"));
    assert_eq!(plan_graphics(&h, 100000).options["Massing"], "Tall");
    assert!(h.query_by_label("Active design option:").is_some(), "now there are options to pick from");
    // Accepting the primary keeps its elements, drops the others', and the set with them.
    press(&mut h, "Accept Primary");
    assert!(set(&h).is_none());
    assert!(!h.state().session.model.elements.contains_key(walls[0]), "the wall in Tall went");
    assert_eq!(h.state().active_option, None, "the active option went with its set");
    assert!(plan_graphics(&h, 100000).options.is_empty());
    press(&mut h, "Close");
}

#[test]
fn visibility_graphics_picks_the_option_a_view_shows_of_each_set_unless_a_template_does() {
    use egui_kittest::kittest::NodeT;
    let mut h = app(&format!("{BOX}\n{MASSING}"));
    shortcut(&mut h, "VV");
    press(&mut h, "Design Options");
    pick(&mut h, Role::ComboBox, "Massing");
    press(&mut h, "Low");
    // A draft until OK, as every tab's.
    assert!(plan_graphics(&h, 100000).options.is_empty());
    press(&mut h, "OK");
    assert_eq!(plan_graphics(&h, 100000).options["Massing"], "Low");
    // The primary again takes the choice away.
    shortcut(&mut h, "VV");
    press(&mut h, "Design Options");
    pick(&mut h, Role::ComboBox, "Massing");
    press(&mut h, "Primary (Tall)");
    press(&mut h, "OK");
    assert!(plan_graphics(&h, 100000).options.is_empty());
    // A template that controls the options holds them: the box is there, and can't be used.
    h.state_mut().session.execute("template", &json!({"name": "T", "from": 100000, "controls": ["options"]})).unwrap();
    h.state_mut().session.execute("graphics", &json!({"view": 100000, "template": "T"})).unwrap();
    shortcut(&mut h, "VV");
    press(&mut h, "Design Options");
    assert!(h.get_by_role_and_label(Role::ComboBox, "Massing").accesskit_node().is_disabled());
    assert!(h.query_by_label("Design Options follow view template T: change them there.").is_some());
    press(&mut h, "Cancel");
    // A project with no sets says where to make them.
    let mut h = app(BOX);
    shortcut(&mut h, "VV");
    press(&mut h, "Design Options");
    assert!(h.query_by_label("The project has no design options: Manage > Design Options makes them.").is_some());
}

#[test]
fn an_active_option_the_project_no_longer_has_is_dropped() {
    let mut h = app(&format!("{BOX}\n{MASSING}"));
    pick(&mut h, Role::ComboBox, "Active design option:");
    press(&mut h, "Massing: Low");
    assert_eq!(h.state().active_option.as_deref(), Some("Massing: Low"));
    // Undo takes the choice the view made, then the set itself.
    for _ in 0..2 {
        h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
        h.run();
    }
    assert!(h.state().session.model.option_sets.is_empty());
    assert_eq!(h.state().active_option, None);
    assert!(h.query_by_label("Active design option:").is_none(), "nothing left to pick from");
    // Walls made now are in no option.
    let wall = draw_wall(&mut h, P2(2000.0, 1500.0), P2(6000.0, 1500.0));
    assert_eq!(param(&h, wall, "Design Option"), None);
}

#[test]
fn a_view_that_does_not_show_the_active_option_says_so_and_picking_it_again_shows_it_there() {
    let mut h = app(&format!("{BOX}\n{MASSING}"));
    assert!(!h.state().active_option_unseen(), "no option active: nothing to miss");
    pick(&mut h, Role::ComboBox, "Active design option:");
    press(&mut h, "Massing: Low");
    assert!(!h.state().active_option_unseen(), "Level 1 shows it now");
    // Level 2's plan still shows the primary: what is made there in Low would not be seen.
    topmost(&mut h, "Level 2");
    assert!(plan_graphics(&h, 100001).options.is_empty());
    assert!(h.state().active_option_unseen());
    // The same pick again shows it in the view on screen.
    pick(&mut h, Role::ComboBox, "Active design option:");
    press(&mut h, "Massing: Low");
    assert_eq!(plan_graphics(&h, 100001).options["Massing"], "Low");
    assert!(!h.state().active_option_unseen());
}

/// The value of `Phase Demolished` that element `id` holds of its own.
fn demolished(h: &Harness<BimApp>, id: Id) -> Option<Value> {
    param(h, id, "Phase Demolished")
}

#[test]
fn demolish_takes_what_is_clicked_down_in_the_views_phase_and_a_second_click_takes_it_back() {
    use bimcraft_core::phasing::{self, Status};
    let mut h = app(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}\nphase {{\"name\": \"Demolition\", \"before\": \"New Construction\"}}"));
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 0.0)));
    let east = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(8000.0, 0.0)));
    let door = find(&h, |e| matches!(e, Element::Door(_)));
    let steps = |h: &Harness<BimApp>| h.state().session.history().len();
    // The Modify tab has the tool, in a panel of its own.
    topmost(&mut h, "Modify");
    press(&mut h, "Demolish");
    assert_eq!(h.state().tool, Tool::Demolish);
    // A view with no phase of its own shows the project's last: the wall comes down in it, as one undo step.
    let before = steps(&h);
    click(&mut h, P2(5000.0, 0.0));
    assert_eq!(demolished(&h, wall), Some(json!("New Construction")));
    assert_eq!((steps(&h), h.state().session.history().last()), (before + 1, Some(&"Modify")));
    // The door in it is left alone: with no phase of its own, it follows its wall.
    assert_eq!(demolished(&h, door), None);
    let m = Arc::clone(&h.state().session.model);
    let last = phasing::index(&m, "New Construction").unwrap();
    assert_eq!(phasing::status(&m, &m.elements[&door], last), Some(Status::Demolished));
    // Clicking the door says why nothing happens: it goes back up with its wall.
    click(&mut h, P2(2000.0, 0.0));
    assert_eq!((demolished(&h, door), steps(&h)), (None, before + 1));
    let note = h.state().status.clone();
    assert!(!note.error && note.text.contains("Door follows its wall") && note.text.contains("New Construction"), "{note:?}");
    // A second click on the wall takes it back, and a step back puts it down again.
    click(&mut h, P2(5000.0, 0.0));
    assert_eq!((demolished(&h, wall), steps(&h)), (None, before + 2));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(demolished(&h, wall), Some(json!("New Construction")));
    // A view with a phase of its own takes things down in that one, and what comes down in another phase, in this one.
    open_combo(&mut h, "Phase");
    press(&mut h, "Demolition");
    click(&mut h, P2(8000.0, 3000.0));
    assert_eq!(demolished(&h, east), Some(json!("Demolition")));
    click(&mut h, P2(5000.0, 0.0));
    assert_eq!(demolished(&h, wall), Some(json!("Demolition")));
    click(&mut h, P2(8000.0, 3000.0));
    assert_eq!(demolished(&h, east), None);
    // The tool stays on until Esc.
    assert_eq!(h.state().tool, Tool::Demolish);
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Modify);
}

#[test]
fn demolish_works_in_a_section_and_in_the_3d_view_as_in_a_plan() {
    let mut h = app_at(&format!("{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}\nsheet {{}}"), 0.05);
    let west = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 6000.0)));
    let south = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 0.0)));
    let section = ids(&h, "section")[0];
    topmost(&mut h, "Modify");
    press(&mut h, "Demolish");
    // In the section, points run along its line from its start, and up: the west wall is cut at 1000.
    browse(&mut h, "Section 1");
    assert_eq!((h.state().sheet, &h.state().tool), (Sheet::Section(section), &Tool::Demolish));
    assert!(h.query_by_label("Click an element to take it down in the view's phase; click it again to take it back.").is_some());
    click(&mut h, P2(1060.0, 1500.0));
    assert_eq!(demolished(&h, west), Some(json!("New Construction")));
    // And in the default 3D view, on the south wall's face.
    press(&mut h, "Default 3D View");
    assert_eq!((h.state().sheet, &h.state().tool), (Sheet::View3D, &Tool::Demolish));
    let at = h.state().screen3([5000.0, -100.0, 1500.0]);
    click_at(&mut h, at);
    assert_eq!(demolished(&h, south), Some(json!("New Construction")));
    // Where it can't work, on a sheet, it gives way to Modify.
    browse(&mut h, "A101 - Unnamed");
    assert_eq!(h.state().tool, Tool::Modify);
}

#[test]
fn demolish_says_so_when_what_is_clicked_holds_no_parameters_and_changes_nothing() {
    let mut h = app(&format!("{BOX}\ndimension {{\"refs\": [[0, 3000], [8000, 3000]], \"at\": [4000, -1000]}}"));
    let dimension = ids(&h, "dimension")[0];
    topmost(&mut h, "Modify");
    press(&mut h, "Demolish");
    let (model, steps) = (Arc::clone(&h.state().session.model), h.state().session.history().len());
    click(&mut h, P2(4000.0, -1000.0));
    let status = h.state().status.clone();
    assert!(status.error && status.text == "Dimension holds no parameters, so it has no phase to demolish", "{status:?}");
    assert!(Arc::ptr_eq(&model, &h.state().session.model), "the model is the one it was");
    assert_eq!(h.state().session.history().len(), steps);
    assert_eq!(ids(&h, "dimension"), [dimension]);
    // Nothing under the cursor is nothing to say.
    click(&mut h, P2(4000.0, 3000.0));
    assert!(Arc::ptr_eq(&model, &h.state().session.model));
}

#[test]
fn demolish_speaks_portuguese_and_spanish() {
    use crate::lang::Lang;
    let mut h =
        app(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}\ndimension {{\"refs\": [[0, 3000], [8000, 3000]], \"at\": [4000, -1000]}}"));
    // The wall comes down, so that the door only follows it.
    topmost(&mut h, "Modify");
    press(&mut h, "Demolish");
    click(&mut h, P2(5000.0, 0.0));
    let cases = [
        (
            Lang::Portuguese,
            "Demolir",
            "Clique num elemento para demoli-lo na fase da vista; clique de novo para desfazer.",
            "Porta acompanha a parede, que é demolida em New Construction: desfaça a demolição da parede",
            "Cota não tem parâmetros, então não tem fase a demolir",
        ),
        (
            Lang::Spanish,
            "Demoler",
            "Haga clic en un elemento para demolerlo en la fase de la vista; haga clic otra vez para deshacerlo.",
            "Puerta acompaña al muro, que se demuele en New Construction: deshaga la demolición del muro",
            "Cota no tiene parámetros, así que no tiene fase que demoler",
        ),
    ];
    for (language, button, prompt, follows, none) in cases {
        h.state_mut().lang = language;
        h.run();
        press(&mut h, button);
        assert!(h.query_by_label(prompt).is_some(), "{language:?}: the prompt");
        click(&mut h, P2(2000.0, 0.0));
        assert_eq!(h.state().status.text, follows, "{language:?}: a door that follows its wall");
        click(&mut h, P2(4000.0, -1000.0));
        assert_eq!(h.state().status.text, none, "{language:?}: a dimension");
    }
}

#[test]
fn paint_lays_a_material_on_the_face_clicked_in_3d_and_remove_paint_takes_it_off() {
    let mut h = app(BOX);
    press(&mut h, "Default 3D View");
    topmost(&mut h, "Modify");
    h.state_mut().paint_material = "Timber".into();
    press(&mut h, "Paint");
    assert_eq!(h.state().tool, Tool::Paint);
    assert!(h.query_by_label("Modify | Paint").is_some(), "its contextual tab");
    // The south wall's outer face: a wall's exterior is to the left of its start to its end, so for these walls, drawn the
    // way round they are, it's their interior.
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 0.0)));
    let at = h.state().screen3([5000.0, -100.0, 1500.0]);
    click_at(&mut h, at);
    assert_eq!(field(&h, wall, "params"), json!({"Paint": {"interior": "Timber"}}));
    // Another material for the next click, over the same face; one undo step each.
    h.state_mut().paint_material = "Plaster".into();
    click_at(&mut h, at);
    assert_eq!(field(&h, wall, "params"), json!({"Paint": {"interior": "Plaster"}}));
    press(&mut h, "Remove Paint");
    assert_eq!(h.state().tool, Tool::Unpaint);
    click_at(&mut h, at);
    assert!(field(&h, wall, "params").is_null());
    // The sky has no face to paint, and says so.
    let corner = h.state().orbit.rect.min + vec2(4.0, 4.0);
    click_at(&mut h, corner);
    assert!(h.state().session.history().iter().filter(|l| **l == "Paint").count() == 3);
}

#[test]
fn pt_picks_the_paint_tool_in_3d_from_any_view_and_ks_lists_it() {
    let mut h = app(BOX);
    assert!(matches!(h.state().sheet, Sheet::Plan(_)));
    shortcut(&mut h, "PT");
    assert_eq!(h.state().tool, Tool::Paint);
    assert_eq!(h.state().sheet, Sheet::View3D, "Paint works in the 3D view: it opens it");
    key(&mut h, Key::Escape);
    shortcut(&mut h, "KS");
    assert!(h.query_by_label("PT").is_some() && h.query_by_label("Paint").is_some());
}

#[test]
fn the_materials_window_gives_a_material_a_texture_and_keeps_it_in_the_favorites_for_other_projects() {
    let file = std::env::temp_dir().join(format!("bimcraft-ui-favorites-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&file);
    let mut h = app(BOX);
    h.state_mut().favorites_path = Some(file.clone());
    press(&mut h, "Manage");
    press(&mut h, "Materials");
    // Brick comes first, with its pattern; give it a tile's, of its own size and turned.
    let texture = |h: &Harness<BimApp>| h.state().session.model.materials["Brick"].texture.clone();
    assert_eq!(texture(&h).map(|t| t.kind.name()), Some("brick"));
    pick(&mut h, Role::ComboBox, "Texture");
    press(&mut h, "Tile");
    assert_eq!(texture(&h).map(|t| t.kind.name()), Some("tile"));
    type_into(&mut h, "Tile Width", "450");
    key(&mut h, Key::Enter);
    type_into(&mut h, "Texture Angle", "45");
    key(&mut h, Key::Enter);
    let t = texture(&h).unwrap();
    assert_eq!((t.size[0], t.angle), (450.0, 45.0));
    // Typed nonsense is said, and the texture is as it was.
    type_into(&mut h, "Tile Width", "wide");
    key(&mut h, Key::Enter);
    assert!(h.state().status.error && texture(&h) == Some(t));
    // Kept in the Favorites, once, whatever it is now.
    press(&mut h, "Save to Favorites");
    let kept = bimcraft_core::favorites::Favorites::read(&file).unwrap();
    assert_eq!(kept.materials["Brick"].material.texture.as_ref().map(|t| t.angle), Some(45.0));
    // And, from the Favorites window, into the project again, under another name, since it has that one.
    press(&mut h, "Close");
    press(&mut h, "Favorites");
    press(&mut h, "Add to Project");
    assert_eq!(h.state().session.model.materials["Brick 2"].texture.as_ref().map(|t| t.angle), Some(45.0));
    press(&mut h, "Remove");
    assert!(bimcraft_core::favorites::Favorites::read(&file).unwrap().materials.is_empty());
    let _ = std::fs::remove_file(file);
}

#[test]
fn the_family_editor_edits_a_family_says_what_wont_build_and_starts_a_new_one_of_a_category() {
    let mut h = app(&format!("{BOX}\ncomponent {{\"family\": \"Table\", \"type\": \"1600 x 900\", \"at\": [3000, 3000]}}"));
    let table = find(&h, |e| matches!(e, Element::Component(_)));
    // Its top is drawn as an outline: click on its east edge.
    click(&mut h, P2(3800.0, 3000.0));
    assert_eq!(h.state().selection, [table]);
    h.set_size(vec2(1800.0, 900.0));
    press(&mut h, "Manage");
    press(&mut h, "Edit Family");
    assert!(h.state().family_editor.is_some(), "the window is open on the table's family");
    // The table's parameters, a row each: its fourth is its top's thickness, 40 mm.
    type_into(&mut h, "Parameter Value 4", "55");
    press_near(&mut h, "Apply", "Cancel");
    let thickness = |h: &Harness<BimApp>| h.state().session.model.families["Table"].parameters[3].value.clone();
    assert_eq!(thickness(&h), json!(55.0));
    // A formula naming what isn't there is said, in the window; the project's family is as it was.
    type_into(&mut h, "Parameter Formula 4", "Nope + 1");
    press_near(&mut h, "Apply", "Cancel");
    assert!(h.state().status.text.is_empty(), "the window says it itself: a box over the view's corner would lie on its buttons");
    assert_eq!(h.query_all_by_label_contains("Nope").count(), 1, "the window says what's wrong, once");
    assert_eq!(h.state().session.model.families["Table"].parameters[3].formula, None);
    press(&mut h, "Cancel");
    assert!(h.state().family_editor.is_none(), "Cancel closes it");
    // A new family of a category: its name, then OK; a door's has the opening's Width and Height.
    key(&mut h, Key::Escape);
    press(&mut h, "New Family");
    press(&mut h, "Door");
    type_into(&mut h, "Family name", "Cupboard Door");
    press(&mut h, "OK");
    let f = &h.state().session.model.families["Cupboard Door"];
    assert_eq!(f.category, bimcraft_core::family::Category::Door);
    assert!(f.parameters.iter().any(|p| p.name == "Width") && !f.forms.is_empty());
}

#[test]
fn the_door_tool_places_the_projects_single_door_of_a_family_and_the_type_selector_offers_both_kinds() {
    let mut h = app(BOX);
    tool(&mut h, "Door");
    click(&mut h, P2(2000.0, 0.0));
    let door = ids(&h, "door")[0];
    assert_eq!((field(&h, door, "family"), field(&h, door, "type")), (json!("Single Door"), json!("900 x 2100")));
    // Selected, its Type Selector lists the project's own types, then the families' as `Family: Type`.
    key(&mut h, Key::Escape);
    click(&mut h, P2(2000.0, 0.0));
    press(&mut h, "Type Selector");
    assert!(h.query_by_label("Double Door: 1500 x 2100").is_some() && h.query_by_label("Single 800 x 2100").is_some());
    // One of the project's own takes it off the family; another family's gives it that.
    press(&mut h, "Single 800 x 2100");
    assert_eq!((field(&h, door, "family"), field(&h, door, "type")), (Value::Null, json!("Single 800 x 2100")));
    press(&mut h, "Type Selector");
    press(&mut h, "Double Door: 1500 x 2100");
    assert_eq!((field(&h, door, "family"), field(&h, door, "type")), (json!("Double Door"), json!("1500 x 2100")));
    // Its Edit Type is its family type's, with a family's own parameters.
    press(&mut h, "Edit Type");
    assert!(h.query_by_label("Family:").is_some());
}

// ---------- updates ----------

/// A tool or button from the Massing tab.
fn massing(h: &mut Harness<BimApp>, label: &str) {
    press(h, "Massing");
    press(h, label);
}

#[test]
fn masses_are_clicked_out_as_a_footprint_previewed_and_finished_with_enter_or_the_first_point() {
    let mut h = app(BOX);
    massing(&mut h, "Mass");
    assert!(h.query_by_label("Modify | Create Mass").is_some());
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(5000.0, 1000.0));
    // The footprint a third corner would make, with its area.
    h.hover_at(h.state().screen(P2(5000.0, 4000.0)));
    h.run();
    assert!(painted_text(&h).contains(&"6.00 m²".to_string()), "{:?}", painted_text(&h));
    assert!(ids(&h, "mass").is_empty(), "a preview isn't an element, and nothing is made until it is finished");
    click(&mut h, P2(5000.0, 4000.0));
    click(&mut h, P2(1000.0, 4000.0));
    key(&mut h, Key::Enter);
    let first = ids(&h, "mass");
    assert_eq!(first.len(), 1);
    assert_eq!(
        (field(&h, first[0], "area_m2"), field(&h, first[0], "level"), field(&h, first[0], "top_level")),
        (json!(12.0), json!(100000), json!(100001))
    );
    assert_eq!(h.state().tool, Tool::Mass(Vec::new()), "ready for the next one");
    // Clicking back on the first point closes it too; two points make nothing.
    for p in [P2(6000.0, 1000.0), P2(7000.0, 1000.0), P2(7000.0, 2000.0), P2(6000.0, 1000.0)] {
        click(&mut h, p);
    }
    assert_eq!(ids(&h, "mass").len(), 2);
    click(&mut h, P2(6000.0, 3000.0));
    click(&mut h, P2(7000.0, 3000.0));
    key(&mut h, Key::Enter);
    assert_eq!(ids(&h, "mass").len(), 2);
    // Selected by its outline, in the plan of every level it passes through; its palette.
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    click(&mut h, P2(3000.0, 1000.0));
    assert_eq!(h.state().selection, first);
    assert!(h.query_by_label("Modify | Masses").is_some());
}

#[test]
fn the_massing_tab_makes_floors_walls_a_floor_and_a_roof_from_the_selected_mass() {
    let mut h =
        app("level {\"elevation\": 6000}\nmass {\"boundary\": [[0, 0], [10000, 0], [10000, 6000], [0, 6000]], \"top_level\": \"Level 3\"}");
    let mass = ids(&h, "mass");
    // With nothing selected there's nothing to make them from.
    massing(&mut h, "Mass Floors");
    assert!(ids(&h, "floor").is_empty());
    click(&mut h, P2(5000.0, 0.0));
    assert_eq!(h.state().selection, mass);
    massing(&mut h, "Mass Floors");
    assert_eq!(ids(&h, "floor").len(), 2, "Levels 1 and 2; Level 3 is its top");
    massing(&mut h, "Wall by Face");
    assert_eq!(ids(&h, "wall").len(), 4);
    massing(&mut h, "Floor by Face");
    massing(&mut h, "Roof by Face");
    assert_eq!((ids(&h, "floor").len(), ids(&h, "roof").len()), (3, 1));
    // Each its own undo step.
    key(&mut h, Key::Escape);
    for _ in 0..4 {
        h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
        h.run();
    }
    assert_eq!((ids(&h, "floor").len(), ids(&h, "wall").len(), ids(&h, "roof").len()), (0, 0, 0));
    // Their shortcuts.
    click(&mut h, P2(5000.0, 0.0));
    shortcut(&mut h, "MF");
    assert_eq!(ids(&h, "floor").len(), 2);
    shortcut(&mut h, "WB");
    shortcut(&mut h, "FB");
    shortcut(&mut h, "RB");
    assert_eq!((ids(&h, "wall").len(), ids(&h, "floor").len(), ids(&h, "roof").len()), (4, 3, 1));
    key(&mut h, Key::Escape);
    shortcut(&mut h, "MS");
    assert_eq!(h.state().tool, Tool::Mass(Vec::new()));
}

mod align;
mod autosave;
mod browser;
mod cad;
mod chinese;
mod clouds;
mod control;
mod dialog;
mod dimensions;
mod dock;
mod draw;
mod fills;
mod filter;
mod fonts;
mod gpu;
mod graphics;
mod groups;
mod light_canvas;
mod lines;
mod lineweights;
mod links;
mod look;
mod mep;
mod notes;
mod objectstyles;
mod patterns;
mod perspective;
mod planviews;
mod popup;
mod purge;
mod reinforce;
mod revit;
mod rooms;
mod section_box;
mod sketch;
mod smallest;
mod standards;
mod start;
mod templates;
mod titlebar;
mod tools;
mod trim;
mod typeprops;
mod window;

#[test]
fn a_mass_edits_its_levels_and_its_top_outline_in_its_properties_which_lofts_it() {
    let mut h =
        app("level {\"elevation\": 6000}\nmass {\"boundary\": [[0, 0], [10000, 0], [10000, 6000], [0, 6000]], \"top_level\": \"Level 3\"}");
    click(&mut h, P2(5000.0, 0.0));
    let mass = h.state().selection[0];
    assert!(h.query_by_label("Base Constraint").is_some() && h.query_by_label("Top Constraint").is_some());
    assert!(h.query_by_label("Top Outline").is_some());
    assert_eq!(field(&h, mass, "volume_m3"), json!(360.0));
    // Typed corner by corner as the footprint's are, twin for twin: 6 x 2 m in the middle.
    type_into(&mut h, "Top Outline", "2000, 2000; 8000, 2000; 8000, 4000; 2000, 4000");
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, mass, "top"), json!([[2000.0, 2000.0], [8000.0, 2000.0], [8000.0, 4000.0], [2000.0, 4000.0]]));
    assert_eq!(field(&h, mass, "volume_m3"), json!(200.0));
    // The wrong number of corners is refused, and the mass is as it was.
    type_into(&mut h, "Top Outline", "2000, 2000; 8000, 2000; 8000, 4000");
    key(&mut h, Key::Enter);
    assert!(h.state().status.error, "{}", h.state().status.text);
    assert_eq!(field(&h, mass, "volume_m3"), json!(200.0));
    // Blank takes the loft away.
    h.get_by_label("Top Outline").focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    key(&mut h, Key::Backspace);
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, mass, "volume_m3"), json!(360.0));
    assert!(field(&h, mass, "top").is_null());
}

#[test]
fn update_to_face_makes_what_was_made_by_face_follow_the_selected_mass_again() {
    let mut h = app(&format!("{BOX}\nmass {{\"boundary\": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], \"height\": 3000}}"));
    let mass = ids(&h, "mass");
    click(&mut h, P2(4000.0, 1500.0));
    assert_eq!(h.state().selection, mass);
    massing(&mut h, "Floor by Face");
    let floor = *ids(&h, "floor").last().unwrap();
    // The mass grows; nothing follows it until Update to Face.
    h.state_mut().session.execute("modify", &json!({"id": mass[0], "boundary": [[0, 0], [5000, 0], [5000, 3000], [0, 3000]]})).unwrap();
    let area_of = |h: &mut Harness<BimApp>| {
        h.state_mut().session.execute("elements", &json!({"category": "floor"})).unwrap()["elements"]
            .as_array()
            .and_then(|a| a.iter().find(|e| e["id"] == floor).and_then(|e| e["area_m2"].as_f64()))
    };
    assert_eq!(area_of(&mut h), Some(12.0));
    massing(&mut h, "Update to Face");
    assert_eq!(area_of(&mut h), Some(15.0));
    // Its shortcut; and with nothing selected it says to select a mass.
    h.state_mut().session.execute("modify", &json!({"id": mass[0], "boundary": [[0, 0], [6000, 0], [6000, 3000], [0, 3000]]})).unwrap();
    shortcut(&mut h, "UF");
    assert_eq!(area_of(&mut h), Some(18.0));
    key(&mut h, Key::Escape);
    shortcut(&mut h, "UF");
    assert!(h.state().status.error, "{}", h.state().status.text);
}

#[test]
fn show_mass_hides_and_shows_the_masses_in_the_view_on_screen() {
    let mut h = app(&format!("{BOX}\nmass {{\"boundary\": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], \"height\": 3000}}"));
    let hidden = |h: &Harness<BimApp>, view| {
        bimcraft_core::graphics::graphics_of(&h.state().session.model, view)
            .is_some_and(|g| g.categories.get("mass").is_some_and(|o| o.hidden))
    };
    assert!(!hidden(&h, 100_000));
    massing(&mut h, "Show Mass");
    assert!(hidden(&h, 100_000) && !hidden(&h, 100_001), "Level 1's plan alone");
    // Its shortcut turns them back on; a click on the mass picks nothing while it is hidden.
    shortcut(&mut h, "MH");
    assert!(!hidden(&h, 100_000));
    click(&mut h, P2(4000.0, 1500.0));
    assert_eq!(h.state().selection, ids(&h, "mass"));
    key(&mut h, Key::Escape);
    shortcut(&mut h, "MH");
    click(&mut h, P2(4000.0, 1500.0));
    assert!(h.state().selection.is_empty(), "a hidden mass can't be picked");
    // The 3D view is a view of its own; a sheet has no graphics.
    browse(&mut h, "{3D}");
    massing(&mut h, "Show Mass");
    assert!(hidden(&h, h.state().session.model.default_3d().unwrap_or_default()));
    let mut h = app(&format!("{BOX}\nsheet {{\"name\": \"Plans\"}}"));
    browse(&mut h, "A101 - Plans");
    massing(&mut h, "Show Mass");
    assert!(h.state().status.error, "{}", h.state().status.text);
}

mod structure;

mod updates {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use bimcraft_update::testing::{Fake, Scratch, release_json};
    use bimcraft_update::{Handover, LATEST_URL, State, Target, Updater, Version};

    use super::*;

    const INSTALLER: &[u8] = b"pretend this is an installer";

    /// What the stand-in for the installer's hand-over was given.
    type Handed = Arc<Mutex<Vec<(PathBuf, Option<PathBuf>)>>>;

    struct Rig {
        h: Harness<'static, BimApp>,
        net: Arc<Fake>,
        dir: Scratch,
        handed: Handed,
        /// While set, the stand-in for the install fails.
        failing: Arc<AtomicBool>,
    }

    /// The app, running 0.4.0, with an updater on a network where `tag` is the latest release (with an installer), working as it's
    /// asked; `installing` is whether this copy can be updated in place.
    fn rig(tag: &str, installing: bool) -> Rig {
        let mut h = app(BOX);
        let dir = Scratch::new("ui-updates");
        let installer_url =
            format!("https://github.com/IPedrax/BIMCraft/releases/download/{tag}/bimcraft-{}-windows-x64.msi", tag.trim_start_matches('v'));
        let net = Arc::new(Fake::default().answer(LATEST_URL, release_json(tag, Some(INSTALLER))).answer(&installer_url, INSTALLER));
        let handed: Handed = Arc::default();
        let (log, failing) = (Arc::clone(&handed), Arc::new(AtomicBool::new(false)));
        let fails = Arc::clone(&failing);
        let updater = Updater::new(net.clone(), Version::new(0, 4, 0), Some(dir.0.join("update.json")), || {})
            .inline()
            .target(installing.then_some(Target::Windows))
            .in_folder(dir.0.clone())
            .handing_over(move |job: &Handover| {
                log.lock().unwrap().push((job.file.clone(), job.project.clone()));
                if fails.load(Ordering::SeqCst) { Err("no room".to_string()) } else { Ok(()) }
            });
        h.state_mut().updates = Some(updater);
        Rig { h, net, dir, handed, failing }
    }

    impl Rig {
        fn updater(&mut self) -> &mut Updater {
            self.h.state_mut().updates.as_mut().unwrap()
        }

        /// The startup check, and the frames that take its answer.
        fn start(&mut self) {
            self.updater().check_at_startup();
            self.h.run();
        }

        fn menu(&mut self, item: &str) {
            press(&mut self.h, "File");
            self.h.get_by_label(item).click();
            self.h.run();
        }
    }

    const OFFER: &str = "BIMCraft 0.5.0 is available. You have 0.4.0.";

    #[test]
    fn a_window_without_an_updater_shows_none_of_it() {
        let mut h = app(BOX);
        assert!(h.state().updates.is_none());
        press(&mut h, "File");
        assert!(h.query_by_label("Check for Updates Now").is_none() && h.query_by_label("Check for updates at startup").is_none());
    }

    #[test]
    fn a_newer_release_is_offered_in_a_banner_and_nothing_else_changes() {
        let mut r = rig("v0.5.0", true);
        assert!(r.h.query_by_label(OFFER).is_none(), "nothing before the check comes back");
        let model = Arc::clone(&r.h.state().session.model);
        r.start();
        assert!(r.h.query_by_label(OFFER).is_some());
        for label in ["Update now", "Later", "Skip this version", "What's new"] {
            assert!(r.h.query_by_label(label).is_some(), "{label}");
        }
        assert!(r.h.query_by_label("Download").is_none(), "this copy can install it");
        assert!(Arc::ptr_eq(&model, &r.h.state().session.model) && !r.h.state().dirty(), "the project is as it was");
        assert!(r.h.state().status.text.is_empty(), "and the status box has nothing to say");
        assert_eq!(r.net.asked(), [LATEST_URL], "only the release was asked for");
    }

    #[test]
    fn later_hides_the_banner_and_skip_hides_it_for_good() {
        let mut r = rig("v0.5.0", true);
        r.start();
        press(&mut r.h, "Later");
        assert!(r.h.query_by_label(OFFER).is_none());
        r.start();
        assert!(r.h.query_by_label(OFFER).is_some(), "asked again at the next start");
        press(&mut r.h, "Skip this version");
        assert!(r.h.query_by_label(OFFER).is_none());
        r.start();
        assert!(r.h.query_by_label(OFFER).is_none(), "skipped stays skipped, and is kept in the settings file");
        assert!(r.dir.0.join("update.json").exists());
    }

    #[test]
    fn update_now_downloads_then_restart_hands_the_installer_over_and_closes() {
        let mut r = rig("v0.5.0", true);
        r.start();
        press(&mut r.h, "Update now");
        assert!(r.h.query_by_label("BIMCraft 0.5.0 is ready. It will close, update and start again.").is_some());
        assert!(r.h.query_by_label("Update now").is_none() && r.handed.lock().unwrap().is_empty(), "nothing is installed yet");
        assert!(!r.h.state().closing);
        press(&mut r.h, "Restart and install");
        let handed = r.handed.lock().unwrap().clone();
        assert_eq!(handed.len(), 1);
        assert_eq!(std::fs::read(&handed[0].0).unwrap(), INSTALLER, "what was downloaded and checked");
        assert_eq!(handed[0].1, None, "a project never saved has no file to open again");
        assert!(r.h.state().closing, "the window closes so the installer can replace the program");
        assert!(r.h.query_by_label("Restart and install").is_none(), "the banner goes once the installer is handed over");
    }

    #[test]
    fn restart_keeps_the_project_open_and_asks_about_unsaved_changes_first() {
        let mut r = rig("v0.5.0", true);
        let project = r.dir.0.join("house.bimcraft");
        assert!(r.h.state_mut().save_to(&project));
        r.h.state_mut().session.execute("wall", &json!({"points": [[0, 0], [4000, 0]]})).unwrap();
        assert!(r.h.state().dirty());
        r.start();
        press(&mut r.h, "Update now");
        press(&mut r.h, "Restart and install");
        assert!(r.h.query_by_label_contains("Save changes to").is_some(), "unsaved work is asked about, not dropped");
        assert!(r.handed.lock().unwrap().is_empty() && !r.h.state().closing);
        press(&mut r.h, "Cancel");
        assert!(r.handed.lock().unwrap().is_empty() && !r.h.state().closing, "Cancel leaves everything as it was");
        assert!(r.h.query_by_label("Restart and install").is_some(), "and the update still ready");
        press(&mut r.h, "Restart and install");
        // The Quick Access Toolbar has a Save too: the dialog's is the lower one.
        let save = r.h.get_all_by_label("Save").max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).map(|n| n.rect().center());
        click_at(&mut r.h, save.unwrap());
        assert!(!r.h.state().dirty(), "saved");
        let handed = r.handed.lock().unwrap().clone();
        assert_eq!(handed[0].1.as_deref(), Some(project.as_path()), "the project is opened again when BIMCraft starts");
        assert!(r.h.state().closing);
    }

    #[test]
    fn an_install_that_fails_says_why_and_the_update_is_ready_to_try_again() {
        let mut r = rig("v0.5.0", true);
        r.failing.store(true, Ordering::SeqCst);
        r.start();
        press(&mut r.h, "Update now");
        press(&mut r.h, "Restart and install");
        let status = r.h.state().status.clone();
        assert!(status.error && status.text == "Couldn't install the update: no room", "{status:?}");
        assert!(!r.h.state().closing, "the window stays, with the project in it");
        assert!(r.h.query_by_label("Restart and install").is_some() && r.h.query_by_label("Installing BIMCraft 0.5.0…").is_none());
        r.failing.store(false, Ordering::SeqCst);
        press(&mut r.h, "Restart and install");
        assert!(r.h.state().closing && r.handed.lock().unwrap().len() == 2, "the second try went through");
    }

    /// Frames a few milliseconds apart until `done`: for an updater on threads of its own, as in the app.
    fn wait(h: &mut Harness<'static, BimApp>, done: impl Fn(&BimApp) -> bool) {
        for _ in 0..2000 {
            h.step();
            if done(h.state()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("gave up waiting: {:?}", h.state().updates.as_ref().map(|u| u.state().clone()));
    }

    #[test]
    fn while_an_update_installs_the_window_says_so_and_holds_still_until_it_closes() {
        let mut r = rig("v0.5.0", true);
        // An install that waits for the test, as a slow one does, and then goes through or fails as it's told.
        let (open, gate) = std::sync::mpsc::channel::<bool>();
        let gate = Mutex::new(gate);
        let updater = Updater::new(r.net.clone(), Version::new(0, 4, 0), None, || {})
            .target(Some(Target::Windows))
            .in_folder(r.dir.0.clone())
            .handing_over(move |_| {
                let works = gate.lock().map_err(|e| e.to_string())?.recv_timeout(Duration::from_secs(20)).map_err(|e| e.to_string())?;
                if works { Ok(()) } else { Err("no room".to_string()) }
            });
        r.h.state_mut().updates = Some(updater);
        // A project that is saved, with a wall in its history that Ctrl+Z could take out: nothing is asked when the install starts.
        let bare = app(BOX).state().view_rect;
        r.h.state_mut().session.execute("wall", &json!({"points": [[0, 0], [0, 9000]]})).unwrap();
        assert!(r.h.state_mut().save_to(&r.dir.0.join("house.bimcraft")));
        r.updater().check_at_startup();
        wait(&mut r.h, |app| app.has_update_banner());
        r.h.get_by_label("Update now").click();
        wait(&mut r.h, |app| app.updates.as_ref().is_some_and(|u| matches!(u.state(), State::Ready { .. })));
        r.h.get_by_label("Restart and install").click();
        // The box has a spinner, which asks for frame after frame: `run` would never settle.
        r.h.step();
        r.h.step();
        assert!(r.h.state().installing());
        assert!(r.h.query_by_label("Installing BIMCraft 0.5.0…").is_some(), "the box says what is going on");
        assert!(r.h.query_by_label("Restart and install").is_none(), "and the banner gives way to it");
        assert_eq!(r.h.state().view_rect, bare, "leaving no empty strip of it over the views");
        // Nothing can start or change under it: a new project, an undo, the system asking to quit or to open a file, or the window's
        // close button, which would end the install half way.
        let model = Arc::clone(&r.h.state().session.model);
        let walls_now = walls(&r.h).len();
        r.h.key_press_modifiers(Modifiers::COMMAND, Key::N);
        r.h.step();
        assert!(Arc::ptr_eq(&model, &r.h.state().session.model), "Ctrl+N started a new project under the box");
        r.h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
        r.h.step();
        assert_eq!(walls(&r.h).len(), walls_now, "Ctrl+Z undid a wall under the box");
        // The system asking to quit waits in its inbox, which is not emptied meanwhile.
        let inbox: std::rc::Rc<std::cell::RefCell<Vec<crate::OsEvent>>> = Default::default();
        let queue = std::rc::Rc::clone(&inbox);
        r.h.state_mut().os_events = Some(Box::new(move || queue.borrow_mut().drain(..).collect()));
        inbox.borrow_mut().push(crate::OsEvent::Quit);
        r.h.step();
        assert_eq!(inbox.borrow().len(), 1, "the system's Quit was left for after the install");
        assert!(!r.h.state().closing && r.h.state().pending.is_none() && r.h.state().status.text.is_empty());
        r.h.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
        r.h.step();
        let cancelled =
            r.h.output()
                .viewport_output
                .get(&egui::ViewportId::ROOT)
                .is_some_and(|v| v.commands.contains(&egui::ViewportCommand::CancelClose));
        assert!(cancelled && !r.h.state().closing && r.h.state().pending.is_none(), "closing the window waits for the install");
        // The install fails: the window says so, and is heard about the Quit that waited.
        open.send(false).unwrap();
        wait(&mut r.h, |app| app.closing);
        let status = r.h.state().status.clone();
        assert!(status.error && status.text == "Couldn't install the update: no room", "{status:?}");
        assert!(inbox.borrow().is_empty(), "the Quit was heard once the install was over");
    }

    #[test]
    fn a_copy_that_cannot_update_itself_is_told_and_given_the_link() {
        let mut r = rig("v0.5.0", false);
        r.start();
        assert!(r.h.query_by_label(OFFER).is_some());
        assert!(r.h.query_by_label("Update now").is_none(), "no promise it can't keep");
        // What the frame of the click asked the system to open.
        let opened = |h: &Harness<BimApp>| -> Vec<String> {
            let commands = &h.output().platform_output.commands;
            commands.iter().filter_map(|c| if let egui::OutputCommand::OpenUrl(u) = c { Some(u.url.clone()) } else { None }).collect()
        };
        let page = "https://github.com/IPedrax/BIMCraft/releases/tag/v0.5.0";
        for button in ["Download", "What's new"] {
            r.h.get_by_label(button).click();
            r.h.step();
            assert_eq!(opened(&r.h), [page], "{button} opens the release's page, which was built from its tag");
            r.h.run();
        }
        assert!(matches!(r.h.state().updates.as_ref().unwrap().state(), State::Available(_)), "it only opens the page");
        assert_eq!(r.net.asked(), [LATEST_URL], "and downloads nothing");
        for label in ["Later", "Skip this version"] {
            assert!(r.h.query_by_label(label).is_some(), "{label}");
        }
    }

    #[test]
    fn a_release_without_an_installer_is_only_told_of_even_where_it_could_be_installed() {
        let mut r = rig("v0.5.0", true);
        let net = Arc::new(Fake::default().answer(LATEST_URL, release_json("v0.5.0", None)));
        r.h.state_mut().updates =
            Some(Updater::new(net, Version::new(0, 4, 0), None, || {}).inline().target(Some(Target::Windows)).in_folder(r.dir.0.clone()));
        r.start();
        assert!(r.h.query_by_label(OFFER).is_some());
        assert!(r.h.query_by_label("Update now").is_none(), "there is nothing to download");
        assert!(r.h.query_by_label("Download").is_some(), "so the page is what it offers");
    }

    #[test]
    fn an_updater_with_nothing_to_say_takes_no_room() {
        let mut r = rig("v0.4.0", true);
        let bare = app(BOX).state().view_rect;
        r.start();
        assert!(matches!(r.h.state().updates.as_ref().unwrap().state(), State::Idle));
        assert_eq!(r.h.state().view_rect, bare, "the views sit where they do without one: no empty strip over them");
    }

    #[test]
    fn the_file_menu_checks_by_hand_and_switches_the_startup_check() {
        let mut r = rig("v0.4.0", true);
        r.menu("Check for Updates Now");
        assert_eq!(r.h.state().status.text, "BIMCraft 0.4.0 is up to date.");
        assert!(!r.h.state().status.error);
        assert!(r.h.query_by_label(OFFER).is_none());
        // Off, and it stays off for the next run.
        press(&mut r.h, "File");
        r.h.get_by_label("Check for updates at startup").click();
        r.h.run();
        assert!(!r.updater().checks_at_startup());
        let net = Arc::clone(&r.net);
        r.updater().check_at_startup();
        assert_eq!(net.asked().len(), 1, "no check at startup once it's off");
        let settings = bimcraft_update::prefs::Prefs::load(&r.dir.0.join("update.json"));
        assert!(!settings.check, "kept in the settings file");
    }

    #[test]
    fn a_failed_check_by_hand_says_so_in_the_status_box_and_one_at_startup_does_not() {
        let mut r = rig("v0.5.0", true);
        let net = Arc::new(Fake::default().fail(LATEST_URL, "offline"));
        let updater = Updater::new(net, Version::new(0, 4, 0), None, || {}).inline();
        r.h.state_mut().updates = Some(updater);
        r.start();
        assert!(r.h.state().status.text.is_empty() && r.h.query_by_label(OFFER).is_none());
        r.menu("Check for Updates Now");
        let status = r.h.state().status.clone();
        assert!(status.error && status.text.starts_with("Couldn't check for updates:") && status.text.contains("offline"), "{status:?}");
    }

    #[test]
    fn a_download_that_fails_its_check_says_why_and_the_offer_stays() {
        let mut r = rig("v0.5.0", true);
        let url = "https://github.com/IPedrax/BIMCraft/releases/download/v0.5.0/bimcraft-0.5.0-windows-x64.msi";
        let net = Arc::new(Fake::default().answer(LATEST_URL, release_json("v0.5.0", Some(INSTALLER))).answer(url, "another file"));
        let updater =
            Updater::new(net, Version::new(0, 4, 0), None, || {}).inline().target(Some(Target::Windows)).in_folder(r.dir.0.clone());
        r.h.state_mut().updates = Some(updater);
        r.start();
        press(&mut r.h, "Update now");
        let status = r.h.state().status.clone();
        assert!(status.error && status.text.starts_with("Couldn't download the update:"), "{status:?}");
        assert!(r.h.query_by_label(OFFER).is_some() && r.h.query_by_label("Update now").is_some(), "it can be tried again");
    }

    #[test]
    fn the_updater_survives_new_and_open() {
        let mut r = rig("v0.5.0", true);
        r.start();
        press(&mut r.h, "File");
        r.h.get_by_label_contains("New").click();
        r.h.run();
        assert!(r.h.state().updates.is_some() && r.h.query_by_label(OFFER).is_some(), "a new project is still in the version offered");
    }

    #[test]
    fn the_banner_and_the_menu_speak_portuguese_and_spanish() {
        use crate::lang::Lang;
        let cases = [
            (
                Lang::Portuguese,
                "O BIMCraft 0.5.0 está disponível. Você tem a versão 0.4.0.",
                ["Atualizar agora", "Depois", "Ignorar esta versão", "O que há de novo"],
                ["Verificar atualizações ao iniciar", "Verificar atualizações agora"],
                "O BIMCraft 0.5.0 está pronto. Ele vai fechar, atualizar e abrir de novo.",
                "Reiniciar e instalar",
            ),
            (
                Lang::Spanish,
                "BIMCraft 0.5.0 está disponible. Tienes la versión 0.4.0.",
                ["Actualizar ahora", "Más tarde", "Omitir esta versión", "Novedades"],
                ["Buscar actualizaciones al iniciar", "Buscar actualizaciones ahora"],
                "BIMCraft 0.5.0 está listo. Se cerrará, se actualizará y se abrirá de nuevo.",
                "Reiniciar e instalar",
            ),
        ];
        for (language, offer, buttons, menu, ready, restart) in cases {
            let mut r = rig("v0.5.0", true);
            r.h.state_mut().lang = language;
            r.start();
            assert!(r.h.query_by_label(offer).is_some(), "{language:?}: {offer}");
            for label in buttons {
                assert!(r.h.query_by_label(label).is_some(), "{language:?}: {label}");
            }
            press(&mut r.h, buttons[0]);
            assert!(r.h.query_by_label(ready).is_some(), "{language:?}: {ready}");
            assert!(r.h.query_by_label(restart).is_some(), "{language:?}: {restart}");
            press(&mut r.h, crate::lang::translated_in(language, "File").unwrap());
            for label in menu {
                assert!(r.h.query_by_label(label).is_some(), "{language:?}: {label}");
            }
        }
        // Every word the updater's windows use is in the table in both languages, however it got there.
        let words = [
            "BIMCraft {} is available. You have {}.",
            "Update now",
            "Download",
            "Later",
            "Skip this version",
            "What's new",
            "Downloading BIMCraft {}…",
            "BIMCraft {} is ready. It will close, update and start again.",
            "Restart and install",
            "Check for updates at startup",
            "Check for Updates Now",
            "BIMCraft {} is up to date.",
            "Couldn't check for updates: {}",
            "Couldn't download the update: {}",
            "Couldn't install the update: {}",
            "Installing BIMCraft {}…",
            "Couldn't save the setting: {}",
            "Downloads and checks the update; BIMCraft then closes, updates and starts again.",
            "Opens the release page on GitHub. This copy can't update itself.",
            "Asks to save first if there are unsaved changes, then closes BIMCraft, installs the update and starts it again.",
        ];
        for language in [Lang::Portuguese, Lang::Spanish] {
            for word in words {
                assert!(crate::lang::translated_in(language, word).is_some(), "{language:?} has no {word:?}");
            }
        }
    }
}

/// A right click at a point on screen.
fn right_click_at(h: &mut Harness<BimApp>, pos: Pos2) {
    h.hover_at(pos);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
    }
    h.run();
}

/// Which of `labels` a right click at `pos` offers: those the menu it opens adds to the window
/// (the navigation bar's Zoom to Fit is there anyway).
fn offered(h: &mut Harness<BimApp>, pos: Pos2, labels: &[&'static str]) -> Vec<&'static str> {
    let count = |h: &Harness<BimApp>, l: &str| h.query_all_by(|n| n.label().is_some_and(|x| x.starts_with(l))).count();
    let before: Vec<usize> = labels.iter().map(|l| count(h, l)).collect();
    right_click_at(h, pos);
    labels.iter().zip(before).filter(|(l, n)| count(h, l) > *n).map(|(l, _)| *l).collect()
}

/// The open menu's item `label` (its shortcut beside it): the one drawn last, over the view.
fn choose_item(h: &mut Harness<BimApp>, label: &str) {
    h.get_all_by(|n| n.label().is_some_and(|x| x.starts_with(label))).last().unwrap().click();
    h.run();
}

const MENU: [&str; 8] = ["Cancel", "Zoom to Fit", "Zoom In", "Home View", "Edit Type", "Select All Instances", "Hide Element", "Delete"];

#[test]
fn a_right_click_in_a_plan_opens_a_menu_of_what_the_view_and_the_selection_can_do() {
    let mut h = app(BOX);
    // Nothing selected: the view's own items, not the selection's.
    let at = h.state().screen(P2(4000.0, 3000.0));
    assert_eq!(offered(&mut h, at, &MENU), ["Cancel", "Zoom to Fit", "Zoom In"]);
    // Zoom In zooms about the view's middle, and the menu closes.
    let before = h.state().screen(P2(0.0, 0.0));
    choose_item(&mut h, "Zoom In");
    assert_ne!(h.state().screen(P2(0.0, 0.0)), before);
    assert_eq!(h.query_all_by_label("Zoom Out").count(), 1, "the menu closed: only the navigation bar's");
    // A wall selected: Edit Type opens its type's properties.
    let south = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 0.0)));
    click(&mut h, P2(4000.0, 0.0));
    assert_eq!(h.state().selection, [south]);
    let at = h.state().screen(P2(4000.0, 3000.0));
    assert_eq!(
        offered(&mut h, at, &MENU),
        ["Cancel", "Zoom to Fit", "Zoom In", "Edit Type", "Select All Instances", "Hide Element", "Delete"]
    );
    choose_item(&mut h, "Edit Type");
    assert!(h.state().type_dialog.is_some(), "Type Properties is open");
    assert_eq!(h.state().selection, [south], "a right click leaves the selection as it was");
}

#[test]
fn every_view_has_the_right_click_menu() {
    let mut h = app(&format!(
        "{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}\nroom {{\"point\": [4000, 3000], \"name\": \"Hall\"}}"
    ));
    // The 3D view: Home View among its items.
    press(&mut h, "Default 3D View");
    let at = h.state().orbit.rect.center();
    assert_eq!(offered(&mut h, at, &MENU), ["Cancel", "Zoom to Fit", "Zoom In", "Home View"]);
    key(&mut h, Key::Escape);
    // A section.
    browse(&mut h, "Section 1");
    let at = h.state().screen(P2(5000.0, 1500.0));
    assert_eq!(offered(&mut h, at, &MENU), ["Cancel", "Zoom to Fit", "Zoom In"]);
    key(&mut h, Key::Escape);
    // A schedule: no zooming; its row's room selected, deleted from the menu.
    browse(&mut h, "Room Schedule");
    h.get_by_label("Hall").click();
    h.run();
    let at = h.get_by_label("Hall").rect().center();
    assert_eq!(offered(&mut h, at, &MENU), ["Cancel", "Edit Type", "Select All Instances", "Delete"]);
    choose_item(&mut h, "Delete");
    assert!(ids(&h, "room").is_empty());
}

#[test]
fn a_selected_levels_ends_are_grips_in_a_section_that_drag_its_extent() {
    let mut h = app(&format!(
        "{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}\nlevel {{\"elevation\": 1500, \"name\": \"Mezzanine\", \"extent\": [[0, 0], [4000, 6000]]}}"
    ));
    let mezz = find(&h, |e| matches!(e, Element::Level(l) if l.name == "Mezzanine"));
    browse(&mut h, "Section 1");
    click(&mut h, P2(3000.0, 1500.0));
    assert_eq!(h.state().selection, [mezz]);
    // Its far end, along the line from 5000 to 7000: the extent's east edge goes 2 m east.
    drag(&mut h, P2(5000.0, 1500.0), P2(7000.0, 1500.0));
    assert_eq!(field(&h, mezz, "extent"), json!([[0.0, 0.0], [6000.0, 6000.0]]));
    assert_eq!(h.state().selection, [mezz], "the grip took the drag, not a selection box");
    // And its near end back west.
    drag(&mut h, P2(1000.0, 1500.0), P2(500.0, 1500.0));
    assert_eq!(field(&h, mezz, "extent"), json!([[-500.0, 0.0], [6000.0, 6000.0]]));
}

#[test]
fn a_selected_levels_plane_has_a_grip_on_each_edge_in_3d_that_drags_it() {
    let mut h = app(&format!("{BOX}\nlevel {{\"elevation\": 9000, \"name\": \"Top\", \"extent\": [[0, 0], [8000, 6000]]}}"));
    let top = find(&h, |e| matches!(e, Element::Level(l) if l.name == "Top"));
    press(&mut h, "Default 3D View");
    // Its far north edge picks it.
    let at = h.state().screen3([4000.0, 6000.0, 9000.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [top]);
    let before = h.state().orbit.camera;
    // The east edge's grip, dragged a metre east on the level's plane.
    let (a, b) = (h.state().screen3([8000.0, 3000.0, 9000.0]), h.state().screen3([9000.0, 3000.0, 9000.0]));
    drag_screen(&mut h, a, b, PointerButton::Primary);
    let extent = field(&h, top, "extent");
    assert!((extent[1][0].as_f64().unwrap() - 9000.0).abs() <= 10.0, "{extent}");
    assert_eq!((extent[0].clone(), extent[1][1].clone()), (json!([0.0, 0.0]), json!(6000.0)));
    assert_eq!(h.state().orbit.camera, before, "the grip took the drag, not the camera");
}

#[test]
fn select_by_id_finds_elements_by_id_guid_or_ifc_globalid_says_what_it_missed_and_shows_them() {
    let mut h = app(&format!("{BOX}\nwall {{\"points\": [[20000, 20000], [23000, 20000]], \"level\": \"Level 2\"}}"));
    let walls = ids(&h, "wall");
    let (high, low) = (walls[4], walls[1]);
    let ifc = bimcraft_core::ifc::compress(gid(&h, low).as_u128());
    // Manage grew with Room Area Computation and Technical Standards: the ribbon folds Inquiry away in a narrower window.
    h.set_size(vec2(1800.0, 900.0));
    press(&mut h, "Manage");
    press(&mut h, "Select by ID");
    // Live: what the list finds, and what it doesn't, as it is typed.
    let list = format!("{} {ifc} 100099", gid(&h, high));
    type_into(&mut h, "IDs, GUIDs or IFC GlobalIds, as many as you like:", &list);
    assert!(h.query_by_label("Finds 2; nothing has these: 100099").is_some());
    press(&mut h, "OK");
    assert_eq!(h.state().selection, [high, low]);
    assert!(!h.state().status.error && h.state().status.text.contains("100099"), "found some: a note of the rest, no error");
    // Shown: the plan of the first one's level, zoomed in on them.
    assert_eq!(h.state().sheet, Sheet::Plan(100_001));
    h.run();
    let at = h.state().screen(P2(21500.0, 20000.0));
    assert!(h.ctx.content_rect().contains(at), "{at:?}");
    // Added to the selection, unshown: the view stays.
    press(&mut h, "Select by ID");
    type_into(&mut h, "IDs, GUIDs or IFC GlobalIds, as many as you like:", &walls[0].to_string());
    pick(&mut h, Role::CheckBox, "Add to the selection");
    pick(&mut h, Role::CheckBox, "Show: open the plan they are in, zoomed to them");
    press(&mut h, "OK");
    assert_eq!(h.state().selection, [high, low, walls[0]]);
    assert_eq!(h.state().sheet, Sheet::Plan(100_001));
    assert!(!h.state().status.error);
    // A material's id is the library's, not an element to select: it says so, and the selection stays.
    let brick = h.state().session.model.library.id(&bimcraft_core::library::Kind::Material, "Brick").unwrap();
    press(&mut h, "Select by ID");
    type_into(&mut h, "IDs, GUIDs or IFC GlobalIds, as many as you like:", &brick.to_string());
    assert!(h.query_by_label("Finds 1 in the library, not elements.").is_some());
    press(&mut h, "OK");
    assert!(h.state().status.text.contains(&format!("material Brick ({brick})")), "{}", h.state().status.text);
    assert_eq!(h.state().selection.len(), 3);
    // Nothing found: OK waits, and Cancel leaves the selection be.
    press(&mut h, "Select by ID");
    type_into(&mut h, "IDs, GUIDs or IFC GlobalIds, as many as you like:", "nowhere");
    assert!(h.query_by_label("Finds 0; nothing has these: nowhere").is_some());
    h.get_by_label("OK").click();
    h.run();
    press(&mut h, "Cancel");
    assert_eq!(h.state().selection.len(), 3);
}

#[test]
fn ids_of_selection_shows_each_ones_id_guid_and_ifc_globalid_and_copies_them() {
    let mut h = app(BOX);
    let walls = ids(&h, "wall");
    press(&mut h, "Manage");
    // Nothing selected: nothing to show.
    from_list(&mut h, "Inquiry", "IDs of Selection");
    assert!(h.query_by_label("Copy All").is_none());
    h.state_mut().selection = vec![walls[0], walls[2]];
    h.run();
    from_list(&mut h, "Inquiry", "IDs of Selection");
    let (a, b) = (gid(&h, walls[0]), gid(&h, walls[2]));
    for text in [walls[0].to_string(), a.to_string(), bimcraft_core::ifc::compress(b.as_u128()), "2 selected.".to_string()] {
        assert!(h.query_by_label(&text).is_some(), "{text}");
    }
    press(&mut h, "Copy GUIDs");
    assert_eq!(h.state().copied.as_deref(), Some(format!("{a}\n{b}").as_str()));
    press(&mut h, "Copy IDs");
    assert_eq!(h.state().copied.as_deref(), Some(format!("{}\n{}", walls[0], walls[2]).as_str()));
    press(&mut h, "Copy All");
    let all = h.state().copied.clone().unwrap();
    let lines: Vec<&str> = all.lines().collect();
    assert_eq!(lines[0], "Category\tName\tID\tGUID\tIFC GlobalId");
    assert!(lines[1].starts_with("wall\tGeneric - 200mm\t") && lines[1].contains(&a.to_string()) && lines.len() == 3, "{all}");
    assert!(h.query_by_label("Copied 2 rows, tab-separated.").is_some());
    h.get_all_by_label("Copy").next().unwrap().click();
    h.run();
    assert!(h.state().copied.as_deref().is_some_and(|c| c.starts_with(&format!("{}\t{a}\t", walls[0]))));
    press(&mut h, "Close");
    assert!(h.query_by_label("Copy All").is_none());
}

#[test]
fn plan_cache_reuses_drawings_and_invalidates_geometry_view_and_ceiling() {
    let mut s = Session::new();
    s.script(BOX).unwrap();
    let level = s.model.levels()[0].0;
    let view = bimcraft_core::render::plan(&s.model, level, 800, 600).view;
    let mut cache = crate::canvas::PlanCache::default();
    let first = cache.drawing(&s.model, level, view, false);
    let again = cache.drawing(&s.model, level, view, false);
    assert!(Arc::ptr_eq(&first, &again), "idle and pointer-only frames reuse geometry");
    let zoom = cache.drawing(&s.model, level, bimcraft_core::render::View { scale: view.scale * 2.0, ..view }, false);
    assert!(!Arc::ptr_eq(&first, &zoom));
    let ceiling = cache.drawing(&s.model, level, view, true);
    assert!(!Arc::ptr_eq(&first, &ceiling));
    s.execute("wall", &json!({"points": [[2000, 0], [2000, 6000]]})).unwrap();
    let changed = cache.drawing(&s.model, level, view, false);
    assert!(!Arc::ptr_eq(&first, &changed));
    assert_ne!(first.prims.len(), changed.prims.len());
    s.execute("undo", &json!({})).unwrap();
    let undone = cache.drawing(&s.model, level, view, false);
    assert!(!Arc::ptr_eq(&changed, &undone));
    assert_eq!(first.prims.len(), undone.prims.len());
    let other = s.model.levels()[1].0;
    let upper = cache.drawing(&s.model, other, view, false);
    assert!(!Arc::ptr_eq(&undone, &upper));
}

#[test]
fn opening_a_file_with_fields_a_newer_version_added_says_they_are_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("newer.bimcraft");
    let mut s = Session::new();
    s.execute("wall", &json!({"points": [[0, 0], [4000, 0]]})).unwrap();
    let mut v = serde_json::to_value(&*s.model).unwrap();
    v["weather"] = json!("sunny");
    v["elements"]["100002"]["finish_date"] = json!("2027-01-01");
    std::fs::write(&project, v.to_string()).unwrap();
    let mut h = app("");
    assert!(h.state_mut().open_path(&project));
    h.run();
    assert_eq!(walls(&h).len(), 1);
    let status = &h.state().status;
    assert!(!status.error && status.text.contains("newer.bimcraft opened without 2 fields"), "{}", status.text);
}

#[test]
fn the_arrows_beside_undo_and_redo_list_the_steps_by_name_and_take_back_as_many_as_picked() {
    let mut h = app("");
    h.state_mut().run("wall", json!({"points": [[0, 0], [4000, 0], [4000, 3000]]}));
    let first = h.state().session.model.elements.iter().find(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id).unwrap();
    h.state_mut().run("door", json!({"wall": first, "offset": 1500}));
    // The second wall: the first would take its door with it.
    let first = first + 1;
    h.state_mut().run("delete", json!({"ids": [first]}));
    h.run();
    press(&mut h, "Undo History");
    // Newest first: what each step made or took away.
    h.get_by_label("Delete Wall");
    h.get_by_label("Create Door");
    h.get_by_label("Create Walls");
    // Hovering a step marks it and those above it, and a click undoes them all.
    h.get_by_label("Create Door").hover();
    h.run();
    h.get_by_label("Undo 2 steps");
    press(&mut h, "Create Door");
    assert_eq!(walls(&h).len(), 2);
    assert!(h.state().session.model.elements.values().all(|e| !matches!(e, Element::Door(_))));
    press(&mut h, "Redo History");
    h.get_by_label("Create Door");
    press(&mut h, "Delete Wall");
    assert_eq!(walls(&h).len(), 1, "redone both");
    assert!(h.state().session.steps(true).is_empty());
}

#[test]
fn a_view_tabs_close_button_is_part_of_its_tab() {
    let mut h = app(BOX);
    h.state_mut().open_sheet(Sheet::View3D);
    h.run();
    let tab = h.get_all_by_label("{3D}").min_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap().rect();
    let close = h.get_by_label("Close {3D}").rect();
    assert!(tab.contains_rect(close), "the × is inside the tab: {tab:?} {close:?}");
    press(&mut h, "Close {3D}");
    assert!(!h.state().views.contains(&Sheet::View3D));
}

/// Manage > Room Area Computation: the button drops the two choices down, a pick sets the project's `room_area` through the command,
/// and undo puts it back.
#[test]
fn room_area_computation_is_set_from_manage_and_undone() {
    use bimcraft_core::model::RoomArea;
    let mut h = app(BOX);
    h.state_mut().tab = Tab::Manage;
    h.run();
    assert_eq!(h.state().session.model.room_area, RoomArea::Centre);
    press(&mut h, "Room Area Computation");
    assert!(h.query_by_label("At wall centre line").is_some(), "both choices are on screen");
    press(&mut h, "At wall finish");
    assert_eq!(h.state().session.model.room_area, RoomArea::Finish);
    h.state_mut().run("undo", json!({}));
    h.run();
    assert_eq!(h.state().session.model.room_area, RoomArea::Centre);
}

mod background_grid;
