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

/// Family `name` (of `script`) open in its document, the views' pictures kept.
fn open(script: &str, name: &str) -> (Harness<'static, BimApp>, Pictures) {
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
fn view(h: &mut Harness<BimApp>, which: Option<Which>) {
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

fn editor<'a>(h: &'a Harness<'_, BimApp>) -> &'a crate::editor::FamilyEditor {
    h.state().family_editor.as_ref().expect("a family open")
}

/// The view's last picture.
fn picture(h: &Harness<BimApp>, pictures: &Pictures) -> egui::ColorImage {
    let named = |id: &egui::TextureId| h.ctx.tex_manager().read().meta(*id).is_some_and(|m| m.name == "family-view");
    let kept = pictures.0.lock().unwrap();
    kept.iter().rev().find(|(id, _)| named(id)).map(|(_, p)| p.clone()).expect("the view was drawn")
}

/// The picture's pixels within `r` of family point `p`.
fn around(h: &Harness<BimApp>, pictures: &Pictures, p: [f64; 3], r: i32) -> Vec<Color32> {
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
fn darkest(cs: &[Color32]) -> u32 {
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
fn click_family(h: &mut Harness<BimApp>, p: [f64; 3]) {
    let at = editor(h).views.screen_of(p).expect("a view shown");
    click_at(h, at);
}

/// Form `n` (from 1) selected from the Project Browser's Forms list, the Properties' Symbols tab in front.
fn select_form(h: &mut Harness<BimApp>, n: usize) {
    if h.query_by_label("Symbols").is_some() {
        press(h, "Symbols");
    }
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
        assert!(h.query_all_by_label(&format!("Box · {title}")).count() > 0, "the tab names {title}");
        let p = picture(&h, &pictures);
        assert!(!seen.contains(&p.pixels), "{title} is a picture of its own");
        seen.push(p.pixels);
    }
    view(&mut h, None);
    assert_eq!(editor(&h).views.shown, None);
    assert!(h.query_all_by_label("Box · {3D}").count() > 0);
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
    assert!(h.query_all_by_label("Modify | Form").count() > 0, "its contextual tab, as in {{3D}}");
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

#[test]
fn work_plane_set_picks_a_plane_and_show_tints_it() {
    let (mut h, _) = boxed();
    view(&mut h, Some(Which::Elevation(Side::Front)));
    assert_eq!(editor(&h).views.work_plane(), Some(bimcraft_core::family::PlaneRef::Plane(bimcraft_core::family::CENTER_FRONT_BACK)));
    let edge = plane(&h, "Edge").guid;
    if let Some(ed) = h.state_mut().family_editor.as_mut() {
        ed.views.picking_plane = true;
    }
    click_family(&mut h, [-400.0, 0.0, 900.0]);
    assert_eq!(editor(&h).views.work_plane(), Some(bimcraft_core::family::PlaneRef::Plane(edge)), "picked");
    assert!(!editor(&h).views.picking_plane);
    press(&mut h, "Show");
    assert!(editor(&h).views.show_work_plane);
    // Each view keeps its own: the plan's is still Ref. Level.
    view(&mut h, Some(Which::Plan));
    assert_eq!(editor(&h).views.work_plane(), Some(bimcraft_core::family::PlaneRef::Plane(bimcraft_core::family::REF_LEVEL)));
}

#[test]
fn a_form_the_type_hides_is_halftone_until_preview_visibility_and_a_form_hidden_for_now_is_gone() {
    let (mut h, pictures) = open(&format!("{BOX}\n{CABINET}"), "Cabinet");
    // Type B hides the second form (2000 to 3000 east).
    if let Some(ed) = h.state_mut().family_editor.as_mut() {
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

/// The family's floor plan and Front elevation as the window shows them, saved where `BIMCRAFT_F4_DUMP` says (only then).
#[test]
fn family_views_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_F4_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let mut h = app(BOX_FAMILY);
    h.set_size(vec2(1600.0, 1000.0));
    h.state_mut().edit_family("Box");
    h.run();
    for (name, which) in [("floor-plan", Some(Which::Plan)), ("front-elevation", Some(Which::Elevation(Side::Front)))] {
        view(&mut h, which);
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("f4-{name}.png"))).unwrap();
    }
}
