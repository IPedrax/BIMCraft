//! CPFL's GED 13 (v2.24, 18/09/2020) as printed, and its Example 3 worked by hand.

use super::*;

fn near(a: f64, b: f64, e: f64) -> bool {
    (a - b).abs() <= e
}

#[test]
fn tabela_2_gives_the_least_outlets_by_built_area() {
    // (area, outlets of 100 W, kitchen outlets of 600 W, total W)
    for (a, n, k, total) in [
        (6.0, 1, 1, 700.0),
        (8.0, 1, 1, 700.0),
        (8.5, 3, 1, 900.0),
        (15.0, 3, 1, 900.0),
        (20.0, 4, 2, 1600.0),
        (30.0, 5, 2, 1700.0),
        (40.0, 6, 3, 2400.0),
        (115.0, 10, 3, 2800.0),
        (180.0, 12, 3, 3000.0),
        (250.0, 14, 3, 3200.0),
    ] {
        let (cn, ck) = least_outlets(a).unwrap();
        assert_eq!((cn, ck), (n, k), "{a} m2");
        assert!(near(f64::from(cn) * 100.0 + f64::from(ck) * 600.0, total, 1e-9), "{a} m2");
    }
    assert!(least_outlets(250.5).is_none(), "above 250 m2 the outlets are declared");
}

#[test]
fn the_factors_are_as_printed() {
    // Tabela 3
    for (c, f) in [(0.5, 0.86), (1.0, 0.86), (1.5, 0.75), (4.2, 0.52), (6.0, 0.45), (10.0, 0.27), (10.1, 0.24), (40.0, 0.24)] {
        assert!(near(lighting_factor(c), f, 1e-9), "{c} kW");
    }
    // Tabela 4
    for (n, f) in [(1, 1.0), (2, 1.0), (3, 0.84), (6, 0.65), (7, 0.60), (13, 0.46), (20, 0.40), (25, 0.38), (40, 0.38)] {
        assert!(near(heater_factor(n), f, 1e-9), "{n}");
    }
    // Tabela 5, 6, 7, 9
    assert_eq!([1, 2, 3, 9].map(boiler_factor), [1.0, 0.72, 0.62, 0.62]);
    assert_eq!([1, 2, 4, 5, 6, 7, 8, 9].map(appliance_factor), [1.0, 0.7, 0.7, 0.6, 0.6, 0.5, 0.5, 0.5]);
    assert_eq!(
        [1, 2, 3, 4, 9, 10, 11, 12, 15, 16, 25, 30].map(stove_factor),
        [1.0, 0.6, 0.48, 0.4, 0.31, 0.3, 0.3, 0.28, 0.28, 0.26, 0.26, 0.26]
    );
    assert_eq!([1, 10, 11, 20, 21, 31, 51, 200].map(air_factor), [1.0, 1.0, 0.9, 0.9, 0.82, 0.8, 0.75, 0.75]);
    // Tabela 10, by the rank of the motor
    assert_eq!([1, 2, 3, 5, 6].map(motor_factor), [1.0, 0.9, 0.8, 0.8, 0.7]);
}

#[test]
fn the_tables_of_equipment_are_as_printed() {
    assert_eq!(window_air(14000.0), Some((2100.0, 1900.0)));
    assert_eq!(window_air(7100.0), Some((1100.0, 900.0)));
    assert_eq!(window_air(30000.0), Some((4000.0, 3600.0)));
    assert_eq!(window_air(13000.0), None);
    assert_eq!(motor(1.0, 3), Some((1.52, 1.05)));
    assert_eq!(motor(10.0, 3), Some((11.54, 8.89)));
    assert_eq!(motor(0.5, 1), Some((1.18, 0.79)));
    assert_eq!(motor(5.0, 1), Some((6.16, 5.11)));
    assert_eq!(motor(20.0, 1), None, "Tabela 14 stops at 15 cv");
    assert_eq!(motor(2.5, 3), None);
}

