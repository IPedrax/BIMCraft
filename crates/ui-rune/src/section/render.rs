//! What the section's thread makes: the picture of what the view sees past its line, and the drawing over it (the cut in poché, the
//! levels, the grids, the notes), for a request. egui's `section_worker.rs` is the reference; this is it for one picture an image at
//! one point to a pixel, and both move to the core when egui adopts them (docs/areas/rune-ui.md, extraction 5).
//!
//! The thread keeps the whole model's work (`SectionWork`, made lazily and in chunks the request's `cancelled` can stop) for the
//! model it was made of, and the picture of the last view, so a request that only changes what is selected tints the picture
//! again and draws the overlay, and builds no geometry.

use std::sync::Arc;

use bimcraft_core::ids::Id;
use bimcraft_core::model::Model;
use bimcraft_core::render::{self, Drawing, Rgb, View};
use bimcraft_core::view3d::{Image, MAX_SIDE};

use crate::worker::Cancelled;

/// What to make: section `id` of `model`, through `view` (none: fitted to `size`, which the answer says), the selected elements tinted.
#[derive(Clone)]
pub struct Request {
    pub model: Arc<Model>,
    pub id: Id,
    pub view: Option<View>,
    pub size: [u32; 2],
    pub tints: Vec<(Id, Rgb)>,
}

impl Request {
    /// Whether it asks for what `other` does, of the same model.
    pub fn same(&self, other: &Request) -> bool {
        Arc::ptr_eq(&self.model, &other.model)
            && self.id == other.id
            && self.view == other.view
            && self.size == other.size
            && self.tints == other.tints
    }
}

/// The answer: the picture with which element each of its pixels shows, and the drawing over it, for the view they were made for.
pub struct Frame {
    pub request: Request,
    pub view: View,
    /// The picture's own view: its size in pixels, which is the view's, to the limit the core draws (`MAX_SIDE`).
    pub pixels: View,
    pub image: Option<Arc<Image>>,
    pub drawing: Drawing,
}

type Base = (Id, View, View, render::SectionOverlay, Option<Arc<Image>>);

#[derive(Default)]
pub struct Renderer {
    model: Option<Arc<Model>>,
    work: Option<render::SectionWork>,
    base: Option<Base>,
}

impl Renderer {
    /// One job, asking `cancelled` between its chunks: stopped, it answers nothing, and what the renderer keeps of it is whole, so the
    /// next request of the same model goes on from there.
    pub fn render(&mut self, request: Request, cancelled: Cancelled<'_>) -> Option<Frame> {
        if cancelled() {
            return None;
        }
        let m = &request.model;
        if !self.model.as_ref().is_some_and(|old| Arc::ptr_eq(old, m)) {
            // The old model's geometry goes before the new one's work is made.
            self.base = None;
            self.work = None;
            self.model = None;
            self.work = Some(render::SectionWork::new_deferred_cancellable(m, cancelled)?);
            self.model = Some(Arc::clone(m));
        }
        let work = self.work.as_ref()?;
        let (w, h) = (request.size[0].max(1), request.size[1].max(1));
        let view = match request.view {
            Some(view) => view,
            None => {
                let fit = render::section_fit_cancellable(m, work, request.id, w, h, cancelled)?;
                View { width: w, height: h, ..fit }
            }
        };
        if cancelled() {
            return None;
        }
        let pixels = View { width: view.width.clamp(1, MAX_SIDE), height: view.height.clamp(1, MAX_SIDE), ..view };
        let rebuilt = !self.base.as_ref().is_some_and(|(id, old, p, ..)| *id == request.id && *old == view && *p == pixels);
        if rebuilt {
            self.base = None;
            let overlay = render::SectionOverlay::new_cancellable(m, work, request.id, view, cancelled)?;
            let picture = render::section_picture_cancellable(m, work, request.id, pixels, &[], cancelled).map(Arc::new);
            if cancelled() {
                return None;
            }
            self.base = Some((request.id, view, pixels, overlay, picture));
        }
        let (_, _, _, overlay, picture) = self.base.as_mut()?;
        if let Some(image) = picture {
            // The frame published before still holds the untinted picture: make_mut copies it, the published one stays as it is.
            Arc::make_mut(image).retint_section(&request.tints);
        }
        let drawing = overlay.draw_cancellable(m, picture.as_deref(), cancelled)?;
        Some(Frame { request, view, pixels, image: picture.clone(), drawing })
    }
}
