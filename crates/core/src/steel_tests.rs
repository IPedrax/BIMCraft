//! The steel list and the cut plan: a bar's shape and mark, `steel_list`, `cut_plan` and the steel schedules, through the commands
//! every front end drives. Marks are worked out when asked and stored nowhere, so a changed bar is a changed mark.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::element_json;
use crate::engine::Session;
use crate::model::Element;
use crate::steel::{self, Piece, Stock};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn ids(r: &Value) -> Vec<u64> {
    r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("bimcraft-steel-{}-{name}", std::process::id()))
}

/// A beam 200 x 500 and 6 m long along y = `y`, with its top at the level.
fn beam(s: &mut Session, y: i32) -> u64 {
    ids(&run(s, "beam", json!({"points": [[0, y], [6000, y]]})))[0]
}

/// Arms `host` with 3 bottom bars of 16 mm, 2 top bars of 12.5 mm and stirrups of 5 mm every 150 (of CA-60).
fn arm(s: &mut Session, host: u64) -> Vec<u64> {
    let spec = json!({
        "host": host, "cover": 25, "bottom": {"count": 3, "diameter": 16}, "top": {"count": 2, "diameter": 12.5},
        "stirrup": {"diameter": 5, "spacing": 150}, "grade_stirrup": "CA-60"
    });
    ids(&run(s, "reinforce", spec))
}

/// A bar of 20 mm drawn by hand on `host`, `length` mm long, `y` mm across the beam.
fn hand(s: &mut Session, host: u64, length: f64, y: f64) -> u64 {
    let points = json!([[50, y, -400], [50.0 + length, y, -400]]);
    ids(&run(s, "rebar", json!({"points": points, "diameter": 20, "host": host})))[0]
}

fn bar(s: &Session, id: u64) -> Value {
    element_json(&s.model, id)
}

fn all_bars(s: &Session) -> Vec<u64> {
    s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Rebar(_))).map(|(id, _)| *id).collect()
}

const HOOP: [[f64; 3]; 5] = [[0.0, 0.0, 0.0], [140.0, 0.0, 0.0], [140.0, 440.0, 0.0], [0.0, 440.0, 0.0], [0.0, 0.0, 0.0]];

#[test]
fn a_reversed_bar_has_the_same_shape() {
    let bar = [[0.0, 0.0, 0.0], [500.0, 0.0, 0.0], [500.0, 300.0, 0.0]];
    let back = [bar[2], bar[1], bar[0]];
    let shape = steel::shape(&bar);
    assert_eq!(shape.key(), steel::shape(&back).key());
    assert_eq!((shape.closed, shape.label()), (false, "500 x 300".to_string()));
    assert!(shape.turns.len() == 1 && near(shape.turns[0], 90.0), "{:?}", shape.turns);
    let straight = steel::shape(&[[0.0, 0.0, 0.0], [5900.0, 0.0, 0.0]]);
    assert_eq!((straight.label(), straight.turns.len(), straight.twists.len()), ("5900".to_string(), 0, 0));
}

#[test]
fn a_hoop_started_at_another_corner_has_the_same_shape() {
    let other = [[140.0, 440.0, 0.0], [0.0, 440.0, 0.0], [0.0, 0.0, 0.0], [140.0, 0.0, 0.0], [140.0, 440.0, 0.0]];
    let mut turned = HOOP.to_vec();
    turned.reverse();
    let hoop = steel::shape(&HOOP);
    assert!(hoop.closed);
    assert_eq!(hoop.label(), "closed 140 x 440 x 140 x 440");
    assert_eq!(hoop.key(), steel::shape(&other).key());
    assert_eq!(hoop.key(), steel::shape(&turned).key());
    // Another hoop is another bar.
    let wider = [[0.0, 0.0, 0.0], [150.0, 0.0, 0.0], [150.0, 440.0, 0.0], [0.0, 440.0, 0.0], [0.0, 0.0, 0.0]];
    assert_ne!(hoop.key(), steel::shape(&wider).key());
}