#[test]
fn the_category_follows_the_load_and_the_demand() {
    let a = |c: f64, d: f64| category(System::S127_220, c, d).map(|k| k.name);
    assert_eq!(a(5.0, 0.0), Some("A1"));
    assert_eq!(a(10.0, 0.0), Some("A2"));
    assert_eq!(a(15.0, 0.0), Some("B1"));
    assert_eq!(a(20.0, 0.0), Some("B2"));
    assert_eq!(a(40.0, 22.0), Some("C1"));
    assert_eq!(a(46.1, 32.96), Some("C3"));
    assert_eq!(a(60.0, 47.0), Some("C4"));
    assert_eq!(a(70.0, 76.0), Some("C6"));
    assert_eq!(a(70.0, 76.1), None);
    assert_eq!(a(75.1, 10.0), None);
    let b = |c: f64, d: f64| category(System::S220_380, c, d).map(|k| k.name);
    assert_eq!(b(9.0, 0.0), Some("A3"));
    assert_eq!(b(14.0, 0.0), Some("A4"));
    assert_eq!(b(20.0, 0.0), Some("B3"));
    assert_eq!(b(30.0, 26.0), Some("C7"));
    assert_eq!(b(50.0, 66.0), Some("C10"));
    assert_eq!(b(70.0, 82.0), Some("C11"));
    // The pieces of a category, as Tabela 1A prints them.
    let c3 = category(System::S127_220, 46.1, 33.0).unwrap();
    assert_eq!((c3.breaker, c3.cable, c3.conduit, c3.earth), (100.0, 35.0, 40.0, 10.0));
    let c11 = category(System::S220_380, 70.0, 80.0).unwrap();
    assert_eq!((c11.breaker, c11.cable, c11.conduit, c11.earth), (125.0, 50.0, 50.0, 16.0));
}

fn load(w: f64, count: u32) -> Load {
    Load { w, count }
}

/// The residence of the GED's Example 3: 180 m², 12 rooms and the appliances it lists.
fn example_3() -> Input {
    Input {
        residential: true,
        area: Some(180.0),
        outlets: None,
        lighting_points: 12,
        other: None,
        heaters: vec![load(6500.0, 4), load(3000.0, 1), load(1000.0, 1)],
        boilers: vec![],
        appliances: vec![load(1500.0, 1), load(2000.0, 1), load(2500.0, 1)],
        stoves: vec![],
        air: vec![Air { va: 2100.0, w: 1900.0, count: 2 }],
        motors: vec![Motor { kva: 1.52, kw: 1.05, count: 2 }],
        special: vec![],
        hydro: vec![],
        commercial_air: false,
    }
}

#[test]
fn example_3_of_the_ged_comes_out() {
    let r = compute(&example_3()).unwrap();
    // C = 3000 + 1200 + 39800 + 2100 W.
    assert!(near(r.installed_kw, 46.1, 1e-9), "{}", r.installed_kw);
    assert!(near(r.terms.a, 4.2 * 0.52, 1e-9), "{}", r.terms.a);
    assert!(near(r.terms.b, 19.5, 1e-9));
    assert!(near(r.terms.d, 4.2, 1e-9));
    assert!(near(r.terms.f, 4.2, 1e-9));
    assert!(near(r.terms.g, 1.52 + 1.52 * 0.9, 1e-9));
    // The GED gets 32.96 kVA with its rounding of g to 2.88.
    assert!(near(r.demand_kva, 32.97, 0.02), "{}", r.demand_kva);
    assert_eq!(r.demand_kva.ceil(), 33.0);
}

#[test]
fn a_big_flat_declares_its_outlets() {
    let mut i = example_3();
    i.area = Some(300.0);
    assert!(compute(&i).is_err(), "above 250 m2 the outlets must be declared");
    i.outlets = Some(20);
    let r = compute(&i).unwrap();
    // 20 outlets of 100 W, the kitchen's 3 of 600 W and the lights.
    assert!(near(r.lighting_outlets_w, 2000.0 + 1800.0 + 1200.0, 1e-9), "{}", r.lighting_outlets_w);
}

#[test]
fn special_equipment_is_taken_at_a_power_factor_of_0_75_by_rank() {
    let mut i = example_3();
    i.special = vec![Special { kind: SpecialKind::ResistanceWeld, kw: 4.0, count: 2 }];
    let r = compute(&i).unwrap();
    // 5.333 kVA at 1 and 5.333 at 0.6.
    assert!(near(r.terms.h, 4.0 / 0.75 * (1.0 + 0.6), 1e-9), "{}", r.terms.h);
}

