//! A scene brought up to date after an edit: the triangles of the elements it touched made again, with those of what depends
//! on them, and every other element's kept as they were. [`Scene::new`] stays the whole computation the update is held to.
//!
//! What an element's triangles depend on, besides itself:
//! - what it points at and what points at it (`Element::refs`, `Lookup::referrers`): a door's wall, a wall's doors, a level's
//!   elements, a roof a wall is attached to;
//! - what touches it: a wall's joins, a slab cut by a shaft. Each element made again is compared with what the scene held, and
//!   only one that came out otherwise, or one the edit touched, makes those its box touches (grown a little) be made again, until
//!   none does;
//! - the library (types, materials, families) and the project's settings: an edit of those is a whole scene again.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use super::{
    P3, Scene, ShapeKey, Span, Tri, bounds, corner_normals, layered, local_curves, painted, rules_of, skins_for, solids_in, whole_model,
};
use crate::component_geometry::ComponentInstance;
use crate::ids::Changed;
use crate::model::{Element, Id, Model};

/// How far past an element's box another's must lie not to be touched by it, mm: more than a wall's join reaches past its faces.
const REACH: f64 = 10.0;

/// Past this share of the elements made again, the whole scene is made instead.
const MOST: usize = 4;

/// The rounds of making again before the whole scene is made instead: a chain of joins this long is a whole model's.
const ROUNDS: usize = 64;

impl Scene {
    /// The scene of `after`, this being `before`'s, made by what changed between them.
    pub fn update(&self, before: &Model, after: &Model) -> Scene {
        self.update_cancellable(before, after, &|| false).unwrap_or_else(|| Scene::new(after))
    }

