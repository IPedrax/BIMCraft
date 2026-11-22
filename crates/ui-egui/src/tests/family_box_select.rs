//! FE-1: box selection in a family's plans and elevations. A drag from nothing, with no tool
//! in hand, draws a box: left to right a window, taking what lies wholly inside it; right to left a crossing, taking what it
//! touches too. Ctrl adds, Shift takes away, else it is the selection; so does a click with Ctrl or Shift. Several selected,
//! Properties say how many, Delete takes them all away in one step (Undo puts them all back), Escape and Modify let them go.
//! The selection is one set: what goes from the draft (undone, redone, changed while another tab was in front) goes from it.
//! Nothing of it reaches the project.
use super::family_selection::command;
use super::family_views::{click_family, editor, open, view};
use super::*;
use crate::editor::select_box::Picked;
use bimcraft_core::family::views::{Side, Which};
use bimcraft_core::ids::Guid;

/// Two boxes on Ref. Level, west (A) and east (B) of the Center (Left/Right) plane, both behind Center (Front/Back); a plane of
/// its own, Edge, further west; a reference line north of them; a symbolic line in front; and a dimension in plan from Edge to
/// Center (Left/Right), in front of everything.
pub(super) const BOXES: &str = concat!(
    r#"family {"name": "Boxes", "category": "generic", "types": {"T": {}}, "#,
    r#""forms": [{"profile": [[-900, 200], [-500, 200], [-500, 600], [-900, 600]], "to": 600}, "#,
    r#"{"profile": [[500, 200], [900, 200], [900, 600], [500, 600]], "to": 600}], "#,
    r#""ref_planes": [{"guid": "fe100000-0000-4000-8000-000000000001", "name": "Edge", "start": [-1300, -1000, 0], "end": [-1300, 1000, 0], "up": [0, 0, 1]}], "#,
    r#""ref_lines": [{"guid": "fe100000-0000-4000-8000-000000000002", "start": [-200, 900, 0], "end": [200, 900, 0]}], "#,
    r#""symbols": [{"line": [[-100, -700], [100, -700]]}], "#,
    r#""dimensions": [{"guid": "fe100000-0000-4000-8000-000000000003", "refs": [{"plane": "fe100000-0000-4000-8000-000000000001"}, "#,
    r#"{"plane": "9e3c0a10-1f0b-4d8c-8a5e-0000000000a3"}], "view": "plan", "at": [-650, -1200, 0]}]}"#
);

pub(super) const EDGE: &str = "fe100000-0000-4000-8000-000000000001";
pub(super) const LINE: &str = "fe100000-0000-4000-8000-000000000002";
pub(super) const DIM: &str = "fe100000-0000-4000-8000-000000000003";

pub(super) fn g(s: &str) -> Guid {
    Guid::parse(s).expect("a GUID")
}

/// The family's document open on its floor plan.
pub(super) fn plan() -> Harness<'static, BimApp> {
    let (mut h, _) = open(BOXES, "Boxes");
    view(&mut h, Some(Which::Plan));
    h
}

/// What is selected, in order.
pub(super) fn selected(h: &Harness<BimApp>) -> Vec<Picked> {
    let mut v = editor(h).selection.items().to_vec();
    v.sort();
    v
}

pub(super) fn set(mut v: Vec<Picked>) -> Vec<Picked> {
    v.sort();
    v
}

/// A primary drag on screen from family point `a` to `b` of the view shown, in steps as a hand makes it, `mods` held.
fn drag_family(h: &mut Harness<BimApp>, a: [f64; 3], b: [f64; 3], mods: Modifiers, button: PointerButton) {
    let (a, b) = (editor(h).views.screen_of(a).expect("a view shown"), editor(h).views.screen_of(b).expect("a view shown"));
    h.event(Event::ModifiersChanged(mods));
    h.hover_at(a);
    h.run();
    h.event(Event::PointerButton { pos: a, button, pressed: true, modifiers: mods });
    h.run();
    for t in [0.25, 0.5, 0.75, 1.0] {
        h.hover_at(a + (b - a) * t);
        h.run();
    }
    h.event(Event::PointerButton { pos: b, button, pressed: false, modifiers: mods });
    h.run();
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run();
}

