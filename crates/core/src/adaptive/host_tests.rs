//! Adaptive components and work planes: an adaptive component takes its place from its own points, so `Model::check` refuses
//! a `host` on one and `Model::component_placement` and `Model::settle_hosts` leave it as the adaptive families placed it,
//! whatever reference planes the project has.

use serde_json::{Value, json};

use super::four_point_panel;
use crate::engine::Session;
use crate::model::{Component, Element, HostRef, Hosted, Id};

const SIN30: f64 = 0.5;
const COS30: f64 = 0.866_025_403_784_438_6;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn id_of(v: &Value) -> Id {
    v["id"].as_u64().unwrap()
}

fn component(s: &Session, id: Id) -> &Component {
    match s.model.elements.get(id) {
        Some(Element::Component(c)) => c,
        other => panic!("{id} is no component: {other:?}"),
    }
}

/// A project with the four-point panel, work plane-based and not Always Vertical (so a plane would tilt it, were it not
/// adaptive), a reference plane rising 30 degrees toward +y, and two panels: one on the family's test placement (90 up,
/// turned 30) and one on frames captured from it, placed elsewhere (`angle` -45, `offset` 250).
fn project() -> (Session, Id, Id, Id) {
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({"name": "Panel", "types": {"A": {}}, "adaptive": four_point_panel(),
               "options": {"work_plane_based": true, "always_vertical": false}}),
    );
    let plane =
        id_of(&run(&mut s, "reference_plane", json!({"name": "Ramp", "start": [0, 0, 0], "end": [10000, 0, 0], "up": [0, COS30, SIN30]})));
    let preview = id_of(&run(&mut s, "component", json!({"family": "Panel", "at": [100, 200], "angle": 30, "offset": 90})));
    let sources: Vec<Value> = (1..=4).map(|n| json!({"id": preview, "host": {"type": "frame", "point": format!("p{n}")}})).collect();
    let capture = run(&mut s, "adaptive_capture", json!({ "sources": sources }));
    let placed = run(
        &mut s,
        "adaptive_place",
        json!({"family": "Panel", "placements": [{"at": [3000, -500], "angle": -45, "offset": 250, "adaptive": capture}]}),
    );
    let framed = placed["ids"][0].as_u64().unwrap();
    assert!(component(&s, framed).adaptive.is_some());
    (s, plane, preview, framed)
}

/// Component `c`'s pieces as the adaptive families placed them before components had hosts: its own, turned `angle` about
/// the vertical at `at`, up its level's elevation and its offset.
fn placed_as_before(s: &Session, c: &Component) -> Vec<[f64; 3]> {
    let base = s.model.elevation(c.level) + c.offset;
    let (sin, cos) = c.angle.to_radians().sin_cos();
    s.model
        .component_local_pieces(c)
        .iter()
        .flat_map(|p| p.tris.iter().flatten())
        .map(|p| [c.at.0 + p[0] * cos - p[1] * sin, c.at.1 + p[0] * sin + p[1] * cos, base + p[2]])
        .collect()
}

fn points(s: &Session, c: &Component) -> Vec<[f64; 3]> {
    s.model.component_pieces(c).into_iter().flat_map(|p| p.tris.into_iter().flatten()).collect()
}

#[test]
fn a_host_on_an_adaptive_component_is_refused() {
    let (mut s, plane, preview, framed) = project();
    let before = serde_json::to_string(&*s.model).unwrap();
    // Placed on the plane: refused, whether the component has frames of its own or stands on the family's test placement.
    for extra in [json!({}), json!({"adaptive": component(&s, framed).adaptive.clone()})] {
        let mut p = json!({"family": "Panel", "at": [0, 0, 0], "host": {"plane": "Ramp"}});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let e = s.execute("component", &p).unwrap_err();
        assert!(e.contains("adaptive components take their place from their points"), "{e}");
    }
    // Put on the plane afterwards: refused the same way, and nothing changes.
    for id in [preview, framed] {
        let e = s.execute("edit_work_plane", &json!({"ids": [id], "host": {"plane": plane}})).unwrap_err();
        assert!(e.contains("adaptive components take their place from their points"), "{e}");
    }
    assert_eq!(serde_json::to_string(&*s.model).unwrap(), before);
    // And a file that holds one is refused by the check.
    let mut m = (*s.model).clone();
    if let Some(Element::Component(c)) = m.elements.get_mut(framed) {
        c.host = Some(Hosted { host: HostRef::Plane(s.model.guid(plane)), uv: [3000.0, 0.0], offset: 0.0 });
    }
    let e = m.check().unwrap_err();
    assert!(e.contains("adaptive components take their place from their points"), "{e}");
}

#[test]
fn adaptive_components_are_placed_by_their_points_as_before() {
    let (s, plane, preview, framed) = project();
    for id in [preview, framed] {
        let c = component(&s, id);
        let placed = s.model.component_placement(c);
        assert!(placed.upright);
        assert_eq!(placed.origin, [c.at.0, c.at.1, s.model.elevation(c.level) + c.offset]);
        let got = points(&s, c);
        assert!(!got.is_empty());
        assert_eq!(got, placed_as_before(&s, c), "component {id} builds where the adaptive families built it");
        // A host it could only be given past the check changes neither where it is nor what it builds.
        let mut hosted = c.clone();
        hosted.host = Some(Hosted { host: HostRef::Plane(s.model.guid(plane)), uv: [0.0, 0.0], offset: 0.0 });
        assert_eq!(s.model.component_placement(&hosted), placed);
        assert_eq!(points(&s, &hosted), got);
    }
    // The instance placed on captured frames builds where they were captured, as in the adaptive families' own tests.
    let corners = |c: &Component| {
        let mut v: Vec<[i64; 3]> = points(&s, c).into_iter().map(|q| q.map(|x| (x * 100.0).round() as i64)).collect();
        v.sort();
        v.dedup();
        v
    };
    assert_eq!(corners(component(&s, framed)), corners(component(&s, preview)));
}

#[test]
fn settling_hosts_leaves_adaptive_components_where_they_are() {
    let (mut s, plane, preview, framed) = project();
    let guid = s.model.guid(plane);
    let mut m = (*s.model).clone();
    for id in [preview, framed] {
        if let Some(Element::Component(c)) = m.elements.get_mut(id) {
            c.host = Some(Hosted { host: HostRef::Plane(guid), uv: [5000.0, 5000.0], offset: 300.0 });
        }
    }
    let kept: Vec<Component> = [preview, framed].iter().map(|id| component(&s, *id).clone()).collect();
    // Read from a file (no model before) and after a command (one before): neither moves them.
    for before in [None, Some(&*s.model)] {
        let mut settled = m.clone();
        settled.settle_hosts(before);
        for (id, was) in [preview, framed].iter().zip(&kept) {
            let Some(Element::Component(c)) = settled.elements.get(*id) else { panic!() };
            assert_eq!((c.at, c.offset), (was.at, was.offset), "component {id} stays where its points put it");
        }
    }
    // Moving the plane in the project moves no adaptive component either.
    let points_before: Vec<_> = kept.iter().map(|c| points(&s, c)).collect();
    run(&mut s, "move", json!({"ids": [plane], "by": [500, 1000]}));
    for ((id, was), pts) in [preview, framed].iter().zip(&kept).zip(&points_before) {
        assert_eq!(component(&s, *id), was);
        assert_eq!(&points(&s, component(&s, *id)), pts);
    }
}
