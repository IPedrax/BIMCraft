//! F8: Family Element Visibility Settings in the family document (from a form's contextual tab and from its Properties'
//! Visibility/Graphics Overrides Edit…), a form its settings hide in a view drawn halftone there, the document's Detail
//! Level, and the project's views' Detail Level changing what a placed door draws, in a plan and in the 3D view.
use super::family_views::{around, darkest, editor, open, select_form, view};
use super::preview_visibility::toggle;
use super::*;
use bimcraft_core::family::Visibility;
use bimcraft_core::family::views::{Side, Which};
use egui_kittest::kittest::NodeT;

/// A door: two jambs, a head, a leaf (form 4) 760 wide and 40 thick on the wall's centre with `leaf` settings, and a
/// symbolic swing in Opening.
fn door(leaf: &str) -> String {
    let leaf = if leaf.is_empty() { String::new() } else { format!(r#", "visibility": {leaf}"#) };
    let block = |x0: i32, x1: i32, z0: i32, z1: i32| {
        format!(
            r#"{{"profile": [[{x0}, "-Wall Thickness / 2"], [{x1}, "-Wall Thickness / 2"], [{x1}, "Wall Thickness / 2"], [{x0}, "Wall Thickness / 2"]], "from": {z0}, "to": {z1}}}"#
        )
    };
    format!(
        r#"family {{"name": "Test Door", "category": "door", "parameters": [{{"name": "Width", "value": 900}}, {{"name": "Height", "value": 2100}}], "types": {{"900 x 2100": {{}}}}, "forms": [{}, {}, {}, {{"profile": [[-380, -20], [380, -20], [380, 20], [-380, 20]], "to": 2000{leaf}}}], "symbols": [{{"arc": {{"centre": [-380, 100], "radius": 760, "from": 0, "to": 90}}, "subcategory": "Opening"}}, {{"line": [[-380, 100], [-380, 860]], "subcategory": "Opening"}}]}}"#,
        block(-450, -400, 0, 2050),
        block(400, 450, 0, 2050),
        block(-450, 450, 2050, 2100)
    )
}

/// The draft's leaf (form 4) settings.
fn leaf(h: &Harness<BimApp>) -> Option<Visibility> {
    editor(h).draft_family().ok().and_then(|f| f.forms.get(3).and_then(|x| x.visibility))
}

#[test]
fn visibility_settings_from_the_contextual_tab_and_from_properties_set_the_forms_settings() {
    let (mut h, _) = open(&door(""), "Test Door");
    select_form(&mut h, 4);
    assert!(h.query_all_by_label("Modify | Form").count() > 0);
    press(&mut h, "Visibility Settings");
    assert!(h.query_by_label("Family Element Visibility Settings").is_some(), "the dialog");
    for b in ["Plan/RCP", "Front/Back", "Left/Right", "Coarse", "Medium", "Fine"] {
        assert_eq!(
            h.get_by_role_and_label(Role::CheckBox, b).accesskit_node().toggled(),
            Some(egui::accesskit::Toggled::True),
            "{b} ticked as new"
        );
    }
    pick(&mut h, Role::CheckBox, "Plan/RCP");
    pick(&mut h, Role::CheckBox, "Coarse");
    press(&mut h, "OK");
    assert!(h.query_by_label("Family Element Visibility Settings").is_none(), "OK closes it");
    assert_eq!(leaf(&h), Some(Visibility { plan: false, coarse: false, ..Default::default() }));
    // From Properties: Visibility/Graphics Overrides' Edit…, ticked as set; Cancel changes nothing.
    press(&mut h, "Edit…");
    assert_eq!(h.get_by_role_and_label(Role::CheckBox, "Plan/RCP").accesskit_node().toggled(), Some(egui::accesskit::Toggled::False));
    pick(&mut h, Role::CheckBox, "Only shown when cut");
    press(&mut h, "Cancel");
    assert_eq!(leaf(&h), Some(Visibility { plan: false, coarse: false, ..Default::default() }), "Cancel");
    // Default ticks everything back; OK keeps it.
    press(&mut h, "Edit…");
    press(&mut h, "Default");
    press(&mut h, "OK");
    assert_eq!(leaf(&h), Some(Visibility::default()));
    // An undo of the document takes it back.
    shortcut_undo(&mut h);
    assert_eq!(leaf(&h), Some(Visibility { plan: false, coarse: false, ..Default::default() }), "a step of the history");
}

/// Ctrl+Z in the family document.
fn shortcut_undo(h: &mut Harness<BimApp>) {
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run();
}

/// The darkest of `cs` that isn't a reference plane's green: the sum of its channels.
fn darkest_grey(cs: &[egui::Color32]) -> u32 {
    let grey: Vec<egui::Color32> = cs.iter().copied().filter(|c| i32::from(c.g()) <= i32::from(c.r()) + 20).collect();
    darkest(&grey)
}

#[test]
fn a_form_hidden_in_a_view_by_its_settings_is_halftone_there_and_gone_with_preview_visibility() {
    let (mut h, pictures) = open(&door(r#"{"plan": false}"#), "Test Door");
    view(&mut h, Some(Which::Plan));
    // The leaf's face, away from the jambs and the swing: halftone, not ink.
    let face = [0.0, -20.0, 0.0];
    let jamb = [-425.0, -100.0, 0.0];
    let ink = darkest_grey(&around(&h, &pictures, jamb, 2));
    let leaf = darkest_grey(&around(&h, &pictures, face, 2));
    assert!(ink < 200, "the jamb, in ink: {ink}");
    assert!(leaf > ink + 100 && leaf < 700, "the leaf, hidden in plan, halftone: {leaf} against {ink}");
    toggle(&mut h);
    assert!(darkest_grey(&around(&h, &pictures, face, 2)) > 740, "Preview Visibility: gone, as the project's plan draws it");
    toggle(&mut h);
    // Front shows it as ever, in ink: Front/Back is ticked.
    view(&mut h, Some(Which::Elevation(Side::Front)));
    let top = [0.0, 0.0, 2000.0];
    assert!(darkest_grey(&around(&h, &pictures, top, 2)) < 200, "in Front, the leaf's top edge in ink");
}

#[test]
fn the_documents_detail_level_greys_a_form_not_ticked_for_it() {
    let (mut h, pictures) = open(&door(r#"{"coarse": false}"#), "Test Door");
    view(&mut h, Some(Which::Elevation(Side::Front)));
    let top = [0.0, 0.0, 2000.0];
    assert!(darkest(&around(&h, &pictures, top, 2)) < 200, "Fine: the leaf's top in ink");
    choose_detail(&mut h, "Coarse");
    assert_eq!(editor(&h).views.detail, bimcraft_core::graphics::Detail::Coarse);
    let shown = darkest(&around(&h, &pictures, top, 2));
    assert!((200..700).contains(&shown), "Coarse: halftone, {shown}");
}

/// The view control bar's Detail Level menu (the rightmost of the nodes so named: Properties names its row so too) and `level` in it.
fn choose_detail(h: &mut Harness<BimApp>, level: &str) {
    let at = h.get_all_by_label("Detail Level").map(|n| n.rect().center()).max_by(|a, b| a.x.total_cmp(&b.x)).unwrap();
    click_at(h, at);
    h.run();
    let item = h.get_all_by_label(level).map(|n| n.rect().center()).min_by(|a, b| a.distance(at).total_cmp(&b.distance(at))).unwrap();
    click_at(h, item);
    h.run();
}

/// A project of the door in a 6 m wall along x, its leaf hidden in plan and at Coarse.
fn project() -> Harness<'static, BimApp> {
    let script = format!(
        "{}\nwall {{\"points\": [[0, 0], [6000, 0]]}}\ndoor {{\"at\": [3000, 0], \"family\": \"Test Door\", \"type\": \"900 x 2100\"}}",
        door(r#"{"plan": false, "coarse": false}"#)
    );
    app(&script)
}

/// How many primitives the door draws in Level 1's plan as the app's model has it now.
fn door_prims(h: &Harness<BimApp>) -> usize {
    let m = &h.state().session.model;
    let id = m.elements.iter().find(|(_, e)| matches!(e, Element::Door(_))).map(|(id, _)| *id).unwrap();
    let view = bimcraft_core::render::View { scale: 0.5, cx: 3000.0, cy: 300.0, width: 800, height: 800 };
    let d = bimcraft_core::render::draw(m, 100000, view);
    d.owners.iter().filter(|o| **o == Some(id)).count()
}

#[test]
fn the_view_control_bars_detail_level_changes_what_a_placed_door_draws_in_plan_and_in_3d() {
    let mut h = project();
    let fine = door_prims(&h);
    choose_detail(&mut h, "Coarse");
    assert_eq!(plan_graphics(&h, 100000).detail, bimcraft_core::graphics::Detail::Coarse);
    assert!(door_prims(&h) < fine, "Coarse: the swing goes ({} against {fine})", door_prims(&h));
    // The 3D view has a Detail Level of its own now, kept by its view.
    h.state_mut().open_sheet(Sheet::View3D);
    h.run();
    choose_detail(&mut h, "Coarse");
    let m = &h.state().session.model;
    let v = m.default_3d().expect("the 3D view made when its level changed");
    assert_eq!(bimcraft_core::graphics::graphics_of(m, v).map(|g| g.detail), Some(bimcraft_core::graphics::Detail::Coarse));
    let look = bimcraft_core::graphics::Look::of(m, v);
    assert_eq!(look.forms().detail, bimcraft_core::graphics::Detail::Coarse, "what the 3D view draws the leaf by");
}

/// What the dialog and the door family's plan look like, in the three looks, when `BIMCRAFT_F8_DUMP` names a folder.
#[test]
fn visibility_settings_in_three_looks() {
    let Ok(folder) = std::env::var("BIMCRAFT_F8_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (name, dark, light_canvas) in [("light", false, false), ("dark", true, false), ("dark-ui-light-canvas", true, true)] {
        let mut h = app(&door(r#"{"plan": false, "coarse": false}"#));
        h.ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
        h.state_mut().light_canvas = light_canvas;
        h.set_size(vec2(1600., 1000.));
        h.state_mut().edit_family("Test Door");
        h.run();
        view(&mut h, Some(Which::Plan));
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("f8-family-plan-{name}.png"))).unwrap();
        select_form(&mut h, 4);
        press(&mut h, "Visibility Settings");
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("f8-dialog-{name}.png"))).unwrap();
    }
}
