//! A stair type's dialog, with no window: the type of the stair selected, the sizes that apply to what carries it, the rules ticked, what is
//! sent to the `type` command and the refusals in words.

use bimcraft_core::model::Element;

use super::*;
use crate::app::App;

/// A stair on the sample's level, far from the house; its id.
fn stair(app: &mut App) -> Id {
    let level = app.level.expect("a level");
    let made = app.session.execute("stair", &json!({"start": [-30000.0, -30000.0], "end": [-26000.0, -30000.0], "level": level}));
    assert!(made.is_ok(), "{made:?}");
    app.session.model.elements.iter().filter(|(_, e)| matches!(e, Element::Stair(_))).map(|(id, _)| *id).max().expect("a stair")
}

fn names(d: &StairType) -> Vec<String> {
    let cells = d.sections().into_iter().flat_map(|s| s.rows).flatten();
    cells.filter_map(|c| if let Cell::Field { name, .. } = c { Some(name) } else { None }).collect()
}

fn type_in(d: &mut StairType, i: usize, text: &str) {
    if let Some(e) = d.edit_mut(i) {
        *e = TextEdit::new(text);
    }
}

#[test]
fn it_opens_on_the_type_of_the_stair_selected_and_says_what_to_select_otherwise() {
    let mut app = App::sample();
    let id = stair(&mut app);
    let m = &*app.session.model;
    assert!(StairType::open(m, &[]).err().unwrap_or_default().contains("Select one stair"));
    let wall = m.elements.iter().find_map(|(id, e)| matches!(e, Element::Wall(_)).then_some(*id)).expect("a wall");
    assert!(StairType::open(m, &[wall]).is_err(), "a wall has no stair type");
    let d = StairType::open(m, &[id]).unwrap_or_else(|why| panic!("{why}"));
    assert!(d.title().contains(&d.name), "{}", d.title());
    assert!(!d.sections().is_empty() && d.foot().len() == 3);
}

#[test]
fn the_sizes_shown_are_the_ones_that_apply_to_what_carries_the_treads() {
    let mut app = App::sample();
    let id = stair(&mut app);
    let mut d = StairType::open(&app.session.model, &[id]).unwrap_or_else(|why| panic!("{why}"));
    let has = |d: &StairType, name: &str| names(d).iter().any(|n| n == name);
    d.choose(CONSTRUCTION, 0, &mut app);
    assert_eq!(d.construction(), "monolithic");
    assert!(has(&d, "Slab Thickness") && !has(&d, "Tread Thickness") && !has(&d, "Stringer Width"), "{:?}", names(&d));
    d.choose(CONSTRUCTION, 1, &mut app);
    assert_eq!(d.construction(), "stringers");
    assert!(
        has(&d, "Tread Thickness")
            && has(&d, "Stringer Width")
            && has(&d, "Centre Stringer Width (Optional)")
            && !has(&d, "Slab Thickness")
    );
    assert!(!has(&d, "Finish Side Overhang"), "stringers carry the sides");
    d.choose(CONSTRUCTION, 2, &mut app);
    assert!(has(&d, "Centre Stringer Width") && !has(&d, "Stringer Width") && has(&d, "Finish Side Overhang"), "{:?}", names(&d));
    let p = d.params().unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(
        (&p["construction"], &p["profile"], p.get("waist").is_none()),
        (&json!("carriage"), &json!("square"), true),
        "sizes that do not apply are not sent"
    );
}

#[test]
fn a_rule_ticked_is_a_number_the_types_own_and_one_that_is_not_is_null_and_a_bad_number_names_its_rule() {
    let mut app = App::sample();
    let id = stair(&mut app);
    let mut d = StairType::open(&app.session.model, &[id]).unwrap_or_else(|why| panic!("{why}"));
    let p = d.params().unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(p["rules"].as_object().map(|r| r.len()), Some(14), "every key of the eight rules is sent");
    assert!(p["rules"]["riser_max"].is_null(), "a rule left off is the building code's: {}", p["rules"]);
    d.press("check", TICK, &mut app);
    type_in(&mut d, RULE_EDITS, "170");
    let want = app.session.model.units.parse("170").expect("a length");
    assert_eq!(d.params().map(|p| p["rules"]["riser_min"].clone()), Ok(json!(want)));
    type_in(&mut d, RULE_EDITS, "tall");
    let why = d.params().err().unwrap_or_default();
    assert!(why.contains("Riser Height") && why.contains("tall"), "{why}");
    d.press("check", TICK, &mut app);
    assert!(d.params().is_ok(), "a rule off is not read");
    // A ratio is a number, a pitch is in degrees.
    d.press("check", TICK + 3 * 2, &mut app);
    type_in(&mut d, RULE_EDITS + 3 * 2, "1,5");
    d.press("check", TICK + 5 * 2 + 1, &mut app);
    type_in(&mut d, RULE_EDITS + 5 * 2 + 1, "42\u{b0}");
    let p = d.params().unwrap_or_else(|why| panic!("{why}"));
    assert_eq!((&p["rules"]["ratio_min"], &p["rules"]["pitch_max"]), (&json!(1.5), &json!(42.0)));
}

#[test]
fn ok_changes_the_type_through_the_command_and_a_list_chooses_the_core_s_word() {
    let mut app = App::sample();
    let id = stair(&mut app);
    let mut d = StairType::open(&app.session.model, &[id]).unwrap_or_else(|why| panic!("{why}"));
    let ways = d.choice(PLAN).expect("the ways it shows in plan");
    assert_eq!(ways.options.len(), 10);
    d.choose(PLAN, 9, &mut app);
    d.choose(ARROW, 3, &mut app);
    d.press("check", NUMBERS, &mut app);
    let at = SIZES.iter().position(|(k, _)| *k == "tread_cover").expect("a size");
    type_in(&mut d, at, "20");
    let name = d.name.clone();
    assert_eq!(d.press("ok", 0, &mut app), Outcome::Close, "{:?}", d.error());
    let kind = app.session.model.types.stair.get(&name).expect("the type").clone();
    let json = serde_json::to_value(&kind).expect("a type");
    assert_eq!((json["plan"]["below"].as_str(), json["arrow"].as_str(), kind.numbers), (Some("not_shown"), Some("tick"), true));
    assert_eq!(kind.tread_cover, app.session.model.units.parse("20").expect("a length"));
    assert!(app.undo(), "one undo step");
    assert_eq!(app.session.model.types.stair.get(&name).map(|k| k.numbers), Some(false));
}
