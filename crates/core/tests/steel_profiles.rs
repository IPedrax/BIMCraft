//! Structural sections through the public command interface.
#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use serde_json::json;

    #[test]
    fn parametrically_authored_hollow_column_keeps_its_section_in_native_and_ifc_output() {
        let mut session = Session::new();
        let ty = session
            .execute("type", &json!({"category":"structuralcolumn","name":"Tube","shape":"RHS","width":200,"depth":100,"thickness":8}))
            .unwrap();
        assert_eq!(ty["type"]["profile"]["shape"], "rhs", "a hollow type must not silently become a rectangle: {ty}");
        session.execute("structural_column", &json!({"at":[0,0],"type":"Tube","height":3000})).unwrap();
        let path = std::env::temp_dir().join(format!("steel-public-hollow-{}.ifc", std::process::id()));
        session.execute("save", &json!({"path":path})).unwrap();
        let ifc = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(ifc.contains("IFCRECTANGLEHOLLOWPROFILEDEF"));
        assert!(ifc.contains("'CrossSectionArea',$,$,0.004544"), "the tube's material area, not its 0.02 m2 box");
    }

    #[test]
    fn catalogue_beam_can_be_created_scheduled_saved_and_reopened() {
        let mut session = Session::new();
        let result = session.execute("beam", &json!({"points":[[0,0],[6000,0]],"type":"IPE 300"}));
        assert!(result.is_ok(), "a catalogue steel beam should be available: {result:?}");
        let schedule = session
            .execute("schedule", &json!({"category":"beam","fields":["Profile","Section Area","Weight per Metre","Weight"]}))
            .unwrap();
        assert_eq!(schedule["rows"][0][0], "IPE 300");
        assert_eq!(schedule["rows"][0][1], 53.81);
        let path = std::env::temp_dir().join(format!("steel-public-{}.bimcraft", std::process::id()));
        session.execute("save", &json!({"path":path})).unwrap();
        let mut reopened = Session::new();
        let opened = reopened.execute("open", &json!({"path":path})).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(opened.get("left_out").is_none_or(|v| v.as_array().is_none_or(Vec::is_empty)), "{opened}");
        let again = reopened
            .execute("schedule", &json!({"category":"beam","fields":["Profile","Section Area","Weight per Metre","Weight"]}))
            .unwrap();
        assert_eq!(again["rows"], schedule["rows"]);
    }

    #[test]
    fn steel_type_plate_edits_refresh_cached_sections_without_changing_elements() {
        use bimcraft_core::render::{self, SectionCache};
        let mut s = Session::new();
        s.execute("type", &json!({"category":"beam","name":"Tube","shape":"RHS","width":200,"depth":200,"thickness":8})).unwrap();
        s.execute("beam", &json!({"points":[[0,0],[4000,0]],"type":"Tube","offset":3000})).unwrap();
        let section = s.execute("section", &json!({"start":[2000,-500],"end":[2000,500]})).unwrap()["id"].as_u64().unwrap();
        let mut cache = SectionCache::default();
        let first = cache.work(&s.model);
        let view = render::View { scale: 2.0, cx: 500.0, cy: 2900.0, width: 600, height: 600 };
        let before = render::section_picture_and_over_with(&s.model, &first, section, view, view, &[]).1;
        let elements = s.model.elements.clone();
        s.execute("type", &json!({"category":"beam","name":"Tube","thickness":20})).unwrap();
        assert_eq!(s.model.elements, elements, "plate-only type edit leaves element values unchanged");
        let cached = cache.work(&s.model);
        let a = render::section_picture_and_over_with(&s.model, &cached, section, view, view, &[]).1;
        let b = render::section_picture_and_over(&s.model, section, view, view, &[]).1;
        assert_eq!(render::png(&a).unwrap(), render::png(&b).unwrap());
        assert_ne!(render::png(&before).unwrap(), render::png(&b).unwrap(), "thicker walls must visibly change the cut");
        assert!(cache.stats().built >= 2, "the plate change must rebuild the primitive");
    }

    #[test]
    fn equal_enclosing_boxes_with_distinct_steel_sections_export_distinct_profiles_and_true_quantities() {
        let mut s = Session::new();
        for (name, shape, web, flange, thickness) in
            [("I", "I", 7, 10, 0), ("Thick I", "I", 9, 15, 0), ("U", "U", 7, 10, 0), ("RHS", "RHS", 0, 0, 8)]
        {
            let mut p = json!({"category":"beam","name":name,"shape":shape,"width":150,"depth":300});
            if shape == "RHS" {
                p["thickness"] = json!(thickness);
            } else {
                p["web"] = json!(web);
                p["flange"] = json!(flange);
            }
            s.execute("type", &p).unwrap();
            let y = s.model.elements.len() as f64 * 1000.0;
            s.execute("beam", &json!({"points":[[0,y],[6000,y]],"type":name})).unwrap();
        }
        let ifc = bimcraft_core::ifc::export(&s.model);
        assert_eq!(ifc.matches("=IFCISHAPEPROFILEDEF(").count(), 2);
        assert_eq!(ifc.matches("=IFCUSHAPEPROFILEDEF(").count(), 1);
        assert_eq!(ifc.matches("=IFCRECTANGLEHOLLOWPROFILEDEF(").count(), 1);
        assert_eq!(ifc.matches("=IFCMATERIALPROFILESET(").count(), 4);
        assert!(ifc.contains("'CrossSectionArea',$,$,0.00496"), "the I's true plate area");
        assert!(ifc.contains("'CrossSectionArea',$,$,0.00693"), "the thicker I's true plate area");
        let globals: Vec<_> = ifc
            .lines()
            .filter(|l| l.contains("=IFCBEAMTYPE(") || l.contains("=IFCRELDEFINESBYTYPE(") || l.contains("=IFCRELASSOCIATESMATERIAL("))
            .filter_map(|l| l.split(char::from(39)).nth(1))
            .collect();
        let unique: std::collections::HashSet<_> = globals.iter().collect();
        assert_eq!(globals.len(), unique.len());
        assert_eq!(ifc, bimcraft_core::ifc::export(&s.model));
    }

    #[test]
    fn unsupported_categories_reject_section_parameters_and_plain_shape_replaces_a_hollow_circle() {
        let mut s = Session::new();
        let before = s.model.clone();
        let result = s.execute("type", &json!({"category":"wall","name":"Bad wall","thickness":200,"profile":"IPE 300"}));
        assert!(result.is_err(), "a wall must not silently discard a steel profile: {result:?}");
        assert_eq!(s.model, before);
        s.execute("type", &json!({"category":"beam","name":"Hollow","shape":"CHS","width":200,"thickness":10})).unwrap();
        let result = s.execute("type", &json!({"category":"beam","name":"Hollow","shape":"rectangle"})).unwrap();
        assert_eq!(result["type"]["round"], false, "an explicit rectangle is rectangular");
        assert!(result["type"].get("profile").is_none());
    }
}

