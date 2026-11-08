//! F5: a form's sketch. Files of 23 and before (a form's `profile`, `circle` and `axis`) read as sketches, building the same
//! triangles and the same IFC as they did (`testdata/family_format23.golden.json`, written by the build of 23), and saved and
//! read again unchanged, each line keeping its id and GUID; sketches of several loops, holes and arcs; what's wrong with
//! one, named; a revolve's axis line; formulas in a sketch renamed and moved with its plane.

use std::collections::{BTreeMap, HashMap};

use serde_json::{Value, json};

use crate::engine::Session;
use crate::family::sketch::{FormCurve, MAX_SKETCH};
use crate::family::{Expr, Family};
use crate::model::{FORMAT, Model};
use crate::view3d::{P3, cross, dot};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn err(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(r) => panic!("{id} {p} should have been refused, and did {r}"),
        Err(e) => e,
    }
}

/// The forms of the families of 23 the golden file was written from, as a file of 23 holds them.
fn legacy_forms() -> Value {
    json!([
        {"profile": [[0, 0], ["Width", 0], ["Width", "Width / 2"], [0, "Width / 2"]], "to": 300, "material": "Concrete"},
        {"profile": [[50, 50], [150, 50], [150, 120], [50, 120]], "from": 100, "to": 400, "void": true},
        {"circle": {"centre": ["Width + 200", 100], "radius": "R"}, "from": 0, "to": "Width", "visible": "Show"},
        {"plane": "xz", "profile": [[0, 0], [200, 0], [200, 150], [100, 260], [0, 150]], "from": 0, "to": 80},
        {"plane": "yz", "profile": [[0, 0], [120, 0], [0, 180]], "from": -30, "to": 30}
    ])
}

fn turned_forms() -> Value {
    json!([
        {"plane": "xz", "profile": [[100, 0], [300, 0], [300, "H"], [100, "H"]], "axis": [[0, 0], [0, 1]], "to": 360},
        {"plane": "xz", "profile": [[400, 0], [500, 0], [450, 120]], "axis": [[0, 0], [0, "H"]], "from": 30, "to": 200},
        {"circle": {"centre": [700, 200], "radius": 50}, "axis": [[600, 0], [600, 10]], "to": 90}
    ])
}

/// The project the golden file was written from: two families, three components.
fn legacy_project() -> Session {
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({"name": "Legacy", "category": "furniture",
        "parameters": [{"name": "Width", "value": 400}, {"name": "R", "value": 60}, {"name": "Show", "kind": "yesno", "value": 1}],
        "types": {"A": {}, "B": {"Width": 650, "R": 90}}, "forms": legacy_forms()}),
    );
    run(
        &mut s,
        "family",
        json!({"name": "Turned", "category": "furniture", "parameters": [{"name": "H", "value": 500}], "types": {"T": {}},
               "forms": turned_forms()}),
    );
    run(&mut s, "component", json!({"family": "Legacy", "type": "A", "at": [0, 0]}));
    run(&mut s, "component", json!({"family": "Legacy", "type": "B", "at": [3000, 0]}));
    run(&mut s, "component", json!({"family": "Turned", "type": "T", "at": [0, 3000]}));
    s
}

/// What the golden file holds of a model: each family's pieces for each type, and the IFC's geometry.
fn built(m: &Model) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for name in ["Legacy", "Turned"] {
        let f = &m.families[name];
        for t in f.types.keys() {
            let values = f.values(t, &BTreeMap::new()).unwrap();
            let pieces = f.pieces(&values).unwrap();
            let tris: Vec<Vec<[P3; 3]>> = pieces.iter().map(|p| p.tris.clone()).collect();
            out.insert(
                format!("{name}/{t}"),
                json!({"tris": tris, "outlines": pieces.iter().map(|p| p.outline.clone()).collect::<Vec<_>>(),
                       "round": pieces.iter().map(|p| p.round).collect::<Vec<_>>(), "caps": pieces.iter().map(|p| p.caps).collect::<Vec<_>>()}),
            );
        }
    }
    let ifc = crate::ifc::export(m);
    let geometry: Vec<&str> = ifc
        .lines()
        .filter(|l| l.contains("IFCCARTESIANPOINTLIST3D") || l.contains("FACESET") || l.contains("IFCINDEXEDPOLYGONALFACE"))
        .collect();
    out.insert("ifc".into(), json!(geometry));
    out
}

