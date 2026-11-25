//! The app's state and handlers, tested with no window: `App` driven by its methods, as the elements drive it.

use super::*;

/// A point of the canvas that picks an element, found by looking over the plan.
fn pickable(app: &App) -> Option<(f32, f32, Id)> {
    let (w, h) = app.size;
    (0..(h as u32))
        .step_by(6)
        .flat_map(|y| (0..(w as u32)).step_by(6).map(move |x| (x as f32, y as f32)))
        .find_map(|(x, y)| app.pick(x, y).map(|id| (x, y, id)))
}

#[test]
fn the_sample_house_opens_on_its_first_level_drawn_and_framed() {
    let app = App::sample();
    assert!(app.message.is_none(), "the sample script runs: {:?}", app.message);
    assert!(app.level.is_some() && !app.level_name.is_empty());
    let d = app.drawing.as_ref().expect("a drawing");
    assert!(!d.prims.is_empty());
    let c = app.camera;
    assert!(c.scale > 0.0 && c.cx.is_finite() && c.cy.is_finite());
    assert!(pickable(&app).is_some(), "something on the plan can be picked");
}

#[test]
fn the_plan_can_be_the_cores_own_raster_shown_as_one_image() {
    let mut app = App::sample();
    app.mode = PlanMode::Pixels;
    app.redraw();
    let size = app.pixels.with_rgba(|w, h, bytes| (w, h, bytes.len()));
    let (w, h, len) = size.expect("an image");
    assert_eq!((w as f32, h as f32), app.size);
    assert_eq!(len, (w * h * 4) as usize, "RGBA, row by row");
    let ink = app.pixels.with_rgba(|_, _, bytes| bytes.chunks(4).any(|p| p[0] < 128)).unwrap_or(false);
    assert!(ink, "the plan has dark lines on its paper");
}

#[test]
fn a_click_selects_what_it_hits_and_shows_its_properties() {
    let mut app = App::sample();
    let (x, y, id) = pickable(&app).expect("a pickable point");
    app.release((x - 1.0, y - 1.0), (x, y), How::Replace);
    assert_eq!(app.selection, vec![id]);
    assert!(app.props.iter().any(|r| r.name == "id" && r.value == id.to_string()), "{:?}", app.props);
    app.release((x - 40.0, y), (x, y), How::Replace);
    assert_eq!(app.selection, vec![id], "a drag that drew no box is not a click");
    app.select_at(-1000.0, -1000.0, How::Replace);
    assert!(app.selection.is_empty() && app.props.is_empty());
}

/// A point of the canvas and the element it picks.
type Spot = (f32, f32, Id);

/// Two points of the canvas that pick different elements.
fn two_distinct(app: &App) -> Option<(Spot, Spot)> {
    let (w, h) = app.size;
    let mut first: Option<Spot> = None;
    for y in (0..(h as u32)).step_by(6) {
        for x in (0..(w as u32)).step_by(6) {
            let (x, y) = (x as f32, y as f32);
            let Some(id) = app.pick(x, y) else { continue };
            match first {
                None => first = Some((x, y, id)),
                Some(a) if a.2 != id => return Some((a, (x, y, id))),
                Some(_) => {}
            }
        }
    }
    None
}

/// A point with more than one element under it, as a door in a wall is.
fn stacked(app: &App) -> Option<(f32, f32)> {
    let (w, h) = app.size;
    let d = app.drawing.as_ref()?;
    (0..(h as u32))
        .step_by(3)
        .flat_map(|y| (0..(w as u32)).step_by(3).map(move |x| (x as f32, y as f32)))
        .find(|&p| d.pick_all(p, REACH).len() >= 2)
}

#[test]
fn ctrl_adds_shift_takes_away_and_a_plain_click_replaces() {
    let mut app = App::sample();
    let ((ax, ay, a), (bx, by, b)) = two_distinct(&app).expect("two things to pick");
    app.select_at(ax, ay, How::Replace);
    app.select_at(bx, by, How::Add);
    assert_eq!(app.selection, vec![a, b]);
    app.select_at(ax, ay, How::Add);
    assert_eq!(app.selection, vec![a, b], "adding what is there adds nothing");
    app.select_at(ax, ay, How::Remove);
    assert_eq!(app.selection, vec![b]);
    app.select_at(-1000.0, -1000.0, How::Add);
    app.select_at(-1000.0, -1000.0, How::Remove);
    assert_eq!(app.selection, vec![b], "nothing under the pointer changes nothing");
    app.select_at(ax, ay, How::Replace);
    assert_eq!(app.selection, vec![a]);
}

