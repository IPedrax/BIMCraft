//! Revit's Filter, on a selection, and more: what is selected is counted by category, each to keep or leave out; and, by
//! any parameter the selected elements all have (a wall's Type, Base Constraint, Length; a door's Mark, Level), by the
//! values they hold. The parameters are the schedules' fields (`schedule::fields`): those every selected element's
//! category has, Category always first.

use std::collections::BTreeMap;

use crate::model::{Id, Model};
use crate::schedule::{self, Kind};

/// What every element has: its category. As a value, the category's key (`wall`, `door`, `detail_line`).
pub const CATEGORY: &str = "Category";

/// A value the selection holds, and the elements that hold it, in the order selected. An element with no value for the
/// parameter holds `""`.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub value: String,
    pub ids: Vec<Id>,
}

/// The parameters every element of `ids` has, with what each holds: Category first, then the fields their categories share,
/// in the first category's order. A count of one each is no parameter to filter by, so Count is left out.
pub fn common(m: &Model, ids: &[Id]) -> Vec<(String, Kind)> {
    let mut cats: Vec<&str> = ids.iter().filter_map(|id| m.elements.get(id)).map(|e| e.category()).collect();
    cats.sort_unstable();
    cats.dedup();
    let mut out = vec![(CATEGORY.to_string(), Kind::Text)];
    let Some((first, rest)) = cats.split_first() else { return out };
    let others: Vec<Vec<(String, Kind)>> = rest.iter().map(|c| schedule::fields(m, c)).collect();
    out.extend(
        schedule::fields(m, first)
            .into_iter()
            .filter(|(n, _)| n != "Count" && n != CATEGORY)
            .filter(|f| others.iter().all(|o| o.contains(f))),
    );
    out
}

/// The values `by`, a parameter of every element of `ids`, takes, each with the elements holding it: in natural order
/// (numbers by value), the elements with none last. Category's values are categories' keys; the others, values as a
/// schedule shows them, in the project's units.
pub fn groups(m: &Model, ids: &[Id], by: &str) -> Vec<Group> {
    let mut found: BTreeMap<String, Vec<Id>> = BTreeMap::new();
    let rooms = crate::rooms::Rooms::new(m);
    for &id in ids {
        let Some(e) = m.elements.get(id) else { continue };
        let value = match by {
            CATEGORY => e.category().to_string(),
            _ => schedule::field_text_with(m, id, by, &rooms).unwrap_or_default(),
        };
        let held = found.entry(value).or_default();
        if !held.contains(&id) {
            held.push(id);
        }
    }
    let mut out: Vec<Group> = found.into_iter().map(|(value, ids)| Group { value, ids }).collect();
    out.sort_by(|a, b| a.value.is_empty().cmp(&b.value.is_empty()).then_with(|| schedule::natural_text(&a.value, &b.value)));
    out
}

