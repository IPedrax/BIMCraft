use super::*;

#[test]
fn visibility_graphics_categories_follow_the_alphabet_of_the_displayed_names() {
    let mut h = app(BOX);
    shortcut(&mut h, "VV");
    for names in [["Beams", "Cable Trays", "Cables", "Ceilings"], ["Dimensions", "Elevations", "Grids", "Levels"]] {
        let positions: Vec<_> = names.iter().map(|n| h.get_by_role_and_label(Role::CheckBox, n).rect().top()).collect();
        assert!(positions.windows(2).all(|p| p[0] < p[1]), "{names:?}: {positions:?}");
        press(&mut h, "Annotation Categories");
    }
}

#[test]
fn visibility_graphics_sorts_categories_in_each_interface_language() {
    use crate::lang::{self, Lang};
    use bimcraft_core::graphics::{ANNOTATION, MODEL, SCANS};
    let mut h = app(BOX);
    for language in [Lang::Portuguese, Lang::Spanish, Lang::Chinese] {
        crate::lang::assume_chinese_font(&h.ctx);
        h.state_mut().lang = language;
        shortcut(&mut h, "VV");
        for (tab, cats) in
            [("Model Categories", MODEL.iter().chain(&SCANS).copied().collect::<Vec<_>>()), ("Annotation Categories", ANNOTATION.to_vec())]
        {
            let tr = |en| lang::translated_in(language, en).unwrap_or(en);
            press(&mut h, tr(tab));
            // A subcategory sits folded under its category, not among them.
            let mut names: Vec<_> =
                cats.into_iter().filter(|c| bimcraft_core::graphics::sub(c).is_none()).map(|c| lang::category(c, true)).collect();
            names.sort_by_key(|n| n.to_lowercase());
            // Equal translated names may belong to different categories, so inspect all their positions.
            let positions: Vec<_> = names
                .iter()
                .map(|n| h.get_all_by_role_and_label(Role::CheckBox, n).map(|r| r.rect().top()).fold(f32::INFINITY, f32::min))
                .collect();
            assert!(positions.windows(2).all(|p| p[0] <= p[1]), "{language:?} {tab}: {names:?} {positions:?}");
        }
        press(&mut h, lang::translated_in(language, "Cancel").unwrap());
    }
}

#[test]
fn visibility_graphics_edits_pattern_layers_as_a_draft_and_clears_only_the_pattern_cell() {
    let mut h = app(BOX);
    shortcut(&mut h, "VV");
    pick(&mut h, Role::Button, "Floors: Projection Patterns");
    pick(&mut h, Role::CheckBox, "Projection: Foreground");
    choose(&mut h, "Projection: Foreground Pattern", "Diagonal Up");
    pick(&mut h, Role::CheckBox, "Projection: Background");
    choose(&mut h, "Projection: Background Pattern", "Solid fill");
    assert!(plan_graphics(&h, 100000).is_default());
    press(&mut h, "OK");
    press_near(&mut h, "Apply", "Cancel");
    let g = serde_json::to_value(plan_graphics(&h, 100000)).unwrap();
    assert_eq!(g["categories"]["floor"]["projection"]["foreground"]["pattern"], "Diagonal Up");
    assert_eq!(g["categories"]["floor"]["projection"]["background"]["pattern"], "Solid fill");
    pick(&mut h, Role::Button, "Floors: Projection Lines");
    pick(&mut h, Role::CheckBox, "Projection: Line weight");
    press(&mut h, "OK");
    pick(&mut h, Role::Button, "Floors: Projection Patterns");
    choose(&mut h, "Projection: Foreground Pattern", "<None>");
    press(&mut h, "Cancel");
    pick(&mut h, Role::Button, "Floors: Projection Patterns");
    press(&mut h, "Clear Overrides");
    press(&mut h, "OK");
    press(&mut h, "OK");
    let g = plan_graphics(&h, 100000);
    assert_eq!(g.categories["floor"].projection.weight, Some(Pen::PROJECTION));
    assert!(g.categories["floor"].projection.foreground.is_none() && g.categories["floor"].projection.background.is_none());
    shortcut(&mut h, "VV");
    press(&mut h, "Annotation Categories");
    pick(&mut h, Role::Button, "Filled Regions: Projection Patterns");
    assert!(h.query_by_role_and_label(Role::CheckBox, "Projection: Foreground").is_some());
    assert!(h.query_by_role_and_label(Role::CheckBox, "Cut: Foreground").is_none());
    press(&mut h, "Cancel");
    press(&mut h, "Cancel");
}
