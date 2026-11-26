//! The canvas of a sheet: the core's drawing of the paper (title block, views at their scales, schedules, notes) painted once for the
//! zoom by the sheet's thread, shown as one image on the desk, and moved through Rune's live offset and scale while the next is
//! painted. What is selected or hovered is lit by a frame round its box on the paper. The primary button picks or drags out a box, the
//! middle one pans, the wheel zooms about the pointer.

use std::collections::HashSet;

use rune::tok::type_;
use rune::{Color, El, LineCap, Phase, PointerButton, Shape, canvas, el, text};

use crate::app::App;
use crate::lang::t;
use crate::paint::{self, Palette};
use crate::select::How;
use bimcraft_core::render::Rgb;

fn layer(key: &'static str, shape: Shape) -> El {
    canvas(shape).key(key).inert().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
}

/// A frame round each of `boxes`, in `color`, with a tint inside.
fn frames(boxes: &[[f32; 4]], color: Color, fill: Color) -> Shape {
    boxes.iter().fold(Shape::new(), |shape, [l, t, r, b]| {
        let ring = vec![[*l, *t], [*r, *t], [*r, *b], [*l, *b]];
        shape.fill(ring.clone(), fill).stroke_with(ring, true, 2.0, LineCap::Butt, color)
    })
}

pub fn view(s: &App) -> El {
    let pal = Palette::dark();
    let p = &s.paper;
    let frame = p.frame.as_ref();
    let picture = match frame.and_then(|f| f.at) {
        Some([x, y, w, h]) => el().key("sheet-picture").inert().pin(Some(x), Some(y), None, None).w(w).h(h).pixels(&p.pixels),
        None => el().key("sheet-picture").inert(),
    };
    let selected: HashSet<_> = s.selection.iter().copied().collect();
    let selected_ids: Vec<u64> = s.selection.clone();
    let hovered_ids: Vec<u64> = s.hovered.filter(|h| !selected.contains(h)).into_iter().collect();
    let world = el()
        .key("sheet-world")
        .inert()
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .live_offset(&p.live_offset)
        .live_scale(&p.live_scale)
        .children([
            picture,
            layer("sheet-selected", frames(&p.boxes(&selected_ids), pal.accent, pal.accent_fill)),
            layer("sheet-hovered", frames(&p.boxes(&hovered_ids), pal.hover, pal.hover_fill)),
        ]);
    let band = match &s.rubber {
        Some(r) => layer("sheet-rubber", paint::rubber(r, &pal).shape),
        None => layer("sheet-rubber", Shape::new()),
    };
    let note = match (&p.error, frame) {
        (Some(why), _) => Some(why.clone()),
        (None, None) => Some(t("Making the view...").to_string()),
        _ => None,
    };
    el().key("sheet")
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .fill(pal.paper)
        .clip_soft(0.0)
        .on_layout(|s: &mut App, r| s.paper_resize(r[2], r[3]))
        .on_pointer_move_local(|s: &mut App, e, _| s.paper_hover(e.x, e.y))
        .on_hover_change(|s: &mut App, over, _| {
            if !over {
                s.paper_leave();
            }
        })
        .on_wheel(|s: &mut App, e, _| {
            s.paper_zoom(e.x, e.y, (e.dy * 0.002).exp());
            true
        })
        .pointer_buttons([PointerButton::Middle])
        .on_drag(|s: &mut App, d, tx| {
            let (from, to) = ((d.start_x, d.start_y), (d.x, d.y));
            match (tx.button(), d.phase) {
                (PointerButton::Middle, Phase::Move) => s.paper_pan(d.dx, d.dy),
                (PointerButton::Middle, Phase::Up) => {}
                (_, Phase::Down) => s.paper_grab(),
                (_, Phase::Move) => s.paper_sweep(from, to),
                (_, Phase::Up) if d.cancelled() => s.cancel_press(),
                (_, Phase::Up) => {
                    let m = tx.mods();
                    s.paper_release(from, to, How::of(m.shift, m.cmd));
                }
            }
        })
        .child(world)
        .child(band)
        .children(note.map(|n| {
            text(n).key("sheet-note").font(type_::BODY).color(paint::ink(Rgb(200, 200, 200), false)).inert().pin(
                Some(12.0),
                Some(12.0),
                None,
                None,
            )
        }))
}

#[cfg(test)]
mod tests;
