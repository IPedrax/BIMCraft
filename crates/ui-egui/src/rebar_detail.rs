//! Structure > Reinforcement > Detail (DD): the Reinforcement Detail dialog over the `rebar_detail` command, and a detail's own tab. A
//! detail is a view like a panel schedule: it opens read-only as a `Sheet::Table` tab, is listed in the Project Browser, and goes on
//! a sheet, where the sheet's own tools move it. What it shows is the beam's and its bars' as they are, so there is nothing to edit in
//! it: a refusal is the command's words, said inside the dialog.

use std::sync::Arc;

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id};
use bimcraft_core::render::View;
use bimcraft_core::sheet::{self, DENSITY};
use bimcraft_core::units::Units;
use egui::{Align, Checkbox, Color32, ComboBox, Context, Id as UiId, Layout, Rect, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use serde_json::{Value, json};

use crate::canvas::{Place, paint};
use crate::lang::{t, tf};
use crate::popup::{self, Modal};
use crate::reinforce::labelled;
use crate::style::pal;
use crate::{BimApp, Sheet, Status};

/// Where the details go when they are made.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) enum Onto {
    /// Nowhere: they are views, which a sheet places later.
    #[default]
    Nothing,
    /// The sheet with this number.
    Sheet(String),
    /// A sheet made with them.
    New,
}

/// The dialog: the beams it is for, and what is typed.
pub(crate) struct DetailDialog {
    pub(crate) hosts: Vec<Id>,
    /// Where the sections are cut, along a beam or a column, in the project's unit: blank is the default.
    pub(crate) stations: String,
    /// The n of 1 : n.
    pub(crate) scale: String,
    pub(crate) list: bool,
    pub(crate) onto: Onto,
    pub(crate) error: String,
}

impl DetailDialog {
    /// The command's parameters from the fields.
    pub(crate) fn params(&self, units: Units) -> Result<Value, String> {
        let mut p = json!({"hosts": self.hosts, "list": self.list});
        let pieces: Vec<&str> = self.stations.split([',', ';']).map(str::trim).filter(|s| !s.is_empty()).collect();
        if !pieces.is_empty() {
            let mut at = Vec::new();
            for piece in pieces {
                at.push(units.parse(piece).ok_or_else(|| tf("{} is not {}: `{}`", &[t("A station"), t("a length"), piece]))?);
            }
            p["stations"] = json!(at);
        }
        let scale = self.scale.trim();
        if !scale.is_empty() {
            p["scale"] =
                json!(units.parse_scale(scale).ok_or_else(|| tf("{} is not {}: `{}`", &[t("The scale"), t("a scale such as 50"), scale]))?);
        }
        Ok(p)
    }
}

/// The first sheet number A101, A102, … that no sheet has.
fn free_number(m: &bimcraft_core::model::Model) -> String {
    let taken: std::collections::BTreeSet<&str> = m.sheets().into_iter().filter_map(|id| m.sheet(id)).map(|s| s.number.as_str()).collect();
    (101..).map(|n| format!("A{n}")).find(|n| !taken.contains(n.as_str())).unwrap_or_default()
}

impl BimApp {
    /// The project's rebar details: their ids and names, by name.
    pub(crate) fn rebar_details(&self) -> Vec<(Id, &str)> {
        // A rare kind, kept by the lookup: the browser asks every frame.
        let m = &self.session.model;
        let mut out: Vec<(Id, &str)> = m
            .lookup()
            .rare("rebardetail")
            .filter_map(|id| match m.elements.get(id) {
                Some(Element::RebarDetail(rd)) => Some((id, rd.name.as_str())),
                _ => None,
            })
            .collect();
        out.sort_by_key(|(_, name)| name.to_lowercase());
        out
    }

    /// Whether the view on screen is a rebar detail.
    pub(crate) fn showing_rebar_detail(&self) -> Option<Id> {
        match self.sheet {
            Sheet::Table(id) if matches!(self.session.model.elements.get(id), Some(Element::RebarDetail(_))) => Some(id),
            _ => None,
        }
    }

