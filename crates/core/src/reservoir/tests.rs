//! Reservation as NBR 5626:2020 asks: volumes, refill flow and the checks, each of one kind.

use super::*;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9
}

fn input(daily_l: f64) -> Input {
    Input {
        daily_l,
        days: 1.0,
        fire_l: 0.0,
        house: false,
        lower_percent: None,
        compartments: None,
        fill_flow_lps: None,
        pumps: None,
        towers: 1,
        smallest_l: None,
        peak_l: None,
    }
}

fn status(o: &Outcome, id: &str) -> Status {
    o.checks.iter().find(|c| c.id == id).map(|c| c.status).unwrap_or_else(|| panic!("no check {id}"))
}

#[test]
fn the_volume_is_the_days_of_consumption_and_the_fire_reserve_kept_apart() {
    let o = size(&Input { days: 2.0, fire_l: 20_000.0, ..input(30_000.0) });
    assert!(near(o.potable_l, 60_000.0));
    assert!(near(o.total_l, 80_000.0), "the fire reserve is added, not counted as potable");
    assert_eq!(status(&o, "autonomy"), Status::Ok);
    assert_eq!(status(&o, "potability"), Status::Ok);
}

#[test]
fn less_than_a_day_or_more_than_three_fail() {
    assert_eq!(status(&size(&Input { days: 0.5, ..input(1000.0) }), "autonomy"), Status::Fail);
    assert_eq!(status(&size(&Input { days: 3.0, ..input(1000.0) }), "potability"), Status::Ok);
    assert_eq!(status(&size(&Input { days: 3.5, ..input(1000.0) }), "potability"), Status::Fail);
}

#[test]
fn the_fire_reserve_does_not_count_against_the_three_days() {
    let o = size(&Input { days: 3.0, fire_l: 100_000.0, ..input(1000.0) });
    assert_eq!(status(&o, "potability"), Status::Ok);
}

#[test]
fn the_refill_flow_empties_the_day_in_six_hours_or_three_for_a_house() {
    let o = size(&input(21_600.0));
    assert!(near(o.refill_hours, 6.0));
    assert!(near(o.min_fill_flow_lps, 1.0), "21600 L in 6 h");
    let h = size(&Input { house: true, ..input(21_600.0) });
    assert!(near(h.refill_hours, 3.0));
    assert!(near(h.min_fill_flow_lps, 2.0));
    assert_eq!(status(&o, "refill"), Status::Pending, "no flow given");
    assert_eq!(status(&size(&Input { fill_flow_lps: Some(1.0), ..input(21_600.0) }), "refill"), Status::Ok);
    assert_eq!(status(&size(&Input { fill_flow_lps: Some(0.99), ..input(21_600.0) }), "refill"), Status::Fail);
}

#[test]
fn an_elevated_reservoir_has_two_compartments_but_a_house() {
    assert_eq!(status(&size(&input(1000.0)), "compartments"), Status::Pending);
    assert_eq!(status(&size(&Input { compartments: Some(1), ..input(1000.0) }), "compartments"), Status::Fail);
    assert_eq!(status(&size(&Input { compartments: Some(2), ..input(1000.0) }), "compartments"), Status::Ok);
    assert!(size(&Input { house: true, ..input(1000.0) }).checks.iter().all(|c| c.id != "compartments"));
}

#[test]
fn the_smallest_compartment_takes_the_peak_of_a_maintenance() {
    let c = |s, p| size(&Input { smallest_l: s, peak_l: p, ..input(1000.0) });
    assert_eq!(status(&c(None, None), "smallest_compartment"), Status::Pending);
    assert_eq!(status(&c(Some(500.0), None), "smallest_compartment"), Status::Pending);
    assert_eq!(status(&c(Some(500.0), Some(400.0)), "smallest_compartment"), Status::Ok);
    assert_eq!(status(&c(Some(300.0), Some(400.0)), "smallest_compartment"), Status::Fail);
}

