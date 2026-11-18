//! FE-3: Revit's zoom commands in the family document's views, its plans, ceiling plan, elevations and {3D}. Zoom to Fit (the
//! bar's button, ZF, ZE, ZA) fits the view shown; Zoom Out (2x) halves it about its middle; Zoom In Region fits the rectangle
//! dragged, and Escape gives it up; Previous Pan/Zoom steps back through the cameras the commands left in that view, the
//! wheel's ticks not among them. The view control bar has them beside Zoom to Fit. None of them touches the project's views,
//! and the project's own zoom never touches the family's.
use super::family_views::{editor, view};
use super::*;
use bimcraft_core::family::views::{Side, Which};
use bimcraft_core::view3d::Camera;

/// A box from Ref. Level up to 600, 400 by 300.
const FAMILY: &str = r#"family {"name": "Box", "category": "generic", "types": {"T": {}}, "forms": [{"profile": [[0, 0], [400, 0], [400, 300], [0, 300]], "from": 0, "to": 600}]}"#;

/// Two of the family's points, apart in plan and in every elevation: where they are on screen says how the view is looked at.
const A: [f64; 3] = [0.0, 0.0, 0.0];
const B: [f64; 3] = [400.0, 300.0, 600.0];

/// The views of a family that isn't adaptive, each kind once: none is {3D}.
const VIEWS: [Option<Which>; 4] = [Some(Which::Plan), Some(Which::Ceiling), Some(Which::Elevation(Side::Front)), None];

/// The project's four walls, one of them selected, and its family Box open in its document over them.
fn open() -> Harness<'static, BimApp> {
    let mut h = app(&format!("{BOX}\n{FAMILY}"));
    h.set_size(vec2(1600.0, 1100.0));
    h.run();
    let wall = ids(&h, "wall")[0];
    h.state_mut().selection = vec![wall];
    h.state_mut().edit_family("Box");
    h.run();
    h
}

/// Where the family's point `q` is on screen, in the view shown: a plan's or an elevation's drawing, or {3D}'s picture.
fn on_screen(h: &Harness<BimApp>, q: [f64; 3]) -> Pos2 {
    let ed = editor(h);
    match ed.views.shown {
        Some(_) => ed.views.screen_of(q).expect("a plan or an elevation drawn"),
        None => ed.drawn_3d().expect("{3D} drawn").screen(h.state().view_rect, q),
    }
}

/// How the view shown is looked at: where [`A`] and [`B`] are on screen.
fn look(h: &Harness<BimApp>) -> [Pos2; 2] {
    [on_screen(h, A), on_screen(h, B)]
}

/// The middle of the view, which Zoom Out keeps.
fn middle(h: &Harness<BimApp>) -> Pos2 {
    h.state().view_rect.center()
}

fn near(a: Pos2, b: Pos2, slack: f32, what: &str) {
    assert!(a.distance(b) <= slack, "{what}: {a:?} is not {b:?}");
}

fn same(a: [Pos2; 2], b: [Pos2; 2], what: &str) {
    for (p, q) in a.into_iter().zip(b) {
        near(p, q, 1.5, what);
    }
}

/// The project as a family's command must leave it: its plan and its 3D view looked at as before, its selection and tool.
#[derive(Debug, PartialEq)]
struct Project {
    plan: [Pos2; 2],
    orbit: Option<Camera>,
    selection: usize,
    tool: Tool,
}

fn project(h: &Harness<BimApp>) -> Project {
    let app = h.state();
    Project {
        plan: [app.screen(P2(0.0, 0.0)), app.screen(P2(8000.0, 6000.0))],
        orbit: app.orbit.camera,
        selection: app.selection.len(),
        tool: app.tool.clone(),
    }
}

/// A rectangle in the view, clear of its tabs, its ViewCube and its control bar, a little left of and above its middle.
fn rectangle(h: &Harness<BimApp>) -> (Pos2, Pos2) {
    let c = middle(h);
    (c + vec2(-160.0, -90.0), c + vec2(90.0, 50.0))
}

