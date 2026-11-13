//! Manage > Project Parameters, driven as a user does.

use egui_kittest::kittest::NodeT;

use super::*;

fn open(h: &mut Harness<BimApp>) {
    super::press(h, "Manage");
    super::press(h, "Project Parameters");
}

/// Clicks `label` where it's on top: the window's, over a ribbon or a browser that says the same.
fn press(h: &mut Harness<BimApp>, label: &str) {
    let nodes: Vec<_> = h.query_all_by_label(label).collect();
    nodes.last().unwrap_or_else(|| panic!("no `{label}`")).click();
    h.run();
}

fn temp(name: &str) -> String {
    tempfile::tempdir().unwrap().keep().join(name).to_string_lossy().into_owned()
}

#[test]
fn a_parameter_is_made_in_the_table_applied_at_once_and_given_values_in_properties() {
    use bimcraft_core::model::ParameterKind;
    let mut h = app(BOX);
    open(&mut h);
    assert!(h.query_by_label("No project parameters yet: Add Parameter makes one, Import/Export brings them from a file.").is_some());
    press(&mut h, "Add Parameter");
    assert!(h.state().session.model.parameters.is_empty(), "staged until Apply");
    assert!(h.query_by_label("1 parameters | create 1 | modify 0 | existing 0 | delete 0").is_some());
    type_role(&mut h, Role::TextInput, "Name of Parameter 1", "Finish Height");
    h.get_by_role_and_label(Role::ComboBox, "Type of Finish Height").click();
    h.run();
    press(&mut h, "Length");
    type_role(&mut h, Role::TextInput, "Group of Finish Height", "Finishes");
    type_role(&mut h, Role::TextInput, "Description of Finish Height", "To the top of the tile");
    press(&mut h, "Apply");
    let p = h.state().session.model.parameters["Finish Height"].clone();
    assert_eq!(
        (p.kind, p.categories.clone(), p.group.as_ref().map_or("", |u| u.label())),
        (ParameterKind::Length, vec!["wall".to_string()], "Finishes")
    );
    assert!(h.query_by_label("1 parameters | create 0 | modify 0 | existing 1 | delete 0").is_some());
    press(&mut h, "Close");

    // The bottom wall's value, typed in the project's units, under its group; emptied, it has none.
    let wall = ids(&h, "wall")[0];
    click(&mut h, P2(4000.0, 0.0));
    assert!(h.query_by_label("Finishes").is_some(), "its group");
    type_role(&mut h, Role::TextInput, "Finish Height", "2400");
    assert_eq!(field(&h, wall, "params"), json!({"Finish Height": 2400.0}));
    type_role(&mut h, Role::TextInput, "Finish Height", "");
    assert_eq!(field(&h, wall, "params"), Value::Null);
    key(&mut h, Key::Escape);
    h.state_mut().run("parameter", json!({"name": "Finish Height", "read_only": true}));
    h.state_mut().run("modify", json!({"id": wall, "params": {"Finish Height": 2400}}));
    h.run();
    click(&mut h, P2(4000.0, 0.0));
    assert!(h.query_by_role_and_label(Role::TextInput, "Finish Height").is_none(), "shown, not editable");
    assert!(h.query_by_label("Finish Height").is_some());
}

#[test]
fn checked_rows_are_renamed_given_categories_deleted_and_undone_as_one_step() {
    let mut h = app(BOX);
    for name in ["Old One", "Old Two", "Going"] {
        h.state_mut().run("parameter", json!({"name": name, "categories": ["wall"]}));
    }
    open(&mut h);
    pick(&mut h, Role::CheckBox, "Check Old One");
    pick(&mut h, Role::CheckBox, "Check Old Two");
    press(&mut h, "Rename Checked");
    type_role(&mut h, Role::TextInput, "Prefix", "AE_");
    type_role(&mut h, Role::TextInput, "Find", "Old");
    type_role(&mut h, Role::TextInput, "Replace", "New");
    press(&mut h, "Rename");
    // Categories for the two still checked.
    press(&mut h, "Categories");
    pick(&mut h, Role::CheckBox, "Doors");
    press(&mut h, "Parameters");
    assert!(h.state().session.model.parameters.contains_key("Old One"), "staged");
    assert!(h.query_by_label("3 parameters | create 0 | modify 2 | existing 1 | delete 0").is_some());
    pick(&mut h, Role::CheckBox, "Check AE_New One");
    pick(&mut h, Role::CheckBox, "Check AE_New Two");
    pick(&mut h, Role::CheckBox, "Check Going");
    press(&mut h, "Batch Actions");
    press(&mut h, "Delete Checked");
    assert!(h.query_by_label("3 parameters | create 0 | modify 2 | existing 0 | delete 1").is_some());
    press(&mut h, "Apply");
    let m = &h.state().session.model.parameters;
    assert_eq!(m.keys().collect::<Vec<_>>(), ["AE_New One", "AE_New Two"]);
    assert_eq!(m["AE_New One"].categories, ["door", "wall"]);
    h.state_mut().session.execute("undo", &json!({})).unwrap();
    assert_eq!(h.state().session.model.parameters.len(), 3, "one undo step");
}

