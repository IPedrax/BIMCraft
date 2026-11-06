//! Metadata-only, copy-on-write revision columns. No element objects or models are
//! retained by a snapshot, and runtime stamps never enter the file format.
use super::{Id, chunk_of, token};
use crate::model::Element;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// An opaque runtime stamp, meaningful together with an element id. Snapshot copies
/// preserve it; granting mutable access produces a new stamp across undo branches too.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Revision(u64);
impl Revision {
    pub(super) fn fresh() -> Self {
        Self(token())
    }
}

type Column = Arc<Vec<(Id, Revision)>>;

/// A snapshot of revision metadata, sharing unchanged id chunks. Its comparison reads
/// only metadata from changed chunks, never old element objects or model definitions.
#[derive(Clone, Debug, Default)]
pub struct RevisionSnapshot {
    chunks: BTreeMap<Id, Column>,
}

impl RevisionSnapshot {
    pub fn get(&self, id: Id) -> Option<Revision> {
        let column = self.chunks.get(&chunk_of(id))?;
        column.binary_search_by_key(&id, |(id, _)| *id).ok().and_then(|i| column.get(i)).map(|(_, revision)| *revision)
    }

    pub(super) fn touch(&mut self, id: Id, revision: Revision) {
        let column = Arc::make_mut(self.chunks.entry(chunk_of(id)).or_default());
        match column.binary_search_by_key(&id, |(id, _)| *id) {
            Ok(i) => {
                if let Some(entry) = column.get_mut(i) {
                    entry.1 = revision;
                }
            }
            Err(i) => column.insert(i, (id, revision)),
        }
    }

    pub(super) fn remove(&mut self, id: Id) {
        let key = chunk_of(id);
        let Some(column) = self.chunks.get_mut(&key) else { return };
        if let Ok(i) = column.binary_search_by_key(&id, |(id, _)| *id) {
            Arc::make_mut(column).remove(i);
        }
        if column.is_empty() {
            self.chunks.remove(&key);
        }
    }

    /// Added, removed or mutably accessed ids, even across skipped snapshots and branches.
    /// Mutable access is conservative: an unchanged value may have a newer stamp.
    pub fn changed_since(&self, before: &Self) -> BTreeSet<Id> {
        let keys: BTreeSet<Id> = self.chunks.keys().chain(before.chunks.keys()).copied().collect();
        let mut changed = BTreeSet::new();
        for key in keys {
            let (now, old) = (self.chunks.get(&key), before.chunks.get(&key));
            if let (Some(now), Some(old)) = (now, old)
                && Arc::ptr_eq(now, old)
            {
                continue;
            }
            let now = now.map_or(&[][..], |column| column.as_slice());
            let old = old.map_or(&[][..], |column| column.as_slice());
            for (id, revision) in now {
                let previous = old.binary_search_by_key(id, |(id, _)| *id).ok().and_then(|i| old.get(i)).map(|(_, r)| *r);
                if previous != Some(*revision) {
                    changed.insert(*id);
                }
            }
            for (id, _) in old {
                if now.binary_search_by_key(id, |(id, _)| *id).is_err() {
                    changed.insert(*id);
                }
            }
        }
        changed
    }

    /// Retained vector capacities plus conservative Arc/map container overhead. A cache
    /// may charge this whole amount even when authoritative storage shares the columns.
    pub fn retained_bytes(&self) -> usize {
        self.chunks.values().fold(std::mem::size_of::<Self>(), |bytes, column| {
            bytes.saturating_add(column.capacity().saturating_mul(std::mem::size_of::<(Id, Revision)>())).saturating_add(512)
        })
    }
}

/// Add revision stamps only as mutable iterator items are yielded. Constructing or
/// partially consuming an iterator must not invalidate the untouched suffix.
pub struct Revised<'a, I> {
    inner: I,
    revisions: &'a mut RevisionSnapshot,
    epoch: Revision,
}
impl<'a, I> Revised<'a, I> {
    pub(super) fn new(inner: I, revisions: &'a mut RevisionSnapshot) -> Self {
        Self { inner, revisions, epoch: Revision::fresh() }
    }
}
impl<'a, I: Iterator<Item = (&'a Id, &'a mut Element)>> Iterator for Revised<'a, I> {
    type Item = (&'a Id, &'a mut Element);
    fn next(&mut self) -> Option<Self::Item> {
        let item = self.inner.next()?;
        self.revisions.touch(*item.0, self.epoch);
        Some(item)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}
impl<'a, I: DoubleEndedIterator<Item = (&'a Id, &'a mut Element)>> DoubleEndedIterator for Revised<'a, I> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let item = self.inner.next_back()?;
        self.revisions.touch(*item.0, self.epoch);
        Some(item)
    }
}
impl<'a, I: std::iter::FusedIterator<Item = (&'a Id, &'a mut Element)>> std::iter::FusedIterator for Revised<'a, I> {}
