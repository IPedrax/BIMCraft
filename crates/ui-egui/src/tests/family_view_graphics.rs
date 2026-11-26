//! View graphics in the family document. Each of its views ({3D}, the two plans and the four elevations) keeps its own
//! Visibility/Graphics, visual style, Graphic Display Options and, in a plan, View Range, with Thin Lines the window's, as the
//! project's tab has it. The commands are run as Find runs them. What they set is the view's for
//! the session: no part of the draft, so neither saved, nor a step of its history, nor an unsaved change.
use super::family_selection::command;
use super::family_views::{around, darkest, editor, open, view};
use super::preview_visibility::{differ, preview};
use super::*;
use bimcraft_core::family::views::{Side, Which};
use bimcraft_core::graphics::VisualStyle;
use egui_kittest::kittest::NodeT;

/// A box from Ref. Level to 2 m, east and behind the origin, crossing the plan's cut plane.
const BOX: &str = r#"family {"name": "Box", "category": "generic", "types": {"T": {}}, "forms": [{"profile": [[500, 300], [1000, 300], [1000, 600], [500, 600]], "to": 2000}]}"#;

/// The box's edge in plan and in the Front elevation.
const IN_PLAN: [f64; 3] = [750.0, 300.0, 0.0];
const IN_FRONT: [f64; 3] = [1000.0, 0.0, 1000.0];

/// The Visibility/Graphics dialog's title for the family's views.
fn vg_open(h: &Harness<BimApp>) -> bool {
    h.query_by_label_contains("Visibility/Graphics Overrides for").is_some()
}

/// Whether the family document is as it was opened: no step of its history, no unsaved change.
fn untouched(h: &Harness<BimApp>) -> bool {
    let ed = editor(h);
    ed.steps() == 0 && !ed.unsaved()
}

#[test]
fn visibility_graphics_hides_the_familys_category_in_the_view_shown_alone_for_the_session() {
    let (mut h, pictures) = open(BOX, "Box");
    view(&mut h, Some(Which::Plan));
    assert!(darkest(&around(&h, &pictures, IN_PLAN, 2)) < 200, "the box, cut");
    command(&mut h, "Visibility/Graphics");
    assert!(vg_open(&h), "Visibility/Graphics opens on the plan");
    // A family has no filters, links, design options or elements of the project's: the two categories' tabs alone.
    assert!(h.query_by_label("Model Categories").is_some() && h.query_by_label("Annotation Categories").is_some());
    for gone in ["Filters", "Design Options", "Imported Categories", "Elements"] {
        assert!(h.query_by_label(gone).is_none(), "no {gone} tab");
    }
    // Cancel leaves it as it was.
    pick(&mut h, Role::CheckBox, "Generic Components");
    press(&mut h, "Cancel");
    assert!(!vg_open(&h));
    assert!(darkest(&around(&h, &pictures, IN_PLAN, 2)) < 200, "Cancel changed nothing");
    // OK hides the category in the plan.
    command(&mut h, "Visibility/Graphics");
    assert!(vg_open(&h), "again");
    pick(&mut h, Role::CheckBox, "Generic Components");
    press(&mut h, "OK");
    assert!(!vg_open(&h));
    assert!(darkest(&around(&h, &pictures, IN_PLAN, 2)) > 740, "the plan hides Generic Components");
    assert!(editor(&h).views.kept.graphics_of(Some(Which::Plan)).categories.get("generic").is_some_and(|o| o.hidden));
    // The plan's alone: the Front elevation still draws it.
    view(&mut h, Some(Which::Elevation(Side::Front)));
    assert!(darkest(&around(&h, &pictures, IN_FRONT, 2)) < 300, "Front keeps its own graphics");
    // The view's for the session: the draft is untouched.
    assert!(untouched(&h), "no step and no unsaved change");
    // Back in the plan it is still hidden; the dialog opened there has the row unticked.
    view(&mut h, Some(Which::Plan));
    assert!(darkest(&around(&h, &pictures, IN_PLAN, 2)) > 740, "kept for the session");
    command(&mut h, "Visibility/Graphics");
    assert_eq!(
        h.get_by_role_and_label(Role::CheckBox, "Generic Components").accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::False)
    );
    pick(&mut h, Role::CheckBox, "Generic Components");
    press(&mut h, "OK");
    assert!(darkest(&around(&h, &pictures, IN_PLAN, 2)) < 200, "shown again");
}

