//! CON03-W1. Dimensions in the fixture drawing are inches; the command takes and returns mm.
#![cfg(test)]

use bimcraft_core::engine::Session;
use serde_json::{Value, json};

const IN: f64 = 25.4;

fn fixture() -> (Session, Value) {
    let mut s = Session::new();
    s.execute("type", &json!({"category":"wall","name":"Test frame","layers":[{"function":"structure","thickness":3.5*IN}]})).unwrap();
    s.execute("type", &json!({"category":"window","name":"Test opening","width":36.*IN,"height":36.*IN})).unwrap();
    let wall = s.execute("wall", &json!({"type":"Test frame","points":[[0,0],[144.*IN,0]],"height":96.*IN})).unwrap()["ids"][0].clone();
    let window = s.execute("window", &json!({"wall":wall,"type":"Test opening","offset":72.*IN,"sill":36.*IN})).unwrap()["id"].clone();
    let layer = s.model.types.wall["Test frame"].layers[0].guid;
    (s, json!({"wall":wall,"window":window,"layer":layer,"spacing":16.*IN,"thickness":1.5*IN,"depth":3.5*IN,"header_height":5.5*IN}))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn extent(layout: &Value, kind: &str, inches: [f64; 4]) -> String {
    let matched: Vec<_> = layout[kind]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["xz"].as_array().unwrap().iter().zip(inches).all(|(n, i)| close(n.as_f64().unwrap(), i * IN)))
        .collect();
    assert_eq!(matched.len(), 1, "{kind} {inches:?}");
    matched[0]["key"].as_str().unwrap().to_owned()
}

#[test]
fn a_window_move_retires_the_niche_bay_and_preserves_the_outlet_bay() {
    let (mut s, p) = fixture();
    let history = s.history();
    s.read_only = Some("fixture");
    let before = s.execute("timber_framing", &p).unwrap();
    assert_eq!(before, s.execute("timber_framing", &p).unwrap());
    assert_eq!(s.history(), history);
    assert_eq!(before["members"].as_array().unwrap().len(), 21);
    assert_eq!(before["bays"].as_array().unwrap().len(), 14);
    let outlet = extent(&before, "bays", [32.75, 47.25, 1.5, 93.]);
    let niche = extent(&before, "bays", [96.75, 111.25, 1.5, 93.]);
    assert_ne!(extent(&before, "bays", [64.75, 79.25, 1.5, 34.5]), extent(&before, "bays", [64.75, 79.25, 77.5, 93.]));
    s.read_only = None;
    s.execute("modify", &json!({"id":p["window"],"offset":88.*IN})).unwrap();
    let moved = s.execute("timber_framing", &p).unwrap();
    assert_eq!(moved["members"].as_array().unwrap().len(), 21);
    assert_eq!(outlet, extent(&moved, "bays", [32.75, 47.25, 1.5, 93.]));
    assert!(moved["bays"].as_array().unwrap().iter().all(|b| b["key"] != niche));
    // Keep the niche's intended envelope and identify both member penetrations.
    let hits: Vec<_> = moved["members"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| {
            let b = &m["xz"];
            b[0].as_f64().unwrap() < 110.5 * IN
                && b[1].as_f64().unwrap() > 97.5 * IN
                && b[2].as_f64().unwrap() < 60.5 * IN
                && b[3].as_f64().unwrap() > 43.5 * IN
        })
        .map(|m| m["key"].as_str().unwrap())
        .collect();
    assert_eq!(hits.len(), 2);
    assert!(hits.contains(&extent(&moved, "members", [106., 107.5, 1.5, 72.]).as_str()));
    assert!(hits.contains(&extent(&moved, "members", [107.5, 109., 1.5, 93.]).as_str()));
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(before, s.execute("timber_framing", &p).unwrap());
    s.execute("redo", &json!({})).unwrap();
    assert_eq!(moved, s.execute("timber_framing", &p).unwrap());
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("wall.bimcraft");
    s.execute("save", &json!({"path":file})).unwrap();
    let saved = std::fs::read(&file).unwrap();
    let mut reopened = Session::new();
    reopened.execute("open", &json!({"path":file})).unwrap();
    reopened.read_only = Some("fixture");
    assert_eq!(moved, reopened.execute("timber_framing", &p).unwrap());
    assert_eq!(saved, std::fs::read(file).unwrap());
}

