use std::collections::HashSet;

use serde_json::json;

use super::*;
use crate::app::App;

fn group<'a>(groups: &'a [Group], label: &str) -> &'a Group {
    groups
        .iter()
        .find(|g| g.label == label)
        .unwrap_or_else(|| panic!("no group {label}: {:?}", groups.iter().map(|g| g.label).collect::<Vec<_>>()))
}

fn labels(g: &Group) -> Vec<&str> {
    g.entries.iter().map(|e| e.label.as_str()).collect()
}

#[test]
fn the_browser_lists_every_view_of_the_sample_house_in_egui_groups() {
    let app = App::sample();
    let m = &app.session.model;
    let groups = tree(m);
    let order: Vec<&str> = groups.iter().map(|g| g.label).collect();
    assert_eq!(
        order,
        [
            "Floor Plans",
            "Ceiling Plans",
            "3D Views",
            "Elevations (Building Elevation)",
            "Sections (Building Section)",
            "Schedules/Quantities (all)",
            "Sheets (all)",
            "Families"
        ],
        "the groups a project has, in egui's order: no empty callouts, panel schedules or reinforcement details, and the families"
    );

    let floors = group(&groups, "Floor Plans");
    assert_eq!(floors.entries.len(), m.levels().len(), "a plan for each level");
    for (entry, (id, level)) in floors.entries.iter().zip(m.levels()) {
        assert_eq!(
            (entry.label.as_str(), entry.kind, entry.key.as_str()),
            (level.name.as_str(), ViewKind::Plan(id), format!("level-{id}").as_str())
        );
    }
    let ceilings = group(&groups, "Ceiling Plans");
    assert!(ceilings.entries.iter().all(|e| matches!(e.kind, ViewKind::Ceiling(_))) && ceilings.entries.len() == floors.entries.len());

    let three = group(&groups, "3D Views");
    assert_eq!((three.entries[0].label.as_str(), three.entries[0].kind), ("{3D}", ViewKind::View3D(m.default_3d())));
    assert!(m.default_3d().is_some(), "the sample places its {{3D}} on a sheet");

    let elevations = labels(group(&groups, "Elevations (Building Elevation)"));
    assert!(elevations.contains(&"South") && elevations.contains(&"East"), "{elevations:?}");
    assert_eq!(labels(group(&groups, "Sections (Building Section)")), ["Section 1"]);
    assert_eq!(labels(group(&groups, "Sheets (all)")), ["A101 - Ground Floor", "A102 - Section and Elevations", "A103 - 3D View"]);

    let schedules = group(&groups, "Schedules/Quantities (all)");
    assert_eq!(schedules.entries.len(), schedule::VIEWS.len(), "the seven every project has");
    assert!(labels(schedules).contains(&"Room Schedule"));
}

#[test]
fn nothing_the_tree_lists_is_missing_from_the_model_and_every_row_has_a_key_of_its_own() {
    let app = App::sample();
    let m = &app.session.model;
    let groups = tree(m);
    let entries: Vec<&Entry> = groups.iter().flat_map(|g| &g.entries).collect();
    assert!(entries.iter().filter(|e| e.pick == Pick::Open).all(|e| exists(m, e.kind)), "every view row opens something the model has");
    let keys: HashSet<&str> = entries.iter().map(|e| e.key.as_str()).collect();
    assert_eq!(keys.len(), entries.len(), "a key finds one row");
    assert!(entries.iter().all(|e| !e.label.is_empty()));
}

#[test]
fn plan_views_follow_their_level_and_callouts_are_named_after_their_parent() {
    let mut app = App::sample();
    let level = app.levels[0].1.clone();
    app.session.execute("duplicate_view", &json!({"view": level})).expect("a plan view");
    app.session.execute("detail_view", &json!({"parent": level, "region": [[0, 0], [2000, 2000]]})).expect("a callout of the plan");
    app.session
        .execute("detail_view", &json!({"parent": "Section 1", "region": [[1000, 0], [3000, 2000]]}))
        .expect("a callout of the section");
    let m = &app.session.model;
    let groups = tree(m);

    let floors = group(&groups, "Floor Plans");
    let first = &floors.entries[0];
    let copy = &floors.entries[1];
    assert_eq!(first.kind, ViewKind::Plan(app.levels[0].0));
    let ViewKind::Plan(view) = copy.kind else { panic!("the plan view is a plan: {copy:?}") };
    assert_eq!(copy.label, format!("{level} Copy 1"), "the plan view comes right after its level");
    assert_eq!(copy.key, format!("plan-{view}"));
    assert_eq!(level_of(m, copy.kind), Some(app.levels[0].0), "it stands on the level it is of");
    let ceilings = group(&groups, "Ceiling Plans");
    assert_eq!(ceilings.entries[1].kind, ViewKind::Ceiling(view), "a plan view has a ceiling plan too");

    let callouts = group(&groups, "Callouts");
    assert_eq!(callouts.entries.len(), 2);
    assert!(
        callouts.entries.iter().any(|e| e.label.starts_with(&format!("{level}: ")) && matches!(e.kind, ViewKind::Plan(_))),
        "{callouts:?}"
    );
    assert!(callouts.entries.iter().any(|e| e.label.starts_with("Section 1: ") && matches!(e.kind, ViewKind::Section(_))), "{callouts:?}");
    assert!(callouts.entries.iter().all(|e| exists(m, e.kind)));
    // A callout of a plan is a plan to draw, and not a section.
    for e in &callouts.entries {
        assert_eq!(matches!(e.kind, ViewKind::Plan(_)), level_of(m, e.kind).is_some(), "{e:?}");
    }
}

