//! Rooms' boundaries for one look at a model: what a plan, a schedule, `inspect` or an IFC export asks of many rooms.
//!
//! A room's boundary is the smallest loop of its level's room-bounding lines around its point, less the islands inside it
//! ([`geom::region_in`]). Finding those lines takes each bounding wall's joins, and arranging them into loops is the costly
//! part; both are the same for every room of a level. [`Rooms`] does them once a level and a measure, and finds the walls'
//! joins through a plan index ([`walls::Index`]) rather than among every wall of the project. It borrows the model, so
//! what it holds can't go stale: the borrow ends before anything can change the model.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::geom::{self, P2, Region};
use crate::model::{Element, Guid, Id, Model, Room, RoomArea, TOL, Unbound, region_holds};
use crate::walls::{self, Index};

/// What a level's rooms are found among, at one measure.
struct Plan {
    /// The loops its room-bounding lines make.
    faces: Vec<Vec<P2>>,
    /// At the finish faces, its bounding walls' outlines, by id: a point in one is in a wall, not a room.
    walls: Vec<(Id, Vec<P2>)>,
}

/// A level's rooms, in order of id.
type Placed<'a> = Rc<Vec<(Id, &'a Room)>>;

/// The boundaries of a model's rooms, each level's found once.
pub struct Rooms<'a> {
    m: &'a Model,
    index: OnceCell<Index>,
    plans: RefCell<HashMap<(Guid, bool), Rc<Plan>>>,
    /// Each level's rooms, in order of id.
    placed: RefCell<HashMap<Guid, Placed<'a>>>,
}

impl<'a> Rooms<'a> {
    pub fn new(m: &'a Model) -> Self {
        Rooms { m, index: OnceCell::new(), plans: RefCell::new(HashMap::new()), placed: RefCell::new(HashMap::new()) }
    }

    fn plan(&self, level: Guid, at: RoomArea) -> Rc<Plan> {
        let key = (level, at == RoomArea::Finish);
        if let Some(plan) = self.plans.borrow().get(&key) {
            return Rc::clone(plan);
        }
        // The walls' joins through the model's lookup when it is made (its tiles and heights), else an index of every wall.
        let m = self.m;
        let index = match m.derived.made(&m.elements) {
            Some(_) => None,
            None => Some(self.index.get_or_init(|| Index::new(m))),
        };
        let walls: Vec<(Id, Vec<P2>)> = match at {
            RoomArea::Finish => m.bounding_walls(&level, true).map(|id| (id, walls::outline_in(m, id, index))).collect(),
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
        let plan = Rc::new(Plan { faces: geom::faces(&lines, TOL), walls });
        self.plans.borrow_mut().insert(key, Rc::clone(&plan));
        plan
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
                .any(|(id, _)| geom::contains(&walls::outline_in(m, *id, None), r.point));
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
