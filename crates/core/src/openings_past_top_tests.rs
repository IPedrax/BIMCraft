//! A door or window whose head rises past its wall's top, as Revit lets one (issue #215): the wall is cut up to its top, a
//! notch open at the top, and the door or window keeps its full size; one whose sill is at or above the top, so that nothing of
//! it would be in the wall, is refused.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Id};
use crate::render::{self, PAPER, POCHE};
use crate::view3d::{P3, Scene};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

fn element(s: &mut Session, id: Id) -> Value {
    let all = run(s, "elements", json!({}));
    all["elements"].as_array().unwrap().iter().find(|e| e["id"] == id).cloned().unwrap()
}

/// A wall 6 m along x, 1060 mm tall, with a window 1200 wide and 1100 tall at a sill of 30 in its middle, from x = 2400 to
/// 3600: its head at 1130, 70 mm past the wall's top, as one in the file of issue #215. The session, the wall's id and the
/// window's.
fn low_wall() -> (Session, Id, Id) {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "window", "name": "Tall", "width": 1200, "height": 1100, "sill": 30}));
    let wall = run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "height": 1060}))["ids"][0].as_u64().unwrap();
    let window = run(&mut s, "window", json!({"wall": wall, "offset": 3000, "type": "Tall"}))["id"].as_u64().unwrap();
    (s, wall, window)
}

#[test]
fn a_window_whose_head_passes_its_walls_top_is_made_and_keeps_its_size() {
    let (mut s, wall, window) = low_wall();
    assert_eq!(s.model.check(), Ok(()));
    let w = element(&mut s, window);
    assert_eq!((&w["width"], &w["height"], &w["sill"]), (&json!(1200.0), &json!(1100.0), &json!(30.0)), "{w}");
    assert_eq!(element(&mut s, wall)["height"], json!(1060.0), "the wall keeps its height");
    // Raised further, and the wall lowered, it still has its sill in the wall.
    run(&mut s, "modify", json!({"id": window, "sill": 900}));
    run(&mut s, "modify", json!({"id": wall, "height": 950}));
    assert_eq!(s.model.check(), Ok(()));
    // A door too: one 2100 tall in the same wall, its head 1150 past the top.
    run(&mut s, "door", json!({"wall": wall, "offset": 1000}));
    assert_eq!(s.model.check(), Ok(()));
}

#[test]
fn an_opening_whose_sill_is_at_or_above_its_walls_top_is_refused_saying_what_to_do() {
    let (mut s, wall, window) = low_wall();
    for sill in [1060.0, 1059.5, 1500.0] {
        let e = fails(&mut s, "window", json!({"wall": wall, "offset": 1000, "type": "Tall", "sill": sill}));
        assert!(e.contains("sill") && e.contains("top") && e.contains(&format!("wall {wall}")), "{sill}: {e}");
        assert!(e.contains("lower") && e.contains("raise"), "says what to do: {e}");
        assert!(e.contains("1060 mm up"), "says how high the wall is there: {e}");
        assert!(e.contains("sill to 1059 or less"), "and the highest sill it takes: {e}");
    }
    // The same rule whichever moves: the window up, or the wall down under it.
    let e = fails(&mut s, "modify", json!({"id": window, "sill": 1200}));
    assert!(e.contains(&format!("window {window}")) && e.contains("lower"), "{e}");
    let e = fails(&mut s, "modify", json!({"id": wall, "height": 20}));
    assert!(e.contains(&format!("window {window}")) && e.contains("raise"), "{e}");
    // Under its base, as before.
    let e = fails(&mut s, "modify", json!({"id": window, "sill": -10}));
    assert!(e.contains("sill") && e.contains("base"), "{e}");
}

