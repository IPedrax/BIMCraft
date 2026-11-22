//! FE-20: the selection's commands in a family document, over the one selection set
//! (FE-1). Filter keeps the kinds checked; Select Previous brings back the selection before it was let go or replaced,
//! without what was deleted since; Select by ID selects what a list of ids and GUIDs names and says what names nothing, and
//! IDs of Selection shows them; Select Elements by Face says whether a face selects what it is of, and Drag Elements on
//! Selection whether a drag from a face is a box's or the element's. A command with nothing to work on does nothing, and
//! nothing of it reaches the project. The commands are run as Find runs them (`BimApp::family_find_action`).
use super::family_box_select::{BOXES, DIM, EDGE, LINE, boxed, click_with_mods, g, plan, selected, set};
use super::family_views::{click_family, editor, view};
use super::*;
use crate::editor::select_box::Picked;
use bimcraft_core::family::views::{Side, Which};

/// The family document's command `name`, as Find runs it (`BimApp::family_find_action`): what its button does, where the
/// button is enabled; nothing where the family has it not.
pub(super) fn command(h: &mut Harness<BimApp>, name: &str) {
    h.state_mut().family_find_action(name);
    h.run();
}

fn editor_mut<'a>(h: &'a mut Harness<'_, BimApp>) -> &'a mut crate::editor::FamilyEditor {
    h.state_mut().active_family_mut().expect("a family open")
}

/// The project as a file holds it, and what it has selected and open: what the family document's commands must leave be.
fn project(h: &Harness<BimApp>) -> (String, Vec<Id>, bool) {
    let app = h.state();
    let dialogs = app.selection_filter.is_some() || app.select_by_id.is_some() || app.ids_of_selection.is_some();
    (app.session.model.to_json().unwrap(), app.selection.clone(), dialogs)
}

/// Both boxes, Edge, the reference line, the symbolic line and the dimension selected, by boxes as a hand draws them.
fn all_six(h: &mut Harness<BimApp>) {
    boxed(h, [400.0, 700.0, 0.0], [1000.0, 100.0, 0.0], Modifiers::NONE);
    boxed(h, [-400.0, 500.0, 0.0], [-1400.0, 300.0, 0.0], Modifiers::COMMAND);
    boxed(h, [-200.0, -600.0, 0.0], [200.0, -800.0, 0.0], Modifiers::COMMAND);
    boxed(h, [-300.0, 1000.0, 0.0], [300.0, 800.0, 0.0], Modifiers::COMMAND);
    boxed(h, [-1450.0, -1100.0, 0.0], [150.0, -1300.0, 0.0], Modifiers::COMMAND);
    assert_eq!(
        selected(h),
        set(vec![
            Picked::Form(0),
            Picked::Form(1),
            Picked::Plane(g(EDGE)),
            Picked::Line(g(LINE)),
            Picked::Dimension(g(DIM)),
            Picked::Symbol(0)
        ]),
        "both boxes, Edge, the reference line, the dimension and the symbol"
    );
}

/// The family document's Manage tab shown (the window's own bar has a Manage of its own in this profile).
fn manage(h: &mut Harness<BimApp>) {
    editor_mut(h).doc_tab = crate::editor::DocTab::Manage;
    h.run();
}

/// Whether the Filter dialog is open.
fn filtering(h: &Harness<BimApp>) -> bool {
    h.query_by_label("Check All").is_some()
}

#[test]
fn filter_counts_the_selection_by_kind_and_keeps_the_kinds_checked() {
    let mut h = plan();
    let before = project(&h);
    all_six(&mut h);
    // The Select panel's Filter, there while something is selected, as the project's.
    press(&mut h, "Filter");
    assert!(filtering(&h), "Filter opens its dialog");
    for kind in ["Extrusions", "Reference Planes", "Reference Lines", "Dimensions", "Symbols"] {
        assert!(h.query_by_role_and_label(Role::CheckBox, kind).is_some(), "{kind} listed");
    }
    assert!(h.query_by_role_and_label(Role::CheckBox, "Connectors").is_none(), "only the kinds selected");
    // In an order of their own, the same in every language: forms first, then datums, dimensions and symbols.
    let tops: Vec<f32> = ["Extrusions", "Reference Planes", "Reference Lines", "Dimensions", "Symbols"]
        .iter()
        .map(|kind| h.get_by_role_and_label(Role::CheckBox, kind).rect().top())
        .collect();
    assert!(tops.windows(2).all(|w| w[0] < w[1]), "the kinds in their fixed order, not by name: {tops:?}");
    h.get_by_label("Total selected items: 6");
    // Each with its count: the two extrusions cleared leave four, Edge three.
    pick(&mut h, Role::CheckBox, "Extrusions");
    h.get_by_label("Total selected items: 4");
    pick(&mut h, Role::CheckBox, "Reference Planes");
    h.get_by_label("Total selected items: 3");
    press(&mut h, "OK");
    assert!(!filtering(&h));
    assert_eq!(
        selected(&h),
        set(vec![Picked::Line(g(LINE)), Picked::Dimension(g(DIM)), Picked::Symbol(0)]),
        "OK keeps what is checked selected"
    );
    // The command opens it too; Check None and Cancel leave the selection as it was.
    command(&mut h, "Filter Selection");
    assert!(filtering(&h), "Filter Selection");
    h.get_by_label("Total selected items: 3");
    press(&mut h, "Check None");
    h.get_by_label("Total selected items: 0");
    press(&mut h, "Cancel");
    assert!(!filtering(&h));
    assert_eq!(selected(&h).len(), 3, "Cancel keeps the selection");
    // Check None, Check All, one cleared: OK keeps the others.
    command(&mut h, "Filter Selection");
    press(&mut h, "Check None");
    press(&mut h, "Check All");
    h.get_by_label("Total selected items: 3");
    pick(&mut h, Role::CheckBox, "Symbols");
    press(&mut h, "OK");
    assert_eq!(selected(&h), set(vec![Picked::Line(g(LINE)), Picked::Dimension(g(DIM))]));
    // The status bar's funnel counts the family's selection and filters it, never the project's.
    assert!(painted_text(&h).contains(&":2".to_string()), "the status bar counts the family's selection");
    press(&mut h, "Filter Selection");
    assert!(filtering(&h), "the status bar's funnel");
    h.get_by_label("Total selected items: 2");
    press(&mut h, "Check None");
    press(&mut h, "OK");
    assert!(selected(&h).is_empty(), "nothing checked, nothing kept");
    assert_eq!(project(&h), before, "nothing of it reaches the project");
}

