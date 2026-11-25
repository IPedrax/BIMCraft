//! The section or elevation on screen: the camera it is looked at through (the core's `View`: along its line, and up), the picture
//! and drawing a thread of its own has made for it, and what a click, a hover or a box finds there. No toolkit but the two handles Rune
//! shows an image and a moved layer through (`Pixels`, `Live`), so what it does is tested with no window.
//!
//! **A thread, not the interface's.** The core's picture of a section is a software render of everything the view sees, behind a
//! `SectionWork` of the whole model: seconds on the tower. So `ask` hands the newest request to a thread of its own (`worker::Latest`:
//! a newer one cancels the job being made and replaces any waiting) and the window polls for the answer. While it works, the last
//! picture is shown moved and scaled through Rune's live offset and scale, as the plan is under `CameraMode::Live`, and picking maps the
//! pointer back into it (`drawn_at`).

use std::collections::BTreeSet;
use std::sync::Arc;

use bimcraft_core::geom::P2;
use bimcraft_core::ids::Id;
use bimcraft_core::model::Model;
use bimcraft_core::render::Rgb;
use rune::{Live, Pixels};

use crate::camera::Camera;
use crate::worker::Latest;

mod render;
pub use render::{Frame, Renderer, Request};

/// How near a click or a hover must be to a line to take it, points.
const REACH: f32 = 4.0;
/// What the selected elements are tinted in the picture: the plan's accent.
pub const SELECT: Rgb = Rgb(255, 170, 40);

pub struct Cut {
    /// Where the view looks and how close (`View` of a section: the first coordinate along its line, the second up).
    pub camera: Camera,
    /// The canvas's size, points.
    pub size: (f32, f32),
    /// The section shown.
    pub id: Option<Id>,
    fitted: bool,
    /// A person has moved the camera: a resize no longer frames the view again.
    pub moved: bool,
    /// The last picture made, for the camera `frame.view` has: shown through `live_offset` and `live_scale` while the camera is elsewhere.
    pub frame: Option<Frame>,
    /// Counts the pictures taken in: what the canvas's kept shapes are keyed by.
    pub revision: u64,
    /// The picture, as Rune shows an image.
    pub pixels: Pixels,
    pub live_offset: Live<[f32; 2]>,
    pub live_scale: Live<f32>,
    /// The pointer over the canvas, points from its top left.
    pub cursor: Option<(f32, f32)>,
    /// How far the press now dragging has been followed, points.
    pub followed: (f32, f32),
    worker: Option<Latest<Request, Frame>>,
    asked: Option<Request>,
    /// Why the last picture could not be made.
    pub error: Option<String>,
}

impl Default for Cut {
    fn default() -> Cut {
        Cut::new()
    }
}

