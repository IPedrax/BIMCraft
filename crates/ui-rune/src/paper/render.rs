//! What the sheet's thread makes: the core's drawing of the sheet (its title block, its views at their scales, its schedules and
//! notes: `sheet::draw`, which costs what the views on it cost) once for a model, and a raster of it for the camera, which is what
//! the window shows as one image at the sheet's scale, so lettering and line weights keep to the paper at any zoom. The thread keeps
//! the drawing, so a wheel notch or a pan only paints it again.

use std::sync::Arc;

use bimcraft_core::geom::P2;
use bimcraft_core::ids::Id;
use bimcraft_core::model::{Model, paper};
use bimcraft_core::render::{self, Drawing, View};
use bimcraft_core::sheet::{self, DENSITY};

use crate::worker::Cancelled;

/// What to make: sheet `id` of `model`, through `view` (paper millimetres, y up).
#[derive(Clone)]
pub struct Request {
    pub model: Arc<Model>,
    pub id: Id,
    pub view: View,
}

impl Request {
    pub fn same(&self, other: &Request) -> bool {
        Arc::ptr_eq(&self.model, &other.model) && self.id == other.id && self.view == other.view
    }
}

/// The answer: the drawing (kept for picking and for what is lit), and the visible part of the paper painted at the camera's scale.
pub struct Frame {
    pub request: Request,
    pub view: View,
    /// The paper's size, mm.
    pub paper: P2,
    pub drawing: Arc<Drawing>,
    /// Where the image stands on the canvas: left, top, width, height, points; none while the paper is off the canvas.
    pub at: Option<[f32; 4]>,
    pub rgba: Vec<u8>,
}

#[derive(Default)]
pub struct Renderer {
    drawn: Option<(Arc<Model>, Id, Arc<Drawing>)>,
}

impl Renderer {
    /// One job, asking `cancelled` between its steps. A drawing made is kept even when the job is stopped after it.
    pub fn render(&mut self, request: Request, cancelled: Cancelled<'_>) -> Option<Frame> {
        if cancelled() {
            return None;
        }
        let m = &request.model;
        let paper = paper(&m.sheet(request.id)?.size)?;
        if !self.drawn.as_ref().is_some_and(|(old, id, _)| Arc::ptr_eq(old, m) && *id == request.id) {
            self.drawn = None;
            self.drawn = Some((Arc::clone(m), request.id, Arc::new(sheet::draw(m, request.id)?)));
            if cancelled() {
                return None;
            }
        }
        let (_, _, drawing) = self.drawn.as_ref()?;
        let view = request.view;
        let k = (view.scale / DENSITY) as f32;
        let (w, h) = (view.width as f32, view.height as f32);
        // The paper's top left and bottom right corners on the canvas; the part of it the canvas shows is what is painted.
        let (left, top) = view.px(P2(0.0, paper.1));
        let (right, bottom) = view.px(P2(paper.0, 0.0));
        let (x0, y0, x1, y1) = (left.max(0.0).floor(), top.max(0.0).floor(), right.min(w).ceil(), bottom.min(h).ceil());
        let (at, rgba) = if k.is_finite() && k > 0.0 && x1 > x0 && y1 > y0 {
            let (iw, ih) = ((x1 - x0) as u32, (y1 - y0) as u32);
            match render::raster(drawing, iw, ih, k, (left - x0, top - y0), None) {
                Ok(rgba) => (Some([x0, y0, iw as f32, ih as f32]), rgba),
                Err(_) => (None, Vec::new()),
            }
        } else {
            (None, Vec::new())
        };
        if cancelled() {
            return None;
        }
        Some(Frame { request, view, paper, drawing: Arc::clone(drawing), at, rgba })
    }
}
