//! FE-11: Create > Datum > Reference Line in a family of any category that isn't adaptive (an adaptive family draws its own in
//! {3D}): two clicks in a plan or an elevation, a line on the view's work plane, selected once drawn; refused, saying why,
//! where the view sees the work plane edge on. Find lists it where its button is enabled.
use super::family_views::{click_family, editor, open, view};
use super::*;
use crate::editor::select_box::Picked;
use bimcraft_core::family::views::{Side, Which};
use bimcraft_core::family::{CENTER_FRONT_BACK, PlaneRef, REF_LEVEL};

const BOX: &str = r#"family {"name": "Box", "category": "generic", "types": {"T": {}}, "forms": [{"profile": [[500, 300], [1000, 300], [1000, 600], [500, 600]], "to": 2000}]}"#;

const WINDOW: &str = r#"family {"name": "Pane", "category": "window", "parameters": [{"name": "Width", "value": 900}, {"name": "Height", "value": 1200}], "types": {"T": {}}, "forms": [{"plane": "xz", "profile": [[-450, 0], [450, 0], [450, 1200], [-450, 1200]], "from": -40, "to": 40}]}"#;

/// What the status bar says.
fn status(h: &Harness<BimApp>) -> String {
    h.state().status.text.clone()
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 20.0)
}

/// Reference Line pressed: on the ribbon, or, in a window too narrow for it, from the Datum panel's list (Datum yields first,
/// keeping Reference Plane in sight).
fn reference_line(h: &mut Harness<BimApp>) {
    if h.query_by_label("Reference Line").is_none() {
        press(h, "Datum ▾");
    }
    press(h, "Reference Line");
}

#[test]
fn a_reference_line_drawn_in_plan_lies_on_ref_level_and_loads_with_the_family() {
    let (mut h, _) = open(BOX, "Box");
    view(&mut h, Some(Which::Plan));
    reference_line(&mut h);
    click_family(&mut h, [-1000.0, -800.0, 0.0]);
    click_family(&mut h, [1000.0, -800.0, 0.0]);
    assert!(h.query_all_by_label("Modify | Place Reference Line").count() > 0, "the contextual tab names the tool");
    assert_eq!(h.query_all_by_label("Modify | Place Reference Plane").count(), 0);
    let lines = &editor(&h).lines;
    assert_eq!(lines.len(), 1, "one line drawn");
    let l = &lines[0];
    assert!(near(l.start, [-1000.0, -800.0, 0.0]) && near(l.end, [1000.0, -800.0, 0.0]), "{l:?}");
    assert_eq!(l.host, PlaneRef::Plane(REF_LEVEL), "on the plan's work plane");
    assert_eq!(editor(&h).only_selected(), Some(Picked::Line(l.guid)), "selected once drawn");
    // The tool stays on for the next one, as Reference Plane's does.
    click_family(&mut h, [-1000.0, 800.0, 0.0]);
    click_family(&mut h, [-1000.0, 1600.0, 0.0]);
    assert_eq!(editor(&h).lines.len(), 2, "a second line");
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    press(&mut h, "Load into Project");
    if h.query_by_label("Overwrite the existing version").is_some() {
        press(&mut h, "Overwrite the existing version");
    }
    assert_eq!(h.state().session.model.families["Box"].lines.len(), 2, "the lines in the family loaded");
}

#[test]
fn a_reference_line_drawn_in_a_windows_front_elevation_lies_on_center_front_back() {
    let (mut h, _) = open(WINDOW, "Pane");
    view(&mut h, Some(Which::Elevation(Side::Front)));
    reference_line(&mut h);
    click_family(&mut h, [-300.0, 0.0, 200.0]);
    click_family(&mut h, [300.0, 0.0, 900.0]);
    let lines = &editor(&h).lines;
    assert_eq!(lines.len(), 1, "one line drawn: {}", status(&h));
    let l = &lines[0];
    assert!(near(l.start, [-300.0, 0.0, 200.0]) && near(l.end, [300.0, 0.0, 900.0]), "{l:?}");
    assert_eq!(l.host, PlaneRef::Plane(CENTER_FRONT_BACK), "on the elevation's work plane");
}

