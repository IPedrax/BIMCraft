//! A drag on a selected element in a family's plan or elevation moves the whole selection: forms
//! (their sketch where the view sees their plane flat, their Start and End where it sees it edge-on), planes, symbols and a
//! dimension's line, live and as one step of the history. A press on an element not selected selects it first (with Ctrl,
//! adds it). What a lock or a label holds, and a number that is a formula, refuse the whole move, nothing changed; so does a
//! move another type can't build. Escape, Undo and another view mid-drag give it up; a drag back where it began leaves
//! nothing. Handles, grips and a drag from nothing keep doing what they did.
use super::family_views::{editor, open, view};
use super::*;
use crate::editor::select_box::{How, Picked};
use bimcraft_core::family::sketch::FormCurve;
use bimcraft_core::family::views::{Side, Which};
use bimcraft_core::family::{Expr, Family};
use bimcraft_core::ids::Guid;

pub(super) const A: &str = "fe700000-0000-4000-8000-000000000001";
pub(super) const EDGE: &str = "fe700000-0000-4000-8000-000000000002";
pub(super) const LINE: &str = "fe700000-0000-4000-8000-000000000003";
pub(super) const DIM: &str = "fe700000-0000-4000-8000-000000000004";
pub(super) const A_LINE: &str = "fe700000-0000-4000-8000-0000000000a";
pub(super) const CENTER_LR: &str = "9e3c0a10-1f0b-4d8c-8a5e-0000000000a3";

pub(super) fn g(s: &str) -> Guid {
    Guid::parse(s).expect("a GUID")
}

/// A square's sketch from (x0, y0) to (x1, y1), its lines' GUIDs `<prefix>0` to `3` (south, east, north, west) when given.
pub(super) fn square(x0: i32, y0: i32, x1: i32, y1: i32, prefix: Option<&str>) -> String {
    let pts = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
    let lines: Vec<String> = (0..4)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % 4]);
            let guid = prefix.map(|p| format!(r#""guid": "{p}{i}", "#)).unwrap_or_default();
            format!(r#"{{{guid}"start": [{}, {}], "end": [{}, {}]}}"#, a.0, a.1, b.0, b.1)
        })
        .collect();
    format!("[{}]", lines.join(", "))
}

/// Box A (100 to 500 square, 300 high) and box B west of it (-1000 to -600), a plane Edge at x 1000, a reference line north,
/// a symbolic line south and a dimension from Center (Left/Right) to Edge; `dims`, more dimensions (JSON after it).
pub(super) fn family_with(dims: &str) -> String {
    format!(
        r#"family {{"name": "Movers", "category": "generic", "parameters": [{{"name": "Len", "value": 100}}], "types": {{"T": {{}}}}, "ref_planes": [{{"guid": "{EDGE}", "name": "Edge", "start": [1000, -1500, 0], "end": [1000, 1500, 0], "up": [0, 0, 1]}}], "ref_lines": [{{"guid": "{LINE}", "start": [-200, 1200, 0], "end": [200, 1200, 0]}}], "symbols": [{{"line": [[-200, -800], [200, -800]]}}], "forms": [{{"guid": "{A}", "sketch": {}, "from": 0, "to": 300}}, {{"sketch": {}, "from": 0, "to": 300}}], "dimensions": [{{"guid": "{DIM}", "refs": [{{"plane": "{CENTER_LR}"}}, {{"plane": "{EDGE}"}}], "view": "plan", "at": [500, -1200, 0]}}{dims}]}}"#,
        square(100, 100, 500, 500, Some(A_LINE)),
        square(-1000, 100, -600, 500, None),
    )
}

pub(super) fn plan_of(script: &str) -> Harness<'static, BimApp> {
    let (mut h, _) = open(script, "Movers");
    view(&mut h, Some(Which::Plan));
    h
}

pub(super) fn plan() -> Harness<'static, BimApp> {
    plan_of(&family_with(""))
}

pub(super) fn ed<'a>(h: &'a mut Harness<'_, BimApp>) -> &'a mut crate::editor::FamilyEditor {
    h.state_mut().active_family_mut().expect("a family open")
}

pub(super) fn pick(h: &mut Harness<BimApp>, items: &[Picked]) {
    ed(h).selection.apply(items, How::Replace);
    h.run();
}

pub(super) fn family(h: &Harness<BimApp>) -> Family {
    editor(h).draft_family().unwrap()
}

/// Form `i`'s sketch's points as numbers (a formula infinite).
pub(super) fn points(h: &Harness<BimApp>, i: usize) -> Vec<[f64; 2]> {
    let v = |e: &Expr| match e {
        Expr::Number(x) => *x,
        _ => f64::INFINITY,
    };
    family(h).forms[i]
        .sketch
        .iter()
        .flat_map(|l| match &l.curve {
            FormCurve::Line { start, end, .. } => vec![[v(&start[0]), v(&start[1])], [v(&end[0]), v(&end[1])]],
            FormCurve::Circle { centre, .. } => vec![[v(&centre[0]), v(&centre[1])]],
        })
        .collect()
}

/// How far each of `after` is from `before`, which must be the same for all (a rigid move): that shift.
pub(super) fn shift(before: &[[f64; 2]], after: &[[f64; 2]]) -> [f64; 2] {
    let d = [after[0][0] - before[0][0], after[0][1] - before[0][1]];
    for (a, b) in before.iter().zip(after) {
        assert!((b[0] - a[0] - d[0]).abs() < 1e-6 && (b[1] - a[1] - d[1]).abs() < 1e-6, "not rigid: {before:?} {after:?}");
    }
    d
}

