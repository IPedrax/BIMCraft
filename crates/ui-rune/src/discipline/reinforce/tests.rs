//! Reinforce, with no window: the fields a host has, what they make of the command's parameters, the refusals in words, the presets.

use bimcraft_core::model::Element;

use super::*;
use crate::app::App;

/// A beam on the sample's level, far from the house.
fn beam(app: &mut App) -> Id {
    let level = app.level.expect("a level");
    let made = app.session.execute("beam", &json!({"points": [[-30000.0, -30000.0], [-26000.0, -30000.0]], "level": level}));
    assert!(made.is_ok(), "{made:?}");
    app.session.model.elements.iter().filter(|(_, e)| matches!(e, Element::Beam(_))).map(|(id, _)| *id).max().expect("a beam")
}

fn put(d: &mut Reinforce, key: &str, text: &str) {
    d.put(key, text.to_string());
}

/// The names of every box on the dialog, as a screen reader says them.
fn names(d: &Reinforce) -> Vec<String> {
    let cells = d.sections().into_iter().flat_map(|s| s.rows).flatten();
    cells.filter_map(|c| if let Cell::Field { name, .. } = c { Some(name) } else { None }).collect()
}

#[test]
fn it_opens_on_one_kind_of_host_and_says_why_it_will_not_on_anything_else() {
    let mut app = App::sample();
    let id = beam(&mut app);
    let m = &*app.session.model;
    let wall = m.elements.iter().find_map(|(id, e)| matches!(e, Element::Wall(_)).then_some(*id)).expect("a wall");
    assert!(Reinforce::open(m, &[]).is_err(), "nothing selected");
    assert!(Reinforce::open(m, &[wall]).is_err(), "a wall is not armed");
    assert!(Reinforce::open(m, &[id, wall]).is_err(), "a beam and a wall are not one kind");
    let d = Reinforce::open(m, &[id]).unwrap_or_else(|why| panic!("{why}"));
    assert!(d.title().contains(&id.to_string()), "{}", d.title());
    let fields = names(&d);
    for want in ["Bottom bars: Count", "Stirrups: Spacing", "Cover", "Cover: Top", "Steel: Grade", "Check: fck", "Preset: Name"] {
        assert!(fields.iter().any(|n| n == want), "a box named {want}: {fields:?}");
    }
    assert!(!d.foot().iter().any(|f| f.action == "remove"), "no bars made yet, none to remove");
}

#[test]
fn the_fields_filled_are_the_parameters_and_one_that_is_no_number_names_its_group() {
    let mut app = App::sample();
    let id = beam(&mut app);
    let m = &*app.session.model;
    let mut d = Reinforce::open(m, &[id]).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(d.params(), Ok(json!({"hosts": [id]})), "nothing typed, nothing but the host");
    put(&mut d, "bottom.count", "3");
    put(&mut d, "bottom.diameter", "16");
    put(&mut d, "stirrup.spacing", "150");
    put(&mut d, "cover", "25");
    let spacing = m.units.parse("150").expect("a length");
    let cover = m.units.parse("25").expect("a length");
    let p = d.params().unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(p["bottom"], json!({"count": 3, "diameter": 16.0}));
    assert_eq!(p["stirrup"], json!({"spacing": spacing}));
    assert_eq!(p["cover"], json!(cover), "a cover alone is every face's");
    put(&mut d, "cover_top", "30");
    let top = m.units.parse("30").expect("a length");
    assert_eq!(
        d.params().map(|p| p["cover"].clone()),
        Ok(json!({"top": top, "bottom": cover, "sides": cover})),
        "a face typed over the rest"
    );
    put(&mut d, "bottom.count", "x");
    let why = d.params().err().unwrap_or_default();
    assert!(why.contains("Bottom bars: Count") && why.contains('x'), "{why}");
    put(&mut d, "bottom.count", "3");
    put(&mut d, "stirrup.spacing", "far");
    assert!(d.params().err().unwrap_or_default().contains("Stirrups: Spacing"));
}

