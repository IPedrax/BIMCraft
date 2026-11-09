//! Default sheet scales exercised through the same commands as CLI and MCP users.
#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use bimcraft_core::model::Element;
    use bimcraft_core::{render, sheet};
    use serde_json::{Value, json};

    fn building(width: f64, depth: f64) -> Session {
        let mut s = Session::new();
        s.execute("wall", &json!({"points": [[0, 0], [width, 0], [width, depth], [0, depth]], "closed": true})).unwrap();
        s.execute("sheet", &json!({"number": "A101", "size": "A3", "name": "Synthetic 3D"})).unwrap();
        s
    }

    fn viewport(s: &mut Session) -> Value {
        s.execute("elements", &json!({"category": "viewport"})).unwrap()["elements"][0].clone()
    }

    #[test]
    fn large_default_3d_view_fits_the_sheet_and_keeps_its_picture_after_save_and_reopen() {
        let mut s = building(60000.0, 40000.0);
        let placed = s.execute("place", &json!({"sheet": "A101", "view": "{3D}"})).unwrap();
        let vp = viewport(&mut s);
        assert_eq!(vp["scale"], 500.0, "a 1 : 100 picture would be 734 x 405 mm on A3");
        assert_eq!(placed["scale"], vp["scale"], "the command reports the scale it chose");

        let id = placed["id"].as_u64().unwrap();
        let (frame, (x, y)) = sheet::frame(&s.model, id).unwrap();
        // ISO A3: left 20, other borders 10, title box 40 mm high. Keep the frame and
        // its title above the title box and inside all borders, in paper pixels.
        assert!(x >= 25.0 * 5.0 && x + frame.width as f32 <= 405.0 * 5.0);
        assert!(y >= 15.0 * 5.0 && y + frame.height as f32 + 15.0 * 5.0 <= 242.0 * 5.0);
        let page = s.model.sheet_numbered("A101").unwrap();
        let drawing = sheet::draw(&s.model, page).unwrap();
        let png = render::png(&drawing).unwrap();
        let pixels = tiny_skia::Pixmap::decode_png(&png).unwrap();
        let border = pixels.pixel(100, 750).unwrap();
        assert!(border.red() < 120 && border.green() < 120 && border.blue() < 120, "the view must not hide the left sheet border");
        let picture = drawing
            .prims
            .iter()
            .find_map(|p| match p {
                render::Prim::Image { rgba, .. } => Some(rgba),
                _ => None,
            })
            .unwrap();
        assert!(
            picture.as_chunks::<4>().0.iter().filter(|p| p[0] < 230 && p[1] < 230 && p[2] < 230).count() > 1000,
            "the fitted view contains the building"
        );

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sheet.bimcraft");
        s.execute("save", &json!({"path": path})).unwrap();
        let mut reopened = Session::new();
        let opened = reopened.execute("open", &json!({"path": path})).unwrap();
        assert!(opened.get("left_out").is_none(), "{opened}");
        assert_eq!(viewport(&mut reopened), vp, "the chosen viewport scale and position survive native save/open");
        let page = reopened.model.sheet_numbered("A101").unwrap();
        assert_eq!(render::png(&sheet::draw(&reopened.model, page).unwrap()).unwrap(), png);
    }

    #[test]
    fn fitting_uses_the_current_standard_sheet_margins() {
        // Seen horizontally, this model needs 379 mm at 1 : 200: ISO/JIS leave
        // 380 mm after clearance; NBR leaves 378 and ANSI leaves 378.25 mm.
        for (standard, expected) in [("iso", 200.0), ("eu", 200.0), ("jis", 200.0), ("nbr", 500.0), ("ansi", 500.0)] {
            let mut s = building(60000.0, 40000.0);
            s.execute("standard", &json!({"set": standard})).unwrap();
            // ANSI adapts project units too; isolate its paper margins here.
            s.execute("units", &json!({"length": "mm"})).unwrap();
            s.execute("place", &json!({"sheet": "A101", "view": "{3D}", "elevation": 0, "azimuth": -45})).unwrap();
            assert_eq!(viewport(&mut s)["scale"], expected, "{standard}");
        }
    }

    #[test]
    fn fitting_uses_the_finest_offered_imperial_scale_that_fits() {
        let mut s = building(60000.0, 40000.0);
        s.execute("units", &json!({"length": "in"})).unwrap();
        s.execute("place", &json!({"sheet": "A101", "view": "{3D}"})).unwrap();
        assert_eq!(viewport(&mut s)["scale"], 240.0, "the offered imperial scales are not sorted numerically");
    }

    #[test]
    fn a_small_or_empty_3d_view_keeps_the_project_default_scale() {
        for mut s in [building(8000.0, 6000.0), Session::new()] {
            if s.model.sheet_numbered("A101").is_none() {
                s.execute("sheet", &json!({"number": "A101"})).unwrap();
            }
            s.execute("place", &json!({"sheet": "A101", "view": "{3D}"})).unwrap();
            assert_eq!(viewport(&mut s)["scale"], 100.0);
        }
    }

    #[test]
    fn explicit_viewport_scale_and_position_are_preserved_even_when_the_picture_does_not_fit() {
        let mut s = building(60000.0, 40000.0);
        s.execute("place", &json!({"sheet": "A101", "view": "{3D}", "scale": 100, "at": [10, 20]})).unwrap();
        let vp = viewport(&mut s);
        assert_eq!(vp["scale"], 100.0);
        assert_eq!(vp["at"], json!([10.0, 20.0]));
        assert!(vp["size"][0].as_f64().unwrap() > 420.0);
    }

    #[test]
    fn explicit_position_is_preserved_when_a_default_3d_scale_is_fitted() {
        let mut s = building(60000.0, 40000.0);
        s.execute("place", &json!({"sheet": "A101", "view": "{3D}", "at": [90, 120]})).unwrap();
        let vp = viewport(&mut s);
        assert_eq!(vp["scale"], 500.0);
        assert_eq!(vp["at"], json!([90.0, 120.0]));
    }

    #[test]
    fn assigned_view_and_template_scales_are_preserved() {
        for template in [false, true] {
            let mut s = building(60000.0, 40000.0);
            s.execute("visual_style", &json!({"style": "wireframe"})).unwrap();
            let view = s.model.view_named("{3D}").unwrap();
            if template {
                s.execute("template", &json!({"name": "3D scale", "kind": "3d", "controls": ["scale"], "scale": 200})).unwrap();
                s.execute("graphics", &json!({"view": view, "template": "3D scale"})).unwrap();
            } else {
                s.execute("modify", &json!({"id": view, "graphics": {"scale": 200}})).unwrap();
            }
            s.execute("place", &json!({"sheet": "A101", "view": view})).unwrap();
            assert_eq!(viewport(&mut s)["scale"], 200.0, "template={template}");
        }
    }

    #[test]
    fn fitting_falls_back_to_the_coarsest_offered_scale_when_no_scale_fits() {
        let mut s = building(900000.0, 600000.0);
        s.execute("place", &json!({"sheet": "A101", "view": "{3D}"})).unwrap();
        let vp = viewport(&mut s);
        assert_eq!(vp["scale"], 1000.0);
        assert!(vp["size"][0].as_f64().unwrap() > 420.0, "offered scales cannot fit every model");
    }

    #[test]
    fn plans_keep_their_assigned_scale_instead_of_fitting_to_paper() {
        let mut s = building(60000.0, 40000.0);
        s.execute("scale", &json!({"view": "Level 1", "scale": 50})).unwrap();
        s.execute("place", &json!({"sheet": "A101", "view": "Level 1"})).unwrap();
        assert_eq!(viewport(&mut s)["scale"], 50.0);
    }

    #[test]
    fn invalid_explicit_scale_does_not_leave_a_view_or_viewport_behind() {
        let mut s = building(60000.0, 40000.0);
        assert!(s.execute("place", &json!({"sheet": "A101", "view": "{3D}", "scale": 0})).is_err());
        assert!(s.model.view_named("{3D}").is_none());
        assert!(!s.model.elements.values().any(|e| matches!(e, Element::Viewport(_))));
    }
}
