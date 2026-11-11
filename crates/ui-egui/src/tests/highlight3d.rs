//! P1 and T11 in 3D: the project's work plane drawn into the 3D view's picture, hidden by what stands in front of it and seen
//! on a face it lies on; and in a family's {3D}, what is selected and what is under the pointer drawn in the project's
//! selection and hover colours.
use super::family_views::{editor, view};
use super::preview_visibility::Pictures;
use super::*;
use bimcraft_core::family::views::{Part, Which};
use egui::Color32;

/// A cabinet: a plinth 600 by 400, 300 high, and a top 600 by 400 from 700 to 800; a void through the top; a reference plane
/// of its own, Edge, upright at x = -450; and a reference line on Ref. Level in front of it.
const CABINET: &str = r#"family {"name": "Cabinet", "category": "furniture", "types": {"T": {}}, "forms": [{"profile": [[-300, -200], [300, -200], [300, 200], [-300, 200]], "to": 300}, {"profile": [[-300, -200], [300, -200], [300, 200], [-300, 200]], "from": 700, "to": 800}, {"profile": [[150, -100], [250, -100], [250, 100], [150, 100]], "from": 650, "to": 850, "void": true}], "ref_planes": [{"name": "Edge", "start": [-450, -2000, 0], "end": [-450, 2000, 0], "up": [0, 0, 1]}], "ref_lines": [{"start": [-300, -350, 0], "end": [300, -350, 0]}]}"#;

/// A box family's file, 300 by 200 by 400, in `dir`.
fn box_file(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("Box.json");
    let family = json!({"name": "Box", "category": "generic", "types": {"B": {}},
        "forms": [{"profile": [[0, 0], [300, 0], [300, 200], [0, 200]], "to": 400}]});
    std::fs::write(&path, family.to_string()).unwrap();
    path
}

/// The cabinet open in its document in {3D} (in `h`), the box loaded into it and placed at (-150, -100) on Ref. Level, 300
/// above it (by Offset): on the plinth, under the top.
fn nest_box(h: &mut Harness<BimApp>, dir: &std::path::Path) {
    h.state_mut().nest_family_file(&box_file(dir));
    h.run();
    view(h, Some(Which::Plan));
    ed_mut(h).component_tool();
    h.run();
    let at = editor(h).views.screen_of([-150.0, -100.0, 0.0]).expect("the floor plan shows");
    click_at(h, at);
    h.run();
    if let Some(ed) = h.state_mut().family_editor.as_mut() {
        ed.nested_instances[0].offset = bimcraft_core::family::Expr::Number(300.0);
    }
    key(h, Key::Escape);
    view(h, None);
    h.run();
}

/// [`nest_box`] in a window that draws.
fn cabinet(dir: &std::path::Path) -> Harness<'static, BimApp> {
    let mut h = app(CABINET);
    h.set_size(vec2(1600., 1000.));
    h.state_mut().edit_family("Cabinet");
    h.run();
    nest_box(&mut h, dir);
    h
}

/// [`nest_box`] in a window that keeps its pictures.
fn cabinet_kept(dir: &std::path::Path) -> (Harness<'static, BimApp>, Pictures) {
    let (mut h, pictures) = super::family_views::open(CABINET, "Cabinet");
    nest_box(&mut h, dir);
    (h, pictures)
}

fn ed_mut<'a>(h: &'a mut Harness<'_, BimApp>) -> &'a mut crate::editor::FamilyEditor {
    h.state_mut().family_editor.as_mut().expect("a family open")
}

/// The last picture kept of the texture named `name` (`family-preview`, `view3d`).
fn kept(h: &Harness<BimApp>, pictures: &Pictures, name: &str) -> egui::ColorImage {
    let named = |id: &egui::TextureId| h.ctx.tex_manager().read().meta(*id).is_some_and(|m| m.name == name);
    let kept = pictures.0.lock().unwrap();
    kept.iter().rev().find(|(id, _)| named(id)).map(|(_, p)| p.clone()).expect("a picture drawn")
}

/// {3D}'s colour at family point `p`, and where it is on screen.
fn in_3d(h: &Harness<BimApp>, pictures: &Pictures, p: [f64; 3]) -> (Color32, egui::Pos2) {
    let img = kept(h, pictures, "family-preview");
    let preview = editor(h).preview().expect("drawn");
    let (x, y, _) = preview.cam.project(p);
    (img.pixels[y as usize * img.size[0] + x as usize], preview.screen(h.state().view_rect, p))
}

