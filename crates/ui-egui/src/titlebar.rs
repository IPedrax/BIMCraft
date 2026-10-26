//! The window's own title bar, where it draws one ([`BimApp::own_title_bar`], Windows and Linux, unless Settings > User Interface
//! asks for the system's): the app bar's empty space moves the window and a double-click there maximizes or restores it,
//! Minimize, Maximize and Close sit flush in its top right corner, and thin zones along its edges and corners resize it. Moving
//! and resizing are the system's own (`StartDrag`, `BeginResize`), so Windows' snapping to the screen's edges works as it does
//! with the system's bar, and Close asks as the system's Close does, so unsaved work is still asked about.

use egui::{Color32, CursorIcon, Id, LayerId, Order, PointerButton, Rect, ResizeDirection, Sense, Stroke, StrokeKind, Ui, ViewportCommand};
use egui::{WidgetInfo, WidgetType, pos2, vec2};

use crate::BimApp;
use crate::lang::t;
use crate::style::pal;

/// One caption button, as wide as the system's on Windows.
pub(crate) const BUTTON_WIDTH: f32 = 46.0;
/// How far in from the window's edge a press resizes it: Windows' own resize border at 100 % (8 px), in points, so it scales as
/// the system's does.
const EDGE: f32 = 8.0;
/// Close's fill under the pointer, as the system's.
const CLOSE_HOVER: Color32 = Color32::from_rgb(196, 43, 28);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Caption {
    Minimize,
    Maximize,
    Close,
}

/// Left to right.
const CAPTIONS: [Caption; 3] = [Caption::Minimize, Caption::Maximize, Caption::Close];

fn maximized(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().maximized.unwrap_or(false))
}

impl BimApp {
    /// Whether the window draws its own title bar now.
    pub(crate) fn own_bar(&self) -> bool {
        self.own_title_bar && !self.system_title_bar
    }

    /// Settings > User Interface's "Use the system's title bar": the system's bar comes back, or goes, at once.
    pub(crate) fn use_system_title_bar(&mut self, ctx: &egui::Context, on: bool) {
        self.system_title_bar = on;
        if self.own_title_bar {
            ctx.send_viewport_cmd(ViewportCommand::Decorations(on));
        }
    }
}

/// The bar's ground, `rect`: a press and drag on it moves the window, a double-click maximizes or restores it. Laid out before
/// what is on the bar, so its buttons keep their presses.
pub(crate) fn drag_area(ui: &Ui, rect: Rect) {
    let ctx = ui.ctx();
    let r = ui.interact(rect, Id::new("title-bar"), Sense::click_and_drag());
    // egui hands a drag that starts on a button to the ground under it: remember whether the press landed on bare ground.
    if ctx.input(|i| i.pointer.any_pressed()) {
        let bare = ctx.viewport(|v| v.hits.click.is_some_and(|w| w.id == r.id));
        ctx.data_mut(|d| d.insert_temp(r.id, bare));
    }
    if r.double_clicked() {
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized(ctx)));
    } else if r.drag_started_by(PointerButton::Primary) && ctx.data(|d| d.get_temp(r.id).unwrap_or(false)) {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
}

/// Minimize, Maximize (Restore Down when maximized) and Close at the right end of `bar`, its full height.
pub(crate) fn caption_buttons(ui: &mut Ui, bar: Rect) {
    let ctx = ui.ctx().clone();
    let max = maximized(&ctx);
    for (i, c) in CAPTIONS.into_iter().enumerate() {
        let rect = Rect::from_min_size(pos2(bar.right() - (3 - i) as f32 * BUTTON_WIDTH, bar.top()), vec2(BUTTON_WIDTH, bar.height()));
        let label = t(match c {
            Caption::Minimize => "Minimize",
            Caption::Maximize if max => "Restore Down",
            Caption::Maximize => "Maximize",
            Caption::Close => "Close",
        });
        let r = ui.interact(rect, Id::new(("caption", c as u8)), Sense::click());
        r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
        let hot = r.hovered() || r.has_focus();
        let (fill, ink) = match (c, hot) {
            (Caption::Close, true) => (CLOSE_HOVER, Color32::WHITE),
            (_, true) => (pal().app_bar_hover, pal().app_bar_text),
            _ => (Color32::TRANSPARENT, pal().app_bar_text),
        };
        let p = ui.painter();
        p.rect_filled(rect, 0.0, fill);
        glyph(p, c, max, rect.center(), Stroke::new(1.0, ink));
        if r.on_hover_text(label).clicked() {
            ctx.send_viewport_cmd(match c {
                Caption::Minimize => ViewportCommand::Minimized(true),
                Caption::Maximize => ViewportCommand::Maximized(!max),
                Caption::Close => ViewportCommand::Close,
            });
        }
    }
}

