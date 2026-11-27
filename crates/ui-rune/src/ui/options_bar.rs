//! The Options Bar: a row over the plan's top left, under the shape bar, holding what the tool in hand lets a person set before the click
//! (`options::items`): a height, a location line or a justification as a list that opens under the bar, typed fields, Chain and the
//! like. A handler only calls a method of `App` (`app/options.rs`); the settings join the params of the click there.

use rune::tok::{self, Theme, type_};
use rune::{El, ScrollSpec, col, el, row, text};
use rune_kit::{Checkbox, Kit, TextInput};

use super::draw_bar;
use crate::app::App;
use crate::options::{self, Editor, Item};

/// How wide a typed field is, points.
const FIELD: f32 = 72.0;
/// How tall an open list grows before it scrolls, points.
const LIST: f32 = 200.0;

fn button(key: String, label: &str, th: &Theme, on_press: impl Fn(&mut App) + 'static) -> El {
    el().key(key)
        .role("button")
        .label(label.to_string())
        .focusable(true)
        .px(8.0)
        .h(24.0)
        .center()
        .radius(6.0)
        .fill(th.control_track)
        .child(text(label.to_string()).font(type_::LABEL).color(th.text))
        .on_press(on_press)
}

/// A typed field of the bar, by the place of its text edit among the bar's fields.
fn field(s: &App, item: &Item, hint: &str, kit: &Kit) -> El {
    let Some((i, f)) = s.bar.fields.iter().enumerate().find(|(_, f)| f.key == item.key) else {
        return text(item.label.clone()).font(type_::LABEL);
    };
    TextInput::new(&f.edit)
        .label(item.label.clone())
        .placeholder(hint.to_string())
        .width(Some(FIELD))
        .on_edit(move |s: &mut App| s.bar.fields.get_mut(i).map_or(&mut s.palette.scratch, |f| &mut f.edit))
        .build(kit)
}

fn cell(s: &App, item: &Item, th: &Theme, kit: &Kit) -> El {
    let key = item.key;
    let label = (!item.label.is_empty()).then(|| text(item.label.clone()).font(type_::LABEL).color(th.text_secondary));
    let editor = match &item.editor {
        Editor::Field { hint } => field(s, item, hint, kit),
        Editor::List { now, .. } => button(format!("bar-{key}"), now, th, move |s: &mut App| s.bar_open(key)),
        Editor::Check(on) => {
            let on = *on;
            Checkbox::new(on)
                .label(item.label.clone())
                .on_toggle(move |s: &mut App| {
                    s.bar_check(key, !on);
                })
                .build(kit)
        }
    };
    // A check box says its own label.
    let label = if matches!(item.editor, Editor::Check(_)) { None } else { label };
    row().gap(4.0).center_y().children(label).child(editor)
}

/// The open list's options, one under the other.
fn options_of(key: &'static str, now: &str, options: &[String], th: &Theme) -> El {
    let items = options.iter().enumerate().map(|(i, name)| {
        let b = row()
            .key(format!("bar-opt-{key}-{i}"))
            .role("button")
            .label(name.clone())
            .focusable(true)
            .w_full()
            .h(24.0)
            .px(8.0)
            .center_y()
            .radius(4.0)
            .child(text(name.clone()).font(type_::BODY).color(th.text).truncate())
            .on_press(move |s: &mut App| {
                s.bar_choose(key, i);
            });
        if name == now { b.fill(th.control_off) } else { b }
    });
    col()
        .key("bar-list")
        .gap(1.0)
        .padding(4.0)
        .radius(6.0)
        .fill(th.card)
        .border(1.0, th.card_border)
        .max_h(LIST)
        .children(items)
        .scroll(ScrollSpec::default())
}

/// The bar of the tool in hand, or none while the tool has nothing to set.
pub fn bar(s: &App) -> Option<El> {
    let (tool, level) = (s.tool?, s.level?);
    let items = options::items(tool, &s.session.model, level, &s.bar.settings);
    if items.is_empty() {
        return None;
    }
    let th = tok::theme(s.dark());
    let kit = Kit::neutral(s.dark());
    let cells: Vec<El> = items.iter().map(|item| cell(s, item, &th, &kit)).collect();
    // Under the shape bar, which the Wall tool has: two rows of buttons, 8 + 4 + 24 + 4 + 24 + 4 points tall.
    let top = if draw_bar::takes_shape(s) { 72.0 } else { 8.0 };
    let mut c = col().key("options-bar").gap(4.0).pin(Some(8.0), Some(top), None, None);
    c = c.child(row().gap(10.0).padding(4.0).radius(8.0).fill(th.card).center_y().children(cells));
    let open = s.bar.open.and_then(|key| items.iter().find(|i| i.key == key));
    if let Some(item) = open
        && let Editor::List { now, options } = &item.editor
    {
        c = c.child(options_of(item.key, now, options, &th));
    }
    Some(c)
}

#[cfg(test)]
mod tests;
