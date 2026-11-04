//! What a model's elements point at, looked up the other way, and its walls and dimensions: kept with each model, and brought up to date from the model
//! it was copied from by what changed since ([`Elements::changed_since`]), not found again among every element.
//!
//! A model holds its [`Lookup`] once something asked for it, for the state its elements are in. A copy of the model (what
//! a command changes) starts from that one: the first time it is asked for, the elements the command touched have their
//! references taken out and put in again. Each part is held in chunks behind `Arc`s, as the elements are, so the copy
//! shares what didn't change. Where the change can't be followed (a sweep through every element, another model's
//! elements) it is built again.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::sync::{Arc, Mutex};

use crate::ids::{Elements, Guid, Id};
use crate::model::Element;

const SHIFT: u32 = 10;

fn bucket(g: &Guid) -> u16 {
    (g.as_u128() >> 116) as u16
}

/// The references of a model's elements, both ways, and its walls.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lookup {
    /// Each element's references, by chunk of ids.
    refs: BTreeMap<Id, Arc<BTreeMap<Id, Vec<Guid>>>>,
    /// What points at each GUID, by bucket of GUIDs.
    referrers: BTreeMap<u16, Arc<HashMap<Guid, BTreeSet<Id>>>>,
    /// The walls.
    walls: Arc<BTreeSet<Id>>,
    /// The dimensions.
    dimensions: Arc<BTreeSet<Id>>,
    /// The grids.
    grids: Arc<BTreeSet<Id>>,
    /// Each plan tile's walls, by band of height: those whose location line's box reaches into it (an arc's, its circle's)
    /// and whose heights reach the band. A wall whose heights the lookup doesn't know (one attached to what's over or
    /// under it) is in band [`ANY`] of its tiles.
    /// (A tile's bands behind one `Arc` and each band's walls behind another, so a copy shares all but what an edit touched.)
    tiles: BTreeMap<(i64, i64), Arc<Bands>>,
    /// Each wall's tiles and bands, `[x0, y0, x1, y1, b0, b1]`; or none, a wall in no tile (too long for them, or not finite).
    tiled: Arc<BTreeMap<Id, Option<[i64; 6]>>>,
    /// The walls in no tile: a candidate everywhere.
    everywhere: Arc<BTreeSet<Id>>,
    /// The tiled walls by band of height, wherever they are in plan (band [`ANY`]: those whose heights aren't known).
    bands: BTreeMap<i64, Arc<BTreeSet<Id>>>,
    /// The elements of the kinds a project has few of ([`RARE`]), by category.
    rare: BTreeMap<&'static str, Arc<BTreeSet<Id>>>,
    /// The elements phased, or in a design option: with a value of a name [`crate::phasing::reserved`] keeps.
    phased: Arc<BTreeSet<Id>>,
    /// The levels: walls' heights are of them, so when one changes the lookup is made again.
    levels: Arc<BTreeSet<Id>>,
    /// Components and runs by [`Group`], each group by band of the heights the elements' own values give: a component's
    /// base, a run's ends. What the library adds (a family's geometry, a run type's depth) a plan adds as it asks.
    heights: BTreeMap<Arc<Group>, Arc<Bands>>,
    /// Each banded element's group and bands, by chunk of ids.
    banded: BTreeMap<Id, Arc<Banded>>,
    /// What a plan finds no other way: the elements of the kinds it looks for among all of them (grids, floors, roofs,
    /// stairs, sections…), and the runs too steep to band.
    other: Arc<BTreeSet<Id>>,
}

/// What a banded element's heights share with the others of its group beside its own values: a component's family, type,
/// mirroring and values (its geometry), a run's category and type (its depth).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    Component { family: String, type_name: String, mirrored: bool, params: String },
    Run { category: &'static str, type_name: String },
}

/// How much steeper than its plan length a run may be and be banded: its section adds at most twice its depth over and
/// under its ends (`runs::Geom::half`, and its justification).
const STEEP: f64 = 2.0;