#[test]
fn filter_does_nothing_with_nothing_selected_and_none_of_the_commands_work_while_a_form_is_sketched() {
    let mut h = plan();
    let before = project(&h);
    assert!(selected(&h).is_empty());
    assert_eq!(h.query_all_by_label("Filter").count(), 0, "no Filter button with nothing selected");
    command(&mut h, "Filter Selection");
    assert!(!filtering(&h), "Filter with nothing selected");
    // The funnel, with nothing selected, opens nothing either.
    press(&mut h, "Filter Selection");
    assert!(!filtering(&h));
    // Select Previous with no selection before this one: nothing.
    click_family(&mut h, [700.0, 400.0, 0.0]);
    assert_eq!(selected(&h), vec![Picked::Form(1)]);
    command(&mut h, "Select Previous");
    assert_eq!(selected(&h), vec![Picked::Form(1)], "Select Previous");
    // While a form is sketched, the selection's commands wait: its contextual tab has none of them.
    key(&mut h, Key::Escape);
    h.get_by_role_and_label(Role::Button, "Extrusion").click();
    h.run();
    assert!(editor(&h).sketching());
    for name in ["Filter Selection", "Select by ID", "Select Previous"] {
        assert!(!h.state().family_find_actions().contains(&name), "Find doesn't list {name} while sketching");
        command(&mut h, name);
        assert!(!filtering(&h) && h.query_by_label("Add to the selection").is_none(), "{name} while sketching");
        assert!(editor(&h).sketching(), "{name}");
    }
    assert_eq!(project(&h), before, "nothing of it reaches the project");
}

#[test]
fn pr_brings_back_the_selection_before_this_one_without_what_was_deleted_since() {
    let mut h = plan();
    let before = project(&h);
    let three = set(vec![Picked::Form(0), Picked::Form(1), Picked::Plane(g(EDGE))]);
    boxed(&mut h, [400.0, 700.0, 0.0], [1000.0, 100.0, 0.0], Modifiers::NONE);
    boxed(&mut h, [-400.0, 500.0, 0.0], [-1400.0, 300.0, 0.0], Modifiers::COMMAND);
    assert_eq!(selected(&h), three);
    // Let go, it comes back.
    key(&mut h, Key::Escape);
    assert!(selected(&h).is_empty());
    command(&mut h, "Select Previous");
    assert_eq!(selected(&h), three, "Select Previous after Escape");
    // Replaced by a click, it comes back, and what it replaced is the previous one: the two change places.
    click_family(&mut h, [-150.0, 900.0, 0.0]);
    let one = vec![Picked::Line(g(LINE))];
    assert_eq!(selected(&h), one, "the reference line alone");
    command(&mut h, "Select Previous");
    assert_eq!(selected(&h), three, "Select Previous after another was selected");
    command(&mut h, "Select Previous");
    assert_eq!(selected(&h), one, "and back");
    command(&mut h, "Select Previous");
    assert_eq!(selected(&h), three);
    // Ctrl and Shift change the selection without making it another: Select Previous still brings back the one before.
    click_with_mods(&mut h, [-700.0, 400.0, 0.0], Modifiers::SHIFT);
    assert_eq!(selected(&h), set(vec![Picked::Form(1), Picked::Plane(g(EDGE))]));
    command(&mut h, "Select Previous");
    assert_eq!(selected(&h), one, "the one before, not the three less one");
    command(&mut h, "Select Previous");
    assert_eq!(selected(&h), set(vec![Picked::Form(1), Picked::Plane(g(EDGE))]));
    // Deleted since: A, the first form, goes; Select Previous brings back the others, B now the first form.
    click_with_mods(&mut h, [-700.0, 400.0, 0.0], Modifiers::COMMAND);
    assert_eq!(selected(&h), three);
    key(&mut h, Key::Escape);
    click_family(&mut h, [-700.0, 400.0, 0.0]);
    assert_eq!(selected(&h), vec![Picked::Form(0)], "A alone");
    key(&mut h, Key::Delete);
    let f = editor(&h).draft_family().unwrap();
    assert_eq!(f.forms.len(), 1, "A deleted");
    assert!(selected(&h).is_empty());
    command(&mut h, "Select Previous");
    assert_eq!(selected(&h), set(vec![Picked::Form(0), Picked::Plane(g(EDGE))]), "B, now the first form, and Edge: A is skipped");
    assert!((editor(&h).draft_family().unwrap().forms[0].corners().is_some()), "B is the form left");
    // Let go and both deleted from the browser's side of things: nothing is left to bring back, and Select Previous does
    // nothing.
    key(&mut h, Key::Escape);
    editor_mut(&mut h).remove_form(0);
    editor_mut(&mut h).delete_plane(g(EDGE)).unwrap();
    h.run();
    command(&mut h, "Select Previous");
    assert!(selected(&h).is_empty(), "nothing left to bring back");
    // The reference line selected and let go is the one Select Previous brings back now.
    click_family(&mut h, [-150.0, 900.0, 0.0]);
    key(&mut h, Key::Escape);
    command(&mut h, "Select Previous");
    assert_eq!(selected(&h), one);
    assert_eq!(project(&h), before, "nothing of it reaches the project");
}

#[test]
fn id_selects_what_a_list_of_ids_and_guids_names_and_says_what_names_nothing() {
    let mut h = plan();
    let before = project(&h);
    let family = editor(&h).draft_family().unwrap();
    let (a, b) = (family.forms[0].id, family.forms[1].guid);
    assert!(a >= 100_000 && !b.is_nil(), "a form of a family the project has holds an id and a GUID: {a} {b}");
    let label = "IDs, GUIDs or IFC GlobalIds, as many as you like:";
    // Manage > Inquiry > Select by ID, as the project's.
    manage(&mut h);
    press(&mut h, "Select by ID");
    type_into(&mut h, label, &format!("{a}, {EDGE} 123"));
    assert!(h.query_by_label("Finds 2; nothing has these: 123").is_some(), "what it finds, and what names nothing, as typed");
    press(&mut h, "OK");
    assert_eq!(selected(&h), set(vec![Picked::Form(0), Picked::Plane(g(EDGE))]));
    assert!(!h.state().status.error && h.state().status.text.contains("123"), "{}", h.state().status.text);
    // The command opens it too. A list that names nothing says so, and OK waits; Cancel leaves the selection be.
    command(&mut h, "Select by ID");
    type_into(&mut h, label, "nowhere 99");
    assert!(h.query_by_label("Finds 0; nothing has these: nowhere, 99").is_some());
    h.get_by_label("OK").click();
    h.run();
    assert!(h.query_by_label("Add to the selection").is_some(), "OK waits while nothing is found");
    press(&mut h, "Cancel");
    assert_eq!(selected(&h).len(), 2);
    // Added to the selection: B by its GUID without its dashes, the dimension by its own.
    command(&mut h, "Select by ID");
    type_into(&mut h, label, &format!("{} {DIM}", b.to_string().replace('-', "")));
    assert!(h.query_by_label("Finds 2.").is_some());
    pick(&mut h, Role::CheckBox, "Add to the selection");
    press(&mut h, "OK");
    assert_eq!(selected(&h), set(vec![Picked::Form(0), Picked::Form(1), Picked::Plane(g(EDGE)), Picked::Dimension(g(DIM))]));
    // IDs of Selection, beside it: each one's id and GUID, to read and copy.
    manage(&mut h);
    press(&mut h, "IDs of Selection");
    for text in [a.to_string(), b.to_string(), EDGE.to_string(), "4 selected.".to_string(), "1. Extrusion".to_string()] {
        assert!(h.query_all_by_label(&text).count() > 0, "{text}");
    }
    press(&mut h, "Copy GUIDs");
    let copied = editor(&h).selecting.copied.clone().unwrap();
    assert!(copied.lines().count() == 4 && copied.contains(EDGE) && copied.contains(&b.to_string()), "{copied}");
    press(&mut h, "Copy IDs");
    assert!(editor(&h).selecting.copied.as_deref().is_some_and(|c| c.lines().any(|l| l == a.to_string())));
    press(&mut h, "Close");
    assert!(h.query_by_label("Copy GUIDs").is_none());
    // A plane drawn since has no id yet: a dash, and a note of it; it is found by its GUID.
    shortcut(&mut h, "RP");
    click_family(&mut h, [-1100.0, -1000.0, 0.0]);
    click_family(&mut h, [-1100.0, 1000.0, 0.0]);
    key(&mut h, Key::Escape);
    let drawn = editor(&h).planes.last().unwrap().guid;
    assert_eq!(selected(&h), vec![Picked::Plane(drawn)]);
    manage(&mut h);
    press(&mut h, "IDs of Selection");
    assert!(h.query_all_by_label("\u{2014}").count() > 0 && h.query_all_by_label(&drawn.to_string()).count() > 0, "no id yet, its GUID");
    press(&mut h, "Close");
    key(&mut h, Key::Escape);
    command(&mut h, "Select by ID");
    type_into(&mut h, label, &drawn.to_string());
    press(&mut h, "OK");
    assert_eq!(selected(&h), vec![Picked::Plane(drawn)]);
    assert_eq!(project(&h), before, "nothing of it reaches the project");
}