/// The view control bar's button `label` clicked, and the pointer taken off it, so its tip doesn't stay.
fn bar(h: &mut Harness<BimApp>, label: &str) {
    press(h, label);
    h.hover_at(pos2(2.0, 2.0));
    h.run();
}

const FIT: &str = "Zoom to Fit";
const REGION: &str = "Zoom In Region";
const OUT: &str = "Zoom Out (2x)";
const PREVIOUS: &str = "Previous Pan/Zoom";

/// Zoom In Region, dragged from `a` to `b`.
fn region(h: &mut Harness<BimApp>, a: Pos2, b: Pos2) {
    bar(h, REGION);
    drag_screen(h, a, b, PointerButton::Primary);
    h.run();
}

/// The wheel turned over the view's middle: no step of Previous Pan/Zoom.
fn wheel(h: &mut Harness<BimApp>) {
    h.hover_at(middle(h) + vec2(40.0, 30.0));
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, 120.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.run();
}

/// Frames for {3D} to fit itself on its worker.
fn settle(h: &mut Harness<BimApp>) {
    for _ in 0..20 {
        h.run();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn zoom_out_halves_each_view_about_its_middle() {
    let mut h = open();
    for which in VIEWS {
        view(&mut h, which);
        settle(&mut h);
        // Twice: each halves the view as it is then.
        for n in 1..=2 {
            let before = look(&h);
            bar(&mut h, OUT);
            let c = middle(&h);
            let after = look(&h);
            for (p, q) in before.into_iter().zip(after) {
                near(q, c + (p - c) * 0.5, 1.5, &format!("Zoom Out {n} in {which:?}"));
            }
        }
    }
}

#[test]
fn zoom_in_region_fits_the_rectangle_dragged_in_each_view() {
    let mut h = open();
    for which in VIEWS {
        view(&mut h, which);
        bar(&mut h, FIT);
        settle(&mut h);
        let before = look(&h);
        let (a, b) = rectangle(&h);
        let azimuth = editor(&h).orbit.camera.map(|c| c.azimuth);
        region(&mut h, a, b);
        // The rectangle's middle comes to the view's, and the rectangle fills it as far as it can without being cut.
        let pane = h.state().view_rect;
        let m = a + (b - a) / 2.0;
        let f = (pane.width() / (b.x - a.x)).min(pane.height() / (b.y - a.y));
        let after = look(&h);
        for (p, q) in before.into_iter().zip(after) {
            near(q, pane.center() + (p - m) * f, 2.0 + 0.01 * (p - m).length() * f, &format!("{which:?}"));
        }
        assert_eq!(editor(&h).orbit.camera.map(|c| c.azimuth), azimuth, "the drag doesn't turn {which:?}");
        // Once fitted, the region is done: another drag is the view's own.
        let again = look(&h);
        drag_screen(&mut h, a, b, PointerButton::Middle);
        assert_ne!(look(&h), again, "the middle button pans {which:?} again");
    }
}

#[test]
fn escape_gives_up_the_region_and_nothing_else() {
    let mut h = open();
    view(&mut h, Some(Which::Plan));
    super::family_views::select_form(&mut h, 1);
    assert!(editor(&h).selected_form().is_some());
    let before = look(&h);
    bar(&mut h, REGION);
    key(&mut h, Key::Escape);
    assert!(editor(&h).selected_form().is_some(), "the form stays selected: Escape gave up the region alone");
    let (a, b) = rectangle(&h);
    drag_screen(&mut h, a, b, PointerButton::Primary);
    same(look(&h), before, "a drag after Escape zooms nothing");
}

#[test]
fn previous_steps_back_through_the_zooms_of_the_view_shown_and_not_the_wheels() {
    let mut h = open();
    for which in VIEWS {
        view(&mut h, which);
        bar(&mut h, FIT);
        settle(&mut h);
        let fitted = look(&h);
        bar(&mut h, OUT);
        let out = look(&h);
        let (a, b) = rectangle(&h);
        region(&mut h, a, b);
        let zoomed = look(&h);
        assert_ne!(zoomed, out);
        wheel(&mut h);
        assert_ne!(look(&h), zoomed, "the wheel zooms {which:?}");
        bar(&mut h, PREVIOUS);
        same(look(&h), out, &format!("Previous in {which:?}: before the region"));
        bar(&mut h, PREVIOUS);
        same(look(&h), fitted, &format!("Previous in {which:?}: before Zoom Out"));
        // Fit, the first command of all here, is where it stops.
        for _ in 0..3 {
            bar(&mut h, PREVIOUS);
        }
        let first = look(&h);
        bar(&mut h, PREVIOUS);
        same(look(&h), first, &format!("Previous in {which:?} with nothing before"));
    }
    // Each view keeps its own: Zoom Out in the plan is no step of the elevation's.
    view(&mut h, Some(Which::Elevation(Side::Front)));
    let elevation = look(&h);
    view(&mut h, Some(Which::Plan));
    let plan = look(&h);
    bar(&mut h, OUT);
    view(&mut h, Some(Which::Elevation(Side::Front)));
    bar(&mut h, PREVIOUS);
    same(look(&h), elevation, "Previous in the elevation after Zoom Out in the plan");
    view(&mut h, Some(Which::Plan));
    bar(&mut h, PREVIOUS);
    same(look(&h), plan, "Previous in the plan");
}

#[test]
fn zoom_to_fit_fits_the_view_shown() {
    let mut h = open();
    for which in VIEWS {
        view(&mut h, which);
        bar(&mut h, FIT);
        settle(&mut h);
        let fitted = look(&h);
        // The bar's button, and the project's keys for it, which a family tab shares.
        for how in [FIT, "ZF", "ZE", "ZA"] {
            // Zoomed by the wheel, which a plan's own fit used to leave as it was.
            wheel(&mut h);
            assert_ne!(look(&h), fitted, "the wheel zooms {which:?}");
            if how == FIT {
                bar(&mut h, FIT);
            } else {
                shortcut(&mut h, how);
            }
            settle(&mut h);
            same(look(&h), fitted, &format!("{how} in {which:?}"));
        }
    }
}

#[test]
fn the_view_control_bar_has_them_beside_zoom_to_fit() {
    let mut h = open();
    view(&mut h, Some(Which::Plan));
    let fitted = look(&h);
    bar(&mut h, OUT);
    let c = middle(&h);
    near(look(&h)[1], c + (fitted[1] - c) * 0.5, 1.5, OUT);
    let out = look(&h);
    bar(&mut h, REGION);
    let (a, b) = rectangle(&h);
    drag_screen(&mut h, a, b, PointerButton::Primary);
    assert!((look(&h)[1] - look(&h)[0]).length() > (out[1] - out[0]).length() * 2.0, "Zoom In Region zoomed in");
    bar(&mut h, PREVIOUS);
    same(look(&h), out, PREVIOUS);
    // In a row, after Zoom to Fit, in the bar at the view's bottom left.
    let bar: Vec<_> = [FIT, REGION, OUT, PREVIOUS].map(|l| h.get_by_label(l).rect()).into();
    for w in bar.windows(2) {
        assert!(w[1].left() >= w[0].right() - 0.5 && (w[1].center().y - w[0].center().y).abs() < 1.0, "{bar:?}");
    }
    assert!(bar[0].bottom() > h.state().view_rect.bottom() - 40.0, "at the bottom");
}

#[test]
fn an_adaptive_familys_modelling_view_zooms_too() {
    let (mut h, ..) = super::family_temporary_hide::adaptive();
    h.run();
    let cam = |h: &Harness<BimApp>| editor(h).modeller.camera.expect("the modelling view drawn");
    let before = cam(&h);
    bar(&mut h, OUT);
    let out = cam(&h);
    assert!((out.scale / before.scale - 0.5).abs() < 1e-6, "Zoom Out halves it: {} then {}", before.scale, out.scale);
    let c = middle(&h);
    region(&mut h, c + vec2(-100.0, -60.0), c + vec2(60.0, 40.0));
    let zoomed = cam(&h);
    assert!(zoomed.scale > out.scale * 3.0, "the region fits the rectangle: {} then {}", out.scale, zoomed.scale);
    assert_eq!((zoomed.azimuth, zoomed.elevation), (out.azimuth, out.elevation), "the drag doesn't turn it");
    bar(&mut h, PREVIOUS);
    assert_eq!(cam(&h), out, "Previous");
    bar(&mut h, PREVIOUS);
    assert_eq!(cam(&h), before, "Previous again");
}

#[test]
fn a_family_zoom_never_touches_the_projects_views_nor_the_projects_zoom_the_familys() {
    let mut h = open();
    let untouched = project(&h);
    for which in VIEWS {
        view(&mut h, which);
        for label in [OUT, FIT, PREVIOUS] {
            bar(&mut h, label);
            assert_eq!(project(&h), untouched, "{label} in the family's {which:?}");
        }
        shortcut(&mut h, "ZF");
        assert_eq!(project(&h), untouched, "ZF in the family's {which:?}");
        let (a, b) = rectangle(&h);
        region(&mut h, a, b);
        assert_eq!(project(&h), untouched, "Zoom In Region in the family's {which:?}");
    }
    // On the project's tab, its own zoom leaves the family's plan as it was, and arms nothing there.
    view(&mut h, Some(Which::Plan));
    let family = look(&h);
    press(&mut h, "Level 1");
    assert!(h.state().active_family().is_none(), "the project's tab");
    for label in ["Zoom Out", "Zoom to Fit"] {
        bar(&mut h, label);
    }
    shortcut(&mut h, "ZF");
    press(&mut h, "Box - Floor Plan: Ref. Level");
    same(look(&h), family, "the family's plan after the project's zoom");
    let (a, b) = rectangle(&h);
    drag_screen(&mut h, a, b, PointerButton::Primary);
    same(look(&h), family, "no region was armed from the project's tab");
}

/// What the status bar says while Zoom In Region is armed.
const PROMPT: &str = "Zoom In Region: drag a rectangle over the view to fit it. Esc cancels.";

fn prompted(h: &Harness<BimApp>) -> bool {
    h.query_all_by_label_contains(PROMPT).next().is_some()
}

#[test]
fn the_status_bar_asks_for_the_rectangle_while_the_region_is_armed() {
    let mut h = open();
    for which in VIEWS {
        view(&mut h, which);
        assert!(!prompted(&h), "{which:?}: not armed");
        bar(&mut h, REGION);
        assert!(prompted(&h), "{which:?}: armed, the status bar says so");
        key(&mut h, Key::Escape);
        assert!(!prompted(&h), "{which:?}: Escape, and it no longer does");
    }
    // Done by its drag, the same.
    view(&mut h, Some(Which::Plan));
    let (a, b) = rectangle(&h);
    region(&mut h, a, b);
    assert!(!prompted(&h), "fitted");
}

#[test]
fn another_view_shown_gives_the_region_up() {
    let mut h = open();
    view(&mut h, Some(Which::Plan));
    bar(&mut h, REGION);
    assert!(prompted(&h));
    view(&mut h, Some(Which::Elevation(Side::Front)));
    assert!(!prompted(&h), "the elevation shown: no region");
    let (a, b) = rectangle(&h);
    let elevation = look(&h);
    drag_screen(&mut h, a, b, PointerButton::Primary);
    same(look(&h), elevation, "a drag in the elevation zooms nothing");
    // Given up, not set aside: back in the plan, a drag is the plan's own again.
    view(&mut h, Some(Which::Plan));
    assert!(!prompted(&h), "back in the plan: no region either");
    let plan = look(&h);
    drag_screen(&mut h, a, b, PointerButton::Primary);
    same(look(&h), plan, "a drag in the plan zooms nothing");
    // {3D} too.
    bar(&mut h, REGION);
    view(&mut h, None);
    settle(&mut h);
    assert!(!prompted(&h), "{{3D}} shown: no region");
}

#[test]
fn previous_while_a_fit_of_3d_is_still_to_come_keeps_the_camera_it_went_back_to() {
    let mut h = open();
    view(&mut h, None);
    settle(&mut h);
    bar(&mut h, OUT);
    settle(&mut h);
    let out = editor(&h).orbit.camera.expect("{3D} drawn");
    // Zoom to Fit asks for a fit, which comes back frames later; Previous before it does.
    {
        let ed = h.state_mut().active_family_mut().expect("a family open");
        ed.zoom_to_fit();
        ed.previous_zoom();
    }
    settle(&mut h);
    assert_eq!(editor(&h).orbit.camera, Some(out), "Previous wins over the fit it went back from");
}

/// The view control bar with the new buttons, light and dark, and a region being dragged in the plan and once it is fitted,
/// saved where `BIMCRAFT_FE3_DUMP` says (only then).
#[test]
fn family_zoom_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_FE3_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let save = |h: &mut Harness<BimApp>, name: &str| {
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("fe3u-{name}.png"))).unwrap();
    };
    for (shade, theme) in [("light", egui::ThemePreference::Light), ("dark", egui::ThemePreference::Dark)] {
        let mut h = open();
        h.ctx.set_theme(theme);
        view(&mut h, Some(Which::Plan));
        settle(&mut h);
        // The bar with the pointer over Zoom Out (2x), its tip shown.
        let at = h.get_by_label(OUT).rect().center();
        h.hover_at(at);
        for _ in 0..6 {
            h.run();
        }
        save(&mut h, &format!("bar-{shade}"));
        bar(&mut h, REGION);
        let (a, b) = rectangle(&h);
        h.hover_at(a);
        h.run();
        h.event(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.run();
        for t in [0.5, 1.0] {
            h.hover_at(a + (b - a) * t);
            h.run();
        }
        save(&mut h, &format!("region-armed-{shade}"));
        h.event(Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        h.run();
        settle(&mut h);
        save(&mut h, &format!("region-after-{shade}"));
    }
}

/// The view control bar's zoom icons beside Zoom In, Zoom Out and Undo, at 18 points as the bar paints them, once at one pixel a
/// point and once at four, saved where `BIMCRAFT_FE3_DUMP` says (only then).
#[test]
fn zoom_icons_screenshot() {
    use crate::icons::Icon;
    let Ok(folder) = std::env::var("BIMCRAFT_FE3_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let icons = [Icon::Fit, Icon::ZoomRegion, Icon::ZoomOut, Icon::ZoomPrevious, Icon::ZoomIn, Icon::Undo];
    for ppp in [1.0, 4.0] {
        let mut h = Harness::builder().with_size([icons.len() as f32 * 26.0 + 8.0, 32.0]).with_pixels_per_point(ppp).build_ui(|ui| {
            ui.painter().rect_filled(ui.max_rect().expand(8.0), 0.0, egui::Color32::WHITE);
            for (k, icon) in icons.iter().enumerate() {
                let at = egui::Rect::from_min_size(pos2(4.0 + 26.0 * k as f32 + 4.0, 7.0), vec2(18.0, 18.0));
                crate::icons::paint(ui.painter(), *icon, at, egui::Color32::from_gray(40));
            }
        });
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("fe3u-icons-x{ppp}.png"))).unwrap();
    }
}

