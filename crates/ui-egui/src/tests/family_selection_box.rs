//! Selection Box in the family's {3D}, as the project's, run as Find runs it: with forms or nested instances selected, {3D} is
//! cut to a box round them, its cut faces filled; with nothing selected it takes the box away. The view's own for the session,
//! as its other graphics are: not saved, no step of the history, no unsaved change.
use super::family_selection::command;
use super::preview_visibility::{Pictures, differ, preview};
use super::*;
use crate::editor::select_box::Picked;

/// Preview Visibility's Cabinet, its two forms side by side (x 0 to 1000 and 2000 to 3000), on a screen tall enough for the
/// browser's Forms list.
fn cabinet() -> (Harness<'static, BimApp>, Pictures) {
    let (mut h, pictures) = super::preview_visibility::cabinet();
    h.set_size(vec2(1600.0, 1200.0));
    h.run();
    (h, pictures)
}

/// The section box {3D} keeps for now, if any.
fn boxed(h: &Harness<BimApp>) -> Option<([f64; 3], [f64; 3])> {
    h.state().active_family().and_then(|ed| ed.views.kept.section_box)
}

#[test]
fn selection_box_cuts_3d_to_a_box_round_the_form_selected_and_on_nothing_takes_it_away() {
    let (mut h, pictures) = cabinet();
    let shown = preview(&h, &pictures);
    let ground = shown.pixels[0];
    // With nothing selected, Selection Box has nothing to box: it says so and changes nothing.
    command(&mut h, "Section Box");
    assert!(boxed(&h).is_none());
    assert!(h.state().status.text.contains("Select"), "{:?}", h.state().status);
    // The first form selected: a box round it, a little bigger, which leaves the second form out.
    press(&mut h, "1. Extrusion");
    command(&mut h, "Section Box");
    let (lo, hi) = boxed(&h).expect("a box round the form");
    assert!(lo[0] < 0.0 && hi[0] > 1000.0 && hi[0] < 2000.0, "round the first form, short of the second: {lo:?} {hi:?}");
    assert!(lo[2] < 0.0 && hi[2] > 500.0);
    let cut = preview(&h, &pictures);
    let gone = differ(&shown, &cut);
    assert!(gone.iter().filter(|&&i| cut.pixels[i] == ground).count() > 500, "the second form is gone");
    // Not the family's: no unsaved change, and no step of the history.
    let ed = h.state().active_family().unwrap();
    assert!(!ed.unsaved());
    assert_eq!(ed.steps(), 0);
    // Selection Box on nothing: the box goes, and {3D} is drawn as it was.
    key(&mut h, Key::Escape);
    command(&mut h, "Section Box");
    assert!(boxed(&h).is_none());
    assert_eq!(differ(&preview(&h, &pictures), &shown), Vec::<usize>::new());
}

#[test]
fn the_box_goes_with_the_family() {
    let (mut h, _) = cabinet();
    press(&mut h, "1. Extrusion");
    command(&mut h, "Section Box");
    assert!(boxed(&h).is_some());
    press(&mut h, "Close Family");
    h.state_mut().edit_family("Cabinet");
    h.run();
    assert!(boxed(&h).is_none(), "the view's for the session: opened again, no box");
}

/// A family with a solid a void cuts (x 0 to 1000), a leg nested at x 2500 (200 square), and a solid far off at x 4000 to
/// 5000, open in {3D}.
fn mixed() -> Harness<'static, BimApp> {
    let leg = r#"{"types": {"A": {}}, "forms": [{"profile": [[-100, -100], [100, -100], [100, 100], [-100, 100]], "to": 500}]}"#;
    let family = format!(
        r#"family {{"name": "Mixed", "category": "generic", "types": {{"T": {{}}}}, "nested": [{{"name": "Leg", "family": {leg}}}], "nested_instances": [{{"family": "Leg", "type": "A", "at": [2500, 0]}}], "forms": [{{"profile": [[0, -250], [1000, -250], [1000, 250], [0, 250]], "to": 500}}, {{"void": true, "profile": [[200, -100], [800, -100], [800, 100], [200, 100]], "to": 500}}, {{"profile": [[4000, -250], [5000, -250], [5000, 250], [4000, 250]], "to": 500}}]}}"#
    );
    let mut h = app(&format!("{BOX}\n{family}"));
    h.set_size(vec2(1600.0, 1000.0));
    h.run();
    h.state_mut().edit_family("Mixed");
    h.run();
    h
}

/// Selection Box on `items`: the box it keeps.
fn box_on(h: &mut Harness<BimApp>, items: &[Picked]) -> Option<([f64; 3], [f64; 3])> {
    super::family_drag_move::pick(h, items);
    command(h, "Section Box");
    boxed(h)
}

#[test]
fn selection_box_boxes_a_solid_a_void_cuts_a_nested_instance_and_both() {
    let mut h = mixed();
    let (lo, hi) = box_on(&mut h, &[Picked::Form(0)]).expect("a solid a void cuts has a box");
    assert!(lo[0] < 0.0 && (1000.0..2400.0).contains(&hi[0]), "{lo:?} {hi:?}");
    let (lo, hi) = box_on(&mut h, &[Picked::Nested(0)]).expect("a nested instance has a box");
    assert!((1000.0..2400.0).contains(&lo[0]) && (2600.0..4000.0).contains(&hi[0]), "{lo:?} {hi:?}");
    let (lo, hi) = box_on(&mut h, &[Picked::Form(0), Picked::Nested(0)]).expect("both");
    assert!(lo[0] < 0.0 && (2600.0..4000.0).contains(&hi[0]), "both whole, the far solid out: {lo:?} {hi:?}");
}

#[test]
fn a_selection_box_that_boxes_nothing_keeps_the_box_and_says_why() {
    let mut h = mixed();
    let kept = box_on(&mut h, &[Picked::Form(0)]).expect("a box");
    // A reference plane alone: no form, no nested instance, so no box; the one there was stays, with a note saying so.
    let after = box_on(&mut h, &[Picked::Plane(super::family_drag_move::g(super::family_drag_move::CENTER_LR))]);
    assert_eq!(after, Some(kept));
    let status = h.state().status.clone();
    assert!(status.error && status.text.contains("unchanged"), "{status:?}");
}

/// {3D} before and after Selection Box on the first form, in the light and the dark look, written to the folder
/// `BIMCRAFT_FAMILY_SELECTION_BOX_DUMP` names.
#[test]
fn selection_box_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_FAMILY_SELECTION_BOX_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (name, dark) in [("light", false), ("dark", true)] {
        let (mut h, _) = cabinet();
        h.ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
        let save = |h: &mut Harness<BimApp>, what: &str| {
            for _ in 0..6 {
                h.run();
            }
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("selection-box-{what}-{name}.png"))).unwrap();
        };
        save(&mut h, "before");
        press(&mut h, "1. Extrusion");
        command(&mut h, "Section Box");
        key(&mut h, Key::Escape);
        save(&mut h, "boxed");
    }
}
