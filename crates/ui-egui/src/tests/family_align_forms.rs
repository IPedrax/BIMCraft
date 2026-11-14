//! Align on a finished form's faces, as a tester hit it: a blank Generic family's starter box, and a box drawn with Create >
//! Extrusion and Rectangle. Align (AL) to a reference plane and the box's edge in plan, its side and its end in an elevation,
//! with the padlock; the reverse order; one form's face to another's; a reference line; and the paths that pick the same
//! edge (click-select, Aligned Dimension).
use super::family_views::{click_family, editor, open};
use super::*;
use bimcraft_core::family::sketch::FormCurve;
use bimcraft_core::family::views::{Side, Which};
use bimcraft_core::family::{CENTER_FRONT_BACK, CENTER_LEFT_RIGHT, DimRef, Family};
use bimcraft_core::geom::P2;
use bimcraft_core::ids::Guid;

type Values = std::collections::BTreeMap<String, bimcraft_core::family::Val>;

fn project() -> Harness<'static, BimApp> {
    let mut s = Session::new();
    s.script(r#"wall {"points": [[0,0],[8000,0]]}"#).unwrap();
    let mut app = BimApp::new(s);
    (app.profile, app.docking) = (crate::style::Profile::Rev, crate::dock::Docking::revit());
    let mut h = Harness::builder()
        .with_size([1600.0, 1100.0])
        .with_max_steps(32)
        .with_step_dt(0.05)
        .build_ui_state(|ui, app: &mut BimApp| app.show(ui), app);
    h.run();
    h
}

fn status(h: &Harness<BimApp>) -> String {
    let ed = editor(h);
    format!("{} / {:?}", h.state().status.text, ed.views.status)
}

fn draft(h: &Harness<BimApp>) -> Family {
    editor(h).draft_family().unwrap()
}

fn button(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_role_and_label(Role::Button, label).click();
    h.run();
}

/// The draft as the type previewed flexes it.
fn flexed(h: &Harness<BimApp>) -> Family {
    let f = draft(h);
    f.flexed(&editor(h).values()).unwrap().into_owned()
}

/// Form `i`'s straight sketch lines of `f` (flexed) with `values`: start and end, in the plane's axes.
pub(super) fn lines_of(f: &Family, values: &Values, i: usize) -> Vec<(Guid, [f64; 2], [f64; 2])> {
    let n = |e: &bimcraft_core::family::Expr| crate::adaptive_editor::worth(e, values).unwrap();
    f.forms[i]
        .sketch
        .iter()
        .filter_map(|l| match &l.curve {
            FormCurve::Line { start, end, .. } => Some((l.guid, [n(&start[0]), n(&start[1])], [n(&end[0]), n(&end[1])])),
            _ => None,
        })
        .collect()
}

/// Form `i`'s extent along x and y in plan, as flexed.
fn extent(h: &Harness<BimApp>, i: usize) -> [f64; 4] {
    let ls = lines_of(&flexed(h), &editor(h).values(), i);
    let span = |k: usize| {
        ls.iter().flat_map(|(_, a, b)| [a[k], b[k]]).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)))
    };
    let ((x0, x1), (y0, y1)) = (span(0), span(1));
    [x0, x1, y0, y1]
}

/// A click at point `q` of the plane being sketched on (its own axes).
fn click_plane(h: &mut Harness<BimApp>, q: P2) {
    let ed = editor(h);
    let frame = ed.plane_set().frame(&ed.sketch.as_ref().expect("sketching").host).unwrap();
    let at = ed.views.screen_of(frame.point(q, 0.0)).expect("a view shown");
    click_at(h, at);
}

/// A new Generic family, its starter box (Width 1000 by Depth 600, 750 tall) as File > New > Family gives it, in its plan.
fn generic() -> Harness<'static, BimApp> {
    let mut h = project();
    new_family_of(&mut h, "Generic");
    assert!(h.state().active_family().is_some(), "the family document is open");
    key(&mut h, Key::Enter);
    super::family_views::view(&mut h, Some(Which::Plan));
    assert_eq!(draft(&h).forms.len(), 1, "the starter box");
    assert_eq!(extent(&h, 0), [-500.0, 500.0, -300.0, 300.0]);
    h
}

/// A box drawn as a user draws one: Create > Extrusion, Rectangle from `a` to `b` on Ref. Level, Finish. Its index.
fn draw_box(h: &mut Harness<BimApp>, a: P2, b: P2) -> usize {
    let before = draft(h).forms.len();
    button(h, "Extrusion");
    assert!(editor(h).sketching(), "{}", status(h));
    button(h, "Rectangle");
    click_plane(h, a);
    click_plane(h, b);
    press(h, "Finish Edit Mode");
    assert_eq!(draft(h).forms.len(), before + 1, "finished: {}", status(h));
    key(h, Key::Escape);
    before
}

