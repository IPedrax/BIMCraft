//! The typed length and angle with no window: what a field takes, and the point it makes.

use super::*;

fn typed(length: &str, angle: &str) -> Typed {
    Typed { length: length.into(), angle: angle.into(), field: Field::Length }
}

const MM: Units = Units::Millimetres;

#[test]
fn a_field_starts_with_a_digit_or_a_point_and_then_takes_what_follows_and_an_angle_may_start_with_a_minus() {
    let mut t = Typed::default();
    assert!(!t.push('m') && !t.push('-') && !t.push(' '), "a length starts with a digit or a point");
    assert!(t.push('3') && t.push('.') && t.push('5') && t.push(' ') && t.push('m'), "then it takes a unit");
    assert_eq!(t.length, "3.5 m");
    assert!(!t.push('\n') && !t.push('\u{7}'), "never a control character");
    assert!(t.switch());
    assert!(t.push('-') && t.push('4') && t.push('5'));
    assert_eq!(t.angle, "-45");
    assert!(t.pop() && t.pop() && t.pop() && !t.pop(), "Backspace takes the field's characters, then none");
    assert!(t.switch() && t.field == Field::Length);
    t.clear();
    assert!(t.is_empty() && !t.switch(), "with nothing typed Tab is not ours");
    let long = "9".repeat(MAX_CHARS + 10);
    for ch in long.chars() {
        t.push(ch);
    }
    assert_eq!(t.length.chars().count(), MAX_CHARS, "a field is as long as it can be read");
}

#[test]
fn a_length_and_an_angle_make_a_point_and_without_an_angle_it_lies_toward_the_cursor() {
    let base = P2(1000.0, 2000.0);
    let near = |p: P2, x: f64, y: f64| (p.0 - x).abs() < 1e-6 && (p.1 - y).abs() < 1e-6;
    let east = point(base, None, &typed("500", "0"), MM).unwrap_or_default();
    assert!(near(east, 1500.0, 2000.0), "{east:?}");
    let north = point(base, None, &typed("500", "90"), MM).unwrap_or_default();
    assert!(near(north, 1000.0, 2500.0), "{north:?}");
    let west = point(base, None, &typed("500", "-180°"), MM).unwrap_or_default();
    assert!(near(west, 500.0, 2000.0), "{west:?}");
    let toward = point(base, Some(P2(1000.0, 9000.0)), &typed("250", ""), MM).unwrap_or_default();
    assert!(near(toward, 1000.0, 2250.0), "the pointer's direction: {toward:?}");
    let as_far = point(base, Some(P2(1300.0, 2400.0)), &typed("", "0"), MM).unwrap_or_default();
    assert!(near(as_far, 1500.0, 2000.0), "an angle alone is as far as the pointer: {as_far:?}");
    let metres = point(base, None, &typed("2", ""), Units::Metres).unwrap_or_default();
    assert!(near(metres, 3000.0, 2000.0), "a bare number is in the project's unit: {metres:?}");
    let with_unit = point(base, None, &typed("30 cm", ""), MM).unwrap_or_default();
    assert!(near(with_unit, 1300.0, 2000.0), "{with_unit:?}");
}

#[test]
fn what_is_no_length_or_no_angle_or_reaches_too_far_is_refused_with_what_was_typed() {
    let base = P2(0.0, 0.0);
    for bad in ["x", "0", "-5", "1e999", "99999999999"] {
        let why = point(base, None, &typed(bad, ""), MM).err().unwrap_or_default();
        assert!(why.contains(bad.trim()) || why.contains("outside"), "{bad}: {why}");
    }
    for bad in ["x", "400", "1e999"] {
        assert!(point(base, None, &typed("100", bad), MM).is_err(), "{bad} is no angle");
    }
    assert!(point(base, None, &typed("", ""), MM).is_err(), "with no pointer and no length there is nothing to measure");
    assert!(point(P2(f64::NAN, 0.0), None, &typed("5", ""), MM).is_err());
    let r = reading(&Typed { length: "350".into(), angle: String::new(), field: Field::Angle });
    assert!(r.contains("350") && r.contains('_'), "{r}");
}

#[test]
fn a_shapes_numbers_are_read_in_the_order_of_its_names_a_length_or_an_angle_and_a_blank_one_keeps_the_pointers() {
    let names = ["Width", "Height"];
    assert_eq!(numbers(&names, &typed("3000", "2000"), MM), Ok(vec![Some(3000.0), Some(2000.0)]));
    assert_eq!(numbers(&names, &typed("3000", ""), MM), Ok(vec![Some(3000.0), None]));
    assert_eq!(numbers(&["Angle"], &typed("90°", ""), MM), Ok(vec![Some(90.0)]));
    assert_eq!(numbers(&["Radius"], &typed("2", ""), Units::Metres), Ok(vec![Some(2000.0)]), "a bare number is in the project's unit");
    for bad in ["x", "0", "-5", "1e999", "99999999999"] {
        assert!(numbers(&["Radius"], &typed(bad, ""), MM).is_err(), "{bad} is no length");
    }
    for bad in ["x", "0", "400", "1e999"] {
        assert!(numbers(&["Angle"], &typed(bad, ""), MM).is_err(), "{bad} is no angle");
    }
}

#[test]
fn the_reading_names_each_number_and_marks_the_field_tab_is_on() {
    let r = shape_reading(&["Width", "Height"], &typed("30", ""));
    assert!(r.contains("Width 30") && r.contains("Height …") && r.contains("Tab"), "{r}");
    let on_height = shape_reading(&["Width", "Height"], &Typed { field: Field::Angle, ..typed("30", "") });
    assert!(on_height.contains("Height _"), "{on_height}");
    let one = shape_reading(&["Radius"], &typed("", ""));
    assert!(one.contains("Radius _") && !one.contains("Tab"), "a shape with one number has no field to go to: {one}");
}
