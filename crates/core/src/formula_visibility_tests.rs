//! Formulas as Revit's behave (T9).
//!
//! - A formula that names a parameter the family doesn't have is refused when the family is checked by a command, wherever
//!   the formula lives (a parameter's, a form's sizes and `visible`, a symbol's, a connector's, the room calculation point, an
//!   adaptive definition's), even in a branch of an `if` that is never taken, saying the name and that names are case
//!   sensitive. A project file that holds one still opens, saying so.
//! - A number in a formula takes a unit: `90°`, `52.6 mm`, `1.2 m`, in a comparison or a result.
//! - A parameter a formula works out is set by nothing: a type or a component that sets it is refused, naming the formula.
//!
//! Driven through `Session::execute`, as every front end drives it.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::family::Expr;
use crate::model::Id;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn err(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(r) => panic!("{id} {p} should have been refused, and did {r}"),
        Err(e) => e,
    }
}

fn id_of(v: &Value) -> Id {
    v["id"].as_u64().unwrap_or_default()
}

/// A formula that builds whatever the name it reads is, since its `if` never takes that branch: only a check that reads
/// every name finds it.
const HIDDEN: &str = "if(1 > 2, Hieght, 100)";

/// A box family `Box`, 1000 x 500 x Height, with `extra` fields over it.
fn boxed(extra: Value) -> Value {
    let mut f = json!({
        "name": "Box",
        "parameters": [{"name": "Width", "value": 1000}, {"name": "Height", "value": 500}],
        "types": {"A": {}},
        "forms": [{"profile": [[0, 0], ["Width", 0], ["Width", 500], [0, 500]], "to": "Height"}]
    });
    if let (Some(f), Value::Object(extra)) = (f.as_object_mut(), extra) {
        f.extend(extra);
    }
    f
}

fn says_not_a_parameter(e: &str, name: &str) {
    assert!(e.contains(&format!("not a valid parameter: {name}")), "it names `{name}`: {e}");
    assert!(e.contains("case sensitive"), "it says names are case sensitive: {e}");
}

#[test]
fn a_formula_naming_a_parameter_the_family_lacks_is_refused_naming_it_and_saying_names_are_case_sensitive() {
    let mut s = Session::new();
    let e =
        err(&mut s, "family", boxed(json!({"parameters": [{"name": "Width", "value": 1000}, {"name": "Height", "formula": "width / 2"}]})));
    says_not_a_parameter(&e, "width");
    assert!(e.contains("`Width`"), "the parameter it differs from in case only is offered: {e}");
    // In a branch never taken, it is still refused: the check reads every name, not only those worked out.
    let e = err(
        &mut s,
        "family",
        boxed(
            json!({"parameters": [{"name": "Width", "value": 1000}, {"name": "Height", "formula": format!("if(Width > 0, 500, {HIDDEN})")}]}),
        ),
    );
    says_not_a_parameter(&e, "Hieght");
    assert!(s.model.families.get("Box").is_none(), "nothing was made");
}

#[test]
fn every_place_a_formula_lives_is_read_for_names_the_family_lacks() {
    let profile = json!([[0, 0], [1000, 0], [1000, 500], [0, 500]]);
    let cases: Vec<(&str, Value)> = vec![
        ("form from", json!({"forms": [{"profile": profile, "from": HIDDEN, "to": 500}]})),
        ("form to", json!({"forms": [{"profile": profile, "to": HIDDEN}]})),
        ("form visible", json!({"forms": [{"profile": profile, "to": 500, "visible": HIDDEN}]})),
        ("form profile", json!({"forms": [{"profile": [[0, 0], [HIDDEN, 0], [1000, 500], [0, 500]], "to": 500}]})),
        ("form circle", json!({"forms": [{"circle": {"centre": [0, 0], "radius": HIDDEN}, "to": 500}]})),
        ("symbol line", json!({"symbols": [{"line": [[0, 0], [HIDDEN, 0]]}]})),
        ("symbol visible", json!({"symbols": [{"line": [[0, 0], [100, 0]], "visible": HIDDEN}]})),
        ("label size", json!({"symbols": [{"text": {"at": [0, 0], "height": HIDDEN, "value": "A"}}]})),
        ("label text", json!({"symbols": [{"text": {"at": [0, 0], "height": 100, "value": "{Hieght}"}}]})),
        ("room calculation point", json!({"options": {"room_calculation_point": [HIDDEN, 0, 0]}})),
        (
            "connector",
            json!({"category": "lighting", "connectors": [{"kind": "power", "system": "power-balanced", "poles": 1, "voltage": HIDDEN, "load_va": 60}]}),
        ),
    ];
    for (what, extra) in cases {
        let mut s = Session::new();
        let e = err(&mut s, "family", boxed(extra));
        says_not_a_parameter(&e, "Hieght");
        assert!(s.model.families.get("Box").is_none(), "{what}: nothing was made");
    }
    // An adaptive definition's: an extrusion's distance.
    let mut panel = crate::adaptive::four_point_panel();
    for n in &mut panel.nodes {
        if let crate::adaptive::Operation::Extrude { distance, .. } = &mut n.operation {
            *distance = Expr::Formula(HIDDEN.into());
        }
    }
    let mut s = Session::new();
    let e = err(&mut s, "family", json!({"name": "Panel", "types": {"A": {}}, "adaptive": panel}));
    says_not_a_parameter(&e, "Hieght");
}

