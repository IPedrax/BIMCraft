//! The Properties panel: the selected element's rows (`App::props`) and the buttons that run commands on it.

use rune::tok::{Theme, type_};
use rune::{El, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Kit, TextInput};

use crate::app::App;
use crate::lang::t;

pub fn panel(s: &App, th: &Theme) -> El {
    let title = text(t("Properties")).font(type_::TITLE).color(th.text);
    let buttons = row().gap(8.0).child(button("delete", t("Delete"), th, |s: &mut App| {
        s.delete_selection();
    }));
    let body = if s.props.is_empty() {
        col().child(text(t("Nothing selected")).font(type_::BODY).color(th.text_secondary))
    } else {
        let kit = Kit::neutral(true);
        col().key("rows").gap(4.0).children(s.props.iter().map(|r| {
            let value = match s.fields.iter().position(|f| f.name == r.name) {
                Some(i) => TextInput::new(&s.fields[i].edit)
                    .label(r.name.clone())
                    .width(None)
                    .on_edit(move |s: &mut App| s.fields.get_mut(i).map_or(&mut s.scratch, |f| &mut f.edit))
                    .on_submit(move |s: &mut App, _: &mut Tx| {
                        s.commit_field(i);
                    })
                    .build(&kit),
                None => text(r.value.clone()).font(type_::BODY).color(th.text).truncate(),
            };
            row()
                .gap(8.0)
                .center_y()
                .child(text(r.name.clone()).font(type_::LABEL).color(th.text_secondary).w(100.0))
                .child(value.grow(1.0))
        }))
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
        .child(body.scroll(ScrollSpec::default()).grow(1.0).min_h(0.0))
}

fn button(key: &'static str, label: &str, th: &Theme, on_press: impl Fn(&mut App) + 'static) -> El {
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
