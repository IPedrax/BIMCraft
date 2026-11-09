//! Property boundaries exercised through the command API and exchanged output.
#[cfg(test)]
mod tests {
    use bimcraft_core::{engine::Session, ifc, import, model::Model, render};
    use serde_json::{Value, json};

    fn run(s: &mut Session, command: &str, params: Value) -> Value {
        s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
    }

    #[test]
    fn property_boundary_command_draw_schedule_native_and_ifc_round_trip() {
        let mut s = Session::new();
        let made = run(&mut s, "property_line", json!({"name":"Synthetic Plot", "points":[[0,0],[6000,0],[6000,4000],[0,4000],[0,0]]}));
        let id = made["id"].as_u64().unwrap();
        assert_eq!(made["area_m2"], 24.0);
        let elements = run(&mut s, "elements", json!({"category":"property lines"}));
        let plot = &elements["elements"][0];
        assert_eq!(plot["perimeter_m"], 20.0);
        assert_eq!(plot["segments"][0]["length"], 6000.0);
        assert_eq!(plot["segments"][0]["azimuth"], 90.0);
        assert_eq!(plot["segments"][1]["azimuth"], 0.0);
        assert_eq!(plot["segments"][2]["azimuth"], 270.0);
        assert_eq!(plot["segments"][3]["azimuth"], 180.0);
        let schedule = run(&mut s, "schedule", json!({"category":"property lines"}));
        assert_eq!(schedule["rows"][0], json!(["Synthetic Plot", "Level 1", 24.0, 20000.0]));
        let level = s.model.level_of(s.model.elements.get(id).unwrap()).unwrap();
        let view = render::View { cx: 3000.0, cy: 2000.0, scale: 0.05, width: 400, height: 300 };
        let drawing = render::draw(&s.model, level, view);
        let svg = render::svg(&drawing);
        assert!(svg.contains("<path") || svg.contains("<polyline"), "{svg}");
        let png = render::png(&drawing).unwrap();
        let pixels = tiny_skia::Pixmap::decode_png(&png).unwrap();
        assert!(pixels.pixel(60, 250).unwrap().red() < 230, "a long dash must print");
        assert_eq!(pixels.pixel(76, 250).unwrap().red(), 255, "a dash gap must remain white");
        assert!(!render::draw_ceiling(&s.model, level, view).owners.contains(&Some(id)));
        assert!(drawing.owners.contains(&Some(id)), "boundary must be pickable");
        let native = s.model.to_json().unwrap();
        let (reopened, left) = Model::read(&native).unwrap();
        assert!(left.is_empty());
        assert_eq!(reopened.guid(id), s.model.guid(id));
        assert_eq!(reopened.elements.get(id), s.model.elements.get(id));
        let step = ifc::export(&s.model);
        assert!(step.contains("'FootPrint','Curve2D'"));
        let loaded = import::ifc(step.as_bytes(), "Synthetic Plot").unwrap();
        loaded.model.check().unwrap();
        let mut imported = Session::new();
        imported.reset(loaded.model, None);
        let back = run(&mut imported, "elements", json!({"category":"propertyline"}));
        assert_eq!(back["elements"][0]["area_m2"], 24.0, "{back}");
        assert_eq!(back["elements"][0]["guid"], plot["guid"]);
    }

    #[test]
    fn property_boundary_invalid_inputs_roll_back_and_read_only_refuses() {
        let mut s = Session::new();
        run(&mut s, "property_line", json!({"points":[[0,0],[6000,0],[6000,4000],[0,4000]]}));
        for params in [
            json!({"points": vec![[0,0]; bimcraft_core::model::MAX_POINTS + 1]}),
            json!({"points": [[0,0],[1e100,0],[0,4000]]}),
            json!({"name":" ","points": [[0,0],[6000,0],[0,4000]]}),
            json!({"points": [[0,0],[1,1]]}),
            json!({"points": [[0,0],[1000,0],[2000,0]]}),
            json!({"points": [[0,0],[3000,0],[3000,3000],[0,3000],[2000,-1000]]}),
            json!({"points": [[0,0],[3000,0],[3000,0],[0,3000]]}),
            json!({"points": [[0,0],[6000,0],[0,4000]],"level":999998}),
            json!({"points": [[0,0],[6000,0],[0,4000]],"typo":true}),
        ] {
            let before = s.model.to_json().unwrap();
            assert!(s.execute("property_line", &params).is_err(), "{params}");
            assert_eq!(s.model.to_json().unwrap(), before, "failed commands must be atomic");
        }
        s.read_only = Some("test read-only session");
        assert!(s.execute("property_line", &json!({"points":[[0,0],[6000,0],[0,4000]]})).is_err());
    }

