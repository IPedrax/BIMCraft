//! Cross-snapshot reuse begins with isolated primitives; the full renderer is the oracle.
#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use bimcraft_core::model::{Element, Id, Model};
    use bimcraft_core::render::{self, SectionCache, SectionWork};
    use serde_json::json;

    fn fixture() -> (Session, Id, Id, Id) {
        let mut s = Session::new();
        let column = s.execute("column", &json!({"at":[1000,0],"height":3000})).unwrap()["id"].as_u64().unwrap();
        let run = s.execute("pipe", &json!({"points":[[0,500],[3000,500]],"offset":1000})).unwrap()["ids"][0].as_u64().unwrap();
        let section = s.execute("section", &json!({"start":[-1000,-1000],"end":[4000,-1000]})).unwrap()["id"].as_u64().unwrap();
        (s, section, column, run)
    }

    fn same_output(m: &Model, section: Id, cached: &SectionWork) {
        let fresh = SectionWork::new(m);
        let view = render::section_fit_with(m, &fresh, section, 240, 180);
        assert_eq!(render::section_fit_with(m, cached, section, 240, 180), view);
        let (a, da) = render::section_picture_and_over_with(m, cached, section, view, view, &[]);
        let (b, db) = render::section_picture_and_over_with(m, &fresh, section, view, view, &[]);
        let (a, b) = (a.unwrap(), b.unwrap());
        assert_eq!(a.rgba, b.rgba);
        assert_eq!(render::png(&da).unwrap(), render::png(&db).unwrap());
        for y in 0..view.height {
            for x in 0..view.width {
                assert_eq!(a.pick(f64::from(x), f64::from(y)), b.pick(f64::from(x), f64::from(y)));
                assert_eq!(a.face(f64::from(x), f64::from(y)), b.face(f64::from(x), f64::from(y)));
                assert_eq!(a.covered(f64::from(x), f64::from(y)), b.covered(f64::from(x), f64::from(y)));
            }
        }
    }

    #[test]
    fn annotation_edits_rebuild_no_primitive_fragments_and_local_moves_rebuild_one() {
        let (mut s, section, column, _) = fixture();
        let mut cache = SectionCache::new(1024 * 1024);
        same_output(&s.model, section, &cache.work(&s.model));
        assert_eq!(cache.stats().built, 2);
        s.execute("text", &json!({"view":section,"at":[1000,2000],"text":"A note"})).unwrap();
        same_output(&s.model, section, &cache.work(&s.model));
        assert_eq!(cache.stats().built, 2);
        assert_eq!(cache.stats().hits, 2);
        s.execute("move", &json!({"ids":[column],"by":[1000,0]})).unwrap();
        same_output(&s.model, section, &cache.work(&s.model));
        assert_eq!(cache.stats().built, 3);
        s.execute("undo", &json!({})).unwrap();
        same_output(&s.model, section, &cache.work(&s.model));
        assert_eq!(cache.stats().built, 4);
    }

    #[test]
    fn type_and_level_edits_refresh_geometry_but_material_edits_only_refresh_paint() {
        let (mut s, section, _, run) = fixture();
        let mut cache = SectionCache::new(1024 * 1024);
        cache.work(&s.model);
        let name = s.model.elements.get(run).unwrap().run().unwrap().type_name.clone();
        s.model_mut().types.runs.get_mut("pipe").unwrap().get_mut(&name).unwrap().material = "Another material".into();
        same_output(&s.model, section, &cache.work(&s.model));
        assert_eq!(cache.stats().built, 2);
        s.model_mut().types.runs.get_mut("pipe").unwrap().get_mut(&name).unwrap().width *= 2.0;
        same_output(&s.model, section, &cache.work(&s.model));
        assert_eq!(cache.stats().built, 3);
        let level = s.model.elements.get(run).unwrap().run().unwrap().level;
        let Element::Level(l) = s.model_mut().elements.get_mut(level).unwrap() else { panic!("level") };
        l.elevation += 100.0;
        same_output(&s.model, section, &cache.work(&s.model));
        assert_eq!(cache.stats().built, 5);
    }

    #[test]
    fn zero_budget_eviction_and_cancellation_preserve_fresh_output() {
        let (s, section, _, _) = fixture();
        let mut cache = SectionCache::new(0);
        same_output(&s.model, section, &cache.work(&s.model));
        assert_eq!(cache.stats().entries, 0);
        assert_eq!(cache.stats().retained_bytes, 0);
        assert!(cache.work_cancellable(&s.model, &|| true).is_none());
        cache.set_budget(1024 * 1024);
        same_output(&s.model, section, &cache.work(&s.model));
        assert_eq!(cache.stats().entries, 2);
        assert!(cache.stats().retained_bytes > 0);
        cache.set_budget(0);
        assert_eq!(cache.stats().entries, 0);
        assert_eq!(cache.stats().retained_bytes, 0);
        same_output(&s.model, section, &cache.work(&s.model));
    }

    #[test]
    fn parallel_preparation_preserves_depth_ties_and_the_original_element_order() {
        let (s, section, column, _) = fixture();
        let mut m = (*s.model).clone();
        let prototype = m.elements.get(column).unwrap().clone();
        // Coincident instances make element order observable in pixel ownership.
        for id in 200000..201100 {
            m.elements.insert(id, prototype.clone());
        }
        let mut cache = SectionCache::default();
        same_output(&m, section, &cache.work(&m));
        assert_eq!(cache.stats().built, 1102);
        same_output(&m, section, &cache.work(&m));
        assert_eq!(cache.stats().built, 1102);
        assert_eq!(cache.stats().hits, 1102);
        assert_eq!(cache.stats().visited, 2 * m.elements.len() as u64, "assembly is still whole-model work");
        assert_eq!(cache.stats().prepared_uncached, 0);
    }
}
