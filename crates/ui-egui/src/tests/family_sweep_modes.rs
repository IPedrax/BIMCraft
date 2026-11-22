//! A Sweep and a Swept Blend are made a mode inside a mode. Create > Sweep opens Modify | Sweep,
//! which draws nothing itself: Sketch Path opens Modify | Sweep > Sketch Path, whose own Finish checks the path chains and
//! goes back, and whose own Cancel puts the path back as it was; Edit Profile opens Modify | Sweep > Edit Profile the same
//! way, asking Go To View (the views that see the profile's plane flat) where the view shown doesn't. The profile's plane is
//! marked where it crosses the path. Modify | Sweep's Finish makes the form, one step of the history.
use super::*;
use bimcraft_core::family::forms::FormKind;
use bimcraft_core::family::views::{Side, Which};
use egui::accesskit::Role;

const FAMILY: &str = r#"family {"name": "Box", "category": "generic", "types": {"T": {}}, "forms": [{"profile": [[2000, 2000], [2400, 2000], [2400, 2300], [2000, 2300]], "from": 0, "to": 300}]}"#;

fn open() -> Harness<'static, BimApp> {
    let mut h = app(FAMILY);
    h.set_size(vec2(1600.0, 1000.0));
    h.state_mut().edit_family("Box");
    for _ in 0..3 {
        h.run();
    }
    super::family_views::view(&mut h, Some(Which::Plan));
    h
}

fn editor<'a>(h: &'a Harness<'_, BimApp>) -> &'a crate::editor::FamilyEditor {
    h.state().active_family().expect("a family open")
}

fn button(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_role_and_label(Role::Button, label).click();
    h.run();
    h.run();
}

fn has_button(h: &Harness<BimApp>, label: &str) -> bool {
    h.query_by_role_and_label(Role::Button, label).is_some()
}

fn title(h: &Harness<BimApp>) -> &'static str {
    editor(h).sketch_title().unwrap_or("")
}

fn status(h: &Harness<BimApp>) -> (String, bool) {
    (h.state().status.text.clone(), h.state().status.error)
}

/// A click at point `q` of the plane being sketched on (its own axes), in the view shown.
fn click_plane(h: &mut Harness<BimApp>, q: P2) {
    let ed = editor(h);
    let p = ed.sketch_frame().expect("sketching on a plane").point(q, 0.0);
    let at = match ed.views.shown {
        Some(_) => ed.views.screen_of(p).expect("a view shown"),
        None => ed.drawn_3d().expect("{3D} drawn").screen(h.state().view_rect, p),
    };
    click_at(h, at);
    h.run();
}

/// An L: east 1000, then north 800, chained, with Line.
fn l_path(h: &mut Harness<BimApp>) {
    button(h, "Line");
    for q in [P2(0.0, 0.0), P2(1000.0, 0.0), P2(1000.0, 800.0)] {
        click_plane(h, q);
    }
    h.key_press(Key::Escape);
    h.run();
}

fn rectangle(h: &mut Harness<BimApp>, a: P2, b: P2) {
    button(h, "Rectangle");
    click_plane(h, a);
    click_plane(h, b);
}

/// The lines of the sketch's part `p`, on screen or waiting.
fn part_len(h: &Harness<BimApp>, p: crate::editor::sketch::Part) -> usize {
    editor(h).sketch.as_ref().map_or(0, |s| s.part_lines(p).len())
}

/// Go To View's Open View on the view called `name`.
fn go_to(h: &mut Harness<BimApp>, name: &str) {
    assert!(editor(h).views.go_to.is_some(), "Go To View asked");
    h.get_by_label(name).click();
    h.run();
    button(h, "Open View");
}

