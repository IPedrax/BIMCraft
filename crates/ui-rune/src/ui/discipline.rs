//! The discipline dialogs, drawn: one widget for every `Discipline` (`discipline.rs`), from what the dialog says is on it. Tabs over the
//! sections, a section's rows of cells (words, boxes, lists, check boxes, buttons), then what the dialog notes and what stopped it, then
//! its foot. A handler only calls a method of `App` (`app/discipline.rs`); a refusal is said inside the dialog, where no status line can
//! cover it.

use rune::{El, FileDialogOptions, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Checkbox, Dialog, Kit, SegmentedControl, Select, SelectAppearance, TextInput};

use crate::app::App;
use crate::app::discipline::Open;
use crate::discipline::{Cell, Foot};

/// A box typed in.
fn field(edit: usize, name: &str, hint: &str, width: f32, o: &Open, k: &Kit) -> El {
    let Some(e) = o.dialog.edit(edit) else { return el() };
    TextInput::new(e)
        .label(name.to_string())
        .placeholder(hint.to_string())
        .width(Some(width))
        .on_edit(move |s: &mut App| s.dialogs.discipline_edit(edit))
        .build(k)
        .key(format!("discipline-edit-{edit}"))
}

/// A list to choose from, its choices opened over the dialog when it is the one open.
fn pick(i: usize, width: f32, o: &Open, k: &Kit, layer: &mut Vec<El>) -> El {
    let Some(choice) = o.dialog.choice(i) else { return el() };
    let current = choice.current.and_then(|c| choice.options.get(c)).map(String::as_str);
    let options = choice.options.clone();
    Select::new(format!("discipline-pick-{i}"), &choice.options)
        .label(choice.name.clone())
        .current(current)
        .appearance(SelectAppearance::Field)
        .width(width)
        .open(o.open == Some(i))
        .on_toggle(move |s: &mut App| s.discipline_toggle(i))
        .on_close(|s: &mut App| s.discipline_close_list())
        .on_choose(move |s: &mut App, picked: &str, _: &mut Tx| {
            if let Some(option) = options.iter().position(|name| name == picked) {
                s.discipline_choose(i, option);
            }
        })
        .build(k, layer)
}

fn cell(c: &Cell, o: &Open, k: &Kit, layer: &mut Vec<El>) -> El {
    match c {
        Cell::Text { text: words, strong, width } => {
            let color = if *strong { k.c.text_base } else { k.c.text_muted };
            text(words.clone()).font(k.ty.ui).color(color).wrap().w(*width)
        }
        Cell::Field { edit, name, hint, width } => field(*edit, name, hint, *width, o, k),
        Cell::Pick { pick: i, width } => pick(*i, *width, o, k, layer),
        Cell::Button { action, arg, label, on } => {
            let (action, arg) = (*action, *arg);
            Button::new(label.clone())
                .disabled(!*on)
                .on_press(move |s: &mut App| {
                    s.discipline_press(action, arg);
                })
                .build(k)
                .key(format!("discipline-{action}-{arg}"))
        }
        Cell::Check { action, label, on, enabled } => {
            let action = *action;
            let check = Checkbox::new(*on).label(label.clone()).disabled(!*enabled).on_toggle(move |s: &mut App| {
                s.discipline_press(action, 0);
            });
            check.build(k).key(format!("discipline-{action}"))
        }
    }
}

/// A button of the foot. One that asks for a file opens the system's dialog first and tells the dialog where.
fn foot(f: &Foot, k: &Kit) -> El {
    let (action, label, file) = (f.action, f.label.clone(), f.file);
    let b = Button::new(label.clone()).disabled(!f.on);
    let b = if f.primary { b.variant(ButtonVariant::Submit) } else { b };
    let b = match file {
        None => b.on_press(move |s: &mut App| {
            s.discipline_press(action, 0);
        }),
        Some(name) => b.on_press_tx(move |_: &mut App, tx: &mut Tx| {
            let options = FileDialogOptions { title: label.clone(), file_name: Some(name.to_string()), ..Default::default() }
                .filter("CSV", &["csv"])
                .filter("Excel", &["xlsx"]);
            tx.save_file(options, move |s: &mut App, answer, _| match answer {
                Ok(Some(path)) => {
                    s.discipline_saved(action, path);
                }
                Ok(None) => {}
                Err(why) => s.message = Some(why.to_string()),
            });
        }),
    };
    b.build(k).key(format!("discipline-foot-{action}"))
}

/// The dialog's scrim and panel, when one is open.
pub fn dialog(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(o) = &s.dialogs.discipline else { return };
    let d = &*o.dialog;
    let mut body = col().key("discipline-body").w_full().padding(16.0).gap(10.0);
    let tabs = d.tabs();
    if !tabs.is_empty() {
        body = body.child(
            SegmentedControl::new(&tabs, Some(d.tab()))
                .on_pick(|s: &mut App, i: usize, _: &mut Tx| {
                    s.discipline_press("tab", i);
                })
                .build(k)
                .key("discipline-tabs"),
        );
    }
    let about = d.about();
    if !about.is_empty() {
        body = body.child(text(about).font(k.ty.caption()).color(k.c.text_muted).wrap().w_full());
    }
    for (si, section) in d.sections().iter().enumerate() {
        if !section.title.is_empty() {
            body = body.child(
                text(section.title.clone()).key(format!("discipline-title-{si}")).font(k.ty.ui).color(k.c.text_base).wrap().w_full(),
            );
        }
        for (ri, cells) in section.rows.iter().enumerate() {
            let built: Vec<El> = cells.iter().map(|c| cell(c, o, k, layer)).collect();
            body = body.child(row().key(format!("discipline-row-{si}-{ri}")).gap(8.0).center_y().children(built));
        }
    }
    let mut status = col().key("discipline-status").w_full().px(16.0).gap(4.0);
    for note in d.notes() {
        status = status.child(text(format!("\u{2022} {note}")).font(k.ty.ui).color(k.c.text_muted).wrap().w_full());
    }
    if let Some(why) = d.error() {
        status = status.child(text(why.to_string()).key("discipline-error").font(k.ty.ui).color(k.c.danger.fg).wrap().w_full());
    }
    let feet: Vec<El> = d.foot().iter().map(|f| foot(f, k)).collect();
    let footer = row().key("discipline-footer").w_full().padding(16.0).gap(8.0).justify_end().children(feet);
    Dialog::new("discipline", d.title())
        .width(d.width())
        .fit()
        .open(true)
        .on_close(|s: &mut App| s.close_dialogs())
        .child(body.shrink().min_h(0.0).scroll(ScrollSpec::default()))
        .child(status)
        .child(footer)
        .build(k, layer);
}