#[test]
fn a_box_left_to_right_takes_what_it_holds_and_right_to_left_what_it_touches() {
    let mut app = App::sample();
    let (w, h) = app.size;
    let half = (w / 2.0, h);
    app.sweep((0.0, 0.0), half);
    assert_eq!(app.rubber, Some(Rubber { from: (0.0, 0.0), to: half }), "the box shows while the press is held");
    app.release((0.0, 0.0), half, How::Replace);
    assert!(app.rubber.is_none(), "the box is gone once the press ends");
    let window = app.selection.clone();
    app.sweep(half, (0.0, 0.0));
    app.release(half, (0.0, 0.0), How::Replace);
    let crossing = app.selection.clone();
    assert!(!window.is_empty(), "the left half of the plan holds something whole");
    assert!(window.iter().all(|id| crossing.contains(id)), "what a window holds, a crossing box touches");
    assert!(crossing.len() > window.len(), "and what lies across the middle only a crossing box takes: {window:?} v {crossing:?}");
}

#[test]
fn a_box_adds_and_takes_away_like_a_click() {
    let mut app = App::sample();
    let ((ax, ay, a), _) = two_distinct(&app).expect("two things to pick");
    let (w, h) = app.size;
    let held = app.drawing.as_ref().map(|d| d.in_box((0.0, 0.0), (w, h), false)).unwrap_or_default();
    assert!(held.len() > 1, "a box over the whole canvas holds something");
    app.select_at(ax, ay, How::Replace);
    app.sweep((0.0, 0.0), (w, h));
    app.release((0.0, 0.0), (w, h), How::Add);
    assert_eq!(app.selection.first(), Some(&a), "what was selected stays first");
    assert!(held.iter().all(|id| app.selection.contains(id)), "and the box added what it held");
    app.sweep((0.0, 0.0), (w, h));
    app.release((0.0, 0.0), (w, h), How::Remove);
    assert!(app.selection.iter().all(|id| !held.contains(id)), "the same box with Shift takes all of it away again");
}

#[test]
fn with_a_tool_in_hand_the_primary_drag_pans_and_draws_no_box() {
    let mut app = App::sample();
    app.set_tool("wall");
    let before = app.camera;
    app.grab();
    app.sweep((100.0, 100.0), (130.0, 110.0));
    assert!(app.rubber.is_none());
    assert_ne!(app.camera, before, "the plan followed the pointer");
}

#[test]
fn tab_steps_through_what_lies_under_the_pointer_and_a_click_takes_the_one_it_reached() {
    let mut app = App::sample();
    let (x, y) = stacked(&app).expect("elements lie over one another in the house");
    app.hover(x, y);
    let under = app.under.clone();
    assert_eq!(app.hovered, under.first().copied(), "the nearest first");
    assert!(app.step(false));
    assert_eq!(app.hovered, Some(under[1]));
    app.select_at(x, y, How::Replace);
    assert_eq!(app.selection, vec![under[1]], "a click takes what Tab reached");
    for _ in 1..under.len() {
        app.step(false);
    }
    assert_eq!(app.hovered, under.first().copied(), "past the last it comes round");
    assert!(app.step(true));
    assert_eq!(app.hovered, under.last().copied(), "Shift+Tab goes back, round the other end");
    app.hover(x, y);
    assert_eq!(app.hovered, under.last().copied(), "the pointer has not moved: what Tab chose stays chosen");
    app.hover(x + 0.1, y);
    assert_eq!(app.hovered, app.under.first().copied(), "a move starts over from the nearest");
    app.leave();
    assert!(app.under.is_empty() && !app.step(false), "with the pointer gone there is nothing to step through");
}

#[test]
fn zoom_to_fit_frames_the_plan_again_and_previous_goes_back_one_command_at_a_time() {
    let mut app = App::sample();
    let framed = app.camera;
    app.zoom(300.0, 200.0, 4.0);
    let wheeled = app.camera;
    assert_ne!(wheeled, framed);
    app.zoom_fit();
    assert_eq!(app.camera, framed, "the level is framed as it was first shown");
    assert!(app.previous(), "back to how it was before Fit");
    assert_eq!(app.camera, wheeled);
    assert!(!app.previous(), "the wheel is no command: there is nothing before Fit to go back to");
}

