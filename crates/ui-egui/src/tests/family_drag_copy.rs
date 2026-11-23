//! Ctrl and a drag on a selected element in a family's plan or elevation copy the selection to where it is let go:
//! the copies (new GUIDs, formulas as they were) follow the pointer and are selected after, the originals stay,
//! one step of the history. What holds a constraint among the copies is copied with them; a lock to what isn't copied is
//! dropped. Ctrl as the button comes up decides between a copy and a move. A datum, and copies the family's checks refuse,
//! leave nothing; so do Escape and a drag back where it began.
use super::family_drag_move::{
    A, A_LINE, CENTER_LR, DIM, EDGE, ed, ends, family, family_with, g, near, pick, plan, plan_of, points, press_and_move, shift, status,
    untouched,
};
use super::family_views::{editor, view};
use super::*;
use crate::editor::select_box::Picked;
use bimcraft_core::family::sketch::FormCurve;
use bimcraft_core::family::views::{Side, Which};
use bimcraft_core::family::{DimRef, Expr};
use bimcraft_core::ids::Guid;

const DIM2: &str = "fe400000-0000-4000-8000-000000000001";

fn release_with(h: &mut Harness<BimApp>, at: egui::Pos2, mods: Modifiers) {
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: mods });
    h.run();
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run();
}

/// Ctrl and a drag from family point `p` by `by` (mm), Ctrl still held as the button comes up.
pub(super) fn copy_drag(h: &mut Harness<BimApp>, p: [f64; 3], by: [f64; 3]) {
    let at = press_and_move(h, p, by, Modifiers::CTRL);
    release_with(h, at, Modifiers::CTRL);
}

fn mods(h: &mut Harness<BimApp>, m: Modifiers) {
    h.event(Event::ModifiersChanged(m));
    h.run();
    h.run();
}

/// The GUIDs of form `i`'s sketch lines.
fn line_guids(h: &Harness<BimApp>, i: usize) -> Vec<Guid> {
    family(h).forms[i].sketch.iter().map(|l| l.guid).collect()
}

/// [`family_with`] with Edge on A's east side (x 500), Align-locked to it, and `more` dimensions.
fn locked(more: &str) -> String {
    let lock = format!(
        r#", {{"refs": [{{"plane": "{EDGE}"}}, {{"form": "{A}", "line": "{A_LINE}1"}}], "at": [700, 300, 0], "locked": [{{"segment": 0, "length": 0}}], "alignment": true}}{more}"#
    );
    family_with(&lock).replace("[1000, -1500, 0], \"end\": [1000, 1500, 0]", "[500, -1500, 0], \"end\": [500, 1500, 0]")
}

fn edge_of(h: &Harness<BimApp>, guid: Guid) -> f64 {
    family(h).planes.iter().find(|p| p.guid == guid).map_or(f64::NAN, |p| p.start[0])
}

#[test]
fn a_form_copied_has_new_guids_is_selected_and_its_original_stays() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    let before = family(&h);
    let (a0, lines) = (points(&h, 0), line_guids(&h, 0));
    // The status bar says a drag copies while Ctrl is held over it.
    let (over, _) = ends(&h, [300.0, 300.0, 0.0], [0.0; 3]);
    h.hover_at(over);
    mods(&mut h, Modifiers::CTRL);
    assert!(h.state().prompt().contains("to copy"), "{}", h.state().prompt());
    assert!(!h.state().prompt().contains("click to add"), "it is selected already: {}", h.state().prompt());
    mods(&mut h, Modifiers::NONE);
    copy_drag(&mut h, [300.0, 300.0, 0.0], [0.0, -700.0, 0.0]);
    let f = family(&h);
    assert_eq!(f.forms.len(), 3, "{}", status(&h));
    assert_eq!(f.forms[0], before.forms[0], "the original stays");
    let d = shift(&a0, &points(&h, 1));
    assert!(near(d[0], 0.0) && near(d[1], -700.0), "{d:?}");
    assert!(f.forms[1].guid != g(A) && !f.forms[1].guid.is_nil());
    let copied = line_guids(&h, 1);
    assert!(copied.iter().all(|c| !c.is_nil() && !lines.contains(c)), "{copied:?}");
    assert_eq!((f.forms[1].from.clone(), f.forms[1].to.clone()), (f.forms[0].from.clone(), f.forms[0].to.clone()));
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(1)], "the copy is selected");
    assert_eq!(editor(&h).steps(), 1);
    assert!(editor(&h).unsaved());
    // Undo takes the copy away, Redo brings it back.
    h.state_mut().family_undo();
    h.run();
    assert_eq!(family(&h).forms.len(), 2);
    h.state_mut().family_redo();
    h.run();
    assert_eq!(family(&h).forms.len(), 3);
}

