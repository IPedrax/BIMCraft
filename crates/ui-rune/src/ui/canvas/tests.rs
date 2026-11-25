//! The browser's views and the canvas that follows the one opened, driven headless as the window is.

use rune::{Env, Runtime};

use crate::app::App;
use crate::ui::view;
use crate::views::ViewKind;

fn window() -> Runtime<App> {
    let mut rt = Runtime::new(App::sample(), view, (1280.0, 800.0), Env::new(true, 1.0));
    for _ in 0..3 {
        rt.frame(0.0);
    }
    rt
}

fn tap(rt: &mut Runtime<App>, at: (f32, f32)) {
    rt.pointer_down(at.0, at.1);
    rt.pointer_up(at.0, at.1);
    for _ in 0..2 {
        rt.frame(0.0);
    }
}

fn text_of(rt: &Runtime<App>, key: &str) -> Option<String> {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).and_then(|e| e.text)
}

/// Opens a view from outside any handler, and lets the window draw it.
fn open(rt: &mut Runtime<App>, kind: ViewKind) {
    rt.update(|s, _| {
        s.open_view(kind);
    });
    for _ in 0..3 {
        rt.frame(0.0);
    }
}

#[test]
fn the_browser_has_a_row_for_every_view_and_a_press_on_a_group_folds_it() {
    let mut rt = window();
    let keys: Vec<String> = rt.state.views.groups.iter().flat_map(|g| g.entries.iter().map(|e| e.key.clone())).collect();
    assert!(keys.len() > 10);
    for key in &keys {
        assert!(rt.find(key).is_some(), "a row for {key}");
    }
    let rows: Vec<_> = rt.elements().into_iter().filter(|e| e.role == "button").filter_map(|e| e.key).collect();
    assert!(
        rows.iter().any(|k| k == "group-Floor Plans") && rows.iter().any(|k| k == "group-Sheets (all)"),
        "the groups are buttons: {rows:?}"
    );

    let level = rt.state.levels[0].0;
    let row = format!("level-{level}");
    let head = rt.find("group-Floor Plans").expect("the Floor Plans group");
    tap(&mut rt, head);
    assert!(rt.state.views.folded.contains("Floor Plans") && rt.find(&row).is_none(), "folded: its rows go");
    assert!(rt.find(&format!("ceiling-{level}")).is_some(), "the other groups stay");
    let head = rt.find("group-Floor Plans").expect("the header stays");
    tap(&mut rt, head);
    assert!(rt.state.views.folded.is_empty() && rt.find(&row).is_some(), "opened again: its rows are back");
}

#[test]
fn each_kind_of_view_has_the_canvas_of_its_own_and_a_plan_comes_back() {
    let mut rt = window();
    assert!(rt.find("plan").is_some() && rt.find("section").is_none());
    let first = |rt: &Runtime<App>, group: &str| {
        rt.state
            .views
            .groups
            .iter()
            .find(|g| g.label == group)
            .and_then(|g| g.entries.first())
            .map(|e| e.kind)
            .expect("a view in the group")
    };
    let canvases = ["plan", "section", "sheet", "schedule", "view3d"];
    for (group, canvas) in [
        ("Sections (Building Section)", "section"),
        ("Sheets (all)", "sheet"),
        ("Schedules/Quantities (all)", "schedule"),
        ("3D Views", "view3d"),
    ] {
        let kind = first(&rt, group);
        open(&mut rt, kind);
        for other in canvases {
            assert_eq!(rt.find(other).is_some(), other == canvas, "{group}: the {other} canvas");
        }
    }
    assert!(rt.find("view-pending").is_none(), "no view is left without a canvas");
    assert!(text_of(&rt, "status-line").unwrap_or_default().starts_with("3D View: "), "the status bar names the view");

    let level = rt.state.levels[0].0;
    open(&mut rt, ViewKind::Ceiling(level));
    assert!(rt.find("plan").is_some() && rt.find("view3d").is_none(), "a ceiling plan is the plan canvas's");
    assert!(rt.state.drawing.is_some());
    assert!(text_of(&rt, "status-line").unwrap_or_default().starts_with("Ceiling Plan: "));
}

#[test]
fn a_press_on_a_plan_view_row_opens_it_and_the_selection_stays() {
    let mut rt = window();
    let level = rt.state.levels[0].0;
    let id = rt.state.drawing.as_ref().and_then(|d| d.owners.iter().flatten().next().copied()).expect("something is drawn");
    rt.update(|s, _| s.select(vec![id]));
    rt.frame(0.0);
    let other = rt.state.levels.iter().map(|(l, _)| *l).find(|l| *l != level).expect("a second level");
    let row = rt.find(&format!("level-{other}")).expect("its row");
    tap(&mut rt, row);
    assert_eq!(rt.state.view, ViewKind::Plan(other));
    assert_eq!(rt.state.selection, vec![id], "the selection is the model's: the browser's press leaves it");
}

#[test]
fn the_export_buttons_ask_the_dialog_and_write_the_view_shown() {
    let dir = std::env::temp_dir().join(format!("bimcraft-rune-ui-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    let mut rt = window();
    assert!(rt.find("export-pdf").is_some() && rt.find("export-png").is_some());
    let png = rt.find("export-png").expect("the PNG button");
    tap(&mut rt, png);
    assert_eq!(rt.file_dialog().map(|(kind, _)| kind), Some(rune::FileDialogKind::Save), "it asks where");
    let file = dir.join("plan.png");
    rt.resolve_file_dialog(Some(file.clone())).expect("the dialog answers");
    rt.frame(0.0);
    assert!(file.exists(), "written through the engine: {:?}", rt.state.message);
    assert!(text_of(&rt, "status-line").unwrap_or_default().starts_with("Written to "));

    let pdf = rt.find("export-pdf").expect("the PDF button");
    tap(&mut rt, pdf);
    rt.resolve_file_dialog(None).expect("a cancelled dialog");
    rt.frame(0.0);
    assert!(rt.state.message.as_deref().is_some_and(|m| m.starts_with("Written to ")), "cancelling says nothing new");
    std::fs::remove_dir_all(&dir).ok();
}