#[test]
fn a_work_plane_the_view_sees_edge_on_takes_no_reference_line_and_says_why() {
    let (mut h, _) = open(BOX, "Box");
    view(&mut h, Some(Which::Plan));
    h.state_mut().active_family_mut().unwrap().views.set_work_plane(PlaneRef::Plane(CENTER_FRONT_BACK));
    reference_line(&mut h);
    click_family(&mut h, [-1000.0, -800.0, 0.0]);
    click_family(&mut h, [1000.0, -800.0, 0.0]);
    assert!(editor(&h).lines.is_empty(), "no line");
    assert!(status(&h).contains("edge on"), "{}", status(&h));
}

#[test]
fn an_adaptive_family_has_no_reference_line_tool_in_its_plans() {
    let (mut h, ..) = super::family_temporary_hide::adaptive();
    view(&mut h, Some(Which::Plan));
    reference_line(&mut h);
    assert!(editor(&h).views.drawing.is_none(), "no Reference Line in an adaptive family's views");
}

#[test]
fn find_lists_reference_line_in_a_plan_and_turns_the_tool_on() {
    let (mut h, _) = open(BOX, "Box");
    view(&mut h, None);
    assert!(!h.state().family_find_actions().contains(&"Reference Line"), "not in {{3D}}");
    view(&mut h, Some(Which::Plan));
    assert!(h.state().family_find_actions().contains(&"Reference Line"));
    assert!(h.state_mut().family_find_action("Reference Line"));
    h.run();
    assert!(editor(&h).views.drawing.is_some() && editor(&h).views.drawing_line, "the Reference Line tool on");
}

#[test]
fn undo_takes_a_drawn_reference_line_away_and_redo_puts_it_back() {
    let (mut h, _) = open(BOX, "Box");
    view(&mut h, Some(Which::Plan));
    reference_line(&mut h);
    click_family(&mut h, [-1000.0, -800.0, 0.0]);
    click_family(&mut h, [1000.0, -800.0, 0.0]);
    key(&mut h, Key::Escape);
    let drawn = editor(&h).lines.clone();
    assert_eq!(drawn.len(), 1, "drawn");
    h.state_mut().family_undo();
    h.run();
    assert!(editor(&h).lines.is_empty(), "undone: gone");
    assert_eq!(editor(&h).only_selected(), None, "and gone from the selection");
    h.state_mut().family_redo();
    h.run();
    assert_eq!(editor(&h).lines, drawn, "redone: back as it was");
}

/// The Datum panel in a wide window and, folded, in a narrow one with its list open, and a line just drawn in plan,
/// selected, saved where `BIMCRAFT_REFERENCE_LINE_DUMP` says (only then).
#[test]
fn family_reference_line_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_REFERENCE_LINE_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let save = |h: &mut Harness<BimApp>, name: &str| {
        h.run();
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("fe11-{name}.png"))).unwrap();
    };
    for (w, name) in [(1920.0, "wide"), (1280.0, "narrow")] {
        let mut h = app(BOX);
        h.set_size(egui::vec2(w, 1000.0));
        h.state_mut().edit_family("Box");
        h.run();
        view(&mut h, Some(Which::Plan));
        save(&mut h, &format!("ribbon-{name}"));
        if h.query_by_label("Reference Line").is_none() {
            press(&mut h, "Datum ▾");
            save(&mut h, &format!("ribbon-{name}-list"));
        }
        press(&mut h, "Reference Line");
        click_family(&mut h, [-1000.0, -800.0, 0.0]);
        click_family(&mut h, [1000.0, -800.0, 0.0]);
        save(&mut h, &format!("drawn-{name}"));
    }
}
