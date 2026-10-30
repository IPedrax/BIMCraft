//! View-owned grid editing and the canvas background layer.
use bimcraft_core::background_grid::{BackgroundGrid, GridStyle};
use bimcraft_core::graphics::{self, Group};
use bimcraft_core::model::{Id, MAX_COORD};
use bimcraft_core::pen::MAX_PEN;
use bimcraft_core::render::View;
use bimcraft_core::units::Units;
use bimcraft_i18n::msg;
use egui::{Align, Context, DragValue, Layout, Painter, Rect, Stroke, Ui};
use serde_json::json;

use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::{BimApp, Sheet};

pub(crate) enum Target {
    View(Id),
    Template(String),
}

pub(crate) struct GridDialog {
    target: Target,
    own: BackgroundGrid,
    pub(crate) draft: BackgroundGrid,
    error: Option<String>,
}

impl BimApp {
    pub(crate) fn grid_view(&self) -> Option<Id> {
        match self.sheet {
            Sheet::Plan(id) | Sheet::Ceiling(id) | Sheet::Section(id) => Some(id),
            _ => None,
        }
    }

    pub(crate) fn background_grid(&self) -> BackgroundGrid {
        self.graphics_shown().background_grid
    }

    /// Why the active view cannot edit its grid, matching other template-controlled parameters.
    pub(crate) fn grid_controlled(&self, view: Id) -> Option<String> {
        let model = &self.session.model;
        if model.plan_source(view) != model.guid(view) {
            let name = model.view_name(model.plan_source(view)).unwrap_or("");
            return Some(tf("Graphics follow parent view {}", &[name]));
        }
        graphics::graphics_of(model, view)?.controlled_by(model, Group::BackgroundGrid).map(crate::graphics::controlled_note)
    }

    pub(crate) fn set_background_grid(&mut self, grid: BackgroundGrid) {
        if let Some(view) = self.grid_view() {
            self.run("graphics", json!({"view": view, "background_grid": grid}));
        }
    }

    pub(crate) fn open_background_grid(&mut self) {
        if let Some(view) = self.grid_view() {
            let own = self.background_grid();
            self.background_grid_dialog = Some(GridDialog { target: Target::View(view), own, draft: own, error: None });
        }
    }

    pub(crate) fn open_template_background_grid(&mut self, name: String) {
        if let Some(template) = self.session.model.view_templates.get(&name) {
            let own = template.graphics.background_grid;
            self.background_grid_dialog = Some(GridDialog { target: Target::Template(name), own, draft: own, error: None });
        }
    }

    pub(crate) fn paint_background_grid(&self, painter: &Painter, rect: Rect, view: View) {
        let grid = self.background_grid();
        if !grid.shown() {
            return;
        }
        let scale = self.graphics_shown().scale_n(self.session.model.units);
        let weights = self.session.model.line_weights().at(scale);
        for line in grid.lines(view) {
            let points = line.points.map(|p| {
                let (x, y) = view.px(p);
                rect.min + egui::vec2(x, y)
            });
            let width = if self.thin { 1.0 } else { (weights.mm(line.style.pen, false) * scale * view.scale) as f32 };
            let [r, g, b] = line.style.color;
            painter.line_segment(points, Stroke::new(width.max(1.0), crate::canvas::color(bimcraft_core::render::Rgb(r, g, b))));
        }
    }

    pub(crate) fn background_grid_window(&mut self, ctx: &Context) {
        let Some(mut dialog) = self.background_grid_dialog.take() else { return };
        let (title, controlled) = match &dialog.target {
            Target::View(view) => (self.session.model.view_name(*view).unwrap_or("").to_string(), self.grid_controlled(*view)),
            Target::Template(name) => (tf("View template {}", &[name]), None),
        };
        let (mut ok, mut apply, mut cancel) = (false, false, false);
        let r = Modal::new(egui::Id::new("background-grid-settings"), t("Background Grids")).width(480.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(&title);
                if let Some(note) = &controlled {
                    ui.label(note);
                }
                ui.add_enabled_ui(controlled.is_none(), |ui| grid_fields(ui, self.session.model.units, &mut dialog.draft));
                if let Some(error) = &dialog.error {
                    ui.colored_label(crate::style::pal().error, error);
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button(t("Cancel")).clicked();
                    apply = ui.add_enabled(controlled.is_none() && dialog.draft != dialog.own, egui::Button::new(t("Apply"))).clicked();
                    ok = ui.add_enabled(controlled.is_none(), egui::Button::new(t("OK"))).clicked();
                });
            });
        });
        if (ok || apply) && dialog.draft != dialog.own {
            let result = match &dialog.target {
                Target::View(view) => self.run("graphics", json!({"view": view, "background_grid": dialog.draft})),
                Target::Template(name) => self.run("template", json!({"name": name, "background_grid": dialog.draft})),
            };
            if result.is_some() {
                dialog.own = dialog.draft;
                dialog.error = None;
            } else {
                dialog.error = Some(self.status.text.clone());
                ok = false;
            }
        }
        if !(ok || cancel || r.should_close()) {
            self.background_grid_dialog = Some(dialog);
        }
    }
}

fn grid_fields(ui: &mut Ui, units: Units, grid: &mut BackgroundGrid) {
    ui.label(t("Grids start at 0, 0 in each view and rotate together."));
    ui.checkbox(&mut grid.visible, t("Show grids"));
    ui.separator();
    ui.checkbox(&mut grid.main.enabled, t("Main grid"));
    for (axis, label) in [msg!("Main spacing X"), msg!("Main spacing Y")].into_iter().enumerate() {
        ui.horizontal(|ui| {
            let l = ui.label(t(label));
            ui.add(
                DragValue::new(&mut grid.spacing[axis])
                    .range(0.001..=MAX_COORD)
                    .speed(10.0)
                    .custom_formatter(move |n, _| units.with_unit(n))
                    .custom_parser(move |s| units.parse(s).filter(|n| (0.001..=MAX_COORD).contains(n))),
            )
            .labelled_by(l.id);
        });
    }
    style(ui, &mut grid.main, msg!("Main grid colour"), msg!("Main grid pen"));
    ui.separator();
    ui.checkbox(&mut grid.sub.enabled, t("Subgrid"));
    for (axis, label) in [msg!("Subgrid divisions X"), msg!("Subgrid divisions Y")].into_iter().enumerate() {
        ui.horizontal(|ui| {
            let l = ui.label(t(label));
            ui.add(DragValue::new(&mut grid.divisions[axis]).range(1..=1000)).labelled_by(l.id);
        });
    }
    style(ui, &mut grid.sub, msg!("Subgrid colour"), msg!("Subgrid pen"));
    ui.separator();
    ui.horizontal(|ui| {
        let l = ui.label(t("Grid rotation"));
        ui.add(DragValue::new(&mut grid.rotation).range(-360.0..=360.0).suffix("°").speed(1.0)).labelled_by(l.id);
    });
}

fn style(ui: &mut Ui, style: &mut GridStyle, colour: &str, pen: &str) {
    ui.horizontal(|ui| {
        let l = ui.label(t(colour));
        ui.color_edit_button_srgb(&mut style.color).labelled_by(l.id);
        let l = ui.label(t(pen));
        ui.add(DragValue::new(&mut style.pen.0).range(0..=MAX_PEN)).labelled_by(l.id);
    });
}
