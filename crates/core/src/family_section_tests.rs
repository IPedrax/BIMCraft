//! Sections cut family forms (ROADMAP 0.8): where a section's line passes through a form of a family whose category is
//! built into the building (a generic model, casework, a door, a window), the cut is poché in the form's own shape, a hole
//! in it left open; furniture and fixtures are shown beyond the line and never cut, as before.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::Id;
use crate::render::{self, POCHE, Prim};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn id_of(v: Value) -> Id {
    v["id"].as_u64().unwrap()
}

fn pixel(png: &[u8], (x, y): (f32, f32)) -> (u8, u8, u8) {
    let pm = tiny_skia::Pixmap::decode_png(png).unwrap();
    let c = pm.pixel(x as u32, y as u32).unwrap();
    (c.red(), c.green(), c.blue())
}

/// The section `sec`'s drawing at a fitted view, and whether element `id` is filled at (along, up) mm in it: a point inside
/// an odd number of its fills' rings, as the fill is drawn.
fn filled(s: &Session, sec: Id, id: Id) -> impl Fn(f64, f64) -> bool + use<> {
    let view = render::section_fit(&s.model, sec, 1000, 800);
    let d = render::section(&s.model, sec, view);
    let rings: Vec<Vec<P2>> = d
        .prims
        .iter()
        .zip(&d.owners)
        .filter(|(_, o)| **o == Some(id))
        .flat_map(|(p, _)| match p {
            Prim::Fill { rings, .. } => rings.iter().map(|r| r.iter().map(|q| view.mm(*q)).collect()).collect(),
            _ => Vec::new(),
        })
        .collect();
    move |along, up| rings.iter().any(|r| crate::geom::contains(r, P2(along, up)))
}

/// A generic model 1000 square in plan and 500 tall at (0, 0), with a 600 square hole down through it: a frame lying flat.
fn flat_frame(s: &mut Session, category: &str) -> Id {
    run(
        s,
        "family",
        json!({"name": "Frame", "category": category, "types": {"A": {}}, "forms": [
            {"profile": [[0, 0], [1000, 0], [1000, 1000], [0, 1000]], "to": 500},
            {"void": true, "profile": [[200, 200], [800, 200], [800, 800], [200, 800]], "from": -10, "to": 510}
        ]}),
    );
    id_of(run(s, "component", json!({"family": "Frame", "at": [0, 0]})))
}

#[test]
fn a_section_cuts_a_generic_models_forms_in_their_own_shape() {
    let mut s = Session::new();
    let id = flat_frame(&mut s, "generic");
    // Across the frame's middle, looking north, from x = -500: the two sides cut, the hole between them open.
    let sec = id_of(run(&mut s, "section", json!({"start": [-500, 500], "end": [1500, 500]})));
    let at = filled(&s, sec, id);
    assert!(at(600.0, 250.0), "the west side, cut");
    assert!(at(1400.0, 250.0), "the east side, cut");
    assert!(!at(1000.0, 250.0), "the hole between them");
    assert!(!at(600.0, 600.0), "over its top");
    let view = render::section_fit(&s.model, sec, 1000, 800);
    let png = render::section_png(&s.model, sec, view).unwrap();
    assert_eq!(pixel(&png, view.px(P2(600.0, 250.0))), (POCHE.0, POCHE.1, POCHE.2), "poché");
    // A line that misses it cuts nothing.
    let past = id_of(run(&mut s, "section", json!({"start": [-500, 1500], "end": [1500, 1500]})));
    assert!(!filled(&s, past, id)(1000.0, 250.0));
}

#[test]
fn a_hole_inside_a_form_is_left_open_where_the_line_passes_through_it() {
    let mut s = Session::new();
    // A box 1000 square and tall with a closed cavity 600 wide and tall in its middle, nowhere open to the outside.
    run(
        &mut s,
        "family",
        json!({"name": "Hollow", "category": "generic", "types": {"A": {}}, "forms": [
            {"profile": [[0, 0], [1000, 0], [1000, 1000], [0, 1000]], "to": 1000},
            {"void": true, "profile": [[200, 200], [800, 200], [800, 800], [200, 800]], "from": 200, "to": 800}
        ]}),
    );
    let id = id_of(run(&mut s, "component", json!({"family": "Hollow", "at": [0, 0]})));
    let sec = id_of(run(&mut s, "section", json!({"start": [0, 500], "end": [1000, 500]})));
    let at = filled(&s, sec, id);
    assert!(at(100.0, 500.0) && at(900.0, 500.0) && at(500.0, 100.0) && at(500.0, 900.0), "round the cavity");
    assert!(!at(500.0, 500.0), "the cavity, open");
}

#[test]
fn furniture_is_shown_beyond_a_section_and_never_cut() {
    let mut s = Session::new();
    let id = flat_frame(&mut s, "furniture");
    let sec = id_of(run(&mut s, "section", json!({"start": [-500, 500], "end": [1500, 500]})));
    assert!(!filled(&s, sec, id)(600.0, 250.0), "furniture isn't cut");
    // Made a generic model, the same frame is.
    let mut s2 = Session::new();
    let generic = flat_frame(&mut s2, "generic");
    let sec2 = id_of(run(&mut s2, "section", json!({"start": [-500, 500], "end": [1500, 500]})));
    assert!(filled(&s2, sec2, generic)(600.0, 250.0));
}

#[test]
fn a_section_through_a_family_window_cuts_its_frame() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]]}));
    let family = s.model.families.iter().find(|(_, f)| f.category == crate::family::Category::Window).map(|(n, _)| n.clone()).unwrap();
    let window = id_of(run(&mut s, "window", json!({"at": [3000, 0], "family": family})));
    // Across the wall through the window's middle, looking east.
    let sec = id_of(run(&mut s, "section", json!({"start": [3000, -1500], "end": [3000, 1500]})));
    let view = render::section_fit(&s.model, sec, 1000, 800);
    let d = render::section(&s.model, sec, view);
    let cut = d.prims.iter().zip(&d.owners).any(|(p, o)| *o == Some(window) && matches!(p, Prim::Fill { .. }));
    assert!(cut, "the window's forms are cut where the line passes through them");
}
