//! Per-element revisions are runtime metadata, independent of semantic model storage.
#[cfg(test)]
mod tests {
    use bimcraft_core::ids::{CHUNK, Elements, Id};
    use bimcraft_core::model::{Element, Model};
    use std::collections::BTreeSet;

    fn elements() -> Elements {
        Model::new("Revision fixture").elements
    }
    fn ids(e: &Elements) -> [Id; 2] {
        [*e.keys().next().unwrap(), *e.keys().next_back().unwrap()]
    }
    fn moved(e: &mut Elements, id: Id, z: f64) {
        let Element::Level(level) = e.get_mut(id).unwrap() else { panic!("level") };
        level.elevation = z;
    }

    #[test]
    fn a_local_mutation_revises_only_its_element_even_inside_a_shared_chunk() {
        let original = elements();
        let [first, second] = ids(&original);
        assert_eq!(first / CHUNK, second / CHUNK);
        let snapshot = original.revision_snapshot();
        let mut edited = original.clone();
        assert_eq!(snapshot.get(first), edited.revision(first));
        moved(&mut edited, first, 100.0);
        assert_ne!(original.revision(first), edited.revision(first));
        assert_eq!(original.revision(second), edited.revision(second));
        assert_eq!(snapshot.get(first), original.revision(first));
        assert_eq!(edited.revision_snapshot().changed_since(&snapshot), BTreeSet::from([first]));
        assert_eq!(original.guid(first), edited.guid(first));
    }

    #[test]
    fn mutable_iterators_stamp_only_the_items_they_yield() {
        let mut e = elements();
        let [first, second] = ids(&e);
        let start = e.revision_snapshot();
        {
            let _unconsumed = e.iter_mut();
        }
        assert!(e.revision_snapshot().changed_since(&start).is_empty());
        let _ = e.iter_mut().next();
        assert_eq!(e.revision_snapshot().changed_since(&start), BTreeSet::from([first]));
        let middle = e.revision_snapshot();
        let _ = e.range_mut(second..).next_back();
        assert_eq!(e.revision_snapshot().changed_since(&middle), BTreeSet::from([second]));
    }

    #[test]
    fn revision_deltas_cover_skipped_snapshots_and_independent_branches() {
        let original = elements();
        let [first, second] = ids(&original);
        let base = original.revision_snapshot();
        let mut left = original.clone();
        moved(&mut left, first, 100.0);
        let mut later = left.clone();
        moved(&mut later, second, 4000.0);
        assert_eq!(later.revision_snapshot().changed_since(&base), BTreeSet::from([first, second]));
        let mut right = original.clone();
        moved(&mut right, first, 200.0);
        assert_ne!(left.revision(first), right.revision(first));
        assert_eq!(later.revision_snapshot().changed_since(&right.revision_snapshot()), BTreeSet::from([first, second]));
    }

    #[test]
    fn removal_replacement_and_clear_update_metadata_without_changing_guids_or_file_shape() {
        let mut e = elements();
        let [first, second] = ids(&e);
        let start = e.revision_snapshot();
        let json = serde_json::to_string(&e).unwrap();
        let guid = e.guid(first).unwrap();
        let value = e.remove(first).unwrap();
        assert!(e.revision(first).is_none());
        assert_eq!(e.revision_snapshot().changed_since(&start), BTreeSet::from([first]));
        e.insert_as(first, guid, value).unwrap();
        assert_eq!(e.guid(first), Some(guid));
        assert_ne!(e.revision(first), start.get(first));
        assert_eq!(json, serde_json::to_string(&e).unwrap());
        let read: Elements = serde_json::from_str(&json).unwrap();
        assert_eq!(read, e, "runtime stamps must not affect element equality");
        assert_ne!(read.revision(first), e.revision(first));
        e.clear();
        assert_eq!(e.revision_snapshot().changed_since(&start), BTreeSet::from([first, second]));
    }

    #[test]
    fn read_access_and_sparse_ids_do_not_expand_or_mutate_revision_metadata() {
        let mut e = elements();
        let [first, _] = ids(&e);
        let before = e.revision_snapshot();
        let _ = e.get(first);
        let _ = e.get_mut(42);
        assert!(e.revision_snapshot().changed_since(&before).is_empty());
        e.insert(1 << 40, e.get(first).unwrap().clone());
        let snapshot = e.revision_snapshot();
        assert!(snapshot.get(1 << 40).is_some());
        assert!(snapshot.retained_bytes() < 4096, "sparse metadata allocated dense id space");
    }

    #[test]
    fn annotation_commands_and_undo_do_not_revise_unrelated_geometry() {
        let mut session = bimcraft_core::engine::Session::new();
        session.script(r#"wall {"points":[[0,0],[8000,0]]}"#).unwrap();
        let before = session.model.elements.revision_snapshot();
        session.script(r#"text {"view":100000,"at":[2000,2000],"text":"Note"}"#).unwrap();
        let note = session.model.elements.iter().find_map(|(&id, e)| matches!(e, Element::Text(_)).then_some(id)).unwrap();
        assert_eq!(session.model.elements.revision_snapshot().changed_since(&before), BTreeSet::from([note]));
        for command in [format!("move {{\"ids\":[{note}],\"by\":[100,0]}}"), format!("delete {{\"ids\":[{note}]}}"), "undo {}".into()] {
            let before = session.model.elements.revision_snapshot();
            session.script(&command).unwrap();
            assert_eq!(session.model.elements.revision_snapshot().changed_since(&before), BTreeSet::from([note]), "{command}");
        }
    }
}
