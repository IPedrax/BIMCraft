//! Every element's two names, as Revit's: its id, a number from [`FIRST_ID`] up that people and agents type and schedules
//! show, which a project gives anew to an element that comes in from another file when its own is taken; and its GUID,
//! made once and never changed, which is what everything that points at an element holds (a wall its level, a door its
//! wall, a dimension what it measures, a viewport its view), so a new id never breaks a reference.
//!
//! A GUID read where a reference is given may be a number instead: the id of the element meant, until the model swaps it
//! for that element's GUID ([`Guid::of_id`], `Model::resolve`). That is how a command takes `"level": 100000`, and how a
//! file from before GUIDs opens.

use std::collections::{BTreeMap, BTreeSet, HashMap, btree_map};
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::model::Element;

pub type Id = u64;

/// The first id a project gives: ids have six digits, and more once a project has made 900 000 elements.
pub const FIRST_ID: Id = 100_000;

/// Where the ids of linked elements begin, in the model the views draw ([`crate::link::seen`]): past any a project makes.
pub const LINKED: Id = 100_000_000;

mod revisions;
use revisions::Revised;
pub use revisions::{Revision, RevisionSnapshot};

/// An element's GUID: 128 bits, random (a version 4 UUID), written `xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx`. One whose top
/// half is zero, which a random one never is, stands for the element with the id in its bottom half, until resolved.
/// Its default, the nil GUID, points at nothing: [`Model::check`](crate::model::Model::check) says so.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Guid(u128);

impl Guid {
    /// A new GUID, from the system's random numbers (and, should they fail, the clock, the process and a count, mixed).
    #[allow(clippy::new_without_default)]
    pub fn new() -> Guid {
        let mut b = [0u8; 16];
        if getrandom::fill(&mut b).is_err() {
            static COUNT: AtomicU64 = AtomicU64::new(0);
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
            let n = COUNT.fetch_add(1, Ordering::Relaxed);
            let hi = crate::model::mix(nanos ^ u64::from(std::process::id()).rotate_left(32));
            let lo = crate::model::mix(hi ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            b = ((u128::from(hi) << 64) | u128::from(lo)).to_be_bytes();
        }
        // Version 4, variant 10.
        b[6] = (b[6] & 0x0F) | 0x40;
        b[8] = (b[8] & 0x3F) | 0x80;
        Guid(u128::from_be_bytes(b))
    }

    /// The GUID `seed` and `n` make, the same every time: for what a file reads in that the file gives none.
    pub fn seeded(seed: u64, n: u64) -> Guid {
        let hi = crate::model::mix(seed ^ crate::model::mix(n));
        let lo = crate::model::mix(hi ^ n.rotate_left(29));
        let mut b = ((u128::from(hi) << 64) | u128::from(lo)).to_be_bytes();
        b[6] = (b[6] & 0x0F) | 0x40;
        b[8] = (b[8] & 0x3F) | 0x80;
        Guid(u128::from_be_bytes(b))
    }

    /// The element whose id is `id`, until the model puts its GUID in its place.
    pub const fn of_id(id: Id) -> Guid {
        Guid(id as u128)
    }

    /// The id it stands for, if it isn't a GUID yet.
    pub fn pending(self) -> Option<Id> {
        (self.0 >> 64 == 0).then_some(self.0 as u64)
    }

    /// Whether it is the nil GUID: what a part not yet given one holds.
    pub fn is_nil(&self) -> bool {
        self.0 == 0
    }

    pub const fn from_u128(n: u128) -> Guid {
        Guid(n)
    }

    pub const fn as_u128(self) -> u128 {
        self.0
    }

    /// `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` in either case, braces or not; or digits, an id.
    pub fn parse(s: &str) -> Option<Guid> {
        let s = s.trim();
        if !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit()) {
            return s.parse().ok().map(Guid::of_id);
        }
        let s = s.strip_prefix('{').and_then(|s| s.strip_suffix('}')).unwrap_or(s);
        let groups: Vec<&str> = s.split('-').collect();
        if groups.iter().map(|g| g.len()).collect::<Vec<_>>() != [8, 4, 4, 4, 12] {
            return None;
        }
        let hex: String = groups.concat();
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        u128::from_str_radix(&hex, 16).ok().map(Guid).filter(|g| g.pending().is_none())
    }
}