/// How much bluer than red a colour is: the selection's and the hover colours are blue.
fn blue(c: Color32) -> i32 {
    i32::from(c.b()) - i32::from(c.r())
}

/// The top's front, the plinth's and the nested box's.
const TOP: [f64; 3] = [0.0, -200.0, 750.0];
const PLINTH: [f64; 3] = [0.0, -200.0, 150.0];
const NESTED: [f64; 3] = [0.0, -100.0, 500.0];

#[test]
fn a_form_and_a_nested_instance_selected_are_drawn_in_the_selection_s_colour_in_3d_and_nothing_else_is() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, pictures) = cabinet_kept(dir.path());
    let colour = |h: &Harness<BimApp>, p| in_3d(h, &pictures, p).0;
    let (top, plinth, nested) = (colour(&h, TOP), colour(&h, PLINTH), colour(&h, NESTED));
    ed_mut(&mut h).select_form(1);
    h.run();
    assert!(blue(colour(&h, TOP)) > blue(top) + 30, "the top selected: {:?} from {top:?}", colour(&h, TOP));
    assert_eq!(colour(&h, PLINTH), plinth, "the plinth not");
    assert_eq!(colour(&h, NESTED), nested, "nor the box");
    ed_mut(&mut h).select_nested(Some(0));
    h.run();
    assert!(blue(colour(&h, NESTED)) > blue(nested) + 30, "the box selected: {:?} from {nested:?}", colour(&h, NESTED));
    assert_eq!(colour(&h, TOP), top, "the top let go");
    // The void through the top, selected: seen, see-through in the selection's colour, where it is in the top.
    ed_mut(&mut h).select_nested(None);
    ed_mut(&mut h).select_form(2);
    h.run();
    let hole = [200.0, 0.0, 800.0];
    let (cut, _) = in_3d(&h, &pictures, hole);
    assert!(blue(cut) > 20, "the void lit: {cut:?}");
}

#[test]
fn the_pointer_over_a_form_or_a_nested_instance_lights_it_and_a_click_selects_it_as_in_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, pictures) = cabinet_kept(dir.path());
    let (plinth, at) = in_3d(&h, &pictures, PLINTH);
    h.hover_at(at);
    h.run();
    let lit = in_3d(&h, &pictures, PLINTH).0;
    assert!(blue(lit) > blue(plinth) + 20, "lit under the pointer: {lit:?} from {plinth:?}");
    assert_ne!(lit, in_3d(&h, &pictures, TOP).0, "the top isn't");
    click_at(&mut h, at);
    assert_eq!(editor(&h).selected_form(), Some(0), "a click selects the plinth");
    let (_, on_box) = in_3d(&h, &pictures, NESTED);
    click_at(&mut h, on_box);
    assert_eq!(editor(&h).selected_nested(), Some(0), "and the nested box");
    assert_eq!(editor(&h).selected_form(), None);
    let nothing = h.state().view_rect.min + vec2(40.0, 120.0);
    click_at(&mut h, nothing);
    assert_eq!((editor(&h).selected_form(), editor(&h).selected_nested()), (None, None), "a click on nothing lets it go");
    // Under the pointer, as {3D} finds it.
    let ed = editor(&h);
    let cam = ed.preview().unwrap().cam;
    let f = ed.draft_family().unwrap();
    let (x, y, _) = cam.project(NESTED);
    assert_eq!(bimcraft_core::family::views::part_at_3d(&f, &ed.values(), &cam, (x, y)), Some(Part::Nested(0)));
}

#[test]
fn a_reference_plane_or_line_selected_in_a_plan_is_drawn_in_3d_in_the_selection_s_colour() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, pictures) = cabinet_kept(dir.path());
    let on_plane = [-450.0, 0.0, 400.0];
    let on_line = [0.0, -350.0, 0.0];
    let (plane, line) = (in_3d(&h, &pictures, on_plane).0, in_3d(&h, &pictures, on_line).0);
    let edge = editor(&h).planes.iter().find(|p| p.name == "Edge").unwrap().guid;
    ed_mut(&mut h).views.plane = Some(edge);
    h.run();
    assert!(blue(in_3d(&h, &pictures, on_plane).0) > blue(plane) + 20, "Edge drawn where it is");
    assert_eq!(in_3d(&h, &pictures, on_line).0, line);
    let guid = editor(&h).lines[0].guid;
    ed_mut(&mut h).views.plane = None;
    ed_mut(&mut h).views.line = Some(guid);
    h.run();
    assert!(blue(in_3d(&h, &pictures, on_line).0) > blue(line) + 20, "the line drawn where it is");
    assert_eq!(in_3d(&h, &pictures, on_plane).0, plane, "Edge let go");
}

