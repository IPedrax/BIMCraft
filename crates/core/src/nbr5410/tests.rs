//! The circuit sizing against the standard's tables as printed and against circuits worked by hand.

use super::*;

fn near(a: f64, b: f64, e: f64) -> bool {
    (a - b).abs() <= e
}

fn lighting(watts: f64) -> Circuit {
    Circuit { load: watts, use_: Use::Lighting, ..Circuit::default() }
}

fn power(watts: f64) -> Circuit {
    Circuit { load: watts, ..Circuit::default() }
}

#[test]
fn tabela_36_is_as_printed() {
    // 1.5 mm2 and 2.5 mm2 in B1 with two and three loaded conductors, in C, and in D
    assert_eq!(ampacity(Insulation::Pvc, 1.5, "7", 2), Some(17.5));
    assert_eq!(ampacity(Insulation::Pvc, 1.5, "7", 3), Some(15.5));
    assert_eq!(ampacity(Insulation::Pvc, 2.5, "7", 2), Some(24.0));
    assert_eq!(ampacity(Insulation::Pvc, 2.5, "11", 2), Some(27.0));
    assert_eq!(ampacity(Insulation::Pvc, 2.5, "61", 2), Some(29.0));
    assert_eq!(ampacity(Insulation::Pvc, 16.0, "7", 2), Some(76.0));
    assert_eq!(ampacity(Insulation::Pvc, 35.0, "1", 2), Some(99.0));
    assert_eq!(ampacity(Insulation::Pvc, 1000.0, "11", 3), Some(996.0));
    assert_eq!(ampacity(Insulation::Pvc, 3.0, "7", 2), None, "a section the table does not list");
    assert_eq!(ampacity(Insulation::Pvc, 2.5, "99", 2), None, "a method the table does not cover");
}

#[test]
fn tabela_37_is_as_printed() {
    assert_eq!(ampacity(Insulation::Epr, 1.5, "7", 2), Some(23.0));
    assert_eq!(ampacity(Insulation::Epr, 10.0, "11", 2), Some(80.0));
    assert_eq!(ampacity(Insulation::Epr, 240.0, "61", 3), Some(351.0));
}

#[test]
fn every_section_has_a_row_in_both_tables_and_a_resistance() {
    for s in SECTIONS {
        for ins in [Insulation::Pvc, Insulation::Epr] {
            assert!(ampacity(ins, *s, "7", 2).is_some(), "{s} mm2");
            assert!(resistance(*s, ins, false).is_some(), "{s} mm2");
        }
    }
    assert_eq!(CU_PVC.len(), SECTIONS.len());
    assert_eq!(CU_EPR.len(), SECTIONS.len());
    // an ampacity never falls as the section grows
    for t in [CU_PVC, CU_EPR] {
        let mut sorted = t.to_vec();
        sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
        for pair in sorted.windows(2) {
            for (a, b) in pair[0].1.iter().zip(pair[1].1.iter()) {
                assert!(b >= a, "{} mm2", pair[1].0);
            }
        }
    }
}

