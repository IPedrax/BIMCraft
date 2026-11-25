//! The modal dialogs, as functions of `App`'s state (`app/dialogs.rs`): the generic form of a command, drawn from the core's own
//! table (`forms.rs`), and Find (`find.rs`). They are built into a layer the window mounts over everything else, so the plan is not
//! wrapped in anything for them. A handler only calls a method of `App`; Run goes through `Session::execute`, and what the form or the
//! core says against it is shown inside the dialog, where no status line can cover it.

use rune::{El, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Dialog, Kit, Select, SelectAppearance, TextInput};

use super::find;
use crate::app::App;
use crate::app::dialogs::FormState;
use crate::forms::{Field, Kind};
use crate::lang::t;

/// The form dialog's width, points.
const WIDTH: f32 = 600.0;
/// The tallest the reply of a command is shown before it scrolls, points.
const REPLY_HEIGHT: f32 = 160.0;

/// What a dialog that is open adds to the window: its scrim and panel, and the list of a select that is open.
pub fn layers(s: &App, k: &Kit, layer: &mut Vec<El>) {
    if let Some(f) = &s.dialogs.form {
        form(f, k, layer);
    }
    find::dialog(s, k, layer);
    super::settings::dialog(s, k, layer);
}

/// The text of a field's choice that means none.
const NONE: &str = "(none)";

fn choice(f: &FormState, i: usize, field: &Field, pairs: Vec<(String, String)>, k: &Kit, layer: &mut Vec<El>) -> El {
    let shown: Vec<String> = std::iter::once(NONE.to_string()).chain(pairs.iter().map(|(shown, _)| shown.clone())).collect();
    let chosen = f.entries.get(i).map(|e| e.choice.as_str()).unwrap_or_default();
    let current = pairs.iter().find(|(_, value)| value == chosen).map_or_else(|| NONE.to_string(), |(shown, _)| shown.clone());
    Select::new(format!("choice-{i}"), &shown)
        .label(field.name.clone())
        .current(Some(current.as_str()))
        .appearance(SelectAppearance::Field)
        .width(240.0)
        .open(f.open == Some(i))
        .on_toggle(move |s: &mut App| s.toggle_choices(i))
        .on_close(|s: &mut App| s.close_choices())
        .on_choose(move |s: &mut App, picked: &str, _: &mut Tx| {
            let value = pairs.iter().find(|(shown, _)| shown == picked).map(|(_, value)| value.clone()).unwrap_or_default();
            s.choose(i, value);
        })
        .build(k, layer)
}

/// The control of field `i`: a select for a choice or a yes or no, a text box for the rest.
fn control(f: &FormState, i: usize, field: &Field, k: &Kit, layer: &mut Vec<El>) -> El {
    let Some(entry) = f.entries.get(i) else { return el() };
    match &field.kind {
        Kind::Choice(options) => choice(f, i, field, options.iter().map(|o| (o.clone(), o.clone())).collect(), k, layer),
        Kind::Bool => {
            choice(f, i, field, vec![(t("yes").to_string(), "true".to_string()), (t("no").to_string(), "false".to_string())], k, layer)
        }
        kind => TextInput::new(&entry.edit)
            .label(field.name.clone())
            .placeholder(t(kind.hint()).to_string())
            .width(None)
            .on_edit(move |s: &mut App| s.dialogs.field_edit(Some(i)))
            .on_submit(|s: &mut App, _: &mut Tx| {
                s.run_form();
            })
            .build(k)
            .key(format!("field-{i}")),
    }
}

fn form(f: &FormState, k: &Kit, layer: &mut Vec<El>) {
    let mut rows = col().key("form-rows").w_full().padding(16.0).gap(10.0);
    if !f.form.prose.is_empty() {
        rows = rows.child(text(f.form.prose.clone()).font(k.ty.caption()).color(k.c.text_muted).wrap().w_full());
    }
    for (i, field) in f.form.fields.iter().enumerate() {
        let name = if field.required { format!("{} *", field.name) } else { field.name.clone() };
        rows = rows.child(
            row()
                .key(format!("row-{i}"))
                .w_full()
                .gap(12.0)
                .items_start()
                .child(text(name).font(k.ty.ui).color(k.c.text_base).w(140.0).mt(4.0))
                .child(control(f, i, field, k, layer).grow(1.0)),
        );
    }
    if f.form.rest {
        let more = TextInput::new(&f.rest)
            .label(t("More parameters").to_string())
            .placeholder(t("A JSON object, like {\"key\": 1}").to_string())
            .width(None)
            .on_edit(|s: &mut App| s.dialogs.field_edit(None))
            .on_submit(|s: &mut App, _: &mut Tx| {
                s.run_form();
            })
            .build(k)
            .key("field-rest");
        rows = rows.child(
            row()
                .key("row-rest")
                .w_full()
                .gap(12.0)
                .items_start()
                .child(text(t("More parameters").to_string()).font(k.ty.ui).color(k.c.text_base).w(140.0).mt(4.0))
                .child(more.grow(1.0)),
        );
    }
    let mut status = col().key("form-status").w_full().px(16.0).gap(8.0);
    if let Some(why) = &f.error {
        status = status.child(text(why.clone()).key("form-error").font(k.ty.ui).color(k.c.danger.fg).wrap().w_full());
    }
    if let Some(reply) = &f.reply {
        status = status.child(
            col()
                .w_full()
                .max_h(REPLY_HEIGHT)
                .shrink()
                .min_h(0.0)
                .padding(8.0)
                .radius(k.radius.md)
                .fill(k.c.layer_02)
                .child(text(reply.clone()).key("form-reply").font(k.ty.mono).color(k.c.text_base).wrap().w_full())
                .scroll(ScrollSpec::default()),
        );
    }
    let footer = row()
        .key("form-footer")
        .w_full()
        .padding(16.0)
        .gap(8.0)
        .justify_end()
        .child(Button::new(t("Close").to_string()).on_press(|s: &mut App| s.close_dialogs()).build(k).key("form-close"))
        .child(
            Button::new(t("Run").to_string())
                .variant(ButtonVariant::Submit)
                .on_press(|s: &mut App| {
                    s.run_form();
                })
                .build(k)
                .key("form-run"),
        );
    // The command's id, and its shortcut when it has one, under its name.
    let shortcuts = bimcraft_core::engine::find(f.form.id).map(|c| c.aliases.join(", ")).unwrap_or_default();
    let said = if shortcuts.is_empty() { f.form.id.to_string() } else { format!("{} · {shortcuts}", f.form.id) };
    Dialog::new("command", t(f.form.label).to_string())
        .description(said)
        .width(WIDTH)
        .fit()
        .open(true)
        .on_close(|s: &mut App| s.close_dialogs())
        .child(rows.shrink().min_h(0.0).scroll(ScrollSpec::default()))
        .child(status)
        .child(footer)
        .build(k, layer);
}