#[test]
fn the_view_tab_has_visibility_graphics_and_thin_lines() {
    let (mut h, _) = open(BOX, "Box");
    view(&mut h, Some(Which::Plan));
    // The family document's View tab (the project's ribbon names a tab View too).
    h.state_mut().active_family_mut().unwrap().doc_tab = crate::editor::DocTab::View;
    h.run();
    assert!(!is_disabled(&h, Role::Button, "Visibility/Graphics"), "the View tab's Visibility/Graphics");
    press(&mut h, "Visibility/Graphics");
    assert!(vg_open(&h));
    press(&mut h, "Cancel");
    assert!(!h.state().thin);
    press(&mut h, "Thin Lines");
    assert!(h.state().thin, "the View tab's Thin Lines");
    assert!(lit(&h, "Thin Lines"));
    press(&mut h, "Thin Lines");
    assert!(!h.state().thin);
}

/// The widths of the lines the last frame painted, points, but the insertion point's: its ring's cross, a mark of the
/// window's that Thin Lines leaves as it is, counted apart ([`origin_strokes`]).
fn painted_widths(h: &Harness<BimApp>) -> Vec<f32> {
    strokes(h).into_iter().filter(|(_, ink)| *ink != crate::editor::origin_mark::INK).map(|(w, _)| w).collect()
}

/// The widths of the lines of the insertion point's ink the last frame painted, points: its cross's two.
fn origin_strokes(h: &Harness<BimApp>) -> Vec<f32> {
    strokes(h).into_iter().filter(|(_, ink)| *ink == crate::editor::origin_mark::INK).map(|(w, _)| w).collect()
}

/// The insertion point's cross's width, points: its two strokes are this, with or without Thin Lines.
const CROSS: f32 = 1.6;