/// A reference plane drawn by two clicks at family points `a` and `b` in the view shown: its GUID.
fn reference_plane(h: &mut Harness<BimApp>, a: [f64; 3], b: [f64; 3]) -> Guid {
    shortcut(h, "RP");
    click_family(h, a);
    click_family(h, b);
    let g = editor(h).planes.last().unwrap().guid;
    key(h, Key::Escape);
    key(h, Key::Escape);
    g
}

/// Align (AL): a click at `reference`, then at `target` (family points), and the padlock it offers clicked.
fn align(h: &mut Harness<BimApp>, reference: [f64; 3], target: [f64; 3]) {
    shortcut(h, "AL");
    assert!(editor(h).views.aligning.is_some(), "Align is on");
    click_family(h, reference);
    assert!(matches!(editor(h).views.aligning, Some(Some(_))), "a reference at {reference:?}: {}", status(h));
    click_family(h, target);
    assert!(editor(h).views.align_lock.is_some(), "aligned, the padlock offered: {}", status(h));
    h.run();
    let at = editor(h).views.offer_shown.expect("the padlock shown");
    click_at(h, at);
    key(h, Key::Escape);
}

/// The Align locks of the draft: their two references.
fn locks(h: &Harness<BimApp>) -> Vec<Vec<DimRef>> {
    editor(h).dimensions.iter().filter(|d| d.alignment).map(|d| d.refs.clone()).collect()
}

/// Form `i`'s sketch line through family point `q` in plan.
fn line_through(h: &Harness<BimApp>, i: usize, q: [f64; 2]) -> DimRef {
    let f = flexed(h);
    let on = |a: [f64; 2], b: [f64; 2]| {
        let (d, e) = ([b[0] - a[0], b[1] - a[1]], [q[0] - a[0], q[1] - a[1]]);
        (d[0] * e[1] - d[1] * e[0]).abs() < 1e-6 * (d[0].hypot(d[1])).max(1.0)
    };
    let g =
        lines_of(&f, &editor(h).values(), i).into_iter().find(|(_, a, b)| on(*a, *b)).map(|(g, ..)| g).expect("a line through the point");
    DimRef::Sketch(f.forms[i].guid, g)
}

#[test]
fn the_blank_generic_box_s_edge_aligns_to_a_plane_in_plan_and_its_padlock_locks_it() {
    let mut h = generic();
    // Center (Front/Back), then the box's north edge: it moves onto the plane, the south one staying.
    let north = line_through(&h, 0, [0.0, 300.0]);
    align(&mut h, [900.0, 0.0, 0.0], [250.0, 300.0, 0.0]);
    assert_eq!(extent(&h, 0), [-500.0, 500.0, -300.0, 0.0], "{}", status(&h));
    assert_eq!(locks(&h), vec![vec![DimRef::Plane(CENTER_FRONT_BACK), north]]);
}

#[test]
fn the_blank_generic_box_s_edge_is_picked_by_click_select_and_aligned_dimension_in_plan() {
    let mut h = generic();
    click_family(&mut h, [250.0, -300.0, 0.0]);
    assert_eq!(editor(&h).selected_form(), Some(0), "selected by its edge: {}", status(&h));
    key(&mut h, Key::Escape);
    let south = line_through(&h, 0, [0.0, -300.0]);
    press(&mut h, "Aligned Dimension");
    click_family(&mut h, [900.0, 0.0, 0.0]);
    click_family(&mut h, [250.0, -300.0, 0.0]);
    click_family(&mut h, [800.0, -150.0, 0.0]);
    let d = editor(&h).dimensions.last().cloned().expect("placed");
    assert_eq!(d.refs, vec![DimRef::Plane(CENTER_FRONT_BACK), south], "{}", status(&h));
}

#[test]
fn the_blank_generic_box_s_side_and_end_align_in_an_elevation() {
    let mut h = generic();
    super::family_views::view(&mut h, Some(Which::Elevation(Side::Front)));
    // Its west side, seen edge on, onto Center (Left/Right).
    let west = line_through(&h, 0, [-500.0, 0.0]);
    align(&mut h, [0.0, 0.0, 1500.0], [-500.0, 0.0, 300.0]);
    assert_eq!(extent(&h, 0)[..2], [0.0, 500.0], "{}", status(&h));
    assert_eq!(locks(&h), vec![vec![DimRef::Plane(CENTER_LEFT_RIGHT), west]]);
    // Its top, onto a plane drawn at 1000.
    let top = reference_plane(&mut h, [-1500.0, 0.0, 1000.0], [1500.0, 0.0, 1000.0]);
    align(&mut h, [1200.0, 0.0, 1000.0], [250.0, 0.0, 750.0]);
    let form = flexed(&h).forms[0].clone();
    let z = editor(&h).planes.iter().find(|p| p.guid == top).map(|p| p.start[2]).unwrap();
    let to = crate::adaptive_editor::worth(&form.to, &editor(&h).values()).unwrap();
    assert!((to - z).abs() < 1e-6 && (z - 1000.0).abs() < 0.1, "its top on the plane: {to} {z} {}", status(&h));
    let end = DimRef::Form(form.guid, bimcraft_core::family::Extent::End);
    assert!(locks(&h).contains(&vec![DimRef::Plane(top), end]), "{:?}", locks(&h));
}

