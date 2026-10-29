//! PyAddins: the tab, the Script Automation palette, scripts an agent writes into the folder showing up, and runs from the
//! palette and from a ribbon button, with Stop and Restart. The runs need a Python on the machine; without one those tests
//! say so and pass.

use super::*;
use crate::dock::{Docking, Palette, Side};
use bimcraft_pyaddins::{Button, Store};

/// The app on `script`'s model, with a PyAddins folder of its own.
fn with_folder(script: &str) -> (Harness<'static, BimApp>, tempfile::TempDir, Store) {
    let mut h = app(script);
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    h.state_mut().use_pyaddins(store.clone());
    h.run();
    (h, dir, store)
}

fn has_python() -> bool {
    match bimcraft_pyaddins::python::find(None) {
        Ok(_) => true,
        Err(e) => {
            eprintln!("skipped, no Python here: {e}");
            false
        }
    }
}

/// Frames, the window's clock going on, until the run is over (a real process, so real time passes too).
fn until_done(h: &mut Harness<BimApp>) {
    for _ in 0..500 {
        h.step();
        if h.state().pyaddins.running.is_none() {
            h.run();
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the script did not end");
}

/// Clicks what starts a script: while it runs the window repaints every frame, so frames are stepped, not run to rest.
fn start(h: &mut Harness<BimApp>, label: &str) {
    // Below the fold of a long form too, as a screen reader presses it.
    h.get_by_label(label).click_accesskit();
    h.step();
    h.step();
}

/// Frames for the window's clock to pass the folder's next look.
fn a_moment(h: &mut Harness<BimApp>) {
    h.run_steps(5);
    h.run();
}

fn wall_count(h: &Harness<BimApp>) -> usize {
    h.state().session.model.elements.values().filter(|e| matches!(e, Element::Wall(_))).count()
}

const ROW: &str = r#""""Walls in a row, one above the other."""
__title__ = "Row of Walls"
import bimcraft as bc

def main(count: int = 2, length: bc.Length = 3000):
    """Walls in a row.

    count: how many walls
    """
    made = []
    for i in range(count):
        made.extend(bc.wall(points=[[0, 10000 + i * 2000], [length, 10000 + i * 2000]])["ids"])
    print("made", len(made))
    bc.select(made)
    return "%d walls" % len(made)
"#;

#[test]
fn the_player_starts_closed_and_opens_on_the_right_from_the_pyaddins_tab() {
    let (mut h, _dir, _store) = with_folder(BOX);
    assert!(!h.state().docking.shown(Palette::Scripts), "a fresh window has it closed");
    press(&mut h, "PyAddins");
    press(&mut h, "Player");
    assert_eq!(h.state().docking.docked(Palette::Scripts), Some(Side::Right));
    assert!(h.query_by_label("Script Automation").is_some());
    assert!(h.query_by_label("Change Wall Type").is_some(), "a new folder starts with the samples");
    // Its button closes it again.
    press(&mut h, "Player");
    assert!(!h.state().docking.shown(Palette::Scripts));
}

#[test]
fn settings_from_before_the_player_keep_it_closed() {
    // What the settings file kept before there was a Script Automation palette: it names neither placed nor closed.
    let old =
        json!({"left": [{"tabs": ["Properties"]}, {"tabs": ["Project Browser"]}], "right": [], "bottom": [], "floats": [], "closed": []});
    let d = Docking::from_json(&old);
    assert!(d.shown(Palette::Properties) && d.shown(Palette::Browser));
    assert!(!d.shown(Palette::Scripts));
    assert!(!Docking::default().shown(Palette::Scripts));
    // Kept open, it opens again where it was.
    let mut d = Docking::default();
    d.show(Palette::Scripts, true);
    let back = Docking::from_json(&d.to_json());
    assert_eq!(back.docked(Palette::Scripts), Some(Side::Right));
}

#[test]
fn a_script_and_a_button_an_agent_writes_appear_as_they_come() {
    let (mut h, _dir, store) = with_folder(BOX);
    press(&mut h, "PyAddins");
    press(&mut h, "Player");
    assert!(h.query_by_label("Row").is_none());
    // An agent writes into the folder (the MCP server's pyaddin_save does just this).
    store.save("Row", ROW).unwrap();
    a_moment(&mut h);
    assert!(h.query_by_label("Row").is_some() || h.query_by_label("Row of Walls").is_some(), "the script is in the Player");
    store.set_button(Button::from_json(&json!({"label": "Two Walls", "script": "Row", "panel": "Walls"})).unwrap()).unwrap();
    a_moment(&mut h);
    assert!(h.query_by_label("Two Walls").is_some(), "its button is on the tab");
    assert_eq!(h.state().pyaddins.buttons[0].panel, "Walls");
    // Deleted, both go.
    store.delete("Row").unwrap();
    a_moment(&mut h);
    assert!(h.query_by_label("Two Walls").is_none() && h.query_by_label("Row").is_none());
}

#[test]
fn a_script_runs_from_its_row_with_its_form_and_is_one_undo_step() {
    if !has_python() {
        return;
    }
    let (mut h, _dir, store) = with_folder(BOX);
    store.save("Row", ROW).unwrap();
    h.state_mut().pyaddins.reload();
    press(&mut h, "PyAddins");
    press(&mut h, "Player");
    // Opened, the row reads what main asks for: its title, its doc and its fields.
    press(&mut h, "Row");
    assert!(h.query_by_label("Row of Walls").is_some());
    assert!(h.query_by_label("Walls in a row.").is_some());
    assert!(h.query_by_label("Count").is_some() && h.query_by_label("Length").is_some());
    let (walls, steps) = (wall_count(&h), h.state().session.history().len());
    start(&mut h, "▶ Run Script");
    until_done(&mut h);
    assert_eq!(wall_count(&h), walls + 2);
    assert_eq!(h.state().session.history().len(), steps + 1, "one step for the run");
    assert_eq!(h.state().session.history().last(), Some(&"PyAddin"));
    assert_eq!(h.state().selection.len(), 2, "what it selected is selected");
    assert!(h.query_by_label("made 2").is_some(), "what it printed is in its log");
    assert!(h.state().status.text.contains("2 walls"), "{}", h.state().status.text);
    // Undo takes the whole run back.
    h.state_mut().session.execute("undo", &json!({})).unwrap();
    assert_eq!(wall_count(&h), walls);
}

#[test]
fn a_ribbon_button_runs_with_its_values_a_failure_says_its_line_and_restart_runs_it_again() {
    if !has_python() {
        return;
    }
    let (mut h, _dir, store) = with_folder(BOX);
    store.save("Row", ROW).unwrap();
    store.set_button(Button::from_json(&json!({"label": "Four Walls", "script": "Row", "inputs": {"count": 4}})).unwrap()).unwrap();
    h.state_mut().pyaddins.reload();
    press(&mut h, "PyAddins");
    let walls = wall_count(&h);
    start(&mut h, "Four Walls");
    until_done(&mut h);
    assert_eq!(wall_count(&h), walls + 4);
    start(&mut h, "Restart");
    until_done(&mut h);
    assert_eq!(wall_count(&h), walls + 8, "the same values again");
    // A script that fails: the status says why, by the script's line.
    store.save("Broken", "import bimcraft as bc\nbc.door(at=[99000, 99000])\n").unwrap();
    store.set_button(Button::from_json(&json!({"label": "Broken Door", "script": "Broken"})).unwrap()).unwrap();
    h.state_mut().pyaddins.reload();
    h.run();
    start(&mut h, "Broken Door");
    until_done(&mut h);
    let status = h.state().status.clone();
    assert!(status.error && status.text.contains("CommandError"), "{}", status.text);
}

#[test]
fn stop_ends_a_script_that_would_run_forever() {
    if !has_python() {
        return;
    }
    let (mut h, _dir, store) = with_folder(BOX);
    store.save("Forever", "import time\nwhile True:\n    time.sleep(0.01)\n").unwrap();
    store.set_button(Button::from_json(&json!({"label": "Spin", "script": "Forever"})).unwrap()).unwrap();
    h.state_mut().pyaddins.reload();
    press(&mut h, "PyAddins");
    start(&mut h, "Spin");
    h.run_steps(3);
    assert!(h.state().pyaddins.running.is_some());
    start(&mut h, "Stop");
    until_done(&mut h);
    assert!(h.state().status.text.contains("stopped") || h.state().status.error, "{}", h.state().status.text);
}

#[test]
fn without_python_a_run_says_how_to_get_one() {
    let (mut h, _dir, store) = with_folder(BOX);
    store.save("Row", ROW).unwrap();
    store.set_python(Some("/no/such/python")).unwrap();
    store.set_button(Button::from_json(&json!({"label": "Two Walls", "script": "Row"})).unwrap()).unwrap();
    h.state_mut().pyaddins.reload();
    press(&mut h, "PyAddins");
    press(&mut h, "Two Walls");
    let status = h.state().status.clone();
    assert!(status.error && status.text.contains("/no/such/python"), "{}", status.text);
    assert!(h.state().pyaddins.running.is_none());
}

#[test]
fn a_project_opened_or_started_keeps_the_folder_and_stops_the_old_ones_run() {
    let (mut h, _dir, store) = with_folder(BOX);
    store.save("Row", ROW).unwrap();
    store.save("Forever", "import time\nwhile True:\n    time.sleep(0.01)\n").unwrap();
    h.state_mut().pyaddins.reload();
    let python = has_python();
    if python {
        let ctx = h.ctx.clone();
        h.state_mut().run_pyaddin(&ctx, "Forever", &serde_json::Map::new());
        assert!(h.state().pyaddins.running.is_some());
    }
    // File > New, or Open: the window starts over on the new project.
    h.state_mut().session.execute("new", &json!({})).unwrap();
    h.run();
    assert!(h.state().pyaddins.store.is_some(), "the folder is the user's, not the project's");
    assert_eq!(h.state().pyaddins.scripts, ["Change Wall Type", "Forever", "Row"]);
    press(&mut h, "PyAddins");
    press(&mut h, "Player");
    assert!(h.query_by_label_contains("no PyAddins folder").is_none());
    assert!(h.query_by_label("Row").is_some());
    // A run of the old project's is stopped: it would work on the new one.
    assert!(h.state().pyaddins.running.is_none());
}

#[test]
fn the_sample_changes_wall_types_picked_from_two_lists() {
    if !has_python() {
        return;
    }
    let (mut h, _dir, _store) = with_folder(BOX);
    press(&mut h, "PyAddins");
    press(&mut h, "Player");
    press(&mut h, "Change Wall Type");
    assert!(h.query_by_label("From Type").is_some() && h.query_by_label("To Type").is_some());
    // The two lists start on two types: the project's first, and its next.
    let form = |h: &Harness<BimApp>, input: &str| match h.state().pyaddins.forms["Change Wall Type"].get(input) {
        Some(crate::pyaddins::Field::Text(t)) => t.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!((form(&h, "from_type"), form(&h, "to_type")), ("Interior - 100mm".into(), "Generic - 200mm".into()));
    // From: Generic, what the box's walls are; To: Exterior.
    let list = |h: &mut Harness<BimApp>, input: &str, pick: &str| {
        h.get_by_role_and_label(Role::ComboBox, input).click();
        h.run();
        h.get_all_by_label(pick).last().unwrap().click();
        h.run();
    };
    list(&mut h, "To Type", "Exterior - 300mm");
    list(&mut h, "From Type", "Generic - 200mm");
    assert_eq!((form(&h, "from_type"), form(&h, "to_type")), ("Generic - 200mm".into(), "Exterior - 300mm".into()));
    start(&mut h, "▶ Run Script");
    until_done(&mut h);
    let types: Vec<String> = h
        .state()
        .session
        .model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Wall(w) => Some(w.type_name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(types, ["Exterior - 300mm"; 4], "{}", h.state().status.text);
    assert!(h.state().status.text.contains("4 walls changed"), "{}", h.state().status.text);
    assert_eq!(h.state().selection.len(), 4);
}

#[test]
fn the_player_keeps_its_width_frame_after_frame() {
    let (mut h, _dir, store) = with_folder(BOX);
    store.save("Row", ROW).unwrap();
    h.state_mut().pyaddins.reload();
    press(&mut h, "PyAddins");
    press(&mut h, "Player");
    let width = |h: &Harness<BimApp>| egui::PanelState::load(&h.ctx, egui::Id::new("dock-right")).map(|s| s.outer_rect.width()).unwrap();
    let before = width(&h);
    h.run_steps(20);
    assert_eq!(width(&h), before, "the palette with its rows closed");
    // Opened, with its form, Advanced Settings and a long description.
    press(&mut h, "Change Wall Type");
    h.state_mut().pyaddins.advanced = true;
    h.run_steps(20);
    let open = width(&h);
    h.run_steps(20);
    assert_eq!(width(&h), open, "the palette with a row open");
    assert!(open <= before + 1.0, "{before} → {open}");
}

#[test]
fn new_script_starts_from_a_dummy_with_every_kind_of_field() {
    let (mut h, _dir, _store) = with_folder(BOX);
    press(&mut h, "PyAddins");
    press(&mut h, "New Script");
    press(&mut h, "Create");
    assert_eq!(h.state().pyaddins.open.as_deref(), Some("Dummy Script"));
    assert!(h.state().docking.shown(Palette::Scripts));
    // A second is offered a name of its own.
    assert_eq!(h.state().pyaddins.free_name(), "Dummy Script 2");
    if !has_python() {
        return;
    }
    for field in [
        "Dummy Level",
        "Dummy Wall Type",
        "Dummy Dropdown",
        "Dummy Text Field",
        "Dummy Number",
        "Dummy Length",
        "Dummy Checkbox",
        "Dummy Selection",
    ] {
        assert!(h.query_by_label(field).is_some(), "{field}");
    }
    start(&mut h, "▶ Run Script");
    until_done(&mut h);
    let status = h.state().status.text.clone();
    assert!(status.contains("Dummy script ran: 4 walls on Level 1, nothing changed"), "{status}");
    assert!(h.query_by_label("Dummy dropdown: Dummy A").is_some());
}

#[test]
fn the_python_dialog_sets_an_editor_and_offers_the_skill_for_agents() {
    let (mut h, _dir, store) = with_folder(BOX);
    press(&mut h, "PyAddins");
    press(&mut h, "Python");
    assert!(h.query_by_label_contains("Install Skill").is_some() || h.query_by_label("Reinstall Skill").is_some());
    assert!(h.query_by_label_contains("claude mcp add bimcraft").is_some());
    assert!(h.query_by_label_contains("codex mcp add bimcraft").is_some());
    h.get_by_role_and_label(Role::TextInput, "Editor").focus();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "Editor").type_text("kate");
    h.run();
    press(&mut h, "OK");
    assert_eq!(store.setting("editor").as_deref(), Some("kate"));
}