#[test]
fn the_correction_factors_are_as_printed() {
    assert_eq!(temperature_factor(Insulation::Pvc, 30.0, false), Some(1.0));
    assert_eq!(temperature_factor(Insulation::Pvc, 40.0, false), Some(0.87));
    assert_eq!(temperature_factor(Insulation::Epr, 40.0, false), Some(0.91));
    assert_eq!(temperature_factor(Insulation::Pvc, 33.0, false), Some(0.94), "between rows: the next one up, on the safe side");
    assert_eq!(temperature_factor(Insulation::Pvc, 65.0, false), None, "hotter than PVC allows");
    assert_eq!(temperature_factor(Insulation::Epr, 80.0, false), Some(0.41));
    assert_eq!(temperature_factor(Insulation::Epr, 85.0, false), None);
    assert_eq!(temperature_factor(Insulation::Pvc, 5.0, false), None);
    assert_eq!(temperature_factor(Insulation::Pvc, 20.0, true), Some(1.0));
    assert_eq!(temperature_factor(Insulation::Pvc, 30.0, true), Some(0.89));
    assert_eq!(soil_factor(2.5), Some(1.0));
    assert_eq!(soil_factor(1.0), Some(1.18));
    assert_eq!(soil_factor(3.0), Some(0.96));
    assert_eq!(soil_factor(3.5), None);
    assert_eq!(soil_factor(0.5), None);

    let g = |m: &str, bundle: bool, n: u32, s: f64| grouping_factor(m, bundle, n, s);
    assert_eq!(g("7", false, 1, 0.0), Some(1.0));
    assert_eq!(g("7", false, 2, 0.0), Some(0.80));
    assert_eq!(g("7", false, 4, 0.0), Some(0.65));
    assert_eq!(g("7", false, 8, 0.0), Some(0.52));
    assert_eq!(g("7", false, 9, 0.0), Some(0.50));
    assert_eq!(g("7", false, 12, 0.0), Some(0.45));
    assert_eq!(g("7", false, 16, 0.0), Some(0.41));
    assert_eq!(g("7", false, 20, 0.0), Some(0.38));
    assert_eq!(g("11", false, 3, 0.0), Some(0.79), "a layer on a wall");
    assert_eq!(g("11", true, 3, 0.0), Some(0.70), "cables of a method C bundled use the bundle's row");
    assert_eq!(g("11", false, 12, 0.0), Some(0.70));
    assert_eq!(g("11A", false, 1, 0.0), Some(0.95));
    assert_eq!(g("11A", false, 12, 0.0), Some(0.61));
    assert_eq!(g("61", false, 1, 0.0), Some(1.0));
    assert_eq!(g("61", false, 3, 0.0), Some(0.75));
    assert_eq!(g("61", false, 3, 0.3), g("61", false, 3, 0.25), "0.3 m falls in the 0.25 m column, the narrower one: the safe side");
    assert_eq!(g("61", false, 3, 0.1), g("61", false, 3, 0.0), "0.1 m takes the nil column");
    assert!(g("61", false, 3, 0.3) < g("61", false, 3, 0.5), "a wider spacing never gets a smaller factor");
    assert_eq!(g("61A", false, 6, 1.0), Some(0.90));
    assert_eq!(g("61", false, 7, 0.0), None, "Tabela 45 stops at six circuits");
    assert_eq!(g("61", false, 3, 2.0), None, "and at 1 m of spacing");
}

#[test]
fn the_neutral_and_the_protective_conductor() {
    assert_eq!(minimum_section(Use::Lighting), 1.5);
    assert_eq!(minimum_section(Use::Power), 2.5);
    assert_eq!(neutral_section(25.0, true), 25.0);
    assert_eq!(neutral_section(70.0, false), 70.0, "as the phases unless asked");
    assert_eq!(neutral_section(70.0, true), 35.0);
    assert_eq!(neutral_section(400.0, true), 185.0);
    assert_eq!(protective_section(10.0), 10.0);
    assert_eq!(protective_section(25.0), 16.0);
    assert_eq!(protective_section(70.0), 35.0);
    assert_eq!(protective_section(240.0), 120.0);
    for (phase, pe) in [(95.0, 50.0), (120.0, 70.0), (150.0, 95.0), (185.0, 95.0), (300.0, 150.0), (400.0, 240.0)] {
        assert_eq!(protective_section(phase), pe, "PE of {phase} mm²: half, up to a standard section");
        assert!(SECTIONS.contains(&protective_section(phase)));
    }
    assert_eq!(cable_text(Phases::PhaseNeutral, 2.5, false), "1 × 2.5 + N 2.5 + PE 2.5 mm²");
    assert_eq!(cable_text(Phases::ThreeNeutral, 70.0, true), "3 × 70 + N 35 + PE 35 mm²");
    assert_eq!(cable_text(Phases::Three, 10.0, false), "3 × 10 + PE 10 mm²");
}

#[test]
fn the_conductor_resistance_is_the_makers_at_the_working_temperature() {
    let rigid = resistance(2.5, Insulation::Pvc, false).unwrap_or_default();
    let flexible = resistance(2.5, Insulation::Pvc, true).unwrap_or_default();
    assert!(near(rigid, 7.41 * (1.0 + 0.00393 * 50.0), 1e-9));
    assert!(near(flexible, 7.98 * (1.0 + 0.00393 * 50.0), 1e-9));
    assert!(near(resistance(2.5, Insulation::Epr, true).unwrap_or_default(), 7.98 * (1.0 + 0.00393 * 70.0), 1e-9));
    let (flex, rigid) =
        (resistance(300.0, Insulation::Pvc, true).unwrap_or_default(), resistance(300.0, Insulation::Pvc, false).unwrap_or_default());
    assert!(
        near(flex, rigid * 1.07, 1e-9) && flex > rigid * 0.0641 / 0.0601,
        "above 240 mm2 the rigid figure raised by 7 %, over the 6.7 % of NBR NM 280"
    );
    assert!(flexible >= rigid);
}

