//! Doors > Opening and Windows > Opening: the subcategory a door's swing and a window's opening lines are drawn in, as
//! Revit's, so a view's Visibility/Graphics (and Object Styles) turns them off or restyles them apart from the leaf and frame.
//! A family's symbol names it (`subcategory: "Opening"`); the starter doors' and windows' symbols do.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::Id;
use crate::render::{self, PAPER, Rgb, View};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn id_of(v: Value) -> Id {
    v["id"].as_u64().unwrap()
}

/// A plan of Level 1 round the opening at (3000, 0): 0.5 px a mm, 3 m square.
fn view() -> View {
    View { scale: 0.5, cx: 3000.0, cy: 0.0, width: 1500, height: 1500 }
}

fn picture(s: &Session) -> tiny_skia::Pixmap {
    let d = render::draw(&s.model, 100000, view());
    tiny_skia::Pixmap::decode_png(&render::png(&d).unwrap()).unwrap()
}

/// The pixels within `r` px of model point `p` that aren't paper, and those that are `colour`.
fn ink_near(pm: &tiny_skia::Pixmap, p: P2, r: i32) -> (usize, usize) {
    let (x, y) = view().px(p);
    let (mut ink, mut red) = (0, 0);
    for dx in -r..=r {
        for dy in -r..=r {
            let Some(c) = pm.pixel((x as i32 + dx) as u32, (y as i32 + dy) as u32) else { continue };
            let c = Rgb(c.red(), c.green(), c.blue());
            ink += usize::from(c != PAPER);
            red += usize::from(c.0 > 150 && c.1 < 80 && c.2 < 80);
        }
    }
    (ink, red)
}

/// The non-paper pixels in the box from model point `lo` to `hi`.
fn ink_in(pm: &tiny_skia::Pixmap, lo: P2, hi: P2) -> usize {
    let (a, b) = (view().px(lo), view().px(hi));
    let (x0, x1) = (a.0.min(b.0) as u32, a.0.max(b.0) as u32);
    let (y0, y1) = (a.1.min(b.1) as u32, a.1.max(b.1) as u32);
    (x0..=x1).flat_map(|x| (y0..=y1).map(move |y| (x, y))).filter(|&(x, y)| pm.pixel(x, y).is_some_and(|c| c.red() < 200)).count()
}

/// The middle of the longest mark the opening draws in its subcategory `key`: a point on its swing.
fn on_swing(s: &Session, id: Id, key: &str) -> P2 {
    let e = &s.model.elements[&id];
    let marks = s.model.opening_marks(e);
    let line = marks
        .iter()
        .filter(|(_, sub)| *sub == Some(key))
        .filter_map(|(m, _)| match m {
            crate::family::Mark::Line(pts) => Some(pts.clone()),
            _ => None,
        })
        .max_by_key(|pts| pts.len())
        .unwrap_or_else(|| panic!("no mark of {key}: {marks:?}"));
    line[line.len() / 2]
}

/// A 200 mm wall along x and a door of the starter Single Door in its middle.
fn door(s: &mut Session) -> Id {
    run(s, "wall", json!({"points": [[0, 0], [6000, 0]]}));
    id_of(run(s, "door", json!({"at": [3000, 0], "family": "Single Door"})))
}

#[test]
fn the_starter_doors_and_windows_draw_their_swings_and_opening_lines_in_opening() {
    let s = Session::new();
    for (name, f) in s.model.families.iter().filter(|(_, f)| f.category.hosted()) {
        for (i, y) in f.symbols.iter().enumerate() {
            assert_eq!(y.subcategory.as_deref(), Some("Opening"), "{name}: symbol {}", i + 1);
        }
    }
    let swing = &s.model.families["Single Door"].symbols;
    assert!(swing.iter().any(|y| y.arc.is_some()), "the single door's swing is an arc");
    // The subcategories are rows of Visibility/Graphics under Doors and Windows.
    let subs = |parent: &str| crate::graphics::subcategories(parent).map(|s| (s.key, s.name)).collect::<Vec<_>>();
    assert_eq!(subs("door"), [("dooropening", "Opening")]);
    assert_eq!(subs("window"), [("windowopening", "Opening")]);
}