    /// [`Scene::update`], none when `cancelled`.
    pub fn update_cancellable(&self, before: &Model, after: &Model, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<Scene> {
        match self.made_again(before, after, cancelled) {
            Made::Some(made) => self.spliced(after, made, cancelled),
            Made::Whole => Scene::new_cancellable(after, cancelled),
            Made::Cancelled => None,
        }
    }

    /// The elements of `after` whose triangles an edit from `before` changed, each made again.
    fn made_again(&self, before: &Model, after: &Model, cancelled: &(dyn Fn() -> bool + Sync)) -> Made {
        let Changed::Ids(changed) = after.changed_since(before) else { return Made::Whole };
        // Numbers past a card's planes and copies would need the whole scene numbered again.
        if self.table.next > u32::MAX as usize / 4 {
            return Made::Whole;
        }
        let (old, new) = (before.lookup(), after.lookup());
        let index = crate::walls::Index::new(after);
        let thickest =
            [before, after].iter().flat_map(|m| m.types.wall.values().map(|t| t.thickness)).filter(|t| t.is_finite()).fold(0.0, f64::max);
        // What the scene draws that has no triangles yet: what touches it may give it some.
        let empty: Vec<Id> = after.elements.iter().filter(|(id, e)| draws(e) && self.span(**id).is_none()).map(|(id, _)| *id).collect();
        let mut made: HashMap<Id, Vec<Tri>> = HashMap::new();
        // What is to be made next: what the edit touched, then what touches what came out otherwise.
        let mut pending: BTreeSet<Id> = changed.clone();
        // What came out otherwise in the round before, whose pointers either way are made in this one.
        let mut moved: Vec<Id> = Vec::new();
        for _ in 0..ROUNDS {
            if pending.is_empty() && moved.is_empty() {
                return Made::Some(made);
            }
            if cancelled() {
                return Made::Cancelled;
            }
            // With what points at it, either way.
            let mut wanted: BTreeSet<Id> = BTreeSet::new();
            wanted.extend(pending.iter().copied());
            for &id in pending.iter().chain(&moved) {
                for (m, lookup) in [(before, &old), (after, &new)] {
                    let Some(e) = m.elements.get(id) else { continue };
                    if let Some(g) = m.elements.guid(id) {
                        wanted.extend(lookup.referrers(&g));
                    }
                    wanted.extend(e.refs().iter().filter_map(|g| after.elements.id(*g)));
                }
            }
            let fresh: Vec<(Id, &Element)> =
                wanted.iter().filter(|id| !made.contains_key(id)).filter_map(|id| Some((*id, after.elements.get(*id)?))).collect();
            if (made.len() + fresh.len()) * MOST > after.elements.len().max(1) {
                return Made::Whole;
            }
            let tris = element_tris(after, &fresh, &index);
            // Where what came out otherwise was and is: what the edit touched (gone, too) and what was made unlike before.
            let mut regions: Vec<[P3; 2]> = Vec::new();
            moved = Vec::new();
            for id in wanted.iter().filter(|id| !made.contains_key(id)) {
                let own = tris.get(id).map(Vec::as_slice).unwrap_or_default();
                let was = self.span(*id).and_then(|s| self.tris.get(s.at.clone())).unwrap_or_default();
                if changed.contains(id) || own != was {
                    regions.extend(self.span(*id).map(|s| s.bounds));
                    regions.extend(bounds(own.iter().flat_map(|t| t.p)));
                    regions.extend(shaft_reach(before, *id));
                    regions.extend(shaft_reach(after, *id));
                    moved.push(*id);
                }
            }
            for (id, _) in &fresh {
                made.insert(*id, tris.get(id).cloned().unwrap_or_default());
            }
            // Gone, it was made as nothing.
            for id in &wanted {
                made.entry(*id).or_default();
            }
            let grown: Vec<[P3; 2]> = regions.iter().map(|[lo, hi]| [lo.map(|v| v - REACH), hi.map(|v| v + REACH)]).collect();
            let reach = Reach::of(&grown);
            pending = self.spans.iter().filter(|s| reach.touches(&s.bounds)).map(|s| s.id).collect();
            // A wall joins another whose location line comes within the thickest wall's thickness of its end, which its own
            // box, cut back at a join, may be farther from.
            let near: Vec<([f64; 4], f64, f64)> = regions.iter().map(|[lo, hi]| ([lo[0], lo[1], hi[0], hi[1]], lo[2], hi[2])).collect();
            if !near.is_empty() {
                pending.extend(old.walls_near_at(&near, thickest + REACH));
                pending.extend(new.walls_near_at(&near, thickest + REACH));
            }
            if !grown.is_empty() {
                pending.extend(empty.iter().copied());
            }
            pending.retain(|id| !made.contains_key(id));
        }
        Made::Whole
    }

    /// This scene with `made`'s elements' triangles in place of theirs, and every element of `after` that isn't any more taken
    /// out.
    fn spliced(&self, after: &Model, mut made: HashMap<Id, Vec<Tri>>, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<Scene> {
        let mut skins = self.skins.clone();
        for own in made.values() {
            skins_for(after, &mut skins, own);
        }
        let rules = rules_of(after, made.keys().copied());
        let mut table = self.table.clone();
        let n = self.tris.len() + made.values().map(Vec::len).sum::<usize>();
        let mut out = Parts {
            tris: Vec::with_capacity(n),
            planes: Vec::with_capacity(n),
            smooth: Vec::with_capacity(n),
            faces: Vec::with_capacity(n),
            placements: Vec::with_capacity(self.placements.len()),
            spans: Vec::with_capacity(self.spans.len()),
        };
        let mut keys = self.shape_keys.clone();
        let mut shapes = self.shapes.clone();
        let copies: HashMap<usize, &ComponentInstance> = self.placements.iter().map(|(at, c)| (*at, c)).collect();
        for (n, (id, e)) in after.elements.iter().enumerate() {
            if n % 4096 == 0 && cancelled() {
                return None;
            }
            let start = out.tris.len();
            if let Some(own) = made.remove(id) {
                let faces: Vec<&'static str> = own.iter().map(|t| rules.get(&t.owner).map_or("", |r| r.face(&t.p))).collect();
                out.smooth.extend(corner_normals(&own));
                out.planes.extend(own.iter().map(|t| table.assign(t)));
                out.faces.extend(faces);
                out.tris.extend(own);
                let own = out.tris.get(start..).unwrap_or_default();
                if let Some(copy) = copy_of(after, *id, e, &mut keys, &mut shapes)
                    && shapes.get(copy.shape).is_some_and(|local| placed(local, &copy, own))
                {
                    out.placements.push((start, copy));
                }
            } else if let Some(span) = self.span(*id) {
                let at = span.at.clone();
                out.tris.extend_from_slice(self.tris.get(at.clone())?);
                out.planes.extend_from_slice(self.planes.get(at.clone())?);
                out.smooth.extend_from_slice(self.smooth.get(at.clone())?);
                out.faces.extend_from_slice(self.faces.get(at.clone())?);
                if let Some(copy) = copies.get(&at.start) {
                    out.placements.push((start, **copy));
                }
                out.spans.push(Span { id: *id, at: start..out.tris.len(), bounds: span.bounds });
                continue;
            }
            let end = out.tris.len();
            if end > start {
                let bounds = bounds(out.tris[start..end].iter().flat_map(|t| t.p)).unwrap_or_default();
                out.spans.push(Span { id: *id, at: start..end, bounds });
            }
        }
        let (masses, levels, scans) = whole_model(after, &out.spans);
        Some(Scene {
            tris: out.tris,
            planes: out.planes,
            smooth: out.smooth,
            faces: Arc::new(out.faces),
            skins,
            edges: std::sync::OnceLock::new(),
            masses,
            levels,
            scans,
            placements: out.placements,
            shapes,
            shape_keys: keys,
            spans: out.spans,
            table,
        })
    }

    /// Where element `id`'s triangles are.
    fn span(&self, id: Id) -> Option<&Span> {
        let k = self.spans.binary_search_by_key(&id, |s| s.id).ok()?;
        self.spans.get(k)
    }
}

/// Boxes, in a coarse grid of cells, so whether a box touches one of them asks only those in the cells it is in.
struct Reach<'a> {
    boxes: &'a [[P3; 2]],
    cells: HashMap<[i64; 3], Vec<usize>>,
    /// Those over too many cells to list in each: asked of every box.
    everywhere: Vec<usize>,
}

impl<'a> Reach<'a> {
    /// A cell's side, mm: a storey.
    const CELL: f64 = 4000.0;
    /// The most cells a box is listed in.
    const MOST: i64 = 4096;

