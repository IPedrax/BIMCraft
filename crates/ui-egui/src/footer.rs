//! The view control bar under a view, as Revit's has it: the view's Scale and Detail Level, or the 3D view's Visual Style, which
//! the project keeps (the `scale`, `detail` and `visual_style` commands), then Temporary Hide/Isolate and Reveal Hidden Elements. What is hidden or revealed here is
//! the window's, per view: it isn't in the project, saved or undone. The view draws a copy of the model with its graphics
//! changed ([`BimApp::view_model`]), made once for each model and each setting, through the same `graphics` command the
//! project's own changes go through; Apply to View makes the same change for good.

use bimcraft_i18n::msg;

use std::collections::BTreeSet;
use std::sync::Arc;

use bimcraft_core::engine::Session;
use bimcraft_core::graphics::{self, ANNOTATION, Detail, Group, MODEL, VisualStyle};
use bimcraft_core::model::{Id, Model};
use egui::{
    Align, Color32, FontId, Frame, Key, Layout, Margin, Popup, Response, RichText, Sense, Shape, Stroke, StrokeKind, TextEdit, Ui,
    UiBuilder, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::{Value, json};

use crate::graphics::controlled_note;
use crate::icons::Icon;
use crate::lang::{t, tf};
use crate::style::pal;
use crate::{BimApp, Sheet, Status, small_button};

/// What Revit draws round a view that is hiding or isolating for now.
const FRAME: Color32 = Color32::from_rgb(0, 190, 220);

/// What a view hides or reveals for now.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Temp {
    elements: BTreeSet<Id>,
    categories: BTreeSet<String>,
    /// What the view hides for good is drawn instead, in magenta.
    reveal: bool,
}

impl Temp {
    fn hides(&self) -> bool {
        !self.elements.is_empty() || !self.categories.is_empty()
    }
}

/// The model a view draws while it hides or reveals something for now, and what it was made from.
pub(crate) struct Shadow {
    base: Arc<Model>,
    sheet: Sheet,
    temp: Temp,
    model: Arc<Model>,
}

/// A button of the bar that opens a menu: named `label` for screen readers, and showing `shown`, the setting it holds. Greyed
/// out, and saying so, when view template `set_by` sets it.
fn menu_button(ui: &mut Ui, label: &str, shown: &str, tip: &str, set_by: Option<&str>) -> Response {
    let (label, tip) = (t(label), t(tip));
    let galley = ui.painter().layout_no_wrap(t(shown).to_string(), FontId::proportional(12.0), pal().text);
    ui.add_enabled_ui(set_by.is_none(), |ui| {
        let (rect, r) = ui.allocate_exact_size(vec2(galley.size().x + 28.0, 25.0), Sense::click());
        let enabled = ui.is_enabled();
        r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
        let p = ui.painter();
        if r.hovered() {
            p.rect_filled(rect, 4.0, pal().hover_fill);
        }
        if r.has_focus() {
            p.rect_stroke(rect, 4.0, crate::style::focus_ring(), StrokeKind::Inside);
        }
        let ink = if enabled { pal().text } else { pal().muted };
        p.galley(pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0), galley, ink);
        let c = pos2(rect.right() - 10.0, rect.center().y);
        p.add(Shape::convex_polygon(vec![c + vec2(-3.5, -2.0), c + vec2(3.5, -2.0), c + vec2(0.0, 2.5)], pal().icon, Stroke::NONE));
        match set_by {
            Some(template) => r.on_disabled_hover_text(controlled_note(template)),
            None => r.on_hover_text(tip),
        }
    })
    .inner
}

/// What Reveal Hidden Elements draws what a view hides in.
const REVEALED: &str = "#FF00FF";

/// What High Contrast fills a view's point clouds with until the user picks another (Settings > Graphics): a crimson that reads
/// on white paper and on the dark canvas, and apart from the model's black lines.
pub(crate) const HIGH_CONTRAST: [u8; 3] = [220, 20, 60];

