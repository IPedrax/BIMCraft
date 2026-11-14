//! Type Properties, one look for every kind of type: Family and Type with Duplicate and Rename, the parameters in a table
//! under the bars of their groups, OK, Cancel and Apply; the same size for a wall's type, a family's, a text type and a filled
//! region type.

use super::*;

const CHAIR: &str = r#"family {"name": "Chair", "category": "furniture", "parameters": [{"name": "Seat", "kind": "length", "value": 450}], "types": {"Small": {}, "Large": {}}, "forms": [{"plane": "xy", "profile": [[-200, -200], [200, -200], [200, 200], [-200, 200]], "to": 450}]}
component {"family": "Chair", "type": "Small", "at": [2000, 2000]}
text {"text": "Note", "at": [5000, 3000]}
filled_region {"sketch": [[[1000,4000],[2000,4000]],[[2000,4000],[2000,5000]],[[2000,5000],[1000,4000]]]}"#;

/// The dialog `id` is open, where on the screen.
fn area(h: &Harness<BimApp>, id: &str) -> egui::Rect {
    h.ctx.memory(|m| m.area_rect(egui::Id::new(id))).unwrap_or_else(|| panic!("{id} is not open"))
}

#[test]
fn every_kind_of_type_opens_in_one_dialog_of_one_size_with_family_type_duplicate_and_rename() {
    let mut h = app(&format!("{BOX}\n{CHAIR}"));
    let m = h.state().session.model.clone();
    let opened = [
        ("type-properties", "wall"),
        ("family-type-properties", "component"),
        ("text-type-properties", "text"),
        ("fill-patterns", "region"),
    ];
    let mut widths = Vec::new();
    for (id, category) in opened {
        let of = ids(&h, category)[0];
        h.state_mut().select_and_edit_type(of);
        h.run();
        for label in ["Type Properties", "Family:", "Type Parameters", "OK", "Cancel"] {
            assert!(h.query_by_label(label).is_some(), "{id}: no `{label}`");
        }
        for label in ["Duplicate…", "Rename…"] {
            assert!(h.query_by_role_and_label(Role::Button, label).is_some(), "{id}: no `{label}`");
        }
        widths.push(area(&h, id).width());
        h.key_press(Key::Escape);
        h.run();
        let s = h.state_mut();
        (s.type_dialog, s.family_dialog, s.text_type_dialog, s.fill_patterns_dialog) = (None, None, None, None);
        h.run();
    }
    assert!(widths.iter().all(|w| (w - widths[0]).abs() < 1.0), "one width: {widths:?}");
    assert_eq!(h.state().session.model.to_json().unwrap(), m.to_json().unwrap(), "looking changes nothing");
}

#[test]
fn rename_in_type_properties_renames_a_wall_type_and_a_family_type_and_what_is_of_them_follows() {
    let mut h = app(&format!("{BOX}\n{CHAIR}"));
    let wall = ids(&h, "wall")[0];
    h.state_mut().select_and_edit_type(wall);
    h.run();
    // The groups are bars that fold.
    assert!(h.query_by_role_and_label(Role::TextInput, "Width").is_some());
    h.get_by_label("Construction").click();
    h.run();
    assert!(h.query_by_role_and_label(Role::TextInput, "Width").is_none(), "folded");
    h.get_by_label("Construction").click();
    h.run();
    press(&mut h, "Rename…");
    type_into(&mut h, "New name", "Party Wall");
    key(&mut h, Key::Enter);
    press(&mut h, "OK");
    assert!(h.state().type_dialog.is_none(), "{}", h.state().status.text);
    assert_eq!(field(&h, wall, "type"), json!("Party Wall"));
    // A family's type: the component follows.
    let chair = ids(&h, "component")[0];
    h.state_mut().select_and_edit_type(chair);
    h.run();
    assert!(h.query_by_label("Dimensions").is_some(), "a length is a dimension");
    press(&mut h, "Rename…");
    type_into(&mut h, "New name", "Child");
    press(&mut h, "Rename");
    press(&mut h, "OK");
    assert_eq!(field(&h, chair, "type"), json!("Child"));
    // A name another type has is said in the dialog, which stays open.
    h.state_mut().select_and_edit_type(chair);
    h.run();
    press(&mut h, "Rename…");
    type_into(&mut h, "New name", "Large");
    press(&mut h, "Rename");
    assert!(h.query_all_by_label_contains("already").next().is_some());
    assert!(h.state().family_dialog.is_some());
}

impl BimApp {
    fn select_and_edit_type(&mut self, id: Id) {
        self.selection = vec![id];
        self.edit_type();
    }
}

#[test]
fn a_stair_type_shows_its_model_then_its_graphics_and_only_the_sizes_its_construction_uses() {
    let mut h = app(r#"stair {"start": [0, 0], "angle": 0, "type": "Monolithic 150mm"}"#);
    let of = ids(&h, "stair")[0];
    h.state_mut().select_and_edit_type(of);
    h.run();
    for label in ["Model", "Graphics", "Slab Thickness", "Walk Line Arrow", "Cut Mark", "Finish Edge Profile"] {
        assert!(h.query_by_label(label).is_some(), "no `{label}`");
    }
    // A monolithic stair has no stringers to size.
    assert!(h.query_by_label("Stringer Width").is_none());
    h.state_mut().type_dialog.as_mut().unwrap().set_construction("stringers");
    h.run();
    assert!(h.query_by_label("Stringer Width").is_some() && h.query_by_label("Slab Thickness").is_none());
    press(&mut h, "OK");
    let m = &h.state().session.model;
    assert_eq!(m.types.stair["Monolithic 150mm"].built(), bimcraft_core::model::Construction::Stringers);
}
