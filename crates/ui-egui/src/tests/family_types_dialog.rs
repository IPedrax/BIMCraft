//! T8: the family document's Family Types as Revit's: the Type name drop-down with New Type, Rename Type and Delete Type, a
//! search, one table (Parameter, Value, Formula, Lock) under the groups' headings, and New, Edit and Delete Parameter under it.
//! Everything goes to the draft; Cancel puts back what the dialog began from.
use super::*;
use bimcraft_core::family::{Expr, Family, Group};
use egui_kittest::kittest::NodeT;

const CABINET: &str = r#"family {"name": "Cabinet", "category": "generic", "parameters": [{"name": "Width", "value": 1000}, {"name": "Height", "formula": "Width / 2"}, {"name": "Depth", "value": 600}, {"name": "Show Top", "kind": "yesno", "group": "visibility", "value": true}, {"name": "Note", "kind": "text", "instance": true, "value": "dry"}], "types": {"A": {}, "B": {"Width": 600}}, "forms": [{"profile": [[0, 0], ["Width", 0], ["Width", "Depth"], [0, "Depth"]], "to": "Height"}]}"#;

/// Cabinet opened as a document, Family Types open on it.
fn opened() -> Harness<'static, BimApp> {
    let mut h = app(CABINET);
    h.set_size(vec2(1800.0, 1000.0));
    h.state_mut().edit_family("Cabinet");
    h.run();
    press(&mut h, "Family Types");
    h
}

/// The draft as it stands, built.
fn draft(h: &Harness<BimApp>) -> Family {
    h.state().family_editor.as_ref().unwrap().draft_family().unwrap()
}

fn open(h: &Harness<BimApp>) -> bool {
    h.state().family_editor.as_ref().is_some_and(|e| e.dialog.is_some())
}

fn disabled(h: &Harness<BimApp>, label: &str) -> bool {
    h.get_by_label(label).accesskit_node().is_disabled()
}

fn value(h: &Harness<BimApp>, label: &str) -> Option<String> {
    h.get_by_label(label).accesskit_node().value().map(|v| v.to_string())
}

fn names(f: &Family) -> Vec<&str> {
    f.parameters.iter().map(|p| p.name.as_str()).collect()
}

/// The Name or Parameter Properties dialog's OK, over Family Types' own.
fn ok_over(h: &mut Harness<BimApp>) {
    press_near(h, "OK", "Name:");
}

/// The type `name` picked in Type name.
fn pick_type(h: &mut Harness<BimApp>, name: &str) {
    press(h, "Type Name");
    press_near(h, name, "Type Name");
}

#[test]
fn new_rename_and_delete_type_and_the_last_type_stays() {
    let mut h = opened();
    pick_type(&mut h, "B");
    // New Type: a name asked, the type shown copied.
    press(&mut h, "New Type");
    type_into(&mut h, "Name:", "C");
    ok_over(&mut h);
    assert_eq!(value(&h, "Type Name").as_deref(), Some("C"), "the new type is the one shown");
    assert_eq!(draft(&h).types["C"]["Width"], json!(600.0), "a copy of B's values");
    assert_eq!(value(&h, "Parameter Value Width").as_deref(), Some("600"));
    // A name taken is refused, in the Name dialog.
    press(&mut h, "Rename Type");
    type_into(&mut h, "Name:", "A");
    ok_over(&mut h);
    assert!(h.query_by_label_contains("named A already").is_some());
    type_into(&mut h, "Name:", "D");
    ok_over(&mut h);
    assert!(draft(&h).types.contains_key("D") && !draft(&h).types.contains_key("C"));
    // Delete Type, down to the last, which stays, saying why.
    for _ in 0..2 {
        press(&mut h, "Delete Type");
    }
    assert_eq!(draft(&h).types.len(), 1);
    press(&mut h, "Delete Type");
    assert_eq!(draft(&h).types.len(), 1, "the last type stays");
    assert!(h.query_by_label_contains("The last type can't be deleted").is_some());
    // Cancel puts every type back.
    press(&mut h, "Cancel");
    assert!(!open(&h));
    assert_eq!(draft(&h).types.keys().collect::<Vec<_>>(), ["A", "B"]);
}

#[test]
fn search_keeps_the_rows_whose_name_holds_what_is_typed_whatever_its_case() {
    let mut h = opened();
    type_into(&mut h, "Search parameters", "wID");
    assert!(h.query_by_label("Parameter Value Width").is_some());
    assert!(h.query_by_label("Parameter Value Depth").is_none());
    assert!(h.query_by_label("Parameter Value Show Top").is_none());
    type_into(&mut h, "Search parameters", "zzz");
    assert!(h.query_by_label("Nothing matches the search.").is_some());
}

