//! Underlays on screen: the view a plan, section or elevation shows under (or over) it, as Archicad's Trace & Reference
//! ([`bimcraft_core::underlay`] draws it). It is made on a worker of its own and only once the view's own drawing is made, so
//! the view on screen always comes first; until a new one is made, the last is shown moved with the view. Its lines are kept
//! on the card as the plan's are, and snapped to, never picked. Properties' Underlay group turns it on and opens Underlay
//! Settings, whose Move and Rotate pick their points in the view while the dialog stands aside.

use std::sync::{Arc, Weak};

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Id, Model};
use bimcraft_core::render::{Drawing, View};
use bimcraft_core::underlay::{self as core, Kind, Picture, Underlay};
use egui::{Color32, ColorImage, Context, Painter, Pos2, Rect, Response, Stroke, TextureHandle, TextureOptions, Ui, vec2};
use serde_json::json;

use crate::canvas::{Place, angle_between, color, paint};
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::view_worker::{Cancel, Latest};
use crate::{BimApp, Sheet, Status};

/// An underlay made: of which model, for which view, with which settings, through the view's framing then.
pub(crate) struct Made {
    model: Weak<Model>,
    target: Id,
    settings: Underlay,
    view: View,
    drawing: Arc<Drawing>,
    picture: Option<((f32, f32), Picture)>,
    front: bool,
    /// Its lines in the view's own coordinates, mm, for snapping.
    segments: Vec<(P2, P2)>,
}

struct Job {
    model: Arc<Model>,
    target: Id,
    view: View,
}

/// What was last asked, so a frame that asks the same asks nothing.
type Asked = (Weak<Model>, Id, Underlay, View);

/// The underlay of the view on screen, between frames.
pub(crate) struct Layer {
    shown: Option<Arc<Made>>,
    worker: Option<Latest<Job, Made>>,
    asked: Option<Asked>,
    retained: crate::retained::Retained,
    texture: Option<(Weak<Made>, bool, TextureHandle)>,
    /// Made in the frame, not on a worker: the tests', which drive the window frame by frame.
    inline: bool,
}

// Not derived: a test's is made in the frame, not on a worker.
#[allow(clippy::derivable_impls)]
impl Default for Layer {
    fn default() -> Self {
        Layer { shown: None, worker: None, asked: None, retained: Default::default(), texture: None, inline: cfg!(test) }
    }
}

/// A copy of the window starts its own.
impl Clone for Layer {
    fn clone(&self) -> Self {
        Layer::default()
    }
}

