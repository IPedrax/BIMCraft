//! A plan's view range, as Revit's: four planes, each measured from a level, which say what the plan shows and how.
//!
//! Between the cut plane and the top only what a plan shows over its cut is seen: doors, windows, casework and generic
//! models ([`shown_over_cut`]), and the overhead lines of roofs, beams and bars. What the cut plane passes through is drawn
//! cut. What lies between the cut plane and the bottom is seen from above, in projection: an element whose top touches the
//! bottom is too. What lies between the bottom and the view depth is drawn in halftone, the underlay; past the view depth,
//! nothing.
//!
//! Each plane is a level and an offset over it: the plan's own (Associated Level), the level above or below it, a level
//! named, or no limit at all (Unlimited: the top up and the bottom and view depth down). The cut plane is always the plan's
//! own level's. A view template's view range is measured from the levels of each plan that follows it, never from a level
//! named, so one template serves every storey.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::model::{CUT_HEIGHT, Element, Guid, Key, Model, TOL};

/// What a plane of a view range is measured from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Base {
    /// The plan's own level.
    #[default]
    Associated,
    /// The next level up from the plan's; none above, no limit.
    Above,
    /// The next level down from the plan's; none below, no limit.
    Below,
    /// No limit: the top as high as the model goes, the bottom and the view depth as low.
    Unlimited,
    /// A level named, by GUID.
    Level(Guid),
}

impl Base {
    pub fn is_associated(&self) -> bool {
        *self == Base::Associated
    }

    /// The level it names, if it names one.
    pub fn level(&self) -> Option<Guid> {
        match self {
            Base::Level(g) => Some(*g),
            _ => None,
        }
    }

    /// Whether it is measured from the level of the plan it is in: a view template can hold it.
    pub fn relative(&self) -> bool {
        !matches!(self, Base::Level(_))
    }
}

/// A plan's view range, as Revit's: mm over the level each plane is measured from. Cut by the `cut` plane (always over the
/// plan's own level), seen from it down to the `bottom` and, beyond in halftone, to the view `depth`; over the cut, up to the
/// `top`, for what a plan shows overhead. The top without an offset of its own (`top` none, measured from its own level) is
/// the level above, as it was before planes were measured from other levels.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ViewRange {
    pub top: Option<f64>,
    pub cut: f64,
    pub bottom: f64,
    pub depth: f64,
    #[serde(skip_serializing_if = "Base::is_associated")]
    pub top_from: Base,
    #[serde(skip_serializing_if = "Base::is_associated")]
    pub bottom_from: Base,
    #[serde(skip_serializing_if = "Base::is_associated")]
    pub depth_from: Base,
}

impl Default for ViewRange {
    fn default() -> ViewRange {
        ViewRange {
            top: None,
            cut: CUT_HEIGHT,
            bottom: 0.0,
            depth: 0.0,
            top_from: Base::Associated,
            bottom_from: Base::Associated,
            depth_from: Base::Associated,
        }
    }
}

/// One of a view range's planes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plane {
    Top,
    Cut,
    Bottom,
    Depth,
}

impl Plane {
    pub const ALL: [Plane; 4] = [Plane::Top, Plane::Cut, Plane::Bottom, Plane::Depth];

    /// Its name, as a message says it.
    pub fn name(self) -> &'static str {
        match self {
            Plane::Top => "top",
            Plane::Cut => "cut plane",
            Plane::Bottom => "bottom",
            Plane::Depth => "view depth",
        }
    }
}

impl ViewRange {
    /// Plane `p`: what it is measured from, and its offset over that.
    pub fn plane(&self, p: Plane) -> (Base, f64) {
        match p {
            // The top as it was: the level above.
            Plane::Top if self.top.is_none() && self.top_from == Base::Associated => (Base::Above, 0.0),
            Plane::Top => (self.top_from, self.top.unwrap_or(0.0)),
            Plane::Cut => (Base::Associated, self.cut),
            Plane::Bottom => (self.bottom_from, self.bottom),
            Plane::Depth => (self.depth_from, self.depth),
        }
    }

    /// It with plane `p` measured from `base`, `offset` over it. The cut plane is always over the plan's own level. The level
    /// above without an offset is written as it always was: no top.
    pub fn with(mut self, p: Plane, base: Base, offset: f64) -> ViewRange {
        match p {
            Plane::Top if base == Base::Above && offset == 0.0 => (self.top, self.top_from) = (None, Base::Associated),
            Plane::Top => (self.top, self.top_from) = (Some(offset), base),
            Plane::Cut => self.cut = offset,
            Plane::Bottom => (self.bottom, self.bottom_from) = (offset, base),
            Plane::Depth => (self.depth, self.depth_from) = (offset, base),
        }
        self
    }

    /// The levels its planes name.
    pub fn levels(&self) -> impl Iterator<Item = Guid> + '_ {
        [self.top_from, self.bottom_from, self.depth_from].into_iter().filter_map(|b| b.level())
    }
}