/// `base` with the graphics of the view `sheet` changed as `temp` says. `three_d` is the 3D view on screen (a camera view
/// by id, or none for the default `{3D}`).
fn shadowed(base: &Arc<Model>, sheet: Sheet, three_d: Option<Id>, temp: &Temp) -> Result<Arc<Model>, String> {
    let (view, id) = match sheet {
        // A dependent view hides what its parent does.
        Sheet::Plan(l) | Sheet::Ceiling(l) => {
            let source = base.elements.id(base.plan_source(l)).unwrap_or(l);
            (json!(source), Some(source))
        }
        Sheet::Section(s) => (json!(s), Some(s)),
        Sheet::View3D => (three_d.map_or_else(|| json!("{3D}"), |id| json!(id)), three_d.or_else(|| base.default_3d())),
        Sheet::Paper(_) | Sheet::Schedule(_) | Sheet::Table(_) => return Ok(Arc::clone(base)),
    };
    let drawn = |id: &Id| base.elements.get(id).is_some_and(|e| graphics::drawn(e.category()));
    let mut p = json!({
        "view": view,
        "hide_elements": temp.elements.iter().copied().filter(drawn).collect::<Vec<_>>(),
    });
    // An empty list is still asking for categories, which a view template that controls them refuses.
    if !temp.categories.is_empty() {
        p["hide"] = json!(temp.categories);
    }
    if temp.reveal
        && let Some(own) = id.and_then(|id| graphics::graphics_of(base, id))
    {
        let g = own.resolved(base);
        let cats: Vec<&str> = g.categories.iter().filter(|(_, o)| o.hidden).map(|(c, _)| c.as_str()).collect();
        let els: Vec<Id> =
            g.elements.iter().filter(|(_, o)| o.hidden).filter_map(|(g, _)| base.elements.id(g)).filter(|i| drawn(i)).collect();
        // Each of them, and each element of a category hidden, in magenta.
        let paint = json!({"projection": {"color": REVEALED}, "cut": {"color": REVEALED}});
        let by_category = base.elements.iter().filter(|(_, e)| cats.contains(&e.category())).map(|(i, _)| *i);
        if !cats.is_empty() {
            p["show"] = json!(cats);
        }
        p["show_elements"] = json!(els);
        p["elements"] =
            els.iter().copied().chain(by_category).map(|i| (i.to_string(), paint.clone())).collect::<serde_json::Map<_, _>>().into();
    }
    let mut s = Session::new();
    s.reset((**base).clone(), None);
    s.execute("graphics", &p)?;
    Ok(s.model)
}

impl BimApp {
    /// What the active view hides or reveals for now.
    pub(crate) fn temporary(&self) -> Option<&Temp> {
        self.temporary.get(&self.sheet)
    }

    /// The model the active view draws: the project's, but for what it hides or reveals for now. A setting the project's
    /// graphics refuse (categories that follow a view template) ends it, and says why.
    pub(crate) fn view_model(&mut self) -> Arc<Model> {
        let base = self.linked_model();
        let Some(temp) = self.temporary().filter(|t| t.hides() || t.reveal).cloned() else { return base };
        if let Some(s) = &self.shadow
            && s.sheet == self.sheet
            && s.temp == temp
            && Arc::ptr_eq(&s.base, &base)
        {
            return Arc::clone(&s.model);
        }
        match shadowed(&base, self.sheet, self.active_3d(), &temp) {
            Ok(model) => {
                self.shadow = Some(Shadow { base, sheet: self.sheet, temp, model: Arc::clone(&model) });
                model
            }
            Err(e) => {
                self.temporary.remove(&self.sheet);
                self.status = Status { text: e, error: true };
                base
            }
        }
    }

    /// The project with what its links build, which its views draw and snap to; the project itself where it links nothing.
    /// Made again only when the project changes.
    fn linked_model(&mut self) -> Arc<Model> {
        let model = Arc::clone(&self.session.model);
        if let Some((from, seen)) = &self.seen
            && Arc::ptr_eq(from, &model)
        {
            return Arc::clone(seen);
        }
        let seen = bimcraft_core::link::seen(&model);
        self.seen = Some((model, Arc::clone(&seen)));
        seen
    }

