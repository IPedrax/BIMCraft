//! Edit Railing: a railing type's rails, posts, balusters and panel, made as Archicad and the curtain wall's Edit Pattern
//! make theirs: a part is added, picked in the preview (an elevation of two bays and a section across them) or in the
//! list, and its shape, size, height, offset and material set beside it. OK or Apply puts them on the type (`type`).

use std::sync::Arc;

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Model, PanelPart, PartShape, PostKind, PostPart, RailPart, RailingType};
use bimcraft_core::railings::{self, Piece, Which};
use bimcraft_core::units::Units;
use bimcraft_i18n::msg;
use egui::{Color32, Context, DragValue, Grid, Id as UiId, Rect, Sense, Stroke, TextEdit, Ui, WidgetInfo, WidgetType, pos2, vec2};
use serde_json::json;

use crate::BimApp;
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::{canvas_pal, pal};
use crate::typeprops;

/// Edit Railing: one railing type's parts as they are being made, which OK or Apply puts on the type.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RailingEditor {
    name: String,
    rails: Vec<RailPart>,
    posts: Vec<PostPart>,
    panel: Option<PanelPart>,
    selected: Option<Which>,
    error: Option<String>,
}

/// How long the preview's elevation is, mm: two bays of the posts' spacing, or 2.4 m.
fn sample_length(posts: &[PostPart]) -> f64 {
    posts.iter().filter(|p| p.kind == PostKind::Post && p.spacing > 0.0).map(|p| p.spacing * 2.0).reduce(f64::min).unwrap_or(2400.0)
}

/// A length in a field of the project's units, dragged or typed.
fn length(ui: &mut Ui, v: &mut f64, units: Units) -> egui::Response {
    ui.add(
        DragValue::new(v)
            .speed(5.0)
            .custom_formatter(move |x, _| units.field(x))
            .custom_parser(move |s| units.parse(s))
            .update_while_editing(false),
    )
}

/// A row of a part's fields: its label, and what it holds.
fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.label(t(label));
    add(ui);
    ui.end_row();
}

/// A drop-down of `options`, by what each reads as: what was picked.
fn pick<T: Clone + PartialEq>(ui: &mut Ui, salt: &str, value: &mut T, options: &[(T, String)]) {
    let now = options.iter().find(|(v, _)| v == value).map(|(_, l)| l.clone()).unwrap_or_default();
    egui::ComboBox::from_id_salt(("railing-edit", salt)).selected_text(now).width(170.0).show_ui(ui, |ui| {
        for (v, l) in options {
            if ui.selectable_label(v == value, l).clicked() {
                *value = v.clone();
            }
        }
    });
}

impl RailingEditor {
    pub(crate) fn new(m: &Model, name: &str) -> Option<RailingEditor> {
        let t = m.types.railing.get(name)?;
        let (rails, posts, panel) = railings::stack(t);
        let selected = (!rails.is_empty()).then_some(Which::Rail(0));
        Some(RailingEditor { name: name.to_string(), rails, posts, panel, selected, error: None })
    }

    /// The type as it is being made.
    fn draft(&self) -> RailingType {
        let mut t =
            RailingType { rails: self.rails.clone(), posts: self.posts.clone(), panel: self.panel.clone(), ..RailingType::default() };
        t.height = railings::height(&t).max(1.0);
        t
    }

    /// A name for a new part that none of `taken` has: `base`, then `base 2`, `base 3`…
    fn fresh(base: &str, taken: impl Iterator<Item = String> + Clone) -> String {
        (1..)
            .map(|i| if i == 1 { base.to_string() } else { format!("{base} {i}") })
            .find(|n| !taken.clone().any(|x| &x == n))
            .unwrap_or_default()
    }

    fn add_rail(&mut self) {
        let name = Self::fresh("Rail", self.rails.iter().map(|r| r.name.clone()));
        let below = self.rails.iter().map(|r| r.height).fold(f64::MAX, f64::min);
        let height = if below == f64::MAX { 1000.0 } else { (below / 2.0).max(100.0) };
        self.rails.push(RailPart { name, width: 40.0, depth: 40.0, height, ..RailPart::default() });
        self.selected = Some(Which::Rail(self.rails.len() - 1));
    }