#[test]
fn shapes_are_the_same_within_a_millimetre_and_a_degree_and_not_beyond() {
    let line = |x: f64| steel::shape(&[[0.0, 0.0, 0.0], [x, 0.0, 0.0]]).key();
    assert_eq!(line(5000.0), line(5000.4));
    assert_ne!(line(5000.0), line(5001.0));
    let bent = |degrees: f64| {
        let (s, c) = degrees.to_radians().sin_cos();
        steel::shape(&[[0.0, 0.0, 0.0], [500.0, 0.0, 0.0], [500.0 + 300.0 * c, 300.0 * s, 0.0]]).key()
    };
    assert_eq!(bent(90.0), bent(90.3));
    assert_ne!(bent(90.0), bent(92.0));
}

#[test]
fn a_u_and_a_z_of_the_same_legs_are_not_the_same_bar_and_a_bend_out_of_plane_has_a_twist() {
    let u = steel::shape(&[[0.0, 0.0, 0.0], [0.0, 100.0, 0.0], [300.0, 100.0, 0.0], [300.0, 0.0, 0.0]]);
    let z = steel::shape(&[[0.0, 0.0, 0.0], [0.0, 100.0, 0.0], [300.0, 100.0, 0.0], [300.0, 200.0, 0.0]]);
    assert_eq!((u.legs.clone(), u.turns.len()), (z.legs.clone(), z.turns.len()));
    assert!(near(u.twists[0], 0.0) && near(z.twists[0], 180.0), "{:?} {:?}", u.twists, z.twists);
    assert_ne!(u.key(), z.key());
    // Three legs along x, y and z: the second bend is out of the first's plane.
    let out = steel::shape(&[[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [100.0, 100.0, 0.0], [100.0, 100.0, 100.0]]);
    assert!(near(out.twists[0], 90.0), "{:?}", out.twists);
}

#[test]
fn a_shape_of_nothing_or_of_nonsense_is_still_a_shape() {
    let paths: [Vec<[f64; 3]>; 5] = [
        vec![],
        vec![[0.0; 3]],
        vec![[0.0; 3]; 5],
        vec![[f64::NAN; 3], [1.0, 2.0, 3.0]],
        vec![[f64::INFINITY, 0.0, 0.0], [0.0, 0.0, 0.0], [1e308, 1e308, 1e308], [0.0, 0.0, 0.0]],
    ];
    for path in &paths {
        let shape = steel::shape(path);
        let _ = (shape.key(), shape.label());
    }
}

#[test]
fn identical_bars_share_a_mark_and_the_marks_are_numbered_from_the_bottom_bars() {
    let mut s = Session::new();
    let host = beam(&mut s, 0);
    let made = arm(&mut s, host);
    assert_eq!(made.len(), 46);
    for id in made {
        let b = bar(&s, id);
        let mark = match b["layer"].as_str() {
            Some("bottom") => "N1",
            Some("top") => "N2",
            Some("stirrup") => "N3",
            other => panic!("{other:?}"),
        };
        assert_eq!(b["mark"], json!(mark), "{b}");
        assert_eq!(b["host_mark"], json!(""));
    }
    let bottom = bar(&s, all_bars(&s).into_iter().find(|id| bar(&s, *id)["layer"] == json!("bottom")).unwrap());
    assert_eq!((bottom["shape_label"].as_str(), bottom["shape"]["closed"].as_bool()), (Some("5950"), Some(false)));
    let stirrup = bar(&s, all_bars(&s).into_iter().find(|id| bar(&s, *id)["layer"] == json!("stirrup")).unwrap());
    assert!(stirrup["shape_label"].as_str().is_some_and(|l| l.starts_with("closed ")), "{stirrup}");
}

#[test]
fn bars_that_differ_by_a_millimetre_in_length_get_two_marks() {
    let mut s = Session::new();
    let host = beam(&mut s, 0);
    let (a, b, c) = (hand(&mut s, host, 5000.0, -40.0), hand(&mut s, host, 5001.0, 0.0), hand(&mut s, host, 5000.0, 40.0));
    let mark = |id: u64| bar(&s, id)["mark"].clone();
    assert_eq!(mark(a), mark(c));
    assert_ne!(mark(a), mark(b));
}

#[test]
fn marks_are_numbered_bottom_bars_first_and_hand_drawn_bars_last() {
    let mut s = Session::new();
    let host = beam(&mut s, 0);
    // The thickest and the longest bar, drawn first, still comes last.
    let drawn = hand(&mut s, host, 5900.0, 0.0);
    arm(&mut s, host);
    assert_eq!(bar(&s, drawn)["mark"], json!("N4"));
    let layers: Vec<(Value, Value)> = all_bars(&s)
        .into_iter()
        .map(|id| bar(&s, id))
        .filter(|b| b["layer"] != json!(""))
        .map(|b| (b["layer"].clone(), b["mark"].clone()))
        .collect();
    assert!(layers.contains(&(json!("bottom"), json!("N1"))) && layers.contains(&(json!("top"), json!("N2"))), "{layers:?}");
}

#[test]
fn reinforcing_again_gives_the_same_marks() {
    let mut s = Session::new();
    let host = beam(&mut s, 0);
    let marks = |s: &Session| {
        let mut m: Vec<(String, String)> =
            all_bars(s).into_iter().map(|id| bar(s, id)).map(|b| (b["layer"].to_string(), b["mark"].to_string())).collect();
        m.sort();
        m
    };
    arm(&mut s, host);
    let first = marks(&s);
    arm(&mut s, host);
    assert_eq!(first, marks(&s));
}

#[test]
fn two_hosts_may_both_have_an_n1_and_the_list_names_the_host() {
    let mut s = Session::new();
    let (one, two) = (beam(&mut s, 0), beam(&mut s, 3000));
    arm(&mut s, one);
    arm(&mut s, two);
    let r = run(&mut s, "steel_list", json!({}));
    let rows = r["rows"].as_array().unwrap();
    let n1: Vec<&Value> = rows.iter().filter(|row| row["mark"] == json!("N1")).collect();
    assert_eq!(n1.len(), 2);
    let hosts: Vec<u64> = n1.iter().map(|row| row["host"].as_u64().unwrap()).collect();
    assert_eq!(hosts, [one, two]);
}

#[test]
fn a_bar_tag_shows_its_mark_by_default() {
    let mut s = Session::new();
    let host = beam(&mut s, 0);
    arm(&mut s, host);
    let bottom = all_bars(&s).into_iter().find(|id| bar(&s, *id)["layer"] == json!("bottom")).unwrap();
    let field = crate::model::Tag::usual("rebar");
    assert_eq!(crate::schedule::field_text(&s.model, bottom, field).as_deref(), Some("N1"));
}

#[test]
fn the_steel_schedules_are_grouped_by_mark_and_by_level_grade_and_diameter() {
    let mut s = Session::new();
    let host = beam(&mut s, 0);
    arm(&mut s, host);
    let list = run(&mut s, "schedule", json!({"template": "steel_list"}));
    assert_eq!((list["name"].as_str(), list["category"].as_str()), (Some("Steel List"), Some("rebar")));
    let rows = list["rows"].as_array().unwrap();
    // Host Mark, Mark, Diameter, Grade, Shape, Count, Length, Total Length, Weight.
    let marks: Vec<(&str, f64)> = rows.iter().map(|r| (r[1].as_str().unwrap(), r[5].as_f64().unwrap())).collect();
    assert_eq!(marks, [("N1", 3.0), ("N2", 2.0), ("N3", 41.0)]);
    let weights: f64 = all_bars(&s).into_iter().map(|id| bar(&s, id)["weight_kg"].as_f64().unwrap()).sum();
    assert!((list["totals"][8].as_f64().unwrap() - weights).abs() < 0.05, "{} {weights}", list["totals"]);
    // Made again, it is the same schedule.
    let again = run(&mut s, "schedule", json!({"template": "steel_list"}));
    assert_eq!(again["id"], list["id"]);

    let summary = run(&mut s, "schedule", json!({"template": "steel_summary"}));
    assert_eq!(summary["name"], json!("Steel Summary"));
    // Level, Grade, Diameter, Count, Total Length, Weight: CA-50 12.5 and 16, then CA-60 5.
    let lines: Vec<(&str, f64, f64)> = summary["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r[1].as_str().unwrap(), r[2].as_f64().unwrap(), r[3].as_f64().unwrap()))
        .collect();
    assert_eq!(lines, [("CA-50", 12.5, 2.0), ("CA-50", 16.0, 3.0), ("CA-60", 5.0, 41.0)]);
}

#[test]
fn a_steel_schedule_of_one_host_lists_only_that_hosts_bars_and_show_changes_only_the_columns() {
    let mut s = Session::new();
    let (one, two) = (beam(&mut s, 0), beam(&mut s, 3000));
    arm(&mut s, one);
    let own = hand(&mut s, two, 3000.0, 3000.0);
    let r = run(&mut s, "schedule", json!({"template": "steel_list", "host": two}));
    assert_eq!(r["name"], json!(format!("Steel List {two}")));
    assert_eq!(r["elements"], json!([[own]]));
    let mut columns = |show: &str| {
        let r = run(&mut s, "schedule", json!({"template": "steel_list", "show": show, "name": format!("By {show}")}));
        r["columns"].as_array().unwrap().len()
    };
    assert_eq!((columns("both"), columns("count"), columns("weight")), (9, 8, 8));
    let e = s.execute("schedule", &json!({"template": "steel_list", "category": "wall"})).unwrap_err();
    assert!(e.contains("rebar"), "{e}");
    let e = s.execute("schedule", &json!({"template": "zap"})).unwrap_err();
    assert!(e.contains("template"), "{e}");
}

#[test]
fn the_steel_list_totals_equal_the_rebar_weights() {
    let mut s = Session::new();
    let host = beam(&mut s, 0);
    arm(&mut s, host);
    // A bar with no host is listed once, in its level.
    let loose = ids(&run(&mut s, "rebar", json!({"points": [[0, 0, 0], [2000, 0, 0]], "diameter": 10})))[0];
    let r = run(&mut s, "steel_list", json!({}));
    let weights: f64 = all_bars(&s).into_iter().map(|id| bar(&s, id)["weight_kg"].as_f64().unwrap()).sum();
    assert!((r["totals"]["weight_kg"].as_f64().unwrap() - weights).abs() < 0.05, "{} {weights}", r["totals"]);
    let rows = r["rows"].as_array().unwrap();
    let counted: u64 = rows.iter().map(|row| row["count"].as_u64().unwrap()).sum();
    assert_eq!(counted as usize, all_bars(&s).len());
    let row = rows.iter().find(|row| row["bars"] == json!([loose])).unwrap();
    assert!(row["host"].is_null() && row["level"].as_str().is_some_and(|l| !l.is_empty()), "{row}");
    let by_diameter: f64 = r["summary"].as_array().unwrap().iter().map(|line| line["weight_kg"].as_f64().unwrap()).sum();
    assert!((by_diameter - weights).abs() < 0.05);
}

#[test]
fn a_steel_list_for_one_host_lists_only_that_hosts_bars() {
    let mut s = Session::new();
    let (one, two) = (beam(&mut s, 0), beam(&mut s, 3000));
    arm(&mut s, one);
    let made = arm(&mut s, two);
    let r = run(&mut s, "steel_list", json!({"ids": [two]}));
    let mut listed: Vec<u64> =
        r["rows"].as_array().unwrap().iter().flat_map(|row| row["bars"].as_array().unwrap().iter().map(|b| b.as_u64().unwrap())).collect();
    listed.sort_unstable();
    let mut expected = made.clone();
    expected.sort_unstable();
    assert_eq!(listed, expected);
    // A bar stands for itself.
    let r = run(&mut s, "steel_list", json!({"ids": [made[0]]}));
    assert_eq!(r["totals"]["bars"], json!(1));
    // Nothing of a level with no bars.
    run(&mut s, "level", json!({"name": "Roof", "elevation": 9000}));
    let r = run(&mut s, "steel_list", json!({"level": "Roof"}));
    assert_eq!((r["rows"].as_array().map(Vec::len), r["totals"]["bars"].as_u64()), (Some(0), Some(0)));
}

#[test]
fn a_coil_diameter_is_listed_by_weight_only() {
    let mut s = Session::new();
    let host = beam(&mut s, 0);
    arm(&mut s, host);
    let r = run(&mut s, "steel_list", json!({"coil_to": 10, "stock_length": 12000}));
    let line = |d: f64| r["summary"].as_array().unwrap().iter().find(|l| near(l["diameter"].as_f64().unwrap(), d)).unwrap().clone();
    let (stirrups, bottom) = (line(5.0), line(16.0));
    assert_eq!((stirrups["form"].as_str(), stirrups["count"].as_u64()), (Some("coil"), Some(41)));
    assert!(stirrups.get("stock_bars").is_none() && stirrups["weight_kg"].as_f64().unwrap() > 0.0, "{stirrups}");
    assert_eq!((bottom["form"].as_str(), bottom["stock_bars"].as_u64()), (Some("bar"), Some(2)));
}

#[test]
fn csv_and_xlsx_hold_the_rows_of_the_reply() {
    let mut s = Session::new();
    let host = beam(&mut s, 0);
    arm(&mut s, host);
    let (csv, xlsx) = (temp("list.csv"), temp("list.xlsx"));
    let r = run(&mut s, "steel_list", json!({"path": csv}));
    let text = std::fs::read_to_string(&csv).unwrap();
    // A heading, a row for each mark and a totals row.
    assert!(text.lines().count() > r["rows"].as_array().unwrap().len(), "{text}");
    assert!(text.contains("N1") && text.contains("N3") && text.contains("5950"), "{text}");
    run(&mut s, "steel_list", json!({"path": xlsx}));
    assert!(std::fs::read(&xlsx).unwrap().starts_with(b"PK"));
    let plan = temp("plan.csv");
    run(&mut s, "cut_plan", json!({"stock_length": 12000, "path": plan}));
    let text = std::fs::read_to_string(&plan).unwrap();
    assert!(text.contains("N1") && text.contains("5950"), "{text}");
    for path in [csv, xlsx, plan] {
        let _ = std::fs::remove_file(path);
    }
}

fn piece(bar: u64, length: f64) -> Piece {
    Piece { bar, host_mark: String::new(), mark: "N1".into(), length }
}

#[test]
fn a_cut_plan_never_exceeds_the_stock_length_and_holds_every_piece_once() {
    let mut x: u64 = 12345;
    let pieces: Vec<Piece> = (0..300)
        .map(|i| {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            piece(i, 300.0 + ((x >> 33) as f64) % 11000.0)
        })
        .collect();
    let total: f64 = pieces.iter().map(|p| p.length).sum();
    let packed = steel::pack(pieces, &Stock { length: 12000.0, kerf: 3.0, min_offcut: 0.0 });
    let mut seen: Vec<u64> = Vec::new();
    let mut bars = 0;
    for pattern in &packed.patterns {
        let cuts = 3.0 * (pattern.pieces.len() as f64 - 1.0);
        let held: f64 = pattern.pieces.iter().map(|(_, _, length, _)| length).sum();
        assert!(pattern.offcut >= -1e-9 && (held + cuts + pattern.offcut - 12000.0).abs() < 0.02, "{held} {cuts} {}", pattern.offcut);
        bars += pattern.count;
        seen.extend(pattern.pieces.iter().flat_map(|(.., ids)| ids.iter().copied()));
    }
    seen.sort_unstable();
    assert_eq!(bars, packed.stock_bars);
    assert_eq!(seen, (0..300).collect::<Vec<u64>>());
    assert!((packed.used - total).abs() < 0.02 && packed.waste >= 0.0);
    // Never fewer bars than the steel needs.
    assert!(packed.stock_bars as f64 * 12000.0 >= total);
}

#[test]
fn kerf_makes_a_bar_hold_one_piece_fewer_where_it_matters() {
    let three = || vec![piece(1, 4000.0), piece(2, 4000.0), piece(3, 4000.0)];
    assert_eq!(steel::pack(three(), &Stock { length: 12000.0, kerf: 0.0, min_offcut: 0.0 }).stock_bars, 1);
    assert_eq!(steel::pack(three(), &Stock { length: 12000.0, kerf: 1.0, min_offcut: 0.0 }).stock_bars, 2);
}

#[test]
fn an_offcut_of_the_minimum_is_reusable_and_not_waste() {
    let pieces = || vec![piece(1, 5950.0), piece(2, 5950.0)];
    let kept = steel::pack(pieces(), &Stock { length: 12000.0, kerf: 0.0, min_offcut: 100.0 });
    assert_eq!((kept.reusable.clone(), kept.waste), (vec![100.0], 0.0));
    let none = steel::pack(pieces(), &Stock { length: 12000.0, kerf: 0.0, min_offcut: 0.0 });
    assert_eq!((none.reusable.len(), none.waste), (0, 100.0));
}

#[test]
fn a_cut_plan_says_its_waste_per_diameter_and_plans_every_bar_once() {
    let mut s = Session::new();
    let host = beam(&mut s, 0);
    let made = arm(&mut s, host);
    let r = run(&mut s, "cut_plan", json!({"stock_length": 12000}));
    let line = |d: f64| r["diameters"].as_array().unwrap().iter().find(|l| near(l["diameter"].as_f64().unwrap(), d)).unwrap().clone();
    // Three bottom bars of 5,950 in stock bars of 12,000: two in one (100 left) and one in another.
    let bottom = line(16.0);
    assert_eq!((bottom["stock_bars"].as_u64(), bottom["pieces"].as_u64()), (Some(2), Some(3)));
    assert!(near(bottom["waste_mm"].as_f64().unwrap(), 6150.0), "{bottom}");
    assert!((bottom["waste_percent"].as_f64().unwrap() - 25.63).abs() < 0.011, "{bottom}");
    let offcuts: Vec<f64> = bottom["patterns"].as_array().unwrap().iter().map(|p| p["offcut"].as_f64().unwrap()).collect();
    assert_eq!(offcuts, [100.0, 6050.0]);
    let mut planned: Vec<u64> = r["diameters"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|l| l["patterns"].as_array().unwrap().iter())
        .flat_map(|p| p["pieces"].as_array().unwrap().iter())
        .flat_map(|piece| piece["ids"].as_array().unwrap().iter().map(|id| id.as_u64().unwrap()))
        .collect();
    planned.sort_unstable();
    let mut expected = made;
    expected.sort_unstable();
    assert_eq!(planned, expected);
    assert!(
        r["notes"].as_array().unwrap().iter().any(|n| n.as_str().is_some_and(|n| n.contains("16 mm CA-50: 2 stock bars"))),
        "{}",
        r["notes"]
    );
    // Run again, it is the same plan.
    assert_eq!(run(&mut s, "cut_plan", json!({"stock_length": 12000})), r);
}

#[test]
fn a_piece_longer_than_the_stock_bar_is_a_note_and_is_not_planned() {
    let mut s = Session::new();
    let host = beam(&mut s, 0);
    arm(&mut s, host);
    let r = run(&mut s, "cut_plan", json!({"stock_length": 5000}));
    let notes: Vec<&str> = r["notes"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(notes.iter().any(|n| n.contains("longer than the 5000 mm stock bar")), "{notes:?}");
    let bottom = r["diameters"].as_array().unwrap().iter().find(|l| near(l["diameter"].as_f64().unwrap(), 16.0)).unwrap();
    assert_eq!((bottom["stock_bars"].as_u64(), bottom["pieces"].as_u64()), (Some(0), Some(0)));
}

#[test]
fn a_cut_plan_needs_a_stock_length_and_reads_only_what_it_knows() {
    let mut s = Session::new();
    for p in [
        json!({}),
        json!({"stock_length": 5}),
        json!({"stock_length": 1e300}),
        json!({"stock_length": "x"}),
        json!({"stock_length": 12000, "kerf": 500}),
    ] {
        let e = s.execute("cut_plan", &p).unwrap_err();
        assert!(e.contains("stock_length") || e.contains("kerf"), "{p}: {e}");
    }
    let e = s.execute("cut_plan", &json!({"stock_length": 12000, "round_to": 50})).unwrap_err();
    assert!(e.contains("round_to"), "{e}");
    let e = s.execute("steel_list", &json!({"stock_len": 12000})).unwrap_err();
    assert!(e.contains("stock_len"), "{e}");
    // The project is untouched by both.
    let before = s.model.clone();
    run(&mut s, "cut_plan", json!({"stock_length": 12000}));
    run(&mut s, "steel_list", json!({}));
    assert_eq!(*s.model, *before);
}

/// Payloads for the sweep: a beam armed in the base project, a bar with no host, and what the steel commands and templates are
/// given that they are not meant for.
pub(crate) fn sweep_payloads(base: &mut Session) -> Vec<(&'static str, Value)> {
    let host = base.execute("beam", &json!({"points": [[500, 500], [5500, 500]]})).map_or(0, |r| r["ids"][0].as_u64().unwrap_or(0));
    let spec = json!({"host": host, "cover": 25, "bottom": {"count": 3, "diameter": 16}, "stirrup": {"diameter": 5, "spacing": 150}});
    let _ = base.execute("reinforce", &spec);
    let _ = base.execute("rebar", &json!({"points": [[0, 0, 0], [3000, 0, 0]], "diameter": 20}));
    let p = |cmd: &'static str, v: Value| (cmd, v);
    vec![
        p("steel_list", json!({})),
        p("steel_list", json!({"ids": [host], "coil_to": 10, "stock_length": 12000})),
        p("steel_list", json!({"ids": [u64::MAX]})),
        p("steel_list", json!({"ids": []})),
        p("steel_list", json!({"ids": [host, host, 100000]})),
        p("steel_list", json!({"level": []})),
        p("steel_list", json!({"level": "no such level"})),
        p("steel_list", json!({"coil_to": 1e300})),
        p("steel_list", json!({"coil_to": -1})),
        p("steel_list", json!({"coil_to": "x"})),
        p("steel_list", json!({"stock_length": 0})),
        p("steel_list", json!({"stock_length": 5e-324})),
        p("steel_list", json!({"stock_length": f64::MAX})),
        p("steel_list", json!({"stock_length": null, "path": 5})),
        p("steel_list", json!({"path": "/nonexistent/folder/list.csv"})),
        p("steel_list", json!({"unknown": 1})),
        p("cut_plan", json!({"stock_length": 12000})),
        p("cut_plan", json!({"stock_length": 12000, "kerf": 100, "min_offcut": 12000, "coil_to": 100})),
        p("cut_plan", json!({"stock_length": 12000, "kerf": 1e300})),
        p("cut_plan", json!({"stock_length": 12000, "kerf": -1})),
        p("cut_plan", json!({"stock_length": 12000, "min_offcut": 1e300})),
        p("cut_plan", json!({"stock_length": 12000, "min_offcut": "x"})),
        p("cut_plan", json!({"stock_length": 12000, "ids": [u64::MAX]})),
        p("cut_plan", json!({"stock_length": 0})),
        p("cut_plan", json!({"stock_length": -1})),
        p("cut_plan", json!({"stock_length": 5e-324})),
        p("cut_plan", json!({"stock_length": 1e300})),
        p("cut_plan", json!({"stock_length": "x"})),
        p("cut_plan", json!({"stock_length": null})),
        p("cut_plan", json!({"stock_length": 12000, "path": "/nonexistent/folder/plan.xlsx"})),
        p("schedule", json!({"template": 5})),
        p("schedule", json!({"template": "zap"})),
        p("schedule", json!({"template": "steel_list", "host": u64::MAX})),
        p("schedule", json!({"template": "steel_list", "host": [1]})),
        p("schedule", json!({"template": "steel_list", "host": host, "level": []})),
        p("schedule", json!({"template": "steel_list", "level": "no such level"})),
        p("schedule", json!({"template": "steel_summary", "show": 5})),
        p("schedule", json!({"template": "steel_summary", "show": "none"})),
        p("schedule", json!({"template": "steel_list", "category": "wall"})),
        p("schedule", json!({"template": "steel_list", "name": 5})),
        p("schedule", json!({"template": "steel_list", "fields": [5], "sort": "x"})),
    ]
}

/// What marks cost for each bar at two sizes, as `crates/core/CLAUDE.md` asks (compare the cost per element, not the totals):
/// `cargo test -p bimcraft-core marks_cost -- --ignored --nocapture`.
#[test]
#[ignore = "a timing, run by hand"]
fn marks_cost_per_bar_at_two_sizes() {
    for (hosts, bars) in [(2_u32, "2,000"), (20, "20,000")] {
        let mut s = Session::new();
        for k in 0..hosts {
            let host = beam(&mut s, i32::try_from(k * 300).unwrap_or(0));
            let spec =
                json!({"points": [[50, 0, -400], [5950, 0, -400]], "diameter": 16, "host": host, "count": 1000, "step": [0, 0.1, 0]});
            run(&mut s, "rebar", spec);
        }
        let started = std::time::Instant::now();
        let r = run(&mut s, "steel_list", json!({}));
        let list = started.elapsed();
        let started = std::time::Instant::now();
        run(&mut s, "schedule", json!({"template": "steel_list", "name": "Timing"}));
        let schedule = started.elapsed();
        let n = u32::try_from(r["totals"]["bars"].as_u64().unwrap_or(1).max(1)).unwrap_or(1);
        println!("{bars} bars: steel_list {list:?} ({:?} a bar), schedule {schedule:?} ({:?} a bar)", list / n, schedule / n);
    }
}
