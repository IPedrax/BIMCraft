//! Graphic Display Options (GD): the 3D view's edges, silhouettes and ambient shadows, set in the dialog that GD and the view
//! control bar's Visual Style menu open, alike in both looks of the window.

use super::*;
use bimcraft_core::graphics::Look;
use bimcraft_core::view3d::DisplayOptions;

fn options(h: &Harness<BimApp>) -> DisplayOptions {
    Look::of_3d(&h.state().session.model).display()
}

fn open(h: &Harness<BimApp>) -> bool {
    h.state().display_dialog.is_some()
}

/// The dialog's own button `label`: of those so named (Properties has an Apply too), the one under its title nearest across.
fn button_of_dialog(h: &mut Harness<BimApp>, label: &str) {
    let title = h.get_by_label("Graphic Display Options").rect().center();
    let off = |r: egui::Rect| (r.center().x - title.x).abs();
    let at = h.get_all_by_label(label).map(|n| n.rect()).filter(|r| r.top() > title.y).min_by(|a, b| off(*a).total_cmp(&off(*b)));
    click_at(h, at.map(|r| r.center()).unwrap_or(title));
}

#[test]
fn graphic_display_options_open_by_gd_and_from_the_visual_style_menu_in_both_looks_and_set_the_3d_views() {
    for rev in [false, true] {
        let mut h = if rev { app(BOX) } else { bimcraft(BOX) };
        // A plan has none to set, and says so.
        shortcut(&mut h, "GD");
        assert!(!open(&h) && h.state().status.error, "rev {rev}");
        shortcut(&mut h, "3D");
        assert_eq!(h.state().sheet, Sheet::View3D);
        assert_eq!(options(&h), DisplayOptions::default());
        // GD: edges off, a 3 px silhouette, ambient shadows; Apply sets them and leaves the dialog open.
        shortcut(&mut h, "GD");
        assert!(open(&h), "rev {rev}");
        pick(&mut h, Role::CheckBox, "Show Edges");
        choose(&mut h, "Silhouettes", "3 px");
        pick(&mut h, Role::CheckBox, "Show Ambient Shadows");
        button_of_dialog(&mut h, "Apply");
        let set = DisplayOptions { edges: false, silhouette: 3, ambient_shadows: true };
        assert_eq!(options(&h), set, "rev {rev}");
        assert!(open(&h));
        assert_eq!(h.state().session.history().last(), Some(&"Graphic Display Options"));
        // Cancel takes back only what was changed since.
        pick(&mut h, Role::CheckBox, "Show Ambient Shadows");
        button_of_dialog(&mut h, "Cancel");
        assert!(!open(&h));
        assert_eq!(options(&h), set);
        // From the Visual Style menu, it shows what the view has; OK sets and closes.
        pick(&mut h, Role::Button, "Visual Style");
        press(&mut h, "Graphic Display Options...");
        assert!(open(&h), "rev {rev}");
        assert_eq!(h.state().display_dialog.as_ref().map(|d| d.options), Some(set));
        pick(&mut h, Role::CheckBox, "Show Edges");
        choose(&mut h, "Silhouettes", "<None>");
        button_of_dialog(&mut h, "OK");
        assert!(!open(&h));
        assert_eq!(options(&h), DisplayOptions { edges: true, silhouette: 0, ambient_shadows: true }, "rev {rev}");
        // OK with nothing changed is no step to undo.
        let steps = h.state().session.history().len();
        shortcut(&mut h, "GD");
        button_of_dialog(&mut h, "OK");
        assert!(!open(&h));
        assert_eq!(h.state().session.history().len(), steps);
    }
}

#[test]
fn hidden_line_draws_its_edges_whatever_the_view_says_so_show_edges_is_greyed_out() {
    for rev in [false, true] {
        let mut h = if rev { app(BOX) } else { bimcraft(BOX) };
        shortcut(&mut h, "3D");
        shortcut(&mut h, "HL");
        shortcut(&mut h, "GD");
        let edges = h.get_by_role_and_label(Role::CheckBox, "Show Edges");
        assert!(egui_kittest::kittest::NodeT::accesskit_node(&edges).is_disabled(), "rev {rev}");
        button_of_dialog(&mut h, "Cancel");
        assert!(!open(&h));
    }
}
