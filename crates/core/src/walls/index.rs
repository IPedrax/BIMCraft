//! Read-only acceleration for one model snapshot. Plan tiles and heights never decide geometry: the existing overlap
//! and join rules still test every candidate, in the same order (by id). A wall's candidates at one of its ends are the
//! walls whose bodies reach that end's tile and overlap it in height, not every wall of the project; a run's, the runs of
//! any category whose bodies reach the node's tile (its body grown by its half width, so a run ending on another's middle
//! is found there too) and whose heights overlap as [`crate::walls::stacked`] says for two runs.
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::geom::P2;
use crate::model::{Element, Guid, Id, Model, TOL};

const MAX_REFERENCES: usize = 1_000_000;
/// Plan tiles, mm on a side, and the most tiles one wall (or run) goes in (a longer one is a candidate everywhere).
const TILE: f64 = 6000.0;
const MAX_TILES: i64 = 4096;

/// A tile's elements by how low they begin, with their heights, and the tallest's height: of a tower's, what a wall can
/// reach is its own storeys, not the 100 stacked in the tile.
#[derive(Clone, Debug, Default)]
struct Tile {
    /// Each one's bottom, id and height, by bottom and then id.
    items: Vec<(f64, Id, f64)>,
    tallest: f64,
}

impl Tile {
    /// Those that may overlap `lo` to `hi`: more, near the ends, as the caller still asks each. Numbers that aren't finite
    /// leave the search wider, never narrower.
    fn may_overlap(&self, lo: f64, hi: f64) -> impl Iterator<Item = Id> + '_ {
        // A wall's top is its bottom and its height, rounded: a margin well past what the rounding moves.
        let floor = lo - self.tallest - 1.0 - 1e-6 * lo.abs();
        let start = self.items.partition_point(|(bottom, ..)| *bottom < floor);
        let end = self.items.partition_point(|(bottom, ..)| bottom.total_cmp(&hi).is_lt());
        self.items.get(start..end.max(start)).unwrap_or_default().iter().map(|(_, id, _)| *id)
    }

    fn insert(&mut self, bottom: f64, id: Id, height: f64) {
        let at = self.items.partition_point(|(b, i, _)| b.total_cmp(&bottom).then(i.cmp(&id)).is_lt());
        self.items.insert(at, (bottom, id, height));
        self.tallest = self.tallest.max(height);
    }

    fn remove(&mut self, id: Id) {
        self.items.retain(|(_, i, _)| *i != id);
        self.tallest = self.items.iter().fold(0.0, |t, (_, _, h)| t.max(*h));
    }

    /// The same as `other`, numbers to the bit.
    #[cfg(test)]
    fn same(&self, other: &Tile) -> bool {
        self.tallest.to_bits() == other.tallest.to_bits()
            && self.items.len() == other.items.len()
            && self
                .items
                .iter()
                .zip(&other.items)
                .all(|(a, b)| a.0.to_bits() == b.0.to_bits() && a.1 == b.1 && a.2.to_bits() == b.2.to_bits())
    }
}

/// What one element puts in the index: a wall's or a run's heights and the tiles it reaches (none: it is a candidate
/// everywhere), or the wall a door or a window is in.
#[derive(Clone, Copy, Debug)]
enum Placed {
    Wall { span: (f64, f64), tiles: Option<[i64; 4]> },
    Run { span: (f64, f64), tiles: Option<[i64; 4]> },
    Opening(Guid),
}

#[cfg(test)]
impl Placed {
    fn same(&self, other: &Placed) -> bool {
        let span = |a: (f64, f64), b: (f64, f64)| a.0.to_bits() == b.0.to_bits() && a.1.to_bits() == b.1.to_bits();
        match (self, other) {
            (Placed::Wall { span: a, tiles: s }, Placed::Wall { span: b, tiles: t }) => span(*a, *b) && s == t,
            (Placed::Run { span: a, tiles: s }, Placed::Run { span: b, tiles: t }) => span(*a, *b) && s == t,
            (Placed::Opening(a), Placed::Opening(b)) => a == b,
            _ => false,
        }
    }
}

