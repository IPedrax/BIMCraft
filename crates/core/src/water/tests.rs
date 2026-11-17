//! The design flow, the head loss and the PVC tables.

use super::*;

fn near(a: f64, b: f64, e: f64) -> bool {
    (a - b).abs() <= e
}

#[test]
fn the_design_flow_follows_the_method() {
    let f = [("lav", 1.0), ("wc", 1.0), ("shower", 1.0)];
    assert!(near(design_flow(Method::Weights, &f), 0.3, 1e-9), "0.3 sqrt(1.0)");
    assert!(near(design_flow(Method::Sum, &f), 0.5, 1e-9));
    assert!(near(design_flow(Method::Weights, &[("lav", 1.0)]), 0.15, 1e-9), "never more than the fixtures' own");
    assert!(near(design_flow(Method::Weights, &[("sink", 2.0)]), 0.3 * 1.4f64.sqrt(), 1e-9));
    assert_eq!(design_flow(Method::Weights, &[]), 0.0);
}

#[test]
fn the_head_loss_is_darcy_weisbach() {
    let (v, j) = head_loss(0.3, 21.6, library().roughness);
    assert!(near(v, 0.3e-3 / (std::f64::consts::PI * 0.0216 * 0.0216 / 4.0), 1e-9));
    let re = v * 0.0216 / NU;
    assert!(re > 17000.0 && re < 18000.0);
    let f = colebrook(re, library().roughness / 21.6);
    assert!(f > 0.026 && f < 0.028, "{f}");
    assert!(near(j, f / 0.0216 * v * v / (2.0 * G), 1e-12));
    assert_eq!(head_loss(0.0, 21.6, library().roughness), (0.0, 0.0));
    assert!(near(colebrook(1000.0, 1e-4), 0.064, 1e-12), "laminar: 64 / Re");
}

#[test]
fn the_pvc_tables_are_as_printed() {
    assert_eq!(inside(25.0), Some(21.6));
    assert!(inside(110.0).is_some_and(|d| near(d, 97.8, 1e-9)));
    assert_eq!(inside(30.0), None);
    assert_eq!(equivalent_length(25.0, "elbow90"), Some(1.2));
    assert_eq!(equivalent_length(110.0, "check"), Some(10.4));
    assert_eq!(equivalent_length(25.0, "tap"), None);
    assert_eq!(equivalent_length(30.0, "gate"), None);
    for f in &library().fittings {
        assert!(library().pipes.iter().all(|p| equivalent_length(p[0], f).is_some()), "{f}");
    }
}

#[test]
fn the_smallest_dn_keeps_the_speed_within_three_metres_per_second() {
    assert_eq!(smallest_dn(0.15), Some(20.0), "0.15 L/s at 17 mm inside: 0.66 m/s");
    let dn = smallest_dn(1.5).unwrap_or(0.0);
    let d = inside(dn).unwrap_or(0.0);
    assert!(head_loss(1.5, d, library().roughness).0 <= library().max_speed);
    let smaller = library().dns().into_iter().rfind(|x| *x < dn).and_then(inside).unwrap_or(0.0);
    assert!(head_loss(1.5, smaller, library().roughness).0 > library().max_speed, "the one below is too fast");
    assert_eq!(smallest_dn(100.0), None, "no DN of the table carries it");
}

mod command {
    use serde_json::{Value, json};

    use crate::engine::Session;

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("water_size", &p).unwrap_or_else(|e| panic!("water_size {p}: {e}"))
    }

    #[test]
    fn a_bathroom_group() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"fixtures": {"lav": 1, "wc": 1, "shower": 1}, "length": 10000}));
        assert_eq!(r["flow_lps"], json!(0.3));
        assert_eq!(r["dn"], json!(20.0), "17 mm inside: 1.3 m/s");
        assert_eq!(r["ok"], json!(true));
        let r = run(
            &mut s,
            json!({"flow": 0.3, "dn": 25, "length": 10000, "fittings": {"elbow90": 2, "tee_side": 1}, "pressure": 100, "rise": 2000}),
        );
        assert_eq!(r["equivalent_length_m"], json!(14.8), "10 m + 2 x 1.2 + 2.4");
        let loss = r["loss_kpa"].as_f64().unwrap_or(f64::NAN);
        let end = r["end_kpa"].as_f64().unwrap_or(f64::NAN);
        assert!((end - (100.0 - loss - 2.0 * 9.81)).abs() < 0.1, "{end} {loss}");
        assert_eq!(r["ok"], json!(end >= 10.0));
        let r = run(&mut s, json!({"flow": 1.5, "dn": 20}));
        assert_eq!(r["ok"], json!(false), "too fast in DN 20");
        assert_eq!(crate::engine::find("water_size").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn a_bad_parameter_says_which() {
        let mut s = Session::new();
        for (bad, says) in [
            (json!({}), "flow"),
            (json!({"flow": 0.3, "fixtures": {"wc": 1}}), "fixtures"),
            (json!({"fixtures": {"bidet": 1}}), "bidet"),
            (json!({"fixtures": {"wc": 1.5}}), "whole"),
            (json!({"fixtures": 3}), "object"),
            (json!({"flow": 0.3, "method": "magic"}), "method"),
            (json!({"flow": 0.3, "dn": 30}), "dn"),
            (json!({"flow": 100}), "DN"),
            (json!({"flow": 0.3, "fittings": {"tap": 1}, "dn": 25}), "tap"),
            (json!({"flow": 0.3, "dn": 25, "fittings": {"elbow90": -1}}), "elbow90"),
            (json!({"flow": 0.3, "length": -1}), "length"),
            (json!("x"), "object"),
        ] {
            let e = s.execute("water_size", &bad).expect_err("should fail");
            assert!(e.contains(says), "{bad}: {e}");
        }
    }
}
