//! A family read from a file (`generate --family-file`): the toy `workshop` family, which no code has seen, through the engine.

use serde_json::Value;

use super::family::{Family, shapes};
use super::{Request, build_family};

const TOY: &str = include_str!("../testdata/shape-graph/toy-family.json");

fn toy_request(k: u64) -> Request {
    let ids: Vec<String> = shapes().expect("the catalogue reads").into_iter().map(|s| s.id).collect();
    Request {
        archetype: String::new(),
        seed: k + 1,
        rooms: 7 + (k % 12) as i64,
        tolerance: 0,
        shape: Some(ids[(k as usize) % ids.len()].clone()),
        gridlines: false,
        interior: super::Interior::Aligned,
        bumps: false,
    }
}

#[test]
fn toy_family_lays_out_every_brief() {
    let fam = Family::parse(TOY).expect("the toy family parses");
    for k in 0..12 {
        let req = toy_request(k);
        match build_family(&fam, &req, false) {
            Ok(built) => assert_eq!(built.rooms as i64, req.rooms, "brief {k}"),
            Err(r) => panic!("brief {k} ({req:?}): {r}"),
        }
    }
}

#[test]
fn toy_family_swing_doors_say_where_they_open() {
    let fam = Family::parse(TOY).expect("the toy family parses");
    let built = build_family(&fam, &toy_request(3), false).expect("a plan");
    let text = built.files.iter().find(|(n, _)| *n == "topology-graph.json").map(|(_, t)| t.clone()).unwrap_or_default();
    let v: Value = serde_json::from_str(&text).expect("the graph is JSON");
    let mut swings = 0;
    for e in v["edges"].as_array().into_iter().flatten() {
        if matches!(e["operation"].as_str(), Some("swing" | "double-swing")) {
            swings += 1;
            assert!(!e["opens_into"].is_null() && !e["hinge"].is_null(), "{e}");
        }
    }
    assert!(swings > 0);
}
