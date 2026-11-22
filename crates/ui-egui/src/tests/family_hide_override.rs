//! Hide in View, Reveal Hidden Elements, Unhide and Override Graphics by Element in the family document, run as Find runs
//! them, each view's own for the session, as Visibility/Graphics is. A form hidden by element, or with its category, is left
//! out of that view alone; Reveal Hidden Elements draws what the view hides in magenta inside the project's
//! frame, where it can be picked and unhidden; an override draws a form otherwise in that view alone. None of it is saved, a
//! step of the history, or an unsaved change.
use super::family_selection::command;
use super::family_views::{around, click_family, darkest, editor, open, select_form, view};
use super::*;
use bimcraft_core::family::views::{Side, Which};
use egui::Color32;
use egui_kittest::kittest::NodeT;

/// Two boxes from Ref. Level to 2 m, A west of B, and a plane of the family's own, Edge, west of both.
const BOXES: &str = r#"family {"name": "Boxes", "category": "generic", "types": {"T": {}}, "forms": [{"profile": [[500, 300], [1000, 300], [1000, 600], [500, 600]], "to": 2000}, {"profile": [[1500, 300], [2000, 300], [2000, 600], [1500, 600]], "to": 2000}], "ref_planes": [{"name": "Edge", "start": [-400, -2000, 0], "end": [-400, 2000, 0], "up": [0, 0, 1]}]}"#;

/// Box A's and box B's front edges in plan, and box A's face in the Front elevation; Edge's trace in plan.
const A: [f64; 3] = [750.0, 300.0, 0.0];
const B: [f64; 3] = [1750.0, 300.0, 0.0];
const A_FRONT: [f64; 3] = [1000.0, 0.0, 1000.0];
const EDGE: [f64; 3] = [-400.0, 800.0, 0.0];

/// The family drafted and the project, written out, the family document's steps of history and whether it has an unsaved change.
fn snapshot(h: &Harness<BimApp>) -> (String, String, usize, bool) {
    let ed = editor(h);
    let family = serde_json::to_string(&ed.draft_family().ok()).unwrap_or_default();
    (family, serde_json::to_string(&*h.state().session.model).unwrap_or_default(), ed.steps(), ed.unsaved())
}

/// Whether the family document and the project are as `before` has them: the family and the project unchanged, no step of
/// history and no unsaved change more.
fn untouched(h: &Harness<BimApp>, before: &(String, String, usize, bool)) -> bool {
    let now = snapshot(h);
    assert_eq!(now.2, before.2, "steps");
    assert_eq!(now.3, before.3, "unsaved");
    now == *before
}

/// Magenta, Reveal Hidden Elements' colour.
/// Magenta, however light a thin pen draws it: red and blue well over green.
fn magenta(c: &Color32) -> bool {
    c.r() > 180 && c.b() > 180 && u16::from(c.g()) + 60 < u16::from(c.r().min(c.b()))
}

fn green(c: &Color32) -> bool {
    i16::from(c.g()) - i16::from(c.r()) > 12 && i16::from(c.g()) - i16::from(c.b()) > 8
}

/// Whether Edge's dashed green trace shows near `p`.
fn edge_shown(h: &Harness<BimApp>, pictures: &super::preview_visibility::Pictures) -> bool {
    (0..12).any(|k| around(h, pictures, [EDGE[0], EDGE[1] + f64::from(k) * 25.0, 0.0], 2).iter().any(green))
}

