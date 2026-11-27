//! The tools through the app, with no window: clicks at model points, the commands the core runs, the status bar's message, undo.

use bimcraft_core::draw::Shape;
use bimcraft_core::geom::P2;
use bimcraft_core::model::Element;

use crate::app::App;
use crate::select::How;

/// A click at a model point: the press and release at the pixel it lies at.
fn click(app: &mut App, p: P2) {
    let (x, y) = app.camera.view(app.size.0, app.size.1).px(p);
    app.release((x, y), (x, y), How::Replace);
}

/// The pointer at a model point, no click.
fn hover(app: &mut App, p: P2) {
    let (x, y) = app.camera.view(app.size.0, app.size.1).px(p);
    app.hover(x, y);
}

fn walls(app: &App) -> usize {
    app.session.model.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).count()
}

fn elements(app: &App) -> usize {
    app.session.model.elements.len()
}

/// Far from the house and everything in it, where a point lands on the nearest 10 mm or an angle step.
const A: P2 = P2(-30000.0, -30000.0);
const B: P2 = P2(-26000.0, -30000.0);
const C: P2 = P2(-26000.0, -26000.0);

#[test]
fn a_wall_chain_is_made_click_by_click_and_the_last_point_clicked_again_ends_it() {
    let mut app = App::sample();
    app.set_tool("wall");
    let before = walls(&app);
    click(&mut app, A);
    assert_eq!((walls(&app), app.picked.len()), (before, 1), "the first click only starts the chain");
    click(&mut app, B);
    assert_eq!((walls(&app), app.picked.len()), (before + 1, 2), "the second makes a wall and the chain goes on");
    click(&mut app, C);
    assert_eq!((walls(&app), app.picked.len()), (before + 2, 3));
    click(&mut app, C);
    assert_eq!(walls(&app), before + 2, "the same place again makes nothing");
    assert!(app.picked.is_empty() && app.tool == Some("wall"), "the chain is over and the tool is still in hand");
    assert!(app.message.is_none(), "{:?}", app.message);
    assert!(app.undo());
    assert_eq!(walls(&app), before + 1, "each wall is an undo step of its own");
}

#[test]
fn backspace_gives_the_last_point_back_and_a_wall_chain_goes_on_from_the_one_before() {
    let mut app = App::sample();
    assert!(!app.back_point() && !app.finish_tool(), "with no tool the keys are nobody's here");
    app.set_tool("wall");
    assert!(!app.back_point() && !app.finish_tool(), "and with a tool but no point");
    click(&mut app, A);
    click(&mut app, B);
    assert!(app.back_point());
    assert_eq!(app.picked.len(), 1);
    assert!(app.landing.is_some(), "the pointer still lands, from the point before");
    assert!(app.finish_tool() && app.picked.is_empty(), "Enter ends the chain");
}

#[test]
fn a_point_tool_makes_its_element_on_a_click_and_the_core_refusing_is_the_message() {
    let mut app = App::sample();
    app.set_tool("column");
    let before = elements(&app);
    click(&mut app, A);
    assert_eq!(elements(&app), before + 1, "{:?}", app.message);
    assert!(app.picked.is_empty(), "a point tool starts again at once");
    app.set_tool("door");
    click(&mut app, B);
    assert_eq!(elements(&app), before + 1, "a door with no wall under it is refused");
    assert!(app.message.as_deref().is_some_and(|m| !m.is_empty()), "and the core says why");
    assert!(app.picked.is_empty(), "the click that was refused is given back");
    let wall = (3000.0, 0.0);
    click(&mut app, P2(wall.0, wall.1));
    assert!(elements(&app) > before + 1, "a door in the house's south wall: {:?}", app.message);
    assert!(app.message.is_none());
}

#[test]
fn a_path_is_finished_by_enter_and_a_stair_by_its_second_click() {
    let mut app = App::sample();
    app.set_tool("railing");
    let before = elements(&app);
    for p in [A, B, C] {
        click(&mut app, p);
    }
    assert_eq!((elements(&app), app.picked.len()), (before, 3), "a path waits");
    assert!(app.finish_tool(), "Enter takes it");
    assert_eq!(elements(&app), before + 1, "{:?}", app.message);
    assert!(app.picked.is_empty());
    app.set_tool("stair");
    click(&mut app, A);
    assert_eq!(elements(&app), before + 1);
    click(&mut app, B);
    assert_eq!(elements(&app), before + 2, "{:?}", app.message);
}