/// Ids by chunk: what an edit copies of the index is the chunks it touched.
const SHIFT: u32 = 10;

/// The walls and the runs by plan tile and height, and the doors and windows by the wall they are in. Kept with a model
/// ([`Model::wall_index`]) and brought up to date by what a command changed ([`Index::updated`]): its parts are shared
/// behind `Arc`s with the index it came from, so an edit copies what it touched.
#[derive(Clone, Debug, Default)]
pub(crate) struct Index {
    /// What each wall, run and opening put in it, by chunk of ids.
    placed: BTreeMap<Id, Arc<BTreeMap<Id, Placed>>>,
    /// Each wall's doors and windows, by id.
    hosts: BTreeMap<Guid, Arc<Vec<Id>>>,
    /// Each tile's walls: those whose body, grown by a tolerance, reaches into it.
    tiles: HashMap<(i64, i64), Arc<Tile>>,
    /// Walls in no tile: too long for them, or of no finite size, by id.
    everywhere: Arc<Vec<Id>>,
    /// The runs' own tiles and the runs in no tile, kept apart from the walls' so that a wall query never meets a run and a
    /// run query never meets a wall.
    run_tiles: HashMap<(i64, i64), Arc<Tile>>,
    run_everywhere: Arc<Vec<Id>>,
    /// How many tile entries it holds: past [`MAX_REFERENCES`], what comes in goes everywhere.
    tiled: usize,
    /// The walls attached to what is over or under them, whose heights that decides: brought up to date with every edit.
    attached: Arc<std::collections::BTreeSet<Id>>,
    /// The levels it was made with: the heights of everything in it are of them.
    levels: Arc<Vec<(Id, u64)>>,
}

/// The model's levels, by id, and their elevations' bits.
fn levels_of(m: &Model) -> Vec<(Id, u64)> {
    m.levels().iter().map(|(id, l)| (*id, l.elevation.to_bits())).collect()
}

impl Index {
    pub(crate) fn new(m: &Model) -> Self {
        let mut index = Index { levels: Arc::new(levels_of(m)), ..Index::default() };
        for (&id, e) in &m.elements {
            index.put(m, id, e);
        }
        index
    }

    /// This index, of the model a command changed into `m`, brought up to date where `ids` changed: the same index as
    /// [`Index::new`] makes of `m`. Made again where a level changed (every height is of one) or where it holds as many
    /// tile entries as it may (where they would have gone depends on the order they came in).
    pub(crate) fn updated(&self, m: &Model, ids: &std::collections::BTreeSet<Id>) -> Index {
        if *self.levels != levels_of(m) || ids.iter().any(|id| matches!(m.elements.get(*id), Some(Element::Level(_)))) {
            return Index::new(m);
        }
        let mut index = self.clone();
        let mut ids = ids.clone();
        // A wall attached to a roof, a floor or another wall has the heights they give it: any edit may change them.
        ids.extend(index.attached.iter().copied());
        for id in ids {
            index.take(id);
            if let Some(e) = m.elements.get(id) {
                index.put(m, id, e);
            }
        }
        if index.tiled >= MAX_REFERENCES { Index::new(m) } else { index }
    }

    /// The same as `other`, numbers to the bit: what [`Index::updated`] is held to in tests.
    #[cfg(test)]
    pub(crate) fn same(&self, other: &Index) -> bool {
        let placed = |a: &BTreeMap<Id, Arc<BTreeMap<Id, Placed>>>| -> Vec<(Id, Placed)> {
            a.values().flat_map(|c| c.iter().map(|(id, p)| (*id, *p))).collect()
        };
        let tiles = |a: &HashMap<(i64, i64), Arc<Tile>>, b: &HashMap<(i64, i64), Arc<Tile>>| {
            a.len() == b.len() && a.iter().all(|(k, t)| b.get(k).is_some_and(|u| t.same(u)))
        };
        let (a, b) = (placed(&self.placed), placed(&other.placed));
        a.len() == b.len()
            && a.iter().zip(&b).all(|((i, p), (j, q))| i == j && p.same(q))
            && self.hosts.len() == other.hosts.len()
            && self.hosts.iter().all(|(g, v)| other.hosts.get(g).is_some_and(|w| v == w))
            && tiles(&self.tiles, &other.tiles)
            && tiles(&self.run_tiles, &other.run_tiles)
            && self.everywhere == other.everywhere
            && self.run_everywhere == other.run_everywhere
            && self.tiled == other.tiled
            && self.attached == other.attached
            && self.levels == other.levels
    }