/// The kinds a plan finds by height, by view, by level or by host, not in [`Lookup::other`].
fn found_otherwise(e: &Element) -> bool {
    matches!(
        e,
        Element::Wall(_)
            | Element::Door(_)
            | Element::Window(_)
            | Element::Component(_)
            | Element::Room(_)
            | Element::RoomSeparator(_)
            | Element::Ceiling(_)
            | Element::Text(_)
            | Element::DetailLine(_)
            | Element::FilledRegion(_)
            | Element::Dimension(_)
            | Element::Spot(_)
            | Element::DetailGroup(_)
            | Element::RepeatingDetail(_)
            | Element::Column(_)
            | Element::StructuralColumn(_)
            | Element::Tag(_)
    )
}

/// A level's elevation as `elements` place it: as `Model::elevation` gives it, 0 for what isn't a level.
fn elevation_in(elements: &Elements, level: Guid) -> f64 {
    match elements.get(level) {
        Some(Element::Level(l)) => l.elevation,
        _ => 0.0,
    }
}

/// The group and own heights of an element banded by height; none for another, or a run too steep.
fn banding(elements: &Elements, e: &Element) -> Option<(Group, f64, f64)> {
    if let Element::Component(c) = e {
        let params = serde_json::to_string(&c.params).unwrap_or_default();
        let group = Group::Component { family: c.family.clone(), type_name: c.type_name.clone(), mirrored: c.mirrored, params };
        let base = elevation_in(elements, c.level) + c.offset;
        return Some((group, base, base));
    }
    let r = e.run()?;
    let base = elevation_in(elements, r.level);
    let (z0, z1) = (base + r.offset, base + r.end_offset.unwrap_or(r.offset));
    let plan = r.start.dist(r.end);
    if plan > crate::model::TOL && (z1 - z0).hypot(plan) / plan > STEEP {
        return None;
    }
    Some((Group::Run { category: e.category(), type_name: r.type_name.clone() }, z0.min(z1), z0.max(z1)))
}

/// The most bands one element is kept in; past that, in [`ANY`].
const MAX_BANDS: i64 = 64;

/// The categories a project has few elements of (a column to a hundred others), which views, checks and rooms look for
/// among all of them: kept by kind. Its views, sheets and viewports among them: a drawing set of a thousand sheets is a
/// thousandth of a million elements.
pub const RARE: &[&str] = &[
    "pointcloud",
    "cadimport",
    "mass",
    "link",
    "column",
    "structuralcolumn",
    "shaft",
    "sheet",
    "viewport",
    "planview",
    "section",
    "elevation",
    "schedule",
    "view3d",
];

/// The categories of a drawing set: views, sheets and what places a view on a sheet. Their names and numbers are kept
/// apart, and a view goes on one sheet, so they are checked together ([`crate::model::Model::check_changed`]).
pub const DRAWING_SET: &[&str] = &["sheet", "viewport", "planview", "section", "elevation", "schedule", "view3d"];

/// Banded elements' groups and bands, by id.
type Banded = BTreeMap<Id, (Arc<Group>, [i64; 2])>;

/// A tile's walls by band of height.
type Bands = BTreeMap<i64, Arc<BTreeSet<Id>>>;

/// Plan tiles, mm on a side, and the most one wall goes in.
const TILE: f64 = 6000.0;
const MAX_TILES: i64 = 4096;

fn tile(v: f64) -> i64 {
    (v / TILE).floor() as i64
}

/// Bands of height, mm, and the band of walls whose heights aren't known, which every height reaches.
const BAND: f64 = 1000.0;
const ANY: i64 = i64::MIN;

/// The bands from `lo` to `hi`, either way up: `[ANY, ANY]` where they aren't finite numbers, or when tiles × bands would
/// be more than `MAX_TILES`. Heights overlap only strictly ([`crate::walls::stacked`]), so a top on a band's bottom
/// doesn't reach into it; two spans that overlap share a band (one of no height, the band it is in).
fn bands_of(lo: f64, hi: f64, tiles: i64) -> [i64; 2] {
    let (lo, hi) = (lo.min(hi), lo.max(hi));
    if !(lo.is_finite() && hi.is_finite() && lo.abs() < 1e12 && hi.abs() < 1e12) {
        return [ANY, ANY];
    }
    let b0 = (lo / BAND).floor() as i64;
    let b = [b0, ((hi / BAND).ceil() as i64 - 1).max(b0)];
    if (b[1] - b[0] + 1).saturating_mul(tiles) > MAX_TILES { [ANY, ANY] } else { b }
}

