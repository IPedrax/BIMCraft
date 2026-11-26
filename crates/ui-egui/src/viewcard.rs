//! The graphics card lent to the core for the pictures of sections, elevations, callouts and sheets (core `view3d::Raster`): a
//! card of its own on the window's device, apart from the 3D view's, so a section's worker thread rasterizes on it while the
//! window draws the 3D view, and neither's mesh or targets are the other's. Whatever fails here the processor draws instead,
//! to the same picture, and the card is not tried again for these views until the window starts again.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bimcraft_core::view3d::gpu::Mesh;
use bimcraft_core::view3d::{Camera, Caps, Clip, Raster, Rastered};
use eframe::egui_wgpu::RenderState;

use crate::gpu::Gpu;

pub(crate) struct ViewCard {
    gpu: Mutex<Option<Gpu>>,
    failed: AtomicBool,
    /// Why it failed, the once it did.
    why: Mutex<Option<String>>,
}

impl ViewCard {
    /// A card on the window's device, if one can be made there.
    pub(crate) fn new(state: &RenderState) -> Result<Arc<ViewCard>, String> {
        Gpu::from_device(state.device.clone(), state.queue.clone()).map(ViewCard::with)
    }

    /// A card already made, as a test has one. Each picture is drawn in one go: a section's or a sheet's is read back whole.
    pub(crate) fn with(mut gpu: Gpu) -> Arc<ViewCard> {
        gpu.budget = usize::MAX;
        Arc::new(ViewCard { gpu: Mutex::new(Some(gpu)), failed: AtomicBool::new(false), why: Mutex::new(None) })
    }

    /// Whether it failed: the processor draws these views from then on.
    pub(crate) fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    /// Why it failed, if it did.
    #[cfg(test)]
    pub(crate) fn why(&self) -> Option<String> {
        self.why.lock().ok().and_then(|w| w.clone())
    }

    /// `draw` on the card, or none where it can't: an error lets the card go, for the processor to draw this and what follows.
    fn run<T>(&self, draw: impl FnOnce(&mut Gpu) -> Result<T, String>) -> Option<T> {
        if self.failed() {
            return None;
        }
        let mut held = self.gpu.lock().ok()?;
        let gpu = held.as_mut()?;
        match draw(gpu) {
            Ok(done) => Some(done),
            Err(e) => {
                eprintln!("BIMCraft: the graphics card couldn't draw a section or a sheet ({e}): they are drawn on the processor");
                *held = None;
                self.failed.store(true, Ordering::Relaxed);
                if let Ok(mut why) = self.why.lock() {
                    *why = Some(e);
                }
                None
            }
        }
    }
}

impl Raster for ViewCard {
    fn section(&self, mesh: &Arc<Mesh>, cam: &Camera, clip: &Clip) -> Option<Rastered> {
        self.run(|gpu| gpu.raster_section(mesh, cam, clip))
    }