    fn placed(&self, id: Id) -> Option<&Placed> {
        self.placed.get(&(id >> SHIFT))?.get(&id)
    }

    fn span(&self, id: Id) -> Option<(f64, f64)> {
        match self.placed(id)? {
            Placed::Wall { span, .. } => Some(*span),
            _ => None,
        }
    }

    fn run_span(&self, id: Id) -> Option<(f64, f64)> {
        match self.placed(id)? {
            Placed::Run { span, .. } => Some(*span),
            _ => None,
        }
    }

    /// Every wall (`runs` false) or every run, by id.
    fn all(&self, runs: bool) -> Vec<Id> {
        self.placed
            .values()
            .flat_map(|c| c.iter())
            .filter(|(_, p)| matches!((p, runs), (Placed::Wall { .. }, false) | (Placed::Run { .. }, true)))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Element `id`, `e` of `m`, put where it goes.
    fn put(&mut self, m: &Model, id: Id, e: &Element) {
        let placed = match e {
            Element::Wall(w) => {
                let (lo, hi) = (m.base_z(w), m.top_z(w));
                let tiles = reach(m, w).map(|[x0, y0, x1, y1]| [tile(x0), tile(y0), tile(x1), tile(y1)]);
                let tiles = self.tile_in(tiles, false, id, lo, hi);
                if w.attach_base.is_some() || w.attach_top.is_some() {
                    Arc::make_mut(&mut self.attached).insert(id);
                }
                Placed::Wall { span: (lo, hi), tiles }
            }
            crate::model::run_arm!(_) => {
                let Some(g) = m.run_geom(e) else { return };
                let (lo, hi) = g.z_span();
                // Its body's box, grown by its half width (and twice the tolerance, as a wall's is), so that a run
                // ending on its middle reaches that tile.
                let grow = g.width / 2.0 + 2.0 * TOL;
                let b = [g.a[0].min(g.b[0]) - grow, g.a[1].min(g.b[1]) - grow, g.a[0].max(g.b[0]) + grow, g.a[1].max(g.b[1]) + grow];
                let tiles = b.iter().all(|v| v.is_finite() && v.abs() < 1e12).then(|| [tile(b[0]), tile(b[1]), tile(b[2]), tile(b[3])]);
                let tiles = self.tile_in(tiles, true, id, lo, hi);
                Placed::Run { span: (lo, hi), tiles }
            }
            Element::Door(o) | Element::Window(o) => {
                let ids = Arc::make_mut(self.hosts.entry(o.host).or_default());
                let at = ids.partition_point(|i| *i < id);
                ids.insert(at, id);
                Placed::Opening(o.host)
            }
            _ => return,
        };
        Arc::make_mut(self.placed.entry(id >> SHIFT).or_default()).insert(id, placed);
    }

    /// A wall's (or with `runs`, a run's) entries in `tiles`, `[x0, y0, x1, y1]`, from `lo` to `hi`: the tiles it is in, or
    /// none, everywhere, where they are too many or the index holds as many entries as it may.
    fn tile_in(&mut self, tiles: Option<[i64; 4]>, runs: bool, id: Id, lo: f64, hi: f64) -> Option<[i64; 4]> {
        let tiles = tiles.filter(|[a, b, c, d]| (c - a + 1).saturating_mul(d - b + 1) <= MAX_TILES && self.tiled < MAX_REFERENCES);
        let (map, everywhere) =
            if runs { (&mut self.run_tiles, &mut self.run_everywhere) } else { (&mut self.tiles, &mut self.everywhere) };
        match tiles {
            Some([a, b, c, d]) => {
                for x in a..=c {
                    for y in b..=d {
                        Arc::make_mut(map.entry((x, y)).or_default()).insert(lo, id, hi - lo);
                        self.tiled += 1;
                    }
                }
            }
            None => {
                let all = Arc::make_mut(everywhere);
                let at = all.partition_point(|i| *i < id);
                all.insert(at, id);
            }
        }
        tiles
    }

    /// Element `id` taken out of where it was put.
    fn take(&mut self, id: Id) {
        let Some(chunk) = self.placed.get_mut(&(id >> SHIFT)) else { return };
        let Some(placed) = Arc::make_mut(chunk).remove(&id) else { return };
        if chunk.is_empty() {
            self.placed.remove(&(id >> SHIFT));
        }
        if self.attached.contains(&id) {
            Arc::make_mut(&mut self.attached).remove(&id);
        }
        let (tiles, runs) = match placed {
            Placed::Wall { tiles, .. } => (tiles, false),
            Placed::Run { tiles, .. } => (tiles, true),
            Placed::Opening(host) => {
                if let Some(ids) = self.hosts.get_mut(&host) {
                    Arc::make_mut(ids).retain(|i| *i != id);
                    if ids.is_empty() {
                        self.hosts.remove(&host);
                    }
                }
                return;
            }
        };
        let (map, everywhere) =
            if runs { (&mut self.run_tiles, &mut self.run_everywhere) } else { (&mut self.tiles, &mut self.everywhere) };
        match tiles {
            Some([a, b, c, d]) => {
                for x in a..=c {
                    for y in b..=d {
                        if let Some(t) = map.get_mut(&(x, y)) {
                            Arc::make_mut(t).remove(id);
                            self.tiled -= 1;
                            if t.items.is_empty() {
                                map.remove(&(x, y));
                            }
                        }
                    }
                }
            }
            None => Arc::make_mut(everywhere).retain(|i| *i != id),
        }
    }

    /// The walls that may meet wall `id` at `node`, one of its ends: those that overlap it in height and whose bodies reach
    /// the node's tile, by id. Every wall that ends there or that the node lies in is among them.
    pub(crate) fn neighbours_at(&self, id: Id, node: P2) -> Vec<Id> {
        let Some((lo, hi)) = self.span(id) else { return Vec::new() };
        let mut ids = (*self.everywhere).clone();
        if node.0.is_finite() && node.1.is_finite() {
            if let Some(tile) = self.tiles.get(&(tile(node.0), tile(node.1))) {
                ids.extend(tile.may_overlap(lo, hi));
            }
        } else {
            ids = self.all(false);
        }
        ids.sort_unstable();
        ids.dedup();
        ids.retain(|id| self.span(*id).is_some_and(|(bottom, top)| lo < top && bottom < hi));
        ids
    }

    pub(crate) fn openings(&self, host: Guid) -> &[Id] {
        self.hosts.get(&host).map_or(&[], |ids| ids.as_slice())
    }

    /// The runs that may meet at `node` (a point in plan) at heights `lo` to `hi`: those whose bodies reach the node's
    /// tile and whose heights overlap `lo` to `hi` the way two runs' must ([`crate::walls::stacked`]), by id. Every run
    /// ending there or with the node in its body is among them.
    pub(crate) fn runs_near(&self, node: P2, lo: f64, hi: f64) -> Vec<Id> {
        let mut ids = (*self.run_everywhere).clone();
        if node.0.is_finite() && node.1.is_finite() {
            if let Some(tile) = self.run_tiles.get(&(tile(node.0), tile(node.1))) {
                ids.extend(tile.may_overlap(lo, hi));
            }
        } else {
            ids = self.all(true);
        }
        ids.sort_unstable();
        ids.dedup();
        ids.retain(|id| self.run_span(*id).is_some_and(|(bottom, top)| lo < top && bottom < hi));
        ids
    }

    /// The runs whose bodies reach the box `[x0, y0, x1, y1]`, grown by `grow` mm, at heights `lo` to `hi`, by
    /// id: what a run staying put asks of the runs a command took, to stretch to the joins they had. A move asks
    /// [`RunsNear::runs_near_box`], which tests hold to this.
    #[cfg(test)]
    pub(crate) fn runs_near_box(&self, b: [f64; 4], grow: f64, lo: f64, hi: f64) -> Vec<Id> {
        let grown = [b[0] - grow, b[1] - grow, b[2] + grow, b[3] + grow];
        let tiled = (grown.iter().all(|v| v.is_finite() && v.abs() < 1e12)).then(|| {
            let t = [tile(grown[0]), tile(grown[1]), tile(grown[2]), tile(grown[3])];
            ((t[0], t[1]), (t[2], t[3]))
        });
        let mut ids: Vec<Id> = Vec::new();
        match tiled {
            Some(((a, b), (c, d))) if (c - a + 1).saturating_mul(d - b + 1) <= MAX_TILES => {
                ids.extend(self.run_everywhere.iter().copied());
                for x in a..=c {
                    for y in b..=d {
                        if let Some(tile) = self.run_tiles.get(&(x, y)) {
                            ids.extend(tile.may_overlap(lo, hi));
                        }
                    }
                }
            }
            _ => ids.extend(self.all(true)),
        }
        ids.sort_unstable();
        ids.dedup();
        ids.retain(|id| self.run_span(*id).is_some_and(|(bottom, top)| lo < top && bottom < hi));
        ids
    }
}

/// The runs' half of [`Index`], answered from the model's lookup for a few queries rather than made of every element: what
/// a command moving one wall or one run asks. The same runs as [`Index::runs_near`] and [`Index::runs_near_box`] give, by
/// id: those of the heights asked found among the lookup's runs banded near them (a run's section reaches at most twice
/// its depth past its line's heights, as it is banded no steeper than [`crate::derived`]'s limit) and the few it doesn't
/// band, then held to the index's own test of plan tiles and heights.
pub(crate) struct RunsNear<'a> {
    m: &'a Model,
    lookup: std::sync::Arc<crate::derived::Lookup>,
    /// How far past its line's heights a run's section may reach: twice the deepest (or widest round) run type's, and a
    /// millimetre. Not finite, every run is a candidate.
    margin: f64,
}

