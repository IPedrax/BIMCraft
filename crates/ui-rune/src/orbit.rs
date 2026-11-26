//! The 3D view on screen: the camera it is looked through (the core's `view3d::Camera`: orbited, panned and zoomed as in Revit and
//! as egui's `orbit.rs` does), the picture a thread of its own has made of the model, and what a click, a hover or a box finds in it.
//! The same plan as `section::Cut`: the thread makes the newest request only (`worker::Latest`), and while it works the last picture
//! shows moved and scaled through Rune's live offset and scale when the camera only panned or zoomed; an orbit changes the
//! picture's whole look, so it shows the last one as it was until the new one comes.
//!
//! The processor's picture only (`Scene::render_in`, parallel projection). What a graphics card path would need is in the area doc.

use std::sync::Arc;

use bimcraft_core::ids::Id;
use bimcraft_core::model::Model;
use bimcraft_core::view3d::{Camera, P3};
use rune::{Live, Pixels};

use crate::section::SELECT;
use crate::worker::Latest;

mod render;
pub use render::{Frame, Renderer, Request};

/// Degrees the view turns for a point of the pointer's travel.
const TURN: f64 = 0.4;

/// What a press that moves the camera took: the camera as it was, where the pointer was, and the point it orbits round.
#[derive(Clone, Copy)]
struct Grab {
    camera: Camera,
    at: (f32, f32),
    pivot: Option<P3>,
    orbit: bool,
}

pub struct Orbit {
    /// Where the view looks from; none until the first picture, which fits the model.
    pub camera: Option<Camera>,
    /// Zoom to Fit: the next picture fits the model from where the camera looks from.
    refit: bool,
    /// The canvas's size, points.
    pub size: (f32, f32),
    /// The 3D view shown: `Some(None)` is the default `{3D}`.
    pub shown: Option<Option<Id>>,
    pub frame: Option<Frame>,
    /// Counts the pictures taken in.
    pub revision: u64,
    pub pixels: Pixels,
    pub live_offset: Live<[f32; 2]>,
    pub live_scale: Live<f32>,
    /// What the live offset and scale hold, to take a point of the canvas back into the picture.
    live: (f32, [f32; 2]),
    pub cursor: Option<(f32, f32)>,
    grab: Option<Grab>,
    worker: Option<Latest<Request, Frame>>,
    asked: Option<Request>,
    pub error: Option<String>,
}

impl Default for Orbit {
    fn default() -> Orbit {
        Orbit::new()
    }
}

/// Whether a camera can be asked of the core: every number finite, a scale above nothing.
fn sound(c: &Camera) -> bool {
    c.azimuth.is_finite() && c.elevation.is_finite() && c.scale.is_finite() && c.scale > 0.0 && c.target.iter().all(|v| v.is_finite())
}

impl Orbit {
    pub fn new() -> Orbit {
        Orbit {
            camera: None,
            refit: false,
            size: (900.0, 640.0),
            shown: None,
            frame: None,
            revision: 0,
            pixels: Pixels::new(),
            live_offset: Live::new([0.0, 0.0]),
            live_scale: Live::new(1.0),
            live: (1.0, [0.0, 0.0]),
            cursor: None,
            grab: None,
            worker: None,
            asked: None,
            error: None,
        }
    }

    /// Shows 3D view `view` (none: the default) on a canvas of `size`, from scratch.
    pub fn show(&mut self, view: Option<Id>, size: (f32, f32)) {
        if self.shown == Some(view) {
            return;
        }
        *self = Orbit::new();
        self.shown = Some(view);
        if size.0.is_finite() && size.1.is_finite() && size.0 >= 2.0 && size.1 >= 2.0 {
            self.size = size;
        }
    }

    /// Another kind of view is shown: the thread, the picture and the camera are let go.
    pub fn stop(&mut self) {
        if self.shown.is_some() || self.worker.is_some() {
            *self = Orbit::new();
        }
    }

    pub fn busy(&self) -> bool {
        self.worker.as_ref().is_some_and(Latest::busy)
    }

    /// The canvas has laid out at `w` x `h` points: true when the picture has to be made again for it.
    pub fn resize(&mut self, w: f32, h: f32) -> bool {
        if !(w.is_finite() && h.is_finite()) || w < 2.0 || h < 2.0 || ((w - self.size.0).abs() < 0.5 && (h - self.size.1).abs() < 0.5) {
            return false;
        }
        self.size = (w, h);
        let [pw, ph] = self.side();
        if let Some(c) = &mut self.camera {
            (c.width, c.height) = (pw, ph);
        }
        true
    }

    fn side(&self) -> [u32; 2] {
        let side = |v: f32| if v.is_finite() { v.clamp(1.0, 16384.0) as u32 } else { 1 };
        [side(self.size.0), side(self.size.1)]
    }

