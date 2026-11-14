//! Rooms' boundaries for one look at a model: what a plan, a schedule, `inspect` or an IFC export asks of many rooms.
//!
//! A room's boundary is the smallest loop of its level's room-bounding lines around its point, less the islands inside it
//! ([`geom::region_in`]). Finding those lines takes each bounding wall's joins, and arranging them into loops is the costly
//! part; both are the same for every room of a level. [`Rooms`] does them once a level and a measure, keeps the plan with
//! the model's [`crate::derived::Derived`], and finds the walls' joins through a plan index ([`walls::Index`]) rather than
//! among every wall of the project. What it keeps of the model is keyed by the elements' state; a command's copy of the model
//! makes it again rather than bringing it up to date.

use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::sync::Arc;

use crate::engine::Session;
use crate::geom::{self, P2, Region};
use crate::ids::Changed;
use crate::model::{Ceiling, Element, Finish, Floor, Guid, Id, Model, Part, Room, RoomArea, RoomType, TOL, Unbound, region_holds};
use crate::walls::{self, Index};

/// What a level's rooms are found among, at one measure.
pub(crate) struct Plan {
    /// The loops its room-bounding lines make.
    faces: Vec<Vec<P2>>,
    /// At the finish faces, its bounding walls' outlines, by id: a point in one is in a wall, not a room.
    walls: Vec<(Id, Vec<P2>)>,
}

/// A level's rooms, in order of id.
type Placed<'a> = Rc<Vec<(Id, &'a Room)>>;

/// The boundaries of a model's rooms, each level's found once for the model.
pub struct Rooms<'a> {
    m: &'a Model,
    index: OnceCell<Index>,
    /// The plans this look asked for, shared with the model's derived answers.
    plans: RefCell<HashMap<(Guid, bool), Arc<Plan>>>,
    /// Each level's rooms, in order of id.
    placed: RefCell<HashMap<Guid, Placed<'a>>>,
    /// Each room's boundary at its walls' centre lines, found once: what [`Rooms::room_at`] asks of many points.
    regions: RefCell<HashMap<Id, Option<Rc<Region>>>>,
}

impl<'a> Rooms<'a> {
    pub fn new(m: &'a Model) -> Self {
        Rooms {
            m,
            index: OnceCell::new(),
            plans: RefCell::new(HashMap::new()),
            placed: RefCell::new(HashMap::new()),
            regions: RefCell::new(HashMap::new()),
        }
    }

    fn plan(&self, level: Guid, at: RoomArea) -> Arc<Plan> {
        let key = (level, at == RoomArea::Finish);
        if let Some(plan) = self.plans.borrow().get(&key) {
            return Arc::clone(plan);
        }
        let made = self.m.derived.room_plan(&self.m.elements, key, || self.find_plan(level, at));
        self.plans.borrow_mut().insert(key, Arc::clone(&made));
        made
    }

    /// The plan itself: the walls' joins and the loops their lines make, found for `level` at `at`.
    fn find_plan(&self, level: Guid, at: RoomArea) -> Plan {
        // The walls' joins through the model's lookup when it is made (its tiles and heights), else an index of every wall.
        let m = self.m;
        let index = match m.derived.made(&m.elements) {
            Some(_) => None,
            None => Some(self.index.get_or_init(|| Index::new(m))),
        };
        let walls: Vec<(Id, Vec<P2>)> = match at {
            RoomArea::Finish => m
                .bounding_walls(&level, true)
                .map(|id| (id, walls::outline_filtered(m, id, index, &|w| m.room_bounding(w, level))))
                .collect(),
            RoomArea::Centre => Vec::new(),
        };
        let lines = match at {
            RoomArea::Finish => {
                let mut lines: Vec<(P2, P2)> = walls.iter().flat_map(|(_, ring)| geom::ring_edges(ring)).collect();
                lines.extend(m.separators(level));
                lines
            }
            // (Room-bounding columns' faces are among them.)
            RoomArea::Centre => m.room_lines_in(level, at, index),
        };
        let lines = match at {
            RoomArea::Finish => {
                let mut lines = lines;
                for o in m.bounding_columns(level) {
                    lines.extend(geom::ring_edges(&o));
                }
                lines
            }
            RoomArea::Centre => lines,
        };
        Plan { faces: geom::faces(&lines, TOL), walls }
    }