/// A run's heights and the tiles its body reaches ([`Index::new`]'s), or none for a run in no tile.
type Reach = ((f64, f64), Option<((i64, i64), (i64, i64))>);

impl<'a> RunsNear<'a> {
    pub(crate) fn new(m: &'a Model) -> Self {
        let deepest = m.types.runs.values().flat_map(|t| t.values()).map(|t| t.depth.abs().max(t.width.abs())).fold(0.0, f64::max);
        RunsNear { m, lookup: m.lookup(), margin: 2.0 * deepest + 1.0 }
    }

    /// The runs whose heights may overlap `lo` to `hi`, with their heights and tiles: every run that does, among others.
    fn candidates(&self, lo: f64, hi: f64) -> Vec<(Id, Reach)> {
        let (lo, hi) = (lo - self.margin, hi + self.margin);
        let mut ids: std::collections::BTreeSet<Id> = self.lookup.other().collect();
        for (g, _) in self.lookup.groups() {
            if let crate::derived::Group::Run { .. } = g {
                ids.extend(self.lookup.group_between(g, lo, hi));
            }
        }
        ids.into_iter().filter_map(|id| Some((id, self.reach(self.m.elements.get(id)?)?))).collect()
    }

    /// Run `e`'s heights and tiles, as [`Index::new`] finds them; none for another element or a run of no type.
    fn reach(&self, e: &Element) -> Option<Reach> {
        e.run()?;
        let g = self.m.run_geom(e)?;
        let grow = g.width / 2.0 + 2.0 * TOL;
        let b = [g.a[0].min(g.b[0]) - grow, g.a[1].min(g.b[1]) - grow, g.a[0].max(g.b[0]) + grow, g.a[1].max(g.b[1]) + grow];
        let tiles = b
            .iter()
            .all(|v| v.is_finite() && v.abs() < 1e12)
            .then(|| ((tile(b[0]), tile(b[1])), (tile(b[2]), tile(b[3]))))
            .filter(|((a, b), (c, d))| (c - a + 1).saturating_mul(d - b + 1) <= MAX_TILES);
        Some((g.z_span(), tiles))
    }

