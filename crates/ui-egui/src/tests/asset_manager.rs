//! Manage > Asset Manager, driven as a user drives it: each kind's tab, where an asset is used, and a .bima out and in.

use super::*;

#[test]
fn the_asset_manager_lists_what_the_project_draws_with_and_where_it_is_used() {
    let mut h = app(&format!("{BOX}\nline_style {{\"name\": \"Site\", \"pen\": 3}}"));
    let line =
        h.state_mut().run("detail_line", json!({"view": "Level 1", "points": [[0, 0], [1000, 0]], "style": "Site"})).unwrap()["id"].clone();
    press(&mut h, "Manage");
    press(&mut h, "Annotation Settings");
    press(&mut h, "Asset Manager");
    assert!(h.state().asset_manager.is_some());
    assert!(h.query_by_label_contains("Line Styles (").is_some());
    h.get_by_label_contains("Line Styles (").click();
    h.run();
    press(&mut h, "Site");
    assert!(h.query_by_label("Used by").is_some());
    assert!(h.query_by_label_contains(&line.to_string()).is_some(), "the detail line drawn in it, by its id");
    // Import's choice for duplicates is under it, as it says.
    let import = h.get_by_label("Import…").rect();
    let choice = h.get_by_label("When importing duplicates:").rect();
    assert!(choice.top() >= import.bottom() - 1.0, "{import:?} {choice:?}");
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

#[test]
fn hovering_the_asset_managers_rows_leaves_it_where_it_stands() {
    let mut h = app(&format!("{BOX}\nmaterial {{\"name\": \"Oak\", \"color\": \"#a0522d\"}}"));
    h.state_mut().open_asset_manager();
    h.run();
    let rect = |h: &Harness<BimApp>| h.ctx.memory(|m| m.area_rect(egui::Id::new("asset-manager"))).expect("the window");
    let first = rect(&h);
    let names: Vec<String> =
        h.state().asset_manager.as_ref().unwrap().rows().iter().take(6).filter_map(|r| r["name"].as_str().map(str::to_string)).collect();
    for _ in 0..3 {
        for name in &names {
            h.get_by_label(name).hover();
            h.run();
            // Picked, it shows where it's used beside the list; the window stays as it was.
            h.get_by_label(name).click();
            h.run();
            let now = rect(&h);
            assert!(
                (now.top() - first.top()).abs() < 0.5 && (now.height() - first.height()).abs() < 0.5,
                "{name}: {first:?} became {now:?}"
            );
        }
    }
}

#[test]
fn delete_takes_an_unused_asset_out_and_says_why_a_used_one_stays() {
    let mut h = app("material {\"name\": \"Spare\", \"color\": \"#00ff00\"}\nline_style {\"name\": \"Site\", \"pen\": 3}");
    let line = h.state_mut().run("detail_line", json!({"view": "Level 1", "points": [[0, 0], [1000, 0]], "style": "Site"}));
    assert!(line.is_some());
    h.state_mut().open_asset_manager();
    h.run();
    // Down the list, as a user scrolls to it.
    h.get_by_label("Spare").scroll_to_me();
    h.run();
    h.get_by_label("Spare").click();
    h.run();
    press(&mut h, "Delete 1");
    assert!(!h.state().session.model.materials.contains_key("Spare"));
    h.get_by_label_contains("Line Styles (").click();
    h.run();
    h.get_by_label("Site").scroll_to_me();
    h.run();
    h.get_by_label("Site").click();
    h.run();
    press(&mut h, "Delete 1");
    assert!(h.state().session.model.line_styles.contains_key("Site"));
    assert!(h.query_by_label_contains("take it off them first").is_some(), "said in the window");
}

#[test]
fn a_long_name_leaves_the_asset_manager_its_width() {
    let long = "A material named at such length that no window could ever hope to show the whole of it on one line";
    let mut h = app(&format!("material {{\"name\": \"{long}\", \"color\": \"#a0522d\"}}"));
    h.state_mut().open_asset_manager();
    h.run();
    let w = h.ctx.memory(|m| m.area_rect(egui::Id::new("asset-manager"))).unwrap().width();
    assert!(w < 800.0, "{w} wide");
}