/// A project with family `Box` and one component of it, saved as a file whose box's height is `HIDDEN`, as a file from
/// before this check may hold one; and the file.
fn old_file(tag: &str) -> std::path::PathBuf {
    let mut s = Session::new();
    run(&mut s, "family", boxed(json!({})));
    run(&mut s, "component", json!({"family": "Box", "type": "A", "at": [0, 0]}));
    if let Some(f) = s.model_mut().families.get_mut("Box") {
        f.forms[0].to = Expr::Formula(HIDDEN.into());
    }
    let path = std::env::temp_dir().join(format!("bimcraft-formula-{tag}-{}.json", std::process::id()));
    run(&mut s, "save", json!({"path": path}));
    path
}

#[test]
fn a_file_holding_such_a_formula_opens_saying_so_and_a_command_changing_that_family_is_refused() {
    let path = old_file("open");
    let mut s = Session::new();
    let r = run(&mut s, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    let warned = r["warnings"].to_string();
    assert!(warned.contains("Box") && warned.contains("Hieght") && warned.contains("case sensitive"), "{r}");
    // Reading the family says what is wrong with it.
    let read = run(&mut s, "family", json!({"name": "Box"}));
    assert!(read["errors"].to_string().contains("Hieght"), "{read}");
    // What doesn't change the family goes ahead: another of it placed, a wall drawn.
    run(&mut s, "component", json!({"family": "Box", "type": "A", "at": [3000, 0]}));
    run(&mut s, "wall", json!({"points": [[0, 3000], [4000, 3000]]}));
    // Changing it is refused until the formula is mended; mended, it goes ahead.
    let e = err(&mut s, "family", json!({"name": "Box", "types": {"A": {}, "B": {"Width": 1200}}}));
    says_not_a_parameter(&e, "Hieght");
    let f = s.model.families.get("Box").cloned().unwrap_or_default();
    let mut form = serde_json::to_value(&f.forms[0]).unwrap_or_default();
    form["to"] = json!("if(1 > 2, Height, 100)");
    run(&mut s, "family", json!({"name": "Box", "form": form}));
}

#[test]
fn a_command_that_brings_in_a_family_with_such_a_formula_is_refused() {
    // From the Favorites: a family kept there with a name it lacks comes in as nothing.
    let mut s = Session::new();
    run(&mut s, "family", boxed(json!({})));
    let mut family = s.model.families.get("Box").cloned().unwrap_or_default();
    family.forms[0].to = Expr::Formula(HIDDEN.into());
    let mut kept = crate::favorites::Favorites { version: 1, ..Default::default() };
    kept.families.insert("Bad Box".into(), crate::favorites::FamilyEntry { family, materials: Default::default() });
    let path = std::env::temp_dir().join(format!("bimcraft-formula-favorites-{}.json", std::process::id()));
    kept.write(&path).unwrap_or_else(|e| panic!("{e}"));
    let e = err(&mut s, "favorite", json!({"action": "load", "kind": "family", "name": "Bad Box", "path": path}));
    let _ = std::fs::remove_file(&path);
    says_not_a_parameter(&e, "Hieght");
    assert!(s.model.families.get("Bad Box").is_none());
}

#[test]
fn a_number_in_a_formula_takes_a_unit_in_a_comparison_and_in_a_result() {
    let get = &mut |n: &str| -> Result<f64, String> {
        match n {
            "Swing Angle" => Ok(90.0),
            "Width" => Ok(60.0),
            _ => Err(format!("no {n}")),
        }
    };
    let cases = [
        ("90°", 90.0),
        ("90 °", 90.0),
        ("52.6 mm", 52.6),
        ("1.2 m", 1200.0),
        ("5 cm", 50.0),
        ("3 dm", 300.0),
        ("2 m²", 2.0e6),
        ("1 m³", 1.0e9),
        ("1e3 mm", 1000.0),
        ("if(Swing Angle = 90°, 1, 0)", 1.0),
        ("if(Swing Angle > 90°, 1, 0)", 0.0),
        ("if(Width > 52.6 mm, 1 m, 20 cm)", 1000.0),
        ("if(Width < 5 cm, 1 m, 20 cm)", 200.0),
        ("Width + 1 m", 1060.0),
        ("2 * 1.5 m", 3000.0),
    ];
    for (formula, want) in cases {
        let got = crate::family::eval(formula, get).unwrap_or_else(|e| panic!("{formula}: {e}"));
        assert!((got - want).abs() < 1e-9 * want.abs().max(1.0), "{formula} = {got}, not {want}");
    }
    // A unit it doesn't know is named, and a word after a number is still no name.
    let e = crate::family::eval("3 furlongs", get).unwrap_err();
    assert!(e.contains("furlongs") && e.contains("unit"), "{e}");
    // In a family, through the command: a door swing's angle and a width compared in mm.
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        boxed(json!({"parameters": [
            {"name": "Width", "value": 1000},
            {"name": "Swing Angle", "kind": "angle", "value": 90},
            {"name": "Height", "formula": "if(Swing Angle = 90°, if(Width > 52.6 mm, 1.2 m, 300 mm), 0.5 m)"}
        ]})),
    );
    let read = run(&mut s, "family", json!({"name": "Box"}));
    assert_eq!(read["values"]["A"]["Height"], json!(1200.0), "{read}");
    // A parameter renamed leaves a unit alone, even one named as a unit is.
    assert_eq!(crate::family::renamed_in_formula("2 m + m", "m", "Mass"), "2 m + Mass");
    assert_eq!(crate::family::renamed_in_formula("90° + Angle", "Angle", "Swing"), "90° + Swing");
}

