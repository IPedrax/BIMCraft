//! Themes: the system's by default, presets (Dracula, Midnight, AMOLED), and an accent of the user's own, the same in both
//! profiles.

use super::*;
use crate::style::{MIDNIGHT, Scheme, contrast, pal};

#[test]
fn a_theme_preset_and_an_accent_hold_in_both_profiles_and_the_settings_file_keeps_them() {
    for rev in [false, true] {
        let file = std::env::temp_dir().join(format!("bimcraft-ui-themes-{rev}-{}", std::process::id())).join("settings.json");
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
        let mut h = if rev { app(BOX) } else { bimcraft(BOX) };
        h.state_mut().use_settings(file.clone());
        h.run();
        assert_eq!(h.state().scheme, Scheme::Own, "the profile's own colours, light or dark as the system is");
        press(&mut h, "Settings");
        // Below the page's fold: pressed as a screen reader does.
        h.get_by_role_and_label(Role::RadioButton, "Midnight").click_accesskit();
        h.run();
        press(&mut h, "Close");
        assert!(crate::style::dark() && pal().panel == MIDNIGHT.panel, "rev {rev}");
        let kept = |f: &std::path::Path| -> Value { serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap() };
        assert_eq!(kept(&file)["scheme"], "midnight");
        // An accent of the user's, its ink stepped until it reads on every ground.
        h.state_mut().accent = Some([255, 200, 0]);
        h.run();
        let p = pal();
        assert_eq!(p.accent, egui::Color32::from_rgb(255, 200, 0));
        for ground in [p.panel, p.bar, p.surface, p.select_fill, p.hover_fill] {
            assert!(contrast(p.accent_ink, ground) >= 4.5, "rev {rev}: {:?} on {ground:?}", p.accent_ink);
        }
        assert_eq!(kept(&file)["accent"], json!([255, 200, 0]));
        press(&mut h, "Settings");
        h.get_by_role_and_label(Role::RadioButton, "As the System").click_accesskit();
        h.run();
        press(&mut h, "Close");
        assert_eq!(h.state().scheme, Scheme::Own);
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }
}
