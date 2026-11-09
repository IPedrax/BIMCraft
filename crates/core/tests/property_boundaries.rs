//! Property boundaries exercised through the command API and exchanged output.
#[cfg(test)]
mod tests {
    use bimcraft_core::{engine::Session, ifc, import, model::Model, render};
    use serde_json::{Value, json};

    fn run(s: &mut Session, command: &str, params: Value) -> Value {
        s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
    }

    // Bounds include the actual output strokes, not guessed name lengths or private layout state.
    fn text_ink(d: &render::Drawing, id: u64) -> Vec<((f32, f32), f32)> {
        d.prims
            .iter()
            .zip(&d.owners)
            .zip(&d.parts)
            .filter_map(|((p, owner), part)| {
                if *owner == Some(id) && *part == render::Part::Text {
                    if let render::Prim::Line { pts, width, .. } = p {
                        return Some(pts.iter().map(|p| (*p, *width)).collect::<Vec<_>>());
                    }
                }
                None
            })
            .flatten()
            .collect()
    }

    fn ink_box(ink: &[((f32, f32), f32)]) -> [f32; 4] {
        assert!(!ink.is_empty(), "both label lines must remain drawn");
        ink.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, &((x, y), w)| {
            [b[0].min(x - w / 2.0), b[1].min(y - w / 2.0), b[2].max(x + w / 2.0), b[3].max(y + w / 2.0)]
        })
    }

    fn assert_separate(a: [f32; 4], b: [f32; 4]) {
        assert!(
            a[2] + 1.0 < b[0] || b[2] + 1.0 < a[0] || a[3] + 1.0 < b[1] || b[3] + 1.0 < a[1],
            "property and room label ink must be separated: property={a:?}, room={b:?}"
        );
    }

    fn centered_room() -> (Session, u64, u64) {
        let mut s = Session::new();
        run(&mut s, "wall", json!({"points":[[0,0],[18000,0],[18000,10000],[0,10000]],"closed":true}));
        let room = run(&mut s, "room", json!({"point":[9000,5000],"name":"Synthetic Assembly"}))["id"].as_u64().unwrap();
        let level = s.model.levels()[0].0;
        (s, room, level)
    }

    fn centered_plot(s: &mut Session, name: &str) -> u64 {
        run(s, "property_line", json!({"name":name,"points":[[-2000,-2000],[20000,-2000],[20000,12000],[-2000,12000]]}))["id"]
            .as_u64()
            .unwrap()
    }

    #[test]
    fn property_and_centered_room_labels_remain_readable_at_different_scales_and_name_widths() {
        for (scale, name) in [(0.015, "Plot"), (0.03, "Synthetic boundary"), (0.05, "Long synthetic property boundary gyp 123")] {
            let (mut s, room, level) = centered_room();
            let view = render::View { cx: 9000.0, cy: 5000.0, scale, width: 1600, height: 1000 };
            let original_room = text_ink(&render::draw(&s.model, level, view), room);
            let plot = centered_plot(&mut s, name);
            let native = s.model.to_json().unwrap();
            let elements = run(&mut s, "elements", json!({}));
            let drawing = render::draw(&s.model, level, view);
            assert_eq!(text_ink(&drawing, room), original_room, "adding a plot must not move the room tag");
            assert_separate(ink_box(&text_ink(&drawing, plot)), ink_box(&original_room));
            assert_eq!(s.model.to_json().unwrap(), native, "drawing cannot change model geometry");
            assert_eq!(run(&mut s, "elements", json!({})), elements);
            let (reopened, left) = Model::read(&native).unwrap();
            assert!(left.is_empty());
            assert_eq!(reopened.elements, s.model.elements);
            assert_eq!(render::png(&render::draw(&reopened, level, view)).unwrap(), render::png(&drawing).unwrap());
        }
    }

    #[test]
    fn printed_sheet_preserves_room_pixels_when_a_centered_property_is_added_and_reopened() {
        use bimcraft_core::sheet;
        for scale in [100, 200] {
            let (mut s, room, level) = centered_room();
            let sh =
                run(&mut s, "sheet", json!({"number":"S-01","name":"Synthetic label coexistence","size":"A3"}))["id"].as_u64().unwrap();
            let vp = run(&mut s, "place", json!({"sheet":"S-01","view":"Level 1","scale":scale,"at":[210,165]}))["id"].as_u64().unwrap();
            let (frame, offset) = sheet::frame(&s.model, vp).unwrap();
            let room_anchor_x = offset.0 + frame.px(bimcraft_core::geom::P2(9000.0, 5000.0)).0;
            let original_room = text_ink(&render::draw_at(&s.model, level, frame, 1.0), room);
            let b = ink_box(&original_room);
            let before = tiny_skia::Pixmap::decode_png(&render::png(&sheet::draw(&s.model, sh).unwrap()).unwrap()).unwrap();
            let plot = centered_plot(&mut s, "Synthetic boundary");
            let path = tempfile::tempdir().unwrap();
            run(&mut s, "save", json!({"path":path.path().join("coexisting.bimcraft")}));
            let mut reopened = Session::new();
            run(&mut reopened, "open", json!({"path":path.path().join("coexisting.bimcraft")}));
            assert_eq!(reopened.model.elements, s.model.elements);
            let (frame, new_offset) = sheet::frame(&reopened.model, vp).unwrap();
            // The plot expands the viewport framing, but the sheet center and room anchor stay fixed.
            let after = tiny_skia::Pixmap::decode_png(&render::png(&sheet::draw(&reopened.model, sh).unwrap()).unwrap()).unwrap();
            let room_box = [b[0] + offset.0, b[1] + offset.1, b[2] + offset.0, b[3] + offset.1];
            for y in room_box[1].floor() as u32..=room_box[3].ceil() as u32 {
                for x in room_box[0].floor() as u32..=room_box[2].ceil() as u32 {
                    assert_eq!(
                        before.pixel(x, y),
                        after.pixel(x, y),
                        "printed room name/number/area pixels changed at {x},{y}, scale {scale}"
                    );
                }
            }
            let inner = render::draw_at(&reopened.model, level, frame, 1.0);
            assert_separate(ink_box(&text_ink(&inner, plot)), ink_box(&text_ink(&inner, room)));
            assert!((new_offset.0 + frame.px(bimcraft_core::geom::P2(9000.0, 5000.0)).0 - room_anchor_x).abs() < 1.0);
            run(&mut reopened, "render", json!({"view":"S-01","path":path.path().join("sheet.pdf")}));
            assert!(std::fs::read(path.path().join("sheet.pdf")).unwrap().starts_with(b"%PDF-"));
        }
    }

    #[test]
    fn property_labels_clear_multiple_room_tags_but_ignore_hidden_other_level_and_distant_tags() {
        let mut s = Session::new();
        let plot = centered_plot(&mut s, "Synthetic boundary");
        let level = s.model.levels()[0].0;
        let view = render::View { cx: 9000.0, cy: 5000.0, scale: 0.03, width: 1200, height: 800 };
        let alone = text_ink(&render::draw(&s.model, level, view), plot);
        run(&mut s, "wall", json!({"points":[[0,0],[18000,0],[18000,10000],[0,10000]],"closed":true}));
        run(&mut s, "room_separator", json!({"points":[[0,6000],[18000,6000]]}));
        let lower = run(&mut s, "room", json!({"point":[9000,5000],"name":"Assembly"}))["id"].as_u64().unwrap();
        let upper = run(&mut s, "room", json!({"point":[9300,6500],"name":"Upper synthetic room with long name"}))["id"].as_u64().unwrap();
        let drawing = render::draw(&s.model, level, view);
        let property = ink_box(&text_ink(&drawing, plot));
        for room in [lower, upper] {
            assert_separate(property, ink_box(&text_ink(&drawing, room)));
        }
        run(&mut s, "graphics", json!({"view":level,"hide":["room"]}));
        assert_eq!(text_ink(&render::draw(&s.model, level, view), plot), alone, "hidden tags must not displace the property");
        run(&mut s, "graphics", json!({"view":level,"show":["room"]}));
        run(&mut s, "modify", json!({"id":lower,"point":[15000,3000]}));
        run(&mut s, "modify", json!({"id":upper,"level":"Level 2","top_level":null,"limit_offset":2500}));
        assert_eq!(
            text_ink(&render::draw(&s.model, level, view), plot),
            alone,
            "distant and other-level tags must not displace the property"
        );
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