/// What of `ids` holds one of the values `keep` of `by`: the selection the filter leaves, in the order selected.
pub fn kept(m: &Model, ids: &[Id], by: &str, keep: &[String]) -> Vec<Id> {
    let held: Vec<Id> = groups(m, ids, by).into_iter().filter(|g| keep.contains(&g.value)).flat_map(|g| g.ids).collect();
    let mut out: Vec<Id> = Vec::new();
    for id in ids {
        if held.contains(id) && !out.contains(id) {
            out.push(*id);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::engine::Session;

    /// Four walls, two of them of another type, and a door in the first.
    fn house() -> (Session, Vec<Id>, Id) {
        let mut s = Session::new();
        let r = s.execute("wall", &json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true})).unwrap();
        let walls: Vec<Id> = r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
        s.execute("type", &json!({"category": "wall", "name": "Thick", "thickness": 300})).unwrap();
        s.execute("modify", &json!({"ids": [walls[1], walls[3]], "type": "Thick"})).unwrap();
        let door = s.execute("door", &json!({"wall": walls[0], "offset": 2000})).unwrap()["id"].as_u64().unwrap();
        (s, walls, door)
    }

    #[test]
    fn a_selection_counts_by_category_and_keeps_what_is_checked() {
        let (s, walls, door) = house();
        let mut ids = walls.clone();
        ids.insert(1, door);
        let g = groups(&s.model, &ids, CATEGORY);
        assert_eq!(g.iter().map(|g| (g.value.as_str(), g.ids.len())).collect::<Vec<_>>(), [("door", 1), ("wall", 4)]);
        assert_eq!(kept(&s.model, &ids, CATEGORY, &["wall".into()]), walls);
        assert_eq!(kept(&s.model, &ids, CATEGORY, &[]), Vec::<Id>::new());
    }

    #[test]
    fn walls_share_their_parameters_and_are_kept_by_a_value() {
        let (s, walls, door) = house();
        let fields: Vec<String> = common(&s.model, &walls).into_iter().map(|(n, _)| n).collect();
        assert_eq!(fields[0], CATEGORY);
        assert!(fields.iter().any(|f| f == "Type") && fields.iter().any(|f| f == "Length"), "{fields:?}");
        assert!(!fields.iter().any(|f| f == "Count"), "{fields:?}");
        let g = groups(&s.model, &walls, "Type");
        assert_eq!(g.iter().map(|g| (g.value.as_str(), g.ids.len())).collect::<Vec<_>>(), [("Generic - 200mm", 2), ("Thick", 2)]);
        assert_eq!(kept(&s.model, &walls, "Type", &["Thick".into()]), [walls[1], walls[3]]);
        // A door and walls share what both categories have, and only that.
        let mixed: Vec<String> = common(&s.model, &[walls[0], door]).into_iter().map(|(n, _)| n).collect();
        assert!(mixed.iter().any(|f| f == "Type") && !mixed.iter().any(|f| f == "Length"), "{mixed:?}");
        assert_eq!(common(&s.model, &[]), [(CATEGORY.to_string(), Kind::Text)]);
    }

    #[test]
    fn lengths_go_by_value_and_what_has_none_comes_last() {
        let (s, walls, door) = house();
        let lengths: Vec<String> = groups(&s.model, &walls, "Length").into_iter().map(|g| g.value).collect();
        assert_eq!(lengths.len(), 2, "two lengths, each twice: {lengths:?}");
        let marks = groups(&s.model, &[door, walls[0]], "Mark");
        assert_eq!(marks.last().map(|g| g.value.as_str()), Some(""), "{marks:?}");
    }

    #[test]
    fn annotation_has_its_parameters_too_a_line_its_line_style() {
        let mut s = Session::new();
        let line = |s: &mut Session, style: &str| {
            s.execute("detail_line", &json!({"start": [0, 0], "end": [1000, 0], "style": style})).unwrap()["id"].as_u64().unwrap()
        };
        let lines = [line(&mut s, "Thin Lines"), line(&mut s, "Wide Lines"), line(&mut s, "Thin Lines")];
        let fields: Vec<String> = common(&s.model, &lines).into_iter().map(|(n, _)| n).collect();
        assert!(fields.iter().any(|f| f == "Line Style") && fields.iter().any(|f| f == "Length"), "{fields:?}");
        assert_eq!(kept(&s.model, &lines, "Line Style", &["Wide Lines".into()]), [lines[1]]);
        let sketch = json!([[[0, 0], [1000, 0]], [[1000, 0], [1000, 1000]], [[1000, 1000], [0, 0]]]);
        let region = s.execute("filled_region", &json!({"sketch": sketch})).unwrap()["id"].as_u64().unwrap();
        let g = groups(&s.model, &[region], "Type");
        assert_eq!(g.iter().map(|g| g.value.as_str()).collect::<Vec<_>>(), ["Solid Black"]);
        let text = s.execute("text", &json!({"at": [0, 0], "text": "hi"})).unwrap()["id"].as_u64().unwrap();
        assert_eq!(groups(&s.model, &[text], "Text")[0].value, "hi");
    }

    #[test]
    fn the_command_lists_the_groups_and_what_is_kept_and_changes_nothing() {
        let (mut s, walls, door) = house();
        let (steps, before) = (s.history().len(), s.model.clone());
        let ids = [walls[0], walls[1], door];
        let r = s.execute("filter_selection", &json!({"ids": ids})).unwrap();
        assert_eq!(r["by"], CATEGORY);
        assert_eq!(
            r["groups"],
            json!([{"value": "door", "count": 1, "ids": [door]}, {"value": "wall", "count": 2, "ids": [walls[0], walls[1]]}])
        );
        assert_eq!(r["fields"][0], CATEGORY);
        assert!(r.get("ids").is_none(), "nothing kept unless asked");
        let r = s.execute("filter_selection", &json!({"ids": ids, "by": "Type", "keep": ["Thick"]})).unwrap();
        assert_eq!(r["ids"], json!([walls[1]]));
        for bad in [
            json!({}),
            json!({"ids": ids, "by": "Nonsense"}),
            json!({"ids": ids, "keep": "wall"}),
            json!({"ids": ids, "keep": [5]}),
            json!({"ids": [5]}),
        ] {
            assert!(s.execute("filter_selection", &bad).is_err(), "{bad}");
        }
        assert_eq!((s.history().len(), &s.model), (steps, &before), "reading changes nothing and is no undo step");
    }
}