pub(super) fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 15.0
}

pub(super) fn status(h: &Harness<BimApp>) -> String {
    h.state().status.text.clone()
}

pub(super) fn untouched(h: &Harness<BimApp>) -> bool {
    editor(h).steps() == 0 && !editor(h).unsaved()
}

/// Screen points of family points `p` and `p + by`.
pub(super) fn ends(h: &Harness<BimApp>, p: [f64; 3], by: [f64; 3]) -> (egui::Pos2, egui::Pos2) {
    let v = &editor(h).views;
    (v.screen_of(p).expect("a view shown"), v.screen_of([p[0] + by[0], p[1] + by[1], p[2] + by[2]]).expect("a view shown"))
}

/// A press at family point `p`, `mods` held, moved by `by` (mm) in steps, the button still held: where the pointer is.
pub(super) fn press_and_move(h: &mut Harness<BimApp>, p: [f64; 3], by: [f64; 3], mods: Modifiers) -> egui::Pos2 {
    let (a, b) = ends(h, p, by);
    h.event(Event::ModifiersChanged(mods));
    h.hover_at(a);
    h.run();
    h.event(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: mods });
    h.run();
    for t in [0.25, 0.5, 0.75, 1.0] {
        h.hover_at(a + (b - a) * t);
        h.run();
    }
    b
}

pub(super) fn release(h: &mut Harness<BimApp>, at: egui::Pos2) {
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run();
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run();
}

/// A drag from family point `p` by `by` (mm), `mods` held.
pub(super) fn drag(h: &mut Harness<BimApp>, p: [f64; 3], by: [f64; 3], mods: Modifiers) {
    let at = press_and_move(h, p, by, mods);
    release(h, at);
}

#[test]
fn a_selected_form_dragged_in_plan_moves_as_one_step() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    let before = points(&h, 0);
    // The status bar says a drag moves it while the pointer is over it.
    let (over, _) = ends(&h, [300.0, 300.0, 0.0], [0.0; 3]);
    h.hover_at(over);
    h.run();
    h.run();
    assert!(h.state().prompt().contains("Drag to move"), "{}", h.state().prompt());
    drag(&mut h, [300.0, 300.0, 0.0], [300.0, 200.0, 0.0], Modifiers::NONE);
    let d = shift(&before, &points(&h, 0));
    assert!(near(d[0], 300.0) && near(d[1], 200.0), "{d:?} {}", status(&h));
    assert_eq!(editor(&h).steps(), 1);
    assert!(editor(&h).unsaved());
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(0)], "still selected");
    // Its height is as it was; Undo puts it back, Redo moves it again.
    assert_eq!(family(&h).forms[0].to, Expr::Number(300.0));
    h.state_mut().family_undo();
    h.run();
    assert_eq!(points(&h, 0), before);
    h.state_mut().family_redo();
    h.run();
    assert!(near(shift(&before, &points(&h, 0))[0], 300.0));
}

#[test]
fn shift_keeps_a_move_square_to_the_view() {
    // Shift: with it held the selection moves along the view's axis the drag mostly follows, from where it began.
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    let before = points(&h, 0);
    drag(&mut h, [300.0, 300.0, 0.0], [300.0, 100.0, 0.0], Modifiers::SHIFT);
    let d = shift(&before, &points(&h, 0));
    assert!(near(d[0], 300.0) && near(d[1], 0.0), "{d:?} {}", status(&h));
    assert_eq!(editor(&h).steps(), 1);
    // Mostly up: only up.
    let before = points(&h, 0);
    drag(&mut h, [600.0, 300.0, 0.0], [50.0, 250.0, 0.0], Modifiers::SHIFT);
    let d = shift(&before, &points(&h, 0));
    assert!(near(d[0], 0.0) && near(d[1], 250.0), "{d:?} {}", status(&h));
}

#[test]
fn shift_mid_drag_switches_the_axis_and_comes_and_goes_measured_from_where_the_drag_began() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    let before = points(&h, 0);
    let p = [300.0, 300.0, 0.0];
    // Every pointer place worked out as the drag begins: it reads the pointer through the view as it was then.
    let (_, mostly_y) = ends(&h, p, [100.0, 400.0, 0.0]);
    let (_, last) = ends(&h, p, [250.0, 50.0, 0.0]);
    let moved = |h: &Harness<BimApp>| shift(&before, &points(h, 0));
    let is = |d: [f64; 2], x: f64, y: f64| near(d[0], x) && near(d[1], y);
    let mods = |h: &mut Harness<BimApp>, m: Modifiers| {
        h.event(Event::ModifiersChanged(m));
        h.run();
    };
    // Shift held, mostly across: across only.
    press_and_move(&mut h, p, [300.0, 100.0, 0.0], Modifiers::SHIFT);
    assert!(is(moved(&h), 300.0, 0.0) && moved(&h)[1] == 0.0, "{:?}", moved(&h));
    // On to mostly up, Shift still held: the axis switches, up only.
    h.hover_at(mostly_y);
    h.run();
    assert!(is(moved(&h), 0.0, 400.0) && moved(&h)[0] == 0.0, "{:?}", moved(&h));
    // Shift let go: free, where the pointer is.
    mods(&mut h, Modifiers::NONE);
    assert!(is(moved(&h), 100.0, 400.0), "{:?}", moved(&h));
    // Pressed again: square again; let go and on: free.
    mods(&mut h, Modifiers::SHIFT);
    assert!(is(moved(&h), 0.0, 400.0) && moved(&h)[0] == 0.0, "{:?}", moved(&h));
    mods(&mut h, Modifiers::NONE);
    h.hover_at(last);
    h.run();
    assert!(is(moved(&h), 250.0, 50.0), "{:?}", moved(&h));
    release(&mut h, last);
    assert!(is(moved(&h), 250.0, 50.0), "{:?} {}", moved(&h), status(&h));
    assert_eq!(editor(&h).steps(), 1, "one step for the whole drag");
}