    /// The cells a box is in, or none where they are too many (or it isn't finite).
    fn cells([lo, hi]: &[P3; 2]) -> Option<[std::ops::RangeInclusive<i64>; 3]> {
        let at = |v: f64| (v / Self::CELL).floor();
        let (a, b) = (lo.map(at), hi.map(at));
        if !a.iter().chain(&b).all(|v| v.is_finite() && v.abs() < 1e12) {
            return None;
        }
        let (a, b) = (a.map(|v| v as i64), b.map(|v| v as i64));
        let count = (0..3).try_fold(1i64, |n, k| n.checked_mul(b[k] - a[k] + 1))?;
        (count <= Self::MOST).then(|| std::array::from_fn(|k| a[k]..=b[k]))
    }

    fn of(boxes: &'a [[P3; 2]]) -> Reach<'a> {
        let mut out = Reach { boxes, cells: HashMap::new(), everywhere: Vec::new() };
        for (n, b) in boxes.iter().enumerate() {
            match Self::cells(b) {
                Some([xs, ys, zs]) => {
                    for x in xs {
                        for y in ys.clone() {
                            for z in zs.clone() {
                                out.cells.entry([x, y, z]).or_default().push(n);
                            }
                        }
                    }
                }
                None => out.everywhere.push(n),
            }
        }
        out
    }

    /// Whether `b` touches one of the boxes.
    fn touches(&self, b: &[P3; 2]) -> bool {
        let meets = |n: &usize| self.boxes.get(*n).is_some_and(|g| (0..3).all(|k| b[0][k] <= g[1][k] && g[0][k] <= b[1][k]));
        if self.everywhere.iter().any(meets) {
            return true;
        }
        match Self::cells(b) {
            Some([xs, ys, zs]) => xs
                .into_iter()
                .any(|x| ys.clone().any(|y| zs.clone().any(|z| self.cells.get(&[x, y, z]).is_some_and(|ns| ns.iter().any(meets))))),
            None => self.boxes.iter().enumerate().any(|(n, _)| meets(&n)),
        }
    }
}

/// What making again found.
enum Made {
    Some(HashMap<Id, Vec<Tri>>),
    Whole,
    Cancelled,
}

/// A scene's lists, gathered element by element.
#[derive(Default)]
struct Parts {
    tris: Vec<Tri>,
    planes: Vec<usize>,
    smooth: Vec<Option<Arc<[P3; 3]>>>,
    faces: Vec<&'static str>,
    placements: Vec<(usize, ComponentInstance)>,
    spans: Vec<Span>,
}

/// Whether the scene makes triangles of an element of this kind.
fn draws(e: &Element) -> bool {
    e.run().is_some()
        || matches!(
            e,
            Element::Wall(_)
                | Element::Floor(_)
                | Element::Ceiling(_)
                | Element::Roof(_)
                | Element::Column(_)
                | Element::StructuralColumn(_)
                | Element::Footing(_)
                | Element::Rebar(_)
                | Element::Mass(_)
                | Element::Component(_)
                | Element::Stair(_)
                | Element::Ramp(_)
                | Element::Railing(_)
                | Element::Door(_)
                | Element::Window(_)
        )
}

/// Where shaft `id` cuts, in `m`: its plan box over its span.
fn shaft_reach(m: &Model, id: Id) -> Option<[P3; 2]> {
    if !matches!(m.elements.get(id), Some(e) if e.category() == "shaft") {
        return None;
    }
    let shafts = m.derived.shafts(&m.elements, || m.shaft_table());
    let s = shafts.iter().find(|s| s.id == id)?;
    Some([[s.plan[0], s.plan[1], s.bottom], [s.plan[2], s.plan[3], s.top]])
}

/// The triangles of `elements`, each element's as the whole scene makes them: painted, its curves numbered its own.
fn element_tris(m: &Model, elements: &[(Id, &Element)], index: &crate::walls::Index) -> HashMap<Id, Vec<Tri>> {
    let made = |chunk: &[(Id, &Element)]| {
        let mut tris = Vec::new();
        for solid in solids_in(m, chunk, Some(index)).into_iter().flat_map(|s| layered(m, s)) {
            solid.tris(&mut tris);
        }
        tris
    };
    // Many, on the render threads as the whole scene makes them.
    let tris: Vec<Tri> = match (elements.len() >= 1000).then(super::render_pool).flatten() {
        Some(pool) => {
            use rayon::prelude::*;
            pool.install(|| elements.par_chunks(128).map(made).collect::<Vec<_>>()).into_iter().flatten().collect()
        }
        None => made(elements),
    };
    let rules = rules_of(m, elements.iter().map(|(id, _)| *id));
    let mut tris = tris;
    let faces: Vec<&'static str> = tris.iter().map(|t| rules.get(&t.owner).map_or("", |r| r.face(&t.p))).collect();
    painted(m, &mut tris, &faces);
    let mut out: HashMap<Id, Vec<Tri>> = HashMap::new();
    for t in tris {
        out.entry(t.owner).or_default().push(t);
    }
    for own in out.values_mut() {
        local_curves(own);
    }
    out
}

/// The copy element `id` is of a shape, as the whole scene finds it: a component's of its family's, a round run's of its
/// material's tube; a shape it is the first of is added.
fn copy_of(
    m: &Model,
    id: Id,
    e: &Element,
    keys: &mut HashMap<ShapeKey, usize>,
    shapes: &mut Vec<Vec<[P3; 3]>>,
) -> Option<ComponentInstance> {
    match e {
        Element::Component(c) => {
            let key = ShapeKey::Family(crate::component_geometry::SharedComponents::key(m, c));
            let shape = match keys.get(&key) {
                Some(n) => *n,
                None => {
                    shapes.push(m.component_local_pieces(c).iter().flat_map(|p| p.tris.iter().copied()).collect());
                    keys.insert(key, shapes.len() - 1);
                    shapes.len() - 1
                }
            };
            Some(ComponentInstance::placed(id, shape, &m.component_placement(c)))
        }
        _ => {
            let (t, g) = (m.run_type(e)?, m.run_geom(e)?);
            if !g.round || t.profile.is_some() {
                return None;
            }
            let key = ShapeKey::Tube(t.material.clone());
            let shape = match keys.get(&key) {
                Some(n) => *n,
                None => {
                    shapes.push(crate::runs::unit_tube().0);
                    keys.insert(key, shapes.len() - 1);
                    shapes.len() - 1
                }
            };
            Some(g.tube_place(id, shape)).filter(ComponentInstance::solid)
        }
    }
}

/// Whether `own` is `local` placed by `copy`, to the last bit.
fn placed(local: &[[P3; 3]], copy: &ComponentInstance, own: &[Tri]) -> bool {
    !local.is_empty() && local.len() == own.len() && local.iter().zip(own).all(|(l, t)| l.map(|q| copy.place(q)) == t.p)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};

