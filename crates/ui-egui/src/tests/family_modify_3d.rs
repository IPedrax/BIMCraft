//! The Modify panel's tools in the family's {3D}. Move, Copy, Rotate and Mirror - Draw Axis work there on the view's
//! work plane (Ref. Level until Work Plane > Set picks another): a click lands on the plane, and the edit is the one a plan
//! makes. Mirror - Pick Axis, Offset and Align, which pick something of the view, stay greyed there, saying why.

use super::family_drag_move::{ed, family, family_with, pick, points, status};
use super::family_selection::command;
use super::family_views::{editor, open, view};
use super::*;
use crate::editor::modify::Tool as M;
use crate::editor::select_box::Picked;
use bimcraft_core::family::views::Which;

/// Center (Front/Back), the family's upright plane through its origin, facing front.
const FRONT_BACK: &str = "9e3c0a10-1f0b-4d8c-8a5e-0000000000a2";

/// The Movers family (box A from 100 to 500 square and 300 high, box B west of it) in its {3D}, drawn.
fn in_3d() -> Harness<'static, BimApp> {
    let (mut h, _) = open(&family_with(""), "Movers");
    view(&mut h, None);
    for _ in 0..4 {
        h.run();
    }
    assert!(editor(&h).views.shown.is_none(), "{{3D}} shown");
    h
}

/// A click in {3D} where family point `p` shows.
fn click3(h: &mut Harness<BimApp>, p: [f64; 3]) {
    let at = h.state().family_screen3(p).expect("{3D} drawn");
    click_at(h, at);
}

fn tool(h: &Harness<BimApp>) -> Option<M> {
    editor(h).modify_in_hand()
}

/// Form `i`'s sketch's first corner.
fn corner(h: &Harness<BimApp>, i: usize) -> [f64; 2] {
    points(h, i)[0]
}

fn close(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 2.0 && (a[1] - b[1]).abs() < 2.0
}

#[test]
fn move_and_copy_work_in_3d_on_the_work_plane() {
    let mut h = in_3d();
    pick(&mut h, &[Picked::Form(0)]);
    let before = corner(&h, 0);
    // The button is live in {3D}, and Find takes the tool.
    assert_eq!(editor(&h).modify_blocked(&M::Move { copy: false, from: None }), None);
    command(&mut h, "Move");
    assert_eq!(tool(&h), Some(M::Move { copy: false, from: None }));
    // The prompt is the tool's, as in a plan, not {3D}'s own.
    let prompt = h.state().prompt().to_string();
    assert!(prompt.contains("Move") && !prompt.contains("Click a form"), "{prompt}");
    // Two clicks on Ref. Level, the work plane: the form goes by what lies between them.
    click3(&mut h, [0.0, 0.0, 0.0]);
    assert!(matches!(tool(&h), Some(M::Move { from: Some(_), .. })), "{:?}", tool(&h));
    click3(&mut h, [300.0, 200.0, 0.0]);
    let after = corner(&h, 0);
    assert!(close([after[0] - before[0], after[1] - before[1]], [300.0, 200.0]), "{before:?} to {after:?}: {}", status(&h));
    assert_eq!(tool(&h), None, "the move made, the tool is put down");
    assert_eq!(editor(&h).steps(), 1, "one step");
    // Copy: the original stays, a copy of it stands that far off.
    let forms = family(&h).forms.len();
    command(&mut h, "Copy");
    click3(&mut h, [0.0, 0.0, 0.0]);
    click3(&mut h, [0.0, 1000.0, 0.0]);
    assert_eq!(family(&h).forms.len(), forms + 1, "{}", status(&h));
    assert!(close(corner(&h, 0), after), "the original where it was");
}

