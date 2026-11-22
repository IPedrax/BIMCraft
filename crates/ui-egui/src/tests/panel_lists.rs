//! A ribbon panel's name lists its tools only while some of them are out of sight: with every tool on the ribbon the
//! name is the panel's name and no more; once the tab makes room, handing tools over or folding, the name lists them.

use super::*;

/// The names of the panels that list their tools now: "Build ▾" and the like.
fn listing(h: &Harness<BimApp>) -> Vec<String> {
    use egui_kittest::kittest::{By, NodeT};
    h.query_all(By::new()).filter_map(|n| n.accesskit_node().label()).filter(|l| l.ends_with(" ▾")).collect()
}

#[test]
fn a_panels_name_lists_its_tools_only_while_some_are_out_of_sight() {
    let mut h = app(BOX);
    // Wide enough for every tool of Architecture: no panel's name drops a list but Build's, whose Curtain Wall only its list
    // shows, and a tool is pressed where it stands.
    h.set_size(vec2(1900.0, 900.0));
    for _ in 0..3 {
        h.run();
    }
    assert_eq!(listing(&h), vec!["Build ▾".to_string()]);
    assert_eq!(h.get_all_by_label("Wall").count(), 1, "the ribbon's own button, and no row of a list");
    press(&mut h, "Wall");
    assert!(matches!(h.state().tool, Tool::Wall(_)));
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    press(&mut h, "Build ▾");
    assert!(h.query_by_label("Curtain Wall").is_some(), "Build's list has Curtain Wall");
    key(&mut h, Key::Escape);
    // Narrower, the tab makes room: the panels that hand tools over or fold list theirs, the others still don't.
    h.set_size(vec2(900.0, 900.0));
    for _ in 0..3 {
        h.run();
    }
    let lists = listing(&h);
    assert!(!lists.is_empty(), "at 900 wide Architecture hands tools over");
    assert!(lists.contains(&"Build ▾".to_string()), "{lists:?}");
    // The list has every tool of the panel, those still on the ribbon too.
    press(&mut h, "Build ▾");
    let lowest =
        |h: &Harness<BimApp>, name: &str| h.get_all_by_label(name).map(|n| n.rect().center()).max_by(|a, b| a.y.total_cmp(&b.y)).unwrap();
    let row = lowest(&h, "Ceiling");
    click_at(&mut h, row);
    assert_eq!(h.state().tool, Tool::Ceiling);
}

/// The project's Architecture tab and the family's Create tab at 1900 wide, and Architecture at 900, where panels hand tools
/// over and fold; in Rev, light and dark. Saved where `BIMCRAFT_PANEL_LISTS_DUMP` says (only then).
#[test]
fn panel_lists_screenshots() {
    let Ok(dir) = std::env::var("BIMCRAFT_PANEL_LISTS_DUMP") else { return };
    std::fs::create_dir_all(&dir).unwrap();
    let dir = std::path::Path::new(&dir);
    for (look, dark) in [("light", false), ("dark", true)] {
        for (name, width, family) in [("architecture", 1900.0, false), ("create", 1900.0, true), ("narrow", 900.0, false)] {
            let mut h = app(&super::family_drag_move::family_with(""));
            h.set_size(vec2(width, 420.0));
            if family {
                h.state_mut().edit_family("Movers");
            }
            h.ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
            for _ in 0..4 {
                h.run();
            }
            h.render().unwrap().save(dir.join(format!("panel-lists-{name}-{look}.png"))).unwrap();
        }
    }
}
