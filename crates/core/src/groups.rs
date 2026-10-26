//! Detail groups, as Revit's: detail lines, filled regions and text notes of one view made into a group type
//! (`Model::detail_groups`), placed in any view as often as wanted; a change to the type is a change to every one. A group
//! type holds its members as elements about its origin, each a part with its own id and GUID; a [`DetailGroup`] is one
//! placed, at a point, turned and perhaps mirrored. Ungroup gives a placed group's members back as elements of its view.

use serde::{Deserialize, Serialize};

use crate::geom::P2;
use crate::ids::{Guid, Id};
use crate::model::{Element, heading};

/// The most members a group type holds.
pub const MAX_MEMBERS: usize = 10_000;

/// A detail group type: what each one placed draws.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupType {
    pub members: Vec<Member>,
}

/// An element of a group type, about the type's origin; its `view` is none, the view being the placed group's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    /// Its id and GUID, as an element's: given when it's made, kept while it stays where it is in its list.
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: Id,
    #[serde(default, skip_serializing_if = "Guid::is_nil")]
    pub guid: Guid,
    pub element: Element,
}

impl crate::library::HasPart for Member {
    fn ident(&mut self) -> (&mut Id, &mut Guid) {
        (&mut self.id, &mut self.guid)
    }
}

/// A detail group placed in a view: type `type_name`'s members, their origin at `at`, turned `angle` degrees
/// counter-clockwise, and first, with `mirrored`, mirrored across their x axis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetailGroup {
    pub view: Guid,
    #[serde(rename = "type")]
    pub type_name: String,
    pub at: P2,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub angle: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mirrored: bool,
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

/// Whether a detail group can hold `e`: a detail line, a filled region or a text note.
pub fn holds(e: &Element) -> bool {
    matches!(e, Element::DetailLine(_) | Element::FilledRegion(_) | Element::Text(_))
}

/// Where a group at `at`, turned `angle` and perhaps `mirrored`, puts a point of its type: mirrored, turned, then moved.
pub fn placing(at: P2, angle: f64, mirrored: bool) -> impl Fn(P2) -> P2 {
    let (u, v) = (heading(angle), heading(angle + 90.0));
    move |p: P2| {
        let y = if mirrored { -p.1 } else { p.1 };
        at + u * p.0 + v * y
    }
}

/// The members of group `g` where it puts them, each an element of its view.
pub fn placed(m: &crate::model::Model, g: &DetailGroup) -> Vec<Element> {
    let Some(t) = m.detail_groups.get(&g.type_name) else { return Vec::new() };
    let f = placing(g.at, g.angle, g.mirrored);
    let mo = crate::commands::Motion { map: &f, slide: None, mirror: g.mirrored };
    let none = std::collections::BTreeSet::new();
    t.members
        .iter()
        .filter_map(|x| {
            let mut e = crate::commands::transform("detail group", m, x.element.clone(), 0, &mo, &none).ok()?;
            set_view(&mut e, g.view);
            Some(e)
        })
        .collect()
}

/// `e`, a member, put in view `view` (none for a group type's own).
pub fn set_view(e: &mut Element, view: Guid) {
    match e {
        Element::DetailLine(l) => l.view = view,
        Element::FilledRegion(r) => r.view = view,
        Element::Text(n) => n.view = view,
        _ => {}
    }
}

/// The most times a repeating detail repeats.
pub const MAX_REPEATS: usize = 2000;

/// A repeating detail, as Revit's Repeating Detail Component: detail group `type_name` placed along the line from `start`
/// to `end`, every `spacing` mm from `start`, each turned along the line (and, with `mirrored`, mirrored across it).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepeatingDetail {
    pub view: Guid,
    #[serde(rename = "type")]
    pub type_name: String,
    pub start: P2,
    pub end: P2,
    pub spacing: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mirrored: bool,
}

impl RepeatingDetail {
    /// Where its groups go: each one's origin and angle. None past [`MAX_REPEATS`].
    pub fn places(&self) -> Option<Vec<(P2, f64)>> {
        let along = self.end - self.start;
        let len = along.length();
        if !(self.spacing.is_finite() && self.spacing > 0.0 && len.is_finite()) {
            return None;
        }
        let n = (len / self.spacing + 1e-9).floor() + 1.0;
        if n > MAX_REPEATS as f64 {
            return None;
        }
        let angle = along.1.atan2(along.0).to_degrees();
        let u = if len > 0.0 { along * (1.0 / len) } else { P2(1.0, 0.0) };
        Some((0..n as usize).map(|k| (self.start + u * (k as f64 * self.spacing), angle)).collect())
    }

    /// Its groups, each as one placed.
    pub fn groups(&self) -> Vec<DetailGroup> {
        let one =
            |(at, angle): (P2, f64)| DetailGroup { view: self.view, type_name: self.type_name.clone(), at, angle, mirrored: self.mirrored };
        self.places().unwrap_or_default().into_iter().map(one).collect()
    }
}
