//! A family's origin (O1): its insertion point, where the planes that define the origin cross ([`Family::origin`]), and what
//! keeps the instances placed of a family where they stand when it moves: a component in the project, a door or a window in
//! its wall, and a nested instance in its host. The family's forms, planes and lines stay where they are in its axes; only the
//! point it is placed by changes. Every way of putting a family's axes in the model goes through
//! [`crate::hosting::Placement::by_origin`], which places the family by this point.
//!
//! A family records the origin its instances were placed against (`placed_origin`, the datums' crossing when a file has none,
//! as every file of a build before this one placed by the datums alone). Reading a project, and after every command, each
//! family whose origin is not that record has its instances moved by the difference, turned and mirrored with each, and the
//! record made its origin ([`Model::settle_origins`]), so a reload that moves the origin and an old file whose family already
//! had a plane marked both leave everything where it was drawn.

use super::*;
use crate::hosting::HostRef;
use crate::model::{Element, Model};
use crate::view3d::dot;

/// The origin, in a family's own axes, that the instances placed of it were placed against: what [`Model::settle_origins`]
/// compares with the family's origin. Bookkeeping, not part of what the family is: two families that differ only by it are
/// the same family, so it never makes a reload of the same definition a change.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlacedOrigin(pub P3);

impl PartialEq for PlacedOrigin {
    fn eq(&self, _: &PlacedOrigin) -> bool {
        true
    }
}

impl PlacedOrigin {
    /// Whether it is the datums' crossing, as a file leaves it out.
    pub fn datums(&self) -> bool {
        self.0 == [0.0; 3]
    }

    /// The point, or the datums' crossing where a file gave one out of range.
    fn point(&self) -> P3 {
        if self.0.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD) { self.0 } else { [0.0; 3] }
    }
}

impl Family {
    /// Its insertion point in its own axes: where the planes that define the origin cross, one square to each of x, y and z,
    /// a datum's where no plane of its own takes the origin over in that direction (Ref. Level, Center (Front/Back), Center
    /// (Left/Right)). Those planes stay as a type flexes, so this is the same for every type. An adaptive family has none, its
    /// points placing it: the datums' crossing.
    pub fn origin(&self) -> P3 {
        let mut o = [0.0; 3];
        if self.adaptive.is_some() {
            return o;
        }
        for p in self.planes.iter().filter(|p| p.defines_origin && !p.datum()) {
            let frame = p.frame();
            if let Some(k) = (0..3).find(|&k| frame.z[k].abs() > 1.0 - 1e-9)
                && frame.origin[k].is_finite()
                && frame.origin[k].abs() <= MAX_COORD
            {
                o[k] = frame.origin[k] + 0.0;
            }
        }
        o
    }

    /// Whether a family it nests, at any depth, has an origin other than the one its instances were placed against.
    fn nested_origins_unsettled(&self, depth: usize) -> bool {
        depth <= nested::MAX_NESTING
            && self
                .nested
                .iter()
                .any(|n| n.family.placed_origin.point() != n.family.origin() || n.family.nested_origins_unsettled(depth + 1))
    }

    /// Each family it nests, at any depth, whose origin moved from the one its instances were placed against: its instances
    /// moved by as much where they stand in this family ([`NestedInstance::origin_moved`]), and the record made its origin.
    fn settle_nested_origins(&mut self, depth: usize) {
        if depth > nested::MAX_NESTING {
            return;
        }
        let Family { nested, nested_instances, .. } = self;
        for n in nested.iter_mut() {
            n.family.settle_nested_origins(depth + 1);
            let (was, now) = (n.family.placed_origin.point(), n.family.origin());
            if was == now {
                continue;
            }
            let by: P3 = std::array::from_fn(|k| now[k] - was[k]);
            for inst in nested_instances.iter_mut().filter(|i| i.family == n.name) {
                inst.origin_moved(by);
            }
            n.family.placed_origin = PlacedOrigin(now);
        }
    }
}

impl NestedInstance {
    /// Moved so it stays where it stands after its family's origin moved `by` (in the family's axes): its point on its host
    /// plane by `by`'s x and y turned with it, its offset by `by`'s z. A place or a turn given by a formula stays one.
    fn origin_moved(&mut self, by: P3) {
        let (u, v) = match &self.angle {
            Expr::Number(a) => {
                let (s, c) = a.to_radians().sin_cos();
                (Expr::Number(by[0] * c - by[1] * s), Expr::Number(by[0] * s + by[1] * c))
            }
            Expr::Formula(a) => (
                Expr::Formula(format!("({}) * cos(({a})) - ({}) * sin(({a}))", by[0], by[1])),
                Expr::Formula(format!("({}) * sin(({a})) + ({}) * cos(({a}))", by[0], by[1])),
            ),
        };
        self.at[0] = plus(&self.at[0], &u);
        self.at[1] = plus(&self.at[1], &v);
        self.offset = plus(&self.offset, &Expr::Number(by[2]));
    }
}