#[test]
fn a_copy_keeps_its_formulas_where_it_moves_numbers_and_a_formula_in_the_way_refuses_it() {
    let script = family_with("").replace(&super::family_drag_move::square(-1000, 100, -600, 500, None), r#"[{"start": ["-W", 100], "end": ["-W / 2", 100]}, {"start": ["-W / 2", 100], "end": ["-W / 2", 500]}, {"start": ["-W / 2", 500], "end": ["-W", 500]}, {"start": ["-W", 500], "end": ["-W", 100]}]"#).replace(r#""parameters": ["#, r#""parameters": [{"name": "W", "value": 1000}, "#);
    let mut h = plan_of(&script);
    pick(&mut h, &[Picked::Form(1)]);
    copy_drag(&mut h, [-750.0, 300.0, 0.0], [0.0, 700.0, 0.0]);
    assert_eq!(family(&h).forms.len(), 3, "{}", status(&h));
    match &family(&h).forms[2].sketch[0].curve {
        FormCurve::Line { start, .. } => {
            assert_eq!(start[0], Expr::Formula("-W".into()), "its x keeps its formula");
            assert!(matches!(start[1], Expr::Number(y) if near(y, 800.0)), "{start:?}");
        }
        FormCurve::Circle { .. } => panic!(),
    }
    // Across, its x would have to change: refused, nothing made.
    let n = editor(&h).steps();
    pick(&mut h, &[Picked::Form(1)]);
    copy_drag(&mut h, [-750.0, 300.0, 0.0], [300.0, 0.0, 0.0]);
    assert!(status(&h).contains("formula") && status(&h).contains("of form 2 "), "it names the original: {}", status(&h));
    assert_eq!((family(&h).forms.len(), editor(&h).steps()), (3, n));
}

#[test]
fn a_form_and_a_plane_copy_their_align_lock_and_a_form_alone_drops_it() {
    let mut h = plan_of(&locked(""));
    let dims = family(&h).dimensions.len();
    pick(&mut h, &[Picked::Form(0), Picked::Plane(g(EDGE))]);
    copy_drag(&mut h, [300.0, 300.0, 0.0], [1500.0, 300.0, 0.0]);
    let f = family(&h);
    assert_eq!(f.forms.len(), 3, "{}", status(&h));
    let edge = f.planes.last().unwrap().guid;
    // Its trace slides along itself by the drag's part along it, as a moved plane's does.
    assert!(near(f.planes.last().unwrap().start[1], -1200.0), "{:?}", f.planes.last());
    assert_ne!(edge, g(EDGE));
    assert!(near(edge_of(&h, edge), 2000.0) && edge_of(&h, g(EDGE)) == 500.0, "{} {}", edge_of(&h, edge), status(&h));
    // The lock is copied, naming the copies; the dimension from Center (Left/Right) isn't.
    assert_eq!(f.dimensions.len(), dims + 1, "{:?}", f.dimensions);
    let lock = f.dimensions.last().unwrap();
    assert!(lock.alignment);
    assert_eq!(lock.refs, vec![DimRef::Plane(edge), DimRef::Sketch(f.forms[1].guid, f.forms[1].sketch[1].guid)]);
    let mut sel = editor(&h).selection.items().to_vec();
    sel.sort();
    let mut want = vec![Picked::Form(1), Picked::Plane(edge)];
    want.sort();
    assert_eq!(sel, want, "the copies are selected, the lock isn't");
    // Undo takes all of it away in one step.
    h.state_mut().family_undo();
    h.run();
    let f = family(&h);
    assert_eq!((f.forms.len(), f.dimensions.len(), editor(&h).steps()), (2, dims, 0));
    // A alone: its lock to Edge, which isn't copied, is dropped; the copy goes where it is let go.
    let mut h = plan_of(&locked(""));
    pick(&mut h, &[Picked::Form(0)]);
    let a0 = points(&h, 0);
    copy_drag(&mut h, [300.0, 300.0, 0.0], [1500.0, 0.0, 0.0]);
    assert_eq!(family(&h).forms.len(), 3, "{}", status(&h));
    assert!(near(shift(&a0, &points(&h, 1))[0], 1500.0));
    assert_eq!(points(&h, 0), a0);
    assert_eq!(family(&h).dimensions.len(), dims, "no lock copied");
}

#[test]
fn a_dimension_is_copied_where_all_it_measures_is_and_a_label_drives_the_copy() {
    // DIM2 from Edge to A's east side, plain; Len labels A's width (400).
    let more = format!(
        r#", {{"guid": "{DIM2}", "refs": [{{"plane": "{EDGE}"}}, {{"form": "{A}", "line": "{A_LINE}1"}}], "at": [700, 600, 0]}}, {{"refs": [{{"form": "{A}", "line": "{A_LINE}3"}}, {{"form": "{A}", "line": "{A_LINE}1"}}], "at": [300, 700, 0], "label": "Len"}}"#
    );
    let script = family_with(&more).replace(r#""name": "Len", "value": 100"#, r#""name": "Len", "value": 400"#);
    let mut h = plan_of(&script);
    let dims = family(&h).dimensions.len();
    pick(&mut h, &[Picked::Form(0), Picked::Plane(g(EDGE)), Picked::Dimension(g(DIM)), Picked::Dimension(g(DIM2))]);
    copy_drag(&mut h, [300.0, 300.0, 0.0], [0.0, -2000.0, 0.0]);
    let f = family(&h);
    assert_eq!(f.forms.len(), 3, "{}", status(&h));
    // DIM2 and the label come along; DIM, from a datum, doesn't.
    assert_eq!(f.dimensions.len(), dims + 2, "{:?}", f.dimensions);
    let (form, edge) = (f.forms[1].guid, f.planes.last().unwrap().guid);
    let copy2 = f.dimensions.iter().find(|d| d.refs.first() == Some(&DimRef::Plane(edge))).expect("DIM2's copy");
    assert!(copy2.label.is_none() && near(copy2.at[1], -1400.0), "{copy2:?}");
    let label = f.dimensions[dims..].iter().find(|d| d.label.is_some()).expect("the label's copy");
    assert_eq!(label.label.as_deref(), Some("Len"));
    assert!(label.refs.iter().all(|r| r.names(form)), "{label:?}");
    let sel = editor(&h).selection.items();
    assert!(sel.contains(&Picked::Dimension(copy2.guid)), "{sel:?}");
    assert!(!sel.contains(&Picked::Dimension(label.guid)) && !sel.contains(&Picked::Dimension(g(DIM))), "{sel:?}");
    // The copy is driven by Len as the original is: 600 wide for a type that says so.
    let mut f = f.clone();
    f.types.insert("Wide".into(), [("Len".to_string(), serde_json::json!(600))].into_iter().collect());
    let v = f.values("Wide", &std::collections::BTreeMap::new()).unwrap();
    assert!(f.solve(&v).is_ok() && f.pieces(&v).is_ok());
    let flexed = f.flexed(&v).unwrap();
    let xs: Vec<f64> = flexed
        .forms
        .iter()
        .find(|x| x.guid == form)
        .unwrap()
        .sketch
        .iter()
        .flat_map(|l| match &l.curve {
            FormCurve::Line { start, end, .. } => [&start[0], &end[0]].map(|e| match e {
                Expr::Number(x) => *x,
                _ => f64::NAN,
            }),
            FormCurve::Circle { .. } => [f64::NAN; 2],
        })
        .collect();
    let wide = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max) - xs.iter().copied().fold(f64::INFINITY, f64::min);
    assert!((wide - 600.0).abs() < 1e-6, "the copy is {wide} wide in Wide: {xs:?}");
}

#[test]
fn ctrl_as_the_button_comes_up_decides_between_a_copy_and_a_move() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    let a0 = points(&h, 0);
    // Ctrl let go mid-drag: a move.
    let at = press_and_move(&mut h, [300.0, 300.0, 0.0], [200.0, 0.0, 0.0], Modifiers::CTRL);
    assert_eq!(family(&h).forms.len(), 3, "the copy's preview");
    assert!(h.state().prompt().contains("Copying"), "{}", h.state().prompt());
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(1)]);
    mods(&mut h, Modifiers::NONE);
    assert_eq!(family(&h).forms.len(), 2, "the move's preview");
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(0)]);
    release_with(&mut h, at, Modifiers::NONE);
    assert_eq!(family(&h).forms.len(), 2);
    assert!(near(shift(&a0, &points(&h, 0))[0], 200.0), "{}", status(&h));
    assert_eq!(editor(&h).steps(), 1);
    // Ctrl pressed mid-drag: a copy, the original where it was.
    let a1 = points(&h, 0);
    let at = press_and_move(&mut h, [500.0, 300.0, 0.0], [0.0, 900.0, 0.0], Modifiers::NONE);
    mods(&mut h, Modifiers::CTRL);
    release_with(&mut h, at, Modifiers::CTRL);
    assert_eq!(family(&h).forms.len(), 3, "{}", status(&h));
    assert_eq!(points(&h, 0), a1);
    assert!(near(shift(&a1, &points(&h, 1))[1], 900.0));
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(1)]);
    assert_eq!(editor(&h).steps(), 2);
}