#[test]
fn hide_element_hides_a_form_in_the_view_alone_and_reveal_hidden_unhides_it() {
    let (mut h, pictures) = open(BOXES, "Boxes");
    let before = snapshot(&h);
    view(&mut h, Some(Which::Plan));
    select_form(&mut h, 1);
    command(&mut h, "Hide Element");
    let guid = editor(&h).form_guid(0).unwrap();
    assert!(editor(&h).views.kept.graphics_of(Some(Which::Plan)).elements.get(&guid).is_some_and(|o| o.hidden));
    assert!(darkest(&around(&h, &pictures, A, 2)) > 740, "box A hidden in the plan");
    assert!(darkest(&around(&h, &pictures, B, 2)) < 200, "box B stays");
    assert_eq!(editor(&h).selected_form(), None, "the selection goes, as the project's Hide in View lets it go");
    // A click where it was picks nothing: hidden, it can't be selected.
    click_family(&mut h, A);
    assert_eq!(editor(&h).selected_form(), None);
    // The plan's alone.
    view(&mut h, Some(Which::Elevation(Side::Front)));
    assert!(darkest(&around(&h, &pictures, A_FRONT, 2)) < 300, "Front still draws box A");
    view(&mut h, Some(Which::Plan));
    assert!(untouched(&h, &before));
    // Reveal Hidden Elements: box A in magenta, picked by a click and unhidden by Unhide Elements.
    command(&mut h, "Reveal Hidden Elements");
    assert!(editor(&h).views.kept.revealing(Some(Which::Plan)));
    assert!(around(&h, &pictures, A, 2).iter().any(magenta), "box A revealed in magenta");
    assert!(!around(&h, &pictures, B, 2).iter().any(magenta), "box B as it is");
    click_family(&mut h, A);
    assert_eq!(editor(&h).selected_form(), Some(0), "revealed, it can be picked");
    command(&mut h, "Unhide Elements");
    assert!(!editor(&h).views.kept.graphics_of(Some(Which::Plan)).elements.contains_key(&guid), "unhidden");
    command(&mut h, "Reveal Hidden Elements");
    assert!(!editor(&h).views.kept.revealing(Some(Which::Plan)));
    assert!(darkest(&around(&h, &pictures, A, 2)) < 200, "box A drawn again");
    assert!(untouched(&h, &before));
}

#[test]
fn hide_category_hides_every_form_and_unhide_category_brings_them_back() {
    let (mut h, pictures) = open(BOXES, "Boxes");
    let before = snapshot(&h);
    view(&mut h, Some(Which::Plan));
    select_form(&mut h, 2);
    command(&mut h, "Hide Category");
    assert!(darkest(&around(&h, &pictures, A, 2)) > 740 && darkest(&around(&h, &pictures, B, 2)) > 740, "both boxes hidden");
    // It is Visibility/Graphics' row: unticked there.
    command(&mut h, "Visibility/Graphics");
    assert_eq!(
        h.get_by_role_and_label(Role::CheckBox, "Generic Components").accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::False)
    );
    press(&mut h, "Cancel");
    // Revealed, both in magenta; one picked and Unhide Category unhides it.
    command(&mut h, "Reveal Hidden Elements");
    assert!(around(&h, &pictures, A, 2).iter().any(magenta) && around(&h, &pictures, B, 2).iter().any(magenta));
    click_family(&mut h, B);
    command(&mut h, "Unhide Category");
    command(&mut h, "Reveal Hidden Elements");
    assert!(darkest(&around(&h, &pictures, A, 2)) < 200 && darkest(&around(&h, &pictures, B, 2)) < 200, "both back");
    assert!(untouched(&h, &before));
}

#[test]
fn hide_element_hides_a_reference_plane_in_the_view() {
    let (mut h, pictures) = open(BOXES, "Boxes");
    let before = snapshot(&h);
    view(&mut h, Some(Which::Plan));
    assert!(edge_shown(&h, &pictures));
    click_family(&mut h, [-400.0, -1000.0, 0.0]);
    assert!(editor(&h).selected_plane().is_some(), "Edge");
    command(&mut h, "Hide Element");
    assert!(!edge_shown(&h, &pictures), "Edge hidden in the plan");
    click_family(&mut h, [-400.0, -1000.0, 0.0]);
    assert!(editor(&h).selected_plane().is_none(), "hidden, it can't be picked");
    // Revealed, it is picked and unhidden.
    command(&mut h, "Reveal Hidden Elements");
    let revealed = (0..12).any(|k| around(&h, &pictures, [EDGE[0], EDGE[1] + f64::from(k) * 25.0, 0.0], 2).iter().any(magenta));
    assert!(revealed, "revealed in magenta, as a form is");
    click_family(&mut h, [-400.0, -1000.0, 0.0]);
    command(&mut h, "Unhide Elements");
    command(&mut h, "Reveal Hidden Elements");
    assert!(edge_shown(&h, &pictures), "back");
    // Hide Category on a plane alone says why it hides nothing.
    click_family(&mut h, [-400.0, -1000.0, 0.0]);
    command(&mut h, "Hide Category");
    assert!(h.state().status.error, "{}", h.state().status.text);
    assert!(edge_shown(&h, &pictures));
    assert!(untouched(&h, &before));
}

