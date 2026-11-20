//! A project's units and the places its lengths show: the `units` command, what the model shows with them, and its file.

use serde_json::{Value, json};

use crate::dims::DimensionType;
use crate::engine::Session;
use crate::model::Model;
use crate::units::Units;

fn run(s: &mut Session, p: Value) -> Value {
    s.execute("units", &p).unwrap_or_else(|e| panic!("units {p}: {e}"))
}

#[test]
fn a_project_shows_lengths_to_the_decimals_it_asks_for_and_a_rejected_ask_changes_nothing() {
    let mut s = Session::new();
    // As drawings have always shown them: millimetres whole.
    assert_eq!((s.model.length(3.0), s.model.decimals_shown()), ("3".to_string(), Some(0)));
    let said = run(&mut s, json!({"length": "m"}));
    assert_eq!((said["length"].as_str(), said["decimals"].as_u64()), (Some("m"), Some(2)), "{said}");
    assert_eq!((s.model.length(3.0), s.model.length(55.0)), ("0.00".to_string(), "0.06".to_string()));
    // Three places: 3 mm and 55 mm show as they are, in a field and with their unit too.
    let said = run(&mut s, json!({"decimals": 3}));
    assert_eq!(said["decimals"], 3);
    assert_eq!((s.model.length(3.0), s.model.length(55.0)), ("0.003".to_string(), "0.055".to_string()));
    assert_eq!((s.model.with_unit(3.0), s.model.field(3.0)), ("0.003 m".to_string(), "0.003".to_string()));
    assert_eq!(s.model.decimals, Some(3));
    // Asking for nothing changes nothing, and `length` alone to what it is keeps them.
    run(&mut s, json!({}));
    run(&mut s, json!({"length": "m"}));
    assert_eq!(s.model.decimals, Some(3));
    // Out of range, not a whole number, or not for the units: an error that says what to fix, and the model as it was.
    let before = std::sync::Arc::clone(&s.model);
    for (bad, said) in [
        (json!({"decimals": 1}), "2 to 5"),
        (json!({"decimals": 6}), "2 to 5"),
        (json!({"decimals": -1}), "2 to 5"),
        (json!({"decimals": 2.5}), "2 to 5"),
        (json!({"decimals": "3"}), "2 to 5"),
        (json!({"decimals": true}), "2 to 5"),
        (json!({"decimals": 1e300}), "2 to 5"),
        // Not 3 modulo 256.
        (json!({"decimals": 259}), "2 to 5"),
        (json!({"length": "mm", "decimals": 4}), "0 to 3"),
        (json!({"length": "ft-in", "decimals": 3}), "sixteenths"),
        (json!({"length": "in", "decimals": 2}), "sixteenths"),
        (json!({"length": "furlong"}), "mm, cm, m"),
        (json!({"decimal": 3}), "isn't one of its parameters"),
    ] {
        let e = s.execute("units", &bad).expect_err(&bad.to_string());
        assert!(e.contains(said), "{bad}: {e}");
        assert!(s.model.same_settings(&before) && s.model.decimals == Some(3), "{bad}");
    }
    // More places than the most is not shown either, if a file says so.
    assert_eq!(Units::Metres.length_to(3.0, Some(9)), "0.00300");
    // Null, and the fewest, are what none says; so is a change of units, which has places of its own.
    run(&mut s, json!({"decimals": null}));
    assert_eq!((s.model.decimals, s.model.length(3.0)), (None, "0.00".to_string()));
    run(&mut s, json!({"decimals": 2}));
    assert_eq!(s.model.decimals, None);
    run(&mut s, json!({"decimals": 4}));
    let said = run(&mut s, json!({"length": "cm"}));
    assert_eq!((s.model.decimals, said["decimals"].as_u64()), (None, Some(1)));
    // Units and places together.
    run(&mut s, json!({"length": "mm", "decimals": 3}));
    assert_eq!((s.model.units, s.model.decimals, s.model.length(3.5)), (Units::Millimetres, Some(3), "3.500".to_string()));
    // Inches and feet-inches show sixteenths and no places.
    let said = run(&mut s, json!({"length": "ft-in"}));
    assert_eq!((s.model.decimals, said["decimals"].clone()), (None, Value::Null));
    assert_eq!(s.model.length(3000.0), Units::FeetInches.length(3000.0));
}

#[test]
fn the_decimals_of_a_project_are_undone_kept_in_its_file_and_checked_when_a_file_says_them() {
    let mut s = Session::new();
    run(&mut s, json!({"length": "m"}));
    // The file of a project that never asked has no mention of it.
    let top = |s: &Session| serde_json::from_str::<Value>(&s.model.to_json().unwrap()).unwrap();
    assert!(top(&s).get("decimals").is_none());
    run(&mut s, json!({"decimals": 4}));
    assert_eq!(top(&s)["decimals"], 4);
    let (back, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!((back.decimals, back.units, left_out.is_empty()), (Some(4), Units::Metres, true), "{left_out:?}");
    s.execute("undo", &json!({})).unwrap();
    assert_eq!((s.model.units, s.model.decimals), (Units::Metres, None));
    s.execute("redo", &json!({})).unwrap();
    assert_eq!(s.model.decimals, Some(4));
    // A technical standard that changes the units takes the places along: they were another unit's.
    s.execute("standard", &json!({"set": "ansi", "adapt_units": true})).unwrap();
    assert_eq!((s.model.units, s.model.decimals), (Units::FeetInches, None));
    // A file with places its units don't show is refused, saying what to run.
    let mut m = Model::new("x");
    m.decimals = Some(9);
    assert!(m.check().unwrap_err().contains("0 to 3"), "{:?}", m.check());
    m.decimals = Some(2);
    m.units = Units::FeetInches;
    assert!(m.check().unwrap_err().contains("sixteenths"), "{:?}", m.check());
    m.decimals = None;
    assert_eq!(m.check(), Ok(()));
}

#[test]
fn dimensions_and_schedules_follow_the_project_s_decimals_unless_their_own_say_otherwise() {
    let own = DimensionType { decimals: Some(1), ..DimensionType::default() };
    let metres = DimensionType { units: Some(Units::Metres), ..DimensionType::default() };
    // The project's units and places; its own places; its own units, which the project's places are not for.
    assert_eq!(DimensionType::default().length(Units::Metres, Some(3), 3.0), "0.003");
    assert_eq!(DimensionType::default().length(Units::Metres, None, 3.0), "0.00");
    assert_eq!(own.length(Units::Metres, Some(3), 3.0), "0.0");
    assert_eq!(metres.length(Units::Millimetres, Some(3), 3000.0), "3.00");
    assert_eq!(crate::schedule::shown_to(Units::Metres, Some(3), crate::schedule::Kind::Length, &json!(55.0)), "0.055");
    assert_eq!(crate::schedule::shown(Units::Metres, crate::schedule::Kind::Length, &json!(55.0)), "0.06");
}