impl Cut {
    pub fn new() -> Cut {
        Cut {
            camera: Camera { cx: 0.0, cy: 0.0, scale: 0.1, angle: 0.0 },
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

    /// Shows section `id` on a canvas of `size`, from scratch: nothing of another section carries over.
    pub fn show(&mut self, id: Id, size: (f32, f32)) {
        if self.id == Some(id) {
            return;
        }
        *self = Cut::new();
        self.id = Some(id);
        if size.0.is_finite() && size.1.is_finite() && size.0 >= 2.0 && size.1 >= 2.0 {
            self.size = size;
        }
    }

    /// Another kind of view is shown: the thread, the picture and the camera are let go.
    pub fn stop(&mut self) {
        if self.id.is_some() || self.worker.is_some() {
            *self = Cut::new();
        }
    }

    /// Whether the newest request is not answered yet.
    pub fn busy(&self) -> bool {
        self.worker.as_ref().is_some_and(Latest::busy)
    }

    /// The canvas has laid out at `w` x `h` points: true when the picture has to be made again for it.
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

    /// Asks the thread for the picture for the camera as it is, `model`, and the `selected` elements tinted in it. Nothing is sent when
    /// that is what it was last asked for, so this can be called after anything that may have changed one of them.
    pub fn ask(&mut self, model: &Arc<Model>, selected: &[Id]) {
        let Some(id) = self.id else { return };
        let side = |v: f32| if v.is_finite() { v.clamp(1.0, 16384.0) as u32 } else { 1 };
        let request = Request {
            model: Arc::clone(model),
            id,
            view: self.fitted.then(|| self.camera.view(self.size.0, self.size.1)),
            size: [side(self.size.0), side(self.size.1)],
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
            Ok(frame) if Some(frame.request.id) == self.id => frame,
            Ok(_) => return false,
            Err(why) => {
                self.error = Some(why);
                return true;
            }
        };
        if !self.fitted {
            self.camera = Camera::from_view(frame.view);
            self.fitted = true;
        }
        // What it answers, as asked for the view it fitted: the next ask with the camera as it is is the same request.
        let mut answered = frame.request.clone();
        answered.view = Some(frame.view);
        self.asked = Some(answered);
        if let Some(image) = &frame.image
            && (image.width as usize).checked_mul(image.height as usize).and_then(|n| n.checked_mul(4)) == Some(image.rgba.len())
        {
            self.pixels.set(image.width, image.height, image.rgba.clone());
        }
        self.error = None;
        self.frame = Some(frame);
        self.revision += 1;
        self.glide();
        true
    }

    /// The picture on screen follows the camera through the live offset and scale: it was made for `frame.view`.
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

    /// How much bigger the picture on screen is than the one made: 1 while the camera is where it was.
    fn live_k(&self) -> f32 {
        let k = self.frame.as_ref().map_or(1.0, |f| self.camera.scale / f.view.scale);
        if k.is_finite() && k > 0.0 { k as f32 } else { 1.0 }
    }

    /// A point of the canvas as the picture made has it.
    fn drawn_at(&self, (x, y): (f32, f32)) -> (f32, f32) {
        let Some(f) = &self.frame else { return (x, y) };
        let (w, h) = self.size;
        let at = self.camera.view(w, h).px(P2(f.view.cx, f.view.cy));
        let k = self.live_k();
        ((x - at.0) / k + w / 2.0, (y - at.1) / k + h / 2.0)
    }

    /// A point of the drawing, in the picture's pixels (which are the drawing's points, to the limit of the core's image size).
    fn image_at(f: &Frame, (x, y): (f32, f32)) -> Option<(f64, f64)> {
        let (w, h) = (f64::from(f.view.width), f64::from(f.view.height));
        (w > 0.0 && h > 0.0).then(|| (f64::from(x) * f64::from(f.pixels.width) / w, f64::from(y) * f64::from(f.pixels.height) / h))
    }

    /// The element under a point of the canvas: what is drawn over the picture, else what the picture shows there.
    pub fn pick(&self, x: f32, y: f32) -> Option<Id> {
        if !(x.is_finite() && y.is_finite()) {
            return None;
        }
        let f = self.frame.as_ref()?;
        let at = self.drawn_at((x, y));
        f.drawing.pick(at, REACH / self.live_k()).or_else(|| {
            let (ix, iy) = Self::image_at(f, at)?;
            f.image.as_ref()?.pick(ix, iy)
        })
    }

    /// The elements in the box from `a` to `b` (points of the canvas): those that are all inside it, or with `crossing` any that touch it.
    pub fn in_box(&self, a: (f32, f32), b: (f32, f32), crossing: bool) -> Vec<Id> {
        let Some(f) = &self.frame else { return Vec::new() };
        if ![a.0, a.1, b.0, b.1].iter().all(|v| v.is_finite()) {
            return Vec::new();
        }
        let (a, b) = (self.drawn_at(a), self.drawn_at(b));
        let mut found: BTreeSet<Id> = f.drawing.in_box(a, b, crossing).into_iter().collect();
        if let (Some(image), Some(pa), Some(pb)) = (&f.image, Self::image_at(f, a), Self::image_at(f, b)) {
            found.extend(image.in_box(pa, pb, crossing));
        }
        found.into_iter().collect()
    }

    /// The wheel: closer (`factor` above 1) about `at`. The camera is not fitted before the first picture: nothing moves then.
    pub fn zoom(&mut self, at: (f32, f32), factor: f64) {
        if self.fitted {
            self.camera.zoom_about(at, self.size, factor);
            self.moved = true;
            self.glide();
        }
    }

    /// The pointer has travelled `dx`, `dy` points since the press: the view follows it.
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

    /// Zoom to Fit: the view is framed again when the next picture comes.
    pub fn fit(&mut self) {
        (self.fitted, self.moved) = (false, false);
    }
}

#[cfg(test)]
mod tests;
