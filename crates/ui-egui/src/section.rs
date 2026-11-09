//! Sections and elevations: the core's picture of what the view sees past its line, its cut,
//! levels, grids, notes and spot elevations drawn over it, panned and zoomed like a plan (its
//! points run along its line and up). A click picks what it shows: a cut, a level, a grid, or what
//! lies beyond; a drag, a selection box; with Text or Spot Elevation, a click places one. Its crop
//! region's grips drag its edges.

use std::sync::Arc;

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Id, Model};
use bimcraft_core::render::{self, Rgb, View};
use bimcraft_core::view3d::{Image, MAX_SIDE};
use egui::{Color32, ColorImage, Context, Rect, Sense, TextureHandle, TextureOptions, Ui, pos2};
use serde_json::json;

use crate::canvas::{PICK, Place, click_modifiers, color, paint};
use crate::orbit::rgb;
use crate::section_worker::{Frame, Renderer, Request};
use crate::style::canvas_pal;
use crate::view_worker::Latest;
use crate::{BimApp, Status, Tool};

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
    tints: Vec<(Id, Rgb)>,
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
        let grid_background = self.background_grid().shown();
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
                        tints: self.selection.iter().map(|id| (*id, rgb(canvas_pal().accent))).collect(),
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
        let crop = self.crop_grips(ui, &response, rect, &view, id);
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
        let grid_background = self.background_grid().shown();
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
        let hovered = local.filter(|_| matches!(self.tool, Tool::Modify | Tool::Demolish)).and_then(|q| pick(&self.cut, &d, q));
        let tints: Vec<(Id, Rgb)> = self
            .selection
            .iter()
            .map(|id| (*id, rgb(canvas_pal().accent)))
            .chain(hovered.filter(|h| !self.selection.contains(h)).map(|h| (h, rgb(canvas_pal().hover))))
            .collect();
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
        if let Some(t) = self.cut.texture.as_ref().filter(|_| displaying) {
            let image_rect = Rect::from_min_size(place.origin, egui::vec2(source.width as f32, source.height as f32) * k);
            p.image(t.id(), image_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        paint(&p, place, &d, &self.selection, hovered, self.thin, &[]);
        // While a note is written here, the pointer is its editor's.
        if self.writing_in(id) {
            p.extend(crop);
            p.extend(datum);
            self.note_canvas(ui, &response, &p, place, &d);
            self.nav_bar(ui, rect, id);
            return;
        }
        p.extend(crop);
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
                let found = pick(&self.cut, &d, q);
                self.pick_up(found.into_iter().collect(), click_modifiers(ui));
            }
            (Some(q), Tool::Demolish) => {
                if let Some(id) = pick(&self.cut, &d, q) {
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
