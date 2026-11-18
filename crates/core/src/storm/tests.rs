//! Manning against the standard's Table 4, and the abacuses of its Figure 3 as read.

use super::*;

fn lib() -> std::sync::Arc<StormRules> {
    crate::codes::library_rules::<StormRules>()
}

fn near(a: f64, b: f64, e: f64) -> bool {
    (a - b).abs() <= e
}

#[test]
fn horizontal_drains_agree_with_tabela_4_within_one_per_cent() {
    // NBR 10844 Table 4: flow in L/min at two thirds full, n = 0.011, slopes 0.5, 1, 2 and 4 %
    let table: &[(f64, [f64; 4])] = &[
        (50.0, [32.0, 45.0, 64.0, 90.0]),
        (75.0, [95.0, 133.0, 188.0, 267.0]),
        (100.0, [204.0, 287.0, 405.0, 575.0]),
        (125.0, [370.0, 521.0, 735.0, 1040.0]),
        (150.0, [602.0, 847.0, 1190.0, 1690.0]),
        (200.0, [1300.0, 1820.0, 2570.0, 3650.0]),
        (250.0, [2350.0, 3310.0, 4660.0, 6620.0]),
        (300.0, [3820.0, 5380.0, 7590.0, 10800.0]),
    ];
    let mut worst: f64 = 0.0;
    for (d, row) in table {
        for (v, s) in row.iter().zip([0.005, 0.01, 0.02, 0.04]) {
            worst = worst.max((manning_flow(*d, s, lib().n("pvc").unwrap()) / v - 1.0).abs());
        }
    }
    assert!(worst < 0.01, "worst {:.2} % (the table rounds)", worst * 100.0);
    assert_eq!(manning_flow(100.0, 0.0, lib().n("pvc").unwrap()), 0.0, "no slope, no flow");
    assert_eq!(manning_flow(0.0, 0.01, lib().n("pvc").unwrap()), 0.0);
    assert_eq!(manning_flow(100.0, 0.01, 0.0), 0.0);
    assert_eq!(lib().min_slope, 0.005);
}

#[test]
fn the_design_flow_is_the_rain_on_the_area() {
    assert!(near(design_flow(150.0, 100.0), 250.0, 1e-9));
    assert_eq!(design_flow(0.0, 100.0), 0.0);
    assert_eq!(design_flow(150.0, 0.0), 0.0);
}

#[test]
fn the_horizontal_diameter_inverts_manning() {
    for (q, s) in [(250.0, 0.01), (1000.0, 0.005), (3000.0, 0.02)] {
        let d = horizontal_diameter(q, s, lib().n("pvc").unwrap());
        assert!(near(manning_flow(d, s, lib().n("pvc").unwrap()), q, q * 1e-9), "{q} L/min at {s}: {d} mm");
    }
    assert!(horizontal_diameter(100.0, 0.0, lib().n("pvc").unwrap()).is_nan());
}

fn pipe(outlet: Outlet, q: f64, h: f64, l: f64) -> Result<Downpipe, Off> {
    downpipe_diameter(outlet, q, h, l)
}

#[test]
fn the_abacuses_are_as_read() {
    // the reading of Figure 3 as stored (D in mm at Q in L/min), so it does not change by accident
    for (outlet, h, q, d) in [
        (Outlet::Edge, 50.0, 600.0, 99.1),
        (Outlet::Edge, 70.0, 1000.0, 89.0),
        (Outlet::Edge, 100.0, 2400.0, 125.4),
        (Outlet::Funnel, 50.0, 800.0, 117.2),
        (Outlet::Funnel, 90.0, 2000.0, 98.6),
    ] {
        let r = pipe(outlet, q, h, 1.0e6).map(|r| r.by_depth).unwrap_or(f64::NAN);
        assert!(near(r, d, 0.05), "{outlet:?} H {h} at {q}: {r}");
    }
}

#[test]
fn a_longer_downpipe_never_needs_more() {
    let d = |l: f64| pipe(Outlet::Edge, 1000.0, 70.0, l).map(|r| r.diameter).unwrap_or(f64::NAN);
    assert!(d(3.0) >= d(6.0) && d(6.0) >= d(25.0) && d(25.0) >= d(1.0e6));
    let l = |l: f64| pipe(Outlet::Edge, 1000.0, 70.0, l).map(|r| r.by_length).unwrap_or(f64::NAN);
    assert!(l(25.0) >= l(f64::INFINITY));
}

#[test]
fn between_curves_it_interpolates_linearly() {
    let dh = |q: f64, h: f64| pipe(Outlet::Edge, q, h, 4.0).map(|r| r.by_depth).unwrap_or(f64::NAN);
    assert!(near(dh(1200.0, 75.0), (dh(1200.0, 70.0) + dh(1200.0, 80.0)) / 2.0, 1e-9));
    let dl = |l: f64| pipe(Outlet::Edge, 1500.0, 90.0, l).map(|r| r.by_length).unwrap_or(f64::NAN);
    assert!(near(dl(4.5), (dl(3.0) + dl(6.0)) / 2.0, 1e-9), "linear in L");
    assert!(near(dl(50.0), (dl(25.0) + dl(f64::INFINITY)) / 2.0, 1e-9), "between 25 m and infinity: linear in 1/L, 50 m halfway");
}

