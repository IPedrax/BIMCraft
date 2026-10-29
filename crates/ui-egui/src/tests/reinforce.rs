//! Structure > Reinforce: the dialog that arms the selected beam, column, footing or floor through the `reinforce` command.

use super::*;

const BEAM: &str = r#"beam {"points": [[1000, 1000], [5000, 1000]], "offset": 2800}"#;

fn bars(h: &Harness<BimApp>) -> Vec<Id> {
    ids(h, "rebar")
}

fn select(h: &mut Harness<BimApp>, id: Id) {
    h.state_mut().selection = vec![id];
    h.run();
}

fn fill(h: &mut Harness<BimApp>, fields: &[(&str, &str)]) {
    for (label, text) in fields {
        type_into(h, label, text);
    }
}

#[test]
fn the_reinforce_dialog_arms_the_selected_beam_as_one_step() {
    let mut h = app(&format!("{BOX}\n{BEAM}"));
    let beam = ids(&h, "beam")[0];
    select(&mut h, beam);
    press(&mut h, "Structure");
    press(&mut h, "Reinforce");
    assert!(h.state().reinforce_dialog.is_some());
    fill(
        &mut h,
        &[
            ("Cover", "25"),
            ("Bottom bars: Count", "3"),
            ("Bottom bars: Diameter", "16"),
            ("Stirrups: Diameter", "5"),
            ("Stirrups: Spacing", "150"),
        ],
    );
    // Nothing is the project's until OK.
    assert!(bars(&h).is_empty());
    let steps = h.state().session.history().len();
    press(&mut h, "OK");
    assert!(h.state().reinforce_dialog.is_none(), "OK closes it");
    // Three bars below and 28 stirrups from 27.5 to 3972.5 mm, no further apart than 150.
    assert_eq!(bars(&h).len(), 31);
    assert_eq!(h.state().session.history().len(), steps + 1);
    assert!(h.state().status.text.contains("31"), "{}", h.state().status.text);
    let layers: std::collections::BTreeSet<String> =
        bars(&h).iter().map(|id| field(&h, *id, "layer").as_str().unwrap_or("").to_string()).collect();
    assert_eq!(layers, ["bottom", "stirrup"].map(String::from).into());
}

#[test]
fn a_refusal_stays_in_the_dialog_with_what_to_change_and_changes_nothing() {
    let mut h = app(&format!("{BOX}\n{BEAM}"));
    let beam = ids(&h, "beam")[0];
    select(&mut h, beam);
    shortcut(&mut h, "RF");
    assert!(h.state().reinforce_dialog.is_some());
    // Six 25 mm bars in 200 mm touch.
    fill(&mut h, &[("Cover", "25"), ("Bottom bars: Count", "6"), ("Bottom bars: Diameter", "25")]);
    press(&mut h, "OK");
    let d = h.state().reinforce_dialog.as_ref().expect("a refusal keeps the dialog open");
    assert!(d.error.contains("clearance"), "{}", d.error);
    assert!(bars(&h).is_empty());
    // A value that is not a number is said, by its field, before the command is asked.
    type_into(&mut h, "Bottom bars: Count", "x");
    press(&mut h, "OK");
    assert!(
        h.state().reinforce_dialog.as_ref().is_some_and(|d| d.error.contains("Bottom bars: Count")),
        "{:?}",
        h.state().reinforce_dialog.as_ref().map(|d| &d.error)
    );
    press(&mut h, "Cancel");
    assert!(h.state().reinforce_dialog.is_none());
}

#[test]
fn reinforce_wants_a_beam_a_column_a_footing_or_a_floor_selected() {
    let mut h = app(&format!("{BOX}\n{BEAM}"));
    press(&mut h, "Structure");
    press(&mut h, "Reinforce");
    assert!(h.state().reinforce_dialog.is_none());
    assert!(h.state().status.error && h.state().status.text.contains("Select a beam"), "{}", h.state().status.text);
    let wall = ids(&h, "wall")[0];
    select(&mut h, wall);
    shortcut(&mut h, "RF");
    assert!(h.state().reinforce_dialog.is_none());
    let beams = ids(&h, "beam");
    h.state_mut().selection = vec![beams[0], wall];
    h.run();
    shortcut(&mut h, "RF");
    assert!(h.state().reinforce_dialog.is_none(), "one element, not two");
}

#[test]
fn a_column_has_its_own_fields_and_the_dialog_keeps_what_was_typed_for_the_next() {
    let mut h = app(&format!(
        "{BOX}\nstructural_column {{\"at\": [2000, 2000], \"height\": 3000}}\nstructural_column {{\"at\": [6000, 2000], \"height\": 3000}}"
    ));
    let cols = ids(&h, "structuralcolumn");
    select(&mut h, cols[0]);
    shortcut(&mut h, "RF");
    assert!(h.query_by_label("Bottom bars: Count").is_none());
    fill(&mut h, &[("Cover", "25"), ("Bars: Diameter", "16"), ("Stirrups: Diameter", "5"), ("Stirrups: Spacing", "150")]);
    press(&mut h, "OK");
    // Four corner bars and 21 stirrups.
    assert_eq!(bars(&h).len(), 25);
    select(&mut h, cols[1]);
    shortcut(&mut h, "RF");
    assert_eq!(field_text(&h, "Cover"), "25");
    assert_eq!(field_text(&h, "Stirrups: Spacing"), "150");
    press(&mut h, "OK");
    assert_eq!(bars(&h).len(), 50);
}

