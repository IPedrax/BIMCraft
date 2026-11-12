//! F4: the family's views. Its Ref. Level floor and ceiling plans and its four elevations, opened from the Project Browser,
//! each the project's own drawing of the family in a scratch model: the reference planes dashed green with their names, the
//! forms seen and, in plan, cut. A click selects a form (its Properties) or a plane; a selected plane of the family's own moves
//! by a drag; Reference Plane draws one by two clicks (upright in plan, facing across an elevation); Work Plane Set and Show.
//! Preview Visibility and Temporary Hide/Isolate apply as in {3D}. Nothing of it reaches the project or the family but the
//! planes drawn and moved.
use super::preview_visibility::{CABINET, Pictures, toggle};
use super::*;
use bimcraft_core::family::views::{Side, Which};
use egui::Color32;

/// A box from Ref. Level to 2 m, east and behind the origin, crossing the plan's cut plane, and a plane of its own, Edge.
const BOX_FAMILY: &str = r#"family {"name": "Box", "category": "generic", "types": {"T": {}}, "forms": [{"profile": [[500, 300], [1000, 300], [1000, 600], [500, 600]], "to": 2000}], "ref_planes": [{"name": "Edge", "start": [-400, -2000, 0], "end": [-400, 2000, 0], "up": [0, 0, 1]}]}"#;

/// Two reference lines for [`BOX_FAMILY`]: one on Ref. Level, west and in front; one on Center (Front/Back), rising west of it.
const LINES: &str = r#""ref_lines": [{"start": [-1300, -1000, 0], "end": [-700, -1000, 0]}, {"start": [-1300, 0, 300], "end": [-700, 0, 1100], "host": {"plane": "9e3c0a10-1f0b-4d8c-8a5e-0000000000a2"}}]"#;

/// Family `name` (of `script`) open in its document, the views' pictures kept.
pub(super) fn open(script: &str, name: &str) -> (Harness<'static, BimApp>, Pictures) {
    let mut s = Session::new();
    s.script(script).unwrap();
    let pictures = Pictures::default();
    let mut h = Harness::builder()
        .with_size([1600.0, 1200.0])
        .with_max_steps(32)
        .renderer(pictures.clone())
        .build_ui_state(|ui, app: &mut BimApp| app.show(ui), BimApp::new(s));
    h.run();
    h.state_mut().edit_family(name);
    h.run();
    (h, pictures)
}

fn boxed() -> (Harness<'static, BimApp>, Pictures) {
    open(BOX_FAMILY, "Box")
}

/// The Project Browser's entry for `which` clicked: Ref. Level is there twice, the floor plan's first.
pub(super) fn view(h: &mut Harness<BimApp>, which: Option<Which>) {
    let (label, nth) = match which {
        None => ("{3D}", 0),
        Some(Which::Plan) => ("Ref. Level", 0),
        Some(Which::Ceiling) => ("Ref. Level", 1),
        Some(Which::Elevation(s)) => (s.name(), 0),
    };
    if let Some(n) = h.get_all_by_label(label).nth(nth) {
        n.scroll_to_me();
    }
    h.run();
    let at = h.get_all_by_label(label).nth(nth).map(|n| n.rect().center()).expect("the browser's entry");
    click_at(h, at);
    h.run();
}

pub(super) fn editor<'a>(h: &'a Harness<'_, BimApp>) -> &'a crate::editor::FamilyEditor {
    h.state().active_family().expect("a family open")
}

/// The view's pixels, as its drawing paints them over its picture (the drawing is painted as vectors).
fn picture(h: &Harness<BimApp>, _: &Pictures) -> egui::ColorImage {
    editor(h).view_pixels().expect("the view was drawn")
}

/// The picture's pixels within `r` of family point `p`.
pub(super) fn around(h: &Harness<BimApp>, pictures: &Pictures, p: [f64; 3], r: i32) -> Vec<Color32> {
    let img = picture(h, pictures);
    let (x, y) = editor(h).views.pixel_of(p).expect("a view shown");
    let mut out = Vec::new();
    for dx in -r..=r {
        for dy in -r..=r {
            let (px, py) = (x.round() as i32 + dx, y.round() as i32 + dy);
            if px >= 0 && py >= 0 && (px as usize) < img.size[0] && (py as usize) < img.size[1] {
                out.push(img.pixels[py as usize * img.size[0] + px as usize]);
            }
        }
    }
    out
}

