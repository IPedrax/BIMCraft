//! Find/Replace on a family's text, Annotate > Find/Replace: what to find and what to put in its
//! place, Match case and Match whole word only; Find All selects the text it is found in, Replace All replaces it in every
//! text of the family, the dialog's visit one step of the history. A parameter a text names (`{Width}`) is never changed.
//! It has no key in a family yet (the project's FR is Filled Region); Find lists it.
use super::*;
use egui_kittest::kittest::NodeT;

const SIGNS: &str = r#"family {"name": "Signs", "category": "generic", "types": {"T": {}}, "parameters": [{"name": "Door Width", "value": 900}], "forms": [{"profile": [[0, 0], [500, 0], [500, 300], [0, 300]], "to": 400}], "symbols": [ {"text": {"at": [0, 0], "height": 100, "value": "Door A"}}, {"text": {"at": [0, 200], "height": 100, "value": "door b"}}, {"text": {"at": [0, 400], "height": 100, "value": "Doorway"}}, {"line": [[0, 0], [100, 0]]}, {"text": {"at": [0, 600], "height": 100, "value": "Door {Door Width}"}} ]}"#;

fn open() -> Harness<'static, BimApp> {
    let mut h = app(&format!("{BOX}\n{SIGNS}"));
    h.set_size(vec2(1600.0, 1000.0));
    h.run();
    h.state_mut().edit_family("Signs");
    h.run();
    h
}

/// The family's texts, in its symbols' order.
fn texts(h: &Harness<BimApp>) -> Vec<String> {
    let f = h.state().active_family().unwrap().draft_family().unwrap();
    f.symbols.iter().filter_map(|s| s.text.as_ref().map(|l| l.value.clone())).collect()
}

/// Find/Replace opened from Annotate, `find` and `with` typed, the two options as given.
fn ask(h: &mut Harness<BimApp>, find: &str, with: &str, case: bool, whole: bool) {
    press(h, "Annotate");
    press(h, "Find/Replace");
    type_into(h, "Find:", find);
    type_into(h, "Replace with:", with);
    for (label, on) in [("Match case", case), ("Match whole word only", whole)] {
        let ticked = h.get_by_label(label).accesskit_node().toggled() == Some(egui::accesskit::Toggled::True);
        if ticked != on {
            press(h, label);
        }
    }
}

#[test]
fn replace_all_replaces_whole_words_in_every_text_as_one_step_and_leaves_parameters_alone() {
    let mut h = open();
    let before = texts(&h);
    let steps = h.state().active_family().unwrap().steps();
    ask(&mut h, "Door", "Gate", false, true);
    press(&mut h, "Replace All");
    assert_eq!(texts(&h), ["Gate A", "Gate b", "Doorway", "Gate {Door Width}"]);
    assert!(h.state().status.text.contains('3'), "{:?}", h.state().status);
    press(&mut h, "Close");
    assert_eq!(h.state().active_family().unwrap().steps(), steps + 1, "the visit is one step");
    press(&mut h, "Undo");
    assert_eq!(texts(&h), before);
}

#[test]
fn match_case_and_parts_of_words() {
    let mut h = open();
    ask(&mut h, "Door", "Gate", true, false);
    press(&mut h, "Replace All");
    press(&mut h, "Close");
    assert_eq!(texts(&h), ["Gate A", "door b", "Gateway", "Gate {Door Width}"]);
}

#[test]
fn find_all_selects_the_texts_it_is_found_in_and_changes_nothing() {
    let mut h = open();
    let before = texts(&h);
    ask(&mut h, "door", "", false, true);
    press(&mut h, "Find All");
    let ed = h.state().active_family().unwrap();
    use crate::editor::select_box::Picked;
    assert_eq!(ed.selection.items(), [Picked::Symbol(0), Picked::Symbol(1), Picked::Symbol(4)]);
    assert!(h.state().status.text.contains('3'), "{:?}", h.state().status);
    press(&mut h, "Close");
    assert_eq!(texts(&h), before);
    assert!(!h.state().active_family().unwrap().unsaved());
}

#[test]
fn nothing_to_find_finds_nothing() {
    let mut h = open();
    ask(&mut h, "Window", "Gate", false, false);
    press(&mut h, "Replace All");
    assert!(h.state().status.text.contains("Window"), "{:?}", h.state().status);
    press(&mut h, "Close");
    assert!(!h.state().active_family().unwrap().unsaved());
}

#[test]
fn find_lists_find_replace_on_a_family_tab_and_runs_it() {
    let mut h = open();
    assert!(h.state().family_find_actions().contains(&"Find/Replace"));
    assert!(h.state_mut().family_find_action("Find/Replace"));
    h.run();
    assert!(h.state().active_family().unwrap().finding());
    press(&mut h, "Close");
}

#[test]
fn a_brace_in_what_replaces_it_is_refused() {
    let mut h = open();
    let before = texts(&h);
    ask(&mut h, "Door", "{Width}", false, true);
    press(&mut h, "Replace All");
    let status = h.state().status.clone();
    assert!(status.error && status.text.contains('{'), "{status:?}");
    press(&mut h, "Close");
    assert_eq!(texts(&h), before);
}

/// Find/Replace open on the family's text, in the light and the dark look, saved where `BIMCRAFT_FIND_REPLACE_DUMP` says
/// (only then).
#[test]
fn find_replace_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_FIND_REPLACE_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (look, dark) in [("light", false), ("dark", true)] {
        let mut h = open();
        h.ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
        ask(&mut h, "Door", "Gate", false, false);
        for _ in 0..3 {
            h.run();
        }
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("find-replace-{look}.png"))).unwrap();
    }
}
