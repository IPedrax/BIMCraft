//! Sheets in the desktop app: the core's drawing of a sheet (its title block, its views at their
//! scales, its schedules and notes) made at paper density once per change, and scaled as the view
//! zooms, so lettering and line weights keep to the paper. Click a view to select it, drag it (or
//! a note) to move it, double-click it to open it; the Text tool writes on the paper. Properties
//! shows the sheet's parameters when nothing is selected.

use std::sync::Arc;

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id, Model, paper};
use bimcraft_core::render::{Drawing, Prim, View};
use bimcraft_core::sheet::{self, DENSITY};
use egui::{Color32, ColorImage, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, TextureId, TextureOptions, Ui, vec2};

use crate::canvas::{PICK, Place, click_modifiers, paint};
use crate::style::pal;
use crate::{BimApp, SCHEDULES, Sheet, Status, Tool};

/// The sheet on screen, between frames: its drawing and the pictures in it, and a view being
/// dragged (from which paper point).
#[derive(Default)]
pub struct Papers {
    drawn: Option<(Arc<Model>, Id, Arc<Drawing>)>,
    pictures: Vec<TextureHandle>,
    drag: Option<P2>,
}

impl BimApp {
    /// Sheet `id`, filling `ui`.
    pub(crate) fn paper_view(&mut self, ui: &mut Ui, id: Id) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::CLICK | Sense::DRAG);
        let model = Arc::clone(&self.session.model);
        let Some(size) = model.sheet(id).and_then(|s| paper(&s.size)) else { return };
        // The whole paper, 24 points clear of the edges.
        let fit = |w: u32, h: u32| {
            let room = |px: u32, mm: f64| (f64::from(px) - 48.0).max(1.0) / mm;
            View { scale: room(w, size.0).min(room(h, size.1)), cx: size.0 / 2.0, cy: size.1 / 2.0, width: w, height: h }
        };
        let Some((view, local)) = self.canvas.navigate(ui, &response, rect, id, fit) else { return };
        let writing = self.writing();
        let papers = &mut self.paper;
        if !matches!(&papers.drawn, Some((m, s, _)) if Arc::ptr_eq(m, &model) && *s == id) {
            let d = sheet::draw(&model, id).unwrap_or_else(|| Drawing::new(view));
            papers.pictures = d
                .prims
                .iter()
                .filter_map(|p| match p {
                    Prim::Image { width, height, rgba, .. } => {
                        let pixels = ColorImage::from_rgba_unmultiplied([*width as usize, *height as usize], rgba);
                        Some(ui.ctx().load_texture("sheet picture", pixels, TextureOptions::LINEAR))
                    }
                    _ => None,
                })
                .collect();
            papers.drawn = Some((Arc::clone(&model), id, Arc::new(d)));
        }
        let Some((_, _, d)) = &papers.drawn else { return };
        let d = Arc::clone(d);
        let pictures: Vec<TextureId> = papers.pictures.iter().map(TextureHandle::id).collect();
        // The drawing's pixels on screen: the paper's top left corner, k screen px a drawing px.
        let at = Place {
            origin: rect.min + vec2(view.px(P2(0.0, size.1)).0, view.px(P2(0.0, size.1)).1),
            k: (view.scale / DENSITY) as f32,
            paper: true,
            hide: writing,
        };
        let drawn = |q: Pos2| ((q.x - at.origin.x) / at.k, (q.y - at.origin.y) / at.k);
        let pick = |q: Pos2| d.pick(drawn(q), PICK / at.k);
        let modify = self.tool == Tool::Modify;
        let hovered = local.map(|(x, y)| rect.min + vec2(x, y)).filter(|_| modify && papers.drag.is_none()).and_then(pick);
        // A view dragged moves by the paper between where it was pressed and where it's let go.
        if response.drag_started_by(PointerButton::Primary)
            && modify
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
            && let Some(vp) = pick(o).filter(|v| matches!(model.elements.get(v), Some(Element::Viewport(_) | Element::Text(_))))
        {
            papers.drag = Some(view.mm((o.x - rect.min.x, o.y - rect.min.y)));
            self.grab(vp);
        }
        let now = ui.input(|i| i.pointer.latest_pos()).map(|q| view.mm((q.x - rect.min.x, q.y - rect.min.y)));
        let papers = &mut self.paper;
        let by = papers.drag.zip(now).map(|(from, to)| {
            let d = to - from;
            P2(d.0.round(), d.1.round())
        });
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, pal().desk);
        let sheet_rect = Rect::from_two_pos(at.origin, rect.min + vec2(view.px(P2(size.0, 0.0)).0, view.px(P2(size.0, 0.0)).1));
        p.rect(sheet_rect, 0.0, Color32::WHITE, Stroke::new(1.0, pal().border), StrokeKind::Outside);
        paint(&p, at, &d, &self.selection, hovered, self.thin, &pictures);
        // While a note is written here, the pointer is its editor's.
        if self.writing_in(id) {
            self.paper.drag = None;
            self.note_canvas(ui, &response, &p, at, &d);
            self.nav_bar(ui, rect, id);
            return;
        }
        // Where a dragged view would go: its frame, moved.
        if let Some(by) = by {
            for vp in &self.selection {
                if let Some((frame, (x, y))) = sheet::frame(&model, *vp) {
                    let shift = vec2(by.0 as f32, -by.1 as f32) * (view.scale as f32);
                    let r = Rect::from_min_size(at.pos((x, y)) + shift, vec2(frame.width as f32, frame.height as f32) * at.k);
                    p.rect_stroke(r, 0.0, Stroke::new(1.0, pal().accent), StrokeKind::Middle);
                }
            }
        }
        if response.drag_stopped()
            && let Some(by) = std::mem::take(&mut self.paper.drag).and(by)
            && self.finish_drag()
            && (by.0 != 0.0 || by.1 != 0.0)
        {
            self.run("move", serde_json::json!({"ids": self.selection, "by": [by.0, by.1]}));
        }
        // A double click on a view opens it; a click selects.
        let view_of = |id: Id| match model.elements.get(id) {
            Some(Element::Viewport(v)) => match model.elements.get(v.view) {
                Some(Element::Level(_) | Element::PlanView(_)) => model.elements.id(v.view).map(Sheet::Plan),
                Some(Element::Section(_) | Element::Elevation(_)) => model.elements.id(v.view).map(Sheet::Section),
                Some(Element::View3D(_)) => Some(Sheet::View3D),
                Some(Element::Schedule(sc)) => match bimcraft_core::schedule::builtin(Some(&sc.name), None) {
                    Some((cat, _)) => SCHEDULES.iter().find(|(c, _)| *c == cat).map(|(c, _)| Sheet::Schedule(c)),
                    None => model.elements.id(v.view).map(Sheet::Table),
                },
                _ => None,
            },
            _ => None,
        };
        if response.double_clicked()
            && let Some(note) = hovered.filter(|h| crate::notes::is_note(&model, *h))
        {
            self.edit_note(note);
            return;
        }
        if response.double_clicked()
            && let Some(open) = hovered.and_then(view_of)
        {
            // A 3D view opens in the 3D tab from its own camera; another view opens as its own tab.
            match hovered.and_then(|id| match model.elements.get(id) {
                Some(Element::Viewport(v)) if matches!(model.elements.get(v.view), Some(Element::View3D(_))) => model.elements.id(v.view),
                _ => None,
            }) {
                Some(view) => self.open_3d(Some(view)),
                None => self.open_sheet(open),
            }
            return;
        }
        if let Some(q) = self.clicked_at(&response) {
            match pick(q) {
                found if modify => self.pick_up(found.into_iter().collect(), click_modifiers(ui)),
                // Notes in paper mm, to the half millimetre.
                _ if self.tool == Tool::Text => {
                    let p = view.mm((q.x - rect.min.x, q.y - rect.min.y));
                    self.write_note(id, P2((p.0 * 2.0).round() / 2.0, (p.1 * 2.0).round() / 2.0));
                }
                _ => {
                    self.status = Status {
                        text: crate::lang::t("this tool works in plan views: open one from the Project Browser").into(),
                        error: true,
                    }
                }
            }
        }
        self.context_menu(&response);
        self.nav_bar(ui, rect, id);
    }
}