/// A small box of two types, Short 100 wide and Long 300, to nest.
fn small_file(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("Small.json");
    let family = json!({"name": "Small", "category": "generic", "parameters": [{"name": "W", "value": 100}],
        "types": {"Short": {}, "Long": {"W": 300}},
        "forms": [{"profile": [[0, 0], ["W", 0], ["W", 100], [0, 100]], "to": 100}]});
    std::fs::write(&path, family.to_string()).unwrap();
    path
}

/// An instance of Small's type `type_name` placed by Component at `at` of the floor plan.
fn place(h: &mut Harness<BimApp>, type_name: &str, at: [f64; 3]) {
    editor_mut(h).component_tool();
    h.run();
    assert!(editor(h).placing.is_some(), "Component");
    editor_mut(h).placing = Some(("Small".into(), type_name.into()));
    let pos = editor(h).views.screen_of(at).expect("the floor plan shows");
    click_at(h, pos);
    h.run();
}

#[test]
fn the_selections_commands_are_not_an_adaptive_familys() {
    let (mut h, points, _) = super::family_temporary_hide::adaptive();
    editor_mut(&mut h).modeller.selection = vec![points[0].clone()];
    h.run();
    assert!(painted_text(&h).contains(&":1".to_string()), "the status bar counts the point selected");
    for name in ["Filter Selection", "Select Previous", "Select by ID"] {
        command(&mut h, name);
        assert!(!filtering(&h) && h.query_by_label("Add to the selection").is_none(), "{name} in an adaptive family");
        assert_eq!(editor(&h).modeller.selection.len(), 1, "{name}");
        assert_eq!(h.state().tool, Tool::Modify, "{name}: no project tool");
    }
    assert_eq!(h.query_all_by_label("Filter").count(), 0);
}

/// Select Elements by Face: on, a face selects what it is of, by a click and by a crossing box drawn inside it;
/// off, only an edge of its outline does. A box is drawn from inside the face of what isn't selected, either way.
#[test]
fn select_elements_by_face_decides_whether_a_face_selects_by_a_click_and_by_a_crossing_box() {
    let mut h = plan();
    let before = project(&h);
    assert!(h.state().select_by_face, "on to begin with: a click has always taken a form by its face");
    // On: a click inside B selects it, and so does one on its edge.
    click_family(&mut h, [700.0, 400.0, 0.0]);
    assert_eq!(selected(&h), vec![Picked::Form(1)], "a click on its face");
    key(&mut h, Key::Escape);
    click_family(&mut h, [900.0, 400.0, 0.0]);
    assert_eq!(selected(&h), vec![Picked::Form(1)], "a click on its edge");
    key(&mut h, Key::Escape);
    // A crossing box drawn wholly inside A's face takes A; a window there holds none of it.
    boxed(&mut h, [-650.0, 450.0, 0.0], [-750.0, 350.0, 0.0], Modifiers::NONE);
    assert_eq!(selected(&h), vec![Picked::Form(0)], "a crossing inside its face takes it");
    boxed(&mut h, [650.0, 350.0, 0.0], [750.0, 450.0, 0.0], Modifiers::NONE);
    assert!(selected(&h).is_empty(), "a window inside a face holds nothing whole");
    // Ctrl adds what a crossing inside a face takes.
    boxed(&mut h, [-650.0, 450.0, 0.0], [-750.0, 350.0, 0.0], Modifiers::NONE);
    boxed(&mut h, [750.0, 450.0, 0.0], [650.0, 350.0, 0.0], Modifiers::COMMAND);
    assert_eq!(selected(&h), vec![Picked::Form(0), Picked::Form(1)], "Ctrl adds");
    // From the face of one selected no box is drawn: its drag is its own, and moves the selection; Undo puts it back.
    let forms = |h: &Harness<BimApp>| editor(h).draft_family().unwrap().forms;
    let unmoved = forms(&h);
    boxed(&mut h, [750.0, 450.0, 0.0], [650.0, 350.0, 0.0], Modifiers::NONE);
    assert_eq!(selected(&h), vec![Picked::Form(0), Picked::Form(1)], "a drag from a selected form's face boxes nothing");
    assert_ne!(forms(&h), unmoved, "it moves them");
    h.state_mut().family_undo();
    h.run();
    assert_eq!(forms(&h), unmoved);
    // The command turns it off: a face selects nothing, by a click or by a crossing box; an edge still does.
    command(&mut h, "Select Elements by Face");
    assert!(!h.state().select_by_face, "Select Elements by Face");
    key(&mut h, Key::Escape);
    click_family(&mut h, [700.0, 400.0, 0.0]);
    assert!(selected(&h).is_empty(), "off, a click on a face selects nothing");
    click_family(&mut h, [900.0, 400.0, 0.0]);
    assert_eq!(selected(&h), vec![Picked::Form(1)], "off, a click on an edge selects");
    boxed(&mut h, [-650.0, 450.0, 0.0], [-750.0, 350.0, 0.0], Modifiers::NONE);
    assert!(selected(&h).is_empty(), "off, a crossing inside a face takes nothing, and lets the selection go");
    boxed(&mut h, [-400.0, 500.0, 0.0], [-600.0, 300.0, 0.0], Modifiers::NONE);
    assert_eq!(selected(&h), vec![Picked::Form(0)], "off, a crossing over its edge takes it");
    // Off, the face of one selected is still its own: a drag from it moves it and draws no box, as on; Undo puts it back.
    let unmoved = forms(&h);
    boxed(&mut h, [-650.0, 450.0, 0.0], [-750.0, 350.0, 0.0], Modifiers::NONE);
    assert_eq!(selected(&h), vec![Picked::Form(0)], "off, a drag from inside a selected form's face is its own");
    assert_ne!(forms(&h), unmoved, "and moves it");
    h.state_mut().family_undo();
    h.run();
    assert_eq!(forms(&h), unmoved);
    // The status bar's toggle turns it on again.
    press(&mut h, "Select Elements by Face");
    assert!(h.state().select_by_face, "the status bar's toggle");
    click_family(&mut h, [700.0, 400.0, 0.0]);
    assert_eq!(selected(&h), vec![Picked::Form(1)]);
    // An elevation picks as a plan does.
    view(&mut h, Some(Which::Elevation(Side::Front)));
    key(&mut h, Key::Escape);
    boxed(&mut h, [-650.0, 0.0, 350.0], [-750.0, 0.0, 250.0], Modifiers::NONE);
    assert_eq!(selected(&h), vec![Picked::Form(0)], "a crossing inside A's face in the Front elevation");
    command(&mut h, "Select Elements by Face");
    click_family(&mut h, [700.0, 0.0, 300.0]);
    assert!(selected(&h).is_empty(), "off, in an elevation too");
    click_family(&mut h, [700.0, 0.0, 600.0]);
    assert_eq!(selected(&h), vec![Picked::Form(1)], "its top edge");
    assert_eq!(project(&h), before, "nothing of it reaches the project");
}

