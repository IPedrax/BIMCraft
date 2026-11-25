//! Find (Ctrl+K), the pure part: every tool of the palette, every command of the core's table and every view (the levels) that has
//! the words typed in its name, its id or its Revit-style shortcut, best matches first. egui's `find.rs` is the model for it; the
//! dialog (`ui/find.rs`) draws the list and `App::pick_found` does what a row says.

use bimcraft_core::engine;
use bimcraft_core::ids::Id;

use crate::lang::t;
use crate::tools;

/// The most rows Find lists.
pub const MOST: usize = 12;

/// What a row of Find does.
#[derive(Clone, Debug, PartialEq)]
pub enum Found {
    /// Puts the tool in hand (`App::set_tool`).
    Tool(&'static str),
    /// Opens the command's form.
    Command(&'static str),
    /// Shows the level's plan.
    Level(Id),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub label: String,
    /// What kind of row it is, or the shortcut a command has, said beside the label.
    pub note: String,
    pub found: Found,
}

/// Whether `text` has every word of `query` in it, whatever their case. Nothing typed matches nothing.
pub fn matches(text: &str, query: &str) -> bool {
    let text = text.to_lowercase();
    let mut words = query.split_whitespace().peekable();
    words.peek().is_some() && words.all(|w| text.contains(&w.to_lowercase()))
}

/// 0 for a label that is the first word typed, 1 for one that starts with it, 2 for one with a word that does, else 3.
fn rank(label: &str, first: &str) -> u8 {
    let label = label.to_lowercase();
    if label == first {
        0
    } else if label.starts_with(first) {
        1
    } else if label.split_whitespace().any(|w| w.starts_with(first)) {
        2
    } else {
        3
    }
}

/// What Find lists for `query`: [`ranked`], [`MOST`] rows at most.
pub fn find(query: &str, levels: &[(Id, String)]) -> Vec<Hit> {
    let mut hits = ranked(query, levels);
    hits.truncate(MOST);
    hits
}

/// Every tool, command and level with all the words of `query`: the label that is the word first, then one that starts with it, and
/// within each rank tools, then commands, then views.
fn ranked(query: &str, levels: &[(Id, String)]) -> Vec<Hit> {
    let first = query.split_whitespace().next().map(str::to_lowercase).unwrap_or_default();
    // (rank, group, row)
    let mut rows: Vec<(u8, u8, Hit)> = Vec::new();
    let every = tools::TOOLS.iter().flat_map(|(_, list)| list.iter()).chain(tools::MODIFY.iter());
    for &(id, english) in every {
        let label = t(english);
        if matches(&format!("{label} {id}"), query) {
            rows.push((rank(label, &first), 0, Hit { label: label.to_string(), note: t("Tool").to_string(), found: Found::Tool(id) }));
        }
    }
    for cmd in engine::commands() {
        let label = t(cmd.label);
        let shortcuts = cmd.aliases.join(", ");
        if matches(&format!("{label} {} {} {}", cmd.id, shortcuts, cmd.menu.unwrap_or_default()), query) {
            let note = if shortcuts.is_empty() { cmd.id.to_string() } else { shortcuts };
            rows.push((rank(label, &first), 1, Hit { label: label.to_string(), note, found: Found::Command(cmd.id) }));
        }
    }
    for (id, name) in levels {
        if matches(name, query) {
            rows.push((rank(name, &first), 2, Hit { label: name.clone(), note: t("View").to_string(), found: Found::Level(*id) }));
        }
    }
    rows.sort_by_key(|(r, g, _)| (*r, *g));
    rows.into_iter().map(|(_, _, hit)| hit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn levels() -> Vec<(Id, String)> {
        vec![(100_001, "Podium".to_string()), (100_002, "Roof deck".to_string())]
    }

    #[test]
    fn every_word_typed_must_be_in_the_text_and_nothing_typed_matches_nothing() {
        assert!(matches("Curtain Wall", "wall curt"));
        assert!(matches("Curtain Wall", "  WALL "));
        assert!(!matches("Curtain Wall", "wall door"));
        assert!(!matches("Curtain Wall", ""));
        assert!(!matches("Curtain Wall", "   "));
    }

    #[test]
    fn a_tool_a_command_and_a_view_are_each_found_and_the_best_label_comes_first() {
        let hits = find("wall", &levels());
        assert_eq!(hits.first().map(|h| &h.found), Some(&Found::Tool("wall")), "the label that is the word, a tool before a command");
        assert!(hits.iter().any(|h| h.found == Found::Command("wall")), "the command of the same name is there too: {hits:?}");
        assert!(hits.len() <= MOST);

        let hits = find("podium", &levels());
        assert_eq!(hits, [Hit { label: "Podium".into(), note: "View".into(), found: Found::Level(100_001) }]);

        let hits = ranked("roof", &levels());
        assert!(hits.iter().any(|h| h.found == Found::Tool("roof")) && hits.iter().any(|h| h.found == Found::Level(100_002)));
    }

    #[test]
    fn a_shortcut_or_an_id_finds_its_command() {
        let hits = ranked("wa", &[]);
        let wall = hits.iter().find(|h| h.found == Found::Command("wall")).expect("WA is Wall's shortcut");
        assert!(wall.note.contains("WA"), "the shortcut is said beside it: {}", wall.note);
        assert!(ranked("adaptive_move", &[]).iter().any(|h| h.found == Found::Command("adaptive_move")), "by id");
    }

    #[test]
    fn nothing_found_is_an_empty_list_and_a_short_word_is_cut_to_the_most() {
        assert!(find("", &levels()).is_empty());
        assert!(find("zzzzqqqq", &levels()).is_empty());
        assert_eq!(find("e", &levels()).len(), MOST);
        let hostile = "é\u{301}\u{0}".repeat(10_000);
        assert!(find(&hostile, &levels()).is_empty());
    }
}
