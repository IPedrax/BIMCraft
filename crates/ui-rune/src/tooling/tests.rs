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
        Ctx {
            model: &self.model,
            level: self.level,
            hovered: None,
            shape: Shape::Line,
            options: Options::default(),
            selection: &[],
            held: Held::Nothing,
            reach: 100.0,
            tangent: None,
        }
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
    for &(id, _) in crate::tools::MODIFY {
        // Array has no command in the core and Scale needs a typed factor: neither is a tool of this machine yet.
        assert_eq!(spec_of(id).is_some(), !matches!(id, "array" | "scale"), "{id}");
    }
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
    let ctx = |hovered| Ctx {
        model: &model,
        level,
        hovered,
        shape: Shape::Line,
        options: Options::default(),
        selection: &[],
        held: Held::Nothing,
        reach: 100.0,
        tangent: None,
    };
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

const SEL: [Id; 2] = [100100, 100101];

fn approx(v: &serde_json::Value, want: &[f64]) -> bool {
    v.as_array().is_some_and(|a| a.len() == want.len() && a.iter().zip(want).all(|(x, w)| x.as_f64().is_some_and(|x| (x - w).abs() < 1e-6)))
}

#[test]
fn the_angle_between_two_rays_is_counter_clockwise_in_minus_180_to_180() {
    let o = P2(10.0, 10.0);
    let angle = |a: (f64, f64), b: (f64, f64)| angle_between(o, o + P2(a.0, a.1), o + P2(b.0, b.1));
    assert!((angle((1.0, 0.0), (0.0, 1.0)) - 90.0).abs() < 1e-9);
    assert!((angle((1.0, 0.0), (0.0, -1.0)) + 90.0).abs() < 1e-9);
    assert!((angle((1.0, 0.0), (-1.0, 0.0)) - 180.0).abs() < 1e-9, "a half turn is +180, not -180");
    assert!((angle((0.0, 1.0), (1.0, 0.0)) + 90.0).abs() < 1e-9);
    assert!((angle((-1.0, 0.1), (-1.0, -0.1)) - 11.42).abs() < 0.01, "across the cut at 180 it goes the short way, counter-clockwise");
}

#[test]
fn move_and_copy_need_a_selection_then_a_base_point_and_a_destination() {
    let w = world();
    assert!(matches!(advance(of("move"), &w.ctx(), &pts(&[A])), Step::Refuse(_)), "with nothing selected the tool says so");
    let c = Ctx { selection: &SEL, ..w.ctx() };
    assert_eq!(advance(of("move"), &c, &pts(&[A])), Step::More);
    assert_eq!(advance(of("move"), &c, &pts(&[A, A])), Step::Ignore, "the same place is no move");
    let (cmds, keep) = run_of(advance(of("move"), &c, &pts(&[A, C])));
    assert_eq!((cmds[0].0, keep), ("move", Chain::No));
    assert_eq!(cmds[0].1["ids"], json!([100100, 100101]));
    assert!(approx(&cmds[0].1["by"], &[4000.0, 3000.0]), "{:?}", cmds[0].1);
    assert_eq!(run_of(advance(of("copy"), &c, &pts(&[A, B]))).0[0].0, "copy");
    assert!(of("move").ends_tool() && of("copy").ends_tool() && !of("split").ends_tool(), "Split stays in hand, Move does not");
}

