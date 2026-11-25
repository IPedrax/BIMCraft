//! A row of the palette and the editor it takes: read-only text, a field, a check box, a list (opened in place, under its row, so no
//! popup is over the window), a link and the button of a dialog this window does not have yet. A handler only calls a method of
//! `App` (`app/properties.rs`); what an edit becomes, and every change to the model, is the core's through `Session::execute`.

use rune::tok::{Theme, type_};
use rune::{El, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Checkbox, Kit, TextInput};

use crate::app::App;
use crate::props::panel::{Editor, Pick, Row};

/// The label's width, points.
const LABEL: f32 = 110.0;
/// How tall an open list grows before it scrolls, points.
const LIST: f32 = 220.0;

/// A button: its key, its words and what a press does.
pub fn button(key: impl Into<String>, label: &str, th: &Theme, on_press: impl Fn(&mut App) + 'static) -> El {
    el().key(key)
        .role("button")
        .label(label.to_string())
        .focusable(true)
        .px(10.0)
        .h(26.0)
        .center()
        .radius(6.0)
        .fill(th.control_track)
        .child(text(label.to_string()).font(type_::LABEL).color(th.text))
        .on_press(on_press)
}

/// The row: its label, and its editor beside it.
pub fn row_of(s: &App, r: &Row, th: &Theme, kit: &Kit) -> El {
    let value = match &r.editor {
        Editor::Read => text(r.shown.clone()).font(type_::BODY).color(th.text_secondary).truncate(),
        Editor::Text { hint } => field(s, r, hint, kit),
        Editor::Check(on) => {
            let (key, on) = (r.key, *on);
            Checkbox::new(on)
                .label(r.label.clone())
                .hide_label()
                .on_toggle(move |s: &mut App| {
                    s.set_check(key, !on);
                })
                .build(kit)
        }
        Editor::Choice { current, options } => list(s, r, *current, options, th),
        Editor::Link(id) => {
            let id = *id;
            row()
                .key(format!("link-{}", r.key))
                .role("link")
                .label(r.shown.clone())
                .focusable(true)
                .child(text(r.shown.clone()).font(type_::BODY).color(th.text).truncate())
                .on_press(move |s: &mut App| s.select_link(id))
        }
        Editor::Seam(what) => {
            let what = *what;
            button(format!("seam-{}", r.key), &r.shown, th, move |s: &mut App| s.seam(what))
        }
    };
    row()
        .key(format!("row-{}", r.key))
        .gap(8.0)
        .center_y()
        .child(text(r.label.clone()).font(type_::LABEL).color(th.text_secondary).w(LABEL))
        .child(value.grow(1.0))
}

/// A field, committed with Enter.
fn field(s: &App, r: &Row, hint: &str, kit: &Kit) -> El {
    let Some((i, f)) = s.palette.fields.iter().enumerate().find(|(_, f)| f.key == r.key) else {
        return text(r.shown.clone()).font(type_::BODY).truncate();
    };
    TextInput::new(&f.edit)
        .label(r.label.clone())
        .placeholder(hint.to_string())
        .width(None)
        .on_edit(move |s: &mut App| s.palette.fields.get_mut(i).map_or(&mut s.palette.scratch, |f| &mut f.edit))
        .on_submit(move |s: &mut App, _: &mut Tx| {
            s.commit_field(i);
        })
        .build(kit)
}

/// A list: what is in force, which opens the options in place under it.
fn list(s: &App, r: &Row, current: Option<usize>, options: &[Pick], th: &Theme) -> El {
    let key = r.key;
    let open = s.palette.open == Some(key);
    let trigger = row()
        .key(format!("pick-{key}"))
        .role("button")
        .label(r.label.clone())
        .expanded(open)
        .focusable(true)
        .w_full()
        .h(26.0)
        .px(8.0)
        .gap(6.0)
        .center_y()
        .radius(6.0)
        .fill(th.control_track)
        .child(text(r.shown.clone()).font(type_::BODY).color(th.text).truncate().grow(1.0))
        .child(text(if open { "▴" } else { "▾" }).font(type_::LABEL).color(th.text_secondary))
        .on_press(move |s: &mut App| s.open_list(key));
    if !open {
        return col().w_full().child(trigger);
    }
    let items = options.iter().enumerate().map(|(i, o)| {
        let item = row()
            .key(format!("opt-{key}-{i}"))
            .role("button")
            .label(o.label.clone())
            .focusable(true)
            .w_full()
            .h(24.0)
            .px(8.0)
            .center_y()
            .radius(4.0)
            .child(text(o.label.clone()).font(type_::BODY).color(th.text).truncate())
            .on_press(move |s: &mut App| {
                s.choose(key, i);
            });
        if current == Some(i) { item.fill(th.control_off) } else { item }
    });
    let shown = col()
        .key(format!("list-{key}"))
        .w_full()
        .gap(1.0)
        .padding(4.0)
        .radius(6.0)
        .fill(th.card)
        .border(1.0, th.card_border)
        .max_h(LIST)
        .children(items)
        .scroll(ScrollSpec::default());
    col().w_full().gap(4.0).child(trigger).child(shown)
}