    #[test]
    fn survey_bearings_cover_quadrants_and_carry_rounded_seconds() {
        let mut s = Session::new();
        run(&mut s, "property_line", json!({"points":[[0,2000],[2000,0],[0,-2000],[-2000,0]]}));
        let rows = run(&mut s, "elements", json!({"category":"propertyline"}));
        let segments = rows["elements"][0]["segments"].as_array().unwrap();
        for (segment, (az, bearing)) in
            segments.iter().zip([(135.0, "S 45°00'00\" E"), (225.0, "S 45°00'00\" W"), (315.0, "N 45°00'00\" W"), (45.0, "N 45°00'00\" E")])
        {
            assert_eq!(segment["azimuth"], az);
            assert_eq!(segment["bearing"], bearing);
            assert_eq!(segment["length"], 2828.43);
        }
        assert_eq!(
            bimcraft_core::site::bearing(bimcraft_core::geom::P2(0.0, 0.0), bimcraft_core::geom::P2(-0.0001, 1000.0)),
            ("N 0°00'00\" E".into(), 0.0)
        );
    }

    #[test]
    fn boundaries_move_copy_undo_and_refresh_the_level_lookup_without_losing_identity() {
        use bimcraft_core::geom::P2;
        let mut s = Session::new();
        let made = run(&mut s, "property_line", json!({"points":[[0,0],[6000,0],[6000,4000],[0,4000]]}));
        let id = made["id"].as_u64().unwrap();
        let guid = s.model.guid(id);
        let level = s.model.level_of(s.model.elements.get(id).unwrap()).unwrap();
        let view = render::View { cx: 5000.0, cy: 2000.0, scale: 0.03, width: 600, height: 300 };
        let before = render::png(&render::draw(&s.model, level, view)).unwrap();
        s.model.lookup();
        run(&mut s, "move", json!({"id":guid,"by":[8000,0]}));
        assert_eq!(s.model.guid(id), guid);
        let moved = render::draw(&s.model, level, view);
        assert_ne!(before, render::png(&moved).unwrap());
        assert_eq!(moved.pick(view.px(P2(8000.0, 2000.0)), 8.0), Some(id));
        run(&mut s, "copy", json!({"id":guid,"by":[0,8000]}));
        let rows = run(&mut s, "elements", json!({"category":"propertyline"}));
        let copy = rows["elements"].as_array().unwrap().iter().find(|e| e["id"] != json!(id)).unwrap();
        assert_ne!(copy["guid"], json!(guid));
        assert_eq!(copy["area_m2"], 24.0);
        run(&mut s, "undo", json!({}));
        run(&mut s, "undo", json!({}));
        assert_eq!(before, render::png(&render::draw(&s.model, level, view)).unwrap());
        run(&mut s, "redo", json!({}));
        assert_eq!(s.model.guid(id), guid);
        // A level reference remains a GUID and invalid edits roll back without changing the drawing.
        let saved = s.model.to_json().unwrap();
        assert!(s.execute("modify", &json!({"id":guid,"points":[[0,0],[1,1]]})).is_err());
        assert_eq!(s.model.to_json().unwrap(), saved);
    }

    #[test]
    fn native_unknown_fields_are_reported_newer_versions_refused_and_old_files_unchanged() {
        let old = Model::new("Old synthetic file");
        let old_text = old.to_json().unwrap();
        let (read, left) = Model::read(&old_text).unwrap();
        assert!(left.is_empty());
        assert_eq!(old.elements, read.elements);
        let mut s = Session::new();
        let id = run(&mut s, "property_line", json!({"points":[[0,0],[6000,0],[0,4000]]}))["id"].as_u64().unwrap();
        let mut native: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
        native["elements"][id.to_string()]["future_field"] = json!(true);
        let (model, left) = Model::read(&native.to_string()).unwrap();
        assert_eq!(left, [format!("/elements/{id}/future_field")]);
        assert_eq!(model.guid(id), s.model.guid(id));
        native["format"] = json!(bimcraft_core::model::FORMAT + 1);
        assert!(Model::read(&native.to_string()).unwrap_err().contains("newer BIMCraft"));
    }