#[test]
fn a_parameter_a_formula_works_out_is_set_by_no_type_nor_component_and_the_refusal_names_the_formula() {
    let mut s = Session::new();
    let fam = |types: Value| {
        boxed(json!({
            "parameters": [{"name": "Width", "value": 1000}, {"name": "Height", "formula": "Width / 2"},
                           {"name": "Depth", "instance": true, "formula": "Width / 4"}],
            "types": types
        }))
    };
    let e = err(&mut s, "family", fam(json!({"A": {"Height": 700}})));
    assert!(e.contains("`Height`") && e.contains("Width / 2"), "{e}");
    run(&mut s, "family", fam(json!({"A": {}})));
    // What the interface greys: a parameter a formula drives says which.
    let f = s.model.families.get("Box").cloned().unwrap_or_default();
    let driven = |n: &str| f.parameters.iter().find(|p| p.name == n).and_then(|p| p.driven());
    assert_eq!((driven("Height"), driven("Width")), (Some("Width / 2"), None));
    let id = id_of(&run(&mut s, "component", json!({"family": "Box", "type": "A", "at": [0, 0]})));
    let e = err(&mut s, "modify", json!({"id": id, "params": {"Depth": 900}}));
    assert!(e.contains("`Depth`") && e.contains("Width / 4"), "{e}");
    // A formula given to a parameter a type sets takes the type's value away: the formula works it out from then on.
    run(
        &mut s,
        "family",
        json!({"name": "Box", "parameters": [{"name": "Width", "value": 1000}, {"name": "Height", "value": 500}],
        "types": {"A": {"Height": 800}}}),
    );
    run(&mut s, "family", json!({"name": "Box", "parameter": {"name": "Height", "formula": "Width * 0.3"}}));
    let read = run(&mut s, "family", json!({"name": "Box"}));
    assert_eq!(read["values"]["A"]["Height"], json!(300.0), "{read}");
}
