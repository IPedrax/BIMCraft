//! The window driven as a person drives it, headless: Rune's own `Runtime` on the app's real state and view, with a pointer, a
//! wheel and presses at window points, and the elements read back (`Runtime::elements`).

use rune::{Env, Key, KeyEvent, Mods, PhysicalInput, PointerButton, Runtime, WheelEvent};

use super::view;
use crate::app::App;

fn window() -> Runtime<App> {
    let mut rt = Runtime::new(App::sample(), view, (1280.0, 800.0), Env::new(true, 1.0));
    for _ in 0..3 {
        rt.frame(0.0);
    }
    rt
}

/// Where the plan canvas is in the window, points.
fn plan_origin(rt: &Runtime<App>) -> Option<(f32, f32)> {
    rt.presented("plan").map(|(rect, _)| (rect[0], rect[1]))
}

/// A point of the window over something the plan draws, found by looking over the canvas.
fn over_something(rt: &Runtime<App>) -> Option<(f32, f32)> {
    let (ox, oy) = plan_origin(rt)?;
    let (w, h) = rt.state.size;
    (0..(h as u32))
        .step_by(6)
        .flat_map(|y| (0..(w as u32)).step_by(6).map(move |x| (x as f32, y as f32)))
        .find_map(|(x, y)| rt.state.pick(x, y).map(|_| (ox + x, oy + y)))
}

fn tap(rt: &mut Runtime<App>, at: (f32, f32)) {
    rt.pointer_down(at.0, at.1);
    rt.pointer_up(at.0, at.1);
    for _ in 0..2 {
        rt.frame(0.0);
    }
}

fn text_of(rt: &Runtime<App>, key: &str) -> Option<String> {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).and_then(|e| e.text)
}

#[test]
fn the_plan_is_laid_out_and_the_core_draws_it_for_the_size_it_got() {
    let rt = window();
    let (_, w) = rt.presented("plan").map(|(r, _)| ((r[0], r[1]), r[2])).unwrap_or_default();
    assert!(w > 600.0 && w < 1280.0, "the panel takes its share: the plan is {w} wide");
    assert!((rt.state.size.0 - w).abs() < 1.0, "the app knows the size the layout gave: {:?}", rt.state.size);
    assert!(rt.state.drawing.as_ref().is_some_and(|d| !d.prims.is_empty()));
}

#[test]
fn a_pointer_over_the_plan_hovers_what_is_under_it_and_the_status_bar_says_so() {
    let mut rt = window();
    let at = over_something(&rt).expect("something is drawn");
    rt.pointer_move(at.0, at.1);
    rt.frame(0.0);
    assert!(rt.state.hovered.is_some(), "hover picks");
    let line = text_of(&rt, "status-line").unwrap_or_default();
    assert!(line.contains(rt.state.level_name.as_str()) && line.contains(" mm"), "the bar says where: {line}");
}

#[test]
fn a_tap_selects_and_the_panel_lists_its_properties_and_delete_goes_through_the_engine() {
    let mut rt = window();
    let at = over_something(&rt).expect("something is drawn");
    tap(&mut rt, at);
    let id = *rt.state.selection.first().expect("a tap on a drawn element selects it");
    assert!(!rt.state.props.is_empty());
    assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("rows")), "the panel draws the rows");

    let button = rt.find("delete").expect("the Delete button");
    tap(&mut rt, button);
    assert!(rt.state.session.model.elements.get(id).is_none(), "the engine deleted it");
    assert!(rt.state.selection.is_empty());

    let undo = rt.find("undo").expect("the Undo button");
    tap(&mut rt, undo);
    assert!(rt.state.session.model.elements.get(id).is_some(), "undo gave it back");
}

#[test]
fn the_wheel_zooms_about_the_pointer() {
    let mut rt = window();
    let (ox, oy) = plan_origin(&rt).expect("the plan");
    let (x, y) = (ox + 300.0, oy + 200.0);
    let before = rt.state.camera.scale;
    rt.wheel_event(WheelEvent::new(0.0, 200.0).at(x, y));
    rt.frame(0.0);
    assert!(rt.state.camera.scale > before, "wheel forward zooms in: {before} -> {}", rt.state.camera.scale);
}

