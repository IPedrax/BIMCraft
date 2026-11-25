//! FE-3: Revit's zoom commands in the family document's views, through the window's own (`crate::zoom`). Each acts on the view
//! shown: a plan's or an elevation's frame (`Views::frames`), or {3D}'s camera (an adaptive family's modelling view's, else the
//! preview's). Zoom to Fit, Zoom Out (2x) and Zoom In Region each leave a step that Previous Pan/Zoom goes back to, a history of
//! each view; the wheel and the middle button leave none. Nothing here reaches the project's views.
use super::FamilyEditor;
use crate::zoom::{self, Previous};
use bimcraft_core::family::views::Which;
use bimcraft_core::geom::P2;
use bimcraft_core::view3d::Camera;
use egui::{Pos2, Rect, Response, Ui};
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
    /// Zoom In Region, armed.
    region: Option<Armed>,
    /// Each view's steps back ({3D} under none).
    previous: BTreeMap<Option<Which>, Previous<Look>>,
    /// The view Zoom to Fit last fitted, and how it looked once fitted (none while {3D}'s fit is still to come): Fit again on it,
    /// unmoved, leaves no step.
    fitted: Option<(Option<Which>, Option<Look>)>,
    /// The window's pass the views were last drawn in.
    drawn: u64,
}

/// Zoom In Region, armed: the view it was armed in, where its drag began once it has, the tools in hand then, and the pass the
/// view was last drawn in with it. Another view shown, another tool picked, or the document's tab left gives it up.
struct Armed {
    view: Option<Which>,
    from: Option<Pos2>,
    tools: Tools,
    pass: u64,
}

/// The tools of the document's views in hand, as far as they tell one from another: a tool picked changes it.
type Tools = ([bool; 8], Option<(Option<bimcraft_core::draw::Shape>, bool, bool)>, crate::adaptive_view::Draw);

impl FamilyEditor {
    /// The tools in hand: a plane or a line being drawn, a pick, a dimension, Align, a pivot, a component, a sketch and its shape,
    /// the modelling view's tool.
    fn tools(&self) -> Tools {
        let v = &self.views;
        let flags = [
            v.drawing.is_some(),
            v.picking.is_some(),
            v.dimensioning.is_some(),
            v.aligning.is_some(),
            v.pivoting.is_some(),
            self.placing.is_some(),
            self.sketch.is_some(),
            self.modeller.picking_plane,
        ];
        let sketch = self.sketch.as_ref().map(|s| (s.tool, s.axis_tool, s.trim.is_some()));
        (flags, sketch, self.modeller.tool.clone())
    }

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

