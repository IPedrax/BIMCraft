//! A sheet's built-in frame is a property, not a separate drawing path.
use super::*;
use egui_kittest::kittest::NodeT;

#[test]
fn a_sheets_title_block_is_shown_and_switched_in_properties_in_both_profiles_and_every_look() {
    for profile in crate::style::Profile::ALL {
        for (theme, light_canvas) in
            [(egui::ThemePreference::Light, false), (egui::ThemePreference::Dark, false), (egui::ThemePreference::Dark, true)]
        {
            let mut h = harness("sheet {}", 0.25, profile);
            let sheet = ids(&h, "sheet")[0];
            h.state_mut().open_sheet(Sheet::Paper(sheet));
            h.ctx.set_theme(theme);
            h.state_mut().light_canvas = light_canvas;
            h.run();
            assert_eq!(field(&h, sheet, "title_block"), json!(true));
            h.get_by_role_and_label(Role::CheckBox, "Title Block").scroll_to_me();
            h.run();
            assert_eq!(
                h.get_by_role_and_label(Role::CheckBox, "Title Block").accesskit_node().toggled(),
                Some(egui::accesskit::Toggled::True)
            );
            pick(&mut h, Role::CheckBox, "Title Block");
            assert_eq!(field(&h, sheet, "title_block"), json!(false));
            assert!(!h.state().session.model.sheet(sheet).unwrap().title_block);
            pick(&mut h, Role::CheckBox, "Title Block");
            assert_eq!(field(&h, sheet, "title_block"), json!(true));
            assert!(h.state().session.model.sheet(sheet).unwrap().title_block);
        }
    }
}