fn green(c: &Color32) -> bool {
    i16::from(c.g()) - i16::from(c.r()) > 12 && i16::from(c.g()) - i16::from(c.b()) > 8
}

/// The darkest of `cs`: the sum of its channels.
pub(super) fn darkest(cs: &[Color32]) -> u32 {
    cs.iter().map(|c| u32::from(c.r()) + u32::from(c.g()) + u32::from(c.b())).min().unwrap_or(765)
}

/// Whether a dashed green trace runs through family point `p`, along `d` (mm), within 300 mm.
fn green_trace(h: &Harness<BimApp>, pictures: &Pictures, p: [f64; 3], d: [f64; 3]) -> bool {
    (0..12).any(|k| {
        let t = f64::from(k) * 25.0;
        around(h, pictures, [p[0] + d[0] * t, p[1] + d[1] * t, p[2] + d[2] * t], 2).iter().any(green)
    })
}

/// A click at family point `p` in the view shown.
pub(super) fn click_family(h: &mut Harness<BimApp>, p: [f64; 3]) {
    let at = editor(h).views.screen_of(p).expect("a view shown");
    click_at(h, at);
}

/// Form `n` (from 1) selected from the Project Browser's Forms list.
pub(super) fn select_form(h: &mut Harness<BimApp>, n: usize) {
    let label = format!("{n}. Extrusion");
    h.get_by_label(&label).scroll_to_me();
    h.run();
    press(h, &label);
}

fn plane<'a>(h: &'a Harness<'_, BimApp>, name: &str) -> &'a bimcraft_core::family::RefPlane {
    editor(h).planes.iter().find(|p| p.name == name).expect("the plane")
}

#[test]
fn each_browser_entry_opens_its_view_and_the_tab_names_it() {
    let (mut h, pictures) = boxed();
    assert_eq!(editor(&h).views.shown, None, "{{3D}} first");
    let mut seen = Vec::new();
    for (which, title) in [
        (Some(Which::Plan), "Floor Plan: Ref. Level"),
        (Some(Which::Ceiling), "Ceiling Plan: Ref. Level"),
        (Some(Which::Elevation(Side::Back)), "Elevation: Back"),
        (Some(Which::Elevation(Side::Front)), "Elevation: Front"),
        (Some(Which::Elevation(Side::Left)), "Elevation: Left"),
        (Some(Which::Elevation(Side::Right)), "Elevation: Right"),
    ] {
        view(&mut h, which);
        assert_eq!(editor(&h).views.shown, which);
        assert!(h.query_all_by_label(&format!("Box - {title}")).count() > 0, "the tab names {title}");
        let p = picture(&h, &pictures);
        assert!(!seen.contains(&p.pixels), "{title} is a picture of its own");
        seen.push(p.pixels);
    }
    view(&mut h, None);
    assert_eq!(editor(&h).views.shown, None);
    assert!(h.query_all_by_label("Box - {3D}").count() > 0);
}

#[test]
fn the_plan_draws_the_planes_dashed_green_and_the_form_cut() {
    let (mut h, pictures) = boxed();
    view(&mut h, Some(Which::Plan));
    assert!(green_trace(&h, &pictures, [0.0, 800.0, 0.0], [0.0, 1.0, 0.0]), "Center (Left/Right)");
    assert!(green_trace(&h, &pictures, [800.0, 0.0, 0.0], [1.0, 0.0, 0.0]), "Center (Front/Back)");
    assert!(green_trace(&h, &pictures, [-400.0, 800.0, 0.0], [0.0, 1.0, 0.0]), "Edge");
    // The box crosses the cut plane: its edge in the cut pen, dark.
    assert!(darkest(&around(&h, &pictures, [750.0, 300.0, 0.0], 2)) < 150, "the box, cut");
    // The Front elevation: the planes upright across it, the box seen, Ref. Level a level.
    view(&mut h, Some(Which::Elevation(Side::Front)));
    assert!(green_trace(&h, &pictures, [-400.0, 0.0, 600.0], [0.0, 0.0, 1.0]), "Edge, across Front");
    assert!(darkest(&around(&h, &pictures, [1000.0, 0.0, 1000.0], 2)) < 300, "the box's right side");
}