#[test]
fn select_elements_by_face_is_greyed_where_it_changes_nothing_and_is_kept_with_the_settings() {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.json");
    std::fs::write(&settings, "{}").unwrap();
    let mut h = plan();
    h.state_mut().use_settings(settings.clone());
    h.run();
    assert!(h.state().select_by_face, "on, in a settings file that says nothing of it");
    // {3D} takes a form by its face either way, as the project's 3D view does: there the toggle is greyed and Find hasn't
    // the command.
    view(&mut h, None);
    assert!(!h.state().family_find_actions().contains(&"Select Elements by Face"));
    h.get_by_label("Select Elements by Face").click();
    h.run();
    assert!(h.state().select_by_face, "greyed in {{3D}}");
    // In a plan it is the window's, kept in the settings file and not in the family or the project.
    view(&mut h, Some(Which::Plan));
    press(&mut h, "Select Elements by Face");
    assert!(!h.state().select_by_face);
    let kept: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(kept["select_by_face"], false, "{kept}");
    let mut again = app("");
    again.state_mut().use_settings(settings);
    assert!(!again.state().select_by_face, "the next session begins as this one ended");
    // While a form is sketched the view picks for the sketch: greyed.
    h.get_by_role_and_label(Role::Button, "Extrusion").click();
    h.run();
    assert!(editor(&h).sketching());
    h.get_by_label("Select Elements by Face").click();
    h.run();
    assert!(!h.state().select_by_face, "greyed while sketching");
    // An adaptive family selects its own way: no toggle, and Find hasn't the command.
    let (mut h, ..) = super::family_temporary_hide::adaptive();
    let was = h.state().select_by_face;
    assert_eq!(h.query_all_by_label("Select Elements by Face").count(), 0, "no toggle in an adaptive family");
    command(&mut h, "Select Elements by Face");
    assert_eq!(h.state().select_by_face, was);
    // On the project's tab neither: its views pick as they always have.
    let h = app(BOX);
    assert_eq!(h.query_all_by_label("Select Elements by Face").count(), 0, "no toggle on a project's tab");
    assert!(h.state().select_by_face);
}

/// Drag Elements on Selection, beside Select Elements by Face: off (at first), a drag from the face of a form not selected draws a box, a face being
/// the box's to begin in; on, it selects the form and moves it, as a drag from its edge does, and a nested instance's face the same. With
/// Select Elements by Face off a face picks nothing, so it changes nothing: a box from a face, an edge still the element's. A form the view leaves out
/// is taken by neither.
#[test]
fn drag_elements_on_selection_lets_a_drag_from_a_face_not_selected_select_and_move_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = plan();
    let before = project(&h);
    let forms = |h: &Harness<BimApp>| editor(h).draft_family().unwrap().forms;
    assert!(!h.state().drag_on_selection, "off to begin with");
    let unmoved = forms(&h);
    // Off: a window from inside B's face, B not selected, holds none of it and moves nothing.
    boxed(&mut h, [650.0, 350.0, 0.0], [750.0, 450.0, 0.0], Modifiers::NONE);
    assert!(selected(&h).is_empty(), "off, a box from a face not selected");
    assert_eq!(forms(&h), unmoved, "nothing moved");
    // The status bar's toggle turns it on: the same drag selects B and moves it, one step that Undo takes back.
    assert!(h.state().prompt().contains("drag from nothing to box"), "{}", h.state().prompt());
    assert!(!h.state().prompt().contains("Drag a face"), "{}", h.state().prompt());
    press(&mut h, "Drag Elements on Selection");
    assert!(h.state().drag_on_selection, "the status bar's toggle");
    assert!(h.state().prompt().starts_with("Drag a face to move what it is of"), "{}", h.state().prompt());
    boxed(&mut h, [650.0, 350.0, 0.0], [750.0, 450.0, 0.0], Modifiers::NONE);
    assert_eq!(selected(&h), vec![Picked::Form(1)], "on, B selected by its face");
    assert!(!editor(&h).boxing());
    let moved = forms(&h);
    assert_eq!(moved[0], unmoved[0], "A stays");
    assert_ne!(moved[1], unmoved[1], "B moves");
    assert_eq!(editor(&h).steps(), 1);
    h.state_mut().family_undo();
    h.run();
    assert_eq!(forms(&h), unmoved);
    // A press from the face of another, nothing held: that one is the selection, and it alone moves.
    boxed(&mut h, [-700.0, 400.0, 0.0], [-700.0, 100.0, 0.0], Modifiers::NONE);
    assert_eq!(selected(&h), vec![Picked::Form(0)], "A in B's place");
    assert_eq!(forms(&h)[1], unmoved[1], "B stays");
    assert_ne!(forms(&h)[0], unmoved[0], "A moves");
    h.state_mut().family_undo();
    h.run();
    // From nothing a box still begins.
    key(&mut h, Key::Escape);
    boxed(&mut h, [400.0, 700.0, 0.0], [1000.0, 100.0, 0.0], Modifiers::NONE);
    assert_eq!(selected(&h), vec![Picked::Form(1)], "a window from nothing round B");
    assert_eq!(forms(&h), unmoved);
    // Select Elements by Face off: a face picks nothing, so a drag from it is a box again; an edge still selects its form and
    // moves it.
    command(&mut h, "Select Elements by Face");
    assert!(!h.state().select_by_face && h.state().drag_on_selection);
    assert!(!h.state().prompt().contains("Drag a face"), "by face off, the prompt says nothing of faces: {}", h.state().prompt());
    key(&mut h, Key::Escape);
    boxed(&mut h, [-750.0, 350.0, 0.0], [-650.0, 450.0, 0.0], Modifiers::NONE);
    assert!(selected(&h).is_empty(), "by face off, a box from A's face");
    assert_eq!(forms(&h), unmoved, "by face off, nothing moved");
    boxed(&mut h, [-500.0, 400.0, 0.0], [-500.0, 100.0, 0.0], Modifiers::NONE);
    assert_eq!(selected(&h), vec![Picked::Form(0)], "by face off, A by its edge");
    assert_ne!(forms(&h)[0], unmoved[0], "and moved");
    h.state_mut().family_undo();
    h.run();
    command(&mut h, "Select Elements by Face");
    assert!(h.state().select_by_face);
    // A form hidden for now (HH) is taken by neither its face nor a drag from it: a box, which moves nothing.
    click_family(&mut h, [-700.0, 400.0, 0.0]);
    shortcut(&mut h, "HH");
    assert!(editor(&h).temporary().hides() && selected(&h).is_empty(), "HH");
    boxed(&mut h, [-750.0, 350.0, 0.0], [-650.0, 450.0, 0.0], Modifiers::NONE);
    assert!(selected(&h).is_empty() && !editor(&h).boxing(), "a box from where A's face is drawn no more");
    assert_eq!(forms(&h), unmoved, "A hidden stays");
    shortcut(&mut h, "HR");
    // A nested instance by its face, on: selected and moved; off, its face is the box's.
    let doc = h.state().active_doc().expect("a family document");
    h.state_mut().nest_family_file(doc, &small_file(dir.path()));
    h.run();
    place(&mut h, "Short", [0.0, -400.0, 0.0]);
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    let placed = editor(&h).nested_instances[0].at.clone();
    h.state_mut().drag_on_selection = false;
    h.run();
    boxed(&mut h, [50.0, -350.0, 0.0], [400.0, -500.0, 0.0], Modifiers::NONE);
    assert!(selected(&h).is_empty(), "off, a box from its face");
    assert_eq!(editor(&h).nested_instances[0].at, placed);
    h.state_mut().drag_on_selection = true;
    h.run();
    boxed(&mut h, [50.0, -350.0, 0.0], [50.0, -650.0, 0.0], Modifiers::NONE);
    assert_eq!(selected(&h), vec![Picked::Nested(0)], "on, the instance by its face");
    assert_ne!(editor(&h).nested_instances[0].at, placed, "and moved");
    assert_eq!(project(&h), before, "nothing of it reaches the project");
}