/// The lines the last frame painted over the view: their widths, points, and colours.
fn strokes(h: &Harness<BimApp>) -> Vec<(f32, egui::Color32)> {
    fn walk(s: &egui::Shape, out: &mut Vec<(f32, egui::Color32)>) {
        match s {
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            egui::Shape::LineSegment { stroke, .. } => out.push((stroke.width, stroke.color)),
            egui::Shape::Path(p) => {
                let color = match p.stroke.color {
                    egui::epaint::ColorMode::Solid(c) => c,
                    _ => egui::Color32::TRANSPARENT,
                };
                out.push((p.stroke.width, color));
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    let pane = h.state().view_rect;
    for clipped in h.output().shapes.iter().filter(|c| pane.contains_rect(c.clip_rect) || c.clip_rect == pane) {
        walk(&clipped.shape, &mut out);
    }
    out
}

#[test]
fn thin_lines_draws_the_familys_plans_and_elevations_one_pixel_wide() {
    let (mut h, _) = open(BOX, "Box");
    view(&mut h, Some(Which::Plan));
    // The window's own marks over the view (the view control bar, grips) are 1.5 px at most whatever Thin Lines says; the
    // drawing's cut pen is wider. The insertion point's cross, 1.6 px, is counted apart: there, and left as it is.
    let wide = painted_widths(&h);
    assert_eq!(origin_strokes(&h), [CROSS, CROSS], "the insertion point's cross");
    assert!(wide.iter().any(|w| *w > 1.5), "the cut box's pen is wider than the marks: {wide:?}");
    command(&mut h, "Thin Lines");
    assert!(h.state().thin, "the window's Thin Lines");
    h.run();
    let thin = painted_widths(&h);
    let wider: Vec<f32> = thin.iter().copied().filter(|w| *w > 1.5).collect();
    assert_eq!(origin_strokes(&h), [CROSS, CROSS], "the insertion point's cross, as it was");
    assert!(!thin.is_empty() && wider.is_empty(), "the drawing's lines one pixel: {wider:?}");
    let over = |ws: &[f32]| ws.iter().filter(|w| **w > 1.0).count();
    assert!(over(&thin) < over(&wide), "fewer lines wider than a pixel");
    assert!(untouched(&h));
    // The project's tab shares it: Thin Lines is the window's.
    command(&mut h, "Thin Lines");
    assert!(!h.state().thin);
}

#[test]
fn visual_styles_are_3ds_own_and_said_to_be_so_in_a_plan() {
    let (mut h, pictures) = open(BOX, "Box");
    view(&mut h, None);
    let shaded = preview(&h, &pictures);
    command(&mut h, "Wireframe");
    assert_eq!(editor(&h).views.kept.graphics_of(None).style, VisualStyle::Wireframe);
    let wire = preview(&h, &pictures);
    assert!(differ(&shaded, &wire).len() > 500, "Wireframe draws {{3D}} otherwise");
    command(&mut h, "Hidden Line");
    assert_eq!(editor(&h).views.kept.graphics_of(None).style, VisualStyle::HiddenLine);
    command(&mut h, "Shaded");
    assert_eq!(editor(&h).views.kept.graphics_of(None).style, VisualStyle::Shaded);
    assert_eq!(differ(&shaded, &preview(&h, &pictures)), Vec::<usize>::new(), "Shaded again, as it was");
    // The view control bar's Visual Style menu, as the project's 3D view has it, names the style and sets it.
    press(&mut h, "Visual Style");
    press(&mut h, "Hidden Line");
    assert_eq!(editor(&h).views.kept.graphics_of(None).style, VisualStyle::HiddenLine);
    press(&mut h, "Visual Style");
    press(&mut h, "Graphic Display Options...");
    assert!(h.query_by_label("Show Edges").is_some(), "its Graphic Display Options");
    press(&mut h, "Cancel");
    assert!(untouched(&h));
    // A plan has no visual style to set: the status bar says so, and nothing changes.
    view(&mut h, Some(Which::Plan));
    command(&mut h, "Wireframe");
    assert!(h.state().status.error && h.state().status.text.contains("{3D}"), "{}", h.state().status.text);
    assert_eq!(editor(&h).views.kept.graphics_of(Some(Which::Plan)).style, VisualStyle::default());
    assert!(h.query_by_label("Visual Style").is_none(), "a plan's bar has no Visual Style");
}

#[test]
fn graphic_display_options_set_3ds_display() {
    let (mut h, pictures) = open(BOX, "Box");
    view(&mut h, None);
    let before = preview(&h, &pictures);
    command(&mut h, "Graphic Display Options");
    assert!(h.query_by_label("Graphic Display Options").is_some(), "the dialog opens on {{3D}}");
    pick(&mut h, Role::CheckBox, "Show Edges");
    press(&mut h, "OK");
    assert!(h.query_by_label("Graphic Display Options").is_none());
    assert!(!editor(&h).views.kept.display.edges, "the edges off");
    assert!(!differ(&before, &preview(&h, &pictures)).is_empty(), "{{3D}} drawn without its edges");
    assert!(untouched(&h));
    // A plan has none: the status bar says so.
    view(&mut h, Some(Which::Plan));
    command(&mut h, "Graphic Display Options");
    assert!(h.query_by_label("Graphic Display Options").is_none());
    assert!(h.state().status.error && h.state().status.text.contains("{3D}"), "{}", h.state().status.text);
}

/// The View Range dialog's four fields typed, and OK.
fn view_range(h: &mut Harness<BimApp>, top: &str, cut: &str, bottom: &str, depth: &str) {
    for (label, value) in [("Top", top), ("Cut Plane", cut), ("Bottom", bottom), ("View Depth", depth)] {
        h.get_by_label(label).focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, Key::A);
        h.run();
        h.get_by_label(label).type_text(value);
        h.run();
    }
    press(h, "OK");
}

#[test]
fn view_range_sets_a_plans_range_alone() {
    let (mut h, pictures) = open(BOX, "Box");
    view(&mut h, Some(Which::Plan));
    assert!(darkest(&around(&h, &pictures, IN_PLAN, 2)) < 200, "the box, cut");
    command(&mut h, "View Range");
    assert!(h.query_by_label("View Range").is_some(), "the plan's View Range opens");
    // The whole range above the box's top: the plan sees nothing of it.
    view_range(&mut h, "3500", "3000", "2500", "2500");
    assert!(h.query_by_label("Cut Plane").is_none(), "OK closes it");
    let r = editor(&h).views.kept.range_of(Which::Plan);
    assert_eq!((r.top, r.cut, r.bottom, r.depth), (Some(3500.0), 3000.0, 2500.0, 2500.0));
    assert!(darkest(&around(&h, &pictures, IN_PLAN, 2)) > 740, "the box under the view's depth is gone");
    assert!(untouched(&h));
    // The ceiling plan keeps its own.
    assert_ne!(editor(&h).views.kept.range_of(Which::Ceiling), r);
    // A letter typed for a length is refused, and the dialog stays.
    command(&mut h, "View Range");
    view_range(&mut h, "", "high", "0", "0");
    assert!(h.query_by_label("Cut Plane").is_some(), "refused: still open");
    // Out of order (the cut plane over the top), refused with the project's reason.
    view_range(&mut h, "1000", "2000", "0", "0");
    assert!(h.query_by_label("Cut Plane").is_some(), "refused: still open");
    assert!(h.query_by_label_contains("the cut plane can't be higher than the top").is_some(), "says why");
    press(&mut h, "Cancel");
    assert_eq!(editor(&h).views.kept.range_of(Which::Plan), r, "Cancel keeps the range");
    // An elevation has no view range: the status bar says so.
    view(&mut h, Some(Which::Elevation(Side::Front)));
    command(&mut h, "View Range");
    assert!(h.query_by_label("Cut Plane").is_none());
    assert!(h.state().status.error && h.state().status.text.contains("plan"), "{}", h.state().status.text);
}

/// The family's Visibility/Graphics on its plan, {3D} in Wireframe, the plan's View Range and the plan in Thin Lines, in the
/// three looks, when `BIMCRAFT_FAMILY_VIEW_GRAPHICS_DUMP` names a folder.
#[test]
fn family_view_graphics_in_three_looks() {
    let Ok(folder) = std::env::var("BIMCRAFT_FAMILY_VIEW_GRAPHICS_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (name, dark, light_canvas, own) in [
        ("light", false, false, false),
        ("dark", true, false, false),
        ("dark-ui-light-canvas", true, true, false),
        ("bimcraft-profile", false, false, true),
    ] {
        let mut h = if own { bimcraft(BOX) } else { app(BOX) };
        h.set_size(egui::vec2(1600.0, 1000.0));
        h.state_mut().edit_family("Box");
        h.run();
        h.ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
        h.state_mut().light_canvas = light_canvas;
        let save = |h: &mut Harness<BimApp>, what: &str| {
            for _ in 0..4 {
                h.run();
            }
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("view-graphics-{what}-{name}.png"))).unwrap();
        };
        view(&mut h, Some(Which::Plan));
        h.state_mut().active_family_mut().unwrap().doc_tab = crate::editor::DocTab::View;
        command(&mut h, "Visibility/Graphics");
        save(&mut h, "vg");
        press(&mut h, "Cancel");
        command(&mut h, "View Range");
        save(&mut h, "view-range");
        press(&mut h, "Cancel");
        command(&mut h, "Thin Lines");
        save(&mut h, "thin-lines");
        command(&mut h, "Thin Lines");
        // The plan with Generic Components hidden, then shown again with a View Range above the box.
        command(&mut h, "Visibility/Graphics");
        pick(&mut h, Role::CheckBox, "Generic Components");
        press(&mut h, "OK");
        save(&mut h, "plan-vv-hidden");
        command(&mut h, "Visibility/Graphics");
        pick(&mut h, Role::CheckBox, "Generic Components");
        press(&mut h, "OK");
        command(&mut h, "View Range");
        view_range(&mut h, "3500", "3000", "2500", "2500");
        save(&mut h, "plan-vr-above");
        view(&mut h, None);
        command(&mut h, "Wireframe");
        save(&mut h, "3d-wireframe");
        command(&mut h, "Graphic Display Options");
        save(&mut h, "gd");
        press(&mut h, "Cancel");
    }
}

