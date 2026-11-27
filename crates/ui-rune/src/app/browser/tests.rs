use super::*;
use crate::views::Group;

fn group<'a>(app: &'a App, label: &str) -> &'a Group {
    app.views.groups.iter().find(|g| g.label == label).expect("a group")
}

fn open_menu_over(app: &mut App, target: Target) {
    app.open_browser_menu(target, 40.0, 50.0);
    assert!(app.views.menu.is_some());
}

#[test]
fn a_press_on_a_view_opens_it_and_with_ctrl_or_shift_picks_views_together_and_opens_none() {
    let mut app = App::sample();
    let kinds: Vec<ViewKind> = group(&app, "Sheets (all)").entries.iter().map(|e| e.kind).collect();
    app.press_row(&Pick::Open, kinds[0], false, false);
    assert_eq!((app.view, app.views.picked.clone()), (kinds[0], vec![kinds[0]]));
    app.press_row(&Pick::Open, kinds[2], false, true);
    assert_eq!(app.view, kinds[0], "Ctrl picks and does not open");
    assert_eq!(app.views.picked, [kinds[0], kinds[2]]);
    app.press_row(&Pick::Open, kinds[1], true, false);
    assert_eq!(app.view, kinds[0]);
    assert!(app.views.picked.contains(&kinds[1]), "Shift picks the run from the last one picked: {:?}", app.views.picked);
    app.press_row(&Pick::Open, kinds[1], false, false);
    assert_eq!((app.view, app.views.picked.clone()), (kinds[1], vec![kinds[1]]), "a plain press picks it alone");
}

#[test]
fn the_search_and_the_tab_decide_the_views_a_shift_run_goes_over() {
    let mut app = App::sample();
    app.set_browser_tab(Tab::Sheets);
    app.views_find = rune::TextEdit::new("a10");
    assert_eq!(app.views_find_text(), "a10");
    let order = app.views.order(app.views_find_text());
    assert_eq!(order.len(), 3);
    app.press_row(&Pick::Open, order[0], false, false);
    app.press_row(&Pick::Open, order[2], true, false);
    assert_eq!(app.views.picked, order, "the run is of the sheets the search shows");
}

#[test]
fn a_press_on_a_type_opens_its_type_properties_and_one_on_a_link_selects_it() {
    let mut app = App::sample();
    let (pick, label) = group(&app, "Families")
        .entries
        .iter()
        .find(|e| e.label.starts_with("Wall: "))
        .map(|e| (e.pick.clone(), e.label.clone()))
        .expect("a wall type");
    app.press_row(&pick, ViewKind::Nothing, false, false);
    let open = app.dialogs.types.as_ref().map(|d| d.key.clone());
    assert!(matches!(&pick, Pick::Type(c, n) if open.as_ref().is_some_and(|k| &k.category == c && &k.name == n)), "{label}: {open:?}");
    app.close_dialogs();
    let level = app.level.expect("a level");
    app.press_row(&Pick::Select(level), ViewKind::Nothing, false, false);
    assert_eq!(app.selection, [level]);
}

#[test]
fn the_menu_over_a_plan_duplicates_it_three_ways_and_the_new_view_opens() {
    let mut app = App::sample();
    let level = app.level.expect("a level");
    open_menu_over(&mut app, Target::View(ViewKind::Plan(level)));
    let items = app.browser_menu_items();
    let actions: Vec<Action> = items.iter().flatten().map(|i| i.action).collect();
    for a in [
        Action::Duplicate,
        Action::DuplicateDetailing,
        Action::DuplicateDependent,
        Action::CreatePrintSet,
        Action::ExpandAll,
        Action::CollapseAll,
    ] {
        assert!(actions.contains(&a), "{a:?}");
    }
    assert!(!actions.contains(&Action::DeleteView), "a level's own plan is not deleted");
    app.act(Action::Duplicate);
    assert!(app.views.menu.is_none() && app.message.is_none(), "{:?}", app.message);
    let ViewKind::Plan(copy) = app.view else { panic!("the copy opened: {:?}", app.view) };
    assert_ne!(copy, level);
    assert_eq!(app.level, Some(level), "it stands on the same level");
    assert!(group(&app, "Floor Plans").entries.iter().any(|e| e.kind == app.view));

    open_menu_over(&mut app, Target::View(ViewKind::Plan(copy)));
    assert!(app.browser_menu_items().iter().flatten().any(|i| i.action == Action::DeleteView), "a plan view can be deleted");
    app.act(Action::DeleteView);
    assert_eq!(app.view, ViewKind::Plan(level), "the view that went falls back to its level's plan");
    assert!(app.session.model.elements.get(copy).is_none());
    assert!(app.undo());
    assert!(app.session.model.elements.get(copy).is_some());
}