#[test]
fn the_check_takes_the_fields_of_its_standard_and_leaves_the_other_ones_out() {
    let mut app = App::sample();
    let id = beam(&mut app);
    let m = &*app.session.model;
    let mut d = Reinforce::open(m, &[id]).unwrap_or_else(|why| panic!("{why}"));
    for (key, text) in [("aggressiveness", "2"), ("exposure", "XC1"), ("fck", "30"), ("fyk", "500")] {
        put(&mut d, key, text);
    }
    assert!(names(&d).iter().any(|n| n == "Check: Aggressiveness class") && !names(&d).iter().any(|n| n == "Check: Exposure class"));
    let p = d.params().unwrap_or_else(|why| panic!("{why}"));
    assert_eq!((&p["aggressiveness"], &p["fck"]), (&json!(2), &json!(30.0)));
    assert!(p.get("exposure").is_none() && p.get("fyk").is_none() && p.get("check").is_none(), "{p}");
    d.choose(1, 2, &mut app);
    assert!(names(&d).iter().any(|n| n == "Check: Exposure class") && names(&d).iter().any(|n| n == "Check: fyk"));
    let p = d.params().unwrap_or_else(|why| panic!("{why}"));
    assert_eq!((&p["check"], &p["exposure"], &p["fyk"]), (&json!("EN 1992-1-1"), &json!("XC1"), &json!(500.0)));
    assert!(p.get("aggressiveness").is_none(), "the other standard's field is left out: {p}");
    assert_eq!(d.choice(1).and_then(|c| c.current), Some(2));
}

#[test]
fn ok_makes_the_bars_through_the_command_and_a_field_that_is_wrong_keeps_the_dialog_open() {
    let mut app = App::sample();
    let id = beam(&mut app);
    let mut d = Reinforce::open(&app.session.model, &[id]).unwrap_or_else(|why| panic!("{why}"));
    put(&mut d, "bottom.count", "x");
    assert_eq!(d.press("ok", 0, &mut app), Outcome::Keep);
    assert!(d.error().is_some_and(|e| e.contains("Bottom bars")), "{:?}", d.error());
    for (key, text) in [
        ("bottom.count", "2"),
        ("bottom.diameter", "16"),
        ("top.count", "2"),
        ("top.diameter", "12.5"),
        ("stirrup.diameter", "8"),
        ("stirrup.spacing", "200"),
        ("cover", "25"),
    ] {
        put(&mut d, key, text);
    }
    let before = app.session.model.elements.iter().filter(|(_, e)| matches!(e, Element::Rebar(_))).count();
    let out = d.press("ok", 0, &mut app);
    assert!(d.error().is_none(), "the command's words: {:?}", d.error());
    assert_eq!(out, Outcome::Close);
    let after = app.session.model.elements.iter().filter(|(_, e)| matches!(e, Element::Rebar(_))).count();
    assert!(after > before, "bars were made: {before} {after}");
    assert!(app.message.as_deref().is_some_and(|m| m.starts_with("Reinforced")), "{:?}", app.message);
    // Open again: the bars it made are there to take away, and Remove Bars does.
    let mut d = Reinforce::open(&app.session.model, &[id]).unwrap_or_else(|why| panic!("{why}"));
    assert!(d.foot().iter().any(|f| f.action == "remove"));
    assert_eq!(d.press("remove", 0, &mut app), Outcome::Close);
    let left = app.session.model.elements.iter().filter(|(_, e)| matches!(e, Element::Rebar(_))).count();
    assert_eq!(left, before, "the bars the command made are gone");
}

#[test]
fn a_preset_saves_the_fields_loads_them_again_and_is_deleted() {
    let mut app = App::sample();
    let id = beam(&mut app);
    let mut d = Reinforce::open(&app.session.model, &[id]).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(d.press("save_preset", 0, &mut app), Outcome::Keep);
    assert!(d.error().is_some_and(|e| e.contains("name")), "a name first: {:?}", d.error());
    put(&mut d, "bottom.count", "3");
    put(&mut d, "bottom.diameter", "12.5");
    put(&mut d, "save_as", "My beam");
    d.press("save_preset", 0, &mut app);
    assert!(d.error().is_none(), "{:?}", d.error());
    let list = d.choice(0).expect("the preset list");
    assert_eq!(list.options, ["My beam"]);
    assert_eq!(list.current, Some(0), "the preset saved is the one in force");
    put(&mut d, "bottom.count", "");
    put(&mut d, "bottom.diameter", "");
    d.choose(0, 0, &mut app);
    assert_eq!((d.text("bottom.count").as_str(), d.text("bottom.diameter").as_str()), ("3", "12.5"), "the fields are the preset's again");
    d.press("delete_preset", 0, &mut app);
    assert!(d.choice(0).is_some_and(|c| c.options.is_empty() && c.current.is_none()), "deleted");
}
