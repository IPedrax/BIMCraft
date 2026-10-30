//! Sections and elevations: the core's picture of what the view sees past its line, its cut,
//! levels, grids, notes and spot elevations drawn over it, panned and zoomed like a plan (its
//! points run along its line and up). A click picks what it shows: a cut, a level, a grid, or what
//! lies beyond; a drag, a selection box; with Text or Spot Elevation, a click places one. Its crop
//! region's grips drag its edges.

use std::sync::Arc;

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Id, Model};
use bimcraft_core::render::{self, Rgb, View};
use bimcraft_core::view3d::{Image, MAX_SIDE, Scene};
use egui::{Color32, ColorImage, Context, Rect, Sense, TextureHandle, TextureOptions, Ui, pos2};
use serde_json::json;

use crate::canvas::{PICK, Place, click_modifiers, color, paint};
use crate::orbit::{rgb, scene_of};
use crate::style::canvas_pal;
use crate::{BimApp, Status, Tool};

/// What a picture is of: the model, the view, its framing in pixels, the elements tinted, and
/// whether it's drawn dark.
type Shot = (Arc<Model>, Id, View, Vec<(Id, Rgb)>, bool);

/// The section or elevation on screen, between frames: its picture, and its texture.
#[derive(Default)]
pub struct Cut {
    /// How many times a picture was drawn (by the core, for the drawing over it or for the texture): a tick of the test that a
    /// pan or a zoom draws it once.
    #[cfg(test)]
    pub(crate) pictures: usize,
    image: Option<Image>,
    texture: Option<TextureHandle>,
    shown: Option<Shot>,
    grid_background: bool,
}

impl Cut {
    /// The picture of section `id` through `pixels` with `tints` on the elements they name, into the texture, of `model`.
    #[allow(clippy::too_many_arguments)]
    fn draw(
        &mut self,
        ctx: &Context,
        model: &Arc<Model>,
        scene: &Scene,
        id: Id,
        pixels: View,
        tints: Vec<(Id, Rgb)>,
        dark: bool,
        grid_background: bool,
    ) {
        #[cfg(test)]
        {
            self.pictures += 1;
        }
        if let Some(image) = render::section_image(model, scene, id, pixels, &tints) {
            // A hidden-line picture, so dark as the drawing over it is.
            let (rgba, _) = image.rgba.as_chunks::<4>();
            let pixels: Vec<Color32> = rgba
                .iter()
                .enumerate()
                .map(|(i, [r, g, b, _])| {
                    let (x, y) = (i % image.width as usize, i / image.width as usize);
                    if grid_background && Rgb(*r, *g, *b) == render::PAPER && !image.covered(x as f64, y as f64) {
                        Color32::TRANSPARENT
                    } else {
                        color(Rgb(*r, *g, *b))
                    }
                })
                .collect();
            let colors = ColorImage { size: [image.width as usize, image.height as usize], pixels, source_size: Default::default() };
            match &mut self.texture {
                Some(t) => t.set(colors, TextureOptions::NEAREST),
                None => self.texture = Some(ctx.load_texture("section", colors, TextureOptions::NEAREST)),
            }
            self.image = Some(image);
        }
        self.grid_background = grid_background;
        self.shown = Some((Arc::clone(model), id, pixels, tints, dark));
    }
}

