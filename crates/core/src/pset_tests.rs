//! What another program's property sets and classification references hold: each value kept under its set's name (a project
//! parameter remembers the set each category's values came in), the sets a type object holds reaching its occurrences, and a
//! classification system's codes kept as a project parameter of it; all of it written back where it came from. Written by hand.

use crate::engine::Session;
use crate::import::ifc;
use crate::model::{Classification, Element, Model};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// Four walls round a room and a door in the first.
fn house() -> Session {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           door {"at": [2000, 0]}"#,
    )
    .unwrap();
    s
}

/// The entity numbers of the lines of `class` in `file`.
fn of_class(file: &str, class: &str) -> Vec<String> {
    let at = format!("={class}(");
    file.lines().filter(|l| l.contains(&at)).filter_map(|l| l.split('=').next().map(String::from)).collect()
}

/// `file` with `extra` lines put in before its data ends.
fn with(file: &str, extra: &str) -> String {
    file.replacen("ENDSEC;\nEND-ISO", &format!("{extra}ENDSEC;\nEND-ISO"), 1)
}

/// The text of entity `n` (`#12`): what follows its `=`.
fn body<'a>(file: &'a str, n: &str) -> &'a str {
    let head = format!("{n}=");
    file.lines().find_map(|l| l.strip_prefix(&head)).unwrap_or_else(|| panic!("no {n}"))
}

/// Each property set related to entity `n`, by its name: the names of its properties.
fn sets_of(file: &str, n: &str) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for rel in file.lines().filter(|l| l.contains("=IFCRELDEFINESBYPROPERTIES(")) {
        let (related, set) = rel.split_once("$,$,$,(").unwrap().1.split_once("),").unwrap();
        if !related.split(',').any(|r| r == n) {
            continue;
        }
        let set = body(file, set.trim_end_matches(");"));
        let Some(set) = set.strip_prefix("IFCPROPERTYSET(") else { continue };
        let name = set.split(',').nth(2).unwrap().trim_matches('\'').to_string();
        let props = set.rsplit_once(",(").unwrap().1.trim_end_matches(';').trim_end_matches(')');
        let names = props.split(',').map(|p| body(file, p).split('\'').nth(1).unwrap_or("").to_string());
        out.entry(name).or_default().extend(names);
    }
    out
}

/// The first element of `category` in `m`: its values.
fn values<'a>(m: &'a Model, category: &str) -> &'a BTreeMap<String, Value> {
    m.elements.values().find(|e| e.category() == category).and_then(Element::params).unwrap()
}

#[test]
fn a_property_comes_back_from_ifc_in_the_set_it_came_in_and_goes_out_in_it_again() {
    let s = house();
    let file = crate::ifc::export(&s.model);
    let (walls, doors) = (of_class(&file, "IFCWALL"), of_class(&file, "IFCDOOR"));
    // The walls' and the door's `FireRating`, each in its own class's common set, and a maker's set on the walls.
    let file = with(
        &file,
        &format!(
            "#900001=IFCPROPERTYSINGLEVALUE('FireRating',$,IFCLABEL('EI 60'),$);
#900002=IFCPROPERTYSET('0aaaaaaaaaaaaaaaaaaaa1',$,'Pset_WallCommon',$,(#900001));
#900003=IFCRELDEFINESBYPROPERTIES('0aaaaaaaaaaaaaaaaaaaa2',$,$,$,({}),#900002);
#900004=IFCPROPERTYSINGLEVALUE('FireRating',$,IFCLABEL('EI 30'),$);
#900005=IFCPROPERTYSET('0aaaaaaaaaaaaaaaaaaaa3',$,'Pset_DoorCommon',$,(#900004));
#900006=IFCRELDEFINESBYPROPERTIES('0aaaaaaaaaaaaaaaaaaaa4',$,$,$,({}),#900005);
#900007=IFCPROPERTYSINGLEVALUE('Supplier',$,IFCLABEL('Acme'),$);
#900008=IFCPROPERTYSET('0aaaaaaaaaaaaaaaaaaaa5',$,'Acme_Product',$,(#900007));
#900009=IFCRELDEFINESBYPROPERTIES('0aaaaaaaaaaaaaaaaaaaa6',$,$,$,({}),#900008);
",
            walls.join(","),
            doors.join(","),
            walls.join(",")
        ),
    );
    let i = ifc(file.as_bytes(), "x").unwrap();
    let fire = &i.model.parameters["FireRating"];
    let sets: BTreeMap<String, String> =
        [("door", "Pset_DoorCommon"), ("wall", "Pset_WallCommon")].map(|(c, n)| (c.to_string(), n.to_string())).into();
    assert_eq!((&fire.categories, &fire.sets), (&vec!["door".to_string(), "wall".to_string()], &sets));
    assert_eq!(i.model.parameters["Supplier"].sets.get("wall").map(String::as_str), Some("Acme_Product"));
    assert_eq!(values(&i.model, "wall").get("FireRating"), Some(&json!("EI 60")));
    assert_eq!(values(&i.model, "door").get("FireRating"), Some(&json!("EI 30")));

    // Out again: in the sets they came in, not the project's own, and one common set an element.
    let again = crate::ifc::export(&i.model);
    let wall = of_class(&again, "IFCWALL").remove(0);
    let door = of_class(&again, "IFCDOOR").remove(0);
    let (wall_sets, door_sets) = (sets_of(&again, &wall), sets_of(&again, &door));
    assert_eq!(wall_sets.get("Pset_WallCommon"), Some(&vec!["FireRating".to_string()]), "{wall_sets:?}");
    assert_eq!(wall_sets.get("Acme_Product"), Some(&vec!["Supplier".to_string()]), "{wall_sets:?}");
    assert_eq!(door_sets.get("Pset_DoorCommon"), Some(&vec!["FireRating".to_string()]), "{door_sets:?}");
    assert!(!wall_sets.contains_key(crate::ifc::PARAMETERS), "{wall_sets:?}");
    // And in once more, the same.
    let back = ifc(again.as_bytes(), "x").unwrap();
    assert_eq!(back.model.parameters, i.model.parameters);
}