#[test]
fn a_ctrl_press_on_a_form_not_selected_adds_it_and_copies_both() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    // On B's edge: its face, not selected, is the box's to begin in.
    copy_drag(&mut h, [-600.0, 300.0, 0.0], [0.0, 900.0, 0.0]);
    let f = family(&h);
    assert_eq!(f.forms.len(), 4, "{}", status(&h));
    let mut sel = editor(&h).selection.items().to_vec();
    sel.sort();
    assert_eq!(sel, [Picked::Form(1), Picked::Form(3)]);
}

#[test]
fn a_datum_or_copies_the_checks_refuse_leave_nothing() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0), Picked::Plane(g(CENTER_LR))]);
    copy_drag(&mut h, [300.0, 300.0, 0.0], [0.0, 900.0, 0.0]);
    assert!(status(&h).contains("datum"), "{}", status(&h));
    assert_eq!(family(&h).forms.len(), 2);
    assert!(untouched(&h));
    let mut sel = editor(&h).selection.items().to_vec();
    sel.sort();
    let mut want = vec![Picked::Form(0), Picked::Plane(g(CENTER_LR))];
    want.sort();
    assert_eq!(sel, want, "the selection as it was");
    // A family with as many symbols as one holds: a copy of one would be one too many.
    let symbols: Vec<String> = (0..64).map(|k| format!(r#"{{"line": [[-200, {}], [200, {}]]}}"#, -800 - 10 * k, -800 - 10 * k)).collect();
    let script = family_with("")
        .replace(r#""symbols": [{"line": [[-200, -800], [200, -800]]}]"#, &format!(r#""symbols": [{}]"#, symbols.join(", ")));
    let mut h = plan_of(&script);
    assert_eq!(family(&h).symbols.len(), 64);
    pick(&mut h, &[Picked::Form(0)]);
    copy_drag(&mut h, [300.0, 300.0, 0.0], [0.0, 900.0, 0.0]);
    assert_eq!(family(&h).forms.len(), 3, "a form is fine: {}", status(&h));
    let n = editor(&h).steps();
    pick(&mut h, &[Picked::Symbol(0)]);
    copy_drag(&mut h, [150.0, -800.0, 0.0], [0.0, 900.0, 0.0]);
    assert!(status(&h).contains("symbols"), "{}", status(&h));
    assert_eq!((family(&h).symbols.len(), editor(&h).steps()), (64, n));
}

#[test]
fn escape_and_a_drag_back_where_it_began_make_no_copies() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    let at = press_and_move(&mut h, [300.0, 300.0, 0.0], [200.0, 0.0, 0.0], Modifiers::CTRL);
    assert_eq!(family(&h).forms.len(), 3);
    key(&mut h, Key::Escape);
    assert_eq!(family(&h).forms.len(), 2);
    release_with(&mut h, at, Modifiers::CTRL);
    assert_eq!(family(&h).forms.len(), 2);
    assert!(untouched(&h));
    // Back where it began: no copies stacked on the originals, silently.
    pick(&mut h, &[Picked::Form(0)]);
    let (start, _) = ends(&h, [300.0, 300.0, 0.0], [0.0; 3]);
    press_and_move(&mut h, [300.0, 300.0, 0.0], [200.0, 0.0, 0.0], Modifiers::CTRL);
    h.hover_at(start);
    h.run();
    release_with(&mut h, start, Modifiers::CTRL);
    assert_eq!(family(&h).forms.len(), 2);
    assert!(untouched(&h));
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(0)]);
}