    fn add_post(&mut self, kind: PostKind) {
        let base = if kind == PostKind::Post { "Post" } else { "Baluster" };
        let name = Self::fresh(base, self.posts.iter().map(|p| p.name.clone()));
        let top = self.rails.iter().max_by(|a, b| a.height.total_cmp(&b.height)).map(|r| r.name.clone());
        let p = match kind {
            PostKind::Post => PostPart { name, kind, width: 60.0, depth: 60.0, spacing: 1200.0, ..PostPart::default() },
            PostKind::Baluster => PostPart { name, kind, width: 20.0, depth: 20.0, spacing: 120.0, top, ..PostPart::default() },
        };
        self.posts.push(p);
        self.selected = Some(Which::Post(self.posts.len() - 1));
    }

    fn add_panel(&mut self) {
        let top = self.rails.iter().max_by(|a, b| a.height.total_cmp(&b.height)).map(|r| r.name.clone());
        self.panel = Some(PanelPart { thickness: 12.0, bottom: 100.0, top, top_gap: 20.0, side_gap: 20.0, ..PanelPart::default() });
        self.selected = Some(Which::Panel);
    }

    fn remove(&mut self) {
        match self.selected {
            Some(Which::Rail(i)) if i < self.rails.len() => {
                let gone = self.rails.remove(i).name;
                // What stood on it or went up to it goes to the host and the top instead.
                for p in &mut self.posts {
                    if p.bottom.as_deref() == Some(gone.as_str()) {
                        p.bottom = None;
                    }
                    if p.top.as_deref() == Some(gone.as_str()) {
                        p.top = None;
                    }
                }
                if let Some(pn) = &mut self.panel
                    && pn.top.as_deref() == Some(gone.as_str())
                {
                    pn.top = None;
                }
            }
            Some(Which::Post(i)) if i < self.posts.len() => {
                self.posts.remove(i);
            }
            Some(Which::Panel) => self.panel = None,
            _ => {}
        }
        self.selected = None;
    }

    /// What a part is called in the list and the preview.
    fn label(&self, w: Which) -> String {
        match w {
            Which::Rail(i) => tf("Rail: {}", &[self.rails.get(i).map_or("", |r| r.name.as_str())]),
            Which::Post(i) => match self.posts.get(i) {
                Some(p) if p.kind == PostKind::Post => tf("Post: {}", &[&p.name]),
                Some(p) => tf("Baluster: {}", &[&p.name]),
                None => String::new(),
            },
            Which::Panel => t("Panel").to_string(),
        }
    }

    fn all(&self) -> Vec<Which> {
        (0..self.rails.len())
            .map(Which::Rail)
            .chain((0..self.posts.len()).map(Which::Post))
            .chain(self.panel.as_ref().map(|_| Which::Panel))
            .collect()
    }

    /// The colour a part shows in: its material's, or the railing's grey; the one picked in the accent.
    fn colour(&self, m: &Model, w: Which) -> Color32 {
        if self.selected == Some(w) {
            return canvas_pal().accent;
        }
        let material = match w {
            Which::Rail(i) => self.rails.get(i).map(|r| r.material.as_str()),
            Which::Post(i) => self.posts.get(i).map(|p| p.material.as_str()),
            Which::Panel => self.panel.as_ref().map(|p| p.material.as_str()),
        };
        let base = material
            .and_then(|n| m.materials.get(n))
            .map_or(Color32::from_rgb(120, 122, 128), |x| Color32::from_rgb(x.color[0], x.color[1], x.color[2]));
        if w == Which::Panel { base.gamma_multiply(0.55) } else { base }
    }

