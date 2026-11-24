//! On a family document's tabs a ribbon panel's name lists its tools only while some of them are out of sight: with every
//! tool on the ribbon the name is the panel's name and no more; once the tab makes room, handing tools over or folding, the
//! name lists them. A project's tabs are as they were: every panel's name lists its tools.

use super::*;

/// The names of the panels that list their tools now: "Forms ▾" and the like.
fn listing(h: &Harness<BimApp>) -> Vec<String> {
    use egui_kittest::kittest::{By, NodeT};
    h.query_all(By::new()).filter_map(|n| n.accesskit_node().label()).filter(|l| l.ends_with(" ▾")).collect()
}

#[test]
fn a_family_panels_name_lists_its_tools_only_while_some_are_out_of_sight() {
    let mut h = app(&super::family_drag_move::family_with(""));
    h.state_mut().edit_family("Movers");
    // Wide enough for every tool of Create: no panel's name drops a list, and a tool is pressed where it stands.
    h.set_size(vec2(1900.0, 900.0));
    for _ in 0..3 {
        h.run();
    }
    for panel in ["Forms", "Model", "Datum"] {
        assert!(h.query_by_label(&format!("{panel} ▾")).is_none(), "{panel}: no list with every tool in sight");
    }
    assert_eq!(h.get_all_by_label("Reference Plane").count(), 1, "the ribbon's own button, and no row of a list");
    // Narrower, the tab makes room: the panels that hand tools over or fold list theirs.
    h.set_size(vec2(520.0, 900.0));
    for _ in 0..3 {
        h.run();
    }
    let lists = listing(&h);
    assert!(lists.iter().any(|l| ["Forms ▾", "Model ▾", "Datum ▾"].contains(&l.as_str())), "at 520 wide Create makes room: {lists:?}");
}

#[test]
fn a_projects_panels_name_lists_its_tools_with_every_tool_in_sight() {
    let mut h = app(BOX);
    h.set_size(vec2(1900.0, 900.0));
    for _ in 0..3 {
        h.run();
    }
    // Every tool of Build is on the ribbon, and its name still lists them.
    assert_eq!(h.get_all_by_label("Wall").count(), 1, "the ribbon's own button");
    press(&mut h, "Build ▾");
    assert!(h.query_by_label("Curtain Wall").is_some(), "Build's list has Curtain Wall");
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
