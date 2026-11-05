//! The circuits NBR 5410:2004 asks a high-sensitivity residual-current device (30 mA at most) for, by the table of its
//! "additional protection", worked by hand.

use super::*;

fn point(place: Place, kind: Kind) -> Point {
    Point { place, kind, rating: None, height: None, outdoor: false }
}

fn circuit(points: Vec<Point>) -> Circuit {
    Circuit { points, residual: None, it_continuity: false }
}

fn required(c: &Circuit, residential: bool) -> bool {
    check(c, residential).required
}

#[test]
fn every_point_where_there_is_a_bath_or_a_shower_needs_it() {
    for kind in [Kind::Outlet, Kind::Lighting, Kind::Fixed] {
        assert!(required(&circuit(vec![point(Place::Bath, kind)]), true), "{kind:?}");
        assert!(required(&circuit(vec![point(Place::Bath, kind)]), false), "{kind:?}");
    }
}

#[test]
fn outlets_in_external_areas_need_it() {
    assert!(required(&circuit(vec![point(Place::External, Kind::Outlet)]), true));
    assert!(required(&circuit(vec![point(Place::External, Kind::Outlet)]), false));
    // Its lighting is not an outlet.
    assert!(!required(&circuit(vec![point(Place::External, Kind::Lighting)]), true));
}

#[test]
fn an_inside_outlet_that_may_feed_equipment_outdoors_needs_it() {
    let mut p = point(Place::Dry, Kind::Outlet);
    assert!(!required(&circuit(vec![p.clone()]), true));
    p.outdoor = true;
    assert!(required(&circuit(vec![p]), true));
}

#[test]
fn in_a_dwelling_every_point_of_the_wet_rooms_needs_it() {
    for place in [Place::Kitchen, Place::Laundry, Place::Service, Place::Garage, Place::Wet] {
        for kind in [Kind::Outlet, Kind::Fixed, Kind::Lighting] {
            assert!(required(&circuit(vec![point(place, kind)]), true), "{place:?} {kind:?}");
        }
    }
    assert!(!required(&circuit(vec![point(Place::Dry, Kind::Outlet)]), true), "a bedroom's outlets, on their own, do not");
}

#[test]
fn a_light_2_5_m_up_or_more_in_a_dwelling_is_left_out() {
    let mut p = point(Place::Kitchen, Kind::Lighting);
    p.height = Some(2500.0);
    assert!(!required(&circuit(vec![p.clone()]), true));
    p.height = Some(2499.0);
    assert!(required(&circuit(vec![p.clone()]), true));
    // It does not reach the outlets, or a bath.
    let mut o = point(Place::Kitchen, Kind::Outlet);
    o.height = Some(3000.0);
    assert!(required(&circuit(vec![o]), true));
    let mut b = point(Place::Bath, Kind::Lighting);
    b.height = Some(3000.0);
    assert!(required(&circuit(vec![b]), true));
}

#[test]
fn outside_a_dwelling_only_the_outlets_of_the_wet_rooms_need_it() {
    assert!(required(&circuit(vec![point(Place::Kitchen, Kind::Outlet)]), false));
    assert!(!required(&circuit(vec![point(Place::Kitchen, Kind::Lighting)]), false));
    assert!(!required(&circuit(vec![point(Place::Laundry, Kind::Fixed)]), false));
}

#[test]
fn an_outlet_above_32_a_is_not_covered() {
    let mut p = point(Place::External, Kind::Outlet);
    p.rating = Some(32.0);
    assert!(required(&circuit(vec![p.clone()]), true));
    p.rating = Some(32.5);
    assert!(!required(&circuit(vec![p]), true));
    // Note 1 reaches every outlet, a bathroom's too; what else is in the circuit still asks for it.
    let mut b = point(Place::Bath, Kind::Outlet);
    b.rating = Some(63.0);
    assert!(!required(&circuit(vec![b.clone()]), true));
    assert!(required(&circuit(vec![b, point(Place::Bath, Kind::Lighting)]), true));
    let mut k = point(Place::Kitchen, Kind::Outlet);
    k.rating = Some(63.0);
    assert!(!required(&circuit(vec![k]), true));
}

#[test]
fn an_it_circuit_that_must_not_be_cut_is_left_out() {
    let mut c = circuit(vec![point(Place::Bath, Kind::Outlet)]);
    c.it_continuity = true;
    let r = check(&c, true);
    assert!(!r.required && r.exempt, "{r:?}");
}

#[test]
fn a_circuit_asks_for_it_if_any_of_its_points_does_and_says_why() {
    let c = circuit(vec![point(Place::Dry, Kind::Outlet), point(Place::External, Kind::Outlet), point(Place::Bath, Kind::Fixed)]);
    let r = check(&c, true);
    assert!(r.required);
    assert_eq!(r.reasons.len(), 2, "{:?}", r.reasons);
    assert!(r.reasons[0].contains("external") && r.reasons[1].contains("bath"), "{:?}", r.reasons);
}

