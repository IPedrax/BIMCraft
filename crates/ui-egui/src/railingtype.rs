//! Edit Railing: a railing type's rails, posts, balusters and panel, in three columns. On the left the parts, added with
//! the buttons over them and listed by kind; in the middle the picked part's parameters, by section (shape, bottom, top,
//! placing, material); on the right a preview, a side view of two bays and a cut across them, where a click picks a part.
//! OK or Apply puts them on the type (`type`). Also Edit Hosts and Edit Path, for one railing selected.

use std::sync::Arc;

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Model, PanelPart, PartShape, PostKind, PostPart, RailPart, RailingType};
use bimcraft_core::railings::{self, Piece, Which};
use bimcraft_core::units::Units;
use bimcraft_i18n::msg;
use egui::{
    Color32, Context, DragValue, Grid, Id as UiId, Layout, Rect, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui, WidgetInfo, WidgetType,
    pos2, vec2,
};
use serde_json::json;

use crate::BimApp;
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::{canvas_pal, pal};
use crate::typeprops;

/// The window's size: fixed, so it does not move as parts are picked; its columns are as tall as it is.
const WIDTH: f32 = 1160.0;
const HEIGHT: f32 = 600.0;
/// The parts' column and the parameters' column; the preview has the rest.
const PARTS: f32 = 250.0;
const PARAMETERS: f32 = 390.0;

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

/// How long the preview's side view is, mm: two bays of the posts' spacing, or 2.4 m.
fn sample_length(posts: &[PostPart]) -> f64 {
    posts.iter().filter(|p| p.kind == PostKind::Post && p.spacing > 0.0).map(|p| p.spacing * 2.0).reduce(f64::min).unwrap_or(2400.0)
}

/// A length in a field of the project's units, dragged or typed.
fn length(ui: &mut Ui, v: &mut f64, units: Units) {
    ui.add(
        DragValue::new(v)
            .speed(5.0)
            .custom_formatter(move |x, _| units.field(x))
            .custom_parser(move |s| units.parse(s))
            .update_while_editing(false),
    );
}

/// A drop-down of `options`, by what each reads as.
fn pick<T: Clone + PartialEq>(ui: &mut Ui, salt: &str, value: &mut T, options: &[(T, String)]) {
    let now = options.iter().find(|(v, _)| v == value).map(|(_, l)| l.clone()).unwrap_or_default();
    egui::ComboBox::from_id_salt(("railing-edit", salt)).selected_text(now).width(190.0).show_ui(ui, |ui| {
        for (v, l) in options {
            if ui.selectable_label(v == value, l).clicked() {
                *value = v.clone();
            }
        }
    });
}

/// A section of the parameters: its name over a grid of rows.
fn section(ui: &mut Ui, title: &str, rows: impl FnOnce(&mut Ui)) {
    ui.add_space(6.0);
    ui.label(RichText::new(t(title)).strong());
    ui.separator();
    Grid::new(("railing-section", title)).num_columns(2).min_col_width(150.0).spacing(vec2(10.0, 6.0)).show(ui, rows);
}

/// A row of a section: its label, and what it holds.
fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.label(t(label));
    add(ui);
    ui.end_row();
}

