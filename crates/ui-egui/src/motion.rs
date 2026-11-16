//! What a plan's kept triangles are drawn as while the view zooms: lines that stay sharp at any scale, as a GPU map or a vector
//! design tool draws them (docs/sharp-zoom-research.md). egui's tessellator bakes a line's width and its one-pixel fade into its
//! triangles, so scaled they blur: a hairline at four times is four pixels wide with four of fade each side. Here each segment
//! is a quad that keeps its centre line and how far across each corner lies apart: the shader places the centre line where the
//! zoom puts it, keeps the line as wide on screen as it was drawn, and fades the last pixel on screen. Lettering and annotation
//! keep their pixel widths at any zoom in the exact drawing too; a model line's pen widens with the zoom there, which the exact
//! drawing a worker makes shows a moment later (as a CAD viewer's lineweights are drawn in pixels while it zooms). Fills are
//! their triangles, which scale without blurring. A view that stops moving gets its exact drawing, egui's triangles, from a
//! worker, as before; a pan alone (no zoom) draws those moved.
//!
//! The shapes are the ones [`crate::canvas::paint_prim`] makes for the exact triangles, so the colours, the tints and what is
//! left out are the same.

use bytemuck::{Pod, Zeroable};
use egui::epaint::{self, ColorMode, Shape};
use egui::{Color32, Pos2, Vec2};

/// A vertex as the motion shader takes it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct Vertex {
    /// On its line's centre line (or a fill's corner), points, as the drawing was made.
    pub pos: [f32; 2],
    /// Which way its corner lies from the centre line, a unit across and a unit along (at a cap); none for a fill's.
    pub out: [f32; 2],
    /// Half its line's width, points, as the drawing was made, signed by the side of the centre line it is on; 0 for a fill's.
    pub half: f32,
    /// sRGBA, premultiplied, as egui's.
    pub color: u32,
}

/// The bytes of a [`Vertex`].
pub(crate) const STRIDE: u64 = 24;

/// The vertices and triangles of shapes, for the card.
#[derive(Clone, Debug, Default)]
pub(crate) struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

fn packed(c: Color32) -> u32 {
    u32::from_le_bytes(c.to_array())
}

impl Mesh {
    /// `shape`'s lines and fills added.
    pub(crate) fn add(&mut self, shape: &Shape) {
        match shape {
            Shape::Vec(shapes) => shapes.iter().for_each(|s| self.add(s)),
            Shape::LineSegment { points, stroke } => self.segment(points[0], points[1], stroke.width, stroke.color),
            Shape::Path(path) => {
                if path.fill != Color32::TRANSPARENT && path.points.len() >= 3 {
                    // A convex ring, as egui fills it: a fan.
                    let first = self.vertices.len() as u32;
                    for p in &path.points {
                        self.vertices.push(Vertex { pos: [p.x, p.y], color: packed(path.fill), ..Default::default() });
                    }
                    for i in 1..path.points.len() as u32 - 1 {
                        self.indices.extend([first, first + i, first + i + 1]);
                    }
                }
                if let ColorMode::Solid(color) = path.stroke.color
                    && path.stroke.width > 0.0
                    && color != Color32::TRANSPARENT
                {
                    for pair in path.points.windows(2) {
                        self.segment(pair[0], pair[1], path.stroke.width, color);
                    }
                    if path.closed
                        && let (Some(&last), Some(&first)) = (path.points.last(), path.points.first())
                    {
                        self.segment(last, first, path.stroke.width, color);
                    }
                }
            }
            Shape::Mesh(mesh) => self.mesh(mesh),
            // The canvas's plan shapes are lines, fills and meshes; anything else waits for the exact drawing.
            _ => {}
        }
    }

    fn mesh(&mut self, mesh: &epaint::Mesh) {
        let first = self.vertices.len() as u32;
        self.vertices.extend(mesh.vertices.iter().map(|v| Vertex {
            pos: [v.pos.x, v.pos.y],
            color: packed(v.color),
            ..Default::default()
        }));
        self.indices.extend(mesh.indices.iter().map(|i| first + i));
    }