#[test]
fn doors_opening_turned_off_takes_the_swing_away_and_leaves_the_leaf_and_frame() {
    let mut s = Session::new();
    let id = door(&mut s);
    let swing = on_swing(&s, id, "dooropening");
    let on = picture(&s);
    assert!(ink_near(&on, swing, 2).0 > 0, "the swing is drawn");
    // The leaf and the frame, where the wall is cut: across the opening, inside the wall's thickness, short of the face the
    // swing starts from.
    let (lo, hi) = (P2(2500.0, -90.0), P2(3500.0, 90.0));
    let frame_on = ink_in(&on, lo, hi);
    assert!(frame_on > 0, "the leaf and frame are drawn");
    run(&mut s, "graphics", json!({"view": "Level 1", "hide": ["dooropening"]}));
    let off = picture(&s);
    assert_eq!(ink_near(&off, swing, 2).0, 0, "the swing is gone");
    assert_eq!(ink_in(&off, lo, hi), frame_on, "the leaf and frame stay as they were");
    // Shown again, it comes back; Doors hidden takes it with them.
    run(&mut s, "graphics", json!({"view": "Level 1", "show": ["dooropening"]}));
    assert!(ink_near(&picture(&s), swing, 2).0 > 0);
    run(&mut s, "graphics", json!({"view": "Level 1", "hide": ["doors"]}));
    assert_eq!(ink_near(&picture(&s), swing, 2).0, 0, "a category hidden hides its subcategory");
}

#[test]
fn a_door_of_the_projects_own_type_draws_its_swing_in_opening_too() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]]}));
    let types: Vec<String> = s.model.types.door.keys().cloned().collect();
    run(&mut s, "door", json!({"at": [3000, 0], "type": types[0]}));
    let pm = picture(&s);
    // Its swing: the quarter circle off the wall's exterior side, from the jamb at the wall's start.
    let before = ink_in(&pm, P2(2500.0, 200.0), P2(3500.0, 1100.0));
    assert!(before > 0, "a swing is drawn");
    run(&mut s, "graphics", json!({"view": "Level 1", "hide": ["dooropening"]}));
    assert_eq!(ink_in(&picture(&s), P2(2500.0, 200.0), P2(3500.0, 1100.0)), 0, "and turned off with Doors > Opening");
}

/// A casement window of the test's own: a frame, and its opening drawn in plan as a sash swinging in, in Opening.
fn casement(s: &mut Session) -> Id {
    run(
        s,
        "family",
        json!({"name": "Swing Casement", "category": "window", "types": {"A": {}},
        "parameters": [{"name": "Width", "value": 1000}, {"name": "Height", "value": 1200}],
        "forms": [
            {"profile": [["-Width / 2", -40], ["Width / 2", -40], ["Width / 2", 40], ["-Width / 2", 40]], "from": 0, "to": "Height"},
            {"void": true, "profile": [["-Width / 2 + 50", -50], ["Width / 2 - 50", -50], ["Width / 2 - 50", 50], ["-Width / 2 + 50", 50]], "from": 50, "to": "Height - 50"}
        ],
        "symbols": [
            {"line": [["-Width / 2 + 50", -40], ["-Width / 2 + 50", "-40 - Width + 100"]], "subcategory": "Opening"},
            {"arc": {"centre": ["-Width / 2 + 50", -40], "radius": "Width - 100", "from": -90, "to": 0}, "subcategory": "Opening"}
        ]}),
    );
    run(s, "wall", json!({"points": [[0, 0], [6000, 0]]}));
    id_of(run(s, "window", json!({"at": [3000, 0], "family": "Swing Casement", "sill": 600})))
}