/// A line of help under a section, in small grey letters.
fn hint(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(t(text)).small().color(pal().muted)).wrap());
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
            PostKind::Post => PostPart { name, kind, width: 60.0, depth: 60.0, spacing: 1200.0, top, ..PostPart::default() },
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

    /// A copy of the part picked, picked.
    fn duplicate(&mut self) {
        match self.selected {
            Some(Which::Rail(i)) => {
                let Some(mut r) = self.rails.get(i).cloned() else { return };
                r.name = Self::fresh(&r.name, self.rails.iter().map(|r| r.name.clone()));
                self.rails.push(r);
                self.selected = Some(Which::Rail(self.rails.len() - 1));
            }
            Some(Which::Post(i)) => {
                let Some(mut p) = self.posts.get(i).cloned() else { return };
                p.name = Self::fresh(&p.name, self.posts.iter().map(|p| p.name.clone()));
                self.posts.push(p);
                self.selected = Some(Which::Post(self.posts.len() - 1));
            }
            _ => {}
        }
    }

    fn remove(&mut self, which: Which) {
        match which {
            Which::Rail(i) if i < self.rails.len() => {
                let gone = self.rails.remove(i).name;
                // What stood on it or went up to it goes to the host and the top instead.
                for p in &mut self.posts {
                    for end in [&mut p.bottom, &mut p.top] {
                        if end.as_deref() == Some(gone.as_str()) {
                            *end = None;
                        }
                    }
                }
                if let Some(pn) = &mut self.panel
                    && pn.top.as_deref() == Some(gone.as_str())
                {
                    pn.top = None;
                }
            }
            Which::Post(i) if i < self.posts.len() => {
                self.posts.remove(i);
            }
            Which::Panel => self.panel = None,
            _ => {}
        }
        self.selected = None;
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

    /// The left column: the Add buttons, and the parts by kind, each with a button to remove it.
    fn parts(&mut self, ui: &mut Ui) {
        Grid::new("railing-add").num_columns(2).spacing(vec2(6.0, 6.0)).show(ui, |ui| {
            let wide = vec2((PARTS - 14.0) / 2.0, 24.0);
            if ui.add_sized(wide, egui::Button::new(t("+ Rail"))).on_hover_text(t("Add a rail")).clicked() {
                self.add_rail();
            }
            if ui.add_sized(wide, egui::Button::new(t("+ Post"))).on_hover_text(t("Add posts")).clicked() {
                self.add_post(PostKind::Post);
            }
            ui.end_row();
            if ui.add_sized(wide, egui::Button::new(t("+ Baluster"))).on_hover_text(t("Add balusters")).clicked() {
                self.add_post(PostKind::Baluster);
            }
            let panel = ui.add_enabled_ui(self.panel.is_none(), |ui| ui.add_sized(wide, egui::Button::new(t("+ Panel"))));
            if panel.inner.on_hover_text(t("Add a panel between the posts")).clicked() {
                self.add_panel();
            }
            ui.end_row();
        });
        ui.add_space(6.0);
        let mut gone = None;
        let room = (ui.available_height() - 40.0).max(120.0);
        ScrollArea::vertical().id_salt("railing-parts").max_height(room).auto_shrink([false, false]).show(ui, |ui| {
            let rails: Vec<(Which, String)> = self.rails.iter().enumerate().map(|(i, r)| (Which::Rail(i), r.name.clone())).collect();
            let of = |kind: PostKind| -> Vec<(Which, String)> {
                self.posts.iter().enumerate().filter(|(_, p)| p.kind == kind).map(|(i, p)| (Which::Post(i), p.name.clone())).collect()
            };
            let panel: Vec<(Which, String)> = self.panel.iter().map(|_| (Which::Panel, t("Panel").to_string())).collect();
            for (title, items) in [
                (msg!("Rails"), rails),
                (msg!("Posts"), of(PostKind::Post)),
                (msg!("Balusters"), of(PostKind::Baluster)),
                (msg!("Panel"), panel),
            ] {
                ui.label(RichText::new(format!("{} ({})", t(title), items.len())).strong());
                if items.is_empty() {
                    ui.label(RichText::new(t("None")).color(pal().muted));
                }
                for (w, name) in items {
                    ui.horizontal(|ui| {
                        if crate::icon_button(ui, crate::icons::Icon::Close, &tf("Remove {}", &[&name])).clicked() {
                            gone = Some(w);
                        }
                        let label = if name.is_empty() { t("(no name)").to_string() } else { name };
                        if ui.selectable_label(self.selected == Some(w), label).clicked() {
                            self.selected = Some(w);
                        }
                    });
                }
                ui.add_space(6.0);
            }
        });
        if let Some(w) = gone {
            self.remove(w);
        }
        ui.horizontal(|ui| {
            let copyable = matches!(self.selected, Some(Which::Rail(_) | Which::Post(_)));
            if ui.add_enabled(copyable, egui::Button::new(t("Duplicate"))).clicked() {
                self.duplicate();
            }
            if ui.add_enabled(self.selected.is_some(), egui::Button::new(t("Remove"))).clicked()
                && let Some(w) = self.selected
            {
                self.remove(w);
            }
        });
    }

    /// The middle column: the picked part's parameters, by section.
    fn parameters(&mut self, ui: &mut Ui, m: &Model) {
        let units = m.units;
        let none = t("None").to_string();
        let materials: Vec<(String, String)> =
            std::iter::once((String::new(), none.clone())).chain(m.materials.keys().map(|k| (k.clone(), k.clone()))).collect();
        let shapes = [(PartShape::Rectangle, t("Rectangle").to_string()), (PartShape::Circle, t("Circle").to_string())];
        let names: Vec<String> = self.rails.iter().map(|r| r.name.clone()).collect();
        let refs = |first: &str| -> Vec<(Option<String>, String)> {
            std::iter::once((None, t(first).to_string())).chain(names.iter().map(|n| (Some(n.clone()), n.clone()))).collect()
        };
        let mut renamed: Option<(String, String)> = None;
        match self.selected {
            Some(Which::Rail(i)) => {
                let Some(r) = self.rails.get_mut(i) else { return };
                ui.heading(tf("Rail: {}", &[&r.name]));
                let was = r.name.clone();
                section(ui, msg!("General"), |ui| {
                    row(ui, msg!("Name"), |ui| {
                        if ui.add(TextEdit::singleline(&mut r.name).desired_width(190.0)).lost_focus() && r.name != was {
                            renamed = Some((was.clone(), r.name.clone()));
                        }
                    });
                });
                hint(ui, msg!("Posts and balusters can go up to a rail by its name."));
                section(ui, msg!("Shape"), |ui| {
                    row(ui, msg!("Shape"), |ui| pick(ui, "rail-shape", &mut r.shape, &shapes));
                    let round = r.shape == PartShape::Circle;
                    row(ui, if round { msg!("Diameter") } else { msg!("Width") }, |ui| length(ui, &mut r.width, units));
                    if !round {
                        row(ui, msg!("Height of the bar"), |ui| length(ui, &mut r.depth, units));
                    }
                });
                section(ui, msg!("Position"), |ui| {
                    row(ui, msg!("Height of its top"), |ui| length(ui, &mut r.height, units));
                    row(ui, msg!("Side offset"), |ui| length(ui, &mut r.offset, units));
                });
                hint(
                    ui,
                    msg!(
                        "The height is measured from the stair nosing or the floor. A side offset moves it to the left (plus) or right (minus) of the railing line."
                    ),
                );
                section(ui, msg!("Material"), |ui| {
                    row(ui, msg!("Material"), |ui| pick(ui, "rail-material", &mut r.material, &materials));
                });
            }
            Some(Which::Post(i)) => {
                let Some(q) = self.posts.get_mut(i) else { return };
                let post = q.kind == PostKind::Post;
                ui.heading(if post { tf("Post: {}", &[&q.name]) } else { tf("Baluster: {}", &[&q.name]) });
                section(ui, msg!("General"), |ui| {
                    row(ui, msg!("Name"), |ui| {
                        ui.add(TextEdit::singleline(&mut q.name).desired_width(190.0));
                    });
                    let kinds = [(PostKind::Post, t("Post").to_string()), (PostKind::Baluster, t("Baluster").to_string())];
                    row(ui, msg!("Kind"), |ui| pick(ui, "post-kind", &mut q.kind, &kinds));
                });
                hint(ui, msg!("A post always stands on the floor or the stair. A baluster can also stand on a rail."));
                section(ui, msg!("Shape"), |ui| {
                    row(ui, msg!("Shape"), |ui| pick(ui, "post-shape", &mut q.shape, &shapes));
                    let round = q.shape == PartShape::Circle;
                    row(ui, if round { msg!("Diameter") } else { msg!("Width (across)") }, |ui| length(ui, &mut q.width, units));
                    if !round {
                        row(ui, msg!("Depth (along)"), |ui| length(ui, &mut q.depth, units));
                    }
                });
                section(ui, msg!("Bottom"), |ui| {
                    if q.kind == PostKind::Post {
                        q.bottom = None;
                        row(ui, msg!("Stands on"), |ui| {
                            ui.label(t("Floor or stair"));
                        });
                    } else {
                        row(ui, msg!("Stands on"), |ui| pick(ui, "post-bottom", &mut q.bottom, &refs(msg!("Floor or stair"))));
                    }
                    row(ui, msg!("Bottom offset"), |ui| length(ui, &mut q.bottom_offset, units));
                });
                section(ui, msg!("Top"), |ui| {
                    row(ui, msg!("Goes up to"), |ui| pick(ui, "post-top", &mut q.top, &refs(msg!("Top of the railing"))));
                    row(ui, msg!("Top offset"), |ui| length(ui, &mut q.top_offset, units));
                });
                hint(ui, msg!("Up to a rail means up to the bottom of that rail. Plus offsets move up, minus offsets move down."));
                section(ui, msg!("Spacing"), |ui| {
                    let label = if post { msg!("Most space between") } else { msg!("Space between") };
                    row(ui, label, |ui| length(ui, &mut q.spacing, units));
                    if post {
                        let mut on = q.steps > 0;
                        row(ui, msg!("Line up with steps"), |ui| {
                            if ui.checkbox(&mut on, "").changed() {
                                q.steps = if on { 2 } else { 0 };
                            }
                        });
                        if on {
                            row(ui, msg!("On every … step"), |ui| {
                                ui.add(DragValue::new(&mut q.steps).range(1..=50));
                            });
                            let mut ends = !q.skip_ends;
                            row(ui, msg!("On first and last step"), |ui| {
                                if ui.checkbox(&mut ends, "").changed() {
                                    q.skip_ends = !ends;
                                }
                            });
                        }
                    }
                    row(ui, msg!("Side offset"), |ui| length(ui, &mut q.offset, units));
                });
                if post {
                    hint(
                        ui,
                        msg!(
                            "Posts go at the ends and corners, and between them no more than this far apart. On a stair, they can stand on the steps instead."
                        ),
                    );
                } else {
                    hint(ui, msg!("Balusters fill the space between the posts, no more than this far apart."));
                }
                section(ui, msg!("Material"), |ui| {
                    row(ui, msg!("Material"), |ui| pick(ui, "post-material", &mut q.material, &materials));
                });
            }
            Some(Which::Panel) => {
                let Some(pn) = &mut self.panel else { return };
                ui.heading(t("Panel"));
                section(ui, msg!("Shape"), |ui| {
                    row(ui, msg!("Thickness"), |ui| length(ui, &mut pn.thickness, units));
                });
                section(ui, msg!("Bottom"), |ui| {
                    row(ui, msg!("Height above floor"), |ui| length(ui, &mut pn.bottom, units));
                });
                section(ui, msg!("Top"), |ui| {
                    row(ui, msg!("Goes up to"), |ui| pick(ui, "panel-top", &mut pn.top, &refs(msg!("Top of the railing"))));
                    row(ui, msg!("Gap at the top"), |ui| length(ui, &mut pn.top_gap, units));
                });
                section(ui, msg!("Sides"), |ui| {
                    row(ui, msg!("Gap at the posts"), |ui| length(ui, &mut pn.side_gap, units));
                    row(ui, msg!("Side offset"), |ui| length(ui, &mut pn.offset, units));
                });
                hint(ui, msg!("There is one panel between each pair of posts."));
                section(ui, msg!("Material"), |ui| {
                    row(ui, msg!("Material"), |ui| pick(ui, "panel-material", &mut pn.material, &materials));
                });
            }
            None => {
                ui.add_space(20.0);
                ui.label(t("Pick a part on the left or in the picture to change it."));
            }
        }
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

    /// The right column: a side view of two bays and a cut across them; a click on a part picks it.
    fn preview(&mut self, ui: &mut Ui, m: &Model, size: egui::Vec2) {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, crate::canvas::color(bimcraft_core::render::PAPER));
        p.rect_stroke(rect, 0.0, Stroke::new(1.0, pal().border), egui::StrokeKind::Inside);
        let t = self.draft();
        let along = sample_length(&self.posts);
        let pieces = railings::build(&t, vec![(P2(0.0, 0.0), 0.0), (P2(along, 0.0), 0.0)], &railings::Ground::none());
        let top = railings::height(&t).max(100.0);
        let across = 400.0;
        let room = Rect::from_min_max(rect.min + vec2(24.0, 30.0), rect.max - vec2(24.0, 30.0));
        let k = ((room.width() as f64 - 40.0) / (along + across)).min(room.height() as f64 / (top * 1.1));
        // Its ground under the middle of the room, the drawing as high as it needs.
        let ground = (room.center().y + (top * 1.1 * k) as f32 / 2.0).min(room.bottom());
        let ex = |s: f64| room.left() + (s * k) as f32;
        let sx = |o: f64| room.right() - (across / 2.0 * k) as f32 - (o * k) as f32;
        let z = |h: f64| ground - (h * k) as f32;
        let line = Stroke::new(1.0, canvas_pal().border);
        p.line_segment([pos2(ex(0.0) - 10.0, ground), pos2(ex(along) + 10.0, ground)], line);
        p.line_segment([pos2(sx(across / 2.0), ground), pos2(sx(-across / 2.0), ground)], line);
        let font = egui::FontId::proportional(11.0);
        let label = |at: f32, text: &str| {
            p.text(pos2(at, ground + 4.0), egui::Align2::CENTER_TOP, crate::lang::t(text), font.clone(), canvas_pal().muted)
        };
        label(ex(along / 2.0), msg!("Side view"));
        label(sx(0.0), msg!("Cut across"));
        let mut hits: Vec<(Rect, Which)> = Vec::new();
        let mut draw = |r: Rect, w: Which, round: bool| {
            let c = self.colour(m, w);
            if round && r.width() > 2.0 {
                p.circle_filled(r.center(), r.width() / 2.0, c);
            } else {
                p.rect_filled(r, 0.0, c);
            }
            hits.push((r.expand(2.0), w));
        };
        for part in &pieces {
            let (s0, s1, z0, z1) = match &part.piece {
                Piece::Tube(g) => {
                    let r = g.width / 2.0;
                    (g.a[0].min(g.b[0]) - r, g.a[0].max(g.b[0]) + r, g.a[2].min(g.b[2]) - r, g.a[2].max(g.b[2]) + r)
                }
                Piece::Sweep { path, depth, .. } => {
                    let (lo, hi) = path.iter().fold((f64::MAX, f64::MIN), |(lo, hi), q| (lo.min(q[0]), hi.max(q[0])));
                    let (zl, zh) = path.iter().fold((f64::MAX, f64::MIN), |(lo, hi), q| (lo.min(q[2]), hi.max(q[2])));
                    (lo, hi, zl - depth / 2.0, zh + depth / 2.0)
                }
                Piece::Prism(ring, bottom, top) => {
                    let (lo, hi) = ring.iter().fold((f64::MAX, f64::MIN), |(lo, hi), q| (lo.min(q.0), hi.max(q.0)));
                    (lo, hi, bottom.z, top.z)
                }
            };
            draw(Rect::from_min_max(pos2(ex(s0), z(z1)), pos2(ex(s1), z(z0))), part.which, false);
        }
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
        p.text(
            pos2(rect.left() + 10.0, rect.top() + 8.0),
            egui::Align2::LEFT_TOP,
            tf("Railing height: {}", &[&m.length(railings::height(&t))]),
            egui::FontId::proportional(12.0),
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
                .min_by(|a, b| a.0.area().total_cmp(&b.0.area()))
                .map(|(_, w)| *w)
                .or(self.selected);
        }
    }
}

