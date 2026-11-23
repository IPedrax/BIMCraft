//! Sections and elevations: the core's picture of what the view sees past its line, its cut,
//! levels, grids, notes and spot elevations drawn over it, panned and zoomed like a plan (its
//! points run along its line and up). A click picks what it shows: a cut, a level, a grid, or what
//! lies beyond; a drag, a selection box; with Text or Spot Elevation, a click places one. Its crop
//! region's grips drag its edges.

use std::sync::Arc;

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id, Model};
use bimcraft_core::render::{self, Rgb, View};
use bimcraft_core::view3d::{Image, MAX_SIDE};
use egui::{Color32, ColorImage, Context, Rect, Sense, TextureHandle, TextureOptions, Ui, pos2};
use serde_json::json;

use crate::canvas::{PICK, Place, click_modifiers, color, paint};
use crate::orbit::rgb;
use crate::section_worker::{Frame, Renderer, Request};
use crate::style::canvas_pal;
use crate::view_worker::Latest;
use crate::{BimApp, Sheet, Status, Tool};

mod tools;

/// Geometry and visibility identity, independent of interaction paint.
type Shot = (Arc<Model>, Id, View, View);

/// The drawing over a picture, with the picture's generation, the model, the view and its framing it was made for.
type Drawn = (u64, Arc<Model>, Id, View, Arc<render::Drawing>);

/// The section or elevation on screen, between frames: its picture, and its texture.
#[derive(Default)]
pub struct Cut {
    /// How many times model cuts and datums were built; repainting regions does not increment this.
    #[cfg(test)]
    pub(crate) drawings: usize,
    /// Which picture this is: one more each time one is drawn.
    generation: u64,
    /// The drawing over the picture and its generation, model, view and framing.
    /// Region ink is repainted on interaction from retained model cuts and the tinted picture.
    drawn: Option<Drawn>,
    /// What every section of a model asks of all of it, kept with the model (a snapshot) it was made of.
    #[cfg(test)]
    work: Option<(Arc<Model>, render::SectionWork)>,
    /// How many times a picture was drawn (by the core, for the drawing over it or for the texture): a tick of the test that a
    /// pan or a zoom draws it once.
    #[cfg(test)]
    pub(crate) pictures: usize,
    /// How many times the picture went into the texture, its paper in the canvas's colour.
    #[cfg(test)]
    pub(crate) uploads: usize,
    image: Option<Arc<Image>>,
    texture: Option<TextureHandle>,
    shown: Option<Shot>,
    grid_background: bool,
    pub(crate) tints: Vec<(Id, Rgb)>,
    ground: Color32,
    #[cfg(test)]
    overlay: Option<render::SectionOverlay>,
    mode: Mode,
    worker: Option<Latest<Request, Frame>>,
    requested: Option<Request>,
    fitted: Option<(Arc<Model>, Id, View)>,
    levels: std::collections::BTreeMap<Id, (f64, f64)>,
}

// UI harnesses use deterministic inline execution; worker tests exercise real threads.
struct Mode(bool);
impl Default for Mode {
    fn default() -> Self {
        Self(!cfg!(test))
    }
}

impl Cut {
    pub(crate) fn suspend(&mut self) {
        self.worker = None;
        self.requested = None;
        self.fitted = None;
    }

