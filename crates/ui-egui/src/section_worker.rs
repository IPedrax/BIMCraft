//! Section jobs retain geometry on the worker and publish picture/picking/overlay together.
use crate::view_worker::Cancel;
use bimcraft_core::model::{Id, Model};
use bimcraft_core::render::{self, Rgb, View};
use bimcraft_core::view3d::{Image, MAX_SIDE};
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct Request {
    pub model: Arc<Model>,
    pub id: Id,
    pub view: Option<View>,
    pub size: [u32; 2],
    pub fit_height: u32,
    pub ppp: f64,
    pub tints: Vec<(Id, Rgb)>,
}

impl Request {
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.model, &other.model)
            && self.id == other.id
            && self.view == other.view
            && self.size == other.size
            && self.fit_height == other.fit_height
            && self.ppp == other.ppp
            && self.tints == other.tints
    }
}

pub(super) struct Frame {
    pub request: Request,
    pub view: View,
    pub pixels: View,
    pub image: Option<Arc<Image>>,
    pub drawing: render::Drawing,
    pub rebuilt: bool,
    pub levels: std::collections::BTreeMap<Id, (f64, f64)>,
}

type Base = (Id, View, View, render::SectionOverlay, Option<Arc<Image>>);

#[derive(Default)]
pub(super) struct Renderer {
    model: Option<Arc<Model>>,
    work: Option<render::SectionWork>,
    base: Option<Base>,
}

impl Renderer {
    pub fn render(&mut self, request: Request, token: &Cancel) -> Option<Frame> {
        self.render_while(request, &|| token.cancelled())
    }

