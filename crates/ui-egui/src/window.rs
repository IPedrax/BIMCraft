//! The window's size (issue 37): kept in the settings once it has held still, brought back when the app starts, and brought
//! within the screen when it doesn't fit (the default size on a small one, or a kept one after the screen changed). Only the
//! size is kept, never the place: Wayland tells an app none, and a window starts in the middle of its screen.

use std::time::Duration;

use egui::{Context, Vec2, ViewportCommand, vec2};

use crate::BimApp;
use crate::settings::Window;

/// The least a window can be, in points.
pub const MIN_SIZE: [f32; 2] = [900.0, 560.0];

/// What a window may take of its screen, which it shares with its own title bar and the system's bars.
const SHARE: Vec2 = vec2(0.96, 0.9);

/// How long the window must hold still before its size is kept: a drag on its edge is a frame for each pixel.
const STILL: f64 = 0.5;

/// `size` made to fit `monitor`, and not below the least a window can be, or none when it fits already.
pub(crate) fn fitted(size: Vec2, monitor: Vec2) -> Option<Vec2> {
    let room = (monitor * SHARE).max(vec2(MIN_SIZE[0], MIN_SIZE[1]));
    let to = size.min(room);
    (to != size).then_some(to)
}

/// What the app tracks of its window between frames.
#[derive(Default)]
pub(crate) struct Tracker {
    /// How the window was last kept: what the settings file holds.
    pub(crate) kept: Window,
    /// How it is now and since when (egui's time), until it has held still long enough to be kept.
    seen: Option<(Window, f64)>,
    /// How it was in the first frame, until it is kept once: a window left as it started is not written down.
    start: Option<Window>,
    /// Whether the screen has been looked at yet: its size is not known in the first frame.
    looked: bool,
    /// The size asked of a window too big for its screen, and the frames waited for it: once it is that size it is centred.
    fitting: Option<(Vec2, u32)>,
}

impl BimApp {
    /// Once a frame: a window too big for its screen is made smaller (once) and centred, and how the window is left is noted for
    /// the settings.
    pub(crate) fn watch_window(&mut self, ctx: &Context) {
        let (monitor, maximized, away, now) = ctx.input(|i| {
            let v = i.viewport();
            (v.monitor_size, v.maximized == Some(true), v.minimized == Some(true) || v.fullscreen == Some(true), i.time)
        });
        let size = ctx.content_rect().size();
        let w = &mut self.window;
        if !w.looked
            && let Some(monitor) = monitor
        {
            w.looked = true;
            if !maximized && let Some(to) = fitted(size, monitor) {
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(to));
                w.fitting = Some((to, 0));
            }
        }
        if let Some((to, waited)) = w.fitting {
            if (size - to).abs().max_elem() < 2.0 || waited > 60 {
                w.fitting = None;
                if let Some(center) = ViewportCommand::center_on_screen(ctx) {
                    ctx.send_viewport_cmd(center);
                }
            } else {
                w.fitting = Some((to, waited + 1));
                ctx.request_repaint();
            }
        }
        if away {
            return;
        }
        // Maximized, the size it was is kept, so that it comes back to it.
        let live = Window { size: if maximized { w.kept.size } else { Some([size.x.round(), size.y.round()]) }, maximized };
        w.start.get_or_insert(live);
        if w.seen.is_none_or(|(seen, _)| seen != live) {
            w.seen = Some((live, now));
        }
        if let Some((seen, since)) = w.seen
            && seen != w.kept
            && w.start != Some(seen)
        {
            if now - since >= STILL {
                (w.kept, w.start) = (seen, None);
            } else {
                ctx.request_repaint_after(Duration::from_secs_f64(STILL));
            }
        }
    }
}
