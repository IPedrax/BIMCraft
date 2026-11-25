//! The tools' machine, tested as the pure function it is: points in, a step out, and the params the core takes.

use bimcraft_core::draw::{Options, Shape};
use bimcraft_core::family::Category;
use bimcraft_core::geom::P2;
use bimcraft_core::model::Model;
use serde_json::json;

use super::*;
use crate::app::App;

struct World {
    model: Model,
    level: Id,
}

fn world() -> World {
    let app = App::sample();
    World { level: app.level.expect("a level"), model: (*app.session.model).clone() }
}

impl World {
    fn ctx(&self) -> Ctx<'_> {
        Ctx { model: &self.model, level: self.level, hovered: None, shape: Shape::Line, options: Options::default() }
    }
}

fn pts(list: &[(f64, f64)]) -> Vec<P2> {
    list.iter().map(|&(x, y)| P2(x, y)).collect()
}

fn of(id: &str) -> &'static Spec {
    spec_of(id).expect("a spec")
}

fn run_of(step: Step) -> (Vec<Command>, Chain) {
    match step {
        Step::Run { commands, keep } => (commands, keep),
        other => panic!("not a run: {other:?}"),
    }
}

const A: (f64, f64) = (0.0, 0.0);
const B: (f64, f64) = (4000.0, 0.0);
const C: (f64, f64) = (4000.0, 3000.0);

#[test]
fn every_palette_tool_that_draws_has_a_spec_and_every_spec_names_a_command_the_core_has() {
    for &(section, tools) in crate::tools::TOOLS {
        for &(id, _) in tools {
            assert!(spec_of(id).is_some(), "{section}: {id} has no spec");
        }
    }
    let mut ids: Vec<&str> = SPECS.iter().map(|s| s.id).collect();
    let n = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), n, "ids are unique");
    for s in SPECS {
        assert!(crate::tools::label(s.id).is_some(), "{} is a tool of the palette", s.id);
        assert!(bimcraft_core::engine::find(s.command).is_some(), "{} makes `{}`, which the core has", s.id, s.command);
    }
    assert!(spec_of("move").is_none(), "a Modify tool is not a drawing one");
}

#[test]
fn a_wall_chain_makes_a_wall_for_each_click_after_the_first_and_closes_on_its_first_point() {
    let w = world();
    let (c, wall) = (w.ctx(), of("wall"));
    assert_eq!(advance(wall, &c, &pts(&[A])), Step::More, "the first click only starts it");
    let (cmds, keep) = run_of(advance(wall, &c, &pts(&[A, B])));
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].0, "wall");
    assert_eq!(cmds[0].1["points"], json!([[0.0, 0.0], [4000.0, 0.0]]));
    assert_eq!(cmds[0].1["level"], json!(w.level));
    assert_eq!(keep, Chain::Whole, "the chain goes on from the point clicked");
    let (cmds, keep) = run_of(advance(wall, &c, &pts(&[A, B, C])));
    assert_eq!(cmds[0].1["points"], json!([[4000.0, 0.0], [4000.0, 3000.0]]), "the wall is from the click before");
    assert_eq!(keep, Chain::Whole);
    let (cmds, keep) = run_of(advance(wall, &c, &pts(&[A, B, C, A])));
    assert_eq!(cmds[0].1["points"], json!([[4000.0, 3000.0], [0.0, 0.0]]));
    assert_eq!(keep, Chain::No, "back on its first point, the chain closes");
    assert_eq!(advance(wall, &c, &pts(&[A, B, B])), Step::Done, "the last point clicked again ends the chain and makes nothing");
    assert_eq!(advance(wall, &c, &pts(&[A, A])), Step::Done);
}

#[test]
fn a_run_tool_chains_from_the_last_point_only_and_a_curtain_wall_needs_a_curtain_type() {
    let w = world();
    let c = w.ctx();
    let (cmds, keep) = run_of(advance(of("beam"), &c, &pts(&[A, B])));
    assert_eq!((cmds[0].0, keep), ("beam", Chain::Last));
    let (cmds, _) = run_of(advance(of("strip_footing"), &c, &pts(&[A, B, C])));
    assert_eq!(cmds[0].1["points"], json!([[4000.0, 0.0], [4000.0, 3000.0]]));
    match (curtain_type(&w.model), advance(of("curtain_wall"), &c, &pts(&[A, B]))) {
        (Some(name), step) => assert_eq!(run_of(step).0[0].1["type"], json!(name)),
        (None, Step::Refuse(why)) => assert!(why.contains("curtain"), "{why}"),
        (_, other) => panic!("{other:?}"),
    }
}

