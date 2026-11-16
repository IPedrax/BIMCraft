//! IT 22's Tabelas 1, 2 and 3 as printed, and the pipe checks by hand.

use super::*;

fn near(a: f64, b: f64, e: f64) -> bool {
    (a - b).abs() <= e
}

#[test]
fn tabela_2_is_as_printed() {
    let lib = crate::codes::library_rules::<HydrantRules>();
    let t = |k: &str| lib.kind(k).map(|t| (t.kind, t.nozzle_dn, t.jet_flow, t.nozzle_head));
    assert_eq!(t("1"), Some((1, 25.0, 100.0, 80.0)));
    assert_eq!(t("2"), Some((2, 40.0, 150.0, 30.0)));
    assert_eq!(t("3"), Some((3, 40.0, 200.0, 40.0)));
    assert_eq!(t("4-40"), Some((4, 40.0, 300.0, 65.0)));
    assert_eq!(t("4-65"), Some((4, 65.0, 300.0, 30.0)));
    assert_eq!(t("5"), Some((5, 65.0, 600.0, 60.0)));
    let keys: Vec<&str> = lib.types.iter().map(|t| t.key.as_str()).collect();
    assert_eq!(keys, ["1", "2", "3", "4-40", "4-65", "5"]);
    assert!(lib.kind("6").is_none());
}

#[test]
fn tabela_3_is_as_printed() {
    // (column, area, type, reserve m3)
    for (c, a, ty, r) in [
        (1, 100.0, 1, 5.0),
        (1, 2500.0, 1, 5.0),
        (1, 2500.5, 1, 8.0),
        (1, 60000.0, 1, 35.0),
        (2, 10000.0, 2, 18.0),
        (3, 20000.0, 3, 35.0),
        (3, 50001.0, 3, 70.0),
        (4, 5000.0, 4, 32.0),
        (4, 50000.0, 4, 96.0),
        (5, 5000.0, 4, 48.0),
        (5, 5001.0, 5, 64.0),
        (5, 60000.0, 5, 180.0),
    ] {
        assert_eq!(system_for(c, a), Some((ty, r)), "column {c}, {a} m2");
    }
    assert_eq!(system_for(0, 100.0), None);
    assert_eq!(system_for(6, 100.0), None);
    assert_eq!(system_for(1, 0.0), None);
    assert_eq!(system_for(1, f64::NAN), None);
}

#[test]
fn the_reserve_never_falls_with_the_area() {
    for c in 1..=5u8 {
        let mut last = 0.0;
        for a in [100.0, 2500.0, 5000.0, 10000.0, 20000.0, 50000.0, 90000.0] {
            let r = system_for(c, a).map_or(f64::NAN, |x| x.1);
            assert!(r >= last, "column {c} at {a}");
            last = r;
        }
    }
}

#[test]
fn hazen_williams_by_hand() {
    // 600 L/min, C 120, 68 mm inside: 605 * 600^1.85 * 120^-1.85 * 68^-4.87 * 1e4
    assert!(near(hazen_j(600.0, 120.0, 68.0), 0.14143, 1e-4));
    assert!(near(hazen_j(300.0, 150.0, 53.0), 0.08739, 1e-4));
    assert!(near(speed(600.0, 68.0), 2.7535, 1e-3));
    assert!(hazen_j(600.0, 120.0, 100.0) < hazen_j(600.0, 120.0, 68.0), "a larger pipe loses less");
    assert_eq!(hazen_c("steel"), Some(100.0), "black steel in a dry system: the lowest steel row");
    assert_eq!(hazen_c("steel-wet"), Some(120.0));
    assert_eq!(hazen_c("copper"), Some(150.0));
    assert_eq!(hazen_c("lead"), None);
}

#[test]
fn two_jets_at_most() {
    assert_eq!(design_flow("5", 1), 600.0);
    assert_eq!(design_flow("5", 2), 1200.0);
    assert_eq!(design_flow("5", 7), 1200.0);
    assert_eq!(design_flow("2", 2), 300.0);
    assert_eq!(design_flow("2", 0), 0.0);
    assert_eq!(design_flow("9", 2), 0.0);
    assert_eq!(min_nominal("1"), 50.0);
    assert_eq!(min_nominal("2"), 50.0);
    assert_eq!(min_nominal("3"), 65.0);
    assert_eq!(min_nominal("5"), 65.0);
}

mod command {
    use serde_json::{Value, json};

    use crate::engine::Session;

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("hydrant_size", &p).unwrap_or_else(|e| panic!("hydrant_size {p}: {e}"))
    }

    #[test]
    fn a_system_and_a_pipe() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"column": 3, "area": 6000}));
        assert_eq!(r["system"], json!("3"));
        assert_eq!(r["reserve_m3"], json!(25.0));
        let r = run(&mut s, json!({"column": 5, "area": 8000}));
        assert_eq!(r["system"], json!("5"), "type 5 above 5000 m2");
        let r = run(&mut s, json!({"type": "3", "hydrants": 3, "inside": 68, "nominal": 65, "length": 20000, "rise": 4000}));
        assert_eq!(r["flow_lpm"], json!(400.0), "two jets of 200 L/min");
        assert_eq!(r["nozzle_head_mca"], json!(40.0));
        let loss = r["loss_mca"].as_f64().unwrap_or(f64::NAN);
        assert!((r["need_mca"].as_f64().unwrap_or(f64::NAN) - (40.0 + loss + 4.0)).abs() < 0.02);
        assert_eq!(r["speed_ok"], json!(true));
        assert_eq!(r["size_ok"], json!(true));
        assert_eq!(r["ok"], json!(true));
        assert!(r["need_covers"].as_str().is_some_and(|t| t.contains("not the hose")));
        let r = run(&mut s, json!({"column": 3, "area": 6000, "type": "3", "region": "SP"}));
        assert_eq!(r["type"], json!("3"), "a type that agrees is fine");
        let r = run(&mut s, json!({"type": "5", "inside": 40, "nominal": 40, "material": "copper"}));
        assert_eq!(r["speed_ok"], json!(false), "1200 L/min in 40 mm");
        assert_eq!(r["size_ok"], json!(false), "DN 40 is under DN 65");
        assert_eq!(r["ok"], json!(false));
        assert_eq!(crate::engine::find("hydrant_size").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn a_bad_parameter_says_which() {
        let mut s = Session::new();
        for (bad, says) in [
            (json!({}), "type"),
            (json!({"column": 6, "area": 100}), "column"),
            (json!({"column": 1}), "area"),
            (json!({"area": 100}), "column"),
            (json!({"column": 4, "area": 100, "inside": 68}), "4-40"),
            (json!({"type": "9"}), "type"),
            (json!({"type": "3", "hydrants": 0}), "hydrants"),
            (json!({"type": "3", "hydrants": 1.5}), "hydrants"),
            (json!({"type": "3", "inside": 0}), "inside"),
            (json!({"type": "3", "inside": 68, "material": "lead"}), "material"),
            (json!({"type": "3", "inside": 68, "length": -1}), "length"),
            (json!({"type": "3", "nominal": 65}), "inside"),
            (json!({"column": 3, "area": 6000, "type": "5"}), "contradicts"),
            (json!({"type": "3", "region": "RJ"}), "SP"),
            (json!({"type": "3", "length": 1.0e12, "inside": 68}), "length"),
            (json!("x"), "object"),
        ] {
            let e = s.execute("hydrant_size", &bad).expect_err("should fail");
            assert!(e.contains(says), "{bad}: {e}");
        }
    }
}
