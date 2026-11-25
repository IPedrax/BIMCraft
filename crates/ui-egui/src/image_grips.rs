//! A selected image's grips in a plan: a square at each corner. A drag from one resizes it about the corner across from it,
//! keeping its aspect, and on release runs `modify {width, at}`.

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id};
use egui::{Painter, PointerButton, Rect, Response, Stroke, StrokeKind, Ui, vec2};
use serde_json::json;

use crate::style::{canvas_pal, pal};
use crate::{BimApp, Tool};

/// How strong the tint over a selected or hovered image is: light, so what it draws reads through.
const TINT: f32 = 0.14;

/// An image's corners in plan, in turn: lower left, lower right, upper right, upper left.
fn corners(lo: P2, hi: P2) -> [P2; 4] {
    [lo, P2(hi.0, lo.1), hi, P2(lo.0, hi.1)]
}

/// The width and the middle of an image resized by its corner `k` dragged to `to`, the corner across from it staying put:
/// as wide as the drag reaches across, or as tall, whichever is more, at its aspect (width over height, `aspect`).
pub(crate) fn resized(lo: P2, hi: P2, k: usize, to: P2, aspect: f64) -> Option<(f64, P2)> {
    let c = corners(lo, hi);
    let fixed = c[(k + 2) % 4];
    let (dx, dy) = ((to.0 - fixed.0).abs(), (to.1 - fixed.1).abs());
    let width = dx.max(dy * aspect);
    if !(width.is_finite() && width > 1.0 && aspect > 0.0) {
        return None;
    }
    let height = width / aspect;
    // Toward the dragged corner from the one that stays.
    let (sx, sy) = ((c[k].0 - fixed.0).signum(), (c[k].1 - fixed.1).signum());
    Some((width, P2(fixed.0 + sx * width / 2.0, fixed.1 + sy * height / 2.0)))
}

impl BimApp {
    /// The image selected alone, with Modify, and its corners: what has grips.
    pub(crate) fn image_gripped(&self) -> Option<(Id, P2, P2)> {
        let m = &self.session.model;
        match (self.selection.as_slice(), &self.tool) {
            (&[id], Tool::Modify) => match m.elements.get(id) {
                Some(Element::Image(im)) => im.bounds(m).map(|[lo, hi]| (id, lo, hi)),
                _ => None,
            },
            _ => None,
        }
    }

    /// A drag from a corner of the selected image: held while it goes, and on release the image resized to where it stopped.
    pub(crate) fn image_grip_drag(&mut self, ui: &Ui, response: &Response, rect: Rect, view: &bimcraft_core::render::View) {
        let Some((id, lo, hi)) = self.image_gripped() else {
            self.image_grip = None;
            return;
        };
        let at = |q: P2| {
            let (x, y) = view.px(q);
            rect.min + vec2(x, y)
        };
        if response.drag_started_by(PointerButton::Primary)
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
            && let Some(k) = corners(lo, hi).iter().position(|c| at(*c).distance(o) <= 8.0)
        {
            self.image_grip = Some((id, k));
        }
        let Some((held, k)) = self.image_grip else { return };
        if held != id {
            self.image_grip = None;
            return;
        }
        if response.drag_stopped() {
            self.image_grip = None;
            if let Some(q) = ui.input(|i| i.pointer.latest_pos()) {
                let to = view.mm((q.x - rect.min.x, q.y - rect.min.y));
                let aspect = (hi.0 - lo.0) / (hi.1 - lo.1);
                if let Some((width, mid)) = resized(lo, hi, k, to, aspect) {
                    self.run("modify", json!({"id": id, "width": width, "at": [mid.0, mid.1]}));
                }
            }
        }
    }

    /// Every selected or hovered image washed with a light tint, over its whole page, and a selected one outlined, so its edges
    /// show: its own colours are kept (the drawing doesn't light what a picture draws, `Part::Picture`), so it still reads.
    pub(crate) fn image_frames(&self, p: &Painter, rect: Rect, view: &bimcraft_core::render::View, hovered: Option<Id>) {
        let m = &self.session.model;
        let lit = self.selection.iter().map(|id| (*id, true)).chain(hovered.filter(|h| !self.selection.contains(h)).map(|h| (h, false)));
        for (id, selected) in lit {
            let Some(Element::Image(im)) = m.elements.get(id) else { continue };
            let Some([lo, hi]) = im.bounds(m) else { continue };
            let ring: Vec<egui::Pos2> = corners(lo, hi)
                .iter()
                .map(|q| {
                    let (x, y) = view.px(*q);
                    rect.min + vec2(x, y)
                })
                .collect();
            let (colour, line) = if selected { (canvas_pal().accent, 1.5) } else { (canvas_pal().hover, 0.0) };
            let stroke = if selected { Stroke::new(line, colour) } else { Stroke::NONE };
            p.add(egui::Shape::convex_polygon(ring, colour.gamma_multiply(TINT), stroke));
        }
    }