    use super::*;
    use crate::engine::Session;
    use crate::oracle_tests::{Dice, edit};
    use crate::view3d::{AZIMUTH, ELEVATION};

    /// That `a`, a scene brought up to date, is `b`, the whole scene made again: the same triangles, faces, normals, textures'
    /// ways and copies, planes numbered alike (the same triangles share one), and the same picture.
    fn same(a: &Scene, b: &Scene, what: &str) {
        let differ: Vec<(Id, usize, usize)> = {
            let ids: BTreeSet<Id> = a.spans.iter().chain(&b.spans).map(|s| s.id).collect();
            fn own(x: &Scene, id: Id) -> &[Tri] {
                x.span(id).and_then(|s| x.tris.get(s.at.clone())).unwrap_or_default()
            }
            ids.into_iter().filter(|id| own(a, *id) != own(b, *id)).map(|id| (id, own(a, id).len(), own(b, id).len())).collect()
        };
        if !differ.is_empty() {
            for (id, ..) in &differ {
                let (x, y) = (a.span(*id), b.span(*id));
                eprintln!("{id}: {x:?} {y:?}");
                if let (Some(x), Some(y)) = (x, y) {
                    for (p, q) in a.tris[x.at.clone()].iter().zip(&b.tris[y.at.clone()]) {
                        if p != q {
                            eprintln!("  {p:?}\n  {q:?}");
                            break;
                        }
                    }
                }
            }
        }
        assert!(differ.is_empty(), "{what}: the triangles of (element, brought up to date, made whole) {differ:?}");
        assert!(a.tris == b.tris, "{what}: triangles");
        assert!(a.faces == b.faces, "{what}: faces");
        assert!(a.smooth == b.smooth, "{what}: smooth normals");
        assert_eq!(a.spans, b.spans, "{what}: spans");
        assert_eq!((&a.masses, &a.levels), (&b.masses, &b.levels), "{what}: masses and levels");
        let (mut there, mut back) = (HashMap::new(), HashMap::new());
        for (k, (p, q)) in a.planes.iter().zip(&b.planes).enumerate() {
            assert_eq!(*there.entry(*p).or_insert(*q), *q, "{what}: triangle {k}'s plane is another's");
            assert_eq!(*back.entry(*q).or_insert(*p), *p, "{what}: triangle {k}'s plane is another's");
        }
        assert_eq!(a.placements.len(), b.placements.len(), "{what}: copies");
        for ((i, c), (j, d)) in a.placements.iter().zip(&b.placements) {
            assert_eq!((i, c.owner, c.at, c.axes), (j, d.owner, d.at, d.axes), "{what}: a copy");
            assert_eq!(a.shapes.get(c.shape), b.shapes.get(d.shape), "{what}: a copy's shape");
        }
        for t in &b.tris {
            let k = t.texture() as usize;
            assert_eq!(a.skins.get(k).is_some_and(Option::is_some), b.skins.get(k).is_some_and(Option::is_some), "{what}: texture {k}");
        }
        let cam = b.fit(AZIMUTH, ELEVATION, 240, 180);
        assert!(a.render(&cam, &[]).png() == b.render(&cam, &[]).png(), "{what}: the picture");
    }