#[test]
fn a_reference_plane_drawn_in_plan_stands_upright_and_loads_with_nothing_else_changed() {
    let (mut h, _) = boxed();
    let project = h.state().session.model.to_json().unwrap();
    let family = serde_json::to_value(&h.state().session.model.families["Box"]).unwrap();
    view(&mut h, Some(Which::Plan));
    press(&mut h, "Reference Plane");
    assert!(editor(&h).views.drawing.is_some(), "the tool is on");
    click_family(&mut h, [-1000.0, -800.0, 0.0]);
    click_family(&mut h, [1000.0, -800.0, 0.0]);
    let planes = &editor(&h).planes;
    let new = planes.last().unwrap();
    assert_eq!(planes.len(), 5, "the datums, Edge and the new one");
    assert!((new.start[1] + 800.0).abs() < 20.0 && (new.end[1] + 800.0).abs() < 20.0, "{new:?}");
    assert_eq!(new.up, [0.0, 0.0, 1.0], "upright");
    assert_eq!(editor(&h).selected_plane(), Some(new.guid), "selected once drawn");
    // Views opened and a plane drawn: the project is as it was until the family is loaded.
    assert_eq!(h.state().session.model.to_json().unwrap(), project, "the project untouched by the views");
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    press(&mut h, "Load into Project");
    if h.query_by_label("Overwrite the existing version").is_some() {
        press(&mut h, "Overwrite the existing version");
    }
    let after = serde_json::to_value(&h.state().session.model.families["Box"]).unwrap();
    let planes = after["planes"].as_array().unwrap();
    assert_eq!(planes.len(), 5, "the new plane in Family.planes: {planes:?}");
    let mut rest = after.clone();
    let mut was = family.clone();
    rest.as_object_mut().unwrap().remove("planes");
    was.as_object_mut().unwrap().remove("planes");
    assert_eq!(rest, was, "the family otherwise unchanged");
    assert_eq!(&after["planes"].as_array().unwrap()[..4], &family["planes"].as_array().unwrap()[..], "its planes as they were");
    // No reference plane of the scratch models reached the project.
    assert!(h.state().session.model.elements.values().all(|e| !matches!(e, Element::RefPlane(_))));
}

#[test]
fn a_reference_plane_drawn_in_each_elevation_faces_across_it() {
    let (mut h, _) = boxed();
    for side in Side::ALL {
        view(&mut h, Some(Which::Elevation(side)));
        shortcut(&mut h, "RP");
        assert!(editor(&h).views.drawing.is_some(), "{side:?}: RP");
        let (a, b) = match side {
            Side::Front | Side::Back => ([-600.0, 0.0, 200.0], [600.0, 0.0, 1400.0]),
            Side::Left | Side::Right => ([0.0, -600.0, 200.0], [0.0, 600.0, 1400.0]),
        };
        click_family(&mut h, a);
        click_family(&mut h, b);
        let p = editor(&h).planes.last().unwrap().clone();
        let d = [p.end[0] - p.start[0], p.end[1] - p.start[1], p.end[2] - p.start[2]];
        let n = [d[1] * p.up[2] - d[2] * p.up[1], d[2] * p.up[0] - d[0] * p.up[2], d[0] * p.up[1] - d[1] * p.up[0]];
        let l = side.looking();
        assert!((n[0] * l.0 + n[1] * l.1).abs() < 1e-6, "{side:?}: its normal in the elevation's plane: {p:?}");
        assert!((p.start[2] - 200.0).abs() < 20.0 && (p.end[2] - 1400.0).abs() < 20.0, "{side:?}: {p:?}");
        key(&mut h, Key::Escape);
        key(&mut h, Key::Escape);
    }
    assert_eq!(editor(&h).planes.len(), 8);
    // Undone, one plane a step.
    press(&mut h, "Undo");
    assert_eq!(editor(&h).planes.len(), 7);
}

