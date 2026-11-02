//! Laying point fire detectors over a room (SP 484.1311500.2020): every point of the room within a detector's reach, a
//! smoke detector's 6.4 m or a heat detector's 3.5 m, and no detector nearer a wall than half a metre.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::family::Category;
use crate::geom::{P2, Region, dist_to_segment, ring_edges};
use crate::model::Element;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

/// A room bounded by walls round `points`, and its region; `level` when the walls and the room are to stand on a level
/// other than the model's lowest.
fn room(s: &mut Session, points: Value, inside: [f64; 2], level: Option<&str>) -> (u64, Region) {
    run(
        s,
        "wall",
        level.map(|l| json!({"points": points, "closed": true, "level": l})).unwrap_or(json!({"points": points, "closed": true})),
    );
    let id =
        run(s, "room", level.map(|l| json!({"point": inside, "level": l})).unwrap_or(json!({"point": inside})))["id"].as_u64().unwrap();
    let region = match s.model.elements.get(id) {
        Some(Element::Room(r)) => s.model.room_region(r).unwrap(),
        _ => panic!("{id} is no room"),
    };
    (id, region)
}

/// The detectors laid, each its point and its family's category.
fn laid(s: &Session, r: &Value) -> Vec<(P2, Category)> {
    r["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| match s.model.elements.get(v.as_u64().unwrap()) {
            Some(Element::Component(c)) => (c.at, s.model.families[&c.family].category),
            e => panic!("{v} is {e:?}"),
        })
        .collect()
}

/// Every point of the region, every 100 mm, within `reach` of a detector; every detector inside and half a metre off the walls.
fn covers(region: &Region, at: &[P2], reach: f64) {
    let rings = || std::iter::once(&region.outer).chain(&region.holes);
    let wall = |p: P2| rings().flat_map(|r| ring_edges(r)).map(|(a, b)| dist_to_segment(p, a, b)).fold(f64::MAX, f64::min);
    let inside = |p: P2| crate::geom::contains(&region.outer, p) && !region.holes.iter().any(|h| crate::geom::contains(h, p));
    for p in at {
        assert!(inside(*p), "{p:?} is in the room");
        assert!(wall(*p) >= 500.0 - 1e-6, "{p:?} is {} from a wall", wall(*p));
    }
    let (lo, hi) = region.outer.iter().fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), p| {
        (P2(lo.0.min(p.0), lo.1.min(p.1)), P2(hi.0.max(p.0), hi.1.max(p.1)))
    });
    let mut y = lo.1;
    while y <= hi.1 {
        let mut x = lo.0;
        while x <= hi.0 {
            let p = P2(x, y);
            if inside(p) {
                let near = at.iter().map(|d| d.dist(p)).fold(f64::MAX, f64::min);
                assert!(near <= reach + 1e-6, "{p:?} is {near} from the nearest detector");
            }
            x += 100.0;
        }
        y += 100.0;
    }
}

#[test]
fn detectors_cover_a_room_within_their_reach_and_stand_off_its_walls() {
    let mut s = Session::new();
    let (id, region) = room(&mut s, json!([[0, 0], [8000, 0], [8000, 6000], [0, 6000]]), [4000.0, 3000.0], None);
    // A smoke detector reaches 6.4 m: one in the middle of a room under 9 m across.
    let r = run(&mut s, "detectors", json!({"room": id}));
    let smoke = laid(&s, &r);
    assert_eq!(smoke.len(), 1);
    assert_eq!(smoke[0].1, Category::SmokeDetector);
    covers(&region, &smoke.iter().map(|d| d.0).collect::<Vec<_>>(), 6400.0);
    // A heat detector reaches 3.5 m, a grid under 4.95 m: two by two.
    let r = run(&mut s, "detectors", json!({"room": id, "kind": "heat"}));
    let heat = laid(&s, &r);
    assert_eq!(heat.len(), 4);
    assert!(heat.iter().all(|d| d.1 == Category::HeatDetector));
    covers(&region, &heat.iter().map(|d| d.0).collect::<Vec<_>>(), 3500.0);
    // On the room's level, at the family's first type.
    let level = match s.model.elements.get(id) {
        Some(Element::Room(r)) => r.level,
        e => panic!("{id} is {e:?}"),
    };
    let ids: Vec<u64> = r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    for i in ids {
        match s.model.elements.get(i) {
            Some(Element::Component(c)) => {
                assert_eq!(c.level, level);
                assert_eq!(c.type_name, "3000");
            }
            e => panic!("{e:?}"),
        }
    }
    // A reach of one's own: a tighter grid.
    let r = run(&mut s, "detectors", json!({"room": id, "radius": 2000}));
    let tight = laid(&s, &r);
    covers(&region, &tight.iter().map(|d| d.0).collect::<Vec<_>>(), 2000.0);
    assert!(tight.len() > heat.len());
}

