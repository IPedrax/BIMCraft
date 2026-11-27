//! The Options Bar's logic over the sample house: what each tool's bar holds, what its settings do to the params of a click, the types
//! picked, and what a bad field refuses.

use bimcraft_core::engine::Session;
use bimcraft_core::model::{Justify, Location, Model};
use serde_json::{Value, json};

use super::*;
use crate::project::{self, Source};
use crate::tooling::SPECS;

fn house() -> Session {
    project::load(&Source::Sample).0
}

fn level_of(m: &Model) -> Id {
    m.levels().first().map(|(id, _)| *id).unwrap_or_default()
}

fn fed(s: &Settings, tool: &str, m: &Model, params: Value) -> Result<Value, String> {
    let mut params = params;
    s.feed(tool, m, level_of(m), &mut params, 100.0).map(|_| params)
}

#[test]
fn every_tool_with_a_bar_is_a_tool_of_the_palette_and_its_items_are_made_for_the_house() {
    let session = house();
    let m = &*session.model;
    let level = level_of(m);
    let settings = Settings::default();
    let mut with_bars = 0;
    for spec in SPECS {
        let items = items(spec.id, m, level, &settings);
        assert_eq!(items.is_empty(), controls(spec.id).is_empty(), "{}", spec.id);
        with_bars += usize::from(!items.is_empty());
        for item in &items {
            assert!(!item.key.is_empty(), "{}", spec.id);
        }
        for key in field_keys(spec.id) {
            assert!(!default_text(key, m.units).is_empty(), "{key} has a default");
        }
    }
    assert!(with_bars >= 8, "the bar has tools: {with_bars}");
    assert_eq!(category_of("wall"), Some("wall"));
    assert_eq!(category_of("structural_column"), Some("structuralcolumn"));
    assert_eq!(category_of("plumbing_fixture"), Some("component"));
    assert_eq!(category_of("move"), None);
}

#[test]
fn a_walls_params_take_the_bars_height_location_line_and_type() {
    let session = house();
    let m = &*session.model;
    let level = level_of(m);
    let points = json!({"points": [[0, 0], [1000, 0]], "level": level});
    let mut s = Settings::default();
    let above = m.level_above(level).and_then(|g| m.elements.id(g));
    let p = fed(&s, "wall", m, points.clone()).unwrap_or_default();
    assert_eq!(p["top_level"], json!(above), "up to the level above unless another is picked");
    assert_eq!(p["location"], json!("centre"));
    assert!(p.get("type").is_none(), "no type picked: the command's own");

    assert!(s.choose(m, level, "wall.location", 2));
    assert_eq!(s.location, Location::ALL[2]);
    assert!(!s.choose(m, level, "wall.location", 99));
    s.tops.insert("wall", None);
    s.texts.insert("wall.height", "2500".to_string());
    let p = fed(&s, "wall", m, points.clone()).unwrap_or_default();
    assert_eq!(p["height"], json!(2500.0), "unconnected, at the height typed");
    assert!(p.get("top_level").is_none());
    assert_eq!(p["location"], json!(Location::ALL[2].name()));

    s.texts.insert("wall.height", "tall".to_string());
    let why = fed(&s, "wall", m, points.clone()).err().unwrap_or_default();
    assert!(why.contains("tall") && why.contains("height"), "{why}");

    let own = m.type_names("wall").into_iter().next().unwrap_or_default().to_string();
    s.texts.remove("wall.height");
    s.picks.insert("wall", TypePick { name: own.clone(), family: None });
    assert_eq!(fed(&s, "wall", m, points.clone()).unwrap_or_default()["type"], json!(own));
    s.picks.insert("wall", TypePick { name: "No Such Wall".into(), family: None });
    assert!(fed(&s, "wall", m, points).unwrap_or_default().get("type").is_none(), "a type the project has not is no pick");
}

#[test]
fn a_run_ceiling_roof_and_stair_take_their_fields() {
    let session = house();
    let m = &*session.model;
    let mut s = Settings { justify: Justify::Bottom, ..Settings::default() };
    s.texts.insert("run.offset", "150".into());
    assert_eq!(fed(&s, "beam", m, json!({"points": []})).unwrap_or_default(), json!({"points": [], "justify": "bottom", "offset": 150.0}));
    s.texts.insert("ceiling.height", "2400".into());
    assert_eq!(fed(&s, "ceiling", m, json!({"point": [1, 1]})).unwrap_or_default()["offset"], json!(2400.0));
    s.texts.insert("roof.slope", "45".into());
    s.gable = true;
    let roof = fed(&s, "roof", m, json!({"level": 1})).unwrap_or_default();
    assert_eq!((roof["slope"].clone(), roof["gable"].clone(), roof["overhang"].clone()), (json!(45.0), json!(true), json!(500.0)));
    s.texts.insert("roof.slope", "100".into());
    assert!(fed(&s, "roof", m, json!({"level": 1})).is_err(), "a slope of 100 degrees is none");
    s.texts.insert("stair.width", "1200".into());
    assert!(s.choose(m, level_of(m), "stair.line", 0));
    let stair = fed(&s, "stair", m, json!({"start": [0, 0]})).unwrap_or_default();
    assert_eq!((stair["width"].clone(), stair["line"].clone()), (json!(1200.0), json!("left")));
    assert!(fed(&s, "move", m, json!({"ids": [1]})).unwrap_or_default().get("justify").is_none(), "a tool with no bar is not touched");
    assert_eq!(fed(&s, "ceiling", m, json!(3)).unwrap_or_default(), json!(3), "params that are no object are left alone");
}