/// The project's 3D picture's colour where model point `p` is.
fn in_project(h: &Harness<BimApp>, pictures: &Pictures, p: [f64; 3]) -> Color32 {
    let img = kept(h, pictures, "view3d");
    let cam = h.state().orbit.camera.expect("the 3D view shown");
    let (x, y, _) = cam.project(p);
    img.pixels[y as usize * img.size[0] + x as usize]
}

/// The project of `script` in its 3D view, its pictures kept.
fn project(script: &str) -> (Harness<'static, BimApp>, Pictures) {
    let mut s = Session::new();
    s.script(script).unwrap();
    let pictures = Pictures::default();
    let mut h = Harness::builder()
        .with_size([1600.0, 1000.0])
        .with_max_steps(32)
        .renderer(pictures.clone())
        .build_ui_state(|ui, app: &mut BimApp| app.show(ui), BimApp::new(s));
    h.run();
    press(&mut h, "Default 3D View");
    h.run();
    (h, pictures)
}

#[test]
fn the_project_s_work_plane_is_drawn_into_its_3d_picture_where_it_is_seen_and_not_on_walls_in_front_of_it() {
    let (mut h, pictures) = project(&format!("{}\nwork_plane {{\"offset\": 1500}}", super::BOX));
    // Outside the box, in front of everything; on the south wall's face high up, where the wall is in front of the plane;
    // and low down, where the plane is in front of the wall.
    let (open, high, low) = ([-1000.0, -1000.0, 1500.0], [4000.0, -100.0, 2600.0], [4000.0, -100.0, 300.0]);
    let before = [open, high, low].map(|p| in_project(&h, &pictures, p));
    h.state_mut().show_work_plane = true;
    h.run();
    let after = [open, high, low].map(|p| in_project(&h, &pictures, p));
    assert_ne!(after[0], before[0], "the plane tinted where nothing is in front of it");
    assert_eq!(after[1], before[1], "the wall in front of it keeps its colour");
    assert_ne!(after[2], before[2], "the wall behind it is seen through it");
    // On Level 1, where the floor's top is: the floor doesn't hide it (by the north-west corner, which the walls nearer the
    // eye, south and east, don't hide).
    let (mut h, pictures) = project(&format!("{}\nfloor {{}}", super::BOX));
    let middle = [1500.0, 4500.0, 0.0];
    let floor = in_project(&h, &pictures, middle);
    h.state_mut().show_work_plane = true;
    h.run();
    assert_ne!(in_project(&h, &pictures, middle), floor, "on the floor it lies on");
}

