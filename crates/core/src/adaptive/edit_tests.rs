//! Modelling an adaptive family as in Revit's family editor, and moving an instance's adaptive point in the project.
use super::*;
use crate::engine::Session;
use crate::model::Element;
use serde_json::json;

/// A view straight down at 1 px a mm, as a test's camera: x right, y up the picture, nearer the higher.
fn plan(p: P3) -> Option<(f64, f64, f64)> {
    Some((p[0], -p[1], p[2]))
}

fn near(a: P3, b: P3) {
    assert!(frame::norm(view3d_sub(a, b)) < 1e-6, "{a:?} != {b:?}");
}

fn view3d_sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Four reference points drawn on the work plane, made adaptive in the order 3, 1, 2, 4 of drawing, a closed profile through
/// them, and a solid form from it.
fn drawn() -> (Definition, [String; 4], String, String) {
    let mut d = Definition::default();
    let p = [[0., 0., 0.], [1000., 0., 0.], [1000., 800., 0.], [0., 800., 0.]].map(|at| d.add_point(at));
    for k in [2, 0, 1, 3] {
        d.make_adaptive(&p[k]).unwrap();
    }
    let profile = d.add_chain(&p, true).unwrap();
    let form = d.create_form(std::slice::from_ref(&profile), 200., false).unwrap();
    d.check().unwrap();
    (d, p, profile, form)
}

#[test]
fn points_made_adaptive_are_numbered_in_that_order_and_a_closed_profile_makes_a_solid() {
    let (d, p, _, form) = drawn();
    let number = |id: &str| match &d.nodes.iter().find(|n| n.id == id).unwrap().operation {
        Operation::Point { host: Host::Adaptive { number, .. }, .. } => *number,
        _ => 0,
    };
    assert_eq!(p.clone().map(|id| number(&id)), [2, 3, 1, 4]);
    let r = d.evaluate(&BTreeMap::new(), &d.preview);
    assert!(!r.failed(), "{:?}", r.diagnostics);
    let Some(Evaluated::Form { mesh, .. }) = r.values.get(&form) else { panic!("the form evaluates") };
    assert!(mesh.closed);
    assert!((mesh.signed_volume() - 1000. * 800. * 200.).abs() < 1., "{}", mesh.signed_volume());
    // Making one adaptive again keeps its number; a hosted point can't be.
    let mut d = d;
    assert_eq!(d.make_adaptive(&p[0]).unwrap(), 2);
    let line = d.add_chain(&p[..2], false).unwrap();
    let hosted = d.add_hosted_point(&line, 0.25).unwrap();
    assert!(d.make_adaptive(&hosted).is_err());
    assert!(d.create_form(std::slice::from_ref(&line), 100., false).is_err(), "one open line makes no form");
    assert!(d.add_chain(&p[..2], true).is_err(), "a closed profile needs three points");
}

#[test]
fn two_profiles_loft_and_a_void_cuts() {
    let (mut d, _, profile, _) = drawn();
    let top = [[0., 0., 600.], [1000., 0., 900.], [1000., 800., 600.], [0., 800., 600.]].map(|at| d.add_point(at));
    let upper = d.add_chain(&top, true).unwrap();
    let loft = d.create_form(&[profile, upper], 0., false).unwrap();
    assert!(matches!(d.nodes.iter().find(|n| n.id == loft).unwrap().operation, Operation::Loft { .. }));
    let r = d.evaluate(&BTreeMap::new(), &d.preview);
    assert!(!r.failed(), "{:?}", r.diagnostics);
    let hole = [[400., 300., -50.], [600., 300., -50.], [600., 500., -50.], [400., 500., -50.]].map(|at| d.add_point(at));
    let ring = d.add_chain(&hole, true).unwrap();
    let void = d.create_form(&[ring], 2000., true).unwrap();
    assert!(d.nodes.iter().find(|n| n.id == void).unwrap().appearance.void);
    let r = d.evaluate(&BTreeMap::new(), &d.preview);
    assert!(!r.failed(), "{:?}", r.diagnostics);
}

#[test]
fn the_pointer_picks_a_point_before_a_line_before_a_form() {
    let (d, p, profile, form) = drawn();
    let r = d.evaluate(&BTreeMap::new(), &d.preview);
    assert_eq!(r.pick(&plan, (1003., -4.)), Some(Picked::Point(p[1].clone())));
    let Some(Picked::Curve(id, t)) = r.pick(&plan, (500., 3.)) else { panic!("the profile's first side is under the pointer") };
    assert_eq!(id, profile);
    assert!((t - 0.125).abs() < 1e-6, "a quarter of the way along the first of four sides: {t}");
    assert_eq!(r.pick(&plan, (500., -400.)), Some(Picked::Form(form)));
    assert_eq!(r.pick(&plan, (3000., 3000.)), None);
}

