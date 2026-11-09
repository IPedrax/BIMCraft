//! Work planes for families and their components (T4, T13): Edit Work Plane on a family's forms and reference lines, the
//! project's reference planes, work plane-based components hosted on them (tilted with the plane or kept upright by Always
//! Vertical), following the plane when it moves, their Schedule Level, Elevation from Level, Work Plane and Offset from Host,
//! a family's Default Elevation, and files of 17 read by 19.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, FORMAT, Id, Model};
use crate::view3d::{dot, sub};

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
    v["id"].as_u64().unwrap()
}

const SIN30: f64 = 0.5;
const COS30: f64 = 0.866_025_403_784_438_6;

/// A sign: a box 1000 by 1000, 100 thick, work plane-based (Always Vertical as given), and its Default Elevation 0.
fn sign(s: &mut Session, always_vertical: bool) {
    run(
        s,
        "family",
        json!({"name": "Sign", "category": "generic", "types": {"A": {}},
               "options": {"work_plane_based": true, "always_vertical": always_vertical},
               "forms": [{"profile": [[0, 0], [1000, 0], [1000, 1000], [0, 1000]], "to": 100}]}),
    );
}

/// A reference plane through the x axis rising 30 degrees toward +y: its normal (0, -sin 30, cos 30), its height at plan
/// point (x, y) `y tan 30`.
fn ramp(s: &mut Session) -> Id {
    id_of(&run(s, "reference_plane", json!({"name": "Ramp", "start": [0, 0, 0], "end": [10000, 0, 0], "up": [0, COS30, SIN30]})))
}

fn component(s: &Session, id: Id) -> &crate::model::Component {
    match s.model.elements.get(id) {
        Some(Element::Component(c)) => c,
        other => panic!("{id} is no component: {other:?}"),
    }
}

fn points(s: &Session, id: Id) -> Vec<[f64; 3]> {
    s.model.component_pieces(component(s, id)).into_iter().flat_map(|p| p.tris.into_iter().flatten()).collect()
}

fn json_of(s: &mut Session, id: Id) -> Value {
    let all = run(s, "elements", json!({}));
    all["elements"].as_array().unwrap().iter().find(|e| e["id"] == json!(id)).unwrap().clone()
}

/// A direction's ratio as IFC writes it.
fn ratio(x: f64) -> String {
    format!("{x:.12}").trim_end_matches('0').to_string()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn a_reference_plane_is_made_listed_and_checked() {
    let mut s = Session::new();
    let r = ramp(&mut s);
    let v = json_of(&mut s, r);
    assert_eq!(v["category"], "refplane");
    assert_eq!(v["name"], "Ramp");
    // Drawn in plan: two points on the active level, upright (its `up` the vertical).
    let plan = id_of(&run(&mut s, "reference_plane", json!({"start": [0, 5000], "end": [4000, 5000]})));
    let Some(Element::RefPlane(p)) = s.model.elements.get(plan) else { panic!("a reference plane") };
    assert_eq!((p.up, p.start[2]), ([0.0, 0.0, 1.0], 0.0));
    assert!(err(&mut s, "reference_plane", json!({"start": [0, 0], "end": [0, 0]})).contains("one point"));
    assert!(err(&mut s, "reference_plane", json!({"start": [0, 0, 0], "end": [1000, 0, 0], "up": [3, 0, 0]})).contains("up"));
    assert!(err(&mut s, "reference_plane", json!({"name": "Ramp", "start": [0, 0], "end": [100, 0]})).contains("Ramp"));
    assert!(err(&mut s, "reference_plane", json!({"start": [0, 0, f64::MAX], "end": [100, 0, 0]})).contains("start"));
    // It draws in plan as a dashed line along it, its own to pick.
    let level = s.model.levels()[0].0;
    let d = crate::render::plan(&s.model, level, 800, 600);
    assert_eq!(d.pick(d.view.px(crate::geom::P2(2000.0, 5000.0)), 3.0), Some(plan), "an upright reference plane is drawn in plan");
}

#[test]
fn on_a_sloped_plane_a_component_tilts_with_it_unless_always_vertical() {
    for always_vertical in [false, true] {
        let mut s = Session::new();
        sign(&mut s, always_vertical);
        ramp(&mut s);
        let c = id_of(&run(&mut s, "component", json!({"family": "Sign", "at": [2000, 2000], "host": {"plane": "Ramp"}})));
        let z = 2000.0 * SIN30 / COS30;
        let n = [0.0, -SIN30, COS30];
        let base = [2000.0, 2000.0, z];
        let pts = points(&s, c);
        let along: Vec<f64> = pts.iter().map(|p| dot(sub(*p, base), n)).collect();
        let (lo, hi) = (pts.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min), pts.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max));
        if always_vertical {
            // Upright: its base where the plane is at its point, 100 up.
            assert!(close(lo, z) && close(hi, z + 100.0), "{lo} {hi}");
        } else {
            // Tilted: it sits on the plane, 100 thick along its normal.
            let (a, b) = (along.iter().copied().fold(f64::INFINITY, f64::min), along.iter().copied().fold(f64::NEG_INFINITY, f64::max));
            assert!(close(a, 0.0) && close(b, 100.0), "{a} {b}");
            assert!(hi - lo > 500.0, "it climbs with the plane: {lo} to {hi}");
        }
        // Either way its insertion point is on the plane, its level the highest at or below it, its Elevation from Level
        // that height over it.
        let v = json_of(&mut s, c);
        assert_eq!(v["work_plane"], "Reference Plane : Ramp");
        assert!(close(v["offset"].as_f64().unwrap(), z), "{v}");
        assert_eq!(v["offset_from_host"], 0.0);
        let cp = component(&s, c);
        assert!(close(cp.at.0, 2000.0) && close(cp.at.1, 2000.0));
        // A plan shows it, its outline a closed ring round what it covers.
        let outline: Vec<_> = s.model.component_pieces(cp).into_iter().flat_map(|p| p.outline).collect();
        assert!(outline.len() >= 3);
    }
}