#[test]
fn an_extrusion_copied_in_an_elevation_moves_its_copys_ends_and_only_what_the_view_shows_is_copied() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0), Picked::Symbol(0)]);
    view(&mut h, Some(Which::Elevation(Side::Front)));
    let a0 = points(&h, 0);
    copy_drag(&mut h, [300.0, 0.0, 150.0], [0.0, 0.0, 400.0]);
    let f = family(&h);
    assert_eq!(f.forms.len(), 3, "{}", status(&h));
    assert_eq!(f.symbols.len(), 1, "a plan's symbol isn't in Front");
    let num = |e: &Expr| match e {
        Expr::Number(x) => *x,
        _ => f64::NAN,
    };
    let (from, to) = (num(&f.forms[1].from), num(&f.forms[1].to));
    assert!(near(from, 400.0) && (to - from - 300.0).abs() < 1e-6, "{from} {to}");
    assert_eq!((num(&f.forms[0].from), num(&f.forms[0].to)), (0.0, 300.0));
    assert_eq!(points(&h, 1), a0, "its sketch where A's is");
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(1)]);
}

#[test]
fn a_nested_instance_is_copied_as_another_of_its_type() {
    let leg = r#"{"types": {"A": {}}, "forms": [{"profile": [[-100, -100], [100, -100], [100, 100], [-100, 100]], "to": 500}]}"#;
    let script = family_with("").replace(
        r#""forms": ["#,
        &format!(r#""nested": [{{"name": "Leg", "family": {leg}}}], "nested_instances": [{{"family": "Leg", "type": "A", "at": [1500, -500]}}], "forms": ["#),
    );
    let mut h = plan_of(&script);
    assert_eq!(family(&h).nested_instances.len(), 1);
    pick(&mut h, &[Picked::Nested(0)]);
    copy_drag(&mut h, [1500.0, -500.0, 0.0], [0.0, 1000.0, 0.0]);
    let f = family(&h);
    assert_eq!(f.nested_instances.len(), 2, "{}", status(&h));
    let (a, b) = (&f.nested_instances[0], &f.nested_instances[1]);
    assert_eq!((b.family.as_str(), b.type_name.as_str()), ("Leg", "A"));
    assert_ne!(a.guid, b.guid);
    assert_eq!(a.at, [Expr::Number(1500.0), Expr::Number(-500.0)]);
    assert!(matches!(b.at[1], Expr::Number(y) if near(y, 500.0)), "{:?}", b.at);
    assert_eq!(editor(&h).selection.items(), &[Picked::Nested(1)]);
}

/// A and Edge selected in plan with Ctrl held over A (the prompt), mid-copy, and copied, saved where `BIMCRAFT_DRAG_COPY_DUMP`
/// says (only then).
#[test]
fn family_drag_copy_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_DRAG_COPY_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let mut h = app(&family_with(""));
    h.set_size(vec2(1600.0, 1000.0));
    h.state_mut().edit_family("Movers");
    h.run();
    let save = |h: &mut Harness<BimApp>, name: &str| {
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("fe4-{name}.png"))).unwrap();
    };
    view(&mut h, Some(Which::Plan));
    pick(&mut h, &[Picked::Form(0), Picked::Plane(g(EDGE))]);
    let (over, _) = ends(&h, [300.0, 300.0, 0.0], [0.0; 3]);
    h.hover_at(over);
    mods(&mut h, Modifiers::CTRL);
    save(&mut h, "ctrl-prompt");
    let at = press_and_move(&mut h, [300.0, 300.0, 0.0], [500.0, 600.0, 0.0], Modifiers::CTRL);
    save(&mut h, "copying");
    release_with(&mut h, at, Modifiers::CTRL);
    save(&mut h, "copied");
    let _ = ed(&mut h);
}

