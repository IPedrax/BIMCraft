//! Automatic framing through the commands used by CLI and MCP clients.
#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use serde_json::{Value, json};
    use std::path::Path;

    fn run(session: &mut Session, command: &str, params: Value) -> Value {
        session.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
    }

    fn building(session: &mut Session) {
        run(session, "wall", json!({"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}));
        run(session, "section", json!({"name": "Building section", "start": [-1000,3000], "end": [9000,3000]}));
    }

    fn scan(session: &mut Session, directory: &Path) -> u64 {
        let path = directory.join("synthetic.xyz");
        std::fs::write(&path, "0 0 0 255 0 0\n4 0 0 255 0 0\n0 3 2 255 0 0\n4 3 2 255 0 0\n").unwrap();
        run(session, "point_cloud", json!({"path": path, "at": [200000,200000]}))["id"].as_u64().unwrap()
    }

    fn picture(session: &mut Session, directory: &Path, view: &str, name: &str) -> Vec<u8> {
        let path = directory.join(format!("{name}.png"));
        run(session, "render", json!({"view": view, "path": path, "width": 640, "height": 480}));
        let png = std::fs::read(&path).unwrap();
        if let Some(dump) = std::env::var_os("BIMCRAFT_FRAMING_DUMP") {
            std::fs::create_dir_all(&dump).unwrap();
            std::fs::copy(&path, Path::new(&dump).join(format!("{name}.png"))).unwrap();
        }
        png
    }

    fn red_pixels(png: &[u8]) -> usize {
        tiny_skia::Pixmap::decode_png(png).unwrap().pixels().iter().filter(|p| p.red() > 200 && p.green() < 60 && p.blue() < 60).count()
    }

    #[test]
    fn adding_a_distant_scan_keeps_the_building_readable_in_default_views_after_reopening() {
        let directory = tempfile::tempdir().unwrap();
        let mut session = Session::new();
        building(&mut session);
        let section = picture(&mut session, directory.path(), "Building section", "section-before");
        let parallel = picture(&mut session, directory.path(), "{3D}", "parallel-before");
        run(&mut session, "perspective", json!({"on": true}));
        let perspective = picture(&mut session, directory.path(), "{3D}", "perspective-before");
        scan(&mut session, directory.path());
        let perspective_after = picture(&mut session, directory.path(), "{3D}", "perspective-after");
        run(&mut session, "perspective", json!({"on": false}));
        let parallel_after = picture(&mut session, directory.path(), "{3D}", "parallel-after");
        let section_after = picture(&mut session, directory.path(), "Building section", "section-after");
        let project = directory.path().join("building.bimcraft");
        run(&mut session, "save", json!({"path": project}));
        let mut reopened = Session::new();
        run(&mut reopened, "open", json!({"path": project}));
        assert_eq!(run(&mut reopened, "elements", json!({"category": "pointcloud"}))["count"], 1);
        let parallel_reopened = picture(&mut reopened, directory.path(), "{3D}", "parallel-reopened");
        let section_reopened = picture(&mut reopened, directory.path(), "Building section", "section-reopened");
        run(&mut reopened, "perspective", json!({"on": true}));
        let perspective_reopened = picture(&mut reopened, directory.path(), "{3D}", "perspective-reopened");
        let same = [
            ("section", section == section_after && section == section_reopened),
            ("parallel", parallel == parallel_after && parallel == parallel_reopened),
            ("perspective", perspective == perspective_after && perspective == perspective_reopened),
        ];
        println!("building framing unchanged after scan import and save/open: {same:?}");
        assert!(same.iter().all(|(_, same)| *same), "a distant scan changed automatic building framing: {same:?}");
    }

    #[test]
    fn scan_only_fitting_still_shows_scans_in_parallel_and_perspective() {
        let directory = tempfile::tempdir().unwrap();
        for hidden_building in [false, true] {
            let mut session = Session::new();
            if hidden_building {
                building(&mut session);
            }
            scan(&mut session, directory.path());
            if !hidden_building {
                use bimcraft_core::geom::P2;
                assert_eq!(session.model.model_extent(), [P2(197000.0, 197500.0), P2(203000.0, 202500.0)]);
            }
            run(&mut session, "graphics", json!({"view": "{3D}", "hide": ["wall", "level"]}));
            for perspective in [false, true] {
                run(&mut session, "perspective", json!({"on": perspective}));
                let png = picture(&mut session, directory.path(), "{3D}", &format!("scan-only-{hidden_building}-{perspective}"));
                assert!(
                    red_pixels(&png) >= 8,
                    "the fitted scan must remain visible with hidden_building={hidden_building}, perspective={perspective}"
                );
            }
        }
    }

    #[test]
    fn explicit_camera_and_crops_keep_scans_visible_and_survive_save_open() {
        let directory = tempfile::tempdir().unwrap();
        let mut session = Session::new();
        building(&mut session);
        scan(&mut session, directory.path());
        run(&mut session, "camera", json!({"name": "Scan camera", "eye": [190000,190000,6000], "target": [200000,200000,1000]}));
        run(&mut session, "crop", json!({"view": "Level 1", "region": [[197000,198000],[203000,202000]]}));
        run(&mut session, "section", json!({"name": "Scan section", "start": [197000,198000], "end": [203000,198000]}));
        run(&mut session, "crop", json!({"view": "Scan section", "region": [[0,-100],[6000,3000]]}));
        let views = ["Scan camera", "Level 1", "Scan section"];
        let before: Vec<_> =
            views.iter().enumerate().map(|(i, v)| picture(&mut session, directory.path(), v, &format!("explicit-{i}"))).collect();
        assert!(before.iter().all(|png| red_pixels(png) >= 8), "explicit views must still draw the distant scan");
        let project = directory.path().join("explicit.bimcraft");
        run(&mut session, "save", json!({"path": project}));
        let mut reopened = Session::new();
        run(&mut reopened, "open", json!({"path": project}));
        for (i, view) in views.iter().enumerate() {
            assert_eq!(before[i], picture(&mut reopened, directory.path(), view, &format!("explicit-reopened-{i}")));
        }
        let camera = run(&mut reopened, "camera", json!({"view": "Scan camera"}));
        assert_eq!(camera["eye"], json!([190000.0, 190000.0, 6000.0]));
        assert_eq!(camera["target"], json!([200000.0, 200000.0, 1000.0]));
        let error = reopened.execute("point_cloud", &json!({"path": directory.path().join("missing.xyz")})).unwrap_err();
        assert!(error.contains("missing.xyz"), "{error}");
        assert_eq!(before[0], picture(&mut reopened, directory.path(), "Scan camera", "explicit-after-failed-import"));
        assert_eq!(run(&mut reopened, "elements", json!({"category": "pointcloud"}))["count"], 1);
    }

    #[test]
    fn empty_fitting_and_explicit_level_extents_are_preserved() {
        use bimcraft_core::geom::P2;
        let directory = tempfile::tempdir().unwrap();
        let mut session = Session::new();
        assert_eq!(session.model.model_extent(), [P2(-10000.0, -10000.0), P2(10000.0, 10000.0)]);
        for perspective in [false, true] {
            run(&mut session, "perspective", json!({"on": perspective}));
            let png = picture(&mut session, directory.path(), "{3D}", &format!("empty-{perspective}"));
            assert_eq!(red_pixels(&png), 0);
        }
        let id =
            run(&mut session, "level", json!({"name": "Explicit", "elevation": 6000, "extent": [[-50000,-40000],[30000,20000]]}))["id"]
                .as_u64()
                .unwrap();
        building(&mut session);
        scan(&mut session, directory.path());
        assert_eq!(session.model.level_extent(id), Some([P2(-50000.0, -40000.0), P2(30000.0, 20000.0)]));
        // An automatically fitted section box still encloses scans: it is an explicit clipping tool, not a framing default.
        let section_box = run(&mut session, "section_box", json!({}));
        assert!(section_box["max"][0].as_f64().unwrap() > 200000.0, "{section_box}");
    }
}