fn make(
    m: &Arc<Model>,
    work: Option<&bimcraft_core::render::SectionWork>,
    target: Id,
    view: View,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<Made> {
    let settings = core::shown(m, target)?.clone();
    let u = core::draw_cancellable(m, work, target, view, cancelled)??;
    let segments = u.segments();
    Some(Made {
        model: Arc::downgrade(m),
        target,
        settings,
        view,
        drawing: Arc::new(u.drawing),
        picture: u.picture,
        front: u.front,
        segments,
    })
}

/// The worker's job: a section's work kept with the model it is of, for the next job of the same model.
fn worker(ctx: &Context) -> Latest<Job, Made> {
    let mut kept: Option<(Arc<Model>, bimcraft_core::render::SectionWork)> = None;
    let wake = ctx.clone();
    Latest::new(
        move |job: Job, cancel: &Cancel| {
            let cancelled = || cancel.cancelled();
            let m = &job.model;
            let section = core::shown(m, job.target).is_some_and(|u| core::kind(m, u.view) == Some(Kind::Section));
            if section && !kept.as_ref().is_some_and(|(k, _)| Arc::ptr_eq(k, m)) {
                // The old model's work goes before the new is made.
                kept = None;
                kept = Some((Arc::clone(m), bimcraft_core::render::SectionWork::new_deferred_cancellable(m, &cancelled)?));
            }
            let work = kept.as_ref().filter(|_| section).map(|(_, w)| w);
            make(m, work, job.target, job.view, &cancelled)
        },
        move || wake.request_repaint(),
    )
}

impl Layer {
    /// The underlay to show of view `target` through `view`: the one made for it, else the last of the same view and source
    /// while a new one is made. Nothing is asked while `hold`, when the view's own drawing is still being made.
    pub(crate) fn current(&mut self, ctx: &Context, model: &Arc<Model>, target: Id, view: View, hold: bool) -> Option<Arc<Made>> {
        let Some(settings) = core::shown(model, target).cloned() else {
            self.shown = None;
            self.asked = None;
            return None;
        };
        if let Some(done) = self.worker.as_ref().and_then(Latest::take)
            && done.target == target
        {
            self.shown = Some(Arc::new(done));
        }
        let weak = Arc::downgrade(model);
        let fresh = |m: &Made| m.model.ptr_eq(&weak) && m.target == target && m.settings == settings && m.view == view;
        if self.shown.as_deref().is_some_and(fresh) {
            return self.shown.clone();
        }
        if self.inline {
            self.shown = make(model, None, target, view, &|| false).map(Arc::new);
            return self.shown.clone();
        }
        let asked = |a: &Asked| a.0.ptr_eq(&weak) && a.1 == target && a.2 == settings && a.3 == view;
        if !hold && !self.asked.as_ref().is_some_and(asked) {
            let worker = self.worker.get_or_insert_with(|| worker(ctx));
            worker.request(Job { model: Arc::clone(model), target, view });
            self.asked = Some((weak, target, settings.clone(), view));
        }
        if let Some(worker) = &self.worker {
            if worker.start().is_err() || worker.take_error().is_some() {
                // A worker that can't run: made here, as a harness does.
                self.shown = make(model, None, target, view, &|| false).map(Arc::new);
                return self.shown.clone();
            }
            if worker.busy() || hold {
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
        }
        self.shown.clone().filter(|m| m.target == target && m.settings.view == settings.view)
    }

    /// The lines of view `target`'s underlay, mm, for snapping: none while it shows none.
    pub(crate) fn segments(&self, target: Id) -> &[(P2, P2)] {
        self.shown.as_deref().filter(|m| m.target == target).map_or(&[], |m| &m.segments)
    }

    /// Whether view `target`'s underlay is drawn behind it, so what the view lays over its paper must let it through.
    pub(crate) fn behind(m: &Model, target: Id) -> bool {
        core::shown(m, target).is_some_and(|u| !u.front)
    }

    /// Leaving the view: nothing more is made.
    pub(crate) fn suspend(&mut self) {
        self.worker = None;
        self.asked = None;
    }

    /// `made` drawn through `p` on a canvas whose top left is `origin`, at `view`, `nudge` px from where it falls.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint(
        &mut self,
        ctx: &Context,
        p: &Painter,
        pipeline: Option<&Arc<crate::retained::Pipeline>>,
        origin: Pos2,
        view: View,
        made: &Arc<Made>,
        nudge: egui::Vec2,
        thin: bool,
    ) {
        let (k, offset) = if made.view == view {
            (1.0, vec2(0.0, 0.0))
        } else {
            let (x, y) = view.px(made.view.mm((0.0, 0.0)));
            ((view.scale / made.view.scale) as f32, vec2(x, y))
        };
        let place = Place { k, ..Place::at(origin + offset + nudge) };
        if let Some((at, picture)) = &made.picture {
            let dark = crate::style::canvas_dark();
            let stale = !self.texture.as_ref().is_some_and(|(w, d, _)| w.ptr_eq(&Arc::downgrade(made)) && *d == dark);
            if stale {
                let ink = color(made.settings.rgb());
                let pixels: Vec<Color32> = picture
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|px| Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), px[3]))
                    .collect();
                let image = ColorImage { size: [picture.width as usize, picture.height as usize], pixels, source_size: Default::default() };
                self.texture = Some((Arc::downgrade(made), dark, ctx.load_texture("underlay", image, TextureOptions::LINEAR)));
            }
            if let Some((_, _, texture)) = &self.texture {
                let rect = Rect::from_min_size(place.pos(*at), vec2(picture.width as f32, picture.height as f32) * k);
                p.image(texture.id(), rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
        }
        let kept = pipeline.is_some_and(|pipeline| self.retained.paint(ctx, pipeline, p, place, origin, &made.drawing, &[], None, thin));
        if !kept {
            paint(p, place, &made.drawing, &[], None, thin, &[]);
        }
    }
}