#[test]
fn override_graphics_draws_a_form_otherwise_in_the_view_alone() {
    let (mut h, pictures) = open(BOXES, "Boxes");
    let before = snapshot(&h);
    view(&mut h, Some(Which::Plan));
    let ink = darkest(&around(&h, &pictures, A, 2));
    select_form(&mut h, 1);
    command(&mut h, "Override Graphics by Element");
    assert!(h.query_by_label("View-Specific Element Graphics").is_some(), "Override Graphics opens the dialog");
    // Cancel changes nothing.
    pick(&mut h, Role::CheckBox, "Halftone");
    press(&mut h, "Cancel");
    assert!(editor(&h).views.kept.graphics_of(Some(Which::Plan)).elements.is_empty());
    select_form(&mut h, 1);
    command(&mut h, "Override Graphics by Element");
    pick(&mut h, Role::CheckBox, "Halftone");
    press(&mut h, "OK");
    let guid = editor(&h).form_guid(0).unwrap();
    assert!(editor(&h).views.kept.graphics_of(Some(Which::Plan)).elements.get(&guid).is_some_and(|o| o.halftone && !o.hidden));
    let half = darkest(&around(&h, &pictures, A, 2));
    assert!(half > ink + 100 && half < 740, "box A halftone: {half} against {ink}");
    assert!(darkest(&around(&h, &pictures, B, 2)) < 200, "box B as it was");
    // The plan's alone.
    view(&mut h, Some(Which::Elevation(Side::Front)));
    assert!(darkest(&around(&h, &pictures, A_FRONT, 2)) < 300, "Front draws box A as it was");
    assert!(untouched(&h, &before));
    // Nothing selected: Override Graphics says so.
    view(&mut h, Some(Which::Plan));
    key(&mut h, Key::Escape);
    command(&mut h, "Override Graphics by Element");
    assert!(h.query_by_label("View-Specific Element Graphics").is_none());
    assert!(h.state().status.error, "{}", h.state().status.text);
}

#[test]
fn apply_hide_isolate_to_view_keeps_what_is_hidden_for_now_for_good() {
    let (mut h, pictures) = open(BOXES, "Boxes");
    let before = snapshot(&h);
    view(&mut h, Some(Which::Plan));
    select_form(&mut h, 1);
    shortcut(&mut h, "HH");
    assert!(editor(&h).temporary().hides());
    press(&mut h, "Temporary Hide/Isolate");
    press(&mut h, "Apply Hide/Isolate to View");
    assert!(!editor(&h).temporary().hides(), "nothing hidden for now any more");
    let guid = editor(&h).form_guid(0).unwrap();
    assert!(editor(&h).views.kept.graphics_of(Some(Which::Plan)).elements.get(&guid).is_some_and(|o| o.hidden), "hidden for good");
    assert!(darkest(&around(&h, &pictures, A, 2)) > 740);
    assert!(untouched(&h, &before));
}

#[test]
fn the_view_control_bar_has_reveal_hidden_elements() {
    let (mut h, _) = open(BOXES, "Boxes");
    view(&mut h, Some(Which::Plan));
    press(&mut h, "Reveal Hidden Elements");
    assert!(editor(&h).views.kept.revealing(Some(Which::Plan)));
    assert!(lit(&h, "Reveal Hidden Elements"));
    press(&mut h, "Reveal Hidden Elements");
    assert!(!editor(&h).views.kept.revealing(Some(Which::Plan)));
}

