//! T28: Temporary Hide/Isolate in the family document's view, as the project's view has it: Hide Element (HH), Hide Category
//! (HC), Isolate Element (HI), Isolate Category (IC) and Reset (HR), in the 3D preview of a family that isn't adaptive and in an
//! adaptive family's modelling view. The view's own for now: not saved, no step of the history, no unsaved change, gone when the
//! family closes; the view wears the project's cyan frame while it hides anything, beside Preview Visibility's orange one.
use super::preview_visibility::{Pictures, differ, outlines, preview, toggle};
use super::*;
use bimcraft_core::adaptive::{Definition, WorkPlane};
use bimcraft_core::family::{Category, Planes};
use egui::{Color32, Shape};

/// The project's Temporary Hide/Isolate frame.
const CYAN: Color32 = Color32::from_rgb(0, 190, 220);

/// Whether a closed outline of `color` goes round the view.
fn framed_in(h: &Harness<BimApp>, color: Color32) -> bool {
    let pane = h.state().view_rect;
    outlines(h).iter().any(|(r, c)| *c == color && r.width() > pane.width() * 0.9 && r.height() > pane.height() * 0.9)
}

/// Preview Visibility's Cabinet, its two forms both shown in type A, on a screen tall enough for the browser's Forms list.
fn cabinet() -> (Harness<'static, BimApp>, Pictures) {
    let (mut h, pictures) = super::preview_visibility::cabinet();
    h.set_size(vec2(1600.0, 1200.0));
    h.run();
    (h, pictures)
}

/// Whether the family document's view hides anything for now.
fn hiding(h: &Harness<BimApp>) -> bool {
    h.state().family_editor.as_ref().is_some_and(|ed| ed.temporary().hides())
}

/// Form `n` (from 1) of a family that isn't adaptive selected from the Project Browser's Forms list, as a user picks it; the
/// Properties' Symbols tab in front, so the Forms tab's own rows aren't there too.
fn select_form(h: &mut Harness<BimApp>, n: usize) {
    if h.query_by_label("Symbols").is_some() {
        press(h, "Symbols");
    }
    press(h, &format!("{n}. Extrusion"));
}

/// Every pixel of `p` is the preview's ground (its first pixel, a corner nothing stands in).
fn bare(p: &egui::ColorImage, ground: Color32) -> bool {
    p.pixels.iter().all(|c| *c == ground)
}

#[test]
fn hide_and_isolate_element_and_category_change_what_the_preview_draws_and_reset_restores_it() {
    let (mut h, pictures) = cabinet();
    let shown = preview(&h, &pictures);
    let ground = shown.pixels[0];
    assert!(!hiding(&h) && !bare(&shown, ground));
    // HH on the first form: it goes, and the second stays.
    select_form(&mut h, 1);
    shortcut(&mut h, "HH");
    assert!(hiding(&h));
    let first_hidden = preview(&h, &pictures);
    let first = differ(&shown, &first_hidden);
    assert!(first.len() > 500, "the first form is gone: {} pixels", first.len());
    assert!(first.iter().all(|&i| first_hidden.pixels[i] == ground), "where it was is the ground");
    assert!(!bare(&first_hidden, ground), "the second form stays");
    // The form selected was let go; it is still in the Forms list.
    assert!(h.state().family_editor.as_ref().unwrap().selected_form().is_none());
    assert!(h.query_all_by_label("1. Extrusion").count() > 0);
    // HR: as it was.
    shortcut(&mut h, "HR");
    assert!(!hiding(&h));
    assert_eq!(differ(&preview(&h, &pictures), &shown), Vec::<usize>::new(), "reset draws it all again");
    // HI on the second form: it alone, so what HH of the first drew.
    select_form(&mut h, 2);
    shortcut(&mut h, "HI");
    assert_eq!(differ(&preview(&h, &pictures), &first_hidden), Vec::<usize>::new(), "isolated, the second form alone");
    // A form selected afresh and hidden by HH adds to what is hidden: nothing is left.
    select_form(&mut h, 2);
    shortcut(&mut h, "HH");
    assert!(bare(&preview(&h, &pictures), ground));
    shortcut(&mut h, "HR");
    // HC: forms are the category, so every form goes.
    select_form(&mut h, 1);
    shortcut(&mut h, "HC");
    assert!(bare(&preview(&h, &pictures), ground), "Hide Category hides every form");
    shortcut(&mut h, "HR");
    assert_eq!(differ(&preview(&h, &pictures), &shown), Vec::<usize>::new());
}

