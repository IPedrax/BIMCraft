//! The ribbon's panels fit the window: none folds or hands its tools over while the tab has room for them.
use super::*;
use bimcraft_core::family::Category;

/// A new family's document in a window `width` wide, on its Create tab.
fn family(width: f32) -> Harness<'static, BimApp> {
    let mut h = app("");
    h.set_size(vec2(width, 900.0));
    h.state_mut().new_family(Category::Furniture);
    h.run();
    h
}

/// The family's extrusion selected: Modify | Extrusion.
fn select_extrusion(h: &mut Harness<BimApp>) {
    h.get_by_label("1. Extrusion").scroll_to_me();
    h.run();
    press(h, "1. Extrusion");
}

#[test]
fn a_family_at_1280_folds_no_panel_its_ribbon_has_room_for() {
    let mut h = family(1280.0);
    // Create: Work Plane (Set) and Connectors stay whole; Dimension folds and Forms hands its last tools over.
    for tool in ["Set", "Connector", "Extrusion", "Blend", "Revolve"] {
        assert!(h.query_by_label(tool).is_some(), "{tool}");
    }
    // Modify | Extrusion: the Modify panel hands its last tools over, and Work Plane, after it, stays whole.
    select_extrusion(&mut h);
    assert!(h.query_by_label("Edit Work Plane").is_some(), "Work Plane folded");
    assert!(h.query_by_label("Delete").is_none(), "Modify's last tool handed over");
}

/// Modify | Extrusion at 1280 and 1440, the family's Create tab and the project's Architecture tab at 1280, saved where
/// `BIMCRAFT_RIBBON_DUMP` says (only then): the window and the ribbon three times as large.
#[test]
fn ribbon_fit_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_RIBBON_DUMP") else { return };
    let folder = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&folder).unwrap();
    let save = |h: &mut Harness<BimApp>, name: &str| {
        // The pointer off to the side, where it lights nothing.
        let size = h.ctx.content_rect().size();
        h.hover_at(pos2(size.x - 10.0, size.y - 10.0));
        h.run();
        let picture = h.render().unwrap();
        picture.save(folder.join(format!("{name}.png"))).unwrap();
        let ribbon = image::imageops::crop_imm(&picture, 0, 0, picture.width(), 200).to_image();
        ribbon.save(folder.join(format!("{name}-ribbon.png"))).unwrap();
    };
    for width in [1280.0, 1440.0] {
        let mut h = family(width);
        if width == 1280.0 {
            save(&mut h, "family-create-1280");
        }
        select_extrusion(&mut h);
        save(&mut h, &format!("modify-extrusion-{width}"));
    }
    let mut h = app(BOX);
    h.state_mut().tab = Tab::Architecture;
    h.run();
    save(&mut h, "project-architecture-1280");
}