#[test]
fn remove_bars_takes_away_the_bars_the_command_made_and_leaves_the_ones_drawn_by_hand() {
    let mut h = app(&format!("{BOX}\n{BEAM}"));
    let beam = ids(&h, "beam")[0];
    let by_hand = h
        .state_mut()
        .session
        .execute("rebar", &json!({"points": [[1100, 1000, 2400], [4900, 1000, 2400]], "diameter": 20, "host": beam}))
        .unwrap();
    assert!(by_hand["ids"].as_array().is_some_and(|v| v.len() == 1));
    select(&mut h, beam);
    shortcut(&mut h, "RF");
    // Nothing made yet to remove.
    assert!(h.query_by_label("Remove Bars").is_none());
    fill(&mut h, &[("Cover", "25"), ("Bottom bars: Count", "2"), ("Bottom bars: Diameter", "16")]);
    press(&mut h, "OK");
    assert_eq!(bars(&h).len(), 3);
    shortcut(&mut h, "RF");
    press(&mut h, "Remove Bars");
    assert!(h.state().reinforce_dialog.is_none());
    assert_eq!(bars(&h).len(), 1, "the bar drawn by hand stays");
}

#[test]
fn a_footing_and_a_floor_have_their_own_forms() {
    let mut h = app(&format!(
        "{BOX}\nfooting {{\"at\": [3000, 3000], \"type\": \"Footing 1200 x 1200\"}}\nfloor {{\"boundary\": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]]}}"
    ));
    let footing = ids(&h, "footing")[0];
    select(&mut h, footing);
    shortcut(&mut h, "RF");
    fill(
        &mut h,
        &[
            ("Cover", "50"),
            ("Mesh along width: Diameter", "10"),
            ("Mesh along width: Spacing", "150"),
            ("Mesh along depth: Diameter", "10"),
            ("Mesh along depth: Spacing", "150"),
            ("Ends turned up: Hook", "100"),
        ],
    );
    press(&mut h, "OK");
    // Nine bars each way.
    assert_eq!(bars(&h).len(), 18);
    let floor = ids(&h, "floor")[0];
    select(&mut h, floor);
    shortcut(&mut h, "RF");
    // A form of its own: what was typed for the footing is not in it.
    assert!(field_text(&h, "Cover").is_empty());
    fill(&mut h, &[("Cover", "20"), ("Bottom, bars along east: Diameter", "8"), ("Bottom, bars along east: Spacing", "150")]);
    press(&mut h, "OK");
    // Twenty-one bars across the 3 m.
    assert_eq!(bars(&h).len(), 18 + 21);
}

#[test]
fn several_beams_selected_are_armed_alike_and_a_mixed_selection_is_refused() {
    let mut h = app(&format!(
        "{BOX}\n{BEAM}\nbeam {{\"points\": [[1000, 2000], [4000, 2000]], \"offset\": 2800}}\nstructural_column {{\"at\": [6000, 2000], \"height\": 3000}}"
    ));
    let beams = ids(&h, "beam");
    let column = ids(&h, "structuralcolumn")[0];
    // A beam and a column: no one form fits both.
    h.state_mut().selection = vec![beams[0], column];
    h.run();
    shortcut(&mut h, "RF");
    assert!(h.state().reinforce_dialog.is_none());
    assert!(h.state().status.error && h.state().status.text.contains("one kind"), "{}", h.state().status.text);
    // Two beams: one dialog, one command, one step.
    h.state_mut().selection = beams.clone();
    h.run();
    shortcut(&mut h, "RF");
    assert!(h.state().reinforce_dialog.is_some());
    fill(&mut h, &[("Cover", "25"), ("Bottom bars: Count", "2"), ("Bottom bars: Diameter", "16"), ("Bottom bars: Hook", "200")]);
    let steps = h.state().session.history().len();
    press(&mut h, "OK");
    assert_eq!(h.state().session.history().len(), steps + 1);
    assert_eq!(bars(&h).len(), 4, "two bars on each beam");
    // A hook typed is a leg at each end: four points.
    assert!(bars(&h).iter().all(|id| field(&h, *id, "path").as_array().map(Vec::len) == Some(4)));
    assert!(h.state().status.text.contains("4"), "{}", h.state().status.text);
}