#[test]
fn the_view_control_bar_has_the_projects_temporary_hide_menu() {
    let (mut h, pictures) = cabinet();
    let shown = preview(&h, &pictures);
    // Nothing selected: what needs a selection is greyed, and so is Reset with nothing to reset.
    press(&mut h, "Temporary Hide/Isolate");
    for item in ["Hide Element (HH)", "Hide Category (HC)", "Isolate Element (HI)", "Isolate Category (IC)"] {
        assert!(is_disabled(&h, Role::Button, item), "{item}");
    }
    assert!(is_disabled(&h, Role::Button, "Reset Temporary Hide/Isolate (HR)"));
    assert!(is_disabled(&h, Role::Button, "Apply Hide/Isolate to View"), "a family's views keep nothing hidden for good yet");
    key(&mut h, Key::Escape);
    // A shortcut with nothing selected says why, and hides nothing.
    shortcut(&mut h, "HH");
    assert!(!hiding(&h));
    assert!(h.state().status.error && h.state().status.text.contains("select what to hide or isolate first"), "{}", h.state().status.text);
    select_form(&mut h, 1);
    press(&mut h, "Temporary Hide/Isolate");
    press(&mut h, "Isolate Element (HI)");
    assert!(hiding(&h));
    assert!(lit(&h, "Temporary Hide/Isolate"));
    press(&mut h, "Temporary Hide/Isolate");
    press(&mut h, "Reset Temporary Hide/Isolate (HR)");
    assert!(!hiding(&h));
    assert_eq!(differ(&preview(&h, &pictures), &shown), Vec::<usize>::new());
}

/// A door family new in the document: three forms standing in the preview's wall.
fn door() -> (Harness<'static, BimApp>, Pictures) {
    let pictures = Pictures::default();
    let mut h = Harness::builder()
        .with_size([1600.0, 1400.0])
        .with_max_steps(32)
        .renderer(pictures.clone())
        .build_ui_state(|ui, app: &mut BimApp| app.show(ui), BimApp::new(Session::new()));
    h.run();
    h.state_mut().new_family(Category::Door);
    h.run();
    (h, pictures)
}

#[test]
fn a_door_s_wall_is_a_category_of_its_own_isolate_category_hides_it_and_hide_category_leaves_it() {
    let (mut h, pictures) = door();
    let shown = preview(&h, &pictures);
    let ground = shown.pixels[0];
    // IC on a form: the forms stay, the wall goes.
    select_form(&mut h, 1);
    shortcut(&mut h, "IC");
    let forms = preview(&h, &pictures);
    let wall = differ(&shown, &forms);
    assert!(wall.len() > 2000, "the wall is gone: {} pixels", wall.len());
    assert!(!bare(&forms, ground), "the forms stay");
    shortcut(&mut h, "HR");
    // HI on one form: the wall goes with the other forms.
    select_form(&mut h, 1);
    shortcut(&mut h, "HI");
    let one = preview(&h, &pictures);
    assert!(!bare(&one, ground) && differ(&forms, &one).len() > 100, "one form left, not three");
    assert!(wall.iter().filter(|&&i| one.pixels[i] == ground).count() * 10 > wall.len() * 9, "and no wall");
    shortcut(&mut h, "HR");
    // HC on a form: every form goes, the wall stays (its opening with them).
    select_form(&mut h, 1);
    shortcut(&mut h, "HC");
    let walls = preview(&h, &pictures);
    assert!(!bare(&walls, ground));
    // Where the wall was drawn it still is, but for the jambs the frame stood in front of.
    let kept = wall.iter().filter(|&&i| walls.pixels[i] == shown.pixels[i]).count();
    assert!(kept * 10 > wall.len() * 9, "the wall is drawn as it was: {kept} of {}", wall.len());
    assert!(differ(&shown, &walls).len() > 500, "the forms are gone");
}