#[test]
fn windows_opening_turned_off_takes_the_opening_lines_away_and_leaves_the_frame() {
    let mut s = Session::new();
    let id = casement(&mut s);
    let swing = on_swing(&s, id, "windowopening");
    let on = picture(&s);
    assert!(ink_near(&on, swing, 2).0 > 0, "the opening lines are drawn");
    let (lo, hi) = (P2(2400.0, -25.0), P2(3600.0, 100.0));
    // The frame, on the side away from the sash's swing.
    let frame_on = ink_in(&on, lo, hi);
    assert!(frame_on > 0, "the frame is drawn");
    run(&mut s, "graphics", json!({"view": "Level 1", "hide": ["windowopening"]}));
    let off = picture(&s);
    assert_eq!(ink_near(&off, swing, 2).0, 0, "the opening lines are gone");
    assert_eq!(ink_in(&off, lo, hi), frame_on, "the frame stays as it was");
    // Hiding Doors > Opening leaves a window's alone.
    run(&mut s, "graphics", json!({"view": "Level 1", "show": ["windowopening"], "hide": ["dooropening"]}));
    assert!(ink_near(&picture(&s), swing, 2).0 > 0);
}

#[test]
fn the_starter_sliding_windows_sash_lines_go_with_windows_opening() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]]}));
    let id = id_of(run(&mut s, "window", json!({"at": [3000, 0], "family": "Sliding Window"})));
    let lines = |s: &Session| {
        let d = render::draw(&s.model, 100000, view());
        d.owners.iter().filter(|o| **o == Some(id)).count()
    };
    let all = lines(&s);
    run(&mut s, "graphics", json!({"view": "Level 1", "hide": ["windowopening"]}));
    let marks = s.model.opening_marks(&s.model.elements[&id]).len();
    assert_eq!(marks, 2);
    assert_eq!(lines(&s), all - marks, "its two sash lines, and nothing else, are left out");
}

#[test]
fn a_views_override_of_doors_opening_restyles_the_swing_alone() {
    let mut s = Session::new();
    let id = door(&mut s);
    let swing = on_swing(&s, id, "dooropening");
    run(&mut s, "graphics", json!({"view": "Level 1", "categories": {"dooropening": {"projection": {"color": [220, 0, 0], "weight": 8}}}}));
    let pm = picture(&s);
    assert!(ink_near(&pm, swing, 2).1 > 0, "the swing is red");
    let red_in_frame = {
        let (a, b) = (view().px(P2(2500.0, -60.0)), view().px(P2(3500.0, 60.0)));
        let mut n = 0;
        for x in a.0 as u32..=b.0 as u32 {
            for y in b.1 as u32..=a.1 as u32 {
                n += usize::from(pm.pixel(x, y).is_some_and(|c| c.red() > 150 && c.green() < 80));
            }
        }
        n
    };
    assert_eq!(red_in_frame, 0, "the leaf and frame keep their black");
    // Its own line weight: wider than the swing drawn as Doors are.
    let width = |s: &Session| {
        let d = render::draw(&s.model, 100000, view());
        let near = |p: &(f32, f32)| {
            let q = view().px(swing);
            (p.0 - q.0).hypot(p.1 - q.1) < 3.0
        };
        d.prims
            .iter()
            .zip(&d.owners)
            .filter_map(|(p, o)| match p {
                render::Prim::Line { pts, width, .. } if *o == Some(id) && pts.iter().any(near) => Some(*width),
                _ => None,
            })
            .fold(0.0f32, f32::max)
    };
    let wide = width(&s);
    run(&mut s, "graphics", json!({"view": "Level 1", "categories": {"dooropening": null}}));
    assert!(wide > width(&s), "weight 8 is wider than the Doors' projection pen");
    // An element's own override still wins over its subcategory's.
    run(
        &mut s,
        "graphics",
        json!({"view": "Level 1", "categories": {"dooropening": {"projection": {"color": [220, 0, 0]}}},
        "elements": {id.to_string(): {"projection": {"color": [0, 0, 220]}}}}),
    );
    assert_eq!(ink_near(&picture(&s), swing, 2).1, 0, "blue, not red");
}

