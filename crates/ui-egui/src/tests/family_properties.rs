//! Properties in the family document show what is selected, as Revit's do: nothing selected, the family's own (Identity Data,
//! Family Category and Parameters' Other); a form, its own (Constraints, Graphics, Materials and Finishes, Identity Data); a
//! plan symbol, its rows. The forms and symbols are listed in the Project Browser, whose right-click menu selects, deletes,
//! moves a form up and down and edits its sketch; Annotate's Detail panel makes symbols, Manage's Adaptive Component makes the
//! family adaptive.
use super::family_views::editor;
use super::*;
use bimcraft_core::family::Category;

/// A new family of `category` in its document, in a window wide enough for its ribbon.
fn document(category: Category) -> Harness<'static, BimApp> {
    let mut h = app("");
    h.set_size(vec2(1600.0, 1000.0));
    h.state_mut().new_family(category);
    h.run();
    h
}

/// The browser's row `label` clicked, brought into view first: it may lie below the palette's fold.
fn browse(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_label(label).scroll_to_me();
    h.run();
    press(h, label);
}

/// The browser's row `label` right-clicked, then its menu's `item` (the one drawn last, over the palette).
fn browser_menu(h: &mut Harness<BimApp>, label: &str, item: &str) {
    h.get_by_label(label).scroll_to_me();
    h.run();
    h.get_by_label(label).click_secondary();
    h.run();
    h.get_all_by_label(item).last().expect("the menu's item").click();
    h.run();
}

#[test]
fn nothing_selected_shows_the_familys_own_properties_and_no_forms_list() {
    let h = document(Category::Furniture);
    assert_eq!(editor(&h).selected_form(), None);
    for row in ["Family name", "Identity Data", "Other", "Work Plane-Based", "Always Vertical"] {
        assert!(h.query_by_label(row).is_some(), "{row}");
    }
    // The forms are the browser's, made on the Create tab: Properties has no list of them, nor buttons adding them.
    for gone in ["Add Box", "Add Cylinder", "Add Revolve", "Add Void", "Corners", "Symbols", "Adaptive", "Extrusion End"] {
        assert!(h.query_by_label(gone).is_none(), "{gone}");
    }
    assert!(h.query_by_label("1. Extrusion").is_some(), "the browser lists the form");
}

#[test]
fn a_form_selected_in_the_browser_shows_its_own_properties_and_escape_shows_the_familys_again() {
    let mut h = document(Category::Furniture);
    browse(&mut h, "1. Extrusion");
    assert_eq!(editor(&h).selected_form(), Some(0));
    for row in ["Constraints", "Extrusion End", "Extrusion Start", "Work Plane", "Graphics", "Visible", "Material", "Solid/Void"] {
        assert!(h.query_by_label(row).is_some(), "{row}");
    }
    assert!(h.query_by_label("Family name").is_none(), "only what is selected");
    // Its contextual tab and the Options Bar named for its kind, as Revit's.
    assert!(h.query_all_by_label("Modify | Extrusion").count() >= 2, "the tab and the Options Bar");
    assert_eq!(h.get_by_label("Extrusion End").value().as_deref(), Some("750"));
    key(&mut h, Key::Escape);
    assert!(h.query_by_label("Family name").is_some() && h.query_by_label("Extrusion End").is_none());
}

#[test]
fn a_void_selected_shows_solid_void_and_in_a_doors_family_what_it_cuts() {
    let mut h = document(Category::Door);
    browse(&mut h, "1. Extrusion");
    choose(&mut h, "Solid/Void", "Void");
    assert!(editor(&h).draft_family().unwrap().forms[0].void);
    assert_eq!(h.get_by_label("Solid/Void").value().as_deref(), Some("Void"));
    assert_eq!(h.get_by_label("Cuts").value().as_deref(), Some("Forms and Host"));
    // The browser names it a void.
    assert!(h.query_by_label("1. Extrusion (void)").is_some());
    assert!(h.query_all_by_label("Modify | Void Extrusion").count() >= 2, "the tab and the Options Bar");
    assert!(h.query_by_label("Modify | Extrusion").is_none());
}

#[test]
fn the_browsers_menu_moves_a_form_down_and_up_deletes_it_and_edits_its_sketch() {
    let mut h = document(Category::Door);
    let guids = |h: &Harness<BimApp>| editor(h).draft_family().unwrap().forms.iter().map(|f| f.guid).collect::<Vec<_>>();
    let before = guids(&h);
    assert_eq!(before.len(), 3);
    // Move Down: the first goes second (a void cuts the forms listed above it).
    browser_menu(&mut h, "1. Extrusion", "Move Down");
    assert_eq!(guids(&h), [before[1], before[0], before[2]]);
    browser_menu(&mut h, "2. Extrusion", "Move Up");
    assert_eq!(guids(&h), before);
    // Select, then Delete.
    browser_menu(&mut h, "3. Extrusion", "Select");
    assert_eq!(editor(&h).selected_form(), Some(2));
    browser_menu(&mut h, "3. Extrusion", "Delete");
    assert_eq!(guids(&h), before[..2]);
    // Edit Extrusion: its sketch again, sketch mode on.
    browser_menu(&mut h, "1. Extrusion", "Edit Extrusion");
    assert!(editor(&h).sketching(), "{}", h.state().status.text);
}

