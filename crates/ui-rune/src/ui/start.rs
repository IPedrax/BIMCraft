//! The Start dialog (Find: Start): New, Open, the sample house, the recent files and the templates New starts a project from. The state
//! and what each does are `app/start.rs`; this only draws. A file that will not open is said under the lists.

use rune::{El, FileDialogOptions, ScrollSpec, Tx, col, row, text};
use rune_kit::{Button, ButtonVariant, Dialog, Kit};

use crate::app::App;
use crate::lang::t;
use crate::recent;

/// The dialog's width, points.
const WIDTH: f32 = 520.0;

/// A list of files as one button each, under a heading; the button's key is `prefix-i`.
fn list(k: &Kit, heading: &str, prefix: &'static str, paths: &[std::path::PathBuf]) -> Option<El> {
    if paths.is_empty() {
        return None;
    }
    let mut col =
        col().key(format!("{prefix}s")).w_full().gap(6.0).child(text(heading.to_string()).font(k.ty.caption()).color(k.c.text_muted));
    for (i, (path, label)) in paths.iter().zip(recent::labels(paths)).enumerate() {
        let path = path.clone();
        col = col.child(
            Button::new(label)
                .on_press(move |s: &mut App| {
                    s.start_open(&path);
                })
                .build(k)
                .key(format!("{prefix}-{i}")),
        );
    }
    Some(col)
}

/// The dialog's scrim and panel, when it is open.
pub fn dialog(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(d) = &s.dialogs.start else { return };
    let open = Button::new(t("Open").to_string()).build(k).key("start-open").on_press_tx(|_: &mut App, tx: &mut Tx| {
        let options = FileDialogOptions { title: t("Open").to_string(), ..Default::default() }
            .filter("BIMCraft", &["bimc", "bimcraft", "bimt", "ifc"]);
        tx.open_file(options, |s: &mut App, answer, _| match answer {
            Ok(Some(path)) => {
                s.start_open(&path);
            }
            Ok(None) => {}
            Err(why) => s.message = Some(why.to_string()),
        });
    });
    let actions = row()
        .key("start-actions")
        .w_full()
        .gap(8.0)
        .child(
            Button::new(t("New").to_string())
                .variant(ButtonVariant::Submit)
                .on_press(|s: &mut App| {
                    s.start_new();
                })
                .build(k)
                .key("start-new"),
        )
        .child(open)
        .child(
            Button::new(t("Sample house").to_string())
                .on_press(|s: &mut App| {
                    s.start_sample();
                })
                .build(k)
                .key("start-sample"),
        );
    let mut rows = col().key("start-rows").w_full().padding(16.0).gap(14.0).child(actions);
    for lists in [list(k, t("Recent"), "recent", &d.recent), list(k, t("Templates"), "template", &d.templates)].into_iter().flatten() {
        rows = rows.child(lists);
    }
    let mut status = col().key("start-status").w_full().px(16.0).gap(8.0);
    if let Some(why) = &d.error {
        status = status.child(text(why.clone()).key("start-error").font(k.ty.ui).color(k.c.danger.fg).wrap().w_full());
    }
    let footer = row()
        .key("start-footer")
        .w_full()
        .padding(16.0)
        .gap(8.0)
        .justify_end()
        .child(Button::new(t("Close").to_string()).on_press(|s: &mut App| s.close_dialogs()).build(k).key("start-close"));
    Dialog::new("start", t("Start").to_string())
        .description(t("Start a project or open one").to_string())
        .width(WIDTH)
        .fit()
        .open(true)
        .on_close(|s: &mut App| s.close_dialogs())
        .child(rows.shrink().min_h(0.0).scroll(ScrollSpec::default()))
        .child(status)
        .child(footer)
        .build(k, layer);
}