    /// The view control bar's Zoom to Fit (ZF, ZE, ZA): the view shown fitted afresh, a plan, an elevation or {3D}. A step for
    /// Previous unless the view is fitted already (or its fit is still to come).
    pub(crate) fn zoom_to_fit(&mut self) {
        let now = self.look();
        let fitted = match self.zoom.fitted {
            Some((view, look)) if view == self.views.shown => look.is_none_or(|l| l == now),
            _ => now == Look::Flat(FITTED.0, FITTED.1),
        };
        if !fitted {
            self.step();
        }
        self.zoom.fitted = Some((self.views.shown, None));
        match self.views.shown {
            Some(which) => {
                self.views.frames.remove(&which);
                // To the room the family takes up now: an edit never fits the view by itself.
                self.refit();
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
        self.zoom.region = Some(Armed { view: self.views.shown, from: None, tools: self.tools(), pass: self.zoom.drawn });
    }

    /// Whether Zoom In Region is armed in the view shown, with the tools it was armed with.
    pub(crate) fn zoom_region_armed(&self) -> bool {
        self.zoom.region.as_ref().is_some_and(|a| a.view == self.views.shown && a.tools == self.tools())
    }

    /// Zoom In Region given up (Escape).
    pub(crate) fn cancel_zoom_region(&mut self) {
        self.give_up_region();
    }

    /// Zoom In Region given up, if it was armed: a drag of the middle button over its widget, whose release that widget will no
    /// longer see, let go.
    fn give_up_region(&mut self) {
        if self.zoom.region.take().is_some() {
            self.orbit.let_go();
            self.modeller.let_go();
        }
    }

    /// Previous Pan/Zoom: the view shown as it was before its last zoom command; nothing with none before.
    pub(crate) fn previous_zoom(&mut self) {
        if let Some(look) = self.zoom.previous.get_mut(&self.views.shown).and_then(Previous::pop) {
            self.set_look(look);
        }
    }

    /// Each frame the document's view is drawn in `pane`: Zoom to Fit's fit noted once done, and while Zoom In Region is armed, its
    /// drag over the view, drawn over the view after it (so the primary button's drag neither turns, pans nor selects); let go,
    /// the view fits the rectangle and the command is done. A click leaves it armed. The wheel and the middle button go on
    /// zooming, panning and turning the view meanwhile.
    pub(crate) fn zoom_region_drag(&mut self, ui: &Ui, pane: Rect) {
        let pass = ui.ctx().cumulative_pass_nr();
        self.zoom.drawn = pass;
        self.note_fit();
        // Another view shown, another tool picked, or the tab left (a pass the view wasn't drawn in) since it was armed: given up.
        if !self.zoom_region_armed() || self.zoom.region.as_ref().is_some_and(|a| a.pass + 1 < pass) {
            self.give_up_region();
            return;
        }
        let Some(armed) = self.zoom.region.as_mut() else { return };
        armed.pass = pass;
        let (rect, response) = zoom::drag(ui, pane, &mut armed.from);
        self.wheel_and_middle(ui, &response);
        if let Some(r) = rect
            && let Some(look) = self.region(r)
        {
            if look != self.look() {
                self.step();
                self.set_look(look);
            }
            self.zoom.region = None;
        }
    }

    /// Zoom to Fit's fit, once done, as the view shown then looks.
    fn note_fit(&mut self) {
        let done = match self.views.shown {
            Some(_) => true,
            None if self.adaptive.is_some() => self.modeller.camera.is_some(),
            None => !self.orbit.fitting() && self.orbit.camera.is_some(),
        };
        match self.zoom.fitted {
            Some((view, None)) if view == self.views.shown && done => self.zoom.fitted = Some((view, Some(self.look()))),
            Some((view, _)) if view != self.views.shown => self.zoom.fitted = None,
            _ => {}
        }
    }

    /// The wheel and the middle button over Zoom In Region's `response`, which lies over the view shown: handed to the view's own
    /// navigation, as they would be without it.
    fn wheel_and_middle(&mut self, ui: &Ui, response: &Response) {
        match self.views.shown {
            Some(which) => {
                if let Some(scale) = self.views.cache.as_ref().filter(|c| c.which == which).map(|c| c.view.scale) {
                    self.zoom_and_pan(ui, response, which, scale);
                }
            }
            None if self.adaptive.is_some() => self.modeller.wheel_and_middle(ui, response),
            None => {
                if let Some(cam) = self.orbit.camera {
                    let (rect, ppp) = (self.orbit.rect, self.orbit.ppp);
                    // About what the picture shows, while it is of the family as staged now, as the view's own navigation.
                    let scene = self.orbit.scene.as_ref().map(|(_, s)| std::sync::Arc::clone(s));
                    let fresh = self.scene3d.as_ref().is_some_and(|s| self.orbit.shows(&s.model));
                    let moved = self.orbit.wheel_and_middle(ui, rect, ppp, response, cam, scene.as_deref().filter(|_| fresh));
                    // Moved, it no longer fits itself again as its pane is resized.
                    if moved != cam {
                        self.orbit.fitted = None;
                    }
                    self.orbit.camera = Some(moved);
                }
            }
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
