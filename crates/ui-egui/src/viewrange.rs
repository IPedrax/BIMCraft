//! View Range, as Revit's dialog: a plan's four planes, each a level it is measured from and an offset over it, under a
//! picture of what each plane does (`assets/help/view-range.jpg`). The Properties palette's View Range row (Edit…) opens it
//! on the plans it shows, and a view template's View Range row on the template's draft. One plan's planes are measured from
//! levels by name (its own for the cut plane), or Unlimited; plans picked together, and a template, from the level of each
//! plan (Associated Level, the level above or below), or Unlimited. OK and Apply run `modify` on the plans; the core keeps the
//! planes in order and says which is out of place against which.

use bimcraft_core::model::{Element, Guid, Id, Model};
use bimcraft_core::view_range::{Base, Plane, ViewRange};
use bimcraft_i18n::msg;
use egui::{Align, Button, ComboBox, Context, Id as UiId, Layout, RichText, TextEdit, TextureHandle, Ui, vec2};
use serde_json::{Value, json};

use crate::BimApp;
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::pal;

/// The picture the dialog shows: the four planes in a section through a house, and what each does.
const PICTURE: &[u8] = include_bytes!("../../../assets/help/view-range.jpg");
/// How wide the picture is shown, points.
const PICTURE_W: f32 = 700.0;

/// One plane as the form has it: what it is measured from, and its offset as typed.
#[derive(Clone, PartialEq)]
struct Row {
    base: Base,
    offset: String,
}

/// What the planes are measured from in a form.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Mode {
    /// One plan, of this level: levels by name, or Unlimited.
    Plan(Guid),
    /// Plans picked together, or a template: the level of each plan, the one above or below it, or Unlimited.
    Relative,
}

/// The four planes as typed, until OK; and the picture, once loaded.
#[derive(Clone)]
pub(crate) struct RangeForm {
    rows: [Row; 4],
    mode: Mode,
    picture: Option<TextureHandle>,
}

/// `r` as `modify` and `template` take it: every field, so what it merges into takes each base as it is.
pub(crate) fn range_json(r: &ViewRange) -> Value {
    let mut v = serde_json::to_value(r).unwrap_or_default();
    if let Some(o) = v.as_object_mut() {
        for key in ["top_from", "bottom_from", "depth_from"] {
            o.entry(key).or_insert(json!("associated"));
        }
        o.entry("top").or_insert(Value::Null);
    }
    v
}

/// What a plane may be measured from, in the order the list shows them. One plan's: every level by name, low to high, then
/// Unlimited (the cut plane its own level alone). Else: the plan's own level, the next one up (the top) or down (the bottom
/// and the view depth), and Unlimited.
fn bases(m: &Model, p: Plane, mode: Mode) -> Vec<Base> {
    match (mode, p) {
        (Mode::Plan(level), Plane::Cut) => vec![Base::Level(level)],
        (Mode::Plan(_), _) => {
            m.levels().into_iter().map(|(id, _)| Base::Level(m.guid(id))).chain(std::iter::once(Base::Unlimited)).collect()
        }
        (Mode::Relative, Plane::Cut) => vec![Base::Associated],
        (Mode::Relative, Plane::Top) => vec![Base::Associated, Base::Above, Base::Unlimited],
        (Mode::Relative, _) => vec![Base::Associated, Base::Below, Base::Unlimited],
    }
}

/// `b` in a plan of `level` as a level by name: its own, the one above or below it (none there, Unlimited).
fn specific(m: &Model, level: Guid, b: Base) -> Base {
    let named = |g: Option<Guid>| g.map_or(Base::Unlimited, Base::Level);
    match b {
        Base::Associated => Base::Level(level),
        Base::Above => named(m.level_above(level)),
        Base::Below => named(m.level_below(level)),
        other => other,
    }
}

/// What the list calls `b`.
fn base_name(m: &Model, b: Base) -> String {
    match b {
        Base::Associated => t("Associated Level").to_string(),
        Base::Above => t("Level Above").to_string(),
        Base::Below => t("Level Below").to_string(),
        Base::Unlimited => t("Unlimited").to_string(),
        Base::Level(g) => m.level(g).map_or_else(|| t("<None>").to_string(), |l| l.name.clone()),
    }
}

