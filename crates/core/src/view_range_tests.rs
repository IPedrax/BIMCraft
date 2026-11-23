//! A plan's view range, as Revit's: each plane measured from a level (its own, the one above or below, one named, or
//! none), the planes kept in order, a template measured from each plan's level, and what a plan draws in each zone: over
//! the cut only doors, windows, casework and generic models; cut; seen in projection down to the bottom; and in halftone,
//! as the project's Halftone/Underlay says, down to the view depth. Driven through the commands every front end drives.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::graphics::Fade;
use crate::model::Id;
use crate::render::{self, Drawing, Prim, Rgb};

const L1: Id = 100000;
const L2: Id = 100001;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
}

fn fails(s: &mut Session, command: &str, params: Value) -> String {
    match s.execute(command, &params) {
        Ok(v) => panic!("{command} should fail: {v}"),
        Err(e) => e,
    }
}

/// The element a command made: its `id`, or the first of its `ids`.
fn id(v: &Value) -> Id {
    v["id"].as_u64().or_else(|| v["ids"][0].as_u64()).unwrap_or_else(|| panic!("no id in {v}"))
}

fn plan(s: &Session, level: Id) -> Drawing {
    render::plan(&s.model, level, 800, 600)
}

/// The colours of the lines element `id` draws.
fn lines_of(d: &Drawing, id: Id) -> Vec<Rgb> {
    d.prims
        .iter()
        .zip(&d.owners)
        .filter(|(_, o)| **o == Some(id))
        .flat_map(|(p, _)| p.paint().to_vec())
        .filter_map(|p| match p {
            Prim::Line { color, .. } => Some(color),
            _ => None,
        })
        .collect()
}

#[test]
fn planes_are_measured_from_the_level_each_names() {
    let mut s = Session::new();
    let basement = id(&run(&mut s, "level", json!({"elevation": -3000, "name": "Basement"})));
    // From a level named: 200 over the basement.
    run(&mut s, "modify", json!({"id": L2, "view_range": {"depth": 200, "depth_from": {"level": basement}}}));
    let r = s.model.plan_range(L2);
    assert_eq!((r.cut, r.bottom, r.depth), (4200.0, 3000.0, -2800.0));
    // The level below Level 2 is Level 1.
    run(&mut s, "modify", json!({"id": L2, "view_range": {"bottom": 0, "bottom_from": "below"}}));
    assert_eq!(s.model.plan_range(L2).bottom, 0.0);
    // Unlimited: down as far as the model goes, and up.
    run(&mut s, "modify", json!({"id": L2, "view_range": {"depth_from": "unlimited", "top_from": "unlimited", "top": 0}}));
    let r = s.model.plan_range(L2);
    assert!(r.depth == f64::NEG_INFINITY && r.top == f64::INFINITY, "{r:?}");
    // Its own level, and an offset over it.
    run(&mut s, "modify", json!({"id": L2, "view_range": {"top_from": "associated", "top": 2000}}));
    assert_eq!(s.model.plan_range(L2).top, 5000.0);
    // A file holds the level by its GUID, and reads it back.
    let back = crate::model::Model::read(&s.model.to_json().unwrap()).unwrap().0;
    assert_eq!(back.plan_range(L2), s.model.plan_range(L2));
}

