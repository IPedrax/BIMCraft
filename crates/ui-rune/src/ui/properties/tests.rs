//! The palette driven as a person drives it, headless: Rune's `Runtime` on the app's real state and view, a tap on a row's editor, and
//! what the model and the palette say afterwards. The rows mount after the first frames and fade in, so each tap follows a few dozen frames.

use bimcraft_core::model::Id;
use rune::{Env, Runtime, TextEdit};

use crate::app::App;
use crate::lang::t;
use crate::props::panel::{Editor, Subject};
use crate::ui::view;

fn window() -> Runtime<App> {
    let mut rt = Runtime::new(App::sample(), view, (1280.0, 800.0), Env::new(true, 1.0));
    settle(&mut rt);
    rt
}

fn settle(rt: &mut Runtime<App>) {
    for _ in 0..40 {
        rt.frame(1.0 / 60.0);
    }
}

fn tap(rt: &mut Runtime<App>, at: (f32, f32)) {
    rt.pointer_down(at.0, at.1);
    rt.pointer_up(at.0, at.1);
    settle(rt);
}

fn first_of(rt: &Runtime<App>, category: &str) -> Option<Id> {
    rt.state.session.model.elements.iter().find(|(_, e)| e.category() == category).map(|(id, _)| *id)
}

fn select(rt: &mut Runtime<App>, ids: Vec<Id>) {
    rt.update(move |s, _| s.select(ids.clone()));
    settle(rt);
}

#[test]
fn with_nothing_selected_the_palette_shows_the_plans_own_view_and_its_crop_box_goes_through_the_engine_and_back() {
    let mut rt = window();
    let level = rt.state.level.expect("a level");
    assert!(matches!(rt.state.palette.panel.subject, Subject::Plan(l) if l == level));
    let at = rt.find("row-crop").expect("the plan's Crop View row");
    tap(&mut rt, at);
    assert!(rt.state.message.is_none(), "the core took the crop: {:?}", rt.state.message);
    assert!(matches!(rt.state.palette.panel.row("crop").map(|r| &r.editor), Some(Editor::Check(true))));
    let undo = rt.find("undo").expect("Undo");
    tap(&mut rt, undo);
    assert!(matches!(rt.state.palette.panel.row("crop").map(|r| &r.editor), Some(Editor::Check(false))));
}

#[test]
fn a_list_opens_in_place_and_picking_an_option_changes_the_selected_wall_through_the_engine() {
    let mut rt = window();
    let wall = first_of(&rt, "wall").expect("a wall");
    select(&mut rt, vec![wall]);
    let trigger = rt.find("pick-location").expect("the wall's Location Line list");
    assert!(rt.find("opt-location-2").is_none(), "closed: no options");
    tap(&mut rt, trigger);
    assert_eq!(rt.state.palette.open, Some("location"));
    let option = rt.find("opt-location-2").expect("the list opens in place");
    tap(&mut rt, option);
    assert!(rt.state.message.is_none(), "{:?}", rt.state.message);
    assert_eq!(rt.state.palette.open, None, "picking closes it");
    let shown = rt.state.palette.panel.row("location").map(|r| r.shown.clone()).unwrap_or_default();
    assert_eq!(shown, t("Finish Face: Exterior"), "the wall is on its other line");
    assert!(rt.find("opt-location-2").is_none());
}

#[test]
fn a_group_folds_under_its_header_and_opens_again() {
    let mut rt = window();
    let wall = first_of(&rt, "wall").expect("a wall");
    select(&mut rt, vec![wall]);
    assert!(rt.find("row-location").is_some(), "the group is open");
    rt.update(|s, _| s.fold("Constraints"));
    settle(&mut rt);
    assert!(rt.find("row-location").is_none(), "folded: its rows are gone");
    assert!(rt.find("row-length").is_some(), "the other groups stay");
    rt.update(|s, _| s.fold("Constraints"));
    settle(&mut rt);
    assert!(rt.find("row-location").is_some());
}

#[test]
fn a_mixed_selection_offers_its_categories_and_a_tap_on_one_narrows_the_palette_to_it() {
    let mut rt = window();
    let (wall, door) = (first_of(&rt, "wall").expect("a wall"), first_of(&rt, "door").expect("a door"));
    select(&mut rt, vec![wall, door]);
    assert_eq!(rt.state.palette.panel.subject, Subject::Mixed(2));
    assert!(rt.find("row-location").is_none(), "no rows for two categories");
    let walls = rt.find("filter-wall").expect("a button for the walls");
    tap(&mut rt, walls);
    assert_eq!(rt.state.palette.panel.subject, Subject::Element(wall));
    assert_eq!(rt.state.selection, vec![wall, door], "the selection stays as it is");
    assert!(rt.find("row-location").is_some());
    let all = rt.find("filter-all").expect("and one for all of them");
    tap(&mut rt, all);
    assert_eq!(rt.state.palette.panel.subject, Subject::Mixed(2));
}

#[test]
fn a_refused_value_is_said_under_the_title_and_changes_nothing() {
    let mut rt = window();
    let level = rt.state.level.expect("a level");
    select(&mut rt, vec![level]);
    let i = rt.state.palette.fields.iter().position(|f| f.key == "elevation").expect("a level has an elevation");
    let revision = rt.state.revision;
    rt.update(move |s, _| {
        s.palette.fields[i].edit = TextEdit::new("lots");
        s.commit_field(i);
    });
    settle(&mut rt);
    assert_eq!(rt.state.revision, revision, "nothing was asked of the engine");
    let said = rt.state.palette.refusal.clone().unwrap_or_default();
    assert!(said.contains("lots"), "{said}");
    assert!(rt.find("refusal").is_some(), "the palette says it");
}
