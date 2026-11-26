//! The sheet on screen: the camera it is looked at through (paper millimetres, y up, as egui frames it: the whole paper, 24 points
//! clear of the edges), the picture a thread of its own has painted of the core's drawing of it, and what a click, a hover or a box
//! finds there. The same plan as `section::Cut`, for a sheet: the old picture moves through Rune's live offset and scale while the
//! thread paints the next at the new zoom, and `worker::Latest` makes only the newest request.
//!
//! A sheet's drawing is in paper pixels (`sheet::DENSITY` to the millimetre, its origin the paper's top left), so a point of the
//! canvas is taken back to the picture's canvas, to millimetres through the picture's view, and to those pixels (`drawn_at`).

use std::collections::BTreeSet;
use std::sync::Arc;

use bimcraft_core::geom::P2;
use bimcraft_core::ids::Id;
use bimcraft_core::model::{Model, paper};
use bimcraft_core::render::Drawing;
use bimcraft_core::sheet::DENSITY;
use rune::{Live, Pixels};

use crate::camera::Camera;
use crate::worker::Latest;

mod render;
pub use render::{Frame, Renderer, Request};

/// How near a click or a hover must be to a line to take it, points.
const REACH: f32 = 4.0;
/// The room left clear round the paper when it is framed, points a side.
const MARGIN: f64 = 24.0;

pub struct Paper {
    /// Where the view looks and how close (paper millimetres, y up).
    pub camera: Camera,
    /// The canvas's size, points.
    pub size: (f32, f32),
    /// The sheet shown.
    pub id: Option<Id>,
    fitted: bool,
    /// A person has moved the camera: a resize no longer frames the paper again.
    pub moved: bool,
    /// The last picture painted, for the camera `frame.view` has.
    pub frame: Option<Frame>,
    /// Counts the pictures taken in.
    pub revision: u64,
    pub pixels: Pixels,
    pub live_offset: Live<[f32; 2]>,
    pub live_scale: Live<f32>,
    pub cursor: Option<(f32, f32)>,
    pub followed: (f32, f32),
    worker: Option<Latest<Request, Frame>>,
    asked: Option<Request>,
    pub error: Option<String>,
}

impl Default for Paper {
    fn default() -> Paper {
        Paper::new()
    }
}

/// The camera that frames a paper `paper` mm in a canvas of `size` points, clear of the edges.
pub fn framed(paper: P2, size: (f32, f32)) -> Camera {
    let room = |px: f32, mm: f64| (f64::from(px) - 2.0 * MARGIN).max(1.0) / mm.max(1.0);
    Camera { cx: paper.0 / 2.0, cy: paper.1 / 2.0, scale: room(size.0, paper.0).min(room(size.1, paper.1)), angle: 0.0 }
}

/// The box the elements `owner` owns take in a drawing, in its pixels: left, top, right, bottom. None when it owns nothing.
pub fn owner_box(d: &Drawing, owner: Id) -> Option<[f32; 4]> {
    let mut found: Option<[f32; 4]> = None;
    for (prim, o) in d.prims.iter().zip(&d.owners) {
        if *o != Some(owner) {
            continue;
        }
        for (x, y) in prim.points() {
            if x.is_finite() && y.is_finite() {
                found = Some(match found {
                    Some([l, t, r, b]) => [l.min(x), t.min(y), r.max(x), b.max(y)],
                    None => [x, y, x, y],
                });
            }
        }
    }
    found
}

impl Paper {
    pub fn new() -> Paper {
        Paper {
            camera: Camera { cx: 0.0, cy: 0.0, scale: 1.0, angle: 0.0 },
            size: (900.0, 640.0),
            id: None,
            fitted: false,
            moved: false,
            frame: None,
            revision: 0,
            pixels: Pixels::new(),
            live_offset: Live::new([0.0, 0.0]),
            live_scale: Live::new(1.0),
            cursor: None,
            followed: (0.0, 0.0),
            worker: None,
            asked: None,
            error: None,
        }
    }

