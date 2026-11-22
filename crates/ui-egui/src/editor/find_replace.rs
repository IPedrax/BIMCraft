//! Find/Replace on a family's text, Annotate > Find/Replace: what to find, what to put in its
//! place, Match case and Match whole word only. Find All selects the texts it is found in; Replace All replaces it in every
//! text of the family, the dialog's whole visit one step of the history (`FamilyEditor::history`). What a text names in
//! braces (`{Width}`, a parameter) is never found nor changed. It has no key in a family yet: the project's FR is Filled Region.

use egui::{Align, Context, Id as UiId, Layout, TextEdit};

use super::select_box::Picked;
use super::{FamilyEditor, Part};
use crate::lang::{t, tf};
use crate::popup::Modal;

/// Find/Replace, open: what is typed and ticked.
#[derive(Clone, Debug, Default)]
pub(crate) struct FindReplace {
    find: String,
    with: String,
    case: bool,
    whole: bool,
}

/// What a press of the dialog's buttons said, for the status bar.
pub(crate) struct Said {
    pub(crate) text: String,
    pub(crate) error: bool,
}

/// `text` with each `find` in it (as `case` and `whole` say) replaced by `with`, and how many were; none of what is in braces.
pub(crate) fn replaced(text: &str, find: &str, with: &str, case: bool, whole: bool) -> (String, usize) {
    if find.is_empty() {
        return (text.to_string(), 0);
    }
    let chars: Vec<char> = text.chars().collect();
    let wanted: Vec<char> = find.chars().collect();
    let same = |a: char, b: char| if case { a == b } else { a.to_lowercase().eq(b.to_lowercase()) };
    let word = |c: Option<&char>| c.is_some_and(|c| c.is_alphanumeric() || *c == '_');
    let (mut out, mut count, mut i, mut depth) = (String::new(), 0, 0, 0usize);
    while i < chars.len() {
        let c = chars[i];
        if depth == 0
            && i + wanted.len() <= chars.len()
            && wanted.iter().zip(&chars[i..]).all(|(w, c)| same(*w, *c))
            && !chars[i..i + wanted.len()].contains(&'{')
            && (!whole || (!word(i.checked_sub(1).and_then(|k| chars.get(k))) && !word(chars.get(i + wanted.len()))))
        {
            out.push_str(with);
            count += 1;
            i += wanted.len();
            continue;
        }
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        out.push(c);
        i += 1;
    }
    (out, count)
}

impl FamilyEditor {
    /// Find/Replace opened, empty.
    pub(crate) fn open_find_replace(&mut self) {
        self.find_replace = Some(FindReplace::default());
    }

    /// Whether Find/Replace is open.
    pub(crate) fn finding(&self) -> bool {
        self.find_replace.is_some()
    }

    /// Find/Replace, while it is open: what its buttons did, for the status bar.
    pub(crate) fn find_replace_dialog(&mut self, ctx: &Context) -> Option<Said> {
        let mut d = self.find_replace.take()?;
        let (mut find_all, mut replace_all, mut close) = (false, false, false);
        let r = Modal::new(UiId::new("family-find-replace"), t("Find/Replace")).width(380.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                egui::Grid::new("family-find-replace-grid").num_columns(2).show(ui, |ui| {
                    for (label, field) in [(t("Find:"), &mut d.find), (t("Replace with:"), &mut d.with)] {
                        let name = ui.label(label);
                        ui.add(TextEdit::singleline(field).desired_width(240.0)).labelled_by(name.id);
                        ui.end_row();
                    }
                });
                ui.checkbox(&mut d.case, t("Match case"));
                ui.checkbox(&mut d.whole, t("Match whole word only"));
                ui.label(egui::RichText::new(t("In the family's text; what is in braces, a parameter, is left as it is.")).small());
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close = ui.button(t("Close")).clicked();
                    replace_all = ui.button(t("Replace All")).clicked();
                    find_all = ui.button(t("Find All")).clicked();
                });
            });
        });
        let mut said = None;
        // A brace in what goes in would name a parameter, or leave one open that hides the rest of the text from later finds.
        if replace_all && (d.with.contains('{') || d.with.contains('}')) {
            said = Some(Said {
                text: t("Find/Replace: what replaces it can't hold { or }, which name a parameter.").to_string(),
                error: true,
            });
            replace_all = false;
        }
        if find_all || replace_all {
            let found: Vec<usize> = self
                .symbols
                .iter()
                .enumerate()
                .filter(|(_, s)| s.part == Part::Text && replaced(&s.value, &d.find, "", d.case, d.whole).1 > 0)
                .map(|(i, _)| i)
                .collect();
            let n = if replace_all {
                let mut n = 0;
                for i in &found {
                    if let Some(s) = self.symbols.get_mut(*i) {
                        let (text, k) = replaced(&s.value, &d.find, &d.with, d.case, d.whole);
                        s.value = text;
                        n += k;
                    }
                }
                n
            } else {
                let picked: Vec<Picked> = found.iter().map(|i| Picked::Symbol(*i)).collect();
                self.selection.apply(&picked, super::select_box::How::Replace);
                found.len()
            };
            said = Some(if found.is_empty() {
                Said { text: tf("Find/Replace: \"{}\" is in none of the family's text", &[&d.find]), error: true }
            } else if replace_all {
                Said { text: tf("Find/Replace: {} replaced", &[&n.to_string()]), error: false }
            } else {
                Said { text: tf("Find/Replace: found in {} texts, selected", &[&n.to_string()]), error: false }
            });
        }
        if !(close || r.should_close()) {
            self.find_replace = Some(d);
        }
        said
    }
}

#[cfg(test)]
mod tests {
    use super::replaced;

    #[test]
    fn replaced_finds_as_asked_and_never_in_braces() {
        assert_eq!(replaced("Door A", "door", "Gate", false, true), ("Gate A".into(), 1));
        assert_eq!(replaced("Door A", "door", "Gate", true, true), ("Door A".into(), 0));
        assert_eq!(replaced("Doorway", "Door", "Gate", false, true), ("Doorway".into(), 0));
        assert_eq!(replaced("Doorway", "Door", "Gate", false, false), ("Gateway".into(), 1));
        assert_eq!(replaced("Door {Door Width}", "Door", "Gate", false, false), ("Gate {Door Width}".into(), 1));
        assert_eq!(replaced("aaa", "aa", "b", true, false), ("ba".into(), 1));
        assert_eq!(replaced("x", "", "b", true, false), ("x".into(), 0));
        assert_eq!(replaced("Ünïcode ünïcode", "ÜNÏCODE", "u", false, true), ("u u".into(), 2));
        // An underscore or a digit is part of a word: no whole word there.
        assert_eq!(replaced("a_Door Door1 Door", "Door", "X", true, true), ("a_Door Door1 X".into(), 1));
        assert_eq!(replaced("a_Door Door1 Door", "Door", "X", true, false), ("a_X X1 X".into(), 3));
        // A brace already in the text stays where it is, and what follows it past the closing one is found again.
        assert_eq!(replaced("{Door} Door", "Door", "X", true, true), ("{Door} X".into(), 1));
    }
}