#[test]
fn member_faces_and_clear_bays_match_the_dimensioned_wall() {
    let (mut s, p) = fixture();
    let layout = s.execute("timber_framing", &p).unwrap();
    for rect in [
        [0., 144., 0., 1.5],
        [0., 144., 93., 94.5],
        [0., 144., 94.5, 96.],
        [0., 1.5, 1.5, 93.],
        [142.5, 144., 1.5, 93.],
        [51., 52.5, 1.5, 93.],
        [52.5, 54., 1.5, 72.],
        [90., 91.5, 1.5, 72.],
        [91.5, 93., 1.5, 93.],
        [52.5, 91.5, 72., 77.5],
        [54., 90., 34.5, 36.],
    ] {
        extent(&layout, "members", rect);
    }
    for x in [16., 32., 48., 96., 112., 128.] {
        extent(&layout, "members", [x - 0.75, x + 0.75, 1.5, 93.]);
    }
    for x in [64., 80.] {
        extent(&layout, "members", [x - 0.75, x + 0.75, 1.5, 34.5]);
        extent(&layout, "members", [x - 0.75, x + 0.75, 77.5, 93.]);
    }
    for [l, r] in
        [[1.5, 15.25], [16.75, 31.25], [32.75, 47.25], [48.75, 51.], [93., 95.25], [96.75, 111.25], [112.75, 127.25], [128.75, 142.5]]
    {
        extent(&layout, "bays", [l, r, 1.5, 93.]);
    }
    for [l, r] in [[54., 63.25], [64.75, 79.25], [80.75, 90.]] {
        extent(&layout, "bays", [l, r, 1.5, 34.5]);
    }
    for [l, r] in [[52.5, 63.25], [64.75, 79.25], [80.75, 91.5]] {
        extent(&layout, "bays", [l, r, 77.5, 93.]);
    }

    // Mounting face contact is allowed. Free-side clearance, recess lining and depth are fixture inputs.
    let fits = |key: &str, [l, r, b, t, d]: [f64; 5]| {
        let bay = layout["bays"].as_array().unwrap().iter().find(|v| v["key"] == key).unwrap();
        let xz = &bay["xz"];
        l * IN >= xz[0].as_f64().unwrap() - 1e-6
            && r * IN <= xz[1].as_f64().unwrap() + 1e-6
            && b * IN >= xz[2].as_f64().unwrap() - 1e-6
            && t * IN <= xz[3].as_f64().unwrap() + 1e-6
            && d * IN <= layout["depth"].as_f64().unwrap() + 1e-6
    };
    let outlet = extent(&layout, "bays", [32.75, 47.25, 1.5, 93.]);
    assert!(fits(&outlet, [32.75, 35.25, 15.75, 20.25, 2.25]));
    assert!(!fits(&outlet, [32.5, 35., 15.75, 20.25, 2.25]));
    assert!(!fits(&outlet, [32.625, 35.25, 15.75, 20.25, 2.25]));
    let niche = extent(&layout, "bays", [96.75, 111.25, 1.5, 93.]);
    assert!(fits(&niche, [97.5, 110.5, 43.5, 60.5, 3.]));
    assert!(!fits(&niche, [97.5, 110.5, 43.5, 60.5, 3.75]));
    assert!(!fits(&niche, [96.5, 111.5, 43.5, 60.5, 3.]));
}

#[test]
fn unsupported_rules_layers_and_hosts_refuse_without_an_undo_step() {
    let (mut s, p) = fixture();
    let history = s.history();
    for (field, value) in
        [("spacing", json!(0)), ("depth", json!(100)), ("layer", json!(0)), ("window", p["wall"].clone()), ("surprise", json!(true))]
    {
        let mut bad = p.clone();
        bad[field] = value;
        assert!(s.execute("timber_framing", &bad).is_err(), "{field}");
    }
    let mut too_many = p.clone();
    too_many["spacing"] = json!(0.2);
    too_many["thickness"] = json!(0.1);
    assert!(s.execute("timber_framing", &too_many).unwrap_err().contains("stations"));
    assert_eq!(s.history(), history);
    s.execute("door", &json!({"wall":p["wall"],"offset":500})).unwrap();
    assert!(s.execute("timber_framing", &p).unwrap_err().contains("other openings"));
    s.execute("undo", &json!({})).unwrap();
    for points in [json!([[0, 0], [0, 2000]]), json!([[1000, 0], [1000, 2000]]), json!([[1000, -2000], [1000, 2000]])] {
        s.execute("wall", &json!({"points":points})).unwrap();
        assert!(s.execute("timber_framing", &p).unwrap_err().contains("isolated"));
        s.execute("undo", &json!({})).unwrap();
    }
    s.execute("wall", &json!({"points":[[0,1000],[4000,1000]]})).unwrap();
    assert!(s.execute("timber_framing", &p).is_ok());
}

#[test]
fn the_framing_axes_follow_the_wall_and_keys_survive_rigid_motion() {
    let (mut s, p) = fixture();
    let a = s.execute("timber_framing", &p).unwrap();
    s.execute("rotate", &json!({"ids":[p["wall"]],"about":[0,0],"angle":90})).unwrap();
    s.execute("move", &json!({"ids":[p["wall"]],"by":[320,640]})).unwrap();
    let b = s.execute("timber_framing", &p).unwrap();
    for kind in ["members", "bays"] {
        let original = a[kind].as_array().unwrap();
        let transformed = b[kind].as_array().unwrap();
        assert_eq!(original.len(), transformed.len());
        for (before, after) in original.iter().zip(transformed) {
            assert_eq!(before["key"], after["key"]);
            for i in 0..4 {
                assert!(close(before["xz"][i].as_f64().unwrap(), after["xz"][i].as_f64().unwrap()));
            }
        }
    }
    for (field, expected) in [("origin", [320. + 1.75 * IN, 640.]), ("x_axis", [0., 1.]), ("depth_axis", [-1., 0.])] {
        for (i, value) in expected.into_iter().enumerate() {
            assert!(close(b[field][i].as_f64().unwrap(), value), "{field}");
        }
    }
}
