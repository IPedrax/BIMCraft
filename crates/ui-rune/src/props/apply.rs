//! What an edit of a palette row becomes: the command and its params, or why it is none. Every change to an element goes to the core as
//! `modify` (a view's crop as `crop`), nested through the field's dots and given the ids the palette shows, so it is checked,
//! rolled back when it breaks a rule and undone like any other change. This is the only place that makes those params.

use bimcraft_core::units::Units;
use serde_json::{Value, json};

use super::kind::{Kind, nested, parse};
use super::panel::{Editor, Panel, Row};
use crate::lang::{t, tf};

/// What was done to a row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edit<'a> {
    /// A field's text, as typed.
    Text(&'a str),
    Check(bool),
    /// An option of a list, by its place in it.
    Choice(usize),
}

/// A command and its params, for `Session::execute`.
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    pub name: &'static str,
    pub params: Value,
}

fn modify(panel: &Panel, key: &str, value: Value, also: Option<(&'static str, Value)>) -> Command {
    let mut params = nested(key, value);
    if let Some((k, v)) = also {
        params[k] = v;
    }
    params["ids"] = json!(panel.ids);
    Command { name: "modify", params }
}

/// The command that sets `row` to `value`: its own, given the view, for a view's graphics; else a `modify` of the shown elements.
fn set(panel: &Panel, row: &Row, value: Value, also: Option<(&'static str, Value)>) -> Command {
    match &row.via {
        Some(via) => {
            let mut params = json!({ "view": via.view });
            params[via.key] = value;
            Command { name: via.command, params }
        }
        None => modify(panel, row.key, value, also),
    }
}

/// The command for `edit` of row `key`: none when nothing changed (a field typed over with what it had), or why the edit is no value.
pub fn command(panel: &Panel, units: Units, key: &'static str, edit: Edit) -> Result<Option<Command>, String> {
    let row = panel.row(key).ok_or_else(|| tf("`{}` is not a field of what is selected", &[key]))?;
    match (&row.editor, edit) {
        (Editor::Text { .. }, Edit::Text(typed)) => {
            if typed.trim() == row.shown.trim() {
                return Ok(None);
            }
            let value = parse(row.kind, typed, units).map_err(|why| format!("{}: {why}", row.label))?;
            Ok(Some(set(panel, row, value, None)))
        }
        (Editor::Check(was), Edit::Check(on)) => {
            if *was == on {
                return Ok(None);
            }
            // A view's crop: on crops it to what it shows, off uncrops it.
            if row.kind == Kind::Crop {
                let [view] = panel.ids.as_slice() else { return Err(tf("`{}` is changed for one view at a time", &[&row.label])) };
                let params = if on { json!({"view": view}) } else { json!({"view": view, "region": null}) };
                return Ok(Some(Command { name: "crop", params }));
            }
            Ok(Some(set(panel, row, json!(on), None)))
        }
        (Editor::Choice { current, options }, Edit::Choice(i)) => {
            let pick = options.get(i).ok_or_else(|| tf("`{}` has no such choice", &[&row.label]))?;
            if *current == Some(i) {
                return Ok(None);
            }
            Ok(Some(set(panel, row, pick.value.clone(), pick.also.clone())))
        }
        _ => Err(tf("`{}` is not changed here", &[&row.label])),
    }
}

/// The command that gives the shown elements type `i` of the Type Selector's list (a family's too, for a type of a family; a door or a
/// window of one of the project's own is taken off its family): none when it is the type they have, or why it cannot be.
pub fn set_type(panel: &Panel, i: usize) -> Result<Option<Command>, String> {
    let selector = panel.selector.as_ref().ok_or_else(|| t("There is no type to pick for this").to_string())?;
    if !selector.enabled {
        return Err(t("Its type is its room's: change the room's").to_string());
    }
    let ty = selector.types.get(i).ok_or_else(|| t("There is no such type").to_string())?;
    if selector.current.as_deref() == Some(ty.label.as_str()) {
        return Ok(None);
    }
    let mut params = json!({"ids": panel.ids, "type": ty.name});
    match &ty.family {
        Some(family) => params["family"] = json!(family),
        None if selector.hosted => params["family"] = Value::Null,
        None => {}
    }
    Ok(Some(Command { name: "modify", params }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::props::panel::{Group, Pick, Row, Subject};

    fn panel(rows: Vec<Row>, ids: Vec<u64>) -> Panel {
        Panel {
            subject: Subject::Elements("wall"),
            title: String::new(),
            ids,
            categories: Vec::new(),
            groups: vec![Group { key: "Constraints", name: "Constraints".into(), rows }],
            notes: Vec::new(),
            selector: None,
        }
    }

    fn row(key: &'static str, kind: Kind, shown: &str, editor: Editor) -> Row {
        Row { key, label: key.to_string(), kind, shown: shown.to_string(), editor, via: None }
    }

    #[test]
    fn a_typed_field_becomes_one_modify_of_every_element_the_palette_shows() {
        let p = panel(vec![row("base_offset", Kind::Length, "0 mm", Editor::Text { hint: "" })], vec![100_001, 100_002]);
        let c = command(&p, Units::Millimetres, "base_offset", Edit::Text("250")).ok().flatten();
        assert_eq!(c, Some(Command { name: "modify", params: json!({"base_offset": 250.0, "ids": [100_001, 100_002]}) }));
        assert_eq!(command(&p, Units::Millimetres, "base_offset", Edit::Text(" 0 mm ")), Ok(None), "typed over with what it had");
        let why = command(&p, Units::Millimetres, "base_offset", Edit::Text("lots")).err().unwrap_or_default();
        assert!(why.starts_with("base_offset: ") && why.contains("lots"), "{why}");
        assert!(command(&p, Units::Millimetres, "nope", Edit::Text("1")).is_err());
        assert!(command(&p, Units::Millimetres, "base_offset", Edit::Check(true)).is_err(), "a field takes text");
    }

    #[test]
    fn a_dotted_field_is_nested_a_check_is_a_bool_and_a_choice_carries_its_second_field() {
        let options = vec![
            Pick { label: "Not Bounded".into(), value: Value::Null, also: Some(("limit_offset", json!(3000.0))) },
            Pick { label: "Level 2".into(), value: json!("Level 2"), also: None },
        ];
        let p = panel(
            vec![
                row("view_range.cut", Kind::Length, "1200 mm", Editor::Text { hint: "" }),
                row("room_bounding", Kind::Flag, "Yes", Editor::Check(true)),
                row("top_level", Kind::UpperLimit, "Level 2", Editor::Choice { current: Some(1), options }),
            ],
            vec![100_003],
        );
        let nested = command(&p, Units::Metres, "view_range.cut", Edit::Text("1.5")).ok().flatten();
        assert_eq!(nested.map(|c| c.params), Some(json!({"view_range": {"cut": 1500.0}, "ids": [100_003]})));
        let off = command(&p, Units::Millimetres, "room_bounding", Edit::Check(false)).ok().flatten();
        assert_eq!(off.map(|c| c.params), Some(json!({"room_bounding": false, "ids": [100_003]})));
        assert_eq!(command(&p, Units::Millimetres, "room_bounding", Edit::Check(true)), Ok(None));
        let none = command(&p, Units::Millimetres, "top_level", Edit::Choice(0)).ok().flatten();
        assert_eq!(none.map(|c| c.params), Some(json!({"top_level": null, "limit_offset": 3000.0, "ids": [100_003]})));
        assert_eq!(command(&p, Units::Millimetres, "top_level", Edit::Choice(1)), Ok(None), "the one in force");
        assert!(command(&p, Units::Millimetres, "top_level", Edit::Choice(9)).is_err());
    }

    #[test]
    fn a_crop_goes_through_crop_for_one_view_and_is_refused_for_several() {
        let one = panel(vec![row("crop", Kind::Crop, "No", Editor::Check(false))], vec![100_010]);
        let on = command(&one, Units::Millimetres, "crop", Edit::Check(true)).ok().flatten();
        assert_eq!(on, Some(Command { name: "crop", params: json!({"view": 100_010}) }));
        let was = panel(vec![row("crop", Kind::Crop, "Yes", Editor::Check(true))], vec![100_010]);
        let off = command(&was, Units::Millimetres, "crop", Edit::Check(false)).ok().flatten();
        assert_eq!(off.map(|c| c.params), Some(json!({"view": 100_010, "region": null})));
        let two = panel(vec![row("crop", Kind::Crop, "No", Editor::Check(false))], vec![100_010, 100_011]);
        assert!(command(&two, Units::Millimetres, "crop", Edit::Check(true)).is_err());
    }
}