impl Made {
    pub(crate) fn front(&self) -> bool {
        self.front
    }
}

/// Underlay Settings' Move and Rotate, picking their points in the view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Edit {
    /// From a point, to another: the underlay moves by as much.
    Move { view: Id, from: Option<P2> },
    /// About a centre, from one ray to another.
    Rotate { view: Id, centre: Option<P2>, start: Option<P2> },
}

impl Edit {
    fn view(self) -> Id {
        match self {
            Edit::Move { view, .. } | Edit::Rotate { view, .. } => view,
        }
    }

    /// What to click next.
    fn prompt(self) -> &'static str {
        match self {
            Edit::Move { from: None, .. } => bimcraft_i18n::msg!("Move Underlay: click a point to move it from (Esc to stop)"),
            Edit::Move { from: Some(_), .. } => bimcraft_i18n::msg!("Move Underlay: click where that point goes"),
            Edit::Rotate { centre: None, .. } => bimcraft_i18n::msg!("Rotate Underlay: click the centre of rotation (Esc to stop)"),
            Edit::Rotate { start: None, .. } => bimcraft_i18n::msg!("Rotate Underlay: click a point on the line to turn from"),
            Edit::Rotate { .. } => bimcraft_i18n::msg!("Rotate Underlay: click a point on the line to turn to"),
        }
    }
}

/// The colours Underlay Settings offers at a click, besides any other.
const SWATCHES: [[u8; 3]; 8] =
    [core::COLOR, [220, 50, 47], [211, 54, 130], [108, 113, 196], [38, 139, 210], [42, 161, 152], [133, 153, 0], [128, 128, 128]];

impl BimApp {
    /// The view an underlay is shown under now: the plan, section or elevation on screen.
    pub(crate) fn underlay_target(&self) -> Option<Id> {
        match self.sheet {
            Sheet::Plan(id) | Sheet::Section(id) => core::kind(&self.session.model, id).map(|_| id),
            _ => None,
        }
    }

    /// Esc while Move or Rotate picks: it stops, and Underlay Settings comes back. Whether there was one to stop.
    pub(crate) fn cancel_underlay_edit(&mut self) -> bool {
        if self.underlay_edit.take().is_none() {
            return false;
        }
        self.status = Status::default();
        true
    }

    /// Show as Underlay: `source` under the view on screen.
    pub(crate) fn show_as_underlay(&mut self, source: Id) {
        if let Some(view) = self.underlay_target() {
            self.run("underlay", json!({"view": view, "source": source}));
        }
    }

    /// The underlay to draw in view `key` through `view` now, `hold` while the view's own drawing is still being made.
    pub(crate) fn underlay_now(&mut self, ctx: &Context, model: &Arc<Model>, key: Id, view: View, hold: bool) -> Option<Arc<Made>> {
        self.underlay.current(ctx, model, key, view, hold)
    }