impl fmt::Display for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(id) = self.pending() {
            return write!(f, "{id}");
        }
        let h = format!("{:032x}", self.0);
        write!(f, "{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
    }
}

impl fmt::Debug for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Serialize for Guid {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.pending() {
            Some(id) => s.serialize_u64(id),
            None => s.collect_str(self),
        }
    }
}

impl<'de> Deserialize<'de> for Guid {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Guid, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = Guid;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an element's GUID, or its id")
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Guid, E> {
                Ok(Guid::of_id(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Guid, E> {
                u64::try_from(v).map(Guid::of_id).map_err(|_| E::custom(format!("{v} is not an element id")))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Guid, E> {
                // A whole number JSON wrote with a point.
                if v.fract() == 0.0 && (0.0..9e15).contains(&v) {
                    Ok(Guid::of_id(v as u64))
                } else {
                    Err(E::custom(format!("{v} is not an element id")))
                }
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Guid, E> {
                Guid::parse(v).ok_or_else(|| E::custom(format!("`{v}` is not a GUID (xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx) or an id")))
            }
        }
        d.deserialize_any(V)
    }
}

/// Whether `id` is none yet: what a part not yet given one holds.
pub fn unset(id: &Id) -> bool {
    *id == 0
}

/// What an element may be found by: its id or its GUID.
pub trait Key: Copy + fmt::Display {
    fn id_in(&self, e: &Elements) -> Option<Id>;
}

impl Key for Id {
    fn id_in(&self, _: &Elements) -> Option<Id> {
        Some(*self)
    }
}

impl Key for Guid {
    fn id_in(&self, e: &Elements) -> Option<Id> {
        e.guid_id(self).or_else(|| self.pending())
    }
}

impl<K: Key> Key for &K {
    fn id_in(&self, e: &Elements) -> Option<Id> {
        (*self).id_in(e)
    }
}

/// A project's elements by id, each with its GUID. In a file, an object of ids, each element with its `guid`.
///
/// They are held in chunks of [`CHUNK`] ids, each shared between the copies of a model until one of them changes it: a
/// command's copy of a model costs a pointer a chunk, and its change, the chunks it touches.
///
/// Each keeps a journal of what changed in it since it was cloned from another ([`Elements::changed_since`]): a command
/// changes a clone of the model it found, so the journal of the model a command leaves is what the command changed.
#[derive(Debug, Default)]
pub struct Elements {
    revisions: RevisionSnapshot,
    chunks: BTreeMap<Id, Arc<Chunk>>,
    len: usize,
    /// Each GUID's id, in buckets by its first bits.
    ids: BTreeMap<u16, Arc<HashMap<Guid, Id>>>,
    /// This collection, and the one it was cloned from (0: none), among all made in this process.
    token: u64,
    base: u64,
    /// How many changes it has had, and the one it was cloned from had then.
    version: u64,
    base_version: u64,
    journal: Journal,
}

/// Ids a chunk holds: 2^10.
const SHIFT: u32 = 10;
pub const CHUNK: Id = 1 << SHIFT;

/// A chunk's elements, each with its GUID.
type Chunk = BTreeMap<Id, (Guid, Element)>;

fn chunk_of(id: Id) -> Id {
    id >> SHIFT
}

fn bucket(g: &Guid) -> u16 {
    (g.0 >> 116) as u16
}

/// Nothing to find: what `elements[id]` looks in for an id there's no element of, to say so as a map would.
static NONE: BTreeMap<Id, Element> = BTreeMap::new();

/// What changed in an [`Elements`] since it was cloned: elements by id, those from an id on, or everything.
#[derive(Clone, Debug, Default)]
struct Journal {
    ids: BTreeSet<Id>,
    from: Option<Id>,
    all: bool,
}

/// What changed from one model's elements to another's: the ids of those added, changed or removed, or everything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Changed {
    Ids(BTreeSet<Id>),
    All,
}

fn token() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl Clone for Elements {
    /// The same elements, sharing their chunks, with a journal of their own, empty.
    fn clone(&self) -> Elements {
        Elements {
            chunks: self.chunks.clone(),
            len: self.len,
            ids: self.ids.clone(),
            token: token(),
            base: self.token,
            version: 0,
            base_version: self.version,
            journal: Journal::default(),
            revisions: self.revisions.clone(),
        }
    }
}

impl PartialEq for Elements {
    fn eq(&self, other: &Elements) -> bool {
        self.len == other.len
            && self.chunks.len() == other.chunks.len()
            && self.chunks.iter().zip(&other.chunks).all(|((a, x), (b, y))| a == b && (Arc::ptr_eq(x, y) || x == y))
    }
}

type Pairs<'a> = std::iter::Map<btree_map::Iter<'a, Id, (Guid, Element)>, fn((&'a Id, &'a (Guid, Element))) -> (&'a Id, &'a Element)>;
type PairsMut<'a> =
    std::iter::Map<btree_map::IterMut<'a, Id, (Guid, Element)>, fn((&'a Id, &'a mut (Guid, Element))) -> (&'a Id, &'a mut Element)>;

fn pair<'a>((id, (_, e)): (&'a Id, &'a (Guid, Element))) -> (&'a Id, &'a Element) {
    (id, e)
}

fn pair_mut<'a>((id, (_, e)): (&'a Id, &'a mut (Guid, Element))) -> (&'a Id, &'a mut Element) {
    (id, e)
}

