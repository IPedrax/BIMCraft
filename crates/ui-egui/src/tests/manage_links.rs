//! Manage Links, driven as a user drives it: Insert > Manage Links lists a linked model with its instances, unloads and
//! reloads it.

use super::*;
use std::path::PathBuf;

use egui_kittest::kittest::NodeT;

/// A box of walls saved to a file of its own: the path.
fn shell(name: &str) -> PathBuf {
    let mut s = Session::new();
    s.script(BOX).unwrap();
    let path = std::env::temp_dir().join(format!("bimcraft-ui-manage-links-{name}-{}.bimc", std::process::id()));
    s.execute("save", &json!({"path": path})).unwrap();
    path
}

fn walls_drawn(h: &mut Harness<BimApp>) -> usize {
    h.state_mut().view_model().elements.values().filter(|e| e.category() == "wall").count()
}

#[test]
fn manage_links_lists_a_link_with_its_instances_and_unloads_and_reloads_it() {
    let path = shell("unload");
    let mut h = app("");
    h.state_mut().link_path(&path);
    h.state_mut().run("link", json!({"path": path, "name": "East", "at": [20_000, 0]}));
    h.run();
    press(&mut h, "Insert");
    press(&mut h, "Manage Links");
    assert!(h.state().manage_links.is_some());
    let name = path.file_stem().unwrap().to_string_lossy().into_owned();
    assert!(h.query_by_label("BIMCraft (1)").is_some(), "the tab says how many files it lists");
    let rows = h.state().manage_links.as_ref().unwrap().rows();
    assert_eq!((rows.len(), rows[0]["instances"].clone()), (1, json!(2)), "{rows:?}");
    assert!(h.query_by_label("Loaded").is_some());
    assert!(h.query_by_label(&path.to_string_lossy()).is_some(), "its saved path");
    // Nothing picked, nothing to unload.
    assert!(h.get_by_label("Unload").accesskit_node().is_disabled());
    // The row in the dialog, not the Project Browser's entry of the link.
    let row = h.get_all_by_label(&name).max_by(|a, b| a.rect().center().x.total_cmp(&b.rect().center().x)).unwrap();
    row.click();
    h.run();
    press(&mut h, "Unload");
    assert!(h.query_by_label("Unloaded").is_some(), "{}", h.state().status.text);
    assert_eq!(walls_drawn(&mut h), 0);
    press(&mut h, "Reload");
    assert!(h.query_by_label("Loaded").is_some(), "{}", h.state().status.text);
    assert_eq!(walls_drawn(&mut h), 8, "both instances, four walls each");
    press(&mut h, "Close");
    assert!(h.state().manage_links.is_none());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn reload_from_reads_the_file_picked() {
    let a = shell("from-a");
    let mut s = Session::new();
    s.execute("wall", &json!({"points": [[0, 0], [3000, 0]]})).unwrap();
    let b = std::env::temp_dir().join(format!("bimcraft-ui-manage-links-from-b-{}.bimc", std::process::id()));
    s.execute("save", &json!({"path": b})).unwrap();
    let mut h = app("");
    h.state_mut().link_path(&a);
    h.state_mut().open_manage_links();
    h.run();
    let row = h.state().manage_links.as_ref().unwrap().rows()[0].clone();
    assert!(h.state_mut().reload_link("projects", &row, Some(&b)));
    assert_eq!(walls_drawn(&mut h), 1);
    let _ = (std::fs::remove_file(&a), std::fs::remove_file(&b));
}

#[test]
fn a_linked_image_is_placed_on_the_plan_selected_and_listed_on_the_images_tab() {
    let path = std::env::temp_dir().join(format!("bimcraft-ui-manage-links-image-{}.png", std::process::id()));
    // A blank drawing's picture: a white PNG of 4 by 4 px.
    let view = bimcraft_core::render::View { angle: 0.0, scale: 1.0, cx: 0.0, cy: 0.0, width: 4, height: 4 };
    let png = bimcraft_core::render::png(&bimcraft_core::render::Drawing::new(view)).unwrap();
    std::fs::write(&path, png).unwrap();
    let mut h = app("");
    h.state_mut().image_path(&path, true);
    h.run();
    let ids = ids(&h, "image");
    assert_eq!(ids.len(), 1, "{}", h.state().status.text);
    assert!(h.state().selection.contains(&ids[0]), "what came in is selected");
    h.state_mut().open_manage_links();
    h.run();
    let d = h.state_mut().manage_links.as_mut().unwrap();
    d.tab = bimcraft_ui_images_tab();
    let rows = d.rows();
    assert_eq!((rows.len(), rows[0]["status"].clone()), (1, json!("loaded")), "{rows:?}");
    let _ = std::fs::remove_file(&path);
}

/// Where Manage Links has its Images tab.
fn bimcraft_ui_images_tab() -> usize {
    crate::manage_links::TABS.iter().position(|(k, _)| *k == "images").unwrap()
}

/// A white PNG of 40 by 20 px: its path.
fn white_png(name: &str) -> PathBuf {
    let view = bimcraft_core::render::View { angle: 0.0, scale: 1.0, cx: 0.0, cy: 0.0, width: 40, height: 20 };
    let png = bimcraft_core::render::png(&bimcraft_core::render::Drawing::new(view)).unwrap();
    let path = std::env::temp_dir().join(format!("bimcraft-ui-image-{name}-{}.png", std::process::id()));
    std::fs::write(&path, png).unwrap();
    path
}

/// The box with an image 2 m wide in its middle: the app, and the image's id.
fn with_image(name: &str) -> (Harness<'static, BimApp>, Id) {
    let path = white_png(name);
    let mut h = app(BOX);
    let v = h.state_mut().run("image", json!({"path": path.to_string_lossy(), "at": [4000, 3000], "width": 2000})).unwrap();
    let id = v["id"].as_u64().unwrap();
    h.state_mut().selection = vec![id];
    h.run();
    (h, id)
}

fn image(h: &Harness<BimApp>, id: Id) -> bimcraft_core::model::PlacedImage {
    match h.state().session.model.elements.get(id) {
        Some(bimcraft_core::model::Element::Image(im)) => im.clone(),
        _ => panic!("no image {id}"),
    }
}

#[test]
fn an_image_selected_offers_what_it_takes_and_nothing_that_trims_or_paints() {
    let (mut h, _) = with_image("tools");
    // Wide enough that the Modify tab's panels don't fold into lists.
    h.set_size(egui::vec2(1600.0, 800.0));
    h.state_mut().tab = Tab::Modify;
    h.run();
    for kept in ["Move", "Copy", "Mirror Pick Axis", "Mirror Draw Axis", "Delete"] {
        assert!(h.query_by_label(kept).is_some(), "{kept}");
    }
    for gone in ["Trim/Extend to Corner", "Split Element", "Offset", "Paint", "Demolish", "Align"] {
        assert!(h.query_by_label(gone).is_none(), "{gone} isn't for an image");
    }
    // A wall selected has them all.
    let wall = ids(&h, "wall")[0];
    h.state_mut().selection = vec![wall];
    h.state_mut().tab = Tab::Modify;
    h.run();
    assert!(!h.state().images_selected());
    assert!(h.query_by_label("Trim/Extend to Corner").is_some() && h.query_by_label("Paint").is_some(), "a wall's tools");
}

#[test]
fn a_corner_grip_dragged_resizes_the_image_keeping_its_aspect_and_the_corner_across() {
    let (mut h, id) = with_image("grip");
    let before = image(&h, id);
    // Its upper right corner, dragged 1 m out to the right.
    let [lo, hi] = before.bounds(&h.state().session.model).unwrap();
    drag(&mut h, hi, P2(hi.0 + 1000.0, hi.1));
    let after = image(&h, id);
    assert!((after.width - 3000.0).abs() < 50.0, "{} wide", after.width);
    let [lo2, _] = after.bounds(&h.state().session.model).unwrap();
    assert!(lo2.dist(lo) < 1.0, "the lower left corner stays: {lo:?} {lo2:?}");
}

#[test]
fn properties_draw_an_image_in_front_and_set_its_size() {
    let (mut h, id) = with_image("properties");
    assert!(h.query_by_label("Draw in Front of Model").is_some(), "its row");
    assert!(h.query_by_label("Paper Size").is_none(), "a picture has no paper");
    h.get_by_role_and_label(Role::CheckBox, "Draw in Front of Model").scroll_to_me();
    h.run();
    pick(&mut h, Role::CheckBox, "Draw in Front of Model");
    assert!(image(&h, id).front);
}

#[test]
fn a_pdf_of_several_pages_asks_which_to_place() {
    let mut s = Session::new();
    let one = s.execute("sheet", &json!({})).unwrap()["id"].as_u64().unwrap();
    let two = s.execute("sheet", &json!({})).unwrap()["id"].as_u64().unwrap();
    let file = std::env::temp_dir().join(format!("bimcraft-ui-pdf-pages-{}.pdf", std::process::id()));
    s.execute("export", &json!({"format": "pdf", "views": [one, two], "path": file})).unwrap();
    let mut h = app("");
    h.state_mut().image_path(&file, false);
    h.run();
    let pages = h.state().pdf_page_box.as_ref().map(|b| b.pages.len());
    assert_eq!(pages, Some(2), "{}", h.state().status.text);
    assert!(ids(&h, "image").is_empty(), "nothing placed until a page is picked");
    // Each page shown small, drawn on a thread of its own.
    for _ in 0..500 {
        if h.state().pdf_page_box.as_ref().is_some_and(|b| b.thumbs.len() == 2) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        h.run();
    }
    assert_eq!(h.state().pdf_page_box.as_ref().map(|b| b.thumbs.len()), Some(2), "both pages' pictures");
    h.get_by_label_contains("Page 2").click();
    h.run();
    press(&mut h, "OK");
    let placed = ids(&h, "image");
    assert_eq!(placed.len(), 1, "{}", h.state().status.text);
    assert_eq!(image(&h, placed[0]).page, Some(2));
    let _ = std::fs::remove_file(&file);
}

#[test]
fn manage_links_is_of_one_size_whatever_its_tab() {
    let path = shell("size");
    let mut h = app("");
    h.state_mut().link_path(&path);
    h.state_mut().open_manage_links();
    h.run();
    let rect = |h: &Harness<BimApp>| h.ctx.memory(|m| m.area_rect(egui::Id::new("manage-links"))).expect("the window");
    let first = rect(&h);
    for i in 0..crate::manage_links::TABS.len() {
        h.state_mut().manage_links.as_mut().unwrap().tab = i;
        h.run();
        let now = rect(&h);
        assert!((now.height() - first.height()).abs() < 0.5 && (now.top() - first.top()).abs() < 0.5, "tab {i}: {first:?} {now:?}");
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_pdf_snaps_to_its_lines_when_its_box_is_ticked_and_a_picture_has_no_box() {
    let mut s = Session::new();
    let sheet = s.execute("sheet", &json!({})).unwrap()["id"].as_u64().unwrap();
    let file = std::env::temp_dir().join(format!("bimcraft-ui-pdf-snaps-{}.pdf", std::process::id()));
    s.execute("export", &json!({"format": "pdf", "view": sheet, "path": file})).unwrap();
    let mut h = app("");
    h.state_mut().place_image(&file, false, None);
    h.run();
    let id = ids(&h, "image")[0];
    assert!(!image(&h, id).snaps);
    h.get_by_role_and_label(Role::CheckBox, "Snap to Its Lines").scroll_to_me();
    h.run();
    pick(&mut h, Role::CheckBox, "Snap to Its Lines");
    assert!(image(&h, id).snaps, "ticked, it snaps");
    let _ = std::fs::remove_file(&file);
    let (h, _) = with_image("no-snaps");
    assert!(h.query_by_label("Snap to Its Lines").is_none(), "a picture has no lines to snap to");
}
