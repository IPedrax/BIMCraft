//! Title blocks of the project's own: Manage > Title Blocks makes and edits them, and a sheet's Properties picks one and
//! fills in its sheet fields.

use super::*;

const SELO: &str = r#"title_block {"name": "CIVILTEC", "texts": [{"at": [4, 30], "text": "{CLIENTE}"}, {"at": [4, 10], "text": "{DESENHO}"}], "fields": [{"key": "CLIENTE", "label": "Cliente", "scope": "project"}, {"key": "DESENHO", "label": "Desenhista", "default": "B.R."}]}
sheet {"name": "Plantas", "title_block": "CIVILTEC"}
sheet {"name": "Cortes"}"#;

#[test]
fn a_sheets_properties_pick_its_title_block_and_fill_in_its_sheet_fields() {
    let mut h = app(&format!("{BOX}\n{SELO}"));
    let sheets = ids(&h, "sheet");
    h.state_mut().open_sheet(Sheet::Paper(sheets[0]));
    h.run();
    // Its sheet field, labelled, and not the project's.
    assert!(h.query_by_label("Desenhista").is_some(), "the sheet field shows under its label");
    assert!(h.query_by_label("Cliente").is_none(), "a project field isn't the sheet's to fill in");
    type_into(&mut h, "Desenhista", "M.S.");
    h.key_press(Key::Enter);
    h.run();
    let m = &h.state().session.model;
    assert_eq!(m.sheet(sheets[0]).unwrap().fields["DESENHO"], "M.S.");
    // The other sheet, on the built-in one, picks CIVILTEC.
    h.state_mut().open_sheet(Sheet::Paper(sheets[1]));
    h.run();
    assert!(h.query_by_label("Desenhista").is_none());
    assert_eq!(h.get_by_role_and_label(Role::ComboBox, "Title Block").value().as_deref(), Some("Built-in"));
    choose(&mut h, "Title Block", "CIVILTEC");
    assert_eq!(h.state().session.model.sheet(sheets[1]).unwrap().title_block, "CIVILTEC");
    assert!(h.query_by_label("Desenhista").is_some(), "its fields show once it has them");
    // Edit Title Blocks… opens the dialog on it.
    h.get_by_label("Edit Title Blocks…").click_accesskit();
    h.run();
    assert!(h.state().title_blocks_dialog.is_some());
}

#[test]
fn manage_title_blocks_makes_one_and_gives_the_projects_values() {
    let mut h = app(&format!("{BOX}\n{SELO}"));
    press(&mut h, "Manage");
    press(&mut h, "Title Blocks");
    assert!(h.state().title_blocks_dialog.is_some(), "Manage > Title Blocks opens the dialog");
    // A project field's value, typed in the dialog, is the project's on OK.
    {
        let d = h.state_mut().title_blocks_dialog.as_mut().unwrap();
        d.set_value("CLIENTE", "Henrique Güths");
    }
    press_near(&mut h, "Apply", "Cancel");
    assert_eq!(h.state().session.model.title_values["CLIENTE"], "Henrique Güths");
    // New makes another, the built-in one drawn as one; Add Field gives it a field.
    press(&mut h, "New");
    let m = &h.state().session.model;
    assert!(m.title_blocks.contains_key("Title Block 1"), "{:?}", m.title_blocks.keys().collect::<Vec<_>>());
    press(&mut h, "Add Field");
    press(&mut h, "OK");
    assert!(h.state().title_blocks_dialog.is_none(), "OK closes it");
    assert_eq!(h.state().session.model.title_blocks["Title Block 1"].fields.len(), 1);
    // One a sheet is drawn with can't go; the dialog says why and stays.
    press(&mut h, "Title Blocks");
    h.state_mut().open_title_blocks(Some("CIVILTEC"));
    h.run();
    press(&mut h, "Delete");
    assert!(h.state().session.model.title_blocks.contains_key("CIVILTEC"));
    assert!(h.state().title_blocks_dialog.as_ref().unwrap().error().is_some_and(|e| e.contains("sheet")));
}

#[test]
fn a_text_picked_on_the_preview_becomes_a_field_that_says_what_it_said() {
    let mut h = app(&format!(
        "{BOX}\n{SELO}\n{}",
        r#"title_block {"name": "CIVILTEC", "texts": [{"at": [4, 30], "text": "{CLIENTE}"}, {"at": [4, 10], "text": "{DESENHO}"}, {"at": [4, 20], "text": "Rua XV de Novembro, 1"}]}"#
    ));
    h.state_mut().open_title_blocks(Some("CIVILTEC"));
    h.run();
    h.state_mut().title_blocks_dialog.as_mut().unwrap().select(Some(2));
    h.run();
    assert!(h.query_by_label("Make Field").is_some(), "a plain text can be made a field");
    press(&mut h, "Make Field");
    press(&mut h, "OK");
    let tb = &h.state().session.model.title_blocks["CIVILTEC"];
    assert_eq!(tb.texts[2].text, "{RUA_XV_DE_NOVEMBRO_1}");
    assert_eq!(tb.field("RUA_XV_DE_NOVEMBRO_1").map(|f| f.default.as_str()), Some("Rua XV de Novembro, 1"));
}
