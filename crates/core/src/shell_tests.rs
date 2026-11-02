//! `building_shell`: the outer walls, the floors at each storey and the roof of a mass, as an agent or the Site tab asks for them.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Id};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn refused(s: &mut Session, p: Value) -> String {
    s.execute("building_shell", &p).expect_err(&format!("building_shell {p} should be refused"))
}

/// A mass 10 m by 6 m, 9 m tall, on Level 1.
fn house(s: &mut Session) -> Id {
    mass(s, &[[0, 0], [10000, 0], [10000, 6000], [0, 6000]], 9000)
}

fn mass(s: &mut Session, ring: &[[i32; 2]], height: i32) -> Id {
    run(s, "mass", json!({"boundary": ring, "height": height}))["id"].as_u64().unwrap()
}

fn of(s: &Session, category: &str) -> Vec<(Id, Element)> {
    s.model.elements.iter().filter(|(_, e)| e.category() == category).map(|(id, e)| (*id, e.clone())).collect()
}

fn floor_offsets(s: &Session) -> Vec<f64> {
    let mut v: Vec<f64> =
        of(s, "floor").iter().filter_map(|(_, e)| if let Element::Floor(f) = e { Some(f.offset) } else { None }).collect();
    v.sort_by(f64::total_cmp);
    v
}

/// How high a roof's highest point is over its base, mm.
fn peak(s: &Session, id: Id) -> f64 {
    let Some(Element::Roof(r)) = s.model.elements.get(id) else { panic!("not a roof") };
    let base = s.model.elevation(r.level) + r.offset;
    s.model
        .roof_faces(r)
        .unwrap()
        .iter()
        .flat_map(|(loops, plane)| loops.iter().flatten().map(move |p| plane.at(*p)))
        .fold(f64::MIN, f64::max)
        - base
}

fn roof_of(s: &Session) -> (Id, crate::model::Roof) {
    let mut roofs = of(s, "roof");
    assert_eq!(roofs.len(), 1, "one roof");
    let (id, Element::Roof(r)) = roofs.remove(0) else { panic!("a roof") };
    (id, r)
}

#[test]
fn a_shell_is_a_wall_on_each_face_a_floor_at_each_storey_and_a_roof() {
    let mut s = Session::new();
    let m = house(&mut s);
    let r = run(&mut s, "building_shell", json!({"ids": [m]}));
    assert_eq!(of(&s, "wall").len(), 4);
    // 9 m at 3 m a storey: the ground slab and the floors over the first and second storeys; the roof is the third's.
    assert_eq!(floor_offsets(&s), [0.0, 3000.0, 6000.0]);
    let (roof, rf) = roof_of(&s);
    assert_eq!((rf.slope, rf.offset, peak(&s, roof)), (0.0, 9000.0, 0.0), "flat, at the top of the walls");
    assert_eq!((r["walls"].as_u64(), r["floors"].as_u64(), r["roofs"].as_u64()), (Some(4), Some(3), Some(1)), "{r}");
    // One undo step takes the lot back.
    run(&mut s, "undo", json!({}));
    assert!(of(&s, "wall").is_empty() && of(&s, "floor").is_empty() && of(&s, "roof").is_empty());
    assert_eq!(of(&s, "mass").len(), 1);
}

#[test]
fn what_is_asked_for_is_what_is_made_and_the_storeys_are_counted_or_spaced() {
    let mut s = Session::new();
    let m = house(&mut s);
    run(&mut s, "building_shell", json!({"ids": [m], "walls": false, "roof": false, "storeys": 4}));
    assert_eq!((of(&s, "wall").len(), of(&s, "roof").len()), (0, 0));
    // Four storeys in 9 m: a rise of 2.25 m.
    assert_eq!(floor_offsets(&s), [0.0, 2250.0, 4500.0, 6750.0]);
    let mut s = Session::new();
    let m = house(&mut s);
    run(&mut s, "building_shell", json!({"ids": [m], "walls": false, "roof": false, "storey_height": 2500}));
    // By height: 9 m in storeys of 2.5 m is 4 (rounded), one every 2.5 m.
    assert_eq!(floor_offsets(&s), [0.0, 2500.0, 5000.0, 7500.0]);
    let mut s = Session::new();
    let m = house(&mut s);
    run(&mut s, "building_shell", json!({"ids": [m], "walls": false, "roof": false, "floors": false}));
    assert!(of(&s, "floor").is_empty() && of(&s, "wall").is_empty() && of(&s, "roof").is_empty());
}

#[test]
fn a_hipped_roof_peaks_as_high_as_the_height_it_is_given() {
    let mut s = Session::new();
    let m = house(&mut s);
    run(&mut s, "building_shell", json!({"ids": [m], "shape": "hipped", "height": 3000}));
    let (roof, rf) = roof_of(&s);
    assert!(rf.slope > 0.0 && rf.gables.is_empty(), "{rf:?}");
    assert!((peak(&s, roof) - 3000.0).abs() < 0.5, "{}", peak(&s, roof));
    assert_eq!(rf.offset, 9000.0, "the roof stands on the walls");
}