/// A caption's sign, 10 points across round `at`, drawn as the system draws its own.
fn glyph(p: &egui::Painter, c: Caption, max: bool, at: egui::Pos2, s: Stroke) {
    let half = 5.0;
    match c {
        Caption::Minimize => {
            p.line_segment([at + vec2(-half, 0.5), at + vec2(half, 0.5)], s);
        }
        Caption::Maximize if max => {
            // Restore Down: a window in front of another.
            let front = Rect::from_min_size(at + vec2(-half, -half + 2.0), vec2(8.0, 8.0));
            p.rect_stroke(front, 1.0, s, StrokeKind::Inside);
            p.line_segment([at + vec2(-half + 2.0, -half + 2.0), at + vec2(-half + 2.0, -half)], s);
            p.line_segment([at + vec2(-half + 2.0, -half), at + vec2(half, -half)], s);
            p.line_segment([at + vec2(half, -half), at + vec2(half, half - 2.0)], s);
            p.line_segment([at + vec2(half, half - 2.0), at + vec2(half - 2.0, half - 2.0)], s);
        }
        Caption::Maximize => {
            p.rect_stroke(Rect::from_center_size(at, vec2(2.0 * half, 2.0 * half)), 1.0, s, StrokeKind::Inside);
        }
        Caption::Close => {
            p.line_segment([at + vec2(-half, -half), at + vec2(half, half)], s);
            p.line_segment([at + vec2(-half, half), at + vec2(half, -half)], s);
        }
    }
}

/// The zones along a window `w` that resize it, `edge` deep: each with the way it resizes and its cursor. The corners come last
/// and are twice as long, so they win where they meet the edges and are easy to hit.
fn zones(w: Rect, edge: f32) -> [(Rect, ResizeDirection, CursorIcon); 8] {
    use ResizeDirection::*;
    let c = 2.0 * edge;
    [
        (Rect::from_min_max(w.left_top(), pos2(w.right(), w.top() + edge)), North, CursorIcon::ResizeVertical),
        (Rect::from_min_max(pos2(w.left(), w.bottom() - edge), w.right_bottom()), South, CursorIcon::ResizeVertical),
        (Rect::from_min_max(w.left_top(), pos2(w.left() + edge, w.bottom())), West, CursorIcon::ResizeHorizontal),
        (Rect::from_min_max(pos2(w.right() - edge, w.top()), w.right_bottom()), East, CursorIcon::ResizeHorizontal),
        (Rect::from_min_size(w.left_top(), vec2(c, c)), NorthWest, CursorIcon::ResizeNwSe),
        (Rect::from_min_size(w.right_top() - vec2(c, 0.0), vec2(c, c)), NorthEast, CursorIcon::ResizeNeSw),
        (Rect::from_min_size(w.left_bottom() - vec2(0.0, c), vec2(c, c)), SouthWest, CursorIcon::ResizeNeSw),
        (Rect::from_min_size(w.right_bottom() - vec2(c, c), vec2(c, c)), SouthEast, CursorIcon::ResizeNwSe),
    ]
}

/// The window's frame, over everything else: its edges and corners resize it, and a hairline marks its border, as the system's
/// frame would. A maximized or full-screen window has neither.
pub(crate) fn frame(ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    if maximized(&ctx) || ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)) {
        return;
    }
    let w = ctx.content_rect();
    let layer = LayerId::new(Order::Foreground, Id::new("window-frame"));
    let top = ui.new_child(egui::UiBuilder::new().layer_id(layer).max_rect(w));
    top.painter().rect_stroke(w, 0.0, Stroke::new(1.0, pal().border), StrokeKind::Inside);
    for (i, (r, dir, cursor)) in zones(w, EDGE).into_iter().enumerate() {
        let resp = top.interact(r, Id::new(("window-edge", i)), Sense::drag()).on_hover_cursor(cursor);
        if resp.drag_started_by(PointerButton::Primary) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_corners_win_where_they_meet_the_edges() {
        let w = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 700.0));
        let at = |x: f32, y: f32| zones(w, EDGE).into_iter().rev().find(|(r, ..)| r.contains(pos2(x, y))).map(|(_, d, _)| d);
        assert_eq!(at(1.0, 300.0), Some(ResizeDirection::West));
        assert_eq!(at(15.0, 1.0), Some(ResizeDirection::NorthWest), "a corner reaches twice the edge along it");
        assert_eq!(at(17.0, 1.0), Some(ResizeDirection::North));
        assert_eq!(at(999.0, 699.0), Some(ResizeDirection::SouthEast));
        assert_eq!(at(500.0, 350.0), None);
    }
}
