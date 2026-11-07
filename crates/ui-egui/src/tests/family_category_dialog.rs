//! T5b: Family Category and Parameters as Revit's (the category list under a filter, and a Family Parameters table of the
//! family's options), and the same options in Properties' Other group when nothing is selected. Both edit the draft: the
//! dialog's visit is one step of the document's history, Cancel puts back what it began from, and Properties' rows undo.
use super::*;
use bimcraft_core::family::{Category, Expr, Family};
use bimcraft_core::geom::P2;
use bimcraft_core::render::{self, Prim};
use egui_kittest::kittest::NodeT;

const VERSION: &str = "Overwrite the existing version";

/// A box of furniture 1000 square and 500 tall, placed at (0, 0), and one of a work plane-based sign hosted on a plane.
const BOXES: &str = r#"family {"name": "Box", "category": "furniture", "parameters": [{"name": "Width", "value": 1000}], "types": {"A": {}}, "forms": [{"profile": [[0, 0], ["Width", 0], ["Width", 1000], [0, 1000]], "to": 500}]}
component {"family": "Box", "at": [0, 0]}
family {"name": "Sign", "types": {"A": {}}, "options": {"work_plane_based": true}, "forms": [{"profile": [[0, 0], [600, 0], [600, 600], [0, 600]], "to": 50}]}
reference_plane {"name": "Ramp", "start": [3000, 0, 0], "end": [8000, 0, 0], "up": [0, 0.8660254037844386, 0.5]}
component {"family": "Sign", "at": [4000, 2000], "host": {"plane": "Ramp"}}"#;

/// The project, `family` open as a document.
fn opened(family: &str) -> Harness<'static, BimApp> {
    let mut h = app(BOXES);
    h.set_size(vec2(1800.0, 1200.0));
    h.state_mut().edit_family(family);
    h.run();
    h
}

/// Family Category and Parameters opened.
fn dialog(h: &mut Harness<BimApp>) {
    press(h, "Family Category and Parameters");
    assert!(open(h));
}

fn open(h: &Harness<BimApp>) -> bool {
    h.state().family_editor.as_ref().is_some_and(|e| e.dialog.is_some())
}

/// The draft as it stands, built.
fn draft(h: &Harness<BimApp>) -> Family {
    h.state().family_editor.as_ref().unwrap().draft_family().unwrap()
}

/// The widget named `label` furthest right: the dialog's, over Properties' on the left.
fn right<'t>(h: &'t Harness<'_, BimApp>, label: &'t str) -> egui_kittest::Node<'t> {
    h.get_all_by_label(label).max_by(|a, b| a.rect().center().x.total_cmp(&b.rect().center().x)).unwrap()
}

/// The widget named `label` furthest left: Properties', under the dialog.
fn left<'t>(h: &'t Harness<'_, BimApp>, label: &'t str) -> egui_kittest::Node<'t> {
    h.get_all_by_label(label).min_by(|a, b| a.rect().center().x.total_cmp(&b.rect().center().x)).unwrap()
}

fn tick(h: &mut Harness<BimApp>, label: &str) {
    right(h, label).click();
    h.run();
}