#[test]
fn a_shape_makes_its_walls_as_one_undo_step_and_previews_what_the_next_click_makes() {
    let mut app = App::sample();
    app.set_tool("wall");
    app.set_shape(Shape::Rectangle);
    let before = walls(&app);
    click(&mut app, A);
    hover(&mut app, P2(-27000.0, -27000.0));
    assert_eq!(app.shape_preview().len(), 4, "the rectangle the next click would make");
    click(&mut app, P2(-27000.0, -27000.0));
    assert_eq!(walls(&app), before + 4, "{:?}", app.message);
    assert!(app.picked.is_empty(), "a shape is made whole");
    assert!(app.undo());
    assert_eq!(walls(&app), before, "four walls, one undo step");
    app.set_shape(Shape::Line);
    click(&mut app, A);
    hover(&mut app, B);
    assert!(app.shape_preview().is_empty(), "a line is drawn by the chain's own segment, which the pick layer shows");
    app.set_shape(Shape::Circle);
    assert!(app.picked.is_empty(), "another shape starts again");
}

#[test]
fn a_tool_the_machine_does_not_know_leaves_the_points_it_picks_alone_and_the_shape_goes_with_the_tool() {
    let mut app = App::sample();
    app.set_tool("array");
    let before = elements(&app);
    click(&mut app, A);
    click(&mut app, B);
    assert_eq!((elements(&app), app.picked.len()), (before, 2), "Array has no command in the core yet");
    app.set_tool("beam");
    app.set_shape(Shape::Rectangle);
    assert!(app.shape_preview().is_empty(), "a beam takes no shape");
    click(&mut app, A);
    click(&mut app, B);
    assert_eq!(elements(&app), before + 1, "a beam is a line from click to click: {:?}", app.message);
    assert_eq!(app.picked.len(), 1, "and the run goes on from where it ended");
}

/// The ends of wall `id`, as the model has them.
fn ends(app: &App, id: u64) -> (P2, P2) {
    let w = app.session.model.wall(id).expect("a wall");
    (w.start, w.end)
}

/// A wall of the model between two points made through the app, the newest wall there is.
fn make_wall(app: &mut App, a: P2, b: P2) -> u64 {
    let level = app.level;
    assert!(app.run("wall", &serde_json::json!({"points": [[a.0, a.1], [b.0, b.1]], "level": level})), "{:?}", app.message);
    app.session.model.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id).max().expect("a wall")
}

/// The plan looking at `p` at a tenth of a pixel to the millimetre, so what the test made there is drawn, and picked under the pointer.
fn look_at(app: &mut App, p: P2) {
    app.camera = crate::camera::Camera { cx: p.0, cy: p.1, scale: 0.1, angle: 0.0 };
    app.redraw();
}

fn near(p: P2, x: f64, y: f64) -> bool {
    p.dist(P2(x, y)) < 30.0
}

const D1: P2 = P2(-20000.0, -20000.0);
const D2: P2 = P2(-18000.0, -20000.0);

#[test]
fn move_slides_the_selection_by_the_distance_between_two_clicks_and_goes_back_with_undo() {
    let mut app = App::sample();
    let id = make_wall(&mut app, A, B);
    let (start, end) = ends(&app, id);
    app.select(vec![id]);
    app.set_tool("move");
    click(&mut app, D1);
    assert_eq!(app.picked.len(), 1, "the base point");
    click(&mut app, D2);
    let (s, e) = ends(&app, id);
    assert!(near(s, start.0 + 2000.0, start.1) && near(e, end.0 + 2000.0, end.1), "{s:?} {e:?}");
    assert!(app.tool.is_none() && app.picked.is_empty(), "Move is done with");
    assert_eq!(app.selection, vec![id], "and the wall is still selected");
    assert!(app.undo());
    assert_eq!(ends(&app, id), (start, end));
}

#[test]
fn with_nothing_selected_a_modify_tool_says_so_and_a_copy_is_what_is_selected_after_it() {
    let mut app = App::sample();
    let id = make_wall(&mut app, A, B);
    app.set_tool("copy");
    click(&mut app, D1);
    assert!(app.message.as_deref().is_some_and(|m| m.contains("select")), "{:?}", app.message);
    assert!(app.picked.is_empty(), "the click is given back");
    app.select(vec![id]);
    let before = walls(&app);
    click(&mut app, D1);
    click(&mut app, D2);
    assert_eq!(walls(&app), before + 1, "{:?}", app.message);
    assert_eq!(app.selection.len(), 1);
    assert_ne!(app.selection[0], id, "the copy is selected, not the wall it came from");
    assert!(near(ends(&app, id).0, A.0, A.1), "the first is where it was");
}

#[test]
fn rotate_turns_the_selection_about_the_centre_by_the_angle_between_the_rays() {
    let mut app = App::sample();
    let id = make_wall(&mut app, A, B);
    app.select(vec![id]);
    app.set_tool("rotate");
    for p in [D1, P2(D1.0 + 1000.0, D1.1), P2(D1.0, D1.1 + 1000.0)] {
        click(&mut app, p);
    }
    let (s, e) = ends(&app, id);
    let d = e - s;
    assert!(d.0.abs() < 30.0 && (d.1.abs() - 4000.0).abs() < 30.0, "an east wall turned a quarter is a north one: {d:?}");
    assert!(app.tool.is_none(), "Rotate is done with: {:?}", app.message);
}

