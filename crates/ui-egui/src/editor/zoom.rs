//! FE-3: Revit's zoom commands in the family document's views, through the window's own (`crate::zoom`). Each acts on the view
//! shown: a plan's or an elevation's frame (`Views::frames`), or {3D}'s camera (an adaptive family's modelling view's, else the
//! preview's). Zoom to Fit, Zoom Out (2x) and Zoom In Region each leave a step that Previous Pan/Zoom goes back to, a history of
//! each view; the wheel and the middle button leave none. Nothing here reaches the project's views.
use super::FamilyEditor;
use crate::zoom::{self, Previous};
use bimcraft_core::family::views::Which;
use bimcraft_core::geom::P2;
use bimcraft_core::view3d::Camera;
use egui::{Pos2, Rect, Ui};
use std::collections::BTreeMap;

/// How a view of the document is looked at: a plan's or an elevation's frame (times as close as fitted, and moved mm), or
/// {3D}'s camera (none: fitted afresh as it next shows).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Look {
    Flat(f64, (f64, f64)),
    Round(Option<Camera>),
}

/// The fitted frame of a plan or an elevation, and how close it may come.
const FITTED: (f64, (f64, f64)) = (1.0, (0.0, 0.0));
const CLOSEST: (f64, f64) = (0.05, 50.0);

/// What the zoom commands keep of the document's views.
#[derive(Default)]
pub(crate) struct Zoom {
    /// Zoom In Region, armed: the view it was armed in (another view shown gives it up), and where its drag began, once it has.
    region: Option<(Option<Which>, Option<Pos2>)>,
    /// Each view's steps back ({3D} under none).
    previous: BTreeMap<Option<Which>, Previous<Look>>,
}

impl FamilyEditor {
    /// How the view shown is looked at now.
    fn look(&self) -> Look {
        match self.views.shown {
            Some(which) => {
                let (zoom, pan) = self.views.frames.get(&which).copied().unwrap_or(FITTED);
                Look::Flat(zoom, pan)
            }
            None if self.adaptive.is_some() => Look::Round(self.modeller.camera),
            None => Look::Round(self.orbit.camera),
        }
    }

    /// The view shown looked at as `look` says.
    fn set_look(&mut self, look: Look) {
        match (self.views.shown, look) {
            (Some(which), Look::Flat(zoom, pan)) => {
                self.views.frames.insert(which, (zoom, pan));
            }
            (None, Look::Round(cam)) if self.adaptive.is_some() => self.modeller.camera = cam,
            (None, Look::Round(cam)) => {
                // A fit still being worked out would come back over it.
                self.orbit.cancel_fit();
                self.orbit.camera = cam;
                self.orbit.fitted = None;
            }
            _ => {}
        }
    }

    /// A zoom command about to change the view shown: how it is now, a step for Previous Pan/Zoom.
    fn step(&mut self) {
        let now = self.look();
        self.zoom.previous.entry(self.views.shown).or_default().push(now);
    }

    /// The view control bar's Zoom to Fit (ZF, ZE, ZA): the view shown fitted afresh, a plan, an elevation or {3D}.
    pub(crate) fn zoom_to_fit(&mut self) {
        self.step();
        match self.views.shown {
            Some(which) => {
                self.views.frames.remove(&which);
            }
            None => {
                self.modeller.camera = None;
                self.orbit.fit();
            }
        }
    }

    /// Zoom Out (2x): the view shown half as close, about its middle.
    pub(crate) fn zoom_out(&mut self) {
        let look = match self.look() {
            Look::Flat(zoom, pan) => Look::Flat((zoom * zoom::OUT).clamp(CLOSEST.0, CLOSEST.1), pan),
            Look::Round(cam) => Look::Round(cam.map(zoom::out_3d)),
        };
        if look != self.look() {
            self.step();
            self.set_look(look);
        }
    }

    /// Zoom In Region: the next drag in the view shown is a rectangle it fits. Escape gives it up.
    pub(crate) fn zoom_region_tool(&mut self) {
        self.zoom.region = Some((self.views.shown, None));
    }

    /// Whether Zoom In Region is armed in the view shown.
    pub(crate) fn zoom_region_armed(&self) -> bool {
        self.zoom.region.is_some_and(|(view, _)| view == self.views.shown)
    }

    /// Zoom In Region given up (Escape).
    pub(crate) fn cancel_zoom_region(&mut self) {
        self.zoom.region = None;
    }

    /// Previous Pan/Zoom: the view shown as it was before its last zoom command; nothing with none before.
    pub(crate) fn previous_zoom(&mut self) {
        if let Some(look) = self.zoom.previous.get_mut(&self.views.shown).and_then(Previous::pop) {
            self.set_look(look);
        }
    }

    /// While Zoom In Region is armed, its drag over the view in `pane`, drawn over the view after it (so the view's own drag
    /// doesn't take it); let go, the view fits the rectangle and the command is done. A click leaves it armed.
    pub(crate) fn zoom_region_drag(&mut self, ui: &Ui, pane: Rect) {
        if !self.zoom_region_armed() {
            // Another view shown since it was armed: given up.
            self.zoom.region = None;
            return;
        }
        let Some((_, from)) = self.zoom.region.as_mut() else { return };
        if let Some(r) = zoom::drag(ui, pane, from)
            && let Some(look) = self.region(r)
        {
            self.step();
            self.set_look(look);
            self.zoom.region = None;
        }
    }

    /// How the view shown is looked at with rectangle `r` (points on screen) fitted to it.
    fn region(&self, r: Rect) -> Option<Look> {
        match self.look() {
            Look::Flat(zoom, pan) => {
                let which = self.views.shown?;
                let view = self.views.cache.as_ref().filter(|c| c.which == which)?.view;
                let o = self.views.pane?.min;
                let (scale, middle) = zoom::region_2d(&view, (r.min.x - o.x, r.min.y - o.y), (r.max.x - o.x, r.max.y - o.y))?;
                // The fitted scale is the view's over its zoom; its middle is the fitted one moved by the pan.
                let zoom = (zoom * scale / view.scale).clamp(CLOSEST.0, CLOSEST.1);
                let P2(x, y) = middle;
                Some(Look::Flat(zoom, (pan.0 + x - view.cx, pan.1 + y - view.cy)))
            }
            Look::Round(cam) => {
                let cam = cam?;
                // The modelling view's camera is in points; the preview's in pixels.
                let (rect, ppp) = if self.adaptive.is_some() { (self.modeller.rect, 1.0) } else { (self.orbit.rect, self.orbit.ppp) };
                let size = rect.size() * ppp;
                let cam = Camera { width: size.x.round().max(1.0) as u32, height: size.y.round().max(1.0) as u32, ..cam };
                let px = |p: Pos2| (f64::from((p.x - rect.min.x) * ppp), f64::from((p.y - rect.min.y) * ppp));
                zoom::region_3d(cam, px(r.min), px(r.max)).map(|c| Look::Round(Some(c)))
            }
        }
    }
}