#[test]
fn rotate_takes_a_centre_and_two_rays_and_mirror_two_points_for_its_axis() {
    let w = world();
    let c = Ctx { selection: &SEL, ..w.ctx() };
    let rotate = of("rotate");
    assert_eq!(advance(rotate, &c, &pts(&[A])), Step::More);
    assert_eq!(advance(rotate, &c, &pts(&[A, (1000.0, 0.0)])), Step::More);
    assert_eq!(advance(rotate, &c, &pts(&[A, A])), Step::Ignore, "a ray with no length");
    assert_eq!(advance(rotate, &c, &pts(&[A, (1000.0, 0.0), A])), Step::Ignore);
    let (cmds, _) = run_of(advance(rotate, &c, &pts(&[A, (1000.0, 0.0), (0.0, 1000.0)])));
    assert_eq!((cmds[0].0, &cmds[0].1["copy"]), ("rotate", &json!(false)));
    assert!(approx(&cmds[0].1["about"], &[0.0, 0.0]) && cmds[0].1["angle"].as_f64().is_some_and(|a| (a - 90.0).abs() < 1e-9));
    let mirror = of("mirror");
    assert_eq!(advance(mirror, &c, &pts(&[A])), Step::More);
    assert_eq!(advance(mirror, &c, &pts(&[A, A])), Step::Ignore);
    let (cmds, _) = run_of(advance(mirror, &c, &pts(&[A, B])));
    assert_eq!((cmds[0].0, &cmds[0].1["copy"]), ("mirror", &json!(true)));
    assert_eq!(cmds[0].1["axis"], json!([[0.0, 0.0], [4000.0, 0.0]]));
    assert!(matches!(advance(mirror, &w.ctx(), &pts(&[A])), Step::Refuse(_)), "nothing selected");
}

#[test]
fn split_and_trim_work_on_the_elements_picked_under_the_pointer() {
    let w = world();
    assert!(matches!(advance(of("split"), &w.ctx(), &pts(&[A])), Step::Refuse(_)), "nothing there to split");
    let on = |hovered, held| Ctx { hovered, held, ..w.ctx() };
    let (cmds, keep) = run_of(advance(of("split"), &on(Some(7), Held::Nothing), &pts(&[B])));
    assert_eq!((cmds[0].0, keep), ("split", Chain::No));
    assert_eq!((&cmds[0].1["id"], &cmds[0].1["at"]), (&json!(7), &json!([4000.0, 0.0])));
    let trim = of("trim");
    assert_eq!(advance(trim, &on(Some(7), Held::Nothing), &pts(&[A])), Step::Hold(Held::Element(7)), "the first pick is held");
    assert!(matches!(advance(trim, &on(None, Held::Nothing), &pts(&[A])), Step::Refuse(_)));
    let second = pts(&[A, C]);
    assert_eq!(advance(trim, &on(Some(7), Held::Element(7)), &second), Step::Ignore, "the same element twice is no corner");
    let (cmds, _) = run_of(advance(trim, &on(Some(8), Held::Element(7)), &second));
    assert_eq!(cmds[0].0, "corner", "Trim is the core's corner");
    assert_eq!((&cmds[0].1["ids"], &cmds[0].1["keep"]), (&json!([7, 8]), &json!([[0.0, 0.0], [4000.0, 3000.0]])));
}

#[test]
fn align_holds_the_first_line_and_offset_copies_a_wall_across_to_the_point() {
    let w = world();
    let (id, start, end) = w
        .model
        .elements
        .iter()
        .find_map(|(id, e)| match e {
            Element::Wall(wall) if wall.arc == 0.0 && wall.start.dist(wall.end) > 2000.0 => Some((*id, wall.start, wall.end)),
            _ => None,
        })
        .expect("a straight wall in the house");
    let on = |hovered, held, reach| Ctx { hovered, held, reach, ..w.ctx() };
    let align = of("align");
    let Step::Hold(Held::Line(reference)) = advance(align, &on(Some(id), Held::Nothing, 100.0), &[start]) else { panic!("a line is held") };
    assert!(reference.0.dist(reference.1) > 1000.0, "the wall's own edge or axis: {reference:?}");
    let across = (end - start).unit().perp() * 5000.0;
    assert!(matches!(advance(align, &on(Some(id), Held::Nothing, 0.0), &[start + across]), Step::Refuse(_)), "too far from any line");
    assert!(matches!(advance(align, &on(None, Held::Nothing, 100.0), &[start]), Step::Refuse(_)));
    let (cmds, _) = run_of(advance(align, &on(Some(id), Held::Line(reference), 100.0), &[start, end]));
    assert_eq!((cmds[0].0, &cmds[0].1["id"]), ("align", &json!(id)));
    assert!(approx(&cmds[0].1["reference"][0], &[reference.0.0, reference.0.1]), "{:?}", cmds[0].1);

    let offset = of("offset");
    assert_eq!(advance(offset, &on(Some(id), Held::Nothing, 100.0), &[start]), Step::Hold(Held::Element(id)));
    assert!(matches!(advance(offset, &on(None, Held::Nothing, 100.0), &[start]), Step::Refuse(_)));
    let n = (end - start).unit().perp();
    let (cmds, _) = run_of(advance(offset, &on(None, Held::Element(id), 100.0), &[start, start + n * 500.0]));
    assert_eq!((cmds[0].0, &cmds[0].1["ids"]), ("copy", &json!([id])));
    assert!(approx(&cmds[0].1["by"], &[n.0 * 500.0, n.1 * 500.0]), "{:?}", cmds[0].1);
    assert_eq!(advance(offset, &on(None, Held::Element(id), 100.0), &[start, start + n * 0.5]), Step::Ignore, "on the wall: no distance");
}