#[test]
fn a_print_set_holds_the_views_picked_together_or_the_one_clicked_and_names_itself() {
    let mut app = App::sample();
    let sheets: Vec<ViewKind> = group(&app, "Sheets (all)").entries.iter().map(|e| e.kind).collect();
    app.press_row(&Pick::Open, sheets[0], false, false);
    app.press_row(&Pick::Open, sheets[1], false, true);
    open_menu_over(&mut app, Target::View(sheets[0]));
    app.act(Action::CreatePrintSet);
    let set = app.session.model.print_sets.get("Print Set 1").expect("a print set");
    assert_eq!(set.views.len(), 2, "the two picked");
    open_menu_over(&mut app, Target::View(sheets[2]));
    app.act(Action::CreatePrintSet);
    assert_eq!(app.session.model.print_sets.get("Print Set 2").map(|s| s.views.len()), Some(1), "a click outside the picked is the one");
    let level = app.levels[0].0;
    open_menu_over(&mut app, Target::View(ViewKind::Ceiling(level)));
    assert!(
        app.browser_menu_items().iter().flatten().any(|i| i.action == Action::CreatePrintSet && !i.enabled),
        "a ceiling plan cannot yet"
    );
    app.act(Action::CreatePrintSet);
    assert_eq!(app.message.as_deref(), Some("None of these can be printed yet."));
}

#[test]
fn the_menu_over_a_type_a_link_or_a_group_offers_what_each_is_for() {
    let mut app = App::sample();
    let (category, name) = match group(&app, "Families").entries.iter().find_map(|e| match &e.pick {
        Pick::Type(c, n) => Some((c.clone(), n.clone())),
        _ => None,
    }) {
        Some(t) => t,
        None => panic!("a type"),
    };
    open_menu_over(&mut app, Target::Type(category.clone(), name.clone()));
    let actions: Vec<Action> = app.browser_menu_items().iter().flatten().map(|i| i.action).collect();
    assert!(actions.contains(&Action::EditType) && actions.contains(&Action::DeleteType) && actions.contains(&Action::ExpandAll));
    app.act(Action::EditType);
    assert!(app.dialogs.types.is_some() && app.views.menu.is_none());
    app.close_dialogs();
    // A type that a wall of the house uses is refused, and nothing changes.
    open_menu_over(&mut app, Target::Type("wall".into(), "Exterior - 300mm".into()));
    app.act(Action::DeleteType);
    assert!(app.message.is_some(), "the project refuses while an element uses it");
    assert!(app.session.model.elements.values().any(|e| e.category() == "wall"));

    open_menu_over(&mut app, Target::Link(100000));
    let link: Vec<Action> = app.browser_menu_items().iter().flatten().map(|i| i.action).collect();
    assert!(link.contains(&Action::ReloadLink) && link.contains(&Action::RemoveLink));
    app.close_browser_menu();
    assert!(app.views.menu.is_none() && app.browser_menu_items().is_empty());

    open_menu_over(&mut app, Target::Group("Floor Plans"));
    let group_items: Vec<Action> = app.browser_menu_items().iter().flatten().map(|i| i.action).collect();
    assert_eq!(group_items, [Action::ExpandAll, Action::CollapseAll]);
    app.act(Action::CollapseAll);
    assert!(app.views.groups.iter().all(|g| app.views.folded.contains(g.label)));
    open_menu_over(&mut app, Target::Group("Floor Plans"));
    app.act(Action::ExpandAll);
    assert!(app.views.folded.is_empty());
}

#[test]
fn a_menu_at_a_place_that_is_no_number_opens_nothing_and_an_item_with_no_menu_does_nothing() {
    let mut app = App::sample();
    let revision = app.revision;
    app.open_browser_menu(Target::Group("Floor Plans"), f32::NAN, 1.0);
    app.open_browser_menu(Target::Group("Floor Plans"), 1.0, f32::INFINITY);
    assert!(app.views.menu.is_none());
    app.act(Action::Duplicate);
    app.act(Action::DeleteView);
    app.act(Action::RemoveLink);
    assert_eq!((app.revision, app.session.model.elements.len()), (revision, App::sample().session.model.elements.len()));
    // The plan's menu and the browser's are one at a time.
    app.open_menu(5.0, 5.0);
    app.open_browser_menu(Target::Group("Floor Plans"), 5.0, 5.0);
    assert!(app.tooling.menu.is_none() && app.views.menu.is_some());
}