/// Wall `w`'s heights, as `elements` place its levels, where they are its own: not attached to what's over or under it.
fn own_heights(elements: &Elements, w: &crate::model::Wall) -> Option<(f64, f64)> {
    if w.attach_base.is_some() || w.attach_top.is_some() {
        return None;
    }
    // As `Model::own_base` and `Model::own_top` find them.
    let elevation = |g: Guid| match elements.get(g) {
        Some(Element::Level(l)) => l.elevation,
        _ => 0.0,
    };
    let base = elevation(w.level) + w.base_offset;
    Some((base, w.top_level.map_or(base + w.height, |t| elevation(t) + w.top_offset)))
}

/// The tiles a box `[x0, y0, x1, y1]` reaches, if it's of finite numbers and not too big for them.
fn tiles_of(b: [f64; 4]) -> Option<[i64; 4]> {
    if !b.iter().all(|v| v.is_finite() && v.abs() < 1e12) {
        return None;
    }
    let t = [tile(b[0]), tile(b[1]), tile(b[2]), tile(b[3])];
    ((t[2] - t[0] + 1).saturating_mul(t[3] - t[1] + 1) <= MAX_TILES).then_some(t)
}

/// A wall's location line's box: its ends', or an arc's circle's.
pub(crate) fn axis_box(w: &crate::model::Wall) -> [f64; 4] {
    match crate::walls::Axis::of(w).arc() {
        Some((c, r, _, _)) => [c.0 - r, c.1 - r, c.0 + r, c.1 + r],
        None => [w.start.0.min(w.end.0), w.start.1.min(w.end.1), w.start.0.max(w.end.0), w.start.1.max(w.end.1)],
    }
}

impl Lookup {
    /// Of every element of `elements`.
    pub fn of(elements: &Elements) -> Lookup {
        let mut l = Lookup::default();
        for (&id, e) in elements {
            l.put(id, e, elements);
        }
        l
    }

    /// This one, of the elements `before` was of, brought up to `elements` where `ids` changed.
    fn updated(&self, elements: &Elements, ids: &BTreeSet<Id>) -> Lookup {
        if ids.iter().any(|id| self.levels.contains(id) || matches!(elements.get(*id), Some(Element::Level(_)))) {
            return Lookup::of(elements);
        }
        let mut l = self.clone();
        for &id in ids {
            l.take(id);
            if let Some(e) = elements.get(id) {
                l.put(id, e, elements);
            }
        }
        l
    }

    fn put(&mut self, id: Id, e: &Element, elements: &Elements) {
        let refs = e.refs();
        for g in &refs {
            Arc::make_mut(self.referrers.entry(bucket(g)).or_default()).entry(*g).or_default().insert(id);
        }
        if !refs.is_empty() {
            Arc::make_mut(self.refs.entry(id >> SHIFT).or_default()).insert(id, refs);
        }
        match e {
            Element::Wall(w) => {
                Arc::make_mut(&mut self.walls).insert(id);
                let t = tiles_of(axis_box(w)).map(|[x0, y0, x1, y1]| {
                    let tiles = (x1 - x0 + 1) * (y1 - y0 + 1);
                    let [b0, b1] = own_heights(elements, w).map_or([ANY, ANY], |(lo, hi)| bands_of(lo, hi, tiles));
                    [x0, y0, x1, y1, b0, b1]
                });
                if let Some([x0, y0, x1, y1, b0, b1]) = t {
                    for b in b0..=b1 {
                        Arc::make_mut(self.bands.entry(b).or_default()).insert(id);
                    }
                    for x in x0..=x1 {
                        for y in y0..=y1 {
                            for b in b0..=b1 {
                                let bands = Arc::make_mut(self.tiles.entry((x, y)).or_default());
                                Arc::make_mut(bands.entry(b).or_default()).insert(id);
                            }
                        }
                    }
                }
                if t.is_none() {
                    Arc::make_mut(&mut self.everywhere).insert(id);
                }
                Arc::make_mut(&mut self.tiled).insert(id, t);
            }
            Element::Level(_) => {
                Arc::make_mut(&mut self.levels).insert(id);
            }
            Element::Dimension(_) => {
                Arc::make_mut(&mut self.dimensions).insert(id);
            }
            Element::Grid(_) => {
                Arc::make_mut(&mut self.grids).insert(id);
            }
            _ => {}
        }
        if let Some(cat) = RARE.iter().find(|c| **c == e.category()) {
            Arc::make_mut(self.rare.entry(cat).or_default()).insert(id);
        }
        match banding(elements, e) {
            Some((group, lo, hi)) => {
                let [b0, b1] = bands_of(lo, hi, 1);
                let b = if b0 != ANY && b1 - b0 >= MAX_BANDS { [ANY, ANY] } else { [b0, b1] };
                let group = match self.heights.get_key_value(&group) {
                    Some((g, _)) => Arc::clone(g),
                    None => Arc::new(group),
                };
                let bands = Arc::make_mut(self.heights.entry(Arc::clone(&group)).or_default());
                for band in if b[0] == ANY { ANY..=ANY } else { b[0]..=b[1] } {
                    Arc::make_mut(bands.entry(band).or_default()).insert(id);
                }
                Arc::make_mut(self.banded.entry(id >> SHIFT).or_default()).insert(id, (group, b));
            }
            None if !found_otherwise(e) => {
                Arc::make_mut(&mut self.other).insert(id);
            }
            None => {}
        }
        if e.params().is_some_and(|p| p.keys().any(|k| crate::phasing::reserved(k))) {
            Arc::make_mut(&mut self.phased).insert(id);
        }
    }

