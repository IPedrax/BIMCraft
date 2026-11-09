//! NBR 5410:2004 item 9.5.2 by hand: the lighting points and the outlets of a dwelling's rooms.

use super::*;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn lighting_is_100_va_up_to_6_m2_and_60_va_for_each_whole_4_m2_above() {
    assert!(near(lighting_va(2.0), 100.0));
    assert!(near(lighting_va(6.0), 100.0));
    assert!(near(lighting_va(6.5), 100.0));
    assert!(near(lighting_va(9.99), 100.0));
    assert!(near(lighting_va(10.0), 160.0));
    assert!(near(lighting_va(14.0), 220.0));
    assert!(near(lighting_va(20.0), 100.0 + 3.0 * 60.0));
}

#[test]
fn rooms_are_known_by_their_names() {
    for (name, kind) in [
        ("Banheiro", Kind::Bath),
        ("Lavabo", Kind::Bath),
        ("WC social", Kind::Bath),
        ("Cozinha", Kind::Kitchen),
        ("Copa-cozinha", Kind::Kitchen),
        ("Copa", Kind::Kitchen),
        ("Área de serviço", Kind::Service),
        ("Lavanderia", Kind::Service),
        ("Sala de estar", Kind::Dry),
        ("Dormitório 2", Kind::Dry),
        ("Suíte", Kind::Dry),
        ("Varanda", Kind::Veranda),
        ("Circulação", Kind::Other),
        ("", Kind::Other),
    ] {
        assert_eq!(kind_of(name), kind, "{name}");
    }
}

#[test]
fn outlets_follow_the_perimeter_by_kind() {
    assert_eq!(min_points(Kind::Bath, 12.0, 14.0), 1);
    // Wet rooms: one for each 3.5 m of perimeter, a whole number up.
    assert_eq!(min_points(Kind::Kitchen, 9.0, 12.0), 4);
    assert_eq!(min_points(Kind::Service, 4.0, 7.0), 2);
    assert_eq!(min_points(Kind::Service, 1.0, 3.0), 1);
    // Rooms and bedrooms: one for each 5 m.
    assert_eq!(min_points(Kind::Dry, 12.0, 14.0), 3);
    assert_eq!(min_points(Kind::Dry, 12.0, 10.0), 2);
    assert_eq!(min_points(Kind::Veranda, 3.0, 8.0), 1);
    // The others: one up to 6 m², by perimeter above.
    assert_eq!(min_points(Kind::Other, 2.0, 6.0), 1);
    assert_eq!(min_points(Kind::Other, 6.0, 10.0), 1);
    assert_eq!(min_points(Kind::Other, 6.5, 10.5), 3);
}

#[test]
fn power_is_600_va_for_the_first_3_points_of_a_wet_room_and_100_after() {
    assert!(near(power_va(Kind::Bath, 1, false), 600.0));
    assert!(near(power_va(Kind::Kitchen, 3, false), 1800.0));
    assert!(near(power_va(Kind::Kitchen, 5, false), 1800.0 + 200.0));
    assert!(near(power_va(Kind::Service, 4, false), 1900.0));
    assert!(near(power_va(Kind::Dry, 3, false), 300.0));
    assert!(near(power_va(Kind::Veranda, 2, false), 200.0));
    assert!(near(power_va(Kind::Other, 4, false), 400.0));
}

#[test]
fn the_alternative_is_600_va_for_the_first_2_points() {
    assert!(near(power_va(Kind::Kitchen, 3, true), 1300.0));
    assert!(near(power_va(Kind::Kitchen, 5, true), 1200.0 + 300.0));
    assert!(near(power_va(Kind::Bath, 1, true), 600.0));
    assert!(near(power_va(Kind::Dry, 3, true), 300.0));
}