#[test]
fn a_set_the_export_writes_for_itself_holds_the_values_that_came_in_it() {
    // A phased project writes each wall's `Status` in its `Pset_WallCommon`: a value from that set goes in the same one.
    let mut s = house();
    run(&mut s, "phase", json!({"name": "Demolition", "before": "New Construction"}));
    let wall = s.model.elements.iter().find(|(_, e)| e.category() == "wall").map(|(id, _)| *id);
    run(&mut s, "modify", json!({"id": wall, "params": {"Phase Created": "New Construction"}}));
    let file = crate::ifc::export(&s.model);
    let walls = of_class(&file, "IFCWALL");
    let file = with(
        &file,
        &format!(
            "#900001=IFCPROPERTYSINGLEVALUE('AcousticRating',$,IFCLABEL('Rw 45'),$);
#900002=IFCPROPERTYSET('0aaaaaaaaaaaaaaaaaaaa1',$,'Pset_WallCommon',$,(#900001));
#900003=IFCRELDEFINESBYPROPERTIES('0aaaaaaaaaaaaaaaaaaaa2',$,$,$,({}),#900002);
",
            walls.join(",")
        ),
    );
    let i = ifc(file.as_bytes(), "x").unwrap();
    let again = crate::ifc::export(&i.model);
    let wall = of_class(&again, "IFCWALL").remove(0);
    let common: Vec<_> = again
        .lines()
        .filter(|l| l.contains("=IFCRELDEFINESBYPROPERTIES(") && l.contains(&format!("({wall})")))
        .filter(|l| {
            let set = l.rsplit_once(",#").unwrap().1.trim_end_matches(");");
            body(&again, &format!("#{set}")).contains("'Pset_WallCommon'")
        })
        .collect();
    assert_eq!(common.len(), 1, "one Pset_WallCommon: {common:?}");
    let mut names = sets_of(&again, &wall).remove("Pset_WallCommon").unwrap();
    names.sort();
    assert_eq!(names, ["AcousticRating", "Status"]);
}

#[test]
fn a_types_property_sets_reach_its_occurrences_where_they_hold_no_value_of_their_own() {
    let s = house();
    let file = crate::ifc::export(&s.model);
    let walls = of_class(&file, "IFCWALL");
    // The walls' type object holds a set; the first wall says otherwise of one of its values.
    let ty = of_class(&file, "IFCWALLTYPE").remove(0);
    let line = format!("{ty}={}", body(&file, &ty));
    // IfcWallType: GlobalId, OwnerHistory, Name, Description, ApplicableOccurrence, HasPropertySets, ...
    let mut args: Vec<String> = line.split(',').map(String::from).collect();
    args[5] = "(#900003)".into();
    let file = file.replace(&line, &args.join(","));
    let file = with(
        &file,
        &format!(
            "#900001=IFCPROPERTYSINGLEVALUE('Supplier',$,IFCLABEL('Acme'),$);
#900002=IFCPROPERTYSINGLEVALUE('Finish',$,IFCLABEL('Plaster'),$);
#900003=IFCPROPERTYSET('0aaaaaaaaaaaaaaaaaaaa1',$,'Acme_Product',$,(#900001,#900002));
#900004=IFCPROPERTYSINGLEVALUE('Finish',$,IFCLABEL('Tiles'),$);
#900005=IFCPROPERTYSET('0aaaaaaaaaaaaaaaaaaaa2',$,'Acme_Product',$,(#900004));
#900006=IFCRELDEFINESBYPROPERTIES('0aaaaaaaaaaaaaaaaaaaa3',$,$,$,({}),#900005);
",
            walls[0]
        ),
    );
    let i = ifc(file.as_bytes(), "x").unwrap();
    let mut finishes: Vec<(&str, &str)> = i
        .model
        .elements
        .values()
        .filter(|e| e.category() == "wall")
        .filter_map(Element::params)
        .map(|p| (p.get("Supplier").and_then(Value::as_str).unwrap(), p.get("Finish").and_then(Value::as_str).unwrap()))
        .collect();
    finishes.sort();
    assert_eq!(finishes, [("Acme", "Plaster"), ("Acme", "Plaster"), ("Acme", "Plaster"), ("Acme", "Tiles")]);
    assert_eq!(i.model.parameters["Finish"].sets.get("wall").map(String::as_str), Some("Acme_Product"));
}

