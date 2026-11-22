//! The Dimension tool while a form is sketched. Aligned, Angular and Radial dimensions on the sketch's own lines and
//! circles and to a reference plane, held with the sketch and written at Finish (one step, undone with the form); a label and
//! a value typed drive the sketch's lines; a padlock locks; a lock the constraints refuse keeps sketch mode open; Cancel drops
//! them; a line deleted takes its dimensions with it; the draft's dimensions of the form show and are edited while it is.
use super::family_views::{click_family, editor, open, view};
use super::*;
use bimcraft_core::family::views::Which;
use bimcraft_core::family::{DimKind, DimRef, Expr, FamilyDimension};
use bimcraft_core::ids::Guid;
use std::collections::BTreeMap;

const LEFT: &str = "11111111-1111-4111-8111-000000000001";
const RIGHT: &str = "11111111-1111-4111-8111-000000000002";
const SLAB: &str = "33333333-3333-4333-8333-000000000001";
const SOUTH: &str = "33333333-3333-4333-8333-000000000002";
const EAST: &str = "33333333-3333-4333-8333-000000000003";
const WEST: &str = "33333333-3333-4333-8333-000000000005";
const DISC: &str = "33333333-3333-4333-8333-000000000006";
const RING: &str = "33333333-3333-4333-8333-000000000007";
const SPAN: &str = "44444444-4444-4444-8444-000000000001";

/// A box family: Width 1000 (Small 600, Large 1400, Large previewed), Left and Right at ±500 labelled Width; a slab on Ref.
/// Level sketched as a rectangle at ±300 by ±200, and a disc of radius 80 at (0, 600); `dims`, more dimensions as JSON.
fn family(dims: &str) -> String {
    format!(
        r#"family {{"name": "Box", "category": "generic", "parameters": [{{"name": "Width", "value": 1000}}, {{"name": "R", "value": 80}}], "types": {{"Small": {{"Width": 600, "R": 50}}, "Large": {{"Width": 1400, "R": 120}}}}, "ref_planes": [{{"guid": "{LEFT}", "name": "Left", "start": [-500, -2000, 0], "end": [-500, 2000, 0], "up": [0, 0, 1]}}, {{"guid": "{RIGHT}", "name": "Right", "start": [500, -2000, 0], "end": [500, 2000, 0], "up": [0, 0, 1]}}], "forms": [{{"guid": "{SLAB}", "sketch": [{{"guid": "{SOUTH}", "start": [-300, -200], "end": [300, -200]}}, {{"guid": "{EAST}", "start": [300, -200], "end": [300, 200]}}, {{"guid": "33333333-3333-4333-8333-000000000004", "start": [300, 200], "end": [-300, 200]}}, {{"guid": "{WEST}", "start": [-300, 200], "end": [-300, -200]}}], "from": 0, "to": 100}}, {{"guid": "{DISC}", "sketch": [{{"guid": "{RING}", "centre": [0, 600], "radius": 80}}], "from": 0, "to": 50}}], "dimensions": [{{"refs": [{{"plane": "{LEFT}"}}, {{"plane": "{RIGHT}"}}], "at": [0, -900, 0], "label": "Width"}}{dims}]}}"#
    )
}

/// The slab's west and east lines measured across, below it.
fn span() -> String {
    format!(
        r#", {{"guid": "{SPAN}", "refs": [{{"form": "{SLAB}", "line": "{WEST}"}}, {{"form": "{SLAB}", "line": "{EAST}"}}], "at": [0, -450, 0]}}"#
    )
}

fn g(s: &str) -> Guid {
    serde_json::from_value(json!(s)).unwrap()
}

fn ed_mut<'a>(h: &'a mut Harness<'_, BimApp>) -> &'a mut crate::editor::FamilyEditor {
    h.state_mut().active_family_mut().unwrap()
}

fn status(h: &Harness<BimApp>) -> String {
    h.state().status.text.clone()
}

/// Form `i` sketched again in plan (Edit Extrusion), its history and Unsaved mark as they were.
fn sketching(dims: &str, i: usize) -> Harness<'static, BimApp> {
    let (mut h, _) = open(&family(dims), "Box");
    view(&mut h, Some(Which::Plan));
    ed_mut(&mut h).edit_form_sketch(i).unwrap();
    h.run();
    assert!(editor(&h).sketching());
    h
}

/// Where line `line` of the sketch is: its start and end.
fn line(h: &Harness<BimApp>, line: &str) -> (P2, P2) {
    let l = editor(h).sketch.as_ref().unwrap().sketch.lines.iter().find(|l| l.guid == g(line)).expect("the line").clone();
    (l.start(), l.end())
}

/// The dimensions made while sketching.
fn made(h: &Harness<BimApp>) -> Vec<FamilyDimension> {
    editor(h).sketch.as_ref().unwrap().dims.clone()
}