/// Whether `p` lies on triangle `t`, within half a millimetre of its plane.
fn on(t: &[P3; 3], p: P3) -> bool {
    let sub = |a: P3, b: P3| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: P3, b: P3| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let dot = |a: P3, b: P3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
    let len = dot(n, n).sqrt();
    if len < 1e-9 || (dot(n, sub(p, t[0])) / len).abs() > 0.5 {
        return false;
    }
    // On the inner side of each edge, or on it.
    (0..3).all(|i| dot(cross(sub(t[(i + 1) % 3], t[i]), sub(p, t[i])), n) >= -1e-6 * len * len)
}

/// How much the triangles hold, mm³: each with the origin a tetrahedron, signed by the way it turns.
fn held(tris: &[[P3; 3]]) -> f64 {
    tris.iter()
        .map(|[a, b, c]| a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0]))
        .sum::<f64>()
        / 6.0
}

#[test]
fn the_wall_is_notched_open_at_the_top_in_3d_and_holds_what_is_left() {
    let (s, wall, window) = low_wall();
    let scene = Scene::new(&s.model);
    let look = crate::graphics::Look::of_3d(&s.model);
    let tris: Vec<[P3; 3]> = scene.facets(&look).filter(|f| f.owner == wall).map(|f| f.corners).collect();
    let touched = |p: P3| tris.iter().any(|t| on(t, p));
    // No face of the wall spans the notch, on either face or between them, up to the top and past it.
    for x in [2450.0, 3000.0, 3550.0] {
        for y in [-100.0, -50.0, 0.0, 50.0, 100.0] {
            for z in [100.0, 600.0, 1000.0, 1059.0, 1060.0] {
                assert!(!touched([x, y, z]), "a face of the wall spans the notch at {x}, {y}, {z}");
            }
        }
    }
    // Where it is: its faces beside the window, its top beside the notch, the jambs and the notch's floor at the sill.
    for p in [[1000.0, -100.0, 600.0], [5000.0, 100.0, 600.0], [1000.0, 0.0, 1060.0], [2400.0, 0.0, 600.0], [3600.0, 0.0, 600.0]] {
        assert!(touched(p), "the wall at {p:?}");
    }
    assert!(touched([3000.0, 0.0, 30.0]), "the notch's floor, at the sill");
    // What it holds: its 6000 x 200 x 1060, less the 1200 x 200 notch from the sill to the top.
    let net = 6000.0 * 200.0 * 1060.0 - 1200.0 * 200.0 * (1060.0 - 30.0);
    assert!((held(&tris) - net).abs() < 1.0, "{} against {net}", held(&tris));
    // The window is drawn at its full height, past the wall's top.
    let top = scene.facets(&look).filter(|f| f.owner == window).flat_map(|f| f.corners).map(|p| p[2]).fold(f64::MIN, f64::max);
    assert!((top - 1130.0).abs() < 1e-6, "{top}");
}

#[test]
fn a_plan_cut_over_the_walls_top_sees_the_notch_and_one_cut_through_it_opens_the_wall_up_to_the_top() {
    // Level 1's plan cuts at 1200: over this wall's top, so nothing is cut, and the wall is seen from above with its notch.
    let (s, wall, _) = low_wall();
    let d = render::plan(&s.model, 100000, 800, 600);
    let png = render::png(&d).unwrap();
    for x in [1000.0, 3000.0, 5000.0] {
        assert_eq!(pixel(&png, d.view.px(P2(x, 0.0))), (PAPER.0, PAPER.1, PAPER.2), "nothing is cut at {x}");
    }
    assert_eq!(d.pick(d.view.px(P2(1000.0, 100.0)), 2.0), Some(wall), "its outline, seen from above");
    for x in [2400.0, 3600.0] {
        assert_eq!(d.pick(d.view.px(P2(x, 0.0)), 2.0), Some(wall), "the notch's side across the wall at {x}");
    }
    assert_eq!(d.pick(d.view.px(P2(1800.0, 0.0)), 2.0), None, "nothing across the wall but at the notch");
    // A wall 1500 tall with the window at a sill of 900, its head at 2000: the plan cuts through both.
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "window", "name": "Tall", "width": 1200, "height": 1100, "sill": 900}));
    let wall = run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "height": 1500}))["ids"][0].as_u64().unwrap();
    let window = run(&mut s, "window", json!({"wall": wall, "offset": 3000, "type": "Tall"}))["id"].as_u64().unwrap();
    let d = render::plan(&s.model, 100000, 800, 600);
    let png = render::png(&d).unwrap();
    assert_eq!(pixel(&png, d.view.px(P2(1000.0, 0.0))), (POCHE.0, POCHE.1, POCHE.2), "the wall, cut");
    assert_eq!(pixel(&png, d.view.px(P2(3000.0, 50.0))), (PAPER.0, PAPER.1, PAPER.2), "the opening's gap");
    assert_eq!(d.pick(d.view.px(P2(3000.0, 0.0)), 2.0), Some(window), "its glass");
}