#[test]
fn detectors_cover_an_l_shaped_room_and_leave_out_the_corner_it_does_not_have() {
    let mut s = Session::new();
    let (id, region) =
        room(&mut s, json!([[0, 0], [20000, 0], [20000, 8000], [10000, 8000], [10000, 16000], [0, 16000]]), [5000.0, 5000.0], None);
    for (kind, reach) in [("smoke", 6400.0), ("heat", 3500.0)] {
        let r = run(&mut s, "detectors", json!({"room": id, "kind": kind}));
        covers(&region, &laid(&s, &r).iter().map(|d| d.0).collect::<Vec<_>>(), reach);
    }
}

#[test]
fn detectors_want_a_room_and_a_detector_family() {
    let mut s = Session::new();
    let (id, _) = room(&mut s, json!([[0, 0], [8000, 0], [8000, 6000], [0, 6000]]), [4000.0, 3000.0], None);
    assert!(fails(&mut s, "detectors", json!({})).contains("room"));
    let wall = s.model.elements.iter().find(|(_, e)| matches!(e, Element::Wall(_))).map(|(k, _)| *k).unwrap();
    assert!(fails(&mut s, "detectors", json!({"room": wall})).contains("room"));
    assert!(fails(&mut s, "detectors", json!({"room": id, "kind": "flame"})).contains("smoke or heat"));
    assert!(fails(&mut s, "detectors", json!({"room": id, "family": "Camera"})).contains("detector"));
    assert!(fails(&mut s, "detectors", json!({"room": id, "radius": 0})).contains("radius"));
    // A reach finer than a whole grid of detectors is refused, not laid.
    assert!(fails(&mut s, "detectors", json!({"room": id, "radius": 1})).contains("detectors"));
    // A room its walls no longer bound has nothing to cover.
    run(&mut s, "delete", json!({"ids": [wall]}));
    assert!(fails(&mut s, "detectors", json!({"room": id})).contains("bound"));
}

#[test]
fn a_higher_ceiling_closes_the_reach_in_proportion_and_more_are_laid() {
    let mut s = Session::new();
    // The room stands on the model's level at 3 m, and a level 6 m above it closes its ceiling there.
    let (id, region) = room(&mut s, json!([[0, 0], [8000, 0], [8000, 6000], [0, 6000]]), [4000.0, 3000.0], Some("Level 2"));
    run(&mut s, "level", json!({"elevation": 9000, "name": "High"}));
    // Detectors made for 3 m of ceiling reach half as far under 6 m: a smoke detector's 6.4 m makes
    // its 3.2 m, so four over a room 8 x 6 m instead of one, a heat detector's 3.5 m its 1.75 m, twelve.
    for (kind, reach, laid_count) in [("smoke", 3200.0, 4), ("heat", 1750.0, 12)] {
        let r = run(&mut s, "detectors", json!({"room": id, "kind": kind}));
        let at = laid(&s, &r).iter().map(|d| d.0).collect::<Vec<_>>();
        assert_eq!(at.len(), laid_count);
        covers(&region, &at, reach);
    }
    // An explicit `radius` pays the ceiling nothing: its own grid.
    let r = run(&mut s, "detectors", json!({"room": id, "radius": 6400}));
    assert_eq!(r["ids"].as_array().unwrap().len(), 1);
}