#[test]
fn a_sweep_is_its_path_finished_then_its_profile_finished_then_the_sweep() {
    use crate::editor::sketch::Part;
    let mut h = open();
    button(&mut h, "Sweep");
    // Modify | Sweep draws nothing itself: no Draw panel, a click in the view adds nothing.
    assert_eq!(title(&h), "Modify | Sweep");
    assert!(!has_button(&h, "Rectangle"), "no Draw panel at the sweep's own level");
    let at = editor(&h).views.screen_of([500.0, 500.0, 0.0]).unwrap();
    click_at(&mut h, at);
    assert_eq!(part_len(&h, Part::Main), 0);
    // Sketch Path: its own mode, the Draw panel, the path's own Finish.
    button(&mut h, "Sketch Path");
    assert_eq!(title(&h), "Modify | Sweep > Sketch Path");
    l_path(&mut h);
    button(&mut h, "Finish Edit Mode");
    assert_eq!(title(&h), "Modify | Sweep", "{:?}", status(&h));
    assert!(!status(&h).1, "finishing the path is no error: {:?}", status(&h));
    assert_eq!(part_len(&h, Part::Main), 2);
    assert_eq!(editor(&h).views.shown, Some(Which::Plan), "still in plan");
    // The profile's plane, marked where it crosses the path: the middle of its first line.
    let marks = editor(&h).profile_planes();
    assert_eq!(marks.len(), 1);
    assert!((marks[0].origin[0] - 500.0).abs() < 1e-6 && marks[0].origin[1].abs() < 1e-6, "{:?}", marks[0].origin);
    // Edit Profile from plan, which sees its plane edge-on: Go To View, the elevations that see it flat, no {3D}.
    button(&mut h, "Edit Profile");
    let g = editor(&h).views.go_to.clone().expect("Go To View asked");
    assert!(!g.views.is_empty() && g.views.iter().all(|w| matches!(w, Some(Which::Elevation(Side::Left | Side::Right)))), "{:?}", g.views);
    go_to(&mut h, "Elevation: Right");
    assert_eq!(editor(&h).views.shown, Some(Which::Elevation(Side::Right)));
    assert_eq!(title(&h), "Modify | Sweep > Edit Profile");
    rectangle(&mut h, P2(-100.0, 0.0), P2(100.0, 200.0));
    button(&mut h, "Finish Edit Mode");
    assert_eq!(title(&h), "Modify | Sweep", "{:?}", status(&h));
    // The sweep's own Finish: the form, one step of the history.
    let steps = editor(&h).steps();
    button(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{:?}", status(&h));
    let f = editor(&h).draft_family().unwrap();
    assert_eq!(f.forms.len(), 2);
    assert_eq!(f.forms[1].kind(), FormKind::Sweep);
    assert_eq!(editor(&h).steps(), steps + 1);
    // Built, with volume: its solid's box along the L and up the profile.
    let values = f.values("T", &std::collections::BTreeMap::new()).unwrap();
    let s = f.shapes(&values).into_iter().find(|s| s.index == 1).expect("the sweep builds");
    assert!(s.max[0] - s.min[0] > 900.0 && s.max[1] - s.min[1] > 700.0 && s.max[2] - s.min[2] > 150.0, "{:?} {:?}", s.min, s.max);
    // Selected, the Options Bar has no Depth: a sweep has no ends to measure.
    assert!(h.query_by_label("Depth").is_none(), "no Depth for a sweep");
    h.state_mut().family_undo();
    h.run();
    assert_eq!(editor(&h).draft_family().unwrap().forms.len(), 1, "one Undo takes the sweep back");
}

#[test]
fn a_nested_modes_cancel_and_undo_reach_its_own_lines_alone() {
    use crate::editor::sketch::Part;
    let mut h = open();
    button(&mut h, "Sweep");
    button(&mut h, "Sketch Path");
    l_path(&mut h);
    button(&mut h, "Finish Edit Mode");
    // Sketch Path again: a line more, then Cancel: the path as it was finished, the sweep still being made.
    button(&mut h, "Sketch Path");
    button(&mut h, "Line");
    click_plane(&mut h, P2(1000.0, 800.0));
    click_plane(&mut h, P2(0.0, 800.0));
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(part_len(&h, Part::Main), 3);
    button(&mut h, "Cancel Edit Mode");
    assert_eq!(title(&h), "Modify | Sweep");
    assert_eq!(part_len(&h, Part::Main), 2, "the path as it was finished");
    // Undo inside a mode takes back its own lines, never past where it began.
    button(&mut h, "Sketch Path");
    button(&mut h, "Line");
    click_plane(&mut h, P2(1000.0, 800.0));
    click_plane(&mut h, P2(0.0, 800.0));
    h.key_press(Key::Escape);
    h.run();
    for _ in 0..4 {
        h.state_mut().family_undo();
        h.run();
    }
    assert!(editor(&h).sketching());
    assert_eq!(title(&h), "Modify | Sweep > Sketch Path");
    assert_eq!(part_len(&h, Part::Main), 2, "back to where Sketch Path began, no further");
}

#[test]
fn a_path_that_doesnt_chain_is_refused_by_its_own_finish() {
    let mut h = open();
    button(&mut h, "Sweep");
    // Edit Profile before a path: refused, saying so.
    button(&mut h, "Edit Profile");
    assert!(status(&h).1 && status(&h).0.contains("path"), "{:?}", status(&h));
    assert_eq!(title(&h), "Modify | Sweep");
    button(&mut h, "Sketch Path");
    button(&mut h, "Line");
    click_plane(&mut h, P2(0.0, 0.0));
    click_plane(&mut h, P2(1000.0, 0.0));
    h.key_press(Key::Escape);
    h.run();
    click_plane(&mut h, P2(0.0, 500.0));
    click_plane(&mut h, P2(1000.0, 500.0));
    h.key_press(Key::Escape);
    h.run();
    button(&mut h, "Finish Edit Mode");
    assert_eq!(title(&h), "Modify | Sweep > Sketch Path", "still sketching the path");
    assert!(status(&h).1, "said why: {:?}", status(&h));
}

#[test]
fn the_sweeps_finish_with_no_profile_opens_the_profile_to_draw() {
    let mut h = open();
    button(&mut h, "Sweep");
    button(&mut h, "Sketch Path");
    l_path(&mut h);
    button(&mut h, "Finish Edit Mode");
    button(&mut h, "Finish Edit Mode");
    assert!(editor(&h).sketching());
    assert!(status(&h).1, "said why: {:?}", status(&h));
    // Not left in a plan that sees the profile's plane edge-on: Go To View asked for it.
    let g = editor(&h).views.go_to.clone().expect("Go To View asked");
    assert!(g.views.iter().all(|w| matches!(w, Some(Which::Elevation(_)))), "{:?}", g.views);
    go_to(&mut h, "Elevation: Right");
    assert_eq!(title(&h), "Modify | Sweep > Edit Profile");
}

#[test]
fn a_swept_blend_is_its_path_then_profile_1_then_profile_2_each_finished() {
    let mut h = open();
    button(&mut h, "Swept Blend");
    assert_eq!(title(&h), "Modify | Swept Blend");
    button(&mut h, "Sketch Path");
    button(&mut h, "Line");
    click_plane(&mut h, P2(0.0, 0.0));
    click_plane(&mut h, P2(0.0, 1000.0));
    h.key_press(Key::Escape);
    h.run();
    button(&mut h, "Finish Edit Mode");
    assert_eq!(title(&h), "Modify | Swept Blend");
    button(&mut h, "Profile 1");
    go_to(&mut h, "Elevation: Front");
    assert_eq!(title(&h), "Modify | Swept Blend > Edit Profile 1");
    rectangle(&mut h, P2(-100.0, 0.0), P2(100.0, 200.0));
    button(&mut h, "Finish Edit Mode");
    assert_eq!(title(&h), "Modify | Swept Blend");
    // Profile 2's plane faces the way Profile 1's does: the elevation shown sees it flat, no Go To View.
    button(&mut h, "Profile 2");
    assert!(editor(&h).views.go_to.is_none());
    assert_eq!(title(&h), "Modify | Swept Blend > Edit Profile 2");
    rectangle(&mut h, P2(-50.0, 0.0), P2(50.0, 100.0));
    button(&mut h, "Finish Edit Mode");
    button(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{:?}", status(&h));
    assert_eq!(editor(&h).draft_family().unwrap().forms[1].kind(), FormKind::SweptBlend);
}

#[test]
fn edit_sweep_opens_the_sweeps_own_mode_with_its_path_and_profile() {
    use crate::editor::sketch::Part;
    let script = r#"family {"name": "Box", "category": "generic", "types": {"T": {}}, "forms": [{"sketch": [{"start": [0, 0], "end": [1000, 0]}, {"start": [1000, 0], "end": [1000, 800]}], "sweep": {"profile": [{"start": [-100, 0], "end": [100, 0]}, {"start": [100, 0], "end": [100, 200]}, {"start": [100, 200], "end": [-100, 200]}, {"start": [-100, 200], "end": [-100, 0]}]}}]}"#;
    let mut h = app(script);
    h.set_size(vec2(1600.0, 1000.0));
    h.state_mut().edit_family("Box");
    for _ in 0..3 {
        h.run();
    }
    super::family_views::view(&mut h, Some(Which::Plan));
    h.state_mut().active_family_mut().unwrap().edit_form_sketch(0).unwrap();
    h.run();
    assert_eq!(title(&h), "Modify | Sweep");
    assert_eq!((part_len(&h, Part::Main), part_len(&h, Part::Profile)), (2, 4));
    button(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{:?}", status(&h));
}

/// Each step of making a sweep, from its own mode to Sketch Path and Edit Profile, saved where `BIMCRAFT_SWEEP_MODES_DUMP` says (only then).
#[test]
fn sweep_modes_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_SWEEP_MODES_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let mut h = open();
    let save = |h: &mut Harness<BimApp>, name: &str| {
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("sweep-modes-{name}.png"))).unwrap();
    };
    button(&mut h, "Sweep");
    save(&mut h, "1-sweep");
    button(&mut h, "Sketch Path");
    l_path(&mut h);
    save(&mut h, "2-path");
    button(&mut h, "Finish Edit Mode");
    save(&mut h, "3-path-finished");
    button(&mut h, "Edit Profile");
    save(&mut h, "4-go-to-view");
    go_to(&mut h, "Elevation: Right");
    rectangle(&mut h, P2(-100.0, 0.0), P2(100.0, 200.0));
    save(&mut h, "5-profile");
    button(&mut h, "Finish Edit Mode");
    save(&mut h, "6-profile-finished");
    button(&mut h, "Finish Edit Mode");
    super::family_views::view(&mut h, None);
    // {3D} keeps the frame it had; Zoom to Fit takes the sweep in too.
    h.state_mut().active_family_mut().unwrap().zoom_to_fit();
    for _ in 0..4 {
        h.run();
    }
    save(&mut h, "7-made");
}