#[test]
fn a_new_guid_is_no_action_and_discard_takes_back_what_is_staged() {
    let mut h = app(BOX);
    h.state_mut().run("parameter", json!({"name": "Rating", "categories": ["wall"]}));
    open(&mut h);
    pick(&mut h, Role::CheckBox, "Check Rating");
    press(&mut h, "Batch Actions");
    assert!(h.query_by_label("Duplicate Checked").is_some());
    assert!(h.query_by_label("New GUID").is_none() && h.query_by_label("Generate New GUID").is_none());
    press(&mut h, "Duplicate Checked");
    assert!(h.state().session.model.parameters.contains_key("Rating Copy"));
    type_role(&mut h, Role::TextInput, "Description of Rating", "typed");
    press(&mut h, "Discard Changes");
    assert!(h.query_by_label("2 parameters | create 0 | modify 0 | existing 2 | delete 0").is_some());
}

#[test]
fn an_import_shows_what_it_would_do_asks_what_it_cant_tell_and_then_does_it() {
    let path = temp("office.csv");
    let other = "7f2e3d4c-5b6a-4789-8abc-def012345678";
    std::fs::write(&path, format!("GUID,Name,Kind,Categories\n,Cost,number,Walls\n,Odd,text,Gizmos\n{other},Rating,text,Doors\n")).unwrap();
    let mut h = app(BOX);
    h.state_mut().run("parameter", json!({"name": "Rating", "categories": ["wall"]}));
    open(&mut h);
    h.state_mut().parameters_import_from(&path);
    h.run();
    assert!(h.query_by_label("create 1 | modify 0 | existing 0 | conflict 2 | skip 0").is_some());
    assert_eq!(h.state().session.model.parameters.len(), 1, "a dry run");
    assert!(h.get_by_label("Import").accesskit_node().is_disabled(), "not while a row needs a decision");

    h.get_by_role_and_label(Role::ComboBox, "Map Gizmos").click();
    h.run();
    h.get_by_label("Columns").scroll_to_me();
    h.run();
    press(&mut h, "Columns");
    assert!(h.query_by_label("create 2 | modify 0 | existing 0 | conflict 1 | skip 0").is_some());
    h.get_by_role_and_label(Role::ComboBox, "Decision for Rating").click();
    h.run();
    press(&mut h, "Keep Separate");
    assert!(h.query_by_label("create 3 | modify 0 | existing 0 | conflict 0 | skip 0").is_some());
    press(&mut h, "Import");
    let m = &h.state().session.model;
    assert_eq!(m.parameters["Odd"].categories, ["column"]);
    assert_eq!(m.parameters["Rating 2"].categories, ["door"]);
    assert_eq!(m.library.guid(&bimcraft_core::library::Kind::Parameter, "Rating 2").unwrap().to_string(), other);
    assert_eq!(m.parameter_map["Gizmos"].as_deref(), Some("column"), "kept for next time");
    assert!(h.query_by_label("4 parameters | create 0 | modify 0 | existing 4 | delete 0").is_some());
}

