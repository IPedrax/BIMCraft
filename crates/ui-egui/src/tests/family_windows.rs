//! A family's window commands (Close Inactive Views, Tile and Tab Windows, Repeat Last Command), run as Find runs them,
//! work on tabs and tools, never on the draft.
use super::family_selection::command;
use super::*;
use bimcraft_core::family::views::Which;
use egui::Rect;

const FAMILIES: &str = r#"family {"name": "Box", "category": "generic", "types": {"T": {}}, "forms": [{"profile": [[0, 0], [500, 0], [500, 300], [0, 300]], "to": 400}]}
family {"name": "Crate", "category": "generic", "types": {"T": {}}, "forms": [{"profile": [[0, 0], [800, 0], [800, 600], [0, 600]], "to": 600}]}"#;

fn open() -> Harness<'static, BimApp> {
    let mut h = app(&format!("{BOX}\n{FAMILIES}"));
    h.set_size(vec2(1600.0, 1000.0));
    h.run();
    h.state_mut().edit_family("Box");
    h.run();
    super::family_views::view(&mut h, Some(Which::Plan));
    h
}

#[test]
fn tile_windows_is_a_document_local_session_layout() {
    let mut h = open();
    let box_doc = h.state().active_doc().unwrap();
    let before = h.state().active_family().unwrap().draft_family().unwrap();
    let project = h.state().session.model.to_json().unwrap();
    command(&mut h, "Tile Windows");
    h.run();
    assert!(h.state().active_family().unwrap().views.tiled);
    assert_eq!(h.state().sheet, Sheet::Family(box_doc, Some(Which::Plan)));
    assert_eq!(h.state().doc_tabs(box_doc).len(), 2);
    assert!(h.state().active_family().unwrap().draft_family().unwrap() == before);
    assert_eq!(h.state().session.model.to_json().unwrap(), project);
    h.state_mut().edit_family("Crate");
    h.run();
    assert!(!h.state().active_family().unwrap().views.tiled);
    h.state_mut().open_sheet(Sheet::Family(box_doc, None));
    h.run();
    assert!(h.state().active_family().unwrap().views.tiled);
    command(&mut h, "Tab Windows");
    h.run();
    assert!(!h.state().active_family().unwrap().views.tiled);
    assert_eq!(h.state().doc_tabs(box_doc).len(), 2);
    assert!(h.state().active_family().unwrap().draft_family().unwrap() == before);
}

#[test]
fn tile_rects_cover_clear_area_without_overlap() {
    let area = Rect::from_min_max(pos2(20.0, 50.0), pos2(620.0, 450.0));
    for count in 1..=7 {
        let tiles = crate::editor::document::family_tile_rects(area, count);
        assert_eq!(tiles.len(), count);
        for (i, tile) in tiles.iter().enumerate() {
            assert!(area.contains(tile.center()));
            assert!(tile.width() > 0.0 && tile.height() > 0.0);
            for other in tiles.iter().skip(i + 1) {
                assert!(!tile.contains(other.center()));
            }
        }
    }
}

#[test]
fn clicking_an_inactive_tile_activates_its_tab_without_editing_the_family() {
    let mut h = open();
    let doc = h.state().active_doc().unwrap();
    let before = h.state().active_family().unwrap().draft_family().unwrap();
    command(&mut h, "Tile Windows");
    h.run();
    let active = h.state().active_family().unwrap().views.pane.unwrap();
    click_at(&mut h, pos2(active.left() - active.width() / 2.0, active.center().y));
    h.run();
    assert_eq!(h.state().sheet, Sheet::Family(doc, None));
    assert!(h.state().active_family().unwrap().draft_family().unwrap() == before);
    assert!(h.state().active_family().unwrap().views.tiled);
}