impl BimApp {
    pub(crate) fn open_railing_editor(&mut self, name: &str) {
        self.railing_editor = RailingEditor::new(&self.session.model, name);
    }

    /// Edit Railing, while it's open: three columns of one fixed size.
    pub(crate) fn railing_editor_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.railing_editor.take() else { return };
        let model = Arc::clone(&self.session.model);
        let (mut ok, mut cancel, mut apply) = (false, false, false);
        let title = tf("Edit Railing: {}", &[&d.name]);
        let r = Modal::new(UiId::new("railing-structure"), &title).width(WIDTH).min_height(HEIGHT + 90.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                // As tall as the window has room for, less the error line: the same whatever is picked.
                let tall = (ui.available_height() - 30.0).max(HEIGHT);
                ui.horizontal_top(|ui| {
                    let column = |ui: &mut Ui, width: f32, add: &mut dyn FnMut(&mut Ui)| {
                        ui.allocate_ui_with_layout(vec2(width, tall), Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_min_size(vec2(width, tall));
                            ui.set_max_size(vec2(width, tall));
                            add(ui);
                        });
                    };
                    column(ui, PARTS, &mut |ui| d.parts(ui));
                    ui.separator();
                    column(ui, PARAMETERS, &mut |ui| {
                        ScrollArea::vertical().id_salt("railing-parameters").max_height(tall).auto_shrink([false, false]).show(ui, |ui| {
                            d.parameters(ui, &model);
                        });
                    });
                    ui.separator();
                    let rest = (ui.available_width() - 4.0).max(200.0);
                    column(ui, rest, &mut |ui| {
                        d.preview(ui, &model, vec2(rest, tall - 30.0));
                        ui.label(RichText::new(t("Click a part in the picture to change it.")).small().color(pal().muted));
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
            HostPick::Add => Some(msg!(
                "Add Host: click a stair, ramp, floor, roof or toposolid. The railing will stand on it. Click Add again to stop."
            )),
            HostPick::Remove => {
                Some(msg!("Remove Host: click a highlighted host. The railing will stop following it. Click Remove again to stop."))
            }
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
                let tip = msg!("Add: click the stairs, floors and other hosts the railing should stand on");
                if crate::big_button(ui, Icon::AddToGroup, msg!("Add"), edit.pick == Some(HostPick::Add), tip).clicked() {
                    edit.pick = (edit.pick != Some(HostPick::Add)).then_some(HostPick::Add);
                }
                let tip = msg!("Remove: click a highlighted host to take it away from the railing");
                if crate::big_button(ui, Icon::RemoveFromGroup, msg!("Remove"), edit.pick == Some(HostPick::Remove), tip).clicked() {
                    edit.pick = (edit.pick != Some(HostPick::Remove)).then_some(HostPick::Remove);
                }
                let tip = msg!("Finish: stop editing the hosts");
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
                    let tip = msg!("Edit Path: draw the line the railing follows again, in plan");
                    if crate::big_button(ui, Icon::Railing, msg!("Edit Path"), false, tip).clicked() {
                        self.edit_railing_path(id);
                    }
                    let tip = msg!("Edit Hosts: pick the stairs, ramps, floors, roofs and toposolids the railing stands on");
                    if crate::big_button(ui, Icon::Railing, msg!("Edit Hosts"), false, tip).clicked() {
                        self.railing_hosts = Some(HostEdit { railing: id, pick: Some(HostPick::Add) });
                    }
                });
            }
        }
    }
}