#[test]
fn the_higher_of_the_two_readings_and_never_under_70_mm() {
    let r = pipe(Outlet::Edge, 1000.0, 70.0, 3.0).ok();
    assert!(r.is_some_and(|r| r.diameter == r.by_depth.max(r.by_length)));
    let small = pipe(Outlet::Edge, 300.0, 50.0, 1.0).ok();
    assert!(small.is_some_and(|r| r.diameter == 70.0 && r.by_length < 70.0), "at least 70 mm inside");
    assert!(pipe(Outlet::Edge, 0.0, 50.0, 1.0).is_ok_and(|r| r.diameter == 70.0), "no flow: the smallest");
}

#[test]
fn off_the_chart_nothing_is_read() {
    assert_eq!(pipe(Outlet::Edge, 1000.0, 40.0, 3.0), Err(Off::Depth));
    assert_eq!(pipe(Outlet::Edge, 1000.0, 110.0, 3.0), Err(Off::Depth));
    assert_eq!(pipe(Outlet::Edge, 1000.0, 70.0, 0.2), Err(Off::Length));
    assert_eq!(pipe(Outlet::Edge, 2700.0, 50.0, 3.0), Err(Off::Diameter));
    assert_eq!(pipe(Outlet::Edge, 1000.0, f64::NAN, 3.0), Err(Off::Depth));
    // between a shorter curve that is off the chart and the next that is not: an error, never the H curve alone
    assert_eq!(pipe(Outlet::Edge, 2400.0, 100.0, 0.45), Err(Off::Diameter));
    assert!(pipe(Outlet::Edge, 2400.0, 100.0, 0.6).is_ok_and(|d| near(d.diameter, 145.9, 0.1)), "the next curve is on the chart");
}

#[test]
fn more_flow_never_needs_less() {
    for outlet in [Outlet::Edge, Outlet::Funnel] {
        for h in [50.0, 70.0, 100.0] {
            let mut q = 200.0;
            while q <= 2600.0 {
                if let (Ok(a), Ok(b)) = (pipe(outlet, q, h, 2.0), pipe(outlet, q + 100.0, h, 2.0)) {
                    assert!(b.diameter + 1e-9 >= a.diameter, "{outlet:?} H {h} at {q}");
                }
                q += 100.0;
            }
        }
    }
}

mod command {
    use serde_json::{Value, json};

    use crate::engine::Session;

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("storm_size", &p).unwrap_or_else(|e| panic!("storm_size {p}: {e}"))
    }

    #[test]
    fn a_downpipe_and_a_horizontal_drain() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"kind": "downpipe", "area": 100, "gutter": 50, "length": 6300}));
        assert_eq!(r["flow_lpm"], json!(250.0), "150 mm/h on 100 m2");
        assert_eq!(r["inside_mm"], json!(70.0), "under 70 mm on the chart: 70");
        let r = run(&mut s, json!({"kind": "downpipe", "area": 300, "gutter": 70, "length": 6350, "outlet": "funnel", "rainfall": 150}));
        assert_eq!(r["flow_lpm"], json!(750.0));
        assert!(r["inside_mm"].as_f64().is_some_and(|d| (70.0..100.0).contains(&d)));
        let r = run(&mut s, json!({"kind": "horizontal", "flow": 1000, "slope": 0.01}));
        assert!(r["inside_mm"].as_f64().is_some_and(|d| d > 130.0 && d < 165.0));
        let r = run(&mut s, json!({"kind": "horizontal", "flow": 1000, "slope": 0.01, "inside": 144.8}));
        assert_eq!(r["ok"], json!(false), "DN 150 (144.8 mm inside) carries about 770 L/min at 1 %: short of 1000");
        assert_eq!(crate::engine::find("storm_size").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn a_bad_parameter_says_which() {
        let mut s = Session::new();
        for (bad, says) in [
            (json!({}), "kind"),
            (json!({"kind": "gutter"}), "kind"),
            (json!({"kind": "downpipe", "area": 100}), "gutter"),
            (json!({"kind": "downpipe", "area": 100, "gutter": 70}), "length"),
            (json!({"kind": "downpipe", "gutter": 70, "length": 3000}), "area"),
            (json!({"kind": "downpipe", "area": 100, "gutter": 40, "length": 3000}), "50"),
            (json!({"kind": "downpipe", "area": 100, "gutter": 70, "length": 100}), "0.3"),
            (json!({"kind": "downpipe", "area": 9000, "rainfall": 150, "gutter": 50, "length": 3000}), "150 mm inside"),
            (json!({"kind": "downpipe", "area": 300, "gutter": 70, "length": 3000}), "rainfall"),
            (json!({"kind": "downpipe", "area": 100, "flow": 500, "gutter": 70, "length": 3000}), "two ways"),
            (json!({"kind": "downpipe", "area": 100, "gutter": 70, "length": 3000, "slope": 0.01}), "does not apply"),
            (json!({"kind": "horizontal", "area": 100, "slope": 0.01, "gutter": 70}), "does not apply"),
            (json!({"kind": "horizontal", "area": 100}), "slope"),
            (json!({"kind": "horizontal", "area": 100, "slope": 0.001}), "0.5"),
            (json!({"kind": "horizontal", "area": 100, "slope": 0.01, "outlet": "tap"}), "does not apply"),
            (json!({"kind": "horizontal", "area": 100, "slope": 0.01, "pipe": 1}), "pipe"),
            (json!("x"), "object"),
        ] {
            let e = s.execute("storm_size", &bad).expect_err("should fail");
            assert!(e.contains(says), "{bad}: {e}");
        }
    }
}
