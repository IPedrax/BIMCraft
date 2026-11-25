//! What the 3D view's thread makes: the core's `Scene` of the model (every solid found and cut to triangles: the heavy step, which
//! an edit brings up to date instead of making again, `Scene::update_cancellable`), and the software picture of it for a camera
//! (`Scene::render_in`, with the selected elements tinted), whose pixels know which element each shows. egui's `orbit.rs` runs the same
//! two steps on its `view_worker`; this is the processor's picture only, as it is shown through `rune::Pixels`. The thread keeps the
//! scene for the model, so a camera that moves, or another selection, paints again and builds no geometry.

use std::sync::Arc;

use bimcraft_core::graphics::Look;
use bimcraft_core::ids::Id;
use bimcraft_core::model::Model;
use bimcraft_core::render::Rgb;
use bimcraft_core::view3d::{AZIMUTH, Camera, ELEVATION, Image, MAX_SIDE, Scene};

use crate::worker::Cancelled;

/// What to make: the picture of `model` as 3D view `view` (none: the default `{3D}`) looks, from `camera` (none: fitted to the model,
/// from the default azimuth and elevation; with `fit`, fitted to it from where `camera` looks from; the answer says which camera it
/// took), the selected elements tinted.
#[derive(Clone)]
pub struct Request {
    pub model: Arc<Model>,
    pub view: Option<Id>,
    pub camera: Option<Camera>,
    pub fit: bool,
    pub size: [u32; 2],
    pub tints: Vec<(Id, Rgb)>,
}

impl Request {
    /// Whether it asks for what `other` does, of the same model.
    pub fn same(&self, other: &Request) -> bool {
        Arc::ptr_eq(&self.model, &other.model)
            && self.view == other.view
            && self.camera == other.camera
            && self.fit == other.fit
            && self.size == other.size
            && self.tints == other.tints
    }
}

/// The answer: the camera it took the picture through (the fit, when none was asked), and the picture.
pub struct Frame {
    pub request: Request,
    pub camera: Camera,
    pub image: Arc<Image>,
}

#[derive(Default)]
pub struct Renderer {
    scene: Option<(Arc<Model>, Arc<Scene>)>,
}

impl Renderer {
    /// One job, asking `cancelled` between its steps: stopped, it answers nothing, and the scene made so far is kept for the next.
    pub fn render(&mut self, request: Request, cancelled: Cancelled<'_>) -> Option<Frame> {
        if cancelled() {
            return None;
        }
        let m = &request.model;
        let kept = self.scene.as_ref().filter(|(model, _)| Arc::ptr_eq(model, m)).map(|(_, scene)| Arc::clone(scene));
        let scene = match kept {
            Some(scene) => scene,
            None => {
                let made = match self.scene.as_ref() {
                    Some((before, scene)) => scene.update_cancellable(before, m, cancelled)?,
                    None => Scene::new_cancellable(m, cancelled)?,
                };
                let made = Arc::new(made);
                self.scene = Some((Arc::clone(m), Arc::clone(&made)));
                made
            }
        };
        if cancelled() {
            return None;
        }
        let look = Look::of_3d_view(m, request.view);
        let (w, h) = (request.size[0].clamp(1, MAX_SIDE), request.size[1].clamp(1, MAX_SIDE));
        let camera = match (request.camera, request.fit) {
            (Some(camera), false) => Camera { width: w, height: h, ..camera },
            (Some(camera), true) => scene.fit_from(Camera { width: w, height: h, lens: None, ..camera }, &look),
            (None, _) => scene.fit_from(Camera { lens: None, ..scene.fit_in(AZIMUTH, ELEVATION, w, h, &look) }, &look),
        };
        let image = Arc::new(scene.render_in(&camera, &request.tints, &look));
        (!cancelled()).then_some(Frame { request, camera, image })
    }
}