#[test]
fn a_form_a_plane_and_a_symbol_move_together() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0), Picked::Plane(g(EDGE)), Picked::Symbol(0)]);
    let before = points(&h, 0);
    let plane = family(&h).planes.iter().find(|p| p.guid == g(EDGE)).unwrap().clone();
    drag(&mut h, [300.0, 300.0, 0.0], [-200.0, 100.0, 0.0], Modifiers::NONE);
    let d = shift(&before, &points(&h, 0));
    assert!(near(d[0], -200.0) && near(d[1], 100.0), "{d:?} {}", status(&h));
    let f = family(&h);
    let moved = f.planes.iter().find(|p| p.guid == g(EDGE)).unwrap();
    for (k, d) in d.iter().enumerate() {
        assert!((moved.start[k] - plane.start[k] - d).abs() < 1e-6 && (moved.end[k] - plane.end[k] - d).abs() < 1e-6, "{moved:?}");
    }
    let n = |e: &Expr| match e {
        Expr::Number(x) => *x,
        _ => f64::NAN,
    };
    let s = &f.symbols[0].line;
    assert!((n(&s[0][0]) + 200.0 - d[0]).abs() < 1e-6 && (n(&s[0][1]) + 800.0 - d[1]).abs() < 1e-6, "{s:?}");
    // B, the line and the dimension (an annotation, not selected) stay.
    assert_eq!(f.forms[1].sketch[0].curve, family_of_script().forms[1].sketch[0].curve);
    assert_eq!(f.dimensions[0].at, [500.0, -1200.0, 0.0]);
    assert_eq!(editor(&h).steps(), 1);
}

fn family_of_script() -> Family {
    let h = plan();
    family(&h)
}

#[test]
fn an_extrusion_seen_edge_on_moves_its_start_and_end() {
    let mut h = plan();
    view(&mut h, Some(Which::Elevation(Side::Front)));
    pick(&mut h, &[Picked::Form(0)]);
    let before = points(&h, 0);
    // Up 200 on the screen: its Start and End go up together, its sketch stays.
    let (a, b) = ends(&h, [300.0, 0.0, 150.0], [0.0, 0.0, 200.0]);
    let b = egui::pos2(a.x, b.y);
    h.hover_at(a);
    h.run();
    h.event(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run();
    for t in [0.25, 0.5, 0.75, 1.0] {
        h.hover_at(a + (b - a) * t);
        h.run();
    }
    release(&mut h, b);
    let f = family(&h);
    let num = |e: &Expr| match e {
        Expr::Number(x) => *x,
        _ => f64::NAN,
    };
    let (from, to) = (num(&f.forms[0].from), num(&f.forms[0].to));
    assert!(near(from, 200.0) && (to - from - 300.0).abs() < 1e-6, "{from} {to} {}", status(&h));
    assert_eq!(points(&h, 0), before, "the sketch stays");
    // Across, right 100: the sketch moves along x, the ends stay.
    drag(&mut h, [300.0, 0.0, 300.0], [100.0, 0.0, 0.0], Modifiers::NONE);
    let d = shift(&before, &points(&h, 0));
    assert!(near(d[0], 100.0) && d[1] == 0.0, "{d:?} {}", status(&h));
    assert_eq!(num(&family(&h).forms[0].from), from);
}

#[test]
fn a_press_on_a_form_not_selected_selects_it_and_moves_it_and_ctrl_adds_it() {
    let mut h = plan();
    let (a0, b0) = (points(&h, 0), points(&h, 1));
    // Each pressed on an edge: the face of a form not selected is the box's to begin in.
    drag(&mut h, [-600.0, 300.0, 0.0], [0.0, 300.0, 0.0], Modifiers::NONE);
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(1)]);
    let d = shift(&b0, &points(&h, 1));
    assert!(near(d[1], 300.0) && near(d[0], 0.0), "{d:?} {}", status(&h));
    assert_eq!(points(&h, 0), a0);
    // Ctrl on A: added, and both move.
    let b1 = points(&h, 1);
    drag(&mut h, [500.0, 300.0, 0.0], [100.0, 0.0, 0.0], Modifiers::CTRL);
    let mut sel = editor(&h).selection.items().to_vec();
    sel.sort();
    assert_eq!(sel, [Picked::Form(0), Picked::Form(1)]);
    let (da, db) = (shift(&a0, &points(&h, 0)), shift(&b1, &points(&h, 1)));
    assert!(near(da[0], 100.0) && da.iter().zip(&db).all(|(a, b)| (a - b).abs() < 1e-6), "{da:?} {db:?} {}", status(&h));
}