/// Cancel in a nested mode puts back all it changed, a dimension made there too.
#[test]
fn a_nested_cancel_takes_back_a_dimension_made_in_it() {
    let mut h = open();
    button(&mut h, "Sweep");
    button(&mut h, "Sketch Path");
    l_path(&mut h);
    button(&mut h, "Finish Edit Mode");
    button(&mut h, "Edit Profile");
    go_to(&mut h, "Elevation: Right");
    rectangle(&mut h, P2(-100.0, 0.0), P2(100.0, 200.0));
    button(&mut h, "Finish Edit Mode");
    button(&mut h, "Edit Profile");
    // A dimension made while Edit Profile is open, as the Dimension tool makes one.
    let dim: bimcraft_core::family::FamilyDimension =
        serde_json::from_value(serde_json::json!({"refs": [{"plane": "11111111-1111-4111-8111-000000000001"}], "at": [0, 0, 0]})).unwrap();
    h.state_mut().active_family_mut().unwrap().sketch.as_mut().unwrap().dims.push(dim);
    h.run();
    button(&mut h, "Cancel Edit Mode");
    assert_eq!(title(&h), "Modify | Sweep");
    assert!(editor(&h).sketch.as_ref().unwrap().dims.is_empty(), "the dimension went with the mode");
}

