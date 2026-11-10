//! FE-10: Align onto a reference that isn't parallel. A slanted reference plane aligned to Center (Left/Right) turns onto it
//! (a form drawn on it turning with it), a reference line turns about where it crosses the reference, a plane Align-locked to
//! a form's line carries that line, a sketch line turns in sketch mode, and a pivoted plane or line turns about its pivot
//! (its own angle, or its parameter). What a dimension holds, and a form's face, are refused in words that say why.
use super::family_views::{click_family, editor, open, view};
use super::*;
use bimcraft_core::family::views::Which;
use bimcraft_core::family::{CENTER_LEFT_RIGHT, DimRef, Family, Val};
use bimcraft_core::ids::Guid;

const SLANT: &str = "77777777-7777-4777-8777-000000000001";
const EAST: &str = "77777777-7777-4777-8777-000000000002";
const HELD: &str = "77777777-7777-4777-8777-000000000003";
const BOX: &str = "77777777-7777-4777-8777-000000000004";
const BOX_EAST: &str = "77777777-7777-4777-8777-000000000005";
const PANE: &str = "77777777-7777-4777-8777-000000000006";
const LINE: &str = "77777777-7777-4777-8777-000000000007";
const CFB: &str = "9e3c0a10-1f0b-4d8c-8a5e-0000000000a2";

fn g(s: &str) -> Guid {
    serde_json::from_value(json!(s)).unwrap()
}

/// Slant, a plane from (300, -1000) to (600, 1000) (x = 450 + 0.15 y), a pane drawn on it; East at x = 500, a box's east line
/// Align-locked to it; Held at y = -1200, Depth from Center (Front/Back); and a reference line from (-800, -200) to (-300, 400).
fn family() -> String {
    format!(
        r#"family {{"name": "Turns", "category": "generic", "parameters": [{{"name": "Depth", "value": 1200}}], "types": {{"T": {{}}}}, "ref_planes": [{{"guid": "{SLANT}", "name": "Slant", "start": [300, -1000, 0], "end": [600, 1000, 0], "up": [0, 0, 1]}}, {{"guid": "{EAST}", "name": "East", "start": [500, -1500, 0], "end": [500, 1500, 0], "up": [0, 0, 1]}}, {{"guid": "{HELD}", "name": "Held", "start": [-1500, -1200, 0], "end": [1500, -1200, 0], "up": [0, 0, 1]}}], "ref_lines": [{{"guid": "{LINE}", "start": [-800, -200, 0], "end": [-300, 400, 0]}}], "forms": [{{"guid": "{BOX}", "sketch": [{{"start": [-300, -200], "end": [500, -200]}}, {{"guid": "{BOX_EAST}", "start": [500, -200], "end": [500, 200]}}, {{"start": [500, 200], "end": [-300, 200]}}, {{"start": [-300, 200], "end": [-300, -200]}}], "to": 100}}, {{"guid": "{PANE}", "host": {{"plane": "{SLANT}"}}, "profile": [[200, 0], [600, 0], [600, 300], [200, 300]], "to": 50}}], "dimensions": [{{"refs": [{{"plane": "{CFB}"}}, {{"plane": "{HELD}"}}], "at": [1200, -600, 0], "label": "Depth"}}, {{"refs": [{{"plane": "{EAST}"}}, {{"form": "{BOX}", "line": "{BOX_EAST}"}}], "at": [500, 0, 0], "locked": [{{"segment": 0, "length": 0}}], "alignment": true}}]}}"#
    )
}

fn in_plan() -> Harness<'static, BimApp> {
    let (mut h, _) = open(&family(), "Turns");
    view(&mut h, Some(Which::Plan));
    h
}

fn status(h: &Harness<BimApp>) -> String {
    format!("{} / {:?}", h.state().status.text, editor(h).views.status)
}

fn flexed(h: &Harness<BimApp>) -> Family {
    let f = editor(h).draft_family().unwrap();
    f.flexed(&editor(h).values()).unwrap().into_owned()
}