#[test]
fn offset_from_host_lifts_it_along_the_planes_normal() {
    let mut s = Session::new();
    sign(&mut s, false);
    ramp(&mut s);
    let c = id_of(&run(&mut s, "component", json!({"family": "Sign", "at": [2000, 2000], "host": {"plane": "Ramp"}, "offset": 200})));
    let z = 2000.0 * SIN30 / COS30;
    let base = [2000.0, 2000.0 - 200.0 * SIN30, z + 200.0 * COS30];
    let n = [0.0, -SIN30, COS30];
    let along: Vec<f64> = points(&s, c).iter().map(|p| dot(sub(*p, base), n)).collect();
    assert!(close(along.iter().copied().fold(f64::INFINITY, f64::min), 0.0));
    let v = json_of(&mut s, c);
    assert_eq!(v["offset_from_host"], 200.0);
    assert!(close(v["offset"].as_f64().unwrap(), z + 200.0 * COS30), "{v}");
}

#[test]
fn moving_or_turning_the_host_plane_moves_the_instance_and_moving_the_instance_keeps_it_on_the_plane() {
    let mut s = Session::new();
    sign(&mut s, false);
    let r = ramp(&mut s);
    let c = id_of(&run(&mut s, "component", json!({"family": "Sign", "at": [2000, 2000], "host": {"plane": "Ramp"}})));
    run(&mut s, "move", json!({"ids": [r], "by": [500, 1000]}));
    let cp = component(&s, c);
    assert!(close(cp.at.0, 2500.0) && close(cp.at.1, 3000.0), "{:?}", cp.at);
    // The plane moved in plan only: the point keeps its place on it, so its height is as before.
    assert!(close(cp.offset, 2000.0 * SIN30 / COS30), "{}", cp.offset);
    // Moved itself, it slides on the plane: its height is the plane's at its new point.
    run(&mut s, "move", json!({"ids": [c], "by": [0, 1000]}));
    let cp = component(&s, c);
    assert!(close(cp.at.1, 4000.0));
    assert!(close(cp.offset, 3000.0 * SIN30 / COS30), "{}", cp.offset);
    // A copy is on the same plane, where it was put.
    let copied = run(&mut s, "copy", json!({"ids": [c], "by": [1000, 0]}));
    let k = copied["ids"][0].as_u64().unwrap();
    let ck = component(&s, k);
    assert_eq!(ck.host, component(&s, c).host.map(|h| crate::model::Hosted { uv: ck.host.unwrap().uv, ..h }));
    assert!(close(ck.at.0, 3500.0) && close(ck.offset, 3000.0 * SIN30 / COS30));
    // The plane turned about a point, the instance turns with it.
    run(&mut s, "rotate", json!({"ids": [r], "about": [0, 0], "angle": 90}));
    let cp = component(&s, c);
    assert!(close(cp.at.0, -4000.0 + 0.0) && close(cp.at.1, 2500.0), "{:?}", cp.at);
    // Undone, it is back.
    run(&mut s, "undo", json!({}));
    assert!(close(component(&s, c).at.1, 4000.0));
}