    /// Room `r`'s boundary where the project measures rooms; `None` when it isn't enclosed.
    pub fn region(&self, r: &Room) -> Option<Region> {
        self.region_at(r, self.m.room_area)
    }

    /// What room `r` covers in plan ([`Model::room_outline`]), for asking of many rooms.
    pub fn outline(&self, r: &Room) -> (Vec<P2>, bool) {
        match self.region(r) {
            Some(g) => (g.outer, true),
            None => (crate::model::unbounded_square(r.point), false),
        }
    }

    /// Room `r`'s boundary measured at `at`.
    pub fn region_at(&self, r: &Room, at: RoomArea) -> Option<Region> {
        self.bound(r, at).ok()
    }

    /// Room `r`'s boundary measured at `at`, or why it has none ([`Model::room_bound`]): a frozen room's, the one it keeps;
    /// else the loop round its point, unless its point is in a wall or a room placed before it has the space.
    pub fn bound(&self, r: &Room, at: RoomArea) -> Result<Region, Unbound> {
        if let Some(k) = &r.kept {
            return k.at(at).cloned().ok_or(Unbound::NotEnclosed);
        }
        let plan = self.plan(r.level, at);
        if plan.walls.iter().any(|(_, ring)| geom::contains(ring, r.point)) {
            return Err(Unbound::NotEnclosed);
        }
        let g = geom::region_in(&plan.faces, r.point, TOL).ok_or(Unbound::NotEnclosed)?;
        self.before(r, &g).map_or(Ok(g), |id| Err(Unbound::Shared(id)))
    }

    /// The room on `r`'s level placed before it (of a lower id; any, for a room not in the model) whose point lies in `g`
    /// ([`Model::room_before`]).
    pub fn before(&self, r: &Room, g: &Region) -> Option<Id> {
        let rooms = self.placed(r.level);
        let me = rooms.iter().find(|(_, o)| std::ptr::eq(*o, r)).map(|(id, _)| *id);
        let m = self.m;
        rooms
            .iter()
            .take_while(|(id, _)| me.is_none_or(|me| *id < me))
            .find(|(_, o)| m.same(o.level, r.level) && region_holds(g, o.point))
            .map(|(id, _)| *id)
    }

    /// The room on `level` whose boundary at its walls' centre lines holds `point`, the first by id; none where no enclosed
    /// room does. Which room a point is in doesn't follow where the project measures rooms' areas, as in Revit: a point inside
    /// a wall is in the room on its side of the wall's centre line, so a device on a wall's face counts in that room.
    pub fn room_at(&self, level: Guid, point: P2) -> Option<Id> {
        let rooms = self.placed(level);
        rooms.iter().find(|(id, r)| self.cached_region(*id, r).is_some_and(|g| region_holds(&g, point))).map(|(id, _)| *id)
    }

    /// The room on `level` whose volume holds `point`, the first by id: its boundary in plan, as [`Rooms::room_at`], and its
    /// height from its bottom up to its top ([`Model::room_span`]), each end included.
    pub fn room_holding(&self, level: Guid, [x, y, z]: [f64; 3]) -> Option<Id> {
        let rooms = self.placed(level);
        let within = |r: &Room| {
            let (bottom, top) = self.m.room_span(r);
            bottom - TOL <= z && z <= top + TOL
        };
        rooms.iter().find(|(id, r)| within(r) && self.cached_region(*id, r).is_some_and(|g| region_holds(&g, P2(x, y)))).map(|(id, _)| *id)
    }