#[test]
fn temporary_hide_is_not_saved_no_step_and_no_unsaved_change_and_goes_with_the_family() {
    let (mut h, pictures) = cabinet();
    let shown = preview(&h, &pictures);
    select_form(&mut h, 1);
    shortcut(&mut h, "HH");
    assert!(hiding(&h));
    let ed = h.state().family_editor.as_ref().unwrap();
    assert!(!ed.unsaved(), "hiding is no change to the family");
    assert_eq!(ed.draft_family().unwrap().forms.iter().filter(|f| f.visible.is_some()).count(), 1, "the draft is as it was");
    // Undo: nothing to undo, so the form stays hidden.
    press(&mut h, "Undo");
    assert!(hiding(&h), "hiding is no step of the history");
    assert_ne!(differ(&preview(&h, &pictures), &shown), Vec::<usize>::new());
    // Loaded into the project, the project's family has both forms, neither hidden.
    press(&mut h, "Load into Project");
    let f = &h.state().session.model.families["Cabinet"];
    assert_eq!(f.forms.len(), 2);
    assert!(f.forms[0].visible.is_none());
    // Closed and opened again: nothing hidden.
    press(&mut h, "Close Family");
    h.state_mut().edit_family("Cabinet");
    h.run();
    assert!(!hiding(&h));
    assert_eq!(differ(&preview(&h, &pictures), &shown), Vec::<usize>::new());
}

#[test]
fn the_view_wears_the_projects_cyan_frame_and_note_beside_preview_visibilitys_orange_one() {
    let (mut h, _) = cabinet();
    let note = |h: &Harness<BimApp>| painted_text(h).into_iter().any(|t| t == "Temporary Hide/Isolate (HR resets)");
    assert!(!framed_in(&h, CYAN) && !note(&h));
    select_form(&mut h, 1);
    shortcut(&mut h, "HH");
    assert!(framed_in(&h, CYAN), "{:?}", outlines(&h));
    assert!(note(&h));
    // Preview Visibility too: both frames show.
    toggle(&mut h);
    assert!(framed_in(&h, CYAN) && framed_in(&h, crate::editor::PREVIEW_FRAME), "{:?}", outlines(&h));
    shortcut(&mut h, "HR");
    assert!(!framed_in(&h, CYAN) && !note(&h) && framed_in(&h, crate::editor::PREVIEW_FRAME));
}

// The adaptive modelling view.

const CORNERS: [[f64; 3]; 4] = [[0., 0., 0.], [2000., 0., 0.], [2000., 1500., 0.], [0., 1500., 0.]];

/// An adaptive family of four reference points, a closed profile through them and a solid form of it, open in its document;
/// the points' ids and the form's.
fn adaptive() -> (Harness<'static, BimApp>, Vec<String>, String) {
    let mut d = Definition::default();
    let planes = Planes { planes: &[], lines: &[] };
    let mut points = Vec::new();
    for p in CORNERS {
        let r = d.evaluate(&Default::default(), &d.preview);
        points.push(d.add_point_on(&WorkPlane::default(), p, &r, planes).unwrap());
    }
    let profile = d.add_chain(&points, true).unwrap();
    let form = d.create_form(&[profile], 1000., false).unwrap();
    let mut h = app("");
    h.set_size(vec2(1800., 1000.));
    h.state_mut().new_family(Category::Generic);
    if let Some(ed) = h.state_mut().family_editor.as_mut() {
        ed.adaptive = Some(d);
    }
    h.run();
    h.run();
    (h, points, form)
}

