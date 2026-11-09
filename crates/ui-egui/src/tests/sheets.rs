//! A sheet drawn with no title block is a choice of its Title Block, beside the built-in one and the project's, not a
//! separate drawing path.
use super::*;

#[test]
fn a_sheets_title_block_is_shown_and_set_to_none_in_properties_in_both_profiles_and_every_look() {
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
            assert_eq!(field(&h, sheet, "title_block"), json!(""));
            h.get_by_role_and_label(Role::ComboBox, "Title Block").scroll_to_me();
            h.run();
            assert_eq!(h.get_by_role_and_label(Role::ComboBox, "Title Block").value().as_deref(), Some("Built-in"));
            choose(&mut h, "Title Block", "None");
            assert_eq!(field(&h, sheet, "title_block"), json!("none"), "{profile:?}");
            assert_eq!(h.state().session.model.sheet(sheet).unwrap().title_block, "none");
            assert_eq!(h.get_by_role_and_label(Role::ComboBox, "Title Block").value().as_deref(), Some("None"));
            choose(&mut h, "Title Block", "Built-in");
            assert_eq!(field(&h, sheet, "title_block"), json!(""), "{profile:?}");
            assert_eq!(h.state().session.model.sheet(sheet).unwrap().title_block, "");
        }
    }
}