#[test]
fn turning_the_host_plane_about_the_vertical_turns_the_instance_with_it() {
    for always_vertical in [true, false] {
        let mut s = Session::new();
        sign(&mut s, always_vertical);
        let r = ramp(&mut s);
        let c = id_of(&run(&mut s, "component", json!({"family": "Sign", "at": [2000, 2000], "host": {"plane": "Ramp"}, "angle": 10})));
        let axes = |s: &Session| s.model.component_placement(component(s, c)).axes;
        let before = axes(&s);
        run(&mut s, "rotate", json!({"ids": [r], "about": [0, 0], "angle": 90}));
        let same = |a: f64, b: f64| ((a - b + 180.0).rem_euclid(360.0) - 180.0).abs() < 1e-4;
        assert!(same(component(&s, c).angle, 100.0), "{}", component(&s, c).angle);
        // Every axis of it turned a quarter about the vertical, as the plane did.
        let quarter = |v: [f64; 3]| [-v[1], v[0], v[2]];
        for (a, b) in axes(&s).into_iter().zip(before) {
            assert!(a.iter().zip(quarter(b)).all(|(x, y)| (x - y).abs() < 1e-9), "{a:?} against {b:?}");
        }
        // Turned with its plane in one command, it turns once.
        run(&mut s, "rotate", json!({"ids": [r, c], "about": [0, 0], "angle": 90}));
        assert!(same(component(&s, c).angle, 190.0), "{}", component(&s, c).angle);
        // Moved, the plane doesn't turn it.
        run(&mut s, "move", json!({"ids": [r], "by": [500, 0]}));
        assert!(same(component(&s, c).angle, 190.0));
    }
}

#[test]
fn schedule_level_moves_nothing_and_a_level_moved_under_it_changes_its_elevation_from_level() {
    let mut s = Session::new();
    sign(&mut s, true);
    ramp(&mut s);
    let levels: Vec<(Id, f64)> = s.model.levels().iter().map(|(id, l)| (*id, l.elevation)).collect();
    let (one, two) = (levels[0], levels[1]);
    // High up the ramp, above Level 2: that's its Schedule Level.
    let y = (two.1 + 600.0) * COS30 / SIN30;
    let c = id_of(&run(&mut s, "component", json!({"family": "Sign", "at": [1000, y], "host": {"plane": "Ramp"}})));
    let cp = component(&s, c);
    assert_eq!(s.model.elements.id(cp.level), Some(two.0));
    assert!(close(cp.offset, 600.0), "{}", cp.offset);
    let top = |s: &Session| points(s, c).iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max);
    let before = top(&s);
    run(&mut s, "modify", json!({"id": c, "level": one.0}));
    assert!(close(top(&s), before), "a new Schedule Level leaves it where it is");
    assert!(close(component(&s, c).offset, two.1 + 600.0));
    run(&mut s, "modify", json!({"id": one.0, "elevation": -1000}));
    assert!(close(top(&s), before), "a level moved under it leaves it on its plane");
    assert!(close(component(&s, c).offset, two.1 + 1600.0));
    assert_eq!(crate::schedule::field_text(&s.model, c, "Work Plane").as_deref(), Some("Reference Plane : Ramp"));
    assert!(crate::schedule::field_text(&s.model, c, "Elevation from Level").is_some());
    assert!(crate::schedule::field_text(&s.model, c, "Offset from Host").is_some());
}