#[test]
fn the_zoom_buttons_keep_the_middle_of_the_canvas_and_a_bad_factor_does_nothing() {
    let mut app = App::sample();
    let centre = (app.size.0 / 2.0, app.size.1 / 2.0);
    let held = app.camera.view(app.size.0, app.size.1).mm(centre);
    let scale = app.camera.scale;
    app.zoom_by(2.0);
    assert!((app.camera.scale - scale * 2.0).abs() < 1e-9);
    let back = app.camera.view(app.size.0, app.size.1).px(held);
    assert!((back.0 - centre.0).abs() < 0.01 && (back.1 - centre.1).abs() < 0.01, "the middle stays the middle: {back:?}");
    let (camera, revision) = (app.camera, app.revision);
    for bad in [f64::NAN, 0.0, -1.0, f64::INFINITY] {
        app.zoom_by(bad);
    }
    assert_eq!((app.camera, app.revision), (camera, revision));
    assert!(app.previous() && (app.camera.scale - scale).abs() < 1e-9, "one step back undoes the button");
}

#[test]
fn zoom_in_region_zooms_to_the_box_dragged_either_way_and_selects_nothing() {
    let mut app = App::sample();
    let (x, y, _) = pickable(&app).expect("a pickable point");
    app.select_at(x, y, How::Replace);
    let (selection, before) = (app.selection.clone(), app.camera);
    app.arm_region();
    assert!(app.region);
    app.sweep((250.0, 150.0), (50.0, 50.0));
    assert_eq!(app.rubber, Some(Rubber { from: (50.0, 50.0), to: (250.0, 150.0) }), "a region has no crossing: drawn as a window");
    app.release((250.0, 150.0), (50.0, 50.0), How::Replace);
    assert!(app.camera.scale > before.scale, "closer");
    assert!(!app.region && app.rubber.is_none(), "one region per arming");
    assert_eq!(app.selection, selection, "and nothing was selected");
    app.arm_region();
    app.release((10.0, 10.0), (11.0, 11.0), How::Replace);
    assert!(app.region, "a click leaves it armed");
    assert!(app.escape() && !app.region, "Escape disarms it before anything else");
    app.arm_region();
    app.arm_region();
    assert!(!app.region, "arming twice puts it down");
    assert!(app.previous() && app.camera == before, "and Previous gives the plan back");
}

#[test]
fn previous_keeps_the_last_twenty_steps() {
    let mut app = App::sample();
    for _ in 0..(STEPS + 5) {
        app.zoom_by(1.1);
    }
    assert_eq!(app.history.len(), STEPS);
    let mut n = 0;
    while app.previous() {
        n += 1;
    }
    assert_eq!(n, STEPS);
}

#[test]
fn escape_puts_down_the_tool_then_clears_the_selection() {
    let mut app = App::sample();
    let (x, y, _) = pickable(&app).expect("a pickable point");
    app.select_at(x, y, How::Replace);
    app.set_tool("wall");
    assert!(app.escape());
    assert!(app.tool.is_none() && !app.selection.is_empty(), "the tool goes first");
    assert!(app.escape());
    assert!(app.selection.is_empty());
    assert!(!app.escape(), "with nothing in hand and nothing selected the key is free");
}

#[test]
fn a_box_or_a_click_with_hostile_numbers_selects_nothing_and_leaves_no_band() {
    let mut app = App::sample();
    let (x, y, id) = pickable(&app).expect("a pickable point");
    app.select_at(x, y, How::Replace);
    app.sweep((f32::NAN, 0.0), (10.0, 10.0));
    app.sweep((0.0, 0.0), (f32::INFINITY, 10.0));
    assert!(app.rubber.is_none());
    app.sweep((0.0, 0.0), (200.0, 200.0));
    assert!(app.rubber.is_some());
    app.release((0.0, 0.0), (f32::NAN, 1.0), How::Replace);
    assert!(app.rubber.is_none());
    assert_eq!(app.selection, vec![id], "nothing was taken");
    app.sweep((0.0, 0.0), (200.0, 200.0));
    app.cancel_press();
    assert!(app.rubber.is_none());
    assert_eq!(app.selection, vec![id], "a cancelled press selects nothing");
}