#[test]
fn points_move_where_they_are_dragged_whatever_hosts_them() {
    let (mut d, p, profile, _) = drawn();
    let hosted = d.add_hosted_point(&profile, 0.125).unwrap();
    let child = d.add_point([0.; 3]);
    if let Some(n) = d.nodes.iter_mut().find(|n| n.id == child) {
        n.operation = Operation::Point { host: Host::Frame { point: p[1].clone() }, transform: Transform::default() };
    }
    // An adaptive point, then one with a test frame of its own: that frame moves, the family's stays.
    let r = d.evaluate(&BTreeMap::new(), &d.preview);
    d.move_point(&p[0], &r, [0., 0., 300.]).unwrap();
    let r = d.evaluate(&BTreeMap::new(), &d.preview);
    near(r.origin(&p[0]).unwrap(), [0., 0., 300.]);
    d.preview.frames.insert(3, Frame::at([1000., 0., 0.]));
    let r = d.evaluate(&BTreeMap::new(), &d.preview);
    d.move_point(&p[1], &r, [1200., 0., 0.]).unwrap();
    assert_eq!(d.preview.frames[&3].origin, [1200., 0., 0.]);
    // Hosted on a point: its offset, in the host's axes.
    let r = d.evaluate(&BTreeMap::new(), &d.preview);
    d.move_point(&child, &r, [1200., 50., 0.]).unwrap();
    let r = d.evaluate(&BTreeMap::new(), &d.preview);
    near(r.origin(&child).unwrap(), [1200., 50., 0.]);
    // On a line: the nearest place along it.
    d.move_point(&hosted, &r, [100., 800., 40.]).unwrap();
    let r = d.evaluate(&BTreeMap::new(), &d.preview);
    let at = r.origin(&hosted).unwrap();
    assert!((at[1] - 800.).abs() < 5. && (at[0] - 100.).abs() < 15., "on the side from p3 to p4: {at:?}");
    assert!(d.move_point(&p[2], &r, [f64::NAN, 0., 0.]).is_err());
}

#[test]
fn deleting_a_point_deletes_what_depends_on_it_and_closes_up_the_numbers() {
    let (mut d, p, profile, form) = drawn();
    let spare = d.add_point([5000., 0., 0.]);
    let gone = d.remove(std::slice::from_ref(&p[2]));
    assert_eq!(gone.len(), 3, "{gone:?}");
    assert!(gone.contains(&profile) && gone.contains(&form));
    assert!(d.nodes.iter().any(|n| n.id == spare));
    let numbers: Vec<u32> = d
        .nodes
        .iter()
        .filter_map(|n| match &n.operation {
            Operation::Point { host: Host::Adaptive { number, .. }, .. } => Some(*number),
            _ => None,
        })
        .collect();
    assert_eq!(numbers, [1, 2, 3], "adaptive point 1 went: 2, 3 and 4 close up");
    d.check().unwrap();
    assert_eq!(d.next_number(), 4);
}

#[test]
fn an_instances_adaptive_point_moves_to_where_its_grip_is_dragged_in_one_undo_step() {
    let mut s = Session::new();
    let (d, ..) = drawn();
    s.execute("family", &json!({"name":"Drawn panel","types":{"A":{}},"adaptive":d})).unwrap();
    let id =
        s.execute("component", &json!({"family":"Drawn panel","at":[100,200],"angle":30,"offset":500})).unwrap()["id"].as_u64().unwrap();
    let points = |s: &Session| {
        let Some(Element::Component(c)) = s.model.elements.get(id) else { panic!() };
        s.model.adaptive_points(c)
    };
    let before = points(&s);
    assert_eq!(before.len(), 4);
    s.execute("adaptive_move", &json!({"id": id, "number": 3, "to": [5000, 0, 700]})).unwrap();
    let after = points(&s);
    for ((n, a), (_, b)) in before.iter().zip(&after) {
        if *n == 3 {
            near(b.origin, [5000., 0., 700.]);
            near(b.z, a.z);
        } else {
            near(b.origin, a.origin);
        }
    }
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(points(&s), before);
    assert!(s.execute("adaptive_move", &json!({"id": id, "number": 9, "to": [0, 0, 0]})).is_err());
    assert!(s.execute("adaptive_move", &json!({"id": id, "number": 1, "to": [0, 0]})).is_err());
    assert!(s.execute("adaptive_move", &json!({"id": id, "number": 1, "to": [1e12, 0, 0]})).is_err());
}