#[test]
fn a_flat_is_added_up_with_the_points_it_has() {
    let rooms = [
        Room { name: "Cozinha", kind: Kind::Kitchen, area: 9.0, perimeter: 12.0, provided: None },
        Room { name: "Banheiro", kind: Kind::Bath, area: 4.0, perimeter: 8.0, provided: Some(2) },
        Room { name: "Sala", kind: Kind::Dry, area: 16.0, perimeter: 16.0, provided: None },
    ];
    let t = total(&rooms, false);
    assert_eq!(t.rooms[0].minimum, 4);
    assert_eq!(t.rooms[0].points, 4);
    assert!(near(t.rooms[0].tug_va, 1900.0));
    // The bathroom has two where one is the least, and is paid for two.
    assert_eq!((t.rooms[1].minimum, t.rooms[1].points), (1, 2));
    assert!(near(t.rooms[1].tug_va, 1200.0));
    assert_eq!(t.rooms[2].points, 4);
    assert!(near(t.rooms[2].tug_va, 400.0));
    assert_eq!(t.wet_points, 6);
    assert!(!t.alternative_possible, "6 points is not above 6");
    assert!(near(t.lighting_va, 100.0 + 100.0 + 220.0), "{}", t.lighting_va);
}

#[test]
fn above_6_wet_points_the_alternative_may_be_taken() {
    let rooms = [
        Room { name: "Cozinha", kind: Kind::Kitchen, area: 12.0, perimeter: 14.0, provided: None },
        Room { name: "Banheiro", kind: Kind::Bath, area: 4.0, perimeter: 8.0, provided: Some(2) },
        Room { name: "Lavanderia", kind: Kind::Service, area: 5.0, perimeter: 9.0, provided: None },
    ];
    let standard = total(&rooms, false);
    let alt = total(&rooms, true);
    assert_eq!(standard.wet_points, 4 + 2 + 3);
    assert!(standard.alternative_possible);
    assert!(!standard.alternative_applied && alt.alternative_applied);
    assert!(alt.tug_va < standard.tug_va);
    // 4 -> 1200 + 200; 2 -> 1200; 3 -> 1200 + 100.
    assert!(near(alt.tug_va, 1400.0 + 1200.0 + 1300.0));
}

#[test]
fn the_alternative_is_not_applied_below_the_threshold() {
    let rooms = [Room { name: "Cozinha", kind: Kind::Kitchen, area: 9.0, perimeter: 12.0, provided: None }];
    let t = total(&rooms, true);
    assert!(!t.alternative_applied && !t.alternative_possible);
    assert!(near(t.tug_va, 1900.0));
}

#[test]
fn a_kitchen_asks_for_two_outlets_over_the_counter() {
    assert_eq!(counter_outlets(Kind::Kitchen, "Cozinha"), 2);
    assert_eq!(counter_outlets(Kind::Kitchen, "Copa"), 2);
    assert_eq!(counter_outlets(Kind::Service, "Lavanderia"), 0);
    assert_eq!(counter_outlets(Kind::Dry, "Sala"), 0);
}

#[test]
fn every_rule_of_every_standard_names_its_source() {
    let sets = rule_sets();
    assert!(!sets.is_empty(), "data/load-points.json did not load");
    for set in sets {
        assert!(!set.standard.trim().is_empty() && !set.source.trim().is_empty(), "{}: no source", set.standard);
        assert!(!set.lighting.source.trim().is_empty(), "{}: the lighting names no source", set.standard);
        assert!(!set.alternative.source.trim().is_empty(), "{}: the alternative names no source", set.standard);
        for key in KINDS {
            let rows: Vec<_> = set.rooms.iter().filter(|r| r.kind == *key).collect();
            assert_eq!(rows.len(), 1, "{}: `{key}` must have one row", set.standard);
            assert!(!rows[0].source.trim().is_empty(), "{}: `{key}` names no source", set.standard);
        }
        assert_eq!(set.rooms.len(), KINDS.len(), "{}: a row of no kind", set.standard);
        let l = &set.lighting;
        for x in [l.va, l.up_to_m2, l.step_m2, l.step_va] {
            assert!(x.is_finite() && x >= 0.0, "{}: lighting {x}", set.standard);
        }
        assert!(l.step_m2 > 0.0);
        for r in &set.rooms {
            for x in [r.first_va, r.va, r.above_m2.unwrap_or(0.0), r.per_perimeter_m.unwrap_or(1.0)] {
                assert!(x.is_finite() && x >= 0.0, "{} {}: {x}", set.standard, r.kind);
            }
            assert!(r.per_perimeter_m.is_none_or(|m| m > 0.0), "{} {}", set.standard, r.kind);
        }
    }
    assert!(rule_set(DEFAULT).is_some(), "the default `{DEFAULT}` is not in the data");
}