/// Drag Elements on Selection is the window's, as Select Elements by Face is: kept in the settings file, carried to a new
/// project's family tabs, greyed in {3D} and while a form is sketched, absent from an adaptive family and a project's tab.
#[test]
fn drag_elements_on_selection_is_kept_with_the_settings_and_is_greyed_as_select_by_face_is() {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.json");
    std::fs::write(&settings, "{}").unwrap();
    let mut h = plan();
    h.state_mut().use_settings(settings.clone());
    h.run();
    assert!(!h.state().drag_on_selection, "off, in a settings file that says nothing of it");
    // Find's command toggles it, on a family's tab.
    command(&mut h, "Drag Elements on Selection");
    assert!(h.state().drag_on_selection, "Find's command");
    let kept: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(kept["drag_on_selection"], true, "{kept}");
    let mut again = app("");
    again.state_mut().use_settings(settings);
    assert!(again.state().drag_on_selection, "the next session begins as this one ended");
    // {3D}: greyed, and Find hasn't the command.
    view(&mut h, None);
    h.get_by_label("Drag Elements on Selection").click();
    h.run();
    command(&mut h, "Drag Elements on Selection");
    assert!(h.state().drag_on_selection, "greyed in {{3D}}");
    // While a form is sketched: the same.
    view(&mut h, Some(Which::Plan));
    h.get_by_role_and_label(Role::Button, "Extrusion").click();
    h.run();
    assert!(editor(&h).sketching());
    h.get_by_label("Drag Elements on Selection").click();
    h.run();
    command(&mut h, "Drag Elements on Selection");
    assert!(h.state().drag_on_selection, "greyed while sketching");
    // A new project (File > New) keeps it, and so does the next family opened.
    h.state_mut().reopened();
    h.run();
    assert!(h.state().drag_on_selection, "kept by a new project");
    h.state_mut().new_family(bimcraft_core::family::Category::Generic);
    h.run();
    assert!(editor(&h).selecting.drag_faces, "a fresh family editor has it");
    // An adaptive family selects its own way: no toggle. Nor has a project's tab.
    let (h, ..) = super::family_temporary_hide::adaptive();
    assert_eq!(h.query_all_by_label("Drag Elements on Selection").count(), 0, "no toggle in an adaptive family");
    let h = app(BOX);
    assert_eq!(h.query_all_by_label("Drag Elements on Selection").count(), 0, "no toggle on a project's tab");
}

/// Looking at the result: with `BIMCRAFT_FAMILY_SELECTION_DUMP` naming a folder, the family's plan with several selected
/// (the Select panel's Filter, the status bar's count, funnel and Select Elements by Face), Filter, Select by ID and IDs of
/// Selection, each in the three looks.
#[test]
fn the_selections_dialogs_and_status_bar_in_the_three_looks() {
    let Ok(folder) = std::env::var("BIMCRAFT_FAMILY_SELECTION_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (name, dark, light_canvas) in [("light", false, false), ("dark", true, false), ("dark-ui-light-canvas", true, true)] {
        let mut h = app(BOXES);
        h.ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
        h.state_mut().light_canvas = light_canvas;
        h.set_size(vec2(1600., 1000.));
        h.state_mut().edit_family("Boxes");
        h.run();
        view(&mut h, Some(Which::Plan));
        let shot = |h: &mut Harness<BimApp>, what: &str| {
            h.run();
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("selection-{what}-{name}.png"))).unwrap();
        };
        all_six(&mut h);
        shot(&mut h, "several");
        command(&mut h, "Filter Selection");
        shot(&mut h, "filter");
        press(&mut h, "Cancel");
        command(&mut h, "Select Elements by Face");
        command(&mut h, "Select by ID");
        type_into(&mut h, "IDs, GUIDs or IFC GlobalIds, as many as you like:", &format!("{EDGE} 123"));
        shot(&mut h, "by-id-and-face-off");
        press(&mut h, "Cancel");
        manage(&mut h);
        press(&mut h, "IDs of Selection");
        shot(&mut h, "ids");
    }
}

