//! Title blocks of the project's own: View > Title Blocks makes and edits them, and a sheet's Properties picks one and
//! fills in its sheet fields.

use super::*;

const SELO: &str = r#"title_block {"name": "ESCRITORIO", "texts": [{"at": [4, 30], "text": "{CLIENTE}"}, {"at": [4, 10], "text": "{DESENHO}"}], "fields": [{"key": "CLIENTE", "label": "Cliente", "scope": "project"}, {"key": "DESENHO", "label": "Desenhista", "default": "A.B."}]}
sheet {"name": "Plantas", "title_block": "ESCRITORIO"}
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
    // The other sheet, on the built-in one, picks ESCRITORIO.
    h.state_mut().open_sheet(Sheet::Paper(sheets[1]));
    h.run();
    assert!(h.query_by_label("Desenhista").is_none());
    assert_eq!(h.get_by_role_and_label(Role::ComboBox, "Title Block").value().as_deref(), Some("Built-in"));
    choose(&mut h, "Title Block", "ESCRITORIO");
    assert_eq!(h.state().session.model.sheet(sheets[1]).unwrap().title_block, "ESCRITORIO");
    assert!(h.query_by_label("Desenhista").is_some(), "its fields show once it has them");
    // Edit Title Blocks… opens the dialog on it.
    h.get_by_label("Edit Title Blocks…").click_accesskit();
    h.run();
    assert!(h.state().title_blocks_dialog.is_some());
}

#[test]
fn manage_title_blocks_makes_one_and_gives_the_projects_values() {
    let mut h = app(&format!("{BOX}\n{SELO}"));
    press(&mut h, "View");
    press(&mut h, "Title Blocks");
    assert!(h.state().title_blocks_dialog.is_some(), "View > Title Blocks opens the dialog");
    // A project field's value, typed in the dialog, is the project's on OK.
    {
        let d = h.state_mut().title_blocks_dialog.as_mut().unwrap();
        d.set_value("CLIENTE", "Maria Conceição");
    }
    press_near(&mut h, "Apply", "Cancel");
    assert_eq!(h.state().session.model.title_values["CLIENTE"], "Maria Conceição");
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
    h.state_mut().open_title_blocks(Some("ESCRITORIO"));
    h.run();
    press(&mut h, "Delete");
    assert!(h.state().session.model.title_blocks.contains_key("ESCRITORIO"));
    assert!(h.state().title_blocks_dialog.as_ref().unwrap().error().is_some_and(|e| e.contains("sheet")));
}

#[test]
fn a_text_picked_on_the_preview_becomes_a_field_that_says_what_it_said() {
    let mut h = app(&format!(
        "{BOX}\n{SELO}\n{}",
        r#"title_block {"name": "ESCRITORIO", "texts": [{"at": [4, 30], "text": "{CLIENTE}"}, {"at": [4, 10], "text": "{DESENHO}"}, {"at": [4, 20], "text": "Rua XV de Novembro, 1"}]}"#
    ));
    h.state_mut().open_title_blocks(Some("ESCRITORIO"));
    h.run();
    h.state_mut().title_blocks_dialog.as_mut().unwrap().select(Some(2));
    h.run();
    assert!(h.query_by_label("Make Field").is_some(), "a plain text can be made a field");
    press(&mut h, "Make Field");
    press(&mut h, "OK");
    let tb = &h.state().session.model.title_blocks["ESCRITORIO"];
    assert_eq!(tb.texts[2].text, "{RUA_XV_DE_NOVEMBRO_1}");
    assert_eq!(tb.field("RUA_XV_DE_NOVEMBRO_1").map(|f| f.default.as_str()), Some("Rua XV de Novembro, 1"));
}