/// Pick Path from a view that sees the work plane edge-on goes through Go To View and picks there.
#[test]
fn pick_path_through_go_to_view_picks() {
    // A sweep on Ref. Level, begun in plan, then the front elevation shown, which sees that plane edge-on.
    let mut h = open();
    button(&mut h, "Sweep");
    super::family_views::view(&mut h, Some(Which::Elevation(Side::Front)));
    button(&mut h, "Pick Path");
    assert!(editor(&h).views.go_to.is_some(), "Go To View asked");
    button(&mut h, "Open View");
    assert_eq!(title(&h), "Modify | Sweep > Sketch Path");
    assert_eq!(editor(&h).sketch.as_ref().unwrap().tool, Some(crate::draw::Shape::PickLines), "picking, not drawing");
}

/// Past the history's cap, Undo inside a nested mode still reaches as far back as the history holds of it.
#[test]
fn a_nested_undo_floor_follows_the_history_cap() {
    let mut h = open();
    button(&mut h, "Sweep");
    button(&mut h, "Sketch Path");
    l_path(&mut h);
    button(&mut h, "Finish Edit Mode");
    button(&mut h, "Sketch Path");
    let start = part_len(&h, crate::editor::sketch::Part::Main);
    {
        let s = h.state_mut().active_family_mut().unwrap().sketch.as_mut().unwrap();
        let line = s.sketch.lines[0].clone();
        for _ in 0..120 {
            s.sketch.lines.push(line.clone());
            s.track();
        }
    }
    while h.state_mut().active_family_mut().unwrap().sketch.as_mut().unwrap().undo() {}
    // 120 steps, 100 kept: back to 20 lines in, every step the history holds taken back.
    assert_eq!(part_len(&h, crate::editor::sketch::Part::Main), start + 20);
}