#[test]
fn chain_is_the_bars_for_the_tools_that_have_it_and_a_check_sets_it() {
    let session = house();
    let m = &*session.model;
    let mut s = Settings::default();
    assert!(s.chains("wall") && s.chains("beam"));
    assert!(s.check("chain", false));
    assert!(!s.chains("wall") && !s.chains("beam") && !s.chains("pipe"));
    assert!(s.chains("door"), "a door has no chain to turn off");
    assert!(!s.check("nope", true));
    let wall = items("wall", m, level_of(m), &s);
    assert!(matches!(wall.iter().find(|i| i.key == "chain").map(|i| &i.editor), Some(Editor::Check(false))));
}

#[test]
fn the_wall_bars_height_is_a_list_of_the_levels_above_and_unconnected_with_a_field_only_then() {
    let session = house();
    let m = &*session.model;
    let level = level_of(m);
    let mut s = Settings::default();
    let bar = items("wall", m, level, &s);
    let keys: Vec<&str> = bar.iter().map(|i| i.key).collect();
    assert_eq!(keys, ["wall.top", "wall.location", "chain"]);
    let Some(Editor::List { now, options }) = bar.first().map(|i| i.editor.clone()) else { panic!("a list") };
    assert_eq!(options.last().map(String::as_str), Some("Unconnected"));
    assert!(options.len() >= 2, "the house has a level above the first");
    assert_eq!(now, options[0], "the level above is the default");
    assert!(s.choose(m, level, "wall.top", options.len() - 1));
    let keys: Vec<String> = items("wall", m, level, &s).iter().map(|i| i.key.to_string()).collect();
    assert_eq!(keys, ["wall.top", "wall.height", "wall.location", "chain"]);
    assert!(!s.choose(m, level, "wall.top", 99));
}

#[test]
fn a_door_takes_the_type_picked_and_the_family_of_a_family_type_and_a_family_tool_only_its_own_kind() {
    let session = house();
    let m = &*session.model;
    let mut s = Settings::default();
    s.picks.insert("door", TypePick { name: "Single 800 x 2100".into(), family: None });
    let own = fed(&s, "door", m, json!({"at": [0, 0]})).unwrap_or_default();
    assert_eq!((own["type"].clone(), own.get("family").is_none()), (json!("Single 800 x 2100"), true));
    s.picks.insert("door", TypePick { name: "900 x 2100".into(), family: Some("Single Door".into()) });
    let family = fed(&s, "door", m, json!({"at": [0, 0]})).unwrap_or_default();
    assert_eq!((family["type"].clone(), family["family"].clone()), (json!("900 x 2100"), json!("Single Door")));
    // A door's family is not a plumbing fixture: that tool keeps what its machine chose.
    s.picks.insert("component", TypePick { name: "900 x 2100".into(), family: Some("Single Door".into()) });
    let fixture = fed(&s, "plumbing_fixture", m, json!({"family": "Basin", "type": "Wall", "at": [0, 0]})).unwrap_or_default();
    assert_eq!(fixture["family"], json!("Basin"));
}

#[test]
fn a_door_and_a_window_are_centred_on_their_wall_and_take_their_hinge_and_face_and_space_flips_them() {
    let session = house();
    let m = &*session.model;
    let level = level_of(m);
    let mut s = Settings::default();
    let at = json!({"at": [3000, 0], "level": level});
    let door = fed(&s, "door", m, at.clone()).unwrap_or_default();
    assert!(door["flip_facing"].is_boolean(), "a door takes the pointer's side: {door}");
    assert_eq!(door["flip_hand"], json!(false), "a door that swings takes its hinge");
    let window = fed(&s, "window", m, at.clone()).unwrap_or_default();
    assert!(window["flip_facing"].is_boolean() && window.get("flip_hand").is_none(), "{window}");
    s.flip = true;
    let flipped = fed(&s, "door", m, at.clone()).unwrap_or_default();
    assert_eq!((flipped["flip_hand"].clone(), flipped["flip_facing"].clone()), (json!(true), door["flip_facing"].clone()));
    let turned = fed(&s, "window", m, at).unwrap_or_default();
    assert_ne!(turned["flip_facing"], window["flip_facing"], "Space turns a window the other way");
    assert!(fed(&Settings::default(), "door", m, json!({"at": [-30000, -30000], "level": level})).is_ok(), "no wall there: left as it is");
}