    /// Temporary Hide Element (HH), or with `category`, Hide Category (HC), of what is selected.
    pub(crate) fn temp_hide(&mut self, category: bool) {
        let Some(picked) = self.temp_picked() else { return };
        let t = self.temporary.entry(self.sheet).or_default();
        match category {
            true => t.categories.extend(picked.1.into_iter().map(String::from)),
            false => t.elements.extend(picked.0),
        }
        self.selection.clear();
    }

    /// Temporary Isolate Element (HI), or with `category`, Isolate Category (IC): all but what is selected goes.
    pub(crate) fn temp_isolate(&mut self, category: bool) {
        let Some((elements, categories)) = self.temp_picked() else { return };
        let m = &self.session.model;
        let t = self.temporary.entry(self.sheet).or_default();
        let kept = |id: &Id| !elements.contains(id);
        (t.elements, t.categories) = match category {
            true => {
                (BTreeSet::new(), MODEL.iter().chain(&ANNOTATION).filter(|c| !categories.contains(**c)).map(|c| c.to_string()).collect())
            }
            false => (
                m.elements.iter().filter(|(id, e)| graphics::drawn(e.category()) && kept(id)).map(|(id, _)| *id).collect(),
                BTreeSet::new(),
            ),
        };
        self.selection.clear();
    }

    /// What is selected, and its categories, when the view has graphics and something drawn is selected; else says why not.
    fn temp_picked(&mut self) -> Option<(BTreeSet<Id>, BTreeSet<&'static str>)> {
        let m = Arc::clone(&self.session.model);
        let elements: BTreeSet<Id> =
            self.selection.iter().copied().filter(|id| m.elements.get(id).is_some_and(|e| graphics::drawn(e.category()))).collect();
        if self.graphic_view().is_none() {
            self.status = Status { text: t("This view has no graphics to override.").to_string(), error: true };
            return None;
        }
        if elements.is_empty() {
            self.status = Status { text: t("select what to hide or isolate first").into(), error: true };
            return None;
        }
        let categories = elements.iter().filter_map(|id| m.elements.get(id)).map(|e| e.category()).collect();
        Some((elements, categories))
    }

    /// Reset Temporary Hide/Isolate (HR): what the view hid or isolated for now comes back, and Reveal Hidden Elements goes off.
    pub(crate) fn temp_reset(&mut self) {
        self.temporary.remove(&self.sheet);
    }

    /// Reveal Hidden Elements (RH): what the view hides for good is drawn in magenta, or hidden again.
    pub(crate) fn temp_reveal(&mut self) {
        if self.graphic_view().is_some() {
            let t = self.temporary.entry(self.sheet).or_default();
            t.reveal = !t.reveal;
        }
    }

    /// Apply Hide/Isolate to View: what the view hides for now, hidden in it for good.
    fn temp_apply(&mut self) {
        let (Some(view), Some(t)) = (self.graphic_view(), self.temporary().cloned()) else { return };
        let mut p: Value = json!({"view": view, "hide_elements": t.elements.iter().copied().collect::<Vec<_>>()});
        if !t.categories.is_empty() {
            p["hide"] = json!(t.categories);
        }
        if self.run("graphics", p).is_some() {
            self.temporary.remove(&self.sheet);
        }
    }

    /// High Contrast Point Clouds: the view fills its point clouds with the user's colour ([`HIGH_CONTRAST`] at first), or,
    /// when it fills them with any colour, gives them their own back; the rest of the category's override stays. One `graphics`
    /// command, one undo step.
    pub(crate) fn toggle_scan_contrast(&mut self) {
        let Some(view) = self.graphic_view() else { return };
        let filled = self.scans_filled();
        // The command merges what it's given into the override: a null fill takes it away.
        let fill = if filled { Value::Null } else { json!(self.scan_contrast) };
        self.run("graphics", json!({"view": view, "categories": {graphics::SCANS[0]: {"projection": {"fill": fill}}}}));
    }

    /// Whether the view fills its point clouds with a colour, whoever set it. That is the category's override, which a scan's own
    /// (or a view filter's) comes after: a scan with its own colour is lit here and keeps it.
    fn scans_filled(&self) -> bool {
        self.graphics_shown().categories.get(graphics::SCANS[0]).is_some_and(|o| o.projection.fill.is_some())
    }