#[test]
fn a_preset_is_saved_from_the_fields_chosen_for_another_beam_and_deleted() {
    let mut h = app(&format!("{BOX}\n{BEAM}\nbeam {{\"points\": [[1000, 2000], [4000, 2000]], \"offset\": 2800}}"));
    let beams = ids(&h, "beam");
    select(&mut h, beams[0]);
    shortcut(&mut h, "RF");
    fill(
        &mut h,
        &[
            ("Cover", "25"),
            ("Bottom bars: Count", "3"),
            ("Bottom bars: Diameter", "16"),
            ("Stirrups: Diameter", "5"),
            ("Stirrups: Spacing", "150"),
        ],
    );
    // A preset needs a name.
    press(&mut h, "Save Preset");
    assert!(h.state().reinforce_dialog.as_ref().is_some_and(|d| d.error.contains("name")));
    fill(&mut h, &[("Preset: Name", "V 20 x 50")]);
    press(&mut h, "Save Preset");
    assert!(
        h.state().reinforce_dialog.as_ref().is_some_and(|d| d.error.is_empty()),
        "{:?}",
        h.state().reinforce_dialog.as_ref().map(|d| &d.error)
    );
    let saved = &h.state().session.model.reinforcement["V 20 x 50"];
    assert_eq!(saved.kind, "beam");
    assert_eq!(saved.params["bottom"], json!({"count": 3, "diameter": 16.0}));
    assert_eq!(saved.params["cover"], json!(25.0));
    press(&mut h, "Cancel");
    assert!(bars(&h).is_empty(), "saving a preset arms nothing");
    // Another beam: choose it, and the fields are as they were saved.
    select(&mut h, beams[1]);
    shortcut(&mut h, "RF");
    type_into(&mut h, "Cover", "");
    type_into(&mut h, "Bottom bars: Count", "");
    choose(&mut h, "Preset:", "V 20 x 50");
    assert_eq!(field_text(&h, "Cover"), "25");
    assert_eq!(field_text(&h, "Bottom bars: Count"), "3");
    assert_eq!(field_text(&h, "Stirrups: Spacing"), "150");
    press(&mut h, "OK");
    // 3 bars and the stirrups of a 3 m beam.
    assert_eq!(bars(&h).len(), 3 + 21);
    // Deleting takes it away.
    shortcut(&mut h, "RF");
    choose(&mut h, "Preset:", "V 20 x 50");
    press(&mut h, "Delete Preset");
    assert!(h.state().session.model.reinforcement.is_empty());
    press(&mut h, "Cancel");
}

#[test]
fn a_round_column_a_floors_hooks_and_a_ties_tails_have_their_fields() {
    let mut h = app(&format!(
        "{BOX}\nstructural_column {{\"at\": [2000, 2000], \"type\": \"Concrete Round 400\", \"height\": 3000}}\nstructural_column {{\"at\": [6000, 2000], \"height\": 3000}}\nfloor {{\"boundary\": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]]}}"
    ));
    let cols = ids(&h, "structuralcolumn");
    // A round column: bars are counted, and there are no ties; a rectangular one has the others.
    select(&mut h, cols[0]);
    shortcut(&mut h, "RF");
    assert!(
        h.query_by_label("Bars: Count").is_some()
            && h.query_by_label("Bars: Along width").is_none()
            && h.query_by_label("Ties: Diameter").is_none()
    );
    fill(
        &mut h,
        &[("Cover", "25"), ("Bars: Diameter", "16"), ("Bars: Count", "8"), ("Stirrups: Diameter", "5"), ("Stirrups: Spacing", "150")],
    );
    press(&mut h, "OK");
    assert_eq!(bars(&h).len(), 8 + 21);
    select(&mut h, cols[1]);
    shortcut(&mut h, "RF");
    assert!(h.query_by_label("Bars: Count").is_none() && h.query_by_label("Bars: Along width").is_some());
    fill(
        &mut h,
        &[
            ("Cover", "25"),
            ("Bars: Diameter", "16"),
            ("Bars: Along width", "3"),
            ("Bars: Along depth", "3"),
            ("Stirrups: Diameter", "5"),
            ("Stirrups: Spacing", "150"),
            ("Ties: Diameter", "6.3"),
            ("Ties: Hook", "50"),
        ],
    );
    press(&mut h, "OK");
    let tied: Vec<Id> = bars(&h).into_iter().filter(|id| field(&h, *id, "layer") == json!("tie")).collect();
    assert_eq!(tied.len(), 42);
    assert!(tied.iter().all(|id| field(&h, *id, "path").as_array().map(Vec::len) == Some(4)));
    // A floor: a hook turned up from the bottom bars.
    let floor = ids(&h, "floor")[0];
    select(&mut h, floor);
    shortcut(&mut h, "RF");
    fill(
        &mut h,
        &[
            ("Cover", "20"),
            ("Bottom, bars along east: Diameter", "8"),
            ("Bottom, bars along east: Spacing", "150"),
            ("Bottom ends turned up: Hook", "100"),
        ],
    );
    press(&mut h, "OK");
    let low: Vec<Id> = bars(&h).into_iter().filter(|id| field(&h, *id, "layer") == json!("bottom x")).collect();
    assert_eq!(low.len(), 21);
    assert!(low.iter().all(|id| field(&h, *id, "path").as_array().map(Vec::len) == Some(4)));
}