/// `a + b`: a number where both are, else a formula.
fn plus(a: &Expr, b: &Expr) -> Expr {
    let text = |e: &Expr| match e {
        Expr::Number(x) => format!("({x})"),
        Expr::Formula(f) => format!("({f})"),
    };
    match (a, b) {
        (_, Expr::Number(y)) if *y == 0.0 => a.clone(),
        (Expr::Number(x), Expr::Number(y)) => Expr::Number(x + y),
        _ => Expr::Formula(format!("{} + {}", text(a), text(b))),
    }
}

impl Model {
    /// Every family's instances placed against an origin that is no longer its own, moved by the difference so they stay where
    /// they stand (O1), and each family's record (`placed_origin`) made its origin. The origin each was placed against is
    /// `before`'s record of the family of that name (what a reload or an edit of a family moved it from), else the family's
    /// own (a project read from a file: the datums' crossing for a file of a build that placed by them). A component standing
    /// on a level or a reference plane moves by the difference turned and mirrored with it; one on a face of a wall, a floor,
    /// a ceiling or a roof, and a door or a window in its wall, keep their place along their host only, the host fixing where
    /// they are across it. The families they nest are settled the same way, in the families that nest them.
    pub fn settle_origins(&mut self, before: Option<&Model>) {
        for (name, was, now) in self.origins_moved(before) {
            if let Some(f) = self.families.get_mut(&name) {
                f.settle_nested_origins(1);
                f.placed_origin = PlacedOrigin(now);
            }
            if was != now {
                self.origin_moved(&name, std::array::from_fn(|k| now[k] - was[k]));
            }
        }
    }

    /// Whether [`Model::settle_origins`] after `before` has anything to do.
    pub fn origins_unsettled(&self, before: &Model) -> bool {
        !self.origins_moved(Some(before)).is_empty()
    }

    /// The families [`Model::settle_origins`] settles: each one's name, the origin its instances were placed against and its
    /// own.
    fn origins_moved(&self, before: Option<&Model>) -> Vec<(String, P3, P3)> {
        if before.is_some_and(|b| b.families.same_allocation(&self.families)) {
            return Vec::new();
        }
        self.families
            .iter()
            .filter_map(|(name, f)| {
                let was = before.and_then(|b| b.families.get(name)).map_or(f.placed_origin, |b| b.placed_origin).point();
                let now = f.origin();
                (was != now || f.placed_origin.0 != now || f.nested_origins_unsettled(1)).then(|| (name.clone(), was, now))
            })
            .collect()
    }

    /// Family `name`'s instances in the project moved so they stay where they stand after its origin moved `by` (its axes).
    fn origin_moved(&mut self, name: &str, by: P3) {
        let ids: Vec<crate::ids::Id> = self
            .elements
            .iter()
            .filter(|(_, e)| match e {
                Element::Component(c) => c.family == name,
                Element::Door(o) | Element::Window(o) => o.family.as_deref() == Some(name),
                _ => false,
            })
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            let moved = match self.elements.get(id) {
                Some(Element::Component(c)) => self.component_origin_moved(c, by).map(Element::Component),
                Some(Element::Door(o)) => Some(Element::Door(opening_origin_moved(o, by))),
                Some(Element::Window(o)) => Some(Element::Window(opening_origin_moved(o, by))),
                _ => None,
            };
            if let (Some(e), Some(slot)) = (moved, self.elements.get_mut(id)) {
                *slot = e;
            }
        }
    }

    /// Component `c` where it stands after its family's origin moved `by`; none for one its adaptive points place.
    fn component_origin_moved(&self, c: &crate::model::Component, by: P3) -> Option<crate::model::Component> {
        if self.placed_by_points(c) {
            return None;
        }
        let placed = self.component_placement(c);
        let local = [by[0], if c.mirrored { -by[1] } else { by[1] }, by[2]];
        let [a, b, z] = placed.axes;
        let w: P3 = std::array::from_fn(|i| local[0] * a[i] + local[1] * b[i] + local[2] * z[i]);
        let mut c = c.clone();
        let Some(mut h) = c.host else {
            c.at = crate::geom::P2(c.at.0 + w[0], c.at.1 + w[1]);
            c.offset += w[2];
            return Some(c);
        };
        match h.host {
            HostRef::Plane(_) => {
                let frame = self.host_frame(&h.host)?;
                h.uv = [h.uv[0] + dot(w, frame.x), h.uv[1] + dot(w, frame.y)];
                h.offset += dot(w, frame.z);
            }
            HostRef::Face { .. } => {
                // Along the face only: across it, the face fixes where it stands.
                let n = self.host_at(&h)?.normal;
                let across = dot(w, n);
                let to: P3 = std::array::from_fn(|i| placed.insertion[i] + w[i] - n[i] * across);
                h.uv = self.host_uv(&h, to)?;
            }
        }
        (c.at, c.offset) = self.hosted_at(&h, c.level)?;
        c.host = Some(h);
        Some(c)
    }
}

/// Door or window `o` where it stands after its family's origin moved `by`: along its wall and up it, flipped as it is; across
/// the wall its middle fixes where it is.
fn opening_origin_moved(o: &crate::model::Opening, by: P3) -> crate::model::Opening {
    let fx = if o.flip_hand { -1.0 } else { 1.0 };
    crate::model::Opening { offset: o.offset + fx * by[0], sill: o.sill + by[2], ..o.clone() }
}
