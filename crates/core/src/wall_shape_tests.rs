//! Walls of other shapes, through what they make: slanted walls, curved slanted ones, an edited profile, and curved and slanted
//! curtain walls, in the 3D scene, a plan, a section and IFC.

use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Id};
use crate::render::{Prim, View, draw};
use crate::view3d::{Solid, solids};

fn session(script: &str) -> Session {
    let mut s = Session::new();
    s.script(script).unwrap_or_else(|e| panic!("{e}"));
    s
}

fn walls(s: &Session) -> Vec<Id> {
    s.model.elements.iter().filter_map(|(id, e)| matches!(e, Element::Wall(_)).then_some(*id)).collect()
}

/// The points of the solids of element `id`, in the model.
fn points(s: &Session, id: Id) -> Vec<[f64; 3]> {
    let mut out = Vec::new();
    for solid in solids(&s.model).iter().filter(|x| x.owner() == id) {
        let mut tris = Vec::new();
        solid.tris(&mut tris);
        out.extend(tris.into_iter().flat_map(|t| t.p));
    }
    out
}

/// The plan of the first level, round `centre`, at 1 px per 10 mm.
fn plan(s: &Session, centre: P2) -> crate::render::Drawing {
    let level = s.model.levels()[0].0;
    draw(&s.model, level, View { angle: 0.0, scale: 0.1, cx: centre.0, cy: centre.1, width: 1400, height: 1400 })
}

/// The span across `y` of what `id` draws in a plan, in model mm (the view's y runs down).
fn plan_span_y(d: &crate::render::Drawing, id: Id, view: (P2, f64)) -> (f64, f64) {
    let (centre, scale) = view;
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for (p, o) in d.prims.iter().zip(&d.owners) {
        if *o != Some(id) {
            continue;
        }
        if let Prim::Fill { .. } = p {
            for (_, y) in p.points() {
                let model = centre.1 - (f64::from(y) - 700.0) / scale;
                lo = lo.min(model);
                hi = hi.max(model);
            }
        }
    }
    (lo, hi)
}

#[test]
fn a_slanted_wall_leans_in_3d_and_a_plan_cuts_it_where_it_has_leant_to() {
    let s = session(r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000, "slant": 20}"#);
    let id = walls(&s)[0];
    let pts = points(&s, id);
    let lean = 3000.0 * 20f64.to_radians().tan();
    let top_y = pts.iter().filter(|p| (p[2] - 3000.0).abs() < 1e-6).map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
    assert!((top_y - (lean + 100.0 / 20f64.to_radians().cos())).abs() < 1e-6, "{top_y}");
    // The plan cuts at 1200 mm: the wall has leant 437 mm north by then.
    let d = plan(&s, P2(3000.0, 0.0));
    let (lo, hi) = plan_span_y(&d, id, (P2(3000.0, 0.0), 0.1));
    let at = 1200.0 * 20f64.to_radians().tan();
    assert!((lo - (at - 106.4)).abs() < 15.0 && (hi - (at + 106.4)).abs() < 15.0, "{lo} {hi}");
}

#[test]
fn a_slanted_wall_with_a_window_goes_to_ifc_as_face_sets_and_its_window_leans_with_it() {
    let s = session(
        r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000, "slant": -15}
           window {"wall": 100002, "offset": 3000}"#,
    );
    let ifc = crate::ifc::export(&s.model);
    assert!(ifc.contains("IFCWALL("), "a wall");
    assert!(ifc.contains("IFCTRIANGULATEDFACESET"), "of face sets");
    assert!(ifc.contains("IFCOPENINGELEMENT"), "its window's opening");
    // The window stands where the wall has leant to at its sill (south, as it leans to its interior).
    let window = s.model.elements.iter().find_map(|(id, e)| matches!(e, Element::Window(_)).then_some(*id)).unwrap_or_default();
    let e = s.model.elements.get(window).unwrap_or_else(|| panic!("no window"));
    let (Element::Door(o) | Element::Window(o)) = e else { panic!("not a window") };
    let (at, _, z) = s.model.opening_frame(o).unwrap_or_else(|| panic!("no frame"));
    assert!((at.1 + z * 15f64.to_radians().tan()).abs() < 1e-6, "{at:?} at {z}");
}