    /// Runs `cmd` and holds the scene brought up to date to the whole one: the scene after.
    fn held(s: &mut Session, scene: Scene, cmd: &str, p: &Value) -> Scene {
        let before = Arc::clone(&s.model);
        if s.execute(cmd, p).is_err() {
            return scene;
        }
        let up = scene.update(&before, &s.model);
        same(&up, &Scene::new(&s.model), &format!("{cmd} {p}"));
        up
    }

    fn of(s: &Session, category: &str) -> Vec<Id> {
        s.model.elements.iter().filter(|(_, e)| e.category() == category).map(|(id, _)| *id).collect()
    }

    #[test]
    fn a_scene_brought_up_to_date_by_an_edit_is_the_scene_made_whole() {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
               door {"at": [2000, 0]}
               window {"at": [8000, 3000]}
               floor {}
               component {"family": "Table", "type": "1600 x 900", "at": [2000, 3000], "angle": 30}
               component {"family": "Table", "type": "1600 x 900", "at": [5000, 3000]}"#,
        )
        .unwrap();
        let mut scene = Scene::new(&s.model);
        let walls = of(&s, "wall");
        let level = of(&s, "level")[0];
        let table = of(&s, "component")[0];
        let edits = [
            ("move", json!({"ids": [walls[1]], "by": [500, 0]})),
            ("move", json!({"ids": [walls[0]], "by": [0, 300], "end": "end"})),
            ("modify", json!({"id": walls[2], "height": 4000})),
            ("modify", json!({"id": level, "elevation": 500})),
            ("paint", json!({"ids": [walls[0]], "face": "exterior", "material": "Timber"})),
            ("shaft", json!({"boundary": [[3000, 3000], [4000, 3000], [4000, 4000], [3000, 4000]]})),
            ("move", json!({"ids": [table], "by": [1000, 500]})),
            ("copy", json!({"ids": [table], "by": [0, 1500]})),
            ("component", json!({"family": "Chair", "at": [6000, 1000]})),
            ("delete", json!({"ids": [of(&s, "door")[0]]})),
            ("wall", json!({"points": [[4000, 0], [4000, 6000]]})),
            ("material", json!({"name": "Timber", "color": [200, 40, 40]})),
            ("delete", json!({"ids": [walls[3]]})),
        ];
        for (cmd, p) in &edits {
            scene = held(&mut s, scene, cmd, p);
        }
        // An undo and a redo: one model of the history for another.
        for step in ["undo", "redo", "undo"] {
            let before = Arc::clone(&s.model);
            s.execute(step, &json!({})).unwrap();
            let up = scene.update(&before, &s.model);
            same(&up, &Scene::new(&s.model), step);
            scene = up;
        }
    }