/// How many forms the draft has.
fn forms(h: &Harness<BimApp>) -> usize {
    editor(h).draft_family().map(|f| f.forms.len()).unwrap_or(0)
}

#[test]
fn keys_are_the_dialogs_not_the_familys_while_one_is_open() {
    let (mut h, _) = open(BOX, "Box");
    view(&mut h, Some(Which::Plan));
    // Delete under Visibility/Graphics deletes nothing.
    super::family_views::select_form(&mut h, 1);
    command(&mut h, "Visibility/Graphics");
    assert!(vg_open(&h));
    key(&mut h, Key::Delete);
    assert_eq!(forms(&h), 1, "the form stays under the dialog");
    press(&mut h, "Cancel");
    // Nor under View Range, nor under Graphic Display Options.
    super::family_views::select_form(&mut h, 1);
    command(&mut h, "View Range");
    key(&mut h, Key::Delete);
    assert_eq!(forms(&h), 1, "the form stays under View Range");
    press(&mut h, "Cancel");
    view(&mut h, None);
    command(&mut h, "Graphic Display Options");
    key(&mut h, Key::Delete);
    assert_eq!(forms(&h), 1, "the form stays under Graphic Display Options");
    press(&mut h, "Cancel");
    assert!(untouched(&h));
}

/// A footing under Ref. Level, from 800 below it to 200 below, and the box over it.
const FOOTING: &str = r#"family {"name": "Footing", "category": "generic", "types": {"T": {}}, "forms": [{"profile": [[500, 300], [1000, 300], [1000, 600], [500, 600]], "from": -800, "to": -200}]}"#;

