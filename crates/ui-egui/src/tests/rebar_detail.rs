//! Structure > Reinforcement > Detail (DD): the Reinforcement Detail dialog over `rebar_detail`, and a detail's own tab.

use super::*;
use crate::rebar_detail::Onto;

const BEAM: &str = r#"beam {"points": [[1000, 1000], [5000, 1000]], "offset": 2800}"#;

/// A window on a beam, the beam selected; armed with 3 bottom bars of 16 mm and stirrups of 5 mm every 150 when `bars`.
fn window_on(window: fn(&str) -> Harness<'static, BimApp>, bars: bool) -> Harness<'static, BimApp> {
    let mut h = window(&format!("{BOX}\n{BEAM}"));
    let beam = ids(&h, "beam")[0];
    if bars {
        let spec =
            json!({"hosts": [beam], "cover": 25, "bottom": {"count": 3, "diameter": 16}, "stirrup": {"diameter": 5, "spacing": 150}});
        h.state_mut().session.execute("reinforce", &spec).unwrap();
    }
    h.state_mut().selection = vec![beam];
    h.run();
    h
}

#[test]
fn dd_opens_the_dialog_and_make_makes_the_detail_and_opens_its_tab_in_both_profiles() {
    let windows: [fn(&str) -> Harness<'static, BimApp>; 2] = [app, bimcraft];
    for window in windows {
        let mut h = window_on(window, true);
        let steps = h.state().session.history().len();
        shortcut(&mut h, "DD");
        assert!(h.state().detail_dialog.is_some(), "DD opens the dialog");
        press(&mut h, "Make");
        assert!(h.state().detail_dialog.is_none(), "it is made, so the dialog is done");
        let made = ids(&h, "rebardetail");
        assert_eq!(made.len(), 1);
        assert_eq!(h.state().session.history().len(), steps + 1, "one undo step");
        assert_eq!(h.state().sheet, Sheet::Table(made[0]));
        assert_eq!(h.state().showing_rebar_detail(), Some(made[0]));
        // Its tab draws, read-only: a frame passes and nothing it shows is selected.
        h.run();
        assert!(h.state().selection.len() <= 1);
        assert_eq!(h.state().rebar_details().len(), 1, "the Project Browser lists it");
    }
}

#[test]
fn a_beam_with_no_bars_is_refused_inside_the_dialog_and_nothing_changes() {
    let mut h = window_on(app, false);
    let steps = h.state().session.history().len();
    shortcut(&mut h, "DD");
    press(&mut h, "Make");
    let d = h.state().detail_dialog.as_ref().expect("a refusal keeps the dialog open");
    assert!(d.error.contains("reinforce"), "{}", d.error);
    assert_eq!((ids(&h, "rebardetail").len(), h.state().session.history().len()), (0, steps));
}

#[test]
fn dd_with_nothing_selected_says_what_to_select_and_opens_no_dialog() {
    let mut h = window_on(app, true);
    h.state_mut().selection.clear();
    shortcut(&mut h, "DD");
    assert!(h.state().detail_dialog.is_none());
    assert!(h.state().status.error && h.state().status.text.contains("beams"), "{}", h.state().status.text);
}

#[test]
fn a_station_that_is_not_a_length_is_said_inside_the_dialog() {
    let mut h = window_on(app, true);
    shortcut(&mut h, "DD");
    type_into(&mut h, "Stations", "abc");
    press(&mut h, "Make");
    let d = h.state().detail_dialog.as_ref().expect("the dialog stays");
    assert!(d.error.contains("abc"), "{}", d.error);
    assert!(ids(&h, "rebardetail").is_empty());
}

#[test]
fn the_dialog_can_place_the_detail_on_a_new_sheet_in_one_undo_step() {
    let mut h = window_on(app, true);
    let steps = h.state().session.history().len();
    shortcut(&mut h, "DD");
    h.state_mut().detail_dialog.as_mut().unwrap().onto = Onto::New;
    press(&mut h, "Make");
    let sheets = h.state().session.model.sheets();
    assert_eq!(sheets.len(), 1);
    assert_eq!(h.state().sheet, Sheet::Paper(sheets[0]));
    assert_eq!(h.state().session.history().len(), steps + 1, "the sheet and the detail are one step");
    let placed = h.state().session.model.elements.values().filter(|e| matches!(e, Element::Viewport(_))).count();
    assert_eq!(placed, 1);
    // Undo takes back both.
    h.state_mut().session.execute("undo", &json!({})).unwrap();
    assert!(h.state().session.model.sheets().is_empty() && ids(&h, "rebardetail").is_empty());
}

#[test]
fn the_structure_tab_has_a_detail_button() {
    let mut h = window_on(app, true);
    press(&mut h, "Structure");
    press(&mut h, "Detail");
    assert!(h.state().detail_dialog.is_some());
}

#[test]
fn dd_makes_the_detail_of_a_column_a_footing_and_a_floor_in_both_profiles() {
    let column = json!({"bars": {"diameter": 16, "x": 3, "y": 3}, "stirrup": {"diameter": 5, "spacing": 150}});
    let footing = json!({"mesh": {"x": {"diameter": 10, "spacing": 150}, "y": {"diameter": 10, "spacing": 150}}});
    let floor = json!({"bottom": {"x": {"diameter": 8, "spacing": 150}, "y": {"diameter": 8, "spacing": 200}}});
    let members = [
        (r#"structural_column {"at": [2000, 2000], "type": "Concrete 300 x 300", "height": 3000}"#, "structuralcolumn", column),
        (r#"footing {"at": [3000, 3000], "type": "Footing 1200 x 1200"}"#, "footing", footing),
        (r#"floor {"boundary": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]]}"#, "floor", floor),
    ];
    let windows: [fn(&str) -> Harness<'static, BimApp>; 2] = [app, bimcraft];
    for window in windows {
        for (script, category, spec) in &members {
            let mut h = window(&format!("{BOX}\n{script}"));
            let host = ids(&h, category)[0];
            let mut spec = spec.clone();
            spec["hosts"] = json!([host]);
            spec["cover"] = json!(30);
            h.state_mut().session.execute("reinforce", &spec).unwrap();
            h.state_mut().selection = vec![host];
            h.run();
            shortcut(&mut h, "DD");
            assert!(h.state().detail_dialog.is_some(), "DD opens the dialog for a {category}");
            press(&mut h, "Make");
            let made = ids(&h, "rebardetail");
            assert_eq!(made.len(), 1, "{category}");
            assert_eq!(h.state().sheet, Sheet::Table(made[0]));
            h.run();
        }
    }
}
