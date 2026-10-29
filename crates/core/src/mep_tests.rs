//! The MEP tabs' elements (plumbing, electrical, lightning protection, data) through the commands every front end drives. The
//! run categories share one table, so a tab adds a row; what only one tab has (its fixtures, its devices) has tests of its own.

use serde_json::{Value, json};

use crate::element_json;
use crate::engine::Session;
use crate::geom::P2;
use crate::render::{self, Drawing, POCHE, Prim, View};

/// A run category and what is true of it.
struct Case {
    category: &'static str,
    /// The command that makes it, and its Revit shortcut (or the letters BIMCraft gives it).
    command: &'static str,
    alias: &'static str,
    /// The type a new one gets, and whether its section is round.
    default_type: &'static str,
    round: bool,
    /// The schedule's columns, from the system on.
    columns: &'static [&'static str],
    /// Whether its class and predefined type say which category it is when another tool wrote them, with nothing of
    /// BIMCraft's beside them.
    alone: bool,
    /// The IFC class and predefined type it is written as.
    class: &'static str,
    predefined: &'static str,
}

const CASES: &[Case] = &[
    Case {
        category: "pipe",
        command: "pipe",
        alias: "PI",
        default_type: "PVC 50",
        round: true,
        columns: &["System", "Length", "Diameter"],
        alone: true,
        class: "IFCPIPESEGMENT",
        predefined: ".RIGIDSEGMENT.",
    },
    Case {
        category: "conduit",
        command: "conduit",
        alias: "CN",
        default_type: "PVC 25",
        round: true,
        columns: &["System", "Length", "Diameter"],
        alone: true,
        class: "IFCCABLECARRIERSEGMENT",
        predefined: ".CONDUITSEGMENT.",
    },
    Case {
        category: "cabletray",
        command: "cable_tray",
        alias: "CT",
        default_type: "Perforated 100 x 50",
        round: false,
        columns: &["System", "Length", "Width", "Depth"],
        alone: true,
        class: "IFCCABLECARRIERSEGMENT",
        predefined: ".CABLETRAYSEGMENT.",
    },
    Case {
        category: "wire",
        command: "wire",
        alias: "EW",
        default_type: "Wire 2.5",
        round: true,
        columns: &["System", "Length", "Diameter"],
        alone: true,
        class: "IFCCABLESEGMENT",
        predefined: ".CABLESEGMENT.",
    },
    Case {
        category: "downconductor",
        command: "down_conductor",
        alias: "DC",
        default_type: "Steel 8",
        round: true,
        columns: &["System", "Length", "Width", "Depth"],
        alone: true,
        class: "IFCCABLESEGMENT",
        predefined: ".CONDUCTORSEGMENT.",
    },
    Case {
        category: "groundconductor",
        command: "ground_conductor",
        alias: "GC",
        default_type: "Copper 9",
        round: true,
        columns: &["System", "Length", "Width", "Depth"],
        alone: false,
        class: "IFCCABLESEGMENT",
        predefined: ".CONDUCTORSEGMENT.",
    },
    Case {
        category: "cable",
        command: "cabling",
        alias: "CB",
        default_type: "Cat6",
        round: true,
        columns: &["System", "Length", "Diameter"],
        alone: false,
        class: "IFCCABLESEGMENT",
        predefined: ".CABLESEGMENT.",
    },
];

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn ids(r: &Value) -> Vec<u64> {
    r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect()
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

fn made(s: &mut Session, id: &str, p: Value) -> u64 {
    let r = run(s, id, p);
    r["id"].as_u64().or_else(|| ids(&r).first().copied()).unwrap()
}

/// The category's plural, as a person types it.
fn plural(c: &Case) -> String {
    format!("{}s", c.category)
}

/// The columns a run schedule starts with.
fn columns(c: &Case) -> Value {
    let mut all = vec!["Id", "Type", "Level"];
    all.extend(c.columns);
    json!(all)
}

#[test]
fn a_run_of_the_mep_is_made_between_points_one_for_each_segment_centred_on_its_height() {
    for c in CASES {
        let mut s = Session::new();
        let r = run(&mut s, c.command, json!({"points": [[0, 0], [6000, 0], [6000, 4000]]}));
        let made = ids(&r);
        assert_eq!(made.len(), 2, "{}", c.category);
        let e = element_json(&s.model, made[0]);
        assert_eq!(
            (e["category"].as_str(), e["type"].as_str(), e["justify"].as_str(), e["round"].as_bool()),
            (Some(c.category), Some(c.default_type), Some("centre"), Some(c.round)),
            "{}",
            c.category
        );
        // Its centre line is at the level, where its `offset` says.
        assert_eq!(
            (e["length"].as_f64(), e["axis"].clone()),
            (Some(6000.0), json!([[0.0, 0.0, 0.0], [6000.0, 0.0, 0.0]])),
            "{}",
            c.category
        );
        assert_eq!(element_json(&s.model, made[1])["length"], 4000.0);
        // The shortcut; closed, a ring; the plural finds them.
        let ring = run(&mut s, c.alias, json!({"points": [[0, 0], [1000, 0], [1000, 1000]], "closed": true}));
        assert_eq!(ids(&ring).len(), 3, "{}", c.alias);
        assert_eq!(run(&mut s, "elements", json!({"category": plural(c)}))["count"], 5, "{}", c.category);
    }
}

#[test]
fn a_run_of_the_mep_falls_rises_stands_and_says_which_system_it_is_in() {
    for c in CASES {
        let mut s = Session::new();
        // A drain falling 60 mm over 6 m, in a system; a riser from the floor to the ceiling above, upright.
        let r = run(
            &mut s,
            c.command,
            json!({"points": [[0, 0], [6000, 0]], "offset": 2400, "end_offset": 2340, "system": "Esgoto", "level": "Level 1"}),
        );
        let e = element_json(&s.model, ids(&r)[0]);
        assert_eq!(e["axis"], json!([[0.0, 0.0, 2400.0], [6000.0, 0.0, 2340.0]]), "{}", c.category);
        assert_eq!(e["system"], "Esgoto");
        let up = run(&mut s, c.command, json!({"points": [[500, 500], [500, 500]], "offset": 0, "end_offset": 3000}));
        let e = element_json(&s.model, ids(&up)[0]);
        assert_eq!(
            (e["length"].as_f64(), e["axis"].clone()),
            (Some(3000.0), json!([[500.0, 500.0, 0.0], [500.0, 500.0, 3000.0]])),
            "{}",
            c.category
        );
        // And its height changes with `modify`, its system too.
        run(&mut s, "modify", json!({"id": ids(&up)[0], "offset": 100, "system": "Água fria"}));
        let e = element_json(&s.model, ids(&up)[0]);
        assert_eq!((e["axis"][0][2].as_f64(), e["system"].as_str()), (Some(100.0), Some("Água fria")), "{}", c.category);
    }
}

#[test]
fn a_run_of_the_mep_that_cannot_be_one_is_refused_and_nothing_changes() {
    for c in CASES {
        let mut s = Session::new();
        let line = json!([[0, 0], [3000, 0]]);
        let cmd = c.command;
        assert!(fails(&mut s, cmd, json!({"points": [[0, 0], [0, 0]]})).contains("too short"));
        assert!(fails(&mut s, cmd, json!({"points": [[0, 0]]})).contains("at least 2"));
        assert!(fails(&mut s, cmd, json!({"points": line, "type": "Nope"})).contains("type `Nope` doesn't exist"));
        assert!(fails(&mut s, cmd, json!({"points": line, "justify": "sideways"})).contains("justify"));
        assert!(fails(&mut s, cmd, json!({"points": line, "offset": 1e300})).contains("offset"));
        assert!(fails(&mut s, cmd, json!({"points": line, "system": "x".repeat(300)})).contains("system"));
        assert!(fails(&mut s, cmd, json!({"points": [[0, 0], [1000, 0], [2000, 0]], "end_offset": 100})).contains("end_offset"));
    }
}

#[test]
fn types_of_a_run_are_made_and_changed_with_type_round_or_not_and_in_a_material() {
    for c in CASES {
        let mut s = Session::new();
        let made = run(
            &mut s,
            "type",
            json!({"category": c.category, "name": "Mine", "width": 75, "depth": 75, "round": c.round, "material": "Metal"}),
        );
        assert_eq!(made["type"], json!({"width": 75.0, "depth": 75.0, "round": c.round, "material": "Metal"}), "{}", c.category);
        let id = ids(&run(&mut s, c.command, json!({"points": [[0, 0], [3000, 0]], "type": "Mine"})))[0];
        run(&mut s, "type", json!({"category": c.category, "name": "Mine", "width": 90}));
        assert_eq!(element_json(&s.model, id)["width"], 90.0, "it changes where it is used");
        assert!(
            fails(
                &mut s,
                "type",
                json!({"category": c.category, "name": "Odd", "width": 50, "depth": 50, "round": c.round, "material": "Unobtainium"})
            )
            .contains("library")
        );
    }
}

#[test]
fn the_runs_of_the_mep_move_copy_rotate_mirror_and_delete() {
    for c in CASES {
        let mut s = Session::new();
        let a = ids(&run(&mut s, c.command, json!({"points": [[0, 0], [4000, 0]], "offset": 2800, "end_offset": 3000})))[0];
        let axis = |s: &Session, id| element_json(&s.model, id)["axis"].clone();
        let (z0, z1) = (axis(&s, a)[0][2].clone(), axis(&s, a)[1][2].clone());
        run(&mut s, "move", json!({"ids": [a], "by": [1000, 500]}));
        assert_eq!(axis(&s, a)[0], json!([1000.0, 500.0, z0]), "heights stay");
        run(&mut s, "rotate", json!({"ids": [a], "about": [1000, 500], "angle": 90}));
        let end = axis(&s, a)[1].clone();
        assert!((end[0].as_f64().unwrap() - 1000.0).abs() < 1e-6 && (end[1].as_f64().unwrap() - 4500.0).abs() < 1e-6, "{end}");
        let m = run(&mut s, "mirror", json!({"ids": [a], "axis": [[0, 0], [0, 1000]], "copy": true}));
        let mirrored = axis(&s, ids(&m)[0]);
        assert_eq!((&mirrored[0][2], &mirrored[1][2]), (&z0, &z1), "each end keeps its height");
        run(&mut s, "copy", json!({"ids": [a], "by": [0, -2000]}));
        assert_eq!(run(&mut s, "elements", json!({"category": c.category}))["count"], 3);
        run(&mut s, "delete", json!({"ids": [a]}));
        run(&mut s, "undo", json!({}));
        assert_eq!(run(&mut s, "elements", json!({"category": c.category}))["count"], 3);
    }
}

#[test]
fn a_project_with_runs_of_the_mep_saves_and_opens_again() {
    let mut s = Session::new();
    for c in CASES {
        run(&mut s, c.command, json!({"points": [[0, 0], [4000, 0]], "system": "S"}));
    }
    let path = std::env::temp_dir().join(format!("bimcraft-test-{}-mep.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(*t.model, *s.model);
}

// ---------- drawn ----------

fn view() -> View {
    View { scale: 0.1, cx: 3000.0, cy: 0.0, width: 1000, height: 800 }
}

fn of(d: &Drawing, id: u64) -> Vec<&Prim> {
    d.prims.iter().zip(&d.owners).filter(|(_, o)| **o == Some(id)).map(|(p, _)| p).collect()
}

/// The box the closed lines and fills of `id` take in the drawing, in model mm.
fn extent(d: &Drawing, id: u64) -> (P2, P2) {
    let pts: Vec<P2> = of(d, id)
        .into_iter()
        .flat_map(|p| match p {
            Prim::Line { pts, .. } => pts.clone(),
            Prim::Fill { rings, .. } => rings.iter().flatten().copied().collect(),
            Prim::Image { .. } => Vec::new(),
        })
        .map(|q| d.view.mm(q))
        .collect();
    let lo = pts.iter().fold(P2(f64::MAX, f64::MAX), |a, q| P2(a.0.min(q.0), a.1.min(q.1)));
    let hi = pts.iter().fold(P2(f64::MIN, f64::MIN), |a, q| P2(a.0.max(q.0), a.1.max(q.1)));
    let tenth = |x: f64| (x * 10.0).round() / 10.0;
    (P2(tenth(lo.0), tenth(lo.1)), P2(tenth(hi.0), tenth(hi.1)))
}

#[test]
fn a_plan_sees_a_run_through_its_cut_plane_overhead_and_under_the_level_and_the_sketch_draws_it_too() {
    for c in CASES {
        let mut s = Session::new();
        // 1200 up: the cut plane passes through it. 2800 up: overhead, under the level above. Hanging with its top at the level.
        let cut = made(&mut s, c.command, json!({"points": [[0, 0], [6000, 0]], "offset": 1200}));
        let over = made(&mut s, c.command, json!({"points": [[0, 1000], [6000, 1000]], "offset": 2800}));
        let high = made(&mut s, c.command, json!({"points": [[0, 4000], [6000, 4000]], "offset": 6000}));
        let d = render::draw(&s.model, 100000, view());
        let half = element_json(&s.model, cut)["width"].as_f64().unwrap() / 2.0;
        assert_eq!(extent(&d, cut), (P2(0.0, -half), P2(6000.0, half)), "{}", c.category);
        assert!(of(&d, cut).iter().any(|p| matches!(p, Prim::Fill { color, .. } if *color == POCHE)), "{} is cut", c.category);
        assert!(
            !of(&d, over).is_empty() && of(&d, over).iter().all(|p| matches!(p, Prim::Line { dash: Some(_), .. })),
            "{} overhead is dashed",
            c.category
        );
        assert!(of(&d, high).is_empty(), "{} over the view range's top is not seen", c.category);
        let sketch = render::sketch(&s.model, view(), 999, &s.model.elements[&cut]);
        assert_eq!(extent(&sketch, 999), (P2(0.0, -half), P2(6000.0, half)), "{}", c.category);
    }
}

#[test]
fn a_section_cuts_a_run_across_its_section_a_round_one_in_a_ring() {
    for c in CASES {
        let mut s = Session::new();
        let a = made(&mut s, c.command, json!({"points": [[0, 0], [6000, 0]], "offset": 2400}));
        let section = made(&mut s, "section", json!({"start": [3000, -2000], "end": [3000, 2000]}));
        let d = render::section(&s.model, section, View { scale: 0.1, cx: 2000.0, cy: 1500.0, width: 1000, height: 800 });
        let w = element_json(&s.model, a)["width"].as_f64().unwrap();
        let (lo, hi) = extent(&d, a);
        // Across the line at 2000 along it, centred on 2400 up; a round one's ring is a polygon in its circle, to the tenth.
        let (dx, dz) = if c.round { (w / 2.0, w / 2.0) } else { (w / 2.0, element_json(&s.model, a)["depth"].as_f64().unwrap() / 2.0) };
        let tol = if c.round { 0.2 * w / 2.0 / 50.0 + 0.5 } else { 0.05 };
        assert!(
            (lo.0 - (2000.0 - dx)).abs() <= tol + 0.1
                && (hi.0 - (2000.0 + dx)).abs() <= tol + 0.1
                && (lo.1 - (2400.0 - dz)).abs() <= tol + 0.1
                && (hi.1 - (2400.0 + dz)).abs() <= tol + 0.1,
            "{}: {lo:?} {hi:?}",
            c.category
        );
        assert!(of(&d, a).iter().any(|p| matches!(p, Prim::Fill { color, .. } if *color == POCHE)), "{} is cut in poché", c.category);
    }
}

#[test]
fn the_3d_view_has_a_solid_for_a_run() {
    for c in CASES {
        let mut s = Session::new();
        let a = made(&mut s, c.command, json!({"points": [[0, 0], [6000, 0]], "offset": 2400}));
        let solids = crate::view3d::solids(&s.model);
        let mine: Vec<_> = solids.iter().filter(|x| x.owner() == a).collect();
        let w = element_json(&s.model, a)["width"].as_f64().unwrap();
        let depth = element_json(&s.model, a)["depth"].as_f64().unwrap();
        let (lo, hi) = mine[0].heights();
        assert!((lo - (2400.0 - depth / 2.0)).abs() < 1e-6 && (hi - (2400.0 + depth / 2.0)).abs() < 1e-6, "{}: {lo} {hi} {w}", c.category);
        assert_eq!(matches!(mine.as_slice(), [crate::view3d::Solid::Mesh { round: true, .. }]), c.round, "{}", c.category);
    }
}

#[test]
fn points_picked_snap_to_the_ends_and_middle_of_a_run() {
    use crate::snap::{Kind, Reach, Snaps, snap};
    for c in CASES {
        let mut s = Session::new();
        made(&mut s, c.command, json!({"points": [[0, 0], [6000, 0]], "offset": 2400}));
        let reach = Reach { point: 100.0, line: 60.0 };
        let at = |p: P2| {
            let f = snap(&s.model, 100000, p, reach, &[], false, &Snaps::default());
            (f.at, f.kind)
        };
        assert_eq!(at(P2(6040.0, 30.0)), (P2(6000.0, 0.0), Kind::Endpoint), "{}", c.category);
        assert_eq!(at(P2(3040.0, 30.0)), (P2(3000.0, 0.0), Kind::Midpoint), "{}", c.category);
    }
}

#[test]
fn a_run_of_the_mep_has_a_schedule_by_system_hides_by_category_and_holds_project_parameters() {
    for c in CASES {
        let mut s = Session::new();
        let a = made(&mut s, c.command, json!({"points": [[0, 0], [6000, 0]], "system": "Sistema A"}));
        made(&mut s, c.command, json!({"points": [[0, 3000], [3000, 3000]], "system": "Sistema B"}));
        let r = run(&mut s, "schedule", json!({"name": "S", "category": plural(c)}));
        assert_eq!(r["columns"], columns(c), "{}", c.category);
        assert_eq!(r["rows"].as_array().unwrap().len(), 2);
        let r = run(
            &mut s,
            "schedule",
            json!({"name": "S", "fields": ["System", "Count", {"name": "Length", "total": true}], "sort": ["System"], "grouped": true}),
        );
        assert_eq!(r["rows"], json!([["Sistema A", 1.0, 6000.0], ["Sistema B", 1.0, 3000.0]]), "{}", c.category);
        assert_eq!(r["totals"], json!([null, null, 9000.0]));
        // Hidden by category in the plan.
        let shown = |s: &Session| render::draw(&s.model, 100000, view()).owners.iter().filter(|o| **o == Some(a)).count();
        assert!(shown(&s) > 0, "{}", c.category);
        run(&mut s, "graphics", json!({"view": "Level 1", "hide": [c.category]}));
        assert_eq!(shown(&s), 0, "{}", c.category);
        // A project parameter for the category, and a value of it.
        run(&mut s, "parameter", json!({"name": "Circuito", "kind": "text", "categories": [c.category]}));
        run(&mut s, "modify", json!({"id": a, "params": {"Circuito": "C1"}}));
        assert_eq!(element_json(&s.model, a)["params"]["Circuito"], "C1");
    }
}

// ---------- IFC ----------

fn ifc(s: &mut Session) -> String {
    let path = std::env::temp_dir().join(format!("bimcraft-test-{}-{}.ifc", std::process::id(), s.model.uid));
    run(s, "save", json!({"path": path}));
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(path);
    text
}

fn entities<'a>(file: &'a str, name: &str) -> Vec<&'a str> {
    let head = format!("={name}(");
    file.lines().filter(|l| l.contains(&head)).collect()
}

#[test]
fn a_run_of_the_mep_is_written_as_its_ifc_class_swept_along_its_axis_in_a_system() {
    for c in CASES {
        let mut s = Session::new();
        made(&mut s, c.command, json!({"points": [[0, 0], [3000, 0]], "offset": 0, "end_offset": 4000, "system": "Cold water"}));
        made(&mut s, c.command, json!({"points": [[0, 1000], [3000, 1000]], "system": "Cold water"}));
        made(&mut s, c.command, json!({"points": [[0, 2000], [3000, 2000]], "system": "Esgoto"}));
        let file = ifc(&mut s);
        let class = entities(&file, c.class);
        assert_eq!(class.len(), 3, "{}: {file}", c.category);
        assert!(class[0].contains(&format!("'{}'", c.default_type)) && class[0].ends_with(&format!("{});", c.predefined)), "{}", class[0]);
        let profile = if c.round { "IFCCIRCLEPROFILEDEF" } else { "IFCRECTANGLEPROFILEDEF" };
        assert_eq!(entities(&file, profile).len(), 3, "{}", c.category);
        // The sloped one is aimed along (3, 0, 4) / 5 and is 5000 long.
        assert!(
            file.contains("IFCDIRECTION((0.6,0.,0.8))") && entities(&file, "IFCEXTRUDEDAREASOLID")[0].ends_with(",5000.);"),
            "{}",
            c.category
        );
        // A system is an IfcSystem the runs in it are assigned to: two systems, three runs.
        let systems = entities(&file, "IFCSYSTEM");
        assert_eq!(systems.len(), 2, "{}", c.category);
        assert!(systems.iter().any(|l| l.contains("'Cold water'")) && systems.iter().any(|l| l.contains("'Esgoto'")));
        let assigns = entities(&file, "IFCRELASSIGNSTOGROUP");
        assert_eq!(assigns.len(), 2, "{}", c.category);
        assert!(assigns.iter().any(|l| l.matches('#').count() == 4), "two runs and the system, in one relationship: {assigns:?}");
        // Nothing in a system when it names none.
        made(&mut s, c.command, json!({"points": [[0, 3000], [3000, 3000]]}));
        assert_eq!(entities(&ifc(&mut s), "IFCSYSTEM").len(), 2);
    }
}

// ---------- IFC, back in ----------

/// What a run is and where, to the tenth of a millimetre, whatever its id: its section, its system, where its justify line is
/// at each end, and which way it is drawn left to the file's say (a run comes back running east, or north, or up).
fn described(m: &crate::model::Model, category: &str) -> Vec<String> {
    let tenths = |v: &Value| format!("{:.1}", v.as_f64().unwrap_or(f64::NAN) + 0.0);
    let mut out: Vec<String> = m
        .elements
        .iter()
        .filter(|(_, e)| e.category() == category)
        .map(|(id, _)| {
            let e = element_json(m, *id);
            let end = |k: &str, h: &Value| format!("{},{},{}", tenths(&e[k][0]), tenths(&e[k][1]), tenths(h));
            let (z0, z1) = (e["offset"].clone(), if e["end_offset"].is_null() { e["offset"].clone() } else { e["end_offset"].clone() });
            let mut ends = [end("start", &z0), end("end", &z1)];
            ends.sort();
            format!(
                "{}x{} round={} length={} justify={} system={} ends={}->{}",
                tenths(&e["width"]),
                tenths(&e["depth"]),
                e["round"],
                tenths(&e["length"]),
                e["justify"],
                e["system"],
                ends[0],
                ends[1]
            )
        })
        .collect();
    out.sort();
    out
}

fn the_mep(s: &mut Session) {
    for c in CASES {
        let cmd = c.command;
        run(s, cmd, json!({"points": [[0, 0], [6000, 0]], "offset": 2400, "system": "Cold water"}));
        run(s, cmd, json!({"points": [[0, 3000], [4000, 3000]], "offset": 2400, "end_offset": 2340, "justify": "top", "system": "Sewer"}));
        run(s, cmd, json!({"points": [[500, 5000], [3500, 4000]], "offset": 1000, "end_offset": 1900, "justify": "bottom"}));
        // Drawn west, and running north-north-west, whose direction the body's points alone cannot sign.
        run(s, cmd, json!({"points": [[6000, 6500], [0, 6500]], "offset": 2000, "system": "Cold water"}));
        run(s, cmd, json!({"points": [[7500, 500], [6500, 4500]], "offset": 2000, "justify": "top"}));
        // A riser: upright, its heights from end to end; and one drawn down.
        run(s, cmd, json!({"points": [[8000, 1000], [8000, 1000]], "offset": 0, "end_offset": 3000, "system": "Sewer"}));
        run(s, cmd, json!({"points": [[9000, 1000], [9000, 1000]], "offset": 3000, "end_offset": 500}));
        run(s, "type", json!({"category": c.category, "name": "Mine 75", "width": 75, "depth": 75, "round": c.round, "material": "Metal"}));
        run(s, cmd, json!({"points": [[0, 8000], [3000, 8000]], "type": "Mine 75"}));
    }
}

#[test]
fn the_runs_of_the_mep_come_back_from_ifc_with_their_justification_and_their_system() {
    let mut s = Session::new();
    the_mep(&mut s);
    let path = std::env::temp_dir().join(format!("bimcraft-test-{}-mep-round.ifc", std::process::id()));
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    let r = run(&mut t, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(r["skipped"], json!([]), "{r}");
    for c in CASES {
        let (before, after) = (described(&s.model, c.category), described(&t.model, c.category));
        assert_eq!(before.len(), 8, "{}", c.category);
        assert_eq!(after, before, "{}", c.category);
        // The type they were of came with them, in its material.
        let mine = &t.model.types.runs[c.category]["Mine 75"];
        assert_eq!((mine.width, mine.round, mine.material.as_str()), (75.0, c.round, "Metal"), "{}", c.category);
    }
    assert!(t.model.check().is_ok());
}

#[test]
fn a_system_an_ifc_file_names_comes_back_on_its_runs_and_a_class_alone_says_the_category() {
    for c in CASES.iter().filter(|c| c.alone) {
        let mut s = Session::new();
        made(&mut s, c.command, json!({"points": [[0, 0], [3000, 0]], "system": "Cold water"}));
        let path = std::env::temp_dir().join(format!("bimcraft-test-{}-mep-system-{}.ifc", std::process::id(), c.category));
        run(&mut s, "save", json!({"path": path}));
        let text = std::fs::read_to_string(&path).unwrap();
        // Another tool's file: the run and its system, with BIMCraft's own property set taken out.
        let other: String = text.lines().filter(|l| !l.contains("BIMCraft_Run")).collect::<Vec<_>>().join("\n");
        std::fs::write(&path, other).unwrap();
        let mut t = Session::new();
        run(&mut t, "open", json!({"path": path}));
        let _ = std::fs::remove_file(&path);
        let id = t.model.elements.iter().find(|(_, e)| e.category() == c.category).map(|(id, _)| *id);
        let e = element_json(&t.model, id.unwrap_or_else(|| panic!("a {} came back", c.category)));
        assert_eq!((e["system"].as_str(), e["justify"].as_str()), (Some("Cold water"), Some("centre")), "{}", c.category);
    }
}

// ---------- fixtures and devices ----------

use crate::family::Category;
use crate::language::Language;

/// A starter family of a tab (in English), its category, its IFC class and predefined type, and where its forms are over its base
/// (mm: the range its lowest point may be in, and its highest), so a fixture's drawing is checked against what it is.
struct Fixture {
    family: &'static str,
    category: Category,
    class: &'static str,
    predefined: &'static str,
    /// The object type a proxy carries, if the class has no home for it.
    object_type: Option<&'static str>,
    base: (f64, f64),
    top: (f64, f64),
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        family: "WC",
        category: Category::Plumbing,
        class: "IFCSANITARYTERMINAL",
        predefined: ".NOTDEFINED.",
        object_type: None,
        base: (0.0, 0.0),
        top: (700.0, 900.0),
    },
    Fixture {
        family: "Washbasin",
        category: Category::Plumbing,
        class: "IFCSANITARYTERMINAL",
        predefined: ".NOTDEFINED.",
        object_type: None,
        base: (0.0, 0.0),
        top: (800.0, 900.0),
    },
    Fixture {
        family: "Kitchen Sink",
        category: Category::Plumbing,
        class: "IFCSANITARYTERMINAL",
        predefined: ".NOTDEFINED.",
        object_type: None,
        base: (0.0, 0.0),
        top: (850.0, 1300.0),
    },
    Fixture {
        family: "Shower",
        category: Category::Plumbing,
        class: "IFCSANITARYTERMINAL",
        predefined: ".NOTDEFINED.",
        object_type: None,
        base: (0.0, 0.0),
        top: (2000.0, 2200.0),
    },
    Fixture {
        family: "Floor Drain",
        category: Category::Drain,
        class: "IFCWASTETERMINAL",
        predefined: ".FLOORTRAP.",
        object_type: None,
        base: (-100.0, 0.0),
        top: (0.0, 0.0),
    },
    Fixture {
        family: "Elbow 90",
        category: Category::PipeFitting,
        class: "IFCPIPEFITTING",
        predefined: ".NOTDEFINED.",
        object_type: None,
        base: (-100.0, -10.0),
        top: (10.0, 100.0),
    },
    Fixture {
        family: "Tee",
        category: Category::PipeFitting,
        class: "IFCPIPEFITTING",
        predefined: ".NOTDEFINED.",
        object_type: None,
        base: (-100.0, -10.0),
        top: (10.0, 100.0),
    },
    Fixture {
        family: "Gate Valve",
        category: Category::PipeAccessory,
        class: "IFCVALVE",
        predefined: ".NOTDEFINED.",
        object_type: None,
        base: (-100.0, -10.0),
        top: (40.0, 300.0),
    },
    Fixture {
        family: "Distribution Board",
        category: Category::Equipment,
        class: "IFCELECTRICDISTRIBUTIONBOARD",
        predefined: ".DISTRIBUTIONBOARD.",
        object_type: None,
        base: (1200.0, 1400.0),
        top: (1700.0, 2300.0),
    },
    Fixture {
        family: "Ceiling Luminaire",
        category: Category::Lighting,
        class: "IFCLIGHTFIXTURE",
        predefined: ".NOTDEFINED.",
        object_type: None,
        base: (2900.0, 3000.0),
        top: (2950.0, 3050.0),
    },
    Fixture {
        family: "Recessed Luminaire",
        category: Category::Lighting,
        class: "IFCLIGHTFIXTURE",
        predefined: ".NOTDEFINED.",
        object_type: None,
        base: (2900.0, 3000.0),
        top: (2950.0, 3050.0),
    },
    Fixture {
        family: "Outlet",
        category: Category::Outlet,
        class: "IFCOUTLET",
        predefined: ".POWEROUTLET.",
        object_type: None,
        base: (200.0, 2100.0),
        top: (300.0, 2200.0),
    },
    Fixture {
        family: "Three-Phase Outlet",
        category: Category::Outlet,
        class: "IFCOUTLET",
        predefined: ".POWEROUTLET.",
        object_type: None,
        base: (200.0, 2100.0),
        top: (300.0, 2200.0),
    },
    Fixture {
        family: "Switch",
        category: Category::Switch,
        class: "IFCSWITCHINGDEVICE",
        predefined: ".TOGGLESWITCH.",
        object_type: None,
        base: (200.0, 1200.0),
        top: (300.0, 1300.0),
    },
    Fixture {
        family: "Air Terminal",
        category: Category::AirTerminal,
        class: "IFCBUILDINGELEMENTPROXY",
        predefined: ".USERDEFINED.",
        object_type: Some("LightningTerminal"),
        base: (-1.0, 1.0),
        top: (400.0, 2100.0),
    },
    Fixture {
        family: "Earthing Electrode",
        category: Category::Earthing,
        class: "IFCBUILDINGELEMENTPROXY",
        predefined: ".USERDEFINED.",
        object_type: Some("EarthingRod"),
        base: (-3100.0, -2300.0),
        top: (0.0, 0.0),
    },
    Fixture {
        family: "Inspection Box",
        category: Category::InspectionBox,
        class: "IFCBUILDINGELEMENTPROXY",
        predefined: ".USERDEFINED.",
        object_type: Some("InspectionBox"),
        base: (-350.0, -200.0),
        top: (0.0, 0.0),
    },
    Fixture {
        family: "Data Outlet",
        category: Category::DataOutlet,
        class: "IFCOUTLET",
        predefined: ".DATAOUTLET.",
        object_type: None,
        base: (200.0, 1200.0),
        top: (300.0, 1300.0),
    },
    Fixture {
        family: "Voice Outlet",
        category: Category::VoiceOutlet,
        class: "IFCOUTLET",
        predefined: ".TELEPHONEOUTLET.",
        object_type: None,
        base: (200.0, 1200.0),
        top: (300.0, 1300.0),
    },
    Fixture {
        family: "Patch Panel",
        category: Category::PatchPanel,
        class: "IFCCOMMUNICATIONSAPPLIANCE",
        predefined: ".NETWORKAPPLIANCE.",
        object_type: None,
        base: (1400.0, 1600.0),
        top: (1500.0, 1700.0),
    },
    Fixture {
        family: "Rack",
        category: Category::Rack,
        class: "IFCBUILDINGELEMENTPROXY",
        predefined: ".USERDEFINED.",
        object_type: Some("Rack"),
        base: (-1.0, 1.0),
        top: (1900.0, 2100.0),
    },
];

#[test]
fn a_new_project_has_the_starter_fixtures_of_its_tabs_in_every_language_and_each_one_builds() {
    for l in Language::ALL {
        let m = crate::model::Model::new_in("x", l);
        assert_eq!(m.check(), Ok(()), "{l:?}: every family builds, every type of it");
        for f in FIXTURES {
            let name = l.word(f.family);
            let family = m.families.get(name).unwrap_or_else(|| panic!("{l:?}: no family `{name}`"));
            assert_eq!(family.category, f.category, "{l:?} {name}");
            assert!(!family.types.is_empty(), "{l:?} {name}");
        }
    }
}

#[test]
fn a_starter_fixture_stands_as_it_is_and_every_type_of_it_has_a_body() {
    let mut s = Session::new();
    for f in FIXTURES {
        let types: Vec<String> = s.model.families[f.family].types.keys().cloned().collect();
        for ty in types {
            let id = made(&mut s, "component", json!({"family": f.family, "type": ty, "at": [1000, 1000]}));
            let Some(crate::model::Element::Component(c)) = s.model.elements.get(id) else { panic!("{} is a component", f.family) };
            let pieces = s.model.component_pieces(c);
            assert!(!pieces.is_empty(), "{} {ty}", f.family);
            let top = pieces.iter().map(|p| p.top).fold(f64::MIN, f64::max);
            let bottom = pieces.iter().map(|p| p.bottom).fold(f64::MAX, f64::min);
            assert!(top >= f.top.0 - 1e-6 && top <= f.top.1 + 1e-6, "{} {ty}: reaches {top}", f.family);
            assert!(bottom >= f.base.0 - 1e-6 && bottom <= f.base.1 + 1e-6, "{} {ty}: starts at {bottom}", f.family);
        }
    }
}

#[test]
fn a_category_of_family_is_written_as_its_ifc_class_and_read_back_as_the_same_category() {
    for f in FIXTURES {
        assert_eq!(f.category.ifc(), (f.class, f.predefined, f.object_type), "{}", f.family);
        assert_eq!(Category::from_ifc(f.class, Some(f.predefined.trim_matches('.')), f.object_type), Some(f.category), "{}", f.family);
    }
    // A class BIMCraft makes no family of, or a proxy that says nothing, is the generic one's, or none.
    assert_eq!(Category::from_ifc("IFCBEAM", None, None), None);
    assert_eq!(Category::from_ifc("IFCBUILDINGELEMENTPROXY", None, None), Some(Category::Generic));
    assert_eq!(Category::from_ifc("IFCFURNISHINGELEMENT", None, None), Some(Category::Furniture));
    // An outlet of no type, or of one BIMCraft has no category for, is the power outlet, the class's first; a type it has is its own.
    assert_eq!(Category::from_ifc("IFCOUTLET", None, None), Some(Category::Outlet));
    assert_eq!(Category::from_ifc("IFCOUTLET", Some("NOTDEFINED"), None), Some(Category::Outlet));
    assert_eq!(Category::from_ifc("IFCOUTLET", Some("TELEPHONEOUTLET"), None), Some(Category::VoiceOutlet));
    // A proxy of an object type no category names is the generic one's.
    assert_eq!(Category::from_ifc("IFCBUILDINGELEMENTPROXY", Some("USERDEFINED"), Some("Sculpture")), Some(Category::Generic));
}

#[test]
fn fixtures_are_ifc_products_of_their_class_and_come_back_in_a_family_of_their_category() {
    let mut s = Session::new();
    for f in FIXTURES {
        let ty = s.model.families[f.family].types.keys().next().cloned().unwrap();
        made(&mut s, "component", json!({"family": f.family, "type": ty, "at": [1000, 1000]}));
    }
    let file = ifc(&mut s);
    for f in FIXTURES {
        let written: Vec<&str> = entities(&file, f.class).into_iter().filter(|l| l.contains(&format!("'{}:", f.family))).collect();
        assert_eq!(written.len(), 1, "{}: {file}", f.family);
        assert!(written[0].ends_with(&format!("{});", f.predefined)), "{}: {}", f.family, written[0]);
        if let Some(object) = f.object_type {
            assert!(written[0].contains(&format!(",'{object}',")), "{}: its object type says what it is: {}", f.family, written[0]);
        }
    }
    let path = std::env::temp_dir().join(format!("bimcraft-test-{}-fixtures.ifc", std::process::id()));
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    let r = run(&mut t, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(r["skipped"], json!([]), "{r}");
    for f in FIXTURES {
        let family = t.model.families.get(f.family).unwrap_or_else(|| panic!("{} came back", f.family));
        assert_eq!(family.category, f.category, "{}", f.family);
    }
    assert_eq!(t.model.elements.values().filter(|e| e.category() == "component").count(), FIXTURES.len());
    // Each in the family it was of: the library's of the same name would stand in for one the import named wrongly.
    let named: std::collections::BTreeSet<String> = t
        .model
        .elements
        .iter()
        .filter(|(_, e)| e.category() == "component")
        .filter_map(|(id, _)| element_json(&t.model, *id)["family"].as_str().map(String::from))
        .collect();
    let wanted: std::collections::BTreeSet<String> = FIXTURES.iter().map(|f| f.family.to_string()).collect();
    assert_eq!(named, wanted);
}

#[test]
fn an_upright_run_comes_back_running_up_whichever_end_it_was_drawn_from() {
    for c in CASES {
        let mut s = Session::new();
        // Drawn downward: from 3000 to 500 over its level.
        made(&mut s, c.command, json!({"points": [[2000, 2000], [2000, 2000]], "offset": 3000, "end_offset": 500}));
        let path = std::env::temp_dir().join(format!("bimcraft-test-{}-riser-{}.ifc", std::process::id(), c.category));
        run(&mut s, "save", json!({"path": path}));
        let mut t = Session::new();
        run(&mut t, "open", json!({"path": path}));
        let _ = std::fs::remove_file(&path);
        let id = t.model.elements.iter().find(|(_, e)| e.category() == c.category).map(|(id, _)| *id).unwrap();
        let e = element_json(&t.model, id);
        assert_eq!((e["offset"].as_f64(), e["end_offset"].as_f64()), (Some(500.0), Some(3000.0)), "{}: {e}", c.category);
    }
}

// ---------- connectors ----------

use crate::ids::FIRST_ID;
use crate::model::Model;

fn warnings(s: &mut Session) -> Vec<String> {
    run(s, "inspect", json!({}))["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap().to_string()).collect()
}

fn connectors(s: &Session, id: u64) -> Vec<Value> {
    element_json(&s.model, id)["connectors"].as_array().cloned().unwrap_or_default()
}

#[test]
fn the_starter_devices_carry_connectors_in_every_language_and_a_switch_carries_no_load() {
    for l in Language::ALL {
        let m = Model::new_in("x", l);
        assert_eq!(m.check(), Ok(()), "{l:?}");
        for (family, system, poles, voltage) in [
            ("Ceiling Luminaire", "power-balanced", 1, 127.0),
            ("Recessed Luminaire", "power-balanced", 1, 127.0),
            ("Outlet", "power-balanced", 1, 127.0),
            ("Three-Phase Outlet", "power-unbalanced", 3, 220.0),
        ] {
            let f = &m.families[l.word(family)];
            assert_eq!(f.connectors.len(), 1, "{l:?} {family}");
            let c = &f.connectors[0];
            assert_eq!((serde_json::to_value(c.system).unwrap(), c.poles), (json!(system), poles), "{l:?} {family}");
            for t in f.types.keys() {
                let v = &f.connector_values(t, &Default::default()).unwrap()[0];
                assert_eq!((v.voltage, v.power_factor > 0.0, v.load_va > 0.0), (voltage, true, true), "{l:?} {family} {t}");
            }
        }
        let switch = &m.families[l.word("Switch")];
        assert_eq!(switch.connectors.len(), 1, "{l:?}");
        let v = &switch.connector_values("Medium 1100", &Default::default()).unwrap()[0];
        assert_eq!((v.kind, v.load_va, v.load_va_by_phase.is_empty()), (crate::family::ConnectorKind::Switch, 0.0, true), "{l:?}");
        // A board is where the loads end up, not a load; the data and voice outlets are a tab of their own.
        assert!(m.families[l.word("Distribution Board")].connectors.is_empty(), "{l:?}");
    }
}

#[test]
fn a_placed_device_reports_its_load_and_classification_and_the_type_sets_the_load() {
    let mut s = Session::new();
    let small = made(&mut s, "component", json!({"family": "Ceiling Luminaire", "type": "300", "at": [1000, 1000]}));
    let large = made(&mut s, "component", json!({"family": "Ceiling Luminaire", "type": "400", "at": [2000, 1000]}));
    let (a, b) = (&connectors(&s, small)[0], &connectors(&s, large)[0]);
    assert_eq!((a["load_va"].as_f64(), b["load_va"].as_f64()), (Some(12.0), Some(18.0)));
    assert_eq!(
        (a["classification"].as_str(), a["kind"].as_str(), a["system"].as_str()),
        (Some("lighting"), Some("power"), Some("power-balanced"))
    );
    assert_eq!((a["poles"].as_u64(), a["voltage"].as_f64()), (Some(1), Some(127.0)));
    assert_eq!(a["load_va_by_phase"], json!([12.0]));
    assert!(a["guid"].is_string() && a["id"].as_u64().unwrap() >= FIRST_ID, "a connector has an id and a GUID: {a}");
    // The switch reports its connector, with no load.
    let switch = made(&mut s, "component", json!({"family": "Switch", "at": [3000, 1000]}));
    let c = &connectors(&s, switch)[0];
    assert_eq!((c["kind"].as_str(), c["load_va"].as_f64(), c["load_va_by_phase"].clone()), (Some("switch"), Some(0.0), json!([])));
    // A device of a family without any has no `connectors` to report.
    let board = made(&mut s, "component", json!({"family": "Distribution Board", "at": [4000, 1000]}));
    assert!(element_json(&s.model, board).get("connectors").is_none());
}

#[test]
fn an_unbalanced_device_may_load_each_phase_differently() {
    let mut s = Session::new();
    let even = made(&mut s, "component", json!({"family": "Three-Phase Outlet", "type": "Balanced 1100", "at": [1000, 1000]}));
    let uneven = made(&mut s, "component", json!({"family": "Three-Phase Outlet", "type": "Unbalanced 1100", "at": [2000, 1000]}));
    let (a, b) = (&connectors(&s, even)[0], &connectors(&s, uneven)[0]);
    assert_eq!(a["load_va_by_phase"], json!([2000.0, 2000.0, 2000.0]));
    assert_eq!(b["load_va_by_phase"], json!([3000.0, 2000.0, 1000.0]));
    assert_eq!((a["load_va"].as_f64(), b["load_va"].as_f64()), (Some(6000.0), Some(6000.0)));
    // A component sets the load of a phase of its own too, when the family makes that parameter its instance's.
    run(
        &mut s,
        "family",
        json!({"name": "Spot", "category": "outlet", "types": {"A": {}},
        "parameters": [{"name": "L1", "kind": "number", "instance": true, "value": 100}],
        "connectors": [{"kind": "power", "system": "power-unbalanced", "poles": 2, "voltage": 220, "load_va": ["L1", 50]}]}),
    );
    let spot = made(&mut s, "component", json!({"family": "Spot", "at": [0, 0], "params": {"L1": 700}}));
    assert_eq!(connectors(&s, spot)[0]["load_va_by_phase"], json!([700.0, 50.0]));
}

#[test]
fn a_family_with_two_power_connectors_is_refused_and_nothing_changes() {
    let mut s = Session::new();
    let power = json!({"kind": "power", "system": "power-balanced", "voltage": 127, "load_va": 10});
    let e = fails(&mut s, "family", json!({"name": "Twin", "category": "lighting", "types": {"A": {}}, "connectors": [power, power]}));
    assert!(e.contains("two power connectors") && e.contains("keep one"), "{e}");
    assert!(!s.model.families.contains_key("Twin"));
    // A misspelled key of a connector is an error, as it is in every command.
    let e = fails(
        &mut s,
        "family",
        json!({"name": "Twin", "types": {"A": {}}, "connectors": [{"kind": "power", "system": "power-balanced", "load": 5}]}),
    );
    assert!(e.contains("connectors") && e.contains("load"), "{e}");
}

#[test]
fn a_device_without_a_connector_is_warned_of_not_refused_and_old_families_still_place() {
    let mut s = Session::new();
    run(&mut s, "family", json!({"name": "Bare", "category": "lighting", "types": {"A": {}}}));
    assert_eq!(warnings(&mut s), Vec::<String>::new(), "a family nothing is placed of says nothing");
    made(&mut s, "component", json!({"family": "Bare", "at": [0, 0]}));
    made(&mut s, "component", json!({"family": "Bare", "at": [500, 0]}));
    made(&mut s, "component", json!({"family": "Outlet", "at": [1000, 0]}));
    let w = warnings(&mut s);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("`Bare`") && w[0].contains("2 placed devices") && w[0].contains("zero"), "{w:?}");
    assert_eq!(s.model.check(), Ok(()), "a warning is no error");
    assert_eq!(s.model.warnings(), w);
    // A furniture family is no electrical device.
    run(&mut s, "family", json!({"name": "Stool", "category": "furniture", "types": {"A": {}}}));
    made(&mut s, "component", json!({"family": "Stool", "at": [0, 500]}));
    assert_eq!(warnings(&mut s), w);
    // Giving it a connector answers the warning.
    run(
        &mut s,
        "family",
        json!({"name": "Bare", "connectors": [{"kind": "power", "system": "power-balanced", "voltage": 127, "load_va": 9}]}),
    );
    assert_eq!(warnings(&mut s), Vec::<String>::new());
}

#[test]
fn a_file_from_before_connectors_opens_and_its_devices_place() {
    let mut s = Session::new();
    made(&mut s, "component", json!({"family": "Outlet", "at": [0, 0]}));
    let mut v: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(v["format"], json!(12));
    v["format"] = json!(11);
    for f in v["families"].as_object_mut().unwrap().values_mut() {
        f.as_object_mut().unwrap().remove("connectors");
    }
    let (m, left_out) = Model::read(&v.to_string()).unwrap();
    assert_eq!((m.format, left_out.len()), (crate::model::FORMAT, 0));
    assert!(m.families.values().all(|f| f.connectors.is_empty()));
    assert_eq!(m.check(), Ok(()));
    let mut t = Session::new();
    *t.model_mut() = m;
    made(&mut t, "component", json!({"family": "Outlet", "at": [500, 0]}));
    // It is a family of lights and outlets without a load now, which is said, not refused.
    assert!(warnings(&mut t).iter().any(|w| w.contains("`Outlet`") && w.contains("2 placed devices")));
    // And a build of the next format is refused, as ever.
    v["format"] = json!(crate::model::FORMAT + 1);
    assert!(Model::read(&v.to_string()).is_err());
}

#[test]
fn a_connector_keeps_its_id_and_guid_while_the_family_is_edited_and_a_family_file_keeps_connectors() {
    let mut s = Session::new();
    let power = json!({"kind": "power", "system": "power-balanced", "voltage": 127, "load_va": 9, "classification": "lighting"});
    run(&mut s, "family", json!({"name": "Lamp", "category": "lighting", "types": {"A": {}}, "connectors": [power]}));
    let first = s.model.families["Lamp"].connectors[0].clone();
    assert!(first.id >= FIRST_ID && !first.guid.is_nil(), "a part is given an id and a GUID");
    let mut again = power.clone();
    again["load_va"] = json!(12);
    run(&mut s, "family", json!({"name": "Lamp", "connectors": [again]}));
    let second = &s.model.families["Lamp"].connectors[0];
    assert_eq!((second.id, second.guid), (first.id, first.guid), "the one at its place stays itself");
    // The file a family is saved to holds them, and loading it brings them back.
    let path = std::env::temp_dir().join(format!("bimcraft-test-{}-lamp.json", std::process::id()));
    run(&mut s, "family", json!({"name": "Lamp", "save": path}));
    let mut t = Session::new();
    run(&mut t, "family", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    let loaded = &t.model.families["Lamp"].connectors;
    assert_eq!(loaded.len(), 1);
    assert_eq!(
        (loaded[0].classification.as_str(), loaded[0].load_va.clone()),
        ("lighting", crate::family::Load::One(crate::family::Expr::Number(12.0)))
    );
    // Saved with the project and opened, they are the same connectors, by GUID.
    let project = std::env::temp_dir().join(format!("bimcraft-test-{}-lamp.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": project}));
    let mut u = Session::new();
    run(&mut u, "open", json!({"path": project}));
    let _ = std::fs::remove_file(&project);
    assert_eq!(u.model.families["Lamp"].connectors[0].guid, first.guid);
}

#[test]
fn a_placed_device_is_written_to_ifc_with_the_electrical_device_property_set() {
    let mut s = Session::new();
    made(&mut s, "component", json!({"family": "Ceiling Luminaire", "type": "400", "at": [0, 0]}));
    made(&mut s, "component", json!({"family": "Three-Phase Outlet", "type": "Unbalanced 1100", "at": [1000, 0]}));
    made(&mut s, "component", json!({"family": "Distribution Board", "at": [2000, 0]}));
    let file = ifc(&mut s);
    let sets =
        |name: &str| entities(&file, "IFCPROPERTYSET").into_iter().filter(|l| l.contains(&format!(",'{name}',"))).collect::<Vec<_>>();
    // A device holding a connector, not the board, and not a set for each connector twice.
    assert_eq!(sets("Pset_ElectricalDeviceCommon").len(), 2, "{file}");
    assert_eq!(sets("BIMCraft_Connector").len(), 2, "{file}");
    // What each holds, by the property it names: a single value is one line of the file.
    let has = |text: &str| file.contains(text);
    for what in [
        "'NumberOfPoles',$,IFCINTEGER(1)",
        "'NumberOfPoles',$,IFCINTEGER(3)",
        "'PowerFactor',$,IFCNORMALISEDRATIOMEASURE(0.95)",
        "'RatedVoltage',$,$,$,$,IFCELECTRICVOLTAGEMEASURE(127.)",
    ] {
        assert!(has(what), "{what}: {file}");
    }
    // The values are the type's own: 18 VA for the larger luminaire; 3000, 2000 and 1000 for the phases.
    for what in [
        "'LoadVA',$,IFCREAL(18.)",
        "'LoadVA',$,IFCREAL(6000.)",
        "'LoadVAPhase1',$,IFCREAL(3000.)",
        "'LoadVAPhase3',$,IFCREAL(1000.)",
        "'Classification',$,IFCTEXT('lighting')",
        "'System',$,IFCTEXT('power-unbalanced')",
    ] {
        assert!(has(what), "{what}: {file}");
    }
}