    /// Shows sheet `id` on a canvas of `size`, from scratch.
    pub fn show(&mut self, id: Id, size: (f32, f32)) {
        if self.id == Some(id) {
            return;
        }
        *self = Paper::new();
        self.id = Some(id);
        if size.0.is_finite() && size.1.is_finite() && size.0 >= 2.0 && size.1 >= 2.0 {
            self.size = size;
        }
    }

    /// Another kind of view is shown: the thread and the picture are let go.
    pub fn stop(&mut self) {
        if self.id.is_some() || self.worker.is_some() {
            *self = Paper::new();
        }
    }

    pub fn busy(&self) -> bool {
        self.worker.as_ref().is_some_and(Latest::busy)
    }

    /// The canvas has laid out at `w` x `h` points: true when the picture has to be painted again for it.
    pub fn resize(&mut self, w: f32, h: f32) -> bool {
        if !(w.is_finite() && h.is_finite()) || w < 2.0 || h < 2.0 || ((w - self.size.0).abs() < 0.5 && (h - self.size.1).abs() < 0.5) {
            return false;
        }
        self.size = (w, h);
        if !self.moved {
            self.fitted = false;
        }
        true
    }

    /// Asks the thread for the picture for the camera as it is and `model`: nothing is sent when that is what it was last asked for.
    pub fn ask(&mut self, model: &Arc<Model>) {
        let Some(id) = self.id else { return };
        let Some(page) = model.sheet(id).and_then(|s| paper(&s.size)) else { return };
        if !self.fitted {
            self.camera = framed(page, self.size);
            self.fitted = true;
        }
        let request = Request { model: Arc::clone(model), id, view: self.camera.view(self.size.0, self.size.1) };
        if self.asked.as_ref().is_some_and(|r| r.same(&request)) {
            return;
        }
        if self.worker.is_none() {
            let mut renderer = Renderer::default();
            match Latest::new(move |r, cancelled| renderer.render(r, cancelled)) {
                Ok(worker) => self.worker = Some(worker),
                Err(why) => {
                    self.error = Some(why);
                    return;
                }
            }
        }
        if let Some(worker) = &mut self.worker {
            match worker.request(request.clone()) {
                Ok(()) => self.asked = Some(request),
                Err(why) => {
                    self.error = Some(why);
                    self.worker = None;
                }
            }
        }
    }

    /// Takes in the picture the thread has painted, if there is one for the newest request: true when what is on screen changed.
    pub fn poll(&mut self) -> bool {
        let Some(answer) = self.worker.as_mut().and_then(Latest::take) else { return false };
        let frame = match answer {
            Ok(frame) if Some(frame.request.id) == self.id => frame,
            Ok(_) => return false,
            Err(why) => {
                self.error = Some(why);
                return true;
            }
        };
        if let Some([_, _, w, h]) = frame.at
            && (w as usize).checked_mul(h as usize).and_then(|n| n.checked_mul(4)) == Some(frame.rgba.len())
        {
            self.pixels.set(w as u32, h as u32, frame.rgba.clone());
        }
        self.error = None;
        self.frame = Some(frame);
        self.revision += 1;
        self.glide();
        true
    }

    /// The picture on screen follows the camera through the live offset and scale: it was painted for `frame.view`.
    fn glide(&mut self) {
        let (w, h) = self.size;
        let live = self.frame.as_ref().and_then(|f| {
            let k = self.camera.scale / f.view.scale;
            let at = self.camera.view(w, h).px(P2(f.view.cx, f.view.cy));
            (k.is_finite() && k > 0.0 && at.0.is_finite() && at.1.is_finite()).then_some((k as f32, [at.0 - w / 2.0, at.1 - h / 2.0]))
        });
        let (k, offset) = live.unwrap_or((1.0, [0.0, 0.0]));
        self.live_scale.set(k);
        self.live_offset.set(offset);
    }

    fn live_k(&self) -> f32 {
        let k = self.frame.as_ref().map_or(1.0, |f| self.camera.scale / f.view.scale);
        if k.is_finite() && k > 0.0 { k as f32 } else { 1.0 }
    }

