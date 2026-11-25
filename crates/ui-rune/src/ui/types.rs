//! Type Properties (Edit Type in the Properties panel, and Find): the type of the selected element, a list to show another type of
//! its category, the fields of the type that can be typed in boxes and the rest as rows, and Apply. The state and what Apply does are
//! `app/types.rs`; this only draws. A refusal, or a field that is no number, is said under the rows.

use rune::{El, ScrollSpec, Tx, col, row, text};
use rune_kit::{Button, ButtonVariant, Dialog, Kit, Select, SelectAppearance, TextInput};

use crate::app::App;
use crate::lang::t;

/// The dialog's width, points.
const WIDTH: f32 = 560.0;

fn line(k: &Kit, name: &str, control: El) -> El {
    row()
        .w_full()
        .gap(12.0)
        .items_start()
        .child(text(name.to_string()).font(k.ty.ui).color(k.c.text_base).w(140.0).mt(4.0))
        .child(control.grow(1.0))
}

/// The dialog's scrim and panel, when it is open.
pub fn dialog(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(d) = &s.dialogs.types else { return };
    let types = Select::new("type", &d.names)
        .label(t("Type").to_string())
        .current(Some(d.key.name.as_str()))
        .appearance(SelectAppearance::Field)
        .width(300.0)
        .open(d.open)
        .on_toggle(|s: &mut App| s.toggle_type_list())
        .on_close(|s: &mut App| s.close_type_list())
        .on_choose(|s: &mut App, name: &str, _: &mut Tx| {
            s.switch_type(name);
        })
        .build(k, layer);
    let mut rows = col().key("type-rows").w_full().padding(16.0).gap(10.0).child(line(k, t("Type"), types));
    for (i, f) in d.fields.iter().enumerate() {
        let input = TextInput::new(&f.edit)
            .label(f.name.clone())
            .width(None)
            .on_edit(move |s: &mut App| s.dialogs.type_edit(i))
            .on_submit(|s: &mut App, _: &mut Tx| {
                s.apply_type();
            })
            .build(k)
            .key(format!("type-field-{i}"));
        rows = rows.child(line(k, &f.name, input));
    }
    // What cannot be typed here (a wall's layers) is shown and left to the commands made for it.
    for r in d.rows.iter().filter(|r| !d.fields.iter().any(|f| f.name == r.name)) {
        rows = rows.child(line(k, &r.name, text(r.value.clone()).font(k.ty.ui).color(k.c.text_muted).wrap().w_full().mt(4.0)));
    }
    let mut status = col().key("type-status").w_full().px(16.0).gap(8.0);
    if let Some(why) = &d.error {
        status = status.child(text(why.clone()).key("type-error").font(k.ty.ui).color(k.c.danger.fg).wrap().w_full());
    } else if d.done {
        status = status.child(text(t("Applied").to_string()).key("type-done").font(k.ty.ui).color(k.c.text_muted));
    }
    let footer = row()
        .key("type-footer")
        .w_full()
        .padding(16.0)
        .gap(8.0)
        .justify_end()
        .child(Button::new(t("Close").to_string()).on_press(|s: &mut App| s.close_dialogs()).build(k).key("type-close"))
        .child(
            Button::new(t("Apply").to_string())
                .variant(ButtonVariant::Submit)
                .on_press(|s: &mut App| {
                    s.apply_type();
                })
                .build(k)
                .key("type-apply"),
        );
    Dialog::new("types", t("Type Properties").to_string())
        .description(format!("{}: {}", d.key.category, d.key.name))
        .width(WIDTH)
        .fit()
        .open(true)
        .on_close(|s: &mut App| s.close_dialogs())
        .child(rows.shrink().min_h(0.0).scroll(ScrollSpec::default()))
        .child(status)
        .child(footer)
        .build(k, layer);
}
