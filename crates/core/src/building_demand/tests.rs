//! A building's demand by each method of `data/building-demand.json`, against the standards' own examples and worked by hand, and
//! the data itself: every table sourced, in order, and the tables the standards share alike.

use super::*;

fn method_of(name: &str) -> &'static Method {
    method(name).unwrap_or_else(|| panic!("no method {name}"))
}

fn alike(count: u32, area: f64) -> Group {
    Group { count, area: Some(area), ..Group::default() }
}

fn declared(count: u32, demand: f64) -> Group {
    Group { count, demand: Some(demand), ..Group::default() }
}

fn graded(count: u32, grade: &str) -> Group {
    Group { count, grade: Some(grade.into()), ..Group::default() }
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn the_file_reads_and_has_each_method() {
    let names: Vec<&str> = methods().iter().map(|m| m.standard.as_str()).collect();
    for n in ["CELESC N-321.0003", "CEMIG ND-5.2", "FECOERGS REGD 035.01.06", "HIDROPAN RIC BT 2014", "ITC-BT-10"] {
        assert!(names.contains(&n), "{n} missing from {names:?}");
    }
    assert_eq!(method("celesc").map(|m| m.standard.as_str()), Some("CELESC N-321.0003"), "a utility, in any case");
    assert!(method("COPEL").is_none());
}

#[test]
fn celesc_s_own_example_takes_the_weighted_mean_area_up_to_the_next_row() {
    // N-321.0003 item 5.6.2, note 2: 20 apartments of 100 m² and 16 of 80 m² are 36 apartments of 91.1 m², taken as 92 m².
    let m = method_of("CELESC N-321.0003");
    let o = compute(m, &Input { dwellings: vec![alike(20, 100.0), alike(16, 80.0)], ..Input::default() }).unwrap();
    assert_eq!(o.dwellings, 36);
    assert!(o.mean_area.is_some_and(|a| near(a, 91.11, 0.01)), "{o:?}");
    assert_eq!((o.per_dwelling, o.diversity), (2.0, 27.1), "Tabela 02 at 92 m², Tabela 01 at 36");
    assert!(near(o.terms.dwellings, 1.2 * 27.1 * 2.0, 1e-9), "k = 1.2: {o:?}");
}

#[test]
fn celesc_s_common_areas_go_by_bands_and_k_takes_them_too() {
    // Lighting 100% of the first 10 kW and 25% above, outlets 20%, both at a power factor of 0.90; k on D1 + D2, not on De.
    let m = method_of("CELESC N-321.0003");
    let i = Input {
        dwellings: vec![alike(20, 100.0), alike(16, 80.0)],
        common: vec![("lighting".into(), 14.0), ("outlets".into(), 5.0)],
        special: 7.0,
        ..Input::default()
    };
    let o = compute(m, &i).unwrap();
    assert!(near(o.terms.common, 1.2 * (11.0 + 1.0) / 0.9, 1e-9), "{o:?}");
    assert_eq!(o.terms.special, 7.0, "a charging station counts whole, outside k");
    assert!(near(o.demand, 65.04 + 16.0 + 7.0, 1e-9), "{o:?}");
    let e = o.entrance.unwrap();
    assert_eq!((e.over, e.up_to, e.breaker_a), (78.0, 95.0, 150.0), "Tabela 11");
    assert_eq!(e.columns.get("exposed entry, copper, mm²"), Some(&Some("50".to_string())));
}

#[test]
fn celesc_above_400_m2_takes_its_curve() {
    let m = method_of("CELESC N-321.0003");
    assert_eq!(m.by_area(400.0), Some(7.45));
    assert!(m.by_area(500.0).is_some_and(|d| near(d, 0.034939 * 500f64.powf(0.895075), 1e-9)));
    assert_eq!(m.by_area(30.0), Some(1.57), "1.57 kVA under 70 m²");
}

#[test]
fn an_erratum_is_taken_unless_the_printed_value_is_asked_for() {
    let m = method_of("CELESC N-321.0003");
    let (f, e) = m.diversity(203, false).unwrap();
    assert_eq!(f, 80.99);
    assert!(e.is_some_and(|e| e.printed == 80.89 && e.why.contains("Cemig")));
    assert_eq!(m.diversity(203, true).map(|d| d.0), Some(80.89));
    let o = compute(m, &Input { dwellings: vec![alike(203, 80.0)], ..Input::default() }).unwrap();
    assert_eq!(o.errata.len(), 1, "the reply says which erratum it took");
    let o = compute(m, &Input { dwellings: vec![alike(203, 80.0)], as_printed: true, ..Input::default() }).unwrap();
    assert!(o.errata.is_empty());
    assert!(m.diversity(1, false).is_none(), "Tabela 01 prints none for one apartment");
}

#[test]
fn cemig_s_example_gives_54_5_kva() {
    // ND-5.2 Anexo A: 1.4 × 19.86 × 1.96 = 54.50 kVA, 24 apartments of 86 to 90 m².
    let m = method_of("CEMIG ND-5.2");
    let o = compute(m, &Input { dwellings: vec![alike(24, 88.0)], ..Input::default() }).unwrap();
    assert_eq!((o.diversity, o.per_dwelling), (19.86, 1.96));
    assert!(near(o.demand, 54.50, 0.005), "{o:?}");
    assert_eq!(m.diversity(51, false).map(|d| d.0), Some(35.9), "printed 31.90");
    assert!(compute(m, &Input { dwellings: vec![alike(3, 88.0)], ..Input::default() }).is_err(), "Tabela 12 starts at 4");
    assert!(compute(m, &Input { dwellings: vec![alike(301, 88.0)], ..Input::default() }).is_err(), "and ends at 300");
    assert!(compute(m, &Input { dwellings: vec![alike(10, 1200.0)], ..Input::default() }).is_err(), "Tabela 13 ends at 1000 m²");
}

#[test]
fn fecoergs_grows_the_units_only() {
    // REGD 035.01.06 item 7.2.2.1: Anexo T × Anexo U × 1.2, plus the condominium.
    let m = method_of("FECOERGS REGD 035.01.06");
    let i = Input { dwellings: vec![alike(4, 120.0)], common_demand: 5.0, ..Input::default() };
    let o = compute(m, &i).unwrap();
    assert_eq!((o.per_dwelling, o.diversity), (2.54, 9.64), "Anexo U prints 'up to 10' = 9.64");
    assert!(near(o.demand, 1.2 * 9.64 * 2.54 + 5.0, 1e-9), "{o:?}");
    assert!(o.entrance.is_none(), "no entrance table by demand");
}

#[test]
fn spain_s_itc_bt_10_takes_the_mean_of_declared_powers() {
    let m = method_of("ITC-BT-10");
    assert_eq!(m.unit, "kW");
    // Tabla 1 up to 21, then 15.3 + (n − 21)·0.5.
    assert_eq!(m.diversity(21, false).map(|d| d.0), Some(15.3));
    assert_eq!(m.diversity(30, false).map(|d| d.0), Some(19.8));
    let o = compute(m, &Input { dwellings: vec![graded(30, "basic")], ..Input::default() }).unwrap();
    assert!(near(o.demand, 19.8 * 5.75, 1e-9), "{o:?}");
    let o = compute(m, &Input { dwellings: vec![graded(10, "basic"), graded(10, "elevated")], ..Input::default() }).unwrap();
    assert!(near(o.per_dwelling, (5.75 + 9.2) / 2.0, 1e-9) && near(o.demand, 14.8 * 7.475, 1e-9), "{o:?}");
    // Under 5 750 W a dwelling is raised to it, and the reply says how many were.
    let o = compute(m, &Input { dwellings: vec![declared(10, 4.0)], ..Input::default() }).unwrap();
    assert_eq!((o.per_dwelling, o.raised), (5.75, 10));
}

#[test]
fn itc_bt_10_s_services_shops_and_garages_count_whole() {
    let m = method_of("ITC-BT-10");
    let i = Input {
        dwellings: vec![graded(30, "basic")],
        common_demand: 15.0,
        premises: vec![("commercial".into(), 20.0), ("garage_forced".into(), 500.0)],
        ..Input::default()
    };
    let o = compute(m, &i).unwrap();
    assert!(near(o.terms.premises, 3.45 + 10.0, 1e-9), "a shop of 20 m² takes the least 3 450 W: {o:?}");
    assert!(near(o.demand, 113.85 + 15.0 + 13.45, 1e-9), "{o:?}");
}

#[test]
fn what_a_method_does_not_take_is_refused_with_what_it_does() {
    let celesc = method_of("CELESC N-321.0003");
    let itc = method_of("ITC-BT-10");
    let cemig = method_of("CEMIG ND-5.2");
    let err = |m, i: Input| compute(m, &i).unwrap_err();
    assert!(err(celesc, Input { dwellings: vec![declared(10, 5.0)], ..Input::default() }).contains("area"));
    assert!(err(itc, Input { dwellings: vec![alike(10, 80.0)], ..Input::default() }).contains("demand"));
    assert!(err(itc, Input { dwellings: vec![graded(10, "luxury")], ..Input::default() }).contains("basic, elevated"));
    let e = err(cemig, Input { dwellings: vec![alike(10, 80.0)], common: vec![("lighting".into(), 3.0)], ..Input::default() });
    assert!(e.contains("common_demand"), "{e}");
    let e = err(celesc, Input { dwellings: vec![alike(10, 80.0)], common: vec![("lifts".into(), 3.0)], ..Input::default() });
    assert!(e.contains("lighting, outlets"), "{e}");
    let e = err(celesc, Input { dwellings: vec![alike(10, 80.0)], premises: vec![("garage_forced".into(), 3.0)], ..Input::default() });
    assert!(e.contains("commercial"), "{e}");
}

#[test]
fn every_table_names_its_source_and_every_method_what_it_leaves_out() {
    for m in methods() {
        let n = &m.standard;
        for (what, s) in
            [("source", &m.source), ("dwelling", &m.dwelling.source), ("diversity", &m.diversity.source), ("factor", &m.factor_source)]
        {
            assert!(!s.trim().is_empty(), "{n}: {what} names no source");
        }
        assert!(m.dwelling.above.as_ref().is_none_or(|c| !c.source.trim().is_empty()), "{n}: curve");
        assert!(m.dwelling.grades.iter().all(|g| !g.source.trim().is_empty()), "{n}: grades");
        assert!(m.common.iter().all(|c| !c.source.trim().is_empty()), "{n}: common");
        assert!(m.premises.iter().all(|p| !p.source.trim().is_empty()), "{n}: premises");
        assert!(m.entrance.as_ref().is_none_or(|e| !e.source.trim().is_empty()), "{n}: entrance");
        assert!(m.diversity.errata.iter().all(|e| !e.why.trim().is_empty()), "{n}: an erratum without its reason");
        assert!(!m.not_covered.is_empty(), "{n}: say what it leaves out");
        assert!(["kVA", "kW"].contains(&m.unit.as_str()), "{n}: unit {}", m.unit);
        assert!(m.country.len() == 2 && m.country.chars().all(|c| c.is_ascii_uppercase()), "{n}: country {}", m.country);
        assert!(
            match m.mean_of {
                MeanOf::Area => !m.dwelling.by_area.is_empty(),
                MeanOf::Demand => !m.dwelling.grades.is_empty() || m.dwelling.least.is_some(),
            },
            "{n}: no way to a dwelling's demand"
        );
    }
}

/// The values of a table with its errata taken.
fn corrected(steps: &[(f64, f64)], errata: &[Erratum]) -> Vec<(f64, f64)> {
    steps.iter().map(|&(k, v)| (k, errata.iter().find(|e| e.at == k).map_or(v, |e| e.value))).collect()
}

#[test]
fn every_table_is_in_order_once_its_errata_are_taken() {
    for m in methods() {
        let n = &m.standard;
        for e in &m.diversity.errata {
            let row = m.diversity.steps.iter().find(|s| s.0 == e.at);
            assert_eq!(row.map(|s| s.1), Some(e.printed), "{n}: the erratum at {} does not match the printed row", e.at);
        }
        for (what, t) in [("area", m.dwelling.by_area.clone()), ("diversity", corrected(&m.diversity.steps, &m.diversity.errata))] {
            for w in t.windows(2) {
                assert!(w[0].0 < w[1].0, "{n} {what}: keys out of order at {}", w[1].0);
                assert!(w[0].1 <= w[1].1, "{n} {what}: {} gives less than {}", w[1].0, w[0].0);
            }
            assert!(t.iter().all(|s| s.0.is_finite() && s.1.is_finite() && s.1 > 0.0), "{n} {what}");
        }
        assert!(m.diversity.steps.iter().all(|s| s.1 <= s.0), "{n}: F above the number of dwellings");
        if let Some(e) = &m.entrance {
            for w in e.rows.windows(2) {
                assert_eq!(w[0].up_to, w[1].over, "{n}: the entrance table has a gap or an overlap at {}", w[0].up_to);
                assert!(w[0].breaker_a < w[1].breaker_a, "{n}: breakers out of order at {}", w[1].over);
            }
        }
        for c in &m.common {
            assert!(c.power_factor > 0.0 && c.power_factor <= 1.0, "{n} {}", c.class);
            assert!(c.bands.iter().all(|b| b.factor > 0.0 && b.factor <= 1.0), "{n} {}", c.class);
            assert!(c.bands.last().is_some_and(|b| b.up_to.is_none()), "{n} {}: the last band takes the rest", c.class);
        }
        if let (Some(c), Some(last)) = (&m.dwelling.above, m.dwelling.by_area.last()) {
            let past = c.coefficient * last.0.powf(c.exponent);
            assert!(near(past, last.1, 0.01), "{n}: the curve does not meet the table at {} m² ({past} × {})", last.0, last.1);
        }
    }
}

#[test]
fn the_brazilian_utilities_share_one_diversity_table() {
    // CODI's RTD-027: with the errata taken, Celesc's Tabela 01, Cemig's Tabela 12, Fecoergs's Anexo U (past its 'up to 10') and
    // Hidropan's Tabela 4 are one table.
    let table = |n: &str| {
        let m = method_of(n);
        corrected(&m.diversity.steps, &m.diversity.errata).into_iter().collect::<Vec<_>>()
    };
    let celesc = table("CELESC N-321.0003");
    let at = |t: &[(f64, f64)], k: f64| t.iter().find(|s| s.0 == k).map(|s| s.1);
    for (name, from) in [("CEMIG ND-5.2", 4.0), ("FECOERGS REGD 035.01.06", 11.0), ("HIDROPAN RIC BT 2014", 11.0)] {
        for (k, v) in table(name).into_iter().filter(|s| s.0 >= from) {
            assert_eq!(at(&celesc, k), Some(v), "{name} at {k}");
        }
    }
}

#[test]
fn celesc_s_area_table_is_its_own_curve_to_the_cent() {
    let m = method_of("CELESC N-321.0003");
    for &(a, d) in m.dwelling.by_area.iter().filter(|s| s.0 > 70.0) {
        assert!(near(d, 0.034939 * a.powf(0.895075), 0.005), "{a} m²: {d}");
    }
}

#[test]
fn a_method_is_data_and_another_country_s_reads_the_same() {
    let m: Method = serde_json::from_str(
        r#"{"standard": "Example", "country": "XX", "source": "a test", "unit": "kVA", "min_dwellings": 1, "mean_of": "area",
            "dwelling": {"source": "t", "by_area": [[50, 2], [100, 3]]},
            "diversity": {"source": "t", "steps": [[1, 1], [2, 1.8]], "beyond": 0.6},
            "factor": 1.0, "factor_source": "t", "factor_on_common": false, "not_covered": ["nothing"]}"#,
    )
    .unwrap();
    let o = compute(&m, &Input { dwellings: vec![alike(5, 60.0)], ..Input::default() }).unwrap();
    assert!(near(o.demand, (1.8 + 3.0 * 0.6) * 3.0, 1e-9), "{o:?}");
    assert!(serde_json::from_str::<Method>(r#"{"standard": "x", "colour": 1}"#).is_err(), "unknown keys are refused");
}

mod command {
    use crate::engine::Session;
    use serde_json::{Value, json};

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("building_demand", &p).unwrap_or_else(|e| panic!("building_demand {p}: {e}"))
    }

    #[test]
    fn a_building_comes_back_with_its_terms_its_entrance_and_its_sources() {
        let mut s = Session::new();
        let r = run(
            &mut s,
            json!({"utility": "CELESC", "dwellings": [{"count": 20, "area": 100}, {"count": 16, "area": 80}],
                   "common": {"lighting": 14, "outlets": 5}, "special": 7}),
        );
        assert_eq!(r["standard"], "CELESC N-321.0003");
        assert_eq!(r["unit"], "kVA");
        assert_eq!(r["dwellings"], 36);
        assert_eq!(r["diversity"], 27.1);
        assert_eq!(r["demand"], 88.04);
        assert_eq!(r["terms"]["common"], 16.0);
        assert_eq!(r["entrance"]["breaker_a"], 150.0);
        assert!(r["sources"].as_array().is_some_and(|a| a.iter().any(|t| t.as_str().is_some_and(|t| t.contains("Tabela 11")))), "{r}");
        assert!(r["not_covered"].as_array().is_some_and(|a| !a.is_empty()), "{r}");
        assert_eq!(crate::engine::find("building_demand").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn a_standard_is_named_by_itself_too_and_errata_are_said() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"standard": "CEMIG ND-5.2", "dwellings": [{"count": 51, "area": 60}]}));
        assert_eq!(r["diversity"], 35.9);
        assert_eq!(r["errata"][0]["printed"], 31.9);
        let r = run(&mut s, json!({"standard": "CEMIG ND-5.2", "dwellings": [{"count": 51, "area": 60}], "as_printed": true}));
        assert_eq!(r["diversity"], 31.9);
        assert_eq!(r["errata"], json!([]));
        let r = run(
            &mut s,
            json!({"standard": "ITC-BT-10", "dwellings": [{"count": 30, "grade": "basic"}], "premises": {"garage_forced": 500}}),
        );
        assert_eq!((r["unit"].as_str(), r["demand"].as_f64()), (Some("kW"), Some(123.85)));
        assert_eq!(r["entrance"], Value::Null);
    }

    #[test]
    fn hostile_input_is_refused() {
        let mut s = Session::new();
        let ok = json!([{"count": 10, "area": 80}]);
        for p in [
            json!([]),
            json!({}),
            json!({"dwellings": ok.clone()}),
            json!({"utility": "COPEL", "dwellings": ok.clone()}),
            json!({"utility": 5, "dwellings": ok.clone()}),
            json!({"utility": "CELESC", "standard": "CEMIG ND-5.2", "dwellings": ok.clone()}),
            json!({"utility": "CELESC"}),
            json!({"utility": "CELESC", "dwellings": []}),
            json!({"utility": "CELESC", "dwellings": 5}),
            json!({"utility": "CELESC", "dwellings": [5]}),
            json!({"utility": "CELESC", "dwellings": [{"count": 0, "area": 80}]}),
            json!({"utility": "CELESC", "dwellings": [{"count": -1, "area": 80}]}),
            json!({"utility": "CELESC", "dwellings": [{"count": 1.5, "area": 80}]}),
            json!({"utility": "CELESC", "dwellings": [{"count": 4294967295u64, "area": 80}, {"count": 10, "area": 80}]}),
            json!({"utility": "CELESC", "dwellings": [{"count": 10, "area": -80}]}),
            json!({"utility": "CELESC", "dwellings": [{"count": 10, "area": 1e300}]}),
            json!({"utility": "CELESC", "dwellings": [{"count": 10, "area": "big"}]}),
            json!({"utility": "CELESC", "dwellings": [{"count": 10, "area": 80, "floors": 3}]}),
            json!({"utility": "CELESC", "dwellings": [{"count": 10, "area": 80, "grade": "basic"}]}),
            json!({"utility": "CELESC", "dwellings": ok.clone(), "common": {"lighting": -1}}),
            json!({"utility": "CELESC", "dwellings": ok.clone(), "common": {"lighting": "x"}}),
            json!({"utility": "CELESC", "dwellings": ok.clone(), "common": [1]}),
            json!({"utility": "CELESC", "dwellings": ok.clone(), "special": -5}),
            json!({"utility": "CELESC", "dwellings": ok.clone(), "special": 1e300}),
            json!({"utility": "CELESC", "dwellings": ok.clone(), "commercial": "x"}),
            json!({"utility": "CELESC", "dwellings": ok.clone(), "premises": {"commercial": 20}}),
            json!({"utility": "CELESC", "dwellings": ok.clone(), "as_printed": "yes"}),
            json!({"utility": "CELESC", "dwellings": ok.clone(), "units": 1}),
        ] {
            assert!(s.execute("building_demand", &p).is_err(), "{p}");
        }
    }
}
