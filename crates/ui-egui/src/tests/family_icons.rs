//! The Family Editor's Create tab, each tool with an icon of its own: the six forms, which shared Create Form's, Model
//! Text, Connector and Show; Reference Plane and Set take the icons the project's own tools have.

use super::*;
use crate::icons::Icon;

/// The Create tab's own icons, in its order.
const DRAWN: [Icon; 9] = [
    Icon::Extrusion,
    Icon::Blend,
    Icon::Revolve,
    Icon::Sweep,
    Icon::SweptBlend,
    Icon::VoidForms,
    Icon::ModelText,
    Icon::Connector,
    Icon::ShowWorkPlane,
];

#[test]
fn a_selected_forms_properties_show_its_own_tools_icon() {
    // The icon heading Properties is its tool's, not Create Form's for every form: read from what the family holds.
    let mut h = app(&super::family_drag_move::family_with(""));
    h.state_mut().edit_family("Movers");
    h.run();
    let icons = super::family_views::editor(&h).form_icons();
    assert!(!icons.is_empty());
    for icon in icons {
        assert_ne!(icon, Icon::CreateForm);
        assert!(DRAWN.contains(&icon), "{icon:?}");
    }
}

/// The Create tab in both profiles and the three looks, and a sheet of the new icons at the ribbon's size, at a list's and
/// enlarged four times: saved where `BIMCRAFT_FAMILY_CREATE_ICONS_DUMP` says (only then).
#[test]
fn family_create_tab_icons_screenshots() {
    let Ok(dir) = std::env::var("BIMCRAFT_FAMILY_CREATE_ICONS_DUMP") else { return };
    std::fs::create_dir_all(&dir).unwrap();
    let dir = std::path::Path::new(&dir);
    let script = super::family_drag_move::family_with("");
    for (profile, rev) in [("rev", true), ("bimcraft", false)] {
        for (look, dark, light_canvas) in [("light", false, false), ("dark", true, false), ("dark-ui-light-canvas", true, true)] {
            let mut h = if rev { app(&script) } else { bimcraft(&script) };
            h.set_size(vec2(1900.0, 900.0));
            h.state_mut().edit_family("Movers");
            h.ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
            h.state_mut().light_canvas = light_canvas;
            for _ in 0..3 {
                h.run();
            }
            h.render().unwrap().save(dir.join(format!("create-icons-{profile}-{look}.png"))).unwrap();
        }
    }
    for (name, ink, paper) in [
        ("light", egui::Color32::from_gray(40), egui::Color32::WHITE),
        ("dark", egui::Color32::from_gray(225), egui::Color32::from_gray(32)),
    ] {
        let mut h = Harness::builder().with_size([DRAWN.len() as f32 * 112.0 + 16.0, 200.0]).build_ui(move |ui| {
            ui.painter().rect_filled(ui.max_rect().expand(8.0), 0.0, paper);
            for (k, icon) in DRAWN.iter().enumerate() {
                let x = 16.0 + 112.0 * k as f32;
                // A ribbon button's 26, a list row's 20, a stacked button's 16; and four times the first.
                for (dx, y, side) in [(0.0, 12.0, 26.0), (34.0, 15.0, 20.0), (62.0, 17.0, 16.0), (0.0, 60.0, 104.0)] {
                    crate::icons::paint(ui.painter(), *icon, egui::Rect::from_min_size(pos2(x + dx, y), vec2(side, side)), ink);
                }
                ui.painter().text(pos2(x + 52.0, 182.0), egui::Align2::CENTER_CENTER, icon.name(), egui::FontId::proportional(12.0), ink);
            }
        });
        h.run();
        h.render().unwrap().save(dir.join(format!("create-icons-sheet-{name}.png"))).unwrap();
    }
}