#[test]
fn the_design_current_follows_the_load_the_phases_and_the_voltage() {
    assert!(near(design_current(&lighting(500.0)), 500.0 / 127.0, 1e-9));
    let mut c = power(1000.0);
    c.power_factor = 0.8;
    assert!(near(design_current(&c), 1000.0 / (127.0 * 0.8), 1e-9), "a load in W is divided by the power factor");
    c.apparent = true;
    assert!(near(design_current(&c), 1000.0 / 127.0, 1e-9), "one in VA is not");
    let t = Circuit { load: 10_000.0, phases: Phases::Three, system: 1, ..Circuit::default() };
    assert!(near(design_current(&t), 10_000.0 / (3f64.sqrt() * 380.0), 1e-9));
    assert_eq!(volts(0, Phases::PhaseNeutral), 127.0);
    assert_eq!(volts(0, Phases::TwoPhases), 220.0);
    assert_eq!(volts(2, Phases::ThreeNeutral), 400.0);
    c.demand = 0.5;
    assert!(near(design_current(&c), 500.0 / 127.0, 1e-9), "the demand factor scales the load");
}

#[test]
fn a_lighting_circuit_is_the_smallest_conductor_and_breaker() {
    let d = design(&lighting(500.0)).needed;
    let n = d.map(|e| (e.section, e.breaker));
    assert_eq!(n, Some((1.5, 6.0)), "3.9 A: 6 A, 1.5 mm2 (17.5 A in B1)");
}

#[test]
fn grouping_and_temperature_make_the_section_grow() {
    let mut c = power(1800.0);
    c.grouped = 4;
    let d = design(&c);
    assert!(near(d.current, 14.17, 0.01));
    let f = d.factors;
    assert_eq!(f.map(|f| f.grouping), Some(0.65));
    // 2.5 mm2 carries 24 * 0.65 = 15.6 A, under the 16 A breaker 14.2 A asks for: 4 mm2
    assert_eq!(d.needed.map(|e| (e.section, e.breaker)), Some((4.0, 16.0)));
    assert!(d.needed.is_some_and(|e| near(e.carries, 32.0 * 0.65, 1e-9)));
    c.ambient = Some(40.0);
    let hot = design(&c).needed;
    assert!(hot.is_some_and(|e| near(e.carries, 32.0 * 0.65 * 0.87, 1e-9)), "40 degrees takes 0.87 of it: 18.1 A, still over 16 A");
}

#[test]
fn a_long_circuit_is_sized_by_its_voltage_drop() {
    let mut c = power(2000.0);
    c.length = 60.0; // metres
    let d = design(&c);
    // 15.75 A, 16 A breaker. 6 mm2 flexible drops 5.9 %, over 4 %: 10 mm2 drops 3.4 %
    let n = d.needed;
    assert_eq!(n.map(|e| (e.section, e.breaker)), Some((10.0, 16.0)));
    assert!(n.and_then(|e| e.drop).is_some_and(|(p, ok)| ok && near(p, 3.40, 0.01)));
    let six = evaluate(&c, 6.0, 16.0).ok();
    assert!(six.and_then(|e| e.drop).is_some_and(|(p, ok)| !ok && near(p, 5.87, 0.01)));
    c.flexible = false;
    let rigid = evaluate(&c, 6.0, 16.0).ok().and_then(|e| e.drop);
    assert!(rigid.is_some_and(|d| d.0 < 5.87), "a rigid conductor drops less");
    c.length = 0.0;
    assert_eq!(design(&c).needed.map(|e| e.section), Some(2.5), "no length, no drop to check");
}

#[test]
fn the_breaker_list_has_70_and_90_a() {
    let d = design(&power(8500.0)).needed;
    assert_eq!(d.map(|e| (e.section, e.breaker)), Some((16.0, 70.0)), "66.9 A: a 70 A breaker, 16 mm2 carries 76 A");
    assert!(BREAKERS.contains(&90.0));
}

#[test]
fn a_chosen_section_and_breaker_are_never_made_smaller() {
    let mut c = power(1000.0);
    c.section = 6.0;
    c.breaker = 32.0;
    let d = design(&c);
    assert_eq!(d.needed.map(|e| (e.section, e.breaker)), Some((6.0, 32.0)));
    assert_eq!(d.chosen.map(|e| e.breaker_ok), Some(Some(true)));
    c.section = 2.5;
    c.breaker = 25.0;
    let bad = design(&c).chosen;
    assert_eq!(bad.map(|e| e.breaker_ok), Some(Some(false)), "25 A on 2.5 mm2 (24 A in B1): the cable does not carry the breaker");
}

#[test]
fn three_phases_carry_less_per_conductor() {
    let t = Circuit { load: 20_000.0, phases: Phases::ThreeNeutral, system: 1, ..Circuit::default() };
    let d = design(&t);
    assert!(near(d.current, 20_000.0 / (3f64.sqrt() * 380.0), 1e-9));
    assert!(d.needed.is_some_and(|e| near(e.breaker, 32.0, 1e-9) && e.section >= 4.0));
}