#[test]
fn the_buttons_are_buttons_to_a_screen_reader() {
    let rt = window();
    let buttons: Vec<_> = rt.elements().into_iter().filter(|e| e.role == "button").filter_map(|e| e.key).collect();
    for key in ["delete", "undo", "redo"] {
        assert!(buttons.iter().any(|k| k == key), "{key} is a button: {buttons:?}");
    }
}

#[test]
fn a_tool_is_picked_from_the_palette_and_the_status_bar_says_so() {
    let mut rt = window();
    let wall = rt.find("tool-wall").expect("the Wall tool in the palette");
    tap(&mut rt, wall);
    assert_eq!(rt.state.tool, Some("wall"));
    assert!(text_of(&rt, "status-line").unwrap_or_default().contains("Tool: Wall"));
    assert!(
        rt.elements().iter().any(|e| e.key.as_deref() == Some("tool-wall") && e.role == "button"),
        "a tool is a button to a screen reader"
    );
}

fn rev() -> Runtime<App> {
    let mut app = App::sample();
    app.profile = crate::profile::Profile::Rev;
    let mut rt = Runtime::new(app, view, (1280.0, 800.0), Env::new(true, 1.0));
    for _ in 0..3 {
        rt.frame(0.0);
    }
    rt
}

#[test]
fn a_press_on_a_level_shows_its_plan() {
    let mut rt = window();
    let other = rt.state.levels.iter().map(|(id, _)| *id).find(|id| Some(*id) != rt.state.level).expect("a second level");
    let row = rt.find(&format!("level-{other}")).expect("its row in the browser");
    tap(&mut rt, row);
    assert_eq!(rt.state.level, Some(other));
    let name = rt.state.level_name.clone();
    assert!(text_of(&rt, "status-line").unwrap_or_default().starts_with(&name), "the status bar names the level shown");
}

#[test]
fn rev_has_a_ribbon_where_bimcraft_has_a_palette_and_both_draw_the_same_tools() {
    let bim = window();
    assert!(bim.find("tools").is_some() && bim.find("ribbon").is_none());
    let mut rt = rev();
    assert!(rt.find("ribbon").is_some() && rt.find("tools").is_none());
    let wall = rt.find("tool-wall").expect("the Wall tool in the ribbon");
    tap(&mut rt, wall);
    assert_eq!(rt.state.tool, Some("wall"), "a ribbon button picks the tool the palette's row picks");
    let tab = rt.find("tab-1").expect("the Structure tab");
    tap(&mut rt, tab);
    assert_eq!(rt.state.tab, 1);
    assert!(rt.find("tool-beam").is_some() && rt.find("tool-wall").is_none(), "the tab shows its own tools");
    assert!(rt.find("tool-move").is_some(), "and the Modify group stays");
    let (w, plan) = (rt.presented("plan").map_or(0.0, |(r, _)| r[2]), rt.presented("ribbon").map(|(r, _)| r[3]));
    assert!(w > 700.0 && plan.is_some_and(|h| h > 100.0), "the plan is laid out beside the side column, under the ribbon");
}