/// Looking at the status bar: with `BIMCRAFT_FAMILY_SELECTION_DUMP` naming a folder, Drag Elements on Selection and Select
/// Elements by Face in each of their four states, greyed in {3D}, and the new toggle's tooltip, light and dark.
#[test]
fn the_status_bars_two_toggles_in_each_state() {
    let Ok(folder) = std::env::var("BIMCRAFT_FAMILY_SELECTION_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (name, dark) in [("light", false), ("dark", true)] {
        let mut h = app(BOXES);
        h.ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
        h.set_size(vec2(1600., 1000.));
        h.state_mut().edit_family("Boxes");
        h.run();
        view(&mut h, Some(Which::Plan));
        let shot = |h: &mut Harness<BimApp>, what: &str| {
            h.run();
            h.run();
            let full = h.render().unwrap();
            full.save(std::path::Path::new(&folder).join(format!("toggles-{what}-{name}.png"))).unwrap();
            // The status bar's right end, four times as large, to look at the toggles closely.
            let (w, ht) = full.dimensions();
            let bar = image::imageops::crop_imm(&full, w - 480, ht - 52, 480, 52).to_image();
            let big = image::imageops::resize(&bar, 480 * 4, 52 * 4, image::imageops::FilterType::Nearest);
            big.save(std::path::Path::new(&folder).join(format!("bar-{what}-{name}.png"))).unwrap();
        };
        for (by_face, drag) in [(true, false), (true, true), (false, true), (false, false)] {
            (h.state_mut().select_by_face, h.state_mut().drag_on_selection) = (by_face, drag);
            shot(&mut h, &format!("bf-{}-drag-{}", if by_face { "on" } else { "off" }, if drag { "on" } else { "off" }));
        }
        let at = h.get_by_label("Drag Elements on Selection").rect().center();
        h.hover_at(at);
        for _ in 0..30 {
            h.step();
        }
        shot(&mut h, "tooltip");
        h.hover_at(egui::pos2(800.0, 500.0));
        view(&mut h, None);
        shot(&mut h, "greyed-3d");
    }
}

/// A reference plane or a reference line is clicked before a nested instance's edge, as before a form's: an instance
/// aligned to a plane leaves the plane beside it to be clicked, Select Elements by Face on or off.
#[test]
fn a_click_on_a_plane_beside_a_nested_instance_aligned_to_it_selects_the_plane() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = plan();
    let doc = h.state().active_doc().expect("a family document");
    h.state_mut().nest_family_file(doc, &small_file(dir.path()));
    h.run();
    // Its west side on Center (Left/Right), in front of the boxes.
    place(&mut h, "Short", [0.0, -400.0, 0.0]);
    key(&mut h, Key::Escape);
    let center = bimcraft_core::family::CENTER_LEFT_RIGHT;
    // Three points left of its west side, on the plane's trace.
    let beside = editor(&h).views.screen_of([0.0, -350.0, 0.0]).unwrap() - vec2(3.0, 0.0);
    for by_face in [true, false] {
        h.state_mut().select_by_face = by_face;
        h.run();
        click_at(&mut h, beside);
        h.run();
        assert_eq!(selected(&h), vec![Picked::Plane(center)], "the plane beside the instance, by face {by_face}");
        key(&mut h, Key::Escape);
        // Its east side, where no plane is, still selects it.
        click_family(&mut h, [100.0, -350.0, 0.0]);
        assert_eq!(selected(&h), vec![Picked::Nested(0)], "its own edge, by face {by_face}");
        key(&mut h, Key::Escape);
    }
    // Its face, while a face selects, is the instance's, as it always was.
    h.state_mut().select_by_face = true;
    h.run();
    click_family(&mut h, [50.0, -350.0, 0.0]);
    assert_eq!(selected(&h), vec![Picked::Nested(0)]);
}

/// What Select Previous brings back names a form by its place, which an undo or a redo may give to another: it follows the
/// form. A symbol, a connector and a nested instance are followed by their GUIDs (`pr_keeps_symbols_…`).
#[test]
fn pr_after_an_undo_brings_back_the_form_that_was_selected_not_the_one_now_in_its_place() {
    let mut h = plan();
    let undo = |h: &mut Harness<BimApp>| {
        h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
        h.run();
    };
    // B selected and let go, then A selected and deleted: B is the first form now, and the one Select Previous would bring back.
    click_family(&mut h, [700.0, 400.0, 0.0]);
    assert_eq!(selected(&h), vec![Picked::Form(1)]);
    key(&mut h, Key::Escape);
    click_family(&mut h, [-700.0, 400.0, 0.0]);
    key(&mut h, Key::Delete);
    assert_eq!(editor(&h).draft_family().unwrap().forms.len(), 1, "A deleted");
    // Undo puts A back in the first place: Select Previous brings back B, in the second, not A.
    undo(&mut h);
    assert_eq!(editor(&h).draft_family().unwrap().forms.len(), 2, "A back");
    command(&mut h, "Select Previous");
    assert_eq!(selected(&h), vec![Picked::Form(1)], "B, where it is now");
    // Move Up, undone: B let go, moved up to the first place and back by Undo, is still the one Select Previous brings back.
    key(&mut h, Key::Escape);
    editor_mut(&mut h).move_form(1, -1);
    h.run();
    command(&mut h, "Select Previous");
    assert_eq!(selected(&h), vec![Picked::Form(0)], "B, moved up");
    key(&mut h, Key::Escape);
    undo(&mut h);
    command(&mut h, "Select Previous");
    assert_eq!(selected(&h), vec![Picked::Form(1)], "B, back in the second place");
    // Redo moves it up again, and Select Previous follows it.
    key(&mut h, Key::Escape);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Y);
    h.run();
    command(&mut h, "Select Previous");
    assert_eq!(selected(&h), vec![Picked::Form(0)], "B, moved up again by Redo");
    // A plane is followed by its GUID as it always was, and the symbol by its own, through an undo that left it be.
    boxed(&mut h, [-200.0, -600.0, 0.0], [200.0, -800.0, 0.0], Modifiers::NONE);
    click_with_mods(&mut h, [-1300.0, -300.0, 0.0], Modifiers::COMMAND);
    assert_eq!(selected(&h), set(vec![Picked::Plane(g(EDGE)), Picked::Symbol(0)]));
    key(&mut h, Key::Escape);
    undo(&mut h);
    command(&mut h, "Select Previous");
    assert_eq!(selected(&h), set(vec![Picked::Plane(g(EDGE)), Picked::Symbol(0)]), "the plane and the symbol");
}

/// An L-shaped extrusion, its inner corner at (600, 600): the two edges that meet there run back into what its hull spans,
/// whose chord from (1200, 600) to (600, 1200) crosses empty space.
pub(super) const ELL: &str = concat!(
    r#"family {"name": "Ell", "category": "generic", "types": {"T": {}}, "#,
    r#""forms": [{"profile": [[200, 200], [1200, 200], [1200, 600], [600, 600], [600, 1200], [200, 1200]], "to": 500}]}"#
);

