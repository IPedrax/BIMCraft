//! A drawing cut to a rectangle of its pixels: an export of a region of a view. Lines are cut where they leave it, fills
//! are clipped to it and pictures cut to its pixels, and the view moves with it, so a pixel of what is left is still where
//! it was in the model.

use super::{Drawing, Prim, View};

/// The part of segment `a`–`b` inside the box, as fractions along it (Liang–Barsky); none when none of it is.
fn clip_segment(a: (f32, f32), b: (f32, f32), lo: (f32, f32), hi: (f32, f32)) -> Option<(f32, f32)> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [(-dx, a.0 - lo.0), (dx, hi.0 - a.0), (-dy, a.1 - lo.1), (dy, hi.1 - a.1)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
            if t0 > t1 {
                return None;
            }
        }
    }
    Some((t0, t1))
}

/// The pieces of a polyline inside the box, each a run of points.
fn clip_line(pts: &[(f32, f32)], closed: bool, lo: (f32, f32), hi: (f32, f32)) -> Vec<Vec<(f32, f32)>> {
    let at = |a: (f32, f32), b: (f32, f32), t: f32| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
    let mut ring = pts.to_vec();
    if closed && let Some(first) = pts.first() {
        ring.push(*first);
    }
    let mut pieces: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut current: Vec<(f32, f32)> = Vec::new();
    for w in ring.windows(2) {
        let [a, b] = [w[0], w[1]];
        match clip_segment(a, b, lo, hi) {
            Some((t0, t1)) => {
                let (p, q) = (at(a, b, t0), at(a, b, t1));
                if current.last() != Some(&p) {
                    if current.len() > 1 {
                        pieces.push(std::mem::take(&mut current));
                    }
                    current = vec![p];
                }
                current.push(q);
                if t1 < 1.0 {
                    pieces.push(std::mem::take(&mut current));
                }
            }
            None => {
                if current.len() > 1 {
                    pieces.push(std::mem::take(&mut current));
                }
                current.clear();
            }
        }
    }
    if current.len() > 1 {
        pieces.push(current);
    }
    pieces
}

