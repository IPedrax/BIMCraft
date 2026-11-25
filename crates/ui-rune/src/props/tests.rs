//! The palette's logic over real projects, with no window: the sample house and a tower with its drawings, every category they have,
//! one element and several, a mixed selection and the plan's own view; and an edit carried to the core and read back.

use bimcraft_core::engine::Session;
use bimcraft_core::model::{Id, Model};
use bimcraft_core::sample::tower::{Drawings, Plate, drawn};
use serde_json::json;

use super::apply::{self, Edit};
use super::kind::{Kind, MAX_VALUE};
use super::panel::{Editor, Panel, Subject};
use crate::project::{self, Source};

fn house() -> Session {
    project::load(&Source::Sample).0
}

fn tower() -> Model {
    drawn(Plate::SMALL, 2, Drawings { annotation: true, sheets: true }).expect("the tower builds")
}

/// The ids of the first `n` elements of each category of `m`.
fn some_of_each(m: &Model, n: usize) -> Vec<(&'static str, Vec<Id>)> {
    let mut found: Vec<(&'static str, Vec<Id>)> = Vec::new();
    for (id, e) in m.elements.iter() {
        let category = e.category();
        match found.iter_mut().find(|(c, _)| *c == category) {
            Some((_, ids)) if ids.len() < n => ids.push(*id),
            Some(_) => {}
            None => found.push((category, vec![*id])),
        }
    }
    found
}

fn first_of(m: &Model, category: &str) -> Option<Id> {
    m.elements.iter().find(|(_, e)| e.category() == category).map(|(id, _)| *id)
}

fn text_of(panel: &Panel, key: &str) -> Option<String> {
    panel.row(key).map(|r| r.shown.clone())
}

/// Every row of `panel` edited as it stands: what was typed over with itself changes nothing, and a check or a list gives a command or
/// none, never a refusal of the edit itself.
fn edit_every_row(panel: &Panel, m: &Model) {
    for row in panel.all_rows() {
        let key = row.key;
        match &row.editor {
            Editor::Text { .. } => assert_eq!(apply::command(panel, m.units, key, Edit::Text(&row.shown)), Ok(None), "{key}"),
            Editor::Check(on) => {
                let c = apply::command(panel, m.units, key, Edit::Check(!on));
                // A crop is one view's.
                if row.kind == Kind::Crop && panel.ids.len() > 1 {
                    assert!(c.is_err(), "{key}");
                } else {
                    assert!(matches!(c, Ok(Some(_))), "{key}: {c:?}");
                }
            }
            Editor::Choice { options, .. } => {
                for i in 0..options.len() {
                    assert!(apply::command(panel, m.units, key, Edit::Choice(i)).is_ok(), "{key} {i}");
                }
            }
            Editor::Read | Editor::Link(_) | Editor::Seam(_) => {
                assert!(apply::command(panel, m.units, key, Edit::Text("x")).is_err(), "{key} is not edited here");
            }
        }
    }
}

#[test]
fn every_category_of_the_house_and_the_tower_has_a_palette_that_shows_and_edits_without_a_fault() {
    let house = house();
    let tower = tower();
    for m in [&*house.model, &tower] {
        let mut rows = 0;
        for (category, ids) in some_of_each(m, 3) {
            // One element, several of the category, and the selection as a whole.
            for selection in [vec![ids[0]], ids.clone()] {
                let panel = Panel::of(m, &selection, None, None);
                assert!(!panel.title.is_empty(), "{category}");
                assert_eq!(panel.ids, selection, "{category}");
                for row in panel.all_rows() {
                    assert!(row.shown.chars().count() <= MAX_VALUE * 4, "{category} {}: a row is a line, not a page", row.key);
                    assert!(!row.label.is_empty(), "{category} {}", row.key);
                    if matches!(row.kind, Kind::Shown | Kind::LengthShown) {
                        assert_eq!(row.editor, Editor::Read, "{category} {}: derived rows are read only", row.key);
                    }
                }
                rows += panel.all_rows().count();
                edit_every_row(&panel, m);
            }
        }
        assert!(rows > 100, "the palette has rows for what a project holds: {rows}");
    }
}

#[test]
fn a_wall_shows_its_groups_with_a_list_of_the_levels_a_field_in_millimetres_and_derived_rows_that_cannot_be_typed() {
    let session = house();
    let m = &*session.model;
    let Some(wall) = first_of(m, "wall") else { panic!("the house has walls") };
    let panel = Panel::of(m, &[wall], None, None);
    assert_eq!(panel.subject, Subject::Element(wall));
    assert_eq!(panel.title, "Walls (1)");
    let groups: Vec<&str> = panel.groups.iter().map(|g| g.key).collect();
    assert!(groups.starts_with(&["Constraints"]) && groups.contains(&"Dimensions"), "{groups:?}");
    let levels: Vec<String> = m.levels().iter().map(|(_, l)| l.name.clone()).collect();
    match panel.row("level").map(|r| &r.editor) {
        Some(Editor::Choice { current, options }) => {
            let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
            assert_eq!(labels, levels.iter().map(String::as_str).collect::<Vec<_>>());
            assert!(current.is_some(), "the wall's level is one of them");
        }
        other => panic!("the wall's level is a list: {other:?}"),
    }
    assert!(matches!(panel.row("location").map(|r| &r.editor), Some(Editor::Choice { options, .. }) if options.len() == 6));
    assert!(text_of(&panel, "base_offset").is_some_and(|t| t.ends_with(" mm")), "a length with its unit");
    assert!(matches!(panel.row("base_offset").map(|r| &r.editor), Some(Editor::Text { .. })));
    assert!(text_of(&panel, "length").is_some_and(|t| t.ends_with(" mm")));
    assert_eq!(panel.row("length").map(|r| &r.editor), Some(&Editor::Read));
    assert!(matches!(panel.row("room_bounding").map(|r| &r.editor), Some(Editor::Check(_))));
}

#[test]
fn an_edit_goes_through_the_engine_and_the_palette_reads_it_back() {
    let mut session = house();
    let Some(wall) = first_of(&session.model, "wall") else { panic!("the house has walls") };
    let panel = Panel::of(&session.model, &[wall], None, None);
    let units = session.model.units;
    let Ok(Some(command)) = apply::command(&panel, units, "base_offset", Edit::Text("150")) else { panic!("150 is an offset") };
    assert_eq!(command.params, json!({"base_offset": 150.0, "ids": [wall]}));
    assert!(session.execute(command.name, &command.params).is_ok());
    let again = Panel::of(&session.model, &[wall], None, None);
    assert_eq!(text_of(&again, "base_offset").as_deref(), Some("150 mm"));
    // A check box.
    let Ok(Some(off)) = apply::command(&again, units, "room_bounding", Edit::Check(false)) else { panic!("a flag") };
    assert!(session.execute(off.name, &off.params).is_ok());
    assert!(matches!(Panel::of(&session.model, &[wall], None, None).row("room_bounding").map(|r| &r.editor), Some(Editor::Check(false))));
}

#[test]
fn several_elements_show_what_they_share_and_blank_where_they_differ_and_an_edit_changes_all_of_them() {
    let mut session = house();
    let walls: Vec<Id> = some_of_each(&session.model, 2).into_iter().find(|(c, _)| *c == "wall").map(|(_, ids)| ids).unwrap_or_default();
    assert_eq!(walls.len(), 2, "the house has two walls or more");
    let units = session.model.units;
    let same = Panel::of(&session.model, &walls, None, None);
    assert_eq!(same.subject, Subject::Elements("wall"));
    assert_eq!(same.title, "Walls (2)");
    assert_eq!(text_of(&same, "base_offset").as_deref(), Some("0 mm"), "they share it");
    let Ok(Some(one)) = apply::command(&Panel::of(&session.model, &walls[..1], None, None), units, "base_offset", Edit::Text("40")) else {
        panic!("an offset")
    };
    assert!(session.execute(one.name, &one.params).is_ok());
    let differ = Panel::of(&session.model, &walls, None, None);
    assert_eq!(text_of(&differ, "base_offset").as_deref(), Some(""), "blank where they differ");
    assert_eq!(differ.row("base_offset").map(|r| &r.editor), Some(&Editor::Text { hint: "" }));
    let Ok(Some(all)) = apply::command(&differ, units, "base_offset", Edit::Text("75")) else { panic!("an offset") };
    assert_eq!(all.params["ids"], json!(walls));
    assert!(session.execute(all.name, &all.params).is_ok());
    assert_eq!(text_of(&Panel::of(&session.model, &walls, None, None), "base_offset").as_deref(), Some("75 mm"));
}

#[test]
fn a_mixed_selection_lists_its_categories_and_a_filter_narrows_what_the_palette_shows() {
    let session = house();
    let m = &*session.model;
    let (Some(wall), Some(door)) = (first_of(m, "wall"), first_of(m, "door")) else { panic!("walls and doors") };
    let mixed = Panel::of(m, &[wall, door], None, None);
    assert_eq!(mixed.subject, Subject::Mixed(2));
    assert_eq!(mixed.title, "Multiple Categories (2)");
    assert!(mixed.groups.is_empty());
    assert_eq!(mixed.categories, vec![("door", 1), ("wall", 1)]);
    let narrowed = Panel::of(m, &[wall, door], Some("wall"), None);
    assert_eq!(narrowed.subject, Subject::Element(wall));
    assert_eq!(narrowed.ids, vec![wall], "an edit changes the walls only");
    assert!(!narrowed.groups.is_empty());
    assert_eq!(narrowed.categories.len(), 2, "the filter still lists both");
    let nothing_of_it = Panel::of(m, &[wall, door], Some("roof"), None);
    assert_eq!(nothing_of_it.subject, Subject::Mixed(2), "a filter that matches none leaves the selection");
}

#[test]
fn nothing_selected_shows_the_plans_own_view_and_an_element_that_is_gone_shows_nothing() {
    let mut session = house();
    let level = session.model.levels().first().map(|(id, _)| *id);
    let panel = Panel::of(&session.model, &[], None, level);
    assert!(matches!(panel.subject, Subject::Plan(_)), "{:?}", panel.subject);
    assert!(panel.title.starts_with("Floor Plan: "), "{}", panel.title);
    let name = text_of(&panel, "name").unwrap_or_default();
    assert!(!name.is_empty() && panel.title.ends_with(&name));
    assert!(matches!(panel.row("name").map(|r| &r.editor), Some(Editor::Text { .. })));
    assert!(matches!(panel.row("crop").map(|r| &r.editor), Some(Editor::Check(_))));
    assert!(matches!(panel.row("view_range").map(|r| &r.editor), Some(Editor::Seam("View Range"))));
    let units = session.model.units;
    let Ok(Some(rename)) = apply::command(&panel, units, "name", Edit::Text("Ground")) else { panic!("a name") };
    assert!(session.execute(rename.name, &rename.params).is_ok());
    assert_eq!(session.model.levels().first().map(|(_, l)| l.name.clone()).as_deref(), Some("Ground"));
    assert_eq!(Panel::of(&session.model, &[], None, None).subject, Subject::Nothing);
    assert_eq!(Panel::of(&session.model, &[999_999_999], None, None).subject, Subject::Nothing, "an id the model has not");
}

#[test]
fn more_elements_than_the_palette_works_out_are_counted_and_given_no_rows() {
    let session = house();
    let Some(wall) = first_of(&session.model, "wall") else { panic!("the house has walls") };
    let many = vec![wall; super::panel::MAX_IDS + 5];
    let panel = Panel::of(&session.model, &many, None, None);
    assert!(panel.groups.is_empty(), "no rows");
    assert_eq!(panel.notes.len(), 1, "the count is said: {:?}", panel.notes);
    assert_eq!(panel.ids.len(), many.len(), "an edit would still be all of them, which is why no row is offered");
    assert!(panel.title.contains(&(super::panel::MAX_IDS + 5).to_string()), "{}", panel.title);
}

#[test]
fn the_type_selector_lists_the_categorys_types_and_picking_one_sets_it_through_the_engine() {
    let mut session = house();
    let Some(wall) = first_of(&session.model, "wall") else { panic!("the house has walls") };
    let panel = Panel::of(&session.model, &[wall], None, None);
    let Some(selector) = panel.selector.clone() else { panic!("a wall has a Type Selector") };
    let names: Vec<String> = session.model.type_names("wall").into_iter().map(String::from).collect();
    let mut listed: Vec<String> = selector.types.iter().map(|t| t.name.clone()).collect();
    listed.sort();
    assert_eq!(listed, names, "every type the project has, and nothing else, for a wall");
    assert_eq!(selector.heading, "Wall");
    assert!(selector.enabled && selector.editable());
    let current = selector.current.clone().unwrap_or_default();
    assert!(names.contains(&current), "{current}");
    let here = selector.types.iter().position(|t| t.label == current).unwrap_or(0);
    assert_eq!(apply::set_type(&panel, here), Ok(None), "it is that type already");
    let Some(other) = selector.types.iter().position(|t| t.label != current) else { panic!("the house has two wall types") };
    let Ok(Some(command)) = apply::set_type(&panel, other) else { panic!("another type") };
    assert_eq!(command.params, json!({"ids": [wall], "type": selector.types[other].name}));
    assert!(session.execute(command.name, &command.params).is_ok());
    let after = Panel::of(&session.model, &[wall], None, None);
    assert_eq!(after.selector.and_then(|s| s.current), Some(selector.types[other].name.clone()));
    assert!(apply::set_type(&panel, 9_999).is_err());
    assert!(apply::set_type(&Panel::of(&session.model, &[], None, None), 0).is_err(), "the plan has no type to pick");
}

#[test]
fn a_door_of_a_family_lists_the_projects_types_and_the_familys_and_a_project_type_takes_it_off_the_family() {
    let session = house();
    let Some(door) = first_of(&session.model, "door") else { panic!("the house has doors") };
    let panel = Panel::of(&session.model, &[door], None, None);
    let Some(selector) = panel.selector.clone() else { panic!("a door has a Type Selector") };
    assert!(selector.hosted);
    assert!(selector.types.iter().any(|t| t.family.is_none()), "the project's own types");
    assert!(selector.types.iter().any(|t| t.family.is_some()), "the families' types");
    let Some(own) = selector.types.iter().position(|t| t.family.is_none() && Some(&t.label) != selector.current.as_ref()) else {
        panic!("a door type of the project's own besides its type")
    };
    let Ok(Some(command)) = apply::set_type(&panel, own) else { panic!("a project type") };
    assert_eq!(command.params["family"], serde_json::Value::Null, "a project type takes it off its family");
    assert_eq!(command.params["type"], json!(selector.types[own].name));
    let Some(of_family) = selector.types.iter().position(|t| t.family.is_some() && Some(&t.label) != selector.current.as_ref()) else {
        panic!("a family's type besides its own")
    };
    let Ok(Some(command)) = apply::set_type(&panel, of_family) else { panic!("a family's type") };
    assert_eq!(command.params["family"], json!(selector.types[of_family].family));
}

#[test]
fn the_plans_graphics_rows_set_scale_detail_and_north_through_their_own_commands() {
    let mut session = house();
    let Some(level) = session.model.levels().first().map(|(id, _)| *id) else { panic!("a level") };
    let units = session.model.units;
    let panel = Panel::of(&session.model, &[], None, Some(level));
    let Some(graphics) = panel.groups.iter().find(|g| g.key == "Graphics") else { panic!("a Graphics group") };
    let keys: Vec<&str> = graphics.rows.iter().map(|r| r.key).collect();
    assert_eq!(keys, ["scale", "scale_value", "detail", "orientation", "template", "overrides", "grids"]);
    assert_eq!(text_of(&panel, "scale_value").as_deref(), Some("100"), "1 : 100 until a view is given another");
    let Ok(Some(command)) = apply::command(&panel, units, "scale_value", Edit::Text("1 : 50")) else { panic!("a scale") };
    assert_eq!((command.name, command.params.clone()), ("scale", json!({"view": level, "scale": 50.0})));
    assert!(session.execute(command.name, &command.params).is_ok());
    let again = Panel::of(&session.model, &[], None, Some(level));
    assert_eq!(text_of(&again, "scale_value").as_deref(), Some("50"));
    assert_eq!(text_of(&again, "scale").as_deref(), Some("1 : 50"));
    assert!(apply::command(&again, units, "scale_value", Edit::Text("big")).is_err(), "not a scale");
    let Ok(Some(coarse)) = apply::command(&again, units, "detail", Edit::Choice(0)) else { panic!("a level of detail") };
    assert_eq!((coarse.name, coarse.params), ("detail", json!({"view": level, "detail": "Coarse"})));
    let Ok(Some(north)) = apply::command(&again, units, "orientation", Edit::Choice(1)) else { panic!("a north") };
    assert_eq!((north.name, north.params), ("graphics", json!({"view": level, "orientation": "true_north"})));
    assert!(matches!(again.row("overrides").map(|r| &r.editor), Some(Editor::Seam("Visibility/Graphics"))));
}
