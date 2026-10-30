//! View Templates: the list by kind of view, New from This View, Duplicate, Rename and Delete, the settings drafted until OK,
//! Visibility/Graphics on a template, and the template a view follows picked from its Properties.

use super::*;
use bimcraft_core::graphics::{Detail, Group, ViewKind};

/// The last of the widgets of `role` named `label`: the dialog's, drawn over the Properties palette's of the same name.
fn last<'h>(h: &'h Harness<BimApp>, role: Role, label: &'h str) -> egui_kittest::Node<'h> {
    h.get_all_by(move |n| n.role() == role && n.label().as_deref() == Some(label)).last().unwrap()
}

/// Pick `option` from the dialog's drop-down named `label`.
fn choose_last(h: &mut Harness<BimApp>, label: &str, option: &str) {
    last(h, Role::ComboBox, label).click_accesskit();
    h.run();
    h.get_all_by(|n| n.label().as_deref() == Some(option)).last().unwrap().click_accesskit();
    h.run();
    if egui::Popup::is_any_open(&h.ctx) {
        last(h, Role::ComboBox, label).click_accesskit();
        h.run();
    }
}

fn template(h: &Harness<BimApp>, name: &str) -> bimcraft_core::graphics::ViewTemplate {
    h.state().session.model.view_templates[name].clone()
}

#[test]
fn a_template_is_made_from_a_view_listed_by_its_kind_and_its_settings_wait_for_ok() {
    let mut h = app(&format!("{BOX}\nroom {{\"point\": [4000, 3000], \"name\": \"Living\"}}"));
    shortcut(&mut h, "VV");
    pick(&mut h, Role::CheckBox, "Rooms");
    press(&mut h, "OK");
    shortcut(&mut h, "VT");
    assert!(h.query_by_label("No Plan templates yet: New from This View makes one from a view of the kind.").is_some());
    // One place makes one: from the view open, under the name typed.
    press(&mut h, "New from This View…");
    type_into(&mut h, "Name:", "Presentation");
    press(&mut h, "Create");
    assert!(template(&h, "Presentation").graphics.hides("room"));
    assert_eq!(template(&h, "Presentation").kind, Some(ViewKind::Plan));
    // The 3D tab lists 3D templates alone, and a plan can't make one.
    press(&mut h, "3D");
    assert!(h.query_by_label("Presentation").is_none());
    assert!(is_disabled(&h, Role::Button, "New from This View…"));
    press(&mut h, "Plan");
    press(&mut h, "Presentation");
    // What it holds and what it controls, edited in the table: nothing is the project's until OK, then one command.
    let steps = h.state().session.history().len();
    pick(&mut h, Role::CheckBox, "View Range");
    choose_last(&mut h, "Detail Level", "Coarse");
    choose_last(&mut h, "Phase", "Existing");
    assert!(template(&h, "Presentation").controls.contains(&Group::Range));
    assert_eq!(template(&h, "Presentation").graphics.detail, Detail::Fine);
    press(&mut h, "OK");
    assert!(h.state().templates_dialog.is_none());
    let t = template(&h, "Presentation");
    assert!(!t.controls.contains(&Group::Range));
    assert_eq!((t.graphics.detail, t.graphics.phase.as_deref()), (Detail::Coarse, Some("Existing")));
    assert_eq!(h.state().session.history().len(), steps + 1);
    // Cancel leaves it as it was.
    shortcut(&mut h, "VT");
    pick(&mut h, Role::CheckBox, "Categories");
    press(&mut h, "Cancel");
    assert!(template(&h, "Presentation").controls.contains(&Group::Categories));
}

