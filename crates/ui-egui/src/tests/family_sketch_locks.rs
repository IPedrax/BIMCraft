//! F7b: a form's sketch lines locked. Align and its padlock on a line of a form being sketched (kept through Finish, dropped
//! by Cancel, and dropped with a line deleted), on a finished form in plan and on an extrusion's face in an elevation; Aligned
//! Dimension to a sketch line; Radial Dimension labelled; and the locked form flexing through its types.
use super::family_views::{click_family, editor, open, view};
use super::*;
use bimcraft_core::family::views::{Side, Which};
use bimcraft_core::family::{DimKind, DimRef, Expr, Family};
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

/// A box family: Width 1000 (Small 600, Large 1400), Left and Right at ±500 labelled Width, EQ about Center (Left/Right). A
/// slab on Ref. Level sketched as a rectangle at ±300 by ±200, 100 thick, and a disc of radius 80 at (0, 600); `dims`, more
/// dimensions as JSON.
fn family(dims: &str) -> String {
    format!(
        r#"family {{"name": "Box", "category": "generic", "parameters": [{{"name": "Width", "value": 1000}}, {{"name": "R", "value": 80}}], "types": {{"Small": {{"Width": 600, "R": 50}}, "Large": {{"Width": 1400, "R": 120}}}}, "ref_planes": [{{"guid": "{LEFT}", "name": "Left", "start": [-500, -2000, 0], "end": [-500, 2000, 0], "up": [0, 0, 1]}}, {{"guid": "{RIGHT}", "name": "Right", "start": [500, -2000, 0], "end": [500, 2000, 0], "up": [0, 0, 1]}}], "forms": [{{"guid": "{SLAB}", "sketch": [{{"guid": "{SOUTH}", "start": [-300, -200], "end": [300, -200]}}, {{"guid": "{EAST}", "start": [300, -200], "end": [300, 200]}}, {{"guid": "33333333-3333-4333-8333-000000000004", "start": [300, 200], "end": [-300, 200]}}, {{"guid": "{WEST}", "start": [-300, 200], "end": [-300, -200]}}], "from": 0, "to": 100}}, {{"guid": "{DISC}", "sketch": [{{"guid": "{RING}", "centre": [0, 600], "radius": 80}}], "from": 0, "to": 50}}], "dimensions": [{{"refs": [{{"plane": "{LEFT}"}}, {{"plane": "{RIGHT}"}}], "at": [0, -900, 0], "label": "Width"}}, {{"refs": [{{"plane": "{LEFT}"}}, {{"plane": "9e3c0a10-1f0b-4d8c-8a5e-0000000000a3"}}, {{"plane": "{RIGHT}"}}], "at": [0, -1100, 0], "equal": true}}{dims}]}}"#
    )
}

fn g(s: &str) -> Guid {
    serde_json::from_value(json!(s)).unwrap()
}

fn in_plan(dims: &str) -> Harness<'static, BimApp> {
    let (mut h, _) = open(&family(dims), "Box");
    view(&mut h, Some(Which::Plan));
    h
}

fn ed_mut<'a>(h: &'a mut Harness<'_, BimApp>) -> &'a mut crate::editor::FamilyEditor {
    h.state_mut().family_editor.as_mut().unwrap()
}

fn status(h: &Harness<BimApp>) -> String {
    h.state().status.text.clone()
}

/// Line `line` of the slab as type `t` flexes the draft: its start and end.
fn flexed_line(h: &Harness<BimApp>, t: &str, line: &str) -> ([f64; 2], [f64; 2]) {
    let f = editor(h).draft_family().unwrap();
    let v = f.values(t, &BTreeMap::new()).unwrap();
    let flexed: Family = f.flexed(&v).unwrap().into_owned();
    let l = flexed.forms[0].sketch.iter().find(|l| l.guid == g(line)).expect("the line");
    let n = |e: &Expr| crate::adaptive_editor::worth(e, &v).unwrap();
    match &l.curve {
        bimcraft_core::family::sketch::FormCurve::Line { start, end, .. } => ([n(&start[0]), n(&start[1])], [n(&end[0]), n(&end[1])]),
        _ => panic!("a straight line"),
    }
}

/// The alignment locks of the draft that measure to line `line` of the slab.
fn locks_to(h: &Harness<BimApp>, line: &str) -> Vec<Vec<DimRef>> {
    editor(h)
        .dimensions
        .iter()
        .filter(|d| d.alignment && d.refs.contains(&DimRef::Sketch(g(SLAB), g(line))))
        .map(|d| d.refs.clone())
        .collect()
}