#[test]
fn annotates_detail_panel_makes_symbols_the_browser_lists_and_selects_and_deletes() {
    let mut h = document(Category::Door);
    press(&mut h, "Annotate");
    press(&mut h, "Text");
    assert_eq!(editor(&h).selected_symbol(), Some(0), "made selected");
    // Its rows in Properties.
    type_into(&mut h, "Symbol Text 1", "D1");
    assert!(h.query_by_label("Symbol Radius 1").is_some());
    // Selected, it has the contextual tab, as Revit's: Annotate again for the next.
    assert!(h.query_all_by_label("Modify | Symbols").count() > 0);
    press(&mut h, "Annotate");
    press(&mut h, "Dot");
    assert_eq!(editor(&h).selected_symbol(), Some(1));
    // The browser lists both; a click selects one.
    browse(&mut h, "1. Text");
    assert_eq!(editor(&h).selected_symbol(), Some(0));
    assert_eq!(h.get_by_label("Symbol Text 1").value().as_deref(), Some("D1"));
    browser_menu(&mut h, "2. Dot", "Delete");
    let f = editor(&h).draft_family().unwrap();
    assert_eq!(f.symbols.len(), 1);
    assert_eq!(f.symbols[0].text.as_ref().map(|l| l.value.as_str()), Some("D1"));
    // Delete takes the selected one, as the ribbon's does.
    browse(&mut h, "1. Text");
    key(&mut h, Key::Delete);
    assert!(editor(&h).draft_family().unwrap().symbols.is_empty());
}

#[test]
fn manages_adaptive_component_makes_the_family_adaptive() {
    let mut h = document(Category::Generic);
    press(&mut h, "Manage");
    press(&mut h, "Make Family Adaptive");
    assert!(editor(&h).adaptive.is_some());
}

#[test]
fn a_sweeps_properties_name_its_path_and_profile() {
    let mut h = app("");
    h.set_size(vec2(1600.0, 1000.0));
    let made = h.state_mut().run(
        "family",
        json!({"name": "Rail", "category": "generic", "types": {"T": {}},
               "forms": [{"sketch": [{"start": [0, 0], "end": [1000, 0]}, {"start": [1000, 0], "end": [1000, 800]}],
                          "sweep": {"profile": [{"start": [-50, 0], "end": [50, 0]}, {"start": [50, 0], "end": [50, 80]},
                                                {"start": [50, 80], "end": [-50, 80]}, {"start": [-50, 80], "end": [-50, 0]}]}}]}),
    );
    assert!(made.is_some());
    h.state_mut().edit_family("Rail");
    h.run();
    browse(&mut h, "1. Sweep");
    assert!(h.query_all_by_label("Modify | Sweep").count() > 0);
    assert!(h.query_by_label("Path").is_some() && h.query_by_label("Profile").is_some());
    assert!(h.query_by_label("Sketch, 2 lines and arcs").is_some());
    assert!(h.query_by_label("Sketch, 4 lines and arcs").is_some());
    assert!(h.query_by_label("Extrusion End").is_none(), "a sweep has no ends of its own");
}

/// The window as it looks at each step this area changed, saved as `rv2-*.png` in `BIMCRAFT_RV2_DUMP`: the start page, File's
/// New, Modify | Furniture's Edit Family, and the family document's Properties with nothing, an extrusion and a void selected, and
/// the browser's Forms with a form's menu.
#[test]
#[ignore = "saves screenshots for looking at"]
fn rv2_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_RV2_DUMP") else { return };
    let folder = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&folder).unwrap();
    // Each picture, and a crop of `zoom` (x, y, width, height) three times as large, to look at closely.
    let save = |h: &mut Harness<BimApp>, name: &str, zoom: [u32; 4]| {
        // The pointer off to the side, where it lights nothing.
        h.hover_at(pos2(1590.0, 600.0));
        h.run();
        let picture = h.render().unwrap();
        picture.save(folder.join(format!("rv2-{name}.png"))).unwrap();
        let [x, y, w, ht] = zoom;
        let crop = image::imageops::crop_imm(&picture, x, y, w, ht).to_image();
        let large = image::imageops::resize(&crop, w * 3, ht * 3, image::imageops::FilterType::Nearest);
        large.save(folder.join(format!("rv2-{name}-zoom.png"))).unwrap();
    };
    let mut h = app(&format!("{BOX}\ncomponent {{\"family\": \"Table\", \"type\": \"1600 x 900\", \"at\": [3000, 3000]}}"));
    h.set_size(vec2(1600.0, 1000.0));
    h.state_mut().show_start_page();
    h.run();
    save(&mut h, "start", [200, 140, 290, 520]);
    h.state_mut().home.shown = false;
    h.run();
    press(&mut h, "File");
    press(&mut h, "New ⏵");
    save(&mut h, "file-menu", [40, 40, 290, 240]);
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    click(&mut h, P2(3800.0, 3000.0));
    save(&mut h, "modify-furniture", [820, 50, 770, 125]);
    press(&mut h, "Edit Family");
    key(&mut h, Key::Escape);
    save(&mut h, "properties-nothing", [16, 190, 280, 310]);
    browse(&mut h, "1. Extrusion");
    save(&mut h, "properties-extrusion", [16, 50, 580, 250]);
    key(&mut h, Key::Escape);
    h.state_mut().new_family(Category::Door);
    h.run();
    browse(&mut h, "1. Extrusion");
    choose(&mut h, "Solid/Void", "Void");
    // Its Identity Data, below the palette's fold.
    h.get_by_label("Cuts").scroll_to_me();
    save(&mut h, "properties-void", [16, 50, 580, 450]);
    h.get_by_label("2. Extrusion").click_secondary();
    h.run();
    save(&mut h, "browser-forms-menu", [16, 700, 280, 250]);
}