#[test]
fn the_commercial_air_conditioners_have_their_own_factor() {
    let mut i = example_3();
    i.air = vec![Air { va: 2100.0, w: 1900.0, count: 11 }];
    assert!(near(compute(&i).unwrap().terms.f, 11.0 * 2.1, 1e-9));
    i.commercial_air = true;
    assert!(near(compute(&i).unwrap().terms.f, 11.0 * 2.1 * 0.9, 1e-9));
}

mod command {
    use crate::engine::Session;
    use serde_json::{Value, json};

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("demand", &p).unwrap_or_else(|e| panic!("demand {p}: {e}"))
    }

    fn example_3() -> Value {
        json!({
            "utility": "CPFL",
            "area": 180,
            "lighting_points": 12,
            "heaters": [{"w": 6500, "count": 4}, {"w": 3000}, {"w": 1000}],
            "appliances": [{"w": 1500}, {"w": 2000}, {"w": 2500}],
            "air_conditioners": [{"btu": 14000, "count": 2}],
            "motors": [{"cv": 1, "phases": 3, "count": 2}]
        })
    }

    #[test]
    fn the_ged_example_comes_out() {
        let mut s = Session::new();
        let r = run(&mut s, example_3());
        assert_eq!(r["utility"], "CPFL");
        assert!(r["installed_kw"].as_f64().is_some_and(|x| (x - 46.1).abs() < 1e-6), "{r}");
        assert_eq!(r["installed_kw_rounded_up"], 47.0);
        assert!(r["demand_kva"].as_f64().is_some_and(|x| (x - 32.97).abs() < 0.02), "{r}");
        assert_eq!(r["demand_kva_rounded_up"], 33.0);
        assert_eq!(r["category"]["name"], "C3");
        assert_eq!(r["category"]["breaker_a"], 100.0);
        assert!(r["source"].as_str().is_some_and(|x| x.contains("GED 13") && x.contains("2.24")), "{r}");
        assert_eq!(crate::engine::find("demand").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn a_small_load_takes_its_category_from_the_load() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"utility": "CPFL", "area": 40, "lighting_points": 4, "heaters": [{"w": 6500}], "system": "220/380"}));
        // 2400 + 400 + 6500 W: 9.3 kW.
        assert_eq!(r["category"]["name"], "A3");
    }

    #[test]
    fn another_utility_is_refused_and_named() {
        let mut s = Session::new();
        for u in ["Light", "COPEL", "xx"] {
            let e = s.execute("demand", &json!({"utility": u, "area": 40})).unwrap_err();
            assert!(e.contains("CPFL") && e.contains("ENEL") && e.contains("CELESC") && e.contains(u), "{e}");
        }
        let e = s.execute("demand", &json!({"area": 40})).unwrap_err();
        assert!(e.contains("utility"), "{e}");
    }

    #[test]
    fn hostile_input_is_refused() {
        let mut s = Session::new();
        let ok = || json!({"utility": "CPFL", "area": 40, "lighting_points": 4});
        let mut bad = vec![json!([]), json!({}), json!({"utility": 5})];
        for (k, v) in [
            ("area", json!(-1)),
            ("area", json!("x")),
            ("area", json!(1e300)),
            ("outlets", json!(-1)),
            ("lighting_points", json!(1e10)),
            ("lighting_points", json!(-2)),
            ("system", json!("110")),
            ("building", json!("castle")),
            ("heaters", json!(5)),
            ("heaters", json!([5])),
            ("heaters", json!([{"w": -1}])),
            ("heaters", json!([{"w": 1e300}])),
            ("heaters", json!([{"w": 100, "count": 1e9}])),
            ("heaters", json!([{"w": 100, "colour": 1}])),
            ("air_conditioners", json!([{"btu": 13000}])),
            ("air_conditioners", json!([{}])),
            ("motors", json!([{"cv": 2.5, "phases": 3}])),
            ("motors", json!([{"cv": 1, "phases": 2}])),
            ("motors", json!([{}])),
            ("special", json!([{"kind": "laser", "kw": 1}])),
            ("special", json!([{"kind": "xray"}])),
            ("units", json!(1)),
        ] {
            let mut p = ok();
            p[k] = v;
            bad.push(p);
        }
        for p in bad {
            assert!(s.execute("demand", &p).is_err(), "{p}");
        }
    }
}