#[test]
fn object_styles_give_doors_opening_its_own_pen_which_starts_as_the_doors() {
    let mut s = Session::new();
    let id = door(&mut s);
    let swing = on_swing(&s, id, "dooropening");
    let r = run(&mut s, "object_styles", json!({}));
    let row = |r: &Value, key: &str| r["categories"].as_array().unwrap().iter().find(|c| c["category"] == key).cloned();
    let door_row = row(&r, "door").unwrap();
    let opening = row(&r, "dooropening").expect("Doors > Opening is a row of Object Styles");
    assert_eq!((&opening["projection"], &opening["own"]), (&door_row["projection"], &json!(false)), "it starts as Doors do");
    let pen_at = |s: &Session| {
        let d = render::draw(&s.model, 100000, view());
        let q = view().px(swing);
        d.prims
            .iter()
            .zip(&d.owners)
            .zip(&d.pens)
            .find_map(|((p, o), pen)| match p {
                render::Prim::Line { pts, .. } if *o == Some(id) && pts.iter().any(|p| (p.0 - q.0).hypot(p.1 - q.1) < 3.0) => *pen,
                _ => None,
            })
            .map(|p| p.0)
    };
    run(&mut s, "object_styles", json!({"categories": {"dooropening": {"projection": 12}}}));
    assert_eq!(pen_at(&s), Some(12), "the swing takes the subcategory's pen");
    // Doors' own pen changed leaves the subcategory's as it was given; reset, it follows Doors again.
    run(&mut s, "object_styles", json!({"categories": {"door": {"projection": 3}}}));
    assert_eq!(pen_at(&s), Some(12));
    run(&mut s, "object_styles", json!({"categories": {"dooropening": {"projection": 3}}}));
    assert!(!s.model.object_styles.contains_key("dooropening"), "given what it would inherit, it holds nothing of its own");
    assert_eq!(pen_at(&s), Some(3));
}

#[test]
fn a_symbol_names_a_subcategory_its_category_has_or_is_refused() {
    let mut s = Session::new();
    let family = |sub: &str, category: &str| {
        json!({"name": "Odd", "category": category, "types": {"A": {}},
            "parameters": [{"name": "Width", "value": 900}, {"name": "Height", "value": 2100}],
            "forms": [{"profile": [[-450, -50], [450, -50], [450, 50], [-450, 50]], "to": 2100}],
            "symbols": [{"line": [[0, 0], [0, 500]], "subcategory": sub}]})
    };
    let e = s.execute("family", &family("Glass", "door")).unwrap_err();
    assert!(e.contains("Glass") && e.contains("Opening"), "says what it may be: {e}");
    let e = s.execute("family", &family("Opening", "furniture")).unwrap_err();
    assert!(e.contains("Opening"), "{e}");
    // Named in any case, it is kept as written and drawn in it.
    run(&mut s, "family", family("opening", "door"));
    assert_eq!(s.model.families["Odd"].symbols[0].subcategory.as_deref(), Some("opening"));
    // None, nothing is written; a file round trip keeps it.
    let v = serde_json::to_value(&s.model.families["Single Door"].symbols[0]).unwrap();
    assert_eq!(v["subcategory"], "Opening");
    let plain: crate::family::Symbol = serde_json::from_value(json!({"line": [[0, 0], [1, 1]]})).unwrap();
    assert!(serde_json::to_value(&plain).unwrap().get("subcategory").is_none());
}

#[test]
fn a_symbols_subcategory_is_patched_like_any_of_its_fields() {
    let mut s = Session::new();
    let guid = s.model.families["Single Door"].symbols[1].guid;
    run(&mut s, "family", json!({"name": "Single Door", "symbol": {"guid": guid, "subcategory": null}}));
    assert_eq!(s.model.families["Single Door"].symbols[1].subcategory, None);
    run(&mut s, "family", json!({"name": "Single Door", "symbol": {"guid": guid, "subcategory": "Opening"}}));
    assert_eq!(s.model.families["Single Door"].symbols[1].subcategory.as_deref(), Some("Opening"));
}

#[test]
fn a_project_that_restyles_a_subcategory_is_saved_as_a_newer_format_and_read_back() {
    let mut s = Session::new();
    door(&mut s);
    run(&mut s, "graphics", json!({"view": "Level 1", "hide": ["dooropening"]}));
    let path = std::env::temp_dir().join(format!("bimcraft-opening-sub-{}.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": path}));
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["format"], json!(crate::model::FORMAT));
    const { assert!(crate::model::FORMAT >= 18) };
    let (m, left_out) = crate::model::Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    let g = crate::graphics::graphics_of(&m, 100000).unwrap();
    assert!(g.hides("dooropening"));
}
