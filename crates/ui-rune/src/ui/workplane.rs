//! The Work Plane dialog: Current Work Plane and Dissociate, then the name of a level or a reference plane to set, with its offset, and OK.
//! The state and what each button does are `app/workplane.rs`; this only draws. A refusal, or an offset that is no length, is said under the
//! fields. As Revit's, the 3D view's work plane; Show and the three picks are not here yet.

use rune::{El, Tx, col, row, text};
use rune_kit::{Button, ButtonVariant, Dialog, Kit, Select, SelectAppearance, TextInput};

use crate::app::App;
use crate::lang::t;

/// The dialog's width, points.
const WIDTH: f32 = 480.0;

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
    let Some(d) = &s.dialogs.workplane else { return };
    let names = Select::new("work-plane-name", &d.choices)
        .label(t("Name").to_string())
        .current(Some(d.name.as_str()))
        .appearance(SelectAppearance::Field)
        .width(300.0)
        .open(d.open)
        .on_toggle(|s: &mut App| s.toggle_work_plane_list())
        .on_close(|s: &mut App| s.close_work_plane_list())
        .on_choose(|s: &mut App, picked: &str, _: &mut Tx| s.choose_work_plane(picked))
        .build(k, layer);
    let offset = TextInput::new(&d.offset)
        .width(None)
        .on_edit(|s: &mut App| s.dialogs.work_plane_edit())
        .on_submit(|s: &mut App, _: &mut Tx| {
            s.apply_work_plane();
        })
        .build(k)
        .key("work-plane-offset");
    let follows = s.work_plane_follows();
    let rows = col()
        .key("work-plane-rows")
        .w_full()
        .padding(16.0)
        .gap(10.0)
        .child(line(
            k,
            t("Current Work Plane"),
            row()
                .w_full()
                .gap(12.0)
                .items_start()
                .child(text(s.work_plane_name()).key("work-plane-current").font(k.ty.ui).color(k.c.text_base).wrap().grow(1.0).mt(4.0))
                .child(
                    Button::new(t("Dissociate").to_string())
                        .disabled(!follows)
                        .on_press(|s: &mut App| {
                            s.dissociate_work_plane();
                        })
                        .build(k)
                        .key("work-plane-dissociate"),
                ),
        ))
        .child(line(k, t("Specify a new Work Plane"), names))
        .child(line(k, t("Offset"), offset));
    let mut status = col().key("work-plane-status").w_full().px(16.0).gap(8.0);
    if let Some(why) = &d.error {
        status = status.child(text(why.clone()).key("work-plane-error").font(k.ty.ui).color(k.c.danger.fg).wrap().w_full());
    }
    let footer = row()
        .key("work-plane-footer")
        .w_full()
        .padding(16.0)
        .gap(8.0)
        .justify_end()
        .child(Button::new(t("Close").to_string()).on_press(|s: &mut App| s.close_dialogs()).build(k).key("work-plane-close"))
        .child(
            Button::new(t("OK").to_string())
                .variant(ButtonVariant::Submit)
                .on_press(|s: &mut App| {
                    s.apply_work_plane();
                })
                .build(k)
                .key("work-plane-ok"),
        );
    Dialog::new("work-plane-dialog", t("Work Plane").to_string())
        .width(WIDTH)
        .fit()
        .open(true)
        .on_close(|s: &mut App| s.close_dialogs())
        .child(rows)
        .child(status)
        .child(footer)
        .build(k, layer);
}
