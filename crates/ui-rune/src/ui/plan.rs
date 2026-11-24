//! The plan canvas. The drawing's shapes are one element kept while the drawing is the same (`memo`, keyed by the drawing's
//! revision and the canvas's size), so a pointer that only moves over it re-runs nothing but the small layer that lights what is
//! under it. The canvas itself is the element that hears the pointer, the wheel and the drag; the layers are inert, so the
//! events reach it through them.

use std::collections::HashSet;

use rune::{El, Phase, Shape, canvas, el, memo};

use crate::app::App;
use crate::paint::{self, Palette};

fn layer(key: &'static str, shape: Shape) -> El {
    canvas(shape).key(key).inert().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
}

pub fn plan(s: &App) -> El {
    let pal = Palette::dark();
    let size = s.size;
    let base = memo("plan-base", (s.revision, size.0.to_bits(), size.1.to_bits()), || {
        layer("plan-base", s.drawing.as_ref().map(|d| paint::base(d, size, true).shape).unwrap_or_default())
    });
    let selected: HashSet<_> = s.selection.iter().copied().collect();
    let hovered: HashSet<_> = s.hovered.filter(|h| !selected.contains(h)).into_iter().collect();
    let lit = |key: &'static str, owners: &HashSet<u64>, color, fill| match &s.drawing {
        Some(d) => layer(key, paint::lit(d, size, owners, color, fill).shape),
        None => layer(key, Shape::new()),
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
        .on_drag(|s: &mut App, d, _| match d.phase {
            Phase::Down => s.grab(),
            Phase::Move => s.drag(d.dx, d.dy),
            Phase::Up => s.release(d.x, d.y, d.dx, d.dy),
        })
        .child(base)
        .child(lit("plan-selected", &selected, pal.accent, pal.accent_fill))
        .child(lit("plan-hovered", &hovered, pal.hover, pal.hover_fill))
}