#[test]
fn an_angle_sets_the_slope_and_a_pyramid_is_a_hip_on_a_square() {
    let mut s = Session::new();
    let m = mass(&mut s, &[[0, 0], [6000, 0], [6000, 6000], [0, 6000]], 6000);
    run(&mut s, "building_shell", json!({"ids": [m], "shape": "pyramidal", "angle": 45}));
    let (roof, rf) = roof_of(&s);
    assert_eq!(rf.slope, 45.0);
    // Half of 6 m across at 45 degrees: 3 m.
    assert!((peak(&s, roof) - 3000.0).abs() < 0.5, "{}", peak(&s, roof));
    // With no angle and no height, 30 degrees.
    let mut s = Session::new();
    let m = house(&mut s);
    run(&mut s, "building_shell", json!({"ids": [m], "shape": "hipped"}));
    assert_eq!(roof_of(&s).1.slope, 30.0);
    // A height that would need a slope over the limit is cut to it, and said.
    let mut s = Session::new();
    let m = house(&mut s);
    let r = run(&mut s, "building_shell", json!({"ids": [m], "shape": "hipped", "height": 100000}));
    assert!(roof_of(&s).1.slope <= 75.0 && r["notes"].to_string().contains("75"), "{r}");
}

#[test]
fn a_gabled_roof_stands_its_short_ends_upright_and_across_swaps_them() {
    let mut s = Session::new();
    let m = house(&mut s);
    run(&mut s, "building_shell", json!({"ids": [m], "shape": "gabled", "height": 2000}));
    let (roof, rf) = roof_of(&s);
    assert_eq!(rf.gables, [1, 3], "the 6 m ends of a 10 m by 6 m house");
    // The ridge runs the length of the house: the roof is 3 m up to its ridge at the slope that makes 2 m.
    assert!((peak(&s, roof) - 2000.0).abs() < 0.5, "{}", peak(&s, roof));
    let mut s = Session::new();
    let m = house(&mut s);
    run(&mut s, "building_shell", json!({"ids": [m], "shape": "gabled", "orientation": "across", "height": 2000}));
    assert_eq!(roof_of(&s).1.gables, [0, 2]);
}

#[test]
fn a_shape_that_is_not_made_is_a_flat_roof_and_a_note() {
    let mut s = Session::new();
    let m = house(&mut s);
    let r = run(&mut s, "building_shell", json!({"ids": [m], "shape": "skillion"}));
    assert_eq!(roof_of(&s).1.slope, 0.0);
    assert!(r["notes"].to_string().contains("skillion"), "{r}");
    let mut s = Session::new();
    let l = mass(&mut s, &[[0, 0], [10000, 0], [10000, 3000], [3000, 3000], [3000, 10000], [0, 10000]], 6000);
    let r = run(&mut s, "building_shell", json!({"ids": [l], "shape": "gabled"}));
    assert!(roof_of(&s).1.slope > 0.0 && roof_of(&s).1.gables.is_empty(), "hipped instead");
    assert!(r["notes"].to_string().contains("not near a rectangle"), "{r}");
    // Too many corners for a pitched roof: flat, and said.
    let mut s = Session::new();
    let ring: Vec<[i32; 2]> = (0..300)
        .map(|i| {
            let a = f64::from(i) / 300.0 * std::f64::consts::TAU;
            [(20000.0 * a.cos()) as i32, (20000.0 * a.sin()) as i32]
        })
        .collect();
    let big = mass(&mut s, &ring, 6000);
    let r = run(&mut s, "building_shell", json!({"ids": [big], "shape": "hipped", "walls": false, "floors": false}));
    assert_eq!(roof_of(&s).1.slope, 0.0);
    assert!(r["notes"].to_string().contains("256"), "{r}");
}

#[test]
fn a_mass_that_has_a_shell_already_is_not_given_another() {
    let mut s = Session::new();
    let m = house(&mut s);
    run(&mut s, "building_shell", json!({"ids": [m]}));
    let before = s.model.elements.len();
    let r = run(&mut s, "building_shell", json!({"ids": [m]}));
    assert_eq!(s.model.elements.len(), before, "{r}");
    assert!(r["skipped"].to_string().contains("already"), "{r}");
}

