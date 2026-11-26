//! The canvas of a section or an elevation: the core's picture (the image the section's thread made) under the drawing over it (the cut
//! in poché, the levels, grids and notes) as Rune shapes, the two moved together through Rune's live offset and scale while the camera
//! moves and the thread makes the next picture. The primary button picks or drags out a box, the middle one pans, the wheel zooms
//! about the pointer. The kept shapes are one element while the picture is the same (`memo`), so a pointer that only moves over the
//! canvas builds the small hover layer and nothing else.

use std::collections::HashSet;

use rune::tok::type_;
use rune::{El, Phase, PointerButton, Shape, canvas, el, memo, text};

use crate::app::App;
use crate::lang::t;
use crate::paint::{self, Palette};
use crate::select::How;
use bimcraft_core::render::{PAPER, Rgb};

fn layer(key: &'static str, shape: Shape) -> El {
    canvas(shape).key(key).inert().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
}

pub fn view(s: &App) -> El {
    let pal = Palette::dark();
    let cut = &s.cut;
    let frame = cut.frame.as_ref();
    // The drawing is made for the size the view had, which the shapes are cut to.
    let size = frame.map_or(cut.size, |f| (f.view.width as f32, f.view.height as f32));
    let base = memo("section-base", (cut.revision, size.0.to_bits(), size.1.to_bits()), || {
        layer("section-base", frame.map(|f| paint::base(&f.drawing, size, false).shape).unwrap_or_default())
    });
    let picture = el().key("section-picture").inert().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).pixels(&cut.pixels);
    let selected: HashSet<_> = s.selection.iter().copied().collect();
    let hovered: HashSet<_> = s.hovered.filter(|h| !selected.contains(h)).into_iter().collect();
    let lit = |key: &'static str, owners: &HashSet<u64>, color, fill| match frame {
        Some(f) => layer(key, paint::lit(&f.drawing, size, owners, color, fill).shape),
        None => layer(key, Shape::new()),
    };
    let world = el()
        .key("section-world")
        .inert()
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .live_offset(&cut.live_offset)
        .live_scale(&cut.live_scale)
        .children([
            picture,
            base,
            lit("section-selected", &selected, pal.accent, pal.accent_fill),
            lit("section-hovered", &hovered, pal.hover, pal.hover_fill),
        ]);
    let band = match &s.rubber {
        Some(r) => layer("section-rubber", paint::rubber(r, &pal).shape),
        None => layer("section-rubber", Shape::new()),
    };
    // What the picture is waiting for, or why it could not be made.
    let note = match (&cut.error, frame) {
        (Some(why), _) => Some(why.clone()),
        (None, None) => Some(t("Making the view...").to_string()),
        _ => None,
    };
    el().key("section")
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .fill(paint::ink(PAPER, false))
        .clip_soft(0.0)
        .on_layout(|s: &mut App, r| s.section_resize(r[2], r[3]))
        .on_pointer_move_local(|s: &mut App, e, _| s.section_hover(e.x, e.y))
        .on_hover_change(|s: &mut App, over, _| {
            if !over {
                s.section_leave();
            }
        })
        .on_wheel(|s: &mut App, e, _| {
            s.section_zoom(e.x, e.y, (e.dy * 0.002).exp());
            true
        })
        .pointer_buttons([PointerButton::Middle])
        .on_drag(|s: &mut App, d, tx| {
            let (from, to) = ((d.start_x, d.start_y), (d.x, d.y));
            match (tx.button(), d.phase) {
                (PointerButton::Middle, Phase::Move) => s.section_pan(d.dx, d.dy),
                (PointerButton::Middle, Phase::Up) => {}
                (_, Phase::Down) => s.section_grab(),
                (_, Phase::Move) => s.section_sweep(from, to),
                (_, Phase::Up) if d.cancelled() => s.cancel_press(),
                (_, Phase::Up) => {
                    let m = tx.mods();
                    s.section_release(from, to, How::of(m.shift, m.cmd));
                }
            }
        })
        .child(world)
        .child(band)
        .children(note.map(|n| {
            text(n).key("section-note").font(type_::BODY).color(paint::ink(Rgb(70, 70, 70), false)).inert().pin(
                Some(12.0),
                Some(12.0),
                None,
                None,
            )
        }))
}

#[cfg(test)]
mod tests;
