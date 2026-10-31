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
use crate::model::{Guid, Id, Model, Room, RoomArea, TOL};
use crate::walls::{self, Index};

/// What a level's rooms are found among, at one measure.
struct Plan {
    /// The loops its room-bounding lines make.
    faces: Vec<Vec<P2>>,
    /// At the finish faces, its bounding walls' outlines, by id: a point in one is in a wall, not a room.
    walls: Vec<(Id, Vec<P2>)>,
}

/// The boundaries of a model's rooms, each level's found once.
pub struct Rooms<'a> {
    m: &'a Model,
    index: OnceCell<Index>,
    plans: RefCell<HashMap<(Guid, bool), Rc<Plan>>>,
}

impl<'a> Rooms<'a> {
    pub fn new(m: &'a Model) -> Self {
        Rooms { m, index: OnceCell::new(), plans: RefCell::new(HashMap::new()) }
    }

    fn plan(&self, level: Guid, at: RoomArea) -> Rc<Plan> {
        let key = (level, at == RoomArea::Finish);
        if let Some(plan) = self.plans.borrow().get(&key) {
            return Rc::clone(plan);
        }
        let (m, index) = (self.m, self.index.get_or_init(|| Index::new(self.m)));
        let walls: Vec<(Id, Vec<P2>)> = match at {
            RoomArea::Finish => m.bounding_walls(&level).map(|id| (id, walls::outline_in(m, id, Some(index)))).collect(),
            RoomArea::Centre => Vec::new(),
        };
        let lines = match at {
            RoomArea::Finish => {
                let mut lines: Vec<(P2, P2)> = walls.iter().flat_map(|(_, ring)| geom::ring_edges(ring)).collect();
                lines.extend(m.separators(level));
                lines
            }
            RoomArea::Centre => m.room_lines_in(level, at, Some(index)),
        };
        let plan = Rc::new(Plan { faces: geom::faces(&lines, TOL), walls });
        self.plans.borrow_mut().insert(key, Rc::clone(&plan));
        plan
    }

    /// Room `r`'s boundary where the project measures rooms; `None` when it isn't enclosed.
    pub fn region(&self, r: &Room) -> Option<Region> {
        self.region_at(r, self.m.room_area)
    }

    /// Room `r`'s boundary measured at `at`.
    pub fn region_at(&self, r: &Room, at: RoomArea) -> Option<Region> {
        let plan = self.plan(r.level, at);
        if plan.walls.iter().any(|(_, ring)| geom::contains(ring, r.point)) {
            return None;
        }
        geom::region_in(&plan.faces, r.point, TOL)
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

    /// A room's boundary as it was always found: among every wall of the project, its level's lines arranged for it alone.
    fn brute(m: &Model, r: &Room, at: RoomArea) -> Option<Region> {
        if at == RoomArea::Finish {
            let inside = m.bounding_walls(&r.level).any(|id| geom::contains(&walls::outline_in(m, id, None), r.point));
            if inside {
                return None;
            }
        }
        geom::region_at(&m.room_lines_in(r.level, at, None), r.point, TOL)
    }

    /// Every room's boundary, at both measures, as [`Rooms`] finds it and as it was always found.
    pub(crate) fn agree(m: &Model) {
        let rooms = Rooms::new(m);
        for (id, e) in &m.elements {
            let Element::Room(r) = e else { continue };
            for at in [RoomArea::Centre, RoomArea::Finish] {
                assert_eq!(rooms.region_at(r, at), brute(m, r, at), "room {id} ({}) at {at:?}", r.name);
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
            m.insert(Element::Room(Room {
                level,
                point: P2(point.0, point.1),
                name: "Odd".into(),
                number: "9".into(),
                params: Default::default(),
            }));
        }
        agree(&s.model);
        let tower = crate::sample::tower::tower_on(crate::sample::tower::Plate::SMALL, 2).unwrap();
        agree(&tower);
    }
}