#[test]
fn only_a_work_plane_based_family_is_hosted_and_it_stays_so_while_hosted() {
    let mut s = Session::new();
    sign(&mut s, true);
    let r = ramp(&mut s);
    run(&mut s, "family", json!({"name": "Box", "types": {"A": {}}, "forms": [{"profile": [[0, 0], [10, 0], [10, 10]], "to": 10}]}));
    let e = err(&mut s, "component", json!({"family": "Box", "at": [0, 0], "host": {"plane": r}}));
    assert!(e.contains("work_plane_based"), "{e}");
    let c = id_of(&run(&mut s, "component", json!({"family": "Sign", "at": [0, 1000], "host": {"plane": r}})));
    let e = err(&mut s, "family", json!({"name": "Sign", "options": {"work_plane_based": false}}));
    assert!(e.contains(&c.to_string()) && e.contains("work_plane_based"), "{e}");
    // On a level, it may go.
    run(&mut s, "edit_work_plane", json!({"ids": [c], "host": {"level": "Level 1"}}));
    assert_eq!(component(&s, c).host, None);
    run(&mut s, "family", json!({"name": "Sign", "options": {"work_plane_based": false}}));
    // A door's or a window's family is never work plane-based.
    let e = err(&mut s, "family", json!({"name": "Single Door", "options": {"work_plane_based": true}}));
    assert!(e.contains("work plane-based") || e.contains("work_plane_based"), "{e}");
    // A plane that isn't one.
    run(&mut s, "family", json!({"name": "Sign", "options": {"work_plane_based": true}}));
    run(&mut s, "wall", json!({"points": [[0, 0], [5000, 0]]}));
    let wall = s.model.elements.iter().find(|(_, e)| e.category() == "wall").map(|(id, _)| *id).unwrap();
    assert!(err(&mut s, "component", json!({"family": "Sign", "at": [0, 0], "host": {"plane": wall}})).contains("reference plane"));
    assert!(err(&mut s, "component", json!({"family": "Sign", "at": [0, 0], "host": {"face": 3}})).contains("host"));
}

#[test]
fn deleting_a_host_plane_deletes_what_it_hosts() {
    let mut s = Session::new();
    sign(&mut s, false);
    let r = ramp(&mut s);
    let c = id_of(&run(&mut s, "component", json!({"family": "Sign", "at": [0, 1000], "host": {"plane": r}})));
    let free = id_of(&run(&mut s, "component", json!({"family": "Sign", "at": [5000, 5000]})));
    run(&mut s, "delete", json!({"ids": [r]}));
    assert!(s.model.elements.get(c).is_none());
    assert!(s.model.elements.get(free).is_some());
}

#[test]
fn an_upright_plane_takes_a_point_in_space_and_the_instance_lies_on_it_when_not_always_vertical() {
    let mut s = Session::new();
    sign(&mut s, false);
    let wallish = id_of(&run(&mut s, "reference_plane", json!({"name": "Face", "start": [0, 0, 0], "end": [5000, 0, 0]})));
    let e = err(&mut s, "component", json!({"family": "Sign", "at": [1000, 0], "host": {"plane": wallish}}));
    assert!(e.contains("[x, y, z]"), "{e}");
    let c = id_of(&run(&mut s, "component", json!({"family": "Sign", "at": [1000, 300, 1500], "host": {"plane": wallish}})));
    let cp = component(&s, c);
    // Projected onto the plane (y = 0) along its normal.
    assert!(close(cp.at.1, 0.0) && close(cp.at.0, 1000.0) && close(cp.offset, 1500.0), "{cp:?}");
    // Its normal is the plane's, -y: it stands out of the plane 100 toward -y.
    let ys: Vec<f64> = points(&s, c).iter().map(|p| p[1]).collect();
    let (lo, hi) = (ys.iter().copied().fold(f64::INFINITY, f64::min), ys.iter().copied().fold(f64::NEG_INFINITY, f64::max));
    assert!(close(lo, -100.0) && close(hi, 0.0), "{lo} {hi}");
}

