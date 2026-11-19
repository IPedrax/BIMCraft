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