#[test]
fn a_lock_or_a_label_refuses_the_whole_move_and_what_is_locked_to_a_moved_plane_follows() {
    // A's west line (x 100) labelled Len from Center (Left/Right).
    let label = format!(
        r#", {{"refs": [{{"plane": "{CENTER_LR}"}}, {{"form": "{A}", "line": "{A_LINE}3"}}], "at": [50, 700, 0], "label": "Len"}}"#
    );
    let mut h = plan_of(&family_with(&label));
    pick(&mut h, &[Picked::Form(0), Picked::Symbol(0)]);
    let (before, symbol) = (points(&h, 0), family(&h).symbols.clone());
    drag(&mut h, [300.0, 300.0, 0.0], [200.0, 0.0, 0.0], Modifiers::NONE);
    assert!(status(&h).contains("dimension 2"), "{}", status(&h));
    assert_eq!(points(&h, 0), before);
    assert_eq!(family(&h).symbols, symbol, "the symbol stays too");
    assert!(untouched(&h), "no step, nothing unsaved");
    // Along the label's line it is free: north 200 moves it.
    drag(&mut h, [300.0, 300.0, 0.0], [0.0, 200.0, 0.0], Modifiers::NONE);
    assert!(near(shift(&before, &points(&h, 0))[1], 200.0), "{}", status(&h));
    // A's east line (x 500) Align-locked to Edge: Edge and the symbol moved west 300, A's east side follows Edge.
    let lock = format!(
        r#", {{"refs": [{{"plane": "{EDGE}"}}, {{"form": "{A}", "line": "{A_LINE}1"}}], "at": [700, 300, 0], "locked": [{{"segment": 0, "length": 0}}], "alignment": true}}"#
    );
    // Edge drawn on A's east side, at 500.
    let script = family_with(&lock).replace("[1000, -1500, 0], \"end\": [1000, 1500, 0]", "[500, -1500, 0], \"end\": [500, 1500, 0]");
    let mut h = plan_of(&script);
    let east = |h: &Harness<BimApp>| points(h, 0)[2][0];
    let edge = |h: &Harness<BimApp>| family(h).planes.iter().find(|p| p.guid == g(EDGE)).unwrap().start[0];
    let was = (east(&h), edge(&h));
    pick(&mut h, &[Picked::Plane(g(EDGE)), Picked::Symbol(0)]);
    assert_eq!(was, (500.0, 500.0));
    drag(&mut h, [was.1, -400.0, 0.0], [-100.0, 0.0, 0.0], Modifiers::NONE);
    assert!(near(edge(&h), was.1 - 100.0), "{} {}", edge(&h), status(&h));
    assert!((east(&h) - edge(&h)).abs() < 1e-6, "A's east side follows Edge: {} {}", east(&h), edge(&h));
    assert_eq!(points(&h, 0)[0][0], 100.0, "its west side stays");
}