#[test]
fn default_elevation_places_a_level_based_component_at_it() {
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({"name": "Shelf", "types": {"A": {}}, "options": {"default_elevation": 900},
               "forms": [{"profile": [[0, 0], [10, 0], [10, 10]], "to": 10}]}),
    );
    let c = id_of(&run(&mut s, "component", json!({"family": "Shelf", "at": [0, 0]})));
    assert_eq!(component(&s, c).offset, 900.0);
    let c = id_of(&run(&mut s, "component", json!({"family": "Shelf", "at": [0, 0], "offset": 0})));
    assert_eq!(component(&s, c).offset, 0.0);
    assert_eq!(run(&mut s, "family", json!({"name": "Shelf"}))["options"]["default_elevation"], 900.0);
    assert!(err(&mut s, "family", json!({"name": "Shelf", "options": {"default_elevation": 1e300}})).contains("default_elevation"));
    // A work plane-based family is placed on its plane: its Default Elevation isn't read.
    sign(&mut s, true);
    run(&mut s, "family", json!({"name": "Sign", "options": {"default_elevation": 900}}));
    let c = id_of(&run(&mut s, "component", json!({"family": "Sign", "at": [0, 0]})));
    assert_eq!(component(&s, c).offset, 0.0);
    let v = json_of(&mut s, c);
    assert_eq!(v["work_plane"], "Level : Level 1");
}

#[test]
fn edit_work_plane_rehosts_components_between_levels_and_planes() {
    let mut s = Session::new();
    sign(&mut s, false);
    let r = ramp(&mut s);
    let c = id_of(&run(&mut s, "component", json!({"family": "Sign", "at": [2000, 2000]})));
    run(&mut s, "edit_work_plane", json!({"ids": [c], "host": {"plane": r}}));
    let cp = component(&s, c);
    assert!(cp.host.is_some() && close(cp.offset, 2000.0 * SIN30 / COS30) && close(cp.at.1, 2000.0), "{cp:?}");
    run(&mut s, "edit_work_plane", json!({"ids": [c], "host": {"level": "Level 2"}}));
    let cp = component(&s, c);
    let two = s.model.levels()[1].0;
    assert_eq!((cp.host, s.model.elements.id(cp.level), cp.offset), (None, Some(two), 0.0));
    assert!(err(&mut s, "edit_work_plane", json!({"ids": [r], "host": {"level": "Level 1"}})).contains("component"));
}

/// The family `P`: a box on Ref. Level, and a plane `Up` 300 above it.
fn family_with_plane(s: &mut Session) {
    run(
        s,
        "family",
        json!({"name": "P", "types": {"A": {}},
               "ref_planes": [{"name": "Up", "start": [0, 0, 300], "end": [1000, 0, 300], "up": [0, 1, 0]},
                              {"name": "Down", "start": [0, 0, 300], "end": [-1000, 0, 300], "up": [0, 1, 0]},
                              {"name": "Side", "start": [0, 0, 0], "end": [0, 0, 1000], "up": [0, 1, 0]}],
               "ref_lines": [{"start": [0, 0, 0], "end": [500, 0, 0]}],
               "forms": [{"profile": [[0, 0], [100, 0], [100, 50], [0, 50]], "from": 10, "to": 60}]}),
    );
}

fn box_of(s: &Session) -> ([f64; 3], [f64; 3]) {
    let f = &s.model.families["P"];
    let v = f.values("A", &BTreeMap::new()).unwrap();
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in f.pieces(&v).unwrap().iter().flat_map(|p| p.tris.iter().flatten()) {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo, hi)
}

