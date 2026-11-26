//! Point clouds in the window, driven as a user drives them: Insert > Point Cloud brings scans in through its box, the
//! Properties palette shows and moves one, and Visibility/Graphics hides them as a category.

use std::path::PathBuf;

use super::*;

/// A scan file in a folder of this test's own: a wall face 2 m long and 2 m tall at `y` m, in metres.
fn face(test: &str, name: &str, y: f64) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-clouds-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let text: String =
        (0..=50).flat_map(|i| (0..=50).map(move |k| format!("{} {y} {}\n", f64::from(i) * 0.04, f64::from(k) * 0.04))).collect();
    std::fs::write(&path, text).unwrap();
    path
}

fn scans(h: &Harness<BimApp>) -> Vec<Id> {
    let m = &h.state().session.model;
    m.elements.iter().filter(|(_, e)| matches!(e, Element::PointCloud(_))).map(|(id, _)| *id).collect()
}

#[test]
fn insert_point_cloud_brings_the_files_picked_in_as_one_scan_and_selects_it() {
    let mut h = app(BOX);
    press(&mut h, "Insert");
    assert!(h.query_by_label("Link Model").is_some(), "the Insert tab links models too");
    // The system's dialog gives the files; a test hands them over as it would.
    let (a, b) = (face("insert", "east.xyz", 0.0), face("insert", "west.xyz", 3.0));
    h.state_mut().point_cloud_paths(vec![a, b]);
    h.run();
    assert!(h.query_by_label("2 files, brought in as one scan's stations").is_some());
    type_into(&mut h, "Name", "survey");
    type_into(&mut h, "Spacing (mm)", "100");
    pick(&mut h, Role::RadioButton, "Auto - Origin to Origin");
    press(&mut h, "OK");
    let placed = scans(&h);
    assert_eq!(placed.len(), 1);
    let m = &h.state().session.model;
    let cloud = m.clouds.get("survey").unwrap();
    assert_eq!(cloud.stations().len(), 2);
    // Origin to Origin: where the files put it.
    let (lo, hi) = m.cloud_box(placed[0]).unwrap();
    assert!((lo[1] - 0.0).abs() < 1.0 && (hi[1] - 3000.0).abs() < 1.0, "{lo:?} {hi:?}");
    assert_eq!(h.state().selection, placed, "what was brought in is selected, as Revit does");
    assert!(h.query_by_label("2 files, brought in as one scan's stations").is_none(), "the box closed");
}

#[test]
fn a_scan_that_cant_come_in_says_why_in_its_box() {
    let mut h = app(BOX);
    let bad = face("bad", "scan.rcp", 0.0);
    h.state_mut().point_cloud_paths(vec![bad]);
    h.run();
    press(&mut h, "OK");
    assert!(scans(&h).is_empty());
    assert!(h.query_by_label_contains("export the scan from ReCap").is_some(), "the reason shows in the box, which stays open");
    press(&mut h, "Cancel");
    assert!(h.query_by_label("OK").is_none());
}