    /// A line from `a` to `b`, `width` points wide: a quad round its centre line, its ends squared off half its width past them,
    /// so the segments of a polyline overlap at their joints as egui's mitres would cover them.
    fn segment(&mut self, a: Pos2, b: Pos2, width: f32, color: Color32) {
        if width <= 0.0 || color == Color32::TRANSPARENT || !(a.is_finite() && b.is_finite()) {
            return;
        }
        let along: Vec2 = (b - a).normalized();
        let along = if along.is_finite() && along != Vec2::ZERO { along } else { Vec2::X };
        let across = along.rot90();
        let half = width / 2.0;
        let first = self.vertices.len() as u32;
        for (p, end) in [(a, -1.0), (b, 1.0)] {
            for side in [-1.0f32, 1.0] {
                let out = across * side + along * end;
                self.vertices.push(Vertex { pos: [p.x, p.y], out: [out.x, out.y], half: half * side, color: packed(color) });
            }
        }
        self.indices.extend([first, first + 1, first + 3, first, first + 3, first + 2]);
    }

    /// This mesh's indices moved by `base`, for one made after another's vertices.
    pub(crate) fn offset(&mut self, base: u32) {
        self.indices.iter_mut().for_each(|i| *i += base);
    }
}

/// Where the shader puts vertex `v` on screen at scale `k` and moved by `moved`, `ppp` pixels to a point, as `motion.wgsl` does,
/// and how far across its line it is and how wide that is drawn (points): for the tests, which hold the shader's sums to these.
#[cfg(test)]
pub(crate) fn placed(v: &Vertex, k: f32, moved: Vec2, ppp: f32) -> (Pos2, f32, f32) {
    let fade = 1.0 / ppp;
    let half = v.half.abs();
    let reach = if v.half == 0.0 { 0.0 } else { half + fade };
    let at = Pos2::new(v.pos[0], v.pos[1]) * k + moved + Vec2::new(v.out[0], v.out[1]) * reach;
    (at, reach * v.half.signum(), half)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Stroke, pos2};

    #[test]
    fn a_line_zoomed_keeps_its_width_and_its_one_pixel_fade_on_screen() {
        let mut m = Mesh::default();
        m.add(&Shape::line_segment([pos2(10.0, 10.0), pos2(110.0, 10.0)], Stroke::new(3.0, Color32::BLACK)));
        m.add(&Shape::line_segment([pos2(10.0, 20.0), pos2(110.0, 20.0)], Stroke::new(1.0, Color32::BLACK)));
        assert_eq!((m.vertices.len(), m.indices.len()), (8, 12));
        // At four times: its centre line where the zoom puts it, still 3 wide, its corners a pixel past that; the hairline one.
        let (at, across, half) = placed(&m.vertices[1], 4.0, Vec2::ZERO, 1.0);
        assert_eq!((at, across, half), (pos2(40.0 - 2.5, 40.0 - 2.5), 2.5, 1.5));
        let (_, across, _) = placed(&m.vertices[1], 4.0, Vec2::ZERO, 2.0);
        assert_eq!(across, 2.0, "half a point of fade where a point is two pixels");
        let (at, across, half) = placed(&m.vertices[5], 4.0, Vec2::ZERO, 1.0);
        assert_eq!((at, across, half), (pos2(40.0 - 1.5, 80.0 - 1.5), 1.5, 0.5));
    }

    #[test]
    fn fills_are_their_triangles_and_paths_their_segments() {
        let mut m = Mesh::default();
        let square = vec![pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(10.0, 10.0), pos2(0.0, 10.0)];
        m.add(&Shape::convex_polygon(square.clone(), Color32::RED, Stroke::new(2.0, Color32::BLUE)));
        // Four corners filled as two triangles, and four sides of four corners each.
        assert_eq!(m.vertices.len(), 4 + 4 * 4);
        assert_eq!(m.indices.len(), 6 + 4 * 6);
        assert!(m.vertices[..4].iter().all(|v| v.half == 0.0 && v.out == [0.0, 0.0]));
        // A fill's corner goes where the zoom puts it, and nowhere past.
        assert_eq!(placed(&m.vertices[2], 2.0, Vec2::new(5.0, 0.0), 1.0).0, pos2(25.0, 20.0));
        let mut n = Mesh::default();
        n.add(&Shape::Noop);
        n.add(&Shape::line_segment([pos2(1.0, 1.0), pos2(1.0, 1.0)], Stroke::new(1.0, Color32::BLACK)));
        assert_eq!(n.vertices.len(), 4, "a segment of no length is still a dot");
    }
}
