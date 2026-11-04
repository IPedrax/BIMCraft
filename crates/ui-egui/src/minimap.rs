//! The Navigator's minimap: the active plan, ceiling plan, section or elevation drawn whole and small at the top of the
//! Navigator (Rev's Project Browser), the part the view shows outlined on it. A click centres the view there; a drag pans it
//! as it goes. The 3D view, sheets and schedules have none.

use std::sync::{Arc, Weak};

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Id, Model};
use bimcraft_core::render::{self, Drawing, Rgb, View};
use egui::{
    CollapsingHeader, Color32, ColorImage, Context, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, WidgetInfo,
    WidgetType, pos2, vec2,
};

use crate::canvas::{Place, color, paint};
use crate::lang::t;
use crate::orbit::scene_of;
use crate::style::{canvas_pal, pal};
use crate::{BimApp, Sheet};

/// The size the minimap is drawn at, px: shown as wide as the palette and no taller than a third of what is left of it, up to a
/// quarter bigger.
const SIZE: (u32, u32) = (240, 160);

/// The minimap between frames.
#[derive(Default)]
pub(crate) struct Minimap {
    shown: Option<Shot>,
    /// Where its drawing's pixel (0, 0) was on screen last frame, and how many points a pixel is.
    at: Option<(Pos2, f32)>,
    /// A drag's press on screen, and the middle of the view it moves from, mm.
    grab: Option<(Pos2, P2)>,
}

/// The drawing on the minimap: of which model and view, in which canvas theme, and a section's picture under it.
struct Shot {
    model: Weak<Model>,
    sheet: Sheet,
    dark: bool,
    drawing: Drawing,
    picture: Option<TextureHandle>,
}

impl Minimap {
    /// Where model point `p` of the view mapped was on screen last frame: where a test clicks for it.
    #[cfg(test)]
    pub(crate) fn screen(&self, p: P2) -> Option<Pos2> {
        let (origin, k) = self.at?;
        let (x, y) = self.shown.as_ref()?.drawing.view.px(p);
        Some(origin + vec2(x, y) * k)
    }
}

impl BimApp {
    /// The id the active view's camera is kept by, when it is one the minimap maps: a plan, a ceiling plan, a section or an
    /// elevation.
    fn mapped(&self) -> Option<Id> {
        let m = &self.session.model;
        match self.sheet {
            Sheet::Plan(key) | Sheet::Ceiling(key) if m.plan_level(key).is_some() => Some(key),
            Sheet::Section(id) if m.section(id).is_some() => Some(id),
            _ => None,
        }
    }

    /// The minimap under a header that folds it away, folded at first so the tree keeps its room; nothing for a view it doesn't
    /// map.
    pub(crate) fn minimap(&mut self, ui: &mut Ui) {
        let Some(key) = self.mapped() else { return };
        CollapsingHeader::new(t("Minimap")).id_salt("minimap").default_open(false).show(ui, |ui| self.minimap_picture(ui, key));
        ui.add_space(4.0);
    }

    /// The drawing, drawn again only for another model, view or canvas theme, and the view's outline over it.
    fn minimap_picture(&mut self, ui: &mut Ui, key: Id) {
        let model = self.view_model();
        let dark = crate::style::canvas_dark();
        let current = |s: &Shot| s.model.ptr_eq(&Arc::downgrade(&model)) && s.sheet == self.sheet && s.dark == dark;
        if !self.minimap.shown.as_ref().is_some_and(current) {
            self.minimap.shown = Some(self.minimap_shot(ui.ctx(), &model, key, dark));
        }
        let k = (ui.available_width() / SIZE.0 as f32).min(ui.available_height() / (3.0 * SIZE.1 as f32)).clamp(0.1, 1.25);
        let size = vec2(SIZE.0 as f32, SIZE.1 as f32) * k;
        let (rect, response) = ui.vertical_centered(|ui| ui.allocate_exact_size(size, Sense::click_and_drag())).inner;
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, t("Minimap of the view")));
        self.minimap.at = Some((rect.min, k));
        let Some(mini) = self.minimap.shown.as_ref().map(|s| s.drawing.view) else { return };
        let on_map = |p: P2| {
            let (x, y) = mini.px(p);
            rect.min + vec2(x, y) * k
        };
        let to_mm = |q: Pos2| mini.mm(((q.x - rect.min.x) / k, (q.y - rect.min.y) / k));
        // What the view shows, never smaller than can be seen.
        let outline = |v: View| {
            let shows = Rect::from_two_pos(on_map(v.mm((0.0, 0.0))), on_map(v.mm((v.width as f32, v.height as f32))));
            Rect::from_center_size(shows.center(), shows.size().max(vec2(6.0, 6.0)))
        };
        // A drag begun on the outline takes the view with it; one begun beside it moves the view there first.
        if response.drag_started()
            && let (Some(v), Some(from)) = (self.canvas.view(key), ui.input(|i| i.pointer.press_origin()))
        {
            self.minimap.grab = Some((from, if outline(v).contains(from) { P2(v.cx, v.cy) } else { to_mm(from) }));
        }
        if let (Some((from, middle)), Some(now)) = (self.minimap.grab, ui.input(|i| i.pointer.latest_pos()))
            && (response.dragged() || response.drag_stopped())
        {
            let by = (now - from) / k;
            self.canvas.centre(key, middle + P2(f64::from(by.x) / mini.scale, -f64::from(by.y) / mini.scale));
        }
        if response.drag_stopped() {
            self.minimap.grab = None;
        }
        if response.clicked()
            && let Some(at) = response.interact_pointer_pos()
        {
            self.canvas.centre(key, to_mm(at));
        }
        let Some(shot) = &self.minimap.shown else { return };
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 2.0, color(render::PAPER));
        if let Some(t) = &shot.picture {
            p.image(t.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        paint(&p, Place { origin: rect.min, k, paper: false, hide: None }, &shot.drawing, &self.selection, None, true, &[]);
        if let Some(v) = self.canvas.view(key) {
            let ink = canvas_pal();
            p.rect(outline(v), 0.0, ink.select_fill.gamma_multiply(0.5), Stroke::new(1.5, ink.accent), StrokeKind::Middle);
        }
        p.rect_stroke(rect, 2.0, Stroke::new(1.0, pal().border), StrokeKind::Inside);
    }

    /// The minimap's drawing of view `key` of `model`: a plan or a ceiling plan fitted, or a section's or an elevation's
    /// drawing over its picture.
    fn minimap_shot(&mut self, ctx: &Context, model: &Arc<Model>, key: Id, dark: bool) -> Shot {
        let (w, h) = SIZE;
        let (drawing, picture) = match self.sheet {
            Sheet::Ceiling(_) => (render::ceiling_plan(model, key, w, h), None),
            Sheet::Section(_) => {
                let view = render::section_fit(model, key, w, h);
                match render::section_image(model, scene_of(&mut self.orbit.scene, model), key, view, &[]) {
                    Some(image) => {
                        let (rgba, _) = image.rgba.as_chunks::<4>();
                        let pixels = rgba.iter().map(|[r, g, b, _]| color(Rgb(*r, *g, *b))).collect();
                        let size = [image.width as usize, image.height as usize];
                        let colors = ColorImage { size, pixels, source_size: Default::default() };
                        let texture = ctx.load_texture("minimap", colors, TextureOptions::LINEAR);
                        (render::section_over(model, key, view, &image), Some(texture))
                    }
                    None => (render::section(model, key, view), None),
                }
            }
            _ => (render::plan(model, key, w, h), None),
        };
        Shot { model: Arc::downgrade(model), sheet: self.sheet, dark, drawing, picture }
    }
}