#[test]
fn hide_and_override_work_in_3d_too() {
    use super::preview_visibility::{differ, preview};
    let (mut h, pictures) = open(BOXES, "Boxes");
    let before = snapshot(&h);
    view(&mut h, None);
    let shown = preview(&h, &pictures);
    select_form(&mut h, 1);
    command(&mut h, "Override Graphics by Element");
    pick(&mut h, Role::CheckBox, "Halftone");
    press(&mut h, "OK");
    let half = preview(&h, &pictures);
    assert!(differ(&shown, &half).len() > 300, "box A halftone in {{3D}}");
    assert!(editor(&h).views.kept.graphics_of(Some(Which::Plan)).elements.is_empty(), "{{3D}}'s alone");
    select_form(&mut h, 1);
    command(&mut h, "Hide Element");
    let gone = preview(&h, &pictures);
    // Box A stands partly before box B: where it was, the ground or box B.
    let ground = gone.pixels[0];
    let drawn = |p: &egui::ColorImage| p.pixels.iter().filter(|c| **c != ground).count();
    assert!(drawn(&gone) + 300 < drawn(&half), "box A gone: {} drawn against {}", drawn(&gone), drawn(&half));
    command(&mut h, "Reveal Hidden Elements");
    let revealed = preview(&h, &pictures);
    assert!(!half.pixels.iter().any(magenta), "nothing magenta before");
    let m = revealed.pixels.iter().filter(|c| magenta(c)).count();
    assert!(m > 300, "box A in magenta: {m} pixels");
    assert!(untouched(&h, &before));
}

/// Box A hidden and revealed in the plan, and View-Specific Element Graphics, in the three looks, when
/// `BIMCRAFT_FAMILY_HIDE_IN_VIEW_DUMP` names a folder.
#[test]
fn family_hide_and_override_in_three_looks() {
    let Ok(folder) = std::env::var("BIMCRAFT_FAMILY_HIDE_IN_VIEW_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (name, dark, light_canvas) in [("light", false, false), ("dark", true, false), ("dark-ui-light-canvas", true, true)] {
        let mut h = app(BOXES);
        h.set_size(egui::vec2(1600.0, 1000.0));
        h.state_mut().edit_family("Boxes");
        h.run();
        h.ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
        h.state_mut().light_canvas = light_canvas;
        let save = |h: &mut Harness<BimApp>, what: &str| {
            for _ in 0..4 {
                h.run();
            }
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("hide-in-view-{what}-{name}.png"))).unwrap();
        };
        view(&mut h, Some(Which::Plan));
        select_form(&mut h, 1);
        command(&mut h, "Hide Element");
        command(&mut h, "Reveal Hidden Elements");
        save(&mut h, "revealed");
        command(&mut h, "Reveal Hidden Elements");
        select_form(&mut h, 2);
        command(&mut h, "Override Graphics by Element");
        save(&mut h, "override");
        press(&mut h, "Cancel");
    }
}

#[test]
fn keys_are_the_override_dialogs_while_it_is_open() {
    let (mut h, _) = open(BOXES, "Boxes");
    let before = snapshot(&h);
    view(&mut h, Some(Which::Plan));
    select_form(&mut h, 1);
    command(&mut h, "Override Graphics by Element");
    assert!(h.query_by_label("View-Specific Element Graphics").is_some());
    key(&mut h, Key::Delete);
    shortcut(&mut h, "HH");
    h.key_combination_modifiers(Modifiers::COMMAND, &[Key::Z]);
    h.run();
    assert_eq!(editor(&h).draft_family().map(|f| f.forms.len()).unwrap_or(0), 2, "Delete under the dialog deletes nothing");
    assert!(!editor(&h).temporary().hides(), "nor does HH hide");
    assert!(editor(&h).views.kept.graphics_of(Some(Which::Plan)).elements.is_empty());
    press(&mut h, "Cancel");
    assert!(untouched(&h, &before));
}

/// Two overlapping boxes, A [0,1000]x[0,600] and B [800,1800]x[0,600], from Ref. Level to 2 m, and a void [600,1200]x[200,400]
/// through both.
const VOIDED: &str = r#"family {"name": "Voided", "category": "generic", "types": {"T": {}}, "forms": [{"profile": [[0, 0], [1000, 0], [1000, 600], [0, 600]], "to": 2000}, {"profile": [[800, 0], [1800, 0], [1800, 600], [800, 600]], "to": 2000}, {"profile": [[600, 200], [1200, 200], [1200, 400], [600, 400]], "from": -10, "to": 2010, "void": true}]}"#;