#[test]
fn a_click_selects_a_form_or_a_plane_and_a_selected_plane_moves_by_a_drag() {
    let (mut h, _) = boxed();
    view(&mut h, Some(Which::Elevation(Side::Front)));
    click_family(&mut h, [750.0, 0.0, 1000.0]);
    assert_eq!(editor(&h).selected_form(), Some(0), "the box");
    assert!(h.query_all_by_label("Modify | Extrusion").count() > 0, "its contextual tab, as in {{3D}}");
    view(&mut h, Some(Which::Plan));
    assert_eq!(editor(&h).selected_form(), Some(0), "a view opened keeps the selection");
    let edge = plane(&h, "Edge").guid;
    click_family(&mut h, [-400.0, -1000.0, 0.0]);
    assert_eq!(editor(&h).selected_plane(), Some(edge), "Edge");
    assert_eq!(editor(&h).selected_form(), None);
    assert!(h.query_all_by_label("Modify | Reference Planes").count() > 0);
    // Dragged 300 east: it moves square to itself, Center planes and all else staying.
    let (a, b) = (editor(&h).views.screen_of([-400.0, 0.0, 0.0]).unwrap(), editor(&h).views.screen_of([-100.0, 200.0, 0.0]).unwrap());
    drag_screen(&mut h, a, b, PointerButton::Primary);
    let p = plane(&h, "Edge");
    assert!((p.start[0] + 100.0).abs() < 15.0 && (p.end[0] + 100.0).abs() < 15.0, "moved east, square to itself: {p:?}");
    assert_eq!((p.start[1], p.end[1]), (-2000.0, 2000.0), "not along itself");
    // A datum is selected, never moved.
    click_family(&mut h, [0.0, -1000.0, 0.0]);
    let center = editor(&h).selected_plane().unwrap();
    assert_eq!(center, bimcraft_core::family::CENTER_LEFT_RIGHT);
    let (a, b) = (editor(&h).views.screen_of([0.0, 0.0, 0.0]).unwrap(), editor(&h).views.screen_of([300.0, 0.0, 0.0]).unwrap());
    drag_screen(&mut h, a, b, PointerButton::Primary);
    assert!(editor(&h).planes.iter().all(|p| !p.datum() || p.as_datum()), "the datums as they are");
}

/// A window with a Sill plane on Ref. Level, Align-locked to it, and Head 1200 above: as the blank window has Sill (F10c).
const SILL_WINDOW: &str = r#"family {"name": "Sill Window", "category": "window", "parameters": [{"name": "Width", "value": 900}, {"name": "Height", "value": 1200}], "types": {"T": {}}, "ref_planes": [{"guid": "f1000000-0000-4000-8000-000000000003", "name": "Head", "start": [-1500, 0, 1200], "end": [1500, 0, 1200], "up": [0, 1, 0]}, {"guid": "f1000000-0000-4000-8000-000000000004", "name": "Sill", "start": [-1500, 0, 0], "end": [1500, 0, 0], "up": [0, 1, 0]}], "dimensions": [{"refs": [{"plane": "9e3c0a10-1f0b-4d8c-8a5e-0000000000a1"}, {"plane": "f1000000-0000-4000-8000-000000000004"}], "view": "front", "at": [1300, 0, 0], "locked": [{"segment": 0, "length": 0}], "alignment": true}], "forms": [{"plane": "xz", "profile": [[-450, 0], [450, 0], [450, 1200], [-450, 1200]], "from": -40, "to": 40}]}"#;

/// The pointer over family point `p` in the view shown.
fn hover_family(h: &mut Harness<BimApp>, p: [f64; 3]) {
    let at = editor(h).views.screen_of(p).expect("a view shown");
    h.hover_at(at);
    h.run();
}