#[test]
fn over_a_copy_just_dropped_the_status_bar_says_a_drag_moves_it_and_over_a_plane_not_selected_it_doesnt() {
    // Dropped with the pointer on Edge's trace (x 1000), which a press there would select: the general prompt.
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    copy_drag(&mut h, [300.0, 300.0, 0.0], [700.0, 600.0, 0.0]);
    h.run();
    assert!(!h.state().prompt().contains("Drag to move"), "{}", h.state().prompt());
    // Dropped clear of it: over the copy, selected, a drag moves it, and with Ctrl copies it.
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    copy_drag(&mut h, [300.0, 300.0, 0.0], [500.0, 600.0, 0.0]);
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(1)]);
    h.run();
    h.run();
    assert!(h.state().prompt().contains("Drag to move"), "{}", h.state().prompt());
    mods(&mut h, Modifiers::CTRL);
    assert!(h.state().prompt().contains("to copy"), "{}", h.state().prompt());
}

#[test]
fn a_dimension_alone_that_cant_be_copied_keeps_the_selection_and_says_so() {
    // DIM measures from Center (Left/Right), a datum, which isn't copied.
    let mut h = plan();
    pick(&mut h, &[Picked::Dimension(g(DIM))]);
    copy_drag(&mut h, [500.0, -1200.0, 0.0], [0.0, -300.0, 0.0]);
    assert!(status(&h).contains("Nothing selected here can be copied"), "{}", status(&h));
    assert_eq!(editor(&h).selection.items(), &[Picked::Dimension(g(DIM))]);
    assert_eq!(family(&h).dimensions[0].at, [500.0, -1200.0, 0.0]);
    assert!(untouched(&h));
}