#[test]
fn rotate_and_mirror_draw_axis_work_in_3d_about_the_work_planes_normal() {
    let mut h = in_3d();
    pick(&mut h, &[Picked::Form(0)]);
    // Mirror - Draw Axis: the axis drawn along y through the origin, the box from 100 to 500 goes to -500 to -100.
    command(&mut h, "Mirror - Draw Axis");
    assert_eq!(tool(&h), Some(M::Mirror { draw: true, start: None }));
    editor_mut(&mut h).views.modify_options.mirror_copy = false;
    click3(&mut h, [0.0, -300.0, 0.0]);
    assert!(matches!(tool(&h), Some(M::Mirror { start: Some(_), .. })), "{:?}", tool(&h));
    click3(&mut h, [0.0, 600.0, 0.0]);
    assert_eq!(tool(&h), None, "{}", status(&h));
    let xs: Vec<f64> = points(&h, 0).iter().map(|p| p[0]).collect();
    let (lo, hi) = (xs.iter().copied().fold(f64::INFINITY, f64::min), xs.iter().copied().fold(f64::NEG_INFINITY, f64::max));
    assert!((lo + 500.0).abs() < 2.0 && (hi + 100.0).abs() < 2.0, "{lo} to {hi}: {}", status(&h));
    // Rotate: its centre is the middle of what it takes; from east of it to north of it turns a quarter, counter-clockwise
    // seen from over the plane.
    let before = points(&h, 0);
    let centre = [-300.0, 300.0];
    command(&mut h, "Rotate");
    for _ in 0..2 {
        h.run();
    }
    editor_mut(&mut h).views.modify_options.rotate_copy = false;
    click3(&mut h, [centre[0] + 800.0, centre[1], 0.0]);
    click3(&mut h, [centre[0], centre[1] + 800.0, 0.0]);
    let after = points(&h, 0);
    for (a, b) in before.iter().zip(&after) {
        // A quarter turn about the centre: (x, y) to (-y, x) from it.
        let want = [centre[0] - (a[1] - centre[1]), centre[1] + (a[0] - centre[0])];
        assert!(close(*b, want), "{a:?} to {b:?}, not {want:?}: {}", status(&h));
    }
}

fn editor_mut<'a>(h: &'a mut Harness<'_, BimApp>) -> &'a mut crate::editor::FamilyEditor {
    ed(h)
}

#[test]
fn an_upright_work_plane_moves_up_and_down() {
    let mut h = in_3d();
    // Work Plane > Set, for {3D}: Center (Front/Back), upright through the origin.
    let plane = bimcraft_core::family::PlaneRef::Plane(super::family_drag_move::g(FRONT_BACK));
    ed(&mut h).views.set_work_plane(plane);
    pick(&mut h, &[Picked::Form(0)]);
    let (before, ends) = (corner(&h, 0), serde_json::to_value(&family(&h).forms[0]).unwrap());
    command(&mut h, "Move");
    click3(&mut h, [0.0, 0.0, 0.0]);
    click3(&mut h, [0.0, 0.0, 400.0]);
    assert_eq!(tool(&h), None, "{}", status(&h));
    let now = serde_json::to_value(&family(&h).forms[0]).unwrap();
    assert!(close(corner(&h, 0), before), "its sketch where it was in plan");
    let number = |v: &Value| v.as_f64().unwrap_or(f64::NAN);
    assert!((number(&now["from"]) - number(&ends["from"]) - 400.0).abs() < 2.0, "{} to {}", ends["from"], now["from"]);
    assert!((number(&now["to"]) - number(&ends["to"]) - 400.0).abs() < 2.0, "{} to {}", ends["to"], now["to"]);
}