/// F10c: a plane on another (Sill on Ref. Level) is drawn, and picked: a click takes the one Tab has come to, lit, Tab stepping
/// through those under the pointer as Revit's does; so a dimension measures from it. The blank window has one, locked.
#[test]
fn a_sill_plane_on_ref_level_is_picked_with_tab_and_dimensioned_to() {
    let (mut h, _) = open(SILL_WINDOW, "Sill Window");
    view(&mut h, Some(Which::Elevation(Side::Front)));
    let sill = plane(&h, "Sill").guid;
    let head = plane(&h, "Head").guid;
    let level = bimcraft_core::family::REF_LEVEL;
    // Both drawn there: the pointer over them finds both.
    hover_family(&mut h, [-1300.0, 0.0, 0.0]);
    let mut under = editor(&h).views.under.clone();
    under.sort();
    let mut both = vec![sill, level];
    both.sort();
    assert_eq!(under, both, "Sill and Ref. Level, one on the other");
    // A click takes the first; Tab, the next; Tab again, back to the first.
    click_family(&mut h, [-1300.0, 0.0, 0.0]);
    let first = editor(&h).selected_plane().unwrap();
    key(&mut h, Key::Tab);
    click_family(&mut h, [-1300.0, 0.0, 0.0]);
    let second = editor(&h).selected_plane().unwrap();
    assert_ne!(first, second, "Tab took the other");
    key(&mut h, Key::Tab);
    click_family(&mut h, [-1300.0, 0.0, 0.0]);
    assert_eq!(editor(&h).selected_plane(), Some(first), "and round again");
    key(&mut h, Key::Escape);
    // Aligned Dimension from Sill (Tabbed to, where it isn't first) to Head.
    h.get_by_role_and_label(Role::Button, "Aligned Dimension").click();
    h.run();
    hover_family(&mut h, [-1300.0, 0.0, 0.0]);
    while editor(&h).views.under.get(editor(&h).views.cycle) != Some(&sill) {
        key(&mut h, Key::Tab);
    }
    click_family(&mut h, [-1300.0, 0.0, 0.0]);
    click_family(&mut h, [-1300.0, 0.0, 1200.0]);
    click_family(&mut h, [-1700.0, 0.0, 600.0]);
    let d = editor(&h).dimensions.last().unwrap().clone();
    assert_eq!(d.refs, vec![bimcraft_core::family::DimRef::Plane(sill), bimcraft_core::family::DimRef::Plane(head)]);
    // The blank window ships with Sill, Align-locked to Ref. Level, and builds.
    let blank = crate::editor::FamilyEditor::blank(bimcraft_core::family::Category::Window).draft_family().unwrap();
    let sill = blank.planes.iter().find(|p| p.name == "Sill").expect("the blank's Sill");
    assert!(sill.start[2] == 0.0 && sill.end[2] == 0.0, "on Ref. Level");
    let lock = blank.dimensions.iter().find(|d| d.alignment).expect("its lock");
    assert_eq!(lock.refs, vec![bimcraft_core::family::DimRef::Plane(level), bimcraft_core::family::DimRef::Plane(sill.guid)]);
    assert_eq!(blank.check(&|_| true), Ok(()));
}

#[test]
fn work_plane_set_picks_a_plane_and_show_tints_it() {
    let (mut h, _) = boxed();
    view(&mut h, Some(Which::Elevation(Side::Front)));
    assert_eq!(editor(&h).views.work_plane(), bimcraft_core::family::PlaneRef::Plane(bimcraft_core::family::CENTER_FRONT_BACK));
    let edge = plane(&h, "Edge").guid;
    // Set: the Work Plane dialog (T12); Pick a plane, OK, then Edge's trace. Seen edge-on here, so Go To View: Left.
    super::family_work_plane::set(&mut h);
    pick(&mut h, Role::RadioButton, "Pick a plane");
    press(&mut h, "OK");
    assert!(editor(&h).views.picking.is_some());
    click_family(&mut h, [-400.0, 0.0, 900.0]);
    assert!(editor(&h).views.picking.is_none());
    assert!(editor(&h).views.go_to.is_some(), "Edge is upright across Front: Go To View");
    press(&mut h, "Open View");
    assert_eq!(editor(&h).views.shown, Some(Which::Elevation(Side::Left)));
    assert_eq!(editor(&h).views.work_plane(), bimcraft_core::family::PlaneRef::Plane(edge), "picked, set in Left");
    press(&mut h, "Show");
    assert!(editor(&h).views.show_work_plane);
    // Each view keeps its own: the plan's is still Ref. Level.
    view(&mut h, Some(Which::Plan));
    assert_eq!(editor(&h).views.work_plane(), bimcraft_core::family::PlaneRef::Plane(bimcraft_core::family::REF_LEVEL));
}

/// Where family point `p` is on screen in {3D}.
fn in_3d(h: &Harness<BimApp>, p: [f64; 3]) -> egui::Pos2 {
    editor(h).drawn_3d().expect("{3D} drawn").screen(h.state().view_rect, p)
}

/// The {3D} preview's last picture's colour at family point `p`.
fn preview_at(h: &Harness<BimApp>, pictures: &Pictures, p: [f64; 3]) -> Color32 {
    let img = super::preview_visibility::preview(h, pictures);
    let (x, y, _) = editor(h).drawn_3d().expect("{3D} drawn").cam.project(p);
    img.pixels[y as usize * img.size[0] + x as usize]
}

/// A point of Ref. Level in front of the box, seen; and one on the box's front face, in front of Ref. Level.
const ON_PLANE: [f64; 3] = [300.0, 100.0, 0.0];
const ON_BOX: [f64; 3] = [750.0, 300.0, 1500.0];

