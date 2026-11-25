//! Coordinates shared by drawing tools and their commands. A vertical drawing's x is distance
//! along its section line from its start; y is world height, never model northing.
use crate::geom::{self, P2};
use crate::ids::{Guid, Key};
use crate::model::{Element, End, Face, Line, Model, Reference, Section};

/// The section line a vertical drawing is measured along: a detail's is its parent's, so its annotation keeps the parent's
/// coordinates.
fn section_plane(m: &Model, view: impl Key) -> Option<Section> {
    match m.elements.get(view)? {
        Element::DetailView(d) => match m.elements.get(d.parent)? {
            Element::Section(v) | Element::Elevation(v) => Some(v.clone()),
            _ => None,
        },
        _ => m.section_geometry(view),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DrawingPlane {
    pub origin: [f64; 3],
    pub right: [f64; 3],
    pub up: [f64; 3],
}

impl DrawingPlane {
    pub fn of(m: &Model, view: impl Key) -> Option<Self> {
        if let Some(s) = section_plane(m, view) {
            let u = (s.end - s.start).unit();
            Some(Self { origin: [s.start.0, s.start.1, 0.0], right: [u.0, u.1, 0.0], up: [0.0, 0.0, 1.0] })
        } else {
            let level = m.plan_level(view)?;
            Some(Self { origin: [0.0, 0.0, m.elevation(level)], right: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0] })
        }
    }

    pub fn vector(self, p: P2) -> [f64; 3] {
        std::array::from_fn(|i| self.right[i] * p.0 + self.up[i] * p.1)
    }

    pub fn point(self, p: P2) -> [f64; 3] {
        let by = self.vector(p);
        std::array::from_fn(|i| self.origin[i] + by[i])
    }

    pub fn project(self, p: [f64; 3]) -> P2 {
        let d: [f64; 3] = std::array::from_fn(|i| p[i] - self.origin[i]);
        P2((0..3).map(|i| d[i] * self.right[i]).sum(), (0..3).map(|i| d[i] * self.up[i]).sum())
    }
}

/// A persistent semantic reference resolved in its owning drawing, for validation, dimensions
/// and picking alike. No mesh edge is replaced by a plan reference or an invented endpoint.
pub fn reference(m: &Model, view: Option<Guid>, r: &Reference) -> Result<Line, String> {
    let Some((view, s)) = view.and_then(|v| section_plane(m, v).map(|s| (v, s))) else { return m.reference_geometry(r) };
    match m.elements.get(r.id) {
        Some(Element::DetailLine(l)) if m.same(l.view, view) => m.reference_geometry(r),
        Some(Element::Level(l)) if r.face == Face::Centre && r.point.is_none() => {
            Ok((P2(0.0, l.elevation), P2(s.start.dist(s.end), l.elevation)))
        }
        Some(Element::Grid(g)) if r.face == Face::Centre && r.point.is_none() => {
            let u = (s.end - s.start).unit();
            if u.dot((g.end - g.start).unit()).abs() > 1e-6 {
                return Err("a grid dimension in a section or elevation needs a grid perpendicular to the view line".into());
            }
            let p = geom::line_intersection(s.start, s.end, g.start, g.end).ok_or("the grid does not cross the view plane")?;
            let x = (p - s.start).dot(u);
            Ok((P2(x, 0.0), P2(x, 1.0)))
        }
        _ => Err("section/elevation dimensions support levels, perpendicular grids and detail lines of their own view".into()),
    }
}

/// References available on an already-visible picked element. Datum ends are display extents,
/// not stable model points, so only detail curves offer endpoint references.
pub fn references(m: &Model, view: Guid, id: impl Key) -> Vec<(Reference, Line)> {
    let id = m.guid(id);
    let mut refs = vec![Reference::line(id, Face::Centre)];
    if matches!(m.elements.get(id), Some(Element::DetailLine(_))) {
        refs.extend([End::Start, End::End].map(|point| Reference { id, face: Face::Centre, point: Some(point), link: None }));
    }
    refs.into_iter().filter_map(|r| Some((r, reference(m, Some(view), &r).ok()?))).collect()
}

pub fn pick(m: &Model, view: Guid, id: impl Key, p: P2, near: f64, refs: &[Reference]) -> Option<Reference> {
    let first = refs.iter().find(|r| r.point.is_none()).and_then(|r| reference(m, Some(view), r).ok());
    let mut candidates = references(m, view, id);
    candidates.sort_by_key(|(r, _)| r.point.is_none());
    candidates.into_iter().filter(|(r, (a, b))| r.point.is_some() || first.is_none_or(|(c, d)| geom::parallel(*a, *b, c, d))).find_map(
        |(r, (a, b))| {
            let dist = if r.point.is_some() { p.dist(a) } else { (p - a).cross((b - a).unit()).abs() };
            (dist <= near).then_some(r)
        },
    )
}