    /// The preview: an elevation of two bays, and beside it a section across them; a click on a part picks it.
    fn preview(&mut self, ui: &mut Ui, m: &Model) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 320.0), Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, crate::canvas::color(bimcraft_core::render::PAPER));
        p.rect_stroke(rect, 0.0, Stroke::new(1.0, pal().border), egui::StrokeKind::Inside);
        let t = self.draft();
        let along = sample_length(&self.posts);
        let pieces = railings::build(&t, vec![(P2(0.0, 0.0), 0.0), (P2(along, 0.0), 0.0)]);
        let top = railings::height(&t).max(100.0);
        // The elevation on the left, the section (offsets across, as looking along the railing) on the right.
        let across = 400.0;
        let room = rect.shrink(28.0);
        let k = ((room.width() as f64 - 40.0) / (along + across)).min(room.height() as f64 / (top * 1.1));
        let ground = room.bottom();
        let ex = |s: f64| room.left() + (s * k) as f32;
        let sx = |o: f64| room.right() - (across / 2.0 * k) as f32 - (o * k) as f32;
        let z = |h: f64| ground - (h * k) as f32;
        let line = Stroke::new(1.0, canvas_pal().border);
        p.line_segment([pos2(ex(0.0) - 10.0, ground), pos2(ex(along) + 10.0, ground)], line);
        p.line_segment([pos2(sx(across / 2.0), ground), pos2(sx(-across / 2.0), ground)], line);
        let font = egui::FontId::proportional(11.0);
        p.text(
            pos2(ex(along / 2.0), ground + 4.0),
            egui::Align2::CENTER_TOP,
            crate::lang::t("Elevation"),
            font.clone(),
            canvas_pal().muted,
        );
        p.text(pos2(sx(0.0), ground + 4.0), egui::Align2::CENTER_TOP, crate::lang::t("Section"), font.clone(), canvas_pal().muted);
        let mut hits: Vec<(Rect, Which)> = Vec::new();
        let mut draw = |r: Rect, w: Which, round: bool| {
            let c = self.colour(m, w);
            if round && r.width() > 2.0 && (r.width() - r.height()).abs() < 1.0 {
                p.circle_filled(r.center(), r.width() / 2.0, c);
            } else {
                p.rect_filled(r, 0.0, c);
            }
            hits.push((r.expand(2.0), w));
        };
        // The elevation: each piece's extent along and up.
        for part in &pieces {
            let (s0, s1, z0, z1, round) = match &part.piece {
                Piece::Tube(g) => {
                    let r = g.width / 2.0;
                    (g.a[0].min(g.b[0]) - r, g.a[0].max(g.b[0]) + r, g.a[2].min(g.b[2]) - r, g.a[2].max(g.b[2]) + r, false)
                }
                Piece::Prism(ring, bottom, top) => {
                    let (lo, hi) = ring.iter().fold((f64::MAX, f64::MIN), |(lo, hi), q| (lo.min(q.0), hi.max(q.0)));
                    (lo, hi, bottom.z, top.z, false)
                }
            };
            draw(Rect::from_min_max(pos2(ex(s0), z(z1)), pos2(ex(s1), z(z0))), part.which, round);
        }
        // The section: each part once, where it stands across.
        for (i, r) in self.rails.iter().enumerate() {
            let d = railings::rail_depth(r);
            let rc =
                Rect::from_min_max(pos2(sx(r.offset + r.width / 2.0), z(r.height)), pos2(sx(r.offset - r.width / 2.0), z(r.height - d)));
            draw(rc, Which::Rail(i), r.shape == PartShape::Circle);
        }
        let rail_named = |n: &Option<String>| n.as_ref().and_then(|n| self.rails.iter().find(|r| &r.name == n));
        for (i, q) in self.posts.iter().enumerate() {
            let bottom = match (q.kind, rail_named(&q.bottom)) {
                (PostKind::Baluster, Some(r)) => r.height + q.bottom_offset,
                _ => q.bottom_offset,
            };
            let up = rail_named(&q.top).map_or(top + q.top_offset, |r| r.height - railings::rail_depth(r) + q.top_offset);
            let rc = Rect::from_min_max(pos2(sx(q.offset + q.width / 2.0), z(up)), pos2(sx(q.offset - q.width / 2.0), z(bottom)));
            draw(rc, Which::Post(i), false);
        }
        if let Some(pn) = &self.panel {
            let up = rail_named(&pn.top).map_or(top, |r| r.height - railings::rail_depth(r)) - pn.top_gap;
            let rc =
                Rect::from_min_max(pos2(sx(pn.offset + pn.thickness / 2.0), z(up)), pos2(sx(pn.offset - pn.thickness / 2.0), z(pn.bottom)));
            draw(rc, Which::Panel, false);
        }
        // Its height, the top of its highest rail.
        p.text(
            pos2(room.left(), rect.top() + 6.0),
            egui::Align2::LEFT_TOP,
            tf("Height {}", &[&m.length(railings::height(&t))]),
            font,
            canvas_pal().text,
        );
        // Picks: the smallest under the click, so a baluster in front of a panel is the baluster's.
        let response = ui.interact(rect, UiId::new("railing-preview"), Sense::click());
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "Railing preview"));
        if response.clicked()
            && let Some(at) = response.interact_pointer_pos()
        {
            self.selected = hits
                .iter()
                .filter(|(r, _)| r.contains(at))
                .min_by(|a, b| (a.0.area()).total_cmp(&b.0.area()))
                .map(|(_, w)| *w)
                .or(self.selected);
        }
    }

    /// The picked part's fields.
    fn fields(&mut self, ui: &mut Ui, m: &Model) {
        let units = m.units;
        let none = t("None").to_string();
        let materials: Vec<(String, String)> =
            std::iter::once((String::new(), none.clone())).chain(m.materials.keys().map(|k| (k.clone(), k.clone()))).collect();
        let shapes = [(PartShape::Rectangle, t("Rectangle").to_string()), (PartShape::Circle, t("Circle").to_string())];
        let names: Vec<String> = self.rails.iter().map(|r| r.name.clone()).collect();
        let refs = |host: &str| -> Vec<(Option<String>, String)> {
            std::iter::once((None, t(host).to_string())).chain(names.iter().map(|n| (Some(n.clone()), n.clone()))).collect()
        };
        let mut renamed: Option<(String, String)> = None;
        Grid::new("railing-part").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| match self.selected {
            Some(Which::Rail(i)) => {
                let Some(r) = self.rails.get_mut(i) else { return };
                let was = r.name.clone();
                row(ui, msg!("Name"), |ui| {
                    if ui.add(TextEdit::singleline(&mut r.name).desired_width(170.0)).lost_focus() && r.name != was {
                        renamed = Some((was.clone(), r.name.clone()));
                    }
                });
                row(ui, msg!("Shape"), |ui| pick(ui, "rail-shape", &mut r.shape, &shapes));
                let round = r.shape == PartShape::Circle;
                row(ui, if round { msg!("Diameter") } else { msg!("Width") }, |ui| {
                    length(ui, &mut r.width, units);
                });
                if !round {
                    row(ui, msg!("Depth"), |ui| {
                        length(ui, &mut r.depth, units);
                    });
                }
                row(ui, msg!("Height (to its top)"), |ui| {
                    length(ui, &mut r.height, units);
                });
                row(ui, msg!("Offset"), |ui| {
                    length(ui, &mut r.offset, units);
                });
                row(ui, msg!("Material"), |ui| pick(ui, "rail-material", &mut r.material, &materials));
            }
            Some(Which::Post(i)) => {
                let Some(q) = self.posts.get_mut(i) else { return };
                row(ui, msg!("Name"), |ui| {
                    ui.add(TextEdit::singleline(&mut q.name).desired_width(170.0));
                });
                let kinds = [(PostKind::Post, t("Post: stands on the host").to_string()), (PostKind::Baluster, t("Baluster").to_string())];
                row(ui, msg!("Kind"), |ui| pick(ui, "post-kind", &mut q.kind, &kinds));
                row(ui, msg!("Shape"), |ui| pick(ui, "post-shape", &mut q.shape, &shapes));
                let round = q.shape == PartShape::Circle;
                row(ui, if round { msg!("Diameter") } else { msg!("Width") }, |ui| {
                    length(ui, &mut q.width, units);
                });
                if !round {
                    row(ui, msg!("Depth"), |ui| {
                        length(ui, &mut q.depth, units);
                    });
                }
                let spacing = if q.kind == PostKind::Post { msg!("Max Spacing (0: ends and corners)") } else { msg!("Max Spacing") };
                row(ui, spacing, |ui| {
                    length(ui, &mut q.spacing, units);
                });
                if q.kind == PostKind::Post {
                    q.bottom = None;
                    row(ui, msg!("Bottom"), |ui| {
                        ui.label(t("Host"));
                    });
                } else {
                    row(ui, msg!("Bottom"), |ui| pick(ui, "post-bottom", &mut q.bottom, &refs(msg!("Host"))));
                }
                row(ui, msg!("Bottom Offset"), |ui| {
                    length(ui, &mut q.bottom_offset, units);
                });
                row(ui, msg!("Top (up to)"), |ui| pick(ui, "post-top", &mut q.top, &refs(msg!("Railing Top"))));
                row(ui, msg!("Top Offset"), |ui| {
                    length(ui, &mut q.top_offset, units);
                });
                row(ui, msg!("Offset"), |ui| {
                    length(ui, &mut q.offset, units);
                });
                row(ui, msg!("Material"), |ui| pick(ui, "post-material", &mut q.material, &materials));
            }
            Some(Which::Panel) => {
                let Some(pn) = &mut self.panel else { return };
                row(ui, msg!("Thickness"), |ui| {
                    length(ui, &mut pn.thickness, units);
                });
                row(ui, msg!("Offset"), |ui| {
                    length(ui, &mut pn.offset, units);
                });
                row(ui, msg!("Bottom (over the host)"), |ui| {
                    length(ui, &mut pn.bottom, units);
                });
                row(ui, msg!("Top (up to)"), |ui| pick(ui, "panel-top", &mut pn.top, &refs(msg!("Railing Top"))));
                row(ui, msg!("Gap Under Top"), |ui| {
                    length(ui, &mut pn.top_gap, units);
                });
                row(ui, msg!("Gap at Posts"), |ui| {
                    length(ui, &mut pn.side_gap, units);
                });
                row(ui, msg!("Material"), |ui| pick(ui, "panel-material", &mut pn.material, &materials));
            }
            None => {
                ui.label(t("Click a part in the preview or the list to edit it."));
                ui.end_row();
            }
        });
        // A rail renamed keeps what stands on it and goes up to it.
        if let Some((from, to)) = renamed {
            for q in &mut self.posts {
                for end in [&mut q.bottom, &mut q.top] {
                    if end.as_deref() == Some(from.as_str()) {
                        *end = Some(to.clone());
                    }
                }
            }
            if let Some(pn) = &mut self.panel
                && pn.top.as_deref() == Some(from.as_str())
            {
                pn.top = Some(to);
            }
        }
    }
}