    /// The Scale and Detail Level menus of a plan, a ceiling plan, a section or an elevation, `view`: what the view keeps.
    fn scale_and_detail(&mut self, ui: &mut Ui, view: &Value) {
        let now = self.graphics_shown();
        let units = self.session.model.units;
        let n_now = now.scale_n(units);
        let shown = units.scale(n_now);
        let (set_by, tip) = (
            self.controlled_by(Group::Scale),
            msg!("Scale: the paper this view stands for, 1 : n of the model: its annotation and its line weights are that paper's"),
        );
        let r = menu_button(ui, msg!("Scale"), &shown, tip, set_by.as_deref());
        Popup::menu(&r).show(|ui| {
            // Wide enough for the Custom row's field.
            ui.set_min_width(150.0);
            for &n in units.scales() {
                if ui.selectable_label(n_now == n, units.scale(n)).clicked() {
                    self.run("scale", json!({"view": view, "scale": n}));
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                // A menu draws its widgets bare: the field gets its box back.
                ui.visuals_mut().extreme_bg_color = pal().surface;
                ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(1.0, pal().border);
                let l = ui.label(t("Custom"));
                let edit = ui.add(TextEdit::singleline(&mut self.scale_text).desired_width(96.0)).labelled_by(l.id);
                if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    match units.parse_scale(&self.scale_text) {
                        Some(n) => {
                            self.run("scale", json!({"view": view, "scale": n}));
                        }
                        None => self.status = Status { text: tf("`{}` is not a scale", &[self.scale_text.trim()]), error: true },
                    }
                    ui.close();
                }
            });
        });
        let tip = msg!(
            "Detail Level: Coarse draws what is cut in one poché, Medium adds the layers and the symbols of openings, Fine the patterns"
        );
        let r = menu_button(ui, "Detail Level", now.detail.name(), tip, self.controlled_by(Group::Detail).as_deref());
        Popup::menu(&r).show(|ui| {
            for d in Detail::ALL {
                if ui.selectable_label(now.detail == d, t(d.name())).clicked() {
                    self.run("detail", json!({"view": view, "detail": d.name()}));
                }
            }
        });
        ui.separator();
    }

    /// Visual Style (WF, HL, SD): how the 3D view draws its surfaces; in another view, says there is none to set.
    pub(crate) fn set_visual_style(&mut self, style: VisualStyle) {
        if self.sheet == Sheet::View3D {
            let view = self.active_3d().map_or_else(|| json!("{3D}"), |id| json!(id));
            self.run("visual_style", json!({"view": view, "style": style.name()}));
        } else {
            self.status = Status { text: t("visual styles are for the 3D view: open it from the Project Browser").into(), error: true };
        }
    }

    /// The bar, when the view has one: a plan, a ceiling plan, a section or an elevation, the 3D view.
    pub(crate) fn view_control_bar(&mut self, ui: &mut Ui) {
        if self.graphic_view().is_none() {
            return;
        }
        let temp = self.temporary().cloned().unwrap_or_default();
        // A small card floating over the view's bottom left corner, the view drawn under it; a bar along its foot in the
        // Compact layout, the view above it.
        let card = crate::style::card([true, false, false, true]).inner_margin(Margin::symmetric(6, 2));
        let compact = crate::style::compact();
        let (panel, card) = match compact {
            true => (card.outer_margin(0), Frame::NONE),
            false => (Frame::NONE, card),
        };
        let (id, over) = (egui::Id::new(("view-control", self.sheet)), self.view_rect);
        let mut controls = |ui: &mut Ui| {
            card.show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    match (self.graphic_view(), self.sheet == Sheet::View3D) {
                        (Some(view), false) => self.scale_and_detail(ui, &view),
                        (_, true) => {
                            let now = self.graphics_shown().style;
                            let tip = msg!(
                                "Visual Style: how the 3D view draws surfaces: Shaded, Hidden Line (WF, HL and SD set them) or Wireframe"
                            );
                            let r = menu_button(ui, "Visual Style", now.name(), tip, self.controlled_by(Group::Style).as_deref());
                            Popup::menu(&r).show(|ui| {
                                for v in VisualStyle::ALL {
                                    if ui.selectable_label(now == v, t(v.name())).clicked() {
                                        self.set_visual_style(v);
                                    }
                                }
                            });
                            ui.separator();
                        }
                        _ => {}
                    }
                    let tip = msg!(
                        "Temporary Hide/Isolate: hide or isolate what is selected, or its categories, in this view until you reset it"
                    );
                    let r = small_button(ui, Icon::TempHide, "Temporary Hide/Isolate", temp.hides(), tip);
                    // Revit greys out what needs a selection until there is one.
                    let picked = !self.drawn_selection(&self.session.model).is_empty();
                    Popup::menu(&r).show(|ui| {
                        for (label, hide, category) in [
                            (msg!("Hide Element (HH)"), true, false),
                            (msg!("Hide Category (HC)"), true, true),
                            (msg!("Isolate Element (HI)"), false, false),
                            (msg!("Isolate Category (IC)"), false, true),
                        ] {
                            if ui.add_enabled(picked, egui::Button::new(t(label))).clicked() {
                                match hide {
                                    true => self.temp_hide(category),
                                    false => self.temp_isolate(category),
                                }
                            }
                        }
                        ui.separator();
                        if ui.add_enabled(temp.hides(), egui::Button::new(t("Apply Hide/Isolate to View"))).clicked() {
                            self.temp_apply();
                        }
                        if ui.add_enabled(temp.hides() || temp.reveal, egui::Button::new(t("Reset Temporary Hide/Isolate (HR)"))).clicked()
                        {
                            self.temp_reset();
                        }
                    });
                    let tip = msg!("Reveal Hidden Elements (RH): draw what this view hides in magenta; again to hide it");
                    if small_button(ui, Icon::Reveal, "Reveal Hidden Elements", temp.reveal, tip).clicked() {
                        self.temp_reveal();
                    }
                    // Only where there is a scan to show: the project's or a link's. A command drops a scan nothing places, so one held
                    // is one placed.
                    if !self.linked_model().clouds.is_empty() {
                        let filled = self.scans_filled();
                        let set_by = self.controlled_by(Group::Categories);
                        let tip = msg!(
                            "High Contrast Point Clouds: draw this view's scans in one strong colour, so what they show reads; \
                             again for their own colours. A scan given a colour of its own in Visibility/Graphics keeps it."
                        );
                        let r = ui
                            .add_enabled_ui(set_by.is_none(), |ui| {
                                small_button(ui, Icon::PointCloud, "High Contrast Point Clouds", filled, tip)
                            })
                            .inner;
                        let r = match set_by {
                            Some(template) => r.on_disabled_hover_text(controlled_note(&template)),
                            None => r,
                        };
                        if r.clicked() {
                            self.toggle_scan_contrast();
                        }
                    }
                    if temp.hides() || temp.reveal {
                        ui.add_space(8.0);
                        let note = if temp.reveal && !temp.hides() { msg!("Revealing hidden elements") } else { "Temporary Hide/Isolate" };
                        ui.label(RichText::new(tf("{} (HR resets)", &[t(note)])).color(FRAME));
                    }
                })
            });
        };
        if compact {
            egui::Panel::bottom(id).frame(panel).show_separator_line(false).show(ui, controls);
        } else {
            // A child that takes no room of the view's: over it, not after it.
            controls(&mut ui.new_child(UiBuilder::new().id_salt(id).max_rect(over).layout(Layout::bottom_up(Align::Min))));
        }
    }

    /// The cyan frame round a view that hides, isolates or reveals for now, as Revit draws.
    pub(crate) fn temp_frame(&self, ui: &Ui) {
        if self.temporary().is_some_and(|t| t.hides() || t.reveal) {
            ui.painter().rect_stroke(self.view_rect.shrink(1.5), 0.0, Stroke::new(3.0, FRAME), StrokeKind::Inside);
        }
    }
}
