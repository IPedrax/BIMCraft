//! Copy, cut and paste (Ctrl+C, Ctrl+X, Ctrl+V) over a view: the project's own clipboard, which keeps the elements as they were
//! when they were taken and brings them back as new ones. The system's clipboard stays the text fields', the note editor's and
//! the views' own: `keys` hands them the frame before this. When the copy was made on a plan other than the one in view, a panel
//! asks where the paste goes: where it was on X and Y, or wherever a click puts it.

use egui::{Align2, Area, Context, Frame, Id as UiId, Key, Order, RichText, Stroke, vec2};

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Guid, Id, Model};
use bimcraft_core::render::{self, View};

use crate::lang::{t, tf};
use crate::style::pal;
use crate::{BimApp, Status, Tool};

/// Ctrl+V on a plan other than the one the copy was made on: where the paste goes.
pub(crate) struct Ask {
    /// The plan being pasted into.
    pub(crate) level: Guid,
    /// The plan the copy was made in.
    pub(crate) copied: Guid,
}

/// The clipboard after "No": its elements follow the cursor, and a click puts them where it is.
pub(crate) struct Drop {
    /// The middle of what was copied, which the click puts under the cursor.
    pub(crate) anchor: P2,
    /// The plan a click pastes them on; none when the view isn't a plan.
    pub(crate) level: Option<Guid>,
}

impl BimApp {
    /// Ctrl+C, or Ctrl+X: the selection taken for Paste, cut taking it out of the project as well.
    pub(crate) fn take_selection(&mut self, cut: bool) {
        if self.selection.is_empty() {
            self.status = Status { text: t("Select what to copy first").into(), error: true };
            return;
        }
        let level = self.plan_level().map(|id| self.session.model.guid(id));
        let ids = self.selection.clone();
        let mut params = serde_json::json!({"ids": ids});
        if let Some(level) = level {
            params["level"] = serde_json::json!(level);
        }
        match self.exec(if cut { "clipboard_cut" } else { "clipboard_copy" }, &params) {
            Ok(_) => {
                let taken = ids.len();
                if cut {
                    self.selection.clear();
                }
                self.status = Status {
                    text: tf(if cut { "{} cut: Ctrl+V pastes them" } else { "{} copied: Ctrl+V pastes them" }, &[&taken.to_string()]),
                    error: false,
                };
            }
            Err(e) => self.status = Status { text: e, error: true },
        }
    }

    /// Ctrl+V: the clipboard where it was, or the question first when it was taken on another plan.
    pub(crate) fn paste_clipboard(&mut self) {
        let Some(clip) = self.session.clipboard.as_ref() else {
            self.status = Status { text: t("Nothing was copied: Ctrl+C copies what is selected").into(), error: true };
            return;
        };
        let level = self.plan_level().map(|id| self.session.model.guid(id));
        if let (Some(copied), Some(level)) = (clip.level, level)
            && copied != level
        {
            self.paste_ask = Some(Ask { level, copied });
            return;
        }
        self.paste_here(level, P2(0.0, 0.0));
    }

    /// The paste itself: the clipboard on `level`, moved `by` in plan; its elements are selected afterwards.
    pub(crate) fn paste_here(&mut self, level: Option<Guid>, by: P2) {
        let mut params = serde_json::json!({"by": [by.0, by.1]});
        if let Some(level) = level {
            params["level"] = serde_json::json!(level);
        }
        match self.exec("clipboard_paste", &params) {
            Ok(made) => {
                self.selection =
                    made["ids"].as_array().map(|a| a.iter().filter_map(serde_json::Value::as_u64).collect()).unwrap_or_default();
                self.paste_drop = None;
                self.set_tool(Tool::Modify);
                let pasted = self.selection.len();
                self.status = Status { text: tf("{} pasted", &[&pasted.to_string()]), error: false };
            }
            Err(e) => {
                self.paste_drop = None;
                self.set_tool(Tool::Modify);
                self.status = Status { text: e, error: true };
            }
        }
    }

    /// Ctrl+V answered "No": the clipboard follows the cursor, and a click pastes it where it is.
    pub(crate) fn paste_follow(&mut self) {
        let Some(clip) = self.session.clipboard.as_ref() else { return };
        let anchor = middle(&clip.model, &clip.ids).unwrap_or(P2(0.0, 0.0));
        let level = self.plan_level().map(|id| self.session.model.guid(id));
        self.paste_drop = Some(Drop { anchor, level });
        self.set_tool(Tool::Paste);
    }

    /// A click while the paste follows the cursor: the clipboard put down where it is, its middle at `at`.
    pub(crate) fn paste_drop_here(&mut self, at: P2) {
        let Some(drop) = self.paste_drop.take() else {
            self.set_tool(Tool::Modify);
            return;
        };
        self.paste_here(drop.level, at - drop.anchor);
    }

    /// The panel a Ctrl+V on another plan waits on: where the paste goes. Escape lets the paste go.
    pub(crate) fn paste_question(&mut self, ctx: &Context) {
        let Some(ask) = self.paste_ask.as_ref() else { return };
        let (level, copied) = (ask.level, ask.copied);
        let name = |g: Guid| self.session.model.level(g).map(|l| l.name.clone()).unwrap_or_default();
        let (here, there) = (name(level), name(copied));
        let mut answer = None;
        let at = self.view_rect.center_bottom() - vec2(0.0, 18.0);
        Area::new(UiId::new("paste-question")).order(Order::Middle).fixed_pos(at).pivot(Align2::CENTER_BOTTOM).show(ctx, |ui| {
            Frame::popup(ui.style()).fill(pal().surface).stroke(Stroke::new(1.0, pal().accent)).show(ui, |ui| {
                ui.set_max_width(440.0);
                ui.label(RichText::new(t("Paste at the same place on the X and Y axes?")).strong().color(pal().accent_ink));
                ui.label(RichText::new(tf("Copied on {}, pasted on {}.", &[&there, &here])).color(pal().muted));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(t("Yes")).clicked() {
                        answer = Some(true);
                    }
                    if ui.button(t("No, click where it goes")).clicked() {
                        answer = Some(false);
                    }
                });
            });
        });
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.paste_ask = None;
            return;
        }
        match answer {
            Some(true) => {
                self.paste_ask = None;
                self.paste_here(Some(level), P2(0.0, 0.0));
            }
            Some(false) => {
                self.paste_ask = None;
                self.paste_follow();
            }
            None => {}
        }
    }
}

/// The middle of the elements `ids` as a plan draws them, mm: the point a click puts under the cursor when the paste follows it.
fn middle(m: &Model, ids: &[Id]) -> Option<P2> {
    let view = View { scale: 1.0, cx: 0.0, cy: 0.0, width: 0, height: 0, angle: 0.0 };
    let (mut min, mut max) = ((f32::INFINITY, f32::INFINITY), (f32::NEG_INFINITY, f32::NEG_INFINITY));
    for &id in ids {
        let Some(e) = m.elements.get(id) else { continue };
        for prim in render::sketch(m, view, id, e).prims {
            for (x, y) in prim.points() {
                min = (min.0.min(x), min.1.min(y));
                max = (max.0.max(x), max.1.max(y));
            }
        }
    }
    (min.0.is_finite() && max.0.is_finite()).then(|| view.mm(((min.0 + max.0) / 2.0, (min.1 + max.1) / 2.0)))
}