/// The project's 3D view and the family's {3D}, saved where `BIMCRAFT_P1_DUMP` says (only then); with `BIMCRAFT_P1_CROPS`
/// (x, y, w, h; ';' between), those parts of each three times as big.
#[test]
fn p1_pictures() {
    let Ok(folder) = std::env::var("BIMCRAFT_P1_DUMP") else { return };
    let folder = std::path::Path::new(&folder);
    std::fs::create_dir_all(folder).unwrap();
    let crops: Vec<[u32; 4]> = std::env::var("BIMCRAFT_P1_CROPS")
        .unwrap_or_default()
        .split(';')
        .filter_map(|c| c.split(',').map(|v| v.trim().parse().ok()).collect::<Option<Vec<u32>>>()?.try_into().ok())
        .collect();
    let save = |h: &mut Harness<BimApp>, name: &str| {
        h.run();
        let img = h.render().unwrap();
        img.save(folder.join(format!("p1-{name}.png"))).unwrap();
        for (k, [x, y, w, hh]) in crops.iter().enumerate() {
            let part = image::imageops::crop_imm(&img, *x, *y, *w, *hh).to_image();
            let big = image::imageops::resize(&part, w * 3, hh * 3, image::imageops::FilterType::Nearest);
            big.save(folder.join(format!("crop-{name}-{k}.png"))).unwrap();
        }
    };
    let dark = |h: &mut Harness<BimApp>| {
        h.ctx.set_theme(egui::ThemePreference::Dark);
        h.run();
        h.run();
    };
    // The work plane 1500 up the walls: those in front of it hide it.
    let mut h = app(&format!("{}\nwork_plane {{\"offset\": 1500}}", super::BOX));
    h.set_size(vec2(1600., 1000.));
    press(&mut h, "Default 3D View");
    h.state_mut().show_work_plane = true;
    save(&mut h, "project-walls");
    // Turned to look from the north-east, the patch behind the walls.
    if let Some(c) = h.state_mut().orbit.camera.as_mut() {
        c.azimuth = 135.0;
    }
    save(&mut h, "project-walls-behind");
    // On Level 1, where a floor's top is: the floor doesn't hide it.
    let mut h = app(&format!("{}\nfloor {{}}", super::BOX));
    h.set_size(vec2(1600., 1000.));
    press(&mut h, "Default 3D View");
    h.state_mut().show_work_plane = true;
    save(&mut h, "project-coplanar");
    dark(&mut h);
    save(&mut h, "project-coplanar-dark");
    // The family: nothing selected, the top selected, the nested box selected, the void, the plinth under the pointer, and
    // the reference plane and line selected.
    let dir = tempfile::tempdir().unwrap();
    let mut h = cabinet(dir.path());
    save(&mut h, "family-none");
    ed_mut(&mut h).select_form(1);
    save(&mut h, "family-form");
    ed_mut(&mut h).select_nested(Some(0));
    save(&mut h, "family-nested");
    ed_mut(&mut h).select_nested(None);
    ed_mut(&mut h).select_form(2);
    save(&mut h, "family-void");
    ed_mut(&mut h).let_form_go();
    let at = editor(&h).preview().unwrap().screen(h.state().view_rect, PLINTH);
    h.hover_at(at);
    save(&mut h, "family-hover");
    h.hover_at(h.state().view_rect.min + vec2(5.0, 300.0));
    let edge = editor(&h).planes.iter().find(|p| p.name == "Edge").unwrap().guid;
    ed_mut(&mut h).views.plane = Some(edge);
    save(&mut h, "family-plane");
    let line = editor(&h).lines[0].guid;
    ed_mut(&mut h).views.plane = None;
    ed_mut(&mut h).views.line = Some(line);
    save(&mut h, "family-line");
    ed_mut(&mut h).views.line = None;
    ed_mut(&mut h).select_nested(Some(0));
    dark(&mut h);
    save(&mut h, "family-nested-dark");
}

/// The project's 3D view of the box turned to look from the north-east, low: the work plane's patch, by the south-west corner,
/// lies behind the walls.
pub(super) fn patch_behind(h: &mut Harness<BimApp>) -> egui::Pos2 {
    press(h, "Default 3D View");
    let patch = h.state().work_plane_patch().expect("drawn in the 3D view");
    click_at(h, patch);
    assert!(h.state().work_plane.is_some(), "seen, the patch opens the dialog");
    press(h, "Cancel");
    if let Some(c) = h.state_mut().orbit.camera.as_mut() {
        (c.azimuth, c.elevation) = (45.0, 10.0);
    }
    h.run();
    h.state().work_plane_patch().expect("still on the plane")
}

#[test]
fn a_click_where_a_wall_hides_the_work_plane_s_patch_is_the_wall_s() {
    let mut h = app(super::BOX);
    let patch = patch_behind(&mut h);
    click_at(&mut h, patch);
    assert!(h.state().work_plane.is_none(), "hidden, the patch opens nothing");
    assert_eq!(h.state().selection.len(), 1, "the wall in front of it is picked");
}

/// A door's form selected in {3D}: the part is another opening of the preview's wall where the door is, so it is tinted where it
/// is and nothing else changes, the wall's opening and the other forms as they were.
#[test]
fn a_door_s_form_selected_in_3d_is_tinted_where_it_stands_in_the_wall_and_nothing_else_changes() {
    let pictures = Pictures::default();
    let mut h = Harness::builder()
        .with_size([1600.0, 1200.0])
        .with_max_steps(32)
        .renderer(pictures.clone())
        .build_ui_state(|ui, app: &mut BimApp| app.show(ui), BimApp::new(Session::new()));
    h.run();
    h.state_mut().new_family(bimcraft_core::family::Category::Door);
    h.run();
    let before = kept(&h, &pictures, "family-preview");
    ed_mut(&mut h).select_form(1);
    h.run();
    let after = kept(&h, &pictures, "family-preview");
    let changed: Vec<usize> = (0..before.pixels.len()).filter(|&i| before.pixels[i] != after.pixels[i]).collect();
    assert!(changed.len() > 200, "the form tinted: {} pixels", changed.len());
    let bluer = changed.iter().filter(|&&i| blue(after.pixels[i]) > blue(before.pixels[i])).count();
    assert!(bluer * 100 >= changed.len() * 98, "all of it toward the selection's colour: {bluer} of {}", changed.len());
}