#[test]
fn a_wheel_zooms_about_the_pointer_and_a_drag_pans_the_plan() {
    let mut app = App::sample();
    let before = app.camera;
    let at = (300.0, 200.0);
    let held = app.camera.view(app.size.0, app.size.1).mm(at);
    app.zoom(at.0, at.1, 2.0);
    assert!((app.camera.scale - before.scale * 2.0).abs() < 1e-9);
    let back = app.camera.view(app.size.0, app.size.1).px(held);
    assert!((back.0 - at.0).abs() < 0.01 && (back.1 - at.1).abs() < 0.01, "the point under the wheel stays under it");
    let revision = app.revision;
    app.grab();
    app.drag(10.0, 0.0);
    app.drag(30.0, 5.0);
    let moved = app.camera.view(app.size.0, app.size.1).px(held);
    assert!((moved.0 - (at.0 + 30.0)).abs() < 0.01 && (moved.1 - (at.1 + 5.0)).abs() < 0.01, "followed to the last point: {moved:?}");
    assert_eq!(app.revision, revision, "the plan on screen moves through the live offset and scale, and is not drawn again");
    app.tick(SETTLE + 1.0);
    assert!(app.settle_if_still(), "once the camera has been still it is");
    assert!(app.revision > revision);
    assert_eq!((app.live_offset.get(), app.live_scale.get()), ([0.0, 0.0], 1.0), "and the drawing needs no moving");
}

#[test]
fn the_plan_on_screen_is_the_last_drawing_moved_and_picks_through_the_move() {
    let mut app = App::sample();
    let (x, y, id) = pickable(&app).expect("a pickable point");
    let revision = app.revision;
    app.grab();
    app.drag(40.0, 25.0);
    let (offset, scale) = (app.live_offset.get(), app.live_scale.get());
    assert!((offset[0] - 40.0).abs() < 0.01 && (offset[1] - 25.0).abs() < 0.01 && scale == 1.0, "{offset:?} {scale}");
    assert_eq!(app.revision, revision, "no drawing by the core");
    assert_eq!(app.pick(x + 40.0, y + 25.0), Some(id), "where the element shows now, it is picked");
    app.hover(x + 40.0, y + 25.0);
    assert_eq!(app.hovered, Some(id), "and hovered");

    // A wheel notch too: the point of the drawing, where the camera now puts it, comes back to where it was drawn.
    app.zoom(300.0, 200.0, 2.0);
    let mm = app.drawn.view(app.size.0, app.size.1).mm((x, y));
    let shown = app.camera.view(app.size.0, app.size.1).px(mm);
    let back = app.drawn_at(shown);
    assert!((back.0 - x).abs() < 0.01 && (back.1 - y).abs() < 0.01, "{back:?} for {x},{y}");
    assert!((app.live_scale.get() - 2.0).abs() < 1e-6);

    // A click ends the wait: what is picked is picked in the exact drawing.
    app.release((shown.0 - 1.0, shown.1 - 1.0), (shown.0, shown.1), How::Replace);
    assert!(app.revision > revision && app.live_scale.get() == 1.0, "a release draws exactly first");
}

#[test]
fn the_plan_waits_for_the_camera_to_be_still_before_it_is_drawn_exactly() {
    let close = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 1e-9);
    let mut app = App::sample();
    assert_eq!(app.settle_in(), None, "an exact plan waits for nothing");
    app.tick(2.0);
    app.zoom(300.0, 200.0, 1.5);
    assert!(close(app.settle_in(), SETTLE), "{:?}", app.settle_in());
    app.tick(2.1);
    assert!(close(app.settle_in(), SETTLE - 0.1));
    assert!(!app.settle_if_still(), "not yet");
    app.zoom(300.0, 200.0, 1.5);
    assert!(close(app.settle_in(), SETTLE), "a move starts the wait over");
    app.tick(2.1 + SETTLE + 0.01);
    assert!(app.settle_if_still());
    assert_eq!(app.settle_in(), None);
    app.tick(f64::NAN);
    assert_eq!(app.now, 2.1 + SETTLE + 0.01, "a clock that is no number is not taken");
}

#[test]
fn deleting_goes_through_the_engine_and_undo_brings_the_element_back() {
    let mut app = App::sample();
    let (x, y, id) = pickable(&app).expect("a pickable point");
    app.select_at(x, y, How::Replace);
    assert!(app.session.model.elements.get(id).is_some());
    assert!(app.delete_selection(), "refused: {:?}", app.message);
    assert!(app.session.model.elements.get(id).is_none(), "the engine deleted it");
    assert!(app.selection.is_empty());
    assert!(app.undo(), "refused: {:?}", app.message);
    assert!(app.session.model.elements.get(id).is_some(), "undo gave it back");
    assert!(!app.delete_selection(), "with nothing selected there is nothing to delete");
}