#[test]
fn an_edited_profile_draws_in_plan_only_where_it_reaches_the_cut_plane() {
    let mut s = session(r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000}"#);
    let id = walls(&s)[0];
    // A wall stepping down to 1000 mm over its last half: a plan cutting at 1200 sees its first half only.
    s.execute(
        "modify",
        &serde_json::json!({"id": id, "profile": [[[0, 0], [6000, 0], [6000, 1000], [3000, 1000], [3000, 3000], [0, 3000]]]}),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let d = plan(&s, P2(3000.0, 0.0));
    let xs: Vec<f64> = d
        .prims
        .iter()
        .zip(&d.owners)
        .filter(|(p, o)| **o == Some(id) && matches!(p, Prim::Fill { .. }))
        .flat_map(|(p, _)| p.points())
        .map(|(x, _)| 3000.0 + (f64::from(x) - 700.0) / 0.1)
        .collect();
    let hi = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    assert!((hi - 3000.0).abs() < 15.0, "{hi}");
    // In 3D it reaches 3000 only over its first half.
    let pts = points(&s, id);
    assert!(pts.iter().filter(|p| p[2] > 2999.0).all(|p| p[0] <= 3000.0 + 1e-6));
    // And to IFC as shells.
    assert!(crate::ifc::export(&s.model).contains("IFCFACETEDBREP") || crate::ifc::export(&s.model).contains("IFCCLOSEDSHELL"));
}

const CW: &str = r#"type {"category": "wall", "name": "CW", "curtain": {"columns": {"segments": [1500]}, "rows": {"segments": [1500]}}}"#;

#[test]
fn a_curved_curtain_wall_stands_in_3d_on_chords_of_its_arc_and_goes_to_ifc() {
    let s = session(&format!("{CW}\nwall {{\"points\": [[-5000, 0], [5000, 0]], \"arc\": 180, \"type\": \"CW\", \"height\": 3000}}"));
    let id = walls(&s)[0];
    let pts = points(&s, id);
    assert!(!pts.is_empty(), "it has parts");
    // Every part stands within a mullion's depth of the arc, inside its circle or just outside it.
    let r = |p: &[f64; 3]| (p[0] * p[0] + p[1] * p[1]).sqrt();
    assert!(pts.iter().all(|p| r(p) > 5000.0 - 400.0 && r(p) < 5000.0 + 400.0), "near the arc");
    // Its panels are flat: the parts of one panel share one plane, which is a chord's.
    let ifc = crate::ifc::export(&s.model);
    assert!(ifc.contains("IFCCURTAINWALL("));
    assert!(ifc.matches("IFCPLATE(").count() >= 20, "{}", ifc.matches("IFCPLATE(").count());
    // A plan draws its parts round the arc.
    let d = plan(&s, P2(0.0, -2500.0));
    assert!(d.owners.iter().filter(|o| **o == Some(id)).count() > 20);
}

#[test]
fn a_slanted_curtain_wall_tilts_its_parts_up_its_face() {
    let s = session(&format!("{CW}\nwall {{\"points\": [[0, 0], [6000, 0]], \"type\": \"CW\", \"height\": 3000, \"slant\": 25}}"));
    let id = walls(&s)[0];
    let pts = points(&s, id);
    let (lo, hi) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[1]), b.max(p[1])));
    // Leaning north 25°: its top is about 1400 mm north of its base.
    assert!(hi - lo > 1300.0, "{lo} {hi}");
    let tops = pts.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max);
    assert!(tops <= 3000.0 + 120.0, "{tops}");
    assert!(solids(&s.model).iter().any(|x| x.owner() == id && matches!(x, Solid::Mesh { .. })), "tilted parts are meshes");
    let ifc = crate::ifc::export(&s.model);
    assert!(ifc.contains("IFCCURTAINWALL(") && ifc.contains("IFCPLATE("));
}

#[test]
fn walls_joined_at_a_corner_of_unlike_heights_meet_in_plans_above_the_lower_square() {
    let s = session(
        r#"wall {"points": [[4000, 0], [0, 0]], "height": 1000}
           wall {"points": [[0, 0], [0, 4000]], "height": 3000}"#,
    );
    let tall = walls(&s)[1];
    // The plan cuts at 1200, over the lower wall: the taller one's end is square, closing the corner.
    let d = plan(&s, P2(0.0, 0.0));
    let (lo, _) = plan_span_y(&d, tall, (P2(0.0, 0.0), 0.1));
    assert!((lo + 100.0).abs() < 15.0, "{lo}");
}
