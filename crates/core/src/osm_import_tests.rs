//! `import_osm`: buildings from an OpenStreetMap file made into masses on the ground, through `Session::execute`.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::Element;

const HOME: (f64, f64) = (-46.6333, -23.5505);

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn refused(s: &mut Session, p: Value) -> String {
    s.execute("import_osm", &p).expect_err(&format!("import_osm {p} should be refused"))
}

/// A project placed at `HOME`.
fn placed() -> Session {
    let mut s = Session::new();
    run(&mut s, "geolocate", json!({"latitude": HOME.1, "longitude": HOME.0}));
    s
}

/// A square about `size` metres across with its corner at `HOME` moved by (`east`, `north`) degrees, closed.
fn square(east: f64, north: f64, size: f64) -> Vec<(f64, f64)> {
    let (lon, lat) = (HOME.0 + east, HOME.1 + north);
    let (dx, dy) = (size * 0.000_01, size * 0.000_009);
    vec![(lon, lat), (lon + dx, lat), (lon + dx, lat + dy), (lon, lat + dy), (lon, lat)]
}

fn geometry(points: &[(f64, f64)]) -> String {
    points.iter().map(|(lon, lat)| format!(r#"{{"lat":{lat},"lon":{lon}}}"#)).collect::<Vec<_>>().join(",")
}

fn way(id: i64, points: &[(f64, f64)], tags: &str) -> String {
    format!(r#"{{"type":"way","id":{id},"geometry":[{}],"tags":{{{tags}}}}}"#, geometry(points))
}

fn file(elements: &[String]) -> String {
    format!(r#"{{"version":0.6,"elements":[{}]}}"#, elements.join(","))
}

fn masses(s: &Session) -> Vec<(u64, crate::model::Mass)> {
    s.model.elements.iter().filter_map(|(id, e)| if let Element::Mass(m) = e { Some((*id, m.clone())) } else { None }).collect()
}

fn osm_id(m: &crate::model::Mass) -> String {
    m.params.get("OSM Id").and_then(Value::as_str).unwrap_or_default().to_string()
}

fn two_houses() -> String {
    file(&[
        way(1, &square(0.0, 0.0, 20.0), r#""building":"house","height":"12","name":"Casa""#),
        way(2, &square(0.0005, 0.0, 30.0), r#""building":"yes","building:levels":"3""#),
    ])
}

#[test]
fn buildings_become_masses_with_their_heights_and_their_osm_ids() {
    let mut s = placed();
    let r = run(&mut s, "import_osm", json!({"data": two_houses()}));
    assert_eq!((r["made"].clone(), r["updated"].clone(), r["skipped"].clone()), (json!(2), json!(0), json!(0)), "{r}");
    let all = masses(&s);
    assert_eq!(all.len(), 2);
    let (_, house) = all.iter().find(|(_, m)| osm_id(m) == "way/1").unwrap();
    assert_eq!((house.name.as_str(), house.height, house.base_offset), ("Casa", 12000.0, 0.0));
    assert_eq!(house.params.get("OSM Building"), Some(&json!("house")));
    let area = crate::geom::area(&house.footprint());
    assert!((380e6..430e6).contains(&area), "20 m by 20 m, about: {area} mm²");
    assert!(house.footprint().iter().all(|p| p.0 > -1.0 && p.1 > -1.0), "from the origin, which is where the location is");
    let (_, other) = all.iter().find(|(_, m)| osm_id(m) == "way/2").unwrap();
    assert_eq!(other.height, 9000.0, "three levels of 3 m");
    assert_eq!(other.name, "way/2", "named for its id when the file names it nothing");
    assert!(r["attribution"].as_str().unwrap().contains("OpenStreetMap"));
    // The parameters it declared are text parameters of masses, and the project still checks.
    assert_eq!(s.model.parameters["OSM Id"].categories, ["mass"]);
}

#[test]
fn buildings_stand_on_the_terrain_at_its_lowest_under_them() {
    let mut s = placed();
    // 0 to 20 m rising with x over 200 m square: the ground is x / 10 high.
    let points: Vec<Value> =
        (0..21).flat_map(|i| (0..21).map(move |j| json!([i as f64 * 10000.0, j as f64 * 10000.0, i as f64 * 1000.0]))).collect();
    run(&mut s, "toposurface", json!({"points": points}));
    let data = file(&[way(1, &square(0.0005, 0.0005, 20.0), r#""building":"yes","height":"10""#)]);
    let r = run(&mut s, "import_osm", json!({"data": data}));
    let (_, m) = masses(&s).remove(0);
    // About 51 m east of the origin: 5.1 m of ground at the west wall.
    assert!((m.base_offset - 5100.0).abs() < 60.0, "{}", m.base_offset);
    assert_eq!(m.height, 10000.0);
    assert_eq!(r["off_terrain"], 0);
    // Past the edge of the terrain there is no ground to find: it stands at the level, and the report says so.
    let far = file(&[way(2, &square(0.01, 0.0, 20.0), r#""building":"yes","height":"10""#)]);
    let r = run(&mut s, "import_osm", json!({"data": far}));
    assert_eq!(r["off_terrain"], 1);
    let (_, m) = masses(&s).into_iter().find(|(_, m)| osm_id(m) == "way/2").unwrap();
    assert_eq!(m.base_offset, 0.0);
}

#[test]
fn a_part_lifted_off_the_ground_stands_on_its_min_height() {
    let mut s = placed();
    let data = file(&[way(1, &square(0.0, 0.0, 20.0), r#""building:part":"yes","height":"20","min_height":"8""#)]);
    run(&mut s, "import_osm", json!({"data": data, "parts": true}));
    let (_, m) = masses(&s).remove(0);
    assert_eq!((m.base_offset, m.height), (8000.0, 12000.0), "from 8 m up to 20 m");
}

#[test]
fn importing_again_updates_the_masses_in_place_and_leaves_the_rest() {
    let mut s = placed();
    run(&mut s, "import_osm", json!({"data": two_houses()}));
    let (id, guid) = {
        let (id, _) = masses(&s).into_iter().find(|(_, m)| osm_id(m) == "way/1").unwrap();
        (id, s.model.guid(id))
    };
    // Someone's own parameter on it, and a mass of their own.
    run(&mut s, "parameter", json!({"name": "Owner", "kind": "text", "categories": ["mass"]}));
    run(&mut s, "modify", json!({"ids": [id], "params": {"Owner": "Ana"}}));
    let own = run(&mut s, "mass", json!({"boundary": [[100000, 100000], [110000, 100000], [110000, 110000]], "height": 3000}))["id"]
        .as_u64()
        .unwrap();
    let taller = file(&[way(1, &square(0.0, 0.0, 20.0), r#""building":"house","height":"18","name":"Casa Grande""#)]);
    let r = run(&mut s, "import_osm", json!({"data": taller}));
    assert_eq!((r["made"].clone(), r["updated"].clone()), (json!(0), json!(1)), "{r}");
    let (_, m) = masses(&s).into_iter().find(|(i, _)| *i == id).unwrap();
    assert_eq!((s.model.guid(id), m.height, m.name.as_str()), (guid, 18000.0, "Casa Grande"));
    assert_eq!(m.params.get("Owner"), Some(&json!("Ana")), "what is theirs stays");
    assert_eq!(masses(&s).len(), 3, "way/2 and the one drawn by hand are still there");
    assert!(masses(&s).iter().any(|(i, _)| *i == own));
    // Not replacing makes another.
    let r = run(&mut s, "import_osm", json!({"data": taller, "replace": false}));
    assert_eq!((r["made"].clone(), r["updated"].clone()), (json!(1), json!(0)));
}

#[test]
fn walls_are_made_on_request_for_the_buildings_asked_for() {
    let mut s = placed();
    let count = |s: &Session, c: &str| s.model.elements.values().filter(|e| e.category() == c).count();
    run(&mut s, "import_osm", json!({"data": two_houses()}));
    assert_eq!((count(&s, "wall"), count(&s, "floor"), count(&s, "roof")), (0, 0, 0), "masses only, by default");
    let r = run(&mut s, "import_osm", json!({"data": two_houses(), "detail": ["way/1"]}));
    assert_eq!(count(&s, "wall"), 4, "{r}");
    // A floor at the ground and over each of the three storeys that 12 m makes, and a roof.
    assert_eq!((count(&s, "floor"), count(&s, "roof")), (4, 1));
    assert_eq!(r["detail"]["detailed"], json!(["way/1"]));
    let r = run(&mut s, "import_osm", json!({"data": two_houses(), "detail": ["way/99"]}));
    assert_eq!(r["detail"]["not_found"], json!(["way/99"]));
}

#[test]
fn courtyards_are_counted_and_buildings_outside_the_box_are_left_out() {
    let mut s = placed();
    let outer = square(0.0, 0.0, 40.0);
    let court = square(0.00015, 0.00013, 8.0);
    let rel = format!(
        r#"{{"type":"relation","id":5,"members":[{{"type":"way","ref":1,"role":"outer","geometry":[{}]}},{{"type":"way","ref":2,"role":"inner","geometry":[{}]}}],"tags":{{"type":"multipolygon","building":"yes","height":"9"}}}}"#,
        geometry(&outer),
        geometry(&court)
    );
    let r = run(
        &mut s,
        "import_osm",
        json!({"data": file(&[rel, way(7, &square(0.5, 0.0, 20.0), r#""building":"yes""#)]), "bbox": [-23.56, -46.64, -23.54, -46.62]}),
    );
    assert_eq!((r["made"].clone(), r["courtyards_filled"].clone()), (json!(1), json!(1)), "{r}");
    assert_eq!(masses(&s).len(), 1);
}

#[test]
fn the_same_buildings_come_from_json_xml_and_a_file() {
    let node = |id: i64, p: (f64, f64)| format!(r#"<node id="{id}" lat="{}" lon="{}"/>"#, p.1, p.0);
    let ring = square(0.0, 0.0, 20.0);
    let xml = format!(
        r#"<osm version="0.6">{}<way id="1">{}<tag k="building" v="yes"/><tag k="height" v="12"/></way></osm>"#,
        ring[..4].iter().enumerate().map(|(i, p)| node(i as i64 + 1, *p)).collect::<String>(),
        (1..=4).chain([1]).map(|i| format!(r#"<nd ref="{i}"/>"#)).collect::<String>()
    );
    let json_text = file(&[way(1, &ring, r#""building":"yes","height":"12""#)]);
    let (mut a, mut b, mut c) = (placed(), placed(), placed());
    run(&mut a, "import_osm", json!({"data": xml}));
    run(&mut b, "import_osm", json!({"data": json_text.clone()}));
    // An Overpass reply as a JSON object, not text.
    run(&mut c, "import_osm", json!({"data": serde_json::from_str::<Value>(&json_text).unwrap()}));
    let (ma, mb, mc) = (masses(&a).remove(0).1, masses(&b).remove(0).1, masses(&c).remove(0).1);
    for m in [&mb, &mc] {
        assert_eq!((m.height, m.footprint().len()), (ma.height, ma.footprint().len()));
        assert!(m.footprint().iter().zip(ma.footprint()).all(|(p, q)| (p.0 - q.0).abs() < 1.0 && (p.1 - q.1).abs() < 1.0));
    }
    // A file beside the project.
    let dir = std::env::temp_dir().join(format!("bimcraft-osm-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("block.json"), &json_text).unwrap();
    let mut d = placed();
    let r = run(&mut d, "import_osm", json!({"path": dir.join("block.json").to_str().unwrap()}));
    assert_eq!(r["made"], 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn what_cannot_be_imported_is_refused_with_what_to_fix() {
    let mut s = Session::new();
    assert!(refused(&mut s, json!({"data": two_houses()})).contains("no location"));
    let mut s = placed();
    for (p, says) in [
        (json!({}), "`path`"),
        (json!({"data": two_houses(), "path": "x.json"}), "not both"),
        (json!({"data": "hello"}), "neither"),
        (json!({"data": 5}), "`data`"),
        (json!({"path": "/nonexistent/none.json"}), "none.json"),
        (json!({"data": two_houses(), "bbox": [1, 2, 3]}), "bbox"),
        (json!({"data": two_houses(), "bbox": [10, 0, -10, 1]}), "bbox"),
        (json!({"data": two_houses(), "level_height": 5}), "level_height"),
        (json!({"data": two_houses(), "default_height": -1}), "default_height"),
        (json!({"data": two_houses(), "terrain": 100000}), "toposurface"),
        (json!({"data": two_houses(), "colour": 1}), "colour"),
        (json!({"data": two_houses(), "detail": "all"}), "detail"),
    ] {
        assert!(refused(&mut s, p.clone()).contains(says), "{p}");
    }
    assert!(masses(&s).is_empty(), "nothing was made");
    // Two terrains: say which.
    for _ in 0..2 {
        run(&mut s, "toposurface", json!({"points": [[0, 0, 0], [10000, 0, 0], [0, 10000, 0]]}));
    }
    assert!(refused(&mut s, json!({"data": two_houses()})).contains("terrain"));
}

#[test]
fn skipped_features_are_reported_by_reason() {
    let mut s = placed();
    let sq = square(0.0, 0.0, 20.0);
    let data = file(&[
        way(1, &sq[..4], r#""building":"yes""#),
        way(2, &sq[..4], r#""building":"yes""#),
        way(3, &sq, r#""building":"yes","height":"4","min_height":"6""#),
        way(4, &sq, r#""building":"yes""#),
    ]);
    let r = run(&mut s, "import_osm", json!({"data": data}));
    assert_eq!((r["made"].clone(), r["skipped"].clone()), (json!(1), json!(3)), "{r}");
    let reasons = r["skipped_by_reason"].as_object().unwrap();
    assert!(reasons.values().any(|n| n == 2), "the two open ways are one reason: {reasons:?}");
    assert_eq!(r["skipped_list"].as_array().unwrap().len(), 3);
    assert_eq!(r["defaulted_height"], 1);
}

#[test]
fn a_district_of_thousands_is_one_command() {
    let mut s = placed();
    let elements: Vec<String> = (0..2000)
        .map(|i| way(i + 1, &square((i % 50) as f64 * 0.0003, (i / 50) as f64 * 0.0003, 20.0), r#""building":"yes","building:levels":"2""#))
        .collect();
    let r = run(&mut s, "import_osm", json!({"data": file(&elements)}));
    assert_eq!(r["made"], 2000);
    assert_eq!(masses(&s).len(), 2000);
    assert_eq!(r["ids"].as_array().unwrap().len(), 1000, "the first thousand are listed");
    assert_eq!(r["ids_truncated"], true);
    // One undo takes the lot back.
    s.execute("undo", &json!({})).unwrap();
    assert!(masses(&s).is_empty());
}

#[test]
fn the_masses_export_to_ifc_and_are_drawn_in_plans() {
    use crate::render::{self, Prim};
    let mut s = placed();
    run(&mut s, "import_osm", json!({"data": two_houses()}));
    let text = crate::ifc::export(&s.model);
    assert_eq!(text.matches("=IFCBUILDINGELEMENTPROXY(").count(), 2);
    assert!(text.contains("IFCMAPCONVERSION"), "and the place");
    let ids: Vec<u64> = masses(&s).iter().map(|(i, _)| *i).collect();
    let d = render::plan(&s.model, 100000, 800, 600);
    let drawn =
        d.prims.iter().zip(&d.owners).filter(|(p, o)| matches!(p, Prim::Line { .. }) && o.is_some_and(|o| ids.contains(&o))).count();
    assert!(drawn >= 2, "both are in the plan: {drawn}");
}

/// The elements of `category` with their ids.
fn of(s: &Session, category: &str) -> Vec<(u64, Element)> {
    s.model.elements.iter().filter(|(_, e)| e.category() == category).map(|(id, e)| (*id, e.clone())).collect()
}

#[test]
fn building_parts_replace_their_outline_and_say_whose_they_are() {
    let mut s = placed();
    let (dx, dy) = (40.0 * 0.000_01, 20.0 * 0.000_009);
    let rect = |x0: f64, x1: f64| {
        vec![(HOME.0 + x0, HOME.1), (HOME.0 + x1, HOME.1), (HOME.0 + x1, HOME.1 + dy), (HOME.0 + x0, HOME.1 + dy), (HOME.0 + x0, HOME.1)]
    };
    let data = file(&[
        way(1, &rect(0.0, dx), r#""building":"yes","name":"Hall""#),
        way(2, &rect(0.0, dx / 2.0), r#""building:part":"yes","height":"8""#),
        way(3, &rect(dx / 2.0, dx), r#""building:part":"yes","height":"14","roof:shape":"gabled","roof:height":"3""#),
    ]);
    // Without parts wanted the outline is the building, as before.
    let r = run(&mut s, "import_osm", json!({"data": data}));
    assert_eq!((r["made"].as_u64(), r["outlines_replaced"].as_u64()), (Some(1), Some(0)), "{r}");
    let mut s = placed();
    let r = run(&mut s, "import_osm", json!({"data": data, "parts": true}));
    assert_eq!((r["made"].as_u64(), r["outlines_replaced"].as_u64()), (Some(2), Some(1)), "{r}");
    assert_eq!(r["skipped_by_reason"]["replaced by its building parts"], 1, "{r}");
    let ms = masses(&s);
    let ids: Vec<String> = ms.iter().map(|(_, m)| osm_id(m)).collect();
    assert!(ids.contains(&"way/2".to_string()) && ids.contains(&"way/3".to_string()) && !ids.contains(&"way/1".to_string()), "{ids:?}");
    for (_, m) in &ms {
        assert_eq!(m.params.get("OSM Part Of").and_then(Value::as_str), Some("way/1"));
        assert_eq!(m.name, "Hall", "a part with no name has its outline's");
    }
    // The tall part's facade is its height less its roof: 14 m less 3 m.
    let tall = ms.iter().find(|(_, m)| osm_id(m) == "way/3").unwrap();
    assert_eq!(tall.1.height, 11000.0);
    assert_eq!(tall.1.params.get("OSM Roof Shape").and_then(Value::as_str), Some("gabled"));
    assert_eq!(tall.1.params.get("OSM Roof Height").and_then(Value::as_str), Some("3000"));
}

#[test]
fn the_shell_of_a_building_follows_its_tags_storeys_roof_shape_and_height() {
    let mut s = placed();
    // 12 m tall with a 3 m gabled roof: a 9 m facade of three storeys; the ridge runs along the 30 m side.
    let (w, h) = (30.0 * 0.000_01, 12.0 * 0.000_009);
    let ring = vec![(HOME.0, HOME.1), (HOME.0 + w, HOME.1), (HOME.0 + w, HOME.1 + h), (HOME.0, HOME.1 + h), (HOME.0, HOME.1)];
    let data = file(&[way(1, &ring, r#""building":"house","height":"12","building:levels":"3","roof:shape":"gabled","roof:height":"3""#)]);
    let r = run(&mut s, "import_osm", json!({"data": data, "detail": ["way/1"]}));
    assert_eq!(r["detail"]["detailed"], json!(["way/1"]), "{r}");
    assert_eq!((r["detail"]["walls"].as_u64(), r["detail"]["floors"].as_u64(), r["detail"]["roofs"].as_u64()), (Some(4), Some(3), Some(1)));
    let mass = &masses(&s)[0].1;
    assert!((mass.height - 9000.0).abs() < 1e-6, "{}", mass.height);
    let offsets: Vec<f64> = {
        let mut v: Vec<f64> = of(&s, "floor")
            .iter()
            .filter_map(|(_, e)| if let Element::Floor(f) = e { Some(f.offset - mass.base_offset) } else { None })
            .collect();
        v.sort_by(f64::total_cmp);
        v
    };
    assert_eq!(offsets, [0.0, 3000.0, 6000.0], "a floor at the ground and over each storey but the last, whose top is the roof's");
    let roofs = of(&s, "roof");
    let Element::Roof(roof) = &roofs[0].1 else { panic!("a roof") };
    assert_eq!(roof.gables.len(), 2, "the short ends stand upright");
    let top = s
        .model
        .roof_faces(roof)
        .unwrap()
        .iter()
        .flat_map(|(loops, plane)| loops.iter().flatten().map(move |q| plane.at(*q)))
        .fold(f64::MIN, f64::max);
    let base = s.model.elevation(roof.level) + roof.offset;
    assert!((top - base - 3000.0).abs() < 1.0, "the roof is the 3 m the tag says: {}", top - base);
    // Importing again with the same detail leaves the shell alone: it is reported skipped, not doubled.
    let before = s.model.elements.len();
    let r = run(&mut s, "import_osm", json!({"data": data, "detail": ["way/1"]}));
    assert_eq!(s.model.elements.len(), before, "{r}");
    assert_eq!(r["detail"]["skipped"], json!(["way/1"]));
}