#[test]
fn bars_keep_the_full_canvas_while_only_the_active_view_uses_a_tile() {
    let mut h = open();
    command(&mut h, "Tile Windows");
    h.run();
    let full = h.state().view_rect;
    let active = h.state().active_family().unwrap().views.pane.unwrap();
    assert!(full.width() > active.width());
    assert!(full.contains(active.center()));
    assert!(h.state().tabs_foot <= full.top());
}

#[test]
fn adaptive_family_keeps_its_full_modelling_canvas_when_tiled() {
    let mut h = app("");
    h.set_size(vec2(1600.0, 1000.0));
    h.state_mut().new_family(bimcraft_core::family::Category::Generic);
    h.state_mut().active_family_mut().unwrap().adaptive = Some(bimcraft_core::adaptive::Definition::default());
    h.run();
    super::family_views::view(&mut h, Some(Which::Plan));
    let before = h.state().view_rect;
    // Find leaves Tile Windows out in an adaptive family; tiled all the same, its modelling view keeps the whole canvas.
    assert!(!h.state().family_find_actions().contains(&"Tile Windows"));
    h.state_mut().active_family_mut().unwrap().views.tiled = true;
    h.run();
    assert_eq!(h.state().view_rect, before);
}

#[test]
fn passive_picture_cannot_replace_the_active_views_error() {
    let mut h = open();
    let ed = h.state_mut().active_family_mut().unwrap();
    ed.problem = Some("active view error".into());
    ed.passive_picture(|ed| ed.problem = Some("inactive view error".into()));
    assert_eq!(ed.problem.as_deref(), Some("active view error"));
    ed.passive_picture(|ed| ed.problem = None);
    assert_eq!(ed.problem.as_deref(), Some("active view error"));
}

/// Both layouts, tabbed and tiled, in the light and the dark look, when `BIMCRAFT_FAMILY_WINDOWS_DUMP` names a folder.
#[test]
fn family_window_layout_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_FAMILY_WINDOWS_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (look, dark) in [("light", false), ("dark", true)] {
        let mut h = open();
        h.ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
        super::family_views::view(&mut h, Some(Which::Elevation(bimcraft_core::family::views::Side::Front)));
        for _ in 0..3 {
            h.run();
        }
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("windows-tabs-{look}.png"))).unwrap();
        command(&mut h, "Tile Windows");
        for _ in 0..3 {
            h.run();
        }
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("windows-tiles-{look}.png"))).unwrap();
    }
}

#[test]
fn close_inactive_keeps_the_active_view_and_one_tab_per_other_document() {
    let mut h = open();
    let project = h.state().session.model.to_json().unwrap();
    let box_doc = h.state().active_doc().unwrap();
    h.state_mut().edit_family("Crate");
    h.run();
    let crate_doc = h.state().active_doc().unwrap();
    super::family_views::view(&mut h, Some(Which::Plan));
    h.state_mut().open_sheet(Sheet::Family(box_doc, Some(Which::Plan)));
    h.run();
    let draft = h.state().active_family().unwrap().draft_family().unwrap();
    assert_eq!(h.state().doc_tabs(box_doc).len(), 2);
    assert_eq!(h.state().doc_tabs(crate_doc).len(), 2);
    command(&mut h, "Close Inactive Views");
    assert_eq!(h.state().sheet, Sheet::Family(box_doc, Some(Which::Plan)));
    assert_eq!(h.state().doc_tabs(box_doc).len(), 1);
    assert_eq!(h.state().doc_tabs(crate_doc).len(), 1);
    assert_eq!(h.state().doc_tabs(crate_doc), vec![Sheet::Family(crate_doc, Some(Which::Plan))]);
    assert_eq!(h.state().families.len(), 2);
    assert!(h.state().active_family().unwrap().draft_family().unwrap() == draft);
    assert_eq!(h.state().session.model.to_json().unwrap(), project);
    assert!(h.state().doc_closing.is_none());
}