#[test]
fn a_datum_and_a_formula_refuse_the_move() {
    let mut h = plan();
    let before = points(&h, 0);
    pick(&mut h, &[Picked::Form(0), Picked::Plane(g(CENTER_LR))]);
    drag(&mut h, [300.0, 300.0, 0.0], [200.0, 0.0, 0.0], Modifiers::NONE);
    assert!(status(&h).contains("datum"), "{}", status(&h));
    assert_eq!(points(&h, 0), before);
    assert!(untouched(&h));
    // A box whose x is W's: east refused, north (its y numbers) moved.
    let script = family_with("").replace(&square(-1000, 100, -600, 500, None), r#"[{"start": ["-W", 100], "end": ["-W / 2", 100]}, {"start": ["-W / 2", 100], "end": ["-W / 2", 500]}, {"start": ["-W / 2", 500], "end": ["-W", 500]}, {"start": ["-W", 500], "end": ["-W", 100]}]"#).replace(r#""parameters": ["#, r#""parameters": [{"name": "W", "value": 1000}, "#);
    let mut h = plan_of(&script);
    pick(&mut h, &[Picked::Form(1)]);
    let f = family(&h).forms[1].clone();
    drag(&mut h, [-750.0, 300.0, 0.0], [200.0, 0.0, 0.0], Modifiers::NONE);
    assert!(status(&h).contains("formula"), "{}", status(&h));
    assert_eq!(family(&h).forms[1], f);
    assert!(untouched(&h));
    drag(&mut h, [-750.0, 300.0, 0.0], [0.0, 200.0, 0.0], Modifiers::NONE);
    match &family(&h).forms[1].sketch[0].curve {
        FormCurve::Line { start, .. } => {
            assert_eq!(start[0], Expr::Formula("-W".into()), "its x keeps its formula");
            assert!(matches!(start[1], Expr::Number(y) if near(y, 300.0)), "{start:?} {}", status(&h));
        }
        FormCurve::Circle { .. } => panic!(),
    }
}

#[test]
fn a_move_another_type_cant_build_is_refused_naming_it() {
    // H: a square 0..1000 with a hole whose east side is at W (600 in T, 950 in U); G east of it, its west side Align-locked to
    // H's east side. G west 100 pulls H's east side to 900, past the hole in U.
    const H: &str = "fe700000-0000-4000-8000-0000000000b1";
    const G: &str = "fe700000-0000-4000-8000-0000000000b2";
    let l = |a: (&str, &str), b: (&str, &str), guid: &str| {
        let guid = if guid.is_empty() { String::new() } else { format!(r#""guid": "{guid}", "#) };
        format!(r#"{{{guid}"start": [{}, {}], "end": [{}, {}]}}"#, a.0, a.1, b.0, b.1)
    };
    let h_lines = [
        l(("0", "0"), ("1000", "0"), ""),
        l(("1000", "0"), ("1000", "1000"), "fe700000-0000-4000-8000-0000000000c1"),
        l(("1000", "1000"), ("0", "1000"), ""),
        l(("0", "1000"), ("0", "0"), ""),
        l(("400", "400"), ("400", "600"), ""),
        l(("400", "600"), ("\"W\"", "600"), ""),
        l(("\"W\"", "600"), ("\"W\"", "400"), ""),
        l(("\"W\"", "400"), ("400", "400"), ""),
    ]
    .join(", ");
    let g_lines = [
        l(("1000", "0"), ("1300", "0"), ""),
        l(("1300", "0"), ("1300", "1000"), ""),
        l(("1300", "1000"), ("1000", "1000"), ""),
        l(("1000", "1000"), ("1000", "0"), "fe700000-0000-4000-8000-0000000000c2"),
    ]
    .join(", ");
    let script = format!(
        r#"family {{"name": "Movers", "category": "generic", "parameters": [{{"name": "W", "value": 600}}], "types": {{"T": {{}}, "U": {{"W": 950}}}}, "forms": [{{"guid": "{H}", "sketch": [{h_lines}], "from": 0, "to": 300}}, {{"guid": "{G}", "sketch": [{g_lines}], "from": 0, "to": 300}}], "dimensions": [{{"refs": [{{"form": "{H}", "line": "fe700000-0000-4000-8000-0000000000c1"}}, {{"form": "{G}", "line": "fe700000-0000-4000-8000-0000000000c2"}}], "at": [1000, 1200, 0], "locked": [{{"segment": 0, "length": 0}}], "alignment": true}}]}}"#
    );
    let mut h = plan_of(&script);
    pick(&mut h, &[Picked::Form(1)]);
    let (h0, g0) = (points(&h, 0), points(&h, 1));
    drag(&mut h, [1150.0, 500.0, 0.0], [-100.0, 0.0, 0.0], Modifiers::NONE);
    assert!(status(&h).starts_with("U:"), "{}", status(&h));
    assert_eq!((points(&h, 0), points(&h, 1)), (h0, g0.clone()));
    assert!(untouched(&h));
    // East 100 is fine in both: H's east side follows G to 1100.
    drag(&mut h, [1150.0, 500.0, 0.0], [100.0, 0.0, 0.0], Modifiers::NONE);
    let d = shift(&g0, &points(&h, 1));
    assert!(near(d[0], 100.0), "{d:?} {}", status(&h));
    assert!((points(&h, 0)[2][0] - 1000.0 - d[0]).abs() < 1e-6, "{:?}", points(&h, 0));
}

#[test]
fn escape_undo_and_another_view_give_the_move_up_and_a_drag_back_leaves_nothing() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0), Picked::Symbol(0)]);
    let (before, symbols) = (points(&h, 0), family(&h).symbols.clone());
    let at = press_and_move(&mut h, [300.0, 300.0, 0.0], [200.0, 0.0, 0.0], Modifiers::NONE);
    assert_ne!(points(&h, 0), before, "live");
    assert!(h.state().prompt().contains("Esc"), "{}", h.state().prompt());
    key(&mut h, Key::Escape);
    assert_eq!(points(&h, 0), before);
    h.hover_at(at + vec2(30.0, 0.0));
    h.run();
    release(&mut h, at + vec2(30.0, 0.0));
    assert_eq!((points(&h, 0), family(&h).symbols.clone()), (before.clone(), symbols.clone()));
    assert!(untouched(&h));
    // Undo mid-drag.
    let at = press_and_move(&mut h, [300.0, 300.0, 0.0], [200.0, 0.0, 0.0], Modifiers::NONE);
    h.state_mut().family_undo();
    h.run();
    h.hover_at(at + vec2(0.0, 30.0));
    h.run();
    release(&mut h, at + vec2(0.0, 30.0));
    assert_eq!(points(&h, 0), before);
    assert!(untouched(&h));
    // Another view mid-drag.
    let at = press_and_move(&mut h, [300.0, 300.0, 0.0], [200.0, 0.0, 0.0], Modifiers::NONE);
    ed(&mut h).views.show(Some(Which::Elevation(Side::Front)));
    h.run();
    h.hover_at(at + vec2(30.0, 0.0));
    h.run();
    release(&mut h, at + vec2(30.0, 0.0));
    assert_eq!(points(&h, 0), before);
    assert!(untouched(&h));
    // A drag back where it began.
    view(&mut h, Some(Which::Plan));
    pick(&mut h, &[Picked::Form(0), Picked::Symbol(0)]);
    let (start, _) = ends(&h, [300.0, 300.0, 0.0], [0.0; 3]);
    press_and_move(&mut h, [300.0, 300.0, 0.0], [200.0, 0.0, 0.0], Modifiers::NONE);
    h.hover_at(start);
    h.run();
    release(&mut h, start);
    assert_eq!(points(&h, 0), before);
    assert!(untouched(&h), "{} steps", editor(&h).steps());
}