#[test]
fn a_pump_station_has_two_pumps_and_a_spare_for_several_towers() {
    assert_eq!(status(&size(&input(1000.0)), "pumps"), Status::Pending);
    assert_eq!(status(&size(&Input { pumps: Some(1), ..input(1000.0) }), "pumps"), Status::Fail);
    assert_eq!(status(&size(&Input { pumps: Some(2), ..input(1000.0) }), "pumps"), Status::Ok);
    assert!(size(&input(1000.0)).checks.iter().all(|c| c.id != "spare_pump"));
    assert_eq!(status(&size(&Input { towers: 2, ..input(1000.0) }), "spare_pump"), Status::Pending);
}

#[test]
fn the_split_is_the_projects_and_the_usual_one_only_a_suggestion() {
    let o = size(&input(10_000.0));
    assert!(o.split.is_none());
    assert!(near(o.suggested_lower_percent, 60.0));
    let o = size(&Input { lower_percent: Some(70.0), ..input(10_000.0) });
    let s = o.split.expect("a split");
    assert!(near(s.lower_l, 7000.0) && near(s.upper_l, 3000.0), "{s:?}");
    assert!(o.checks.iter().all(|c| c.id != "split"), "the standard fixes none");
}

#[test]
fn every_check_names_its_kind_and_its_source() {
    let o = size(&Input { pumps: Some(2), compartments: Some(2), fill_flow_lps: Some(5.0), ..input(1000.0) });
    for c in &o.checks {
        assert!(!c.source.is_empty() && !c.text.is_empty(), "{}", c.id);
    }
    assert!(o.checks.iter().any(|c| c.kind == Kind::Requirement));
    assert_eq!(Kind::Historical.name(), "historical reference");
}

mod command {
    use serde_json::{Value, json};

    use crate::engine::Session;

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("reservoir_size", &p).unwrap_or_else(|e| panic!("reservoir_size {p}: {e}"))
    }

    #[test]
    fn a_building_by_its_people() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"people": 120, "per_person_l": 200, "fire_l": 15000, "pumps": 2, "compartments": 2}));
        assert_eq!(r["daily_consumption_l"], json!(24000.0));
        assert_eq!(r["potable_volume_l"], json!(24000.0));
        assert_eq!(r["total_volume_l"], json!(39000.0));
        assert_eq!(r["consumption"]["kind"], json!("configurable assumption"));
        assert_eq!(r["ok"], json!(true));
        assert!(r["pending"].as_array().is_some_and(|p| p.iter().any(|x| x == "refill")), "{r}");
        assert_eq!(crate::engine::find("reservoir_size").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn a_failed_check_makes_it_not_ok_and_the_others_still_run() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"daily_l": 10000, "days": 4, "pumps": 1}));
        assert_eq!(r["ok"], json!(false));
        let failed: Vec<&str> =
            r["checks"].as_array().into_iter().flatten().filter(|c| c["status"] == "fail").filter_map(|c| c["id"].as_str()).collect();
        assert_eq!(failed, vec!["potability", "pumps"]);
        assert_eq!(r["total_volume_l"], json!(40000.0));
    }

    #[test]
    fn a_bad_parameter_says_which() {
        let mut s = Session::new();
        for (bad, says) in [
            (json!({}), "daily_l"),
            (json!({"daily_l": 1000, "people": 10}), "people"),
            (json!({"people": 10}), "per_person_l"),
            (json!({"per_person_l": 100}), "people"),
            (json!({"daily_l": -1}), "daily_l"),
            (json!({"daily_l": 1000, "days": 0}), "days"),
            (json!({"daily_l": 1000, "lower_percent": 120}), "lower_percent"),
            (json!({"daily_l": 1000, "pumps": 1.5}), "pumps"),
            (json!({"daily_l": 1000, "towers": 0}), "towers"),
            (json!({"daily_l": 1000, "house": "yes"}), "house"),
            (json!({"daily_l": 1000, "magic": 1}), "magic"),
            (json!("x"), "object"),
        ] {
            let e = s.execute("reservoir_size", &bad).expect_err("should fail");
            assert!(e.contains(says), "{bad}: {e}");
        }
    }
}