impl BimApp {
    pub(crate) fn open_railing_editor(&mut self, name: &str) {
        self.railing_editor = RailingEditor::new(&self.session.model, name);
    }

    /// Edit Railing, while it's open.
    pub(crate) fn railing_editor_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.railing_editor.take() else { return };
        let model = Arc::clone(&self.session.model);
        let (mut ok, mut cancel, mut apply) = (false, false, false);
        let title = tf("Edit Railing: {}", &[&d.name]);
        let r = Modal::new(UiId::new("railing-structure"), &title).width(900.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(330.0);
                        ui.horizontal_wrapped(|ui| {
                            if ui.button(t("Add Rail")).clicked() {
                                d.add_rail();
                            }
                            if ui.button(t("Add Post")).clicked() {
                                d.add_post(PostKind::Post);
                            }
                            if ui.button(t("Add Baluster")).clicked() {
                                d.add_post(PostKind::Baluster);
                            }
                            if ui.add_enabled(d.panel.is_none(), egui::Button::new(t("Add Panel"))).clicked() {
                                d.add_panel();
                            }
                            if ui.add_enabled(d.selected.is_some(), egui::Button::new(t("Remove"))).clicked() {
                                d.remove();
                            }
                        });
                        ui.separator();
                        egui::ScrollArea::vertical().id_salt("railing-parts").max_height(150.0).show(ui, |ui| {
                            for w in d.all() {
                                let label = d.label(w);
                                if ui.selectable_label(d.selected == Some(w), label).clicked() {
                                    d.selected = Some(w);
                                }
                            }
                        });
                        ui.separator();
                        d.fields(ui, &model);
                    });
                    ui.vertical(|ui| {
                        d.preview(ui, &model);
                        ui.label(t(
                            "Offsets are to the left of the railing's line, going along it. Posts stand on the host at the ends, the \
                             corners and between at most their spacing apart; balusters and the panel fill the bays between them.",
                        ));
                    });
                });
                typeprops::error(ui, d.error.as_deref());
            });
            crate::popup::footer(ui, |ui| {
                (ok, cancel, apply) = typeprops::foot(ui);
            });
        });
        if ok || apply {
            let params = json!({"category": "railing", "name": d.name, "rails": d.rails, "posts": d.posts, "panel": d.panel});
            let done = self.run_for_type("type", params, &mut d.error);
            ok &= done;
            if done && let Some(open) = &mut self.type_dialog {
                open.refresh(&self.session.model);
            }
        }
        if !(ok || cancel || r.should_close()) {
            self.railing_editor = Some(d);
        }
    }
}