#[test]
fn properties_show_a_scan_and_move_it() {
    let path = face("properties", "face.xyz", 0.0);
    let mut h = app(&format!("point_cloud {}", json!({"path": path})));
    let id = scans(&h)[0];
    h.state_mut().selection = vec![id];
    h.run();
    // (File is the File menu's too.)
    for label in ["Scan", "Points", "Offset", "Angle"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    assert!(h.query_by_label("face.xyz").is_some());
    type_into(&mut h, "Offset", "500");
    h.key_press(Key::Enter);
    h.run();
    let Some(Element::PointCloud(pc)) = h.state().session.model.elements.get(id) else { panic!() };
    assert_eq!(pc.offset, 500.0);
}

/// The fill the active plan's graphics give point clouds, and whether they halftone them.
fn scan_fill(h: &Harness<BimApp>) -> (Option<[u8; 3]>, bool) {
    let g = h.state().graphics_shown();
    g.categories.get("pointcloud").map_or((None, false), |o| (o.projection.fill, o.halftone))
}

/// Settings > Graphics' High contrast colour, set as a user does: the colour well opened, then each of R, G and B typed into its
/// field (a field shows `R 220` and, while it is edited, `220`).
fn pick_colour(h: &mut Harness<BimApp>, to: [u8; 3]) {
    h.get_by_role_and_label(Role::ColorWell, "High contrast colour").click();
    h.run();
    for (k, letter) in ["R", "G", "B"].into_iter().enumerate() {
        let field = |text: String| {
            move |n: &egui_kittest::kittest::AccessKitNode<'_>| n.role() == Role::SpinButton && n.value().as_deref() == Some(&text)
        };
        let now = h.state().scan_contrast[k];
        h.get_by(field(format!("{letter} {now}"))).click();
        h.run();
        h.key_press_modifiers(Modifiers::COMMAND, Key::A);
        h.get_by(field(now.to_string())).type_text(&to[k].to_string());
        h.run();
        key(h, Key::Enter);
    }
}

/// The tint of each picture the window painted this frame: the plan's scans, the one thing in it drawn as a texture besides text.
fn picture_tints(h: &Harness<BimApp>) -> Vec<egui::Color32> {
    fn walk(s: &egui::Shape, out: &mut Vec<egui::Color32>) {
        match s {
            egui::Shape::Mesh(m) if m.vertices.len() == 4 && m.texture_id != egui::TextureId::default() => {
                out.extend(m.vertices.first().map(|v| v.color));
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

#[test]
fn high_contrast_on_the_view_control_bar_fills_the_views_point_clouds_with_one_colour_and_takes_it_off_again() {
    // With no scan there is nothing to set.
    let h = app(BOX);
    assert!(h.query_by_label("High Contrast Point Clouds").is_none());
    let path = face("contrast", "face.xyz", 0.0);
    let mut h = app(&format!("point_cloud {}", json!({"path": path})));
    // What the category has besides stays: here, halftone.
    h.state_mut().run("graphics", json!({"categories": {"pointcloud": {"halftone": true}}}));
    h.run();
    assert!(!lit(&h, "High Contrast Point Clouds"));
    press(&mut h, "High Contrast Point Clouds");
    assert_eq!(scan_fill(&h), (Some(crate::footer::HIGH_CONTRAST), true));
    assert!(lit(&h, "High Contrast Point Clouds"));
    // The plan draws the scan as ink, which a dark canvas doesn't dim.
    let m = &h.state().session.model;
    let d = bimcraft_core::render::plan(m, m.levels()[0].0, 600, 600);
    assert!(d.prims.iter().any(|p| matches!(p, bimcraft_core::render::Prim::Image { ink: true, .. })));
    // Again, and the scans have their own colours back, with the rest of the override kept.
    press(&mut h, "High Contrast Point Clouds");
    assert_eq!(scan_fill(&h), (None, true));
    assert!(!lit(&h, "High Contrast Point Clouds"));
    // One step each: undo puts the colour back.
    h.state_mut().run("undo", json!({}));
    h.run();
    assert_eq!(scan_fill(&h).0, Some(crate::footer::HIGH_CONTRAST));
}

#[test]
fn high_contrast_is_lit_whenever_the_view_fills_its_point_clouds_and_off_takes_any_fill_away() {
    let path = face("contrast-vv", "face.xyz", 0.0);
    let mut h = app(&format!("point_cloud {}", json!({"path": path})));
    // A colour of the user's own, given in Visibility/Graphics.
    h.state_mut().run("graphics", json!({"categories": {"pointcloud": {"projection": {"fill": [0, 0, 255]}}}}));
    h.run();
    assert!(lit(&h, "High Contrast Point Clouds"));
    press(&mut h, "High Contrast Point Clouds");
    assert_eq!(scan_fill(&h), (None, false));
    let g = h.state().graphics_shown();
    assert!(!g.categories.contains_key("pointcloud"), "nothing is left to override: {:?}", g.categories);
}

#[test]
fn the_high_contrast_colour_is_the_users_kept_in_settings_and_the_toggle_uses_it() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let path = face("contrast-settings", "face.xyz", 0.0);
    let mut h = app(&format!("point_cloud {}", json!({"path": path})));
    h.state_mut().use_settings(file.clone());
    assert_eq!(h.state().scan_contrast, crate::footer::HIGH_CONTRAST, "crimson until the user picks another");
    press(&mut h, "Settings");
    // (Properties has a Graphics group too.)
    h.state_mut().settings_page = Some(crate::settings::Page::Graphics);
    h.run();
    assert!(h.query_by_label("High contrast colour").is_some());
    // The picker sets it: the colour well opened, each of R, G and B typed into its field.
    pick_colour(&mut h, [0, 90, 255]);
    assert_eq!(h.state().scan_contrast, [0, 90, 255]);
    press(&mut h, "Close");
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(kept["high_contrast"], json!([0, 90, 255]));
    press(&mut h, "High Contrast Point Clouds");
    assert_eq!(scan_fill(&h).0, Some([0, 90, 255]));
    // Another window starts with it.
    let mut again = app("");
    again.state_mut().use_settings(file.clone());
    assert_eq!(again.state().scan_contrast, [0, 90, 255]);
    // Reset Colour puts crimson back.
    press(&mut again, "Settings");
    // (Properties has a Graphics group too.)
    again.state_mut().settings_page = Some(crate::settings::Page::Graphics);
    again.run();
    press(&mut again, "Reset Colour");
    assert_eq!(again.state().scan_contrast, crate::footer::HIGH_CONTRAST);
    // What isn't a colour keeps the default.
    for bad in [r#"{"high_contrast": [300, 0, 0]}"#, r#"{"high_contrast": [1, 2]}"#, r#"{"high_contrast": "red"}"#] {
        std::fs::write(&file, bad).unwrap();
        let mut fresh = app("");
        fresh.state_mut().use_settings(file.clone());
        assert_eq!(fresh.state().scan_contrast, crate::footer::HIGH_CONTRAST, "{bad}");
    }
}

#[test]
fn visibility_graphics_lists_point_clouds_with_the_model_categories() {
    let path = face("vg", "face.xyz", 0.0);
    let mut h = app(&format!("point_cloud {}", json!({"path": path})));
    press(&mut h, "View");
    press(&mut h, "Visibility/Graphics");
    // Last of the model's categories, it is found by name.
    type_into(&mut h, "Category name search:", "point");
    pick(&mut h, Role::CheckBox, "Point Clouds");
    press(&mut h, "OK");
    let m = &h.state().session.model;
    let level = m.levels()[0].0;
    assert!(!bimcraft_core::graphics::Look::of(m, level).shows(scans(&h)[0]), "the plan hides them now");
}

#[test]
fn new_and_open_keep_the_users_high_contrast_colour_and_settings_json_keeps_it_too() {
    let dir = tempfile::tempdir().unwrap();
    let (file, project) = (dir.path().join("settings.json"), dir.path().join("house.bimcraft"));
    let blue = [0, 90, 255];
    std::fs::write(&file, json!({"high_contrast": blue}).to_string()).unwrap();
    let kept = || serde_json::from_str::<Value>(&std::fs::read_to_string(&file).unwrap()).unwrap()["high_contrast"].clone();
    let mut h = app(BOX);
    h.state_mut().use_settings(file.clone());
    assert!(h.state_mut().save_to(&project), "saved, so New and Open start over without asking");
    h.run();
    assert_eq!((h.state().scan_contrast, kept()), (blue, json!(blue)));
    // File > New (Ctrl+N) starts the window over on an empty project. Its colour is the user's still, and the next frames don't
    // write crimson over the settings file.
    h.key_press_modifiers(Modifiers::COMMAND, Key::N);
    h.run();
    empty_project(&mut h);
    h.run();
    assert!(ids(&h, "wall").is_empty(), "the project is a new one");
    assert_eq!((h.state().scan_contrast, kept()), (blue, json!(blue)), "after New");
    // Open (what the dialog, Open Recent and the Start page call) does the same.
    assert!(h.state_mut().open_path(&project));
    h.run();
    h.run();
    assert_eq!(ids(&h, "wall").len(), 4, "the project is the one on file");
    assert_eq!((h.state().scan_contrast, kept()), (blue, json!(blue)), "after Open");
}

#[test]
fn high_contrast_is_greyed_out_in_a_view_following_a_template_that_controls_the_categories_and_says_whose() {
    let path = face("template", "face.xyz", 0.0);
    let mut h = app(&format!("point_cloud {}", json!({"path": path})));
    let run = |h: &mut Harness<BimApp>, p: Value| h.state_mut().session.execute("graphics", &p).unwrap();
    // A template made from the view holds the colour it had, and the view follows it.
    run(&mut h, json!({"view": 100000, "categories": {"pointcloud": {"projection": {"fill": [0, 0, 255]}}}}));
    h.state_mut().session.execute("template", &json!({"name": "Scans", "from": 100000, "controls": ["categories"]})).unwrap();
    run(&mut h, json!({"view": 100000, "reset": true, "template": "Scans"}));
    h.run();
    assert!(lit(&h, "High Contrast Point Clouds"), "the template's colour is the view's");
    assert!(is_disabled(&h, Role::Button, "High Contrast Point Clouds"));
    // A click changes nothing: not the view, not the template.
    let steps = h.state().session.history().len();
    h.get_by_role_and_label(Role::Button, "High Contrast Point Clouds").click();
    h.run();
    assert_eq!(h.state().session.history().len(), steps);
    assert_eq!(scan_fill(&h).0, Some([0, 0, 255]));
    // Hovering says whose it is.
    let at = h.get_by_role_and_label(Role::Button, "High Contrast Point Clouds").rect().center();
    h.hover_at(at);
    for _ in 0..4 {
        h.run();
    }
    let note = "Set by view template Scans: change the template, or stop following it.";
    assert!(painted_text(&h).contains(&note.to_string()), "{:?}", painted_text(&h));
    // Stopped following, the view's own categories are its own again, and the button works.
    run(&mut h, json!({"view": 100000, "template": null}));
    h.run();
    assert!(!is_disabled(&h, Role::Button, "High Contrast Point Clouds") && !lit(&h, "High Contrast Point Clouds"));
    press(&mut h, "High Contrast Point Clouds");
    assert_eq!(scan_fill(&h).0, Some(crate::footer::HIGH_CONTRAST));
}

#[test]
fn high_contrast_is_on_the_3d_views_bar_and_a_sections_and_fills_the_scans_of_that_view_alone() {
    use bimcraft_core::graphics::Look;
    use bimcraft_core::render::Rgb;
    let path = face("views", "face.xyz", 0.0);
    let script = format!("section {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}\npoint_cloud {}", json!({"path": path}));
    let mut h = app(&script);
    let (scan, section, level) = (scans(&h)[0], ids(&h, "section")[0], h.state().session.model.levels()[0].0);
    let [r, g, b] = crate::footer::HIGH_CONTRAST;
    let filled = |h: &Harness<BimApp>, view: Option<Id>| {
        let m = &h.state().session.model;
        let look = view.map_or_else(|| Look::of_3d(m), |v| Look::of(m, v));
        look.surface(scan, Rgb(1, 2, 3)) == Rgb(r, g, b) && look.filled(scan)
    };
    // The 3D view: the default one, which the project holds only once something sets it.
    press(&mut h, "Default 3D View");
    assert_eq!(h.state().sheet, Sheet::View3D);
    assert!(!lit(&h, "High Contrast Point Clouds"));
    press(&mut h, "High Contrast Point Clouds");
    assert_eq!(scan_fill(&h).0, Some(crate::footer::HIGH_CONTRAST));
    assert!(lit(&h, "High Contrast Point Clouds") && filled(&h, None));
    // A section keeps its own graphics, and the 3D view's don't reach it.
    browse(&mut h, "Section 1");
    assert_eq!(h.state().sheet, Sheet::Section(section));
    assert!(!lit(&h, "High Contrast Point Clouds") && !filled(&h, Some(section)));
    press(&mut h, "High Contrast Point Clouds");
    assert_eq!(scan_fill(&h).0, Some(crate::footer::HIGH_CONTRAST));
    assert!(lit(&h, "High Contrast Point Clouds") && filled(&h, Some(section)));
    assert!(!filled(&h, Some(level)), "the plan was never asked");
    // Again, and each has its own colours back.
    press(&mut h, "High Contrast Point Clouds");
    assert_eq!(scan_fill(&h).0, None);
    assert!(!lit(&h, "High Contrast Point Clouds") && !filled(&h, Some(section)));
    press(&mut h, "Default 3D View");
    assert!(lit(&h, "High Contrast Point Clouds"), "the 3D view is as it was left");
    press(&mut h, "High Contrast Point Clouds");
    assert!(!lit(&h, "High Contrast Point Clouds") && !filled(&h, None));
}

#[test]
fn a_dark_canvas_dims_a_scans_own_colours_but_not_the_colour_high_contrast_gives_it() {
    let path = face("dark", "face.xyz", 0.0);
    let mut h = app(&format!("point_cloud {}", json!({"path": path})));
    press(&mut h, "View");
    press(&mut h, "Dark Mode");
    pick(&mut h, Role::RadioButton, "Dark");
    assert!(crate::style::dark());
    // The scan's own colours are mid-tones: dimmed, so the ink lightened over them still reads.
    assert_eq!(picture_tints(&h), [egui::Color32::from_gray(120)]);
    // The colour High Contrast gives it is ink itself, shown as it is.
    press(&mut h, "High Contrast Point Clouds");
    assert_eq!(picture_tints(&h), [egui::Color32::WHITE]);
    // Again, and they are dimmed as before.
    press(&mut h, "High Contrast Point Clouds");
    assert_eq!(picture_tints(&h), [egui::Color32::from_gray(120)]);
    // On the light canvas nothing is dimmed.
    press(&mut h, "Dark Mode");
    pick(&mut h, Role::RadioButton, "Light");
    assert!(!crate::style::dark());
    assert_eq!(picture_tints(&h), [egui::Color32::WHITE]);
}

#[test]
fn a_scan_on_a_light_canvas_in_a_dark_interface_is_not_dimmed() {
    let path = face("mixed", "face.xyz", 0.0);
    let mut h = app(&format!("point_cloud {}", json!({"path": path})));
    press(&mut h, "View");
    press(&mut h, "Dark Mode");
    pick(&mut h, Role::RadioButton, "Dark UI, Light Canvas");
    // The interface is dark, the drawing is not: the dimming is for a dark canvas, which a dark interface does not make.
    assert!(crate::style::dark() && !crate::style::canvas_dark());
    assert_eq!(picture_tints(&h), [egui::Color32::WHITE]);
    press(&mut h, "Dark Mode");
    pick(&mut h, Role::RadioButton, "Dark");
    assert_eq!(picture_tints(&h), [egui::Color32::from_gray(120)], "a dark canvas dims it again");
}

#[test]
fn high_contrast_is_offered_where_only_a_linked_model_holds_the_scans_and_fills_them() {
    use bimcraft_core::graphics::Look;
    let dir = tempfile::tempdir().unwrap();
    let survey = dir.path().join("survey.bimcraft");
    let mut s = Session::new();
    s.execute("point_cloud", &json!({"path": face("link", "face.xyz", 0.0)})).unwrap();
    s.execute("save", &json!({"path": survey})).unwrap();
    let mut h = app("");
    assert!(h.query_by_label("High Contrast Point Clouds").is_none(), "no scan, no button");
    h.state_mut().link_path(&survey);
    h.run();
    // The project holds no scan of its own: the link builds one, which the plan draws.
    assert!(h.state().session.model.clouds.is_empty() && scans(&h).is_empty());
    press(&mut h, "High Contrast Point Clouds");
    let drawn = h.state_mut().view_model();
    let linked = drawn.elements.iter().find(|(_, e)| matches!(e, Element::PointCloud(_))).map(|(id, _)| *id).unwrap();
    let level = drawn.levels()[0].0;
    assert!(Look::of(&drawn, level).filled(linked), "the linked scan is drawn in the colour");
    assert!(lit(&h, "High Contrast Point Clouds"));
}

#[test]
fn high_contrast_goes_with_the_last_scan_placed_and_comes_back_with_it() {
    let path = face("last", "face.xyz", 0.0);
    let mut h = app(&format!("point_cloud {}", json!({"path": path})));
    assert!(h.query_by_label("High Contrast Point Clouds").is_some());
    // The bar asks only whether the project holds a scan; a scan nothing places is dropped by the command that left it so.
    let placed = scans(&h);
    h.state_mut().run("delete", json!({"ids": placed}));
    h.run();
    assert!(h.state().session.model.clouds.is_empty());
    assert!(h.query_by_label("High Contrast Point Clouds").is_none());
    h.state_mut().run("undo", json!({}));
    h.run();
    assert!(h.query_by_label("High Contrast Point Clouds").is_some());
}

#[test]
fn a_scan_with_a_colour_of_its_own_keeps_it_and_the_buttons_tip_says_so() {
    use bimcraft_core::graphics::Look;
    use bimcraft_core::render::Rgb;
    let path = face("own", "face.xyz", 0.0);
    let mut h = app(&format!("point_cloud {}", json!({"path": path})));
    let scan = scans(&h)[0];
    // A colour given to this scan alone comes after the category's, whatever the category says.
    h.state_mut().run("graphics", json!({"elements": {scan.to_string(): {"projection": {"fill": [0, 255, 0]}}}}));
    h.run();
    press(&mut h, "High Contrast Point Clouds");
    let m = &h.state().session.model;
    assert_eq!(Look::of(m, m.levels()[0].0).surface(scan, Rgb(1, 2, 3)), Rgb(0, 255, 0), "the scan keeps its own colour");
    assert!(lit(&h, "High Contrast Point Clouds"), "the button lights for the category, which is what it sets");
    // The tip says the button doesn't reach it.
    let at = h.get_by_role_and_label(Role::Button, "High Contrast Point Clouds").rect().center();
    h.hover_at(at);
    for _ in 0..4 {
        h.run();
    }
    let said = painted_text(&h).into_iter().find(|t| t.starts_with("High Contrast Point Clouds:")).unwrap_or_default();
    assert!(said.contains("a colour of its own in Visibility/Graphics keeps it"), "{said:?}");
}

/// A scan file big enough to be read on a thread of its own: a wall face like [`face`]'s, its points 1 cm apart.
fn big_face(test: &str, name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-clouds-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let text: String =
        (0..=200).flat_map(|i| (0..=200).map(move |k| format!("{} 0 {}\n", f64::from(i) * 0.01, f64::from(k) * 0.01))).collect();
    std::fs::write(&path, text).unwrap();
    assert!(std::fs::metadata(&path).unwrap().len() >= crate::clouds::IN_BACKGROUND, "the test's file is read in the frame");
    path
}

#[test]
fn a_big_scan_is_read_on_a_thread_of_its_own_with_its_progress_and_comes_in_as_one_undo_step() {
    let mut h = app(BOX);
    let before = h.state().session.history().len();
    h.state_mut().point_cloud_paths(vec![big_face("big", "survey.xyz")]);
    h.run();
    press(&mut h, "OK");
    // The window goes on drawing while the file is read, and the box says so, until the scan comes in.
    let mut frames = 0;
    while h.state().cloud_reading.is_some() {
        assert!(h.query_by_label_contains("Reading survey.xyz").is_some(), "the progress box shows while it's read");
        h.run();
        frames += 1;
        assert!(frames < 2000, "it was never read");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let placed = scans(&h);
    assert_eq!(placed.len(), 1, "{}", h.state().status.text);
    assert_eq!(h.state().session.model.clouds.get("survey").map(|c| c.read()), Some(201 * 201));
    assert_eq!(h.state().selection, placed, "what was brought in is selected");
    assert_eq!(h.state().session.history().len(), before + 1, "one undo step");
    assert!(h.query_by_label_contains("Reading survey.xyz").is_none() && h.query_by_label("OK").is_none(), "both boxes closed");
}

#[test]
fn a_big_scan_that_cant_come_in_says_why_in_its_box_once_read() {
    let mut h = app(BOX);
    h.state_mut().point_cloud_paths(vec![big_face("big-bad", "survey.xyz")]);
    h.run();
    type_into(&mut h, "Spacing (mm)", "-5");
    press(&mut h, "OK");
    for _ in 0..2000 {
        if h.state().cloud_reading.is_none() {
            break;
        }
        h.run();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.run();
    assert!(scans(&h).is_empty());
    assert!(h.query_by_label_contains("spacing").is_some(), "the reason shows in the box, which stays open");
    press(&mut h, "Cancel");
    assert!(h.query_by_label("OK").is_none());
}

/// What the view colours its point clouds by, as the category's override says.
fn scan_mode(h: &Harness<BimApp>) -> Option<bimcraft_core::graphics::ScanColour> {
    h.state().graphics_shown().categories.get("pointcloud").and_then(|o| o.scan)
}

#[test]
fn the_view_control_bar_colours_a_views_point_clouds_by_height_or_intensity() {
    use bimcraft_core::graphics::{ScanBy, ScanColour};
    let h = app(BOX);
    assert!(h.query_by_label("Point Cloud Colours").is_none(), "with no scan there is nothing to colour");
    let mut h = app(&format!("point_cloud {}", json!({"path": face("modes", "face.xyz", 0.0)})));
    press(&mut h, "Point Cloud Colours");
    press(&mut h, "Elevation");
    assert_eq!(scan_mode(&h), Some(ScanColour { by: ScanBy::Elevation, ..ScanColour::default() }));
    press(&mut h, "Point Cloud Colours");
    press(&mut h, "Intensity");
    assert_eq!(scan_mode(&h).map(|c| c.by), Some(ScanBy::Intensity));
    // Normal takes the mode away: the scans' own colours.
    press(&mut h, "Point Cloud Colours");
    press(&mut h, "Normal");
    assert_eq!(scan_mode(&h), None);
    // One step each.
    h.state_mut().run("undo", json!({}));
    h.run();
    assert_eq!(scan_mode(&h).map(|c| c.by), Some(ScanBy::Intensity));
}

#[test]
fn visibility_graphics_sets_the_point_clouds_colours_and_the_heights_their_ramp_runs_between() {
    use bimcraft_core::graphics::{ScanBy, ScanColour};
    let mut h = app(&format!("point_cloud {}", json!({"path": face("vg-modes", "face.xyz", 0.0)})));
    shortcut(&mut h, "VG");
    assert!(h.query_by_role_and_label(Role::ComboBox, "Color Mode").is_some(), "the Model Categories tab has the scans' colours");
    choose(&mut h, "Color Mode", "Elevation");
    type_into(&mut h, "Low", "500");
    type_into(&mut h, "High", "1500");
    press(&mut h, "OK");
    assert_eq!(scan_mode(&h), Some(ScanColour { by: ScanBy::Elevation, low: Some(500.0), high: Some(1500.0) }));
}

/// Frames until the scan being read on a thread of its own has come in.
fn read_in(h: &mut Harness<BimApp>) {
    for _ in 0..4000 {
        if h.state().cloud_reading.is_none() {
            break;
        }
        h.run();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(h.state().cloud_reading.is_none(), "it was never read");
    h.run();
}

const KEEP: &str = "Keep full detail (index beside the project)";

#[test]
fn keep_full_detail_brings_a_scan_in_with_its_index_beside_the_saved_project() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("house.bimc");
    let mut h = app(BOX);
    assert!(h.state_mut().save_to(&project));
    h.state_mut().point_cloud_paths(vec![face("keep", "survey.xyz", 0.0)]);
    h.run();
    // A small scan is held whole at first; asked, it keeps every point in an index.
    assert!(!h.state().cloud_box.as_ref().unwrap().index, "off for small files");
    pick(&mut h, Role::CheckBox, KEEP);
    assert!(h.state().cloud_box.as_ref().unwrap().index);
    press(&mut h, "OK");
    read_in(&mut h);
    assert_eq!(scans(&h).len(), 1, "{}", h.state().status.text);
    let cloud = h.state().session.model.clouds.get("survey").unwrap();
    assert!(cloud.index().is_some() && cloud.indexed().is_some(), "the scan has its index, open");
    assert!(dir.path().join("house_clouds").join("survey.bimpc").is_file(), "beside the project");
    // The plan and the 3D view draw it.
    h.run();
    h.state_mut().open_3d(None);
    h.run();
}

#[test]
fn keep_full_detail_in_a_project_without_a_file_keeps_the_index_beside_the_scan() {
    let mut h = app(BOX);
    let scan = face("unsaved", "survey.xyz", 0.0);
    h.state_mut().point_cloud_paths(vec![scan.clone()]);
    h.run();
    pick(&mut h, Role::CheckBox, KEEP);
    assert!(h.query_by_label("The project has no file yet: the index is kept beside the scan's file.").is_some());
    press(&mut h, "OK");
    read_in(&mut h);
    let cloud = h.state().session.model.clouds.get("survey").unwrap();
    assert!(cloud.indexed().is_some(), "indexed beside the scan");
    assert!(scan.with_file_name("survey.bimpc").exists());
}

#[test]
fn a_scan_whose_index_is_missing_says_so_in_the_browser_and_find_index_reads_it_where_it_went() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("house.bimc");
    let mut h = app(BOX);
    assert!(h.state_mut().save_to(&project));
    h.state_mut().point_cloud_paths(vec![face("missing", "survey.xyz", 0.0)]);
    h.run();
    pick(&mut h, Role::CheckBox, KEEP);
    press(&mut h, "OK");
    read_in(&mut h);
    assert!(h.state_mut().save_to(&project));
    type_into(&mut h, "Search the Project Browser", "survey");
    assert!(h.query_by_label("Point Clouds").is_some(), "the browser lists the scans");
    // The index is moved away, and the project opened again.
    let (was, moved) = (dir.path().join("house_clouds").join("survey.bimpc"), dir.path().join("moved.bimpc"));
    std::fs::rename(&was, &moved).unwrap();
    assert!(h.state_mut().open_path(&project));
    h.run();
    type_into(&mut h, "Search the Project Browser", "survey");
    assert!(h.query_by_label("survey (index missing)").is_some(), "the browser says the index is missing");
    // Reload Index looks where the project had it, and says it isn't there.
    h.get_by_label("survey (index missing)").click_secondary();
    h.run();
    press(&mut h, "Reload Index");
    assert!(h.state().status.error, "{}", h.state().status.text);
    assert!(h.query_by_label("survey (index missing)").is_some());
    // Find Index… asks for the file and reads it there.
    h.get_by_label("survey (index missing)").click_secondary();
    h.run();
    press(&mut h, "Find Index…");
    let tx = crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).expect("a file dialog was begun");
    tx.send(Some(vec![moved.clone()])).unwrap();
    h.run();
    h.run();
    let cloud = h.state().session.model.clouds.get("survey").unwrap();
    assert!(cloud.indexed().is_some(), "{}", h.state().status.text);
    assert!(h.query_by_label("survey (index missing)").is_none() && h.query_by_label("Point Clouds").is_some());
    // Open, the browser says what its file takes; Delete Index… asks, then deletes the file and lets the index go.
    let label = format!("survey ({})", bimcraft_core::memory::bytes(std::fs::metadata(&moved).unwrap().len()));
    h.get_by_label(&label).click_secondary();
    h.run();
    press(&mut h, "Delete Index…");
    assert!(h.query_by_label("Delete Index").is_some(), "it asks first");
    assert!(moved.exists());
    press(&mut h, "Delete");
    let cloud = h.state().session.model.clouds.get("survey").unwrap();
    assert!(cloud.index().is_none() && !moved.exists(), "{}", h.state().status.text);
    assert!(h.query_by_label("survey").is_some());
}

#[test]
fn point_cloud_memory_and_points_drawn_in_3d_are_kept_in_settings_json() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let mut h = app("");
    h.state_mut().use_settings(file.clone());
    assert_eq!((h.state().scan_memory_mb, h.state().scan_points), (None, None), "Auto at first");
    press(&mut h, "Settings");
    h.state_mut().settings_page = Some(crate::settings::Page::Graphics);
    h.run();
    assert!(h.query_by_label("Point cloud memory").is_some() && h.query_by_label("Points drawn in 3D").is_some());
    // Auto turned off takes a number, kept in the settings file.
    h.state_mut().scan_memory_mb = Some(2048);
    h.state_mut().scan_points = Some(5_000_000);
    assert!(h.query_by_label("Beside each project").is_some(), "indexes beside each project at first");
    h.state_mut().session.index_dir = Some(dir.path().join("indexes"));
    h.run();
    press(&mut h, "Close");
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let folder = dir.path().join("indexes").to_string_lossy().into_owned();
    assert_eq!(kept["point_clouds"], json!({"memory_mb": 2048, "points_3d": 5_000_000, "index_dir": folder}));
    // Another window starts with them, the memory given to the scans' caches.
    let mut again = app("");
    again.state_mut().use_settings(file.clone());
    assert_eq!((again.state().scan_memory_mb, again.state().scan_points), (Some(2048), Some(5_000_000)));
    assert_eq!(again.state().session.index_dir, Some(dir.path().join("indexes")), "the session looks in the folder of indexes");
    assert_eq!(bimcraft_core::cloud::lod::memory(), 2048 << 20);
    // Null is Auto; what isn't a number in range keeps what it had.
    for (text, memory, points) in [
        (r#"{"point_clouds": {"memory_mb": null, "points_3d": null}}"#, None, None),
        (r#"{"point_clouds": {"memory_mb": 1, "points_3d": "many"}}"#, None, None),
        (r#"{"point_clouds": {"memory_mb": 512, "points_3d": 1e12}}"#, Some(512), None),
        (r#"{"point_clouds": 7}"#, None, None),
    ] {
        std::fs::write(&file, text).unwrap();
        let mut fresh = app("");
        fresh.state_mut().use_settings(file.clone());
        assert_eq!((fresh.state().scan_memory_mb, fresh.state().scan_points), (memory, points), "{text}");
    }
    bimcraft_core::cloud::lod::set_memory(None);
}