    /// A window rising past its wall's top (issue #215) notches the wall: each edit of the window, of the wall's height or of
    /// what its top follows brings the notch up to date, as the scene made whole has it.
    #[test]
    fn a_window_past_its_walls_top_keeps_its_notch_up_to_date_through_edits() {
        let mut s = Session::new();
        s.script(
            r#"type {"category": "window", "name": "Tall", "width": 1200, "height": 1100, "sill": 30}
               wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true, "height": 1060}
               window {"wall": 100002, "offset": 4000, "type": "Tall"}
               roof {"boundary": [[-600, -600], [8600, -600], [8600, 6600], [-600, 6600]], "gable": true}"#,
        )
        .unwrap();
        let mut scene = Scene::new(&s.model);
        let (walls, window, roof) = (of(&s, "wall"), of(&s, "window")[0], of(&s, "roof")[0]);
        let edits = [
            ("modify", json!({"id": walls[0], "height": 900})),
            ("modify", json!({"id": window, "sill": 500})),
            ("modify", json!({"id": walls[0], "height": 2000})),
            ("modify", json!({"id": walls[0], "height": 1060})),
            ("move", json!({"ids": [window], "by": [1500, 0]})),
            ("modify", json!({"ids": walls, "attach_top": roof})),
            ("modify", json!({"ids": walls, "attach_top": null})),
            ("copy", json!({"ids": [walls[0]], "by": [0, -3000]})),
            ("delete", json!({"ids": [window]})),
        ];
        let steps = s.history().len();
        for (cmd, p) in &edits {
            scene = held(&mut s, scene, cmd, p);
        }
        assert_eq!(s.history().len(), steps + edits.len(), "every edit is made: {:?}", &s.history()[steps..]);
        assert!(s.model.elements.values().filter(|e| matches!(e, crate::model::Element::Window(_))).count() == 1, "the copy's");
    }

    #[test]
    fn random_edits_of_a_tower_bring_its_scene_up_to_date_as_made_whole() {
        let model = crate::sample::tower::tower_on(crate::sample::tower::Plate::SMALL, 1).unwrap();
        for seed in [1u64, 2] {
            let mut s = Session::new();
            s.reset(model.clone(), None);
            let mut scene = Scene::new(&s.model);
            let mut dice = Dice(0x9e37_79b9_7f4a_7c15 ^ seed);
            for _ in 0..25 {
                let (cmd, p) = edit(&mut s, &mut dice);
                scene = held(&mut s, scene, &cmd, &p);
            }
        }
    }

    /// The other kinds and commands (3D dimensions, camera views, shaped floors, roofs with a slope for each edge, a wall's top
    /// offset, bars, plan symbols…) edited at random: the scene brought up to date by each is the one made whole.
    #[test]
    fn random_edits_of_the_other_kinds_bring_the_scene_up_to_date_as_made_whole() {
        let model = crate::sample::tower::tower_on(crate::sample::tower::Plate::SMALL, 1).unwrap();
        for seed in [1u64, 2] {
            let mut s = Session::new();
            s.reset(model.clone(), None);
            let mut scene = Scene::new(&s.model);
            let mut dice = Dice(0x3d3d_7a11_c0de_0188 ^ seed);
            for _ in 0..30 {
                let (cmd, p) = crate::oracle_tests::edit_more(&mut s, &mut dice);
                scene = held(&mut s, scene, &cmd, &p);
            }
        }
    }

    #[test]
    fn an_edit_makes_again_what_it_touched_not_the_model() {
        let model = crate::sample::tower::tower_on(crate::sample::tower::Plate::SMALL, 2).unwrap();
        let mut s = Session::new();
        s.reset(model, None);
        let scene = Scene::new(&s.model);
        let before = Arc::clone(&s.model);
        let wall = of(&s, "wall").into_iter().find(|w| s.execute("move", &json!({"ids": [w], "by": [0, 200]})).is_ok()).unwrap();
        let Made::Some(made) = scene.made_again(&before, &s.model, &|| false) else { panic!("the whole scene made again") };
        let all = s.model.elements.len();
        assert!(made.contains_key(&wall));
        assert!(made.len() * 20 < all, "{} of {all} made again", made.len());
    }
}