#[test]
fn the_same_sum_takes_another_standard_s_numbers() {
    // Made-up numbers, not any standard's: what the sum does comes of the rows alone.
    let set: RuleSet = serde_json::from_str(
        r#"{"standard": "Example", "source": "a test",
            "lighting": {"va": 200, "up_to_m2": 10, "step_m2": 5, "step_va": 50, "source": "t"},
            "alternative": {"above": 100, "first": 1, "source": "t"},
            "rooms": [
              {"kind": "bath", "least": 2, "first": 1, "first_va": 1000, "va": 200, "source": "t"},
              {"kind": "kitchen", "per_perimeter_m": 2, "counter": 3, "first": 1, "first_va": 1000, "va": 200, "source": "t"},
              {"kind": "service", "least": 1, "va": 150, "source": "t"},
              {"kind": "dry", "per_perimeter_m": 4, "va": 150, "source": "t"},
              {"kind": "veranda", "least": 0, "va": 150, "source": "t"},
              {"kind": "other", "least": 1, "above_m2": 8, "per_perimeter_m": 4, "va": 150, "source": "t"}
            ]}"#,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(near(set.lighting_va(10.0), 200.0) && near(set.lighting_va(21.0), 300.0));
    assert_eq!(set.min_points(Kind::Bath, 3.0, 7.0), 2);
    assert_eq!(set.min_points(Kind::Kitchen, 9.0, 12.0), 6);
    assert_eq!(set.min_points(Kind::Veranda, 4.0, 8.0), 0);
    assert_eq!((set.min_points(Kind::Other, 8.0, 12.0), set.min_points(Kind::Other, 9.0, 13.0)), (1, 4));
    assert!(near(set.power_va(Kind::Kitchen, 3, false), 1400.0));
    assert!(near(set.power_va(Kind::Service, 3, false), 450.0));
    assert_eq!(set.counter_outlets(Kind::Kitchen, "Cozinha"), 3);
    let t = set.total(&[Room { name: "Banho", kind: Kind::Bath, area: 3.0, perimeter: 7.0, provided: None }], false);
    assert!(near(t.lighting_va, 200.0) && near(t.tug_va, 1200.0), "{t:?}");
}