    /// Properties' Underlay group, for the view on screen: on or off, of which view, and its settings.
    pub(crate) fn underlay_properties(&mut self, ui: &mut Ui) {
        let Some(view) = self.underlay_target() else { return };
        let model = Arc::clone(&self.session.model);
        let group = bimcraft_i18n::msg!("Underlay");
        let open = !self.collapsed.contains(group);
        if crate::properties::group_header(ui, t(group), open) && !self.collapsed.remove(group) {
            self.collapsed.insert(group);
        }
        if !open {
            return;
        }
        let u = core::of(&model, view).cloned();
        let (mut toggle, mut edit) = (None, false);
        crate::properties::grid(ui, "underlay-properties", |ui| {
            let label = ui.label(t("Show Underlay"));
            let mut on = u.as_ref().is_some_and(|u| u.on);
            let r = ui.checkbox(&mut on, "").labelled_by(label.id).on_hover_text(t("Show another view under this one"));
            r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, t("Show Underlay")));
            if r.changed() {
                toggle = Some(on);
            }
            ui.end_row();
            ui.label(t("Underlay View"));
            let name = match &u {
                Some(u) if core::missing(&model, view) => t("<Missing>").to_string(),
                Some(u) => model.view_name(u.view).unwrap_or_default().to_string(),
                None => t("<None>").to_string(),
            };
            ui.label(name);
            ui.end_row();
            ui.label(t("Underlay Settings"));
            let r = crate::properties::wide_button(ui, t("Edit…"));
            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, t("Edit Underlay Settings")));
            edit = r.clicked();
            ui.end_row();
        });
        match (toggle, &u) {
            (Some(on), Some(_)) => {
                self.run("underlay", json!({"view": view, "on": on}));
            }
            (Some(true), None) => match core::default_source(&model, view) {
                Some(source) => {
                    self.run("underlay", json!({"view": view, "source": source}));
                }
                // Nothing to start from (a section, or one level): the settings, to pick a view.
                None => edit = true,
            },
            _ => {}
        }
        if edit {
            self.underlay_dialog = Some(view);
        }
    }

    /// Underlay Settings, while it is open and no Move or Rotate is picking in the view.
    pub(crate) fn underlay_window(&mut self, ctx: &Context) {
        let Some(view) = self.underlay_dialog else { return };
        if self.underlay_edit.is_some() {
            return;
        }
        let model = Arc::clone(&self.session.model);
        if core::kind(&model, view).is_none() {
            self.underlay_dialog = None;
            return;
        }
        let u = core::of(&model, view).cloned();
        let title = tf("Underlay Settings: {}", &[model.view_name(view).unwrap_or_default()]);
        let mut set: Option<serde_json::Value> = None;
        let (mut close, mut pick) = (false, None);
        let r = Modal::new(egui::Id::new("underlay-settings"), title).width(420.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                egui::Grid::new("underlay-settings-grid").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
                    let label = ui.label(t("Show Underlay"));
                    let mut on = u.as_ref().is_some_and(|u| u.on);
                    let r = ui.add_enabled(u.is_some(), egui::Checkbox::without_text(&mut on)).labelled_by(label.id);
                    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, u.is_some(), on, t("Show Underlay")));
                    if r.changed() {
                        set = Some(json!({"on": on}));
                    }
                    ui.end_row();
                    ui.label(t("View"));
                    let now = match &u {
                        Some(_) if core::missing(&model, view) => t("<Missing>").to_string(),
                        Some(u) => model.view_name(u.view).unwrap_or_default().to_string(),
                        None => t("<None>").to_string(),
                    };
                    egui::ComboBox::from_id_salt("underlay-source").selected_text(now).width(240.0).show_ui(ui, |ui| {
                        for id in core::candidates(&model, view) {
                            let name = model.view_name(id).unwrap_or_default();
                            let kind = match core::kind(&model, id) {
                                Some(Kind::Plan) => t("Floor Plan"),
                                _ if matches!(model.elements.get(id), Some(bimcraft_core::model::Element::Elevation(_))) => t("Elevation"),
                                _ => t("Section"),
                            };
                            let chosen = u.as_ref().is_some_and(|u| model.elements.id(u.view) == Some(id));
                            if ui.selectable_label(chosen, format!("{name}  ({kind})")).clicked() {
                                set = Some(json!({"source": id}));
                            }
                        }
                    });
                    ui.end_row();
                    let Some(u) = &u else { return };
                    ui.label(t("Color"));
                    ui.horizontal(|ui| {
                        for c in SWATCHES {
                            let (rect, r) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::click());
                            ui.painter().rect_filled(rect, 3.0, Color32::from_rgb(c[0], c[1], c[2]));
                            if u.color == c {
                                ui.painter().rect_stroke(
                                    rect.expand(2.0),
                                    4.0,
                                    Stroke::new(1.5, crate::style::pal().text),
                                    egui::StrokeKind::Outside,
                                );
                            }
                            let hex = format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
                            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &hex));
                            if r.clicked() {
                                set = Some(json!({"color": c}));
                            }
                        }
                        let mut c = self.underlay_color.unwrap_or(u.color);
                        if egui::color_picker::color_edit_button_srgb(ui, &mut c).changed() {
                            self.underlay_color = Some(c);
                        }
                    });
                    ui.end_row();
                    ui.label(t("Fills"));
                    let mut fills = u.fills;
                    if ui.checkbox(&mut fills, t("Show fills and patterns")).changed() {
                        set = Some(json!({"fills": fills}));
                    }
                    ui.end_row();
                    ui.label(t("Annotation"));
                    let mut hide = u.hide_annotation;
                    if ui.checkbox(&mut hide, t("Hide annotation")).changed() {
                        set = Some(json!({"hide_annotation": hide}));
                    }
                    ui.end_row();
                    ui.label(t("Draw"));
                    ui.horizontal(|ui| {
                        if ui.radio(!u.front, t("Behind the view")).clicked() && u.front {
                            set = Some(json!({"front": false}));
                        }
                        if ui.radio(u.front, t("In front of the view")).clicked() && !u.front {
                            set = Some(json!({"front": true}));
                        }
                    });
                    ui.end_row();
                    ui.label(t("Position"));
                    ui.horizontal(|ui| {
                        if ui.button(t("Move")).on_hover_text(t("Move the underlay by two points picked in the view")).clicked() {
                            pick = Some(Edit::Move { view, from: None });
                        }
                        if ui.button(t("Rotate")).on_hover_text(t("Turn the underlay about a point picked in the view")).clicked() {
                            pick = Some(Edit::Rotate { view, centre: None, start: None });
                        }
                        if ui.add_enabled(!u.placed(), egui::Button::new(t("Reset Position"))).clicked() {
                            set = Some(json!({"reset": true}));
                        }
                    });
                    ui.end_row();
                    if !u.placed() {
                        ui.label("");
                        let (x, y) = (model.units.length(u.offset.0), model.units.length(u.offset.1));
                        let angle = format!("{}", (u.angle * 100.0).round() / 100.0);
                        ui.weak(tf("Moved {}, {}; turned {}°", &[&x, &y, &angle]));
                        ui.end_row();
                    }
                });
                if let Some(error) = self.underlay_error.as_ref() {
                    ui.colored_label(crate::style::pal().error, error);
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    close = ui.button(t("Close")).clicked();
                });
            });
        });
        // A colour picked is set once the hand lets go, as one step to undo.
        if let Some(c) = self.underlay_color
            && !ctx.input(|i| i.pointer.any_down())
        {
            self.underlay_color = None;
            if u.as_ref().is_some_and(|u| u.color != c) {
                set = Some(json!({"color": c}));
            }
        }
        if let Some(mut params) = set {
            params["view"] = json!(view);
            self.underlay_error = self.exec("underlay", &params).err();
        }
        if let Some(edit) = pick {
            self.underlay_edit = Some(edit);
            self.status = Status { text: t(edit.prompt()).into(), error: false };
        }
        if close || r.should_close() {
            self.underlay_dialog = None;
            self.underlay_error = None;
        }
    }

    /// While Move or Rotate picks in view `key`: its clicks, its preview over the canvas and Esc. Whether it is picking, when
    /// the view's own clicks are not to be taken.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn underlay_pick(
        &mut self,
        ui: &Ui,
        response: &Response,
        p: &Painter,
        rect: Rect,
        view: &View,
        key: Id,
        level: Option<Id>,
    ) -> bool {
        let Some(edit) = self.underlay_edit.filter(|e| e.view() == key) else { return false };
        let model = Arc::clone(&self.session.model);
        let local = response.hover_pos().map(|q| (q.x - rect.min.x, q.y - rect.min.y));
        // Plans snap as their tools do, to the underlay too; a section's points round to the millimetre.
        let land = |app: &BimApp, q: (f32, f32), from: &[P2]| match level {
            Some(level) => app.snap_point(&model, level, view, q, from, false),
            None => {
                let reach = bimcraft_core::snap::Reach {
                    point: crate::canvas::SNAP / view.scale,
                    line: f64::from(crate::canvas::PICK) / view.scale,
                };
                let found =
                    bimcraft_core::snap::among(&[], app.underlay.segments(key), &[], view.mm(q), reach, from, false, &app.snaps_now());
                found.at
            }
        };
        let screen = |q: P2| {
            let (x, y) = view.px(q);
            rect.min + vec2(x, y)
        };
        let ink = Stroke::new(1.5, crate::style::canvas_pal().accent);
        let thin = Stroke::new(1.0, crate::style::canvas_pal().accent);
        if let Some(q) = local {
            let at = land(self, q, &[]);
            match edit {
                Edit::Move { from: Some(from), .. } => {
                    let to = land(self, q, &[from]);
                    p.line_segment([screen(from), screen(to)], ink);
                    p.circle_stroke(screen(to), 4.0, ink);
                    // The underlay where it would go.
                    if let Some(made) = self.underlay.shown.clone().filter(|m| m.target == key) {
                        let (fx, fy) = view.px(from);
                        let (tx, ty) = view.px(to);
                        let pipeline = self.card.canvas();
                        self.underlay.paint(ui.ctx(), p, pipeline.as_ref(), rect.min, *view, &made, vec2(tx - fx, ty - fy), self.thin);
                    }
                }
                Edit::Rotate { centre: Some(c), start, .. } => {
                    let cs = screen(c);
                    p.circle_stroke(cs, 5.0, ink);
                    p.line_segment([cs - vec2(8.0, 0.0), cs + vec2(8.0, 0.0)], thin);
                    p.line_segment([cs - vec2(0.0, 8.0), cs + vec2(0.0, 8.0)], thin);
                    let to = land(self, q, &[c]);
                    if let Some(from) = start {
                        p.line_segment([cs, screen(from)], thin);
                        p.line_segment([cs, screen(to)], ink);
                        let angle = angle_between(c, from, to, 0.0);
                        let tip = screen(to) + vec2(12.0, -12.0);
                        p.text(
                            tip,
                            egui::Align2::LEFT_BOTTOM,
                            format!("{}°", (angle * 100.0).round() / 100.0),
                            egui::FontId::proportional(13.0),
                            crate::style::canvas_pal().accent,
                        );
                    } else {
                        p.line_segment([cs, screen(to)], thin);
                    }
                }
                _ => {
                    p.circle_stroke(screen(at), 4.0, ink);
                }
            }
        }
        if let Some(at) = self.clicked_at(response) {
            let q = (at.x - rect.min.x, at.y - rect.min.y);
            let next = match edit {
                Edit::Move { view: v, from: None } => Some(Edit::Move { view: v, from: Some(land(self, q, &[])) }),
                Edit::Move { from: Some(from), .. } => {
                    let to = land(self, q, &[from]);
                    self.run("underlay", json!({"view": key, "move": [to.0 - from.0, to.1 - from.1]}));
                    None
                }
                Edit::Rotate { view: v, centre: None, .. } => Some(Edit::Rotate { view: v, centre: Some(land(self, q, &[])), start: None }),
                Edit::Rotate { view: v, centre: Some(c), start: None } => {
                    Some(Edit::Rotate { view: v, centre: Some(c), start: Some(land(self, q, &[c])) })
                }
                Edit::Rotate { centre: Some(c), start: Some(from), .. } => {
                    let to = land(self, q, &[c]);
                    let angle = angle_between(c, from, to, 0.0);
                    self.run("underlay", json!({"view": key, "rotate": angle, "about": [c.0, c.1]}));
                    None
                }
            };
            self.underlay_edit = next;
            match next {
                Some(e) => self.status = Status { text: t(e.prompt()).into(), error: false },
                None if !self.status.error => self.status = Status::default(),
                None => {}
            }
        }
        true
    }

    /// The underlay of view `key`, drawn through `p` if it is to be on the side `front` says.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_underlay(&mut self, ctx: &Context, p: &Painter, rect: Rect, view: View, made: Option<&Arc<Made>>, front: bool) {
        let Some(made) = made.filter(|m| m.front() == front) else { return };
        // While Move picks its second point, the underlay is drawn where it would go instead.
        if matches!(self.underlay_edit, Some(Edit::Move { from: Some(_), view }) if view == made.target) {
            return;
        }
        let pipeline = self.card.canvas();
        self.underlay.paint(ctx, p, pipeline.as_ref(), rect.min, view, made, vec2(0.0, 0.0), self.thin);
    }
}
