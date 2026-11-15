//! A `required` link is there whenever both rooms exist, and a plan is steered so that its ends can share a wall.

use serde_json::Value;

use super::family::{Family, shapes};
use super::{Interior, Request, build_family};

const TOY: &str = include_str!("testdata/toy-family-required.json");

fn request(seed: u64, rooms: i64, shape: &str) -> Request {
    Request {
        archetype: String::new(),
        seed,
        rooms,
        tolerance: 0,
        shape: Some(shape.into()),
        gridlines: false,
        interior: Interior::Aligned,
        bumps: false,
    }
}

fn linked(built: &crate::generate::Built, a: &str, b: &str) -> bool {
    let text = built.files.iter().find(|(n, _)| *n == "topology-graph.json").map(|(_, t)| t.as_str()).unwrap_or("null");
    let g: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    g["edges"].as_array().into_iter().flatten().any(|e| {
        let (f, t) = (e["from"].as_str().unwrap_or(""), e["to"].as_str().unwrap_or(""));
        (f == a && t == b) || (f == b && t == a)
    })
}

#[test]
fn a_required_link_is_in_every_plan_of_every_shape() {
    let fam = Family::parse(TOY).expect("the toy family parses");
    let mut wrong = Vec::new();
    let mut thin = Vec::new();
    for shape in shapes().expect("the catalogue reads") {
        let mut built_n = 0;
        for rooms in [8, 12] {
            for seed in 1..=3 {
                if let Ok(built) = build_family(&fam, &request(seed, rooms, &shape.id), false) {
                    built_n += 1;
                    if !linked(&built, "foyer", "arcade") {
                        wrong.push(format!("{} {rooms} seed {seed}: built without the required link", shape.id));
                    }
                }
            }
        }
        // The steering makes the far end room's cell run the whole core: a shape that cannot do it in most calls is a bug.
        if built_n < 4 {
            thin.push(format!("{}: {built_n} of 6", shape.id));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
    assert!(thin.is_empty(), "{thin:#?}");
}