#[test]
fn mirror_with_an_axis_of_two_points_makes_a_mirrored_copy() {
    let mut app = App::sample();
    let id = make_wall(&mut app, A, B);
    app.select(vec![id]);
    app.set_tool("mirror");
    let before = walls(&app);
    click(&mut app, P2(-30000.0, -25000.0));
    click(&mut app, P2(-26000.0, -25000.0));
    assert_eq!(walls(&app), before + 1, "{:?}", app.message);
    assert!(app.tool.is_none());
}

#[test]
fn split_cuts_the_element_under_the_pointer_and_stays_in_hand() {
    let mut app = App::sample();
    let id = make_wall(&mut app, A, B);
    let before = walls(&app);
    look_at(&mut app, P2(-28000.0, -29000.0));
    app.set_tool("split");
    click(&mut app, P2(-28000.0, -30000.0));
    assert_eq!(walls(&app), before + 1, "{:?}", app.message);
    assert!(app.session.model.wall(id).is_some_and(|w| w.start.dist(w.end) < 3000.0), "the wall is shorter");
    assert_eq!(app.tool, Some("split"), "Split stays in hand for the next");
    click(&mut app, P2(-30000.0, -23000.0));
    assert!(app.message.is_some() && walls(&app) == before + 1, "nothing there to split");
}

#[test]
fn trim_to_corner_takes_two_picks_and_extends_both_to_where_they_meet() {
    let mut app = App::sample();
    let first = make_wall(&mut app, A, P2(-28500.0, -30000.0));
    let second = make_wall(&mut app, P2(-28000.0, -29000.0), P2(-28000.0, -27000.0));
    look_at(&mut app, P2(-28500.0, -28500.0));
    app.set_tool("trim");
    click(&mut app, P2(-29000.0, -30000.0));
    assert_eq!(app.picked.len(), 1, "the first pick is held with its point");
    click(&mut app, P2(-28000.0, -28500.0));
    let (s, e) = ends(&app, first);
    assert!(near(P2(s.0.max(e.0), s.1), -28000.0, s.1) && (s.1 + 30000.0).abs() < 30.0, "the first reaches the second's line: {s:?} {e:?}");
    assert!(ends(&app, second).0.1.min(ends(&app, second).1.1) < -29900.0, "and the second reaches the first's: {:?}", ends(&app, second));
    assert!(app.picked.is_empty() && app.tool == Some("trim"), "{:?}", app.message);
}

#[test]
fn align_moves_the_second_wall_onto_the_first_ones_line_and_offset_copies_across() {
    let mut app = App::sample();
    make_wall(&mut app, A, B);
    let second = make_wall(&mut app, P2(-29000.0, -25000.0), P2(-27000.0, -25000.0));
    look_at(&mut app, P2(-28000.0, -27500.0));
    app.set_tool("align");
    click(&mut app, P2(-28000.0, -30000.0));
    assert!(app.tooling.held != crate::tooling::Held::Nothing, "the reference line is held");
    click(&mut app, P2(-28000.0, -25000.0));
    let (s, e) = ends(&app, second);
    assert!((s.1 + 30000.0).abs() < 150.0 && (e.1 + 30000.0).abs() < 150.0, "the second wall lies on the first's line: {s:?} {e:?}");
    assert_eq!(app.tool, Some("align"));
    app.set_tool("align");
    app.set_tool("offset");
    let before = walls(&app);
    click(&mut app, P2(-28000.0, -30000.0));
    click(&mut app, P2(-28000.0, -29000.0));
    assert_eq!(walls(&app), before + 1, "{:?}", app.message);
}

/// Two walls meeting at B, picked with Fillet Arc in hand: the first runs east to it, the second north from it.
fn two_walls_picked_for_a_fillet(app: &mut App) -> (u64, u64) {
    let (first, second) = (make_wall(app, A, B), make_wall(app, B, C));
    look_at(app, P2(-28000.0, -28000.0));
    app.set_tool("wall");
    app.set_shape(Shape::Fillet);
    click(app, P2(-29000.0, -30000.0));
    assert_eq!(app.tooling.held, crate::tooling::Held::Element(first), "{:?}", app.message);
    click(app, P2(-26000.0, -28000.0));
    assert_eq!((app.picked.len(), app.tooling.held), (2, crate::tooling::Held::Pair(first, second)), "{:?}", app.message);
    (first, second)
}

