//! Spot coordinates and their types, as Revit's: a leader from the point to a shoulder, its arrowhead, its symbol, and the
//! north, east and elevation it reads, through the commands every front end drives
//! ([coordinates](../../../docs/areas/coordinates.md)).

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::spots::{Arrowhead, DEFAULT, SURVEY};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

fn element(s: &mut Session, id: u64) -> Value {
    let all = run(s, "elements", json!({}));
    all["elements"].as_array().and_then(|es| es.iter().find(|e| e["id"] == id).cloned()).unwrap_or(Value::Null)
}

/// A project whose Project Base Point is at (1000, 2000, 300), at map coordinates far off.
fn surveyed() -> Session {
    let mut s = Session::new();
    run(&mut s, "location", json!({"name": "Internal", "elevation": 760_000.0, "east": 333_000_000.0, "north": 7_394_000_000.0}));
    run(&mut s, "coordinates", json!({"base": [1000.0, 2000.0, 300.0]}));
    s
}

#[test]
fn a_project_starts_with_spot_coordinate_types_from_each_point() {
    let mut s = Session::new();
    let r = run(&mut s, "spot_coordinate_type", json!({}));
    let types = r["types"].as_object().unwrap();
    assert!(types.contains_key(DEFAULT) && types.contains_key(SURVEY), "{r}");
    assert_eq!(types[DEFAULT]["origin"], "base");
    assert_eq!(types[SURVEY]["origin"], "survey");
    assert_eq!(types[DEFAULT]["leader_arrowhead"], "filled");
    assert!(s.model.library.entries().any(|(_, e)| e.name == DEFAULT && e.kind == crate::library::Kind::SpotCoordinateType));
}

#[test]
fn a_spot_coordinate_reads_as_its_type_says() {
    let mut s = surveyed();
    let r = run(&mut s, "spot_coordinate", json!({"origin": "base", "at": [4000, 6000]}));
    assert_eq!(r["type"], DEFAULT, "{r}");
    assert_eq!(r["text"], json!(["N 4000", "E 3000"]), "{r}");
    let id = r["id"].as_u64().unwrap();
    // `origin: survey` takes the type that reads from the Survey Point.
    let r = run(&mut s, "spot_coordinate", json!({"at": [4000, 6000], "origin": "survey"}));
    assert_eq!(r["type"], SURVEY, "{r}");
    assert_eq!(r["north"], 7_394_006_000.0);
    // A type of its own: east on top, the indicators after the numbers, the elevation below, in metres to 2 places.
    run(
        &mut s,
        "spot_coordinate_type",
        json!({"name": "Mine", "like": DEFAULT, "top": "east_west", "bottom": "north_south", "indicator": "suffix",
               "north_south_indicator": " N", "east_west_indicator": " E", "include_elevation": true,
               "elevation_indicator": " EL", "units": "m", "decimals": 2}),
    );
    run(&mut s, "modify", json!({"id": id, "type": "Mine"}));
    assert_eq!(element(&mut s, id)["text"], json!(["3.00 E", "4.00 N", "-0.30 EL"]));
    // From the internal origin: the file's own numbers.
    run(&mut s, "spot_coordinate_type", json!({"name": "Mine", "origin": "internal", "include_elevation": false, "bottom": "none"}));
    assert_eq!(element(&mut s, id)["text"], json!(["4.00 E"]));
}

#[test]
fn a_spot_coordinate_on_a_floor_reads_the_floors_top_as_its_elevation() {
    let mut s = Session::new();
    run(&mut s, "floor", json!({"boundary": [[0, 0], [5000, 0], [5000, 5000], [0, 5000]]}));
    run(&mut s, "spot_coordinate_type", json!({"name": DEFAULT, "include_elevation": true}));
    let r = run(&mut s, "spot_coordinate", json!({"origin": "base", "at": [1000, 1000]}));
    assert_eq!(r["text"][2], "EL 0", "{r}");
    assert!(r["host"].is_u64(), "it reads the floor: {r}");
}