    /// Detail (DD): the dialog for the members selected, which must be beams, columns, footings or floors, all of them.
    pub(crate) fn open_rebar_detail(&mut self) {
        let m = &self.session.model;
        let hosts: Vec<Id> = self
            .selection
            .iter()
            .copied()
            .filter(|id| {
                matches!(
                    m.elements.get(*id),
                    Some(Element::Beam(_) | Element::Column(_) | Element::StructuralColumn(_) | Element::Footing(_) | Element::Floor(_))
                )
            })
            .collect();
        if hosts.is_empty() || hosts.len() != self.selection.len() {
            self.status =
                Status { text: t("Select the reinforced beams, columns, footings or floors to detail first.").into(), error: true };
            return;
        }
        self.detail_dialog = Some(DetailDialog {
            hosts,
            stations: String::new(),
            scale: "50".into(),
            list: true,
            onto: Onto::Nothing,
            error: String::new(),
        });
    }

    /// The dialog, while it is open.
    pub(crate) fn rebar_detail_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.detail_dialog.take() else { return };
        let units = self.session.model.units;
        let hint = units.name();
        let sheets: Vec<(String, String)> = {
            let m = &self.session.model;
            m.sheets().into_iter().filter_map(|id| m.sheet(id)).map(|s| (s.number.clone(), format!("{} - {}", s.number, s.name))).collect()
        };
        let names: Vec<String> = d.hosts.iter().map(Id::to_string).collect();
        let (mut make, mut close) = (false, false);
        let r = Modal::new(UiId::new("rebar-detail"), t("Reinforcement Detail")).width(520.0).min_height(260.0).show(ctx, |ui| {
            popup::body(ui, |ui| {
                ui.add_space(4.0);
                ui.label(tf("Members: {}", &[&names.join(", ")]));
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    labelled(ui, &tf("{}:", &[t("Stations")]), &mut d.stations, t("Stations").to_string(), 160.0, hint);
                    labelled(ui, &tf("{}:", &[t("Scale 1 :")]), &mut d.scale, t("Scale 1 :").to_string(), 56.0, "");
                });
                ui.label(
                    RichText::new(t(
                        "Stations are where the sections are cut, along a beam or a column; blank is the middle of each zone of stirrups of a beam, and the foot, middle and head of a column. A footing and a floor take none.",
                    ))
                    .color(pal().muted),
                );
                ui.add_space(4.0);
                ui.add(Checkbox::new(&mut d.list, t("Include the steel list")));
                ui.horizontal(|ui| {
                    let l = ui.label(t("Place on:"));
                    let shown = match &d.onto {
                        Onto::Nothing => t("No sheet").to_string(),
                        Onto::New => t("A new sheet").to_string(),
                        Onto::Sheet(n) => sheets.iter().find(|(num, _)| num == n).map_or_else(|| n.clone(), |(_, label)| label.clone()),
                    };
                    let c = ComboBox::from_id_salt("rebar-detail-onto").selected_text(shown).width(220.0).show_ui(ui, |ui| {
                        if ui.selectable_label(d.onto == Onto::Nothing, t("No sheet")).clicked() {
                            d.onto = Onto::Nothing;
                        }
                        if ui.selectable_label(d.onto == Onto::New, t("A new sheet")).clicked() {
                            d.onto = Onto::New;
                        }
                        for (number, label) in &sheets {
                            if ui.selectable_label(d.onto == Onto::Sheet(number.clone()), label).clicked() {
                                d.onto = Onto::Sheet(number.clone());
                            }
                        }
                    });
                    c.response.labelled_by(l.id);
                });
                if !d.error.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(&d.error).color(pal().error));
                }
            });
            popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    make = ui.button(t("Make")).clicked();
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        close = ui.button(t("Close")).clicked();
                    });
                });
            });
        });
        let mut keep = !(close || r.should_close());
        if make {
            keep = self.make_rebar_details(&mut d);
        }
        if keep {
            self.detail_dialog = Some(d);
        }
    }

    /// Makes the details the dialog says, with a sheet for them if asked, as one undo step, and opens what it made. False when it
    /// is made and the dialog is done; a refusal stays in the dialog.
    fn make_rebar_details(&mut self, d: &mut DetailDialog) -> bool {
        d.error.clear();
        let mut p = match d.params(self.session.model.units) {
            Ok(p) => p,
            Err(e) => {
                d.error = e;
                return true;
            }
        };
        let mut batch: Vec<(&str, Value)> = Vec::new();
        let number = match &d.onto {
            Onto::Nothing => None,
            Onto::Sheet(n) => Some(n.clone()),
            Onto::New => {
                let n = free_number(&self.session.model);
                batch.push(("sheet", json!({"number": n})));
                Some(n)
            }
        };
        if let Some(n) = &number {
            p["sheet"] = json!(n);
        }
        batch.push(("rebar_detail", p));
        match self.exec_batch("Reinforcement Detail", &batch) {
            Ok(made) => {
                let view = made.last().and_then(|r| r["views"][0]["id"].as_u64());
                let sheet = number.and_then(|n| self.session.model.sheet_numbered(&n));
                match (sheet, view) {
                    (Some(sheet), _) => self.open_sheet(Sheet::Paper(sheet)),
                    (None, Some(view)) => self.open_sheet(Sheet::Table(view)),
                    (None, None) => {}
                }
                self.status = Status { text: t("Made the Reinforcement Detail.").into(), error: false };
                false
            }
            Err(e) => {
                d.error = e;
                true
            }
        }
    }

    /// Detail `id` as the middle of the window shows it: its drawing at its own scale on a sheet of paper the size it takes, which
    /// pans and zooms; nothing in it is picked or moved.
    pub(crate) fn rebar_detail_view(&mut self, ui: &mut Ui, id: Id) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let model = Arc::clone(&self.session.model);
        let Some(Element::RebarDetail(rd)) = model.elements.get(id) else { return };
        let papers = &mut self.paper;
        if !matches!(&papers.detail, Some((m, shown, _)) if Arc::ptr_eq(m, &model) && *shown == id) {
            let drawn = sheet::draw_view(&model, id, rd.scale);
            papers.detail = drawn.map(|d| (Arc::clone(&model), id, Arc::new(d)));
        }
        let Some((_, _, d)) = papers.detail.clone() else {
            ui.painter_at(rect).text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                t("Nothing to draw: the member has no bars."),
                egui::FontId::proportional(14.0),
                pal().muted,
            );
            return;
        };
        // The drawing is at paper density: its size in paper millimetres, y up.
        let size = (f64::from(d.view.width) / DENSITY, f64::from(d.view.height) / DENSITY);
        let fit = |w: u32, h: u32| {
            let room = |px: u32, mm: f64| (f64::from(px) - 48.0).max(1.0) / mm.max(1.0);
            View { angle: 0.0, scale: room(w, size.0).min(room(h, size.1)), cx: size.0 / 2.0, cy: size.1 / 2.0, width: w, height: h }
        };
        let Some((view, _)) = self.canvas.navigate(ui, &response, rect, id, fit) else { return };
        let corner = |p: P2| rect.min + vec2(view.px(p).0, view.px(p).1);
        let at = Place { origin: corner(P2(0.0, size.1)), k: (view.scale / DENSITY) as f32, paper: true, hide: None };
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, pal().desk);
        p.rect(
            Rect::from_two_pos(at.origin, corner(P2(size.0, 0.0))),
            0.0,
            Color32::WHITE,
            Stroke::new(1.0, pal().border),
            StrokeKind::Outside,
        );
        paint(&p, at, &d, &[], None, self.thin, &[]);
        self.nav_bar(ui, rect, id);
    }
}