#[test]
fn a_revit_shared_parameter_file_needs_categories_and_a_bad_file_is_said_in_the_window() {
    let path = temp("revit.txt");
    std::fs::write(
        &path,
        "*GROUP\tID\tNAME\nGROUP\t1\tFire\n*PARAM\tGUID\tNAME\tDATATYPE\tDATACATEGORY\tGROUP\tVISIBLE\tDESCRIPTION\tUSERMODIFIABLE\n\
         PARAM\t1f2e3d4c-5b6a-4789-8abc-def012345678\tFire Rating\tTEXT\t\t1\t1\tIts resistance\t1\n",
    )
    .unwrap();
    let mut h = app(BOX);
    open(&mut h);
    h.state_mut().parameters_import_from("/nonexistent/file.csv");
    h.run();
    assert!(h.get_all_by_label_contains("/nonexistent/file.csv").count() > 0, "said in the window");
    h.state_mut().parameters_import_from(&path);
    h.run();
    assert!(h.query_by_label("create 0 | modify 0 | existing 0 | conflict 1 | skip 0").is_some());
    press(&mut h, "Categories for rows that name none");
    pick(&mut h, Role::CheckBox, "Walls");
    assert!(h.query_by_label("create 1 | modify 0 | existing 0 | conflict 0 | skip 0").is_some());
    press(&mut h, "Import");
    let p = &h.state().session.model.parameters["Fire Rating"];
    assert_eq!((p.group.as_ref().map_or("", |u| u.label()), p.dictionary.as_deref()), ("Fire", Some("revit")));
    assert!(h.query_by_label_contains("Shared: revit").is_some(), "a shared parameter in the one table");
}

#[test]
fn a_dictionary_is_made_filled_and_taken_from_in_the_window() {
    let path = temp("office.bimparams");
    let mut h = app(BOX);
    h.state_mut().run("parameter", json!({"name": "Fire Rating", "categories": ["wall"]}));
    open(&mut h);
    pick(&mut h, Role::CheckBox, "Check Fire Rating");
    press(&mut h, "Dictionaries");
    h.state_mut().parameters_open_dictionary(&path);
    h.run();
    assert!(std::path::Path::new(&path).exists());
    press(&mut h, "Edit");
    press(&mut h, "Add Checked Project Parameters");
    let read = bimcraft_core::parameters::table::read_dictionary_file(&path).unwrap();
    assert_eq!(read.parameters[0].name, "Fire Rating");
    assert_eq!(read.parameters[0].guid, h.state().session.model.library.guid(&bimcraft_core::library::Kind::Parameter, "Fire Rating"));

    // Into another project, from the dictionary's own table.
    let mut other = app(BOX);
    open(&mut other);
    press(&mut other, "Dictionaries");
    other.state_mut().parameters_open_dictionary(&path);
    other.run();
    pick(&mut other, Role::CheckBox, "Check Fire Rating");
    press(&mut other, "Add Checked to Project");
    press(&mut other, "Import");
    let m = &other.state().session.model;
    assert_eq!(m.parameters["Fire Rating"].dictionary.as_deref(), Some("office"));
    assert_eq!(m.library.guid(&bimcraft_core::library::Kind::Parameter, "Fire Rating"), read.parameters[0].guid);
    press(&mut other, "Parameters");
    assert!(other.query_by_label("Shared: office").is_some(), "the table says so");
}

#[test]
fn a_table_is_exported_and_a_familys_parameters_are_transferred() {
    let mut h = app(BOX);
    h.state_mut().run("parameter", json!({"name": "Fire Rating", "categories": ["wall"]}));
    h.state_mut().run("parameter", json!({"name": "Other", "categories": ["wall"]}));
    h.state_mut().run("family", json!({"name": "Aaa", "category": "furniture", "parameters": [{"name": "Reach", "kind": "length", "value": 10.0}], "types": {"T": {}}}));
    h.state_mut().run("family", json!({"name": "Bbb", "category": "furniture", "types": {"T": {}}}));
    open(&mut h);
    pick(&mut h, Role::CheckBox, "Check Fire Rating");
    let path = temp("out.csv");
    h.state_mut().parameters_export_to(&path);
    h.run();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("Fire Rating") && !text.contains("Other"), "what's checked: {text}");
    press(&mut h, "Transfer");
    h.get_by_role_and_label(Role::ComboBox, "Family").click();
    h.run();
    press(&mut h, "Aaa");
    pick(&mut h, Role::CheckBox, "Check Reach");
    pick(&mut h, Role::CheckBox, "Transfer to Bbb");
    press(&mut h, "Transfer Checked");
    assert!(h.state().session.model.families["Bbb"].parameters.iter().any(|p| p.name == "Reach"));
    assert!(h.query_by_label("1 added, 0 skipped: their names are taken.").is_some());
}