/// Edit Hosts' picking: a click adds a stair, a ramp or a floor to the railing's hosts, or takes one out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HostPick {
    Add,
    Remove,
}

/// Edit Hosts: the railing whose hosts are being picked, and what a click does now (none: it selects as ever).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HostEdit {
    pub(crate) railing: bimcraft_core::model::Id,
    pub(crate) pick: Option<HostPick>,
}

impl BimApp {
    /// The one railing selected.
    fn railing_selected(&self) -> Option<bimcraft_core::model::Id> {
        match self.selection.as_slice() {
            [id] if matches!(self.session.model.elements.get(*id), Some(bimcraft_core::model::Element::Railing(_))) => Some(*id),
            _ => None,
        }
    }

    /// The hosts of the railing whose hosts are being edited, lit as the selection is.
    pub(crate) fn railing_hosts_lit(&self) -> Vec<bimcraft_core::model::Id> {
        let Some(e) = self.railing_hosts else { return Vec::new() };
        let m = &self.session.model;
        match m.elements.get(e.railing) {
            Some(bimcraft_core::model::Element::Railing(rl)) => railings::hosts(rl).into_iter().filter_map(|g| m.elements.id(g)).collect(),
            _ => Vec::new(),
        }
    }

    /// What the status bar asks while hosts are picked.
    pub(crate) fn railing_hosts_prompt(&self) -> Option<&'static str> {
        match self.railing_hosts?.pick? {
            HostPick::Add => Some(msg!("Add Host: click a stair, a ramp or a floor for the railing to follow. Add again stops.")),
            HostPick::Remove => Some(msg!("Remove Host: click a lit host to stop the railing following it. Remove again stops.")),
        }
    }

    /// Add or Remove picking: the first stair, ramp or floor among `found` joins the railing's hosts or leaves them, and
    /// the railing stays selected. Whether a pick was taken so.
    pub(crate) fn railing_host_pick_up(&mut self, found: &[bimcraft_core::model::Id]) -> bool {
        use bimcraft_core::model::Element;
        let Some(HostEdit { railing, pick: Some(pick) }) = self.railing_hosts else { return false };
        let m = Arc::clone(&self.session.model);
        let host =
            found.iter().copied().find(|id| matches!(m.elements.get(*id), Some(Element::Stair(_) | Element::Ramp(_) | Element::Floor(_))));
        if let Some(host) = host {
            let key = if pick == HostPick::Add { "add" } else { "remove" };
            self.run("railing_hosts", json!({"id": railing, key: [host]}));
        }
        self.selection = vec![railing];
        true
    }

    /// The Modify tab's Railing panel: Edit Hosts for the one railing selected; while it's on, Add, Remove and Finish.
    pub(crate) fn railing_panel(&mut self, ui: &mut Ui) {
        use crate::icons::Icon;
        if self.railing_hosts.is_some_and(|e| !self.session.model.elements.contains_key(e.railing)) {
            self.railing_hosts = None;
        }
        match self.railing_hosts {
            Some(mut edit) => crate::ribbon::panel(ui, msg!("Railing Hosts"), |ui| {
                let tip = msg!("Add: click stairs, ramps and floors for the railing to follow; click Add again to stop");
                if crate::big_button(ui, Icon::AddToGroup, msg!("Add"), edit.pick == Some(HostPick::Add), tip).clicked() {
                    edit.pick = (edit.pick != Some(HostPick::Add)).then_some(HostPick::Add);
                }
                let tip = msg!("Remove: click a lit host for the railing to stop following it; click Remove again to stop");
                if crate::big_button(ui, Icon::RemoveFromGroup, msg!("Remove"), edit.pick == Some(HostPick::Remove), tip).clicked() {
                    edit.pick = (edit.pick != Some(HostPick::Remove)).then_some(HostPick::Remove);
                }
                let tip = msg!("Finish: stop editing the railing's hosts");
                let finished =
                    crate::inked_button(ui, Icon::Check, msg!("Finish"), false, tip, Some(crate::style::pal().context)).clicked();
                self.railing_hosts = (!finished).then_some(edit);
                if finished {
                    self.selection = vec![edit.railing];
                }
            }),
            None => {
                let Some(id) = self.railing_selected() else { return };
                crate::ribbon::panel(ui, msg!("Railing"), |ui| {
                    let tip = msg!(
                        "Edit Hosts: pick the stairs, ramps and floors the railing follows; where its line passes over one it stands on its top"
                    );
                    if crate::big_button(ui, Icon::Railing, msg!("Edit Hosts"), false, tip).clicked() {
                        self.railing_hosts = Some(HostEdit { railing: id, pick: Some(HostPick::Add) });
                    }
                });
            }
        }
    }
}