#[test]
fn enel_anexo_a_is_as_printed() {
    let r = |n: &str| enel_row(Network::Aerial, n).map(|r| (r.available_kw, r.breaker, r.connection, r.entry, r.earth));
    assert_eq!(r("A1"), Some((5.0, 40.0, (Some(1), 10.0, 10.0), (10.0, 10.0), 10.0)));
    assert_eq!(r("B2"), Some((14.0, 63.0, (Some(2), 16.0, 10.0), (16.0, 10.0), 16.0)));
    assert_eq!(r("C3"), Some((24.0, 63.0, (Some(3), 16.0, 16.0), (25.0, 25.0), 16.0)));
    assert_eq!(r("C4"), Some((30.0, 80.0, (Some(3), 35.0, 54.6), (35.0, 25.0), 16.0)));
    assert_eq!(r("C6"), Some((48.0, 125.0, (Some(3), 35.0, 54.6), (50.0, 25.0), 25.0)));
    assert_eq!(r("C8"), Some((75.0, 200.0, (Some(3), 95.0, 54.6), (95.0, 50.0), 50.0)));
    assert!(enel_row(Network::Aerial, "C9").is_none());
}

#[test]
fn enel_anexo_d_is_as_printed() {
    let r = |n: &str| enel_row(Network::Underground, n).map(|r| (r.available_kw, r.breaker, r.connection, r.entry, r.earth));
    assert_eq!(r("A1"), Some((8.0, 63.0, (None, 16.0, 16.0), (16.0, 16.0), 16.0)));
    assert_eq!(r("B2"), Some((18.0, 80.0, (None, 25.0, 25.0), (25.0, 25.0), 16.0)));
    assert_eq!(r("C3"), Some((48.0, 125.0, (None, 50.0, 25.0), (50.0, 25.0), 25.0)));
    assert_eq!(r("C4"), Some((57.0, 150.0, (None, 95.0, 25.0), (50.0, 25.0), 50.0)));
    assert_eq!(r("C5"), Some((75.0, 200.0, (None, 150.0, 95.0), (150.0, 95.0), 95.0)));
    assert!(enel_row(Network::Underground, "C6").is_none());
}

#[test]
fn enel_picks_the_row_whose_range_holds_the_load() {
    let name = |n: Network, c: f64, ph: Option<u8>| enel_pick(n, c, ph).map(|p| p.rows.iter().map(|r| r.name).collect::<Vec<_>>());
    assert_eq!(name(Network::Aerial, 3.0, None), Some(vec!["A1"]));
    assert_eq!(name(Network::Aerial, 5.0, None), Some(vec!["A2"]));
    // 10 < C <= 14 is B2 and C1: both, the first being the smaller supply.
    assert_eq!(name(Network::Aerial, 12.0, None), Some(vec!["B2", "C1"]));
    assert_eq!(name(Network::Aerial, 12.0, Some(3)), Some(vec!["C1"]));
    assert_eq!(name(Network::Aerial, 12.0, Some(2)), Some(vec!["B2"]));
    // In the gap between two ranges, the next one up.
    assert_eq!(name(Network::Aerial, 20.5, None), Some(vec!["C4"]));
    assert_eq!(name(Network::Aerial, 3.5, None), Some(vec!["A2"]));
    assert_eq!(name(Network::Aerial, 63.0, None), Some(vec!["C8"]));
    assert_eq!(name(Network::Aerial, 63.1, None), None);
    assert_eq!(name(Network::Underground, 72.0, None), Some(vec!["C5"]));
    assert_eq!(name(Network::Underground, 72.5, None), None);
    assert_eq!(name(Network::Underground, 8.5, None), Some(vec!["A2"]));
    assert_eq!(name(Network::Underground, 0.0, None), Some(vec!["A1"]));
}

