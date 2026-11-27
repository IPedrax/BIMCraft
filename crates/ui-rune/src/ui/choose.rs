//! The list Aligned to Selected Levels and Aligned to Selected Views ticks (`clip::Choose`): a row to each level or view, a tick on the ones
//! picked, and Paste. The state and what Paste does are `app/clip.rs`; this only draws.

use rune::{El, ScrollSpec, col, row, text};
use rune_kit::{Button, ButtonVariant, Dialog, Kit};

use crate::app::App;
use crate::lang::t;

/// The dialog's width, points.
const WIDTH: f32 = 420.0;

/// The dialog's scrim and panel, when it is open.
pub fn dialog(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(c) = &s.dialogs.choose else { return };
    let mut rows = col().key("choose-rows").w_full().padding(8.0).gap(2.0);
    for (i, (id, name)) in c.offered.iter().enumerate() {
        let (id, on) = (*id, c.picked.contains(id));
        rows = rows.child(
            row()
                .key(format!("choose-row-{i}"))
                .role("button")
                .label(name.clone())
                .focusable(true)
                .h(28.0)
                .px(8.0)
                .gap(8.0)
                .center_y()
                .radius(6.0)
                .child(text(if on { "\u{2611}" } else { "\u{2610}" }).font(k.ty.ui).color(k.c.text_base))
                .child(text(name.clone()).font(k.ty.ui).color(k.c.text_base).truncate())
                .on_press(move |s: &mut App| s.choose_toggle(id)),
        );
    }
    if c.offered.is_empty() {
        let none = if c.views { t("No view like the one they came from.") } else { t("This project has no level.") };
        rows = rows.child(text(none.to_string()).key("choose-none").font(k.ty.ui).color(k.c.text_muted));
    }
    let footer = row()
        .key("choose-footer")
        .w_full()
        .padding(16.0)
        .gap(8.0)
        .justify_end()
        .child(Button::new(t("Cancel").to_string()).on_press(|s: &mut App| s.close_dialogs()).build(k).key("choose-cancel"))
        .child(
            Button::new(t("Paste").to_string())
                .variant(ButtonVariant::Submit)
                .on_press(|s: &mut App| {
                    s.choose_paste();
                })
                .build(k)
                .key("choose-ok"),
        );
    let title = if c.views { t("Aligned to Selected Views") } else { t("Aligned to Selected Levels") };
    Dialog::new("choose", title.to_string())
        .width(WIDTH)
        .fit()
        .open(true)
        .on_close(|s: &mut App| s.close_dialogs())
        .child(rows.shrink().min_h(0.0).scroll(ScrollSpec::default()))
        .child(footer)
        .build(k, layer);
}