#[cfg(test)]
mod picking {
    use bimcraft_core::{engine::Session, geom::P2, render};
    use serde_json::json;

    #[test]
    fn a_hollow_column_can_be_picked_on_its_wall_but_not_through_its_empty_centre() {
        let mut s = Session::new();
        s.execute("type", &json!({"category":"structuralcolumn","name":"Tube","shape":"RHS","width":200,"depth":100,"thickness":10}))
            .unwrap();
        let id = s.execute("structural_column", &json!({"at":[0,0],"type":"Tube","height":3000})).unwrap()["id"].as_u64().unwrap();
        let d = render::draw(&s.model, 100000, render::View { scale: 3.0, cx: 0.0, cy: 0.0, width: 900, height: 600 });
        assert_eq!(d.pick(d.view.px(P2(0.0, 0.0)), 1.0), None, "the hole is empty");
        assert_eq!(d.pick(d.view.px(P2(95.0, 0.0)), 1.0), Some(id), "the tube wall is pickable");
    }
}

#[cfg(test)]
mod native_versions {
    use bimcraft_core::{engine::Session, model::Model};
    use serde_json::json;

    #[test]
    fn steel_files_require_a_reader_that_knows_sections_and_plain_format_17_files_keep_their_geometry() {
        let mut plain = Session::new();
        plain.execute("beam", &json!({"points":[[0,0],[6000,0]],"offset":3000})).unwrap();
        let mut legacy: serde_json::Value = serde_json::from_str(&plain.model.to_json().unwrap()).unwrap();
        legacy["format"] = json!(17);
        let (migrated, left_out) = Model::read(&serde_json::to_string(&legacy).unwrap()).unwrap();
        assert!(left_out.is_empty(), "legacy fields must be retained: {left_out:?}");
        assert_eq!(migrated.types, plain.model.types);
        assert_eq!(migrated.elements, plain.model.elements, "migration keeps all element geometry and GUIDs");
        let mut steel = Session::new();
        steel.execute("beam", &json!({"points":[[0,0],[6000,0]],"type":"IPE 300"})).unwrap();
        let file = steel.model.to_json().unwrap();
        let encoded: serde_json::Value = serde_json::from_str(&file).unwrap();
        assert!(encoded["format"].as_u64().unwrap() > 17, "a format-17 reader would drop the section and reinterpret its bounding box");
        assert_eq!(migrated.format, encoded["format"].as_u64().unwrap() as u32);
        let (reopened, left_out) = Model::read(&file).unwrap();
        assert!(left_out.is_empty());
        assert_eq!(reopened.types, steel.model.types);
        assert_eq!(reopened.elements, steel.model.elements);
        // The same pre-lenient version gate must continue to reject future geometry formats.
        let mut future = encoded;
        future["format"] = json!(bimcraft_core::model::FORMAT + 1);
        let error = Model::read(&serde_json::to_string(&future).unwrap()).unwrap_err();
        assert!(error.contains("saved by a newer BIMCraft"), "{error}");
    }
}

