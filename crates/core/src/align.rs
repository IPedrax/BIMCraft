//! Plan alignment: the actual straight edges and axes that can be picked, and the rigid motion
//! that puts one on another. These are independent of dimension references and their face names.

use crate::curves::Curve;
use crate::geom::{self, P2};
use crate::model::{Element, Id, Line, Model, TOL, heading};

fn path(points: &[P2], closed: bool) -> Vec<Line> {
    points.windows(2).map(|w| (w[0], w[1])).chain(closed.then(|| points.last().copied().zip(points.first().copied())).flatten()).collect()
}

/// The real straight segments of a curve, without substituting chords for arcs or splines.
pub fn curve_lines(c: &Curve) -> Vec<Line> {
    match c {
        Curve::Poly { points, arcs, closed } => path(points, *closed)
            .into_iter()
            .enumerate()
            .filter(|(i, _)| arcs.get(*i).copied().unwrap_or(0.0) == 0.0)
            .map(|(_, line)| line)
            .collect(),
        // Curves are not approximated by pickable straight chords.
        _ => Vec::new(),
    }
}

fn axes(at: P2, angle: f64, half: f64) -> Vec<Line> {
    [heading(angle), heading(angle + 90.0)].into_iter().map(|u| (at - u * half, at + u * half)).collect()
}

/// Element `id`'s straight plan edges and reference axes. No lines for view metadata or for a
/// curved edge that cannot be made coincident with a straight reference.
pub fn lines(m: &Model, id: Id) -> Vec<Line> {
    let Some(e) = m.elements.get(id) else { return Vec::new() };
    let refs = m.reference_lines(id);
    let out = if !refs.is_empty() { refs.into_iter().map(|(_, l)| l).collect() } else { element_lines(m, e) };
    out.into_iter().filter(|(a, b)| a.dist(*b) > TOL).collect()
}

fn element_lines(m: &Model, e: &Element) -> Vec<Line> {
    if let Some(sketch) = e.sketch().filter(|s| !s.is_empty()) {
        return sketch.iter().filter(|l| !l.invisible).flat_map(|l| curve_lines(&l.curve)).collect();
    }
    match e {
        Element::Column(c) | Element::StructuralColumn(c) | Element::Footing(c) => {
            let Some(t) = m.column_type(e) else { return Vec::new() };
            let mut out = if t.round { Vec::new() } else { path(&m.column_outline(e), true) };
            out.extend(axes(c.at, c.angle, t.width.max(t.depth) / 2.0));
            out
        }
        crate::model::run_arm!(r) => {
            let mut out = m.run_geom(e).map_or_else(Vec::new, |g| {
                if g.vertical() && g.round { axes(r.start, 0.0, g.width / 2.0) } else { path(&g.footprint(), true) }
            });
            out.push((r.start, r.end));
            out
        }
        Element::Component(c) => {
            let mut out: Vec<Line> = m.component_pieces(c).into_iter().filter(|p| !p.round).flat_map(|p| path(&p.outline, true)).collect();
            out.extend(axes(c.at, c.angle, 100.0));
            out
        }
        Element::Floor(f) => std::iter::once(&f.boundary).chain(&f.openings).flat_map(|r| path(r, true)).collect(),
        Element::Ceiling(c) => std::iter::once(&c.boundary).chain(&c.openings).flat_map(|r| path(r, true)).collect(),
        Element::Roof(r) => {
            std::iter::once(m.roof_outline(r)).chain(r.courtyards.clone()).chain(r.openings.clone()).flat_map(|r| path(&r, true)).collect()
        }
        Element::Shaft(s) => path(&s.boundary, true),
        Element::Mass(ms) => path(&ms.boundary, true),
        Element::DetailLine(l) => curve_lines(&l.curve),
        Element::FilledRegion(r) => r.sketch.iter().filter(|l| !l.invisible).flat_map(|l| curve_lines(&l.curve)).collect(),
        Element::DetailGroup(g) => crate::groups::placed(m, g).iter().flat_map(|e| element_lines(m, e)).collect(),
        Element::RepeatingDetail(r) => {
            r.groups().iter().flat_map(|g| crate::groups::placed(m, g)).flat_map(|e| element_lines(m, &e)).collect()
        }
        Element::Stair(_) | Element::Ramp(_) => crate::stairs::layout(m, e).map_or_else(Vec::new, |l| {
            l.runs.iter().flat_map(|r| path(&r.ring(l.width), true)).chain(l.pads.iter().flat_map(|p| path(&p.ring, true))).collect()
        }),
        Element::Railing(r) => path(&crate::stairs::railing(m, r).iter().map(|(p, _)| *p).collect::<Vec<_>>(), false),
        Element::RoomSeparator(r) => vec![(r.start, r.end)],
        Element::Section(s) | Element::Elevation(s) => vec![(s.start, s.end)],
        Element::Text(n) => axes(n.at, n.angle, m.text_types.get(&n.type_name).map_or(crate::model::NOTE_SIZE, |t| t.size)),
        Element::PointCloud(p) => axes(p.at, p.angle, 100.0),
        Element::Link(l) => axes(l.at, l.angle, 100.0),
        Element::CadImport(ci) => {
            let Some(cad) = m.cads.get(&ci.cad) else { return Vec::new() };
            let place = crate::model::cad_place(ci);
            cad.dxf()
                .entities
                .iter()
                .filter(|e| cad.shows(e, &ci.hidden))
                .filter(|e| {
                    matches!(e.shape, crate::dxf::Shape::Line(..) | crate::dxf::Shape::Polyline { .. } | crate::dxf::Shape::Fill { .. })
                })
                .flat_map(|e| crate::cad::paths(&e.shape))
                .flat_map(|(pts, closed)| path(&pts, closed))
                .map(|(a, b)| (place(a), place(b)))
                .collect()
        }
        // These have stable references above, or no location in a plan.
        Element::Wall(_)
        | Element::Door(_)
        | Element::Window(_)
        | Element::Grid(_)
        | Element::Level(_)
        | Element::Dimension(_)
        | Element::Sheet(_)
        | Element::Viewport(_)
        | Element::Schedule(_)
        | Element::View3D(_)
        | Element::PlanView(_) => Vec::new(),
        Element::Room(_) | Element::Spot(_) | Element::Tag(_) | Element::Rebar(_) | Element::Circuit(_) => Vec::new(),
    }
}