    fn take(&mut self, id: Id) {
        for set in self.rare.values_mut() {
            if set.contains(&id) {
                Arc::make_mut(set).remove(&id);
            }
        }
        self.rare.retain(|_, set| !set.is_empty());
        if let Some(chunk) = self.banded.get_mut(&(id >> SHIFT))
            && chunk.contains_key(&id)
        {
            if let Some((group, b)) = Arc::make_mut(chunk).remove(&id)
                && let Some(bands) = self.heights.get_mut(&group)
            {
                let bands_now = Arc::make_mut(bands);
                for band in if b[0] == ANY { ANY..=ANY } else { b[0]..=b[1] } {
                    if let Some(set) = bands_now.get_mut(&band) {
                        Arc::make_mut(set).remove(&id);
                        if set.is_empty() {
                            bands_now.remove(&band);
                        }
                    }
                }
                if bands_now.is_empty() {
                    self.heights.remove(&group);
                }
            }
            if chunk.is_empty() {
                self.banded.remove(&(id >> SHIFT));
            }
        }
        for set in [
            &mut self.walls,
            &mut self.dimensions,
            &mut self.grids,
            &mut self.everywhere,
            &mut self.levels,
            &mut self.phased,
            &mut self.other,
        ] {
            if set.contains(&id) {
                Arc::make_mut(set).remove(&id);
            }
        }
        if self.tiled.contains_key(&id)
            && let Some(Some([x0, y0, x1, y1, b0, b1])) = Arc::make_mut(&mut self.tiled).remove(&id)
        {
            for b in b0..=b1 {
                if let Some(set) = self.bands.get_mut(&b) {
                    Arc::make_mut(set).remove(&id);
                    if set.is_empty() {
                        self.bands.remove(&b);
                    }
                }
            }
            for x in x0..=x1 {
                for y in y0..=y1 {
                    let Some(bands) = self.tiles.get_mut(&(x, y)) else { continue };
                    let bands_now = Arc::make_mut(bands);
                    for b in b0..=b1 {
                        if let Some(set) = bands_now.get_mut(&b) {
                            Arc::make_mut(set).remove(&id);
                            if set.is_empty() {
                                bands_now.remove(&b);
                            }
                        }
                    }
                    if bands_now.is_empty() {
                        self.tiles.remove(&(x, y));
                    }
                }
            }
        }
        let Some(chunk) = self.refs.get_mut(&(id >> SHIFT)) else { return };
        if !chunk.contains_key(&id) {
            return;
        }
        let refs = Arc::make_mut(chunk).remove(&id).unwrap_or_default();
        if chunk.is_empty() {
            self.refs.remove(&(id >> SHIFT));
        }
        for g in refs {
            if let Some(b) = self.referrers.get_mut(&bucket(&g)) {
                let b = Arc::make_mut(b);
                if let Some(set) = b.get_mut(&g) {
                    set.remove(&id);
                    if set.is_empty() {
                        b.remove(&g);
                    }
                }
                if b.is_empty() {
                    self.referrers.remove(&bucket(&g));
                }
            }
        }
    }