fn pick_in(h: &mut Harness<BimApp>, combo: &str, option: &str) {
    h.get_by_role_and_label(Role::ComboBox, combo).click();
    h.run();
    press(h, option);
}

#[test]
fn a_parameter_made_a_type_s_is_wiped_off_elements_and_set_in_type_properties() {
    use bimcraft_core::model::Scope;
    let mut h = app(BOX);
    let wall = ids(&h, "wall")[0];
    h.state_mut().run("parameter", json!({"name": "Rating", "categories": ["wall"]}));
    h.state_mut().run("modify", json!({"id": wall, "params": {"Rating": "mine"}}));
    h.run();
    open(&mut h);
    pick_in(&mut h, "Instance or Type of Rating", "Type");
    assert!(h.query_by_label("Apply wipes the values 1 elements have of Rating.").is_some(), "says so before it happens");
    assert_eq!(h.state().session.model.parameters["Rating"].scope, Scope::Instance, "staged until Apply");
    press(&mut h, "Apply");
    assert_eq!(h.state().session.model.parameters["Rating"].scope, Scope::Type);
    assert_eq!(field(&h, wall, "params"), Value::Null, "wiped");
    press(&mut h, "Close");

    click(&mut h, P2(4000.0, 0.0));
    assert!(h.query_by_role_and_label(Role::TextInput, "Rating").is_none(), "not an element's to set in Properties");
    press(&mut h, "Edit Type");
    type_role(&mut h, Role::TextInput, "Rating", "EI60");
    press(&mut h, "OK");
    assert_eq!(bimcraft_core::schedule::field_text(&h.state().session.model, wall, "Rating").as_deref(), Some("EI60"));
    key(&mut h, Key::Escape);

    // And back: each type's value comes down to its elements.
    open(&mut h);
    pick_in(&mut h, "Instance or Type of Rating", "Instance");
    assert!(h.query_by_label("Apply copies each type's value of Rating to its elements.").is_some());
    press(&mut h, "Apply");
    assert_eq!(field(&h, wall, "params"), json!({"Rating": "EI60"}));
}

#[test]
fn many_categories_break_onto_new_lines_and_the_window_stays_its_width() {
    let mut h = app(BOX);
    let all: Vec<&str> = bimcraft_core::model::PARAMETER_CATEGORIES.to_vec();
    h.state_mut().run("parameter", json!({"name": "Few", "categories": ["wall"]}));
    open(&mut h);
    let right = |h: &Harness<BimApp>| h.get_by_role_and_label(Role::Button, "Close").rect().right();
    let before = right(&h);
    press(&mut h, "Close");
    h.state_mut().run("parameter", json!({"name": "Many", "categories": all}));
    h.state_mut().run("parameter", json!({"name": "After", "categories": ["wall"]}));
    h.state_mut().run("parameter", json!({"name": "Zed", "categories": ["wall"]}));
    open(&mut h);
    press(&mut h, "Categories");
    for _ in 0..4 {
        h.run();
    }
    assert!((right(&h) - before).abs() < 2.0, "the window keeps its width: {} against {before}", right(&h));
    // A row is as tall as its text, and the next starts under it.
    let row = |h: &Harness<BimApp>, n: &str| h.get_by_label(n).rect();
    let walls = h.get_by_label_contains("Beams, Cables").rect();
    assert!(walls.height() > row(&h, "After").height() * 2.0, "it breaks onto new lines: {walls:?}");
    assert!(row(&h, "Zed").top() >= walls.bottom() - 1.0, "not over the next row: {:?} under {walls:?}", row(&h, "Zed"));
    // Its two sides don't stretch the window down either.
    let close = h.get_by_role_and_label(Role::Button, "Close").rect();
    assert!(close.bottom() < 720.0, "{close:?}");
    assert!(h.query_by_label("Name").is_some() && h.query_by_label("Type of Parameter").is_some(), "a table, with headers");
}