pub(super) fn boxed(h: &mut Harness<BimApp>, a: [f64; 3], b: [f64; 3], mods: Modifiers) {
    drag_family(h, a, b, mods, PointerButton::Primary);
}

/// A click at family point `p` of the view shown, `mods` held.
pub(super) fn click_with_mods(h: &mut Harness<BimApp>, p: [f64; 3], mods: Modifiers) {
    let pos = editor(h).views.screen_of(p).expect("a view shown");
    h.hover_at(pos);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: mods });
    }
    h.run();
}

#[test]
fn a_box_left_to_right_takes_what_lies_wholly_inside_it_and_right_to_left_what_it_touches() {
    let mut h = plan();
    let project = h.state().session.model.to_json().unwrap();
    // Left to right round B: B alone, the planes running past it left out.
    boxed(&mut h, [400.0, 700.0, 0.0], [1000.0, 100.0, 0.0], Modifiers::NONE);
    assert_eq!(selected(&h), vec![Picked::Form(1)], "a window takes what is wholly inside it");
    assert_eq!(editor(&h).selected_form(), Some(1), "one alone is the form selected, as a click selects it");
    // Right to left over A's east side and across Edge: both, and nothing else; what was selected let go.
    boxed(&mut h, [-400.0, 500.0, 0.0], [-1400.0, 300.0, 0.0], Modifiers::NONE);
    assert_eq!(selected(&h), set(vec![Picked::Form(0), Picked::Plane(g(EDGE))]), "a crossing takes what it touches too");
    assert_eq!(editor(&h).selected_form(), None, "of several, none is the form selected");
    // The same box left to right holds nothing whole: nothing.
    boxed(&mut h, [-1400.0, 500.0, 0.0], [-400.0, 300.0, 0.0], Modifiers::NONE);
    assert!(selected(&h).is_empty());
    // The symbolic line, wholly inside, though Center (Left/Right) runs through the box; the reference line; the dimension.
    boxed(&mut h, [-200.0, -600.0, 0.0], [200.0, -800.0, 0.0], Modifiers::NONE);
    assert_eq!(selected(&h), vec![Picked::Symbol(0)]);
    boxed(&mut h, [-300.0, 1000.0, 0.0], [300.0, 800.0, 0.0], Modifiers::NONE);
    assert_eq!(selected(&h), vec![Picked::Line(g(LINE))]);
    boxed(&mut h, [-1450.0, -1100.0, 0.0], [150.0, -1300.0, 0.0], Modifiers::NONE);
    assert_eq!(selected(&h), vec![Picked::Dimension(g(DIM))]);
    assert_eq!(h.state().session.model.to_json().unwrap(), project, "nothing of it reaches the project");
}

#[test]
fn ctrl_adds_to_the_selection_shift_takes_away_and_a_box_or_a_click_alone_replaces_it() {
    let mut h = plan();
    boxed(&mut h, [400.0, 700.0, 0.0], [1000.0, 100.0, 0.0], Modifiers::NONE);
    boxed(&mut h, [-200.0, -600.0, 0.0], [200.0, -800.0, 0.0], Modifiers::COMMAND);
    assert_eq!(selected(&h), vec![Picked::Form(1), Picked::Symbol(0)], "Ctrl adds what the box takes");
    boxed(&mut h, [1000.0, 500.0, 0.0], [700.0, 300.0, 0.0], Modifiers::SHIFT);
    assert_eq!(selected(&h), vec![Picked::Symbol(0)], "Shift takes it away");
    // A click on A with Ctrl adds it; with Shift takes it away; alone, it is all that is selected.
    click_with_mods(&mut h, [-700.0, 400.0, 0.0], Modifiers::COMMAND);
    assert_eq!(selected(&h), vec![Picked::Form(0), Picked::Symbol(0)]);
    click_with_mods(&mut h, [-700.0, 400.0, 0.0], Modifiers::SHIFT);
    assert_eq!(selected(&h), vec![Picked::Symbol(0)]);
    click_with_mods(&mut h, [-1300.0, -300.0, 0.0], Modifiers::COMMAND);
    assert_eq!(selected(&h), set(vec![Picked::Plane(g(EDGE)), Picked::Symbol(0)]), "a plane by Ctrl and a click too");
    // On nothing, Ctrl keeps the selection; a click alone lets it go.
    click_with_mods(&mut h, [1200.0, -1000.0, 0.0], Modifiers::COMMAND);
    assert_eq!(selected(&h).len(), 2);
    click_family(&mut h, [-700.0, 400.0, 0.0]);
    assert_eq!(selected(&h), vec![Picked::Form(0)]);
    boxed(&mut h, [400.0, 700.0, 0.0], [1000.0, 100.0, 0.0], Modifiers::NONE);
    assert_eq!(selected(&h), vec![Picked::Form(1)], "a box alone replaces it");
}

