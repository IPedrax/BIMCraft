//! Modify > Scale (RE): the selection's annotation, CAD drawings, images and PDFs made bigger or smaller, as Revit's Scale.
//! A popup asks how, by a percentage or by a length between two points; then the clicks: the point that stays, or two
//! points on what's selected, a line drawn between them, and a field for the length they come to. The command is the
//! core's `scale_elements`, in plans and ceiling plans and in sections and elevations; what keeps its size it leaves out,
//! said in the status bar.

use bimcraft_core::geom::P2;
use bimcraft_i18n::msg;
use egui::{Align, Align2, Context, Key, Layout, Order, Pos2, RichText, TextEdit, vec2};
use serde_json::json;

use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::{BimApp, Sheet, Status, Tool};

/// Where Scale is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    /// The popup asks how to scale.
    Choose,
    /// By `factor`: the next click is the point that stays.
    Base(f64),
    /// By length: the first point, which stays, is clicked next.
    First,
    /// By length: the first point clicked; the second is next, a line following the cursor from it.
    Second(P2),
    /// Both points clicked: the popup asks for the length between them, and the second goes to it.
    Length(P2, P2),
}

/// What the popups hold: kept from one Scale to the next, as Revit keeps its options.
#[derive(Default)]
pub(crate) struct Options {
    /// By a length between two points, not a percentage.
    pub by_length: bool,
    /// The percentage typed, 200 for twice the size.
    pub percent: String,
    /// The length typed for the two points, in the project's units.
    pub length: String,
    /// What's wrong with what was typed, shown under it.
    pub error: Option<String>,
    /// Where the length's popup stands, the pointer's place when the second point was clicked; and whether its field has
    /// had the keys yet.
    pub at: Option<Pos2>,
    pub focused: bool,
}

/// The percentage `text` reads as, a factor: "50", "50 %" or "12,5" (a comma as the point). None for anything else.
pub(crate) fn percentage(text: &str) -> Option<f64> {
    let n = text.trim().trim_end_matches('%').trim().replace(',', ".").parse::<f64>().ok()?;
    Some(n / 100.0).filter(|k| k.is_finite())
}

/// Why factor `k` can't be taken, or None when it can.
fn refused(k: f64) -> Option<String> {
    if !(k.is_finite() && k > 0.0) {
        return Some(t(msg!("Enter a size above 0.")).into());
    }
    let (lo, hi) = (*bimcraft_core::scale::FACTORS.start(), *bimcraft_core::scale::FACTORS.end());
    (!(lo..=hi).contains(&k)).then(|| tf(msg!("Scale makes things from {} to {} times their size."), &[&lo.to_string(), &hi.to_string()]))
}

impl BimApp {
    /// Why Scale can't start here with this selection, said as the status says it; None when it can.
    pub(crate) fn scale_refused(&self) -> Option<String> {
        let m = &self.session.model;
        if !matches!(self.sheet, Sheet::Plan(_) | Sheet::Ceiling(_) | Sheet::Section(_)) {
            return Some(
                t(msg!("Scale works in plans, sections and elevations: what it scales, annotation, CAD drawings, images and PDFs, is drawn there"))
                    .into(),
            );
        }
        if self.selection.is_empty() {
            return Some(t(msg!("select the annotation, CAD drawing, image or PDF to scale first")).into());
        }
        let scalable = self.selection.iter().filter_map(|id| m.elements.get(*id)).any(bimcraft_core::scale::scalable);
        (!scalable).then(|| {
            t(msg!("Scale takes detail lines, filled regions, dimensions, CAD drawings, images and PDFs: text, families and model elements keep their size"))
                .into()
        })
    }

    /// The popups Scale shows: how to scale, while it asks; the length between the two points, once they're clicked.
    pub(crate) fn scale_window(&mut self, ctx: &Context) {
        match self.tool {
            Tool::Scale(Step::Choose) => self.scale_choose(ctx),
            Tool::Scale(Step::Length(a, b)) => self.scale_length(ctx, a, b),
            _ => {}
        }
    }

