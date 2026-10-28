//! Every element's two names, as Revit's: its id, a number from [`FIRST_ID`] up that people and agents type and schedules
//! show, which a project gives anew to an element that comes in from another file when its own is taken; and its GUID,
//! made once and never changed, which is what everything that points at an element holds (a wall its level, a door its
//! wall, a dimension what it measures, a viewport its view), so a new id never breaks a reference.
//!
//! A GUID read where a reference is given may be a number instead: the id of the element meant, until the model swaps it
//! for that element's GUID ([`Guid::of_id`], `Model::resolve`). That is how a command takes `"level": 100000`, and how a
//! file from before GUIDs opens.

use std::collections::{BTreeMap, HashMap, btree_map};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::model::Element;

pub type Id = u64;

/// The first id a project gives: ids have six digits, and more once a project has made 900 000 elements.
pub const FIRST_ID: Id = 100_000;

/// Where the ids of linked elements begin, in the model the views draw ([`crate::link::seen`]): past any a project makes.
pub const LINKED: Id = 100_000_000;

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
            let n = COUNT.fetch_add(1, Ordering::Relaxed);
            let hi = seed();
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
        e.ids.get(self).copied().or_else(|| self.pending())
    }
}

impl<K: Key> Key for &K {
    fn id_in(&self, e: &Elements) -> Option<Id> {
        (*self).id_in(e)
    }
}

/// A project's elements by id, each with its GUID. In a file, an object of ids, each element with its `guid`.
#[derive(Clone, Debug, Default)]
pub struct Elements {
    items: BTreeMap<Id, Element>,
    guids: BTreeMap<Id, Guid>,
    ids: HashMap<Guid, Id>,
}

impl PartialEq for Elements {
    fn eq(&self, other: &Elements) -> bool {
        self.items == other.items && self.guids == other.guids
    }
}

impl Elements {
    pub fn new() -> Elements {
        Elements::default()
    }

    pub fn get(&self, k: impl Key) -> Option<&Element> {
        self.items.get(&k.id_in(self)?)
    }

    pub fn get_mut(&mut self, k: impl Key) -> Option<&mut Element> {
        let id = k.id_in(self)?;
        self.items.get_mut(&id)
    }

    pub fn contains_key(&self, k: impl Key) -> bool {
        k.id_in(self).is_some_and(|id| self.items.contains_key(&id))
    }

    /// The id of the element `k` finds.
    pub fn id(&self, k: impl Key) -> Option<Id> {
        k.id_in(self).filter(|id| self.items.contains_key(id))
    }

    /// The GUID of the element `k` finds.
    pub fn guid(&self, k: impl Key) -> Option<Guid> {
        self.guids.get(&k.id_in(self)?).copied()
    }

    /// Puts `e` at `id`. An element already there keeps its GUID, so this is how an element changes; a new one gets one of its own.
    pub fn insert(&mut self, id: Id, e: Element) -> Option<Element> {
        if !self.guids.contains_key(&id) {
            let g = self.fresh();
            self.guids.insert(id, g);
            self.ids.insert(g, id);
        }
        self.items.insert(id, e)
    }

    /// Puts `e` at `id` with GUID `guid`, or an error if another element has that GUID.
    pub fn insert_as(&mut self, id: Id, guid: Guid, e: Element) -> Result<(), String> {
        if guid.pending().is_some() {
            return Err(format!("element {id}: {guid} is an id, not a GUID"));
        }
        match self.ids.get(&guid) {
            Some(&other) if other != id => return Err(format!("elements {other} and {id} have the same GUID {guid}")),
            _ => {}
        }
        if let Some(old) = self.guids.insert(id, guid) {
            self.ids.remove(&old);
        }
        self.ids.insert(guid, id);
        self.items.insert(id, e);
        Ok(())
    }

    /// A GUID no element here has.
    pub fn fresh(&self) -> Guid {
        loop {
            let g = Guid::new();
            if !self.ids.contains_key(&g) {
                return g;
            }
        }
    }

    pub fn remove(&mut self, k: impl Key) -> Option<Element> {
        let id = k.id_in(self)?;
        if let Some(g) = self.guids.remove(&id) {
            self.ids.remove(&g);
        }
        self.items.remove(&id)
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&Id, &mut Element) -> bool) {
        let gone: Vec<Id> = self.items.iter_mut().filter_map(|(id, e)| (!keep(id, e)).then_some(*id)).collect();
        for id in gone {
            self.remove(id);
        }
    }

    pub fn clear(&mut self) {
        *self = Elements::default();
    }

    pub fn iter(&self) -> btree_map::Iter<'_, Id, Element> {
        self.items.iter()
    }

    pub fn iter_mut(&mut self) -> btree_map::IterMut<'_, Id, Element> {
        self.items.iter_mut()
    }

    pub fn values(&self) -> btree_map::Values<'_, Id, Element> {
        self.items.values()
    }

    pub fn values_mut(&mut self) -> btree_map::ValuesMut<'_, Id, Element> {
        self.items.values_mut()
    }

    pub fn keys(&self) -> btree_map::Keys<'_, Id, Element> {
        self.items.keys()
    }

    pub fn range(&self, r: std::ops::RangeFrom<Id>) -> btree_map::Range<'_, Id, Element> {
        self.items.range(r)
    }

    pub fn range_mut(&mut self, r: std::ops::RangeFrom<Id>) -> btree_map::RangeMut<'_, Id, Element> {
        self.items.range_mut(r)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Every element with its id and its GUID.
    pub fn with_guids(&self) -> impl Iterator<Item = (Id, Guid, &Element)> {
        self.items.iter().filter_map(|(id, e)| Some((*id, *self.guids.get(id)?, e)))
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
            if let Some(e) = self.items.get(&id)
                && !out.ids.contains_key(&g)
            {
                let _ = out.insert_as(id, g, e.clone());
            }
        }
        for (id, g, e) in self.with_guids() {
            if !out.items.contains_key(&id) {
                let g = if out.ids.contains_key(&g) { out.fresh() } else { g };
                let _ = out.insert_as(id, g, e.clone());
            }
        }
        out
    }

    /// The same elements under other ids, `to(id)` for each, their GUIDs theirs still; none if two would share one.
    pub fn renumbered(&self, to: impl Fn(Id) -> Id) -> Option<Elements> {
        let mut out = Elements::new();
        for (id, g, e) in self.with_guids() {
            if out.items.contains_key(&to(id)) {
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
        &self.items[&k.id_in(self).unwrap_or_default()]
    }
}

impl<'a> IntoIterator for &'a Elements {
    type Item = (&'a Id, &'a Element);
    type IntoIter = btree_map::Iter<'a, Id, Element>;
    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<'a> IntoIterator for &'a mut Elements {
    type Item = (&'a Id, &'a mut Element);
    type IntoIter = btree_map::IterMut<'a, Id, Element>;
    fn into_iter(self) -> Self::IntoIter {
        self.items.iter_mut()
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

/// A number unlike the last run's: the clock's nanoseconds and the process. A browser has no process ids (std panics asking).
pub(crate) fn seed() -> u64 {
    let nanos = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    let process = if cfg!(target_arch = "wasm32") { 0 } else { std::process::id() };
    crate::model::mix(nanos ^ u64::from(process).rotate_left(32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::P2;
    use crate::model::Grid;

    fn grid() -> Element {
        Element::Grid(Grid { name: "A".into(), start: P2(0.0, 0.0), end: P2(0.0, 1000.0) })
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