#[test]
fn a_forms_work_plane_changed_moves_it_with_the_plane_or_keeps_it_where_it_is() {
    let mut s = Session::new();
    family_with_plane(&mut s);
    let form = json!(s.model.families["P"].forms[0].guid);
    assert_eq!(box_of(&s), ([0.0, 0.0, 10.0], [100.0, 50.0, 60.0]));
    // Moved with the plane: as Revit's Edit Work Plane, it is the same on Up as it was on Ref. Level.
    run(&mut s, "edit_work_plane", json!({"family": "P", "form": form, "host": {"plane": "Up"}}));
    assert_eq!(box_of(&s), ([0.0, 0.0, 310.0], [100.0, 50.0, 360.0]));
    // Kept where it is: back on Ref. Level, its start and end measured from there again.
    run(&mut s, "edit_work_plane", json!({"family": "P", "form": form, "host": {"plane": "Ref. Level"}, "keep": true}));
    assert_eq!(box_of(&s), ([0.0, 0.0, 310.0], [100.0, 50.0, 360.0]));
    let f = &s.model.families["P"].forms[0];
    assert_eq!((f.from.clone(), f.to.clone()), (crate::family::Expr::Number(310.0), crate::family::Expr::Number(360.0)));
    // Kept on a plane facing the other way (Down's x runs to -x, its normal is -z): its profile and depth are turned.
    run(&mut s, "edit_work_plane", json!({"family": "P", "form": form, "host": {"plane": "Down"}, "keep": true}));
    let (lo, hi) = box_of(&s);
    assert!(
        lo.iter().zip([0.0, 0.0, 310.0]).all(|(a, b)| close(*a, b)) && hi.iter().zip([100.0, 50.0, 360.0]).all(|(a, b)| close(*a, b)),
        "{lo:?} {hi:?}"
    );
    // A plane across it: it can't stay where it is.
    let e = err(&mut s, "edit_work_plane", json!({"family": "P", "form": form, "host": {"plane": "Side"}, "keep": true}));
    assert!(e.contains("parallel"), "{e}");
    // A dry run changes nothing.
    let before = s.model.families["P"].clone();
    run(&mut s, "edit_work_plane", json!({"family": "P", "form": form, "host": {"plane": "Side"}, "dry_run": true}));
    assert_eq!(s.model.families["P"], before);
}

/// Revit's Solid/Void on a selected form: the `family` command's `form` patch switches it, and back, the form keeping its place
/// and identity; a void cuts the solids listed before it.
#[test]
fn a_form_switches_between_solid_and_void_through_the_family_command() {
    let mut s = Session::new();
    family_with_plane(&mut s);
    run(
        &mut s,
        "family",
        json!({"name": "P", "forms": [{"profile": [[0, 0], [100, 0], [100, 50], [0, 50]], "from": 10, "to": 60},
                                                      {"profile": [[20, 0], [40, 0], [40, 50], [20, 50]], "from": 0, "to": 100}]}),
    );
    let (id, guid) = (s.model.families["P"].forms[1].id, s.model.families["P"].forms[1].guid);
    let solid = box_of(&s);
    run(&mut s, "family", json!({"name": "P", "form": {"guid": guid, "void": true}}));
    let f = &s.model.families["P"].forms[1];
    assert!(f.void && f.id == id && f.guid == guid);
    assert_ne!(box_of(&s), solid, "the void cuts the solid instead of adding to it");
    run(&mut s, "family", json!({"name": "P", "form": {"id": id, "void": false}}));
    assert!(!s.model.families["P"].forms[1].void);
    assert_eq!(box_of(&s), solid);
}

#[test]
fn a_reference_lines_work_plane_changed_takes_its_ends_and_what_is_drawn_on_it_along() {
    let mut s = Session::new();
    family_with_plane(&mut s);
    let line = json!(s.model.families["P"].lines[0].guid);
    run(&mut s, "edit_work_plane", json!({"family": "P", "ref_line": line, "host": {"plane": "Up"}}));
    let l = &s.model.families["P"].lines[0];
    assert_eq!((l.start, l.end), ([0.0, 0.0, 300.0], [500.0, 0.0, 300.0]));
    // Kept: it doesn't lie on Ref. Level, so it can't stay where it is on it.
    let e = err(&mut s, "edit_work_plane", json!({"family": "P", "ref_line": line, "host": {"plane": "Ref. Level"}, "keep": true}));
    assert!(e.contains("lie on"), "{e}");
    assert!(err(&mut s, "edit_work_plane", json!({"family": "P", "host": {"plane": "Up"}})).contains("form"));
}