    fn scale_choose(&mut self, ctx: &Context) {
        let (mut ok, mut cancel) = (false, false);
        let o = &mut self.scaling;
        let r = Modal::new(egui::Id::new("scale-how"), t("Scale")).width(380.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.radio_value(&mut o.by_length, false, t(msg!("By percentage")));
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    let l = ui.label(t(msg!("Percentage:")));
                    let field = ui
                        .add_enabled(
                            !o.by_length,
                            TextEdit::singleline(&mut o.percent).id(egui::Id::new("scale-percent")).desired_width(72.0).hint_text("100"),
                        )
                        .labelled_by(l.id);
                    ui.label("%");
                    if !o.focused && !o.by_length {
                        field.request_focus();
                        o.focused = true;
                    }
                    if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        ok = true;
                    }
                });
                ui.add_space(4.0);
                ui.radio_value(&mut o.by_length, true, t(msg!("By length between two points")));
                ui.indent("scale-length-hint", |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(t(msg!(
                                "Click the point that stays, then a second point; type the length between them, and the second goes to it."
                            )))
                            .weak(),
                        )
                        .wrap(),
                    );
                });
                if let Some(e) = &o.error {
                    ui.add_space(4.0);
                    ui.colored_label(crate::style::pal().error, e);
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button(t("Cancel")).clicked();
                    ok |= ui.button(t("OK")).clicked();
                });
            });
        });
        if cancel || r.should_close() {
            self.scaling.error = None;
            self.tool = Tool::Modify;
            return;
        }
        if !ok {
            return;
        }
        if self.scaling.by_length {
            self.scaling.error = None;
            self.tool = Tool::Scale(Step::First);
            return;
        }
        match percentage(&self.scaling.percent) {
            None => self.scaling.error = Some(tf(msg!("`{}` is not a percentage"), &[self.scaling.percent.trim()])),
            Some(k) => match refused(k) {
                Some(why) => self.scaling.error = Some(why),
                None => {
                    self.scaling.error = None;
                    self.tool = Tool::Scale(Step::Base(k));
                }
            },
        }
    }

    /// The length the two points come to, typed; the second point goes to it.
    fn scale_length(&mut self, ctx: &Context, a: P2, b: P2) {
        let now = a.dist(b);
        let at = *self.scaling.at.get_or_insert_with(|| ctx.pointer_latest_pos().unwrap_or(ctx.content_rect().center()) + vec2(16.0, 16.0));
        let (mut ok, mut cancel) = (false, false);
        let shown = self.session.model.length(now);
        let o = &mut self.scaling;
        egui::Area::new(egui::Id::new("scale-length-popup")).order(Order::Foreground).fixed_pos(at).pivot(Align2::LEFT_TOP).show(
            ctx,
            |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(260.0);
                    ui.horizontal(|ui| {
                        let l = ui.label(t(msg!("New length:")));
                        let field = ui
                            .add(TextEdit::singleline(&mut o.length).id(egui::Id::new("scale-length")).desired_width(96.0))
                            .labelled_by(l.id);
                        if !o.focused {
                            field.request_focus();
                            o.focused = true;
                        }
                        if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            ok = true;
                        }
                    });
                    ui.label(RichText::new(tf(msg!("Now {}"), &[&shown])).weak());
                    if let Some(e) = &o.error {
                        ui.colored_label(crate::style::pal().error, e);
                    }
                    ui.horizontal(|ui| {
                        ok |= ui.button(t("OK")).clicked();
                        cancel = ui.button(t("Cancel")).clicked();
                    });
                });
            },
        );
        if cancel || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.scaling.error = None;
            self.tool = Tool::Modify;
            return;
        }
        if !ok {
            return;
        }
        let typed = self.scaling.length.clone();
        let k = match self.session.model.units.parse(&typed) {
            Some(len) if len.is_finite() && len > 0.0 => len / now,
            Some(_) => {
                self.scaling.error = Some(t(msg!("Enter a length above 0.")).into());
                return;
            }
            None => {
                self.scaling.error = Some(tf("`{}` is not a length", &[typed.trim()]));
                return;
            }
        };
        if let Some(why) = refused(k) {
            self.scaling.error = Some(why);
            return;
        }
        self.scaling.error = None;
        self.apply_scale(a, k);
    }

    /// The factor the length typed makes of the two points, while Scale asks for it: the preview's.
    pub(crate) fn scale_typed(&self) -> Option<(P2, f64)> {
        let Tool::Scale(Step::Length(a, b)) = self.tool else { return None };
        let len = self.session.model.units.parse(&self.scaling.length).filter(|l| l.is_finite() && *l > 0.0)?;
        let k = len / a.dist(b);
        refused(k).is_none().then_some((a, k))
    }

    /// A point picked while Scale is on, snapped, in the view's terms: the one that stays, or one of the two.
    pub(crate) fn scale_point(&mut self, at: P2) {
        match self.tool {
            Tool::Scale(Step::Base(k)) => self.apply_scale(at, k),
            Tool::Scale(Step::First) => self.tool = Tool::Scale(Step::Second(at)),
            Tool::Scale(Step::Second(a)) if a.dist(at) <= bimcraft_core::model::TOL => {
                self.status = Status { text: t(msg!("Click a second point away from the first")).into(), error: true };
            }
            Tool::Scale(Step::Second(a)) => {
                self.scaling.length = self.session.model.field(a.dist(at));
                (self.scaling.at, self.scaling.focused, self.scaling.error) = (None, false, None);
                self.tool = Tool::Scale(Step::Length(a, at));
            }
            _ => {}
        }
    }

    /// The selection `k` times its size about `about`: back to Modify, still selected; what kept its size said.
    fn apply_scale(&mut self, about: P2, k: f64) {
        let Some(r) = self.run("scale_elements", json!({"ids": self.selection, "about": [about.0, about.1], "factor": k})) else {
            // Refused: the status says why, and Scale stays where it was, to try another point or size.
            return;
        };
        self.tool = Tool::Modify;
        let left: Vec<&str> = r["left_out"].as_array().into_iter().flatten().filter_map(|l| l["why"].as_str()).collect();
        if let Some(why) = left.first() {
            let m = &self.session.model;
            let names: Vec<String> = r["left_out"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|l| l["id"].as_u64())
                .take(3)
                .map(|id| {
                    m.elements.get(id).map_or_else(|| id.to_string(), |e| format!("{} {id}", crate::lang::category(e.category(), false)))
                })
                .collect();
            let more = if left.len() > names.len() { format!(" (+{})", left.len() - names.len()) } else { String::new() };
            self.status = Status {
                text: tf(
                    msg!("Scaled {}; left as they were: {}{}: {}"),
                    &[&r["scaled"].as_array().map_or(0, Vec::len).to_string(), &names.join(", "), &more, why],
                ),
                error: true,
            };
        }
    }

    /// What of the selection Scale takes: what its preview draws.
    pub(crate) fn scaled_selection(&self) -> Vec<bimcraft_core::model::Id> {
        let m = &self.session.model;
        self.selection.iter().copied().filter(|id| m.elements.get(*id).is_some_and(bimcraft_core::scale::scalable)).collect()
    }
}