/// The sketch's tools wait at the sweep's own level, and the profile's plane can't be moved under the profile.
#[test]
fn the_sweeps_own_mode_takes_no_tool_and_a_profile_keeps_its_plane() {
    let mut h = open();
    button(&mut h, "Sweep");
    super::family_selection::command(&mut h, "Aligned Dimension");
    assert!(editor(&h).views.dimensioning.is_none(), "Aligned Dimension waits");
    assert!(status(&h).1, "said why: {:?}", status(&h));
    button(&mut h, "Sketch Path");
    assert!(editor(&h).views.dimensioning.is_none());
    l_path(&mut h);
    button(&mut h, "Finish Edit Mode");
    button(&mut h, "Edit Profile");
    go_to(&mut h, "Elevation: Right");
    h.state_mut().active_family_mut().unwrap().open_work_plane_dialog(crate::editor::work_plane::Target::Sketch);
    h.run();
    assert!(editor(&h).views.dialog.is_none(), "no Work Plane dialog for a profile");
}

/// Go To View with no view that sees the plane flat says so and offers {3D}.
#[test]
fn go_to_view_offers_3d_alone_when_no_view_sees_the_plane_flat() {
    let mut h = open();
    button(&mut h, "Sweep");
    button(&mut h, "Sketch Path");
    // A path at 30 degrees: its profile's plane is askew to every elevation.
    button(&mut h, "Line");
    click_plane(&mut h, P2(0.0, 0.0));
    click_plane(&mut h, P2(866.0, 500.0));
    h.key_press(Key::Escape);
    h.run();
    button(&mut h, "Finish Edit Mode");
    button(&mut h, "Edit Profile");
    let g = editor(&h).views.go_to.clone().expect("Go To View asked");
    assert_eq!(g.views, vec![None]);
    assert!(h.query_by_label_contains("No view shows").is_some(), "says no view shows it flat");
    button(&mut h, "Open View");
    assert_eq!(editor(&h).views.shown, None);
    assert_eq!(title(&h), "Modify | Sweep > Edit Profile");
}