/// A plan's view range as heights: its top, cut plane, bottom and view depth, mm. Unlimited, the top is infinite and the
/// bottom and the view depth minus infinite.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanRange {
    pub top: f64,
    pub cut: f64,
    pub bottom: f64,
    pub depth: f64,
}

/// Where an element stands in a plan's view range, by its lowest and highest points: what the plan draws of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    /// All of it over the cut plane, some under the top: only what a plan shows over its cut is drawn.
    Above,
    /// The cut plane passes through it: drawn cut.
    Cut,
    /// Under the cut plane, its top at or over the bottom: seen from above, in projection.
    Projection,
    /// Its top under the bottom and over the view depth: in projection, in halftone.
    Underlay,
}

impl PlanRange {
    /// Where an element whose lowest point is `bottom` and highest `top` stands, mm; none, where the plan doesn't see it: all
    /// of it over the top, or under the view depth. One whose top touches the bottom is in projection, and one whose top
    /// touches the view depth in the underlay.
    pub fn zone(&self, bottom: f64, top: f64) -> Option<Zone> {
        if bottom.is_nan() || top.is_nan() || bottom > top || bottom > self.top || top < self.depth - TOL {
            return None;
        }
        Some(if bottom <= self.cut && self.cut < top {
            Zone::Cut
        } else if bottom > self.cut {
            Zone::Above
        } else if top >= self.bottom - TOL {
            Zone::Projection
        } else {
            Zone::Underlay
        })
    }

    /// Whether what reaches up to `top` is in the underlay: under the bottom, over the view depth.
    pub fn under(&self, top: f64) -> bool {
        top < self.bottom - TOL && top >= self.depth - TOL
    }
}

/// Whether a plan shows elements of model category `category` (as [`crate::graphics::category_of`] names it) over its cut
/// plane, under its top, as Revit does: doors, windows, casework and generic models.
pub fn shown_over_cut(category: &str) -> bool {
    matches!(category, "door" | "window" | "casework" | "generic")
}

impl Model {
    /// The next level down from `level`.
    pub fn level_below(&self, level: impl Key) -> Option<Guid> {
        let z = self.elevation(level);
        self.levels().into_iter().rev().find(|(_, l)| l.elevation < z - TOL).map(|(id, _)| self.guid(id))
    }

    /// The height of plane `p` of range `r` in a plan of `level`, mm: infinite where it has no limit.
    pub fn plane_height(&self, level: Guid, r: &ViewRange, p: Plane) -> f64 {
        let (base, offset) = r.plane(p);
        let up = p == Plane::Top;
        let none = if up { f64::INFINITY } else { f64::NEG_INFINITY };
        match base {
            Base::Associated => self.elevation(level) + offset,
            Base::Above => self.level_above(level).map_or(f64::INFINITY, |a| self.elevation(a) + offset),
            Base::Below => self.level_below(level).map_or(f64::NEG_INFINITY, |b| self.elevation(b) + offset),
            Base::Unlimited => none,
            // A level that isn't there (`Model::check` says so) is the plan's own.
            Base::Level(g) => self.level(g).map_or(self.elevation(level), |l| l.elevation) + offset,
        }
    }

    /// The heights of range `r` in a plan of `level`, as they are: the top may be under the cut plane.
    pub fn range_heights(&self, level: Guid, r: &ViewRange) -> PlanRange {
        let z = |p| self.plane_height(level, r, p);
        PlanRange { top: z(Plane::Top), cut: z(Plane::Cut), bottom: z(Plane::Bottom), depth: z(Plane::Depth) }
    }

    /// Whether the view range plan `key` has, its own or its template's, is sound: the levels it names are levels, and its
    /// planes run top ≥ cut plane ≥ bottom ≥ view depth where they are. The top left at the level above may be under the cut
    /// plane, as it always might: the plan then shows nothing over its cut.
    pub(crate) fn check_range(&self, who: &str, key: impl Key) -> Result<(), String> {
        let key = self.guid(key);
        let r = self.view_range_of(key);
        for g in r.levels() {
            if !matches!(self.elements.get(g), Some(Element::Level(_))) {
                return Err(format!("{who}: its view range is measured from {g}, which is not a level"));
            }
        }
        let Some(level) = self.plan_level(key).map(|l| self.guid(l)) else { return Ok(()) };
        let h = self.range_heights(level, &r);
        // The top as files have always had it (none: the level above) isn't held to the cut plane; one given is.
        let implicit_top = r.top.is_none() && r.top_from == Base::Associated;
        // Where a plane is, as the message says it: its height, and what it is measured from.
        let shown = |p: Plane, z: f64| {
            let (base, offset) = r.plane(p);
            let from = match base {
                Base::Associated => "its own level".to_string(),
                Base::Above => "the level above".to_string(),
                Base::Below => "the level below".to_string(),
                Base::Unlimited => return "unlimited".to_string(),
                Base::Level(g) => self.level(g).map_or_else(|| g.to_string(), |l| format!("`{}`", l.name)),
            };
            if z.is_infinite() { format!("unlimited: {from} is missing") } else { format!("{}, {from} {offset:+}", self.with_unit(z)) }
        };
        let order = [(Plane::Top, h.top), (Plane::Cut, h.cut), (Plane::Bottom, h.bottom), (Plane::Depth, h.depth)];
        for pair in order.windows(2) {
            let [(p, hi), (q, lo)] = [pair[0], pair[1]];
            if p == Plane::Top && implicit_top {
                continue;
            }
            if hi < lo - TOL {
                return Err(clash(who, (p, &shown(p, hi)), (q, &shown(q, lo))));
            }
        }
        Ok(())
    }
}