impl RangeForm {
    /// The form for range `r`, its planes measured as `mode` says: one plan's from its levels by name.
    pub(crate) fn new(m: &Model, r: &ViewRange, mode: Mode) -> RangeForm {
        let row = |p: Plane| {
            let (base, offset) = r.plane(p);
            let base = match mode {
                Mode::Plan(level) => specific(m, level, base),
                Mode::Relative => base,
            };
            Row { base, offset: m.field(offset) }
        };
        RangeForm { rows: Plane::ALL.map(row), mode, picture: None }
    }

    /// The range as typed, or what is wrong with it.
    pub(crate) fn range(&self, m: &Model) -> Result<ViewRange, String> {
        let mut r = ViewRange::default();
        for (p, row) in Plane::ALL.into_iter().zip(&self.rows) {
            let offset = match row.base {
                Base::Unlimited => 0.0,
                _ if row.offset.trim().is_empty() => 0.0,
                _ => m.units.parse(&row.offset).ok_or_else(|| tf("{}: the offset is a length", &[t(label(p))]).to_string())?,
            };
            // A plan's own level by name is its associated level, as files have always held it.
            let base = match self.mode {
                Mode::Plan(level) if row.base == Base::Level(level) => Base::Associated,
                _ => row.base,
            };
            r = r.with(p, base, offset);
        }
        Ok(r)
    }

    /// The picture, then the planes: the primary range (Top, Cut Plane, Bottom) and the View Depth, each a list of what it is
    /// measured from and its offset.
    pub(crate) fn show(&mut self, ui: &mut Ui, m: &Model) {
        let picture = self.picture.get_or_insert_with(|| load_picture(ui.ctx()));
        let size = picture.size_vec2();
        let shown = vec2(PICTURE_W, PICTURE_W * size.y / size.x.max(1.0));
        ui.vertical_centered(|ui| {
            ui.add(egui::Image::new((picture.id(), shown)).corner_radius(4.0))
                .on_hover_text(t("Top, Cut Plane, Bottom and View Depth, in a section through a house"));
        });
        ui.add_space(8.0);
        let mode = self.mode;
        egui::Grid::new("view-range-planes").num_columns(4).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
            for (i, p) in Plane::ALL.into_iter().enumerate() {
                // The primary range, then, apart, how far past it the plan sees.
                if p == Plane::Depth {
                    ui.allocate_space(vec2(0.0, 2.0));
                    ui.end_row();
                }
                let row = &mut self.rows[i];
                let name = ui.label(t(label(p)));
                let choices = bases(m, p, mode);
                let c = ui.add_enabled_ui(choices.len() > 1, |ui| {
                    ComboBox::from_id_salt(("view-range-base", i)).width(190.0).selected_text(base_name(m, row.base)).show_ui(ui, |ui| {
                        for b in choices {
                            ui.selectable_value(&mut row.base, b, base_name(m, b));
                        }
                    })
                });
                c.inner.response.labelled_by(name.id);
                ui.label(t("Offset"));
                let unlimited = row.base == Base::Unlimited;
                // Its list and its offset both go by the plane's name, a list and a field.
                ui.add_enabled(!unlimited, TextEdit::singleline(&mut row.offset).desired_width(110.0))
                    .labelled_by(name.id)
                    .on_hover_text(tf("{}: how far over the level it is measured from", &[t(label(p))]));
                ui.end_row();
            }
        });
        ui.add_space(4.0);
        let note = match mode {
            Mode::Plan(_) => t("The cut plane is always over the plan's own level. Top ≥ Cut Plane ≥ Bottom ≥ View Depth."),
            Mode::Relative => {
                t("Measured from the level of each plan: Associated Level is its own. Top ≥ Cut Plane ≥ Bottom ≥ View Depth.")
            }
        };
        ui.label(RichText::new(note).color(pal().muted).small());
    }
}

/// What the dialog calls plane `p`.
fn label(p: Plane) -> &'static str {
    match p {
        Plane::Top => msg!("Top"),
        Plane::Cut => msg!("Cut Plane"),
        Plane::Bottom => msg!("Bottom"),
        Plane::Depth => msg!("View Depth"),
    }
}

/// The picture, as a texture.
fn load_picture(ctx: &Context) -> TextureHandle {
    let image = bimcraft_core::texture::Picture::import("view-range", PICTURE)
        .map(|p| {
            let (w, h) = p.size();
            egui::ColorImage::from_rgba_unmultiplied([w, h], &p.rgba(w as u32, h as u32))
        })
        .unwrap_or_else(|_| egui::ColorImage::new([1, 1], vec![egui::Color32::WHITE]));
    ctx.load_texture("view-range-picture", image, egui::TextureOptions::LINEAR)
}