#[test]
fn a_view_the_model_lacks_is_not_one() {
    let app = App::sample();
    let m = &app.session.model;
    let level = app.levels[0].0;
    assert!(exists(m, ViewKind::Plan(level)) && exists(m, ViewKind::Ceiling(level)));
    for stale in
        [ViewKind::Plan(7), ViewKind::Ceiling(7), ViewKind::Section(7), ViewKind::Sheet(7), ViewKind::Table(7), ViewKind::View3D(Some(7))]
    {
        assert!(!exists(m, stale), "{stale:?}");
    }
    // An id that is another kind of element is no view of this kind.
    assert!(!exists(m, ViewKind::Section(level)) && !exists(m, ViewKind::Sheet(level)) && !exists(m, ViewKind::Table(level)));
    assert!(!exists(m, ViewKind::Schedule("nonsense")) && exists(m, ViewKind::Schedule("room")));
    assert!(exists(m, ViewKind::View3D(None)), "the default 3D view is there before it is made");
    assert!(!exists(m, ViewKind::Nothing));
    assert_eq!(keep(m, ViewKind::Section(7), Some(level)), ViewKind::Plan(level), "a view that went falls back to the level's plan");
    assert_eq!(keep(m, ViewKind::Section(7), None), ViewKind::Nothing);
    assert_eq!(keep(m, ViewKind::Ceiling(level), None), ViewKind::Ceiling(level));
}

#[test]
fn a_plan_is_drawn_and_framed_and_the_other_kinds_are_left_to_their_own_canvas() {
    let app = App::sample();
    let m = &app.session.model;
    let level = app.levels[0].0;
    let view = fit(m, ViewKind::Plan(level), 800, 600).expect("a plan is framed");
    let plan = draw(m, ViewKind::Plan(level), view).expect("a plan is drawn");
    let ceiling = draw(m, ViewKind::Ceiling(level), view).expect("a ceiling plan is drawn");
    assert!(!plan.prims.is_empty() && !ceiling.prims.is_empty());
    for other in [ViewKind::Nothing, ViewKind::Section(level), ViewKind::View3D(None), ViewKind::Sheet(level), ViewKind::Schedule("room")] {
        assert!(draw(m, other, view).is_none() && fit(m, other, 800, 600).is_none(), "{other:?}");
    }
    assert!(ViewKind::Plan(level).is_plan() && ViewKind::Ceiling(level).is_plan() && !ViewKind::Section(level).is_plan());
}

#[test]
fn a_view_is_titled_by_its_kind_and_a_plan_by_its_name() {
    let app = App::sample();
    let m = &app.session.model;
    let (level, name) = (app.levels[0].0, app.levels[0].1.clone());
    assert_eq!(title(m, ViewKind::Plan(level)), name, "the status bar's first words stay the level's name");
    assert_eq!(title(m, ViewKind::Ceiling(level)), format!("Ceiling Plan: {name}"));
    let groups = tree(m);
    let section = group(&groups, "Sections (Building Section)").entries[0].kind;
    assert_eq!(title(m, section), "Section: Section 1");
    let south = group(&groups, "Elevations (Building Elevation)").entries.iter().find(|e| e.label == "South").map(|e| e.kind);
    assert_eq!(title(m, south.expect("a south elevation")), "Elevation: South");
    assert_eq!(title(m, ViewKind::View3D(None)), "3D View: {3D}");
    assert_eq!(title(m, ViewKind::Schedule("room")), "Schedule: Room Schedule");
    let sheet = group(&groups, "Sheets (all)").entries[0].kind;
    assert_eq!(title(m, sheet), "Sheet: A101 - Ground Floor");
    assert_eq!(title(m, ViewKind::Nothing), "");
}

#[test]
fn folding_a_group_changes_the_browser_and_opening_it_again_changes_it_back() {
    let app = App::sample();
    let mut v = Views::default();
    v.rebuild(&app.session.model, ViewKind::Nothing);
    let before = v.rev;
    v.rebuild(&app.session.model, ViewKind::Nothing);
    assert_eq!(v.rev, before, "a model that has not changed leaves the browser as it was");
    assert!(v.folded.contains(FAMILIES) && !v.folded.contains("Floor Plans"), "the families start folded: the longest list");
    v.toggle("Floor Plans");
    assert!(v.folded.contains("Floor Plans") && v.rev > before);
    v.toggle("Floor Plans");
    assert!(!v.folded.contains("Floor Plans"));
    let shown = v.rev;
    v.rebuild(&app.session.model, ViewKind::Ceiling(app.levels[0].0));
    assert!(v.rev > shown, "a view with another title is a change");
}