#[test]
fn what_picks_from_the_view_stays_greyed_in_3d_and_says_why() {
    let mut h = in_3d();
    // Nothing selected: Move asks for a selection, as in a plan.
    let why = editor(&h).modify_blocked(&M::Move { copy: false, from: None }).expect("nothing selected");
    assert!(why.contains("Select"), "{why}");
    pick(&mut h, &[Picked::Form(0)]);
    for t in [M::Mirror { draw: false, start: None }, M::Offset] {
        let why = editor(&h).modify_blocked(&t).expect("greyed in {3D}");
        assert!(why.contains("{3D}") && why.contains("plan or an elevation"), "{t:?}: {why}");
    }
    assert!(!editor(&h).editing_2d(), "Align is a plan's and an elevation's still");
    // In a plan they all work, as before.
    view(&mut h, Some(Which::Plan));
    pick(&mut h, &[Picked::Form(0)]);
    assert_eq!(editor(&h).modify_blocked(&M::Mirror { draw: false, start: None }), None);
}

#[test]
fn a_tool_in_hand_is_this_views_and_esc_lets_go_of_it() {
    // Move in a plan with its first point clicked: in {3D} it starts again, its point let go (the plan's isn't the plane's).
    let (mut h, _) = open(&family_with(""), "Movers");
    view(&mut h, Some(Which::Plan));
    pick(&mut h, &[Picked::Form(0)]);
    command(&mut h, "Move");
    let at = editor(&h).views.screen_of([0.0, 0.0, 0.0]).unwrap();
    click_at(&mut h, at);
    assert!(matches!(tool(&h), Some(M::Move { from: Some(_), .. })));
    view(&mut h, None);
    for _ in 0..4 {
        h.run();
    }
    assert_eq!(tool(&h), Some(M::Move { copy: false, from: None }), "in hand still, from its first step");
    // Esc: the point clicked, then the tool.
    click3(&mut h, [0.0, 0.0, 0.0]);
    assert!(matches!(tool(&h), Some(M::Move { from: Some(_), .. })));
    key(&mut h, Key::Escape);
    assert_eq!(tool(&h), Some(M::Move { copy: false, from: None }));
    key(&mut h, Key::Escape);
    assert_eq!(tool(&h), None);
    // A tool {3D} can't work with, taken in a plan: put down as {3D} opens, saying why; the selection is kept.
    view(&mut h, Some(Which::Plan));
    pick(&mut h, &[Picked::Form(0)]);
    command(&mut h, "Mirror - Pick Axis");
    assert_eq!(tool(&h), Some(M::Mirror { draw: false, start: None }));
    view(&mut h, None);
    for _ in 0..4 {
        h.run();
    }
    assert_eq!(tool(&h), None);
    assert!(status(&h).contains("{3D}"), "{}", status(&h));
    assert_eq!(editor(&h).selection.items(), [Picked::Form(0)]);
}

#[test]
fn a_tools_click_in_3d_is_no_selecting_click() {
    let mut h = in_3d();
    pick(&mut h, &[Picked::Form(0)]);
    command(&mut h, "Move");
    // The click lands on box B, which a selecting click would take: the tool's doesn't.
    click3(&mut h, [-800.0, 300.0, 300.0]);
    assert_eq!(editor(&h).selection.items(), [Picked::Form(0)], "the selection is the tool's still");
    assert!(matches!(tool(&h), Some(M::Move { from: Some(_), .. })));
}

#[test]
fn a_form_hidden_in_3d_is_not_taken_by_a_tool_there() {
    let mut h = in_3d();
    // Box B hidden for now (Temporary Hide): selected still, with A, it stays where it is when the two are moved.
    pick(&mut h, &[Picked::Form(1)]);
    command(&mut h, "Temporary Hide Element");
    for _ in 0..4 {
        h.run();
    }
    pick(&mut h, &[Picked::Form(0), Picked::Form(1)]);
    let (a, b) = (corner(&h, 0), corner(&h, 1));
    command(&mut h, "Move");
    click3(&mut h, [0.0, 0.0, 0.0]);
    click3(&mut h, [300.0, 0.0, 0.0]);
    assert!(close([corner(&h, 0)[0] - a[0], corner(&h, 0)[1] - a[1]], [300.0, 0.0]), "A moved: {}", status(&h));
    assert!(close(corner(&h, 1), b), "B, hidden for now, stayed");
}