fn pairs(c: &Arc<Chunk>) -> Pairs<'_> {
    c.iter().map(pair as fn(_) -> _)
}

fn pairs_mut(c: &mut Arc<Chunk>) -> PairsMut<'_> {
    Arc::make_mut(c).iter_mut().map(pair_mut as fn(_) -> _)
}

/// The elements in id order, with their ids.
pub type Iter<'a> = std::iter::FlatMap<btree_map::Values<'a, Id, Arc<Chunk>>, Pairs<'a>, fn(&'a Arc<Chunk>) -> Pairs<'a>>;
/// The same, to change.
type InnerMut<'a> = std::iter::FlatMap<btree_map::ValuesMut<'a, Id, Arc<Chunk>>, PairsMut<'a>, fn(&'a mut Arc<Chunk>) -> PairsMut<'a>>;
pub type IterMut<'a> = Revised<'a, InnerMut<'a>>;

impl Elements {
    pub fn new() -> Elements {
        Elements { token: token(), ..Elements::default() }
    }

    /// What changed from `before`'s elements to these. When these were cloned from those: the ids the journal names, or
    /// where something went through them all (`values_mut`, `retain`), those whose elements or GUIDs differ. When the
    /// two share most of their chunks still (an undo, a redo: one model of the history for another), those that differ in
    /// the chunks they don't. Else everything: these were made another way.
    pub fn changed_since(&self, before: &Elements) -> Changed {
        let j = &self.journal;
        if self.token == 0 || self.base != before.token {
            let shared = self.chunks.iter().filter(|(k, c)| before.chunks.get(*k).is_some_and(|d| Arc::ptr_eq(c, d))).count();
            let total = self.chunks.len().max(before.chunks.len());
            // A few chunks are quickly compared; many, mostly not shared, are another model's.
            if total > 2 && shared * 2 < total {
                return Changed::All;
            }
            return Changed::Ids(self.differ(before, 0));
        }
        if j.all {
            return Changed::Ids(self.differ(before, 0));
        }
        let mut ids = j.ids.clone();
        if let Some(from) = j.from {
            ids.extend(self.differ(before, from));
        }
        Changed::Ids(ids)
    }

    /// The ids from `from` on whose elements or GUIDs differ between these and `before`, or that only one has. A chunk
    /// both still share is the same.
    fn differ(&self, before: &Elements, from: Id) -> BTreeSet<Id> {
        let mut out = BTreeSet::new();
        let keys: BTreeSet<Id> =
            self.chunks.range(chunk_of(from)..).chain(before.chunks.range(chunk_of(from)..)).map(|(k, _)| *k).collect();
        for k in keys {
            let (a, b) = (self.chunks.get(&k), before.chunks.get(&k));
            if let (Some(a), Some(b)) = (a, b)
                && Arc::ptr_eq(a, b)
            {
                continue;
            }
            let empty = Chunk::new();
            let (a, b) = (a.map_or(&empty, |c| &**c), b.map_or(&empty, |c| &**c));
            for (id, x) in a.range(from..) {
                if b.get(id) != Some(x) {
                    out.insert(*id);
                }
            }
            for id in b.range(from..).map(|(id, _)| id) {
                if !a.contains_key(id) {
                    out.insert(*id);
                }
            }
        }
        out
    }

