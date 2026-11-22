//! Edit Witness Lines on a family's dimension (no key in a family yet: the project's EW is Wire): with an aligned dimension
//! selected, a click on a reference the Dimension tool takes adds it, in order along the dimension; a click on one it has takes it off; a click on
//! nothing, or Esc, ends. Each click is one step. What it can't do it refuses, saying why: a dimension keeps two references,
//! a labelled one measures one segment, a reference must be parallel to the dimension's, a lock or EQ isn't lost silently.
use super::family_views::{click_family, editor, open, view};
use super::*;
use bimcraft_core::family::DimRef;
use bimcraft_core::family::views::Which;

const LEFT: &str = "11111111-1111-4111-8111-000000000001";
const RIGHT: &str = "11111111-1111-4111-8111-000000000002";
const CENTER_LR: &str = "9e3c0a10-1f0b-4d8c-8a5e-0000000000a3";
const FAR: &str = "11111111-1111-4111-8111-000000000003";
const BAR: &str = "22222222-2222-4222-8222-000000000001";

/// A box family: Left and Right planes at x ±500, Far at x -900, and a bar along x on Center (Left/Right) from -400 to 400
/// whose ends are references; `dims`, its dimensions as JSON.
fn family(dims: &str) -> String {
    format!(
        r#"family {{"name": "Box", "category": "generic", "parameters": [{{"name": "Width", "value": 1000}}], "types": {{"T": {{}}}}, "ref_planes": [{{"guid": "{LEFT}", "name": "Left", "start": [-500, -2000, 0], "end": [-500, 2000, 0], "up": [0, 0, 1]}}, {{"guid": "{RIGHT}", "name": "Right", "start": [500, -2000, 0], "end": [500, 2000, 0], "up": [0, 0, 1]}}, {{"guid": "{FAR}", "name": "Far", "start": [-900, -2000, 0], "end": [-900, 2000, 0], "up": [0, 0, 1]}}], "forms": [{{"guid": "{BAR}", "host": {{"plane": "{CENTER_LR}"}}, "profile": [[-100, 0], [100, 0], [100, 50], [-100, 50]], "from": -400, "to": 400}}], "dimensions": [{dims}]}}"#
    )
}

fn plane(g: &str) -> String {
    format!(r#"{{"plane": "{g}"}}"#)
}

/// A dimension along x at y -700 between `refs`, with `more` JSON fields.
fn dim(refs: &[&str], more: &str) -> String {
    let refs: Vec<String> = refs.iter().map(|g| plane(g)).collect();
    format!(r#"{{"refs": [{}], "at": [0, -700, 0]{more}}}"#, refs.join(", "))
}

fn g(s: &str) -> bimcraft_core::ids::Guid {
    serde_json::from_value(json!(s)).unwrap()
}

/// The box open in its floor plan, its one dimension selected and Edit Witness Lines on.
fn editing(dims: &str) -> Harness<'static, BimApp> {
    editing_in(dims, Which::Plan, [200.0, -700.0, 0.0])
}

/// The box open in view `which`, its one dimension selected by a click at `on` and Edit Witness Lines on.
fn editing_in(dims: &str, which: Which, on: [f64; 3]) -> Harness<'static, BimApp> {
    let (mut h, _) = open(&family(dims), "Box");
    view(&mut h, Some(which));
    click_family(&mut h, on);
    assert!(editor(&h).selected_dimension().is_some(), "the dimension is selected");
    // Keyless, as the project's EW is Wire: the Options Bar's button.
    press(&mut h, "Edit Witness Lines");
    assert!(editor(&h).views.witnessing.is_some(), "Edit Witness Lines is on");
    h
}

fn refs(h: &Harness<BimApp>) -> Vec<DimRef> {
    editor(h).dimensions[0].refs.clone()
}

fn planes(gs: &[&str]) -> Vec<DimRef> {
    gs.iter().map(|s| DimRef::Plane(g(s))).collect()
}