    /// The elements that point at `g`, by id.
    pub fn referrers(&self, g: &Guid) -> impl Iterator<Item = Id> + '_ {
        self.referrers.get(&bucket(g)).and_then(|b| b.get(g)).into_iter().flat_map(|s| s.iter().copied())
    }

    /// The walls, by id.
    pub fn walls(&self) -> impl Iterator<Item = Id> + '_ {
        self.walls.iter().copied()
    }

    /// The walls whose location lines, grown by `grow` mm, may reach into the box `[x0, y0, x1, y1]`, by id: every one
    /// that does, and others near. Where the box is too big for tiles, every wall.
    pub fn walls_near(&self, b: [f64; 4], grow: f64) -> BTreeSet<Id> {
        self.walls_near_any(&[b], grow)
    }

    /// [`Lookup::walls_near`] any of `boxes`: each tile's walls taken once.
    pub fn walls_near_any(&self, boxes: &[[f64; 4]], grow: f64) -> BTreeSet<Id> {
        let at: Vec<_> = boxes.iter().map(|b| (*b, f64::NEG_INFINITY, f64::INFINITY)).collect();
        self.walls_near_at(&at, grow)
    }

    /// [`Lookup::walls_near_any`] of boxes `(plan box, lo, hi)` that span heights `lo` to `hi`: the walls near one of them in
    /// plan whose heights may overlap its own. Every wall [`crate::walls::stacked`] with one and near it is among them.
    pub fn walls_near_at(&self, boxes: &[([f64; 4], f64, f64)], grow: f64) -> BTreeSet<Id> {
        // Each tile, with the bands asked of it (none: all of them).
        let mut tiles: BTreeMap<(i64, i64), Option<BTreeSet<i64>>> = BTreeMap::new();
        for (b, lo, hi) in boxes {
            let Some([x0, y0, x1, y1]) = tiles_of([b[0] - grow, b[1] - grow, b[2] + grow, b[3] + grow]) else {
                return self.walls.iter().copied().collect();
            };
            let [b0, b1] = bands_of(*lo, *hi, 1);
            for x in x0..=x1 {
                for y in y0..=y1 {
                    let bands = tiles.entry((x, y)).or_insert_with(|| Some(BTreeSet::new()));
                    match bands {
                        Some(set) if b0 != ANY => set.extend(b0..=b1),
                        _ => *bands = None,
                    }
                }
            }
        }
        let mut out: BTreeSet<Id> = self.everywhere.iter().copied().collect();
        for (t, asked) in tiles {
            let Some(bands) = self.tiles.get(&t) else { continue };
            match asked {
                Some(asked) => {
                    for b in std::iter::once(ANY).chain(asked) {
                        if let Some(set) = bands.get(&b) {
                            out.extend(set.iter().copied());
                        }
                    }
                }
                None => bands.values().for_each(|set| out.extend(set.iter().copied())),
            }
        }
        out
    }

    /// The walls whose heights may reach height `z`, wherever they are, by id: every wall that spans it, and others.
    pub fn walls_at(&self, z: f64) -> BTreeSet<Id> {
        let [b, _] = bands_of(z, z, 1);
        let mut out: BTreeSet<Id> = self.everywhere.iter().copied().collect();
        if b == ANY {
            out.extend(self.walls.iter().copied());
            return out;
        }
        for band in [ANY, b] {
            if let Some(set) = self.bands.get(&band) {
                out.extend(set.iter().copied());
            }
        }
        out
    }

    /// The dimensions, by id.
    pub fn dimensions(&self) -> impl Iterator<Item = Id> + '_ {
        self.dimensions.iter().copied()
    }

    /// The elements of category `cat`, one of [`RARE`], by id; none of another.
    pub fn rare(&self, cat: &str) -> impl Iterator<Item = Id> + '_ {
        self.rare.get(cat).into_iter().flat_map(|s| s.iter().copied())
    }

    /// Whether an element is phased or in a design option.
    pub fn any_phased(&self) -> bool {
        !self.phased.is_empty()
    }

    /// The groups of elements banded by height ([`Group`]), each with one of its elements.
    pub fn groups(&self) -> impl Iterator<Item = (&Group, Id)> + '_ {
        self.heights.iter().filter_map(|(g, bands)| Some((g.as_ref(), *bands.values().next()?.iter().next()?)))
    }

    /// The elements of group `g` whose own heights may reach from `lo` to `hi` (mm, either way up), by id: every one
    /// that does, and others near. Not finite, every one of the group.
    pub fn group_between(&self, g: &Group, lo: f64, hi: f64) -> BTreeSet<Id> {
        let Some(bands) = self.heights.get(g) else { return BTreeSet::new() };
        let mut out = BTreeSet::new();
        let [b0, b1] = bands_of(lo, hi, 1);
        if b0 == ANY {
            bands.values().for_each(|set| out.extend(set.iter().copied()));
            return out;
        }
        for set in std::iter::once(bands.get(&ANY)).chain(bands.range(b0 - 1..=b1 + 1).map(|(_, s)| Some(s))).flatten() {
            out.extend(set.iter().copied());
        }
        out
    }

    /// The walls whose heights may reach from `lo` to `hi` (mm), wherever they are, by id: every one that does, and
    /// others near.
    pub fn walls_between(&self, lo: f64, hi: f64) -> BTreeSet<Id> {
        let mut out: BTreeSet<Id> = self.everywhere.iter().copied().collect();
        let [b0, b1] = bands_of(lo, hi, 1);
        if b0 == ANY {
            out.extend(self.walls.iter().copied());
            return out;
        }
        for set in std::iter::once(self.bands.get(&ANY)).chain(self.bands.range(b0 - 1..=b1 + 1).map(|(_, s)| Some(s))).flatten() {
            out.extend(set.iter().copied());
        }
        out
    }

    /// The elements a plan looks for among all of them ([`Lookup::other`]), by id.
    pub fn other(&self) -> impl Iterator<Item = Id> + '_ {
        self.other.iter().copied()
    }

    /// The levels, by id.
    pub fn levels(&self) -> impl Iterator<Item = Id> + '_ {
        self.levels.iter().copied()
    }

    /// The grids, by id.
    pub fn grids(&self) -> impl Iterator<Item = Id> + '_ {
        self.grids.iter().copied()
    }

    /// Whether it shares chunks with `other`: whether one was brought up to date from the other.
    #[cfg(test)]
    fn shares(&self, other: &Lookup) -> bool {
        self.refs.iter().any(|(k, c)| other.refs.get(k).is_some_and(|d| Arc::ptr_eq(c, d)))
    }
}