    /// The ids changed since these were cloned, when the journal names each (none: it can't say which).
    pub fn touched(&self) -> Option<BTreeSet<Id>> {
        let j = &self.journal;
        (self.token != 0 && self.base != 0 && !j.all && j.from.is_none()).then(|| j.ids.clone())
    }

    /// This collection and how many changes it has had: what something derived from it is of.
    pub fn state(&self) -> (u64, u64) {
        (self.token, self.version)
    }

    /// The element's runtime revision, independent of its id and GUID.
    pub fn revision(&self, key: impl Key) -> Option<Revision> {
        self.revisions.get(self.id(key)?)
    }

    /// Retain change metadata without retaining any element objects or an entire model.
    pub fn revision_snapshot(&self) -> RevisionSnapshot {
        self.revisions.clone()
    }

    /// The collection it was cloned from, as it was then.
    pub fn base(&self) -> (u64, u64) {
        (self.base, self.base_version)
    }

    fn touch(&mut self, id: Id) {
        self.version += 1;
        self.journal.ids.insert(id);
        self.revisions.touch(id, Revision::fresh());
    }

    fn touch_all(&mut self) {
        self.version += 1;
        self.journal.all = true;
    }

    fn slot(&self, id: Id) -> Option<&(Guid, Element)> {
        self.chunks.get(&chunk_of(id))?.get(&id)
    }

    fn slot_mut(&mut self, id: Id) -> Option<&mut (Guid, Element)> {
        let c = self.chunks.get_mut(&chunk_of(id))?;
        if !c.contains_key(&id) {
            return None;
        }
        Arc::make_mut(c).get_mut(&id)
    }

    fn guid_id(&self, g: &Guid) -> Option<Id> {
        self.ids.get(&bucket(g))?.get(g).copied()
    }

    fn set_guid(&mut self, g: Guid, id: Id) {
        Arc::make_mut(self.ids.entry(bucket(&g)).or_default()).insert(g, id);
    }

    fn unset_guid(&mut self, g: &Guid) {
        if let Some(b) = self.ids.get_mut(&bucket(g)) {
            Arc::make_mut(b).remove(g);
            if b.is_empty() {
                self.ids.remove(&bucket(g));
            }
        }
    }

    pub fn get(&self, k: impl Key) -> Option<&Element> {
        self.slot(k.id_in(self)?).map(|(_, e)| e)
    }

    pub fn get_mut(&mut self, k: impl Key) -> Option<&mut Element> {
        let id = k.id_in(self)?;
        if self.slot(id).is_some() {
            self.touch(id);
        }
        self.slot_mut(id).map(|(_, e)| e)
    }

    pub fn contains_key(&self, k: impl Key) -> bool {
        k.id_in(self).is_some_and(|id| self.slot(id).is_some())
    }

    /// The id of the element `k` finds.
    pub fn id(&self, k: impl Key) -> Option<Id> {
        k.id_in(self).filter(|id| self.slot(*id).is_some())
    }

    /// The GUID of the element `k` finds.
    pub fn guid(&self, k: impl Key) -> Option<Guid> {
        self.slot(k.id_in(self)?).map(|(g, _)| *g)
    }

    /// Puts `e` at `id`. An element already there keeps its GUID, so this is how an element changes; a new one gets one of its own.
    pub fn insert(&mut self, id: Id, e: Element) -> Option<Element> {
        self.touch(id);
        if let Some(slot) = self.slot_mut(id) {
            return Some(std::mem::replace(&mut slot.1, e));
        }
        let g = self.fresh();
        self.set_guid(g, id);
        Arc::make_mut(self.chunks.entry(chunk_of(id)).or_default()).insert(id, (g, e));
        self.len += 1;
        None
    }

    /// Puts `e` at `id` with GUID `guid`, or an error if another element has that GUID.
    pub fn insert_as(&mut self, id: Id, guid: Guid, e: Element) -> Result<(), String> {
        if guid.pending().is_some() {
            return Err(format!("element {id}: {guid} is an id, not a GUID"));
        }
        match self.guid_id(&guid) {
            Some(other) if other != id => return Err(format!("elements {other} and {id} have the same GUID {guid}")),
            _ => {}
        }
        self.touch(id);
        if let Some(old) = self.slot(id).map(|(g, _)| *g) {
            self.unset_guid(&old);
        } else {
            self.len += 1;
        }
        self.set_guid(guid, id);
        Arc::make_mut(self.chunks.entry(chunk_of(id)).or_default()).insert(id, (guid, e));
        Ok(())
    }