#[test]
fn open_and_save_ask_the_dialog_and_run_the_same_methods() {
    let dir = std::env::temp_dir().join(format!("bimcraft-rune-ui-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    let file = dir.join("house.bimc");
    let mut rt = window();

    let save = rt.find("save").expect("Save");
    tap(&mut rt, save);
    let asked = rt.file_dialog().map(|(kind, _)| kind);
    assert_eq!(asked, Some(rune::FileDialogKind::Save), "a project with no file asks where");
    rt.resolve_file_dialog(Some(file.clone())).expect("the dialog answers");
    rt.frame(0.0);
    assert!(file.exists(), "saved through the engine: {:?}", rt.state.message);
    assert_eq!(text_of(&rt, "project-name").as_deref(), Some("house.bimc"));

    tap(&mut rt, save);
    assert!(rt.file_dialog().is_none(), "a project with a file saves without asking");

    let open = rt.find("open").expect("Open");
    tap(&mut rt, open);
    assert_eq!(rt.file_dialog().map(|(kind, _)| kind), Some(rune::FileDialogKind::Open));
    rt.resolve_file_dialog(None).expect("a cancelled dialog");
    rt.frame(0.0);
    assert!(rt.state.message.is_none(), "cancelling is no error");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_properties_of_a_selected_level_have_fields_that_edit_through_the_engine() {
    let mut rt = window();
    let level = rt.state.level.expect("a level");
    rt.update(|s, _| s.select(vec![level]));
    for _ in 0..2 {
        rt.frame(0.0);
    }
    assert!(!rt.state.fields.is_empty());
    assert!(rt.elements().iter().any(|e| e.role == "textbox"), "a field is a text box to a screen reader");
}

/// Holds the modifier keys down (or lets them go, with `Mods::default()`) for the pointer events that follow.
fn hold(rt: &mut Runtime<App>, mods: Mods) {
    rt.physical_input(PhysicalInput::Modifiers(mods));
}

/// A point of the window and the element it picks.
type Spot = ((f32, f32), u64);

/// Two points of the window over different elements, with what each picks.
fn two_over(rt: &Runtime<App>) -> Option<(Spot, Spot)> {
    let (ox, oy) = plan_origin(rt)?;
    let (w, h) = rt.state.size;
    let mut first: Option<Spot> = None;
    for y in (0..(h as u32)).step_by(6) {
        for x in (0..(w as u32)).step_by(6) {
            let Some(id) = rt.state.pick(x as f32, y as f32) else { continue };
            let at = (ox + x as f32, oy + y as f32);
            match first {
                None => first = Some((at, id)),
                Some(a) if a.1 != id => return Some((a, (at, id))),
                Some(_) => {}
            }
        }
    }
    None
}

#[test]
fn a_drag_over_the_plan_draws_a_box_and_the_release_selects_what_it_holds_and_pans_nothing() {
    let mut rt = window();
    let (ox, oy) = plan_origin(&rt).expect("the plan");
    let (w, h) = rt.state.size;
    let camera = rt.state.camera;
    rt.pointer_down(ox + 4.0, oy + 4.0);
    rt.pointer_move(ox + w - 4.0, oy + h - 4.0);
    rt.frame(0.0);
    assert!(rt.state.rubber.is_some(), "the box shows while the button is held");
    rt.pointer_up(ox + w - 4.0, oy + h - 4.0);
    rt.frame(0.0);
    assert!(rt.state.rubber.is_none(), "and goes when it is let go");
    assert!(rt.state.selection.len() > 1, "a box over the whole plan takes what it holds: {:?}", rt.state.selection);
    assert_eq!(rt.state.camera, camera, "a drag with nothing in hand no longer pans");
}

#[test]
fn a_ctrl_click_adds_and_a_shift_click_takes_away() {
    let mut rt = window();
    let ((pa, a), (pb, b)) = two_over(&rt).expect("two things to click");
    tap(&mut rt, pa);
    assert_eq!(rt.state.selection, vec![a]);
    hold(&mut rt, Mods { cmd: true, ctrl: true, ..Mods::default() });
    tap(&mut rt, pb);
    assert_eq!(rt.state.selection, vec![a, b], "Ctrl adds");
    hold(&mut rt, Mods { shift: true, ..Mods::default() });
    tap(&mut rt, pa);
    assert_eq!(rt.state.selection, vec![b], "Shift takes away");
    hold(&mut rt, Mods::default());
    tap(&mut rt, pa);
    assert_eq!(rt.state.selection, vec![a], "a plain click replaces");
}

#[test]
fn the_middle_button_pans_the_plan() {
    let mut rt = window();
    let (ox, oy) = plan_origin(&rt).expect("the plan");
    let (x, y) = (ox + 300.0, oy + 200.0);
    let before = rt.state.camera;
    rt.button_down(x, y, PointerButton::Middle);
    rt.pointer_move(x + 40.0, y + 10.0);
    rt.button_up(x + 40.0, y + 10.0, PointerButton::Middle);
    rt.frame(0.0);
    let after = rt.state.camera;
    assert_eq!(after.scale, before.scale);
    assert!(after.cx != before.cx && after.cy != before.cy, "the plan followed the pointer: {before:?} -> {after:?}");
    assert!(rt.state.rubber.is_none() && rt.state.selection.is_empty(), "a middle drag picks and boxes nothing");
}

#[test]
fn tab_steps_through_what_is_under_the_pointer_and_escape_clears() {
    let mut rt = window();
    rt.on_key = Some(super::on_key);
    let (ox, oy) = plan_origin(&rt).expect("the plan");
    let (w, h) = rt.state.size;
    let d = rt.state.drawing.as_ref().expect("a drawing");
    let (x, y) = (0..(h as u32))
        .step_by(3)
        .flat_map(|y| (0..(w as u32)).step_by(3).map(move |x| (x as f32, y as f32)))
        .find(|&p| d.pick_all(p, 4.0).len() >= 2)
        .expect("elements lie over one another in the house");
    rt.pointer_move(ox + x, oy + y);
    rt.frame(0.0);
    let second = *rt.state.under.get(1).expect("two or more under the pointer");
    rt.key(KeyEvent { key: Key::Tab, mods: Mods::default() });
    rt.frame(0.0);
    assert_eq!(rt.state.hovered, Some(second), "Tab goes on to the next");
    tap(&mut rt, (ox + x, oy + y));
    assert_eq!(rt.state.selection, vec![second], "and a click takes that one");
    rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    rt.frame(0.0);
    assert!(rt.state.selection.is_empty(), "Escape clears the selection");
}

#[test]
fn the_zoom_buttons_zoom_fit_and_go_back_and_the_region_button_arms_a_box() {
    let mut rt = window();
    let scale = rt.state.camera.scale;
    let zoom_in = rt.find("zoom-in").expect("Zoom In");
    tap(&mut rt, zoom_in);
    assert!(rt.state.camera.scale > scale, "Zoom In is closer");
    let fit = rt.find("zoom-fit").expect("Zoom to Fit");
    tap(&mut rt, fit);
    assert_eq!(rt.state.camera.scale, scale, "Fit frames the plan as it was first shown");
    let back = rt.find("zoom-previous").expect("Previous Pan/Zoom");
    tap(&mut rt, back);
    assert!(rt.state.camera.scale > scale, "Back goes to how it was before Fit");

    let region = rt.find("zoom-region").expect("Zoom In Region");
    tap(&mut rt, region);
    assert!(rt.state.region);
    let (ox, oy) = plan_origin(&rt).expect("the plan");
    let before = rt.state.camera.scale;
    rt.pointer_down(ox + 100.0, oy + 100.0);
    rt.pointer_move(ox + 300.0, oy + 250.0);
    rt.frame(0.0);
    assert!(rt.state.rubber.is_some(), "the box shows while the button is held");
    rt.pointer_up(ox + 300.0, oy + 250.0);
    rt.frame(0.0);
    assert!(rt.state.camera.scale > before, "the plan zoomed to the box");
    assert!(!rt.state.region && rt.state.selection.is_empty(), "it is disarmed and nothing was selected");

    let buttons: Vec<_> = rt.elements().into_iter().filter(|e| e.role == "button").filter_map(|e| e.key).collect();
    for key in ["zoom-fit", "zoom-in", "zoom-out", "zoom-region", "zoom-previous"] {
        assert!(buttons.iter().any(|k| k == key), "{key} is a button to a screen reader: {buttons:?}");
    }
}

#[test]
fn a_wheel_moves_the_plan_without_drawing_it_and_the_poll_draws_it_when_the_camera_is_still() {
    let mut rt = window();
    rt.state.cam = crate::app::CameraMode::Live;
    rt.poll = Some(super::poll);
    let (ox, oy) = plan_origin(&rt).expect("the plan");
    let revision = rt.state.revision;
    rt.wheel_event(WheelEvent::new(0.0, 200.0).at(ox + 300.0, oy + 200.0));
    rt.frame(0.0);
    assert_eq!(rt.state.revision, revision, "no drawing by the core while the camera moves");
    assert!(rt.state.live_scale.get() > 1.0, "the plan on screen is scaled instead: {}", rt.state.live_scale.get());
    rt.frame(0.05);
    assert_eq!(rt.state.revision, revision, "and it waits for the camera to be still");
    rt.frame(0.2);
    assert!(rt.state.revision > revision, "then the core draws it exactly");
    assert_eq!(rt.state.live_scale.get(), 1.0);
}