/// A ring clipped to the box (Sutherland–Hodgman, one side at a time).
fn clip_ring(ring: &[(f32, f32)], lo: (f32, f32), hi: (f32, f32)) -> Vec<(f32, f32)> {
    let mut out = ring.to_vec();
    // Each side: the axis it cuts across, where, and whether the inside is above it (else below).
    let sides = [(0, lo.0, true), (0, hi.0, false), (1, lo.1, true), (1, hi.1, false)];
    for (axis, at, above) in sides {
        let inside = |p: (f32, f32)| {
            let c = if axis == 0 { p.0 } else { p.1 };
            if above { c >= at } else { c <= at }
        };
        let input = std::mem::take(&mut out);
        let Some(&last) = input.last() else { break };
        let cross = |a: (f32, f32), b: (f32, f32)| {
            let (ca, cb) = if axis == 0 { (a.0, b.0) } else { (a.1, b.1) };
            let t = if cb == ca { 0.0 } else { (at - ca) / (cb - ca) };
            (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
        };
        let mut prev = last;
        for p in input {
            match (inside(p), inside(prev)) {
                (true, true) => out.push(p),
                (true, false) => {
                    out.push(cross(prev, p));
                    out.push(p);
                }
                (false, true) => out.push(cross(prev, p)),
                (false, false) => {}
            }
            prev = p;
        }
    }
    out
}

impl Drawing {
    /// What of it lies in the rectangle `lo`–`hi` of its pixels, drawn with that rectangle's top left corner as its own:
    /// the drawing, and for each primitive of it the one it was cut from. `view` is the drawing's view made the same; another
    /// view of the same pixels (a plan's in the model) is moved with [`View::cropped`].
    pub fn cropped(&self, lo: (f32, f32), hi: (f32, f32)) -> (Drawing, Vec<usize>) {
        let mut d = Drawing::new(self.view.cropped(lo, hi));
        d.notes = self.notes.clone();
        d.weights = self.weights;
        d.k = self.k;
        let mut from = Vec::new();
        let shift = |p: (f32, f32)| (p.0 - lo.0, p.1 - lo.1);
        let mut keep = |d: &mut Drawing, i: usize, p: Prim| {
            d.prims.push(p);
            d.owners.push(self.owners.get(i).copied().flatten());
            d.parts.push(self.parts.get(i).copied().unwrap_or_default());
            d.pens.push(self.pens.get(i).copied().flatten());
            from.push(i);
        };
        for (i, p) in self.prims.iter().enumerate() {
            match p {
                Prim::Line { pts, width, color, dash, closed } => {
                    let inside = pts.iter().all(|q| q.0 >= lo.0 && q.0 <= hi.0 && q.1 >= lo.1 && q.1 <= hi.1);
                    if inside {
                        let pts = pts.iter().map(|q| shift(*q)).collect();
                        keep(&mut d, i, Prim::Line { pts, width: *width, color: *color, dash: dash.clone(), closed: *closed });
                        continue;
                    }
                    for piece in clip_line(pts, *closed, lo, hi) {
                        let pts = piece.into_iter().map(shift).collect();
                        keep(&mut d, i, Prim::Line { pts, width: *width, color: *color, dash: dash.clone(), closed: false });
                    }
                }
                Prim::Fill { rings, color } => {
                    let rings: Vec<Vec<(f32, f32)>> = rings
                        .iter()
                        .map(|r| clip_ring(r, lo, hi))
                        .filter(|r| r.len() > 2)
                        .map(|r| r.into_iter().map(shift).collect())
                        .collect();
                    if !rings.is_empty() {
                        keep(&mut d, i, Prim::Fill { rings, color: *color });
                    }
                }
                Prim::Image { at, width, height, rgba, ink } => {
                    // The picture's pixels inside, whole ones.
                    let x0 = (lo.0 - at.0).floor().max(0.0) as u32;
                    let y0 = (lo.1 - at.1).floor().max(0.0) as u32;
                    let x1 = ((hi.0 - at.0).ceil().max(0.0) as u32).min(*width);
                    let y1 = ((hi.1 - at.1).ceil().max(0.0) as u32).min(*height);
                    if x0 >= x1 || y0 >= y1 || rgba.len() != (*width as usize) * (*height as usize) * 4 {
                        continue;
                    }
                    let (w, h) = (x1 - x0, y1 - y0);
                    let mut px = Vec::with_capacity((w * h * 4) as usize);
                    for row in y0..y1 {
                        let start = ((row * width + x0) * 4) as usize;
                        px.extend_from_slice(rgba.get(start..start + (w * 4) as usize).unwrap_or_default());
                    }
                    let at = shift((at.0 + x0 as f32, at.1 + y0 as f32));
                    keep(&mut d, i, Prim::Image { at, width: w, height: h, rgba: px, ink: *ink });
                }
            }
        }
        (d, from)
    }
}

impl View {
    /// The same view of the rectangle `lo`–`hi` of its pixels, that rectangle's top left corner its own: each point of the
    /// model is where it was, `lo` pixels up and left.
    pub fn cropped(&self, lo: (f32, f32), hi: (f32, f32)) -> View {
        let (w, h) = ((hi.0 - lo.0).round().max(1.0), (hi.1 - lo.1).round().max(1.0));
        let centre = self.mm((lo.0 + w / 2.0, lo.1 + h / 2.0));
        View { cx: centre.0, cy: centre.1, width: w as u32, height: h as u32, ..*self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::P2;
    use crate::render::Rgb;

    #[test]
    fn a_crop_cuts_lines_and_fills_at_its_edge_and_keeps_the_model_where_it_was() {
        let view = View { angle: 30.0, scale: 0.1, cx: 500.0, cy: 200.0, width: 400, height: 300 };
        let mut d = Drawing::new(view);
        // A line across the whole drawing, a square half in, and one wholly out.
        d.push(Prim::Line { pts: vec![(0.0, 150.0), (400.0, 150.0)], width: 1.0, color: Rgb(0, 0, 0), dash: None, closed: false });
        d.push(Prim::Fill { rings: vec![vec![(50.0, 50.0), (150.0, 50.0), (150.0, 150.0), (50.0, 150.0)]], color: Rgb(9, 9, 9) });
        d.push(Prim::Fill { rings: vec![vec![(350.0, 0.0), (390.0, 0.0), (390.0, 40.0)]], color: Rgb(9, 9, 9) });
        let (c, from) = d.cropped((100.0, 100.0), (300.0, 200.0));
        assert_eq!(from, vec![0, 1], "what lies outside is gone");
        assert_eq!((c.view.width, c.view.height), (200, 100));
        match &c.prims[0] {
            Prim::Line { pts, .. } => assert_eq!(pts, &vec![(0.0, 50.0), (200.0, 50.0)]),
            _ => panic!("not what was cut"),
        }
        match &c.prims[1] {
            Prim::Fill { rings, .. } => {
                let xs: Vec<f32> = rings[0].iter().map(|p| p.0).collect();
                assert!(xs.iter().all(|x| (0.0..=50.0).contains(x)), "{rings:?}");
            }
            _ => panic!("not what was cut"),
        }
        // A point of the model is at the same place in both, the crop's corner off.
        let p = P2(520.0, 180.0);
        let (a, b) = (view.px(p), c.view.px(p));
        assert!((a.0 - 100.0 - b.0).abs() < 1e-3 && (a.1 - 100.0 - b.1).abs() < 1e-3, "{a:?} {b:?}");
    }

    #[test]
    fn a_closed_outline_crossing_the_edge_is_left_as_the_runs_inside() {
        let ring = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let pieces = clip_line(&ring, true, (5.0, -1.0), (20.0, 20.0));
        // The square's right half: down its top from x 5, its right side, back along its bottom to x 5.
        assert_eq!(pieces, vec![vec![(5.0, 0.0), (10.0, 0.0), (10.0, 10.0), (5.0, 10.0)]]);
    }
}