/// A model's [`Lookup`], once asked for, and the one of the model it was copied from.
#[derive(Default)]
pub struct Derived {
    /// The lookup, with the elements' token and version it is of.
    now: Mutex<Option<(u64, u64, Arc<Lookup>)>>,
    /// The lookup of the model this was copied from, with the token and version of the elements it is of.
    parent: Option<(u64, u64, Arc<Lookup>)>,
    /// What a slab asks of each shaft, with the token and version of the elements it is of.
    shafts: Mutex<Option<KeptShafts>>,
}

type KeptShafts = (u64, u64, Arc<Vec<ShaftReach>>);

/// A shaft as a slab asks of it: its span, its plan box [x0, y0, x1, y1] and its outline.
#[derive(Debug)]
pub struct ShaftReach {
    pub id: Id,
    pub bottom: f64,
    pub top: f64,
    pub plan: [f64; 4],
    pub boundary: Vec<crate::geom::P2>,
}

impl Derived {
    fn current(&self) -> Option<(u64, u64, Arc<Lookup>)> {
        self.now.lock().ok().and_then(|now| now.clone())
    }

    /// What `make` finds of the shafts of `elements`, kept until they change.
    pub fn shafts(&self, elements: &Elements, make: impl FnOnce() -> Vec<ShaftReach>) -> Arc<Vec<ShaftReach>> {
        let (token, version) = elements.state();
        if let Ok(now) = self.shafts.lock()
            && let Some((t, v, table)) = now.as_ref()
            && (*t, *v) == (token, version)
        {
            return Arc::clone(table);
        }
        let made = Arc::new(make());
        if let Ok(mut now) = self.shafts.lock() {
            *now = Some((token, version, Arc::clone(&made)));
        }
        made
    }

    /// The lookup of `elements` if it is made already: what asks only where it costs nothing.
    pub fn made(&self, elements: &Elements) -> Option<Arc<Lookup>> {
        let state = elements.state();
        self.current().filter(|(t, v, _)| (*t, *v) == state).map(|(_, _, l)| l)
    }