    /// One job, asking `cancelled` between its chunks. Stopped, it publishes nothing; what the renderer keeps of it is
    /// whole (each component's extrema, the snapshot's work), so the next request of the snapshot goes on from there.
    fn render_while(&mut self, request: Request, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<Frame> {
        if cancelled() {
            return None;
        }
        let m = &request.model;
        if !self.model.as_ref().is_some_and(|old| Arc::ptr_eq(old, m)) {
            // Release old geometry before constructing a new snapshot's work.
            self.base = None;
            self.work = None;
            self.model = None;
            self.work = Some(render::SectionWork::new_deferred_cancellable(m, cancelled)?);
            self.model = Some(Arc::clone(m));
        }
        let work = self.work.as_ref()?;
        let view = match request.view {
            Some(view) => view,
            None => {
                let fit = render::section_fit_cancellable(m, work, request.id, request.size[0], request.fit_height, cancelled)?;
                View { width: request.size[0], height: request.size[1], ..fit }
            }
        };
        if cancelled() {
            return None;
        }
        let side = |v: u32| ((f64::from(v) * request.ppp).round().max(1.0) as u32).min(MAX_SIDE);
        let pixels = View { scale: view.scale * request.ppp, width: side(view.width), height: side(view.height), ..view };
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
            Arc::make_mut(image).retint_section(&request.tints);
        }
        let drawing = overlay.draw_cancellable(m, picture.as_deref(), cancelled)?;
        Some(Frame { request, view, pixels, image: picture.clone(), drawing, rebuilt, levels: overlay.levels().clone() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bimcraft_core::engine::Session;
    use serde_json::json;

    fn request() -> Request {
        let mut s = Session::new();
        s.script(r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}"#).unwrap();
        let id = s.execute("section", &json!({"start": [-1000,3000], "end": [9000,3000]})).unwrap()["id"].as_u64().unwrap();
        Request { model: s.model, id, view: None, size: [400, 300], fit_height: 280, ppp: 1.0, tints: Vec::new() }
    }

    /// A desk inside the walls and one past them: only the far one's exact points decide the frame.
    fn request_with_desks() -> Request {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
component {"family":"Desk","type":"1400 x 700","at":[4000,3000],"offset":300}
component {"family":"Desk","type":"1400 x 700","at":[30000,3000],"angle":37,"offset":300}"#,
        )
        .unwrap();
        let id = s.execute("section", &json!({"start": [-1000,-1000], "end": [9000,-1000]})).unwrap()["id"].as_u64().unwrap();
        Request { model: s.model, id, view: None, size: [400, 300], fit_height: 280, ppp: 1.0, tints: Vec::new() }
    }

    fn assert_fresh(frame: &Frame) {
        let request = &frame.request;
        let (m, id) = (&request.model, request.id);
        let fit = render::section_fit(m, id, request.size[0], request.fit_height);
        assert_eq!(frame.view, View { width: request.size[0], height: request.size[1], ..fit });
        let fresh = render::section_picture(m, id, frame.pixels, &[]).unwrap();
        let image = frame.image.as_ref().unwrap();
        assert_eq!(image.rgba, fresh.rgba);
        for y in 0..fresh.height {
            for x in 0..fresh.width {
                let (x, y) = (f64::from(x), f64::from(y));
                assert_eq!((image.pick(x, y), image.face(x, y)), (fresh.pick(x, y), fresh.face(x, y)));
            }
        }
        assert_eq!(render::png(&frame.drawing).unwrap(), render::png(&render::section_over(m, id, frame.view, &fresh)).unwrap());
        let overlay = render::SectionOverlay::new(m, &render::SectionWork::new(m), id, frame.view);
        assert_eq!(&frame.levels, overlay.levels());
    }

    #[test]
    fn the_worker_prepares_component_extrema_lazily_and_publishes_what_the_full_oracle_does() {
        let mut renderer = Renderer::default();
        let frame = renderer.render(request_with_desks(), &Cancel::default()).unwrap();
        assert_fresh(&frame);
        let stats = renderer.work.as_ref().unwrap().preparation_stats();
        assert_eq!((stats.deferred_components, stats.resolved_components), (2, 1), "the desk inside the walls stays deferred");
    }

    #[test]
    fn a_job_cancelled_part_way_publishes_nothing_and_the_next_finds_the_exact_frame() {
        let request = request_with_desks();
        let mut after = 0;
        loop {
            assert!(after < 100_000, "a frame must finish between its checkpoints");
            let calls = std::sync::atomic::AtomicUsize::new(0);
            let mut renderer = Renderer::default();
            let cancelled = || calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= after;
            let Some(frame) = renderer.render_while(request.clone(), &cancelled) else {
                // The renderer keeps only finished work: the same snapshot's next request is whole.
                assert_fresh(&renderer.render(request.clone(), &Cancel::default()).unwrap());
                // Sweeping every checkpoint would redraw the oracle thousands of times; these reach each stage.
                after = after * 2 + 1;
                continue;
            };
            assert_fresh(&frame);
            break;
        }
    }

    #[test]
    fn the_real_worker_replaces_a_lazy_job_and_publishes_only_the_latest_snapshot() {
        let (mut renderer, first) = (Renderer::default(), request_with_desks());
        let latest = crate::view_worker::Latest::new(move |request, token: &Cancel| renderer.render(request, token), || {});
        latest.request(first);
        latest.start().unwrap();
        let last = request_with_desks();
        latest.request(last.clone());
        let deadline = web_time::Instant::now() + web_time::Duration::from_secs(120);
        let frame = loop {
            latest.start().unwrap();
            if !latest.busy()
                && let Some(frame) = latest.take()
            {
                break frame;
            }
            assert!(web_time::Instant::now() < deadline, "the worker did not publish: {:?}", latest.take_error());
            std::thread::sleep(web_time::Duration::from_millis(1));
        };
        assert!(Arc::ptr_eq(&frame.request.model, &last.model), "a replaced job must not publish");
        assert_fresh(&frame);
    }

    #[test]
    fn a_worker_frame_matches_fresh_output_and_tints_keep_its_geometry() {
        let mut renderer = Renderer::default();
        let request = request();
        let frame = renderer.render(request.clone(), &Cancel::default()).unwrap();
        assert!(frame.rebuilt);
        let fresh = render::section_picture(&request.model, request.id, frame.pixels, &[]).unwrap();
        assert_eq!(frame.image.as_ref().unwrap().rgba, fresh.rgba);
        assert_eq!(
            render::png(&frame.drawing).unwrap(),
            render::png(&render::section_over(&request.model, request.id, frame.view, &fresh)).unwrap()
        );
        let mut tinted = request;
        tinted.view = Some(frame.view);
        tinted.tints = vec![(100004, Rgb(37, 99, 235))];
        let next = renderer.render(tinted.clone(), &Cancel::default()).unwrap();
        assert!(!next.rebuilt);
        // The previously published frame remains immutable while the worker repaints.
        assert_eq!(frame.image.as_ref().unwrap().rgba, fresh.rgba);
        let fresh = render::section_picture(&tinted.model, tinted.id, next.pixels, &tinted.tints).unwrap();
        assert_eq!(next.image.as_ref().unwrap().rgba, fresh.rgba);
        for y in 0..fresh.height {
            for x in 0..fresh.width {
                let (x, y) = (f64::from(x), f64::from(y));
                assert_eq!(next.image.as_ref().unwrap().pick(x, y), fresh.pick(x, y));
            }
        }
    }
}