fn two_families(h: &mut Harness<BimApp>) {
    for name in ["Aaa", "Bbb"] {
        h.state_mut().run(
            "family",
            json!({"name": name, "category": "furniture", "parameters": [{"name": "Width", "value": 900}], "types": {"Small": {}, "Big": {"Width": 1800}},
                   "forms": [{"profile": [[0, 0], [100, 0], [100, 100], [0, 100]], "from": 0, "to": 100}]}),
        );
    }
}

#[test]
fn the_families_tab_adds_parameters_to_the_families_checked_in_bulk() {
    use bimcraft_core::family::Val;
    let mut h = app(BOX);
    two_families(&mut h);
    h.state_mut().run("parameter", json!({"name": "Code", "kind": "text", "categories": ["component"]}));
    open(&mut h);
    press(&mut h, "Families");
    pick(&mut h, Role::CheckBox, "Check family Aaa");
    pick(&mut h, Role::CheckBox, "Check family Bbb");
    press(&mut h, "Add Parameter");
    press(&mut h, "Family Parameter");
    assert!(h.query_by_label("2 parameters will be created in 2 families").is_some());
    type_role(&mut h, Role::TextInput, "Name of Parameter 1", "Mark");
    type_role(&mut h, Role::TextInput, "Group of Mark", "Identity");
    type_role(&mut h, Role::TextInput, "Value of Mark", "M1");
    // A shared one, tied to the project's.
    press(&mut h, "Add Parameter");
    press(&mut h, "From a Project Parameter ⏵");
    press(&mut h, "Code");
    type_role(&mut h, Role::TextInput, "Value of Code", "C1");
    assert!(h.query_by_label("4 parameters will be created in 2 families").is_some());
    press(&mut h, "Apply");
    let m = &h.state().session.model;
    for family in ["Aaa", "Bbb"] {
        let f = &m.families[family];
        for ty in ["Small", "Big"] {
            assert_eq!(f.values(ty, &Default::default()).unwrap()["Mark"], Val::Text("M1".into()), "{family} {ty}: on every type");
        }
        let code = f.parameters.iter().find(|q| q.name == "Code").unwrap();
        assert_eq!(code.shared, m.library.guid(&bimcraft_core::library::Kind::Parameter, "Code"));
    }
    assert!(h.query_by_label("0 parameters will be created in 2 families").is_some(), "the table is empty again");
    press(&mut h, "Clear");
    assert!(h.query_by_label("0 parameters will be created in 0 families").is_some());
}

#[test]
fn a_dictionary_s_parameter_is_edited_saved_as_a_revision_and_pushed_to_the_project() {
    use bimcraft_core::model::Scope;
    let path = temp("office.bimparams");
    let mut h = app(BOX);
    h.state_mut().run("dictionary", json!({"path": path, "add": [{"name": "Code", "kind": "text", "categories": ["wall"]}]}));
    h.state_mut().run("parameter_import", json!({"path": path}));
    h.run();
    open(&mut h);
    press(&mut h, "Dictionaries");
    assert!(h.query_by_label("1, up to date").is_some(), "the project has what the file has");
    h.state_mut().parameters_open_dictionary(&path);
    h.run();
    // Its categories, and instance or type, edited in the table.
    press(&mut h, "Categories of Code");
    pick(&mut h, Role::CheckBox, "Floors");
    key(&mut h, Key::Escape);
    pick_in(&mut h, "Instance or Type of Code", "Type");
    press(&mut h, "Save Dictionary");
    let read = bimcraft_core::parameters::table::read_dictionary_file(&path).unwrap();
    assert_eq!(read.revision, 2);
    assert_eq!(read.parameters[0].categories, vec!["floor", "wall"]);
    assert_eq!(read.parameters[0].scope, Some(Scope::Type));
    assert_eq!(h.state().session.model.parameters["Code"].categories, vec!["wall"], "the project is untouched");
    assert!(h.query_by_label("This project has 1, the file 2").is_some(), "and is told it's behind");

    press(&mut h, "Update Project");
    assert!(h.query_by_label("create 0 | modify 1 | existing 0 | conflict 0 | skip 0").is_some());
    press(&mut h, "Import");
    let p = &h.state().session.model.parameters["Code"];
    assert_eq!((p.categories.clone(), p.scope), (vec!["floor".to_string(), "wall".to_string()], Scope::Type));
    assert_eq!(h.state().session.model.dictionaries["office"].revision, 2);
}