    fn picture(&self, mesh: &Arc<Mesh>, cam: &Camera, caps: Option<&Arc<Caps>>) -> Option<Vec<u8>> {
        self.run(|gpu| gpu.picture_of(mesh, cam, caps))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bimcraft_core::engine::Session;
    use bimcraft_core::model::{Element, Id};
    use bimcraft_core::render::{self, Prim, SectionWork, View};
    use bimcraft_core::sheet;
    use bimcraft_core::view3d::Image;
    use serde_json::json;

    fn sample() -> Session {
        let mut s = Session::new();
        s.script(bimcraft_core::sample::SAMPLE).unwrap();
        s
    }

    /// The sections and elevations of `s`'s model.
    fn cuts(s: &Session) -> Vec<Id> {
        s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Section(_) | Element::Elevation(_))).map(|(id, _)| *id).collect()
    }

    /// The pixels where the card's picture isn't the processor's: in colour, or in what a click there picks, or its face.
    fn differing(card: &Image, cpu: &Image) -> Vec<(u32, u32)> {
        assert_eq!((card.width, card.height), (cpu.width, cpu.height));
        let mut out = Vec::new();
        for y in 0..cpu.height {
            for x in 0..cpu.width {
                let k = 4 * (y * cpu.width + x) as usize;
                let (fx, fy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                if card.rgba[k..k + 4] != cpu.rgba[k..k + 4]
                    || card.pick(fx, fy) != cpu.pick(fx, fy)
                    || card.face(fx, fy) != cpu.face(fx, fy)
                    || card.covered(fx, fy) != cpu.covered(fx, fy)
                {
                    out.push((x, y));
                }
            }
        }
        out
    }

    /// How many pixels of a section's `pixels` may differ between the card and the processor: the card snaps a triangle's corners
    /// to a 256th of a pixel, so a face seen all but edge on (a sliver a pixel wide) may cover a pixel's middle on one and not on
    /// the other, and its edge moves with it. One pixel in 2,000, and a software card's few.
    fn may_differ(pixels: usize) -> usize {
        pixels / 2000 + crate::gpu::pixels_that_may_differ()
    }

    /// Section or elevation `id` of `s` through `view`, on the card and on the processor: the same picture, but for a sliver's
    /// pixel or two ([`may_differ`]).
    fn as_the_processor(card: &ViewCard, s: &Session, work: &SectionWork, id: Id, view: View) {
        let cpu = render::section_picture(&s.model, id, view, &[]).unwrap();
        let on_card = render::section_picture_card(&s.model, work, id, view, &[], card, &|| false)
            .unwrap_or_else(|| panic!("the card drew no picture of {id}: {:?}", card.why()));
        let wrong = differing(&on_card, &cpu);
        let say = |(x, y): (u32, u32)| {
            let k = 4 * (y * cpu.width + x) as usize;
            let (fx, fy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
            format!(
                "({x}, {y}): card {:?} {:?} {:?}, processor {:?} {:?} {:?}",
                &on_card.rgba[k..k + 3],
                on_card.pick(fx, fy),
                on_card.face(fx, fy),
                &cpu.rgba[k..k + 3],
                cpu.pick(fx, fy),
                cpu.face(fx, fy)
            )
        };
        assert!(
            wrong.len() <= may_differ((cpu.width * cpu.height) as usize),
            "{id} through {view:?}: {} pixels differ: {:?}",
            wrong.len(),
            wrong.iter().take(8).map(|p| say(*p)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_card_rasters_sections_and_elevations_as_the_processor_draws_them() {
        let Some(gpu) = crate::gpu::headless() else {
            eprintln!("no graphics card here: nothing to test");
            return;
        };
        let card = ViewCard::with(gpu);
        let s = sample();
        let work = SectionWork::new(&s.model);
        let ids = cuts(&s);
        assert_eq!(ids.len(), 5, "the sample's four elevations and its section");
        for id in ids {
            for (w, h) in [(640, 480), (301, 157)] {
                let fit = render::section_fit(&s.model, id, w, h);
                // Fitted, zoomed in on a corner, and zoomed out past its ends.
                let views = [
                    fit,
                    View { scale: fit.scale * 3.0, cx: fit.cx - 2000.0, cy: fit.cy + 700.0, ..fit },
                    View { scale: fit.scale / 2.5, ..fit },
                ];
                for view in views {
                    as_the_processor(&card, &s, &work, id, view);
                }
            }
        }
        assert!(!card.failed(), "{:?}", card.why());
    }

    #[test]
    fn a_section_s_far_clip_crop_and_selection_are_the_processor_s_on_the_card() {
        let Some(gpu) = crate::gpu::headless() else {
            eprintln!("no graphics card here: nothing to test");
            return;
        };
        let card = ViewCard::with(gpu);
        let mut s = sample();
        // A section looking north whose far clip stops short of the back wall, cropped to its middle and the ground floor.
        let id =
            s.execute("section", &json!({"start": [-1500, 1500], "end": [11500, 1500], "depth": 3000})).unwrap()["id"].as_u64().unwrap();
        s.execute("crop", &json!({"view": id, "region": [[3000, -200], [9000, 2800]]})).unwrap();
        let work = SectionWork::new(&s.model);
        let fit = render::section_fit(&s.model, id, 500, 400);
        as_the_processor(&card, &s, &work, id, fit);
        // A pan that puts the crop's edge across the picture.
        as_the_processor(&card, &s, &work, id, View { cx: fit.cx + 4000.0, scale: fit.scale * 2.0, ..fit });
        // Selected walls tinted, the picture's own way.
        let walls: Vec<(Id, render::Rgb)> = s
            .model
            .elements
            .iter()
            .filter(|(_, e)| matches!(e, Element::Wall(_)))
            .map(|(id, _)| (*id, render::Rgb(30, 120, 220)))
            .collect();
        let cpu = render::section_picture(&s.model, id, fit, &walls).unwrap();
        let on_card = render::section_picture_card(&s.model, &work, id, fit, &walls, card.as_ref(), &|| false).unwrap();
        assert!(differing(&on_card, &cpu).len() <= may_differ((cpu.width * cpu.height) as usize));
        assert!(!card.failed(), "{:?}", card.why());
    }

    /// A picture on a sheet: its place, its width and height, and its pixels.
    type Picture = ((f32, f32), u32, u32, Vec<u8>);

    /// The pictures a sheet holds.
    fn pictures(d: &render::Drawing) -> Vec<Picture> {
        d.prims
            .iter()
            .filter_map(|p| match p {
                Prim::Image { at, width, height, rgba, .. } => Some((*at, *width, *height, rgba.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_sheet_s_sections_elevations_and_3d_view_drawn_on_the_card_are_the_processor_s() {
        let Some(gpu) = crate::gpu::headless() else {
            eprintln!("no graphics card here: nothing to test");
            return;
        };
        let card = ViewCard::with(gpu);
        let s = sample();
        let sheet_of = |number: &str| {
            s.model.elements.iter().find(|(_, e)| matches!(e, Element::Sheet(sh) if sh.number == number)).map(|(id, _)| *id).unwrap()
        };
        for number in ["A102", "A103"] {
            let id = sheet_of(number);
            let cpu = pictures(&sheet::draw(&s.model, id).unwrap());
            let on_card = pictures(&sheet::draw_on(&s.model, id, Some(card.as_ref())).unwrap());
            assert!(!cpu.is_empty(), "{number} has pictures");
            assert_eq!(cpu.len(), on_card.len(), "{number}");
            for ((at, w, h, theirs), (at2, w2, h2, ours)) in cpu.iter().zip(&on_card) {
                assert_eq!((at, w, h), (at2, w2, h2), "{number}");
                // A section's picture but for a sliver's pixel ([`may_differ`]); the 3D view's as the card's 3D view is held
                // to: no more than one pixel in 200 further than six levels of a channel from the processor's.
                let far = |k: usize| (0..3).any(|c| theirs[4 * k + c].abs_diff(ours[4 * k + c]) > if number == "A103" { 6 } else { 0 });
                let wrong = (0..theirs.len() / 4).filter(|&k| far(k)).count();
                let allowed = if number == "A103" { theirs.len() / 4 / 200 } else { may_differ(theirs.len() / 4) };
                assert!(wrong <= allowed, "{number}: {wrong} of {} pixels differ", theirs.len() / 4);
            }
        }
        assert!(!card.failed(), "{:?}", card.why());
    }

    #[test]
    fn a_card_that_fails_lets_the_processor_draw_the_same_picture_and_is_not_tried_again() {
        let Some(mut gpu) = crate::gpu::headless() else {
            eprintln!("no graphics card here: nothing to test");
            return;
        };
        // No allocation succeeds: the card can't hold one buffer of the section's vertices.
        gpu.full_at = Some(0);
        let card = ViewCard::with(gpu);
        let s = sample();
        let work = SectionWork::new(&s.model);
        let id = cuts(&s)[4];
        let view = render::section_fit(&s.model, id, 400, 300);
        assert!(render::section_picture_card(&s.model, &work, id, view, &[], card.as_ref(), &|| false).is_none());
        assert!(card.failed() && card.why().is_some());
        let cpu = render::section_picture(&s.model, id, view, &[]).unwrap();
        let fell_back = render::section_picture_on(&s.model, &work, id, view, &[], card.as_ref(), &|| false).unwrap();
        assert!(differing(&fell_back, &cpu).is_empty());
    }
}