/// The View Range dialog on a plan: the form, and why the last Apply was refused.
pub(crate) struct RangeDialog {
    /// The plans it edits: one, or several picked together.
    pub(crate) views: Vec<Id>,
    pub(crate) form: RangeForm,
    /// The template that controls a plan's range, if one does: then it is shown, not edited.
    template: Option<String>,
    pub(crate) error: Option<String>,
}

impl BimApp {
    /// View Range for the plans `views` (a level's own plan or a plan view; a dependent view, its parent's): one, its planes from
    /// levels by name; several, from each one's own level, as the first has them.
    pub(crate) fn open_view_ranges(&mut self, views: &[Id]) {
        let m = &self.session.model;
        let mut plans: Vec<Id> = views.iter().filter_map(|v| m.elements.id(m.plan_source(*v))).collect();
        plans.dedup();
        plans.retain(|v| matches!(m.elements.get(*v), Some(Element::Level(_) | Element::PlanView(_))));
        let Some(&first) = plans.first() else { return };
        let template = plans
            .iter()
            .filter_map(|v| match m.elements.get(*v) {
                Some(Element::Level(l)) => l.graphics.template.clone(),
                Some(Element::PlanView(p)) => p.graphics.template.clone(),
                _ => None,
            })
            .find(|n| m.view_templates.get(n).is_some_and(|t| t.controls.contains(&bimcraft_core::graphics::Group::Range)));
        let level = m.plan_level(first).map(|l| m.guid(l)).unwrap_or_default();
        let form = match plans.len() {
            1 => RangeForm::new(m, &m.view_range_of(first), Mode::Plan(level)),
            _ => RangeForm::new(m, &bimcraft_core::view_range::relative(m, level, &m.view_range_of(first)), Mode::Relative),
        };
        self.view_range_dialog = Some(RangeDialog { views: plans, form, template, error: None });
    }

    pub(crate) fn view_range_window(&mut self, ctx: &Context) {
        let Some(d) = self.view_range_dialog.as_mut() else { return };
        let m = &self.session.model;
        let (mut ok, mut apply, mut close) = (false, false, false);
        let title = match (d.views.as_slice(), d.views.first().and_then(|v| m.view_name(*v))) {
            ([_], Some(n)) => tf("View Range: {}", &[n]).to_string(),
            ([_], None) => t("View Range").to_string(),
            (all, _) => tf("View Range: {} plans", &[&all.len().to_string()]).to_string(),
        };
        let locked = d.template.is_some();
        let r = Modal::new(UiId::new("view-range"), title).width(PICTURE_W + 40.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_enabled_ui(!locked, |ui| d.form.show(ui, m));
                if let Some(n) = &d.template {
                    ui.add_space(4.0);
                    ui.label(RichText::new(tf("A view template, {}, controls the view range.", &[n])).color(pal().muted));
                }
                if let Some(e) = &d.error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(e).color(pal().error));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    let size = vec2(76.0, 0.0);
                    apply = ui.add_enabled(!locked, Button::new(t("Apply")).min_size(size)).clicked();
                    close = ui.add(Button::new(t("Cancel")).min_size(size)).clicked();
                    ok = ui.add(Button::new(t("OK")).min_size(size)).clicked();
                });
            });
        });
        if ok && locked {
            self.view_range_dialog = None;
        } else if ok || apply {
            if self.apply_view_range() && ok {
                self.view_range_dialog = None;
            }
        } else if close || r.should_close() {
            self.view_range_dialog = None;
        }
    }

    /// Runs `modify` on the plan with the range the dialog has, unless the plan has it: whether the plan has it now. What is
    /// wrong is said in the dialog.
    fn apply_view_range(&mut self) -> bool {
        let Some(d) = self.view_range_dialog.as_ref() else { return false };
        let views = d.views.clone();
        let range = match d.form.range(&self.session.model) {
            Ok(r) => r,
            Err(e) => {
                if let Some(d) = self.view_range_dialog.as_mut() {
                    d.error = Some(e);
                }
                return false;
            }
        };
        if views.iter().all(|v| self.session.model.view_range_of(*v) == range) {
            return true;
        }
        if self.run("modify", json!({"ids": views, "view_range": range_json(&range)})).is_some() {
            if let Some(d) = self.view_range_dialog.as_mut() {
                d.error = None;
            }
            return true;
        }
        let e = std::mem::take(&mut self.status).text;
        if let Some(d) = self.view_range_dialog.as_mut() {
            d.error = Some(e);
        }
        false
    }
}