    fn request(&mut self, ctx: &Context, request: Request) -> Result<(), String> {
        if self.worker.is_none() {
            let mut renderer = Renderer::default();
            let ctx = ctx.clone();
            self.worker = Some(Latest::new(move |r, token| renderer.render(r, token), move || ctx.request_repaint()));
        }
        if let Some(worker) = &self.worker {
            if !self.requested.as_ref().is_some_and(|r| r.same(&request)) {
                worker.request(request.clone());
                self.requested = Some(request);
            }
            worker.start()?;
            if let Some(error) = worker.take_error() {
                return Err(error);
            }
            if worker.busy() {
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
        }
        Ok(())
    }

    fn poll(&mut self, ctx: &Context, model: &Arc<Model>, id: Id, ground: Color32, grid_background: bool) {
        let Some(frame) = self.worker.as_ref().and_then(Latest::take) else { return };
        // The mailbox rejects superseded generations, and all display/pick data is installed together.
        let Frame { mut request, view, pixels, image, drawing, rebuilt, levels } = frame;
        if !Arc::ptr_eq(&request.model, model) || request.id != id {
            return;
        }
        if request.view.is_none() {
            self.fitted = Some((Arc::clone(&request.model), request.id, view));
        }
        request.view = Some(view);
        self.generation = self.generation.saturating_add(1);
        self.drawn = Some((self.generation, Arc::clone(&request.model), request.id, view, Arc::new(drawing)));
        self.shown = Some((Arc::clone(&request.model), request.id, view, pixels));
        self.tints = request.tints.clone();
        self.image = image;
        self.levels = levels;
        self.requested = Some(request);
        #[cfg(test)]
        {
            if rebuilt {
                self.pictures += 1;
                self.drawings += 1;
            }
        }
        #[cfg(not(test))]
        let _ = rebuilt;
        self.upload(ctx, ground, grid_background);
    }

    /// The picture of section `id` through `pixels` with `tints` on the elements they name, into the texture, of `model`; and
    /// the drawing over it through `view`, kept with it.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    fn draw(
        &mut self,
        ctx: &Context,
        model: &Arc<Model>,
        id: Id,
        (view, pixels): (View, View),
        tints: Vec<(Id, Rgb)>,
        ground: Color32,
        grid_background: bool,
    ) {
        #[cfg(test)]
        {
            self.pictures += 1;
            self.drawings += 1;
        }
        self.generation += 1;
        // The picture, and the drawing over it, which keeps its regions' ink from it: the whole model's work once for both.
        if !self.work.as_ref().is_some_and(|(m, _)| Arc::ptr_eq(m, model)) {
            self.work = Some((Arc::clone(model), render::SectionWork::new(model)));
        }
        if let Some((_, work)) = &self.work {
            self.overlay = Some(render::SectionOverlay::new(model, work, id, view));
            self.levels = self.overlay.as_ref().map(|o| o.levels().clone()).unwrap_or_default();
            self.image = render::section_picture_with(model, work, id, pixels, &[]).map(Arc::new);
        }
        self.shown = Some((Arc::clone(model), id, view, pixels));
        self.repaint(ctx, model, id, view, tints, ground, grid_background);
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    fn repaint(
        &mut self,
        ctx: &Context,
        model: &Arc<Model>,
        id: Id,
        view: View,
        tints: Vec<(Id, Rgb)>,
        ground: Color32,
        grid_background: bool,
    ) {
        if let Some(image) = &mut self.image {
            Arc::make_mut(image).retint_section(&tints);
        }
        if let Some(overlay) = &self.overlay {
            self.drawn = Some((self.generation, Arc::clone(model), id, view, Arc::new(overlay.draw(model, self.image.as_deref()))));
        }
        self.tints = tints;
        self.upload(ctx, ground, grid_background);
    }

    fn upload(&mut self, ctx: &Context, ground: Color32, grid_background: bool) {
        if let Some(image) = &self.image {
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
        }
        self.grid_background = grid_background;
        self.ground = ground;
        #[cfg(test)]
        {
            self.uploads += 1;
        }
    }
}

impl BimApp {
    /// Section or elevation `id`, filling `ui`.
    pub(crate) fn section_view(&mut self, ui: &mut Ui, id: Id) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::CLICK | Sense::DRAG);
        let model = self.view_model();
        let ground = color(render::PAPER);
        // Under a background grid or an underlay drawn behind, the picture's paper is clear.
        let grid_background = self.background_grid().shown() || crate::underlay::Layer::behind(&model, id);
        if self.cut.mode.0 {
            self.cut.poll(ui.ctx(), &model, id, ground, grid_background);
            if !self.canvas.has_view(id) {
                if let Some((m, v, fit)) = self.cut.fitted.take().filter(|(m, v, _)| Arc::ptr_eq(m, &model) && *v == id) {
                    let _ = (m, v);
                    self.canvas.fitted(id, fit);
                } else {
                    let request = Request {
                        model: Arc::clone(&model),
                        id,
                        view: None,
                        size: [rect.width().max(1.0) as u32, rect.height().max(1.0) as u32],
                        fit_height: (rect.height() - self.canvas.room).max(1.0) as u32,
                        ppp: f64::from(ui.ctx().pixels_per_point()),
                        tints: self.lit_selection().iter().map(|id| (*id, rgb(canvas_pal().accent))).collect(),
                    };
                    if let Err(error) = self.cut.request(ui.ctx(), request) {
                        self.status = Status { text: error, error: true };
                    }
                    ui.painter_at(rect).rect_filled(rect, 0.0, color(render::PAPER));
                    self.nav_bar(ui, rect, id);
                    return;
                }
            }
        }
        let fit = |w, h| render::section_fit(&model, id, w, h);
        let Some((view, local)) = self.canvas.navigate(ui, &response, rect, id, fit) else { return };
        // Where the pointer is on the section's drawing, for the others in a Co-Work session.
        self.cowork_seen.pointer = local.map(|q| (id, view.mm(q)));
        let crop = self.crop_grips(ui, &response, rect, &view, id);
        let callout = self.callout_grips(ui, &response, rect, &view, id);
        let level_ends = self
            .cut
            .shown
            .as_ref()
            .filter(|(m, shown, ..)| Arc::ptr_eq(m, &model) && *shown == id)
            .and_then(|_| self.selection.first().filter(|_| self.selection.len() == 1))
            .and_then(|level| self.cut.levels.get(level))
            .copied();
        let datum = self.level_grips(ui, &response, rect, &view, id, level_ends);
        // The picture at the screen's pixels, framed as the drawing is in points.
        let ppp = f64::from(ui.ctx().pixels_per_point());
        let side = |v: u32| ((f64::from(v) * ppp).round().max(1.0) as u32).min(MAX_SIDE);
        let pixels = View { scale: view.scale * ppp, width: side(view.width), height: side(view.height), ..view };
        let ground = color(render::PAPER);
        // Framing/model changes rebuild visibility; interaction changes only paint.
        // Under a background grid or an underlay drawn behind, the picture's paper is clear.
        let grid_background = self.background_grid().shown() || crate::underlay::Layer::behind(&model, id);
        // A new canvas colour, or the background grid shown or hidden, paints the same picture's paper again.
        if self.cut.ground != ground || self.cut.grid_background != grid_background {
            self.cut.upload(ui.ctx(), ground, grid_background);
        }
        let current =
            matches!(&self.cut.shown, Some((m, v, logical, f)) if Arc::ptr_eq(m, &model) && *v == id && *logical == view && *f == pixels);
        #[cfg(test)]
        if !self.cut.mode.0 && !current {
            let tints = self.cut.tints.clone();
            self.cut.draw(ui.ctx(), &model, id, (view, pixels), tints, ground, grid_background);
        }
        let kept =
            self.cut.drawn.as_ref().filter(|(g, m, v, f, _)| {
                *g == self.cut.generation && *v == id && (self.cut.mode.0 || (Arc::ptr_eq(m, &model) && *f == view))
            });
        let d = match kept {
            Some((.., d)) => Arc::clone(d),
            None => Arc::new(render::Drawing::new(view)),
        };
        let displayed = self.cut.shown.as_ref().filter(|(_, v, ..)| *v == id);
        let displaying = displayed.is_some();
        let (source, source_ppp) = displayed
            .map(|(_, _, v, p)| (*v, [f64::from(p.width) / f64::from(v.width.max(1)), f64::from(p.height) / f64::from(v.height.max(1))]))
            .unwrap_or((view, [ppp; 2]));
        let k = (view.scale / source.scale) as f32;
        let offset = view.px(source.mm((0.0, 0.0)));
        let place = Place { origin: rect.min + egui::vec2(offset.0, offset.1), k, hide: self.writing(), ..Place::at(rect.min) };
        let unproject = |(x, y): (f32, f32)| ((x - offset.0) / k, (y - offset.1) / k);
        // Under the cursor: what's drawn over the picture, else what the picture shows.
        let pick = |cut: &Cut, d: &render::Drawing, (x, y): (f32, f32)| {
            let (x, y) = unproject((x, y));
            d.pick((x, y), PICK / k)
                .or_else(|| cut.image.as_ref().filter(|_| displaying)?.pick(f64::from(x) * source_ppp[0], f64::from(y) * source_ppp[1]))
        };
        // Modify and Demolish pick what is under the cursor, and the section tools take their references from it.
        let picks = matches!(
            self.tool,
            Tool::Modify | Tool::Demolish | Tool::Dimension(..) | Tool::Move(_) | Tool::DetailLine(_) | Tool::Trim(_) | Tool::TrimTo { .. }
        );
        // The level line under the cursor, though a wall or a floor lies nearer: what Aligned to Picked Level takes.
        let pick_level =
            |d: &render::Drawing, q: (f32, f32)| d.pick_all(unproject(q), PICK / k).into_iter().find(|l| model.level(*l).is_some());
        // Under the cursor, lit: what a click would take (Aligned to Picked Level takes only a level line).
        let hovered = local.and_then(|q| match self.tool {
            _ if picks => pick(&self.cut, &d, q),
            Tool::PickLevel => pick_level(&d, q),
            _ => None,
        });
        // On the selected curtain wall under the cursor, Tab goes through its parts there, on the plane the view looks at.
        let tab = ui.input(|i| i.key_pressed(egui::Key::Tab)) && local.is_some();
        if tab {
            ui.ctx().memory_mut(|m| {
                if let Some(f) = m.focused() {
                    m.surrender_focus(f);
                }
            });
        }
        if (std::mem::take(&mut self.tab_cycle) | std::mem::take(&mut self.face_cycle) || tab)
            && let (Some(q), Some(wall)) = (local, hovered)
            && crate::curtain::tab_wall(&model, &self.selection, &[wall], Some(wall), self.tool == Tool::Modify) == Some(wall)
            && let Some((cam, _)) = render::section_camera(&model, id, view)
            && let at = (f64::from(q.0), f64::from(q.1))
            && let laid = self.curtain_laid.of(&model, wall)
            && let Some(list) =
                crate::curtain::parts_on_ray(&model, &laid, id, wall, cam.point_at(at, 0.0), cam.point_at(at, 1000.0), 1.0 / view.scale)
        {
            self.curtain_part = crate::curtain::next_part(self.curtain_part, wall, &list);
            self.curtain_more.clear();
            if self.curtain_part.is_some() {
                self.selection = vec![wall];
            }
        }
        // A selected group lights its members, and a curtain wall selected stays as it is drawn, for the part Tab picks to be
        // seen on it; a group under the pointer lights its members, as a click would take it; in Edit Group the rest is greyed.
        let lit = crate::curtain::tinted_selection(&model, &self.lit_selection());
        let scope = self.session.group_scope();
        let hover_lit = match self.tool {
            Tool::Modify => crate::groups::hover_lit(&model, hovered, scope.as_ref(), self.group_pick()),
            _ => hovered.into_iter().collect(),
        };
        let lit_hover = crate::curtain::tinted_hover(&model, &self.selection, hovered);
        let mut tints: Vec<(Id, Rgb)> = lit
            .iter()
            .map(|id| (*id, rgb(canvas_pal().accent)))
            .chain(
                hover_lit
                    .iter()
                    .filter(|h| !lit.contains(h) && !(self.selection.contains(h) && crate::curtain::is_curtain(&model, **h)))
                    .map(|h| (*h, rgb(canvas_pal().hover))),
            )
            .collect();
        let lit_now: Vec<Id> = tints.iter().map(|(id, _)| *id).collect();
        tints.extend(crate::groups::greyed(&model, scope.as_ref(), &lit_now).into_iter().map(|id| (id, crate::groups::grey())));
        if self.cut.mode.0 {
            let request = Request {
                model: Arc::clone(&model),
                id,
                view: Some(view),
                size: [view.width, view.height],
                fit_height: (rect.height() - self.canvas.room).max(1.0) as u32,
                ppp,
                tints,
            };
            if let Err(error) = self.cut.request(ui.ctx(), request) {
                self.status = Status { text: error, error: true };
            }
            if self.cut.ground != ground || self.cut.grid_background != grid_background {
                self.cut.upload(ui.ctx(), ground, grid_background);
            }
        } else {
            #[cfg(test)]
            if self.cut.tints != tints || self.cut.ground != ground || self.cut.grid_background != grid_background {
                self.cut.repaint(ui.ctx(), &model, id, view, tints, ground, grid_background);
            }
        }
        #[cfg(not(test))]
        let _ = (current, pixels);
        let d = self
            .cut
            .drawn
            .as_ref()
            .filter(|(_, _, shown, ..)| *shown == id)
            .map(|(.., drawing)| Arc::clone(drawing))
            .unwrap_or_else(|| Arc::clone(&d));
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, color(render::PAPER));
        self.paint_background_grid(&p, rect, view);
        // The underlay, made once the section is: behind it here, or over it below.
        let hold = self.cut.worker.as_ref().is_some_and(Latest::busy);
        let under = self.underlay_now(ui.ctx(), &model, id, view, hold);
        self.paint_underlay(ui.ctx(), &p, rect, view, under.as_ref(), false);
        if let Some(t) = self.cut.texture.as_ref().filter(|_| displaying) {
            let image_rect = Rect::from_min_size(place.origin, egui::vec2(source.width as f32, source.height as f32) * k);
            p.image(t.id(), image_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        paint(&p, place, &d, &lit, lit_hover, self.thin, &[]);
        self.paint_underlay(ui.ctx(), &p, rect, view, under.as_ref(), true);
        // Underlay Settings' Move or Rotate is picking here: the view's own clicks wait.
        let picking = self.underlay_pick(ui, &response, &p, rect, &view, id, None);
        crate::cowork::paint_plan(&p, &self.cowork, &self.cowork_seen, &model, ("section", id), rect, &view, (&d, place));
        if let Some((cam, _)) = render::section_camera(&model, id, view) {
            let screen = |c: [f64; 3]| {
                let (x, y, _) = cam.project(c);
                rect.min + egui::vec2(x as f32, y as f32)
            };
            // A curtain wall selected shows by its outline, and the part Tab has picked, and the one it would go to, on it.
            if self.curtain_part.is_none() {
                for w in self.selection.iter().filter_map(|i| model.wall(*i).filter(|_| crate::curtain::is_curtain(&model, *i))) {
                    let ring = bimcraft_core::curtain::outline_on_face(&model, w).iter().map(|c| screen(*c)).collect();
                    crate::curtain::mark_wall(&p, ring);
                }
            }
            let aim = crate::curtain::wall_to_tab(&model, &self.selection, &self.tool).zip(local).and_then(|(wall, q)| {
                let at = (f64::from(q.0), f64::from(q.1));
                let laid = self.curtain_laid.of(&model, wall);
                let list = crate::curtain::parts_on_ray(
                    &model,
                    &laid,
                    id,
                    wall,
                    cam.point_at(at, 0.0),
                    cam.point_at(at, 1000.0),
                    1.0 / view.scale,
                )?;
                Some((wall, list))
            });
            self.curtain_part_aim(aim);
            let face_mark = |p: &egui::Painter, wall: Id, part, picked: bool| {
                let l = self.curtain_laid.of(&model, wall);
                for ring in model.wall(wall).map_or_else(Vec::new, |w| bimcraft_core::curtain::face_rings(&l, &model, w, part)) {
                    crate::curtain::mark_part(p, ring.iter().map(|c| screen(*c)).collect(), picked);
                }
            };
            if let Some((wall, part)) = self.curtain_aim {
                face_mark(&p, wall, part, false);
            }
            if let Some((part_of, _)) = self.curtain_part {
                for part in self.curtain_parts_picked() {
                    face_mark(&p, part_of, part, true);
                }
            }
            // Add/Remove Segments: the stretch of line a click would take off or put back.
            if let Tool::CurtainSegment { wall } = self.tool {
                let aim = local.zip(model.wall(wall)).and_then(|(q, w)| {
                    let at = (f64::from(q.0), f64::from(q.1));
                    bimcraft_core::curtain::on_plane(&model, w, cam.point_at(at, 0.0), cam.point_at(at, 1000.0))
                });
                self.curtain_segment_aim(aim);
                if let Some((wall, at)) = self.curtain_segment_hover
                    && let Some([a, b]) = model.wall(wall).and_then(|w| bimcraft_core::curtain::segment_on_face(&model, w, at))
                {
                    p.line_segment([screen(a), screen(b)], egui::Stroke::new(3.0, canvas_pal().accent));
                }
            }
            // The Curtain Grid tool: where the cursor would place a line.
            if let Tool::CurtainGrid { wall, .. } = self.tool {
                let aim = local.zip(model.wall(wall)).and_then(|(q, w)| {
                    let at = (f64::from(q.0), f64::from(q.1));
                    bimcraft_core::curtain::on_plane(&model, w, cam.point_at(at, 0.0), cam.point_at(at, 1000.0))
                });
                self.curtain_grid_aim(aim);
                if let Some((wall, at, way)) = self.curtain_grid_hover()
                    && let Some(w) = model.wall(wall)
                {
                    let [a, b] = bimcraft_core::curtain::line_on_face(&model, w, way, at);
                    p.line_segment([screen(a), screen(b)], egui::Stroke::new(2.0, canvas_pal().accent));
                }
            }
        }
        // While a note is written here, the pointer is its editor's.
        if self.writing_in(id) {
            p.extend(crop);
            p.extend(datum);
            self.note_canvas(ui, &response, &p, place, &d);
            self.nav_bar(ui, rect, id);
            return;
        }
        p.extend(crop);
        p.extend(callout);
        // A drag draws a selection box: it takes what the drawing over the picture has in it, and
        // what the picture shows.
        let held = self.cut.image.take();
        let image = held.as_ref().filter(|_| displaying);
        let inside = |a: (f32, f32), b: (f32, f32), crossing: bool| {
            let (a, b) = (unproject(a), unproject(b));
            let scaled = |q: (f32, f32)| (f64::from(q.0) * source_ppp[0], f64::from(q.1) * source_ppp[1]);
            let mut found = d.in_box(a, b, crossing);
            found.extend(image.map(|i| i.in_box(scaled(a), scaled(b), crossing)).unwrap_or_default());
            found.sort_unstable();
            found.dedup();
            found
        };
        // Edit Profile sketched here: its lines on the wall's face, a drag from a picked line's end holding its grip, not a box.
        let profile = self
            .sketch_face_in(Sheet::Section(id))
            .zip(render::section_camera(&model, id, view))
            .map(|(surface, (cam, _))| crate::sketch3d::Plane { cam, rect, ppp: 1.0, surface });
        let gripped = self.sketch3d_grab(ui, &response, profile);
        let boxed = if gripped { Vec::new() } else { self.drag_box(ui, &response, rect, inside) };
        self.cut.image = held;
        p.extend(boxed);
        p.extend(datum);
        let clicked = self.clicked_at(&response).filter(|_| !picking).map(|at| (at.x - rect.min.x, at.y - rect.min.y));
        if let Some(plane) = profile {
            self.sketch3d(ui, &response, &p, plane, clicked.map(|q| rect.min + egui::vec2(q.0, q.1)), None);
            self.context_menu(&response);
            self.nav_bar(ui, rect, id);
            return;
        }
        // Along the line and up, to the nearest 10 mm.
        let grid = self.background_grid();
        let snapping = self.grid_snapping;
        let snaps = self.snaps_now();
        let at = |q: (f32, f32)| {
            let p = view.mm(q);
            // What the view shows there first: a stair's nosing, a detail line's end.
            let reach = bimcraft_core::snap::Reach { point: crate::canvas::SNAP / view.scale, line: f64::from(PICK) / view.scale };
            let found = bimcraft_core::snap::snap(&model, id, p, reach, &[], false, &snaps);
            if !matches!(found.kind, bimcraft_core::snap::Kind::Free) {
                return found.at;
            }
            if snapping && let Some(at) = grid.nearest(p) {
                return at;
            }
            P2((p.0 / 10.0).round() * 10.0, (p.1 / 10.0).round() * 10.0)
        };
        // A double click on a detail's callout opens the detail.
        if response.double_clicked()
            && let Some(id) = hovered.filter(|id| matches!(model.elements.get(*id), Some(Element::DetailView(_))))
        {
            self.open_sheet(if model.plan_level(id).is_some() { Sheet::Plan(id) } else { Sheet::Section(id) });
            return;
        }
        // A double click on a note writes in it.
        if response.double_clicked()
            && let Some(note) = hovered.filter(|h| crate::notes::is_note(&model, *h))
        {
            self.edit_note(note);
            return;
        }
        if let Tool::ExportRegion(first) = self.tool {
            let cursor = ui.input(|i| i.pointer.latest_pos()).filter(|c| rect.contains(*c));
            if let Some(cursor) = cursor {
                let screen = |a: P2| rect.min + egui::vec2(view.px(a).0, view.px(a).1);
                crate::export::region_band(&ui.painter_at(rect), first.map(screen), cursor);
            }
            if let Some(q) = clicked {
                self.export_region_at(view.mm(q));
            }
            return;
        }
        // What a click would paste: the box round it, its middle at the cursor.
        if self.tool == Tool::Paste
            && let Some(frame) = bimcraft_core::clipboard::Frame::of(&model, id)
            && let Some([lo, hi]) = self.paste_extent(&frame)
            && let Some(cursor) = ui.input(|i| i.pointer.latest_pos()).filter(|c| rect.contains(*c))
        {
            let mid = (lo + hi) * 0.5;
            let c = at((cursor.x - rect.min.x, cursor.y - rect.min.y));
            let screen = |a: P2| rect.min + egui::vec2(view.px(a + c - mid).0, view.px(a + c - mid).1);
            let r = egui::Rect::from_two_pos(screen(lo), screen(hi));
            ui.painter_at(rect).rect_stroke(r, 0.0, egui::Stroke::new(1.0, crate::style::pal().accent), egui::StrokeKind::Middle);
        }
        // Only edit through the model generation being displayed. A stale picture must never
        // lend a reference to geometry from a newer snapshot.
        let fresh = self.cut.shown.as_ref().is_some_and(|(m, v, ..)| *v == id && Arc::ptr_eq(m, &model));
        if self.vertical_tools(ui, &p, rect, view, id, local, clicked.filter(|_| fresh), hovered) {
            self.context_menu(&response);
            self.nav_bar(ui, rect, id);
            return;
        }
        match (clicked, &self.tool) {
            (Some(q), _) if self.curtain_face_tool() => {
                let wall = match self.tool {
                    Tool::CurtainGrid { wall, .. } | Tool::CurtainSegment { wall } => Some(wall),
                    _ => self.curtain_grid_moving().map(|(w, _)| w),
                };
                let on = wall.and_then(|w| model.wall(w)).zip(render::section_camera(&model, id, view)).and_then(|(w, (cam, _))| {
                    let at = (f64::from(q.0), f64::from(q.1));
                    bimcraft_core::curtain::on_plane(&model, w, cam.point_at(at, 0.0), cam.point_at(at, 1000.0))
                });
                self.curtain_face_click(on);
            }
            (Some(q), Tool::Modify) => {
                // A member of a group is picked as its group, as in a plan.
                let found: Vec<Id> = pick(&self.cut, &d, q).into_iter().collect();
                let found = self.group_picks(&self.session.model, &found, true);
                self.pick_up(found, click_modifiers(ui));
            }
            (Some(q), Tool::Demolish) => {
                if let Some(id) = pick(&self.cut, &d, q) {
                    self.demolish(id);
                }
            }
            (Some(q), Tool::Text) => self.write_note(id, at(q)),
            (Some(q), Tool::Paste) => self.paste_at(id, at(q)),
            (Some(q), Tool::PickLevel) => match pick_level(&d, q) {
                Some(l) => self.paste_on_level(l),
                None => self.status = Status { text: crate::lang::t("click a level line").into(), error: true },
            },
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

#[cfg(test)]
mod worker_tests {
    use super::*;
    use crate::Sheet;
    use bimcraft_core::engine::Session;
    use std::time::{Duration, Instant};

    fn harness() -> (egui_kittest::Harness<'static, BimApp>, Id) {
        let mut session = Session::new();
        session.script(r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}"#).unwrap();
        let id = session.execute("section", &json!({"start": [-1000,3000], "end": [9000,3000]})).unwrap()["id"].as_u64().unwrap();
        let mut app = BimApp::new(session);
        app.cut.mode.0 = true;
        app.open_sheet(Sheet::Section(id));
        let harness = egui_kittest::Harness::builder().with_size([1280.0, 800.0]).build_ui_state(|ui, app: &mut BimApp| app.show(ui), app);
        (harness, id)
    }

    fn settle(harness: &mut egui_kittest::Harness<BimApp>, id: Id) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            // Idle before the step, whose poll then takes what the worker finished: idle after it, a frame may wait unpolled.
            let idle = harness.state().cut.worker.as_ref().is_some_and(|w| !w.busy());
            harness.step();
            let cut = &harness.state().cut;
            if idle && cut.shown.as_ref().is_some_and(|(_, shown, ..)| *shown == id) {
                break;
            }
            assert!(Instant::now() < deadline, "worker did not publish a frame: {}", harness.state().status.text);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn the_real_worker_fits_and_renders_a_section_without_blocking_ui_frames() {
        let (mut harness, id) = harness();
        settle(&mut harness, id);
        let shown = harness.state().cut.shown.as_ref().unwrap();
        assert!(harness.state().canvas.has_view(id));
        assert_eq!(harness.state().cut.drawn.as_ref().unwrap().3, shown.2);
        assert_eq!(harness.state().cut.image.as_ref().unwrap().width, shown.3.width);
        harness.state_mut().open_sheet(Sheet::Plan(100000));
        assert!(harness.state().cut.worker.is_none(), "closing a section must cancel its worker");
    }
    #[test]
    fn navigation_reprojects_the_displayed_picture_and_picks_that_same_generation() {
        let (mut harness, id) = harness();
        settle(&mut harness, id);
        let old = Arc::clone(harness.state().cut.image.as_ref().unwrap());
        let source = harness.state().cut.shown.as_ref().unwrap().2;
        let (x, y) = (old.width / 2, old.height / 2);
        let picked = old.pick(f64::from(x), f64::from(y)).unwrap();
        let point = source.mm((x as f32 * source.width as f32 / old.width as f32, y as f32 * source.height as f32 / old.height as f32));
        let (entered, enter) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let mut renderer = Renderer::default();
        let mut first = true;
        let ctx = harness.ctx.clone();
        harness.state_mut().cut.worker = Some(Latest::new(
            move |request, token| {
                if first {
                    first = false;
                    entered.send(()).ok()?;
                    wait.recv().ok()?;
                }
                renderer.render(request, token)
            },
            move || ctx.request_repaint(),
        ));
        harness.state_mut().cut.requested = None;
        harness.state_mut().canvas.show(id, point - P2(6000.0, 4000.0), point + P2(6000.0, 4000.0));
        harness.step();
        enter.recv_timeout(Duration::from_secs(5)).unwrap();
        let at = harness.state().screen(point);
        harness.hover_at(at);
        harness.step();
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            harness.step();
        }
        assert!(harness.state().selection.contains(&picked), "picking must follow the reprojected old picture");
        assert!(Arc::ptr_eq(&old, harness.state().cut.image.as_ref().unwrap()), "the worker is still held");
        release.send(()).unwrap();
        settle(&mut harness, id);
        assert_ne!(harness.state().cut.shown.as_ref().unwrap().2, source);
        harness.state_mut().cut.suspend();
    }
}