#[test]
fn a_refusal_names_the_symbol_copied_not_its_copy() {
    let script = family_with("").replace(
        r#""symbols": [{"line": [[-200, -800], [200, -800]]}]"#,
        r#""symbols": [{"line": [[-200, -800], [200, -800]]}, {"line": [["-Len", -900], [200, -900]]}]"#,
    );
    let mut h = plan_of(&script);
    pick(&mut h, &[Picked::Symbol(1)]);
    copy_drag(&mut h, [150.0, -900.0, 0.0], [300.0, 0.0, 0.0]);
    assert!(status(&h).contains("symbol 2 "), "{}", status(&h));
    assert_eq!(family(&h).symbols.len(), 2);
    assert!(untouched(&h));
}

#[test]
fn a_lone_plane_pressed_with_ctrl_and_let_go_without_moves_as_its_own_drag_does() {
    // DIM labelled Len (1000): Edge's own drag takes Len with it, where a move of the selection would be held by the label.
    let script = family_with("")
        .replace(r#""at": [500, -1200, 0]}"#, r#""at": [500, -1200, 0], "label": "Len"}"#)
        .replace(r#""name": "Len", "value": 100"#, r#""name": "Len", "value": 1000"#);
    let mut h = plan_of(&script);
    pick(&mut h, &[Picked::Plane(g(EDGE))]);
    let at = press_and_move(&mut h, [1000.0, 800.0, 0.0], [200.0, 0.0, 0.0], Modifiers::CTRL);
    mods(&mut h, Modifiers::NONE);
    release_with(&mut h, at, Modifiers::NONE);
    let f = family(&h);
    assert_eq!(f.planes.iter().filter(|p| !p.datum()).count(), 1, "no copy: {}", status(&h));
    assert!(near(edge_of(&h, g(EDGE)), 1200.0), "{} {}", edge_of(&h, g(EDGE)), status(&h));
    let len = f.values("T", &std::collections::BTreeMap::new()).unwrap()["Len"].clone();
    assert!(matches!(len, bimcraft_core::family::Val::Number(v) if near(v, 1200.0)), "{len:?}");
}

#[test]
fn a_forms_copy_goes_right_after_it_so_a_void_after_it_cuts_it_as_it_cuts_the_original() {
    let script = family_with("").replace(r#"{"sketch": "#, r#"{"void": true, "sketch": "#);
    let mut h = plan_of(&script);
    assert!(family(&h).forms[1].void);
    pick(&mut h, &[Picked::Form(0)]);
    copy_drag(&mut h, [300.0, 300.0, 0.0], [0.0, 900.0, 0.0]);
    let f = family(&h);
    assert_eq!(f.forms.len(), 3, "{}", status(&h));
    assert!(!f.forms[1].void && f.forms[2].void, "A's copy after A, B (the void) after it");
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(1)]);
}

#[test]
fn shift_and_ctrl_keep_a_copy_square_to_the_view() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    let a0 = points(&h, 0);
    let both = Modifiers { ctrl: true, shift: true, ..Modifiers::NONE };
    let at = press_and_move(&mut h, [300.0, 300.0, 0.0], [100.0, 700.0, 0.0], both);
    release_with(&mut h, at, both);
    assert_eq!(family(&h).forms.len(), 3, "{}", status(&h));
    let d = shift(&a0, &points(&h, 1));
    assert!(near(d[0], 0.0) && near(d[1], 700.0) && d[0] == 0.0, "{d:?}");
    assert_eq!(points(&h, 0), a0);
}

