//! Manage > Asset Manager, driven as a user drives it: each kind's tab, where an asset is used, and a .bima out and in.

use super::*;

#[test]
fn the_asset_manager_lists_what_the_project_draws_with_and_where_it_is_used() {
    let mut h = app(&format!("{BOX}\nline_style {{\"name\": \"Site\", \"pen\": 3}}"));
    let line =
        h.state_mut().run("detail_line", json!({"view": "Level 1", "points": [[0, 0], [1000, 0]], "style": "Site"})).unwrap()["id"].clone();
    press(&mut h, "Manage");
    press(&mut h, "Asset Manager");
    assert!(h.state().asset_manager.is_some());
    assert!(h.query_by_label_contains("Line Styles (").is_some());
    h.get_by_label_contains("Line Styles (").click();
    h.run();
    press(&mut h, "Site");
    assert!(h.query_by_label("Used by").is_some());
    assert!(h.query_by_label_contains(&line.to_string()).is_some(), "the detail line drawn in it, by its id");
    press(&mut h, "Close");
    assert!(h.state().asset_manager.is_none());
}

#[test]
fn assets_go_out_to_a_bima_and_come_into_another_project() {
    let path = std::env::temp_dir().join(format!("bimcraft-ui-assets-{}.bima", std::process::id()));
    let mut a = app("material {\"name\": \"Oak\", \"color\": \"#a0522d\"}");
    a.state_mut().open_asset_manager();
    a.run();
    let mut d = a.state_mut().asset_manager.take().unwrap();
    d.checked.insert(("materials".into(), "Oak".into()));
    let chosen = d.chosen();
    assert!(a.state_mut().export_assets_to(&path, chosen), "{}", a.state().status.text);
    let mut b = app("");
    assert!(b.state_mut().import_assets_from(&path, "skip"), "{}", b.state().status.text);
    assert_eq!(b.state().session.model.materials["Oak"].color, [0xa0, 0x52, 0x2d]);
    let _ = std::fs::remove_file(&path);
}