/// With Select Elements by Face off an edge is one the view draws, not a side of the form's hull: an inner edge of an
/// L-shaped form selects it, the hull's chord over empty space doesn't, and a box goes by the same edges.
#[test]
fn an_edge_is_one_the_view_draws_of_a_form_not_a_side_of_its_hull() {
    let (mut h, _) = super::family_views::open(ELL, "Ell");
    view(&mut h, Some(Which::Plan));
    let form = vec![Picked::Form(0)];
    for by_face in [false, true] {
        h.state_mut().select_by_face = by_face;
        h.run();
        key(&mut h, Key::Escape);
        // Its inner edges, each at its middle, and an outer one.
        for at in [[900.0, 600.0, 0.0], [600.0, 900.0, 0.0], [1200.0, 400.0, 0.0]] {
            click_family(&mut h, at);
            assert_eq!(selected(&h), form, "an edge at {at:?}, by face {by_face}");
            key(&mut h, Key::Escape);
        }
        // The middle of the hull's chord, in the empty corner: nothing there to click.
        click_family(&mut h, [900.0, 900.0, 0.0]);
        assert!(selected(&h).is_empty(), "the hull's chord over empty space, by face {by_face}");
        // A crossing box across an inner edge takes the form; one in the empty corner, over the chord alone, doesn't.
        boxed(&mut h, [1000.0, 700.0, 0.0], [800.0, 500.0, 0.0], Modifiers::NONE);
        assert_eq!(selected(&h), form, "a crossing over an inner edge, by face {by_face}");
        boxed(&mut h, [950.0, 950.0, 0.0], [850.0, 850.0, 0.0], Modifiers::NONE);
        assert!(selected(&h).is_empty(), "a crossing over the chord alone takes nothing, and begins there, by face {by_face}");
        // A window round all of it takes it, as before.
        boxed(&mut h, [100.0, 1300.0, 0.0], [1300.0, 100.0, 0.0], Modifiers::NONE);
        assert_eq!(selected(&h), form, "a window round it, by face {by_face}");
    }
    // An elevation goes by its edges too: Front sees the L as a rectangle, its inner corner hidden behind its front face.
    h.state_mut().select_by_face = false;
    view(&mut h, Some(Which::Elevation(Side::Front)));
    key(&mut h, Key::Escape);
    click_family(&mut h, [900.0, 0.0, 250.0]);
    assert!(selected(&h).is_empty(), "the face between its edges, off");
    click_family(&mut h, [1200.0, 0.0, 250.0]);
    assert_eq!(selected(&h), form, "its right side");
}

/// A box takes nothing the view doesn't draw: a form its Visibility Settings leave out of the plan, while Preview Visibility
/// shows the family as placed (off, the plan draws it halftone, and a box takes it as a click does), and a nested instance
/// they leave out, which the plan never draws, nor a click takes.
#[test]
fn a_box_takes_nothing_the_view_leaves_out_by_its_visibility_settings() {
    let dir = tempfile::tempdir().unwrap();
    // A, the west box, left out of the plans.
    let script = BOXES.replacen(r#""to": 600}, "#, r#""to": 600, "visibility": {"plan": false}}, "#, 1);
    let (mut h, _) = super::family_views::open(&script, "Boxes");
    view(&mut h, Some(Which::Plan));
    let before = project(&h);
    // Two Shorts in front, the first left out of the plans too.
    let doc = h.state().active_doc().expect("a family document");
    h.state_mut().nest_family_file(doc, &small_file(dir.path()));
    h.run();
    for x in [200.0, 600.0] {
        place(&mut h, "Short", [x, -400.0, 0.0]);
    }
    editor_mut(&mut h).nested_instances[0].visibility = Some(bimcraft_core::family::Visibility { plan: false, ..Default::default() });
    h.run();
    key(&mut h, Key::Escape);
    // Preview Visibility off: A drawn halftone, taken by a crossing inside its face as by a click on it.
    boxed(&mut h, [-650.0, 450.0, 0.0], [-750.0, 350.0, 0.0], Modifiers::NONE);
    assert_eq!(selected(&h), vec![Picked::Form(0)], "A drawn halftone");
    // The first Short is never drawn in the plan: no box takes it, by its face, across it or round it, nor a click.
    boxed(&mut h, [270.0, -330.0, 0.0], [230.0, -370.0, 0.0], Modifiers::NONE);
    assert!(selected(&h).is_empty(), "a crossing inside where the first Short would be");
    boxed(&mut h, [350.0, -320.0, 0.0], [150.0, -380.0, 0.0], Modifiers::NONE);
    assert!(selected(&h).is_empty(), "a crossing across it");
    boxed(&mut h, [100.0, -200.0, 0.0], [400.0, -500.0, 0.0], Modifiers::NONE);
    assert!(selected(&h).is_empty(), "a window round it");
    click_family(&mut h, [250.0, -350.0, 0.0]);
    assert!(selected(&h).is_empty(), "a click on it");
    // The second, drawn, is taken.
    boxed(&mut h, [750.0, -320.0, 0.0], [550.0, -380.0, 0.0], Modifiers::NONE);
    assert_eq!(selected(&h), vec![Picked::Nested(1)], "a crossing across the second Short");
    // Preview Visibility on: A isn't drawn, and no box takes it.
    super::preview_visibility::toggle(&mut h);
    assert!(editor(&h).preview_visibility(), "Preview Visibility on");
    key(&mut h, Key::Escape);
    boxed(&mut h, [-650.0, 450.0, 0.0], [-750.0, 350.0, 0.0], Modifiers::NONE);
    assert!(selected(&h).is_empty(), "a crossing inside where A would be");
    boxed(&mut h, [-400.0, 500.0, 0.0], [-600.0, 300.0, 0.0], Modifiers::NONE);
    assert!(selected(&h).is_empty(), "a crossing over its edge");
    boxed(&mut h, [-1000.0, 700.0, 0.0], [-400.0, 100.0, 0.0], Modifiers::NONE);
    assert!(selected(&h).is_empty(), "a window round it");
    // B, which the plan draws, still is.
    boxed(&mut h, [750.0, 450.0, 0.0], [650.0, 350.0, 0.0], Modifiers::NONE);
    assert_eq!(selected(&h), vec![Picked::Form(1)], "B");
    assert_eq!(project(&h), before, "nothing of it reaches the project");
}

/// A press at `from` on screen, `mods` held, kept down a second (past what egui still calls a click, so it is a drag) and
/// let go at `to`: a slow click, or a shaky one.
fn slow_press(h: &mut Harness<BimApp>, from: egui::Pos2, to: egui::Pos2, mods: Modifiers) {
    h.event(Event::ModifiersChanged(mods));
    h.hover_at(from);
    h.run();
    h.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: mods });
    h.run_steps(4);
    h.hover_at(to);
    h.run();
    h.event(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: mods });
    h.run();
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run();
}