    /// The selected image's corner grips, and while one is dragged, the outline it would take.
    pub(crate) fn image_grip_overlay(&self, ui: &Ui, p: &Painter, rect: Rect, view: &bimcraft_core::render::View) {
        let Some((id, lo, hi)) = self.image_gripped() else { return };
        let at = |q: P2| {
            let (x, y) = view.px(q);
            rect.min + vec2(x, y)
        };
        let (mut lo2, mut hi2) = (lo, hi);
        let live = self.image_grip.filter(|(held, _)| *held == id).map(|(_, k)| k);
        if let (Some(k), Some(q)) = (live, ui.input(|i| i.pointer.latest_pos())) {
            let to = view.mm((q.x - rect.min.x, q.y - rect.min.y));
            if let Some((w, mid)) = resized(lo, hi, k, to, (hi.0 - lo.0) / (hi.1 - lo.1)) {
                let h = w * (hi.1 - lo.1) / (hi.0 - lo.0);
                (lo2, hi2) = (P2(mid.0 - w / 2.0, mid.1 - h / 2.0), P2(mid.0 + w / 2.0, mid.1 + h / 2.0));
                let c = corners(lo2, hi2).map(at);
                for i in 0..4 {
                    p.line_segment([c[i], c[(i + 1) % 4]], Stroke::new(1.5, pal().accent));
                }
            }
        }
        for (i, c) in corners(lo2, hi2).into_iter().enumerate() {
            let r = Rect::from_center_size(at(c), vec2(9.0, 9.0));
            let held = live == Some(i);
            p.rect_filled(r, 1.0, if held { pal().accent } else { canvas_pal().surface });
            p.rect_stroke(r, 1.0, Stroke::new(1.5, pal().accent), StrokeKind::Middle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{Place, paint_prim};
    use bimcraft_core::render::{Part, Prim, Rgb};
    use egui::{Color32, Pos2, Shape, pos2};

    /// What the fill `rings` of a placed PDF paints, `selected` or not: each mesh vertex's colour, and whether its triangles
    /// cover `q`.
    fn painted(rings: Vec<Vec<(f32, f32)>>, selected: bool, q: Pos2) -> (Vec<Color32>, bool) {
        let prim = Prim::Fill { rings, color: Rgb(10, 10, 10) };
        let set: std::collections::HashSet<Id> = if selected { [7].into() } else { Default::default() };
        let mut shapes = Vec::new();
        let clip = Rect::from_min_max(pos2(-100.0, -100.0), pos2(100.0, 100.0));
        paint_prim(
            &mut |s| shapes.push(s),
            clip,
            Place::at(Pos2::ZERO),
            (&prim, Some(7), Part::Picture),
            &set,
            None,
            false,
            &mut [].iter(),
        );
        let (mut colours, mut covers) = (Vec::new(), false);
        for s in shapes {
            if let Shape::Mesh(m) = s {
                colours.extend(m.vertices.iter().map(|v| v.color));
                for t in m.indices.chunks(3) {
                    let [a, b, c] = [t[0], t[1], t[2]].map(|i| m.vertices[i as usize].pos);
                    let side = |p: Pos2, q1: Pos2, q2: Pos2| (q2.x - q1.x) * (p.y - q1.y) - (q2.y - q1.y) * (p.x - q1.x);
                    let (d1, d2, d3) = (side(q, a, b), side(q, b, c), side(q, c, a));
                    covers |= (d1 >= 0.0 && d2 >= 0.0 && d3 >= 0.0) || (d1 <= 0.0 && d2 <= 0.0 && d3 <= 0.0);
                }
            }
        }
        (colours, covers)
    }

    fn ring(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<(f32, f32)> {
        vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
    }

    /// A square with a square opening, wound the other way.
    fn frame() -> Vec<Vec<(f32, f32)>> {
        let mut inner = ring(10.0, 10.0, 20.0, 20.0);
        inner.reverse();
        vec![ring(0.0, 0.0, 30.0, 30.0), inner]
    }

    #[test]
    fn a_selected_pdfs_shapes_keep_their_colours_and_an_opening_stays_open() {
        let (plain, _) = painted(frame(), false, pos2(5.0, 15.0));
        let (lit, covers_frame) = painted(frame(), true, pos2(5.0, 15.0));
        assert!(!plain.is_empty() && lit == plain, "selected, it is drawn in its own colours (a tint goes over it)");
        assert!(covers_frame, "the frame is filled");
        assert!(!painted(frame(), true, pos2(15.0, 15.0)).1, "its opening is not");
    }

    #[test]
    fn a_pdfs_stroke_thinner_than_a_pixel_still_shades_the_pixels_it_crosses() {
        // A serif letter's bar, 0.4 px tall inside the row of pixels centred at 10.5 but short of the centre: its own triangles
        // miss every pixel, its edges' feather shades that row's, and not the next.
        let bar = || vec![ring(0.0, 10.55, 20.0, 10.95)];
        let (colours, covers) = painted(bar(), false, pos2(5.0, 10.5));
        assert!(covers, "its row shades it");
        assert!(colours.iter().any(|c| c.a() == 0) && colours.iter().any(|c| c.a() == 255), "feathered from ink to clear");
        assert!(!painted(bar(), false, pos2(5.0, 11.5)).1, "and the next row doesn't");
    }

    #[test]
    fn a_corner_dragged_resizes_about_the_one_across_keeping_the_aspect() {
        let (lo, hi) = (P2(0.0, 0.0), P2(4000.0, 2000.0));
        // The upper right corner dragged out to (8000, 3000): twice as wide, the lower left staying.
        let (w, mid) = resized(lo, hi, 2, P2(8000.0, 3000.0), 2.0).unwrap();
        assert_eq!((w, mid), (8000.0, P2(4000.0, 2000.0)));
        // Dragged up past its aspect: as tall as the drag, as wide as that makes it.
        let (w, _) = resized(lo, hi, 2, P2(4100.0, 5000.0), 2.0).unwrap();
        assert_eq!(w, 10000.0);
        // The lower left corner dragged in: the upper right stays.
        let (w, mid) = resized(lo, hi, 0, P2(2000.0, 1000.0), 2.0).unwrap();
        assert_eq!((w, mid), (2000.0, P2(3000.0, 1500.0)));
        assert!(resized(lo, hi, 0, P2(4000.0, 2000.0), 2.0).is_none(), "nothing left of it");
    }
}