/// The slab's west and east lines dimensioned with Aligned Dimension (DI), the dimension placed below the slab; the tool
/// put down.
fn dimension_west_to_east(h: &mut Harness<BimApp>) -> FamilyDimension {
    press(h, "Aligned Dimension");
    assert!(editor(h).views.dimensioning.is_some(), "Aligned Dimension, while sketching");
    click_family(h, [-300.0, 100.0, 0.0]);
    click_family(h, [300.0, 100.0, 0.0]);
    click_family(h, [150.0, -450.0, 0.0]);
    key(h, Key::Escape);
    assert!(editor(h).views.dimensioning.is_none());
    made(h).last().cloned().expect("a dimension made")
}

/// Segment 0's padlock of the aligned dimension whose line runs level through `y`, its middle at `x`, clicked.
fn click_padlock(h: &mut Harness<BimApp>, x: f64, y: f64) {
    let at = editor(h).views.screen_of([x, y, 0.0]).unwrap() + vec2(0.0, 16.0);
    click_at(h, at);
}

#[test]
fn two_sketch_lines_dimensioned_are_written_at_finish_and_undone_with_the_form() {
    let mut h = sketching("", 0);
    let (steps, dims) = (editor(&h).steps(), editor(&h).dimensions.len());
    let d = dimension_west_to_east(&mut h);
    assert_eq!(d.refs, vec![DimRef::Sketch(g(SLAB), g(WEST)), DimRef::Sketch(g(SLAB), g(EAST))], "{}", status(&h));
    assert_eq!(editor(&h).dimensions.len(), dims, "the draft waits for Finish");
    assert_eq!(editor(&h).steps(), steps);
    press(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", status(&h));
    let saved = editor(&h).dimensions.iter().find(|x| x.guid == d.guid).cloned().expect("written with the form");
    assert_eq!(saved.refs, d.refs);
    assert_eq!(editor(&h).steps(), steps + 1, "one step");
    // Undo takes the form's sketch and the dimension back together.
    h.state_mut().family_undo();
    h.run();
    assert_eq!(editor(&h).dimensions.len(), dims);
    assert_eq!(editor(&h).steps(), steps);
}

#[test]
fn a_new_extrusion_dimensioned_is_undone_with_its_dimension() {
    let (mut h, _) = open(&family(""), "Box");
    view(&mut h, Some(Which::Plan));
    let (forms, dims) = (editor(&h).draft_family().unwrap().forms.len(), editor(&h).dimensions.len());
    h.get_by_role_and_label(Role::Button, "Extrusion").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Rectangle").click();
    h.run();
    click_family(&mut h, [-200.0, -1300.0, 0.0]);
    click_family(&mut h, [200.0, -1100.0, 0.0]);
    assert_eq!(editor(&h).sketch.as_ref().unwrap().sketch.lines.len(), 4);
    press(&mut h, "Aligned Dimension");
    click_family(&mut h, [-200.0, -1150.0, 0.0]);
    click_family(&mut h, [200.0, -1150.0, 0.0]);
    click_family(&mut h, [100.0, -1450.0, 0.0]);
    assert_eq!(made(&h).len(), 1, "{}", status(&h));
    press(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", status(&h));
    assert_eq!((editor(&h).draft_family().unwrap().forms.len(), editor(&h).dimensions.len()), (forms + 1, dims + 1));
    let form = editor(&h).draft_family().unwrap().forms.last().unwrap().guid;
    assert!(editor(&h).dimensions.last().unwrap().refs.iter().all(|r| r.names(form)));
    h.state_mut().family_undo();
    h.run();
    assert_eq!(
        (editor(&h).draft_family().unwrap().forms.len(), editor(&h).dimensions.len()),
        (forms, dims),
        "form and dimension undone together"
    );
}

#[test]
fn a_label_drives_the_sketch_lines_and_a_padlock_locks_the_dimension() {
    let mut h = sketching("", 0);
    let d = dimension_west_to_east(&mut h);
    // Selected by its line: its Label on the Options Bar, Width (Large: 1400).
    click_family(&mut h, [100.0, -450.0, 0.0]);
    assert_eq!(editor(&h).sketch.as_ref().unwrap().selected_dim, Some(d.guid), "{}", status(&h));
    h.get_by_label("Label").click();
    h.run();
    press_near(&mut h, "Width", "Label");
    assert_eq!(made(&h)[0].label.as_deref(), Some("Width"), "{}", status(&h));
    let (w, e) = (line(&h, WEST), line(&h, EAST));
    assert!(((e.0.0 - w.0.0) - 1400.0).abs() < 1e-6, "the lines moved apart to Width at once: {w:?} {e:?}");
    // Width taken off, a value typed in: the lines follow it.
    h.get_by_label("Label").click();
    h.run();
    press_near(&mut h, "<None>", "Label");
    assert_eq!(made(&h)[0].label, None);
    ed_mut(&mut h).set_dimension_value(d.guid, 800.0).unwrap();
    let (w, e) = (line(&h, WEST), line(&h, EAST));
    assert!(((e.0.0 - w.0.0) - 800.0).abs() < 1e-6, "{w:?} {e:?}");
    // Locked by its padlock, under its middle.
    let mid = (w.0.0 + e.0.0) / 2.0;
    click_padlock(&mut h, mid, -450.0);
    assert!(made(&h)[0].is_locked(0), "{}", status(&h));
    press(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", status(&h));
    let f = editor(&h).draft_family().unwrap();
    let at = f.dimensions.iter().position(|x| x.guid == d.guid).unwrap();
    for t in ["Small", "Large"] {
        let v = f.values(t, &BTreeMap::new()).unwrap();
        assert_eq!(f.measured(&v).unwrap()[at], vec![800.0], "{t}: locked at 800");
    }
}

#[test]
fn a_lock_the_constraints_refuse_keeps_sketch_mode_open_and_cancel_drops_the_dimensions() {
    let mut h = sketching("", 0);
    let (steps, unsaved, before) = (editor(&h).steps(), editor(&h).unsaved(), editor(&h).draft_family().unwrap());
    dimension_west_to_east(&mut h);
    click_family(&mut h, [100.0, -450.0, 0.0]);
    click_padlock(&mut h, 0.0, -450.0);
    assert!(made(&h)[0].is_locked(0), "{}", status(&h));
    // Align locks the west line to Left and the east one to Right: with the span locked at 600, Width can't be 1400.
    key(&mut h, Key::Escape);
    for (plane, side) in [(-700.0, -300.0), (700.0, 300.0)] {
        press(&mut h, "Align");
        click_family(&mut h, [plane, 900.0, 0.0]);
        click_family(&mut h, [side, 100.0, 0.0]);
        let at = editor(&h)
            .sketch_padlocks_shown()
            .into_iter()
            .find(|(_, w)| *w == crate::editor::sketch::Padlock::Offer)
            .map(|(p, _)| p)
            .expect("Align's padlock");
        click_at(&mut h, at);
    }
    assert_eq!(editor(&h).sketch.as_ref().unwrap().locks.len(), 2);
    press(&mut h, "Finish Edit Mode");
    assert!(editor(&h).sketching(), "refused: sketch mode stays open");
    assert!(status(&h).contains("Large"), "{}", status(&h));
    assert_eq!(editor(&h).draft_family().unwrap(), before, "the draft as it was");
    assert_eq!(editor(&h).steps(), steps);
    // Cancel: the dimension and the locks dropped, no step, no Unsaved mark.
    press(&mut h, "Cancel Edit Mode");
    assert!(!editor(&h).sketching());
    assert_eq!(editor(&h).draft_family().unwrap(), before);
    assert_eq!((editor(&h).steps(), editor(&h).unsaved()), (steps, unsaved));
}

#[test]
fn a_label_made_by_create_parameter_while_sketching_goes_with_cancel() {
    let mut h = sketching("", 0);
    let params = editor(&h).draft_family().unwrap().parameters.len();
    let d = dimension_west_to_east(&mut h);
    click_family(&mut h, [100.0, -450.0, 0.0]);
    h.get_by_label("Label").click();
    h.run();
    press_near(&mut h, "Create Parameter…", "Label");
    type_into(&mut h, "Name", "Span");
    press_near(&mut h, "OK", "Name");
    assert_eq!(made(&h)[0].label.as_deref(), Some("Span"), "{}", status(&h));
    assert_eq!(editor(&h).draft_family().unwrap().parameters.len(), params, "held with the sketch");
    assert_eq!(made(&h)[0].guid, d.guid);
    press(&mut h, "Cancel Edit Mode");
    assert_eq!(editor(&h).draft_family().unwrap().parameters.len(), params, "dropped");
    // Made again and finished: the parameter comes with the form.
    ed_mut(&mut h).edit_form_sketch(0).unwrap();
    h.run();
    dimension_west_to_east(&mut h);
    click_family(&mut h, [100.0, -450.0, 0.0]);
    h.get_by_label("Label").click();
    h.run();
    press_near(&mut h, "Create Parameter…", "Label");
    type_into(&mut h, "Name", "Span");
    press_near(&mut h, "OK", "Name");
    press(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", status(&h));
    let f = editor(&h).draft_family().unwrap();
    assert!(f.parameters.iter().any(|p| p.name == "Span"));
    assert!(f.dimensions.iter().any(|x| x.label.as_deref() == Some("Span")));
}

#[test]
fn a_sketch_line_deleted_takes_its_dimensions_with_it() {
    let mut h = sketching(&span(), 0);
    let mine = dimension_west_to_east(&mut h);
    let names_west = |h: &Harness<BimApp>| {
        editor(h).sketch_dimensions().iter().any(|d| d.refs.iter().any(|r| matches!(r, DimRef::Sketch(_, l) if *l == g(WEST))))
    };
    assert!(names_west(&h));
    // The west line picked and deleted: both dimensions that measure to it go, the one made now and the draft's.
    click_family(&mut h, [-300.0, 100.0, 0.0]);
    assert!(!editor(&h).sketch.as_ref().unwrap().sketch.picked.is_empty(), "picked");
    key(&mut h, Key::Delete);
    assert!(!names_west(&h));
    assert!(editor(&h).sketch_dimensions().iter().all(|d| d.guid != mine.guid && d.guid != g(SPAN)));
    // Drawn again and finished: a new line, and no dimension of the old one.
    h.get_by_role_and_label(Role::Button, "Line").click();
    h.run();
    click_family(&mut h, [-300.0, 200.0, 0.0]);
    click_family(&mut h, [-300.0, -200.0, 0.0]);
    key(&mut h, Key::Escape);
    press(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", status(&h));
    assert!(editor(&h).dimensions.iter().all(|d| !d.refs.iter().any(|r| r.names(g(WEST)))));
    assert!(editor(&h).dimensions.iter().all(|d| d.guid != g(SPAN)));
}

#[test]
fn radial_dimension_on_a_sketch_circle_and_its_label_drives_the_radius() {
    let mut h = sketching("", 1);
    h.get_by_role_and_label(Role::Button, "Radial").click();
    h.run();
    click_family(&mut h, [80.0, 600.0, 0.0]);
    click_family(&mut h, [300.0, 700.0, 0.0]);
    let r = made(&h).last().cloned().expect("a radial dimension");
    assert_eq!((r.kind, r.refs.clone()), (DimKind::Radial, vec![DimRef::Sketch(g(DISC), g(RING))]), "{}", status(&h));
    key(&mut h, Key::Escape);
    ed_mut(&mut h).set_label(r.guid, Some("R".into())).unwrap();
    let ring = editor(&h).sketch.as_ref().unwrap().sketch.lines.iter().find(|l| l.guid == g(RING)).unwrap().clone();
    match ring.curve {
        bimcraft_core::curves::Curve::Circle { radius, .. } => assert!((radius - 120.0).abs() < 1e-6, "Large's R: {radius}"),
        c => panic!("a circle: {c:?}"),
    }
    press(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", status(&h));
    let f = editor(&h).draft_family().unwrap();
    let v = f.values("Small", &BTreeMap::new()).unwrap();
    let at = f.dimensions.iter().position(|d| d.guid == r.guid).unwrap();
    assert_eq!(f.measured(&v).unwrap()[at], vec![50.0], "the radius flexes with R");
}

#[test]
fn angular_dimension_between_two_sketch_lines() {
    let mut h = sketching("", 0);
    press(&mut h, "Angular");
    assert_eq!(editor(&h).views.dim_kind, DimKind::Angular);
    click_family(&mut h, [100.0, -200.0, 0.0]);
    click_family(&mut h, [300.0, 100.0, 0.0]);
    click_family(&mut h, [200.0, -120.0, 0.0]);
    let d = made(&h).last().cloned().expect("an angular dimension");
    assert_eq!(d.kind, DimKind::Angular);
    assert_eq!(d.refs, vec![DimRef::Sketch(g(SLAB), g(SOUTH)), DimRef::Sketch(g(SLAB), g(EAST))], "{}", status(&h));
    press(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", status(&h));
    let f = editor(&h).draft_family().unwrap();
    let v = f.values("Large", &BTreeMap::new()).unwrap();
    let at = f.dimensions.iter().position(|x| x.guid == d.guid).unwrap();
    assert_eq!(f.measured(&v).unwrap()[at], vec![90.0]);
}

#[test]
fn a_reference_plane_dimensioned_to_a_sketch_line_while_sketching() {
    let mut h = sketching("", 0);
    press(&mut h, "Aligned Dimension");
    // Left, where Large (previewed) has it, then the west line.
    click_family(&mut h, [-700.0, 900.0, 0.0]);
    click_family(&mut h, [-300.0, 100.0, 0.0]);
    click_family(&mut h, [-500.0, -600.0, 0.0]);
    let d = made(&h).last().cloned().expect("a dimension");
    assert_eq!(d.refs, vec![DimRef::Plane(g(LEFT)), DimRef::Sketch(g(SLAB), g(WEST))], "{}", status(&h));
    press(&mut h, "Finish Edit Mode");
    assert!(editor(&h).dimensions.iter().any(|x| x.guid == d.guid), "{}", status(&h));
}

#[test]
fn the_drafts_dimensions_of_the_form_show_and_are_edited_while_it_is_sketched() {
    let mut h = sketching(&span(), 0);
    // The span, the draft's, is there to select by its line (with Modify: Escape puts Line down), and its padlock locks it:
    // held with the sketch.
    key(&mut h, Key::Escape);
    click_family(&mut h, [100.0, -450.0, 0.0]);
    assert_eq!(editor(&h).sketch.as_ref().unwrap().selected_dim, Some(g(SPAN)), "{}", status(&h));
    click_padlock(&mut h, 0.0, -450.0);
    assert!(editor(&h).sketch.as_ref().unwrap().changed.iter().any(|d| d.guid == g(SPAN) && d.is_locked(0)), "{}", status(&h));
    assert!(!editor(&h).dimensions.iter().any(|d| d.guid == g(SPAN) && d.is_locked(0)), "the draft waits");
    press(&mut h, "Finish Edit Mode");
    assert!(editor(&h).dimensions.iter().any(|d| d.guid == g(SPAN) && d.is_locked(0)), "{}", status(&h));
    // Selected while sketching, Delete takes it off, at Finish.
    ed_mut(&mut h).edit_form_sketch(0).unwrap();
    h.run();
    key(&mut h, Key::Escape);
    click_family(&mut h, [100.0, -450.0, 0.0]);
    key(&mut h, Key::Delete);
    assert!(editor(&h).dimensions.iter().any(|d| d.guid == g(SPAN)));
    assert_eq!(editor(&h).sketch.as_ref().unwrap().sketch.lines.len(), 4, "no line deleted");
    press(&mut h, "Finish Edit Mode");
    assert!(editor(&h).dimensions.iter().all(|d| d.guid != g(SPAN)), "{}", status(&h));
}

/// Find lists Aligned Dimension while a form is sketched and runs it, Escape putting it down; Find's Delete takes the dimension
/// selected while sketching, as the Delete key does, and leaves the lines.
#[test]
fn find_runs_aligned_dimension_and_delete_while_sketching() {
    let mut h = sketching(&span(), 0);
    assert!(h.state().family_find_actions().contains(&"Aligned Dimension"));
    assert!(h.state_mut().family_find_action("Aligned Dimension"));
    h.run();
    assert!(editor(&h).views.dimensioning.is_some() && editor(&h).views.dim_kind == DimKind::Aligned);
    key(&mut h, Key::Escape);
    assert!(editor(&h).views.dimensioning.is_none());
    key(&mut h, Key::Escape);
    click_family(&mut h, [100.0, -450.0, 0.0]);
    assert_eq!(editor(&h).sketch.as_ref().unwrap().selected_dim, Some(g(SPAN)), "{}", status(&h));
    assert!(h.state_mut().family_find_action("Delete"));
    h.run();
    assert_eq!(editor(&h).sketch.as_ref().unwrap().sketch.lines.len(), 4, "no line deleted");
    press(&mut h, "Finish Edit Mode");
    assert!(editor(&h).dimensions.iter().all(|d| d.guid != g(SPAN)), "{}", status(&h));
}

#[test]
fn no_dimension_tool_while_sketching_in_3d() {
    let mut h = sketching("", 0);
    view(&mut h, None);
    press(&mut h, "Aligned Dimension");
    assert!(editor(&h).views.dimensioning.is_none(), "not in {{3D}}");
    assert!(editor(&h).sketch_undimensionable().is_some());
}

const SWEEP: &str = "55555555-5555-4555-8555-000000000001";
const BOTTOM: &str = "55555555-5555-4555-8555-000000000002";
const TOP: &str = "55555555-5555-4555-8555-000000000003";

/// A sweep along y on Ref. Level, its profile a 100 square on its plane (x across, z up), and a Depth parameter (Small 80,
/// Large 200, Large previewed).
fn sweep_family() -> String {
    format!(
        r#"family {{"name": "Sw", "category": "generic", "parameters": [{{"name": "Depth", "value": 100}}], "types": {{"Small": {{"Depth": 80}}, "Large": {{"Depth": 200}}}}, "forms": [{{"guid": "{SWEEP}", "sketch": [{{"start": [0, -400], "end": [0, 400]}}], "sweep": {{"profile": [{{"guid": "{BOTTOM}", "start": [-50, 0], "end": [50, 0]}}, {{"start": [50, 0], "end": [50, 100]}}, {{"guid": "{TOP}", "start": [50, 100], "end": [-50, 100]}}, {{"start": [-50, 100], "end": [-50, 0]}}]}}}}]}}"#
    )
}

/// A sweep's profile, shown in an elevation that sees it flat, dimensioned and labelled while sketching.
fn sweep_profile_dimensioned(h: &mut Harness<BimApp>) -> FamilyDimension {
    ed_mut(h).edit_form_sketch(0).unwrap();
    h.run();
    ed_mut(h).show_part(crate::editor::sketch::Part::Profile).unwrap();
    h.run();
    h.run();
    assert!(editor(h).views.shown.is_some_and(|w| w != Which::Plan), "an elevation that sees the profile flat");
    assert!(editor(h).sketch_undimensionable().is_none());
    press(h, "Aligned Dimension");
    click_family(h, [20.0, 0.0, 0.0]);
    click_family(h, [20.0, 0.0, 100.0]);
    click_family(h, [300.0, 0.0, 50.0]);
    key(h, Key::Escape);
    let d = made(h).last().cloned().expect("a dimension of the profile");
    assert_eq!(d.refs, vec![DimRef::Sketch(g(SWEEP), g(BOTTOM)), DimRef::Sketch(g(SWEEP), g(TOP))], "{}", status(h));
    ed_mut(h).set_label(d.guid, Some("Depth".into())).unwrap();
    d
}

#[test]
fn a_sweeps_profile_is_dimensioned_and_labelled_while_sketched() {
    let (mut h, _) = open(&sweep_family(), "Sw");
    view(&mut h, Some(Which::Plan));
    let d = sweep_profile_dimensioned(&mut h);
    let (b, t) = (line(&h, BOTTOM), line(&h, TOP));
    assert!(((t.0.1 - b.0.1) - 200.0).abs() < 1e-6, "Large's Depth, at once: {b:?} {t:?}");
    press(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", status(&h));
    let f = editor(&h).draft_family().unwrap();
    let at = f.dimensions.iter().position(|x| x.guid == d.guid).expect("written");
    let v = f.values("Small", &BTreeMap::new()).unwrap();
    assert_eq!(f.measured(&v).unwrap()[at], vec![80.0]);
}

/// A sketch in plan with an aligned dimension between two of its lines, labelled and locked, selected; and a disc's sketch
/// with a radial dimension. Saved where `BIMCRAFT_SKETCH_DIMENSIONS_DUMP` says (only then).
#[test]
fn family_sketch_dimensions_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_SKETCH_DIMENSIONS_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let mut h = app(&family(""));
    h.set_size(vec2(1600.0, 1000.0));
    h.state_mut().edit_family("Box");
    h.run();
    view(&mut h, Some(Which::Plan));
    h.run();
    ed_mut(&mut h).edit_form_sketch(0).unwrap();
    h.run();
    let d = dimension_west_to_east(&mut h);
    click_family(&mut h, [100.0, -450.0, 0.0]);
    ed_mut(&mut h).set_label(d.guid, Some("Width".into())).unwrap();
    h.run();
    let (w, e) = (line(&h, WEST), line(&h, EAST));
    click_padlock(&mut h, (w.0.0 + e.0.0) / 2.0, -450.0);
    h.hover_at(egui::pos2(1500.0, 900.0));
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("fe12-sketch-aligned.png")).unwrap();
    press(&mut h, "Cancel Edit Mode");
    ed_mut(&mut h).edit_form_sketch(1).unwrap();
    h.run();
    h.get_by_role_and_label(Role::Button, "Radial").click();
    h.run();
    click_family(&mut h, [80.0, 600.0, 0.0]);
    click_family(&mut h, [300.0, 750.0, 0.0]);
    key(&mut h, Key::Escape);
    // Selected by its leader, labelled R: its padlock shows.
    let r = made(&h).last().cloned().unwrap();
    ed_mut(&mut h).set_label(r.guid, Some("R".into())).unwrap();
    ed_mut(&mut h).sketch.as_mut().unwrap().selected_dim = Some(r.guid);
    h.hover_at(egui::pos2(1500.0, 900.0));
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("fe12-sketch-radial.png")).unwrap();
    // A sweep's profile in an elevation, its depth labelled, selected.
    let mut h = app(&sweep_family());
    h.set_size(vec2(1600.0, 1000.0));
    h.state_mut().edit_family("Sw");
    h.run();
    view(&mut h, Some(Which::Plan));
    h.run();
    let d = sweep_profile_dimensioned(&mut h);
    ed_mut(&mut h).sketch.as_mut().unwrap().selected_dim = Some(d.guid);
    h.hover_at(egui::pos2(1500.0, 900.0));
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("fe12-sketch-profile.png")).unwrap();
}

const BOX: &str = "66666666-6666-4666-8666-000000000001";
const BOX_BOTTOM: &str = "66666666-6666-4666-8666-000000000002";
const BOX_TOP: &str = "66666666-6666-4666-8666-000000000003";
const BOX_SPAN: &str = "66666666-6666-4666-8666-000000000004";

/// A box whose east corners are formulas of `Width`, its bottom and top 300 apart; `Depth` 300 (T: 500); `dims`, dimensions.
fn formula_box(dims: &str) -> String {
    format!(
        r#"family {{"name": "Fx", "category": "generic", "parameters": [{{"name": "Width", "value": 400}}, {{"name": "Depth", "value": 300}}], "types": {{"T": {{"Depth": 500}}}}, "forms": [{{"guid": "{BOX}", "sketch": [{{"guid": "{BOX_BOTTOM}", "start": [0, 0], "end": ["Width", 0]}}, {{"start": ["Width", 0], "end": ["Width", 300]}}, {{"guid": "{BOX_TOP}", "start": ["Width", 300], "end": [0, 300]}}, {{"start": [0, 300], "end": [0, 0]}}], "from": 0, "to": 100}}], "dimensions": [{dims}]}}"#
    )
}

#[test]
fn a_label_set_while_sketching_keeps_the_lines_formulas_as_one_set_after_finish() {
    // Labelled in sketch mode, then finished.
    let (mut h, _) = open(&formula_box(""), "Fx");
    view(&mut h, Some(Which::Plan));
    ed_mut(&mut h).edit_form_sketch(0).unwrap();
    h.run();
    press(&mut h, "Aligned Dimension");
    click_family(&mut h, [200.0, 0.0, 0.0]);
    click_family(&mut h, [200.0, 300.0, 0.0]);
    click_family(&mut h, [-200.0, 150.0, 0.0]);
    key(&mut h, Key::Escape);
    let d = made(&h).last().cloned().expect("a dimension");
    assert_eq!(d.refs, vec![DimRef::Sketch(g(BOX), g(BOX_BOTTOM)), DimRef::Sketch(g(BOX), g(BOX_TOP))], "{}", status(&h));
    ed_mut(&mut h).set_label(d.guid, Some("Depth".into())).unwrap();
    let (b, t) = (line(&h, BOX_BOTTOM), line(&h, BOX_TOP));
    assert!(((t.0.1 - b.0.1) - 500.0).abs() < 1e-6, "shown at Depth at once: {b:?} {t:?}");
    press(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", status(&h));
    let in_sketch = editor(&h).draft_family().unwrap().forms[0].sketch.clone();
    assert!(
        in_sketch.iter().filter(|l| l.exprs().contains(&&Expr::Formula("Width".into()))).count() >= 2,
        "the lines still follow Width: {in_sketch:?}"
    );
    // Labelled outside sketch mode: the same lines.
    let span = format!(
        r#"{{"guid": "{BOX_SPAN}", "refs": [{{"form": "{BOX}", "line": "{BOX_BOTTOM}"}}, {{"form": "{BOX}", "line": "{BOX_TOP}"}}], "at": [-200, 150, 0]}}"#
    );
    let (mut h2, _) = open(&formula_box(&span), "Fx");
    view(&mut h2, Some(Which::Plan));
    ed_mut(&mut h2).set_label(g(BOX_SPAN), Some("Depth".into())).unwrap();
    let outside = editor(&h2).draft_family().unwrap().forms[0].sketch.clone();
    let strip = |ls: &[bimcraft_core::family::sketch::FormLine]| ls.iter().map(|l| l.curve.clone()).collect::<Vec<_>>();
    assert_eq!(strip(&in_sketch), strip(&outside));
}

const REV: &str = "77777777-7777-4777-8777-000000000001";
const AXIS_LINE: &str = "77777777-7777-4777-8777-000000000002";
const POST: &str = "77777777-7777-4777-8777-000000000003";
const TO_AXIS: &str = "77777777-7777-4777-8777-000000000004";

/// A revolve on Ref. Level, its profile from x 100 to 300, its axis upright at x 50; a plane Post at x -400 and a dimension
/// from it to the axis.
fn revolve() -> String {
    format!(
        r#"family {{"name": "Rv", "category": "generic", "types": {{"T": {{}}}}, "ref_planes": [{{"guid": "{POST}", "name": "Post", "start": [-400, -2000, 0], "end": [-400, 2000, 0], "up": [0, 0, 1]}}], "forms": [{{"guid": "{REV}", "sketch": [{{"start": [100, -200], "end": [300, -200]}}, {{"start": [300, -200], "end": [300, 200]}}, {{"start": [300, 200], "end": [100, 200]}}, {{"start": [100, 200], "end": [100, -200]}}, {{"guid": "{AXIS_LINE}", "start": [50, -500], "end": [50, 500], "axis": true}}], "from": 0, "to": 360}}], "dimensions": [{{"guid": "{TO_AXIS}", "refs": [{{"plane": "{POST}"}}, {{"form": "{REV}", "line": "{AXIS_LINE}"}}], "at": [0, -800, 0]}}]}}"#
    )
}

#[test]
fn a_revolves_axis_drawn_again_keeps_its_dimensions_and_takes_new_ones() {
    let (mut h, _) = open(&revolve(), "Rv");
    view(&mut h, Some(Which::Plan));
    ed_mut(&mut h).edit_form_sketch(0).unwrap();
    h.run();
    h.get_by_role_and_label(Role::Button, "Axis Line").click();
    h.run();
    click_family(&mut h, [50.0, -700.0, 0.0]);
    click_family(&mut h, [50.0, 700.0, 0.0]);
    key(&mut h, Key::Escape);
    // A dimension from Post to the axis drawn again.
    press(&mut h, "Aligned Dimension");
    click_family(&mut h, [-400.0, 900.0, 0.0]);
    click_family(&mut h, [50.0, 600.0, 0.0]);
    click_family(&mut h, [-200.0, 1000.0, 0.0]);
    let d = made(&h).last().cloned().expect("a dimension to the axis");
    press(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", status(&h));
    let f = editor(&h).draft_family().unwrap();
    let axis = f.forms[0].axis_line().expect("its axis").guid;
    for want in [g(TO_AXIS), d.guid] {
        let dim = f.dimensions.iter().find(|x| x.guid == want).unwrap_or_else(|| panic!("dimension {want} kept: {:?}", f.dimensions));
        assert_eq!(dim.refs[1], DimRef::Sketch(g(REV), axis), "it names the axis the form has");
    }
    // The axis deleted and drawn anew: a new line, which a dimension placed on it names at Finish; those of the old one go.
    ed_mut(&mut h).edit_form_sketch(0).unwrap();
    h.run();
    key(&mut h, Key::Escape);
    click_family(&mut h, [50.0, 600.0, 0.0]);
    key(&mut h, Key::Delete);
    h.get_by_role_and_label(Role::Button, "Axis Line").click();
    h.run();
    click_family(&mut h, [50.0, -700.0, 0.0]);
    click_family(&mut h, [50.0, 700.0, 0.0]);
    key(&mut h, Key::Escape);
    press(&mut h, "Aligned Dimension");
    click_family(&mut h, [-400.0, 900.0, 0.0]);
    click_family(&mut h, [50.0, 600.0, 0.0]);
    click_family(&mut h, [-200.0, 1100.0, 0.0]);
    let e = made(&h).last().cloned().expect("a dimension to the new axis");
    press(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", status(&h));
    let f = editor(&h).draft_family().unwrap();
    let axis = f.forms[0].axis_line().expect("its axis").guid;
    assert_ne!(axis, g(AXIS_LINE), "a new line");
    let dim = f.dimensions.iter().find(|x| x.guid == e.guid).expect("kept");
    assert_eq!(dim.refs[1], DimRef::Sketch(g(REV), axis));
    assert!(f.dimensions.iter().all(|x| x.guid != g(TO_AXIS)), "the old axis's went with it");
}

#[test]
fn ctrl_z_in_a_sketch_takes_back_its_dimensions_with_its_lines() {
    let mut h = sketching("", 0);
    let d = dimension_west_to_east(&mut h);
    click_family(&mut h, [100.0, -450.0, 0.0]);
    assert_eq!(editor(&h).sketch.as_ref().unwrap().selected_dim, Some(d.guid));
    ed_mut(&mut h).set_label(d.guid, Some("Width".into())).unwrap();
    h.run();
    assert!(((line(&h, EAST).0.0 - line(&h, WEST).0.0) - 1400.0).abs() < 1e-6);
    // Ctrl+Z: the label and the lines it moved, together.
    h.state_mut().family_undo();
    h.run();
    assert_eq!(made(&h)[0].label, None, "the label taken back");
    assert_eq!((line(&h, WEST).0.0, line(&h, EAST).0.0), (-300.0, 300.0), "the lines where they were");
    // Again: the dimension, and its selection with it.
    h.state_mut().family_undo();
    h.run();
    assert!(made(&h).is_empty(), "the dimension taken back");
    assert_eq!(editor(&h).sketch.as_ref().unwrap().selected_dim, None, "nothing stale selected");
    // A parameter Create Parameter… made goes with its step too.
    let d = dimension_west_to_east(&mut h);
    click_family(&mut h, [100.0, -450.0, 0.0]);
    h.get_by_label("Label").click();
    h.run();
    press_near(&mut h, "Create Parameter…", "Label");
    type_into(&mut h, "Name", "Span");
    press_near(&mut h, "OK", "Name");
    assert_eq!(made(&h)[0].label.as_deref(), Some("Span"), "{}", status(&h));
    h.state_mut().family_undo();
    h.run();
    let s = editor(&h).sketch.as_ref().unwrap();
    assert!(
        editor(&h).sketch_params().is_empty() && s.dims.iter().all(|x| x.guid != d.guid || x.label.is_none()),
        "the parameter taken back"
    );
}

#[test]
fn a_draft_dimension_missing_a_line_is_not_changed_and_comes_back_whole_with_it() {
    let string = format!(
        r#", {{"guid": "{SPAN}", "refs": [{{"plane": "{LEFT}"}}, {{"form": "{SLAB}", "line": "{WEST}"}}, {{"form": "{SLAB}", "line": "{EAST}"}}], "at": [0, -450, 0]}}"#
    );
    let mut h = sketching(&string, 0);
    key(&mut h, Key::Escape);
    click_family(&mut h, [-300.0, 100.0, 0.0]);
    key(&mut h, Key::Delete);
    let shown = editor(&h).sketch_dimensions().into_iter().find(|d| d.guid == g(SPAN)).expect("the string, less the line");
    assert_eq!(shown.refs.len(), 2);
    assert!(ed_mut(&mut h).toggle_equal(g(SPAN)).is_err(), "refused while its line is gone");
    assert!(editor(&h).sketch.as_ref().unwrap().changed.is_empty(), "its references kept as they were");
    h.state_mut().family_undo();
    h.run();
    let back = editor(&h).sketch_dimensions().into_iter().find(|d| d.guid == g(SPAN)).unwrap();
    assert_eq!(back.refs.len(), 3, "whole again with its line");
}

#[test]
fn a_value_that_would_move_a_plane_rather_than_the_sketch_is_refused() {
    // The west line Align-locked to a plane Inner at x -300: a value from Left to it can only move a plane.
    const INNER: &str = "11111111-1111-4111-8111-000000000009";
    let script = family(&format!(
        r#", {{"refs": [{{"plane": "{INNER}"}}, {{"form": "{SLAB}", "line": "{WEST}"}}], "at": [-300, 0, 0], "locked": [{{"segment": 0, "length": 0}}], "alignment": true}}"#
    ))
    .replace(
        r#""ref_planes": ["#,
        &format!(r#""ref_planes": [{{"guid": "{INNER}", "name": "Inner", "start": [-300, -2000, 0], "end": [-300, 2000, 0], "up": [0, 0, 1]}}, "#),
    );
    let (mut h, _) = open(&script, "Box");
    view(&mut h, Some(Which::Plan));
    ed_mut(&mut h).edit_form_sketch(0).unwrap();
    h.run();
    press(&mut h, "Aligned Dimension");
    click_family(&mut h, [-700.0, 900.0, 0.0]);
    click_family(&mut h, [-300.0, 100.0, 0.0]);
    click_family(&mut h, [-500.0, -600.0, 0.0]);
    let d = made(&h).last().cloned().expect("a dimension");
    assert_eq!(d.refs, vec![DimRef::Plane(g(LEFT)), DimRef::Sketch(g(SLAB), g(WEST))], "{}", status(&h));
    let before = line(&h, WEST);
    let e = ed_mut(&mut h).set_dimension_value(d.guid, 300.0).expect_err("refused");
    assert!(e.contains("plane"), "{e}");
    assert_eq!(line(&h, WEST), before, "nothing moved");
}
