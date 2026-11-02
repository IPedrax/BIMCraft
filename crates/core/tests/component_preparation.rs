//! Conservative preparation may defer exact extrema, while fitting remains identical.
#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use bimcraft_core::model::{Id, Model};
    use bimcraft_core::render::{self, SectionOverlay, SectionWork};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn fixture() -> (Session, Id) {
        let mut s = Session::new();
        s.script(
            r#"wall {"points":[[0,0],[10000,0],[10000,10000],[0,10000]],"closed":true}
component {"family":"Desk","type":"1400 x 700","at":[5000,5000],"offset":300}"#,
        )
        .unwrap();
        let section = s.execute("section", &json!({"start":[-1000,-1000],"end":[11000,-1000]})).unwrap()["id"].as_u64().unwrap();
        (s, section)
    }

    fn same_output(m: &Model, id: Id, deferred: &SectionWork) {
        let full = SectionWork::new(m);
        same_output_in(m, id, deferred, &full);
    }

    fn same_output_in(m: &Model, id: Id, deferred: &SectionWork, full: &SectionWork) {
        let view = render::section_fit_with(m, full, id, 240, 180);
        assert_eq!(render::section_fit_with(m, deferred, id, 240, 180), view);
        let a = render::section_picture_with(m, deferred, id, view, &[]).unwrap();
        let b = render::section_picture_with(m, full, id, view, &[]).unwrap();
        assert_eq!(a.rgba, b.rgba);
        let da = SectionOverlay::new(m, deferred, id, view);
        let db = SectionOverlay::new(m, full, id, view);
        assert_eq!(da.levels(), db.levels(), "datum endpoints must remain exact");
        assert_eq!(render::png(&da.draw(m, Some(&a))).unwrap(), render::png(&db.draw(m, Some(&b))).unwrap());
        for y in 0..view.height {
            for x in 0..view.width {
                let (x, y) = (f64::from(x), f64::from(y));
                assert_eq!((a.pick(x, y), a.face(x, y), a.covered(x, y)), (b.pick(x, y), b.face(x, y), b.covered(x, y)));
            }
        }
    }

    #[test]
    fn interior_components_keep_exact_extrema_deferred_during_fitting_and_drawing() {
        let (s, section) = fixture();
        let work = SectionWork::new_deferred(&s.model);
        assert_eq!(work.preparation_stats().deferred_components, 1);
        assert_eq!(work.preparation_stats().resolved_components, 0);
        same_output(&s.model, section, &work);
        assert_eq!(work.preparation_stats().resolved_components, 0);
    }

    #[test]
    fn outlying_components_resolve_only_the_extrema_needed_for_exact_fitting() {
        let (mut s, section) = fixture();
        s.execute("component", &json!({"family":"Desk","type":"1400 x 700","at":[50000,5000],"angle":37,"mirrored":true,"offset":300}))
            .unwrap();
        let work = SectionWork::new_deferred(&s.model);
        assert_eq!(work.preparation_stats().deferred_components, 2);
        same_output(&s.model, section, &work);
        assert_eq!(work.preparation_stats().resolved_components, 1);
        same_output(&s.model, section, &work);
        assert_eq!(work.preparation_stats().resolved_components, 1, "resolved points are retained with the immutable work");
    }

    #[test]
    fn component_only_models_still_fit_their_exact_rotated_geometry() {
        let mut s = Session::new();
        s.script(
            r#"component {"family":"Desk","type":"1400 x 700","at":[0,0],"angle":37,"mirrored":true}
section {"start":[-2000,-2000],"end":[2000,-2000]}"#,
        )
        .unwrap();
        let id = s.model.elements.iter().find_map(|(&id, e)| matches!(e, bimcraft_core::model::Element::Section(_)).then_some(id)).unwrap();
        let work = SectionWork::new_deferred(&s.model);
        same_output(&s.model, id, &work);
        assert_eq!(work.preparation_stats().resolved_components, 1);
    }

    #[test]
    fn unsupported_family_forms_keep_full_preparation_and_cancellation_returns_no_work() {
        let (mut s, section) = fixture();
        s.execute(
            "family",
            &json!({"name":"Cut box","types":{"A":{}},"forms":[
            {"profile":[[0,0],[500,0],[500,500],[0,500]],"to":1000},
            {"profile":[[100,100],[400,100],[400,400],[100,400]],"from":100,"to":900,"void":true}]}),
        )
        .unwrap();
        s.execute("component", &json!({"family":"Cut box","type":"A","at":[5000,5000],"offset":300})).unwrap();
        assert!(SectionWork::new_deferred_cancellable(&s.model, &|| true).is_none());
        let work = SectionWork::new_deferred(&s.model);
        assert_eq!(work.preparation_stats().deferred_components, 1, "unsupported forms use the full path");
        same_output(&s.model, section, &work);
    }

    /// The fixture and `n` desks past its walls, each of which an exact end has to ask for its points.
    fn outliers(n: u32) -> (Session, Id) {
        let (mut s, section) = fixture();
        for i in 0..n {
            let at = [20000 + 5000 * i, 5000];
            s.execute("component", &json!({"family":"Desk","type":"1400 x 700","at":at,"angle":37,"offset":300})).unwrap();
        }
        (s, section)
    }

    /// Asked `after` times whether to go on, it says no from then on, and counts the asking.
    fn cancel_after(after: usize) -> (AtomicUsize, usize) {
        (AtomicUsize::new(0), after)
    }

    fn asked((calls, after): &(AtomicUsize, usize)) -> bool {
        calls.fetch_add(1, Ordering::Relaxed) >= *after
    }

    #[test]
    fn a_fit_cancelled_at_any_checkpoint_stops_there_and_its_work_still_finds_the_exact_result() {
        let (s, section) = outliers(6);
        let m = &s.model;
        let full = SectionWork::new(m);
        let view = render::section_fit_with(m, &full, section, 240, 180);
        let (mut partly, mut after) = (false, 0);
        loop {
            assert!(after < 10_000, "a fit must finish between its checkpoints");
            let work = SectionWork::new_deferred(m);
            let token = cancel_after(after);
            match render::section_fit_cancellable(m, &work, section, 240, 180, &|| asked(&token)) {
                Some(found) => {
                    assert_eq!(found, view);
                    assert_eq!(work.preparation_stats().resolved_components, 6, "each outlier decides an exact end");
                    break;
                }
                None => {
                    assert_eq!(token.0.load(Ordering::Relaxed), after + 1, "a cancelled query checks no further");
                    let resolved = work.preparation_stats().resolved_components;
                    partly |= 0 < resolved && resolved < 6;
                    // What was found stays, whole; what wasn't is found now, and the answer is the full oracle's.
                    assert_eq!(render::section_fit_with(m, &work, section, 240, 180), view, "cancelled after {after} checks");
                }
            }
            after += 1;
        }
        assert!(partly, "the sweep must stop between one component's extrema and the next's");
    }

    #[test]
    fn an_overlay_cancelled_at_any_checkpoint_leaves_exact_datum_ends_and_no_partial_extent() {
        let (s, section) = outliers(3);
        let m = &s.model;
        let full = SectionWork::new(m);
        let view = render::section_fit_with(m, &full, section, 240, 180);
        let fresh = SectionOverlay::new(m, &full, section, view);
        assert!(!fresh.levels().is_empty(), "the fixture's datum ends come from the model's extent");
        let png = render::png(&fresh.draw(m, None)).unwrap();
        let mut after = 0;
        loop {
            assert!(after < 10_000, "an overlay must finish between its checkpoints");
            let work = SectionWork::new_deferred(m);
            let token = cancel_after(after);
            let found = SectionOverlay::new_cancellable(m, &work, section, view, &|| asked(&token));
            let finished = found.is_some();
            // Finished, its own; cancelled, one more from the work it left, which must not hold a partial extent.
            let overlay = found.unwrap_or_else(|| SectionOverlay::new(m, &work, section, view));
            assert_eq!(overlay.levels(), fresh.levels(), "cancelled after {after} checks");
            assert_eq!(render::png(&overlay.draw(m, None)).unwrap(), png, "cancelled after {after} checks");
            if finished {
                break;
            }
            after += 1;
        }
    }

    #[test]
    fn a_fit_of_what_is_not_a_section_is_an_empty_frame_and_not_a_cancellation() {
        let (s, _) = fixture();
        let work = SectionWork::new_deferred(&s.model);
        let empty = render::section_fit_with(&s.model, &work, 1, 240, 180);
        assert_eq!(render::section_fit_cancellable(&s.model, &work, 1, 240, 180, &|| false), Some(empty));
        assert_eq!(render::section_fit_cancellable(&s.model, &work, 1, 240, 180, &|| true), None);
    }

    #[test]
    fn tower_sections_and_elevations_match_full_preparation_with_interior_extrema_deferred() {
        use bimcraft_core::sample::tower::{Drawings, Plate, drawn};
        let m = drawn(Plate::SMALL, 2, Drawings { annotation: false, sheets: true }).unwrap();
        let full = SectionWork::new(&m);
        let deferred = SectionWork::new_deferred(&m);
        assert!(deferred.preparation_stats().deferred_components > 100);
        for name in ["North Elevation", "South Elevation", "East Elevation", "West Elevation", "Section A"] {
            let id = m
                .elements
                .iter()
                .find_map(|(&id, e)| {
                    matches!(e,bimcraft_core::model::Element::Section(v) | bimcraft_core::model::Element::Elevation(v) if v.name == name)
                        .then_some(id)
                })
                .unwrap();
            same_output_in(&m, id, &deferred, &full);
        }
        let stats = deferred.preparation_stats();
        assert!(stats.resolved_components < stats.deferred_components, "interior components should remain deferred: {stats:?}");
    }
}