/// Several selected: Properties say how many, the contextual tab is Modify | Multi-Select, each is drawn selected; Delete takes
/// them all away as one step, which Undo puts back whole; Escape and Modify let them go.
#[test]
fn several_selected_show_their_count_and_go_with_one_delete_that_one_undo_brings_back() {
    let mut h = plan();
    let project = h.state().session.model.to_json().unwrap();
    let pick_four = |h: &mut Harness<BimApp>| {
        boxed(h, [400.0, 700.0, 0.0], [1000.0, 100.0, 0.0], Modifiers::NONE);
        boxed(h, [-400.0, 500.0, 0.0], [-1400.0, 300.0, 0.0], Modifiers::COMMAND);
        boxed(h, [-200.0, -600.0, 0.0], [200.0, -800.0, 0.0], Modifiers::COMMAND);
    };
    pick_four(&mut h);
    assert_eq!(selected(&h), set(vec![Picked::Form(0), Picked::Form(1), Picked::Plane(g(EDGE)), Picked::Symbol(0)]));
    assert!(painted_text(&h).contains(&"Multi-Select (4)".to_string()), "Properties say how many");
    assert!(h.query_by_label("Extrusion End").is_none() && h.query_by_label("Family name").is_none(), "and none of their rows");
    assert!(h.query_all_by_label("Modify | Multi-Select").count() > 0, "the contextual tab");
    // Each drawn in the selection's colour, as one alone is: both boxes' outlines.
    let lit = |h: &Harness<BimApp>, p: [f64; 3]| {
        let at = editor(h).views.screen_of(p).unwrap();
        painted_lines(h).iter().any(|(a, b, c)| *c == crate::editor::views::PICKED && near_segment(at, *a, *b) < 2.0)
    };
    assert!(lit(&h, [-900.0, 400.0, 0.0]) && lit(&h, [900.0, 400.0, 0.0]), "both boxes' outlines lit");
    // Delete: all of it, in one go.
    key(&mut h, Key::Delete);
    let f = editor(&h).draft_family().unwrap();
    assert!(f.forms.is_empty() && f.symbols.is_empty(), "the forms and the symbol gone");
    assert!(!f.planes.iter().any(|p| p.guid == g(EDGE)), "Edge gone");
    assert!(f.dimensions.is_empty(), "and the dimension measuring to Edge with it");
    assert!(selected(&h).is_empty(), "nothing left selected");
    // One Undo puts it all back.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    let f = editor(&h).draft_family().unwrap();
    assert_eq!((f.forms.len(), f.symbols.len(), f.dimensions.len()), (2, 1, 1), "one step back, all of it");
    assert!(f.planes.iter().any(|p| p.guid == g(EDGE)));
    // Escape lets several go; so does Modify.
    pick_four(&mut h);
    assert_eq!(selected(&h).len(), 4);
    key(&mut h, Key::Escape);
    assert!(selected(&h).is_empty(), "Escape");
    pick_four(&mut h);
    command(&mut h, "Modify");
    assert!(selected(&h).is_empty(), "Modify");
    // Delete, as Find runs it, deletes them as the Delete key does, a datum among them staying.
    pick_four(&mut h);
    click_with_mods(&mut h, [0.0, -300.0, 0.0], Modifiers::COMMAND);
    assert!(selected(&h).contains(&Picked::Plane(bimcraft_core::family::CENTER_LEFT_RIGHT)));
    command(&mut h, "Delete");
    let f = editor(&h).draft_family().unwrap();
    assert!(f.forms.is_empty() && f.symbols.is_empty());
    assert!(f.planes.iter().any(|p| p.guid == bimcraft_core::family::CENTER_LEFT_RIGHT), "the datum stays");
    assert_eq!(selected(&h), vec![Picked::Plane(bimcraft_core::family::CENTER_LEFT_RIGHT)], "and stays selected");
    assert_eq!(h.state().session.model.to_json().unwrap(), project, "nothing of it reaches the project");
}