    /// A GUID no element here has.
    pub fn fresh(&self) -> Guid {
        loop {
            let g = Guid::new();
            if self.guid_id(&g).is_none() {
                return g;
            }
        }
    }

    pub fn remove(&mut self, k: impl Key) -> Option<Element> {
        let id = k.id_in(self)?;
        let key = chunk_of(id);
        let c = self.chunks.get_mut(&key)?;
        if !c.contains_key(&id) {
            return None;
        }
        self.touch(id);
        let c = self.chunks.get_mut(&key)?;
        let (g, e) = Arc::make_mut(c).remove(&id)?;
        if c.is_empty() {
            self.chunks.remove(&key);
        }
        self.len -= 1;
        self.revisions.remove(id);
        self.unset_guid(&g);
        Some(e)
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&Id, &mut Element) -> bool) {
        self.touch_all();
        let gone: Vec<Id> = self.iter_mut().filter_map(|(id, e)| (!keep(id, e)).then_some(*id)).collect();
        for id in gone {
            self.remove(id);
        }
    }

    pub fn clear(&mut self) {
        let (token, base, version, base_version) = (self.token, self.base, self.version, self.base_version);
        *self = Elements { token, base, version, base_version, ..Elements::default() };
        self.touch_all();
    }

    pub fn iter(&self) -> Iter<'_> {
        self.chunks.values().flat_map(pairs as fn(_) -> _)
    }

    pub fn iter_mut(&mut self) -> IterMut<'_> {
        self.touch_all();
        Revised::new(self.chunks.values_mut().flat_map(pairs_mut as fn(_) -> _), &mut self.revisions)
    }

    pub fn values(&self) -> impl DoubleEndedIterator<Item = &Element> {
        self.iter().map(|(_, e)| e)
    }

    pub fn values_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut Element> {
        self.iter_mut().map(|(_, e)| e)
    }

    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &Id> {
        self.iter().map(|(id, _)| id)
    }

    pub fn range(&self, r: std::ops::RangeFrom<Id>) -> impl DoubleEndedIterator<Item = (&Id, &Element)> {
        let from = r.start;
        self.chunks.range(chunk_of(from)..).flat_map(move |(_, c)| c.range(from..).map(pair as fn(_) -> _))
    }

    pub fn range_mut(&mut self, r: std::ops::RangeFrom<Id>) -> impl DoubleEndedIterator<Item = (&Id, &mut Element)> {
        let from = r.start;
        self.version += 1;
        self.journal.from = Some(self.journal.from.map_or(from, |f| f.min(from)));
        Revised::new(
            self.chunks.range_mut(chunk_of(from)..).flat_map(move |(_, c)| Arc::make_mut(c).range_mut(from..).map(pair_mut as fn(_) -> _)),
            &mut self.revisions,
        )
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Every element with its id and its GUID.
    pub fn with_guids(&self) -> impl Iterator<Item = (Id, Guid, &Element)> {
        self.chunks.values().flat_map(|c| c.iter().map(|(id, (g, e))| (*id, *g, e)))
    }

    /// The same elements, each whose id `wanted` lists with the GUID it gives (where that is a GUID no other takes first),
    /// the rest with their own.
    pub fn regiven(&self, wanted: &HashMap<Id, u128>) -> Elements {
        let mut out = Elements::new();
        let given: Vec<(Id, Guid)> = self
            .with_guids()
            .filter_map(|(id, _, _)| Some((id, Guid::from_u128(*wanted.get(&id)?))))
            .filter(|(_, g)| g.pending().is_none())
            .collect();
        for (id, g) in given {
            if let Some(e) = self.get(id)
                && out.guid_id(&g).is_none()
            {
                let _ = out.insert_as(id, g, e.clone());
            }
        }
        for (id, g, e) in self.with_guids() {
            if !out.contains_key(id) {
                let g = if out.guid_id(&g).is_some() { out.fresh() } else { g };
                let _ = out.insert_as(id, g, e.clone());
            }
        }
        out
    }

    /// The same elements under other ids, `to(id)` for each, their GUIDs theirs still; none if two would share one.
    pub fn renumbered(&self, to: impl Fn(Id) -> Id) -> Option<Elements> {
        let mut out = Elements::new();
        for (id, g, e) in self.with_guids() {
            if out.contains_key(to(id)) {
                return None;
            }
            out.insert_as(to(id), g, e.clone()).ok()?;
        }
        Some(out)
    }
}