#[test]
fn the_planes_keep_their_order_wherever_they_are_measured_from() {
    let mut s = Session::new();
    let basement = id(&run(&mut s, "level", json!({"elevation": -3000})));
    // The bottom can't be lower than the view depth.
    let e = fails(&mut s, "modify", json!({"id": L2, "view_range": {"bottom": 0, "bottom_from": {"level": basement}}}));
    assert!(e.contains("view depth (3000 mm, its own level +0) is above its bottom (-3000 mm, `Level 3` +0)"), "{e}");
    assert!(e.ends_with("the bottom can't be lower than the view depth"), "{e}");
    // The cut plane can't be higher than the top.
    let e = fails(&mut s, "modify", json!({"id": L1, "view_range": {"top": 0, "top_from": {"level": basement}}}));
    assert!(e.contains("cut plane (1200 mm, its own level +1200) is above its top (-3000 mm, `Level 3` +0)"), "{e}");
    assert!(e.ends_with("the cut plane can't be higher than the top"), "{e}");
    let e = fails(&mut s, "modify", json!({"id": L1, "view_range": {"top": 1000}}));
    assert!(e.ends_with("the cut plane can't be higher than the top"), "{e}");
    // The bottom can't be higher than the cut plane.
    let e = fails(&mut s, "modify", json!({"id": L1, "view_range": {"bottom": 1500}}));
    assert!(e.contains("bottom (+1500 over the plan's level) is above its cut plane (+1200 over the plan's level)"), "{e}");
    assert!(e.ends_with("the bottom can't be higher than the cut plane"), "{e}");
    // An unlimited bottom takes an unlimited view depth.
    let e = fails(&mut s, "modify", json!({"id": L1, "view_range": {"bottom_from": "unlimited"}}));
    assert!(e.ends_with("the bottom can't be lower than the view depth"), "{e}");
    run(&mut s, "modify", json!({"id": L1, "view_range": {"bottom_from": "unlimited", "depth_from": "unlimited"}}));
    // What isn't a level can't be measured from.
    let e = fails(&mut s, "modify", json!({"id": L1, "view_range": {"top": 0, "top_from": {"level": 999999}}}));
    assert!(!e.is_empty());
}

#[test]
fn a_level_a_plan_measures_from_goes_and_the_plan_keeps_its_heights() {
    let mut s = Session::new();
    let basement = id(&run(&mut s, "level", json!({"elevation": -3000})));
    run(&mut s, "modify", json!({"id": L1, "view_range": {"depth": 500, "depth_from": {"level": basement}}}));
    assert_eq!(s.model.plan_range(L1).depth, -2500.0);
    run(&mut s, "delete", json!({"ids": [basement]}));
    let r = s.model.view_range_of(L1);
    assert_eq!((r.depth_from, r.depth), (crate::view_range::Base::Associated, -2500.0));
    assert_eq!(s.model.plan_range(L1).depth, -2500.0);
}

#[test]
fn a_template_is_measured_from_each_plans_own_level() {
    let mut s = Session::new();
    run(&mut s, "template", json!({"name": "Deep", "view_range": {"cut": 1000, "bottom": 0, "depth": -1000}, "controls": ["range"]}));
    run(&mut s, "graphics", json!({"view": L1, "template": "Deep"}));
    run(&mut s, "graphics", json!({"view": L2, "template": "Deep"}));
    let (a, b) = (s.model.plan_range(L1), s.model.plan_range(L2));
    assert_eq!((a.cut, a.depth, b.cut, b.depth), (1000.0, -1000.0, 4000.0, 2000.0));
    // The level below each, too.
    run(&mut s, "template", json!({"name": "Deep", "view_range": {"depth": 0, "depth_from": "below"}}));
    assert_eq!(s.model.plan_range(L2).depth, 0.0);
    assert_eq!(s.model.plan_range(L1).depth, f64::NEG_INFINITY, "none below: no limit");
    // Never a level named: it serves every storey.
    let e = fails(&mut s, "template", json!({"name": "Deep", "view_range": {"depth_from": {"level": L1}}}));
    assert!(e.contains("names a level"), "{e}");
    // Made from a plan whose range names one, it is measured from that plan's level, at the same heights.
    run(&mut s, "graphics", json!({"view": L2, "template": null}));
    run(&mut s, "modify", json!({"id": L2, "view_range": {"depth": 0, "depth_from": {"level": L1}}}));
    let t = run(&mut s, "template", json!({"name": "From 2", "from": L2}));
    assert_eq!((t["view_range"]["depth"].clone(), t["view_range"].get("depth_from")), (json!(-3000.0), None));
}