#[test]
fn classification_references_come_back_from_ifc_as_their_systems_codes_and_go_out_again() {
    let s = house();
    let file = crate::ifc::export(&s.model);
    let (walls, doors) = (of_class(&file, "IFCWALL"), of_class(&file, "IFCDOOR"));
    // A system that cites another (a code under a code) and a name no parameter may have as it is.
    let file = with(
        &file,
        &format!(
            "#900001=IFCCLASSIFICATION('https://example.org/table','2015',$,'Table 3E',$,$,$);
#900002=IFCCLASSIFICATIONREFERENCE($,'3E.01',$,#900001,$,$);
#900003=IFCCLASSIFICATIONREFERENCE($,'3E.01.20','Walls',#900002,$,$);
#900004=IFCRELASSOCIATESCLASSIFICATION('0aaaaaaaaaaaaaaaaaaaa1',$,$,$,({}),#900003);
#900005=IFCCLASSIFICATIONREFERENCE($,'3E.04.10','Doors',#900001,$,$);
#900006=IFCRELASSOCIATESCLASSIFICATION('0aaaaaaaaaaaaaaaaaaaa2',$,$,$,({}),#900005);
",
            walls.join(","),
            doors.join(",")
        ),
    );
    let i = ifc(file.as_bytes(), "x").unwrap();
    let p = &i.model.parameters["Table 3E"];
    assert_eq!(p.classification, Some(Classification { system: "Table 3E".into(), edition: Some("2015".into()) }));
    assert_eq!(p.categories, ["door", "wall"]);
    assert_eq!(values(&i.model, "wall").get("Table 3E"), Some(&json!("3E.01.20")));
    assert_eq!(values(&i.model, "door").get("Table 3E"), Some(&json!("3E.04.10")));

    // Out again: one IfcClassification, a reference a code, each associated with what holds it; not a property.
    let again = crate::ifc::export(&i.model);
    let systems = of_class(&again, "IFCCLASSIFICATION");
    assert_eq!(systems.len(), 1);
    assert!(body(&again, &systems[0]).contains("'2015'") && body(&again, &systems[0]).contains("'Table 3E'"));
    assert_eq!(of_class(&again, "IFCCLASSIFICATIONREFERENCE").len(), 2);
    assert_eq!(of_class(&again, "IFCRELASSOCIATESCLASSIFICATION").len(), 2);
    let wall = of_class(&again, "IFCWALL").remove(0);
    assert!(sets_of(&again, &wall).values().flatten().all(|n| n != "Table 3E"));
    let back = ifc(again.as_bytes(), "x").unwrap();
    assert_eq!(back.model.parameters, i.model.parameters);
    assert_eq!(values(&back.model, "wall").get("Table 3E"), Some(&json!("3E.01.20")));
}

#[test]
fn the_parameter_command_says_where_its_values_go_in_ifc_and_refuses_what_ifc_cant_hold() {
    let mut s = house();
    run(&mut s, "parameter", json!({"name": "Supplier", "categories": ["wall", "door"], "sets": {"wall": "Acme_Product"}}));
    assert_eq!(s.model.parameters["Supplier"].sets.get("wall").map(String::as_str), Some("Acme_Product"));
    // Taking a category's set away puts its values back in the project's own.
    run(&mut s, "parameter", json!({"name": "Supplier", "sets": {"wall": null}}));
    assert!(s.model.parameters["Supplier"].sets.is_empty());
    run(&mut s, "parameter", json!({"name": "Uniclass", "categories": ["wall"], "classification": {"system": "Uniclass 2015"}}));
    assert_eq!(s.model.parameters["Uniclass"].classification.as_ref().map(|c| c.system.as_str()), Some("Uniclass 2015"));
    for bad in [
        json!({"name": "Supplier", "sets": {"stair": "Acme_Product"}}),
        json!({"name": "Supplier", "sets": {"wall": ""}}),
        json!({"name": "Supplier", "sets": {"wall": "x".repeat(10_000)}}),
        json!({"name": "Supplier", "sets": {"wall": crate::ifc::PHASING}}),
        json!({"name": "Supplier", "sets": {"wall": "BIMCraft_Run"}}),
        json!({"name": "Uniclass", "sets": {"wall": "Acme_Product"}}),
        json!({"name": "Uniclass", "kind": "number"}),
        json!({"name": "Uniclass", "classification": {"system": ""}}),
        json!({"name": "Uniclass", "classification": {"system": "a", "nope": 1}}),
    ] {
        let before = s.model.parameters.clone();
        assert!(s.execute("parameter", &bad).is_err(), "{bad}");
        assert_eq!(s.model.parameters, before, "{bad}");
    }
}