    /// The room component `c` is in: the one on its level whose volume holds its family's room calculation point, placed
    /// where it stands ([`Model::component_room_point`]); its insertion point's where its family has none. A point over a
    /// room's top or under its bottom (a ceiling luminaire hung above it, a component raised to a mezzanine) is not in it.
    pub fn component(&self, c: &crate::model::Component) -> Option<Id> {
        self.room_holding(c.level, self.m.component_room_point(c))
    }

    fn cached_region(&self, id: Id, r: &Room) -> Option<Rc<Region>> {
        if let Some(g) = self.regions.borrow().get(&id) {
            return g.clone();
        }
        let g = self.region_at(r, RoomArea::Centre).map(Rc::new);
        self.regions.borrow_mut().insert(id, g.clone());
        g
    }

    /// The rooms on `level`, in order of id: what points at it, from the lookup when it is made, else among every element.
    fn placed(&self, level: Guid) -> Placed<'a> {
        if let Some(rooms) = self.placed.borrow().get(&level) {
            return Rc::clone(rooms);
        }
        let m = self.m;
        let room = |id: Id| match m.elements.get(id) {
            Some(Element::Room(o)) if m.same(o.level, level) => Some((id, o)),
            _ => None,
        };
        let rooms: Vec<(Id, &'a Room)> = match m.derived.made(&m.elements) {
            Some(look) => {
                let ids: std::collections::BTreeSet<Id> = look.referrers(&level).collect();
                ids.into_iter().filter_map(room).collect()
            }
            None => m.elements.keys().filter_map(|id| room(*id)).collect(),
        };
        let rooms = Rc::new(rooms);
        self.placed.borrow_mut().insert(level, Rc::clone(&rooms));
        rooms
    }

    /// The loop of `level`'s room-bounding lines, measured at `at`, around `point`, less its islands, whether or not the point
    /// lies in a wall: what a plan's room label measures.
    pub fn loop_at(&self, level: Guid, at: RoomArea, point: P2) -> Option<Region> {
        geom::region_in(&self.plan(level, at).faces, point, TOL)
    }

    /// The room-bounding wall whose body room `r`'s point lies in, measured at `at` (only the finish faces close a wall's
    /// body into a loop of its own).
    pub fn holding(&self, r: &Room, at: RoomArea) -> Option<Id> {
        if at != RoomArea::Finish {
            return None;
        }
        self.plan(r.level, at).walls.iter().find(|(_, ring)| geom::contains(ring, r.point)).map(|(id, _)| *id)
    }
}

// ---------- a room's own floor and ceiling ----------
//
// A room whose type names a floor type or a ceiling type has one of its own, which follows the room's boundary at the
// walls' faces, wherever the project measures rooms. The room is the one source: its boundary is found once and written
// to both. They keep that outline as any floor and ceiling do, so plans, sections, the 3D view, schedules and IFC read it
// with no room found; `follow` brings it up to date after each command, on the levels the command changed only.

/// One of the changes `follow` makes, found while the model is borrowed and made after.
enum Edit {
    /// The element at this id as it is now.
    Put(Id, Element),
    /// A floor or a ceiling made for this room.
    Make(Id, Part, Element),
    /// This room's link to its floor or its ceiling.
    Link(Id, Part, Finish),
    /// A floor or a ceiling its room's type no longer asks for.
    Remove(Id),
}

fn finishes(m: &Model) -> bool {
    m.types.room.values().any(RoomType::finishes)
}

/// The floors and ceilings of rooms brought up to date with what the command that made the model of `before` changed: made
/// for a room whose type asks for one, given the room's boundary at the walls' faces where that differs, taken out where
/// the type asks for none any more. A project whose room types name no floor and no ceiling pays nothing.
pub(crate) fn follow(s: &mut Session, before: &Model) -> Result<(), String> {
    if !finishes(&s.model) && !finishes(before) {
        return Ok(());
    }
    let levels = dirty_levels(&s.model, before);
    follow_levels(s, levels.as_ref())
}

/// [`follow`] of every level, whatever changed: what a file read needs, and what the tests hold `follow` to.
#[cfg(test)]
pub(crate) fn follow_all(s: &mut Session) -> Result<(), String> {
    follow_levels(s, None)
}