/// Red, cut and seen.
fn red() -> bimcraft_core::graphics::Override {
    let s = bimcraft_core::graphics::Style { color: Some([230, 0, 0]), ..Default::default() };
    bimcraft_core::graphics::Override { projection: s.clone(), cut: s, ..Default::default() }
}

/// View `which`'s override of form `i` set to `o`, as Override Graphics' OK sets it.
fn set_override(h: &mut Harness<BimApp>, which: Option<Which>, i: usize, o: bimcraft_core::graphics::Override) {
    let ed = h.state_mut().active_family_mut().unwrap();
    let g = ed.form_guid(i).unwrap();
    let mut graphics = ed.views.kept.graphics_of(which);
    graphics.elements.insert(g, o);
    ed.views.kept.set_graphics(which, graphics);
    h.run();
    h.run();
}

/// A click at `p` with Ctrl held: what is there added to the selection.
fn ctrl_click_family(h: &mut Harness<BimApp>, p: [f64; 3]) {
    h.event(Event::ModifiersChanged(Modifiers::COMMAND));
    super::family_box_select::click_with_mods(h, p, Modifiers::COMMAND);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run();
}

#[test]
fn a_void_still_cuts_a_form_drawn_apart_in_plan_and_elevation() {
    let (mut h, pictures) = open(VOIDED, "Voided");
    // The hole's west and south edges in plan, inside A alone, and its west edge in the Front elevation, cut through A.
    let (west, south) = ([600.0, 300.0, 0.0], [700.0, 200.0, 0.0]);
    view(&mut h, Some(Which::Plan));
    assert!(darkest(&around(&h, &pictures, west, 2)) < 600 && darkest(&around(&h, &pictures, south, 2)) < 600, "the hole");
    set_override(&mut h, Some(Which::Plan), 0, red());
    assert!(darkest(&around(&h, &pictures, west, 2)) < 600, "the void still cuts A drawn apart, west");
    assert!(darkest(&around(&h, &pictures, south, 2)) < 600, "the void still cuts A drawn apart, south");
    // Revealed, A hidden: drawn apart in magenta, the hole still in it.
    let ed = h.state_mut().active_family_mut().unwrap();
    let g = ed.form_guid(0).unwrap();
    ed.views.kept.set_graphics(Some(Which::Plan), Default::default());
    ed.views.kept.hide(Some(Which::Plan), &[g]);
    command(&mut h, "Reveal Hidden Elements");
    assert!(around(&h, &pictures, south, 2).iter().any(magenta), "the hole's edge in A, revealed");
    // The Front elevation: the void's edges cross A's face, drawn apart.
    view(&mut h, Some(Which::Elevation(Side::Front)));
    let edge = [600.0, 0.0, 1000.0];
    let base = darkest(&around(&h, &pictures, edge, 2));
    set_override(&mut h, Some(Which::Elevation(Side::Front)), 0, red());
    let over = darkest(&around(&h, &pictures, edge, 2));
    assert!((base < 740) == (over < 740), "the elevation draws the void's edge as it did: {base} then {over}");
}

