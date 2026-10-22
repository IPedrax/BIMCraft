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
    h.get_by_label("Level 2").click_accesskit();
    h.run();
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
