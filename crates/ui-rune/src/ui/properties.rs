//! The Properties palette: what the selected elements (or, with none, the plan) have, group by group, as `App::palette` says
//! (`props::panel`), and the buttons that run commands on the selection. A group folds; a row is drawn by `editors`.

use rune::tok::{Theme, type_};
use rune::{El, ScrollSpec, col, row, text};
use rune_kit::{Collapsible, Kit};

use crate::app::App;
use crate::lang::{t, tf};
use crate::props::names::category;
use crate::props::panel::{Group, Subject};

mod editors;
mod selector;
#[cfg(test)]
mod tests;

pub fn panel(s: &App, th: &Theme) -> El {
    let p = &s.palette;
    let kit = Kit::neutral(true);
    let title = text(t("Properties")).font(type_::TITLE).color(th.text);
    let buttons = row().gap(8.0).child(editors::button("delete", t("Delete"), th, |s: &mut App| {
        s.delete_selection();
    }));
    let body = if p.panel.groups.is_empty() {
        let said = p.panel.notes.first().cloned().unwrap_or_else(|| {
            let hint = if matches!(p.panel.subject, Subject::Mixed(_)) {
                t("Pick one category above to see its parameters")
            } else {
                t("Nothing selected")
            };
            hint.to_string()
        });
        col().key("rows").child(text(said).font(type_::BODY).color(th.text_secondary).wrap())
    } else {
        col().key("rows").gap(6.0).children(p.panel.groups.iter().map(|g| group(s, g, th, &kit)))
    };
    col()
        .key("properties")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .fill(th.card)
        .padding(12.0)
        .gap(10.0)
        .child(title)
        .child(buttons)
        .children(selector::selector(s, th))
        .child(subject(s, th))
        .children(p.refusal.as_ref().map(|why| text(why.clone()).key("refusal").font(type_::LABEL).color(th.text).wrap()))
        .child(body.scroll(ScrollSpec::default()).grow(1.0).min_h(0.0))
}

/// What the palette is about: its title, and with several categories selected a button for each, which narrows what it shows.
fn subject(s: &App, th: &Theme) -> El {
    let p = &s.palette;
    let mut c = col().key("subject").gap(4.0);
    if !p.panel.title.is_empty() {
        c = c.child(text(p.panel.title.clone()).key("title").font(type_::LABEL).color(th.text));
    }
    if p.panel.categories.len() > 1 {
        let now = p.filter.as_ref().map(|(category, _)| *category);
        let all = tf("Multiple Categories ({})", &[&s.selection.len().to_string()]);
        c = c.child(filter_button("filter-all".to_string(), all, now.is_none(), None, th));
        for &(key, n) in &p.panel.categories {
            let name = format!("{} ({n})", category(key, true));
            c = c.child(filter_button(format!("filter-{key}"), name, now == Some(key), Some(key), th));
        }
    }
    c
}

/// A button of the filter: `which` is the category it narrows to, none for all of them.
fn filter_button(key: String, label: String, on: bool, which: Option<&'static str>, th: &Theme) -> El {
    let b = editors::button(key, &label, th, move |s: &mut App| s.set_filter(which));
    if on { b.fill(th.control_off) } else { b }
}

/// A group of rows under a header that folds it.
fn group(s: &App, g: &Group, th: &Theme, kit: &Kit) -> El {
    let key = g.key;
    let open = !s.palette.folded.contains(key);
    let mut c = Collapsible::new(g.name.clone(), open).on_toggle(move |s: &mut App| s.fold(key));
    if open {
        c = c.child(col().w_full().gap(4.0).children(g.rows.iter().map(|r| editors::row_of(s, r, th, kit))));
    }
    c.build(kit)
}
