//! The built-in office families are data: each is read with its pack, and the studio's nine pinned plans are built from the file.

use super::family::{self, Family};
use super::{Interior, Request, build};

const STUDIO_PLANS: [(&str, i64); 9] = [
    ("bar", 8),
    ("narrow_bar", 14),
    ("l_shape", 15),
    ("corner_notch", 15),
    ("stepped", 10),
    ("t_shape", 12),
    ("side_nub", 13),
    ("opposite_wings", 9),
    ("court", 11),
];

#[test]
fn each_office_family_names_its_pack() {
    for name in ["office-pattern", "studio-office"] {
        let fam = Family::parse(family::builtin(name).expect("built in")).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(fam.pack_id.as_deref(), Some(name), "the pack of {name}");
    }
}

#[test]
fn the_studio_office_builds_in_every_shape_with_its_reception_joined_to_the_passage() {
    for (shape, rooms) in STUDIO_PLANS {
        let req = Request {
            archetype: "studio-office".into(),
            seed: 2,
            rooms,
            tolerance: 0,
            shape: Some(shape.into()),
            gridlines: false,
            interior: Interior::Aligned,
            bumps: false,
        };
        let built = build(&req, false).unwrap_or_else(|r| panic!("{shape} {rooms}: {r}"));
        assert_eq!(built.rooms, rooms as usize, "{shape}");
        let graph = built.files.iter().find(|(n, _)| *n == "topology-graph.json").map(|(_, t)| t.clone()).unwrap_or_default();
        let g: serde_json::Value = serde_json::from_str(&graph).expect("a graph");
        let joined = g["edges"].as_array().into_iter().flatten().any(|e| {
            let ends = [e["from"].as_str().unwrap_or(""), e["to"].as_str().unwrap_or("")];
            ends.contains(&"reception") && ends.contains(&"passage")
        });
        assert!(joined, "{shape} {rooms}: reception and passage share no link");
    }
}