#[test]
fn a_spot_coordinate_has_a_leader_and_a_shoulder_that_can_be_turned_off_and_moved() {
    let mut s = Session::new();
    let id = run(&mut s, "spot_coordinate", json!({"origin": "base", "at": [1000, 1000], "end": [3000, 2500]}))["id"].as_u64().unwrap();
    let e = element(&mut s, id);
    assert_eq!(e["end"], json!([3000.0, 2500.0]), "{e}");
    assert_eq!((e["leader"].as_bool(), e["shoulder"].as_bool()), (Some(true), Some(true)), "{e}");
    run(&mut s, "modify", json!({"id": id, "leader": false, "shoulder": false}));
    let e = element(&mut s, id);
    assert_eq!((e["leader"].as_bool(), e["shoulder"].as_bool()), (Some(false), Some(false)), "{e}");
    // Moved, its leader's end goes with it.
    run(&mut s, "move", json!({"ids": [id], "by": [500, 0]}));
    assert_eq!(element(&mut s, id)["end"], json!([3500.0, 2500.0]));
}

#[test]
fn spot_coordinate_types_take_every_arrowhead_and_refuse_what_cant_be_drawn() {
    let mut s = Session::new();
    for (name, a) in [
        ("filled", Arrowhead::Filled),
        ("empty", Arrowhead::Empty),
        ("dot", Arrowhead::Dot),
        ("empty_dot", Arrowhead::EmptyDot),
        ("none", Arrowhead::None),
    ] {
        let r = run(&mut s, "spot_coordinate_type", json!({"name": DEFAULT, "leader_arrowhead": name, "arrowhead_size": 3.5}));
        assert_eq!(r["type"]["leader_arrowhead"], name);
        assert_eq!(s.model.spot_coordinate_types[DEFAULT].leader_arrowhead, a);
    }
    assert_eq!(s.model.spot_coordinate_types[DEFAULT].arrowhead_size, 3.5);
    let e = fails(&mut s, "spot_coordinate_type", json!({"name": DEFAULT, "leader_arrowhead": "harpoon"}));
    assert!(e.contains("harpoon"), "{e}");
    let e = fails(&mut s, "spot_coordinate_type", json!({"name": DEFAULT, "arrowhead_size": 80}));
    assert!(e.contains("arrowhead size"), "{e}");
    let e = fails(&mut s, "spot_coordinate_type", json!({"name": DEFAULT, "leader_pen": 40}));
    assert!(e.contains("pen") || e.contains("weight"), "{e}");
    let e = fails(&mut s, "spot_coordinate_type", json!({"name": DEFAULT, "leader_color": "#zz0000"}));
    assert!(e.contains("leader_color"), "{e}");
    let e = fails(&mut s, "spot_coordinate_type", json!({"name": DEFAULT, "north_south_indicator": "a\nb"}));
    assert!(e.contains("indicator"), "{e}");
    let e = fails(&mut s, "spot_coordinate_type", json!({"name": DEFAULT, "text_size": 0}));
    assert!(e.contains("text size"), "{e}");
}

#[test]
fn a_spot_coordinate_type_is_renamed_with_its_spots_and_kept_while_used() {
    let mut s = Session::new();
    let id = run(&mut s, "spot_coordinate", json!({"origin": "base", "at": [0, 0]}))["id"].as_u64().unwrap();
    run(&mut s, "spot_coordinate_type", json!({"name": DEFAULT, "rename": "Site"}));
    assert_eq!(element(&mut s, id)["type"], "Site");
    let e = fails(&mut s, "spot_coordinate_type", json!({"name": "Site", "delete": true}));
    assert!(e.contains(&id.to_string()), "{e}");
    let e = fails(&mut s, "modify", json!({"id": id, "type": "Nothing"}));
    assert!(e.contains("Nothing"), "{e}");
    run(&mut s, "spot_coordinate_type", json!({"name": "Spare", "like": "Site"}));
    run(&mut s, "spot_coordinate_type", json!({"name": "Spare", "delete": true}));
    assert!(!s.model.spot_coordinate_types.contains_key("Spare"));
}