#[test]
fn handles_and_grips_still_shape_and_a_drag_from_nothing_still_boxes() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    let before = points(&h, 0);
    // The east side's handle: the side offsets, the west stays.
    let handle = editor(&h).views.shapes_shown.iter().map(|(p, _)| *p).min_by_key(|p| {
        let want = editor(&h).views.screen_of([500.0, 300.0, 0.0]).unwrap();
        (p.distance(want) * 100.0) as i64
    });
    let handle = handle.expect("a handle");
    drag_screen(&mut h, handle, handle + vec2(40.0, 0.0), PointerButton::Primary);
    let after = points(&h, 0);
    assert_eq!(after[0][0], before[0][0], "its west side stays: {after:?}");
    assert!(after[2][0] > 520.0, "{after:?}");
    // A reference line's end grip stretches it.
    let line = |h: &Harness<BimApp>| family(h).lines[0].clone();
    let l0 = line(&h);
    pick(&mut h, &[Picked::Line(g(LINE))]);
    let grip = editor(&h).views.grips_shown.iter().map(|(_, p)| *p).find(|p| {
        let want = editor(&h).views.screen_of([200.0, 1200.0, 0.0]).unwrap();
        p.distance(want) < 3.0
    });
    let grip = grip.expect("the end's grip");
    drag_screen(&mut h, grip, grip + vec2(40.0, 0.0), PointerButton::Primary);
    let l1 = line(&h);
    assert_eq!(l1.start, l0.start, "its start stays");
    assert!(l1.end[0] > 220.0, "{l1:?}");
    // From nothing: a window, taking B; nothing moves.
    let (b, a) = (points(&h, 1), points(&h, 0));
    let at = press_and_move(&mut h, [-1200.0, -100.0, 0.0], [1100.0, 900.0, 0.0], Modifiers::NONE);
    assert!(editor(&h).boxing(), "a box is drawn");
    release(&mut h, at);
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(1)]);
    assert_eq!((points(&h, 1), points(&h, 0)), (b, a));
}

#[test]
fn a_selected_dimensions_line_moves_and_what_it_measures_stays() {
    let mut h = plan();
    let planes = family(&h).planes.clone();
    drag(&mut h, [500.0, -1200.0, 0.0], [0.0, -300.0, 0.0], Modifiers::NONE);
    assert_eq!(editor(&h).selection.items(), &[Picked::Dimension(g(DIM))]);
    let at = family(&h).dimensions[0].at;
    assert!(near(at[1], -1500.0) && near(at[0], 500.0), "{at:?} {}", status(&h));
    assert_eq!(family(&h).planes, planes);
    assert_eq!(editor(&h).steps(), 1);
}

/// A and Edge selected in plan before, during and after a move, and A moved up in Front, saved where
/// `BIMCRAFT_DRAG_MOVE_DUMP` says (only then).
#[test]
fn family_drag_move_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_DRAG_MOVE_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let mut h = app(&family_with(""));
    h.set_size(vec2(1600.0, 1000.0));
    h.state_mut().edit_family("Movers");
    h.run();
    let save = |h: &mut Harness<BimApp>, name: &str| {
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("fe7-{name}.png"))).unwrap();
    };
    view(&mut h, Some(Which::Plan));
    pick(&mut h, &[Picked::Form(0), Picked::Plane(g(EDGE))]);
    let (over, _) = ends(&h, [300.0, 300.0, 0.0], [0.0; 3]);
    h.hover_at(over);
    h.run();
    save(&mut h, "plan");
    // Mid-drag, the button still down: the selection follows the pointer and the status bar says so.
    let at = press_and_move(&mut h, [300.0, 300.0, 0.0], [-200.0, 300.0, 0.0], Modifiers::NONE);
    save(&mut h, "plan-moving");
    release(&mut h, at);
    save(&mut h, "plan-moved");
    view(&mut h, Some(Which::Elevation(Side::Front)));
    pick(&mut h, &[Picked::Form(0)]);
    save(&mut h, "elevation");
    drag(&mut h, [150.0, 0.0, 100.0], [0.0, 0.0, 300.0], Modifiers::NONE);
    save(&mut h, "elevation-moved");
}

const WALL: &str = "fe700000-0000-4000-8000-000000000005";
const ON_WALL: &str = "fe700000-0000-4000-8000-000000000006";

/// [`family_with`] and a plane Wall at y 1800 (upright, along x) with a reference line drawn on it, 500 up.
fn with_wall() -> String {
    family_with("").replace(
        r#""ref_planes": ["#,
        &format!(r#""ref_planes": [{{"guid": "{WALL}", "name": "Wall", "start": [-2000, 1800, 0], "end": [2000, 1800, 0], "up": [0, 0, 1]}}, "#),
    )
    .replace(
        r#""ref_lines": ["#,
        &format!(r#""ref_lines": [{{"guid": "{ON_WALL}", "host": {{"plane": "{WALL}"}}, "start": [-300, 1800, 500], "end": [300, 1800, 500]}}, "#),
    )
}

fn wall_y(h: &Harness<BimApp>) -> f64 {
    family(h).planes.iter().find(|p| p.guid == g(WALL)).unwrap().start[1]
}

fn line_on_wall(h: &Harness<BimApp>) -> bimcraft_core::family::RefLine {
    family(h).lines.iter().find(|l| l.guid == g(ON_WALL)).unwrap().clone()
}

#[test]
fn a_reference_line_drawn_on_a_moved_plane_goes_with_it_and_the_family_stays_sound() {
    let mut h = plan_of(&with_wall());
    assert!(family(&h).check(&|_| true).is_ok(), "{:?}", family(&h).check(&|_| true));
    // Wall and A together, north 200.
    pick(&mut h, &[Picked::Plane(g(WALL)), Picked::Form(0)]);
    drag(&mut h, [300.0, 300.0, 0.0], [0.0, 200.0, 0.0], Modifiers::NONE);
    assert!(near(wall_y(&h), 2000.0), "{} {}", wall_y(&h), status(&h));
    let l = line_on_wall(&h);
    assert!((l.start[1] - wall_y(&h)).abs() < 1e-6 && (l.end[1] - wall_y(&h)).abs() < 1e-6, "{l:?}");
    assert_eq!((l.start[0], l.end[0], l.start[2]), (-300.0, 300.0, 500.0), "it moves with the plane alone");
    assert!(family(&h).check(&|_| true).is_ok(), "{:?}", family(&h).check(&|_| true));
    // Wall alone, by its own drag: the line goes with it too.
    pick(&mut h, &[Picked::Plane(g(WALL))]);
    let was = wall_y(&h);
    drag(&mut h, [1500.0, was, 0.0], [0.0, 300.0, 0.0], Modifiers::NONE);
    assert!(near(wall_y(&h), was + 300.0), "{} {}", wall_y(&h), status(&h));
    assert!((line_on_wall(&h).start[1] - wall_y(&h)).abs() < 1e-6, "{:?}", line_on_wall(&h));
    assert!(family(&h).check(&|_| true).is_ok(), "{:?}", family(&h).check(&|_| true));
}

#[test]
fn what_the_view_doesnt_show_stays_where_it_is() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0), Picked::Symbol(0), Picked::Dimension(g(DIM))]);
    view(&mut h, Some(Which::Elevation(Side::Front)));
    let (symbols, dims) = (family(&h).symbols.clone(), family(&h).dimensions.clone());
    let before = points(&h, 0);
    drag(&mut h, [300.0, 0.0, 150.0], [100.0, 0.0, 0.0], Modifiers::NONE);
    assert!(near(shift(&before, &points(&h, 0))[0], 100.0), "{}", status(&h));
    assert_eq!(family(&h).symbols, symbols, "a plan's symbol isn't in Front");
    assert_eq!(family(&h).dimensions, dims, "nor a plan's dimension");
}