    /// The lookup of `elements`: kept, brought up to date from the parent's, or made.
    pub fn lookup(&self, elements: &Elements) -> Arc<Lookup> {
        let (token, version) = elements.state();
        if let Some((t, v, l)) = self.current()
            && (t, v) == (token, version)
        {
            return l;
        }
        let made = match (&self.parent, elements.touched()) {
            (Some((t, v, parent)), Some(ids)) if (*t, *v) == elements.base() => parent.updated(elements, &ids),
            _ => Lookup::of(elements),
        };
        let made = Arc::new(made);
        if let Ok(mut now) = self.now.lock() {
            *now = Some((token, version, Arc::clone(&made)));
        }
        made
    }
}

impl Clone for Derived {
    /// For the model's copy: what it starts from, this one's lookup. It is the copy's to bring up to date, if its elements
    /// were cloned from the elements this lookup is of, as they were then.
    fn clone(&self) -> Derived {
        Derived { now: Mutex::new(None), parent: self.current(), shafts: Mutex::new(None) }
    }
}

impl PartialEq for Derived {
    /// What a model derives is no part of what it is.
    fn eq(&self, _: &Derived) -> bool {
        true
    }
}

impl fmt::Debug for Derived {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Derived")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use crate::sample::tower::{Plate, tower_on};
    use serde_json::json;

    #[test]
    fn removing_the_last_reference_or_rare_kind_removes_empty_lookup_buckets() {
        for (command, params) in
            [("text", json!({"view":100000,"at":[0,0],"text":"Only reference"})), ("section", json!({"start":[0,0],"end":[1000,0]}))]
        {
            let mut s = Session::new();
            let id = s.execute(command, &params).unwrap()["id"].as_u64().unwrap();
            let before = Lookup::of(&s.model.elements);
            let mut after = s.model.elements.clone();
            after.remove(id);
            let updated = before.updated(&after, &BTreeSet::from([id]));
            assert_eq!(updated, Lookup::of(&after), "remove only {command}");
            assert!(updated.referrers.values().all(|bucket| !bucket.is_empty()));
            assert!(updated.rare.values().all(|ids| !ids.is_empty()));
        }
    }

    #[test]
    fn a_lookup_brought_up_to_date_is_the_one_made_again() {
        let mut s = Session::new();
        s.reset(tower_on(Plate::SMALL, 1).unwrap(), None);
        let first = s.model.lookup();
        assert_eq!(*first, Lookup::of(&s.model.elements));
        let wall = first.walls().nth(5).unwrap();
        let door = s.model.elements.iter().find(|(_, e)| matches!(e, Element::Door(_))).map(|(id, _)| *id).unwrap();
        for (cmd, p) in [
            ("move", json!({"ids": [wall], "by": [100, 0]})),
            ("copy", json!({"ids": [wall], "by": [0, 50]})),
            ("delete", json!({"ids": [door]})),
            ("wall", json!({"points": [[1000, 1000], [2000, 1000]]})),
            ("undo", json!({})),
        ] {
            let was = s.model.lookup();
            s.execute(cmd, &p).unwrap();
            let kept = s.model.lookup();
            assert_eq!(*kept, Lookup::of(&s.model.elements), "after {cmd}");
            // Deletion, like the other local edits, shares the untouched lookup columns.
            assert!(kept.shares(&was), "after {cmd}");
        }
        // A level that moves moves its walls' heights: the lookup is made again.
        let level = s.model.levels()[0].0;
        let m = s.model_mut();
        if let Some(Element::Level(l)) = m.elements.get_mut(level) {
            l.elevation += 2500.0;
        }
        assert_eq!(*s.model.lookup(), Lookup::of(&s.model.elements), "after a level moved");
        // What points at a wall: its doors, windows and tags.
        let guid = s.model.elements.guid(wall).unwrap();
        let referrers: Vec<&str> =
            s.model.lookup().referrers(&guid).filter_map(|id| s.model.elements.get(id)).map(Element::category).collect();
        assert!(referrers.iter().all(|c| ["door", "window", "tag", "dimension", "room"].contains(c)), "{referrers:?}");
    }

