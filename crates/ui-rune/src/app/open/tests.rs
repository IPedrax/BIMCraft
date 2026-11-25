use bimcraft_core::geom::P2;
use serde_json::json;

use super::*;

/// The first view of a group of the browser.
fn first_of(app: &App, label: &str) -> ViewKind {
    app.views.groups.iter().find(|g| g.label == label).and_then(|g| g.entries.first()).map(|e| e.kind).expect("a view in the group")
}

#[test]
fn opening_a_view_keeps_the_selection_and_drops_what_belongs_to_the_view() {
    let mut app = App::sample();
    let level = app.level.expect("a level");
    let id = app.drawing.as_ref().and_then(|d| d.owners.iter().flatten().next().copied()).expect("something is drawn");
    app.select(vec![id]);
    app.set_tool("wall");
    app.picked.push(P2(1.0, 1.0));
    app.region = true;
    let revision = app.revision;

    assert!(app.open_view(ViewKind::Ceiling(level)));
    assert_eq!((app.view, app.level), (ViewKind::Ceiling(level), Some(level)));
    assert_eq!(app.selection, vec![id], "the selection is the model's, not a view's");
    assert!(app.picked.is_empty() && !app.region, "what a person began in one view does not carry to the next");
    assert!(app.revision > revision && app.drawing.is_some() && app.fitted, "the ceiling plan is drawn and framed");
    assert_eq!(app.views.title, format!("Ceiling Plan: {}", app.level_name));
    assert!(app.status().starts_with("Ceiling Plan: "), "the status bar names the view: {}", app.status());
    assert!(!app.open_view(ViewKind::Ceiling(level)), "a view already on screen is not opened again");

    app.set_level(level);
    assert_eq!(app.view, ViewKind::Plan(level), "a level's name in the browser is its floor plan");
    assert_eq!(app.selection, vec![id]);
}

#[test]
fn a_view_with_no_canvas_yet_has_no_drawing_and_the_plan_comes_back_framed() {
    let mut app = App::sample();
    let (level, name) = (app.level.expect("a level"), app.level_name.clone());
    let id = app.drawing.as_ref().and_then(|d| d.owners.iter().flatten().next().copied()).expect("something is drawn");
    app.select(vec![id]);
    app.hover(40.0, 40.0);
    let section = first_of(&app, "Sections (Building Section)");

    assert!(app.open_view(section));
    assert_eq!(app.view, section);
    assert!(app.drawing.is_none(), "the plan canvas has nothing to draw for a section");
    assert_eq!(app.level, Some(level), "the level is the last one shown");
    assert!(app.cursor.is_none() && app.hovered.is_none(), "the pointer was over the plan, not the section");
    assert_eq!(app.selection, vec![id]);
    assert!(app.status().starts_with("Section: Section 1"), "{}", app.status());

    assert!(app.open_view(ViewKind::Plan(level)));
    assert!(app.drawing.is_some() && app.fitted);
    assert_eq!(app.status().split("   |   ").next(), Some(name.as_str()));
    assert_eq!(app.selection, vec![id], "still the model's selection");
}

#[test]
fn every_kind_the_browser_lists_opens_and_a_stale_one_changes_nothing() {
    let mut app = App::sample();
    let kinds: Vec<ViewKind> = app.views.groups.iter().flat_map(|g| g.entries.iter().map(|e| e.kind)).collect();
    assert!(kinds.len() > 10);
    for kind in kinds {
        app.open_view(kind);
        assert_eq!(app.view, kind, "{kind:?} opens");
        assert_eq!(app.drawing.is_some(), kind.is_plan(), "only a plan has a drawing for the plan canvas: {kind:?}");
    }
    let shown = app.view;
    let revision = app.revision;
    for stale in
        [ViewKind::Section(7), ViewKind::Sheet(7), ViewKind::Table(7), ViewKind::Plan(7), ViewKind::Schedule("nope"), ViewKind::Nothing]
    {
        assert!(!app.open_view(stale), "{stale:?}");
    }
    assert_eq!((app.view, app.revision), (shown, revision), "a refusal changes nothing");
}

#[test]
fn a_view_that_goes_falls_back_to_the_plan_of_its_level() {
    let mut app = App::sample();
    let (level, name) = (app.level.expect("a level"), app.level_name.clone());
    assert!(app.run("duplicate_view", &json!({"view": name})), "refused: {:?}", app.message);
    let copy = app.views.groups[0].entries[1].kind;
    let ViewKind::Plan(view) = copy else { panic!("a plan view: {copy:?}") };
    assert!(app.open_view(copy));
    assert_eq!((app.view, app.level), (copy, Some(level)), "a plan view stands on its level");
    assert!(app.drawing.is_some());

    assert!(app.run("delete", &json!({"ids": [view]})), "refused: {:?}", app.message);
    assert_eq!(app.view, ViewKind::Plan(level), "the view was deleted: its level's plan is shown");
    assert!(app.drawing.is_some());
    assert!(app.undo());
    assert_eq!(app.view, ViewKind::Plan(level), "undo gives the plan view back to the browser, not to the screen");
    assert!(app.views.groups[0].entries.iter().any(|e| e.kind == copy));
}

#[test]
fn a_ceiling_plan_zooms_and_frames_as_a_plan_does() {
    let mut app = App::sample();
    let level = app.level.expect("a level");
    app.open_view(ViewKind::Ceiling(level));
    let framed = app.camera;
    app.zoom_by(2.0);
    assert!((app.camera.scale - framed.scale * 2.0).abs() < 1e-9 && app.drawing.is_some());
    app.zoom_fit();
    assert!((app.camera.scale - framed.scale).abs() < 1e-9, "Zoom to Fit frames the ceiling plan again");
}

#[test]
fn a_view_is_opened_by_its_short_name_and_a_name_that_is_none_opens_nothing() {
    let mut app = App::sample();
    for (name, is) in [
        ("section", (|k: ViewKind| matches!(k, ViewKind::Section(_))) as fn(ViewKind) -> bool),
        ("elevation", |k| matches!(k, ViewKind::Section(_))),
        ("sheet", |k| matches!(k, ViewKind::Sheet(_))),
        ("schedule", |k| matches!(k, ViewKind::Schedule(_))),
        ("ceiling", |k| matches!(k, ViewKind::Ceiling(_))),
        ("3d", |k| matches!(k, ViewKind::View3D(_))),
    ] {
        assert!(app.open_named(name), "{name}");
        assert!(is(app.view), "{name} opened {:?}", app.view);
    }
    let shown = app.view;
    assert!(!app.open_named("nonsense") && !app.open_named("") && app.view == shown);
    assert!(!app.open_named("3d"), "it is the view already shown");
}