#[test]
fn work_plane_show_draws_the_work_plane_into_3d_hidden_by_the_forms_in_front_of_it() {
    let (mut h, pictures) = boxed();
    assert_eq!(editor(&h).views.shown, None, "{{3D}}");
    let (plane, front) = (preview_at(&h, &pictures, ON_PLANE), preview_at(&h, &pictures, ON_BOX));
    let low = [750.0, 300.0, 300.0];
    let box_low = preview_at(&h, &pictures, low);
    press(&mut h, "Show");
    h.run();
    assert!(editor(&h).views.show_work_plane);
    assert_eq!(editor(&h).views.work_plane(), bimcraft_core::family::PlaneRef::Plane(bimcraft_core::family::REF_LEVEL), "{{3D}}'s own");
    assert_ne!(preview_at(&h, &pictures, ON_PLANE), plane, "Ref. Level tinted where it is seen");
    assert_eq!(preview_at(&h, &pictures, ON_BOX), front, "the box in front of it keeps its colours");
    // Set to Center (Front/Back), upright through the origin, in front of the box: the box behind it is tinted.
    if let Some(ed) = h.state_mut().active_family_mut() {
        ed.views.set_work_plane(bimcraft_core::family::PlaneRef::Plane(bimcraft_core::family::CENTER_FRONT_BACK));
    }
    h.run();
    assert_ne!(preview_at(&h, &pictures, low), box_low, "the box seen through it");
    // The plans keep theirs.
    view(&mut h, Some(Which::Plan));
    assert_eq!(editor(&h).views.work_plane(), bimcraft_core::family::PlaneRef::Plane(bimcraft_core::family::REF_LEVEL));
    press(&mut h, "Show");
    view(&mut h, None);
    assert_eq!(preview_at(&h, &pictures, ON_PLANE), plane, "off");
}

