//! Structure > Quantities: the Steel List and Cut Plan dialog (BL and CU) over the `steel_list` and `cut_plan` commands.

use super::*;

const BEAM: &str = r#"beam {"points": [[1000, 1000], [5000, 1000]], "offset": 2800}"#;

/// A window on a beam armed with 3 bottom bars of 16 mm and stirrups of 5 mm every 150, the beam selected.
fn armed(window: fn(&str) -> Harness<'static, BimApp>) -> Harness<'static, BimApp> {
    let mut h = window(&format!("{BOX}\n{BEAM}"));
    let beam = ids(&h, "beam")[0];
    let spec = json!({"hosts": [beam], "cover": 25, "bottom": {"count": 3, "diameter": 16}, "stirrup": {"diameter": 5, "spacing": 150}});
    h.state_mut().session.execute("reinforce", &spec).unwrap();
    h.state_mut().selection = vec![beam];
    h.run();
    h
}

fn plan(h: &Harness<BimApp>) -> serde_json::Value {
    h.state().steel_dialog.as_ref().and_then(|d| d.plan.clone()).expect("a cut plan was calculated")
}

#[test]
fn bl_opens_the_steel_list_of_the_selected_beam_with_its_marks_and_summary_in_both_profiles() {
    let windows: [fn(&str) -> Harness<'static, BimApp>; 2] = [app, bimcraft];
    for window in windows {
        let mut h = armed(window);
        shortcut(&mut h, "BL");
        let d = h.state().steel_dialog.as_ref().expect("BL opens the dialog");
        assert!(!d.cut);
        let list = d.list.as_ref().expect("the list is made as it opens");
        let marks: Vec<&str> = list["rows"].as_array().unwrap().iter().filter_map(|r| r["mark"].as_str()).collect();
        assert_eq!(marks, ["N1", "N2"], "bottom bars first, then the stirrups: {list}");
        assert_eq!(list["totals"]["bars"], 31);
        assert_eq!(list["summary"].as_array().unwrap().len(), 2, "a line for each diameter");
        assert!(h.query_by_label("N2").is_some(), "the mark is on screen");
        press(&mut h, "Close");
        assert!(h.state().steel_dialog.is_none());
    }
}

#[test]
fn cu_asks_for_a_stock_length_and_says_so_inside_the_dialog() {
    let mut h = armed(app);
    let steps = h.state().session.history().len();
    shortcut(&mut h, "CU");
    assert!(h.state().steel_dialog.as_ref().is_some_and(|d| d.cut));
    press(&mut h, "Calculate");
    let d = h.state().steel_dialog.as_ref().expect("a refusal keeps the dialog open");
    assert!(d.error.contains("stock bar") && d.plan.is_none(), "{}", d.error);
    assert_eq!(h.state().session.history().len(), steps, "it reads the project and changes nothing");
}

#[test]
fn the_cut_plan_reads_the_stock_length_in_the_projects_units_and_notes_a_piece_too_long() {
    let mut h = armed(app);
    h.state_mut().run("units", json!({"length": "m"})).unwrap();
    shortcut(&mut h, "CU");
    type_into(&mut h, "Stock length", "12");
    press(&mut h, "Calculate");
    let p = plan(&h);
    assert_eq!(p["stock_length"], 12000.0, "12 in a project of metres is 12 000 mm");
    assert_eq!(p["diameters"].as_array().unwrap().len(), 2);
    assert!(p["totals"]["stock_bars"].as_u64().unwrap() >= 2);
    // Shorter than the bottom bars: they are a note, not a plan.
    type_into(&mut h, "Stock length", "3");
    press(&mut h, "Calculate");
    let notes = plan(&h)["notes"].to_string();
    assert!(notes.contains("longer than"), "{notes}");
}

#[test]
fn create_schedules_makes_the_list_and_the_summary_as_one_undo_step() {
    let mut h = armed(app);
    shortcut(&mut h, "BL");
    let steps = h.state().session.history().len();
    press(&mut h, "Create Schedules");
    assert!(h.state().steel_dialog.is_none(), "it closes with the list open");
    assert_eq!(h.state().session.history().len(), steps + 1);
    assert_eq!(ids(&h, "schedule").len(), 2);
    assert!(matches!(h.state().sheet, Sheet::Table(_)));
    h.state_mut().session.execute("undo", &json!({})).unwrap();
    assert!(ids(&h, "schedule").is_empty(), "one step takes both back");
}

#[test]
fn export_writes_the_rows_of_the_list_to_the_file_picked() {
    let mut h = armed(app);
    shortcut(&mut h, "BL");
    let file = tempfile::tempdir().unwrap().keep().join("steel.csv");
    press(&mut h, "Export…");
    let tx = crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).expect("a file dialog was begun");
    tx.send(Some(vec![file.clone()])).unwrap();
    h.run();
    let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{e}: {}", h.state().status.text));
    assert!(text.contains("N1") && text.contains("N2"), "{text}");
    assert!(h.state().steel_dialog.is_some(), "the dialog stays for the next change");
}

#[test]
fn the_structure_tab_has_the_quantities_panel_and_a_bar_shows_its_marks_in_properties() {
    let mut h = armed(app);
    press(&mut h, "Structure");
    press(&mut h, "Cut Plan");
    assert!(h.state().steel_dialog.as_ref().is_some_and(|d| d.cut));
    press(&mut h, "Close");
    let bar = ids(&h, "rebar")[0];
    h.state_mut().selection = vec![bar];
    h.run();
    for label in ["Host Mark", "Shape"] {
        assert!(h.query_by_label(label).is_some(), "Properties has a {label} row for a bar");
    }
}
