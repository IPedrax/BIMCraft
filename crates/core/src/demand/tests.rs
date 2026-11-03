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
        for u in ["ENEL", "Light", "xx"] {
            let e = s.execute("demand", &json!({"utility": u, "area": 40})).unwrap_err();
            assert!(e.contains("CPFL") && e.contains(u), "{e}");
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