/// `elements[id]`: the element, as a map's index finds it (so it panics where there's none, as one does).
impl<K: Key> std::ops::Index<K> for Elements {
    type Output = Element;
    fn index(&self, k: K) -> &Element {
        let id = k.id_in(self).unwrap_or_default();
        self.get(id).unwrap_or_else(|| &NONE[&id])
    }
}

impl<'a> IntoIterator for &'a Elements {
    type Item = (&'a Id, &'a Element);
    type IntoIter = Iter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> IntoIterator for &'a mut Elements {
    type Item = (&'a Id, &'a mut Element);
    type IntoIter = IterMut<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl Serialize for Elements {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct With<'a> {
            guid: Guid,
            #[serde(flatten)]
            e: &'a Element,
        }
        s.collect_map(self.with_guids().map(|(id, guid, e)| (id.to_string(), With { guid, e })))
    }
}

impl<'de> Deserialize<'de> for Elements {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Elements, D::Error> {
        let raw = BTreeMap::<String, serde_json::Value>::deserialize(d)?;
        let mut out = Elements::new();
        for (k, mut v) in raw {
            let id: Id = k.parse().map_err(|_| de::Error::custom(format!("`{k}` isn't an element id")))?;
            // A file from before GUIDs has none: each element gets one as it's read.
            let guid = match v.as_object_mut().and_then(|o| o.remove("guid")) {
                Some(g) => Some(Guid::deserialize(g).map_err(|e| de::Error::custom(format!("element {id}: {e}")))?),
                None => None,
            };
            let e = Element::deserialize(v).map_err(|e| de::Error::custom(format!("element {id}: {e}")))?;
            let guid = guid.unwrap_or_else(|| out.fresh());
            out.insert_as(id, guid, e).map_err(de::Error::custom)?;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::P2;
    use crate::model::Grid;

    fn grid() -> Element {
        Element::Grid(Grid { name: "A".into(), type_name: "Grid - ISO 128".into(), start: P2(0.0, 0.0), end: P2(0.0, 1000.0) })
    }

    #[test]
    fn a_guid_is_a_version_4_uuid_and_reads_back_as_it_writes() {
        let g = Guid::new();
        let s = g.to_string();
        assert_eq!(s.len(), 36);
        assert_eq!(&s[14..15], "4", "version 4: {s}");
        assert!("89ab".contains(&s[19..20]), "variant 10: {s}");
        assert_eq!(Guid::parse(&s), Some(g));
        assert_eq!(Guid::parse(&s.to_uppercase()), Some(g), "either case");
        assert_eq!(Guid::parse(&format!("{{{s}}}")), Some(g), "braces");
        assert_eq!(serde_json::from_value::<Guid>(serde_json::to_value(g).unwrap()).unwrap(), g);
        assert_ne!(Guid::new(), g, "each is new");
        assert_eq!(g.pending(), None);
        for bad in ["", "x", "00000000-0000-0000-0000-000000000005", "zzzzzzzz-zzzz-zzzz-zzzz-zzzzzzzzzzzz", &s[1..]] {
            assert_eq!(Guid::parse(bad), None, "{bad}");
        }
        assert_eq!(Guid::parse("1234"), Some(Guid::of_id(1234)), "digits are an id");
    }

    #[test]
    fn a_number_where_a_guid_goes_is_the_element_with_that_id_until_resolved() {
        let g: Guid = serde_json::from_value(serde_json::json!(100004)).unwrap();
        assert_eq!(g.pending(), Some(100004));
        assert_eq!(serde_json::to_value(g).unwrap(), serde_json::json!(100004), "it writes as it came");
        assert!(serde_json::from_value::<Guid>(serde_json::json!(-3)).is_err());
        assert!(serde_json::from_value::<Guid>(serde_json::json!(2.5)).is_err());
        assert!(serde_json::from_value::<Guid>(serde_json::json!([1])).is_err());
    }

    #[test]
    fn elements_are_found_by_id_or_guid_and_keep_their_guid_when_changed() {
        let mut es = Elements::new();
        es.insert(FIRST_ID, grid());
        let g = es.guid(FIRST_ID).unwrap();
        assert!(es.get(g).is_some() && es.get(FIRST_ID).is_some() && es.get(Guid::of_id(FIRST_ID)).is_some());
        es.insert(FIRST_ID, grid());
        assert_eq!(es.guid(FIRST_ID), Some(g), "a change keeps it");
        es.insert(FIRST_ID + 1, grid());
        assert_ne!(es.guid(FIRST_ID + 1), Some(g));
        assert!(es.insert_as(FIRST_ID + 2, g, grid()).unwrap_err().contains("same GUID"));
        assert!(es.insert_as(FIRST_ID + 2, Guid::of_id(5), grid()).is_err());
        es.remove(g);
        assert!(es.get(g).is_none() && es.get(FIRST_ID).is_none() && es.len() == 1);
        assert_eq!(es.id(g), None);
    }

    #[test]
    fn elements_write_their_guids_and_a_file_without_them_gets_new_ones() {
        let mut es = Elements::new();
        es.insert(FIRST_ID, grid());
        let v = serde_json::to_value(&es).unwrap();
        assert_eq!(v["100000"]["guid"], serde_json::json!(es.guid(FIRST_ID).unwrap().to_string()));
        assert_eq!(v["100000"]["category"], "grid");
        assert_eq!(serde_json::from_value::<Elements>(v.clone()).unwrap(), es);
        let mut old = v.clone();
        old["100000"].as_object_mut().unwrap().remove("guid");
        let read: Elements = serde_json::from_value(old).unwrap();
        assert!(read.guid(FIRST_ID).is_some_and(|g| g.pending().is_none()));
        let mut twins = v.clone();
        twins["100001"] = v["100000"].clone();
        let e = serde_json::from_value::<Elements>(twins).unwrap_err().to_string();
        assert!(e.contains("same GUID"), "{e}");
        let mut typo = v;
        typo["100000"]["nmae"] = serde_json::json!("x");
        assert!(serde_json::from_value::<Elements>(typo).unwrap_err().to_string().contains("nmae"));
    }
}

#[cfg(test)]
mod journal_tests {
    use super::*;
    use crate::engine::Session;
    use crate::model::Model;
    use serde_json::json;
    use std::sync::Arc;

