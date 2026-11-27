use super::*;
use crate::engine;

const EGUI_LIB: &str = include_str!("../../../ui-egui/src/lib.rs");
const EGUI_SHORTCUTS: &str = include_str!("../../../ui-egui/src/shortcuts.rs");
const EGUI_MEP: &str = include_str!("../../../ui-egui/src/mep.rs");

/// The quoted strings of a line, in order: `("MM", msg!("Mirror Pick Axis")),` holds `MM` and `Mirror Pick Axis`.
fn quoted(line: &str) -> Vec<&str> {
    line.split('"').skip(1).step_by(2).collect()
}

/// The lines of the block of `source` that begins at `start` and ends at the first line that is only `];`, comments left out.
fn block<'a>(source: &'a str, start: &str) -> Vec<&'a str> {
    let from = source.find(start).unwrap_or_else(|| panic!("`{start}` is not in the egui source any more: this table's twin moved"));
    let rest = &source[from..];
    let end = rest.find("\n];").unwrap_or_else(|| panic!("`{start}` has no end"));
    rest[..end].lines().skip(1).map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("//")).collect()
}

fn pairs(source: &str, start: &str) -> Vec<(String, String)> {
    block(source, start)
        .into_iter()
        .map(|l| {
            let q = quoted(l);
            (q.first().copied().unwrap_or_default().to_string(), q.get(1).copied().unwrap_or_default().to_string())
        })
        .collect()
}

fn own(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

const WHY: &str = "the desktop's table and the core's differ: a shortcut is added to both (crates/core/src/shortcuts.rs), never to one";

#[test]
fn the_lists_are_the_ones_the_egui_front_end_has() {
    assert_eq!(pairs(EGUI_LIB, "const SHORTCUTS: [(&str, &str);"), own(DESKTOP), "{WHY}");
    assert_eq!(pairs(EGUI_SHORTCUTS, "const EXTRA_SEQUENCES"), own(EXTRA), "{WHY}");
    // The fixtures' letters sit in a struct each, the label on the line after.
    let lines = block(EGUI_MEP, "pub(crate) const FIXTURES");
    let mut fixtures = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if l.starts_with("letters:") {
            let label = lines[i + 1..].iter().find(|n| n.starts_with("label:")).map(|n| quoted(n)[0]).unwrap_or_default();
            fixtures.push((quoted(l)[0].to_string(), label.to_string()));
        }
    }
    assert_eq!(fixtures, own(FIXTURES), "{WHY}");
    let chords: Vec<(String, Vec<String>)> = block(EGUI_SHORTCUTS, "const DEFAULT_CHORDS")
        .into_iter()
        .map(|l| {
            let q = quoted(l);
            (q[0].to_string(), q[1..].iter().map(|k| k.to_string()).collect())
        })
        .collect();
    let ours: Vec<(String, Vec<String>)> = CHORDS.iter().map(|(a, k)| (a.to_string(), k.iter().map(|k| k.to_string()).collect())).collect();
    assert_eq!(chords, ours, "{WHY}");
}

#[test]
fn a_code_is_two_letters_or_digits_and_every_label_is_named() {
    for (code, label) in DESKTOP.iter().chain(EXTRA).chain(FIXTURES) {
        assert!(code.len() == 2 && code.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()), "{code}");
        assert!(!label.is_empty(), "{code}");
    }
    for c in ALL {
        for alias in c.aliases {
            assert!(alias.len() == 2 && alias.chars().all(|c| c.is_ascii_alphanumeric()), "{} has {alias}", c.id);
        }
    }
}

#[test]
fn a_code_names_one_action_and_the_first_claimant_has_it() {
    let table = table();
    let mut seen: Vec<String> = table.iter().map(|(c, _)| c.to_ascii_uppercase()).collect();
    let n = seen.len();
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), n, "a code is in the table once");
    // Every alias of every command is a code of the table: the command that comes first has it when two share one.
    for c in ALL {
        for alias in c.aliases {
            assert!(target(alias).is_some(), "{alias} of {} is a shortcut", c.id);
        }
    }
    for (code, _) in DESKTOP.iter().chain(EXTRA).chain(FIXTURES) {
        assert!(target(code).is_some(), "{code} is a shortcut");
    }
    assert!(table.len() >= 100, "a table of the shortcuts the desktop has: {}", table.len());
}

#[test]
fn a_command_has_its_shortcut_and_a_desktop_action_has_its_own() {
    assert_eq!(target("CR"), Some(Target::Command("section")), "the command's own alias");
    assert_eq!(target("cr"), Some(Target::Command("section")), "in any case");
    assert_eq!(target("BM"), Some(Target::Command("beam")), "a code of the extra list, for the command that has the label");
    assert_eq!(target("ZF"), Some(Target::Desktop("Zoom to Fit")));
    assert_eq!(target("3D"), Some(Target::Desktop("3D View")));
    assert_eq!(target("MD"), Some(Target::Desktop("Modify")));
    assert_eq!(target("SE"), Some(Target::Desktop("Snap to Endpoints")));
    assert_eq!(target("Q1"), None);
    assert_eq!(target(""), None);
    assert_eq!(target("W"), None, "one letter is not a shortcut yet");
    for (_, t) in table() {
        if let Target::Command(id) = t {
            assert!(engine::commands().iter().any(|c| c.id == *id), "{id} is a command");
        }
    }
}

#[test]
fn a_letter_may_begin_a_shortcut_and_a_whole_one_begins_nothing() {
    assert!(begins("Z") && begins("z") && begins("S"));
    assert!(!begins("ZF") && !begins("QQ") && !begins(""));
    assert!(!begins("ZFZ"), "a shortcut is two letters");
}

#[test]
fn a_chord_names_the_action_it_runs() {
    assert_eq!(chord("Ctrl+Z"), Some("cmd.undo"));
    assert_eq!(chord("ctrl+shift+z"), Some("cmd.redo"));
    assert_eq!(chord("Ctrl+Y"), Some("cmd.redo"));
    assert_eq!(chord("F3"), Some("ui.view.3d"));
    assert_eq!(chord("Delete"), Some("cmd.delete"));
    assert_eq!(chord("Ctrl+Q"), None);
    let mut all: Vec<String> = CHORDS.iter().flat_map(|(_, k)| k.iter().map(|k| k.to_ascii_lowercase())).collect();
    let n = all.len();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), n, "a chord runs one action");
    for (action, _) in CHORDS {
        match action.strip_prefix("cmd.") {
            Some(id) => assert!(engine::find(id).is_some_and(|c| c.id == id), "{action} is a command"),
            None => assert!(action.starts_with("ui."), "{action}"),
        }
    }
}