    /// [`Index::runs_near`].
    pub(crate) fn runs_near(&self, node: P2, lo: f64, hi: f64) -> Vec<Id> {
        let at = (node.0.is_finite() && node.1.is_finite()).then(|| (tile(node.0), tile(node.1)));
        self.candidates(lo, hi)
            .into_iter()
            .filter(|(_, ((bottom, top), tiles))| {
                lo < *top
                    && *bottom < hi
                    && match (at, tiles) {
                        (Some((x, y)), Some(((a, b), (c, d)))) => (*a..=*c).contains(&x) && (*b..=*d).contains(&y),
                        _ => true,
                    }
            })
            .map(|(id, _)| id)
            .collect()
    }

    /// [`Index::runs_near_box`].
    pub(crate) fn runs_near_box(&self, b: [f64; 4], grow: f64, lo: f64, hi: f64) -> Vec<Id> {
        let grown = [b[0] - grow, b[1] - grow, b[2] + grow, b[3] + grow];
        let asked = (grown.iter().all(|v| v.is_finite() && v.abs() < 1e12))
            .then(|| ((tile(grown[0]), tile(grown[1])), (tile(grown[2]), tile(grown[3]))))
            .filter(|((a, b), (c, d))| (c - a + 1).saturating_mul(d - b + 1) <= MAX_TILES);
        self.candidates(lo, hi)
            .into_iter()
            .filter(|(_, ((bottom, top), tiles))| {
                lo < *top
                    && *bottom < hi
                    && match (asked, tiles) {
                        (Some(((a, b), (c, d))), Some(((e, f), (g, h)))) => a <= *g && *e <= c && b <= *h && *f <= d,
                        _ => true,
                    }
            })
            .map(|(id, _)| id)
            .collect()
    }
}

