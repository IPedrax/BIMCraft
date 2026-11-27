//! The Project Browser: its tabs (All, Views, Schedules, Sheets, Families), the search with wildcards, and the tree in the groups egui's
//! browser has (`views::Views::rows`), a row to a view, a type of a family or a linked model, the view on screen lit (or the views
//! picked together with Ctrl or Shift). A press on a row opens the view, a press on a group's name folds it, a right click opens the menu
//! of `context::browser_items`. The list is one element kept by `memo` while the tree, the folds, the view shown, the tab, what is
//! picked and what is typed are the same.

use rune::tok::{Theme, type_};
use rune::{Color, El, ScrollSpec, Tx, col, memo, row, text};
use rune_kit::{Kit, Menu, MenuItem, TextInput};

use crate::app::App;
use crate::lang::t;
use crate::views::{Entry, Group, Pick, Tab, Target};

pub const HEIGHT: f32 = 300.0;

/// The browser, with its menu open over it when there is one: the menu's own elements go in `layer`, to be mounted over the window.
pub fn panel(s: &App, th: &Theme, layer: &mut Vec<El>) -> El {
    let shown = s.view;
    let views = &s.views;
    let find = s.views_find_text().to_string();
    let list = memo("browser-views", (shown, views.rev, find.clone()), || {
        let rows = views.rows(&find);
        // Keeping to one kind, its group is all there is: what is in it, without it.
        let flat = views.tab != Tab::All && rows.len() == 1;
        let lit = |e: &Entry| match e.pick {
            Pick::Open if views.picked.len() > 1 => views.picked.contains(&e.kind),
            Pick::Open => shown == e.kind,
            _ => false,
        };
        col().gap(2.0).children(rows.iter().flat_map(|(g, entries)| {
            // What is typed opens every group it finds something in, and a group with no name to press has no fold.
            let folded = !flat && views.folded.contains(g.label) && find.trim().is_empty();
            group(g, entries, &lit, folded, flat, th)
        }))
    });
    let kit = Kit::neutral(s.dark());
    let search = TextInput::new(&s.views_find)
        .placeholder(t("Search\u{2026}"))
        .clear_button()
        .width(None)
        .on_edit(|s: &mut App| &mut s.views_find)
        .build(&kit)
        .key("browser-find");
    let body = col()
        .key("browser")
        .w_full()
        .h(HEIGHT)
        .fill(th.card)
        .padding(12.0)
        .gap(8.0)
        .child(text(t("Project Browser")).font(type_::TITLE).color(th.text))
        .child(tabs(s, th))
        .child(search)
        .child(list.scroll(ScrollSpec::default()).grow(1.0).min_h(0.0));
    with_menu(s, body, layer)
}

/// One button a tab, the one in force lit.
fn tabs(s: &App, th: &Theme) -> El {
    row().key("browser-tabs").w_full().gap(2.0).children(Tab::ALL.into_iter().map(|tab| {
        let on = s.views.tab == tab;
        let name = t(tab.name()).to_string();
        row()
            .key(tab.key())
            .role("button")
            .label(name.clone())
            .focusable(true)
            .h(22.0)
            .px(6.0)
            .center_y()
            .radius(5.0)
            .fill(if on { th.control_off } else { Color::TRANSPARENT })
            .child(text(name).font(type_::LABEL).color(if on { th.text } else { th.text_secondary }).truncate())
            .on_press(move |s: &mut App| s.set_browser_tab(tab))
    }))
}

/// A group's name, and under it a row for each of its views unless it is folded; without the name when it is all there is.
fn group(g: &Group, entries: &[&Entry], lit: &dyn Fn(&Entry) -> bool, folded: bool, flat: bool, th: &Theme) -> Vec<El> {
    let label = g.label;
    let rows: Vec<El> = if folded { Vec::new() } else { entries.iter().map(|e| entry(e, lit(e), flat, th)).collect() };
    if flat {
        return rows;
    }
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
        .on_press(move |s: &mut App| s.views.toggle(label))
        .on_secondary_press(move |s: &mut App, p, _| s.open_browser_menu(Target::Group(label), p.x, p.y));
    std::iter::once(head).chain(rows).collect()
}

/// A row: pressing it does what it is for (`App::press_row`); Ctrl and Shift pick views together.
fn entry(e: &Entry, lit: bool, flat: bool, th: &Theme) -> El {
    let (kind, pick) = (e.kind, e.pick.clone());
    let target = match &e.pick {
        Pick::Open => Target::View(e.kind),
        Pick::Type(category, name) => Target::Type(category.clone(), name.clone()),
        Pick::Select(id) => Target::Link(*id),
    };
    row()
        .key(e.key.clone())
        .role("button")
        .label(e.label.clone())
        .focusable(true)
        .h(24.0)
        .pl(if flat { 8.0 } else { 22.0 })
        .pr(8.0)
        .center_y()
        .radius(6.0)
        .fill(if lit { th.control_off } else { Color::TRANSPARENT })
        .child(text(e.label.clone()).font(type_::BODY).color(th.text).truncate())
        .on_press_tx(move |s: &mut App, tx: &mut Tx| {
            let m = tx.mods();
            s.press_row(&pick, kind, m.shift, m.cmd);
        })
        .on_secondary_press(move |s: &mut App, p, _| s.open_browser_menu(target.clone(), p.x, p.y))
}

/// `body`, with the browser's menu open over it when there is one: `rune_kit`'s `Menu` at the point the right click came from.
fn with_menu(s: &App, body: El, layer: &mut Vec<El>) -> El {
    let Some((x, y, _)) = s.views.menu else { return body };
    let kit = Kit::neutral(s.dark());
    let mut menu = Menu::new("browser-menu").trigger(body).at(x, y).open(true).on_close(|s: &mut App| s.close_browser_menu());
    for (n, group) in s.browser_menu_items().into_iter().enumerate() {
        if n > 0 {
            menu = menu.separator();
        }
        for item in group {
            let action = item.action;
            menu =
                menu.item(MenuItem::new(t(action.label()).to_string()).disabled(!item.enabled).on_select(move |s: &mut App| s.act(action)));
        }
    }
    menu.build(&kit, layer)
}

#[cfg(test)]
mod tests;