#[cfg(test)]
mod sloped_hollow_bounds {
    use bimcraft_core::{
        bounds::{Bounds, BoundsCache},
        engine::Session,
        render::{self, SectionOverlay, SectionWork},
        runs::Swept,
    };
    use serde_json::json;

    fn beam(profile: bool, end: [f64; 2], rise: f64, justify: &str) -> (Session, u64) {
        let mut s = Session::new();
        let mut params = json!({"category":"beam","name":"Slope","width":200});
        if profile {
            params["shape"] = json!("CHS");
            params["thickness"] = json!(10);
        } else {
            params["round"] = json!(true);
        }
        s.execute("type", &params).unwrap();
        let id =
            s.execute("beam", &json!({"points":[[0,0],end],"type":"Slope","offset":0,"end_offset":rise,"justify":justify})).unwrap()["ids"]
                [0]
            .as_u64()
            .unwrap();
        (s, id)
    }

    #[test]
    fn sloped_chs_bounds_enclose_the_actual_upright_end_sweep_in_every_direction() {
        for (end, rise, justify) in [
            ([3000.0, 0.0], 4000.0, "centre"),
            ([-1800.0, 2400.0], -4000.0, "top"),
            ([0.0, 3000.0], 4000.0, "bottom"),
            ([3000.0, 0.0], 0.0, "centre"),
        ] {
            let (s, id) = beam(true, end, rise, justify);
            let e = s.model.elements.get(id).unwrap();
            let t = s.model.run_type(e).unwrap();
            let g = s.model.run_geom(e).unwrap();
            let points: Vec<_> = g
                .swept(&t.profile.as_ref().unwrap().pieces(t.width, t.depth), [None, None])
                .into_iter()
                .flat_map(|piece| match piece {
                    Swept::Mesh(tris, _) => tris.into_iter().flatten().collect::<Vec<_>>(),
                    Swept::Prism(..) => panic!("CHS must use its hollow mesh"),
                })
                .collect();
            assert!(!points.is_empty());
            let Bounds::Box([lo, hi]) = BoundsCache::default().get(&s.model, id) else { panic!("finite bounds") };
            let outside = points.iter().filter(|p| (0..3).any(|i| p[i] < lo[i] || p[i] > hi[i])).count();
            let min = points.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
            let max = points.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max);
            println!(
                "CHS end={end:?} rise={rise} justify={justify}: cached_z=[{},{}] native_z=[{min},{max}] outside={outside}",
                lo[2], hi[2]
            );
            assert_eq!(outside, 0, "a conservative box must contain every actual steel sweep vertex: {lo:?}..{hi:?}");
        }
    }

    #[test]
    fn plain_round_slopes_keep_the_existing_perpendicular_tube_envelope() {
        let (s, id) = beam(false, [3000.0, 0.0], 4000.0, "centre");
        let g = s.model.run_geom(s.model.elements.get(id).unwrap()).unwrap();
        let Bounds::Box([lo, hi]) = BoundsCache::default().get(&s.model, id) else { panic!("finite bounds") };
        assert!(
            (lo[2] + 60.0).abs() < 1e-7 && (hi[2] - 4060.0).abs() < 1e-7,
            "the plain tube keeps its perpendicular 60 mm vertical radius: {lo:?}..{hi:?}"
        );
        let (triangles, _) = g.tube();
        assert!(triangles.iter().flatten().all(|p| (0..3).all(|i| lo[i] <= p[i] && p[i] <= hi[i])));
        println!("plain round cached_z=[{},{}] contains every perpendicular tube vertex", lo[2], hi[2]);
    }

    #[test]
    fn a_sloped_chs_section_keeps_full_extents_and_pixels_when_neighbour_heights_hide_the_old_box() {
        let (mut s, beam) = beam(true, [3000.0, 0.0], 4000.0, "centre");
        // This column spans -100..4100: it hides the old -60..4060 box,
        // but not the steel's actual -165.24..4165.24 extremities.
        s.execute("column", &json!({"at":[4500,200],"base_offset":-100,"height":4200})).unwrap();
        let section = s.execute("section", &json!({"start":[-500,-300],"end":[5000,-300]})).unwrap()["id"].as_u64().unwrap();
        let full = SectionWork::new(&s.model);
        let deferred = SectionWork::new_deferred(&s.model);
        assert_eq!(deferred.preparation_stats().deferred_runs, 1, "exercise the real deferred round-run path");
        let expected = render::section_fit_with(&s.model, &full, section, 800, 600);
        let actual = render::section_fit_with(&s.model, &deferred, section, 800, 600);
        println!("full section fit={expected:?}; deferred section fit={actual:?}");
        assert_eq!(actual, expected, "nearby geometry must not conceal the CHS extremities while fitting the section");
        let a = render::section_picture_with(&s.model, &deferred, section, actual, &[]).unwrap();
        let b = render::section_picture_with(&s.model, &full, section, expected, &[]).unwrap();
        assert_eq!(a.rgba, b.rgba, "deferred rendering must show the full native geometry");
        assert!(
            (0..actual.height).any(|y| (0..actual.width).any(|x| a.pick(f64::from(x), f64::from(y)) == Some(beam))),
            "the section must actually show the beam"
        );
        let da = SectionOverlay::new(&s.model, &deferred, section, actual);
        let db = SectionOverlay::new(&s.model, &full, section, expected);
        assert_eq!(da.levels(), db.levels(), "datum endpoints must keep the full model extent");
        assert_eq!(render::png(&da.draw(&s.model, Some(&a))).unwrap(), render::png(&db.draw(&s.model, Some(&b))).unwrap());
        for y in 0..actual.height {
            for x in 0..actual.width {
                let (x, y) = (f64::from(x), f64::from(y));
                assert_eq!((a.pick(x, y), a.face(x, y), a.covered(x, y)), (b.pick(x, y), b.face(x, y), b.covered(x, y)));
            }
        }
        if let Some(directory) = std::env::var_os("BIMCRAFT_STEEL_BOUNDS_EVIDENCE") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join("sloped-chs-deferred.png"), a.png().unwrap()).unwrap();
            std::fs::write(directory.join("sloped-chs-section.png"), render::section_png(&s.model, section, expected).unwrap()).unwrap();
        }
    }

    #[test]
    fn a_joined_steel_tubes_deferred_box_takes_in_its_mitre() {
        // Two flat CHS 200s at a corner mitre into an elbow whose outer corner, at (-100, -100), lies in neither tube's box
        // of its line. A section along x + y = -110 passes both boxes by and cuts only the elbow: deferred, the tubes must
        // still be found there, looking either way.
        let mut s = Session::new();
        s.execute("type", &json!({"category": "beam", "name": "Tube", "width": 200, "shape": "CHS", "thickness": 10})).unwrap();
        s.execute("beam", &json!({"points": [[3000, 0], [0, 0], [0, 3000]], "type": "Tube", "justify": "centre"})).unwrap();
        let mut shown = false;
        for (start, end) in [([-1000, 890], [890, -1000]), ([890, -1000], [-1000, 890])] {
            let section = s.execute("section", &json!({"start": start, "end": end})).unwrap()["id"].as_u64().unwrap();
            let (full, deferred) = (SectionWork::new(&s.model), SectionWork::new_deferred(&s.model));
            assert_eq!(deferred.preparation_stats().deferred_runs, 2, "both tubes take the deferred path");
            let view = render::section_fit_with(&s.model, &full, section, 1600, 1200);
            assert_eq!(render::section_fit_with(&s.model, &deferred, section, 1600, 1200), view);
            let a = render::section_picture_with(&s.model, &deferred, section, view, &[]).unwrap();
            let b = render::section_picture_with(&s.model, &full, section, view, &[]).unwrap();
            assert_eq!(a.rgba, b.rgba, "the deferred section shows what the full one does");
            shown |= (0..view.height).any(|y| (0..view.width).any(|x| b.pick(f64::from(x), f64::from(y)).is_some()));
        }
        assert!(shown, "a section through the corner cuts the elbow");
    }
}