    #[test]
    fn concave_reversed_boundaries_and_multiple_levels_survive_ifc_with_names_and_guids() {
        let mut s = Session::new();
        run(&mut s, "level", json!({"name":"Upper synthetic plan","elevation":3000}));
        let a = run(
            &mut s,
            "property_line",
            json!({"name":"Concave synthetic plot","level":"Upper synthetic plan", "points":[[0,4000],[2000,4000],[2000,2000],[6000,2000],[6000,0],[0,0]]}),
        );
        let b = run(&mut s, "property_line", json!({"name":"Triangle synthetic plot","points":[[8000,0],[11000,0],[8000,4000]]}));
        assert_eq!(a["area_m2"], 16.0);
        assert_eq!(b["area_m2"], 6.0);
        let loaded = import::ifc(ifc::export(&s.model).as_bytes(), "synthetic").unwrap();
        for id in [a["id"].as_u64().unwrap(), b["id"].as_u64().unwrap()] {
            let guid = s.model.guid(id);
            let imported = loaded.model.elements.get(guid).unwrap();
            assert_eq!(loaded.model.level_of(imported), s.model.level_of(s.model.elements.get(id).unwrap()));
            assert_eq!(imported, s.model.elements.get(id).unwrap());
        }
    }
    #[test]
    fn mismatched_site_metadata_is_ignored_without_losing_the_boundary_or_identity() {
        let mut s = Session::new();
        let made = run(&mut s, "property_line", json!({"name":"Named metadata plot","points":[[0,0],[6000,0],[0,4000]]}));
        let id = made["id"].as_u64().unwrap();
        let step = ifc::export(&s.model);
        let malformed = step.replace(
            "IFCPROPERTYLISTVALUE('Names',$,(IFCTEXT('Named metadata plot')),$)",
            "IFCPROPERTYLISTVALUE('Names',$,(IFCTEXT('Wrong plot name'),IFCTEXT('Unmatched extra name')),$)",
        );
        assert_ne!(malformed, step);
        let loaded = import::ifc(malformed.as_bytes(), "synthetic malformed metadata").unwrap();
        let plot = loaded.model.elements.get(s.model.guid(id)).unwrap();
        let bimcraft_core::model::Element::PropertyLine(plot) = plot else { panic!("expected the valid boundary") };
        assert_eq!(plot.name, "Property Line 1", "metadata lists must correspond to the footprint curve count");
        assert_eq!(plot.points.len(), 3);
        assert_eq!(plot.level, s.model.level_of(s.model.elements.get(id).unwrap()).unwrap());
        loaded.model.check().unwrap();
    }

    #[test]
    fn an_open_ifc_site_footprint_is_reported_instead_of_silently_closed() {
        let mut s = Session::new();
        run(&mut s, "property_line", json!({"points":[[0,0],[6000,0],[0,4000]]}));
        let step = ifc::export(&s.model);
        let line = step.lines().find(|line| line.contains("=IFCPOLYLINE((")).unwrap();
        let opened = format!("{}));", line.rsplit_once(',').unwrap().0);
        let loaded = import::ifc(step.replace(line, &opened).as_bytes(), "foreign open footprint").unwrap();
        assert!(loaded.skipped.iter().any(|s| s.contains("not a closed horizontal boundary")), "{:?}", loaded.skipped);
        assert!(loaded.model.elements.values().all(|e| e.category() != "propertyline"));
        loaded.model.check().unwrap();
    }
    #[test]
    fn boundary_rotation_mirroring_visibility_and_deletion_use_the_existing_commands() {
        let mut s = Session::new();
        let id = run(&mut s, "property_line", json!({"points":[[0,0],[6000,0],[6000,4000],[0,4000]]}))["id"].as_u64().unwrap();
        let guid = s.model.guid(id);
        run(&mut s, "rotate", json!({"id":guid,"about":[0,0],"angle":90}));
        let rotated = run(&mut s, "elements", json!({"category":"propertyline"}));
        assert_eq!(rotated["elements"][0]["segments"][0]["azimuth"], 0.0);
        run(&mut s, "mirror", json!({"id":guid,"axis":[[0,0],[1000,0]]}));
        let mirrored = run(&mut s, "elements", json!({"category":"propertyline"}));
        assert_eq!(mirrored["elements"][0]["segments"][0]["azimuth"], 180.0);
        assert_eq!(mirrored["elements"][0]["area_m2"], 24.0);
        assert_eq!(s.model.guid(id), guid);
        let level = s.model.level_of(s.model.elements.get(id).unwrap()).unwrap();
        run(&mut s, "graphics", json!({"view":level,"hide":["propertyline"]}));
        assert!(!render::plan(&s.model, level, 400, 300).owners.contains(&Some(id)));
        run(&mut s, "graphics", json!({"view":level,"show":["propertyline"]}));
        assert!(render::plan(&s.model, level, 400, 300).owners.contains(&Some(id)));
        run(&mut s, "delete", json!({"ids":[guid]}));
        assert!(!s.model.elements.contains_key(guid));
        run(&mut s, "undo", json!({}));
        assert_eq!(s.model.guid(id), guid);
        s.model.check().unwrap();
    }
}