/// Whether two JSON values are the same, numbers within a millionth.
fn alike(a: &Value, b: &Value, at: &str) -> Result<(), String> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap_or(f64::NAN), y.as_f64().unwrap_or(f64::NAN));
            if (x - y).abs() <= 1e-6 { Ok(()) } else { Err(format!("{at}: {x} against {y}")) }
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                return Err(format!("{at}: {} against {} items", x.len(), y.len()));
            }
            x.iter().zip(y).enumerate().try_for_each(|(i, (p, q))| alike(p, q, &format!("{at}/{i}")))
        }
        (Value::Object(x), Value::Object(y)) => {
            if x.len() != y.len() {
                return Err(format!("{at}: keys differ"));
            }
            x.iter().try_for_each(|(k, v)| alike(v, y.get(k).unwrap_or(&Value::Null), &format!("{at}/{k}")))
        }
        _ if a == b => Ok(()),
        _ => Err(format!("{at}: {a} against {b}")),
    }
}

fn golden() -> Value {
    serde_json::from_str(include_str!("../testdata/family_format23.golden.json")).unwrap()
}

/// The legacy project as a file of 23 holds it: its forms as profiles, circles and axes.
fn file_of_23() -> String {
    let s = legacy_project();
    let mut file: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    file["format"] = json!(23);
    file["families"]["Legacy"]["forms"] = legacy_forms();
    file["families"]["Turned"]["forms"] = turned_forms();
    file.to_string()
}

#[test]
fn a_file_of_23_reads_as_sketches_and_builds_the_same_triangles_and_ifc() {
    let (m, left_out) = Model::read(&file_of_23()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m.format, FORMAT);
    let g = golden();
    for (k, v) in built(&m) {
        alike(&v, &g[&k], &k).unwrap_or_else(|e| panic!("{e}"));
    }
    // The formulas are the sketch's: the box's corners as lines, the circle a line, the revolves' axes lines.
    let legacy = &m.families["Legacy"];
    assert_eq!(legacy.forms[0].sketch.len(), 4);
    assert!(legacy.forms[0].sketch_exprs().any(|e| *e == Expr::Formula("Width / 2".into())));
    assert!(matches!(&legacy.forms[2].sketch[0].curve, FormCurve::Circle { radius: Expr::Formula(r), .. } if r == "R"));
    let turned = &m.families["Turned"];
    assert_eq!(turned.forms[1].axis(), Some([[Expr::Number(0.0), Expr::Number(0.0)], [Expr::Number(0.0), Expr::Formula("H".into())]]));
    assert!(turned.forms.iter().all(|f| f.revolves()));
    // Every line a part, with an id and a GUID of its own.
    let lines: Vec<_> = m.families.values().flat_map(|f| f.forms.iter().flat_map(|x| x.sketch.iter())).collect();
    assert!(lines.iter().all(|l| l.id != 0 && !l.guid.is_nil()));
    assert!(m.library_in_step());
}