    /// Asks the thread for the picture for the camera as it is, `model`, and the `selected` elements tinted in it. Nothing is sent when
    /// that is what it was last asked for.
    pub fn ask(&mut self, model: &Arc<Model>, selected: &[Id]) {
        let Some(view) = self.shown else { return };
        let [w, h] = self.side();
        let request = Request {
            model: Arc::clone(model),
            view,
            camera: self.camera.map(|c| Camera { width: w, height: h, ..c }),
            fit: self.refit,
            size: [w, h],
            tints: selected.iter().map(|id| (*id, SELECT)).collect(),
        };
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

    /// Takes in the picture the thread has made, if there is one for the newest request: true when what is on screen changed.
    pub fn poll(&mut self) -> bool {
        let Some(answer) = self.worker.as_mut().and_then(Latest::take) else { return false };
        let frame = match answer {
            Ok(frame) if Some(frame.request.view) == self.shown => frame,
            Ok(_) => return false,
            Err(why) => {
                self.error = Some(why);
                return true;
            }
        };
        if self.camera.is_none() || frame.request.fit {
            self.camera = Some(frame.camera);
        }
        // A fit that is answered is done, unless another was asked for since.
        if frame.request.fit && self.asked.as_ref().is_some_and(|r| r.fit) {
            self.refit = false;
        }
        // What it answers, as asked for the camera it took: the next ask with the camera as it is is the same request.
        let mut answered = frame.request.clone();
        answered.camera = Some(frame.camera);
        answered.fit = false;
        self.asked = Some(answered);
        let image = &frame.image;
        if (image.width as usize).checked_mul(image.height as usize).and_then(|n| n.checked_mul(4)) == Some(image.rgba.len()) {
            self.pixels.set(image.width, image.height, image.rgba.clone());
        }
        self.error = None;
        self.frame = Some(frame);
        self.revision += 1;
        self.glide();
        true
    }

    /// The picture on screen follows a camera that only panned or zoomed through the live offset and scale. One that turned has
    /// nothing to follow with: the last picture stays as it was.
    fn glide(&mut self) {
        let (w, h) = self.size;
        let live = match (&self.frame, &self.camera) {
            (Some(f), Some(c)) if c.azimuth == f.camera.azimuth && c.elevation == f.camera.elevation && c.lens == f.camera.lens => {
                let k = c.scale / f.camera.scale;
                let (x, y, _) = Camera { width: w as u32, height: h as u32, ..*c }.project(f.camera.target);
                (k.is_finite() && k > 0.0 && x.is_finite() && y.is_finite()).then_some((k as f32, [x as f32 - w / 2.0, y as f32 - h / 2.0]))
            }
            _ => None,
        };
        self.live = live.unwrap_or((1.0, [0.0, 0.0]));
        self.live_scale.set(self.live.0);
        self.live_offset.set(self.live.1);
    }

    /// A point of the canvas as the picture made has it.
    fn shown_at(&self, (x, y): (f32, f32)) -> (f64, f64) {
        let (w, h) = self.size;
        let (k, [ox, oy]) = self.live;
        (f64::from((x - w / 2.0 - ox) / k + w / 2.0), f64::from((y - h / 2.0 - oy) / k + h / 2.0))
    }

    /// The element under a point of the canvas: what the picture shows there.
    pub fn pick(&self, x: f32, y: f32) -> Option<Id> {
        if !(x.is_finite() && y.is_finite()) {
            return None;
        }
        let (px, py) = self.shown_at((x, y));
        self.frame.as_ref()?.image.pick(px, py)
    }

    /// The elements in the box from `a` to `b` (points of the canvas): those the picture shows only inside it, or with `crossing` any.
    pub fn in_box(&self, a: (f32, f32), b: (f32, f32), crossing: bool) -> Vec<Id> {
        let Some(f) = &self.frame else { return Vec::new() };
        if ![a.0, a.1, b.0, b.1].iter().all(|v| v.is_finite()) {
            return Vec::new();
        }
        f.image.in_box(self.shown_at(a), self.shown_at(b), crossing)
    }

    /// The wheel: closer (`factor` above 1) about the point under the pointer. Before the first picture there is no camera: nothing moves.
    pub fn zoom(&mut self, at: (f32, f32), factor: f64) {
        if let Some(c) = self.camera
            && at.0.is_finite()
            && at.1.is_finite()
            && factor.is_finite()
            && factor > 0.0
        {
            let zoomed = c.zoom(factor, (f64::from(at.0), f64::from(at.1)));
            if sound(&zoomed) {
                self.camera = Some(zoomed);
                self.glide();
            }
        }
    }

    /// A press at `at` begins to move the camera: it orbits (Shift, as Revit's) round the point of the surface under it, or pans.
    pub fn grab(&mut self, at: (f32, f32), orbit: bool) {
        let Some(camera) = self.camera else { return };
        let pivot = orbit.then(|| self.frame.as_ref()?.image.point(&camera, self.shown_at(at).0, self.shown_at(at).1)).flatten();
        self.grab = Some(Grab { camera, at, pivot, orbit });
    }

    /// The pointer has travelled `dx`, `dy` points since the press: the camera as it was, orbited or panned by that.
    pub fn drag(&mut self, dx: f32, dy: f32) {
        let Some(g) = self.grab else { return };
        if !(dx.is_finite() && dy.is_finite()) {
            return;
        }
        let (dx, dy) = (f64::from(dx), f64::from(dy));
        let moved = if g.orbit {
            g.camera.orbit_about(-dx * TURN, dy * TURN, g.pivot.unwrap_or(g.camera.target))
        } else {
            let from = (f64::from(g.at.0), f64::from(g.at.1));
            g.camera.pan(from, (from.0 + dx, from.1 + dy))
        };
        if sound(&moved) {
            self.camera = Some(moved);
            self.glide();
        }
    }

    /// The press that moved the camera is let go.
    pub fn release(&mut self) {
        self.grab = None;
    }

    /// Zoom to Fit: the model is framed again from where the camera looks from, when the next picture is made.
    pub fn fit(&mut self) {
        self.refit = true;
    }
}

#[cfg(test)]
mod tests;