#[test]
fn fillet_arc_picks_two_walls_and_a_click_sizes_the_arc_with_the_walls_cut_back_to_it() {
    let mut app = App::sample();
    let before = walls(&app) + 2;
    let (first, _) = two_walls_picked_for_a_fillet(&mut app);
    hover(&mut app, P2(-26700.0, -29300.0));
    assert_eq!(app.shape_preview().len(), 3, "the ghost is the arc and the two walls cut back to it");
    click(&mut app, P2(-26700.0, -29300.0));
    assert!(app.message.is_none(), "{:?}", app.message);
    assert_eq!(walls(&app), before + 1, "the arc is a wall of its own");
    let (start, end) = ends(&app, first);
    assert!(start.dist(B).min(end.dist(B)) > 500.0, "the first wall is cut back from the corner: {start:?} {end:?}");
    assert!(app.picked.is_empty() && app.tooling.held == crate::tooling::Held::Nothing && app.tool == Some("wall"));
    assert!(app.undo() && walls(&app) == before, "one undo step");
}

#[test]
fn fillet_arc_takes_the_radius_typed_after_its_two_walls_and_says_when_it_does_not_fit() {
    let mut app = App::sample();
    let before = walls(&app) + 2;
    two_walls_picked_for_a_fillet(&mut app);
    hover(&mut app, P2(-26700.0, -29300.0));
    assert!(app.type_text("900"));
    assert!(app.status().contains("Radius") && app.status().contains("900"), "{}", app.status());
    assert_eq!(app.shape_preview().len(), 3, "the ghost has the radius typed");
    assert!(app.finish_tool() && app.message.is_none(), "{:?}", app.message);
    assert_eq!((walls(&app), app.picked.len()), (before + 1, 0));
    // Two walls again, a radius too big for them: refused, and the walls stay picked to be sized again.
    let mut app = App::sample();
    two_walls_picked_for_a_fillet(&mut app);
    assert!(app.type_text("90000") && app.finish_tool());
    assert!(app.message.is_some() && app.picked.len() == 2, "{:?} {:?}", app.message, app.picked);
}

#[test]
fn tangent_end_arc_starts_at_the_end_of_a_wall_leaves_the_way_it_ran_and_goes_on_from_its_end() {
    let mut app = App::sample();
    make_wall(&mut app, A, B);
    let before = walls(&app);
    look_at(&mut app, P2(-24000.0, -28000.0));
    app.set_tool("wall");
    app.set_shape(Shape::TangentArc);
    click(&mut app, P2(-20000.0, -20000.0));
    assert!(app.message.is_some() && app.picked.is_empty(), "no wall ends there: {:?}", app.message);
    click(&mut app, B);
    assert_eq!(app.picked.len(), 1, "{:?}", app.message);
    assert!(
        app.tooling.tangent.is_some_and(|t| (t.0 - 1.0).abs() < 1e-3 && t.1.abs() < 1e-3),
        "east, the way the wall ran: {:?}",
        app.tooling.tangent
    );
    hover(&mut app, P2(-23000.0, -27000.0));
    assert!(!app.shape_preview().is_empty(), "the ghost of the arc");
    click(&mut app, P2(-23000.0, -27000.0));
    assert_eq!(walls(&app), before + 1, "{:?}", app.message);
    assert!(app.picked.len() == 1 && near(app.picked[0], -23000.0, -27000.0), "the next arc starts where this one ended: {:?}", app.picked);
    let arc = app.session.model.elements.iter().find_map(|(_, e)| match e {
        Element::Wall(w) if w.arc != 0.0 => Some(w.arc),
        _ => None,
    });
    assert!(arc.is_some_and(|a| a.abs() > 45.0), "an arc wall was made: {arc:?}");
}

#[test]
fn a_wall_ghost_is_as_thick_as_its_type_and_a_door_or_a_column_stands_where_the_pointer_is() {
    let mut app = App::sample();
    look_at(&mut app, P2(-28000.0, -28000.0));
    app.set_tool("wall");
    hover(&mut app, A);
    assert!(app.shape_preview().is_empty() && app.stands().is_none(), "nothing yet, before the first point");
    click(&mut app, A);
    hover(&mut app, B);
    let ghost = app.shape_preview();
    assert!(matches!(ghost.as_slice(), [bimcraft_core::curves::Curve::Poly { closed: true, .. }]), "the wall's outline: {ghost:?}");
    app.set_tool("wall");
    app.set_tool("column");
    hover(&mut app, A);
    let column = app.stands();
    assert!(column.is_some_and(|(d, id)| d.owners.contains(&Some(id))), "the column drawn as the plan draws it");
    app.set_tool("column");
    app.set_tool("door");
    hover(&mut app, A);
    assert!(app.stands().is_none(), "over no wall a door stands nowhere");
    look_at(&mut app, P2(1000.0, 0.0));
    hover(&mut app, P2(1000.0, 0.0));
    assert!(app.stands().is_some(), "over the house's south wall it stands in it");
}