fn said(h: &Harness<BimApp>) -> (String, bool) {
    let s = &h.state().status;
    (s.text.clone(), s.error)
}

#[test]
fn ew_adds_a_reference_in_order_and_takes_it_off_each_click_one_step() {
    let mut h = editing(&dim(&[LEFT, RIGHT], ""));
    let steps = editor(&h).steps();
    click_family(&mut h, [0.0, 600.0, 0.0]);
    assert_eq!(refs(&h), planes(&[LEFT, CENTER_LR, RIGHT]), "in order along the dimension");
    assert_eq!(editor(&h).steps(), steps + 1);
    click_family(&mut h, [0.0, 600.0, 0.0]);
    assert_eq!(refs(&h), planes(&[LEFT, RIGHT]), "taken off again");
    assert_eq!(editor(&h).steps(), steps + 2);
    // Two references at least: the dimension keeps them, saying why.
    click_family(&mut h, [-500.0, 600.0, 0.0]);
    assert_eq!(refs(&h), planes(&[LEFT, RIGHT]));
    let (text, error) = said(&h);
    assert!(error && text.contains("two references"), "{text}");
    // Esc ends the tool.
    key(&mut h, Key::Escape);
    assert!(editor(&h).views.witnessing.is_none());
}

#[test]
fn ew_refuses_a_reference_not_parallel_and_a_labelled_dimension() {
    let mut h = editing(&dim(&[LEFT, RIGHT], ""));
    // Center (Front/Back) runs along x: square to the dimension's references.
    click_family(&mut h, [-1500.0, 0.0, 0.0]);
    assert_eq!(refs(&h), planes(&[LEFT, RIGHT]));
    let (text, error) = said(&h);
    assert!(error && text.contains("parallel"), "{text}");
    let mut h = editing(&dim(&[LEFT, RIGHT], r#", "label": "Width""#));
    click_family(&mut h, [0.0, 600.0, 0.0]);
    assert_eq!(refs(&h), planes(&[LEFT, RIGHT]));
    let (text, error) = said(&h);
    assert!(error && text.contains("label"), "{text}");
}

#[test]
fn a_lock_or_eq_is_never_lost_without_a_word() {
    // Taking off the middle reference merges two segments: refused while only one of them is locked.
    let mut h = editing(&dim(&[LEFT, CENTER_LR, RIGHT], r#", "locked": [{"segment": 0, "length": 500}]"#));
    click_family(&mut h, [0.0, 600.0, 0.0]);
    assert_eq!(refs(&h).len(), 3);
    let (text, error) = said(&h);
    assert!(error && text.contains("locked"), "{text}");
    // Both locked: the merged segment is locked at their sum.
    let mut h = editing(&dim(&[LEFT, CENTER_LR, RIGHT], r#", "locked": [{"segment": 0, "length": 500}, {"segment": 1, "length": 500}]"#));
    click_family(&mut h, [0.0, 600.0, 0.0]);
    let d = editor(&h).dimensions[0].clone();
    assert_eq!(d.refs, planes(&[LEFT, RIGHT]));
    assert_eq!(d.locked.iter().map(|l| (l.segment, l.length)).collect::<Vec<_>>(), [(0, 1000.0)]);
    // Splitting a locked segment drops its lock, and says so.
    click_family(&mut h, [0.0, 600.0, 0.0]);
    let d = editor(&h).dimensions[0].clone();
    assert_eq!(d.refs, planes(&[LEFT, CENTER_LR, RIGHT]));
    assert!(d.locked.is_empty());
    let (text, error) = said(&h);
    assert!(!error && text.contains("lock"), "{text}");
    // An EQ dimension of two segments left with one is no longer EQ, and says so.
    let mut h = editing(&dim(&[LEFT, CENTER_LR, RIGHT], r#", "equal": true"#));
    click_family(&mut h, [0.0, 600.0, 0.0]);
    let d = editor(&h).dimensions[0].clone();
    assert_eq!((d.refs.len(), d.equal), (2, false));
    let (text, _) = said(&h);
    assert!(text.contains("EQ"), "{text}");
    // Added back: two segments, not EQ until it is asked for again.
    click_family(&mut h, [0.0, 600.0, 0.0]);
    assert_eq!(editor(&h).dimensions[0].refs.len(), 3);
}

fn locks(h: &Harness<BimApp>) -> Vec<(usize, f64)> {
    editor(h).dimensions[0].locked.iter().map(|l| (l.segment, l.length)).collect()
}

#[test]
fn locks_are_renumbered_as_references_come_and_go() {
    let mut h = editing(&dim(&[LEFT, CENTER_LR, RIGHT], r#", "locked": [{"segment": 1, "length": 500}]"#));
    // Far goes before Left: the lock on Center to Right is segment 2 now, and back to 1 when Far goes again.
    click_family(&mut h, [-900.0, 600.0, 0.0]);
    assert_eq!(refs(&h), planes(&[FAR, LEFT, CENTER_LR, RIGHT]));
    assert_eq!(locks(&h), [(2, 500.0)]);
    click_family(&mut h, [-900.0, 600.0, 0.0]);
    assert_eq!(locks(&h), [(1, 500.0)]);
    // An end reference under a lock stays.
    click_family(&mut h, [500.0, 600.0, 0.0]);
    assert_eq!(refs(&h), planes(&[LEFT, CENTER_LR, RIGHT]));
    let (text, error) = said(&h);
    assert!(error && text.contains("locked"), "{text}");
}

#[test]
fn ew_takes_a_forms_end_and_works_in_an_elevation() {
    let mut h = editing(&dim(&[LEFT, RIGHT], ""));
    click_family(&mut h, [400.0, 0.0, 0.0]);
    let r = refs(&h);
    assert_eq!(r.len(), 3, "the bar's end, between Left and Right: {r:?}");
    assert!(matches!(r[1], DimRef::Form(..)), "{r:?}");
    // Front: the planes seen edge on, the dimension along x below the level.
    let front = format!(r#"{{"refs": [{}, {}], "at": [0, 0, -700], "view": "front"}}"#, plane(LEFT), plane(RIGHT));
    let mut h = editing_in(&front, Which::Elevation(bimcraft_core::family::views::Side::Front), [200.0, 0.0, -700.0]);
    click_family(&mut h, [0.0, 0.0, 600.0]);
    assert_eq!(refs(&h), planes(&[LEFT, CENTER_LR, RIGHT]));
}

#[test]
fn modify_delete_and_undo_leave_the_tool_as_they_should() {
    // Modify puts it down: a click on a plane then selects the plane, and the dimension is as it was.
    let mut h = editing(&dim(&[LEFT, RIGHT], ""));
    super::family_selection::command(&mut h, "Modify");
    assert!(editor(&h).views.witnessing.is_none());
    click_family(&mut h, [0.0, 600.0, 0.0]);
    assert_eq!(refs(&h), planes(&[LEFT, RIGHT]));
    // Undo takes back a click; the tool stays on.
    let mut h = editing(&dim(&[LEFT, RIGHT], ""));
    click_family(&mut h, [0.0, 600.0, 0.0]);
    press(&mut h, "Undo");
    assert_eq!(refs(&h), planes(&[LEFT, RIGHT]));
    // The dimension deleted while it is on: the next click ends it.
    let gone = editor(&h).dimensions[0].guid;
    h.state_mut().active_family_mut().unwrap().delete_dimension(gone);
    h.run();
    click_family(&mut h, [0.0, 600.0, 0.0]);
    assert!(editor(&h).views.witnessing.is_none());
}

#[test]
fn the_last_two_references_stay_and_a_coincident_one_is_refused() {
    let mut h = editing(&dim(&[LEFT, RIGHT], ""));
    click_family(&mut h, [500.0, 600.0, 0.0]);
    assert_eq!(refs(&h), planes(&[LEFT, RIGHT]));
    assert!(said(&h).0.contains("two references"));
}