#[test]
fn an_elevation_and_a_section_show_the_window_rising_out_of_the_notch() {
    let (mut s, wall, window) = low_wall();
    // Its south side, looking north; and a section across it through the window, looking west.
    let elevation = run(&mut s, "elevation", json!({"facing": "south"}))["id"].as_u64().unwrap();
    let section = run(&mut s, "section", json!({"start": [3000, -1500], "end": [3000, 1500]}))["id"].as_u64().unwrap();
    let Some(Element::Elevation(e)) = s.model.elements.get(elevation) else { panic!("no elevation") };
    let start = e.start;
    let view = render::section_fit(&s.model, elevation, 800, 600);
    let (cam, clip) = render::section_camera(&s.model, elevation, view).unwrap();
    let image = Scene::new(&s.model).cut(&cam, &clip, &[]);
    let pick = |x: f64, z: f64| {
        let (px, py) = view.px(P2(x - start.0, z));
        image.pick(f64::from(px), f64::from(py))
    };
    assert_eq!(pick(1000.0, 600.0), Some(wall), "the wall's face");
    assert_eq!(pick(3000.0, 600.0), Some(window), "the window, seen in its opening");
    assert_eq!(pick(3000.0, 1045.0), Some(window), "the window up to the wall's top: no face of the wall covers the notch");
    assert_eq!(pick(3000.0, 1100.0), Some(window), "and past it");
    assert_eq!(pick(1000.0, 1100.0), None, "over the wall beside it, nothing");
    let png = render::section_png(&s.model, elevation, view).unwrap();
    assert_eq!(pixel(&png, view.px(P2(1000.0 - start.0, 1100.0))), (PAPER.0, PAPER.1, PAPER.2));
    // Cut across the wall through the window: the wall is cut only up to the sill, then not up to its top; the glass is.
    let view = render::section_fit(&s.model, section, 800, 600);
    let png = render::section_png(&s.model, section, view).unwrap();
    // Along the section from its start at y = -1500; up from Level 1.
    let at = |y: f64, z: f64| view.px(P2(y + 1500.0, z));
    let d = render::section(&s.model, section, view);
    assert_eq!(d.pick(at(80.0, 600.0), 1.0), None, "the notch, between the glass and the wall's face");
    assert_ne!(pixel(&png, at(80.0, 600.0)), (POCHE.0, POCHE.1, POCHE.2), "not cut there");
    assert_eq!(d.pick(at(0.0, 1100.0), 2.0), Some(window), "the glass, past the top");
}

fn pixel(png: &[u8], (x, y): (f32, f32)) -> (u8, u8, u8) {
    let pm = tiny_skia::Pixmap::decode_png(png).unwrap();
    let c = pm.pixel(x as u32, y as u32).unwrap();
    (c.red(), c.green(), c.blue())
}