#[test]
fn each_text_in_the_texts_tab_is_made_a_field_and_back_showing_what_it_says() {
    let mut h = app(&format!(
        "{BOX}\n{SELO}\n{}",
        r#"title_block {"name": "ESCRITORIO", "texts": [{"at": [4, 30], "text": "{CLIENTE}"}, {"at": [4, 10], "text": "{DESENHO}"}, {"at": [4, 20], "text": "Planta Baixa Térreo"}]}"#
    ));
    h.state_mut().open_title_blocks(Some("ESCRITORIO"));
    h.run();
    press(&mut h, "Texts");
    // The one plain text has the button; a field's row says what it says, its key beside it.
    press(&mut h, "Make Field");
    assert!(h.query_by_label("{PLANTA_BAIXA_TERREO}").is_some(), "its key shows beside it");
    assert!(h.query_by_label("Make Field").is_none());
    press(&mut h, "OK");
    let tb = &h.state().session.model.title_blocks["ESCRITORIO"];
    assert_eq!(tb.texts[2].text, "{PLANTA_BAIXA_TERREO}");
    assert_eq!(
        tb.field("PLANTA_BAIXA_TERREO").map(|f| (f.scope, f.default.as_str())),
        Some((bimcraft_core::titleblock::Scope::Sheet, "Planta Baixa Térreo"))
    );
    // Make Text puts back what it said; the field goes with it.
    h.state_mut().open_title_blocks(Some("ESCRITORIO"));
    h.run();
    press(&mut h, "Texts");
    let n = h.query_all_by_label("Make Text").count();
    assert_eq!(n, 3, "each field's row offers it");
    h.query_all_by_label("Make Text").nth(2).unwrap().click_accesskit();
    h.run();
    press(&mut h, "OK");
    let tb = &h.state().session.model.title_blocks["ESCRITORIO"];
    assert_eq!(tb.texts[2].text, "Planta Baixa Térreo");
    assert!(tb.field("PLANTA_BAIXA_TERREO").is_none());
}

#[test]
fn tb_opens_view_title_blocks_and_ks_lists_it() {
    let mut h = app(&format!("{BOX}\n{SELO}"));
    shortcut(&mut h, "TB");
    assert!(h.state().title_blocks_dialog.is_some(), "TB opens View > Title Blocks, as its tooltip says");
    assert!(crate::shortcuts().contains(&("TB", "Title Blocks")), "and Keyboard Shortcuts (KS) lists it");
}

/// The same flow in BIMCraft's own profile: View > Title Blocks is a row of the Tools palette, and a sheet's title block
/// is picked in the Inspector, as Rev's Properties picks it.
#[test]
fn the_tools_palette_opens_title_blocks_and_the_inspector_picks_a_sheets() {
    let mut h = bimcraft(&format!("{BOX}\n{SELO}"));
    assert!(h.query_by_label("Title Blocks").is_none(), "the palette shows its first tab's tools");
    let view = h.get_all_by_label("View").map(|n| n.rect()).find(|r| r.left() < 60.0).unwrap();
    click_at(&mut h, view.center());
    assert_eq!(h.state().tab, Tab::View);
    press(&mut h, "Title Blocks");
    assert!(h.state().title_blocks_dialog.is_some(), "the palette's Title Blocks opens the dialog");
    // A project field's value, typed in the dialog, is the project's on Apply; New and Add Field make another.
    h.state_mut().title_blocks_dialog.as_mut().unwrap().set_value("CLIENTE", "Maria Conceição");
    press_near(&mut h, "Apply", "Cancel");
    assert_eq!(h.state().session.model.title_values["CLIENTE"], "Maria Conceição");
    press(&mut h, "New");
    press(&mut h, "Add Field");
    press(&mut h, "OK");
    assert!(h.state().title_blocks_dialog.is_none(), "OK closes it");
    assert_eq!(h.state().session.model.title_blocks["Title Block 1"].fields.len(), 1);
    // TB opens it here too.
    shortcut(&mut h, "TB");
    assert!(h.state().title_blocks_dialog.is_some(), "TB in BIMCraft's profile");
    press(&mut h, "Cancel");
    // The sheet on the built-in one picks ESCRITORIO in the Inspector, and fills in its sheet field there.
    let sheets = ids(&h, "sheet");
    h.state_mut().open_sheet(Sheet::Paper(sheets[1]));
    h.run();
    assert_eq!(h.get_by_role_and_label(Role::ComboBox, "Title Block").value().as_deref(), Some("Built-in"));
    choose(&mut h, "Title Block", "ESCRITORIO");
    assert_eq!(h.state().session.model.sheet(sheets[1]).unwrap().title_block, "ESCRITORIO");
    assert!(h.query_by_label("Cliente").is_none(), "a project field isn't the sheet's to fill in");
    type_into(&mut h, "Desenhista", "M.S.");
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().session.model.sheet(sheets[1]).unwrap().fields["DESENHO"], "M.S.");
    h.get_by_label("Edit Title Blocks…").click_accesskit();
    h.run();
    assert!(h.state().title_blocks_dialog.is_some(), "Edit Title Blocks… opens the dialog from the Inspector");
}