mod command {
    use crate::engine::Session;
    use serde_json::{Value, json};

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    #[test]
    fn rooms_given_are_added_up() {
        let mut s = Session::new();
        let r = run(
            &mut s,
            "load_points",
            json!({"rooms": [
                {"name": "Cozinha", "area": 9, "perimeter": 12000},
                {"name": "Banheiro", "area": 4, "perimeter": 8000, "points": 2},
                {"name": "Closet", "area": 3, "perimeter": 7000, "kind": "dry"},
                {"name": "Depósito", "area": 3, "perimeter": 7000},
            ]}),
        );
        let k = &r["rooms"][0];
        assert_eq!(
            (&k["kind"], &k["tug_points"], &k["tug_va"], &k["counter_outlets"]),
            (&json!("kitchen"), &json!(4), &json!(1900.0), &json!(2))
        );
        assert_eq!((&r["rooms"][1]["tug_points"], &r["rooms"][1]["tug_va"]), (&json!(2), &json!(1200.0)));
        assert_eq!(r["rooms"][2]["kind"], "dry");
        assert!(r["rooms"][3]["note"].as_str().is_some_and(|n| n.contains("other")), "{r}");
        assert_eq!(r["lighting_va"], 400.0);
        assert_eq!(r["tug_va"], 1900.0 + 1200.0 + 200.0 + 100.0);
        assert_eq!(r["ok"], true);
        assert_eq!(crate::engine::find("load_points").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn fewer_points_than_the_least_are_not_ok() {
        let mut s = Session::new();
        let r = run(&mut s, "load_points", json!({"rooms": [{"name": "Sala", "area": 20, "perimeter": 18000, "points": 2}]}));
        assert_eq!((&r["rooms"][0]["tug_minimum"], &r["rooms"][0]["ok"], &r["ok"]), (&json!(4), &json!(false), &json!(false)));
    }

    #[test]
    fn the_alternative_is_taken_only_when_asked_and_possible() {
        let mut s = Session::new();
        let rooms = json!([
            {"name": "Cozinha", "area": 12, "perimeter": 14000},
            {"name": "Banheiro", "area": 4, "perimeter": 8000, "points": 2},
            {"name": "Lavanderia", "area": 5, "perimeter": 9000},
        ]);
        let a = run(&mut s, "load_points", json!({"rooms": rooms}));
        let b = run(&mut s, "load_points", json!({"rooms": rooms, "alternative": true}));
        assert_eq!((&a["alternative_possible"], &a["alternative_applied"]), (&json!(true), &json!(false)));
        assert_eq!(b["alternative_applied"], true);
        assert!(b["tug_va"].as_f64() < a["tug_va"].as_f64());
    }

    #[test]
    fn the_rooms_of_the_model_are_measured_inside_the_walls() {
        let mut s = Session::new();
        run(&mut s, "wall", json!({"points": [[0, 0], [5000, 0], [5000, 4000], [0, 4000]], "closed": true}));
        run(&mut s, "room", json!({"point": [2500, 2000], "name": "Dormitório"}));
        let r = run(&mut s, "load_points", json!({}));
        let room = &r["rooms"][0];
        let area = room["area_m2"].as_f64().unwrap_or(0.0);
        assert!(area > 15.0 && area < 20.0, "inside the walls, less than 5 m by 4 m: {area}");
        assert_eq!(room["kind"], "dry");
        assert_eq!(room["lighting_va"], super::lighting_va(area));
        assert_eq!(r["skipped"], json!([]));
    }

    #[test]
    fn hostile_input_is_refused_and_changes_nothing() {
        let mut s = Session::new();
        for p in [
            json!([]),
            json!({"rooms": 5}),
            json!({"rooms": [5]}),
            json!({"rooms": [{"name": "A"}]}),
            json!({"rooms": [{"name": "A", "area": -1, "perimeter": 1}]}),
            json!({"rooms": [{"name": "A", "area": 1e300, "perimeter": 1}]}),
            json!({"rooms": [{"name": "A", "area": 1, "perimeter": 1, "points": -1}]}),
            json!({"rooms": [{"name": "A", "area": 1, "perimeter": 1, "points": 1e300}]}),
            json!({"rooms": [{"name": "A", "area": 1, "perimeter": 1, "kind": "attic"}]}),
            json!({"rooms": [{"name": "A", "area": 1, "perimeter": 1, "colour": "red"}]}),
            json!({"alternative": "yes"}),
            json!({"units": 1}),
        ] {
            assert!(s.execute("load_points", &p).is_err(), "{p}");
        }
    }

    #[test]
    fn the_answer_names_the_standard_and_each_room_s_source() {
        let mut s = Session::new();
        let r = run(&mut s, "load_points", json!({"rooms": [{"name": "Cozinha", "area": 9, "perimeter": 12000}]}));
        assert_eq!(r["standard"], "NBR 5410:2004");
        let src = r["rooms"][0]["source"].as_str().unwrap_or_default();
        assert!(src.contains("9.5.2.2.1") && src.contains("9.5.2.2.2"), "{r}");
        assert!(r["lighting_source"].as_str().is_some_and(|t| t.contains("9.5.2.1")), "{r}");
        let same =
            run(&mut s, "load_points", json!({"standard": "NBR 5410:2004", "rooms": [{"name": "Cozinha", "area": 9, "perimeter": 12000}]}));
        assert_eq!(same, r);
        assert!(s.execute("load_points", &json!({"standard": "NEC 2023", "rooms": []})).is_err());
    }
}