impl BimApp {
    /// Section or elevation `id`, filling `ui`.
    pub(crate) fn section_view(&mut self, ui: &mut Ui, id: Id) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::CLICK | Sense::DRAG);
        let model = self.view_model();
        let fit = |w, h| render::section_fit(&model, id, w, h);
        let Some((view, local)) = self.canvas.navigate(ui, &response, rect, id, fit) else { return };
        let crop = self.crop_grips(ui, &response, rect, &view, id);
        let datum = self.level_grips(ui, &response, rect, &view, id);
        // The picture at the screen's pixels, framed as the drawing is in points.
        let ppp = f64::from(ui.ctx().pixels_per_point());
        let side = |v: u32| ((f64::from(v) * ppp).round().max(1.0) as u32).min(MAX_SIDE);
        let pixels = View { scale: view.scale * ppp, width: side(view.width), height: side(view.height), ..view };
        let dark = crate::style::canvas_dark();
        // The picture on screen is this model's, in this view and size (and, tints given, tinted so): a pan or a zoom, a new
        // model or another tab is not. A picture costs a frame, and the drawing over it keeps its regions' ink from it: so it is
        // drawn first and once, with the tints it had, and again only if the hover changed along with the view.
        let grid_background = self.background_grid().shown();
        let current = |cut: &Cut, tints: Option<&[(Id, Rgb)]>| {
            cut.grid_background == grid_background
                && matches!(&cut.shown, Some((m, v, f, t, k)) if Arc::ptr_eq(m, &model) && *v == id && *f == pixels && *k == dark && tints.is_none_or(|x| x == t))
        };
        if !current(&self.cut, None) {
            let tints = self.cut.shown.as_ref().map(|s| s.3.clone()).unwrap_or_default();
            self.cut.draw(ui.ctx(), &model, scene_of(&mut self.orbit.scene, &model), id, pixels, tints, dark, grid_background);
        }
        let d = match &self.cut.image {
            Some(image) => render::section_over(&model, id, view, image),
            None => {
                #[cfg(test)]
                {
                    self.cut.pictures += 1;
                }
                render::section(&model, id, view)
            }
        };
        // Under the cursor: what's drawn over the picture, else what the picture shows.
        let pick = |cut: &Cut, (x, y): (f32, f32)| {
            d.pick((x, y), PICK).or_else(|| cut.image.as_ref()?.pick(f64::from(x) * ppp, f64::from(y) * ppp))
        };
        let hovered = local.filter(|_| matches!(self.tool, Tool::Modify | Tool::Demolish)).and_then(|q| pick(&self.cut, q));
        let tints: Vec<(Id, Rgb)> = self
            .selection
            .iter()
            .map(|id| (*id, rgb(canvas_pal().accent)))
            .chain(hovered.filter(|h| !self.selection.contains(h)).map(|h| (h, rgb(canvas_pal().hover))))
            .collect();
        if !current(&self.cut, Some(&tints)) {
            self.cut.draw(ui.ctx(), &model, scene_of(&mut self.orbit.scene, &model), id, pixels, tints, dark, grid_background);
        }
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, color(render::PAPER));
        self.paint_background_grid(&p, rect, view);
        if let Some(t) = &self.cut.texture {
            p.image(t.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        paint(&p, Place { hide: self.writing(), ..Place::at(rect.min) }, &d, &self.selection, hovered, self.thin, &[]);
        // While a note is written here, the pointer is its editor's.
        if self.writing_in(id) {
            p.extend(crop);
            p.extend(datum);
            self.note_canvas(ui, &response, &p, Place::at(rect.min), &d);
            self.nav_bar(ui, rect, id);
            return;
        }
        p.extend(crop);
        // A drag draws a selection box: it takes what the drawing over the picture has in it, and
        // what the picture shows.
        let held = self.cut.image.take();
        let image = held.as_ref();
        let inside = |a: (f32, f32), b: (f32, f32), crossing: bool| {
            let scaled = |q: (f32, f32)| (f64::from(q.0) * ppp, f64::from(q.1) * ppp);
            let mut found = d.in_box(a, b, crossing);
            found.extend(image.map(|i| i.in_box(scaled(a), scaled(b), crossing)).unwrap_or_default());
            found.sort_unstable();
            found.dedup();
            found
        };
        let boxed = self.drag_box(ui, &response, rect, inside);
        self.cut.image = held;
        p.extend(boxed);
        p.extend(datum);
        let clicked = self.clicked_at(&response).map(|at| (at.x - rect.min.x, at.y - rect.min.y));
        // Along the line and up, to the nearest 10 mm.
        let grid = self.background_grid();
        let snapping = self.grid_snapping;
        let at = |q: (f32, f32)| {
            let p = view.mm(q);
            if snapping && let Some(at) = grid.nearest(p) {
                return at;
            }
            P2((p.0 / 10.0).round() * 10.0, (p.1 / 10.0).round() * 10.0)
        };
        // A double click on a note writes in it.
        if response.double_clicked()
            && let Some(note) = hovered.filter(|h| crate::notes::is_note(&model, *h))
        {
            self.edit_note(note);
            return;
        }
        match (clicked, &self.tool) {
            (Some(q), Tool::Modify) => {
                let found = pick(&self.cut, q);
                self.pick_up(found.into_iter().collect(), click_modifiers(ui));
            }
            (Some(q), Tool::Demolish) => {
                if let Some(id) = pick(&self.cut, q) {
                    self.demolish(id);
                }
            }
            (Some(q), Tool::Text) => self.write_note(id, at(q)),
            (Some(q), Tool::Spot) => {
                self.run("spot", json!({"at": [at(q).0, at(q).1], "view": id}));
            }
            (Some(_), _) => {
                self.status =
                    Status { text: crate::lang::t("this tool works in plan views: open one from the Project Browser").into(), error: true }
            }
            (None, _) => {}
        }
        self.context_menu(&response);
        self.nav_bar(ui, rect, id);
    }
}