#[test]
fn a_point_tool_makes_its_element_on_the_first_click_at_the_key_it_names() {
    let w = world();
    let c = w.ctx();
    for (id, key) in [
        ("door", "at"),
        ("window", "at"),
        ("column", "at"),
        ("structural_column", "at"),
        ("footing", "at"),
        ("room", "point"),
        ("ceiling", "point"),
    ] {
        let (cmds, keep) = run_of(advance(of(id), &c, &pts(&[(6200.0, 0.0)])));
        assert_eq!(cmds.len(), 1, "{id}");
        assert_eq!(cmds[0].1[key], json!([6200.0, 0.0]), "{id}");
        assert_eq!(cmds[0].1["level"], json!(w.level), "{id}");
        assert_eq!(keep, Chain::No, "{id}");
    }
}

#[test]
fn a_roof_and_a_floor_are_made_by_a_click_anywhere_and_the_roof_goes_on_the_level_above() {
    let w = world();
    let c = w.ctx();
    let (cmds, _) = run_of(advance(of("floor"), &c, &pts(&[A])));
    assert_eq!((cmds[0].0, &cmds[0].1["level"]), ("floor", &json!(w.level)));
    assert!(cmds[0].1.get("at").is_none(), "no point is sent");
    let (cmds, _) = run_of(advance(of("roof"), &c, &pts(&[A])));
    let above = w.model.level_above(w.level).unwrap_or_else(|| w.model.guid(w.level));
    assert_eq!(cmds[0].1["level"], json!(above));
}

#[test]
fn a_stair_or_ramp_takes_two_clicks_and_the_same_place_twice_is_no_end() {
    let w = world();
    let c = w.ctx();
    assert_eq!(advance(of("stair"), &c, &pts(&[A])), Step::More);
    assert_eq!(advance(of("stair"), &c, &pts(&[A, A])), Step::Ignore);
    let (cmds, keep) = run_of(advance(of("ramp"), &c, &pts(&[A, B])));
    assert_eq!((cmds[0].0, keep), ("ramp", Chain::No));
    assert_eq!((&cmds[0].1["start"], &cmds[0].1["end"]), (&json!([0.0, 0.0]), &json!([4000.0, 0.0])));
}

#[test]
fn a_railing_is_a_path_finished_by_enter_or_its_last_point_clicked_again() {
    let w = world();
    let (c, rail) = (w.ctx(), of("railing"));
    assert_eq!(advance(rail, &c, &pts(&[A])), Step::More);
    assert_eq!(advance(rail, &c, &pts(&[A, B])), Step::More, "a path goes on until it is finished");
    let (cmds, keep) = run_of(advance(rail, &c, &pts(&[A, B, B])));
    assert_eq!(cmds[0].1["path"], json!([[0.0, 0.0], [4000.0, 0.0]]), "the point clicked twice is one");
    assert_eq!(keep, Chain::No);
    let (cmds, _) = run_of(finish(rail, &c, &pts(&[A, B, C])));
    assert_eq!(cmds[0].1["path"], json!([[0.0, 0.0], [4000.0, 0.0], [4000.0, 3000.0]]));
    assert!(matches!(finish(rail, &c, &pts(&[A])), Step::Refuse(_)), "one point is no path");
    assert_eq!(finish(rail, &c, &[]), Step::More, "with nothing picked Enter is nobody's here");
    assert_eq!(finish(of("wall"), &c, &pts(&[A, B])), Step::Done, "Enter ends a chain");
}