#[test]
fn a_sketch_saved_and_read_again_is_the_same_each_line_keeping_its_id_and_guid() {
    let (m, _) = Model::read(&file_of_23()).unwrap();
    let text = m.to_json().unwrap();
    assert!(!text.contains("\"profile\"") && !text.contains("\"circle\"") && text.contains("\"sketch\""));
    let (again, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty());
    assert_eq!(again.families["Legacy"], m.families["Legacy"]);
    assert_eq!(again.families["Turned"], m.families["Turned"]);
    let g = golden();
    for (k, v) in built(&again) {
        alike(&v, &g[&k], &k).unwrap_or_else(|e| panic!("{e}"));
    }
    // An edit through the command keeps every line's id and GUID: a patch of the form, and the forms given again.
    let mut s = Session::new();
    s.model = std::sync::Arc::new(again);
    let before: Vec<_> = s.model.families["Legacy"].forms[0].sketch.iter().map(|l| (l.id, l.guid)).collect();
    let guid = s.model.families["Legacy"].forms[0].guid;
    run(&mut s, "family", json!({"name": "Legacy", "form": {"guid": guid, "to": 350}}));
    let now: Vec<_> = s.model.families["Legacy"].forms[0].sketch.iter().map(|l| (l.id, l.guid)).collect();
    assert_eq!(now, before);
    run(&mut s, "family", json!({"name": "Legacy", "form": {"guid": guid, "profile": [[0, 0], [500, 0], [500, 200], [0, 200]]}}));
    let now: Vec<_> = s.model.families["Legacy"].forms[0].sketch.iter().map(|l| (l.id, l.guid)).collect();
    assert_eq!(now, before, "the corners given again keep their lines by place");
    run(&mut s, "family", json!({"name": "Legacy", "forms": legacy_forms()}));
    let now: Vec<_> = s.model.families["Legacy"].forms[0].sketch.iter().map(|l| (l.id, l.guid)).collect();
    assert_eq!(now, before);
}

/// The family `S` with `forms`, and its first type's values.
fn sketched(forms: Value) -> (Session, Result<Value, String>) {
    let mut s = Session::new();
    let r = s.execute(
        "family",
        &json!({"name": "S", "category": "furniture", "parameters": [{"name": "W", "value": 1000}], "types": {"A": {}}, "forms": forms}),
    );
    (s, r)
}

fn family(s: &Session) -> &Family {
    &s.model.families["S"]
}

/// Form `i`'s triangles, and the volume they enclose (positive: they face out).
fn solid(f: &Family, i: usize) -> (Vec<[P3; 3]>, f64) {
    let values = f.values("A", &BTreeMap::new()).unwrap();
    let piece = f.solid(i, &values, &mut HashMap::new()).unwrap().unwrap();
    let volume = piece.tris.iter().map(|[a, b, c]| dot(*a, cross(*b, *c)) / 6.0).sum();
    (piece.tris, volume)
}

