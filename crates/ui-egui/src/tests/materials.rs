//! The Materials window: its three columns, its patterns, its stars, its standard colours and its parameters.

use super::*;

type Build = fn(&str) -> Harness<'static, BimApp>;

fn open_materials(h: &mut Harness<BimApp>) {
    press(h, "Manage");
    press(h, "Materials");
}

/// A folder of this test's own for the favorites and the colours the user keeps, and the app told of it.
fn keep_in(h: &mut Harness<BimApp>, test: &str) -> std::path::PathBuf {
    let folder = std::env::temp_dir().join(format!("bimcraft-ui-materials-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    h.state_mut().favorites_path = Some(folder.join("favorites.json"));
    folder
}

#[test]
fn its_three_columns_stand_side_by_side() {
    let mut h = app(BOX);
    open_materials(&mut h);
    let x = |h: &Harness<BimApp>, role, name: &str| h.get_by_role_and_label(role, name).rect().center().x;
    let list = x(&h, Role::Button, "Duplicate");
    let settings = x(&h, Role::ComboBox, "Surface Foreground Pattern");
    let side = h.get_by_label("Mark").rect().center().x;
    assert!(list < settings && settings < side, "{list} {settings} {side}");
    assert!(h.query_by_label("Material preview").is_some());
}

#[test]
fn each_pattern_is_picked_from_a_list_that_shows_it() {
    use bimcraft_core::model::Pattern;
    let mut h = app(BOX);
    open_materials(&mut h);
    press(&mut h, "Brick");
    pick(&mut h, Role::ComboBox, "Surface Background Pattern");
    press(&mut h, "Square Grid");
    pick(&mut h, Role::ComboBox, "Cut Background Pattern");
    press(&mut h, "Solid Fill");
    let m = &h.state().session.model.materials["Brick"];
    assert_eq!(m.surface_background.as_ref().map(|l| l.pattern), Some(Pattern::Grid));
    assert_eq!(m.cut_background.as_ref().map(|l| l.pattern), Some(Pattern::Solid));
    // The project's fill patterns are in the list too.
    let name = h.state().session.model.fill_patterns.keys().next().unwrap().clone();
    choose(&mut h, "Surface Foreground Pattern", &name);
    assert_eq!(h.state().session.model.materials["Brick"].surface_pattern.as_ref(), Some(&name));
}

#[test]
fn duplicate_makes_a_copy_and_picks_it() {
    let mut h = app(BOX);
    open_materials(&mut h);
    press(&mut h, "Brick");
    press(&mut h, "Duplicate");
    let materials = &h.state().session.model.materials;
    assert_eq!(materials["Brick 2"], materials["Brick"]);
    assert_eq!(h.get_by_label("Name").value().as_deref(), Some("Brick 2"));
}

#[test]
fn a_star_keeps_a_material_in_the_favorites_and_the_list_can_show_them_alone() {
    let mut h = app(BOX);
    let folder = keep_in(&mut h, "star");
    open_materials(&mut h);
    pick(&mut h, Role::CheckBox, "Favorite Concrete");
    let kept =
        |h: &Harness<BimApp>| bimcraft_core::favorites::Favorites::read(h.state().favorites_path.as_ref().unwrap()).unwrap().materials;
    assert!(kept(&h).contains_key("Concrete"));
    pick(&mut h, Role::CheckBox, "Favorites only");
    assert!(h.query_by_label("Favorite Concrete").is_some());
    assert!(h.query_by_label("Favorite Brick").is_none());
    pick(&mut h, Role::CheckBox, "Favorites only");
    pick(&mut h, Role::CheckBox, "Favorite Concrete");
    assert!(!kept(&h).contains_key("Concrete"));
    let _ = std::fs::remove_dir_all(folder);
}

#[test]
fn a_standard_colour_is_searched_for_and_makes_a_material() {
    let mut h = app(BOX);
    let folder = keep_in(&mut h, "ral");
    open_materials(&mut h);
    press(&mut h, "New");
    press(&mut h, "Standard Color…");
    assert!(h.query_by_label("Colors are RGB approximations of standard colors.").is_some(), "it says what they are");
    assert!(h.query_by_label_contains("Source: the CIELAB values").is_some(), "it says where the colours are from");
    type_into(&mut h, "Search by code or name", "3020");
    press(&mut h, "RAL 3020 Traffic red");
    press(&mut h, "Create Material");
    let m = &h.state().session.model.materials["RAL 3020 Traffic red"];
    assert_eq!(m.surface, bimcraft_core::model::Pattern::Solid);
    assert_eq!(m.cut, bimcraft_core::model::Pattern::Solid);
    assert_eq!((m.surface_color, m.cut_color), (Some(m.color), Some(m.color)));
    assert_eq!(h.get_by_label("Name").value().as_deref(), Some("RAL 3020 Traffic red"));
    assert!(h.query_by_label("Create Material").is_none(), "the window has closed");
    let _ = std::fs::remove_dir_all(folder);
}

#[test]
fn a_table_of_the_users_own_comes_in_as_a_collection() {
    let mut h = app(BOX);
    let folder = keep_in(&mut h, "csv");
    let file = folder.join("Office.csv");
    std::fs::write(&file, "Code,Name,Colour\n18 E 53,Poppy,#c8102e\n").unwrap();
    open_materials(&mut h);
    press(&mut h, "New");
    press(&mut h, "Standard Color…");
    h.state_mut().import_colors(file);
    h.run();
    type_into(&mut h, "Source", "Our licensed table");
    press(&mut h, "Import");
    assert!(h.query_by_label("Source: Our licensed table").is_some());
    press(&mut h, "Office 18 E 53 Poppy");
    press(&mut h, "Create Material");
    assert_eq!(h.state().session.model.materials["Office 18 E 53 Poppy"].color, [0xc8, 0x10, 0x2e]);
    let _ = std::fs::remove_dir_all(folder);
}

#[test]
fn parameters_are_typed_beside_the_preview_and_new_ones_added_there() {
    let mut h = app(BOX);
    open_materials(&mut h);
    press(&mut h, "Brick");
    type_into(&mut h, "Mark", "M-01");
    key(&mut h, Key::Enter);
    type_into(&mut h, "Cost", "12.5");
    key(&mut h, Key::Enter);
    let held = |h: &Harness<BimApp>, n: &str| h.state().session.model.materials["Brick"].parameters.get(n).cloned();
    assert_eq!(held(&h, "Mark"), Some(json!("M-01")));
    assert_eq!(held(&h, "Cost"), Some(json!(12.5)));
    // Nonsense is said in the window, and nothing changes.
    type_into(&mut h, "Cost", "cheap");
    key(&mut h, Key::Enter);
    assert!(h.state().status.error);
    assert_eq!(held(&h, "Cost"), Some(json!(12.5)));
    // A parameter of the project's, for materials, there to be filled in.
    h.get_by_label("Add Parameter").click_accesskit();
    h.run();
    type_into(&mut h, "Parameter Name", "Fire Rating");
    h.get_by_label("Add").click_accesskit();
    h.run();
    assert_eq!(h.state().session.model.parameters["Fire Rating"].categories, ["material"]);
    type_into(&mut h, "Fire Rating", "A1");
    key(&mut h, Key::Enter);
    assert_eq!(held(&h, "Fire Rating"), Some(json!("A1")));
}

/// A picture for an image fill: a JPEG of 16 by 8 pixels, `gradient`.
const FILL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/testdata/gradient.jpg");

#[test]
fn an_image_fill_takes_the_place_of_a_surfaces_patterns_and_is_sized_there() {
    let mut h = app(BOX);
    let folder = keep_in(&mut h, "image-fill");
    let file = std::path::PathBuf::from(FILL);
    open_materials(&mut h);
    press(&mut h, "Brick");
    assert!(h.query_by_label("Surface Foreground Pattern").is_some());
    h.state_mut().load_fill_image("Brick", false, &file);
    h.run();
    assert!(h.state().session.model.materials["Brick"].surface_image.is_some());
    assert!(h.query_by_label("Surface Foreground Pattern").is_none(), "the patterns make way for it");
    assert!(h.query_by_label("Cut Foreground Pattern").is_some(), "the cut keeps its own");
    assert!(h.query_by_label_contains("gradient (16 × 8 px)").is_some());
    type_into(&mut h, "Surface Image Width", "250");
    key(&mut h, Key::Enter);
    type_into(&mut h, "Surface Image Angle", "45");
    key(&mut h, Key::Enter);
    let fill = h.state().session.model.materials["Brick"].surface_image.clone().unwrap();
    assert_eq!((fill.size[0], fill.angle), (250.0, 45.0));
    // Back to patterns: the picture goes, and the patterns are as they were.
    press(&mut h, "Surface Pattern Fill");
    assert!(h.state().session.model.materials["Brick"].surface_image.is_none());
    assert!(h.query_by_label("Surface Foreground Pattern").is_some());
    // A file that is no picture is said in the window.
    h.state_mut().load_fill_image("Brick", true, &folder.join("favorites.json"));
    h.run();
    assert!(h.state().session.model.materials["Brick"].cut_image.is_none());
    assert!(h.state().status.error);
    let _ = std::fs::remove_dir_all(folder);
}

#[test]
fn the_columns_fill_the_window_down_to_its_footer() {
    let mut h = app(BOX);
    h.set_size(egui::vec2(1280.0, 1000.0));
    h.run();
    open_materials(&mut h);
    press(&mut h, "Brick");
    h.state_mut().load_fill_image("Brick", false, std::path::Path::new(FILL));
    h.state_mut().load_fill_image("Brick", true, std::path::Path::new(FILL));
    h.run();
    // Both image fills and the texture: all of it shows, the middle column as tall as the window lets it be, so the wheel
    // over it has nothing to scroll.
    let footer = h.get_by_role_and_label(Role::Button, "Favorites…").rect();
    assert!(footer.bottom() > 1000.0 - 60.0, "the window stands as tall as the screen lets it: {footer:?}");
    let last = h.get_by_label("Texture Offset Up").rect();
    assert!(last.bottom() < footer.top(), "the last field {last:?}, the footer {footer:?}");
    h.hover_at(h.get_by_label("Cut Image Width").rect().center());
    h.run();
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -400.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    for _ in 0..10 {
        h.run();
    }
    assert_eq!(h.get_by_label("Texture Offset Up").rect(), last, "it all fits: nothing to scroll");
}

#[test]
fn the_window_fits_a_small_screen() {
    let mut h = app(BOX);
    open_materials(&mut h);
    h.set_size(egui::vec2(1000.0, 640.0));
    h.run();
    let close = h.get_by_role_and_label(Role::Button, "Close").rect();
    assert!(close.right() <= 1000.0 && close.bottom() <= 640.0, "{close:?}");
}

/// What the window looks like, for a person to look at: `BIMCRAFT_MATERIALS_DUMP=<folder>` writes it, a pattern's list open and
/// the Standard Colors window, in the three looks.
#[test]
fn materials_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_MATERIALS_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let builds: [(&str, Build); 2] = [("rev", app), ("bimcraft", bimcraft)];
    for (profile, build) in builds {
        for (dark, light_canvas, look) in [(false, false, "light"), (true, false, "dark"), (true, true, "dark-ui")] {
            let mut h = build(BOX);
            keep_in(&mut h, &format!("shots-{profile}-{look}"));
            h.state_mut().light_canvas = light_canvas;
            if dark {
                h.ctx.set_theme(egui::Theme::Dark);
            }
            h.run();
            let save = |h: &mut Harness<BimApp>, name: &str| {
                h.run();
                h.render().unwrap().save(std::path::Path::new(&folder).join(format!("materials-{profile}-{name}-{look}.png"))).unwrap();
            };
            open_materials(&mut h);
            press(&mut h, "Brick");
            save(&mut h, "window");
            pick(&mut h, Role::ComboBox, "Surface Background Pattern");
            save(&mut h, "patterns");
            key(&mut h, Key::Escape);
            h.state_mut().load_fill_image("Brick", true, std::path::Path::new(FILL));
            save(&mut h, "image-fill");
            press(&mut h, "New");
            press(&mut h, "Standard Color…");
            type_into(&mut h, "Search by code or name", "red");
            save(&mut h, "colors");
        }
    }
}