#[test]
fn repeat_last_command_rearms_a_tool_without_replaying_an_edit() {
    let mut h = open();
    let before = h.state().active_family().unwrap().draft_family().unwrap();
    shortcut(&mut h, "RP");
    assert!(h.state().active_family().unwrap().views.drawing.is_some());
    command(&mut h, "Modify");
    assert!(h.state().active_family().unwrap().views.drawing.is_none());
    command(&mut h, "Repeat Last Command");
    assert!(h.state().active_family().unwrap().views.drawing.is_some());
    assert!(h.state().active_family().unwrap().draft_family().unwrap() == before);
}

#[test]
fn repeat_last_command_is_document_local_and_ignores_mutations() {
    let mut h = open();
    shortcut(&mut h, "RP");
    let box_doc = h.state().active_doc().unwrap();
    h.state_mut().edit_family("Crate");
    h.run();
    super::family_views::view(&mut h, Some(Which::Plan));
    command(&mut h, "Repeat Last Command");
    assert!(h.state().active_family().unwrap().views.drawing.is_none(), "no tool recorded in Crate");
    h.state_mut().open_sheet(Sheet::Family(box_doc, Some(Which::Plan)));
    h.run();
    command(&mut h, "Modify");
    command(&mut h, "Repeat Last Command");
    assert!(h.state().active_family().unwrap().views.drawing.is_some(), "Box remembers its own tool");
}

#[test]
fn close_inactive_includes_floating_family_views() {
    let mut h = open();
    let box_doc = h.state().active_doc().unwrap();
    h.state_mut().edit_family("Crate");
    h.run();
    let crate_doc = h.state().active_doc().unwrap();
    super::family_views::view(&mut h, Some(Which::Plan));
    h.state_mut().open_sheet(Sheet::Family(box_doc, Some(Which::Plan)));
    let float = Sheet::Family(crate_doc, None);
    h.state_mut().views.retain(|s| *s != float);
    h.state_mut().floating.push(crate::dock::FloatView { sheet: float, at: pos2(0.0, 0.0), size: vec2(500.0, 400.0), edge: None });
    h.state_mut().close_inactive_family_views();
    assert!(h.state().floating.is_empty());
    assert_eq!(h.state().doc_tabs(crate_doc), vec![Sheet::Family(crate_doc, Some(Which::Plan))]);
}

#[test]
fn repeat_reports_a_blocked_tool_and_rearms_align_in_3d() {
    let mut h = open();
    h.state_mut().active_family_mut().unwrap().select_form(0);
    h.run();
    command(&mut h, "Move");
    assert_eq!(h.state().active_family().unwrap().last_tool, Some("Move"));
    command(&mut h, "Modify");
    command(&mut h, "Repeat Last Command");
    assert!(h.state().status.error && !h.state().status.text.is_empty(), "blocked Move reports why");

    let doc = h.state().active_doc().unwrap();
    h.state_mut().open_sheet(Sheet::Family(doc, None));
    h.run();
    command(&mut h, "Align");
    assert_eq!(h.state().active_family().unwrap().last_tool, Some("Align"));
    command(&mut h, "Modify");
    command(&mut h, "Repeat Last Command");
    assert!(h.state().active_family().unwrap().views.aligning.is_some(), "Align rearmed in {{3D}}");
}

#[test]
fn repeat_component_keeps_the_last_placed_type() {
    let mut h = open();
    let ed = h.state_mut().active_family_mut().unwrap();
    let family = serde_json::from_value(json!({"category": "generic", "types": {"A": {}, "B": {}}})).unwrap();
    ed.nested.push(bimcraft_core::family::NestedFamily { name: "Part".into(), family, ..Default::default() });
    ed.component_tool();
    ed.placing = Some(("Part".into(), "B".into()));
    let host = ed.views.work_plane();
    ed.place_nested(host, (10.0, 20.0), 0.0);
    assert!(ed.placing.is_none());
    ed.modify_tool();
    command(&mut h, "Repeat Last Command");
    assert_eq!(h.state().active_family().unwrap().placing, Some(("Part".into(), "B".into())));
}