/// Delete, as Find runs it (`BimApp::family_find_action`), with several selected: all of them go as one step, as the Delete
/// key takes them, and one Ctrl+Z brings them all back.
#[test]
fn delete_with_several_selected_takes_them_all_away_and_one_undo_brings_them_all_back() {
    let mut h = plan();
    let project = h.state().session.model.to_json().unwrap();
    boxed(&mut h, [400.0, 700.0, 0.0], [1000.0, 100.0, 0.0], Modifiers::NONE);
    boxed(&mut h, [-400.0, 500.0, 0.0], [-1400.0, 300.0, 0.0], Modifiers::COMMAND);
    boxed(&mut h, [-200.0, -600.0, 0.0], [200.0, -800.0, 0.0], Modifiers::COMMAND);
    boxed(&mut h, [-300.0, 1000.0, 0.0], [300.0, 800.0, 0.0], Modifiers::COMMAND);
    assert_eq!(
        selected(&h),
        set(vec![Picked::Form(0), Picked::Form(1), Picked::Plane(g(EDGE)), Picked::Line(g(LINE)), Picked::Symbol(0)]),
        "both boxes, Edge, the reference line and the symbol"
    );
    command(&mut h, "Delete");
    let f = editor(&h).draft_family().unwrap();
    assert!(f.forms.is_empty() && f.symbols.is_empty() && f.lines.is_empty(), "every one of them gone");
    assert!(!f.planes.iter().any(|p| p.guid == g(EDGE)) && f.dimensions.is_empty(), "Edge, and the dimension to it");
    assert!(selected(&h).is_empty());
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    let f = editor(&h).draft_family().unwrap();
    assert_eq!((f.forms.len(), f.symbols.len(), f.lines.len(), f.dimensions.len()), (2, 1, 1, 1), "one Undo, all of it back");
    assert!(f.planes.iter().any(|p| p.guid == g(EDGE)), "Edge too");
    // And one Redo takes them all away again.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Y);
    h.run();
    let f = editor(&h).draft_family().unwrap();
    assert!(f.forms.is_empty() && f.symbols.is_empty() && f.lines.is_empty(), "one Redo, all of it gone again");
    assert_eq!(h.state().session.model.to_json().unwrap(), project, "nothing of it reaches the project");
}