#[test]
fn view_range_ok_with_nothing_typed_leaves_the_plan_as_it_was() {
    let (mut h, pictures) = open(FOOTING, "Footing");
    view(&mut h, Some(Which::Plan));
    assert!(darkest(&around(&h, &pictures, IN_PLAN, 2)) < 300, "the footing under Ref. Level, seen");
    command(&mut h, "View Range");
    assert_eq!(h.get_by_label("Top").accesskit_node().value().unwrap_or_default(), "", "Top unlimited");
    press(&mut h, "OK");
    assert_eq!(editor(&h).views.kept.seen(Which::Plan), None, "nothing set");
    assert!(darkest(&around(&h, &pictures, IN_PLAN, 2)) < 300, "still seen");
}

#[test]
fn view_range_is_the_floor_plans() {
    let (mut h, _) = open(BOX, "Box");
    view(&mut h, Some(Which::Ceiling));
    command(&mut h, "View Range");
    assert!(h.query_by_label("Cut Plane").is_none(), "no View Range on the ceiling plan");
    assert!(h.state().status.error && h.state().status.text.contains("floor plan"), "{}", h.state().status.text);
}

#[test]
fn object_styles_from_the_familys_visibility_graphics_is_the_familys() {
    let (mut h, _) = open(BOX, "Box");
    view(&mut h, Some(Which::Plan));
    command(&mut h, "Visibility/Graphics");
    press(&mut h, "Object Styles…");
    assert!(h.query_by_label_contains("This changes the project's object styles now").is_some(), "the Manage note: it changes the project");
}

#[test]
fn the_family_views_graphics_never_touch_the_project() {
    let (mut h, _) = open(BOX, "Box");
    let model = serde_json::to_string(&*h.state().session.model).unwrap();
    let dirty = h.state().dirty();
    view(&mut h, Some(Which::Plan));
    command(&mut h, "Visibility/Graphics");
    pick(&mut h, Role::CheckBox, "Generic Components");
    press(&mut h, "OK");
    command(&mut h, "View Range");
    view_range(&mut h, "3500", "3000", "2500", "2500");
    view(&mut h, None);
    command(&mut h, "Graphic Display Options");
    pick(&mut h, Role::CheckBox, "Show Ambient Shadows");
    press(&mut h, "OK");
    command(&mut h, "Hidden Line");
    assert_eq!(serde_json::to_string(&*h.state().session.model).unwrap(), model, "the project as it was");
    assert_eq!(h.state().dirty(), dirty);
}

#[test]
fn hidden_line_draws_3d_otherwise_than_shaded_and_wireframe() {
    let (mut h, pictures) = open(BOX, "Box");
    view(&mut h, None);
    let shaded = preview(&h, &pictures);
    command(&mut h, "Wireframe");
    let wire = preview(&h, &pictures);
    command(&mut h, "Hidden Line");
    let hidden = preview(&h, &pictures);
    assert!(differ(&shaded, &hidden).len() > 300, "Hidden Line isn't Shaded");
    assert!(differ(&wire, &hidden).len() > 50, "nor Wireframe: its far edges hidden");
}

#[test]
fn thin_lines_draws_an_elevation_one_pixel_wide_too() {
    let (mut h, _) = open(BOX, "Box");
    view(&mut h, Some(Which::Elevation(Side::Front)));
    h.run();
    let wide = painted_widths(&h);
    assert!(wide.iter().any(|w| *w > 1.5), "the elevation's pens: {wide:?}");
    command(&mut h, "Thin Lines");
    h.run();
    let thin = painted_widths(&h);
    assert!(thin.iter().all(|w| *w <= 1.5), "{thin:?}");
}