/// Whether `r` is a view range that makes sense on its own: usable numbers; planes all measured from the plan's own level
/// running top ≥ cut plane ≥ bottom ≥ view depth; and, in a template (`template`), no level named, since each plan that
/// follows it measures from its own.
pub(crate) fn range_ok(who: &str, r: &ViewRange, template: bool) -> Result<(), String> {
    for (what, v) in [("cut", r.cut), ("bottom", r.bottom), ("depth", r.depth)].into_iter().chain(r.top.map(|t| ("top", t))) {
        crate::model::coord(&format!("{who} view range {what}"), v)?;
    }
    if template && r.levels().next().is_some() {
        return Err(format!(
            "{who} view range names a level; a template's planes are measured from the level of each plan that follows it: \
             associated, above, below or unlimited"
        ));
    }
    // The planes measured from the plan's own level can be put in order without a plan; the others, in each plan.
    let top = r.top.filter(|_| r.top_from == Base::Associated);
    let mine: Vec<(Plane, f64)> = [
        top.map(|t| (Plane::Top, t)),
        Some((Plane::Cut, r.cut)),
        (r.bottom_from == Base::Associated).then_some((Plane::Bottom, r.bottom)),
        (r.depth_from == Base::Associated).then_some((Plane::Depth, r.depth)),
    ]
    .into_iter()
    .flatten()
    .collect();
    if let Some(w) = mine.windows(2).find(|w| w[0].1 < w[1].1) {
        let over = |(p, v): (Plane, f64)| (p, format!("{v:+} over the plan's level"));
        let ((p, a), (q, b)) = (over(w[0]), over(w[1]));
        return Err(clash(who, (p, &a), (q, &b)));
    }
    Ok(())
}

/// What is wrong where plane `upper`, which should be at or over plane `lower`, is under it: which plane is out of place, and
/// against which, each where it is (`upper.1`, `lower.1`).
fn clash(who: &str, upper: (Plane, &str), lower: (Plane, &str)) -> String {
    let ((p, at_p), (q, at_q)) = (upper, lower);
    let rule = match (p, q) {
        (Plane::Top, Plane::Cut) => "the cut plane can't be higher than the top".to_string(),
        (Plane::Bottom, Plane::Depth) => "the bottom can't be lower than the view depth".to_string(),
        _ => format!("the {} can't be higher than the {}", q.name(), p.name()),
    };
    format!("{who}: the view range's {} ({at_q}) is above its {} ({at_p}): {rule}", q.name(), p.name())
}

/// Range `r` of a plan of `level` with each plane that names a level in `which` measured from `level` instead, at the height
/// it was.
fn rebase(m: &Model, level: Guid, r: &ViewRange, which: impl Fn(&Guid) -> bool) -> ViewRange {
    let mut now = *r;
    for p in [Plane::Top, Plane::Bottom, Plane::Depth] {
        if let (Base::Level(g), offset) = r.plane(p)
            && which(&g)
        {
            now = now.with(p, Base::Associated, m.elevation(g) + offset - m.elevation(level));
        }
    }
    now
}

/// Range `r` of a plan of `level` measured from that plan's levels alone: a plane that names a level is measured from
/// `level` at the height it was. What a view template takes from a plan.
pub fn relative(m: &Model, level: Guid, r: &ViewRange) -> ViewRange {
    rebase(m, level, r, |_| true)
}

/// The plans whose own view ranges name a level in `gone`, by id, and their ranges with those planes measured from their own
/// level again at the height they were: what a delete does to them before the levels go.
pub(crate) fn rebased(m: &Model, gone: &BTreeSet<Guid>) -> Vec<(crate::model::Id, ViewRange)> {
    let mut out = Vec::new();
    for (&id, e) in m.elements.iter() {
        let (own, r) = match e {
            Element::Level(l) => (m.guid(id), l.view_range),
            Element::PlanView(v) => (v.level, v.view_range),
            _ => continue,
        };
        if gone.contains(&own) || !r.levels().any(|g| gone.contains(&g)) {
            continue;
        }
        out.push((id, rebase(m, own, &r, |g| gone.contains(g))));
    }
    out
}

/// What plan `e`'s own view range is, mutably: a level's or a plan view's.
pub(crate) fn of_mut(e: &mut Element) -> Option<&mut ViewRange> {
    match e {
        Element::Level(l) => Some(&mut l.view_range),
        Element::PlanView(v) => Some(&mut v.view_range),
        _ => None,
    }
}