#[test]
fn duplicate_rename_and_delete_act_on_the_template_picked() {
    let mut h = app(BOX);
    h.state_mut().session.execute("template", &json!({"name": "Plans", "from": 100000, "detail": "coarse"})).unwrap();
    h.state_mut().session.execute("graphics", &json!({"view": 100001, "template": "Plans"})).unwrap();
    shortcut(&mut h, "VT");
    // Duplicate carries what the dialog has, unsaved edits too.
    choose_last(&mut h, "Detail Level", "Medium");
    press(&mut h, "Duplicate…");
    type_into(&mut h, "Name:", "Copy");
    press(&mut h, "Create");
    assert_eq!(template(&h, "Copy").kind, Some(ViewKind::Plan));
    assert_eq!(last(&h, Role::ComboBox, "Detail Level").value().as_deref(), Some("Medium"));
    // Rename: Level 2 follows it under its new name.
    press(&mut h, "Plans");
    press(&mut h, "Rename…");
    type_into(&mut h, "Name:", "Working");
    press(&mut h, "Rename");
    assert!(!h.state().session.model.view_templates.contains_key("Plans"));
    assert_eq!(plan_graphics(&h, 100001).template.as_deref(), Some("Working"));
    press(&mut h, "OK");
    assert_eq!((template(&h, "Working").graphics.detail, template(&h, "Copy").graphics.detail), (Detail::Medium, Detail::Medium));
    // Delete: gone, and its views keep their own settings.
    shortcut(&mut h, "VT");
    press(&mut h, "Working");
    press(&mut h, "Delete");
    assert!(!h.state().session.model.view_templates.contains_key("Working"));
    assert!(plan_graphics(&h, 100001).template.is_none());
    press(&mut h, "Cancel");
}

#[test]
fn visibility_graphics_edits_a_templates_draft_without_the_elements_tab() {
    let mut h = app(BOX);
    h.state_mut().session.execute("template", &json!({"name": "T", "from": 100000})).unwrap();
    shortcut(&mut h, "VT");
    press(&mut h, "Edit V/G Overrides");
    assert!(h.query_by_label("Visibility/Graphics Overrides for View Template: T").is_some());
    assert!(h.query_by_role_and_label(Role::Button, "Elements").is_none(), "an element is a view's own");
    pick(&mut h, Role::CheckBox, "Walls");
    press(&mut h, "OK");
    // Back in View Templates: the template's draft, sent with its OK.
    assert!(h.state().templates_dialog.is_some() && h.state().vg_dialog.is_none());
    assert!(!template(&h, "T").graphics.hides("wall"));
    press(&mut h, "OK");
    assert!(template(&h, "T").graphics.hides("wall"));
}

#[test]
fn properties_view_template_is_a_button_that_picks_the_views_template_in_the_same_dialog() {
    let mut h = app(BOX);
    h.state_mut().session.execute("template", &json!({"name": "Plans", "from": 100000, "detail": "coarse"})).unwrap();
    topmost(&mut h, "Level 2");
    // As wide as the drop-downs above it, as Edit… is.
    let width = |h: &Harness<BimApp>, role, label| h.get_by_role_and_label(role, label).rect().width();
    let combo = width(&h, Role::ComboBox, "View Scale");
    assert_eq!(width(&h, Role::Button, "View Template"), combo);
    assert_eq!(width(&h, Role::Button, "Edit Visibility/Graphics Overrides"), combo);
    assert_eq!(h.get_by_role_and_label(Role::Button, "View Template").value().as_deref(), Some("<None>"));
    h.get_by_role_and_label(Role::Button, "View Template").click_accesskit();
    h.run();
    assert!(h.query_by_label("Assign View Template").is_some());
    press(&mut h, "Plans");
    press(&mut h, "OK");
    assert_eq!(plan_graphics(&h, 100001).template.as_deref(), Some("Plans"));
    assert_eq!(h.get_by_role_and_label(Role::Button, "View Template").value().as_deref(), Some("Plans"));
    assert!(is_disabled(&h, Role::ComboBox, "Detail Level"), "the template sets it");
    // <None> again: the view's own settings.
    h.get_by_role_and_label(Role::Button, "View Template").click_accesskit();
    h.run();
    press(&mut h, "<None>");
    press(&mut h, "OK");
    assert!(plan_graphics(&h, 100001).template.is_none());
    assert!(!is_disabled(&h, Role::ComboBox, "Detail Level"));
}