#[test]
fn a_selected_dimension_dragged_by_its_text_moves_its_line() {
    let mut h = plan();
    pick(&mut h, &[Picked::Dimension(g(DIM))]);
    // Its text sits over the middle of its line.
    let (line, _) = ends(&h, [500.0, -1200.0, 0.0], [0.0; 3]);
    let text = line - vec2(0.0, 12.0);
    drag_screen(&mut h, text, text + vec2(0.0, 40.0), PointerButton::Primary);
    let at = family(&h).dimensions[0].at;
    assert!(at[1] < -1250.0 && at[0] == 500.0, "{at:?} {}", status(&h));
    assert_eq!(editor(&h).selection.items(), &[Picked::Dimension(g(DIM))]);
}

#[test]
fn shift_on_an_element_not_selected_neither_moves_it_nor_boxes() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    let (a, b) = (points(&h, 0), points(&h, 1));
    // On B's edge: its face, not selected, is the box's to begin in, and Shift's box would take away what it takes.
    let at = press_and_move(&mut h, [-600.0, 300.0, 0.0], [300.0, 300.0, 0.0], Modifiers::SHIFT);
    assert!(!editor(&h).boxing(), "no box");
    release(&mut h, at);
    assert_eq!((points(&h, 0), points(&h, 1)), (a, b));
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(0)]);
    assert!(untouched(&h));
}

#[test]
fn the_options_bars_depth_is_wide_enough_for_its_value_after_a_multi_select() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0), Picked::Form(1)]);
    h.run();
    view(&mut h, Some(Which::Elevation(Side::Front)));
    pick(&mut h, &[Picked::Form(0)]);
    for _ in 0..4 {
        h.run();
    }
    let widest = h.get_all_by_label("Depth").map(|n| n.rect().width()).fold(0.0f32, f32::max);
    assert!(widest >= 80.0, "the Depth field is {widest} wide");
}

/// With Select Elements by Face off a face selects nothing, but a press on the face of what is selected is still on it: a
/// drag from there moves the selection, as it does with the toggle on. The face of one not selected is the box's.
#[test]
fn with_select_elements_by_face_off_a_drag_from_a_selected_forms_face_moves_it() {
    let mut h = plan();
    h.state_mut().select_by_face = false;
    h.run();
    pick(&mut h, &[Picked::Form(0)]);
    let (a0, b0) = (points(&h, 0), points(&h, 1));
    // The status bar says a drag moves it while the pointer is over its face.
    let (over, _) = ends(&h, [300.0, 300.0, 0.0], [0.0; 3]);
    h.hover_at(over);
    h.run();
    h.run();
    assert!(h.state().prompt().contains("Drag to move"), "{}", h.state().prompt());
    drag(&mut h, [300.0, 300.0, 0.0], [300.0, 200.0, 0.0], Modifiers::NONE);
    let d = shift(&a0, &points(&h, 0));
    assert!(near(d[0], 300.0) && near(d[1], 200.0), "{d:?} {}", status(&h));
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(0)], "still selected");
    assert_eq!(editor(&h).steps(), 1);
    // From B's face, B not selected: a box, which moves nothing.
    let at = press_and_move(&mut h, [-800.0, 300.0, 0.0], [-100.0, 100.0, 0.0], Modifiers::NONE);
    assert!(editor(&h).boxing(), "a box from a face not selected");
    release(&mut h, at);
    assert_eq!(points(&h, 1), b0, "B stays");
    assert_eq!(editor(&h).steps(), 1);
}