#[test]
fn rows_go_under_their_groups_headings_in_the_palettes_order_and_a_heading_folds_them() {
    let mut h = opened();
    // The dialog's heading: Properties' Other group, under it on the left, is the family's options (T5b).
    let y = |h: &Harness<BimApp>, label: &str| {
        h.get_all_by_label(label).map(|n| n.rect()).max_by(|a, b| a.center().x.total_cmp(&b.center().x)).unwrap().top()
    };
    // Dimensions before Visibility before Other, as `Group::ALL` (the Properties palette's order).
    assert!(y(&h, "Dimensions") < y(&h, "Visibility") && y(&h, "Visibility") < y(&h, "Other"));
    assert!(y(&h, "Dimensions") < y(&h, "Parameter Value Width") && y(&h, "Parameter Value Width") < y(&h, "Visibility"));
    assert!(y(&h, "Visibility") < y(&h, "Parameter Value Show Top"));
    // Folded, Dimensions shows none of its rows; unfolded, they are back.
    press(&mut h, "Dimensions");
    assert!(h.query_by_label("Parameter Value Width").is_none());
    assert!(h.query_by_label("Parameter Value Show Top").is_some(), "the other groups stay open");
    press(&mut h, "Dimensions");
    assert!(h.query_by_label("Parameter Value Width").is_some());
}

#[test]
fn an_instance_parameter_is_marked_default_and_its_value_is_the_familys() {
    let mut h = opened();
    assert!(h.query_by_label("Note (default)").is_some(), "an instance parameter, as Revit marks it");
    assert!(h.query_by_label("Width (default)").is_none() && h.query_by_label("Width").is_some());
    type_into(&mut h, "Parameter Value Note", "wet");
    press(&mut h, "Apply");
    let f = draft(&h);
    assert_eq!(f.parameters[4].value, json!("wet"), "the default a new instance takes");
    // A type parameter's value is the type shown's own.
    pick_type(&mut h, "B");
    type_into(&mut h, "Parameter Value Depth", "450");
    press(&mut h, "OK");
    let f = draft(&h);
    assert_eq!(f.types["B"]["Depth"], json!(450.0));
    assert!(!f.types["A"].contains_key("Depth"));
}

#[test]
fn a_row_a_formula_drives_is_greyed_and_shows_what_it_comes_to() {
    let h = opened();
    assert!(disabled(&h, "Parameter Value Height"));
    assert_eq!(value(&h, "Parameter Value Height").as_deref(), Some("500"));
    assert!(!disabled(&h, "Parameter Value Width"));
    assert_eq!(value(&h, "Parameter Formula Height").as_deref(), Some("Width / 2"));
}

#[test]
fn a_good_formula_is_taken_and_a_bad_one_refused_with_t9s_words_in_the_dialog() {
    let mut h = opened();
    type_into(&mut h, "Parameter Formula Depth", "Width * 0.5");
    press(&mut h, "Apply");
    assert!(open(&h));
    assert_eq!(draft(&h).parameters[2].formula.as_deref(), Some("Width * 0.5"));
    assert!(disabled(&h, "Parameter Value Depth"), "driven now");
    assert_eq!(value(&h, "Parameter Value Depth").as_deref(), Some("500"));
    // A name the family hasn't: OK is refused, the dialog stays and says why.
    type_into(&mut h, "Parameter Formula Depth", "Nope + 1");
    press(&mut h, "OK");
    assert!(open(&h), "refused: Family Types stays open");
    assert!(h.query_by_label_contains("the following is not a valid parameter: Nope").is_some());
    // Cancel goes back to what Apply kept.
    press(&mut h, "Cancel");
    assert_eq!(draft(&h).parameters[2].formula.as_deref(), Some("Width * 0.5"));
}

#[test]
fn every_row_has_a_lock_shown_disabled_until_labelled_dimensions_come() {
    let h = opened();
    assert!(h.query_by_label("Lock").is_some(), "the column's heading");
    for name in ["Width", "Height", "Depth", "Show Top", "Note"] {
        assert!(disabled(&h, &format!("Lock {name}")), "{name}");
    }
}