/// Move and Rotate mid-way in {3D}, the work plane shown under them, the rubber band and the box where the click would put
/// the form, in the three looks: saved where `BIMCRAFT_MODIFY_3D_DUMP` says (only then).
#[test]
fn modify_in_3d_screenshots() {
    let Ok(dir) = std::env::var("BIMCRAFT_MODIFY_3D_DUMP") else { return };
    std::fs::create_dir_all(&dir).unwrap();
    for (look, dark, light_canvas) in [("light", false, false), ("dark", true, false), ("dark-ui-light-canvas", true, true)] {
        for (name, tool, first, then) in
            [("move", "Move", [0.0, 0.0, 0.0], [-400.0, 500.0, 0.0]), ("rotate", "Rotate", [800.0, 300.0, 0.0], [600.0, 600.0, 0.0])]
        {
            // The ribbon's harness, whose renderer draws: the family's own keeps pictures and draws none.
            let mut h = app(&family_with(""));
            h.set_size(vec2(1400.0, 900.0));
            h.state_mut().edit_family("Movers");
            h.ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
            h.state_mut().light_canvas = light_canvas;
            for _ in 0..4 {
                h.run();
            }
            view(&mut h, None);
            for _ in 0..4 {
                h.run();
            }
            pick(&mut h, &[Picked::Form(0)]);
            command(&mut h, tool);
            for _ in 0..2 {
                h.run();
            }
            click3(&mut h, first);
            let hover = h.state().family_screen3(then).expect("{3D} drawn");
            h.hover_at(hover);
            for _ in 0..3 {
                h.run();
            }
            h.render().unwrap().save(std::path::Path::new(&dir).join(format!("modify-3d-{name}-{look}.png"))).unwrap();
        }
    }
}

/// The Movers family in {3D} in the ribbon's harness, whose renderer draws (the family's own keeps pictures and draws none).
fn drawn_3d() -> Harness<'static, BimApp> {
    let mut h = app(&family_with(""));
    h.set_size(vec2(1400.0, 900.0));
    h.state_mut().edit_family("Movers");
    for _ in 0..4 {
        h.run();
    }
    view(&mut h, None);
    for _ in 0..4 {
        h.run();
    }
    h
}

/// The colours drawn round family point `p` in {3D}: five pixels square, since a line is a pixel or two wide.
fn round(h: &mut Harness<BimApp>, p: [f64; 3]) -> Vec<[u8; 4]> {
    for _ in 0..3 {
        h.run();
    }
    let at = h.state().family_screen3(p).expect("{3D} drawn");
    let image = h.render().unwrap();
    let (x, y) = (at.x.round() as u32, at.y.round() as u32);
    (0..25).map(|k| image.get_pixel(x + k % 5 - 2, y + k / 5 - 2).0).collect()
}

#[test]
fn the_work_plane_shows_while_a_tool_is_in_hand_in_3d() {
    let mut h = drawn_3d();
    pick(&mut h, &[Picked::Form(0)]);
    // Bare ground on Ref. Level, clear of both boxes and of the grid lines through the origin.
    let ground = [-200.0, -200.0, 0.0];
    let bare = round(&mut h, ground);
    command(&mut h, "Move");
    let tinted = round(&mut h, ground);
    assert_ne!(tinted, bare, "the work plane is laid over the ground");
    // Put down, the plane goes: Work Plane > Show is off.
    key(&mut h, Key::Escape);
    assert_eq!(tool(&h), None);
    assert_eq!(round(&mut h, ground), bare);
}