/// Whether no padlock of `spots` covers a line of `lines` or another padlock.
fn padlocks_clear(spots: &[egui::Pos2], lines: &[[egui::Pos2; 2]]) -> bool {
    use crate::editor::dimensions::{crosses, lock_rect};
    spots.iter().enumerate().all(|(i, p)| {
        !lines.iter().any(|[a, b]| crosses(*a, *b, lock_rect(*p)))
            && spots[i + 1..].iter().all(|q| !lock_rect(*p).intersects(lock_rect(*q)))
    })
}

/// The padlock shown while sketching that `which` is, clicked.
fn click_padlock(h: &mut Harness<BimApp>, which: crate::editor::sketch::Padlock) {
    let at = editor(h).sketch_padlocks_shown().into_iter().find(|(_, w)| *w == which).map(|(p, _)| p).expect("the padlock shown");
    click_at(h, at);
}

#[test]
fn align_in_sketch_mode_moves_a_line_onto_a_plane_and_its_padlock_locks_it_at_finish() {
    let mut h = in_plan("");
    ed_mut(&mut h).edit_form_sketch(0).unwrap();
    h.run();
    assert!(editor(&h).sketching());
    shortcut(&mut h, "AL");
    assert!(editor(&h).views.aligning.is_some(), "Align, while sketching");
    // Left (at -700, as Large, the type shown, has it), then the west line: it moves onto Left, the lines joined to it
    // stretching.
    click_family(&mut h, [-700.0, 900.0, 0.0]);
    click_family(&mut h, [-300.0, 0.0, 0.0]);
    let s = editor(&h).sketch.as_ref().unwrap();
    let west = s.sketch.lines.iter().find(|l| l.guid == g(WEST)).unwrap();
    assert!((west.start().0 + 700.0).abs() < 1e-6 && (west.end().0 + 700.0).abs() < 1e-6, "{:?}", west.curve);
    assert!(s.offer.is_some(), "its padlock, open: {}", status(&h));
    let spots: Vec<egui::Pos2> = editor(&h).sketch_padlocks_shown().into_iter().map(|(p, _)| p).collect();
    assert!(padlocks_clear(&spots, &editor(&h).sketch_lines_shown()), "the offered padlock beside its line, on no line: {spots:?}");
    // Outside the slab: to the west of the west line.
    assert!(spots[0].x < editor(&h).views.screen_of([-700.0, 0.0, 0.0]).unwrap().x, "{spots:?}");
    click_padlock(&mut h, crate::editor::sketch::Padlock::Offer);
    assert_eq!(editor(&h).sketch.as_ref().unwrap().locks.len(), 1, "locked");
    assert!(editor(&h).dimensions.len() == 2, "the draft waits for Finish");
    press(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", status(&h));
    assert_eq!(locks_to(&h, WEST), vec![vec![DimRef::Plane(g(LEFT)), DimRef::Sketch(g(SLAB), g(WEST))]]);
    // Locked, the west line follows Left as Width flexes; the east one stays where it is drawn.
    assert_eq!(flexed_line(&h, "Large", WEST).0[0], -700.0);
    assert_eq!(flexed_line(&h, "Small", WEST).0[0], -300.0);
    assert_eq!(flexed_line(&h, "Large", EAST).0[0], 300.0);
    // Edit Extrusion again opens it as the type shows it, and keeps the lock; a lock added then cancelled is dropped.
    ed_mut(&mut h).edit_form_sketch(0).unwrap();
    h.run();
    let west = editor(&h).sketch.as_ref().unwrap().sketch.lines.iter().find(|l| l.guid == g(WEST)).unwrap().clone();
    assert_eq!(west.start().0, -700.0, "Large, the first type, as the view shows it");
    assert!(
        editor(&h).sketch_padlocks_shown().iter().any(|(_, w)| matches!(w, crate::editor::sketch::Padlock::Had(_))),
        "its padlock shows"
    );
    shortcut(&mut h, "AL");
    click_family(&mut h, [700.0, 900.0, 0.0]);
    click_family(&mut h, [300.0, 0.0, 0.0]);
    let spots: Vec<egui::Pos2> = editor(&h).sketch_padlocks_shown().into_iter().map(|(p, _)| p).collect();
    assert_eq!(spots.len(), 2);
    assert!(padlocks_clear(&spots, &editor(&h).sketch_lines_shown()), "two padlocks, each clear: {spots:?}");
    click_padlock(&mut h, crate::editor::sketch::Padlock::Offer);
    press(&mut h, "Cancel Edit Mode");
    assert!(locks_to(&h, EAST).is_empty(), "Cancel drops it");
    assert_eq!(locks_to(&h, WEST).len(), 1);
    // Finished untouched, it keeps the lock and the line's numbers as drawn.
    ed_mut(&mut h).edit_form_sketch(0).unwrap();
    h.run();
    press(&mut h, "Finish Edit Mode");
    assert_eq!(locks_to(&h, WEST).len(), 1);
    assert_eq!(flexed_line(&h, "Small", WEST).0[0], -300.0, "{}", status(&h));
    // The west line deleted and drawn again: a new line, and the lock goes with the old one.
    ed_mut(&mut h).edit_form_sketch(0).unwrap();
    h.run();
    {
        let s = ed_mut(&mut h).sketch.as_mut().unwrap();
        let i = s.sketch.lines.iter().position(|l| l.guid == g(WEST)).unwrap();
        let mut again = s.sketch.lines.remove(i);
        again.guid = Guid::default();
        again.id = 0;
        s.sketch.lines.insert(i, again);
    }
    press(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", status(&h));
    assert!(editor(&h).dimensions.iter().all(|d| !d.alignment), "the lock went with its line");
    assert!(editor(&h).draft_family().unwrap().forms[0].sketch.iter().all(|l| !l.guid.is_nil()), "each line a GUID");
}

#[test]
fn align_on_a_finished_form_in_plan_and_on_its_face_in_an_elevation_locks_its_lines() {
    let mut h = in_plan("");
    shortcut(&mut h, "AL");
    // In plan: Right (at 700, as Large has it), then the slab's east line, seen flat.
    click_family(&mut h, [700.0, 900.0, 0.0]);
    click_family(&mut h, [300.0, 0.0, 0.0]);
    assert_eq!(flexed_line(&h, "Large", EAST).0[0], 700.0, "moved onto Right where the type shows it: {}", status(&h));
    assert!(editor(&h).views.align_lock.is_some(), "its padlock, open");
    // Its padlock, beside the line: clicked where it shows.
    let at = editor(&h).views.offer_shown.expect("shown");
    click_at(&mut h, at);
    assert_eq!(locks_to(&h, EAST), vec![vec![DimRef::Plane(g(RIGHT)), DimRef::Sketch(g(SLAB), g(EAST))]]);
    assert_eq!(flexed_line(&h, "Small", EAST).0[0], 300.0, "and follows Right");
    // In Front: Left, then the slab's west face, its line running the way the view looks.
    key(&mut h, Key::Escape);
    view(&mut h, Some(Which::Elevation(Side::Front)));
    shortcut(&mut h, "AL");
    click_family(&mut h, [-700.0, 0.0, 900.0]);
    click_family(&mut h, [-300.0, 0.0, 50.0]);
    assert_eq!(flexed_line(&h, "Large", WEST).0[0], -700.0, "{}", status(&h));
    let at = editor(&h).views.offer_shown.expect("shown");
    click_at(&mut h, at);
    // Each padlock beside its line, on no line drawn and on no other padlock.
    h.run();
    let spots: Vec<egui::Pos2> = editor(&h).views.locks_shown.iter().map(|(_, p)| *p).collect();
    assert!(!spots.is_empty() && padlocks_clear(&spots, &editor(&h).view_lines_shown()), "{spots:?}");
    let locks = locks_to(&h, WEST);
    assert_eq!(locks, vec![vec![DimRef::Plane(g(LEFT)), DimRef::Sketch(g(SLAB), g(WEST))]]);
    assert_eq!(editor(&h).dimensions.last().unwrap().view, bimcraft_core::family::DimView::Front);
    // Both locked, the slab is the box's width in every type.
    assert_eq!((flexed_line(&h, "Small", WEST).0[0], flexed_line(&h, "Small", EAST).0[0]), (-300.0, 300.0));
    // A line its lock holds can't be aligned elsewhere: asked.
    key(&mut h, Key::Escape);
    view(&mut h, Some(Which::Plan));
    shortcut(&mut h, "AL");
    click_family(&mut h, [0.0, 900.0, 0.0]);
    click_family(&mut h, [-700.0, 0.0, 0.0]);
    assert!(h.query_by_label("Constraints are not satisfied").is_some(), "{}", status(&h));
    press(&mut h, "Cancel");
}

#[test]
fn aligned_dimension_measures_to_a_sketch_line_and_radial_dimension_labels_a_circle() {
    let mut h = in_plan("");
    press(&mut h, "Aligned Dimension");
    click_family(&mut h, [-700.0, 900.0, 0.0]);
    click_family(&mut h, [-300.0, 0.0, 0.0]);
    click_family(&mut h, [-400.0, -600.0, 0.0]);
    let d = editor(&h).dimensions.last().cloned().unwrap();
    assert_eq!(d.refs, vec![DimRef::Plane(g(LEFT)), DimRef::Sketch(g(SLAB), g(WEST))], "{}", status(&h));
    // Locked at its length by its padlock: the west line keeps 400 from Left as Width flexes.
    key(&mut h, Key::Escape);
    click_family(&mut h, [-400.0, -600.0, 0.0]);
    assert_eq!(editor(&h).selected_dimension(), Some(d.guid));
    let at = editor(&h).views.screen_of([-500.0, -600.0, 0.0]).unwrap() + vec2(0.0, 16.0);
    click_at(&mut h, at);
    assert!(editor(&h).dimensions.last().unwrap().is_locked(0), "{}", status(&h));
    assert_eq!(flexed_line(&h, "Small", WEST).0[0], 100.0);
    // Radial Dimension: the disc's edge, then where it goes; labelled R from the Options Bar.
    key(&mut h, Key::Escape);
    press(&mut h, "Radial");
    click_family(&mut h, [80.0, 600.0, 0.0]);
    click_family(&mut h, [300.0, 700.0, 0.0]);
    let r = editor(&h).dimensions.last().cloned().unwrap();
    assert_eq!((r.kind, r.refs.clone()), (DimKind::Radial, vec![DimRef::Sketch(g(DISC), g(RING))]), "{}", status(&h));
    key(&mut h, Key::Escape);
    let rim = editor(&h).views.screen_of([0.0, 600.0, 0.0]).unwrap();
    let out = editor(&h).views.screen_of([150.0, 650.0, 0.0]).unwrap();
    click_at(&mut h, rim + (out - rim) * 0.4);
    assert_eq!(editor(&h).selected_dimension(), Some(r.guid), "{}", status(&h));
    h.get_by_label("Label").click();
    h.run();
    press_near(&mut h, "R", "Label");
    assert_eq!(editor(&h).dimensions.last().unwrap().label.as_deref(), Some("R"));
    let f = editor(&h).draft_family().unwrap();
    let v = f.values("Large", &BTreeMap::new()).unwrap();
    assert_eq!(f.measured(&v).unwrap().last(), Some(&vec![120.0]), "the radius flexes with R");
}

/// The slab in plan being sketched, its west line aligned and locked to Left and Align's padlock offered on the east one;
/// the finished slab flexed to Large with its locks, a dimension to a sketch line and a radial one; and Front with its face
/// locked. Saved where `BIMCRAFT_F7B_DUMP` says (only then).
#[test]
fn family_sketch_locks_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_F7B_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let lock = |a: &str, line: &str, at: &str, view: &str| {
        format!(
            r#", {{"view": "{view}", "refs": [{{"plane": "{a}"}}, {{"form": "{SLAB}", "line": "{line}"}}], "at": {at}, "locked": [{{"segment": 0, "length": 0}}], "alignment": true}}"#
        )
    };
    let dims = format!(
        r#"{}{}, {{"refs": [{{"form": "{SLAB}", "line": "{SOUTH}"}}, {{"plane": "9e3c0a10-1f0b-4d8c-8a5e-0000000000a2"}}], "at": [-900, 0, 0]}}, {{"kind": "radial", "refs": [{{"form": "{DISC}", "line": "{RING}"}}], "at": [400, 800, 0], "label": "R"}}"#,
        lock(LEFT, WEST, "[-300, 0, 0]", "plan"),
        lock(RIGHT, EAST, "[300, 0, 50]", "front")
    );
    let mut h = app(&family(&dims));
    h.set_size(vec2(1600.0, 1000.0));
    h.state_mut().edit_family("Box");
    h.run();
    view(&mut h, Some(Which::Plan));
    h.run();
    // The radial dimension selected, by its leader: its padlock shows.
    click_family(&mut h, [53.7, 626.8, 0.0]);
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("f7b-plan-large.png")).unwrap();
    ed_mut(&mut h).preview_type("Small");
    h.run();
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("f7b-plan-small.png")).unwrap();
    view(&mut h, Some(Which::Elevation(Side::Front)));
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("f7b-front.png")).unwrap();
    view(&mut h, Some(Which::Elevation(Side::Left)));
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("f7b-left.png")).unwrap();
    // Sketch mode: the slab again, its locks' padlocks shown; the south line aligned to Center (Front/Back), offered.
    view(&mut h, Some(Which::Plan));
    ed_mut(&mut h).edit_form_sketch(0).unwrap();
    h.run();
    shortcut(&mut h, "AL");
    click_family(&mut h, [900.0, 0.0, 0.0]);
    click_family(&mut h, [0.0, -200.0, 0.0]);
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("f7b-sketch-align.png")).unwrap();
}
