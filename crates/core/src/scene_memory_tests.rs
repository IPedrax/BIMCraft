//! A 3D view holds every element of its model, however big: what it is told it needs ([`crate::view3d::estimate`]) only
//! says so, and the scene of a big model is the whole scene.

use std::collections::{BTreeSet, HashSet};

use crate::engine::Session;
use crate::view3d::{BYTES_PER_TRIANGLE, Scene, estimate};

fn tower(storeys: u32) -> Session {
    let mut s = Session::new();
    s.execute("tower", &serde_json::json!({"storeys": storeys, "plate": "small"})).unwrap();
    s
}

#[test]
fn a_bigger_model_is_estimated_to_need_more_and_its_scene_holds_every_element() {
    let (small, big) = (tower(2), tower(4));
    assert!(estimate(&big.model) > estimate(&small.model));
    assert_eq!(estimate(&big.model) % BYTES_PER_TRIANGLE, 0);
    // Every element that has triangles is in the scene: none is left out for its size.
    let scene = Scene::new(&big.model);
    let all: HashSet<_> = big.model.elements.keys().copied().collect();
    let held: BTreeSet<_> = scene.boxes(&all).into_keys().collect();
    for (id, e) in &big.model.elements {
        if matches!(e, crate::model::Element::Component(_)) || e.run().is_some() {
            assert!(held.contains(id), "{} {id} is in the scene", e.category());
        }
    }
}