/// The plan box a wall's body lies in, grown by twice the tolerance: [x0, y0, x1, y1]. An arc's whole circle's.
pub(crate) fn reach(m: &Model, w: &crate::model::Wall) -> Option<[f64; 4]> {
    let axis = super::Axis::of(w);
    let (ext, int) = super::faces(m, w);
    let grow = ext.abs().max(int.abs()) + 2.0 * TOL;
    let [x0, y0, x1, y1] = match axis.arc() {
        Some((c, r, _, _)) => [c.0 - r, c.1 - r, c.0 + r, c.1 + r],
        None => [w.start.0.min(w.end.0), w.start.1.min(w.end.1), w.start.0.max(w.end.0), w.start.1.max(w.end.1)],
    };
    let b = [x0 - grow, y0 - grow, x1 + grow, y1 + grow];
    b.iter().all(|v| v.is_finite() && v.abs() < 1e12).then_some(b)
}

fn tile(v: f64) -> i64 {
    (v / TILE).floor() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{engine::Session, model::Model, walls};
    use serde_json::json;

    #[test]
    fn a_tiles_search_by_height_leaves_out_no_wall_that_overlaps() {
        let spans = [(0.0, 4000.0), (0.0, 400_000.0), (4000.0, 8000.0), (-3000.0, -1.0), (8000.0, 8000.0), (396_000.0, 400_000.0)];
        let mut tile = Tile::default();
        for (id, (lo, hi)) in spans.iter().enumerate() {
            tile.items.push((*lo, id as Id, hi - lo));
            tile.tallest = tile.tallest.max(hi - lo);
        }
        tile.items.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let edges = [-1e12, -3000.0, -1.0, 0.0, 1.0, 3999.0, 4000.0, 8000.0, 396_000.0, 400_000.0, 1e12];
        let queries = edges.iter().flat_map(|lo| edges.iter().map(move |hi| (*lo, *hi)));
        for (lo, hi) in queries.chain([(f64::NEG_INFINITY, f64::INFINITY), (0.0, f64::INFINITY), (f64::NAN, 1.0), (0.0, f64::NAN)]) {
            let found: Vec<Id> = tile.may_overlap(lo, hi).collect();
            for (id, (bottom, top)) in spans.iter().enumerate() {
                if lo < *top && *bottom < hi {
                    assert!(found.contains(&(id as Id)), "wall {id} overlaps {lo} to {hi}: {found:?}");
                }
            }
        }
        // With no tall wall, only the storeys near: what makes a tower's tile cheap.
        let storeys = Tile { items: (0..100).map(|i| (f64::from(i) * 4000.0, Id::from(i as u32), 4000.0)).collect(), tallest: 4000.0 };
        assert!(storeys.may_overlap(200_000.0, 204_000.0).count() <= 4);
    }

    #[test]
    fn indexed_joins_match_brute_force_for_storeys_arcs_tall_and_attached_walls() {
        let mut s = Session::new();
        s.script(crate::sample::SAMPLE).unwrap();
        s.execute("wall", &json!({"points":[[14000,0],[18000,0]],"arc":90,"height":9000})).unwrap();
        s.execute("wall", &json!({"points":[[0,0],[0,-5000]],"base_offset":-10000,"height":1000000})).unwrap();
        for level in ["Level 1", "Level 2"] {
            s.execute("wall", &json!({"points":[[0,0],[6000,0],[6000,4000]],"level":level})).unwrap();
        }
        agree(&s.model);
        // Walls meeting on the tiles' edges (every 6 m), crossing, and ending on each other, storeys apart.
        agree(&crate::sample::tower::tower_on(crate::sample::tower::Plate::SMALL, 2).unwrap());
    }

    #[test]
    fn the_runs_near_a_point_or_a_box_from_the_lookup_are_the_indexs() {
        let mut s = Session::new();
        s.reset(crate::sample::tower::tower_on(crate::sample::tower::Plate::SMALL, 2).unwrap_or_else(|e| panic!("{e}")), None);
        // Footings under walls, a sloped beam, a steep pipe the lookup doesn't band, and runs on the tiles' edges.
        for (cmd, p) in [
            ("strip_footing", json!({"points": [[0, 0], [12000, 0]]})),
            ("beam", json!({"points": [[6000, 6000], [18000, 6000]], "offset": 3000, "end_offset": 3600})),
            ("pipe", json!({"points": [[12000, 12000], [12010, 12000]], "offset": 0, "end_offset": 3000})),
            ("conduit", json!({"points": [[5999, 0], [6001, 12000]], "offset": 2500})),
        ] {
            s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"));
        }
        let m = &s.model;
        let (index, near) = (Index::new(m), RunsNear::new(m));
        let mut asked = 0;
        for e in m.elements.values() {
            let boxed = match e {
                Element::Wall(w) => Some((crate::derived::axis_box(w), m.base_z(w) - TOL, m.top_z(w) + TOL)),
                e => m.run_geom(e).map(|g| {
                    let (lo, hi) = g.z_span();
                    ([g.a[0].min(g.b[0]), g.a[1].min(g.b[1]), g.a[0].max(g.b[0]), g.a[1].max(g.b[1])], lo, hi)
                }),
            };
            let Some((b, lo, hi)) = boxed else { continue };
            assert_eq!(near.runs_near_box(b, 2.0 * TOL, lo, hi), index.runs_near_box(b, 2.0 * TOL, lo, hi), "{e:?}");
            for p in [P2(b[0], b[1]), P2(b[2], b[3])] {
                assert_eq!(near.runs_near(p, lo - TOL, hi + TOL), index.runs_near(p, lo - TOL, hi + TOL), "{e:?} at {p:?}");
            }
            asked += 1;
        }
        assert!(asked > 100, "{asked}");
        for (lo, hi) in [(f64::NEG_INFINITY, f64::INFINITY), (f64::NAN, 1.0), (0.0, 1e13)] {
            assert_eq!(near.runs_near(P2(6000.0, 0.0), lo, hi), index.runs_near(P2(6000.0, 0.0), lo, hi));
            assert_eq!(near.runs_near(P2(f64::NAN, 0.0), lo, hi), index.runs_near(P2(f64::NAN, 0.0), lo, hi));
            assert_eq!(near.runs_near_box([0.0, 0.0, 1e13, 1.0], 0.0, lo, hi), index.runs_near_box([0.0, 0.0, 1e13, 1.0], 0.0, lo, hi));
        }
    }

    #[test]
    fn an_index_brought_up_to_date_by_random_edits_is_the_one_made_of_every_element() {
        // Each command's model asks its index, brought up to date from the one before (`Model::wall_index` holds it to the
        // index made of every element in tests): walls moved, copied and deleted with their doors, runs, a level's
        // walls attached to a roof, undo and redo.
        let model = crate::sample::tower::tower_on(crate::sample::tower::Plate::SMALL, 1).unwrap_or_else(|e| panic!("{e}"));
        let mut s = Session::new();
        s.reset(model, None);
        crate::oracle_tests::frame_some(&mut s);
        let level = s.model.levels()[0].0;
        let roof = s
            .execute("roof", &json!({"boundary": [[-1000, -1000], [13000, -1000], [13000, 7000], [-1000, 7000]], "slope": 20, "level": level, "offset": 3500}))
            .unwrap_or_else(|e| panic!("{e}"))["id"]
            .clone();
        let under =
            |w: &crate::model::Wall| [w.start, w.end].iter().all(|q| (0.0..=12000.0).contains(&q.0) && (0.0..=6000.0).contains(&q.1));
        let walls: Vec<Id> =
            s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(w) if under(w))).map(|(id, _)| *id).collect();
        let attached = walls.iter().filter(|w| s.execute("modify", &json!({"id": w, "attach_top": roof})).is_ok()).count();
        assert!(attached > 0, "no wall under the roof attached to it: {walls:?}");
        s.model.wall_index();
        let mut dice = crate::oracle_tests::Dice(0x1d3c_5e7f_9a0b_2c4d);
        let mut updated = 0;
        for n in 0..60 {
            let (cmd, p) = if n % 7 == 6 {
                (if n % 2 == 0 { "undo" } else { "redo" }.to_string(), json!({}))
            } else {
                crate::oracle_tests::edit(&mut s, &mut dice)
            };
            if s.execute(&cmd, &p).is_ok() {
                updated += 1;
            }
            s.model.wall_index();
        }
        assert!(updated > 20, "{updated}");
        // The roof raised and pitched: the walls attached to it change their heights, though not themselves.
        for p in [json!({"id": roof, "offset": 4200}), json!({"id": roof, "slope": 35})] {
            s.execute("modify", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
            s.model.wall_index();
        }
    }

    /// Each wall's geometry found through the index is what it is found among every wall.
    fn agree(m: &Model) {
        let index = Index::new(m);
        for (&id, e) in &m.elements {
            if !matches!(e, Element::Wall(_)) {
                continue;
            }
            assert_eq!(walls::centre_line(m, id), walls::centre_line_in(m, id, Some(&index)), "wall {id}");
            for end in [false, true] {
                assert_eq!(walls::cap(m, id, end), walls::cap_in(m, id, end, Some(&index)));
            }
            assert_eq!(walls::outline(m, id), walls::outline_in(m, id, Some(&index)));
            assert_eq!(walls::layer_lines(m, id), walls::layer_lines_in(m, id, Some(&index)));
            for openings in [false, true] {
                let a = walls::prisms(m, id, openings);
                let b = walls::prisms_in(m, id, openings, Some(&index));
                assert_eq!(a, b);
            }
        }
    }
}