/// The plan of `s`'s first level at 1 px to 20 mm centred on `c`, as a PNG; the pixel of a model point, and how many px a mm
/// on paper is in it.
fn drawn(s: &Session, c: P2) -> (Vec<u8>, impl Fn(P2) -> (f32, f32), f32) {
    let m = &s.model;
    let level = m.elements.iter().find(|(_, e)| matches!(e, crate::model::Element::Level(_))).map(|(id, _)| *id).unwrap();
    let view = crate::render::View { angle: 0.0, scale: 0.05, cx: c.0, cy: c.1, width: 800, height: 600 };
    let d = crate::render::draw(m, level, view);
    let png = crate::render::png(&d).unwrap();
    (png, move |p: P2| view.px(p), d.k * crate::sheet::DENSITY as f32)
}

fn pixel(png: &[u8], (x, y): (f32, f32)) -> (u8, u8, u8) {
    let pm = tiny_skia::Pixmap::decode_png(png).unwrap();
    pm.pixel(x.max(0.0) as u32, y.max(0.0) as u32).map_or((255, 255, 255), |c| (c.red(), c.green(), c.blue()))
}

/// Whether some pixel within `r` px of `at` is reddish (`red`) or bluish.
fn tinted(png: &[u8], at: (f32, f32), r: i32, red: bool) -> bool {
    (-r..=r).any(|dx| {
        (-r..=r).any(|dy| {
            let (pr, _, pb) = pixel(png, (at.0 + dx as f32, at.1 + dy as f32));
            if red { pr > 180 && pb < 90 } else { pb > 180 && pr < 90 }
        })
    })
}

#[test]
fn a_spot_coordinate_draws_its_leader_and_its_text_in_their_own_colours() {
    let mut s = Session::new();
    run(
        &mut s,
        "spot_coordinate_type",
        json!({"name": DEFAULT, "leader_color": "#ff0000", "text_color": "#0000ff", "symbol": "none", "arrowhead_size": 4,
               "leader_pen": 5}),
    );
    run(&mut s, "spot_coordinate", json!({"origin": "base", "at": [0, 0], "end": [2000, 2000]}));
    let (png, px, _) = drawn(&s, P2(1000.0, 1000.0));
    // Half way along the leader: red.
    assert!(tinted(&png, px(P2(1000.0, 1000.0)), 2, true), "the leader in its colour");
    // Over the shoulder, past its start: the text, blue.
    let (x, y) = px(P2(2000.0, 2000.0));
    let blue = (0..60).any(|dx| (-30..0).any(|dy| tinted(&png, (x + dx as f32, y + dy as f32), 0, false)));
    assert!(blue, "the text in its colour");
}

#[test]
fn each_arrowhead_draws_its_own_shape_at_the_point() {
    let inked = |png: &[u8], at: (f32, f32)| {
        let (r, g, b) = pixel(png, at);
        u32::from(r) + u32::from(g) + u32::from(b) < 450
    };
    for (a, arrow, dot) in
        [("filled", true, false), ("empty", false, false), ("dot", false, true), ("empty_dot", false, false), ("none", false, false)]
    {
        let mut s = Session::new();
        run(
            &mut s,
            "spot_coordinate_type",
            json!({"name": DEFAULT, "leader_arrowhead": a, "arrowhead_size": 5, "symbol": "none",
            "arrowhead_pen": 5}),
        );
        // The leader comes to the point from the west, level, with no shoulder: the head is all there is east of 2 m west.
        run(&mut s, "spot_coordinate", json!({"origin": "base", "at": [0, 0], "end": [-3000, 0], "shoulder": false}));
        let (png, px, k) = drawn(&s, P2(0.0, 0.0));
        let (x, y) = px(P2(0.0, 0.0));
        // Inside an arrowhead 5 mm long, beside the leader's line; inside a dot on the point.
        assert_eq!(inked(&png, (x - 2.5 * k, y - 0.4 * k)), arrow, "{a}: inside an arrowhead");
        assert_eq!(inked(&png, (x + 0.4 * k, y - 0.6 * k)), dot, "{a}: inside a dot");
        // An outline has its edge: an empty arrowhead's back, an empty dot's rim.
        let edge = match a {
            "empty" => (0..6).any(|i| inked(&png, (x - 5.0 * k + i as f32 * 0.2, y - 0.5 * k))),
            "empty_dot" => (0..8).any(|i| inked(&png, (x + 1.0 * k + i as f32 * 0.4, y))),
            _ => true,
        };
        assert!(edge, "{a}: its outline");
    }
}