/// F2/T18's reference lines in the plan and the Front elevation: solid green.
#[test]
fn a_reference_line_shows_in_plan_and_in_an_elevation() {
    let script = BOX_FAMILY.replace(r#""ref_planes""#, &format!(r#"{LINES}, "ref_planes""#));
    let (mut h, pictures) = open(&script, "Box");
    view(&mut h, Some(Which::Plan));
    for x in [-1200.0, -1000.0, -800.0] {
        assert!(around(&h, &pictures, [x, -1000.0, 0.0], 2).iter().any(green), "the level one in plan at {x}");
    }
    assert!(!around(&h, &pictures, [-1000.0, -1300.0, 0.0], 2).iter().any(green), "nothing beside it");
    // In Front, the one on Center (Front/Back), rising across it.
    view(&mut h, Some(Which::Elevation(Side::Front)));
    for (x, z) in [(-1150.0, 500.0), (-1000.0, 700.0), (-850.0, 900.0)] {
        assert!(around(&h, &pictures, [x, 0.0, z], 2).iter().any(green), "the rising one in Front at {x}, {z}");
    }
    assert!(!around(&h, &pictures, [-1000.0, 0.0, 1100.0], 2).iter().any(green), "nothing above it");
}

#[test]
fn a_form_the_type_hides_is_halftone_until_preview_visibility_and_a_form_hidden_for_now_is_gone() {
    let (mut h, pictures) = open(&format!("{BOX}\n{CABINET}"), "Cabinet");
    // Type B hides the second form (2000 to 3000 east).
    if let Some(ed) = h.state_mut().active_family_mut() {
        ed.preview_type("B");
    }
    view(&mut h, Some(Which::Plan));
    let second = [2500.0, 500.0, 0.0];
    let first = [750.0, 500.0, 0.0];
    let ink = darkest(&around(&h, &pictures, first, 2));
    let ghost = darkest(&around(&h, &pictures, second, 2));
    assert!(ink < 200, "the first form, in ink: {ink}");
    assert!(ghost > ink + 100 && ghost < 700, "the hidden one, halftone: {ghost} against {ink}");
    toggle(&mut h);
    assert!(darkest(&around(&h, &pictures, second, 2)) > 740, "Preview Visibility: as placed, without it");
    assert!(darkest(&around(&h, &pictures, first, 2)) < 200);
    // Hidden for now, the first is gone with Preview Visibility on and off; the ghost hidden for now too.
    select_form(&mut h, 1);
    assert_eq!(editor(&h).selected_form(), Some(0));
    shortcut(&mut h, "HH");
    assert!(editor(&h).temporary().hides(), "HH hides the first form");
    assert!(darkest(&around(&h, &pictures, first, 2)) > 740, "hidden for now, Preview Visibility on");
    toggle(&mut h);
    assert!(darkest(&around(&h, &pictures, first, 2)) > 740, "hidden for now, Preview Visibility off");
    assert!(darkest(&around(&h, &pictures, second, 2)) < 700, "the ghost still shows");
    select_form(&mut h, 2);
    shortcut(&mut h, "HH");
    assert!(darkest(&around(&h, &pictures, second, 2)) > 740, "the ghost hidden for now");
    toggle(&mut h);
    assert!(darkest(&around(&h, &pictures, second, 2)) > 740, "and with Preview Visibility on");
    shortcut(&mut h, "HR");
    assert!(darkest(&around(&h, &pictures, first, 2)) < 200, "reset");
}

#[test]
fn a_plane_hidden_for_now_goes_from_every_view_of_the_document() {
    let (mut h, pictures) = boxed();
    view(&mut h, Some(Which::Plan));
    click_family(&mut h, [-400.0, -1000.0, 0.0]);
    shortcut(&mut h, "HH");
    assert!(!green_trace(&h, &pictures, [-400.0, 800.0, 0.0], [0.0, 1.0, 0.0]), "Edge hidden in plan");
    assert!(green_trace(&h, &pictures, [0.0, 800.0, 0.0], [0.0, 1.0, 0.0]), "the Center plane stays");
    // One state for the document: hidden in Front too.
    view(&mut h, Some(Which::Elevation(Side::Front)));
    assert!(!green_trace(&h, &pictures, [-400.0, 0.0, 600.0], [0.0, 0.0, 1.0]), "Edge hidden in Front");
    shortcut(&mut h, "HR");
    assert!(green_trace(&h, &pictures, [-400.0, 0.0, 600.0], [0.0, 0.0, 1.0]), "back");
}

#[test]
fn an_adaptive_familys_plans_and_elevations_only_show_it() {
    let (mut h, ..) = super::family_temporary_hide::adaptive();
    view(&mut h, Some(Which::Plan));
    assert_eq!(editor(&h).views.shown, Some(Which::Plan));
    press(&mut h, "Reference Plane");
    assert!(editor(&h).views.drawing.is_none(), "no Reference Plane in an adaptive family's views");
    shortcut(&mut h, "RP");
    assert!(editor(&h).views.drawing.is_none());
    let before = editor(&h).planes.len();
    click_family(&mut h, [1000.0, 750.0, 0.0]);
    assert_eq!(editor(&h).selected_form(), None);
    assert_eq!(editor(&h).planes.len(), before);
    // Its modelling view is {3D}'s, as it was.
    view(&mut h, None);
    assert!(editor(&h).adaptive.is_some() && editor(&h).views.shown.is_none());
}

/// The family's floor plan and Front elevation as the window shows them, with two named planes close together and a reference
/// line, and {3D} with Work Plane > Show on, saved where `BIMCRAFT_F4_DUMP` says (only then).
#[test]
fn family_views_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_F4_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let script = BOX_FAMILY.replace(
        r#""ref_planes": ["#,
        &format!(r#"{LINES}, "ref_planes": [{{"name": "Near", "start": [-300, -2000, 0], "end": [-300, 2000, 0], "up": [0, 0, 1]}}, "#),
    );
    let mut h = app(&script);
    h.set_size(vec2(1600.0, 1000.0));
    h.state_mut().edit_family("Box");
    h.run();
    for (name, which) in [("floor-plan", Some(Which::Plan)), ("front-elevation", Some(Which::Elevation(Side::Front)))] {
        view(&mut h, which);
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("f4-{name}.png"))).unwrap();
    }
    view(&mut h, None);
    press(&mut h, "Show");
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("f4-3d-work-plane.png")).unwrap();
}

/// On a real card (egui's renderer): {3D}'s pixels change with Work Plane > Show where Ref. Level is seen, and not on the box
/// standing in front of it.
mod gpu {
    use super::*;

