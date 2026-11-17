//! The catalogues as printed and the fill worked by hand.

use super::*;

fn rules() -> &'static Rules {
    &rule_sets()[0]
}

fn sizing() -> &'static nbr5410::Rules {
    &nbr5410::rule_sets()[0]
}

fn family(key: &str) -> Option<&'static Family> {
    rules().family(key)
}

fn limit(cables: usize) -> f64 {
    rules().limit(cables)
}

fn cable_diameter(section: f64) -> Option<f64> {
    rules().cable_diameter(section)
}

fn fill(size: &Size, sections: &[f64]) -> Option<Fill> {
    rules().fill(size, sections)
}

fn smallest(f: &'static Family, sections: &[f64]) -> Option<&'static Size> {
    rules().smallest(f, sections)
}

fn conductors(phases: Phases, section: f64, reduced: bool) -> Vec<f64> {
    super::conductors(sizing(), phases, section, reduced)
}

fn near(a: f64, b: f64, e: f64) -> bool {
    (a - b).abs() <= e
}

fn row(f: &str) -> String {
    family(f)
        .map(|f| f.sizes.iter().map(|s| format!("{} {} {} {}", s.0, s.1, s.2, s.3)).collect::<Vec<_>>().join(" | "))
        .unwrap_or_default()
}

#[test]
fn the_catalogues_are_as_printed() {
    assert_eq!(
        row("pvc-thread"),
        "1/2\" 20 20.8 2.2 | 3/4\" 25 26.2 2.3 | 1\" 32 32.9 2.7 | 1.1/4\" 40 41.9 3 | 1.1/2\" 50 47.4 3 | 2\" 60 59.1 3.2 | 2.1/2\" 75 74.9 3.8 | 3\" 85 87.6 4 | 4\" 110 112.7 4.4"
    );
    assert_eq!(row("pvc-solder"), "DN20 20 20 1.6 | DN25 25 25 1.8 | DN32 32 32 2.1");
    assert_eq!(
        row("steel-heavy"),
        "1/2\" 15 20 1.5 | 3/4\" 20 25.2 1.5 | 1\" 25 31.5 1.5 | 1.1/4\" 32 40.5 2 | 1.1/2\" 40 46.6 2.25 | 2\" 50 58.4 2.25 | 2.1/2\" 65 74.1 2.65 | 3\" 80 86.8 2.65 | 4\" 100 111.6 2.65"
    );
    assert!(row("steel-rir").starts_with("1/2\" 15 21.3 2.25 | 3/4\" 20 26.9 2.25 | 1\" 25 33.7 2.65"));
    assert!(row("steel-rir").ends_with("5\" 125 141.3 5 | 6\" 150 168.3 5.3"));
    assert!(rules().families.iter().all(|f| f.url.starts_with("https://") && !f.page.is_empty() && !f.document.is_empty()));
    assert_eq!(rules().cable_diameters.len(), 18);
    assert_eq!(cable_diameter(2.5), Some(3.55));
    assert_eq!(cable_diameter(35.0), Some(10.15));
    assert_eq!(cable_diameter(240.0), Some(24.2));
    assert_eq!(cable_diameter(300.0), None, "no diameter above 240 mm2 in the catalogue");
}

#[test]
fn the_inside_diameter_is_the_outside_less_twice_the_wall() {
    let half = family("pvc-thread").and_then(|f| f.sizes.first());
    assert!(half.is_some_and(|s| near(inside(s), 16.4, 1e-9)), "1/2\" PVC: 16.4 mm");
}

#[test]
fn the_limits_and_the_conductors() {
    assert_eq!((limit(1), limit(2), limit(3), limit(12)), (0.53, 0.31, 0.40, 0.40));
    assert_eq!(conductors(Phases::PhaseNeutral, 2.5, false), vec![2.5, 2.5, 2.5]);
    assert_eq!(conductors(Phases::TwoPhases, 4.0, false), vec![4.0, 4.0, 4.0]);
    assert_eq!(conductors(Phases::Three, 10.0, false), vec![10.0, 10.0, 10.0, 10.0]);
    assert_eq!(conductors(Phases::ThreeNeutral, 35.0, false), vec![35.0, 35.0, 35.0, 35.0, 16.0]);
    assert_eq!(conductors(Phases::ThreeNeutral, 70.0, true), vec![70.0, 70.0, 70.0, 35.0, 35.0]);
}

#[test]
fn a_protective_conductor_is_a_standard_section_so_its_diameter_is_known() {
    for (phase, pe) in [(95.0, 50.0), (120.0, 70.0), (150.0, 95.0), (185.0, 95.0)] {
        let all = conductors(Phases::ThreeNeutral, phase, false);
        assert_eq!(all.last().copied(), Some(pe));
        assert!(
            all.iter().all(|s| cable_diameter(*s).is_some()),
            "{phase} mm2: every conductor has a diameter, so the fill is not pending"
        );
        assert!(family("steel-heavy").and_then(|f| smallest(f, &all)).is_some(), "{phase} mm2 fits some size");
    }
}

#[test]
fn the_fill_is_worked_by_hand() {
    let pvc = family("pvc-thread");
    let sizes = pvc.map(|f| f.sizes.as_slice()).unwrap_or(&[]);
    let f = sizes.first().and_then(|s| fill(s, &[2.5, 2.5, 2.5]));
    assert!(f.is_some_and(|f| near(f.ratio, 3.0 * 3.55 * 3.55 / (16.4 * 16.4), 1e-12)
        && near(f.ratio, 0.1406, 1e-4)
        && f.ok
        && f.limit == 0.40));
    let twelve = [2.5; 12];
    assert!(sizes.get(1).and_then(|s| fill(s, &twelve)).is_some_and(|f| near(f.ratio, 0.324, 1e-3) && f.ok), "four circuits in 3/4\"");
    assert!(sizes.first().and_then(|s| fill(s, &twelve)).is_some_and(|f| near(f.ratio, 0.562, 1e-3) && !f.ok), "in 1/2\": over");
    assert!(pvc.and_then(|f| smallest(f, &[2.5, 2.5, 2.5])).is_some_and(|s| s.0 == "1/2\""));
    assert!(pvc.and_then(|f| smallest(f, &twelve)).is_some_and(|s| s.0 == "3/4\""));
    assert!(pvc.and_then(|f| smallest(f, &[2.5; 14])).is_some_and(|s| s.0 == "3/4\""), "37.8 %");
    assert!(pvc.and_then(|f| smallest(f, &[2.5; 15])).is_some_and(|s| s.0 == "1\""), "3/4\" would be 40.5 %");
    let rir = family("steel-rir");
    let three_n = conductors(Phases::ThreeNeutral, 35.0, false);
    assert!(
        rir.and_then(|f| smallest(f, &three_n)).is_some_and(|s| s.0 == "1.1/4\""),
        "four of 35 mm2 and a 16: 1\" is over, 1 1/4\" takes it"
    );
    assert!(pvc.and_then(|f| smallest(f, &[300.0])).is_none());
    assert!(sizes.first().and_then(|s| fill(s, &[300.0])).is_none(), "no diameter: pending, not guessed");
}

mod command {
    use serde_json::{Value, json};

    use crate::engine::Session;

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("conduit_fill", &p).unwrap_or_else(|e| panic!("conduit_fill {p}: {e}"))
    }

    #[test]
    fn the_command_gives_the_fill_the_smallest_size_and_its_sources() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"section": 2.5, "circuits": 4}));
        assert_eq!((&r["conductors"], &r["smallest"]), (&json!(12), &json!("3/4\"")));
        assert_eq!(r["fill_percent"], json!(null), "no size asked: only the smallest");
        let r = run(&mut s, json!({"section": 4, "circuits": 4, "size": "3/4\""}));
        assert_eq!((&r["limit_percent"], &r["ok"], &r["smallest"]), (&json!(40.0), &json!(false), &json!("1\"")));
        assert!(r["fill_percent"].as_f64().is_some_and(|p| p > 40.0 && p < 44.0));
        assert!(r["source"]["url"].as_str().is_some_and(|u| u.starts_with("https://")));
        let r = run(&mut s, json!({"section": 35, "phases": "3FN", "family": "steel-rir"}));
        assert_eq!((&r["conductors"], &r["smallest"]), (&json!(5), &json!("1.1/4\"")));
        let r = run(&mut s, json!({"section": 300}));
        assert!(r["pending"].as_str().is_some_and(|p| p.contains("300")), "no diameter: pending, never guessed");
        assert_eq!(crate::engine::find("conduit_fill").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn a_bad_parameter_says_which() {
        let mut s = Session::new();
        for (bad, says) in [
            (json!({}), "section"),
            (json!({"section": 2.5, "family": "wood"}), "family"),
            (json!({"section": 2.5, "size": "9\""}), "size"),
            (json!({"section": 2.5, "phases": "2F"}), "phases"),
            (json!({"section": 2.5, "circuits": 0}), "circuits"),
            (json!({"section": 2.5, "pipe": 1}), "pipe"),
            (json!([]), "object"),
        ] {
            let e = s.execute("conduit_fill", &bad).expect_err("should fail");
            assert!(e.contains(says), "{bad}: {e}");
        }
    }
}