fn plane(h: &Harness<BimApp>, s: &str) -> ([f64; 3], [f64; 3]) {
    let p = editor(h).planes.iter().find(|p| p.guid == g(s)).cloned().unwrap();
    (p.start, p.end)
}

/// Whether (x, y) lies on Slant's line, x = 450 + 0.15 y (to a micron, as a sketch rounds).
fn on_slant(p: [f64; 2]) -> bool {
    (p[0] - 450.0 - 0.15 * p[1]).abs() < 2e-3
}

/// Align (AL): a click at `reference`, then at `target` (family points).
fn align(h: &mut Harness<BimApp>, reference: [f64; 3], target: [f64; 3]) {
    shortcut(h, "AL");
    click_family(h, reference);
    assert!(matches!(editor(h).views.aligning, Some(Some(_))), "a reference at {reference:?}: {}", status(h));
    click_family(h, target);
}

/// The padlock Align offers, clicked.
fn lock(h: &mut Harness<BimApp>) {
    assert!(editor(h).views.align_lock.is_some(), "the padlock offered: {}", status(h));
    h.run();
    let at = editor(h).views.offer_shown.expect("the padlock shown");
    click_at(h, at);
    key(h, Key::Escape);
}

/// The pane's corners in the family's axes, as drawn on Slant.
fn pane(h: &Harness<BimApp>) -> Vec<[f64; 3]> {
    let f = flexed(h);
    let form = f.forms.iter().find(|x| x.guid == g(PANE)).unwrap();
    let frame = f.frame(&form.host).unwrap();
    let ls = super::family_align_forms::lines_of(&f, &editor(h).values(), 1);
    ls.iter().map(|(_, a, _)| frame.point(bimcraft_core::geom::P2(a[0], a[1]), 0.0)).collect()
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[test]
fn a_slanted_plane_aligned_to_center_left_right_turns_onto_it_with_what_is_drawn_on_it() {
    let mut h = in_plan();
    let (s0, e0) = plane(&h, SLANT);
    let pane0: Vec<(f64, f64)> = pane(&h).iter().map(|p| (dist(*p, s0), dist(*p, e0))).collect();
    // Center (Left/Right), then Slant: it doesn't reach x = 0, so it turns about its start, the end nearer, and slides on.
    align(&mut h, [0.0, 1300.0, 0.0], [450.0, 0.0, 0.0]);
    let (s, e) = plane(&h, SLANT);
    assert!(s[0].abs() < 1e-6 && e[0].abs() < 1e-6, "on Center (Left/Right): {s:?} {e:?} {}", status(&h));
    assert!((s[1] + 1000.0).abs() < 1e-6 && (dist(s, e) - dist(s0, e0)).abs() < 1e-6, "its start slid square, its length kept");
    // The pane drawn on it turned and slid with it: each corner as far from its ends as before.
    let pane1: Vec<(f64, f64)> = pane(&h).iter().map(|p| (dist(*p, s), dist(*p, e))).collect();
    for (a, b) in pane0.iter().zip(&pane1) {
        assert!((a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6, "{pane0:?} {pane1:?}");
    }
    lock(&mut h);
    let locks: Vec<_> = editor(&h).dimensions.iter().filter(|d| d.alignment).map(|d| d.refs.clone()).collect();
    assert!(locks.contains(&vec![DimRef::Plane(CENTER_LEFT_RIGHT), DimRef::Plane(g(SLANT))]), "{locks:?}");
}

#[test]
fn a_reference_line_turns_about_where_it_crosses_the_reference() {
    let mut h = in_plan();
    align(&mut h, [-1300.0, 0.0, 0.0], [-550.0, 100.0, 0.0]);
    let l = editor(&h).lines.iter().find(|l| l.guid == g(LINE)).cloned().unwrap();
    assert!(l.start[1].abs() < 1e-6 && l.end[1].abs() < 1e-6, "on Center (Front/Back): {l:?} {}", status(&h));
    // About (-633.33, 0), where it crossed: that point stays on it, and its length is kept.
    let len = (l.end[0] - l.start[0]).abs();
    assert!((len - 500f64.hypot(600.0)).abs() < 1e-6, "{l:?}");
    let third = len / 3.0;
    assert!((l.start[0] + 1900.0 / 3.0 + third).abs() < 1e-6 && (l.end[0] + 1900.0 / 3.0 - 2.0 * third).abs() < 1e-6, "{l:?}");
    lock(&mut h);
}

#[test]
fn a_plane_turned_carries_the_form_line_align_locked_to_it() {
    let mut h = in_plan();
    // Slant, then East: East turns about where they cross, (500, 333.3), and the box's east line, locked to it, follows.
    align(&mut h, [570.0, 800.0, 0.0], [500.0, 1300.0, 0.0]);
    let (s, e) = plane(&h, EAST);
    assert!(on_slant([s[0], s[1]]) && on_slant([e[0], e[1]]), "East on Slant: {s:?} {e:?} {}", status(&h));
    let f = flexed(&h);
    let ls = super::family_align_forms::lines_of(&f, &editor(&h).values(), 0);
    let east = ls.iter().find(|(l, ..)| *l == g(BOX_EAST)).unwrap();
    assert!(on_slant(east.1) && on_slant(east.2), "the box's east line with it: {east:?}");
    // The lines joined to it stretched to it: the box is still closed.
    assert!(ls.iter().any(|(_, a, b)| *b == east.1 || *a == east.1) && ls.iter().any(|(_, a, _)| *a == east.2), "{ls:?}");
}

#[test]
fn what_holds_a_plane_or_a_form_s_face_is_named_when_align_can_t_turn_it() {
    let mut h = in_plan();
    // Held is labelled by Depth.
    align(&mut h, [570.0, 800.0, 0.0], [-1300.0, -1200.0, 0.0]);
    assert!(status(&h).contains("Align can't turn `Held`: dimension 1, labelled `Depth` measures to it"), "{}", status(&h));
    assert_eq!(plane(&h, HELD).0, [-1500.0, -1200.0, 0.0], "left as it was");
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    // A form's face doesn't turn.
    align(&mut h, [570.0, 800.0, 0.0], [0.0, -200.0, 0.0]);
    assert!(status(&h).contains("a form's face must be parallel to it"), "{}", status(&h));
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    // A datum doesn't either.
    align(&mut h, [570.0, 800.0, 0.0], [0.0, 1300.0, 0.0]);
    assert!(status(&h).contains("defines the family's origin and doesn't turn"), "{}", status(&h));
}

#[test]
fn a_sketch_line_turns_onto_a_slanted_plane_in_sketch_mode() {
    let mut h = in_plan();
    press(&mut h, "Extrusion");
    press(&mut h, "Rectangle");
    click_family(&mut h, [-1300.0, 600.0, 0.0]);
    click_family(&mut h, [-1000.0, 900.0, 0.0]);
    shortcut(&mut h, "AL");
    click_family(&mut h, [570.0, 800.0, 0.0]);
    click_family(&mut h, [-1000.0, 750.0, 0.0]);
    let s = editor(&h).sketch.as_ref().expect("sketching");
    let on: Vec<_> = s.sketch.lines.iter().filter(|l| on_slant([l.start().0, l.start().1]) && on_slant([l.end().0, l.end().1])).collect();
    let all: Vec<_> = s.sketch.lines.iter().map(|l| (l.start(), l.end())).collect();
    assert_eq!(on.len(), 1, "the east line on Slant: {all:?} {}", status(&h));
    // It doesn't reach Slant, so it turned about its end nearer the crossing, (-1000, 600), slid square onto Slant.
    let u = (0.15f64 / 0.15f64.hypot(1.0), 1.0 / 0.15f64.hypot(1.0));
    let along = (-1000.0 - 450.0) * u.0 + 600.0 * u.1;
    let foot = (450.0 + u.0 * along, u.1 * along);
    let near = |p: bimcraft_core::geom::P2| (p.0 - foot.0).hypot(p.1 - foot.1) < 2e-3;
    assert!(near(on[0].start()) || near(on[0].end()), "an end at {foot:?}: {all:?}");
    assert!(s.offer.is_some(), "its padlock offered");
}

const HINGE: &str = "88888888-8888-4888-8888-000000000001";
const LEAF: &str = "88888888-8888-4888-8888-000000000002";
const PANEL: &str = "88888888-8888-4888-8888-000000000003";

/// The pivots' family: Hinge at x = -450; Leaf from (-450, 0) along x, pivoted there by Swing Angle (0..90); Panel from
/// (-450, -600) along x, pivoted there by its own angle (-90..0); Diag through (-450, -600) at -45°, Up through (-450, 0)
/// at 45°, Off through neither.
fn pivots() -> String {
    format!(
        r#"family {{"name": "Swing", "category": "generic", "parameters": [{{"name": "Swing Angle", "kind": "angle", "value": 0}}], "types": {{"Shut": {{"Swing Angle": 0}}}}, "ref_planes": [{{"guid": "{HINGE}", "name": "Hinge", "start": [-450, -1500, 0], "end": [-450, 1500, 0], "up": [0, 0, 1]}}, {{"guid": "{PANEL}", "name": "Panel", "start": [-450, -600, 0], "end": [450, -600, 0], "up": [0, 0, 1], "pivot": {{"to": {{"fixed": [-450, -600, 0]}}, "min": -90, "max": 0}}}}, {{"name": "Diag", "start": [-450, -600, 0], "end": [450, -1500, 0], "up": [0, 0, 1]}}, {{"name": "Up", "start": [-450, 0, 0], "end": [450, 900, 0], "up": [0, 0, 1]}}, {{"name": "Off", "start": [0, -1500, 0], "end": [300, 1500, 0], "up": [0, 0, 1]}}], "ref_lines": [{{"guid": "{LEAF}", "start": [-450, 0, 0], "end": [450, 0, 0], "pivot": {{"to": {{"fixed": [-450, 0, 0]}}, "parameter": "Swing Angle", "min": 0, "max": 90}}}}]}}"#
    )
}

#[test]
fn a_pivoted_plane_or_line_turns_about_its_pivot_onto_the_reference() {
    let (mut h, _) = open(&pivots(), "Swing");
    view(&mut h, Some(Which::Plan));
    // Off isn't through Leaf's pivot: said.
    align(&mut h, [100.0, -500.0, 0.0], [300.0, 0.0, 0.0]);
    assert!(status(&h).contains("turns about its pivot, which isn't on the reference"), "{}", status(&h));
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    // Diag, then Panel: its own angle set to -45.
    align(&mut h, [-200.0, -850.0, 0.0], [300.0, -600.0, 0.0]);
    assert!((editor(&h).draft_pivot(g(PANEL)).unwrap().angle + 45.0).abs() < 1e-6, "{}", status(&h));
    assert!(editor(&h).views.align_lock.is_none(), "its swing holds it: no padlock");
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    // Up, then Leaf: Swing Angle, its parameter, set to 45 in the type.
    align(&mut h, [-200.0, 250.0, 0.0], [300.0, 0.0, 0.0]);
    match editor(&h).values().get("Swing Angle") {
        Some(Val::Number(v)) => assert!((v - 45.0).abs() < 1e-6, "{v} {}", status(&h)),
        other => panic!("{other:?}"),
    }
}

/// Slant before and after Align turns it onto Center (Left/Right), with the padlock: `align-rotate-*.png` where
/// `BIMCRAFT_ALIGN_DUMP` says (only then).
#[test]
fn align_rotate_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_ALIGN_DUMP") else { return };
    let mut h = app(&family());
    h.set_size(egui::vec2(1600.0, 1000.0));
    h.state_mut().edit_family("Turns");
    h.run();
    view(&mut h, Some(Which::Plan));
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("align-rotate-before.png")).unwrap();
    align(&mut h, [0.0, 1300.0, 0.0], [450.0, 0.0, 0.0]);
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("align-rotate-padlock.png")).unwrap();
    lock(&mut h);
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("align-rotate-locked.png")).unwrap();
}
