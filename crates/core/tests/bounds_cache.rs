//! Bounds are conservative query data, distinct from exact section framing.
#[cfg(test)]
mod tests {
    use bimcraft_core::bounds::{Bounds, BoundsCache};
    use bimcraft_core::engine::Session;
    use bimcraft_core::model::Element;

    fn fixture() -> (Session, u64) {
        let mut s = Session::new();
        s.script(r#"column {"at":[1000,2000],"height":3000}"#).unwrap();
        let id = s.model.elements.iter().find_map(|(&id, e)| matches!(e, Element::Column(_)).then_some(id)).unwrap();
        (s, id)
    }

    #[test]
    fn bounds_reuse_ignores_annotations_materials_and_unused_types() {
        let (mut s, id) = fixture();
        let mut cache = BoundsCache::new(4096);
        let first = cache.get(&s.model, id);
        assert!(matches!(first, Bounds::Box(_)));
        assert_eq!(cache.stats().built, 1);
        s.script(r#"text {"view":100000,"at":[0,0],"text":"Note"}"#).unwrap();
        s.model_mut().types.wall.values_mut().next().unwrap().thickness += 20.0;
        s.model_mut().materials.values_mut().next().unwrap().color = [5, 6, 7];
        assert_eq!(cache.get(&s.model, id), first);
        assert_eq!(cache.stats().built, 1);
        assert_eq!(cache.stats().hits, 1);
    }

    #[test]
    fn direct_type_level_and_branch_edits_cannot_reuse_stale_bounds() {
        let (s, id) = fixture();
        let original = (*s.model).clone();
        let mut cache = BoundsCache::new(4096);
        let first = cache.get(&original, id);
        let mut changed = original.clone();
        let name = changed.elements.get(id).unwrap().column().unwrap().type_name.clone();
        changed.types.column.get_mut(&name).unwrap().width += 900.0;
        assert_ne!(cache.get(&changed, id), first);
        assert_eq!(cache.get(&original, id), first);
        let level = original.elements.get(id).unwrap().column().unwrap().level;
        let mut left = original.clone();
        let Element::Level(l) = left.elements.get_mut(level).unwrap() else { panic!("level") };
        l.elevation += 100.0;
        assert_ne!(cache.get(&left, id), first);
        let mut right = original.clone();
        let Element::Column(c) = right.elements.get_mut(id).unwrap() else { panic!("column") };
        c.at.0 += 1000.0;
        assert_ne!(cache.get(&right, id), first);
        assert_eq!(cache.get(&original, id), first);
    }

    #[test]
    fn missing_and_unsupported_bounds_are_not_empty_and_insertions_invalidate() {
        let (mut s, id) = fixture();
        let mut cache = BoundsCache::new(4096);
        assert_eq!(cache.get(&s.model, 42), Bounds::Empty);
        assert_eq!(cache.get(&s.model, 100000), Bounds::Unknown);
        let first = cache.get(&s.model, id);
        let value = s.model_mut().elements.remove(id).unwrap();
        assert_eq!(cache.get(&s.model, id), Bounds::Empty);
        s.model_mut().elements.insert(id, value);
        assert_eq!(cache.get(&s.model, id), first);
        let name = s.model.elements.get(id).unwrap().column().unwrap().type_name.clone();
        let definition = s.model_mut().types.column.remove(&name).unwrap();
        assert_eq!(cache.get(&s.model, id), Bounds::Unknown);
        s.model_mut().types.column.insert(name, definition);
        assert_eq!(cache.get(&s.model, id), first);
    }

    #[test]
    fn run_bounds_follow_dimensions_but_ignore_the_material_named_by_the_type() {
        let mut s = Session::new();
        s.script(r#"pipe {"points":[[0,0],[3000,1000]],"end_offset":1400}"#).unwrap();
        let id = s.model.elements.iter().find_map(|(&id, e)| matches!(e, Element::Pipe(_)).then_some(id)).unwrap();
        let name = s.model.elements.get(id).unwrap().run().unwrap().type_name.clone();
        let mut cache = BoundsCache::default();
        let first = cache.get(&s.model, id);
        assert!(matches!(first, Bounds::Box(_)));
        s.model_mut().types.runs.get_mut("pipe").unwrap().get_mut(&name).unwrap().material = "Another paint".into();
        assert_eq!(cache.get(&s.model, id), first);
        assert_eq!(cache.stats().built, 1);
        s.model_mut().types.runs.get_mut("pipe").unwrap().get_mut(&name).unwrap().width *= 2.0;
        assert_ne!(cache.get(&s.model, id), first);
        assert_eq!(cache.stats().built, 2);
    }

    #[test]
    fn a_recent_hit_keeps_its_entry_when_the_budget_evicts_an_older_box() {
        let (mut s, first) = fixture();
        s.script(
            r#"column {"at":[9000,2000],"height":3000}
column {"at":[18000,2000],"height":3000}"#,
        )
        .unwrap();
        let mut ids = s.model.elements.iter().filter_map(|(&id, e)| (id != first && matches!(e, Element::Column(_))).then_some(id));
        let (second, third) = (ids.next().unwrap(), ids.next().unwrap());
        let mut cache = BoundsCache::default();
        cache.get(&s.model, first);
        cache.get(&s.model, second);
        cache.set_budget(cache.stats().retained_bytes);
        cache.get(&s.model, first);
        cache.get(&s.model, third);
        let built = cache.stats().built;
        cache.get(&s.model, first);
        assert_eq!(cache.stats().built, built);
        cache.get(&s.model, second);
        assert_eq!(cache.stats().built, built + 1);
        assert_eq!(cache.stats().entries, 2);
    }

    #[test]
    fn family_bounds_cover_mirrored_rotated_instances_and_follow_direct_definition_edits() {
        use serde_json::json;
        let mut s = Session::new();
        s.execute(
            "family",
            &json!({"name":"Bounds box", "parameters":[{"name":"Height","instance":true,"value":750}],
            "types":{"A":{}}, "forms":[{"profile":[[0,0],[400,0],[400,800],[0,800]],"to":"Height"}]}),
        )
        .unwrap();
        let id = s
            .execute(
                "component",
                &json!({"family":"Bounds box","type":"A","at":[1000,2000],
            "angle":37,"mirrored":true,"offset":200}),
            )
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        let mut cache = BoundsCache::default();
        let first = cache.get(&s.model, id);
        let encloses = |m: &bimcraft_core::model::Model, bounds| {
            let Bounds::Box([lo, hi]) = bounds else { panic!("supported family bound") };
            let Element::Component(c) = m.elements.get(id).unwrap() else { panic!("component") };
            let pieces = m.component_pieces(c);
            assert!(!pieces.is_empty());
            for p in pieces.iter().flat_map(|p| p.tris.iter().flatten()) {
                assert!((0..3).all(|i| lo[i] <= p[i] && p[i] <= hi[i]), "{p:?} outside {lo:?}..{hi:?}");
            }
        };
        encloses(&s.model, first);
        s.model_mut().families.get_mut("Bounds box").unwrap().parameters[0].value = json!(4000);
        let next = cache.get(&s.model, id);
        encloses(&s.model, next);
        assert_ne!(next, first);
        assert_eq!(cache.stats().built, 2);
        let note = s.execute("text", &json!({"view":100000,"at":[0,0],"text":"Note"})).unwrap()["id"].as_u64().unwrap();
        assert_eq!(cache.get(&s.model, id), next);
        assert_eq!(cache.stats().built, 2);
        s.execute("delete", &json!({"ids":[note]})).unwrap();
        assert_eq!(cache.get(&s.model, id), next);
        assert_eq!(cache.stats().built, 2);
        let Element::Component(c) = s.model_mut().elements.get_mut(id).unwrap() else { panic!("component") };
        c.params.insert("Height".into(), json!(6000));
        let third = cache.get(&s.model, id);
        encloses(&s.model, third);
        assert_ne!(third, next);
        assert_eq!(cache.stats().built, 3);
    }

    #[test]
    fn the_byte_budget_evicts_and_zero_budget_still_computes() {
        let (mut s, first) = fixture();
        s.script(r#"column {"at":[9000,2000],"height":3000}"#).unwrap();
        let second = s.model.elements.iter().find_map(|(&id, e)| (id != first && matches!(e, Element::Column(_))).then_some(id)).unwrap();
        let mut cache = BoundsCache::new(4096);
        let bounds = cache.get(&s.model, first);
        let one_entry = cache.stats().retained_bytes;
        cache.set_budget(one_entry);
        assert_eq!(cache.get(&s.model, first), bounds);
        cache.get(&s.model, second);
        assert!(cache.stats().retained_bytes <= one_entry);
        assert_eq!(cache.stats().entries, 1);
        assert_eq!(cache.stats().evicted, 1);
        cache.set_budget(0);
        assert_eq!(cache.stats().entries, 0);
        assert_eq!(cache.get(&s.model, first), bounds);
        assert_eq!(cache.stats().retained_bytes, 0);
        assert_eq!(cache.stats().entries, 0);
    }
}