    #[test]
    fn work_plane_show_changes_the_3d_views_pixels_where_the_plane_is_and_not_on_a_form_in_front_of_it() {
        let mut h = app(BOX_FAMILY);
        h.set_size(vec2(1600.0, 1000.0));
        h.state_mut().edit_family("Box");
        h.run();
        let Ok(before) = h.render() else { return };
        let (on, front) = (in_3d(&h, ON_PLANE), in_3d(&h, ON_BOX));
        let pane = h.state().view_rect;
        assert!(pane.contains(on) && pane.contains(front), "{on:?} {front:?} in {pane:?}");
        press(&mut h, "Show");
        h.run();
        let after = h.render().unwrap();
        let ppp = h.ctx.pixels_per_point();
        let at = |img: &image::RgbaImage, q: egui::Pos2| img.get_pixel((q.x * ppp) as u32, (q.y * ppp) as u32).0;
        assert_ne!(at(&before, on), at(&after, on), "tinted where Ref. Level is");
        assert_eq!(at(&before, front), at(&after, front), "the box in front of it keeps its colour");
    }
}

/// A family's connectors are drawn in {3D} and in its plans (#269), each at its place: the pointer over one lights it, a click
/// selects it (Properties show it), as a form is; in a plan too.
#[test]
fn a_connector_is_lit_under_the_pointer_and_selected_by_a_click_in_3d_and_in_a_plan() {
    let (mut h, _) = open("", "WC");
    let marks = editor(&h).connector_marks();
    assert_eq!(marks.len(), 2, "the WC's waste and cold water");
    let (waste, cold) = (marks[0].at, marks[1].at);
    // {3D}: over the waste outlet, it is lit; a click selects it.
    h.hover_at(in_3d(&h, waste));
    h.run();
    assert_eq!(editor(&h).connector_hover, Some(0));
    let at = in_3d(&h, waste);
    click_at(&mut h, at);
    assert_eq!(editor(&h).selected_connector(), Some(0));
    assert!(h.query_all_by_label("Modify | Connectors").count() > 0, "its contextual tab");
    // A click on the cold water's selects that one instead.
    let at = in_3d(&h, cold);
    click_at(&mut h, at);
    assert_eq!(editor(&h).selected_connector(), Some(1));
    // The plan: the same.
    key(&mut h, Key::Escape);
    view(&mut h, Some(Which::Plan));
    hover_family(&mut h, cold);
    assert_eq!(editor(&h).connector_hover, Some(1));
    click_family(&mut h, cold);
    assert_eq!(editor(&h).selected_connector(), Some(1));
    // Off it, a click on the bowl selects a form, and lets the connector go.
    click_family(&mut h, [0.0, -150.0, 0.0]);
    assert_eq!(editor(&h).selected_connector(), None);
}

/// Every point of the lines the last frame painted, on screen.
fn painted_points(h: &Harness<BimApp>) -> Vec<egui::Pos2> {
    fn walk(s: &egui::Shape, out: &mut Vec<egui::Pos2>) {
        match s {
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            egui::Shape::LineSegment { points, .. } => out.extend(points),
            egui::Shape::Path(p) => out.extend(&p.points),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &h.output().shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

#[test]
fn a_family_plan_is_painted_as_vectors_as_a_project_plan_is_with_no_picture_uploaded() {
    let (mut h, pictures) = open(BOX_FAMILY, "Box");
    view(&mut h, Some(Which::Plan));
    let named = |id: &egui::TextureId| h.ctx.tex_manager().read().meta(*id).is_some_and(|m| m.name == "family-view");
    assert!(!pictures.0.lock().unwrap().iter().any(|(id, _)| named(id)), "a plan is no picture");
    // The box's corner, where its outline's lines meet.
    let corner = editor(&h).views.screen_of([500.0, 300.0, 0.0]).expect("a view shown");
    assert!(painted_points(&h).iter().any(|p| p.distance(corner) < 1.5), "the box's outline painted at {corner:?}");
    // An elevation's hidden-line picture goes under its lines.
    view(&mut h, Some(Which::Elevation(Side::Front)));
    let named = |id: &egui::TextureId| h.ctx.tex_manager().read().meta(*id).is_some_and(|m| m.name == "family-view");
    assert!(pictures.0.lock().unwrap().iter().any(|(id, _)| named(id)), "the elevation's picture");
    // Its datums, drawn over it as lines: Ref. Level's line through the origin.
    let origin = editor(&h).views.screen_of([0.0, 0.0, 0.0]).expect("a view shown");
    assert!(painted_points(&h).iter().any(|p| (p.y - origin.y).abs() < 1.5), "Ref. Level painted at {origin:?}");
}