fn line(a: [f64; 2], b: [f64; 2]) -> Value {
    json!({"start": a, "end": b})
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Value> {
    vec![line([x0, y0], [x1, y0]), line([x1, y0], [x1, y1]), line([x1, y1], [x0, y1]), line([x0, y1], [x0, y0])]
}

#[test]
fn a_sketch_with_a_hole_and_another_loop_builds_one_form_of_two_solids() {
    // A rectangle with a square hole, and a second rectangle beside it, drawn in no order.
    let mut lines = rect(0.0, 0.0, 1000.0, 600.0);
    lines.extend(rect(300.0, 200.0, 500.0, 400.0).into_iter().rev());
    lines.extend(rect(2000.0, 0.0, 2100.0, 100.0));
    lines.swap(1, 6);
    let (s, r) = sketched(json!([{"sketch": lines, "to": 100}]));
    r.unwrap();
    let (tris, volume) = solid(family(&s), 0);
    let want = (1000.0 * 600.0 - 200.0 * 200.0 + 100.0 * 100.0) * 100.0;
    assert!((volume - want).abs() < 1e-3, "{volume} against {want}");
    // No cap covers the hole.
    let over_hole = tris.iter().any(|t| {
        let c = [0, 1].map(|k| t.iter().map(|p| p[k]).sum::<f64>() / 3.0);
        t.iter().all(|p| p[2] == 100.0) && c[0] > 300.0 && c[0] < 500.0 && c[1] > 200.0 && c[1] < 400.0
    });
    assert!(!over_hole);
    // An island in the hole is solid again.
    let mut with_island = rect(0.0, 0.0, 1000.0, 600.0);
    with_island.extend(rect(300.0, 200.0, 500.0, 400.0));
    with_island.extend(rect(350.0, 250.0, 450.0, 350.0));
    let (s, r) = sketched(json!([{"sketch": with_island, "to": 10}]));
    r.unwrap();
    let (_, volume) = solid(family(&s), 0);
    assert!((volume - (600_000.0 - 40_000.0 + 10_000.0) * 10.0).abs() < 1e-3, "{volume}");
}

#[test]
fn an_arc_and_a_circle_hole_are_cut_into_chords_as_built() {
    // A half disc: a line across and an arc of 180 degrees back, swept 50; and a circle hole in a square beside it.
    let half = json!([{"start": [-500, 0], "end": [500, 0]}, {"start": [500, 0], "end": [-500, 0], "turn": 180}]);
    let mut holed = rect(2000.0, -500.0, 3000.0, 500.0);
    holed.push(json!({"centre": [2500, 0], "radius": "W / 5"}));
    let (s, r) = sketched(json!([{"sketch": half, "to": 50}, {"sketch": holed, "to": 20}]));
    r.unwrap();
    let f = family(&s);
    let (tris, volume) = solid(f, 0);
    // 16 chords of a half turn (32 to a whole one): the chorded half disc's area.
    let chorded = 0.5 * 16.0 * 500.0 * 500.0 * (std::f64::consts::PI / 16.0).sin();
    assert!((volume - chorded * 50.0).abs() < 1e-3, "{volume}");
    // Its caps (15 triangles each, 17 corners) and its sides (17 edges, two triangles each).
    assert_eq!(tris.len(), 2 * 15 + 2 * 17);
    let (_, volume) = solid(f, 1);
    let circle = 0.5 * 32.0 * 200.0 * 200.0 * (std::f64::consts::TAU / 32.0).sin();
    assert!((volume - (1_000_000.0 - circle) * 20.0).abs() < 1e-3, "{volume}");
}

#[test]
fn what_is_wrong_with_a_sketch_is_named_with_its_lines() {
    let cases = [
        // Open: three sides of a square.
        (json!([line([0., 0.], [100., 0.]), line([100., 0.], [100., 100.]), line([100., 100.], [0., 100.])]), "the loop is open"),
        // Two lines crossing: a bow tie.
        (
            json!([line([0., 0.], [100., 100.]), line([100., 100.], [100., 0.]), line([100., 0.], [0., 100.]), line([0., 100.], [0., 0.])]),
            "intersect",
        ),
        // A line drawn twice.
        (
            json!([line([0., 0.], [100., 0.]), line([100., 0.], [100., 100.]), line([100., 100.], [0., 0.]), line([0., 0.], [100., 0.])]),
            "overlap",
        ),
    ];
    for (lines, want) in cases {
        let (s, r) = sketched(json!([{"sketch": lines, "to": 10}]));
        let e = r.unwrap_err();
        assert!(e.contains(want) && e.contains("form 1") && e.contains("line "), "{e}");
        assert!(!s.model.families.contains_key("S"), "nothing made");
    }
    // A number it can't read, an arc turning all the way, too many lines.
    let (_, r) = sketched(json!([{"sketch": [{"start": [0, 0], "end": ["Nope", 0]}], "to": 10}]));
    assert!(r.unwrap_err().contains("Nope"));
    let (_, r) =
        sketched(json!([{"sketch": [{"start": [0, 0], "end": [10, 0], "turn": 400}, {"start": [10, 0], "end": [0, 0]}], "to": 10}]));
    assert!(r.unwrap_err().contains("whole turn"));
    let many: Vec<Value> = (0..=MAX_SKETCH).map(|k| line([k as f64, 0.0], [k as f64 + 1.0, 0.0])).collect();
    let (_, r) = sketched(json!([{"sketch": many, "to": 10}]));
    assert!(r.unwrap_err().contains("more than"));
    // A sketch and a shorthand for the same thing.
    let (_, r) = sketched(json!([{"sketch": rect(0., 0., 10., 10.), "profile": [[0, 0], [1, 0], [1, 1]], "to": 10}]));
    assert!(r.unwrap_err().contains("profile"));
    // A circle line with a turn, an axis that turns.
    let (_, r) = sketched(json!([{"sketch": [{"centre": [0, 0], "radius": 5, "turn": 4}], "to": 10}]));
    assert!(r.is_err());
    let mut lines = rect(100.0, 0.0, 200.0, 100.0);
    lines.push(json!({"start": [0, 0], "end": [0, 100], "turn": 30, "axis": true}));
    let (_, r) = sketched(json!([{"sketch": lines, "to": 360}]));
    assert!(r.unwrap_err().contains("straight"));
}

#[test]
fn a_revolve_turns_about_its_axis_line_and_takes_one() {
    let mut lines = rect(100.0, 0.0, 300.0, 500.0);
    lines.push(json!({"start": [0, 0], "end": [0, 1], "axis": true}));
    let (s, r) = sketched(json!([{"host": {"plane": crate::family::CENTER_FRONT_BACK}, "sketch": lines.clone(), "to": 360}]));
    r.unwrap();
    let f = family(&s);
    assert!(f.forms[0].revolves());
    let (_, volume) = solid(f, 0);
    // A ring of 32 sides, 300 out and 100 in, 500 high.
    let polygon = |r: f64| 0.5 * 32.0 * r * r * (std::f64::consts::TAU / 32.0).sin();
    assert!((volume - (polygon(300.0) - polygon(100.0)) * 500.0).abs() < 1e-2, "{volume}");
    // Two axes, a profile across its axis.
    let mut two = lines.clone();
    two.push(json!({"start": [50, 0], "end": [50, 1], "axis": true}));
    let (_, r) = sketched(json!([{"sketch": two, "to": 360}]));
    assert!(r.unwrap_err().contains("one axis"));
    let mut across = rect(-100.0, 0.0, 300.0, 500.0);
    across.push(json!({"start": [0, 0], "end": [0, 1], "axis": true}));
    let (_, r) = sketched(json!([{"sketch": across, "to": 360}]));
    assert!(r.unwrap_err().contains("crosses its axis"));
    // A revolve of a holed profile: a hole turned is a hollow.
    let mut holed = rect(100.0, 0.0, 300.0, 500.0);
    holed.extend(rect(150.0, 100.0, 250.0, 400.0));
    holed.push(json!({"start": [0, 0], "end": [0, 1], "axis": true}));
    let (s, r) = sketched(json!([{"sketch": holed, "to": 360}]));
    r.unwrap();
    let (_, hollow) = solid(family(&s), 0);
    let want = (polygon(300.0) - polygon(100.0)) * 500.0 - (polygon(250.0) - polygon(150.0)) * 300.0;
    assert!((hollow - want).abs() < 1e-2, "{hollow} against {want}");
}

#[test]
fn formulas_in_a_sketch_are_renamed_and_kept_on_its_plane_moved() {
    let lines = json!([line([0., 0.], [100., 0.]), {"start": [100, 0], "end": ["W", 50]}, {"start": ["W", 50], "end": [0, 50], "turn": "W / 100"},
                       line([0., 50.], [0., 0.])]);
    let (mut s, r) = sketched(json!([{"sketch": lines, "from": 10, "to": 60}]));
    r.unwrap();
    run(&mut s, "family", json!({"name": "S", "parameter": {"name": "W", "rename": "Wide"}}));
    let f = family(&s);
    assert!(f.forms[0].sketch_exprs().any(|e| *e == Expr::Formula("Wide".into())));
    assert!(f.forms[0].sketch_exprs().any(|e| *e == Expr::Formula("Wide / 100".into())));
    let before = solid(f, 0).1;
    // A plane facing down, its x along -x: kept where it is, its formulas turned with it, the arc turning the other way.
    run(
        &mut s,
        "family",
        json!({"name": "S", "ref_planes": [{"name": "Down", "start": [0, 0, 300], "end": [-1000, 0, 300], "up": [0, 1, 0]}]}),
    );
    let form = json!(family(&s).forms[0].guid);
    run(&mut s, "edit_work_plane", json!({"family": "S", "form": form, "host": {"plane": "Down"}, "keep": true}));
    let f = family(&s);
    let after = solid(f, 0).1;
    assert!((after - before).abs() < 1e-3, "{after} against {before}");
    assert!(f.forms[0].sketch_exprs().any(|e| matches!(e, Expr::Formula(x) if x.contains("Wide"))));
}

#[test]
fn a_sketch_with_formulas_flexes_with_its_types() {
    let mut lines = vec![line([0., 0.], [100., 0.])];
    lines.push(json!({"start": [100, 0], "end": [100, "W"]}));
    lines.push(json!({"start": [100, "W"], "end": [0, "W"]}));
    lines.push(json!({"start": [0, "W"], "end": [0, 0]}));
    lines.extend(rect(20.0, 20.0, 80.0, 80.0));
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({"name": "S", "parameters": [{"name": "W", "value": 1000}], "types": {"A": {}, "B": {"W": 500}}, "forms": [{"sketch": lines, "to": 10}]}),
    );
    let f = &s.model.families["S"];
    let volume = |t: &str| {
        let v = f.values(t, &BTreeMap::new()).unwrap();
        f.pieces(&v).unwrap()[0].tris.iter().map(|[a, b, c]| dot(*a, cross(*b, *c)) / 6.0).sum::<f64>()
    };
    assert!((volume("A") - (100_000.0 - 3600.0) * 10.0).abs() < 1e-3);
    assert!((volume("B") - (50_000.0 - 3600.0) * 10.0).abs() < 1e-3);
    // A new sketch's IFC: the holed box as one face set, its triangles as the form builds them.
    run(&mut s, "component", json!({"family": "S", "type": "B", "at": [0, 0]}));
    let ifc = crate::ifc::export(&s.model);
    assert!(ifc.contains("IFCTRIANGULATEDFACESET") || ifc.contains("IFCPOLYGONALFACESET"));
    // Wrong in a type only: refused, and said.
    let e = err(&mut s, "family", json!({"name": "S", "types": {"C": {"W": 50}}}));
    assert!(e.contains("line"), "{e}");
}

