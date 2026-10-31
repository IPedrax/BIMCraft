//! What a model's elements point at, looked up the other way: kept with each model, and brought up to date from the model
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
}

impl Lookup {
    /// Of every element of `elements`.
    pub fn of(elements: &Elements) -> Lookup {
        let mut l = Lookup::default();
        for (&id, e) in elements {
            l.put(id, e);
        }
        l
    }

    /// This one, of the elements `before` was of, brought up to `elements` where `ids` changed.
    fn updated(&self, elements: &Elements, ids: &BTreeSet<Id>) -> Lookup {
        let mut l = self.clone();
        for &id in ids {
            l.take(id);
            if let Some(e) = elements.get(id) {
                l.put(id, e);
            }
        }
        l
    }

    fn put(&mut self, id: Id, e: &Element) {
        let refs = e.refs();
        for g in &refs {
            Arc::make_mut(self.referrers.entry(bucket(g)).or_default()).entry(*g).or_default().insert(id);
        }
        if !refs.is_empty() {
            Arc::make_mut(self.refs.entry(id >> SHIFT).or_default()).insert(id, refs);
        }
        if matches!(e, Element::Wall(_)) {
            Arc::make_mut(&mut self.walls).insert(id);
        }
    }

    fn take(&mut self, id: Id) {
        if self.walls.contains(&id) {
            Arc::make_mut(&mut self.walls).remove(&id);
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
}

impl Derived {
    fn current(&self) -> Option<(u64, u64, Arc<Lookup>)> {
        self.now.lock().ok().and_then(|now| now.clone())
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
        Derived { now: Mutex::new(None), parent: self.current() }
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
            // Brought up to date, not made again: it shares what didn't change (a sweep, as delete's, makes it again).
            assert!(cmd == "delete" || kept.shares(&was), "after {cmd}");
        }
        // What points at a wall: its doors, windows and tags.
        let guid = s.model.elements.guid(wall).unwrap();
        let referrers: Vec<&str> =
            s.model.lookup().referrers(&guid).filter_map(|id| s.model.elements.get(id)).map(Element::category).collect();
        assert!(referrers.iter().all(|c| ["door", "window", "tag", "dimension", "room"].contains(c)), "{referrers:?}");
    }
}