#[test]
fn a_hosted_component_goes_through_a_file_and_ifc_tilted() {
    let mut s = Session::new();
    sign(&mut s, false);
    ramp(&mut s);
    let c = id_of(&run(&mut s, "component", json!({"family": "Sign", "at": [2000, 2000], "host": {"plane": "Ramp"}})));
    let text = s.model.to_json().unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["format"], json!(FORMAT));
    const { assert!(FORMAT >= 19) };
    let (m, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    // As it was, but for the last bit a file's decimals may lose.
    let (Some(Element::Component(read)), was) = (m.elements.get(c), component(&s, c)) else { panic!() };
    assert_eq!((read.host.map(|h| h.host), read.level), (was.host.map(|h| h.host), was.level));
    assert!(close(read.at.0, was.at.0) && close(read.at.1, was.at.1) && close(read.offset, was.offset));
    // IFC: its placement's z axis is the plane's normal.
    let ifc = crate::ifc::export(&s.model);
    assert!(ifc.contains(&format!("IFCDIRECTION((0.,{},{}))", ratio(-SIN30), ratio(COS30))), "no tilted axis");
    // A file of 17 has no hosts and reads as it did.
    let mut old: Value = v.clone();
    old["format"] = json!(17);
    for e in old["elements"].as_object_mut().unwrap().values_mut() {
        if let Some(o) = e.as_object_mut() {
            o.remove("host");
        }
    }
    old["elements"].as_object_mut().unwrap().retain(|_, e| e["category"] != "refplane");
    let (m, left_out) = Model::read(&old.to_string()).unwrap();
    assert!(left_out.is_empty() && m.format == FORMAT, "{left_out:?}");
    let Some(Element::Component(cp)) = m.elements.get(c) else { panic!() };
    assert!(cp.host.is_none() && close(cp.at.1, component(&s, c).at.1) && close(cp.offset, component(&s, c).offset));
}

/// For the hostile sweep (`tests::hostile_params_never_panic_or_break_the_model`): a sloped plane `Ramp`, a work plane-based
/// family `Sign` tilted with it and one on it, made in `base`; and payloads for the work plane commands that name them.
pub(crate) fn sweep_payloads(base: &mut Session) -> Vec<(&'static str, Value)> {
    sign(base, false);
    let plane = ramp(base);
    let c = base.execute("component", &json!({"family": "Sign", "at": [2000, 2000], "host": {"plane": plane}})).map_or(0, |v| id_of(&v));
    let form = json!(base.model.families["Sign"].forms[0].guid);
    let p = |cmd: &'static str, v: Value| (cmd, v);
    vec![
        p("component", json!({"family": "Sign", "at": [1e300, 0], "host": {"plane": plane}, "offset": 1e300})),
        p("component", json!({"family": "Sign", "at": [0, 1e10], "host": {"plane": plane}, "offset": -1e5})),
        p("component", json!({"family": "Sign", "at": [0, 0, 0], "host": {"plane": "Ramp"}, "level": "nope"})),
        p("edit_work_plane", json!({"ids": [c], "host": {"plane": plane}})),
        p("edit_work_plane", json!({"ids": [c, plane], "host": {"level": "Level 1"}})),
        p("edit_work_plane", json!({"family": "Sign", "form": form, "host": {"plane": "Ref. Level"}, "keep": true})),
        p("edit_work_plane", json!({"family": "Sign", "form": form, "host": {"plane": "nope"}})),
        p("edit_work_plane", json!({"family": "Sign", "form": form, "host": {"line": 5, "face": "start"}, "dry_run": "x"})),
        p("move", json!({"ids": [plane], "by": [9.99e8, 0]})),
        p("rotate", json!({"ids": [plane, c], "about": [0, 0], "angle": 1e300})),
        p("mirror", json!({"ids": [plane], "axis": [[0, 0], [1, 1]], "copy": true})),
        p("modify", json!({"id": c, "host": {"uv": [1e300, 0]}})),
        p("modify", json!({"id": c, "host": {"host": {"plane": 100000}}})),
        p("modify", json!({"id": plane, "up": [0, 0, 0]})),
        p("modify", json!({"id": c, "at": [1e300, 0]})),
        p("family", json!({"name": "Sign", "options": {"work_plane_based": false}})),
        p("family", json!({"name": "Sign", "options": {"always_vertical": true}})),
        p("delete", json!({"ids": [plane]})),
        p("copy", json!({"ids": [c], "by": [1000, 0]})),
        p("modify", json!({"id": base.model.levels()[0].0, "elevation": -1e9})),
    ]
}