#[test]
fn an_import_shows_instance_or_type_and_lets_it_be_picked_row_by_row_or_for_all() {
    use bimcraft_core::model::Scope;
    let path = temp("rows.csv");
    std::fs::write(&path, "Name,Kind,Categories\nOne,text,wall\nTwo,text,wall\nThree,text,wall\n").unwrap();
    let mut h = app(BOX);
    open(&mut h);
    h.state_mut().parameters_import_from(&path);
    h.run();
    pick_in(&mut h, "Instance or Type of One", "Type");
    press(&mut h, "Import");
    let scope = |h: &Harness<BimApp>, n: &str| h.state().session.model.parameters[n].scope;
    assert_eq!((scope(&h, "One"), scope(&h, "Two")), (Scope::Type, Scope::Instance));

    let all = temp("all.csv");
    std::fs::write(&all, "Name,Kind,Categories\nFour,text,wall\nFive,text,wall\n").unwrap();
    h.state_mut().parameters_import_from(&all);
    h.run();
    // The "Type" of Make All, the first of the name in the window's order.
    let nodes: Vec<_> = h.get_all_by_label("Type").collect();
    nodes.first().unwrap().click();
    h.run();
    press(&mut h, "Import");
    assert_eq!((scope(&h, "Four"), scope(&h, "Five")), (Scope::Type, Scope::Type));
}

#[test]
fn dictionaries_stay_loaded_after_the_window_closes_and_one_is_picked_from_a_dropdown() {
    let (one, two) = (temp("one.bimparams"), temp("two.bimparams"));
    let mut h = app(BOX);
    h.state_mut().run("dictionary", json!({"path": two, "add": [{"name": "Mark", "kind": "text"}]}));
    h.state_mut().run("parameter", json!({"name": "Rating", "categories": ["wall"]}));
    open(&mut h);
    press(&mut h, "Dictionaries");
    h.state_mut().parameters_open_dictionary(&one);
    h.run();
    h.state_mut().parameters_open_dictionary(&two);
    h.run();
    assert!(h.query_by_role_and_label(Role::TextInput, "Name of Mark").is_some(), "the one opened last is shown");

    // Closed and opened again, both are there, and the one in use is the one it was.
    press(&mut h, "Close");
    assert_eq!(h.state().session.model.dictionaries.len(), 2, "kept in the project");
    open(&mut h);
    press(&mut h, "Dictionaries");
    assert!(h.query_by_role_and_label(Role::TextInput, "Name of Mark").is_some());
    pick_in(&mut h, "Dictionary in use", "one");
    assert!(h.query_by_role_and_label(Role::TextInput, "Name of Mark").is_none(), "one has no such parameter");

    // The buttons went into Edit: the menu stays open while categories are ticked.
    press(&mut h, "Edit");
    press(&mut h, "Add Parameter");
    pick(&mut h, Role::CheckBox, "Check Parameter 1");
    assert!(h.query_by_label("Remove Checked").is_none(), "closed again");
    press(&mut h, "Edit");
    press(&mut h, "Categories of Checked ⏵");
    pick(&mut h, Role::CheckBox, "Walls");
    pick(&mut h, Role::CheckBox, "Floors");
    key(&mut h, Key::Escape);
    assert!(
        h.get_by_role_and_label(Role::ComboBox, "Dictionary in use").accesskit_node().is_disabled(),
        "not while there are changes to save"
    );
    press(&mut h, "Save Dictionary");
    let read = bimcraft_core::parameters::table::read_dictionary_file(&one).unwrap();
    assert_eq!(read.parameters[0].categories, vec!["floor", "wall"]);
    press(&mut h, "Edit");
    pick(&mut h, Role::CheckBox, "Check Parameter 1");
    press(&mut h, "Remove from Project");
    assert_eq!(h.state().session.model.dictionaries.keys().collect::<Vec<_>>(), ["two"]);
    assert!(h.query_by_role_and_label(Role::TextInput, "Name of Mark").is_some(), "the other is shown");
}
