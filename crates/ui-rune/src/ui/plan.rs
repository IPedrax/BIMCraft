//! The plan canvas. The drawing's shapes are one element kept while the drawing is the same (`memo`, keyed by the drawing's
//! revision and the canvas's size), so a pointer that only moves over it re-runs nothing but the small layer that lights what is
//! under it. The canvas itself is the element that hears the pointer, the wheel and the drag; the layers are inert, so the
//! events reach it through them. The primary button picks (a click) or drags out a box; the middle button pans.

use std::collections::HashSet;

use rune::{El, Phase, PointerButton, Shape, canvas, el, memo};

use crate::app::{App, PlanMode};
use crate::paint::{self, Palette};
use crate::select::How;

fn layer(key: &'static str, shape: Shape) -> El {
    canvas(shape).key(key).inert().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
}

pub fn plan(s: &App) -> El {
    let pal = Palette::dark();
    let size = s.size;
    let base = match s.mode {
        PlanMode::Shapes => memo("plan-base", (s.revision, size.0.to_bits(), size.1.to_bits()), || {
            layer("plan-base", s.drawing.as_ref().map(|d| paint::base(d, size, true).shape).unwrap_or_default())
        }),
        // The image is replaced by `App::redraw` and shows without the view running again.
        PlanMode::Pixels => el().key("plan-base").inert().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).pixels(&s.pixels),
    };
    let selected: HashSet<_> = s.selection.iter().copied().collect();
    let hovered: HashSet<_> = s.hovered.filter(|h| !selected.contains(h)).into_iter().collect();
    let lit = |key: &'static str, owners: &HashSet<u64>, color, fill| match &s.drawing {
        Some(d) => layer(key, paint::lit(d, size, owners, color, fill).shape),
        None => layer(key, Shape::new()),
    };
    let band = match &s.rubber {
        Some(r) => layer("plan-rubber", paint::rubber(r, &pal).shape),
        None => layer("plan-rubber", Shape::new()),
    };
    el().key("plan")
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .fill(pal.paper)
        .clip_soft(0.0)
        .on_layout(|s: &mut App, r| s.resize(r[2], r[3]))
        .on_pointer_move_local(|s: &mut App, e, _| s.hover(e.x, e.y))
        .on_hover_change(|s: &mut App, over, _| {
            if !over {
                s.leave();
            }
        })
        .on_wheel(|s: &mut App, e, _| {
            s.zoom(e.x, e.y, (e.dy * 0.002).exp());
            true
        })
        .pointer_buttons([PointerButton::Middle])
        .on_drag(|s: &mut App, d, tx| {
            let (from, to) = ((d.start_x, d.start_y), (d.x, d.y));
            match (tx.button(), d.phase) {
                (PointerButton::Middle, Phase::Move) => s.drag(d.dx, d.dy),
                (PointerButton::Middle, Phase::Up) => {}
                (_, Phase::Down) => s.grab(),
                (_, Phase::Move) => s.sweep(from, to),
                (_, Phase::Up) if d.cancelled() => s.cancel_press(),
                (_, Phase::Up) => {
                    let m = tx.mods();
                    s.release(from, to, How::of(m.shift, m.cmd));
                }
            }
        })
        .child(base)
        .child(lit("plan-selected", &selected, pal.accent, pal.accent_fill))
        .child(lit("plan-hovered", &hovered, pal.hover, pal.hover_fill))
        .child(band)
}
