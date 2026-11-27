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
        (json!({"decimals": 6}), "0 to 5"),
        (json!({"decimals": -1}), "0 to 5"),
        (json!({"decimals": 2.5}), "0 to 5"),
        (json!({"decimals": "3"}), "0 to 5"),
        (json!({"decimals": true}), "0 to 5"),
        (json!({"decimals": 1e300}), "0 to 5"),
        // Not 3 modulo 256.
        (json!({"decimals": 259}), "0 to 5"),
        (json!({"length": "mm", "decimals": 4}), "0 to 3"),
        (json!({"length": "ft-in", "decimals": 5}), "0 to 4"),
        (json!({"length": "in", "decimals": 5}), "0 to 4"),
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
    // Inches and feet-inches show sixteenths by default, which is four binary places.
    let said = run(&mut s, json!({"length": "ft-in"}));
    assert_eq!((s.model.decimals, said["decimals"].as_u64()), (None, Some(4)));
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
    m.decimals = Some(5);
    m.units = Units::FeetInches;
    assert!(m.check().unwrap_err().contains("0 to 4"), "{:?}", m.check());
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

#[test]
fn a_project_s_signs_are_read_written_reset_and_undone() {
    use crate::dims::{PositiveSign, Signs, ZeroSign};
    let mut s = Session::new();
    assert!(s.model.signs.is_none());
    let said = run(&mut s, json!({"signs": {"positive": "never", "zero": "signed"}}));
    assert_eq!(said["signs"], json!({"positive": "never", "zero": "signed"}));
    assert_eq!(s.model.signs, Some(Signs { positive: PositiveSign::Never, zero: ZeroSign::Signed }));
    // A file says them, and none says nothing.
    let top = |s: &Session| serde_json::from_str::<Value>(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(top(&s)["signs"], json!({"positive": "never", "zero": "signed"}));
    let (back, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!((back.signs, left_out.is_empty()), (s.model.signs, true), "{left_out:?}");
    // Null resets them; absent leaves them.
    run(&mut s, json!({"length": "m"}));
    assert!(s.model.signs.is_some());
    let said = run(&mut s, json!({"signs": null}));
    assert_eq!(said["signs"], Value::Null);
    assert!(s.model.signs.is_none());
    // One undo step goes back to what they were.
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.model.signs, Some(Signs { positive: PositiveSign::Never, zero: ZeroSign::Signed }));
    // A misspelled name is refused and changes nothing.
    let before = std::sync::Arc::clone(&s.model);
    for bad in [
        json!({"signs": {"positive": "sometimes", "zero": "signed"}}),
        json!({"signs": {"positive": "always", "zero": "maybe"}}),
        json!({"signs": {"positive": "always"}}),
        json!({"signs": {"positive": "always", "zero": "plain", "extra": 1}}),
        json!({"signs": true}),
    ] {
        let e = s.execute("units", &bad).expect_err(&bad.to_string());
        assert!(e.contains("signs"), "{bad}: {e}");
        assert!(s.model.same_settings(&before), "{bad}");
    }
}

#[test]
fn a_project_rounds_lengths_before_the_point_and_a_field_keeps_the_value() {
    let mut s = Session::new();
    run(&mut s, json!({"length": "cm"}));
    run(&mut s, json!({"decimals": 0}));
    // Ten centimetres: 3173 mm is nearer 320 cm than 310.
    let said = run(&mut s, json!({"rounding": 1, "decimals": 0}));
    assert_eq!(said["rounding"], 1);
    assert_eq!(s.model.rounding, Some(1));
    assert_eq!(s.model.length(3173.0), "320");
    // A field still says the value itself, so Apply on a field left alone changes nothing.
    assert_eq!(s.model.field(3173.0), "3173 mm");
    // Hundreds of centimetres: 3173 mm is nearer 300 cm.
    run(&mut s, json!({"rounding": 2, "decimals": 0}));
    assert_eq!(s.model.length(3173.0), "300");
    // None rounds again, showing the whole centimetres the decimals ask for.
    let said = run(&mut s, json!({"rounding": null}));
    assert_eq!(said["rounding"], Value::Null);
    assert_eq!(s.model.rounding, None);
    assert_eq!(s.model.length(3173.0), "317");
    // A file keeps it, and a change of units takes it away.
    run(&mut s, json!({"rounding": 1, "decimals": 0}));
    let (back, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!((back.rounding, left_out.is_empty()), (Some(1), true), "{left_out:?}");
    run(&mut s, json!({"length": "mm"}));
    assert_eq!((s.model.units, s.model.rounding), (Units::Millimetres, None));
    // Out of range is refused and changes nothing.
    let before = std::sync::Arc::clone(&s.model);
    for bad in [json!({"rounding": 0}), json!({"rounding": 7}), json!({"rounding": 1.5}), json!({"rounding": "1"})] {
        let e = s.execute("units", &bad).expect_err(&bad.to_string());
        assert!(e.contains("rounding"), "{bad}: {e}");
        assert!(s.model.same_settings(&before), "{bad}");
    }
}

#[test]
fn each_kind_of_value_keeps_its_own_units_and_they_are_saved_with_the_project() {
    let mut s = Session::new();
    let said = run(
        &mut s,
        json!({
            "area": "cm2", "volume": "cm3", "slope": "percent", "mass_density": "lb-ft3",
            "time": "h", "speed": "km-h", "currency": "€"
        }),
    );
    assert_eq!(said["display"]["area"], "cm2");
    assert_eq!(s.model.area(1e6), (10_000.0, "cm²"));
    assert_eq!(s.model.volume_shown(1e9), (1_000_000.0, "cm³"));
    assert_eq!(s.model.slope(45.0).1, "%");
    assert!((s.model.slope(45.0).0 - 100.0).abs() < 1e-9);
    assert_eq!(s.model.mass_density(1.0).1, "lb/ft³");
    assert_eq!(s.model.time(3600.0), (1.0, "h"));
    assert!((s.model.speed(10.0).0 - 36.0).abs() < 1e-9);
    assert_eq!(s.model.display.currency(), "€");
    let (back, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!((back.display, left_out.is_empty()), (s.model.display.clone(), true), "{left_out:?}");
    // Null goes back to what the length units imply; a unit it doesn't know is refused.
    run(&mut s, json!({"area": null, "currency": null}));
    assert_eq!(s.model.area(1e6), (1.0, "m²"));
    assert_eq!(s.model.display.currency(), "$");
    let before = std::sync::Arc::clone(&s.model);
    for bad in [json!({"area": "furlong2"}), json!({"time": "fortnight"}), json!({"currency": "way too long"})] {
        let e = s.execute("units", &bad).expect_err(&bad.to_string());
        assert!(e.contains("area") || e.contains("time") || e.contains("currency"), "{bad}: {e}");
        assert!(s.model.same_settings(&before), "{bad}");
    }
}

#[test]
fn each_kind_can_have_its_own_format_options() {
    use crate::dims::{PositiveSign, ZeroSign};
    use crate::units::Quantity;
    let mut s = Session::new();
    let said = run(
        &mut s,
        json!({"format": {
            "quantity": "area", "show_units": false, "positive": "always", "zero": "plain",
            "suppress_trailing_zeros": true, "suppress_zero_feet": true, "digit_grouping": true,
            "suppress_spaces": true, "rounding": 1, "decimals": 2, "symbol": "m2"
        }}),
    );
    assert_eq!(said["display"]["formats"]["area"]["show_units"], false);
    let o = s.model.display.options(Quantity::Area);
    assert_eq!((o.show_units, o.positive, o.zero), (Some(false), Some(PositiveSign::Always), Some(ZeroSign::Plain)));
    assert!(o.suppress_trailing_zeros && o.suppress_zero_feet && o.digit_grouping && o.suppress_spaces);
    assert_eq!((o.rounding, o.decimals), (Some(1), Some(2)));
    assert_eq!(o.symbol.as_deref(), Some("m2"));
    assert_eq!(s.model.display.options(Quantity::Length), &crate::units::FormatOptions::default());
    // A file keeps it; another kind's format is untouched.
    let (back, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!((back.display.formats, left_out.is_empty()), (s.model.display.formats.clone(), true), "{left_out:?}");
    // A quantity it doesn't know is refused and changes nothing.
    let before = std::sync::Arc::clone(&s.model);
    let e = s.execute("units", &json!({"format": {"quantity": "furlong"}})).expect_err("a furlong");
    assert!(e.contains("quantity"), "{e}");
    assert!(s.model.same_settings(&before));
}
