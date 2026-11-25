//! The plan canvas. The drawing's shapes are one element kept while the drawing is the same (`memo`, keyed by the drawing's
//! revision and the canvas's size), so a pointer that only moves over it re-runs nothing but the small layer that lights what is
//! under it. The canvas itself is the element that hears the pointer, the wheel and the drag; the layers are inert, so the
//! events reach it through them. The primary button picks (a click) or drags out a box; the middle button pans. The layers sit in one
//! `world` element that the camera moves through Rune's live offset and scale, so a wheel notch or a pan costs no drawing by the core:
//! the plan on screen is the last drawing, moved, until the camera is still (`App::settle_if_still`, from `ui::poll`).

use std::collections::HashSet;
use std::time::Duration;

use rune::tok::type_;
use rune::{El, Phase, PointerButton, Shape, Tx, canvas, el, memo, text};

use crate::app::{App, CameraMode, PlanMode};
use crate::paint::{self, Palette};
use crate::select::How;

/// Asks for a frame when the plan, moved through the live offset and scale, is due to be drawn exactly (nothing in `Exact`).
fn settle_soon(s: &App, tx: &mut Tx) {
    if let Some(due) = s.settle_in() {
        tx.wake_after(Duration::from_secs_f64(due));
    }
}

fn layer(key: &'static str, shape: Shape) -> El {
    canvas(shape).key(key).inert().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
}

/// What the pointer is on, as a name beside the glyph of the snap: pinned a little right of and below it, as egui's canvas tags it.
fn tag(s: &App, pal: &Palette) -> Option<El> {
    let l = s.landing.as_ref()?;
    let name = l.tag()?;
    let (x, y) = s.camera.view(s.size.0, s.size.1).px(l.at);
    if !(x.is_finite() && y.is_finite()) {
        return None;
    }
    Some(text(name).key("plan-snap-tag").font(type_::LABEL).color(pal.accent).inert().pin(Some(x + 12.0), Some(y + 14.0), None, None))
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
    // What a pick shows follows the camera as it is now, so it sits outside the world the live camera moves. It is there only with a tool
    // in hand: with none, the plan's elements are as they were before, which the measurements in docs/areas/rune-ui.md were made on.
    let snap = s
        .tool
        .is_some()
        .then(|| layer("plan-snap", paint::over(&s.picked, s.landing.as_ref(), &s.camera.view(size.0, size.1), &pal).shape));
    let layers =
        [base, lit("plan-selected", &selected, pal.accent, pal.accent_fill), lit("plan-hovered", &hovered, pal.hover, pal.hover_fill)];
    // In `Live` the layers sit in one element the camera moves; in `Exact` they are the canvas's own children, as they were before the
    // live camera: with the wrapper bound at rest, a wheel notch over the house cost about twice as much on Linux's CPU renderer
    // (docs/areas/rune-ui.md, run 45532642548 against e282db8d6).
    let world = match s.cam {
        CameraMode::Live => vec![
            el().key("plan-world")
                .inert()
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .live_offset(&s.live_offset)
                .live_scale(&s.live_scale)
                .children(layers),
        ],
        CameraMode::Exact => layers.into(),
    };
    el().key("plan")
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .fill(pal.paper)
        .clip_soft(0.0)
        .on_layout(|s: &mut App, r| s.resize(r[2], r[3]))
        .on_pointer_move_local(|s: &mut App, e, tx| {
            s.square = tx.mods().shift;
            s.hover(e.x, e.y);
        })
        .on_hover_change(|s: &mut App, over, _| {
            if !over {
                s.leave();
            }
        })
        .on_wheel(|s: &mut App, e, tx| {
            s.tick(tx.clock());
            s.zoom(e.x, e.y, (e.dy * 0.002).exp());
            settle_soon(s, tx);
            true
        })
        .pointer_buttons([PointerButton::Middle])
        .on_drag(|s: &mut App, d, tx| {
            let (from, to) = ((d.start_x, d.start_y), (d.x, d.y));
            s.tick(tx.clock());
            match (tx.button(), d.phase) {
                (PointerButton::Middle, Phase::Move) => {
                    s.drag(d.dx, d.dy);
                    settle_soon(s, tx);
                }
                (PointerButton::Middle, Phase::Up) => s.settle_view(),
                (_, Phase::Down) => s.grab(),
                (_, Phase::Move) => {
                    s.sweep(from, to);
                    settle_soon(s, tx);
                }
                (_, Phase::Up) if d.cancelled() => s.cancel_press(),
                (_, Phase::Up) => {
                    let m = tx.mods();
                    s.release(from, to, How::of(m.shift, m.cmd));
                }
            }
        })
        .children(world)
        .child(band)
        .children(snap)
        .children(tag(s, &pal))
}
