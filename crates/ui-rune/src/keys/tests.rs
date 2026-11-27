use rune::{Key, Mods};

use super::*;

fn run(typed: &mut Typed, text: &str, mut now: f64) -> Vec<Step> {
    text.chars()
        .map(|c| {
            now += 0.1;
            typed.push(c, now)
        })
        .collect()
}

#[test]
fn two_letters_make_a_shortcut_and_the_first_waits_for_the_second() {
    let mut t = Typed::default();
    assert_eq!(run(&mut t, "WA", 0.0), [Step::Waiting, Step::Run(Target::Command("wall"))]);
    assert!(t.is_empty(), "a shortcut that ran leaves nothing waiting");
    assert_eq!(run(&mut t, "zf", 1.0), [Step::Waiting, Step::Run(Target::Desktop("Zoom to Fit"))], "in any case");
    assert_eq!(run(&mut t, "3d", 2.0), [Step::Waiting, Step::Run(Target::Desktop("3D View"))], "a digit begins one too");
    t.push('W', 5.0);
    assert_eq!(t.text(), "W", "what waits is shown upper case");
}

#[test]
fn a_second_letter_that_makes_nothing_drops_the_first_and_waits_in_its_place() {
    let mut t = Typed::default();
    assert_eq!(run(&mut t, "YWA", 0.0), [Step::Nothing, Step::Waiting, Step::Run(Target::Command("wall"))], "Y begins none, then WA");
    assert_eq!(
        run(&mut t, "WZF", 1.0),
        [Step::Waiting, Step::Waiting, Step::Run(Target::Desktop("Zoom to Fit"))],
        "WZ is none: Z begins ZF"
    );
    assert_eq!(run(&mut t, "WW", 2.0).last(), Some(&Step::Waiting), "the second W waits in the first's place");
    assert_eq!(t.text(), "W");
}

#[test]
fn a_letter_that_waited_too_long_starts_afresh_and_time_drops_what_waits() {
    let mut t = Typed::default();
    assert_eq!(t.push('W', 0.0), Step::Waiting);
    assert_eq!(t.push('A', WAIT + 0.5), Step::Waiting, "the A is the first of a new one: WA would have run");
    assert_eq!(t.text(), "A");
    assert!(t.due(WAIT + 0.5).is_some_and(|d| (d - WAIT).abs() < 1e-9));
    assert!(!t.expire(WAIT + 1.0) && t.expire(2.0 * WAIT + 1.0) && t.is_empty(), "it is dropped once it has waited");
    assert!(!t.expire(100.0) && t.due(0.0).is_none(), "nothing waits, nothing to drop");
    // A clock that is no number cannot hold a letter.
    assert_eq!(t.push('W', f64::NAN), Step::Waiting);
    assert_eq!(t.push('A', f64::NAN), Step::Waiting, "and a wait that cannot be measured is over");
}

#[test]
fn a_key_that_is_not_a_letter_or_digit_drops_what_waits_and_is_nothing() {
    let mut t = Typed::default();
    t.push('W', 0.0);
    assert_eq!(t.push('-', 0.1), Step::Nothing);
    assert!(t.is_empty());
    assert_eq!(t.push('\u{44b}', 0.2), Step::Nothing, "a letter of another alphabet is no shortcut");
    t.push('Z', 0.3);
    t.clear();
    assert!(t.is_empty());
}

#[test]
fn a_chord_is_written_as_the_core_table_writes_it() {
    let m = |cmd, shift, alt| Mods { cmd, shift, alt, ..Mods::default() };
    assert_eq!(written(&Key::Command('z'), m(true, false, false)).as_deref(), Some("Ctrl+Z"));
    assert_eq!(written(&Key::Command('z'), m(true, true, false)).as_deref(), Some("Ctrl+Shift+Z"));
    assert_eq!(written(&Key::Command(','), m(true, false, false)).as_deref(), Some("Ctrl+,"));
    assert_eq!(written(&Key::Function(3), Mods::default()).as_deref(), Some("F3"));
    assert_eq!(written(&Key::Delete, Mods::default()).as_deref(), Some("Delete"));
    assert_eq!(written(&Key::Function(8), m(false, true, false)).as_deref(), Some("Shift+F8"));
    assert_eq!(written(&Key::Escape, Mods::default()), None);
    assert_eq!(written(&Key::Command(' '), m(true, false, false)), None);
}