fn ticked(h: &Harness<'_, BimApp>, label: &str) -> bool {
    right(h, label).accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

/// Types `text` into the field named `label`, the dialog's when Properties has one too, and lets it go.
fn typed(h: &mut Harness<BimApp>, label: &str, text: &str) {
    right(h, label).focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.key_press(Key::Backspace);
    right(h, label).type_text(text);
    h.run();
    key(h, Key::Tab);
}

fn value(h: &Harness<'_, BimApp>, label: &str) -> String {
    right(h, label).accesskit_node().value().map(|v| v.to_string()).unwrap_or_default()
}

/// Whether a section across the middle of the box (0..1000 square, 500 tall) fills its cut, poché, in `model`.
fn box_cut(h: &Harness<BimApp>) -> bool {
    let mut s = bimcraft_core::engine::Session::new();
    s.model = Arc::clone(&h.state().session.model);
    let id = s.model.elements.iter().find(|(_, e)| e.family().is_some_and(|(f, _)| f == "Box")).map(|(id, _)| *id).unwrap();
    let sec = s.execute("section", &json!({"start": [-500, 500], "end": [1500, 500]})).unwrap()["id"].as_u64().unwrap();
    let view = render::section_fit(&s.model, sec, 1000, 800);
    let d = render::section(&s.model, sec, view);
    d.prims.iter().zip(&d.owners).filter(|(_, o)| **o == Some(id)).any(|(p, _)| match p {
        Prim::Fill { rings, .. } => {
            rings.iter().any(|r| bimcraft_core::geom::contains(&r.iter().map(|q| view.mm(*q)).collect::<Vec<_>>(), P2(1000.0, 250.0)))
        }
        _ => false,
    })
}

#[test]
fn every_option_round_trips_through_the_dialog() {
    let mut h = opened("Box");
    dialog(&mut h);
    assert!(h.query_by_label("Family Parameters").is_some());
    assert!(ticked(&h, "Always Vertical") && !ticked(&h, "Work Plane-Based"), "Revit's defaults");
    for label in ["Work Plane-Based", "Always Vertical", "Cut with Voids When Loaded", "Shared"] {
        tick(&mut h, label);
    }
    typed(&mut h, "OmniClass Number", "23.40.20");
    typed(&mut h, "OmniClass Title", "Tables");
    typed(&mut h, "Default Elevation", "450");
    press(&mut h, "OK");
    assert!(!open(&h));
    let f = draft(&h);
    assert!(f.work_plane_based && !f.always_vertical && f.cut_with_voids_when_loaded && f.shared);
    let o = f.omniclass.clone().unwrap();
    assert_eq!((o.number.as_str(), o.title.as_str()), ("23.40.20", "Tables"));
    assert_eq!(f.default_elevation, 450.0);
    // Reopened, it shows them.
    dialog(&mut h);
    assert!(ticked(&h, "Work Plane-Based") && !ticked(&h, "Always Vertical"));
    assert!(ticked(&h, "Cut with Voids When Loaded") && ticked(&h, "Shared"));
    assert_eq!(value(&h, "OmniClass Number"), "23.40.20");
    assert_eq!(value(&h, "OmniClass Title"), "Tables");
    assert_eq!(value(&h, "Default Elevation"), "450");
    press(&mut h, "Cancel");
    // The visit was one step: Undo takes every option back.
    press(&mut h, "Undo");
    let f = draft(&h);
    assert!(!f.work_plane_based && f.always_vertical && !f.shared && f.omniclass.is_none() && f.default_elevation == 0.0);
    // Loaded, the project's family has them.
    press(&mut h, "Redo");
    press(&mut h, "Load into Project");
    if h.query_by_label(VERSION).is_some() {
        press(&mut h, VERSION);
    }
    let p = &h.state().session.model.families["Box"];
    assert!(p.work_plane_based && p.shared && p.default_elevation == 450.0, "{:?}", h.state().family_editor.as_ref().unwrap().problem());
}

#[test]
fn cancel_puts_back_what_the_dialog_began_from() {
    let mut h = opened("Box");
    let before = draft(&h);
    dialog(&mut h);
    tick(&mut h, "Shared");
    tick(&mut h, "Room Calculation Point");
    typed(&mut h, "OmniClass Number", "23.30");
    // Apply keeps them for Cancel to come back to; only what came after goes.
    press(&mut h, "Apply");
    tick(&mut h, "Cut with Voids When Loaded");
    press(&mut h, "Cancel");
    assert!(!open(&h));
    let f = draft(&h);
    assert!(f.shared && f.room_calculation_point.is_some() && !f.cut_with_voids_when_loaded, "applied kept, the rest put back");
    dialog(&mut h);
    tick(&mut h, "Shared");
    press(&mut h, "Cancel");
    assert!(draft(&h).shared, "cancelled");
    // Back to how it opened.
    press(&mut h, "Undo");
    assert_eq!(draft(&h), before);
}

#[test]
fn an_omniclass_number_the_command_refuses_is_said_in_the_dialog_which_stays_open() {
    let mut h = opened("Box");
    dialog(&mut h);
    typed(&mut h, "OmniClass Number", "23-30");
    press(&mut h, "OK");
    assert!(open(&h), "refused: the dialog stays open");
    assert!(h.query_by_label_contains("an OmniClass number is digits and dots").is_some(), "with the core's reason");
    // A title without a number is refused too.
    typed(&mut h, "OmniClass Number", "");
    typed(&mut h, "OmniClass Title", "Tables");
    press(&mut h, "Apply");
    assert!(open(&h) && h.query_by_label_contains("an OmniClass number").is_some());
    typed(&mut h, "OmniClass Number", "23.30.20.17");
    assert!(h.query_by_label_contains("an OmniClass number is").is_none(), "said only while the draft is what was refused");
    press(&mut h, "OK");
    assert!(!open(&h));
    assert_eq!(draft(&h).omniclass.unwrap().number, "23.30.20.17");
}

#[test]
fn room_calculation_point_turns_on_at_its_start_takes_coordinates_and_turns_off() {
    let mut h = opened("Box");
    dialog(&mut h);
    assert!(h.query_by_label("Room Calculation Point X").is_none(), "no coordinates while it is off");
    tick(&mut h, "Room Calculation Point");
    assert_eq!(
        draft(&h).room_calculation_point,
        Some(crate::editor::options::ROOM_POINT_START.map(Expr::Number)),
        "the insertion point, a little over it"
    );
    typed(&mut h, "Room Calculation Point X", "500");
    typed(&mut h, "Room Calculation Point Y", "-250");
    press(&mut h, "OK");
    let point = draft(&h).room_calculation_point.unwrap();
    assert_eq!(point, [Expr::Number(500.0), Expr::Number(-250.0), Expr::Number(300.0)]);
    dialog(&mut h);
    assert_eq!(value(&h, "Room Calculation Point X"), "500");
    assert_eq!(value(&h, "Room Calculation Point Y"), "-250");
    tick(&mut h, "Room Calculation Point");
    assert!(h.query_by_label("Room Calculation Point X").is_none());
    press(&mut h, "OK");
    assert_eq!(draft(&h).room_calculation_point, None);
}

#[test]
fn enable_cutting_in_views_is_by_category_unless_said_and_sections_show_it_once_loaded() {
    let mut h = opened("Box");
    assert!(!box_cut(&h), "furniture: sections don't cut it, by its category");
    dialog(&mut h);
    assert!(ticked(&h, "By Category") && !ticked(&h, "Enable Cutting in Views"), "the category's rule, shown");
    assert!(right(&h, "Enable Cutting in Views").accesskit_node().is_disabled(), "greyed while the category decides");
    // Its own: off at first (what the category said), then on.
    tick(&mut h, "By Category");
    assert_eq!(draft(&h).cut_in_views, Some(false));
    tick(&mut h, "Enable Cutting in Views");
    assert_eq!(draft(&h).cut_in_views, Some(true));
    press(&mut h, "OK");
    press(&mut h, "Load into Project");
    press(&mut h, VERSION);
    assert!(box_cut(&h), "told to be cut, a section cuts it");
    // Off, then back to the category's rule.
    dialog(&mut h);
    tick(&mut h, "Enable Cutting in Views");
    press(&mut h, "OK");
    assert_eq!(draft(&h).cut_in_views, Some(false));
    press(&mut h, "Load into Project");
    press(&mut h, VERSION);
    assert!(!box_cut(&h));
    dialog(&mut h);
    tick(&mut h, "By Category");
    press(&mut h, "OK");
    assert_eq!(draft(&h).cut_in_views, None);
    // A generic model's category cuts it: By Category shows the tick on.
    pick_category(&mut h, "Generic");
    assert!(ticked(&h, "Enable Cutting in Views"));
    press(&mut h, "Cancel");
}

/// The category `name` picked in Family Category and Parameters' list, through its filter.
fn pick_category(h: &mut Harness<BimApp>, name: &str) {
    dialog(h);
    typed(h, "Filter", name);
    right(h, name).click();
    h.run();
}

/// How many widgets are named `label`.
fn count(h: &Harness<'_, BimApp>, label: &str) -> usize {
    h.query_all_by_label(label).count()
}

#[test]
fn the_category_list_filters_and_a_door_shows_operation_and_refuses_work_plane_based() {
    let mut h = opened("Box");
    dialog(&mut h);
    assert!(h.query_by_label("Operation").is_none(), "only a door's or a window's");
    let furniture = count(&h, "Furniture");
    typed(&mut h, "Filter", "do");
    assert_eq!(count(&h, "Furniture"), furniture - 1, "filtered out of the list");
    right(&h, "Door").click();
    h.run();
    assert!(h.query_by_label("Operation").is_some());
    assert!(right(&h, "Work Plane-Based").accesskit_node().is_disabled(), "a door stands in its wall");
    typed(&mut h, "Filter", "");
    assert!(right(&h, "Furniture").rect().center().x > 600.0, "back in the list");
    press(&mut h, "Cancel");
    assert_eq!(draft(&h).category, Category::Furniture);
}

#[test]
fn properties_other_rows_edit_the_same_options_and_undo() {
    let mut h = opened("Box");
    assert!(count(&h, "Other") > 0, "Properties' Other group, nothing being selected");
    // The palette scrolls to the row, as a user scrolls it.
    left(&h, "Shared").scroll_to_me();
    h.run();
    left(&h, "Shared").click();
    h.run();
    assert!(draft(&h).shared);
    in_properties(&mut h, "OmniClass Number", "23.30");
    assert_eq!(draft(&h).omniclass.unwrap().number, "23.30");
    // The dialog shows what Properties set.
    dialog(&mut h);
    assert!(ticked(&h, "Shared"));
    assert_eq!(value(&h, "OmniClass Number"), "23.30");
    press(&mut h, "Cancel");
    // What the command would refuse is why the draft can't be tried, said in the view.
    in_properties(&mut h, "OmniClass Number", "23.30x");
    assert!(h.query_by_label_contains("an OmniClass number is digits and dots").is_some());
    // Undo, step by step.
    press(&mut h, "Undo");
    assert_eq!(draft(&h).omniclass.unwrap().number, "23.30");
    press(&mut h, "Undo");
    assert!(draft(&h).omniclass.is_none() && draft(&h).shared);
    press(&mut h, "Undo");
    assert!(!draft(&h).shared);
}

/// Types `text` into Properties' field `label`, then lets it go, as a click elsewhere does.
fn in_properties(h: &mut Harness<BimApp>, label: &str, text: &str) {
    left(h, label).scroll_to_me();
    h.run();
    left(h, label).focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    left(h, label).type_text(text);
    h.run();
    h.get_by_label("Close Family").focus();
    h.run();
}

#[test]
fn turning_work_plane_based_off_while_components_are_on_planes_is_refused_on_load() {
    let mut h = opened("Sign");
    let was = h.state().session.model.to_json().unwrap();
    dialog(&mut h);
    assert!(ticked(&h, "Work Plane-Based"));
    tick(&mut h, "Work Plane-Based");
    press(&mut h, "OK");
    press(&mut h, "Load into Project");
    if h.query_by_label(VERSION).is_some() {
        press(&mut h, VERSION);
    }
    let ed = h.state().family_editor.as_ref().unwrap();
    assert!(ed.problem().is_some_and(|p| p.contains("isn't work plane-based")), "{:?}", ed.problem());
    assert!(h.query_by_label_contains("isn't work plane-based").is_some(), "said in the document");
    assert_eq!(h.state().session.model.to_json().unwrap(), was, "the project is as it was");
}