#[test]
fn what_cannot_be_looked_up_is_a_fault() {
    let mut c = power(1000.0);
    c.method = "99".to_string();
    assert_eq!(design(&c).fault, Some(Fault::Method));
    c.method = "7".to_string();
    c.ambient = Some(5.0);
    assert_eq!(design(&c).fault, Some(Fault::Temperature));
    c.ambient = None;
    c.grouped = 7;
    c.method = "61".to_string();
    assert_eq!(design(&c).fault, Some(Fault::Grouping));
    c.grouped = 1;
    c.soil = 9.0;
    assert_eq!(design(&c).fault, Some(Fault::Soil));
    assert_eq!(design(&power(1.0e8)).fault, Some(Fault::TooBig));
}

#[test]
fn buried_cables_use_the_soil_and_the_buried_column() {
    let mut c = power(2000.0);
    c.method = "61".to_string();
    c.soil = 2.0;
    let d = design(&c);
    assert_eq!(d.factors.map(|f| f.soil), Some(1.05));
    assert_eq!(d.factors.map(|f| f.temperature), Some(1.0), "20 degrees in the soil by default");
    assert!(d.needed.is_some_and(|e| near(e.carries, 29.0 * 1.05, 1e-9) || e.section > 2.5));
}

mod command {
    use serde_json::{Value, json};

    use crate::engine::Session;

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("circuit", &p).unwrap_or_else(|e| panic!("circuit {p}: {e}"))
    }

    #[test]
    fn the_circuit_command_sizes_a_circuit_and_changes_nothing() {
        let mut s = Session::new();
        let before = s.execute("inspect", &json!({})).map(|v| v["history"].clone()).unwrap_or_default();
        let r = run(&mut s, json!({"load": 1800, "grouped": 4, "ambient": 40, "length": 25000}));
        assert_eq!((&r["section_mm2"], &r["breaker_a"]), (&json!(4.0), &json!(16.0)));
        assert_eq!(r["conductors"], json!("1 × 4 + N 4 + PE 4 mm²"));
        assert_eq!(r["carries_a"], json!(18.1));
        assert_eq!(r["current_a"], json!(14.17));
        assert_eq!(r["factors"]["temperature"], json!(0.87));
        assert_eq!(r["factors"]["grouping"], json!(0.65));
        assert_eq!(r["drop_percent"], json!(3.3), "25 m of flexible 4 mm2 at 14.2 A in 127 V");
        assert_eq!(r["grew"], json!(true));
        assert_eq!(s.execute("inspect", &json!({})).map(|v| v["history"].clone()).unwrap_or_default(), before, "no undo step");
        // `circuit` is also a circuit of the model, an undo step: the sizing of it never changes the model, so it makes none.
        assert_eq!(crate::engine::find("circuit").map(|c| c.undoable), Some(true));
    }

    #[test]
    fn a_three_phase_circuit_at_another_voltage_and_a_chosen_section() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"load": 20000, "phases": "3FN", "system": "220/380", "section": 70, "reduced_neutral": true}));
        assert_eq!(r["volts"], json!(380.0));
        assert_eq!((&r["section_mm2"], &r["breaker_a"]), (&json!(70.0), &json!(32.0)), "never smaller than chosen");
        assert_eq!(r["conductors"], json!("3 × 70 + N 35 + PE 35 mm²"));
        assert_eq!(r["chosen"]["section_mm2"], json!(70.0));
        assert_eq!(r["grew"], json!(true), "a breaker was still to choose");
    }

    #[test]
    fn a_bad_parameter_says_which() {
        let mut s = Session::new();
        for (bad, says) in [
            (json!({}), "load"),
            (json!({"load": -5}), "load"),
            (json!({"load": 100, "voltage": 220}), "voltage"),
            (json!({"load": 100, "phases": "2F"}), "phases"),
            (json!({"load": 100, "method": "99"}), "method"),
            (json!({"load": 100, "section": 3}), "section"),
            (json!({"load": 100, "breaker": 7}), "breaker"),
            (json!({"load": 100, "power_factor": 0}), "power_factor"),
            (json!({"load": 100, "ambient": 5}), "10 degrees"),
            (json!({"load": 100, "ambient": 85, "insulation": "epr"}), "10 degrees"),
            (json!({"load": 1e8}), "split the circuit"),
            (json!({"load": f64::MAX}), "load"),
            (json!("x"), "object"),
        ] {
            let e = s.execute("circuit", &bad).expect_err("should fail");
            assert!(e.contains(says), "{bad}: {e}");
        }
    }
}