fn built(app: &App) -> Views {
    let mut v = Views::default();
    v.rebuild(&app.session.model, ViewKind::Nothing);
    v
}

#[test]
fn a_tab_keeps_to_one_kind_of_group_and_all_has_every_one() {
    let app = App::sample();
    let mut v = built(&app);
    let all: Vec<&str> = v.rows("").iter().map(|(g, _)| g.label).collect();
    assert_eq!(all.len(), v.groups.len(), "All lists every group");
    for (tab, only) in [
        (Tab::Sheets, &["Sheets (all)"][..]),
        (Tab::Families, &["Families"][..]),
        (Tab::Schedules, &["Schedules/Quantities (all)"][..]),
        (Tab::Views, &["Floor Plans", "Ceiling Plans", "3D Views", "Elevations (Building Elevation)", "Sections (Building Section)"][..]),
    ] {
        v.set_tab(tab);
        let got: Vec<&str> = v.rows("").iter().map(|(g, _)| g.label).collect();
        assert_eq!(got, only, "{tab:?}");
    }
    let rev = v.rev;
    v.set_tab(Tab::Views);
    assert_eq!(v.rev, rev, "the tab that is in force is no change");
    assert_eq!(Tab::ALL.len(), 5);
    assert!(Tab::ALL.iter().all(|t| t.key().starts_with("tab-") && !t.name().is_empty()));
}

#[test]
fn the_families_are_every_type_of_the_project_and_a_press_on_one_is_its_type_properties() {
    let app = App::sample();
    let v = built(&app);
    let families = group(&v.groups, "Families");
    assert!(families.entries.len() > 5, "the system's types: {}", families.entries.len());
    assert!(families.entries.iter().all(|e| e.kind == ViewKind::Nothing && matches!(e.pick, Pick::Type(..))));
    let wall = families.entries.iter().find(|e| e.label.starts_with("Wall: ")).expect("a wall type");
    assert!(matches!(&wall.pick, Pick::Type(c, n) if c == "wall" && wall.label == format!("Wall: {n}")), "{wall:?}");
    let by = |e: &Entry| e.label.split_once(": ").map(|(c, n)| (c.to_lowercase(), n.to_lowercase())).unwrap_or_default();
    let keys: Vec<(String, String)> = families.entries.iter().map(by).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "by category and name");
}

#[test]
fn a_search_finds_what_has_every_word_in_its_name_or_its_groups_and_nothing_typed_is_all() {
    let app = App::sample();
    let mut v = built(&app);
    let level = app.levels[0].1.clone();
    let rows = v.rows(&format!("ceiling {}", level.to_lowercase()));
    assert_eq!(rows.len(), 1, "only the ceiling plans' group has something");
    assert_eq!(rows[0].0.label, "Ceiling Plans");
    assert_eq!(rows[0].1.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(), [level.as_str()]);
    assert!(v.rows("sec* 1").iter().any(|(g, e)| g.label == "Sections (Building Section)" && !e.is_empty()), "a wildcard");
    assert!(v.rows("zzz").is_empty() && v.rows("   ").len() == v.groups.len());
    assert_eq!(
        v.rows("sheets").iter().find(|(g, _)| g.label == "Sheets (all)").map(|(_, e)| e.len()),
        Some(3),
        "a group's name takes all its rows"
    );
    v.set_tab(Tab::Sheets);
    assert!(v.rows("a10*").iter().all(|(g, _)| g.label == "Sheets (all)"), "a tab and a search together");
    assert_eq!(v.order("a101").len(), 1);
}

#[test]
fn views_are_picked_together_as_a_list_does_and_folding_all_goes_through_every_group() {
    let app = App::sample();
    let mut v = built(&app);
    let order = v.order("");
    assert!(order.len() > 8 && order.iter().all(|k| *k != ViewKind::Nothing), "the views, in the order the browser shows them");
    let (a, b, c, d) = (order[0], order[1], order[2], order[3]);
    v.pick_one(a);
    assert_eq!(v.picked, [a]);
    v.pick(c, false, true, &order, a);
    assert_eq!(v.picked, [a, c], "Ctrl adds one");
    v.pick(c, false, true, &order, a);
    assert_eq!(v.picked, [a], "and takes it away again");
    v.pick_one(a);
    v.pick(d, true, false, &order, a);
    assert_eq!(v.picked, [a, b, c, d], "Shift picks the run from the last one picked");
    v.pick_one(a);
    v.pick(ViewKind::Section(7), true, false, &order, a);
    assert_eq!(v.picked, [a, ViewKind::Section(7)], "a view that is not listed is toggled, as Ctrl would");
    let mut w = built(&app);
    w.pick(c, false, true, &order, a);
    assert_eq!(w.picked, [a, c], "Ctrl with nothing picked counts the view on screen first");
    w.pick(d, true, true, &order, a);
    assert_eq!(w.picked, [a, c, d], "Shift and Ctrl add the run from the last one picked, to the one pressed");

    v.fold_all(true);
    assert!(v.groups.iter().all(|g| v.folded.contains(g.label)));
    v.fold_all(false);
    assert!(v.folded.is_empty());
}