/// A plane something not selected is drawn on can't go: nothing selected goes, and the status says why.
#[test]
fn delete_takes_nothing_away_when_one_of_several_cannot_go() {
    // A reference line drawn on Edge, upright, seen end on in plan.
    let on_edge = r#""ref_lines": [{"start": [-1300, -400, 100], "end": [-1300, -400, 500], "host": {"plane": "fe100000-0000-4000-8000-000000000001"}}, "#;
    let script = BOXES.replacen(r#""ref_lines": ["#, on_edge, 1);
    let (mut h, _) = open(&script, "Boxes");
    view(&mut h, Some(Which::Plan));
    assert_eq!(editor(&h).draft_family().unwrap().hosted_on(g(EDGE)).len(), 1, "the line on Edge");
    // B and Edge: B could go, Edge can't while the line is drawn on it.
    click_family(&mut h, [700.0, 400.0, 0.0]);
    click_with_mods(&mut h, [-1300.0, 300.0, 0.0], Modifiers::COMMAND);
    assert_eq!(selected(&h), set(vec![Picked::Form(1), Picked::Plane(g(EDGE))]));
    key(&mut h, Key::Delete);
    let f = editor(&h).draft_family().unwrap();
    assert_eq!(f.forms.len(), 2, "nothing went");
    assert!(f.planes.iter().any(|p| p.guid == g(EDGE)));
    assert!(h.state().status.error && h.state().status.text.contains("drawn on it"), "{}", h.state().status.text);
    assert_eq!(selected(&h).len(), 2, "both still selected");
}

#[test]
fn a_drag_from_an_element_with_a_tool_in_hand_or_by_the_middle_button_draws_no_box() {
    let mut h = plan();
    click_family(&mut h, [700.0, 400.0, 0.0]);
    assert_eq!(selected(&h), vec![Picked::Form(1)]);
    // From A's edge, leftwards over Edge: no box, while it is dragged or once it is let go; the drag is A's, which it selects
    // and moves.
    // (Since FE-20 the face of a form that isn't selected is the box's to begin in, so that a
    // crossing box can be drawn inside a face: Select Elements by Face, `family_selection`. A drag from an element's edge, or
    // from one selected, is still the element's.)
    let (a, b) = (editor(&h).views.screen_of([-500.0, 400.0, 0.0]).unwrap(), editor(&h).views.screen_of([-1400.0, 300.0, 0.0]).unwrap());
    h.hover_at(a);
    h.run();
    h.event(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run();
    for t in [0.25, 0.5, 0.75, 1.0] {
        h.hover_at(a + (b - a) * t);
        h.run();
        assert!(!editor(&h).boxing(), "a drag from a form's edge draws no box");
    }
    h.event(Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run();
    assert_eq!(selected(&h), vec![Picked::Form(0)], "a drag from a form's edge boxes nothing");
    // The middle button pans: no box either.
    drag_family(&mut h, [400.0, 700.0, 0.0], [1000.0, 100.0, 0.0], Modifiers::NONE, PointerButton::Middle);
    assert_eq!(selected(&h), vec![Picked::Form(0)], "the middle button boxes nothing");
    // With Reference Plane in hand, a drag round B draws no box.
    shortcut(&mut h, "RP");
    assert!(editor(&h).views.drawing.is_some());
    boxed(&mut h, [-1450.0, 1400.0, 0.0], [1450.0, -1400.0, 0.0], Modifiers::NONE);
    assert!(selected(&h).is_empty(), "a tool in hand takes the drag: {:?}", selected(&h));
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    // Aligned Dimension too.
    shortcut(&mut h, "DI");
    assert!(editor(&h).views.dimensioning.is_some());
    boxed(&mut h, [400.0, 700.0, 0.0], [1000.0, 100.0, 0.0], Modifiers::NONE);
    assert!(selected(&h).is_empty(), "Aligned Dimension takes the drag");
}

#[test]
fn an_elevation_boxes_as_a_plan_does() {
    let (mut h, _) = open(BOXES, "Boxes");
    view(&mut h, Some(Which::Elevation(Side::Front)));
    boxed(&mut h, [400.0, 0.0, 800.0], [1000.0, 0.0, -100.0], Modifiers::NONE);
    assert_eq!(selected(&h), vec![Picked::Form(1)], "a window round B");
    boxed(&mut h, [-400.0, 0.0, 400.0], [-1400.0, 0.0, 300.0], Modifiers::NONE);
    assert_eq!(selected(&h), set(vec![Picked::Form(0), Picked::Plane(g(EDGE))]), "a crossing over A and Edge");
    // A view opened keeps the selection.
    view(&mut h, Some(Which::Plan));
    assert_eq!(selected(&h), set(vec![Picked::Form(0), Picked::Plane(g(EDGE))]));
}

#[test]
fn what_undo_takes_away_goes_from_the_selection() {
    let mut h = plan();
    shortcut(&mut h, "RP");
    click_family(&mut h, [-1100.0, -1000.0, 0.0]);
    click_family(&mut h, [-1100.0, 1000.0, 0.0]);
    key(&mut h, Key::Escape);
    let drawn = *editor(&h).planes.last().map(|p| &p.guid).unwrap();
    assert_eq!(selected(&h), vec![Picked::Plane(drawn)], "selected once drawn");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert!(!editor(&h).planes.iter().any(|p| p.guid == drawn), "undone");
    assert!(selected(&h).is_empty(), "and gone from the selection: {:?}", selected(&h));
}

#[test]
fn what_redo_takes_away_goes_from_the_selection() {
    let mut h = plan();
    click_family(&mut h, [-1300.0, -300.0, 0.0]);
    assert_eq!(selected(&h), vec![Picked::Plane(g(EDGE))]);
    key(&mut h, Key::Delete);
    assert!(!editor(&h).planes.iter().any(|p| p.guid == g(EDGE)));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert!(editor(&h).planes.iter().any(|p| p.guid == g(EDGE)), "back");
    click_family(&mut h, [-1300.0, -300.0, 0.0]);
    assert_eq!(selected(&h), vec![Picked::Plane(g(EDGE))]);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Y);
    h.run();
    assert!(!editor(&h).planes.iter().any(|p| p.guid == g(EDGE)), "redone");
    assert!(selected(&h).is_empty(), "and gone from the selection: {:?}", selected(&h));
}

#[test]
fn what_went_while_another_tab_was_in_front_goes_from_the_selection() {
    let mut h = plan();
    click_family(&mut h, [700.0, 400.0, 0.0]);
    click_with_mods(&mut h, [-1300.0, -300.0, 0.0], Modifiers::COMMAND);
    assert_eq!(selected(&h), set(vec![Picked::Form(1), Picked::Plane(g(EDGE))]));
    press(&mut h, "Level 1");
    assert!(h.state().active_family().is_none(), "the project's tab");
    // Meanwhile Edge goes from the draft (as a family loaded back into it from a document over it can take its parts).
    let doc = h.state().families[0].id;
    h.state_mut().family_of_mut(doc).unwrap().planes.retain(|p| p.guid != g(EDGE));
    press(&mut h, "Boxes - Floor Plan: Ref. Level");
    assert!(h.state().active_family().is_some());
    assert_eq!(selected(&h), vec![Picked::Form(1)], "Edge gone from the selection, B kept");
}

/// Every line segment the last frame painted, with its colour.
fn painted_lines(h: &Harness<BimApp>) -> Vec<(Pos2, Pos2, egui::Color32)> {
    fn walk(s: &egui::Shape, out: &mut Vec<(Pos2, Pos2, egui::Color32)>) {
        match s {
            egui::Shape::LineSegment { points, stroke } => out.push((points[0], points[1], stroke.color)),
            egui::Shape::Path(p) => {
                let c = match &p.stroke.color {
                    egui::epaint::ColorMode::Solid(c) => *c,
                    _ => egui::Color32::TRANSPARENT,
                };
                out.extend(p.points.windows(2).map(|w| (w[0], w[1], c)));
                if p.closed
                    && let (Some(a), Some(b)) = (p.points.last(), p.points.first())
                {
                    out.push((*a, *b, c));
                }
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

/// How far `p` is from segment `a`-`b`, in points.
fn near_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

/// Looking at the result: with `BIMCRAFT_FAMILY_BOX_DUMP` naming a folder, the family's plan while a window and a crossing
/// box are dragged, and what each took once let go, light and dark.
#[test]
fn box_selection_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_FAMILY_BOX_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (name, dark) in [("light", false), ("dark", true)] {
        let mut h = app(BOXES);
        h.ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
        h.set_size(vec2(1600., 1000.));
        h.state_mut().edit_family("Boxes");
        h.run();
        view(&mut h, Some(Which::Plan));
        let save = |h: &mut Harness<BimApp>, what: &str| {
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("box-{what}-{name}.png"))).unwrap();
        };
        for (what, a, b) in
            [("window", [400.0, 700.0, 0.0], [1000.0, 100.0, 0.0]), ("crossing", [-400.0, 500.0, 0.0], [-1400.0, 300.0, 0.0])]
        {
            let (a, b) = (editor(&h).views.screen_of(a).unwrap(), editor(&h).views.screen_of(b).unwrap());
            h.hover_at(a);
            h.run();
            h.event(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
            h.run();
            for t in [0.25, 0.5, 0.75, 1.0] {
                h.hover_at(a + (b - a) * t);
                h.run();
            }
            save(&mut h, &format!("{what}-dragged"));
            h.event(Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
            h.run();
            h.hover_at(egui::pos2(1500.0, 950.0));
            h.run();
            save(&mut h, &format!("{what}-taken"));
        }
    }
}
