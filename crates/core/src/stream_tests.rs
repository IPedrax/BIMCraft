//! A 3D view of a model too big for the memory there is: what [`crate::view3d::stream`] keeps, and that the scene of what it
//! keeps is, element by element, the whole scene's.

use std::collections::{BTreeSet, HashSet};

use crate::engine::Session;
use crate::model::Element;
use crate::view3d::{BYTES_PER_TRIANGLE, Scene, stream};

fn tower(storeys: u32) -> Session {
    let mut s = Session::new();
    s.execute("tower", &serde_json::json!({"storeys": storeys, "plate": "small"})).unwrap();
    s
}

#[test]
fn a_model_that_fits_is_held_whole_and_a_scene_of_all_of_it_is_the_whole_scene() {
    let s = tower(2);
    assert_eq!(stream(&s.model, u64::MAX, 0.0), None);
    let all: BTreeSet<_> = s.model.elements.keys().copied().collect();
    let whole = Scene::new(&s.model);
    let among = Scene::new_among_cancellable(&s.model, Some(&all), &|| false).unwrap();
    let ids: HashSet<_> = all.iter().copied().collect();
    assert_eq!(whole.boxes(&ids), among.boxes(&ids));
}

#[test]
fn a_model_too_big_keeps_its_fabric_and_the_detail_of_the_storeys_round_where_it_looks() {
    let s = tower(6);
    let m = &s.model;
    let levels = m.levels();
    let (_, third) = levels[2];
    // Room for about two storeys' detail besides the fabric.
    let fabric: u64 = m.elements.values().filter(|e| !matches!(e, Element::Component(_)) && e.run().is_none()).count() as u64;
    let budget = fabric * 200 * BYTES_PER_TRIANGLE;
    let part = stream(m, budget, third.elevation + 100.0).expect("too big for the budget");
    for (id, e) in &m.elements {
        let detail = matches!(e, Element::Component(_) | Element::Rebar(_)) || e.run().is_some();
        if !detail {
            assert!(part.among.contains(id), "fabric {id} ({}) is always held", e.category());
        }
    }
    assert!(part.left_out > 0, "some detail is left out");
    assert!(part.covers(third.elevation + 100.0), "{part:?}");
    assert!(!part.covers(f64::INFINITY) || !part.covers(f64::NEG_INFINITY), "not every storey: {:?}", part.band);
    // What it holds is, element by element, what the whole scene has.
    let whole = Scene::new(m);
    let held = Scene::new_among_cancellable(m, Some(&part.among), &|| false).unwrap();
    let ids: HashSet<_> = part.among.iter().copied().collect();
    assert_eq!(held.boxes(&ids), whole.boxes(&ids));
    // Looking at the top storey, the top storey's detail.
    let (_, top) = levels[levels.len() - 1];
    let up = stream(m, budget, top.elevation + 100.0).unwrap();
    assert!(up.covers(top.elevation + 100.0) && up != part);
}