#[test]
fn pick_lines_makes_the_walls_along_the_element_clicked_beside_it_by_the_offset_and_pick_faces_wants_a_mass() {
    let w = world();
    let wall =
        w.model.elements.iter().find(|(_, e)| matches!(e, bimcraft_core::model::Element::Wall(_))).map(|(id, _)| *id).expect("a wall");
    let ctx = Ctx { hovered: Some(wall), shape: Shape::PickLines, ..w.ctx() };
    let (cmds, keep) = run_of(advance(of("wall"), &ctx, &pts(&[(-300.0, 3500.0)])));
    assert_eq!((cmds.len(), cmds[0].0, keep), (1, "wall", Chain::No), "one wall along its line, and the chain is over");
    let at = |cmds: &[Command], i: usize, k: usize| cmds[0].1["points"][i][k].as_f64().unwrap_or(f64::NAN);
    let line = w.model.wall(wall).map(|x| (x.start, x.end)).expect("a wall");
    assert!((at(&cmds, 0, 0) - line.0.0).abs() < 1e-6 && (at(&cmds, 1, 1) - line.1.1).abs() < 1e-6, "the wall's own line: {:?}", cmds[0].1);
    let beside = Ctx { options: Options { offset: 500.0, ..Options::default() }, ..ctx };
    let (cmds, _) = run_of(advance(of("wall"), &beside, &pts(&[(-300.0, 3500.0)])));
    assert!(((at(&cmds, 0, 0) - line.0.0).abs() - 500.0).abs() < 1.0, "beside it by the offset: {:?}", cmds[0].1);
    let nothing = Ctx { hovered: None, shape: Shape::PickLines, ..w.ctx() };
    assert!(matches!(advance(of("wall"), &nothing, &pts(&[A])), Step::Refuse(_)), "nothing to pick");
    let faces = Ctx { shape: Shape::PickFaces, ..w.ctx() };
    assert!(matches!(advance(of("wall"), &faces, &pts(&[A])), Step::Refuse(_)), "the house has no mass");
}

/// Two straight walls meeting at a corner far from the house: the first runs east to it, the second north from it.
fn corner() -> (World, Id, Id) {
    let mut app = App::sample();
    let level = app.level.expect("a level");
    let mut wall = |a: [f64; 2], b: [f64; 2]| {
        assert!(app.run("wall", &json!({"points": [a, b], "level": level})), "{:?}", app.message);
        let walls = app.session.model.elements.iter().filter(|(_, e)| matches!(e, bimcraft_core::model::Element::Wall(_)));
        walls.map(|(id, _)| *id).max().expect("a wall")
    };
    let a = wall([-30000.0, -30000.0], [-26000.0, -30000.0]);
    let b = wall([-26000.0, -30000.0], [-26000.0, -27000.0]);
    (World { level, model: (*app.session.model).clone() }, a, b)
}

const INSIDE: (f64, f64) = (-26700.0, -29300.0);

