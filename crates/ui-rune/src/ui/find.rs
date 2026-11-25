//! Find (Ctrl+K, and the Find button on the top bar): a field, and under it what matches (`finder.rs`): Enter takes the row the arrow
//! keys are on, a click takes the one clicked, Escape or a click outside closes it. The state and what a row does are
//! `app/dialogs.rs`; this only draws.

use rune::{Color, El, Key, KeyEvent, ScrollSpec, Tx, col, row, text};
use rune_kit::{Dialog, Kit, TextInput};

use crate::app::App;
use crate::lang::t;

/// Find's width, points.
const WIDTH: f32 = 520.0;
/// A row's height, points.
const ROW: f32 = 28.0;

/// Find's scrim and panel, when it is open.
pub fn dialog(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(f) = &s.dialogs.find else { return };
    let hits = s.found();
    let at = f.at.min(hits.len().saturating_sub(1));
    let input = TextInput::new(&f.edit)
        .label(t("Find").to_string())
        .placeholder(t("Find a tool, a view or a command").to_string())
        .width(None)
        .on_edit(|s: &mut App| s.dialogs.find_edit())
        .on_change(|s: &mut App, _: &mut Tx| s.find_reset())
        .on_submit(|s: &mut App, _: &mut Tx| s.pick_current())
        .on_key(|s: &mut App, key: &KeyEvent, _: &mut Tx| match &key.key {
            Key::Down => {
                s.find_step(1);
                true
            }
            Key::Up => {
                s.find_step(-1);
                true
            }
            _ => false,
        })
        .build(k)
        .key("find-input");
    let mut rows = col().key("find-rows").w_full().gap(2.0);
    for (i, hit) in hits.iter().enumerate() {
        let on = if i == at { k.c.hover } else { Color::TRANSPARENT };
        rows = rows.child(
            k.interactive(
                row()
                    .key(format!("hit-{i}"))
                    .role("button")
                    .label(hit.label.clone())
                    .focusable(true)
                    .w_full()
                    .h(ROW)
                    .px(8.0)
                    .gap(8.0)
                    .center_y()
                    .radius(k.radius.md)
                    .fill(on)
                    .child(text(hit.label.clone()).font(k.ty.ui).color(k.c.text_base).grow(1.0).truncate())
                    .child(text(hit.note.clone()).font(k.ty.caption()).color(k.c.text_muted)),
            )
            .on_press(move |s: &mut App| s.pick_found(i)),
        );
    }
    let typed = !f.edit.text.trim().is_empty();
    if hits.is_empty() && typed {
        rows = rows.child(
            row()
                .key("find-none")
                .px(8.0)
                .h(ROW)
                .center_y()
                .child(text(t("Nothing by that name").to_string()).font(k.ty.ui).color(k.c.text_muted)),
        );
    }
    let body = col().w_full().padding(16.0).gap(8.0).child(input).child(rows.shrink().min_h(0.0).scroll(ScrollSpec::default()));
    Dialog::new("find", t("Find").to_string())
        .width(WIDTH)
        .fit()
        .open(true)
        .on_close(|s: &mut App| s.close_dialogs())
        .child(body)
        .build(k, layer);
}