mod enel {
    use crate::engine::Session;
    use serde_json::{Value, json};

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("demand", &p).unwrap_or_else(|e| panic!("demand {p}: {e}"))
    }

    #[test]
    fn an_aerial_supply_is_framed_by_its_installed_load() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"utility": "ENEL", "network": "aerial", "installed_kw": 22, "phases": 3}));
        assert_eq!(r["utility"], "ENEL");
        assert_eq!(r["category"]["name"], "C4", "{r}");
        assert_eq!(r["category"]["breaker_a"], 80.0);
        assert_eq!(r["category"]["entry_mm2"], json!({"phase": 35.0, "neutral": 25.0}));
        assert_eq!(r["category"]["available_kw"], 30.0);
        assert!(r["source"].as_str().is_some_and(|x| x.contains("1569") && x.contains("29/01/2024")), "{r}");
        assert_eq!(r["demand_kva"], Value::Null, "the ET leaves the demand to the designer");
        assert!(r["demand_note"].as_str().is_some_and(|x| x.contains("7.7.2")), "{r}");
        assert_eq!(crate::engine::find("demand").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn an_underground_supply_has_its_own_annex() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"utility": "ENEL", "network": "underground", "installed_kw": 40}));
        assert_eq!(r["category"]["name"], "C3");
        assert_eq!(r["category"]["measurement"], "indirect");
        assert_eq!(r["annex"], "D");
    }

    #[test]
    fn the_designers_demand_is_carried_and_never_worked_out() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"utility": "ENEL", "network": "aerial", "installed_kw": 10, "demand_kva": 7.5}));
        assert_eq!(r["demand_kva"], 7.5);
    }

    #[test]
    fn two_candidates_are_both_named() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"utility": "ENEL", "network": "aerial", "installed_kw": 12}));
        assert_eq!(r["candidates"], json!(["B2", "C1"]));
        assert_eq!(r["category"]["name"], "B2");
    }

    #[test]
    fn above_the_table_the_reply_says_where_it_ends() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"utility": "ENEL", "network": "aerial", "installed_kw": 70}));
        assert_eq!(r["category"], Value::Null);
        assert!(r["category_note"].as_str().is_some_and(|n| n.contains("63")), "{r}");
        let r = run(&mut s, json!({"utility": "ENEL", "network": "aerial", "installed_kw": 80}));
        assert!(r["category_note"].as_str().is_some_and(|n| n.contains("75") && n.contains("primary")), "{r}");
    }

    #[test]
    fn hostile_or_misplaced_input_is_refused() {
        let mut s = Session::new();
        for p in [
            json!({"utility": "ENEL"}),
            json!({"utility": "ENEL", "network": "aerial"}),
            json!({"utility": "ENEL", "installed_kw": 5}),
            json!({"utility": "ENEL", "network": "sky", "installed_kw": 5}),
            json!({"utility": "ENEL", "network": "aerial", "installed_kw": -1}),
            json!({"utility": "ENEL", "network": "aerial", "installed_kw": 1e300}),
            json!({"utility": "ENEL", "network": "aerial", "installed_kw": "x"}),
            json!({"utility": "ENEL", "network": "aerial", "installed_kw": 5, "phases": 2.5}),
            json!({"utility": "ENEL", "network": "aerial", "installed_kw": 5, "phases": 4}),
            json!({"utility": "ENEL", "network": "aerial", "installed_kw": 5, "demand_kva": -2}),
            json!({"utility": "ENEL", "network": "aerial", "installed_kw": 5, "area": 40}),
            json!({"utility": "ENEL", "network": "aerial", "installed_kw": 5, "units": 1}),
            json!({"utility": "CPFL", "area": 40, "network": "aerial"}),
            json!({"utility": "CPFL", "area": 40, "installed_kw": 5}),
        ] {
            assert!(s.execute("demand", &p).is_err(), "{p}");
        }
    }
}