/// The levels whose rooms' boundaries, or whose rooms' floors and ceilings, the change from `before` to `m` may have
/// touched; none, every level.
fn dirty_levels(m: &Model, before: &Model) -> Option<BTreeSet<Guid>> {
    // What a boundary at the faces is made of, besides the elements: how thick the walls are and how wide the columns;
    // and what a room's finishes are, its type.
    let (t, u) = (&*m.types, &*before.types);
    if !std::ptr::eq(t, u) && (t.wall != u.wall || t.column != u.column || t.structural_column != u.structural_column || t.room != u.room) {
        return None;
    }
    let Changed::Ids(ids) = m.elements.changed_since(&before.elements) else { return None };
    let levels: Vec<Guid> = m.levels().into_iter().map(|(id, _)| m.guid(id)).collect();
    let mut dirty = BTreeSet::new();
    for id in ids {
        for of in [before, m] {
            let Some(e) = of.elements.get(id) else { continue };
            let plane = |l: &Guid| of.elevation(*l) + of.level(*l).map_or(crate::model::CUT_HEIGHT, |x| x.computation_height);
            match e {
                // A level moved changes which walls bound which rooms.
                Element::Level(_) => return None,
                Element::Wall(w) => dirty.extend(levels.iter().filter(|l| of.bounding(w, **l))),
                Element::Column(c) | Element::StructuralColumn(c) => {
                    let (bottom, top) = of.span(c.level, c.base_offset, c.top_level, c.height);
                    dirty.extend(levels.iter().filter(|l| (bottom..top).contains(&plane(l))));
                }
                Element::RoomSeparator(rs) => {
                    dirty.insert(rs.level);
                }
                Element::Room(r) => {
                    dirty.insert(r.level);
                }
                // A floor or a ceiling: its room's, when it is one's; and a wall attached to a floor or a roof is as tall
                // as that lets it be.
                Element::Floor(_) | Element::Ceiling(_) | Element::Roof(_) => {
                    let Some(g) = of.elements.guid(id) else { continue };
                    for other in of.lookup().referrers(&g) {
                        match of.elements.get(other) {
                            Some(Element::Room(r)) => {
                                dirty.insert(r.level);
                            }
                            Some(Element::Wall(w)) => dirty.extend(levels.iter().filter(|l| of.bounding(w, **l))),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Some(dirty)
}

/// A room's boundary as a floor's or a ceiling's outline and openings: its corners to the micron, each ring from its
/// south-west corner, so that the same boundary is the same points however its lines were arranged; none when a ring
/// isn't a simple outline (walls that meet at a point pinch one), which a floor can't be.
fn outline(g: &Region) -> Option<(Vec<P2>, Vec<Vec<P2>>)> {
    let outer = crate::commands::tidy(&g.outer);
    let holes: Vec<Vec<P2>> = g.holes.iter().map(|h| crate::commands::tidy(&h.iter().rev().copied().collect::<Vec<P2>>())).collect();
    let simple = |ring: &Vec<P2>| {
        ring.len() >= 3 && crate::sketch::solve(&crate::sketch::lines_of(ring, &[])).is_ok_and(|profile| profile.area() > 0.0)
    };
    (simple(&outer) && holes.iter().all(simple)).then_some((outer, holes))
}

fn follow_levels(s: &mut Session, levels: Option<&BTreeSet<Guid>>) -> Result<(), String> {
    let model = Arc::clone(&s.model);
    let m = &*model;
    let levels: Vec<Guid> = match levels {
        Some(l) => l.iter().copied().collect(),
        None => m.levels().into_iter().map(|(id, _)| m.guid(id)).collect(),
    };
    let rooms = Rooms::new(m);
    let mut edits = Vec::new();
    for level in levels {
        for &(id, r) in rooms.placed(level).iter() {
            let rt = m.types.room.get(&r.type_name);
            if rt.is_none_or(|t| !t.finishes()) && r.floor.is_none() && r.ceiling.is_none() {
                continue;
            }
            // Its boundary at the faces, found when a part first asks, once for both.
            let region = OnceCell::new();
            let region = || region.get_or_init(|| rooms.bound(r, RoomArea::Finish).ok().and_then(|g| outline(&g))).as_ref();
            for part in Part::BOTH {
                let want = rt.and_then(|t| t.finish(part));
                let mut link = r.finish(part);
                if let Finish::Connected(g) = link {
                    let is = matches!(
                        (part, m.elements.get(g)),
                        (Part::Floor, Some(Element::Floor(_))) | (Part::Ceiling, Some(Element::Ceiling(_)))
                    );
                    // Deleted, it is let go as Disconnect lets it; a copy of a room names the room's own, and has none yet.
                    let theirs = || {
                        m.lookup()
                            .referrers(&g)
                            .any(|o| o < id && matches!(m.elements.get(o), Some(Element::Room(x)) if x.finish(part) == link))
                    };
                    if !is {
                        link = Finish::Disconnected;
                    } else if theirs() {
                        link = Finish::None;
                    }
                }
                // (A floor or a ceiling made links itself to its room, when it is made.)
                let mut making = false;
                match (link, want) {
                    (Finish::Connected(g), None) => {
                        edits.extend(m.elements.id(g).map(Edit::Remove));
                        link = Finish::None;
                    }
                    (Finish::Disconnected, None) => link = Finish::None,
                    (Finish::None, Some((ty, offset))) => {
                        if let Some((outer, holes)) = region() {
                            edits.push(Edit::Make(id, part, made(r, part, ty, offset, outer.clone(), holes.clone())));
                            making = true;
                        }
                    }
                    (Finish::Connected(g), Some((ty, offset))) => {
                        if let (Some(at), Some(old)) = (m.elements.id(g), m.elements.get(g)) {
                            let new = followed(m, old, r, ty, offset, region());
                            if new != *old {
                                edits.push(Edit::Put(at, new));
                            }
                        }
                    }
                    _ => {}
                }
                if !making && link != r.finish(part) {
                    edits.push(Edit::Link(id, part, link));
                }
            }
        }
    }
    drop(rooms);
    let mut gone = Vec::new();
    for edit in edits {
        let m = s.model_mut();
        match edit {
            Edit::Put(id, e) => {
                m.elements.insert(id, e);
            }
            Edit::Make(room, part, e) => {
                let id = m.insert(e);
                let link = m.elements.guid(id).map_or(Finish::None, Finish::Connected);
                if let Some(Element::Room(r)) = m.elements.get_mut(room) {
                    *r.finish_mut(part) = link;
                }
            }
            Edit::Link(room, part, link) => {
                if let Some(Element::Room(r)) = m.elements.get_mut(room) {
                    *r.finish_mut(part) = link;
                }
            }
            Edit::Remove(id) => gone.push(id),
        }
    }
    if !gone.is_empty() {
        crate::commands::delete_elements(s, &gone)?;
    }
    Ok(())
}

/// A floor or a ceiling for room `r`, of type `ty`, `offset` from its level, in the room's phases and design option.
fn made(r: &Room, part: Part, ty: &str, offset: f64, boundary: Vec<P2>, openings: Vec<Vec<P2>>) -> Element {
    let params = [crate::phasing::CREATED, crate::phasing::DEMOLISHED, crate::phasing::OPTION]
        .into_iter()
        .filter_map(|k| Some((k.to_string(), r.params.get(k)?.clone())))
        .collect();
    match part {
        Part::Floor => Element::Floor(Floor {
            type_name: ty.into(),
            level: r.level,
            boundary,
            openings,
            offset,
            slope: 0.0,
            arrow: None,
            structural: false,
            pattern_origin: P2(0.0, 0.0),
            pattern_angle: 0.0,
            sketch: Vec::new(),
            shape: Default::default(),
            params,
        }),
        Part::Ceiling => {
            Element::Ceiling(Ceiling { type_name: ty.into(), level: r.level, boundary, openings, offset, sketch: Vec::new(), params })
        }
    }
}

/// Room `r`'s floor or ceiling `old` as the room has it now: on its level, of its type's type and offset, and along
/// `region` when the room is bounded (unbounded, its outline stays as it was). A floor's shape keeps what still lies on it.
fn followed(m: &Model, old: &Element, r: &Room, ty: &str, offset: f64, region: Option<&(Vec<P2>, Vec<Vec<P2>>)>) -> Element {
    match old {
        Element::Floor(f) => {
            let mut new = Floor { type_name: ty.into(), level: r.level, offset, ..f.clone() };
            if let Some((outer, holes)) = region.filter(|(outer, holes)| *outer != f.boundary || *holes != f.openings) {
                new.shape = crate::commands::shape_within(m, f, outer, holes).0;
                (new.boundary, new.openings, new.sketch) = (outer.clone(), holes.clone(), Vec::new());
            }
            Element::Floor(new)
        }
        Element::Ceiling(c) => {
            let mut new = Ceiling { type_name: ty.into(), level: r.level, offset, ..c.clone() };
            if let Some((outer, holes)) = region.filter(|(outer, holes)| *outer != c.boundary || *holes != c.openings) {
                (new.boundary, new.openings, new.sketch) = (outer.clone(), holes.clone(), Vec::new());
            }
            Element::Ceiling(new)
        }
        other => other.clone(),
    }
}

impl Model {
    /// The room a floor or a ceiling is the own one of, and which of the two it is: the room it follows.
    pub fn finish_of(&self, k: impl crate::ids::Key) -> Option<(Id, Part)> {
        let g = self.elements.guid(k)?;
        let part = match self.elements.get(g)? {
            Element::Floor(_) => Part::Floor,
            Element::Ceiling(_) => Part::Ceiling,
            _ => return None,
        };
        self.lookup()
            .referrers(&g)
            .find(|id| matches!(self.elements.get(*id), Some(Element::Room(r)) if r.finish(part) == Finish::Connected(g)))
            .map(|id| (id, part))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use crate::model::Element;
    use serde_json::json;

    /// A room's boundary as it was always found: among every wall of the project, its level's lines arranged for it alone,
    /// and the room that has its space before it among every element.
    fn brute(m: &Model, r: &Room, at: RoomArea) -> Result<Region, Unbound> {
        if let Some(k) = &r.kept {
            return k.at(at).cloned().ok_or(Unbound::NotEnclosed);
        }
        if at == RoomArea::Finish {
            let inside = m
                .elements
                .iter()
                .filter(|(_, e)| matches!(e, Element::Wall(w) if m.room_bounding(w, r.level)))
                .any(|(id, _)| geom::contains(&walls::outline_filtered(m, *id, None, &|w| m.room_bounding(w, r.level)), r.point));
            if inside {
                return Err(Unbound::NotEnclosed);
            }
        }
        let g = geom::region_at(&m.room_lines_in(r.level, at, None), r.point, TOL).ok_or(Unbound::NotEnclosed)?;
        let me = m.elements.iter().find(|(_, e)| matches!(e, Element::Room(o) if std::ptr::eq(o, r))).map(|(id, _)| *id);
        let before = m
            .elements
            .iter()
            .take_while(|(id, _)| me.is_none_or(|me| **id < me))
            .find(|(_, e)| matches!(e, Element::Room(o) if m.same(o.level, r.level) && region_holds(&g, o.point)))
            .map(|(id, _)| *id);
        before.map_or(Ok(g), |id| Err(Unbound::Shared(id)))
    }

    /// Every room's boundary, at both measures, as [`Rooms`] finds it and as it was always found.
    pub(crate) fn agree(m: &Model) {
        let rooms = Rooms::new(m);
        for (id, e) in &m.elements {
            let Element::Room(r) = e else { continue };
            for at in [RoomArea::Centre, RoomArea::Finish] {
                assert_eq!(rooms.bound(r, at), brute(m, r, at), "room {id} ({}) at {at:?}", r.name);
            }
        }
    }

    #[test]
    fn a_levels_plan_is_found_once_for_each_model_and_again_after_an_edit() {
        let mut s = Session::new();
        s.script(crate::sample::SAMPLE).unwrap();
        let model = Arc::clone(&s.model);
        let level = model.guid(model.levels()[0].0);
        let a = Rooms::new(&model).plan(level, RoomArea::Finish);
        let b = Rooms::new(&model).plan(level, RoomArea::Finish);
        assert!(Arc::ptr_eq(&a, &b), "a second look at one model finds the plan it found");
        let wall = model.elements.iter().find_map(|(id, e)| matches!(e, Element::Wall(_)).then_some(*id)).expect("a wall");
        s.execute("move", &json!({"ids": [wall], "by": [10.0, 0.0]})).unwrap();
        let c = Rooms::new(&s.model).plan(level, RoomArea::Finish);
        assert!(!Arc::ptr_eq(&a, &c), "an edit's model finds the level's plan again");
        agree(&s.model);
    }

    #[test]
    fn an_edit_lets_go_of_the_plan_its_undo_step_kept() {
        let mut s = Session::new();
        s.script(crate::sample::SAMPLE).unwrap();
        let level = s.model.guid(s.model.levels()[0].0);
        let a = Rooms::new(&s.model).plan(level, RoomArea::Finish);
        let weak = Arc::downgrade(&a);
        drop(a);
        let wall = s.model.elements.iter().find_map(|(id, e)| matches!(e, Element::Wall(_)).then_some(*id)).expect("a wall");
        s.execute("move", &json!({"ids": [wall], "by": [10.0, 0.0]})).unwrap();
        assert!(weak.upgrade().is_none(), "the model in the undo history lets go of its plan");
    }

    #[test]
    fn a_kept_plan_is_made_again_when_the_same_models_elements_change() {
        let mut s = Session::new();
        s.script(crate::sample::SAMPLE).unwrap();
        let mut m = (*s.model).clone();
        let level = m.guid(m.levels()[0].0);
        let a = Rooms::new(&m).plan(level, RoomArea::Finish);
        let wall = m.elements.iter().find_map(|(id, e)| matches!(e, Element::Wall(_)).then_some(*id)).expect("a wall");
        if let Some(Element::Wall(w)) = m.elements.get_mut(wall) {
            w.height += 10.0;
        }
        let b = Rooms::new(&m).plan(level, RoomArea::Finish);
        assert!(!Arc::ptr_eq(&a, &b), "the same model's changed elements find the plan again");
        agree(&m);
    }

    #[test]
    fn a_kept_plan_finds_the_rooms_brute_finds() {
        let mut s = Session::new();
        s.script(crate::sample::SAMPLE).unwrap();
        let level = s.model.guid(s.model.levels()[0].0);
        let _ = Rooms::new(&s.model).plan(level, RoomArea::Finish);
        let _ = Rooms::new(&s.model).plan(level, RoomArea::Centre);
        agree(&s.model);
    }

    #[test]
    fn rooms_found_a_level_at_a_time_are_those_found_one_by_one() {
        let mut s = Session::new();
        s.script(crate::sample::SAMPLE).unwrap();
        agree(&s.model);
        // A room in a wall, at the finish faces; one not enclosed; one on a level of its own.
        s.execute("room_area", &json!({"at": "finish"})).unwrap();
        let m = s.model_mut();
        let level = m.levels()[0].0;
        let level = m.guid(level);
        for point in [(3000.0, 4000.0), (-5000.0, -5000.0), (0.0, 0.0)] {
            m.insert(Element::Room(Room::new(level, P2(point.0, point.1), "Odd", "9")));
        }
        agree(&s.model);
        let tower = crate::sample::tower::tower_on(crate::sample::tower::Plate::SMALL, 2).unwrap();
        agree(&tower);
    }
}