#[test]
fn another_level_is_shown_framed_and_a_stale_one_is_refused() {
    let mut app = App::sample();
    assert!(app.levels.len() >= 2, "the sample house has two levels: {:?}", app.levels);
    let first = app.level;
    let other = app.levels.iter().map(|(id, _)| *id).find(|id| Some(*id) != first).expect("a second level");
    app.selection = vec![1];
    let revision = app.revision;
    app.set_level(other);
    assert_eq!(app.level, Some(other));
    assert!(app.selection.is_empty() && app.revision > revision && app.drawing.is_some());
    assert_eq!(app.level_name, app.levels.iter().find(|(id, _)| *id == other).map(|(_, n)| n.clone()).unwrap_or_default());
    app.set_level(7);
    assert_eq!(app.level, Some(other), "an id that is no level changes nothing");
    app.set_tab(3);
    app.set_tab(99);
    assert_eq!(app.tab, 3);
}

#[test]
fn a_field_typed_into_goes_to_the_engine_as_a_modify_and_a_bad_number_never_does() {
    let mut app = App::sample();
    let level = app.level.expect("a level");
    app.selection = vec![level];
    app.refresh();
    let name = app.fields.iter().position(|f| f.name == "name").expect("a level has a name to edit");
    assert_eq!(app.fields[name].kind, Kind::Text);
    app.fields[name].edit = TextEdit::new("Ground");
    assert!(app.commit_field(name), "refused: {:?}", app.message);
    assert_eq!(app.level_name, "Ground", "the engine renamed the level and the window follows");
    assert!(app.undo(), "one undo step: {:?}", app.message);
    assert_ne!(app.level_name, "Ground");

    app.selection = vec![level];
    app.refresh();
    let number = app.fields.iter().position(|f| f.kind == Kind::Number).expect("a number to edit");
    app.fields[number].edit = TextEdit::new("lots");
    let before = app.revision;
    assert!(!app.commit_field(number));
    assert!(app.message.as_deref().is_some_and(|m| m.contains("not a number")), "{:?}", app.message);
    assert_eq!(app.revision, before, "nothing was asked of the engine");
    assert!(!app.commit_field(999), "a field that is not there asks nothing");
}

#[test]
fn a_saved_project_opens_again_framed_on_its_first_level() {
    let dir = std::env::temp_dir().join(format!("bimcraft-rune-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    let file = dir.join("house.bimc");
    let mut app = App::sample();
    assert!(!app.has_file());
    assert!(app.save_as(file.clone()), "refused: {:?}", app.message);
    assert!(app.has_file() && file.exists());
    assert!(app.save(), "a saved project saves to its own file: {:?}", app.message);
    let mut other = App::new(Session::new());
    let (new, house) = (other.session.model.elements.len(), app.session.model.elements.len());
    assert_ne!(new, house, "a new project is not the sample house (it has its two levels too, with the same ids)");
    assert!(other.open_path(file.clone()), "refused: {:?}", other.message);
    assert_eq!(other.session.model.elements.len(), house, "the file's elements, not the new project's");
    assert_eq!(other.levels, app.levels);
    assert!(other.drawing.is_some());
    assert!(!other.open_path(dir.join("missing.bimc")) && other.message.is_some());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_tool_is_picked_put_down_and_a_stale_id_picks_nothing() {
    let mut app = App::sample();
    app.set_tool("wall");
    assert_eq!(app.tool, Some("wall"));
    assert!(app.status().contains("Wall"), "the bar names the tool: {}", app.status());
    app.set_tool("door");
    assert_eq!(app.tool, Some("door"));
    app.set_tool("door");
    assert_eq!(app.tool, None, "a second press puts it down");
    app.set_tool("no such tool");
    assert_eq!(app.tool, None);
}

#[test]
fn a_refusal_is_kept_and_the_model_is_left_as_it_was() {
    let mut app = App::sample();
    let count = app.session.model.elements.len();
    app.selection = vec![1];
    assert!(!app.delete_selection());
    assert!(app.message.is_some(), "the status bar says why");
    assert_eq!(app.session.model.elements.len(), count);
}

#[test]
fn hostile_pointer_numbers_change_nothing() {
    let mut app = App::sample();
    let (camera, revision) = (app.camera, app.revision);
    app.hover(f32::NAN, 1.0);
    assert_eq!(app.cursor, None);
    app.drag(f32::INFINITY, 0.0);
    app.zoom(f32::NAN, 0.0, 2.0);
    app.zoom(0.0, 0.0, f32::NAN);
    app.resize(f32::NAN, 100.0);
    app.resize(0.0, 0.0);
    assert_eq!(app.camera, camera);
    assert_eq!(app.size, FIRST_SIZE);
    assert!(app.revision >= revision);
}