#[test]
fn a_second_dimension_reporting_one_parameter_is_refused_its_label() {
    let more =
        format!(r#", {{"guid": "{DIM2}", "refs": [{{"plane": "{EDGE}"}}, {{"form": "{A}", "line": "{A_LINE}1"}}], "at": [700, 600, 0]}}"#);
    let script = family_with(&more)
        .replace(r#""parameters": ["#, r#""parameters": [{"name": "Span", "kind": "length", "reporting": true, "value": 0}, "#)
        .replace(r#""at": [500, -1200, 0]}"#, r#""at": [500, -1200, 0], "label": "Span"}"#);
    let mut h = plan_of(&script);
    let e = ed(&mut h).set_label(g(DIM2), Some("Span".into())).unwrap_err();
    assert!(e.contains("reports `Span` already"), "{e}");
    assert!(family(&h).dimensions.iter().all(|d| d.guid != g(DIM2) || d.label.is_none()));
    // Its own dimension may keep it.
    assert!(ed(&mut h).set_label(g(DIM), Some("Span".into())).is_ok());
}

/// With Select Elements by Face off, Ctrl and a drag from the face of a form selected copy it, as with the toggle on.
#[test]
fn with_select_elements_by_face_off_ctrl_and_a_drag_from_a_selected_forms_face_copy_it() {
    let mut h = plan();
    h.state_mut().select_by_face = false;
    h.run();
    pick(&mut h, &[Picked::Form(0)]);
    let a0 = points(&h, 0);
    copy_drag(&mut h, [300.0, 300.0, 0.0], [0.0, -700.0, 0.0]);
    let f = family(&h);
    assert_eq!(f.forms.len(), 3, "{}", status(&h));
    assert_eq!(points(&h, 0), a0, "the original stays");
    let d = shift(&a0, &points(&h, 1));
    assert!(near(d[0], 0.0) && near(d[1], -700.0), "{d:?}");
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(1)], "the copy is selected");
    assert_eq!(editor(&h).steps(), 1);
}

/// Ctrl and a drag from the face of a form not selected: off (at first), a box that adds what it takes and copies nothing;
/// with Drag Elements on Selection on, the form selected and copied, as from its edge.
#[test]
fn with_drag_elements_on_selection_ctrl_and_a_drag_from_a_face_not_selected_copy_it() {
    let mut h = plan();
    let a0 = points(&h, 0);
    copy_drag(&mut h, [300.0, 300.0, 0.0], [0.0, -700.0, 0.0]);
    assert_eq!(family(&h).forms.len(), 2, "off, nothing copied");
    assert!(untouched(&h));
    key(&mut h, Key::Escape);
    h.state_mut().drag_on_selection = true;
    h.run();
    copy_drag(&mut h, [300.0, 300.0, 0.0], [0.0, -700.0, 0.0]);
    assert_eq!(family(&h).forms.len(), 3, "{}", status(&h));
    assert_eq!(points(&h, 0), a0, "the original stays");
    let d = shift(&a0, &points(&h, 1));
    assert!(near(d[0], 0.0) && near(d[1], -700.0), "{d:?}");
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(1)], "the copy is selected");
    assert_eq!(editor(&h).steps(), 1);
}