    /// What the command `cmd` changed, as the model it left says.
    fn changed(s: &mut Session, cmd: &str, p: serde_json::Value) -> Changed {
        let before = Arc::clone(&s.model);
        s.execute(cmd, &p).unwrap();
        s.model.changed_since(&before)
    }

    fn ids(v: &[Id]) -> Changed {
        Changed::Ids(v.iter().copied().collect())
    }

    #[test]
    fn a_command_leaves_a_journal_of_the_elements_it_added_changed_and_removed() {
        let mut s = Session::new();
        let made = s.execute("wall", &json!({"points": [[0, 0], [5000, 0], [5000, 4000]]})).unwrap();
        let [a, b] = [0, 1].map(|i| made["ids"][i].as_u64().unwrap());
        let before = Arc::clone(&s.model);
        let door = s.execute("door", &json!({"at": [2500, 0]})).unwrap()["id"].as_u64().unwrap();
        assert_eq!(s.model.changed_since(&before), ids(&[door]));
        // Moving a wall stretches the one joined to it, and carries its door.
        let moved = changed(&mut s, "move", json!({"ids": [b], "by": [1000, 0]}));
        let Changed::Ids(moved) = moved else { panic!("{moved:?}") };
        assert!(moved.contains(&a) && moved.contains(&b), "{moved:?}");
        assert_eq!(changed(&mut s, "delete", json!({"ids": [door]})), ids(&[door]));
        // Undo and redo go to a model of the history, which shares the rest still: what differs.
        assert_eq!(changed(&mut s, "undo", json!({})), ids(&[door]));
        assert_eq!(changed(&mut s, "redo", json!({})), ids(&[door]));
        // A command that changes a setting changes everything there is to check, and so does undoing it.
        assert_eq!(changed(&mut s, "room_area", json!({"at": "finish"})), Changed::All);
        assert_eq!(changed(&mut s, "undo", json!({})), Changed::All);
        // Another model's elements: everything.
        assert_eq!(Model::new("x").changed_since(&s.model), Changed::All);
    }