/// Drag Elements on Selection on: a drag from the face of a form not selected selects it and moves it, as one from its edge
/// does; Ctrl adds it and both move; Shift neither moves it nor draws a box. Off, that face is the box's (above).
#[test]
fn with_drag_elements_on_selection_a_drag_from_a_face_not_selected_selects_and_moves_it() {
    let mut h = plan();
    h.state_mut().drag_on_selection = true;
    h.run();
    let (a0, b0) = (points(&h, 0), points(&h, 1));
    drag(&mut h, [-800.0, 300.0, 0.0], [0.0, 300.0, 0.0], Modifiers::NONE);
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(1)], "B, by its face");
    let d = shift(&b0, &points(&h, 1));
    assert!(near(d[1], 300.0) && near(d[0], 0.0), "{d:?} {}", status(&h));
    assert_eq!(points(&h, 0), a0);
    assert_eq!(editor(&h).steps(), 1);
    // Ctrl on A's face: added, and both move.
    let b1 = points(&h, 1);
    drag(&mut h, [300.0, 300.0, 0.0], [100.0, 0.0, 0.0], Modifiers::CTRL);
    let mut sel = editor(&h).selection.items().to_vec();
    sel.sort();
    assert_eq!(sel, [Picked::Form(0), Picked::Form(1)]);
    let (da, db) = (shift(&a0, &points(&h, 0)), shift(&b1, &points(&h, 1)));
    assert!(near(da[0], 100.0) && da.iter().zip(&db).all(|(a, b)| (a - b).abs() < 1e-6), "{da:?} {db:?} {}", status(&h));
    // Shift on A's face, B alone selected: nothing moves and no box is drawn.
    pick(&mut h, &[Picked::Form(1)]);
    let (a, b) = (points(&h, 0), points(&h, 1));
    let at = press_and_move(&mut h, [300.0, 300.0, 0.0], [300.0, 0.0, 0.0], Modifiers::SHIFT);
    assert!(!editor(&h).boxing(), "no box");
    release(&mut h, at);
    assert_eq!((points(&h, 0), points(&h, 1)), (a, b));
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(1)]);
}

/// A selected connector with no place of its own (an electrical device's may have none) stays where it is while the rest of
/// the selection moves, and the status bar says why, a note rather than a refusal.
#[test]
fn a_connector_with_no_place_stays_and_the_status_bar_says_so() {
    let script = family_with("").replace(
        r#""forms": ["#,
        r#""connectors": [{"kind": "power", "system": "power-balanced", "voltage": 230, "load_va": 10}], "forms": ["#,
    );
    let mut h = plan_of(&script);
    assert_eq!(family(&h).connectors.len(), 1);
    pick(&mut h, &[Picked::Form(0), Picked::Connector(0)]);
    let before = points(&h, 0);
    drag(&mut h, [300.0, 300.0, 0.0], [200.0, 0.0, 0.0], Modifiers::NONE);
    let d = shift(&before, &points(&h, 0));
    assert!(near(d[0], 200.0) && near(d[1], 0.0), "the form moves: {d:?} {}", status(&h));
    assert!(family(&h).connectors[0].at.is_none(), "the connector has no place to move");
    let s = h.state().status.clone();
    assert!(s.text.contains("no place of its own") && !s.error, "{s:?}");
    assert_eq!(editor(&h).steps(), 1);
}

/// [`plan`]'s family with box B far west, at x -6000: the room the plan is fitted to reaches out to it.
fn far_b() -> Harness<'static, BimApp> {
    let near = square(-1000, 100, -600, 500, None);
    let script = family_with("").replace(&near, &square(-6000, 100, -5600, 500, None));
    assert_ne!(script, family_with(""));
    plan_of(&script)
}

/// Each view keeps its own frame: the plan, its family shrunk (far box B deleted), keeps the frame it was fitted to, and so
/// it does after the Front elevation was shown and the plan shown again; Zoom to Fit fits it to the smaller family.
#[test]
fn each_view_keeps_its_frame_while_another_is_shown() {
    let mut h = far_b();
    let at = |h: &Harness<BimApp>| editor(h).views.screen_of([300.0, 300.0, 0.0]).unwrap();
    let before = at(&h);
    pick(&mut h, &[Picked::Form(1)]);
    assert!(h.state_mut().family_find_action("Delete"));
    h.run();
    h.run();
    assert_eq!(family(&h).forms.len(), 1, "{}", status(&h));
    assert_eq!(at(&h), before, "the plan keeps its frame as the family shrinks");
    view(&mut h, Some(Which::Elevation(Side::Front)));
    view(&mut h, Some(Which::Plan));
    assert_eq!(at(&h), before, "and keeps it after another view was shown");
    ed(&mut h).zoom_to_fit();
    h.run();
    h.run();
    assert_ne!(at(&h), before, "Zoom to Fit fits it to the family as it is now");
}

/// A family grown past the frame while nothing is dragged (here far box B brought back by Undo, as a file reloaded or a form
/// sketched far off in a family that began empty would) is fitted again, so what it takes up is in sight.
#[test]
fn a_family_grown_past_the_frame_is_fitted_again() {
    let mut h = far_b();
    pick(&mut h, &[Picked::Form(1)]);
    assert!(h.state_mut().family_find_action("Delete"));
    h.run();
    ed(&mut h).zoom_to_fit();
    h.run();
    h.run();
    let pane = editor(&h).views.pane.unwrap();
    let b = |h: &Harness<BimApp>| editor(h).views.screen_of([-5800.0, 300.0, 0.0]).unwrap();
    assert!(!pane.contains(b(&h)), "fitted to A alone, far B's place is out of sight: {:?}", b(&h));
    h.state_mut().family_undo();
    h.run();
    h.run();
    assert_eq!(family(&h).forms.len(), 2);
    assert!(pane.contains(b(&h)), "B back, the view takes it in: {:?} in {pane:?}", b(&h));
}