    /// A point of the canvas as the picture painted has it (its own canvas's).
    fn shown_at(&self, (x, y): (f32, f32)) -> (f32, f32) {
        let Some(f) = &self.frame else { return (x, y) };
        let (w, h) = self.size;
        let at = self.camera.view(w, h).px(P2(f.view.cx, f.view.cy));
        let k = self.live_k();
        ((x - at.0) / k + w / 2.0, (y - at.1) / k + h / 2.0)
    }

    /// A point of the canvas in the drawing's pixels: through the picture's view to the paper's millimetres, and from the paper's top left.
    fn drawn_at(&self, q: (f32, f32)) -> Option<(f32, f32)> {
        let f = self.frame.as_ref()?;
        let mm = f.view.mm(self.shown_at(q));
        let (x, y) = (mm.0 * DENSITY, (f.paper.1 - mm.1) * DENSITY);
        (x.is_finite() && y.is_finite()).then_some((x as f32, y as f32))
    }

    /// The element under a point of the canvas: a view on the sheet, a note, a line of the title block.
    pub fn pick(&self, x: f32, y: f32) -> Option<Id> {
        if !(x.is_finite() && y.is_finite()) {
            return None;
        }
        let f = self.frame.as_ref()?;
        let at = self.drawn_at((x, y))?;
        let k = (f.view.scale / DENSITY) as f32 * self.live_k();
        f.drawing.pick(at, REACH / k)
    }

    /// The elements in the box from `a` to `b` (points of the canvas): those that are all inside it, or with `crossing` any that touch it.
    pub fn in_box(&self, a: (f32, f32), b: (f32, f32), crossing: bool) -> Vec<Id> {
        let Some(f) = &self.frame else { return Vec::new() };
        let (Some(a), Some(b)) = (self.drawn_at(a), self.drawn_at(b)) else { return Vec::new() };
        f.drawing.in_box(a, b, crossing).into_iter().collect::<BTreeSet<Id>>().into_iter().collect()
    }

    /// Where the elements `owners` stand on the canvas the picture was painted for: left, top, right, bottom of each, points.
    pub fn boxes(&self, owners: &[Id]) -> Vec<[f32; 4]> {
        let Some(f) = &self.frame else { return Vec::new() };
        owners
            .iter()
            .filter_map(|id| {
                let [l, t, r, b] = owner_box(&f.drawing, *id)?;
                let to = |x: f32, y: f32| f.view.px(P2(f64::from(x) / DENSITY, f.paper.1 - f64::from(y) / DENSITY));
                let ((x0, y0), (x1, y1)) = (to(l, t), to(r, b));
                Some([x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)])
            })
            .collect()
    }

    /// The wheel: closer (`factor` above 1) about `at`. Before the first request the camera is not framed: nothing moves.
    pub fn zoom(&mut self, at: (f32, f32), factor: f64) {
        if self.fitted {
            self.camera.zoom_about(at, self.size, factor);
            self.moved = true;
            self.glide();
        }
    }

    /// The pointer has travelled `dx`, `dy` points since the press: the sheet follows it.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        if self.fitted && dx.is_finite() && dy.is_finite() {
            self.camera.pan(dx - self.followed.0, dy - self.followed.1, self.size);
            (self.followed, self.moved) = ((dx, dy), true);
            self.glide();
        }
    }

    /// Zoom In Region over the box from `a` to `b`: false for a box with no size.
    pub fn zoom_region(&mut self, a: (f32, f32), b: (f32, f32)) -> bool {
        let Some(camera) = self.fitted.then(|| self.camera.region(a, b, self.size)).flatten() else { return false };
        (self.camera, self.moved) = (camera, true);
        self.glide();
        true
    }

    /// Zoom to Fit: the paper is framed again at the next ask.
    pub fn fit(&mut self) {
        (self.fitted, self.moved) = (false, false);
    }
}

#[cfg(test)]
mod tests;