/// The nearest alignment edge/axis, and its distance from the cursor in model millimetres.
pub fn nearest(m: &Model, id: Id, at: P2) -> Option<(Line, f64)> {
    lines(m, id).into_iter().map(|l| (l, geom::dist_to_segment(at, l.0, l.1))).min_by(|a, b| a.1.total_cmp(&b.1))
}

/// Align a draft sketch's picked curve, keeping its identity and the endpoints joined to it.
/// `fixed` is the sketch reference, when there is one: that curve stays where it was. The
/// returned draft is validated before the caller replaces any lines; Finish still solves its loops.
pub fn sketch_lines(
    lines: &[crate::sketch::SketchLine],
    target: usize,
    reference: Line,
    line: Line,
    fixed: Option<usize>,
) -> Result<Vec<crate::sketch::SketchLine>, String> {
    if fixed == Some(target) {
        return Err("align: pick a different sketch line to align".into());
    }
    let before = lines.get(target).ok_or_else(|| "align: the sketch line no longer exists".to_string())?;
    let placement = Placement::new(reference, line)?;
    let moved = before.map(|q| placement.map(q));
    let junctions = [(before.start(), moved.start()), (before.end(), moved.end())];
    let mut out = lines.to_vec();
    for (i, l) in out.iter_mut().enumerate() {
        if i == target {
            *l = moved.clone();
        } else if fixed != Some(i) && !before.curve.is_closed() {
            match &mut l.curve {
                Curve::Poly { points, .. } | Curve::Fit { points, .. } | Curve::Spline { control: points, .. } => {
                    // Read each old point once: a new endpoint can coincide with another old one.
                    for p in points {
                        if let Some((_, to)) = junctions.iter().find(|(from, _)| from.dist(*p) <= TOL) {
                            *p = *to;
                        }
                    }
                }
                curve if !curve.is_closed() => {
                    let old = curve.clone();
                    for (from, to) in junctions {
                        if old.start().dist(from) <= TOL || old.end().dist(from) <= TOL {
                            *curve = curve.moved(from, to);
                        }
                    }
                }
                _ => {}
            }
        }
        l.curve.check("align")?;
    }
    Ok(out)
}

/// A turn around the target line's midpoint, then the shortest translation onto the reference.
/// Parallel lines pointing in opposite directions do not cause a half turn.
pub(crate) struct Placement {
    pub(crate) about: P2,
    pub(crate) angle: f64,
    pub(crate) by: P2,
}

impl Placement {
    pub(crate) fn new(reference: Line, line: Line) -> Result<Self, String> {
        if reference.0.dist(reference.1) <= TOL || line.0.dist(line.1) <= TOL {
            return Err("align: a reference or target line has no length".into());
        }
        let (u, v) = ((reference.1 - reference.0).unit(), (line.1 - line.0).unit());
        let angle = (v.cross(u).atan2(v.dot(u)).to_degrees() + 90.0).rem_euclid(180.0) - 90.0;
        let about = (line.0 + line.1) * 0.5;
        let n = u.perp();
        Ok(Self { about, angle, by: n * (reference.0 - about).dot(n) })
    }

    pub(crate) fn map(&self, q: P2) -> P2 {
        let (s, c) = self.angle.to_radians().sin_cos();
        let d = q - self.about;
        crate::commands::micron(self.about + P2(d.0 * c - d.1 * s, d.0 * s + d.1 * c) + self.by)
    }
}