#[test]
fn override_graphics_on_several_forms_changes_only_what_was_edited() {
    let (mut h, _) = open(BOXES, "Boxes");
    view(&mut h, Some(Which::Plan));
    let (ga, gb) = (editor(&h).form_guid(0).unwrap(), editor(&h).form_guid(1).unwrap());
    select_form(&mut h, 1);
    command(&mut h, "Override Graphics by Element");
    pick(&mut h, Role::CheckBox, "Halftone");
    press(&mut h, "OK");
    let elements = |h: &Harness<BimApp>| editor(h).views.kept.graphics_of(Some(Which::Plan)).elements;
    // B, then A: the dialog opens on B's (none), and an OK unedited leaves A halftone.
    click_family(&mut h, B);
    ctrl_click_family(&mut h, A);
    command(&mut h, "Override Graphics by Element");
    assert!(h.query_by_label("View-Specific Element Graphics").is_some());
    press(&mut h, "OK");
    assert!(elements(&h).get(&ga).is_some_and(|o| o.halftone), "A keeps its halftone");
    assert!(!elements(&h).contains_key(&gb), "B has nothing");
    // A, then B: opens on A's halftone, and an OK unedited gives B nothing.
    click_family(&mut h, A);
    ctrl_click_family(&mut h, B);
    command(&mut h, "Override Graphics by Element");
    press(&mut h, "OK");
    assert!(!elements(&h).contains_key(&gb), "B got A's halftone");
    // B red; A and B, Halftone ticked off: both lose it, B stays red.
    set_override(&mut h, Some(Which::Plan), 1, red());
    click_family(&mut h, A);
    ctrl_click_family(&mut h, B);
    command(&mut h, "Override Graphics by Element");
    pick(&mut h, Role::CheckBox, "Halftone");
    press(&mut h, "OK");
    assert!(!elements(&h).contains_key(&ga), "A's halftone off: nothing left");
    assert_eq!(elements(&h).get(&gb), Some(&red()), "B keeps its red, the field nobody edited");
}

#[test]
fn hide_category_hides_a_door_form_with_an_override_too() {
    let door = super::family_visibility::door("");
    let (mut h, pictures) = open(&door, "Test Door");
    view(&mut h, Some(Which::Plan));
    // The leaf's back edge, clear of the centre planes.
    let leaf = [200.0, 20.0, 0.0];
    assert!(darkest(&around(&h, &pictures, leaf, 1)) < 600, "the leaf");
    select_form(&mut h, 4);
    command(&mut h, "Override Graphics by Element");
    pick(&mut h, Role::CheckBox, "Halftone");
    press(&mut h, "OK");
    select_form(&mut h, 1);
    command(&mut h, "Hide Category");
    assert!(darkest(&around(&h, &pictures, leaf, 1)) > 740, "the leaf goes with its category, its override and all");
    command(&mut h, "Reveal Hidden Elements");
    assert!(around(&h, &pictures, leaf, 1).iter().any(magenta), "revealed in magenta");
}

#[test]
fn dimension_and_align_pick_no_hidden_form() {
    let (mut h, _) = open(BOXES, "Boxes");
    view(&mut h, Some(Which::Plan));
    select_form(&mut h, 1);
    command(&mut h, "Hide Element");
    // Box A's west edge.
    let edge = [500.0, 450.0, 0.0];
    h.state_mut().active_family_mut().unwrap().views.dimensioning = Some(Vec::new());
    h.run();
    click_family(&mut h, edge);
    assert_eq!(editor(&h).views.dimensioning, Some(Vec::new()), "a dimension picked the hidden form's edge");
    let ed = h.state_mut().active_family_mut().unwrap();
    ed.views.dimensioning = None;
    ed.views.aligning = Some(None);
    h.run();
    click_family(&mut h, edge);
    assert!(matches!(editor(&h).views.aligning, Some(None)), "Align picked the hidden form's edge");
    // Hidden for now (HH), the same.
    let ed = h.state_mut().active_family_mut().unwrap();
    ed.views.aligning = None;
    ed.views.kept.set_graphics(Some(Which::Plan), Default::default());
    h.run();
    select_form(&mut h, 1);
    shortcut(&mut h, "HH");
    h.state_mut().active_family_mut().unwrap().views.dimensioning = Some(Vec::new());
    h.run();
    click_family(&mut h, edge);
    assert_eq!(editor(&h).views.dimensioning, Some(Vec::new()), "a dimension picked the form hidden for now");
}

#[test]
fn apply_hide_isolate_to_view_is_off_in_an_adaptive_family() {
    let (mut h, _points, form) = super::family_temporary_hide::adaptive();
    if let Some(ed) = h.state_mut().active_family_mut() {
        ed.modeller.selection = vec![form];
    }
    h.run();
    shortcut(&mut h, "HC");
    press(&mut h, "Temporary Hide/Isolate");
    let apply = h.query_by_label("Apply Hide/Isolate to View").map(|n| n.accesskit_node().is_disabled());
    assert_eq!(apply, Some(true), "an adaptive family's views hide for now only");
}
