//! Shared dialog frame: a draggable title bar, a corner close button, a scrolling body and a fixed footer.

use egui::{
    Align2, Area, Context, CursorIcon, Id, Label, ModalResponse, Order, Pos2, RichText, ScrollArea, Sense, Stroke, Ui, UiKind, WidgetInfo,
    WidgetType, vec2,
};

use crate::lang::t;
use crate::style::{dialog_row, pal};

const TITLE_HEIGHT: f32 = 30.0;
const FRAME_MARGIN: f32 = 24.0;

pub(crate) struct Modal {
    id: Id,
    title: String,
    width: f32,
    min_height: f32,
    closable: bool,
}

impl Modal {
    pub(crate) fn new(id: Id, title: impl Into<String>) -> Self {
        Self { id, title: title.into(), width: 580.0, min_height: 0.0, closable: true }
    }

    pub(crate) fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub(crate) fn min_height(mut self, height: f32) -> Self {
        self.min_height = height;
        self
    }

    /// An in-progress import or installation cannot be dismissed while it replaces the project or application.
    pub(crate) fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }

    pub(crate) fn show<R>(self, ctx: &Context, content: impl FnOnce(&mut Ui) -> R) -> Response<R> {
        let previous = ctx.memory(|m| m.area_rect(self.id));
        let position_id = self.id.with("position");
        let position = ctx.data(|d| d.get_temp::<Pos2>(position_id));
        let mut area = Area::new(self.id)
            .kind(UiKind::Modal)
            .order(Order::Foreground)
            .sense(Sense::hover())
            .movable(false)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center());
        if let Some(position) = position {
            area = area.current_pos(position);
        }
        let (mut close, mut delta) = (false, egui::Vec2::ZERO);
        let response = egui::Modal::new(self.id).area(area).show(ctx, |ui| {
            ui.set_width(self.width.min((ctx.content_rect().width() - FRAME_MARGIN).max(120.0)));
            // An Area starts with last frame's size. Give scrolling a stable budget on the first frame,
            // so controls do not shift while a click or a combo-box selection is in progress.
            ui.set_max_height((ctx.content_rect().height() - FRAME_MARGIN).max(80.0));
            ui.set_min_height(self.min_height.min((ctx.content_rect().height() - FRAME_MARGIN).max(0.0)));
            let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), TITLE_HEIGHT), Sense::hover());
            ui.painter().rect_filled(bar, 3.0, pal().panel);
            let close_rect = egui::Rect::from_min_max(egui::pos2(bar.right() - TITLE_HEIGHT, bar.top()), bar.right_bottom());
            let title_rect = egui::Rect::from_min_max(bar.left_top(), close_rect.left_bottom());
            let drag = ui.interact(title_rect, self.id.with("title"), Sense::drag()).on_hover_cursor(CursorIcon::Grab);
            if drag.dragged() {
                delta = drag.drag_delta();
            }
            ui.put(title_rect.shrink2(vec2(6.0, 0.0)), Label::new(RichText::new(&self.title).heading()).truncate().selectable(false));
            // The × drawn as two strokes across most of its square, as big as a window's own close button.
            let sense = if self.closable { Sense::click() } else { Sense::hover() };
            let r = ui.interact(close_rect.shrink(3.0), self.id.with("close"), sense);
            let ink = match (self.closable, r.hovered()) {
                (false, _) => pal().muted.gamma_multiply(0.5),
                (true, true) => pal().text,
                (true, false) => pal().muted,
            };
            if self.closable && r.hovered() {
                ui.painter().rect_filled(r.rect, 4.0, pal().hover_fill);
            }
            let (c, arm) = (r.rect.center(), 6.5);
            for (a, b) in [(vec2(-arm, -arm), vec2(arm, arm)), (vec2(-arm, arm), vec2(arm, -arm))] {
                ui.painter().line_segment([c + a, c + b], Stroke::new(1.8, ink));
            }
            let label = t("Close dialog");
            r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, self.closable, label));
            close = r.on_hover_text(t("Close")).clicked();
            ui.add_space(4.0);
            content(ui)
        });
        if previous.is_some_and(|rect| (rect.size() - response.response.rect.size()).length() > 0.5) {
            ctx.request_repaint();
        }
        if delta != egui::Vec2::ZERO {
            ctx.data_mut(|d| d.insert_temp(position_id, response.response.rect.center() + delta));
            ctx.request_repaint();
        }
        Response { response, close, closable: self.closable }
    }
}

pub(crate) struct Response<R> {
    response: ModalResponse<R>,
    close: bool,
    closable: bool,
}

impl<R> Response<R> {
    /// The corner's × or Escape (when this is the dialog on top and no list of it is open). A click beside the dialog, on the
    /// dimmed window behind it, does nothing: a draft is dropped only by asking.
    pub(crate) fn should_close(&self) -> bool {
        let r = &self.response;
        let escape = || r.response.ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        self.closable && (self.close || r.response.should_close() || (r.is_top_modal && !r.any_popup_open && escape()))
    }
}

/// The form scrolls independently of the footer. A dialog's minimum height is filled by its body, so tab changes never
/// leave its action buttons halfway up the window.
pub(crate) fn body<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let footer = dialog_row(ui) + 12.0;
    let max_height = (ui.ctx().content_rect().height() - FRAME_MARGIN - TITLE_HEIGHT - footer - 12.0).max(40.0);
    let min_height = (ui.min_rect().bottom() - ui.cursor().top() - footer).clamp(0.0, max_height);
    ScrollArea::vertical()
        .id_salt("dialog-body")
        .max_height(max_height)
        .min_scrolled_height(0.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.set_min_height(min_height);
            add(ui)
        })
        .inner
}

/// A footer remains outside the body's scroll area, at the bottom of the dialog.
pub(crate) fn footer<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.add_space(4.0);
    let y = ui.cursor().top();
    ui.painter().hline(ui.max_rect().x_range(), y, Stroke::new(1.0, pal().border));
    ui.add_space(8.0);
    let result = add(ui);
    // Dialogs apply their commands after drawing. Paint the resulting validation message on the next frame,
    // even when the pointer stays on a button that was already hovered.
    if ui.input(|i| i.pointer.any_released() || i.key_pressed(egui::Key::Enter)) {
        ui.ctx().request_repaint();
    }
    result
}
