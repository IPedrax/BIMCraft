//! The Project Browser's search and its tabs, typed into and clicked as a user does.

use super::*;

const FIND: &str = "Search the Project Browser";

/// How many of the browser's entries are named `label`: those under its search field.
fn count(h: &Harness<BimApp>, label: &str) -> usize {
    let find = h.get_by_label(FIND).rect();
    h.query_all_by_label(label).filter(|n| n.rect().top() > find.bottom() && n.rect().left() < find.right()).count()
}

#[test]
fn the_search_keeps_the_tree_to_what_matches_as_it_is_typed() {
    let mut h = app(BOX);
    assert_eq!((count(&h, "Level 1"), count(&h, "Level 2")), (2, 2), "the floor and ceiling plans");
    // A word for a group, one for an entry in it: Level 2's ceiling plan alone.
    type_into(&mut h, FIND, "ceil 2");
    assert_eq!((count(&h, "Level 1"), count(&h, "Level 2")), (0, 1), "the ceiling plan alone");
    assert!(h.query_by_label("Ceiling Plans").is_some() && h.query_by_label("Floor Plans").is_none());
    assert!(h.query_by_label("Schedules/Quantities (all)").is_none(), "nor anything else");
    browse(&mut h, "Level 2");
    assert!(matches!(h.state().sheet, Sheet::Ceiling(_)), "what's left opens");
    // Wildcards: ? one character, * any run.
    type_into(&mut h, FIND, "l?v*1");
    assert_eq!(count(&h, "Level 1"), 2, "both of Level 1's plans");
    assert_eq!(count(&h, "Level 2"), 0);
    // Nothing: it says so, and clearing the search brings the tree back.
    type_into(&mut h, FIND, "zzz");
    assert!(h.query_by_label("Nothing matches the search.").is_some());
    press(&mut h, "Clear the Search");
    assert!(h.state().browser_find.is_empty());
    assert!(h.query_by_label("Views (all)").is_some() && h.query_by_label("Floor Plans").is_some());
}

#[test]
fn the_tabs_keep_the_browser_to_one_kind_and_legends_are_still_to_come() {
    let mut h = app(BOX);
    assert!(h.query_by_label("Views (all)").is_some(), "everything, at first");
    press(&mut h, "List Families");
    assert_eq!(h.state().browser_tab, crate::browser::Tab::Families);
    assert!(h.query_by_label("Views (all)").is_none() && h.query_by_label("Schedules/Quantities (all)").is_none());
    // The family is at the top, its group's title gone, and its types under it.
    h.get_by_label("Table").click_accesskit();
    h.run();
    h.get_by_label("2000 x 1000").click_accesskit();
    h.run();
    assert_eq!(h.state().tool, Tool::Component);
    press(&mut h, "List Views");
    assert!(h.query_by_label("Floor Plans").is_some() && h.query_by_label("Table").is_none());
    // The search searches the tab.
    type_into(&mut h, FIND, "Wall");
    assert!(h.query_by_label("Nothing matches the search.").is_some(), "the wall schedule is on Schedules");
    press(&mut h, "List Schedules");
    assert!(h.query_by_label("Wall Schedule").is_some());
    press(&mut h, "Clear the Search");
    // Legends: a tab for later, greyed out, that changes nothing.
    h.get_by_label("List Legends").click();
    h.run();
    assert_eq!(h.state().browser_tab, crate::browser::Tab::Schedules);
    press(&mut h, "List All");
    assert!(h.query_by_label("Views (all)").is_some() && h.query_by_label("Families").is_some());
}

#[test]
fn the_families_list_curtain_panels_and_mullions_under_their_system_families() {
    let mut h = app(BOX);
    press(&mut h, "List Families");
    for label in ["Curtain Panels", "Generic Panel", "Spandrel"] {
        h.get_by_label(label).click_accesskit();
        h.run();
    }
    assert_eq!(h.state().types.get("curtainpanel").map(String::as_str), Some("Spandrel"));
    for label in ["Mullions", "Rectangular Mullion", "50 x 150"] {
        h.get_by_label(label).click_accesskit();
        h.run();
    }
    assert_eq!(h.state().types.get("mullion").map(String::as_str), Some("50 x 150"));
}

/// A text field of the row being renamed: opened by F2, its whole name selected, saved by Enter.
fn rename_with(h: &mut Harness<BimApp>, text: &str) {
    type_into(h, "New name", text);
    h.key_press(Key::Enter);
    h.run();
}

#[test]
fn f2_with_no_row_picked_is_the_shortcuts_floor_plan() {
    let mut h = app(BOX);
    h.state_mut().sheet = Sheet::View3D;
    h.run();
    let row = h.get_by_label("Views (all)").rect().center();
    click_at(&mut h, row);
    assert!(h.state().browser_sel.is_none(), "a group is no row to rename");
    h.key_press(Key::F2);
    h.run();
    let opened = h.state().sheet;
    assert!(
        matches!(opened, Sheet::Plan(id) if h.state().session.model.plan_level(id).is_some()),
        "F2 is Floor Plan again, and it opens a real level's plan: {opened:?}"
    );
    assert_eq!(h.query_all_by_label("New name").count(), 0, "no name is edited");
}