/// A click in the modelling view at model point `p`.
fn click_model(h: &mut Harness<BimApp>, p: [f64; 3]) {
    let pos = h.state().family_editor.as_ref().map(|e| e.modeller.screen(p)).unwrap();
    h.hover_at(pos);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    h.run();
}

fn select(h: &mut Harness<BimApp>, ids: &[&String]) {
    if let Some(ed) = h.state_mut().family_editor.as_mut() {
        ed.modeller.selection = ids.iter().map(|s| s.to_string()).collect();
    }
    h.run();
}

fn selection(h: &Harness<BimApp>) -> Vec<String> {
    h.state().family_editor.as_ref().unwrap().modeller.selection.clone()
}

/// What the modelling view drew: the forms' triangles, the reference points (filled dots 3.5 across), and whether the datums are.
fn drawn(h: &Harness<BimApp>) -> (usize, usize, bool) {
    fn walk(s: &Shape, out: &mut (usize, usize)) {
        match s {
            // The ground is a mesh of four corners; the forms', of their triangles.
            Shape::Mesh(m) if m.vertices.len() > 4 => out.0 += m.indices.len() / 3,
            Shape::Circle(c) if c.radius == 3.5 && c.fill != Color32::TRANSPARENT => out.1 += 1,
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = (0, 0);
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    (out.0, out.1, painted_text(h).iter().any(|t| t == "Center (Front/Back)"))
}

#[test]
fn hide_and_isolate_in_the_adaptive_view_leave_out_points_forms_and_datums_and_hidden_points_cant_be_picked() {
    let (mut h, points, form) = adaptive();
    let (tris, dots, datums) = drawn(&h);
    assert!(tris > 0 && dots == 4 && datums, "{:?}", drawn(&h));
    // HH on the form: gone, the points stay.
    select(&mut h, &[&form]);
    shortcut(&mut h, "HH");
    assert!(hiding(&h) && selection(&h).is_empty());
    assert_eq!(drawn(&h), (0, 4, true));
    shortcut(&mut h, "HR");
    assert_eq!(drawn(&h), (tris, 4, true));
    // HH on a point, clicked: it goes, and a click where it was picks nothing.
    click_model(&mut h, CORNERS[0]);
    assert_eq!(selection(&h), vec![points[0].clone()]);
    shortcut(&mut h, "HH");
    assert_eq!(drawn(&h), (tris, 3, true));
    click_model(&mut h, CORNERS[0]);
    assert!(!selection(&h).contains(&points[0]), "a point hidden for now can't be picked");
    shortcut(&mut h, "HR");
    // HI on the form: it alone, without the points or the datums.
    select(&mut h, &[&form]);
    shortcut(&mut h, "HI");
    assert_eq!(drawn(&h), (tris, 0, false));
    shortcut(&mut h, "HR");
    // HC on a point: the reference points go, the form and the datums stay.
    select(&mut h, &[&points[1]]);
    shortcut(&mut h, "HC");
    assert_eq!(drawn(&h), (tris, 0, true));
    shortcut(&mut h, "HR");
    // IC on a point: the points alone.
    select(&mut h, &[&points[1]]);
    shortcut(&mut h, "IC");
    assert_eq!(drawn(&h), (0, 4, false));
    assert!(framed_in(&h, CYAN), "the adaptive view wears the frame too");
    // No change to the family, no step.
    assert!(!h.state().family_editor.as_ref().unwrap().unsaved());
    press(&mut h, "Undo");
    assert!(hiding(&h));
    shortcut(&mut h, "HR");
    assert_eq!(drawn(&h), (tris, 4, true));
    assert!(!framed_in(&h, CYAN));
}