#[test]
fn new_edit_and_delete_parameter_and_a_parameter_in_use_is_not_deleted() {
    let mut h = opened();
    // New Parameter: Parameter Properties, an instance parameter under Constraints.
    press(&mut h, "New Parameter");
    type_into(&mut h, "Name:", "Shelf");
    pick(&mut h, Role::RadioButton, "Instance");
    press(&mut h, "Group parameter under:");
    press_near(&mut h, "Constraints", "Group parameter under:");
    ok_over(&mut h);
    let f = draft(&h);
    let shelf = f.parameters.iter().find(|p| p.name == "Shelf").unwrap();
    assert!(shelf.instance);
    assert_eq!(shelf.group(), Group::Constraints);
    assert!(h.query_by_label("Shelf (default)").is_some());
    assert!(h.get_by_label("Constraints").rect().top() < h.get_by_label("Dimensions").rect().top());
    // A name taken is refused in Parameter Properties.
    press(&mut h, "New Parameter");
    type_into(&mut h, "Name:", "Depth");
    ok_over(&mut h);
    assert!(h.query_by_label_contains("parameter named Depth already").is_some());
    press_near(&mut h, "Cancel", "Name:");
    // Edit Parameter on Width: renamed, and every formula and form saying it follows.
    press(&mut h, "Width");
    press(&mut h, "Edit Parameter");
    type_into(&mut h, "Name:", "Breadth");
    ok_over(&mut h);
    let f = draft(&h);
    assert!(names(&f).contains(&"Breadth") && !names(&f).contains(&"Width"));
    assert_eq!(f.parameters.iter().find(|p| p.name == "Height").unwrap().formula.as_deref(), Some("Breadth / 2"));
    assert_eq!(f.forms[0].corners().unwrap()[1][0], Expr::Formula("Breadth".into()));
    assert_eq!(f.types["B"]["Breadth"], json!(600.0));
    // Delete Parameter: Note goes; Breadth, which the form and Height use, is refused, saying why.
    press(&mut h, "Note (default)");
    press(&mut h, "Delete Parameter");
    assert!(!names(&draft(&h)).contains(&"Note"));
    press(&mut h, "Breadth");
    press(&mut h, "Delete Parameter");
    assert!(names(&draft(&h)).contains(&"Breadth"), "in use: kept");
    assert!(h.query_by_label_contains("Breadth can't be deleted").is_some());
    // Cancel: the dialog's start.
    press(&mut h, "Cancel");
    assert_eq!(names(&draft(&h)), ["Width", "Height", "Depth", "Show Top", "Note"]);
}

#[test]
fn a_parameter_renamed_in_edit_parameter_loads_into_the_project_renamed() {
    let mut h = opened();
    press(&mut h, "Width");
    press(&mut h, "Edit Parameter");
    type_into(&mut h, "Name:", "Breadth");
    ok_over(&mut h);
    press(&mut h, "OK");
    press(&mut h, "Load into Project");
    if h.query_by_label("Family Already Exists").is_some() {
        press(&mut h, "Overwrite the existing version and its parameter values");
    }
    let f = &h.state().session.model.families["Cabinet"];
    assert_eq!(names(f), ["Breadth", "Height", "Depth", "Show Top", "Note"]);
    assert_eq!(f.types["B"].get("Breadth"), Some(&json!(600.0)));
    assert!(!f.types["B"].contains_key("Width"));
}

#[test]
fn move_up_and_sort_order_a_groups_rows_and_the_family_keeps_the_order() {
    let mut h = opened();
    press(&mut h, "Depth");
    press_near(&mut h, "Move Up", "Delete Parameter");
    assert_eq!(names(&draft(&h)), ["Width", "Depth", "Height", "Show Top", "Note"]);
    press_near(&mut h, "Move Down", "Delete Parameter");
    assert_eq!(names(&draft(&h)), ["Width", "Height", "Depth", "Show Top", "Note"]);
    press(&mut h, "Ascending");
    assert_eq!(names(&draft(&h)), ["Depth", "Height", "Width", "Show Top", "Note"], "each group sorted in its places");
    // The dialog's heading: Properties' Other group, under it on the left, is the family's options (T5b).
    let y = |h: &Harness<BimApp>, label: &str| {
        h.get_all_by_label(label).map(|n| n.rect()).max_by(|a, b| a.center().x.total_cmp(&b.center().x)).unwrap().top()
    };
    assert!(y(&h, "Parameter Value Depth") < y(&h, "Parameter Value Width"), "shown in that order");
    press(&mut h, "Descending");
    assert_eq!(names(&draft(&h)), ["Width", "Height", "Depth", "Show Top", "Note"]);
    // Loaded into the project, the family keeps it.
    press(&mut h, "Ascending");
    press(&mut h, "OK");
    press(&mut h, "Load into Project");
    if h.query_by_label("Family Already Exists").is_some() {
        press(&mut h, "Overwrite the existing version");
    }
    let f = &h.state().session.model.families["Cabinet"];
    assert_eq!(names(f), ["Depth", "Height", "Width", "Show Top", "Note"]);
}

#[test]
fn cancel_puts_back_what_family_types_began_with() {
    let mut h = opened();
    let was = draft(&h);
    type_into(&mut h, "Parameter Value Width", "1234");
    press(&mut h, "New Type");
    type_into(&mut h, "Name:", "Z");
    ok_over(&mut h);
    press(&mut h, "Note (default)");
    press(&mut h, "Delete Parameter");
    press(&mut h, "Cancel");
    assert!(!open(&h));
    assert_eq!(draft(&h), was);
}