#[test]
fn fillet_arc_holds_two_straight_walls_and_a_third_click_or_the_radius_option_sizes_the_arc() {
    let (w, a, b) = corner();
    let wall = of("wall");
    let on = |hovered, held| Ctx { hovered, held, shape: Shape::Fillet, ..w.ctx() };
    assert_eq!(advance(wall, &on(Some(a), Held::Nothing), &pts(&[A])), Step::Hold(Held::Element(a)));
    assert!(matches!(advance(wall, &on(None, Held::Nothing), &pts(&[A])), Step::Refuse(_)), "no wall under the pointer");
    assert_eq!(advance(wall, &on(Some(b), Held::Element(a)), &pts(&[A, B])), Step::Hold(Held::Pair(a, b)));
    assert_eq!(
        advance(wall, &on(Some(b), Held::Pair(a, b)), &pts(&[A, B])),
        Step::Hold(Held::Pair(a, b)),
        "a second pick again, after Backspace"
    );
    assert_eq!(advance(wall, &on(Some(a), Held::Element(a)), &pts(&[A, B])), Step::Ignore, "the same wall twice is no corner");
    assert!(matches!(advance(wall, &on(None, Held::Element(a)), &pts(&[A, B])), Step::Refuse(_)));
    let sized = Ctx { options: Options { radius: Some(500.0), ..Options::default() }, ..on(Some(b), Held::Element(a)) };
    let (cmds, keep) = run_of(advance(wall, &sized, &pts(&[A, B])));
    assert_eq!((cmds[0].0, keep), ("fillet", Chain::No), "the radius option makes it at the second pick");
    assert_eq!((&cmds[0].1["ids"], cmds[0].1["radius"].as_f64()), (&json!([a, b]), Some(500.0)));
    // The pointer 700 mm in from the corner each way: the arc that passes through it has a radius of about 2390.
    let (cmds, _) = run_of(advance(wall, &on(None, Held::Pair(a, b)), &pts(&[A, B, INSIDE])));
    let r = cmds[0].1["radius"].as_f64().unwrap_or(0.0);
    assert!((r - 2390.0).abs() < 5.0, "{r}");
    assert!(fillet_radius_at(&w.model, a, a, P2(INSIDE.0, INSIDE.1)).is_none(), "a wall and itself are parallel");
    assert!(matches!(advance(wall, &on(None, Held::Nothing), &pts(&[A, B, INSIDE])), Step::Ignore), "a third click with no pair held");
}

#[test]
fn tangent_end_arc_leaves_the_wall_that_ends_at_its_first_point_and_the_next_arc_starts_at_its_end() {
    let (w, ..) = corner();
    let wall = of("wall");
    let with = |tangent| Ctx { shape: Shape::TangentArc, tangent, ..w.ctx() };
    assert!(matches!(advance(wall, &with(None), &pts(&[A])), Step::Refuse(_)), "no wall ends at the first point");
    let east = Some(P2(1.0, 0.0));
    assert_eq!(advance(wall, &with(east), &pts(&[A])), Step::More);
    let (cmds, keep) = run_of(advance(wall, &with(east), &pts(&[A, (3000.0, 3000.0)])));
    assert_eq!((cmds[0].0, keep), ("wall", Chain::Last), "the end clicked starts the next arc");
    assert!(cmds[0].1["arc"].as_f64().is_some_and(|a| a.abs() > 45.0), "an arc, not a straight wall: {}", cmds[0].1);
    // The way a wall of the level ran where it ends, or away from where it starts.
    let at = |x: f64, y: f64| tangent_at(&w.model, w.level, P2(x, y));
    let north = at(-26000.0, -27000.0).expect("the second wall ends there");
    assert!(north.0.abs() < 1e-3 && (north.1 - 1.0).abs() < 1e-3, "{north:?}");
    let west = at(-30000.0, -30000.0).expect("the first wall starts there");
    assert!((west.0 + 1.0).abs() < 1e-3 && west.1.abs() < 1e-3, "away from where it starts: {west:?}");
    assert!(at(-12345.0, -12345.0).is_none(), "no line ends there");
}