    #[test]
    fn the_journal_of_a_clone_starts_empty_names_what_changed_and_finds_what_a_sweep_changed() {
        let mut es = Elements::new();
        let g = |at: f64| {
            Element::Grid(crate::model::Grid { start: crate::geom::P2(at, 0.0), end: crate::geom::P2(at, 1.0), name: String::new(), type_name: "Grid - ISO 128".into() })
        };
        for id in 100_000..100_005 {
            es.insert(id, g(0.0));
        }
        let base = es.clone();
        let mut c = base.clone();
        assert_eq!(c.changed_since(&base), ids(&[]));
        if let Some(e) = c.get_mut(100_001) {
            *e = g(1.0);
        }
        c.insert(100_009, g(2.0));
        c.remove(100_000);
        assert_eq!(c.changed_since(&base), ids(&[100_000, 100_001, 100_009]));
        // Gone through from an id on, or all of them: those that differ.
        for (_, e) in c.range_mut(100_003..) {
            if let Element::Grid(gr) = e {
                gr.name = "C".into();
            }
        }
        assert_eq!(c.changed_since(&base), ids(&[100_000, 100_001, 100_003, 100_004, 100_009]));
        // A clone of a clone isn't base's: what differs, found in the chunks they don't share.
        assert_eq!(c.clone().changed_since(&base), ids(&[100_000, 100_001, 100_003, 100_004, 100_009]));
        let mut d = base.clone();
        for e in d.values_mut() {
            if let Element::Grid(gr) = e
                && gr.start.0 == 0.0
            {
                gr.start.0 = 5.0;
            }
        }
        assert_eq!(d.changed_since(&base), ids(&[100_000, 100_001, 100_002, 100_003, 100_004]));
        let mut d = base.clone();
        let _ = d.values_mut();
        assert_eq!(d.changed_since(&base), ids(&[]), "gone through, nothing changed");
        d.clear();
        assert_eq!(d.changed_since(&base), ids(&[100_000, 100_001, 100_002, 100_003, 100_004]));
    }

    #[test]
    fn copies_share_their_chunks_until_one_changes_and_the_other_stays_as_it_was() {
        let g = |at: f64| {
            Element::Grid(crate::model::Grid { start: crate::geom::P2(at, 0.0), end: crate::geom::P2(at, 1.0), name: String::new(), type_name: "Grid - ISO 128".into() })
        };
        let mut es = Elements::new();
        // Across three chunks, with a gap.
        let ids: Vec<Id> = (0..5).map(|i| 100_000 + i * (CHUNK / 2) + 3).collect();
        for &id in &ids {
            es.insert(id, g(id as f64));
        }
        let mut copy = es.clone();
        assert_eq!(copy, es);
        if let Some(e) = copy.get_mut(ids[2]) {
            *e = g(-1.0);
        }
        copy.remove(ids[4]);
        copy.insert(ids[4] + 1, g(7.0));
        assert_eq!(es.len(), 5);
        assert_eq!(es.get(ids[2]), Some(&g(ids[2] as f64)), "the original is as it was");
        assert!(es.contains_key(ids[4]) && !es.contains_key(ids[4] + 1));
        assert_eq!(copy.get(ids[2]), Some(&g(-1.0)));
        assert_eq!(copy.len(), 5);
        assert_ne!(copy, es);
        // In order, across the chunks; from an id inside one; the last.
        assert_eq!(es.keys().copied().collect::<Vec<_>>(), ids);
        assert_eq!(es.range(ids[1] + 1..).map(|(id, _)| *id).collect::<Vec<_>>(), ids[2..].to_vec());
        assert_eq!(copy.keys().next_back(), Some(&(ids[4] + 1)));
        // GUIDs find their elements in each, and the copy's change set is what it changed.
        let guid = es.guid(ids[3]).unwrap();
        assert_eq!((es.id(guid), copy.id(guid)), (Some(ids[3]), Some(ids[3])));
        assert_eq!(copy.changed_since(&es), ids_of(&[ids[2], ids[4], ids[4] + 1]));
    }

    fn ids_of(v: &[Id]) -> Changed {
        Changed::Ids(v.iter().copied().collect())
    }
}
