//! The Project Browser: every view the model has, in the groups egui's browser has (`views::tree`), a row to a view with the one on
//! screen lit. A press on a row opens the view; a press on a group's name folds it. The list is one element kept by `memo` while the
//! tree, the folds and the view shown are the same.

use rune::tok::{Theme, type_};
use rune::{Color, El, ScrollSpec, col, memo, row, text};

use crate::app::App;
use crate::lang::t;
use crate::views::{Entry, Group, ViewKind};

pub const HEIGHT: f32 = 220.0;

pub fn panel(s: &App, th: &Theme) -> El {
    let shown = s.view;
    let views = &s.views;
    let list = memo("browser-views", (shown, views.rev), || {
        col().gap(2.0).children(views.groups.iter().flat_map(|g| group(g, shown, views.folded.contains(g.label), th)))
    });
    col()
        .key("browser")
        .w_full()
        .h(HEIGHT)
        .fill(th.card)
        .padding(12.0)
        .gap(8.0)
        .child(text(t("Views")).font(type_::TITLE).color(th.text))
        .child(list.scroll(ScrollSpec::default()).grow(1.0).min_h(0.0))
}

/// A group's name, and under it a row for each of its views unless it is folded.
fn group(g: &Group, shown: ViewKind, folded: bool, th: &Theme) -> Vec<El> {
    let label = g.label;
    let name = t(label).to_string();
    let head = row()
        // A slash in a key reads as a path to Rune's `find`.
        .key(format!("group-{}", label.replace('/', " ")))
        .role("button")
        .label(name.clone())
        .expanded(!folded)
        .focusable(true)
        .h(24.0)
        .px(8.0)
        .gap(6.0)
        .center_y()
        .radius(6.0)
        .child(text(if folded { "\u{25b8}" } else { "\u{25be}" }).font(type_::LABEL).color(th.text_secondary))
        .child(text(name).font(type_::LABEL).color(th.text_secondary).truncate())
        .on_press(move |s: &mut App| s.views.toggle(label));
    let rows: Vec<El> = if folded { Vec::new() } else { g.entries.iter().map(|e| entry(e, shown, th)).collect() };
    std::iter::once(head).chain(rows).collect()
}

/// A view's row: pressing it opens the view.
fn entry(e: &Entry, shown: ViewKind, th: &Theme) -> El {
    let kind = e.kind;
    row()
        .key(e.key.clone())
        .role("button")
        .label(e.label.clone())
        .focusable(true)
        .h(24.0)
        .pl(22.0)
        .pr(8.0)
        .center_y()
        .radius(6.0)
        .fill(if shown == kind { th.control_off } else { Color::TRANSPARENT })
        .child(text(e.label.clone()).font(type_::BODY).color(th.text).truncate())
        .on_press(move |s: &mut App| {
            s.open_view(kind);
        })
}