#[test]
fn what_lies_between_the_bottom_and_the_view_depth_is_halftone() {
    let mut s = Session::new();
    // A low wall on Level 1 (top at 1000) and a floor on Level 2 (its top on Level 2's bottom).
    let wall = id(&run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "height": 1000})));
    let floor = id(&run(&mut s, "floor", json!({"level": L2, "boundary": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]]})));
    // Level 2's plan sees nothing under its bottom until its view depth reaches down.
    assert!(lines_of(&plan(&s, L2), wall).is_empty());
    run(&mut s, "modify", json!({"id": L2, "view_range": {"depth": 0, "depth_from": "below"}}));
    let d = plan(&s, L2);
    let ink = render::INK;
    let walls = lines_of(&d, wall);
    assert!(!walls.is_empty() && walls.iter().all(|c| *c == Fade(50).apply(ink)), "the wall in halftone: {walls:?}");
    // The floor whose top touches the bottom is seen in projection, as floors are: not lightened.
    let floors = lines_of(&d, floor);
    assert!(!floors.is_empty() && floors.iter().all(|c| *c == render::FAINT), "{floors:?}");
    // The project's halftone says how far toward the paper.
    assert_eq!(run(&mut s, "halftone", json!({"fade": 80}))["fade"], 80);
    let walls = lines_of(&plan(&s, L2), wall);
    assert!(walls.iter().all(|c| *c == Fade(80).apply(ink)), "{walls:?}");
    assert_eq!(run(&mut s, "halftone", json!({}))["fade"], 80);
    for bad in [json!(0), json!(101), json!("half"), json!(50.5)] {
        fails(&mut s, "halftone", json!({"fade": bad}));
    }
    // And Visibility/Graphics' Halftone with it.
    run(&mut s, "graphics", json!({"view": L1, "categories": {"walls": {"halftone": true}}}));
    let walls = lines_of(&plan(&s, L1), wall);
    assert!(!walls.is_empty() && walls.iter().all(|c| *c == Fade(80).apply(ink)), "{walls:?}");
}

#[test]
fn over_the_cut_a_plan_shows_doors_windows_casework_and_generic_models_alone() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]]}));
    let form = json!([{"profile": [[-300, -300], [300, -300], [300, 300], [-300, 300]], "to": 600}]);
    run(&mut s, "family", json!({"name": "Cabinet", "category": "casework", "types": {"A": {}}, "forms": form}));
    run(&mut s, "family", json!({"name": "Shelf", "category": "furniture", "types": {"A": {}}, "forms": form}));
    let cabinet = id(&run(&mut s, "component", json!({"family": "Cabinet", "at": [1000, 2000], "offset": 1500})));
    let shelf = id(&run(&mut s, "component", json!({"family": "Shelf", "at": [3000, 2000], "offset": 1500})));
    // A high window, all of it over the cut plane, in a wall the plane cuts.
    let window = id(&run(&mut s, "window", json!({"at": [4500, 0], "sill": 1800})));
    let d = plan(&s, L1);
    assert!(!lines_of(&d, cabinet).is_empty(), "casework over the cut");
    assert!(lines_of(&d, shelf).is_empty(), "furniture over the cut isn't seen");
    assert!(!lines_of(&d, window).is_empty(), "a window over the cut");
    // Over the top, nothing.
    run(&mut s, "modify", json!({"id": L1, "view_range": {"top": 1400}}));
    let d = plan(&s, L1);
    assert!(lines_of(&d, cabinet).is_empty() && lines_of(&d, window).is_empty());
}

#[test]
fn a_roof_below_is_seen_from_the_plan_over_it() {
    let mut s = Session::new();
    let roof = id(&run(&mut s, "roof", json!({"level": L1, "boundary": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "slope": 10})));
    // Level 2 doesn't reach down to it: not seen.
    assert!(lines_of(&plan(&s, L2), roof).is_empty());
    // Its bottom on Level 1: in projection.
    run(&mut s, "modify", json!({"id": L2, "view_range": {"bottom": 0, "bottom_from": "below", "depth": 0, "depth_from": "below"}}));
    let lines = lines_of(&plan(&s, L2), roof);
    assert!(!lines.is_empty() && lines.iter().all(|c| *c != Fade(50).apply(render::INK)), "{lines:?}");
    // Its bottom over the roof: in halftone.
    run(&mut s, "modify", json!({"id": L2, "view_range": {"bottom": 0, "bottom_from": "associated"}}));
    let lines = lines_of(&plan(&s, L2), roof);
    assert!(!lines.is_empty(), "the roof in the underlay");
    assert!(lines.iter().all(|c| c.0 > render::INK.0), "lightened: {lines:?}");
}