#[test]
fn finish_checks_lines_as_sketch_mode_draws_them_and_keeps_unmoved_formulas() {
    use crate::family::sketch::{Finish, finish_check, kept, lines_of};
    use crate::geom::P2;
    use crate::sketch::{Problem, SketchLine};
    let square = |x0: f64, x1: f64| {
        vec![
            SketchLine::new(P2(x0, 0.0), P2(x1, 0.0)),
            SketchLine::new(P2(x1, 0.0), P2(x1, 100.0)),
            SketchLine::new(P2(x1, 100.0), P2(x0, 100.0)),
            SketchLine::new(P2(x0, 100.0), P2(x0, 0.0)),
        ]
    };
    assert_eq!(finish_check(&square(100.0, 200.0), None, false), Ok(()));
    let open = &square(100.0, 200.0)[..3];
    assert!(matches!(finish_check(open, None, false), Err(Finish::Problems(p)) if p.iter().any(|p| matches!(p, Problem::Open { .. }))));
    assert_eq!(finish_check(&square(100.0, 200.0), None, true), Err(Finish::NoAxis));
    assert_eq!(finish_check(&square(100.0, 200.0), Some([P2(0.0, 0.0), P2(0.0, 100.0)]), true), Ok(()));
    assert_eq!(finish_check(&square(-100.0, 200.0), Some([P2(0.0, 0.0), P2(0.0, 100.0)]), true), Err(Finish::CrossesAxis));
    // A line of formulas unmoved keeps them; one moved, or a circle where a line was, holds numbers.
    let (s, r) = sketched(
        json!([{"sketch": [{"start": [0, 0], "end": ["W", 0]}, {"start": ["W", 0], "end": [0, 100]}, {"start": [0, 100], "end": [0, 0]}], "to": 10}]),
    );
    r.unwrap();
    let old = family(&s).forms[0].sketch.clone();
    let mut num = |e: &Expr| match e {
        Expr::Number(x) => Ok(*x),
        Expr::Formula(f) if f == "W" => Ok(1000.0),
        Expr::Formula(f) => Err(f.clone()),
    };
    let mut drawn: Vec<SketchLine> = old.iter().map(|l| l.curve(&mut num).unwrap()).collect();
    drawn[1].curve = drawn[1].curve.moved(P2(0.0, 100.0), P2(0.0, 200.0));
    let new = kept(&old, &[], lines_of(&drawn), &mut num);
    assert_eq!(new[0], old[0]);
    assert_ne!(new[1], old[1]);
    assert_eq!((new[1].id, new[1].guid), (old[1].id, old[1].guid));
    assert!(new[1].exprs().iter().all(|e| matches!(e, Expr::Number(_))));
}

