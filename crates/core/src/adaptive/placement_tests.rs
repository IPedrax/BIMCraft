//! What an adaptive component's points decide alone: Default Elevation never lifts one, as Revit places it by its points, and
//! the cached bounds of one follow its points when `adaptive_move` moves them.

use serde_json::{Value, json};

use super::four_point_panel;
use crate::bounds::{Bounds, BoundsCache};
use crate::engine::Session;
use crate::model::{Element, Id};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A project with the four-point panel as `Panel`, its options `default_elevation` `elevation`.
fn project(elevation: f64) -> Session {
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({"name": "Panel", "types": {"A": {}}, "adaptive": four_point_panel(), "options": {"default_elevation": elevation}}),
    );
    s
}

/// What `component` and `adaptive_place` make of the panel, given no `offset`: its offset, and where its points and
/// triangles are in the project.
fn placed(s: &mut Session) -> Vec<(f64, Vec<[f64; 3]>)> {
    let plain = run(s, "component", json!({"family": "Panel", "at": [100, 200], "angle": 30}))["id"].as_u64().unwrap();
    let sources: Vec<Value> = (1..=4).map(|n| json!({"id": plain, "host": {"type": "frame", "point": format!("p{n}")}})).collect();
    let capture = run(s, "adaptive_capture", json!({ "sources": sources }));
    let framed = run(s, "adaptive_place", json!({"family": "Panel", "placements": [{"at": [3000, -500], "adaptive": capture}]}))["ids"][0]
        .as_u64()
        .unwrap();
    // The family's test placement given as the instance's own frames.
    let Some(Element::Component(c)) = s.model.elements.get(plain) else { panic!("component") };
    let own = json!(s.model.adaptive_placement(c));
    let by_hand = run(s, "component", json!({"family": "Panel", "at": [0, 0], "adaptive": own}))["id"].as_u64().unwrap();
    [plain, framed, by_hand]
        .into_iter()
        .map(|id| {
            let Some(Element::Component(c)) = s.model.elements.get(id) else { panic!("{id} is no component") };
            let mut points: Vec<[f64; 3]> = s.model.component_pieces(c).into_iter().flat_map(|p| p.tris.into_iter().flatten()).collect();
            let axes = s.model.component_axes(c).unwrap();
            let placement = s.model.adaptive_placement(c).unwrap();
            points.extend(placement.frames.values().map(|f| f.from_local(axes, c.mirrored).unwrap().origin));
            (c.offset, points)
        })
        .collect()
}

#[test]
fn default_elevation_never_lifts_an_adaptive_component() {
    let flat = placed(&mut project(0.0));
    let lifted = placed(&mut project(900.0));
    assert!(flat.iter().all(|(_, points)| !points.is_empty()));
    for (n, (got, want)) in lifted.iter().zip(&flat).enumerate() {
        assert_eq!(got.0, 0.0, "placement {n} stands on its level, not at its family's Default Elevation");
        assert_eq!(got, want, "placement {n}'s points and geometry are where they are with a Default Elevation of 0");
    }
    // An offset given is still the offset.
    let mut s = project(900.0);
    let id = run(&mut s, "component", json!({"family": "Panel", "at": [0, 0], "offset": 250}))["id"].as_u64().unwrap();
    let Some(Element::Component(c)) = s.model.elements.get(id) else { panic!("component") };
    assert_eq!(c.offset, 250.0);
}

fn bounds_of(cache: &mut BoundsCache, s: &Session, id: Id) -> [[f64; 3]; 2] {
    match cache.get(&s.model, id) {
        Bounds::Box(b) => b,
        other => panic!("component {id} has no box: {other:?}"),
    }
}

/// The points of component `id`'s geometry in the project.
fn built(s: &Session, id: Id) -> Vec<[f64; 3]> {
    let Some(Element::Component(c)) = s.model.elements.get(id) else { panic!("{id} is no component") };
    s.model.component_pieces(c).into_iter().flat_map(|p| p.tris.into_iter().flatten()).collect()
}

fn covers([lo, hi]: [[f64; 3]; 2], p: [f64; 3]) -> bool {
    (0..3).all(|i| lo[i] <= p[i] && p[i] <= hi[i])
}

#[test]
fn cached_bounds_follow_a_moved_adaptive_point() {
    let mut s = project(0.0);
    // The panel alone, and the panel with an ordinary form of its own (a kerb), whose box alone would leave the panel out.
    run(
        &mut s,
        "family",
        json!({"name": "Kerbed", "types": {"A": {}}, "adaptive": four_point_panel(),
               "forms": [{"profile": [[0, 0], [400, 0], [400, 300], [0, 300]], "to": 100}]}),
    );
    for family in ["Kerbed", "Panel"] {
        let id = run(&mut s, "component", json!({"family": family, "at": [100, 200]}))["id"].as_u64().unwrap();
        let mut cache = BoundsCache::default();
        let first = bounds_of(&mut cache, &s, id);
        assert!(first[1][0] < 5000.0 && first[1][1] < 5000.0, "{family}: the panel is about 1200 by 900: {first:?}");
        assert!(built(&s, id).into_iter().all(|p| covers(first, p)), "{family}: the box holds what the component builds");
        let far = [20000.0, 30000.0, 4000.0];
        run(&mut s, "adaptive_move", json!({"id": id, "number": 3, "to": far}));
        let moved = bounds_of(&mut cache, &s, id);
        assert!(covers(moved, far), "{family}: {far:?} outside {moved:?}");
        assert!(built(&s, id).into_iter().all(|p| covers(moved, p)), "{family}: the box holds what the component builds now");
        // And the box held is the one a fresh cache builds.
        assert_eq!(cache.get(&s.model, id), BoundsCache::default().get(&s.model, id));
    }
}
