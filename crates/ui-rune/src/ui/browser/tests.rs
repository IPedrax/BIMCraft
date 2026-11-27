//! The Project Browser driven as a person drives it, headless: its tabs, the search, Ctrl and Shift on a row, the right click.

use rune::{Env, Mods, PhysicalInput, Runtime, TextEdit};

use crate::app::App;
use crate::ui::view;
use crate::views::{Tab, ViewKind};

fn window() -> Runtime<App> {
    let mut rt = Runtime::new(App::sample(), view, (1280.0, 800.0), Env::new(true, 1.0));
    for _ in 0..3 {
        rt.frame(0.0);
    }
    rt
}

fn tap(rt: &mut Runtime<App>, key: &str) {
    let at = rt.find(key).unwrap_or_else(|| panic!("an element keyed {key}"));
    rt.pointer_down(at.0, at.1);
    rt.pointer_up(at.0, at.1);
    for _ in 0..3 {
        rt.frame(0.3);
    }
}

#[test]
fn a_tab_keeps_the_browser_to_one_kind_and_shows_its_rows_without_the_groups_name() {
    let mut rt = window();
    let sheet = rt.state.views.groups.iter().find(|g| g.label == "Sheets (all)").map(|g| g.entries[0].key.clone()).expect("a sheet");
    assert!(rt.find("group-Floor Plans").is_some() && rt.find(&sheet).is_some(), "All lists everything");
    tap(&mut rt, "tab-sheets");
    assert_eq!(rt.state.views.tab, Tab::Sheets);
    assert!(rt.find(&sheet).is_some(), "the sheets' rows");
    assert!(rt.find("group-Sheets (all)").is_none() && rt.find("group-Floor Plans").is_none(), "no group's name: it is all there is");
    tap(&mut rt, "tab-families");
    let families = rt.state.views.groups.iter().find(|g| g.label == "Families").map(|g| g.entries[0].key.clone()).expect("a type");
    assert!(rt.find(&families).is_some(), "the families, though that group starts folded under All");
    tap(&mut rt, "tab-all");
    assert!(rt.find("group-Families").is_some() && rt.find(&families).is_none(), "folded under All");
    rt.update(|s, _| s.views.toggle("Families"));
    rt.frame(0.3);
    assert!(rt.find(&families).is_some(), "opened");
}

#[test]
fn what_is_typed_in_the_search_keeps_the_list_to_what_it_finds_and_opens_the_groups() {
    let mut rt = window();
    let level = rt.state.levels[0].0;
    let name = rt.state.levels[0].1.to_lowercase();
    rt.update(|s, _| s.views_find = TextEdit::new(format!("ceiling {name}")));
    for _ in 0..3 {
        rt.frame(0.3);
    }
    assert!(rt.find(&format!("ceiling-{level}")).is_some(), "the ceiling plan of the level");
    assert!(rt.find(&format!("level-{level}")).is_none() && rt.find("group-Floor Plans").is_none(), "the floor plans do not match");
    assert!(rt.find("browser-find").is_some());
    rt.update(|s, _| s.views_find = TextEdit::new("fam* wall"));
    for _ in 0..3 {
        rt.frame(0.3);
    }
    assert!(rt.find("group-Families").is_some(), "a search opens the group it finds something in, folded or not");
    let wall = rt.state.views.rows("fam* wall").into_iter().flat_map(|(_, e)| e).map(|e| e.key.clone()).next().expect("a wall type");
    assert!(rt.find(&wall).is_some());
}

#[test]
fn ctrl_on_a_row_picks_views_together_and_opens_none_and_a_right_click_opens_the_menu() {
    let mut rt = window();
    let shown = rt.state.view;
    let (a, b) = (rt.state.levels[0].0, rt.state.levels[1].0);
    rt.physical_input(PhysicalInput::Modifiers(Mods { cmd: true, ..Mods::default() }));
    tap(&mut rt, &format!("level-{b}"));
    assert_eq!(rt.state.view, shown, "Ctrl opens nothing");
    assert_eq!(rt.state.views.picked, [ViewKind::Plan(a), ViewKind::Plan(b)], "the view on screen counts, then the one pressed");
    rt.physical_input(PhysicalInput::Modifiers(Mods::default()));

    let at = rt.find(&format!("level-{b}")).expect("its row");
    assert!(rt.find("browser-menu-menu").is_none(), "closed, nothing is built");
    rt.secondary_down(at.0, at.1);
    rt.secondary_up(at.0, at.1);
    for _ in 0..3 {
        rt.frame(0.3);
    }
    assert!(rt.state.views.menu.as_ref().is_some_and(|(_, _, t)| matches!(t, crate::views::Target::View(ViewKind::Plan(id)) if *id == b)));
    assert!(rt.find("browser-menu-menu").is_some(), "the menu is on screen");
    // A level's plan: Duplicate, with Detailing, as Dependent, a separator, Create Print Set, a separator, Expand All, Collapse All.
    let duplicate = rt.find("browser-menu-item0").expect("Duplicate");
    rt.pointer_down(duplicate.0, duplicate.1);
    rt.pointer_up(duplicate.0, duplicate.1);
    for _ in 0..3 {
        rt.frame(0.3);
    }
    assert!(rt.state.views.menu.is_none() && rt.find("browser-menu-menu").is_none(), "the item closed the menu");
    let ViewKind::Plan(copy) = rt.state.view else { panic!("the copy opened: {:?}", rt.state.view) };
    assert!(copy != a && copy != b, "a new plan view of the level, opened");
}