/// Rendered as the window shows it (egui's renderer, on a real adapter, as `family_views::gpu` does): {3D}'s pixels show the box
/// smaller after Zoom Out (2x) and larger after a region round it, saved where `BIMCRAFT_FE3_DUMP` says. Without an adapter,
/// nothing.
mod gpu {
    use super::*;

    /// The family's {3D}, the pointer out of the way; none where nothing renders.
    fn rendered() -> Option<Harness<'static, BimApp>> {
        let mut h = open();
        view(&mut h, None);
        h.hover_at(pos2(2.0, 2.0));
        settle(&mut h);
        h.render().ok()?;
        Some(h)
    }

    /// The picture of the view shown, and how many of its pixels are not the background's: the box's.
    fn boxed(h: &mut Harness<BimApp>) -> (image::RgbaImage, usize) {
        let picture = h.render().unwrap();
        let pane = h.state().view_rect.shrink2(vec2(60.0, 70.0));
        let ppp = h.ctx.pixels_per_point();
        let px = |q: Pos2| picture.get_pixel((q.x * ppp) as u32, (q.y * ppp) as u32).0;
        let background = px(pane.left_bottom() + vec2(10.0, -10.0));
        let mut n = 0;
        for y in (pane.top() as u32..pane.bottom() as u32).step_by(2) {
            for x in (pane.left() as u32..pane.right() as u32).step_by(2) {
                let c = px(pos2(x as f32, y as f32));
                if (0..3).map(|k| c[k].abs_diff(background[k]) as u32).sum::<u32>() > 30 {
                    n += 1;
                }
            }
        }
        (picture, n)
    }

    #[test]
    fn zoom_out_and_a_region_change_the_box_in_the_3d_picture() {
        let Some(mut h) = rendered() else { return };
        let folder = std::env::var("BIMCRAFT_FE3_DUMP").ok();
        let save = |name: &str, picture: &image::RgbaImage| {
            if let Some(folder) = &folder {
                std::fs::create_dir_all(folder).unwrap();
                picture.save(std::path::Path::new(folder).join(format!("fe3u-3d-{name}.png"))).unwrap();
            }
        };
        let (picture, fitted) = boxed(&mut h);
        save("1-fitted", &picture);
        bar(&mut h, OUT);
        settle(&mut h);
        let (picture, out) = boxed(&mut h);
        save("2-zoom-out", &picture);
        assert!(out * 3 < fitted && out * 6 > fitted, "half as close, about a quarter of the pixels: {fitted} then {out}");
        // A region round the box as it is now, a little wider than it.
        let (a, b) = [A, B, [400.0, 0.0, 0.0], [0.0, 300.0, 600.0], [0.0, 0.0, 600.0], [400.0, 300.0, 0.0]]
            .map(|q| on_screen(&h, q))
            .into_iter()
            .fold((pos2(f32::MAX, f32::MAX), pos2(f32::MIN, f32::MIN)), |(lo, hi), p| (lo.min(p), hi.max(p)));
        let (a, b) = (a - vec2(30.0, 30.0), b + vec2(30.0, 30.0));
        bar(&mut h, REGION);
        h.hover_at(a);
        h.run();
        h.event(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.run();
        for t in [0.5, 1.0] {
            h.hover_at(a + (b - a) * t);
            h.run();
        }
        save("3-region-dragged", &h.render().unwrap());
        h.event(Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        h.run();
        h.hover_at(pos2(2.0, 2.0));
        settle(&mut h);
        let (picture, zoomed) = boxed(&mut h);
        save("4-region-fitted", &picture);
        // As close as the box's height lets it, from half as close as fitted: over three times the pixels, and more than fitted.
        assert!(
            zoomed > out * 3 && zoomed > fitted,
            "the region round the box fills the view with it: {fitted} fitted, {out} out, {zoomed} now"
        );
    }
}