#[test]
fn the_shell_follows_its_mass_when_update_by_face_is_run() {
    let mut s = Session::new();
    let m = house(&mut s);
    run(&mut s, "building_shell", json!({"ids": [m]}));
    run(&mut s, "modify", json!({"id": m, "base_offset": 500, "boundary": [[0, 0], [12000, 0], [12000, 6000], [0, 6000]]}));
    run(&mut s, "update_by_face", json!({"mass": m}));
    assert_eq!(floor_offsets(&s), [500.0, 3500.0, 6500.0], "the storeys keep their rise over the new base");
    for (_, e) in of(&s, "floor") {
        let Element::Floor(f) = e else { panic!("a floor") };
        assert!(f.boundary.contains(&P2(12000.0, 0.0)), "the floor has the new footprint: {:?}", f.boundary);
    }
    let (_, rf) = roof_of(&s);
    assert!(rf.boundary.contains(&P2(12000.0, 0.0)));
    assert!(of(&s, "wall").iter().all(|(_, e)| matches!(e, Element::Wall(w) if w.base_offset == 500.0)));
}

#[test]
fn a_shell_is_copied_and_deleted_like_any_element_and_its_marks_are_checked() {
    let mut s = Session::new();
    let m = house(&mut s);
    run(&mut s, "building_shell", json!({"ids": [m]}));
    // Deleting the mass leaves ordinary walls, floors and a roof.
    run(&mut s, "delete", json!({"ids": [m]}));
    assert_eq!((of(&s, "wall").len(), of(&s, "floor").len(), of(&s, "roof").len()), (4, 3, 1));
    // A floor of a storey whose mark is wrong is refused where it is set.
    let mut s = Session::new();
    let m = house(&mut s);
    run(&mut s, "building_shell", json!({"ids": [m]}));
    let floor = of(&s, "floor")[0].0;
    for bad in [
        json!({"storey": -1, "rise": 3000}),
        json!({"storey": 1, "rise": 0}),
        json!({"storey": 1, "rise": "tall"}),
        json!({"storey": 9999, "rise": 3000}),
        json!(5),
        json!("side"),
    ] {
        let mark = json!({"mass": m, "face": bad});
        assert!(s.execute("modify", &json!({"id": floor, "params": {"Mass Face": mark}})).is_err(), "{bad}");
    }
}

#[test]
fn what_cannot_be_shelled_is_refused_and_the_project_stays_as_it_was() {
    let mut s = Session::new();
    let m = house(&mut s);
    let wall = run(&mut s, "wall", json!({"points": [[0, 0], [1000, 0]]}))["ids"][0].as_u64().unwrap();
    let before = s.model.elements.len();
    assert!(refused(&mut s, json!({})).contains("ids"));
    assert!(refused(&mut s, json!({"ids": []})).contains("ids"));
    assert!(refused(&mut s, json!({"ids": [wall]})).contains("isn't a mass"));
    assert!(refused(&mut s, json!({"ids": [999_999_999u64]})).contains("no element"));
    assert!(refused(&mut s, json!({"ids": [m], "storeys": 0})).contains("storeys"));
    assert!(refused(&mut s, json!({"ids": [m], "storeys": 100_000})).contains("storeys"));
    assert!(refused(&mut s, json!({"ids": [m], "storey_height": 0})).contains("storey_height"));
    assert!(refused(&mut s, json!({"ids": [m], "storey_height": -3000})).contains("storey_height"));
    assert!(refused(&mut s, json!({"ids": [m], "angle": 95})).contains("angle"));
    assert!(refused(&mut s, json!({"ids": [m], "angle": 0})).contains("angle"));
    assert!(refused(&mut s, json!({"ids": [m], "height": -1})).contains("height"));
    assert!(refused(&mut s, json!({"ids": [m], "orientation": "sideways"})).contains("orientation"));
    assert!(refused(&mut s, json!({"ids": [m], "walls": "yes"})).contains("walls"));
    assert!(refused(&mut s, json!({"ids": [m], "colour": "red"})).contains("colour"), "an unknown key is an error");
    assert_eq!(s.model.elements.len(), before);
    // More masses than one command shells.
    let ids: Vec<Id> = (0..=crate::commands::MAX_SHELL as i32)
        .map(|i| mass(&mut s, &[[20000 + i * 100, 0], [20050 + i * 100, 0], [20050 + i * 100, 50]], 3000))
        .collect();
    assert!(refused(&mut s, json!({"ids": ids})).contains(&crate::commands::MAX_SHELL.to_string()));
}

#[test]
fn many_buildings_are_one_command_one_check_and_one_undo_step() {
    let mut s = Session::new();
    let ids: Vec<Id> = (0..100)
        .map(|i| mass(&mut s, &[[i * 12000, 0], [i * 12000 + 10000, 0], [i * 12000 + 10000, 6000], [i * 12000, 6000]], 6000))
        .collect();
    let start = std::time::Instant::now();
    let r = run(&mut s, "building_shell", json!({"ids": ids, "shape": "gabled", "height": 2000}));
    assert_eq!(r["walls"], 400);
    assert_eq!(of(&s, "roof").len(), 100);
    assert!(start.elapsed().as_secs_f64() < 20.0, "{:?}", start.elapsed());
    run(&mut s, "undo", json!({}));
    assert!(of(&s, "wall").is_empty());
}