#[test]
fn a_sketch_snaps_to_the_planes_traces_and_the_other_forms_edges_on_its_plane() {
    use crate::family::sketch::{creases, on_plane};
    use crate::family::{CENTER_LEFT_RIGHT, PlaneRef, REF_LEVEL};
    use crate::geom::P2;
    use crate::snap::{Kind, Reach, Snaps, among};
    let (mut s, r) = sketched(json!([{"sketch": rect(500.0, 300.0, 1000.0, 600.0), "to": 200}]));
    r.unwrap();
    run(
        &mut s,
        "family",
        json!({"name": "S", "ref_planes": [{"name": "Edge", "start": [-400, -2000, 0], "end": [-400, 2000, 0], "up": [0, 0, 1]}]}),
    );
    let f = family(&s);
    let values = f.values("A", &BTreeMap::new()).unwrap();
    // A box has twelve edges as drawn; its caps' diagonals are none of them.
    let (tris, _) = solid(f, 0);
    assert_eq!(creases(&tris).len(), 12);
    // On Ref. Level: Center (Left/Right)'s trace runs along y at x 0, Edge's at x -400; Ref. Level itself has none.
    let t = f.sketch_targets(&values, &PlaneRef::Plane(REF_LEVEL), None);
    assert_eq!(t.traces.len(), 3, "Center (Front/Back), Center (Left/Right) and Edge");
    let reach = Reach { point: 20.0, line: 20.0 };
    let o = Snaps::default();
    let found = among(&t.traces, &t.segments, &[], P2(-407.3, 123.4), reach, &[], false, &o);
    assert_eq!((found.at.0, found.kind), (-400.0, Kind::Nearest), "{found:?}");
    let found = among(&t.traces, &t.segments, &[], P2(-396.0, 4.0), reach, &[], false, &o);
    assert_eq!((found.at, found.kind), (P2(-400.0, 0.0), Kind::Intersection), "where Edge crosses Center (Front/Back)");
    // The box's corner, seen on Ref. Level from its edges; its upright edges, square to the plane, are no line on it.
    let found = among(&t.traces, &t.segments, &[], P2(1004.0, 596.0), reach, &[], false, &o);
    assert_eq!((found.at, found.kind), (P2(1000.0, 600.0), Kind::Endpoint));
    assert!(on_plane(&f.frame(&PlaneRef::Plane(REF_LEVEL)).unwrap(), ([500.0, 300.0, 0.0], [500.0, 300.0, 200.0])).is_err());
    // Skipping the form sketched again: only the line Edge was drawn as.
    assert_eq!(f.sketch_targets(&values, &PlaneRef::Plane(REF_LEVEL), Some(0)).segments, [(P2(-400.0, -2000.0), P2(-400.0, 2000.0))]);
    // On Center (Left/Right), its own trace is none, and the box is seen side on: y along its x, z along its y.
    let t = f.sketch_targets(&values, &PlaneRef::Plane(CENTER_LEFT_RIGHT), None);
    assert_eq!(t.traces.len(), 2, "Ref. Level and Center (Front/Back); Edge is parallel");
    assert!(t.segments.iter().any(|(a, b)| [*a, *b].contains(&P2(300.0, 200.0))));
}

#[test]
fn a_form_is_picked_in_3d_where_it_shows_nearest_the_eye() {
    use crate::family::views::form_at_3d;
    let (s, r) = sketched(
        json!([{"sketch": rect(0.0, 0.0, 100.0, 100.0), "to": 100}, {"sketch": rect(0.0, 0.0, 100.0, 100.0), "from": 300, "to": 400}]),
    );
    r.unwrap();
    let f = family(&s);
    let values = f.values("A", &BTreeMap::new()).unwrap();
    let cam = crate::view3d::Camera::at([50.0, 50.0, 2000.0], [50.0, 50.0, 0.0], 50.0, 400, 400);
    let (x, y, _) = cam.project([50.0, 50.0, 400.0]);
    assert_eq!(form_at_3d(f, &values, &cam, (x, y)), Some(1), "the upper one, nearer the eye looking down");
    assert_eq!(form_at_3d(f, &values, &cam, (1.0, 1.0)), None);
}