    #[test]
    fn a_wall_raised_past_its_top_level_by_its_top_offset_is_near_at_the_heights_it_reaches() {
        let mut s = Session::new();
        let level = s.model.levels()[0].0;
        // Its top 3 m over its top level, the level it stands on: it reaches from 0 to 3000.
        let made = s.execute("wall", &json!({"points": [[0, 0], [5000, 0]], "top_level": level, "top_offset": 3000})).unwrap();
        let wall = made["ids"][0].as_u64().or_else(|| made["id"].as_u64()).unwrap();
        let w = s.model.wall(wall).unwrap();
        assert_eq!((s.model.base_z(w), s.model.top_z(w)), (0.0, 3000.0));
        let near = s.model.lookup().walls_near_at(&[(axis_box(w), 2500.0, 2600.0)], 0.0);
        assert!(near.contains(&wall), "a wall 3 m tall isn't near at 2.5 m");
        assert_eq!(*s.model.lookup(), Lookup::of(&s.model.elements));
    }

    #[test]
    fn the_walls_near_a_wall_are_every_wall_its_ends_lie_on_and_that_end_on_it() {
        let m = tower_on(Plate::SMALL, 2).unwrap();
        let l = m.lookup();
        let walls: Vec<(Id, &crate::model::Wall)> = l.walls().filter_map(|id| Some((id, m.wall(id)?))).collect();
        let on = |p: crate::geom::P2, w: &crate::model::Wall| crate::walls::Axis::of(w).distance(p, 0.0) <= crate::model::TOL;
        let mut pairs = 0;
        for (a, wa) in &walls {
            let near = l.walls_near(axis_box(wa), 2.0 * crate::model::TOL);
            for (b, wb) in &walls {
                if a != b && (on(wb.start, wa) || on(wb.end, wa) || on(wa.start, wb) || on(wa.end, wb)) {
                    assert!(near.contains(b), "wall {b} meets wall {a} but isn't near it");
                    pairs += 1;
                }
            }
            // Near, not everything: a wall's tiles are its neighbourhood.
            assert!(near.len() < walls.len() / 2, "wall {a}: {} near of {}", near.len(), walls.len());
        }
        assert!(pairs > 500, "{pairs}");
    }

    #[test]
    fn the_walls_near_a_wall_at_its_heights_are_every_wall_it_meets_and_is_stacked_with() {
        let mut m = tower_on(Plate::SMALL, 4).unwrap();
        // One wall attached at its top: the lookup doesn't know how high it goes, so it is near at any height.
        let floor = m.elements.iter().find(|(_, e)| matches!(e, Element::Floor(_))).map(|(id, _)| m.guid(*id)).unwrap();
        let attached = m.lookup().walls().next().unwrap();
        if let Some(Element::Wall(w)) = m.elements.get_mut(attached) {
            w.attach_top = Some(floor);
        }
        let l = m.lookup();
        let walls: Vec<(Id, &crate::model::Wall)> = l.walls().filter_map(|id| Some((id, m.wall(id)?))).collect();
        let on = |p: crate::geom::P2, w: &crate::model::Wall| crate::walls::Axis::of(w).distance(p, 0.0) <= crate::model::TOL;
        let mut pairs = 0;
        for (a, wa) in &walls {
            let at = (axis_box(wa), m.base_z(wa), m.top_z(wa));
            let near = l.walls_near_at(&[at], 2.0 * crate::model::TOL);
            for (b, wb) in &walls {
                let meet = on(wb.start, wa) || on(wb.end, wa) || on(wa.start, wb) || on(wa.end, wb);
                if a != b && meet && crate::walls::stacked(&m, wa, wb) {
                    assert!(near.contains(b), "wall {b} meets wall {a} and is stacked with it, but isn't near it");
                    pairs += 1;
                }
            }
            // Near in height too: of the walls near in plan, those of the storeys about it.
            let plan = l.walls_near(axis_box(wa), 2.0 * crate::model::TOL);
            assert!(near.is_subset(&plan));
            assert!(near.len() * 3 < plan.len() * 2, "wall {a}: {} near of {} in plan", near.len(), plan.len());
        }
        assert!(pairs > 500, "{pairs}");
        // The attached wall, at any height in its tiles.
        let wa = m.wall(attached).unwrap();
        assert!(l.walls_near_at(&[(axis_box(wa), 1e6, 1e6 + 1.0)], 0.0).contains(&attached));
        assert!(!l.walls_near_at(&[(axis_box(wa), 1e6, 1e6 + 1.0)], 0.0).contains(&walls[1].0) || walls[1].0 == attached);
    }
}