#[test]
fn the_box_of_what_a_move_takes_is_drawn_where_the_click_would_put_it() {
    let mut h = drawn_3d();
    pick(&mut h, &[Picked::Form(0)]);
    command(&mut h, "Move");
    click3(&mut h, [0.0, 0.0, 0.0]);
    // Box A's top front edge runs from (100, 100, 300) to (500, 100, 300): moved by (-400, 500), its middle is here.
    let edge = [-100.0, 600.0, 300.0];
    let hover = |h: &mut Harness<BimApp>, p: [f64; 3]| {
        let at = h.state().family_screen3(p).expect("{3D} drawn");
        h.hover_at(at);
    };
    // The pointer elsewhere on the plane: the box is drawn elsewhere, and nothing here.
    hover(&mut h, [200.0, -200.0, 0.0]);
    let empty = round(&mut h, edge);
    hover(&mut h, [-400.0, 500.0, 0.0]);
    let boxed = round(&mut h, edge);
    assert_ne!(boxed, empty, "the box's edge where the move would put it");
}

#[test]
fn a_shape_handle_gives_way_to_a_modify_tool_in_hand() {
    let mut h = in_3d();
    pick(&mut h, &[Picked::Form(0)]);
    for _ in 0..3 {
        h.run();
    }
    // Selected, the form has its handles in {3D}.
    let handles = editor(&h).views.handles3d_shown.clone();
    assert!(!handles.is_empty(), "a selected form's handles show");
    let (at, _) = handles[0];
    // A tool in hand: none is drawn, and none can be grabbed.
    command(&mut h, "Move");
    for _ in 0..2 {
        h.run();
    }
    assert!(editor(&h).views.handles3d_shown.is_empty(), "no handle with a tool in hand");
    let camera = |h: &Harness<BimApp>| editor(h).orbit.camera.expect("{3D} drawn");
    let (before, was) = (serde_json::to_string(&family(&h)).unwrap(), camera(&h));
    // A press and a drag where a handle stood: the view turns, as a drag does with a tool in hand, and nothing changes.
    let to = at + vec2(80.0, 30.0);
    h.hover_at(at);
    h.run();
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run();
    for k in 1..=4 {
        h.event(Event::PointerMoved(at + (to - at) * (k as f32 / 4.0)));
        h.run();
    }
    h.event(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run();
    assert_eq!(serde_json::to_string(&family(&h)).unwrap(), before, "the family is as it was");
    assert_ne!(camera(&h), was, "the drag turned the view");
    assert_eq!(tool(&h), Some(M::Move { copy: false, from: None }), "and was no click of the tool's");
    // Put down, the handles are back.
    key(&mut h, Key::Escape);
    for _ in 0..3 {
        h.run();
    }
    assert!(!editor(&h).views.handles3d_shown.is_empty());
}

#[test]
fn a_plans_shape_handles_give_way_to_a_modify_tool_too() {
    let (mut h, _) = open(&family_with(""), "Movers");
    view(&mut h, Some(Which::Plan));
    pick(&mut h, &[Picked::Form(0)]);
    assert!(editor(&h).handles_shown());
    command(&mut h, "Move");
    assert!(!editor(&h).handles_shown(), "a tool in hand: its clicks are its own");
    key(&mut h, Key::Escape);
    assert!(editor(&h).handles_shown());
}

#[test]
fn what_a_tool_takes_in_3d_is_worked_out_once_as_the_pointer_moves() {
    let mut h = in_3d();
    pick(&mut h, &[Picked::Form(0)]);
    command(&mut h, "Move");
    click3(&mut h, [0.0, 0.0, 0.0]);
    let built = editor(&h).views.cache_builds;
    assert!(built >= 1);
    for p in [[-400.0, 500.0, 0.0], [200.0, 300.0, 0.0], [600.0, -200.0, 0.0]] {
        let at = h.state().family_screen3(p).expect("{3D} drawn");
        h.hover_at(at);
        h.run();
    }
    assert_eq!(editor(&h).views.cache_builds, built, "the pointer only moves the boxes");
    // Another selection: worked out again.
    pick(&mut h, &[Picked::Form(1)]);
    h.run();
    assert!(editor(&h).views.cache_builds > built);
}