#[test]
fn f2_renames_the_picked_plan_view_and_one_undo_brings_the_level_name_back() {
    let mut h = app(BOX);
    let row = browser_row(&h, "Level 1");
    click_at(&mut h, row);
    h.key_press(Key::F2);
    h.run();
    assert_eq!(h.query_all_by_label("New name").count(), 1, "one row edits, not the floor plan and the ceiling plan");
    assert_eq!(h.get_by_role_and_label(Role::TextInput, "New name").value().as_deref(), Some("Level 1"));
    rename_with(&mut h, "Ground");
    let m = &h.state().session.model;
    let level = m.levels().into_iter().find(|(_, l)| l.name == "Level 1").map(|(id, _)| id).expect("the level keeps its name");
    let view = m.primary_plan(level, false).expect("the plan is a view of its own now");
    assert_eq!(m.view_name(view), Some("Ground"), "the view is renamed");
    assert_eq!(m.view_name(level), Some("Ground"), "the level's plan reads the view's name");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    let m = &h.state().session.model;
    assert!(m.primary_plan(level, false).is_none(), "one undo takes the view away");
    assert_eq!(m.view_name(level), Some("Level 1"));
}

#[test]
fn f2_esc_leaves_the_name_and_a_duplicate_stays_in_the_row() {
    let mut h = app(BOX);
    let row = browser_row(&h, "Level 1");
    click_at(&mut h, row);
    h.key_press(Key::F2);
    h.run();
    type_into(&mut h, "New name", "Ground");
    h.key_press(Key::Escape);
    h.run();
    assert!(h.query_by_role_and_label(Role::TextInput, "New name").is_none(), "the editor closes");
    assert!(h.state().session.model.levels().iter().any(|(_, l)| l.name == "Level 1"), "Esc cancels");
    // A name another level has is refused in the row: the field stays open with what was typed.
    h.key_press(Key::F2);
    h.run();
    type_into(&mut h, "New name", "Level 2");
    h.key_press(Key::Enter);
    h.run();
    let field = h.get_by_role_and_label(Role::TextInput, "New name");
    assert_eq!(field.value().as_deref(), Some("Level 2"), "what was typed stays");
    assert!(h.state().session.model.levels().iter().filter(|(_, l)| l.name == "Level 2").count() <= 1);
    h.key_press(Key::Escape);
    h.run();
}

#[test]
fn f2_makes_a_builtin_schedule_its_own_row_and_keeps_one_row_of_it() {
    let mut h = app(BOX);
    press(&mut h, "List Schedules");
    let row = h.get_by_label("Door Schedule").rect().center();
    click_at(&mut h, row);
    h.key_press(Key::F2);
    h.run();
    assert_eq!(h.get_by_role_and_label(Role::TextInput, "New name").value().as_deref(), Some("Door Schedule"));
    rename_with(&mut h, "Doors");
    let m = &h.state().session.model;
    let id = bimcraft_core::schedule::standard(m, "door").0.expect("the built-in is the project's now");
    assert_eq!(m.view_name(id), Some("Doors"));
    assert_eq!(count(&h, "Doors"), 1, "one row, not Door Schedule and Doors");
    assert!(h.query_by_label("Door Schedule").is_none());
}

#[test]
fn the_menu_renames_the_row_it_was_opened_on() {
    let mut h = app(BOX);
    let row = browser_row(&h, "Level 1");
    click_at(&mut h, row);
    // Right-click another row: Rename names it, not the row picked before.
    let pos = browser_row(&h, "Level 2");
    h.hover_at(pos);
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
    }
    h.run();
    press(&mut h, "Rename");
    h.run();
    assert_eq!(h.get_by_role_and_label(Role::TextInput, "New name").value().as_deref(), Some("Level 2"));
    rename_with(&mut h, "Second");
    let m = &h.state().session.model;
    let level = m.levels().into_iter().find(|(_, l)| l.name == "Level 2").map(|(id, _)| id).expect("the level keeps its name");
    let view = m.primary_plan(level, false).expect("the plan is a view of its own now");
    assert_eq!(m.view_name(view), Some("Second"));
}

#[test]
fn the_families_categories_are_listed_alphabetically_by_their_names() {
    use egui_kittest::kittest::NodeT;
    let mut h = app(&format!("{BOX}\nfloor {{\"boundary\": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}}\ndoor {{\"at\": [2000, 0]}}"));
    press(&mut h, "List Families");
    let find = h.get_by_label(FIND).rect();
    // The categories' headers (buttons) under the search field, top to bottom; some nodes have no bounds at all.
    let mut headers: Vec<(f32, String)> = h
        .get_all_by_role(Role::Button)
        .filter(|n| n.accesskit_node().bounding_box().is_some())
        .filter(|n| n.rect().top() > find.bottom() && n.rect().left() < find.right())
        .filter_map(|n| Some((n.rect().top(), n.accesskit_node().label()?)))
        .filter(|(_, l)| l.as_str() != "Families")
        .collect();
    headers.sort_by(|a, b| a.0.total_cmp(&b.0));
    let names: Vec<String> = headers.into_iter().map(|(_, l)| l).collect();
    // The system's categories come first, then the component families, each group by the name shown, case aside; the
    // Properties palette's buttons share the column, so look at the names the families list is known to hold.
    let system = ["Beams", "Ceilings", "Columns", "Doors", "Floors", "Roofs", "Stairs", "Walls"];
    let shown: Vec<&str> = names.iter().map(String::as_str).filter(|n| system.contains(n)).collect();
    assert_eq!(shown, system, "{names:?}");
    let at = |name: &str| names.iter().position(|n| n == name);
    assert!(matches!((at("Washbasin"), at("WC")), (Some(a), Some(b)) if a < b), "case aside: {names:?}");
}
