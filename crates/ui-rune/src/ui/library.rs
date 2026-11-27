//! The library managers (Materials, Line Styles, Phases and the rest; Manage in the top bar, and Find): the entry shown chosen from a
//! list, its fields that can be typed in boxes and the rest as rows, a new name where the command renames, and Apply, Delete and New.
//! The state and what each does are `app/library.rs`; this only draws. A refusal, or a field that is no number, is said under the rows.

use rune::{El, ScrollSpec, Tx, col, row, text};
use rune_kit::{Button, ButtonVariant, Dialog, Kit, Select, SelectAppearance, TextInput};
use serde_json::Value;

use crate::app::App;
use crate::lang::t;
use crate::library::takes;

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

/// The dialog's scrim and panel, when a manager is open.
pub fn dialog(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(d) = &s.dialogs.library else { return };
    let entries = Select::new("lib-entry", &d.names)
        .label(t("Entry").to_string())
        .current(d.selected.as_deref())
        .placeholder(t("Nothing here yet: use New").to_string())
        .appearance(SelectAppearance::Field)
        .width(300.0)
        .open(d.open)
        .on_toggle(|s: &mut App| s.toggle_library_list())
        .on_close(|s: &mut App| s.close_library_list())
        .on_choose(|s: &mut App, name: &str, _: &mut Tx| s.library_select(name))
        .build(k, layer);
    let mut rows = col().key("lib-rows").w_full().padding(16.0).gap(10.0).child(line(k, t("Entry"), entries));
    for (i, f) in d.fields.iter().enumerate() {
        let input = TextInput::new(&f.edit)
            .label(f.name.clone())
            .width(None)
            .on_edit(move |s: &mut App| s.dialogs.library_edit(i))
            .on_submit(|s: &mut App, _: &mut Tx| {
                s.library_apply();
            })
            .build(k)
            .key(format!("lib-field-{i}"));
        rows = rows.child(line(k, &f.name, input));
    }
    if d.selected.is_some() && takes(d.info, "rename") {
        let input = TextInput::new(&d.rename)
            .label(t("New name").to_string())
            .placeholder(t("A new name for it").to_string())
            .width(None)
            .on_edit(|s: &mut App| s.dialogs.library_rename())
            .on_submit(|s: &mut App, _: &mut Tx| {
                s.library_apply();
            })
            .build(k)
            .key("lib-rename");
        rows = rows.child(line(k, t("New name"), input));
    }
    // What cannot be typed here (a pattern's lines, a filter's rules) is shown and left to the command's own form.
    for r in d.rows.iter().filter(|r| !d.fields.iter().any(|f| f.name == r.name)) {
        rows = rows.child(line(k, &r.name, text(r.value.clone()).font(k.ty.ui).color(k.c.text_muted).wrap().w_full().mt(4.0)));
    }
    // The forms of the other commands this kind of entry goes with.
    for (i, &id) in d.info.source.also().iter().enumerate() {
        let label = bimcraft_core::engine::find(id).map_or_else(|| id.to_string(), |c| t(c.label).to_string());
        let open = Button::new(label)
            .on_press(move |s: &mut App| {
                s.open_command(id);
            })
            .build(k)
            .key(format!("lib-also-{i}"));
        rows = rows.child(line(k, if i == 0 { t("Related") } else { "" }, open));
    }
    let mut status = col().key("lib-status").w_full().px(16.0).gap(8.0);
    if let Some(why) = &d.error {
        status = status.child(text(why.clone()).key("lib-error").font(k.ty.ui).color(k.c.danger.fg).wrap().w_full());
    } else if d.done {
        status = status.child(text(t("Applied").to_string()).key("lib-done").font(k.ty.ui).color(k.c.text_muted));
    }
    let mut footer = row().key("lib-footer").w_full().padding(16.0).gap(8.0).justify_end();
    if d.json.get("active").and_then(Value::as_bool) == Some(false) && takes(d.info, "activate") {
        footer = footer.child(
            Button::new(t("Make active").to_string())
                .on_press(|s: &mut App| {
                    s.library_activate();
                })
                .build(k)
                .key("lib-activate"),
        );
    }
    footer = footer.child(Button::new(t("New").to_string()).on_press(|s: &mut App| s.library_new()).build(k).key("lib-new"));
    if d.selected.is_some() && takes(d.info, "delete") {
        footer = footer.child(
            Button::new(t("Delete").to_string())
                .variant(ButtonVariant::Danger)
                .on_press(|s: &mut App| {
                    s.library_delete();
                })
                .build(k)
                .key("lib-delete"),
        );
    }
    footer = footer.child(Button::new(t("Close").to_string()).on_press(|s: &mut App| s.close_dialogs()).build(k).key("lib-close")).child(
        Button::new(t("Apply").to_string())
            .variant(ButtonVariant::Submit)
            .on_press(|s: &mut App| {
                s.library_apply();
            })
            .build(k)
            .key("lib-apply"),
    );
    Dialog::new("library", t(d.info.title).to_string())
        .description(d.info.command.to_string())
        .width(WIDTH)
        .fit()
        .open(true)
        .on_close(|s: &mut App| s.close_dialogs())
        .child(rows.shrink().min_h(0.0).scroll(ScrollSpec::default()))
        .child(status)
        .child(footer)
        .build(k, layer);
}