#[test]
fn align_in_the_reverse_order_moves_the_plane_onto_the_box_s_edge() {
    let mut h = generic();
    let plane = reference_plane(&mut h, [-1500.0, 800.0, 0.0], [1500.0, 800.0, 0.0]);
    align(&mut h, [250.0, 300.0, 0.0], [1200.0, 800.0, 0.0]);
    let p = flexed(&h).planes.iter().find(|p| p.guid == plane).cloned().unwrap();
    assert_eq!((p.start[1], p.end[1]), (300.0, 300.0), "{}", status(&h));
    assert_eq!(extent(&h, 0), [-500.0, 500.0, -300.0, 300.0], "the box stays");
}

#[test]
fn a_drawn_box_s_face_aligns_to_the_starter_box_s_face() {
    let mut h = generic();
    let i = draw_box(&mut h, P2(1000.0, -200.0), P2(1600.0, 200.0));
    align(&mut h, [500.0, 100.0, 0.0], [1000.0, 100.0, 0.0]);
    assert_eq!(extent(&h, i), [500.0, 1600.0, -200.0, 200.0], "{}", status(&h));
    assert_eq!(extent(&h, 0), [-500.0, 500.0, -300.0, 300.0], "the reference stays");
}

/// A box on Ref. Level from (-300, -200) to (300, 200), 100 tall, and a reference line on Ref. Level at x = -700.
const WITH_LINE: &str = r#"family {"name": "Box", "category": "generic", "types": {"T": {}}, "forms": [{"profile": [[-300, -200], [300, -200], [300, 200], [-300, 200]], "to": 100}], "ref_lines": [{"start": [-700, -1000, 0], "end": [-700, 1000, 0]}]}"#;

#[test]
fn a_box_s_edge_aligns_to_a_reference_line_in_plan() {
    let (mut h, _) = open(WITH_LINE, "Box");
    super::family_views::view(&mut h, Some(Which::Plan));
    align(&mut h, [-700.0, 800.0, 0.0], [-300.0, 100.0, 0.0]);
    assert_eq!(extent(&h, 0)[..2], [-700.0, 300.0], "{}", status(&h));
}

#[test]
fn a_click_inside_a_form_says_what_of_it_align_takes() {
    let mut h = generic();
    shortcut(&mut h, "AL");
    click_family(&mut h, [900.0, 0.0, 0.0]);
    click_family(&mut h, [250.0, 150.0, 0.0]);
    assert!(status(&h).contains("Align takes a form's straight edge, a side the view sees edge on, or its end"), "{}", status(&h));
    // The reference again: said so.
    click_family(&mut h, [900.0, 0.0, 0.0]);
    assert!(status(&h).contains("That is the reference"), "{}", status(&h));
}

/// The blank Generic box in plan before Align, aligned to Center (Front/Back) with the padlock offered, and locked; and its
/// side in Front aligned to Center (Left/Right). `align-*.png` where `BIMCRAFT_ALIGN_DUMP` says (only then).
#[test]
fn align_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_ALIGN_DUMP") else { return };
    let save = |h: &mut Harness<BimApp>, name: &str| {
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("align-{name}.png"))).unwrap();
    };
    let mut h = generic();
    save(&mut h, "plan-before");
    shortcut(&mut h, "AL");
    click_family(&mut h, [900.0, 0.0, 0.0]);
    save(&mut h, "plan-reference");
    click_family(&mut h, [250.0, 300.0, 0.0]);
    save(&mut h, "plan-padlock");
    let at = editor(&h).views.offer_shown.expect("the padlock shown");
    click_at(&mut h, at);
    key(&mut h, Key::Escape);
    save(&mut h, "plan-locked");
    super::family_views::view(&mut h, Some(Which::Elevation(Side::Front)));
    save(&mut h, "front-before");
    shortcut(&mut h, "AL");
    click_family(&mut h, [0.0, 0.0, 1500.0]);
    click_family(&mut h, [-500.0, 0.0, 300.0]);
    save(&mut h, "front-padlock");
}