/// A slow or shaky click is a click, not a tiny box that boxes nothing and lets the selection go: a drag let go within
/// the few pixels the project's canvas allows one (`canvas::STILL`) selects what a click where it began would. Past them it
/// is a box.
#[test]
fn a_slow_or_shaky_click_is_a_click_and_not_a_tiny_box() {
    let mut h = plan();
    let before = project(&h);
    let at = |h: &Harness<BimApp>, p: [f64; 3]| editor(h).views.screen_of(p).expect("a view shown");
    click_family(&mut h, [-700.0, 400.0, 0.0]);
    assert_eq!(selected(&h), vec![Picked::Form(0)]);
    // On B's face, which isn't selected (the box's to begin in), the hand moving 3 px: B, as a click there selects it.
    let b = at(&h, [700.0, 400.0, 0.0]);
    slow_press(&mut h, b, b + egui::vec2(3.0, 1.0), Modifiers::NONE);
    assert!(!editor(&h).boxing(), "no box left drawn");
    assert_eq!(selected(&h), vec![Picked::Form(1)], "B, by a click on its face");
    // With Ctrl, A added, as a click with Ctrl adds it; on nothing, the selection let go, as a click on nothing does.
    let a = at(&h, [-700.0, 400.0, 0.0]);
    slow_press(&mut h, a, a + egui::vec2(-2.0, 2.0), Modifiers::COMMAND);
    assert_eq!(selected(&h), vec![Picked::Form(0), Picked::Form(1)], "Ctrl adds A");
    let nothing = at(&h, [300.0, -400.0, 0.0]);
    slow_press(&mut h, nothing, nothing + egui::vec2(2.0, -3.0), Modifiers::NONE);
    assert!(selected(&h).is_empty(), "a click on nothing");
    // Past them, a box: a window 20 px across inside B's face holds none of it, and lets the selection go.
    click_family(&mut h, [-700.0, 400.0, 0.0]);
    slow_press(&mut h, b, b + egui::vec2(20.0, 10.0), Modifiers::NONE);
    assert!(selected(&h).is_empty(), "a window inside B's face");
    assert_eq!(project(&h), before, "nothing of it reaches the project");
}

/// What the view leaves out is picked by nothing, as no box takes it: a form hidden for now (HH), neither by a click on its
/// face with Select Elements by Face on nor by one on its edge with it off, and a drag from its edge draws a box, moving
/// nothing; and while Preview Visibility shows the family as placed, a form its Visibility Settings leave out of the view.
#[test]
fn a_form_the_view_leaves_out_is_picked_neither_by_its_face_nor_by_its_edge() {
    let mut h = plan();
    let before = project(&h);
    let forms = |h: &Harness<BimApp>| editor(h).draft_family().unwrap().forms;
    let unmoved = forms(&h);
    // A, hidden for now: the selection goes with it.
    click_family(&mut h, [-700.0, 400.0, 0.0]);
    assert_eq!(selected(&h), vec![Picked::Form(0)]);
    shortcut(&mut h, "HH");
    assert!(editor(&h).temporary().hides() && selected(&h).is_empty(), "HH");
    click_family(&mut h, [-700.0, 400.0, 0.0]);
    assert!(selected(&h).is_empty(), "a click on A's face, a face selecting");
    h.state_mut().select_by_face = false;
    h.run();
    click_family(&mut h, [-500.0, 400.0, 0.0]);
    assert!(selected(&h).is_empty(), "a click on A's edge, only edges selecting");
    // From A's edge, leftwards over Edge: a crossing box, which takes Edge and not A; nothing moves.
    boxed(&mut h, [-500.0, 400.0, 0.0], [-1400.0, 300.0, 0.0], Modifiers::NONE);
    assert_eq!(selected(&h), vec![Picked::Plane(g(EDGE))], "a box from A's edge");
    assert_eq!(forms(&h), unmoved, "nothing moved");
    assert_eq!(project(&h), before, "nothing of it reaches the project");
    // A left out of the plans by its Visibility Settings, Preview Visibility on: the same.
    let script = BOXES.replacen(r#""to": 600}, "#, r#""to": 600, "visibility": {"plan": false}}, "#, 1);
    let (mut h, _) = super::family_views::open(&script, "Boxes");
    view(&mut h, Some(Which::Plan));
    super::preview_visibility::toggle(&mut h);
    assert!(editor(&h).preview_visibility(), "Preview Visibility on");
    click_family(&mut h, [-700.0, 400.0, 0.0]);
    assert!(selected(&h).is_empty(), "a click on where A's face would be");
    h.state_mut().select_by_face = false;
    h.run();
    click_family(&mut h, [-500.0, 400.0, 0.0]);
    assert!(selected(&h).is_empty(), "a click on where A's edge would be");
    // Off, A is drawn halftone: picked as a click picks it.
    super::preview_visibility::toggle(&mut h);
    click_family(&mut h, [-500.0, 400.0, 0.0]);
    assert_eq!(selected(&h), vec![Picked::Form(0)], "A drawn halftone");
}

/// An undo or a redo that leaves the symbols and the nested instances as they were leaves them in what Select Previous
/// brings back: they are followed by their GUIDs, or kept at their place where the step left that place as it was.
#[test]
fn pr_keeps_symbols_and_nested_instances_an_undo_didnt_touch() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = plan();
    let doc = h.state().active_doc().expect("a family document");
    h.state_mut().nest_family_file(doc, &small_file(dir.path()));
    h.run();
    place(&mut h, "Short", [200.0, -400.0, 0.0]);
    key(&mut h, Key::Escape);
    editor_mut(&mut h).selection.apply(&[Picked::Symbol(0), Picked::Nested(0)], crate::editor::select_box::How::Replace);
    h.run();
    key(&mut h, Key::Escape);
    assert!(selected(&h).is_empty());
    // B deleted, and the deletion undone: neither the symbol nor the instance was touched.
    click_family(&mut h, [700.0, 400.0, 0.0]);
    assert_eq!(selected(&h), vec![Picked::Form(1)]);
    key(&mut h, Key::Delete);
    assert_eq!(editor(&h).draft_family().unwrap().forms.len(), 1, "B deleted");
    h.state_mut().family_undo();
    h.run();
    assert_eq!(editor(&h).draft_family().unwrap().forms.len(), 2, "B back");
    command(&mut h, "Select Previous");
    assert_eq!(selected(&h), vec![Picked::Symbol(0), Picked::Nested(0)], "the symbol and the instance, as they were");
    // Undone again past the instance's placing: it is gone, and goes from what Select Previous brings back; the symbol stays.
    key(&mut h, Key::Escape);
    for _ in 0..4 {
        if editor(&h).draft_family().unwrap().nested_instances.is_empty() {
            break;
        }
        h.state_mut().family_undo();
        h.run();
    }
    assert!(editor(&h).draft_family().unwrap().nested_instances.is_empty(), "the placing undone");
    command(&mut h, "Select Previous");
    assert_eq!(selected(&h), vec![Picked::Symbol(0)], "the symbol alone");
}