#[test]
fn a_click_on_a_stair_makes_a_railing_for_it_and_one_on_a_wall_does_not() {
    let mut app = App::sample();
    let level = app.level.expect("a level");
    let made = app
        .session
        .execute("stair", &json!({"start": [-30000, -30000], "end": [-30000, -29000], "level": level}))
        .expect("a stair far from the house");
    let stair = made["id"].as_u64().expect("an id");
    let wall = app.session.model.elements.iter().find_map(|(id, e)| matches!(e, Element::Wall(_)).then_some(*id)).expect("a wall");
    let model = (*app.session.model).clone();
    let ctx = |hovered| Ctx { model: &model, level, hovered, shape: Shape::Line, options: Options::default() };
    let (cmds, _) = run_of(advance(of("railing"), &ctx(Some(stair)), &pts(&[A])));
    assert_eq!((cmds[0].0, &cmds[0].1["host"]), ("railing", &json!(stair)));
    assert_eq!(advance(of("railing"), &ctx(Some(wall)), &pts(&[A])), Step::More, "a wall is no host: the click starts a path");
}

#[test]
fn a_fixture_tool_places_the_first_family_of_its_kind_and_says_so_when_there_is_none() {
    let mut w = world();
    let c = w.ctx();
    let (cmds, _) = run_of(advance(of("component"), &c, &pts(&[(3000.0, 3000.0)])));
    assert_eq!(cmds[0].0, "component");
    assert!(cmds[0].1["family"].is_string() && cmds[0].1["type"].is_string(), "{:?}", cmds[0].1);
    assert_eq!(cmds[0].1["at"], json!([3000.0, 3000.0]));
    w.model.families = std::collections::BTreeMap::<String, bimcraft_core::family::Family>::new().into();
    let c = w.ctx();
    for id in ["component", "plumbing_fixture", "drain", "outlet", "switch", "lighting"] {
        match advance(of(id), &c, &pts(&[A])) {
            Step::Refuse(why) => assert!(why.contains("family"), "{id}: {why}"),
            other => panic!("{id}: {other:?}"),
        }
    }
    assert!(family_of(&w.model, &[Category::Plumbing]).is_none());
}

#[test]
fn a_shape_waits_for_its_points_and_makes_the_walls_along_what_they_draw() {
    let w = world();
    let wall = of("wall");
    let with = |shape, options| Ctx { shape, options, ..w.ctx() };
    let rect = with(Shape::Rectangle, Options::default());
    assert_eq!(advance(wall, &rect, &pts(&[A])), Step::More);
    assert_eq!(advance(wall, &rect, &pts(&[A, A])), Step::Ignore, "a rectangle with no size");
    let (cmds, keep) = run_of(advance(wall, &rect, &pts(&[A, C])));
    assert_eq!((cmds.len(), keep), (1, Chain::No), "four walls end to end are one command");
    assert_eq!(cmds[0].1["closed"], json!(true));
    assert_eq!(cmds[0].1["points"].as_array().map(Vec::len), Some(4));
    let circle = with(Shape::Circle, Options::default());
    let (cmds, _) = run_of(advance(wall, &circle, &pts(&[A, B])));
    assert_eq!(cmds.len(), 1, "a circle is two half circles joined end to end: one closed command");
    assert_eq!((&cmds[0].1["arc"], &cmds[0].1["closed"]), (&json!(180.0), &json!(true)));
    assert_eq!(cmds[0].1["points"].as_array().map(Vec::len), Some(2));
    let sized = with(Shape::Circle, Options { radius: Some(1500.0), ..Options::default() });
    assert_eq!(run_of(advance(wall, &sized, &pts(&[A]))).0.len(), 1, "with a radius set, one click is enough");
    assert!(matches!(advance(wall, &with(Shape::PickLines, Options::default()), &pts(&[A])), Step::Refuse(_)));
}

#[test]
fn a_point_that_is_no_number_is_left_alone_and_a_path_is_capped() {
    let w = world();
    let c = w.ctx();
    for id in ["wall", "door", "stair", "railing", "component", "roof"] {
        assert_eq!(advance(of(id), &c, &[P2(f64::NAN, 0.0)]), Step::Ignore, "{id}");
        assert_eq!(advance(of(id), &c, &[P2(0.0, 0.0), P2(f64::INFINITY, 0.0)]), Step::Ignore, "{id}");
    }
    let long: Vec<P2> = (0..MAX_POINTS).map(|i| P2(i as f64 * 10.0, 0.0)).collect();
    assert!(matches!(advance(of("railing"), &c, &long), Step::Run { .. }), "a path that reaches the cap is made, not dropped");
    assert_eq!(run_of(advance(of("wall"), &c, &long)).1, Chain::No, "and so is a chain of walls ended");
}