/// The file another program writes of [`low_wall`]'s model, as BIMCraft writes it but for the window's height: `height`.
fn file_with_window(height: f64) -> String {
    let (s, _, _) = low_wall();
    // Its OverallHeight is the IFCWINDOW's ninth attribute.
    crate::ifc::export(&s.model)
        .lines()
        .map(|line| match line.split_once("=IFCWINDOW(") {
            Some((at, args)) => {
                let mut parts: Vec<&str> = args.split(',').collect();
                let h = format!("{height}.");
                parts[8] = &h;
                format!("{at}=IFCWINDOW({}", parts.join(","))
            }
            None => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn an_ifc_with_a_window_past_its_walls_top_opens_with_it_and_saves_and_opens_again() {
    let file = file_with_window(2500.0);
    let i = crate::import::ifc(file.as_bytes(), "x").unwrap();
    assert_eq!(i.skipped, Vec::<String>::new(), "nothing is left out");
    let windows: Vec<&Element> = i.model.elements.values().filter(|e| matches!(e, Element::Window(_))).collect();
    assert_eq!(windows.len(), 1);
    let t = i.model.opening_dims(windows[0]).unwrap();
    let Element::Window(o) = windows[0] else { panic!("not a window") };
    assert_eq!((t.height, o.sill), (2500.0, 30.0), "its own size, 1470 mm past the wall's top");
    // Opened as a project, saved as IFC and as a project, and opened again: the same window.
    let dir = std::env::temp_dir().join(format!("bimcraft-openings-past-top-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = |name: &str| dir.join(name).to_string_lossy().into_owned();
    std::fs::write(path("other.ifc"), &file).unwrap();
    let mut s = Session::new();
    run(&mut s, "open", json!({"path": path("other.ifc")}));
    let size = |s: &Session| {
        let w = s.model.elements.values().find(|e| matches!(e, Element::Window(_))).unwrap();
        let Element::Window(o) = w else { panic!("not a window") };
        (s.model.opening_dims(w).unwrap().height, o.sill)
    };
    assert_eq!(size(&s), (2500.0, 30.0));
    for name in ["again.ifc", "again.bimcraft"] {
        run(&mut s, "save", json!({"path": path(name)}));
        run(&mut s, "open", json!({"path": path(name)}));
        assert_eq!(size(&s), (2500.0, 30.0), "{name}");
        assert_eq!(s.model.check(), Ok(()));
    }
    // Its wall's quantities take off only what is in the wall: 1200 x (1060 - 30) of its side.
    let ifc = crate::ifc::export(&s.model);
    let quantity = |name: &str| {
        let at = ifc.find(&format!("IFCQUANTITYVOLUME('{name}'")).or_else(|| ifc.find(&format!("IFCQUANTITYAREA('{name}'"))).unwrap();
        let rest = &ifc[at..];
        rest.split(',').nth(3).unwrap().trim_end_matches('.').parse::<f64>().unwrap()
    };
    let net = 6.0 * 1.06 - 1.2 * 1.03;
    assert!((quantity("NetSideArea") - net).abs() < 1e-6, "{}", quantity("NetSideArea"));
    assert!((quantity("NetVolume") - net * 0.2).abs() < 1e-6, "{}", quantity("NetVolume"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_ifc_window_whose_sill_is_over_its_walls_top_is_left_out_naming_the_files_wall() {
    // The wall cut down to 20 mm in the file: the window's sill, 30, is over its top.
    let file = file_with_window(1100.0);
    let file: String = file
        .lines()
        .map(|line| {
            if line.contains("=IFCEXTRUDEDAREASOLID(") && line.ends_with(",1060.);") {
                line.replace(",1060.);", ",20.);")
            } else {
                line.into()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let i = crate::import::ifc(file.as_bytes(), "x").unwrap();
    assert!(i.model.elements.values().all(|e| !matches!(e, Element::Window(_))), "{:?}", i.skipped);
    let note = i.skipped.iter().find(|n| n.starts_with("IFCWINDOW")).unwrap();
    assert!(note.contains("its wall IFCWALL #"), "names the file's wall, not BIMCraft's id for it: {note}");
    assert!(note.contains("sill 30") && note.contains("20 mm"), "says the sill and the wall's height: {note}");
    assert!(note.contains("nothing of it is in the wall") && !note.contains("Revit"), "says why, which isn't Revit's leave: {note}");
}