#[test]
fn enel_anexo_c_and_e_are_as_printed() {
    let r = |n: Network, c: &str| enel_row(n, c).map(|r| (r.breaker, r.connection, r.entry, r.earth));
    assert_eq!(r(Network::Aerial120240, "A1"), Some((40.0, (Some(1), 10.0, 10.0), (10.0, 10.0), 10.0)));
    assert_eq!(r(Network::Aerial120240, "A2"), Some((50.0, (Some(1), 16.0, 16.0), (16.0, 16.0), 16.0)));
    assert_eq!(r(Network::Aerial120240, "B1"), Some((50.0, (Some(2), 16.0, 16.0), (16.0, 16.0), 16.0)));
    assert!(enel_row(Network::Aerial120240, "C1").is_none());
    assert_eq!(r(Network::Submersible, "B1"), Some((63.0, (None, 16.0, 16.0), (16.0, 16.0), 16.0)));
    assert_eq!(r(Network::Submersible, "C5"), Some((160.0, (None, 95.0, 95.0), (70.0, 70.0), 35.0)));
    assert_eq!(r(Network::Submersible, "C7"), Some((225.0, (None, 150.0, 150.0), (120.0, 120.0), 70.0)));
    assert_eq!(r(Network::Submersible, "C9"), Some((300.0, (None, 185.0, 185.0), (185.0, 185.0), 95.0)));
    assert_eq!(enel_row(Network::Submersible, "C4").and_then(|r| r.junction_box), Some("type 3"));
    assert_eq!(enel_row(Network::Submersible, "C3").and_then(|r| r.junction_box), Some("type 2"));
    let name = |n: Network, c: f64| enel_pick(n, c, None).map(|p| p.rows.iter().map(|r| r.name).collect::<Vec<_>>());
    assert_eq!(name(Network::Aerial120240, 5.0), Some(vec!["A2"]));
    assert_eq!(name(Network::Aerial120240, 11.0), Some(vec!["B1"]));
    assert_eq!(name(Network::Aerial120240, 11.5), None);
    assert_eq!(name(Network::Submersible, 100.0), Some(vec!["C9"]));
    assert_eq!(name(Network::Submersible, 100.5), None);
    assert_eq!(name(Network::Submersible, 19.0), Some(vec!["B3", "C1"]));
}

#[test]
fn enel_anexo_b_is_picked_by_the_current_of_the_demand() {
    let p = |a: f64, ph: Option<u8>| delta_pick(a, ph).map(|rows| rows.iter().map(|r| (r.names.join("/"), r.breaker)).collect::<Vec<_>>());
    assert_eq!(p(40.0, None), Some(vec![("A1".into(), 50.0), ("B3".into(), 50.0), ("C3".into(), 50.0)]));
    assert_eq!(p(40.0, Some(2)), Some(vec![("B3".into(), 50.0)]));
    assert_eq!(p(40.0, Some(3)), Some(vec![("C3".into(), 50.0)]));
    assert_eq!(p(90.0, Some(3)), Some(vec![("B6/C6".into(), 100.0)]));
    assert_eq!(p(300.0, None).map(|r| r[0].1), Some(300.0));
    assert_eq!(p(300.1, None), None);
    let r = delta_pick(250.0, None).and_then(|r| r.first().copied());
    assert_eq!(
        r.map(|r| (r.breaker, r.connection, r.entry, r.earth, r.measurement)),
        Some((275.0, (Some(3), 150.0, 80.0), 150.0, 95.0, "indirect"))
    );
}

mod enel_more {
    use crate::engine::Session;
    use serde_json::{Value, json};

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("demand", &p).unwrap_or_else(|e| panic!("demand {p}: {e}"))
    }

    #[test]
    fn the_other_networks_have_their_annexes() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"utility": "ENEL", "network": "aerial_120_240", "installed_kw": 8}));
        assert_eq!((&r["annex"], &r["category"]["name"]), (&json!("C"), &json!("B1")));
        let r = run(&mut s, json!({"utility": "ENEL", "network": "underground_submersible", "installed_kw": 40}));
        assert_eq!((&r["annex"], &r["category"]["name"], &r["category"]["junction_box"]), (&json!("E"), &json!("C4"), &json!("type 3")));
        assert_eq!(r["category"]["available_kw"], Value::Null);
        let r = run(&mut s, json!({"utility": "ENEL", "network": "aerial_delta", "demand_current_a": 70, "phases": 3}));
        assert_eq!(r["annex"], "B");
        assert_eq!(r["category"]["names"], json!(["C5"]));
        assert_eq!(r["category"]["breaker_a"], 80.0);
        assert_eq!(r["category"]["entry_mm2"], 25.0);
        assert_eq!(r["installed_kw"], Value::Null);
    }

    #[test]
    fn the_delta_network_goes_by_current_and_the_others_by_load() {
        let mut s = Session::new();
        for p in [
            json!({"utility": "ENEL", "network": "aerial_delta", "installed_kw": 10}),
            json!({"utility": "ENEL", "network": "aerial_delta"}),
            json!({"utility": "ENEL", "network": "aerial_delta", "demand_current_a": -1}),
            json!({"utility": "ENEL", "network": "aerial_delta", "demand_current_a": 1e300}),
            json!({"utility": "ENEL", "network": "aerial", "installed_kw": 10, "demand_current_a": 40}),
        ] {
            assert!(s.execute("demand", &p).is_err(), "{p}");
        }
        let r = run(&mut s, json!({"utility": "ENEL", "network": "aerial_delta", "demand_current_a": 400}));
        assert_eq!(r["category"], Value::Null);
        assert!(r["category_note"].as_str().is_some_and(|n| n.contains("300")), "{r}");
    }
}