#[test]
fn what_it_has_is_judged_against_30_ma() {
    let mut c = circuit(vec![point(Place::Bath, Kind::Outlet)]);
    assert_eq!(check(&c, true).ok, Some(false), "none given: it is missing");
    c.residual = Some(30.0);
    assert_eq!(check(&c, true).ok, Some(true));
    c.residual = Some(300.0);
    assert_eq!(check(&c, true).ok, Some(false));
    c.residual = Some(0.0);
    assert_eq!(check(&c, true).ok, Some(true), "a 0 mA is nonsense, but no more than 30");
    // A circuit that does not need it has no verdict.
    let free = circuit(vec![point(Place::Dry, Kind::Lighting)]);
    assert_eq!(check(&free, true).ok, None);
}

#[test]
fn places_are_known_by_a_name() {
    for (name, place) in [
        ("bath", Place::Bath),
        ("external", Place::External),
        ("kitchen", Place::Kitchen),
        ("laundry", Place::Laundry),
        ("service", Place::Service),
        ("garage", Place::Garage),
        ("wet", Place::Wet),
        ("dry", Place::Dry),
    ] {
        assert_eq!(Place::parse(name), Some(place));
    }
    assert_eq!(Place::parse("attic"), None);
    assert_eq!(Kind::parse("outlet"), Some(Kind::Outlet));
    assert_eq!(Kind::parse("lamp"), None);
}

mod command {
    use crate::engine::Session;
    use serde_json::{Value, json};

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("dr_check", &p).unwrap_or_else(|e| panic!("dr_check {p}: {e}"))
    }

    #[test]
    fn circuits_come_back_with_what_they_need() {
        let mut s = Session::new();
        let r = run(
            &mut s,
            json!({"circuits": [
                {"name": "C1 bath", "points": [{"place": "bath", "kind": "outlet"}], "residual": 30},
                {"name": "C2 bedrooms", "points": [{"place": "dry", "kind": "outlet"}]},
                {"name": "C3 kitchen", "points": [{"place": "kitchen", "kind": "outlet", "rating": 20}]},
                {"name": "C4 pump", "points": [{"place": "external", "kind": "outlet"}], "it_continuity": true},
            ]}),
        );
        let c = &r["circuits"];
        assert_eq!((&c[0]["required"], &c[0]["ok"]), (&json!(true), &json!(true)));
        assert_eq!((&c[1]["required"], &c[1]["ok"]), (&json!(false), &json!(null)));
        assert_eq!((&c[2]["required"], &c[2]["ok"]), (&json!(true), &json!(false)));
        assert_eq!(c[3]["exempt"], true);
        assert_eq!(r["missing"], json!(["C3 kitchen"]));
        assert_eq!(r["max_residual_ma"], 30.0);
        assert!(r["not_covered"].as_str().is_some_and(|n| n.contains("volume")), "{r}");
        assert_eq!(crate::engine::find("dr_check").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn a_building_that_is_not_a_dwelling_is_said() {
        let mut s = Session::new();
        let pts = json!([{"place": "kitchen", "kind": "lighting"}]);
        let r = run(&mut s, json!({"circuits": [{"name": "A", "points": pts}]}));
        assert_eq!(r["circuits"][0]["required"], true);
        let r = run(&mut s, json!({"building": "other", "circuits": [{"name": "A", "points": pts}]}));
        assert_eq!(r["circuits"][0]["required"], false);
    }

    #[test]
    fn hostile_input_is_refused() {
        let mut s = Session::new();
        let ok = json!({"place": "bath", "kind": "outlet"});
        for p in [
            json!([]),
            json!({}),
            json!({"circuits": 5}),
            json!({"circuits": [5]}),
            json!({"circuits": [{"name": "A"}]}),
            json!({"circuits": [{"name": "A", "points": []}]}),
            json!({"circuits": [{"name": "A", "points": [5]}]}),
            json!({"circuits": [{"name": "A", "points": [{"place": "attic", "kind": "outlet"}]}]}),
            json!({"circuits": [{"name": "A", "points": [{"place": "bath", "kind": "lamp"}]}]}),
            json!({"circuits": [{"name": "A", "points": [{"place": "bath", "kind": "outlet", "rating": -1}]}]}),
            json!({"circuits": [{"name": "A", "points": [{"place": "bath", "kind": "outlet", "rating": 1e300}]}]}),
            json!({"circuits": [{"name": "A", "points": [{"place": "bath", "kind": "outlet", "height": "x"}]}]}),
            json!({"circuits": [{"name": "A", "points": [{"place": "bath", "kind": "outlet", "colour": 1}]}]}),
            json!({"circuits": [{"name": "A", "points": [ok.clone()], "residual": -5}]}),
            json!({"circuits": [{"name": "A", "points": [ok.clone()], "it_continuity": "yes"}]}),
            json!({"circuits": [{"name": "A", "points": [ok.clone()], "units": 1}]}),
            json!({"building": "castle", "circuits": [{"name": "A", "points": [ok.clone()]}]}),
            json!({"circuits": [{"name": "A", "points": [ok.clone()]}], "units": 1}),
        ] {
            assert!(s.execute("dr_check", &p).is_err(), "{p}");
        }
    }
}
