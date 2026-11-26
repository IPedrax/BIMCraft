//! A section's and an elevation's near and far clips (ТЗ-029.1): the band a view sees, from `near` mm past its line to
//! `depth` mm past it, through the commands a front end drives.

#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use bimcraft_core::geom::P2;
    use bimcraft_core::model::{Element, FORMAT, Id, Model};
    use bimcraft_core::render::{self, POCHE, Prim};
    use bimcraft_core::view3d::Scene;
    use serde_json::{Value, json};

    fn run(s: &mut Session, command: &str, p: Value) -> Value {
        s.execute(command, &p).unwrap_or_else(|e| panic!("{command} {p}: {e}"))
    }

    fn fails(s: &mut Session, command: &str, p: Value) -> String {
        match s.execute(command, &p) {
            Ok(v) => panic!("{command} {p} was taken: {v}"),
            Err(e) => e,
        }
    }

    fn element(s: &mut Session, id: Id) -> Value {
        let all = run(s, "elements", json!({}));
        all["elements"].as_array().unwrap().iter().find(|e| e["id"] == json!(id)).cloned().unwrap()
    }

    /// Three walls square to a section's line, which looks north from y = -2000: one 300 mm past it, one 2000 mm, one
    /// 3000 mm, side by side along the line. The section's id comes back with theirs.
    struct Fixture {
        s: Session,
        section: Id,
        walls: [Id; 3],
    }

    fn fixture(near: Option<f64>, depth: Option<f64>) -> Fixture {
        let mut s = Session::new();
        let wall =
            |s: &mut Session, y: f64, x: f64| run(s, "wall", json!({"points": [[x, y], [x + 2000.0, y]]}))["ids"][0].as_u64().unwrap();
        let near_wall = wall(&mut s, -1700.0, 0.0);
        let middle = wall(&mut s, 0.0, 3000.0);
        let far = wall(&mut s, 1000.0, 6000.0);
        let mut p = json!({"start": [-1000, -2000], "end": [9000, -2000]});
        if let Some(n) = near {
            p["near"] = json!(n);
        }
        if let Some(d) = depth {
            p["depth"] = json!(d);
        }
        let section = run(&mut s, "section", p)["id"].as_u64().unwrap();
        Fixture { s, section, walls: [near_wall, middle, far] }
    }

    /// The element at `x` along the section's line, 1500 mm up, as its picture shows it; none where nothing is drawn.
    fn shown(f: &Fixture, x: f64) -> Option<Id> {
        let view = render::section_fit(&f.s.model, f.section, 900, 500);
        let image = render::section_image(&f.s.model, &Scene::new(&f.s.model), f.section, view, &[]).unwrap();
        let (px, py) = view.px(P2(x + 1000.0, 1500.0));
        image.pick(f64::from(px), f64::from(py))
    }

    fn walls_shown(f: &Fixture) -> [bool; 3] {
        [shown(f, 1000.0) == Some(f.walls[0]), shown(f, 4000.0) == Some(f.walls[1]), shown(f, 7000.0) == Some(f.walls[2])]
    }

    #[test]
    fn a_far_clip_alone_stops_at_its_depth_and_the_band_starts_at_the_line() {
        let mut f = fixture(None, Some(2150.0));
        assert_eq!(walls_shown(&f), [true, true, false], "the wall 3000 mm past the line is beyond the 2150 mm far clip");
        // Its band starts at its line: 0 at the camera, and the far clip measured from there.
        let (_, clip) = render::section_camera(&f.s.model, f.section, render::section_fit(&f.s.model, f.section, 900, 500)).unwrap();
        assert_eq!((clip.near, clip.far), (0.0, Some(2150.0)));
        assert_eq!(element(&mut f.s, f.section)["near"], json!(0.0));
    }

    #[test]
    fn a_near_clip_cuts_the_first_millimetres_of_the_band() {
        let mut f = fixture(Some(500.0), Some(2150.0));
        assert_eq!(walls_shown(&f), [false, true, false], "the wall 300 mm past the line is nearer than 500 mm");
        let (_, clip) = render::section_camera(&f.s.model, f.section, render::section_fit(&f.s.model, f.section, 900, 500)).unwrap();
        // The clip's depth is toward the camera: near -500, and its far clip measured from there to 2150 mm past the line.
        assert_eq!((clip.near, clip.far), (-500.0, Some(1650.0)));
        let shown = element(&mut f.s, f.section);
        assert_eq!((shown["near"].as_f64(), shown["depth"].as_f64()), (Some(500.0), Some(2150.0)));
    }

    #[test]
    fn a_near_clip_hides_the_plane_of_the_cut_itself() {
        let mut s = Session::new();
        // A wall running north across the section's line, so the line cuts it.
        run(&mut s, "wall", json!({"points": [[0, -2000], [0, 2000]]}));
        let make = |s: &mut Session, near: Option<f64>| {
            let mut p = json!({"start": [-2000, 0], "end": [4000, 0]});
            if let Some(n) = near {
                p["near"] = json!(n);
            }
            run(s, "section", p)["id"].as_u64().unwrap()
        };
        let poched = |id: Id, s: &Session| {
            let view = render::section_fit(&s.model, id, 600, 400);
            render::section(&s.model, id, view).prims.iter().any(|p| matches!(p, Prim::Fill { color, .. } if *color == POCHE))
        };
        let id = make(&mut s, None);
        assert!(poched(id, &s), "the wall is cut at the line");
        let id = make(&mut s, Some(500.0));
        assert!(!poched(id, &s), "with a near clip the plane on the line is cut away too");
    }

    #[test]
    fn near_zero_sees_from_the_line_as_before() {
        let mut f = fixture(Some(0.0), None);
        assert_eq!(walls_shown(&f), [true, true, true]);
        assert_eq!(element(&mut f.s, f.section)["near"], json!(0.0));
        // A near clip of 0 is its line, so the file keeps none.
        assert_eq!(f.s.model.section(f.section).unwrap().near, None);
        // No far clip: as it always was.
        let (_, clip) = render::section_camera(&f.s.model, f.section, render::section_fit(&f.s.model, f.section, 900, 500)).unwrap();
        assert_eq!((clip.near, clip.far), (0.0, None));
    }

    #[test]
    fn a_near_clip_with_no_far_clip_hides_only_the_start_of_the_band() {
        let f = fixture(Some(500.0), None);
        assert_eq!(walls_shown(&f), [false, true, true], "past 500 mm it sees everything");
    }

    #[test]
    fn a_near_clip_at_or_past_the_far_clip_is_refused_and_changes_nothing() {
        let mut f = fixture(Some(500.0), Some(2150.0));
        let before = f.s.model.to_json().unwrap();
        for (near, depth) in [(2150.0, 2150.0), (3000.0, 2150.0)] {
            let e = fails(&mut f.s, "modify", json!({"id": f.section, "near": near, "depth": depth}));
            assert!(e.contains("near clip") && e.contains("far clip"), "{e}");
        }
        assert_eq!(f.s.model.to_json().unwrap(), before, "a refused band leaves the model as it was");
        let v = f.s.model.section(f.section).unwrap();
        assert_eq!((v.near, v.depth), (Some(500.0), Some(2150.0)));
    }

    #[test]
    fn negative_and_unusable_clips_are_refused() {
        let mut s = Session::new();
        run(&mut s, "wall", json!({"points": [[0, 0], [2000, 0]]}));
        let line = json!({"start": [-1000, -2000], "end": [9000, -2000], "depth": 3000});
        for (key, value) in [("near", json!(-1)), ("near", json!(1e300)), ("depth", json!(-1)), ("depth", json!(1e300))] {
            let mut p = line.clone();
            p[key] = value;
            fails(&mut s, "section", p);
        }
        assert_eq!(run(&mut s, "elements", json!({"category": "section"}))["count"], json!(0), "no section was made");
    }

    #[test]
    fn a_nan_clip_is_refused_by_the_model_check() {
        let f = fixture(Some(500.0), Some(2150.0));
        let (mut m, _) = Model::read(&f.s.model.to_json().unwrap()).unwrap();
        let Some(Element::Section(v)) = m.elements.get_mut(f.section) else { panic!("no section") };
        v.near = Some(f64::NAN);
        let e = m.check().unwrap_err();
        assert!(e.contains("near"), "{e}");
    }

    #[test]
    fn modify_changes_either_clip_and_null_puts_it_back() {
        let mut f = fixture(Some(500.0), Some(2150.0));
        run(&mut f.s, "modify", json!({"id": f.section, "near": 400, "depth": 3000}));
        let v = f.s.model.section(f.section).unwrap();
        assert_eq!((v.near, v.depth), (Some(400.0), Some(3000.0)));
        assert_eq!(element(&mut f.s, f.section)["near"], json!(400.0));
        run(&mut f.s, "modify", json!({"id": f.section, "near": null, "depth": null}));
        let v = f.s.model.section(f.section).unwrap();
        assert_eq!((v.near, v.depth), (None, None), "null is 0 for the near clip and no far clip");
        let shown = element(&mut f.s, f.section);
        assert_eq!((shown["near"].as_f64(), shown["depth"].as_f64()), (Some(0.0), None));
    }

    #[test]
    fn crop_does_not_change_either_clip() {
        let mut f = fixture(Some(500.0), Some(2150.0));
        run(&mut f.s, "crop", json!({"view": f.section, "region": [[0, 0], [4000, 3000]]}));
        let v = f.s.model.section(f.section).unwrap();
        assert_eq!((v.near, v.depth), (Some(500.0), Some(2150.0)));
        assert_eq!(v.crop, Some([0.0, 3000.0]));
        assert_eq!(walls_shown(&f), [false, true, false], "the band it sees is the same");
    }

    #[test]
    fn scale_and_place_do_not_change_either_clip() {
        let mut f = fixture(Some(500.0), Some(2150.0));
        run(&mut f.s, "scale", json!({"view": f.section, "scale": 50}));
        run(&mut f.s, "sheet", json!({"number": "A101", "name": "Sections"}));
        run(&mut f.s, "place", json!({"sheet": "A101", "view": f.section}));
        let v = f.s.model.section(f.section).unwrap();
        assert_eq!((v.near, v.depth), (Some(500.0), Some(2150.0)));
        let (_, clip) = render::section_camera(&f.s.model, f.section, render::section_fit(&f.s.model, f.section, 900, 500)).unwrap();
        assert_eq!((clip.near, clip.far), (-500.0, Some(1650.0)), "the viewport draws the same band");
    }

    #[test]
    fn an_elevation_takes_both_clips_in_every_way_it_is_made() {
        let mut s = Session::new();
        run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
        let r = run(&mut s, "elevation", json!({"reference": [[0, 0], [4000, 0]], "at": [2000, -2000], "near": 500, "depth": 3000}));
        let id = r["id"].as_u64().unwrap();
        let v = s.model.section(id).unwrap();
        assert_eq!((v.near, v.depth), (Some(500.0), Some(3000.0)));
        let r = run(&mut s, "elevation", json!({"facing": "south", "near": 250, "depth": 4000}));
        let id = r["id"].as_u64().unwrap();
        let v = s.model.section(id).unwrap();
        assert_eq!((v.near, v.depth), (Some(250.0), Some(4000.0)));
        let r = run(&mut s, "elevation", json!({"start": [-1000, -1000], "end": [5000, -1000], "near": 300, "depth": 3500}));
        let id = r["id"].as_u64().unwrap();
        let v = s.model.section(id).unwrap();
        assert_eq!((v.near, v.depth), (Some(300.0), Some(3500.0)));
    }

    #[test]
    fn a_file_keeps_both_clips_and_an_old_one_opens_at_the_line() {
        let mut f = fixture(Some(500.0), Some(2150.0));
        let dir = std::env::temp_dir().join(format!("bimcraft-section-depth-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("depth.bimc").to_string_lossy().into_owned();
        run(&mut f.s, "save", json!({"path": path}));
        let mut back = Session::new();
        run(&mut back, "open", json!({"path": path}));
        let v = back.model.section(f.section).unwrap();
        assert_eq!((v.near, v.depth), (Some(500.0), Some(2150.0)));

        // A file of 21, read by 22, holds no near clip and sees from its line.
        let mut file: Value = serde_json::from_str(&back.model.to_json().unwrap()).unwrap();
        file["format"] = json!(21);
        let elements = file["elements"].as_object_mut().unwrap();
        elements[&f.section.to_string()].as_object_mut().unwrap().remove("near");
        let (m, left_out) = Model::read(&file.to_string()).unwrap();
        let v = m.section(f.section).unwrap();
        assert_eq!((m.format, v.depth, left_out.len()), (FORMAT, Some(2150.0), 0));
        assert!(v.near.is_none(), "a missing near clip is its line");
    }

    #[test]
    fn a_walls_type_says_whether_it_is_a_curtain_wall() {
        let mut s = Session::new();
        run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
        run(
            &mut s,
            "type",
            json!({"category": "wall", "name": "Glazing", "curtain": {"columns": {"segments": [1000.0]}, "rows": {"segments": [3000.0]}}}),
        );
        run(&mut s, "wall", json!({"points": [[0, 2000], [4000, 2000]], "type": "Glazing"}));
        let walls = run(&mut s, "elements", json!({"category": "wall"}));
        let flags: Vec<bool> = walls["elements"].as_array().unwrap().iter().map(|w| w["curtain_wall"].as_bool().unwrap()).collect();
        assert!(flags.contains(&true) && flags.contains(&false), "{flags:?}");
        // It is read, never written: modify refuses it as a field of a wall.
        let plain =
            walls["elements"].as_array().unwrap().iter().find(|w| !w["curtain_wall"].as_bool().unwrap()).unwrap()["id"].as_u64().unwrap();
        let e = fails(&mut s, "modify", json!({"id": plain, "curtain_wall": true}));
        assert!(e.contains("curtain_wall"), "{e}");
    }

    #[test]
    fn a_section_without_clips_draws_what_it_always_did() {
        let with = fixture(Some(0.0), None);
        let without = fixture(None, None);
        assert_eq!(walls_shown(&with), walls_shown(&without));
        assert_eq!(with.s.model.section(with.section).unwrap().near, None);
        let picture = |f: &Fixture| {
            let view = render::section_fit(&f.s.model, f.section, 900, 500);
            render::section_image(&f.s.model, &Scene::new(&f.s.model), f.section, view, &[]).unwrap().rgba
        };
        assert_eq!(picture(&with), picture(&without));
    }

    #[test]
    fn both_clips_reach_the_rendered_picture() {
        let f = fixture(Some(500.0), Some(2150.0));
        assert_eq!((f.s.model.section(f.section).unwrap().near, f.s.model.section(f.section).unwrap().depth), (Some(500.0), Some(2150.0)));
        assert_eq!(walls_shown(&f), [false, true, false]);
        let without = fixture(None, None);
        let picture = |f: &Fixture| {
            let view = render::section_fit(&f.s.model, f.section, 900, 500);
            render::section_image(&f.s.model, &Scene::new(&f.s.model), f.section, view, &[]).unwrap().rgba
        };
        let band = picture(&f);
        let all = picture(&without);
        assert_ne!(band, all, "a band's picture differs from the whole view's");
        assert_eq!(walls_shown(&without), [true, true, true], "without clips the same model shows all three");
    }
}