/// Celesc's Tabela 01 (N-321.0001, rev. December 2025) as printed: each row's range, demand, breaker, phases and wires, branches and
/// earthing conductor.
#[test]
fn celesc_tabela_01_is_as_printed() {
    type Printed = (&'static str, f64, f64, Option<(f64, f64)>, f64, u8, u8, (f64, f64), f64, (f64, f64));
    let printed: [Printed; 13] = [
        ("A1", 0.0, 11.0, None, 50.0, 1, 2, (10.0, 10.0), 10.0, (10.0, 16.0)),
        ("A2", 11.0, 13.0, None, 63.0, 1, 2, (10.0, 10.0), 10.0, (10.0, 16.0)),
        ("M1", 0.0, 22.0, None, 50.0, 2, 3, (10.0, 10.0), 10.0, (10.0, 16.0)),
        ("M2", 22.0, 28.0, None, 63.0, 2, 3, (10.0, 10.0), 10.0, (10.0, 16.0)),
        ("M3", 28.0, 37.0, None, 90.0, 2, 3, (16.0, 25.0), 16.0, (16.0, 16.0)),
        ("M4", 37.0, 50.0, None, 100.0, 2, 3, (16.0, 25.0), 25.0, (16.0, 25.0)),
        ("B1", 13.0, 20.0, None, 50.0, 2, 3, (10.0, 10.0), 10.0, (10.0, 16.0)),
        ("B2", 20.0, 25.0, None, 63.0, 2, 3, (10.0, 10.0), 10.0, (10.0, 16.0)),
        ("C1", 25.0, 30.0, Some((0.0, 30.0)), 50.0, 3, 4, (10.0, 10.0), 10.0, (10.0, 16.0)),
        ("C2", 30.0, 42.0, Some((30.0, 40.0)), 63.0, 3, 4, (10.0, 25.0), 16.0, (16.0, 16.0)),
        ("C3", 42.0, 50.0, Some((40.0, 45.0)), 80.0, 3, 4, (16.0, 25.0), 16.0, (16.0, 16.0)),
        ("C4", 50.0, 65.0, Some((45.0, 60.0)), 100.0, 3, 4, (25.0, 35.0), 25.0, (16.0, 25.0)),
        ("C5", 65.0, 75.0, Some((60.0, 75.0)), 125.0, 3, 4, (35.0, 50.0), 50.0, (16.0, 35.0)),
    ];
    for (name, lo, hi, demand, breaker, phases, wires, connection, entry, earth) in printed {
        let r = celesc_row(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!((r.lo, r.hi, r.demand, r.breaker), (lo, hi, demand, breaker), "{name}");
        assert_eq!((r.phases, r.wires, r.connection, r.entry, r.earth), (phases, wires, connection, entry, earth), "{name}");
        // Each row is picked at the top of its range, on its network.
        let network = if name.starts_with('M') { "220/440" } else { "220/380" };
        assert_eq!(celesc_pick(network, hi, Some(phases)).map(|r| r.name), Some(name), "{name} at {hi} kW");
    }
}

#[test]
fn celesc_frames_a_unit_by_its_installed_load() {
    let name = |network: &str, kw: f64, phases: Option<u8>| celesc_pick(network, kw, phases).map(|r| r.name);
    // On 220/380 V: single-phase up to 13 kW, two-phase up to 25 kW, three-phase up to 75 kW (items 5.3.2.1 to 5.3.2.3).
    assert_eq!(name("220/380", 4.0, None), Some("A1"));
    assert_eq!(name("220/380", 12.0, None), Some("A2"));
    assert_eq!(name("220/380", 13.5, None), Some("B1"));
    assert_eq!(name("220/380", 26.0, None), Some("C1"));
    assert_eq!(name("220/380", 75.0, None), Some("C5"));
    assert_eq!(name("220/380", 75.5, None), None);
    // A three-phase appliance asks for three phases whatever the load: the first three-phase row.
    assert_eq!(name("220/380", 8.0, Some(3)), Some("C1"));
    assert_eq!(name("220/380", 8.0, Some(2)), Some("B1"));
    // On 220/440 V a single-phase unit of three wires goes up to 50 kW; below 13 kW two wires at 220 V come first.
    assert_eq!(name("220/440", 8.0, None), Some("A1"));
    assert_eq!(name("220/440", 18.0, None), Some("M1"));
    assert_eq!(name("220/440", 8.0, Some(2)), Some("M1"));
    assert_eq!(name("220/440", 50.5, None), None);
}

mod celesc_command {
    use crate::engine::Session;
    use serde_json::{Value, json};

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("demand", &p).unwrap_or_else(|e| panic!("demand {p}: {e}"))
    }

    /// An apartment whose electrical design installs 22.4 kW, two-phase at 220/380 V, and works out a demand of 19.24 kVA: by its
    /// installed load Tabela 01 makes it B2, a 63 A breaker, whatever the demand the design took.
    #[test]
    fn an_apartment_is_framed_by_its_installed_load_and_its_demand_is_carried() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"utility": "CELESC", "installed_kw": 22.426, "demand_kva": 19.24}));
        assert_eq!(r["table"], "01");
        assert_eq!(r["network"], "220/380");
        assert_eq!(r["category"]["name"], "B2");
        assert_eq!(r["category"]["breaker_a"], 63.0);
        assert_eq!((&r["category"]["phases"], &r["category"]["wires"]), (&json!(2), &json!(3)));
        assert_eq!(r["category"]["connection_mm2"], json!({"copper": 10.0, "aluminium": 10.0}));
        assert_eq!(r["demand_kva"], 19.24);
        assert!(r["source"].as_str().is_some_and(|t| t.contains("N-321.0001")), "{r}");
        assert!(r["demand_note"].as_str().is_some_and(|t| t.contains("designer")), "{r}");
    }

    #[test]
    fn a_three_phase_unit_says_the_demand_printed_beside_it() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"utility": "CELESC", "installed_kw": 45, "demand_kva": 47}));
        assert_eq!(r["category"]["name"], "C3");
        assert_eq!(r["category"]["demand_kva"], json!({"over": 40.0, "up_to": 45.0}));
        assert!(r["category_note"].as_str().is_some_and(|n| n.contains("47")), "{r}");
        let r = run(&mut s, json!({"utility": "CELESC", "installed_kw": 70}));
        assert!(r["category_note"].as_str().is_some_and(|n| n.contains("note 1")), "{r}");
        let r = run(&mut s, json!({"utility": "CELESC", "installed_kw": 90}));
        assert_eq!(r["category"], Value::Null);
        assert!(r["category_note"].as_str().is_some_and(|n| n.contains("5.2.2.8")), "{r}");
    }

    #[test]
    fn what_celesc_does_not_take_is_refused() {
        let mut s = Session::new();
        for p in [
            json!({"utility": "CELESC"}),
            json!({"utility": "CELESC", "installed_kw": -1}),
            json!({"utility": "CELESC", "installed_kw": 1e300}),
            json!({"utility": "CELESC", "installed_kw": 5, "network": "127/220"}),
            json!({"utility": "CELESC", "installed_kw": 5, "network": "220/440", "phases": 3}),
            json!({"utility": "CELESC", "installed_kw": 5, "phases": 4}),
            json!({"utility": "CELESC", "installed_kw": 5, "area": 40}),
            json!({"utility": "CELESC", "installed_kw": 5, "demand_current_a": 40}),
        ] {
            assert!(s.execute("demand", &p).is_err(), "{p}");
        }
    }
}