/// What an edit costs the 10-storey annotated tower's scene, against making it whole: `cargo test --release -p bimcraft-core
/// update_speed -- --ignored --nocapture`.
#[cfg(test)]
#[test]
#[ignore]
fn update_speed() {
    use serde_json::json;
    use web_time::Instant;
    let model = crate::sample::tower::annotated(crate::sample::tower::Plate::REFERENCE, 10).unwrap();
    let mut s = crate::engine::Session::new();
    s.reset(model, None);
    s.model.lookup();
    let t = Instant::now();
    let mut scene = Scene::new(&s.model);
    eprintln!("whole scene: {:.2} s, {} triangles", t.elapsed().as_secs_f64(), scene.tris.len());
    let of = |s: &crate::engine::Session, c: &str| s.model.elements.iter().find(|(_, e)| e.category() == c).map(|(id, _)| *id);
    let level = s.model.levels().into_iter().nth(5).map(|(id, _)| id);
    let edits = [
        ("move a wall", "move", json!({"ids": [of(&s, "wall")], "by": [0, 100]})),
        ("move a component", "move", json!({"ids": [of(&s, "component")], "by": [300, 0]})),
        ("a level's elevation", "modify", json!({"id": level, "elevation": 20_100})),
    ];
    for (what, cmd, p) in edits {
        let before = std::sync::Arc::clone(&s.model);
        if let Err(e) = s.execute(cmd, &p) {
            eprintln!("{what}: {e}");
            continue;
        }
        s.model.lookup();
        let t = Instant::now();
        let made = match scene.made_again(&before, &s.model, &|| false) {
            Made::Some(m) => m.len(),
            _ => usize::MAX,
        };
        let found = t.elapsed().as_secs_f64();
        let up = scene.update(&before, &s.model);
        let updated = t.elapsed().as_secs_f64();
        let t = Instant::now();
        let mesh = super::gpu::Mesh::new(&up, &crate::graphics::Look::of_3d(&s.model));
        let meshed = t.elapsed().as_secs_f64();
        let t = Instant::now();
        let whole = Scene::new(&s.model);
        eprintln!(
            "{what}: {made} elements made again in {found:.3} s; brought up to date {:.3} s; whole {:.2} s; mesh {meshed:.2} s ({})",
            updated - found,
            t.elapsed().as_secs_f64(),
            mesh.is_ok()
        );
        drop(whole);
        scene = up;
    }
}
