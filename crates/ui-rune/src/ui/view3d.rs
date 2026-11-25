//! The canvas of a 3D view: the core's software picture of the model (the image the view's thread made), moved through Rune's live
//! offset and scale while the camera pans or zooms and the thread makes the next. The wheel zooms about the pointer, the middle button
//! pans, Shift and the middle button orbit; the primary button picks or drags out a box. The selection is tinted in the picture itself.

use rune::tok::type_;
use rune::{El, Phase, PointerButton, Shape, canvas, el, text};

use crate::app::App;
use crate::lang::t;
use crate::paint::{self, Palette};
use crate::select::How;
use bimcraft_core::render::{PAPER, Rgb};

pub fn view(s: &App) -> El {
    let pal = Palette::dark();
    let o = &s.orbit;
    let picture = el().key("view3d-picture").inert().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).pixels(&o.pixels);
    let world = el()
        .key("view3d-world")
        .inert()
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .live_offset(&o.live_offset)
        .live_scale(&o.live_scale)
        .child(picture);
    let band = match &s.rubber {
        Some(r) => canvas(paint::rubber(r, &pal).shape),
        None => canvas(Shape::new()),
    }
    .key("view3d-rubber")
    .inert()
    .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0));
    let note = match (&o.error, &o.frame) {
        (Some(why), _) => Some(why.clone()),
        (None, None) => Some(t("Making the view...").to_string()),
        _ => None,
    };
    el().key("view3d")
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .fill(paint::ink(PAPER, false))
        .clip_soft(0.0)
        .on_layout(|s: &mut App, r| s.orbit_resize(r[2], r[3]))
        .on_pointer_move_local(|s: &mut App, e, _| s.orbit_hover(e.x, e.y))
        .on_hover_change(|s: &mut App, over, _| {
            if !over {
                s.orbit_leave();
            }
        })
        .on_wheel(|s: &mut App, e, _| {
            s.orbit_zoom(e.x, e.y, (e.dy * 0.002).exp());
            true
        })
        .pointer_buttons([PointerButton::Middle])
        .on_drag(|s: &mut App, d, tx| {
            let (from, to) = ((d.start_x, d.start_y), (d.x, d.y));
            match (tx.button(), d.phase) {
                (PointerButton::Middle, Phase::Down) => s.orbit_grab(from, tx.mods().shift),
                (PointerButton::Middle, Phase::Move) => s.orbit_drag(d.dx, d.dy),
                (PointerButton::Middle, Phase::Up) => s.orbit_release(),
                (_, Phase::Down) => {}
                (_, Phase::Move) => s.orbit_sweep(from, to),
                (_, Phase::Up) if d.cancelled() => s.cancel_press(),
                (_, Phase::Up) => {
                    let m = tx.mods();
                    s.orbit_select(from, to, How::of(m.shift, m.cmd));
                }
            }
        })
        .child(world)
        .child(band)
        .children(note.map(|n| {
            text(n).key("view3d-note").font(type_::BODY).color(paint::ink(Rgb(70, 70, 70), false)).inert().pin(
                Some(12.0),
                Some(12.0),
                None,
                None,
            )
        }))
}

#[cfg(test)]
mod tests;
