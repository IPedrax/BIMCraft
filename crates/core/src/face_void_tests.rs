//! A face-hosted family's voids cut its host (T16b), as Revit's face-based families do: a component placed on a face of a
//! floor, a ceiling, a roof or a wall whose voids reach into its host and cut it (`both` or `host`, wherever listed) makes a hole there, in
//! the host's solids (so the 3D view and sections), in plan and in a ceiling plan, and as an IFC opening voiding the host.
//! A component with no such void leaves its host as it was, and the hole follows its host.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::Id;
use crate::render::{self, POCHE, Prim, View};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn id_of(v: &Value) -> Id {
    v["id"].as_u64().unwrap()
}

/// The first level, whose plan the first view is.
const LEVEL: Id = 100000;

/// A drain: a round void 300 across, `Void Depth` (an instance length, 400 unless given) down from 10 over its insertion
/// point along its z, listed first so it cuts none of its own forms; then a grate 400 square and 20 thick on the face. Work
/// plane-based, and not Always Vertical, so its z is the face's normal: into a ceiling from under it, into a wall from its
/// side.
fn drain(s: &mut Session) {
    run(
        s,
        "family",
        json!({"name": "Drain", "category": "generic", "types": {"A": {}},
        "options": {"work_plane_based": true, "always_vertical": false},
        "parameters": [{"name": "Void Depth", "instance": true, "value": 400}],
        "forms": [
            {"void": true, "circle": {"centre": [0, 0], "radius": 150}, "from": "-Void Depth", "to": 10},
            {"profile": [[-200, -200], [200, -200], [200, 200], [-200, 200]], "to": 20}
        ]}),
    );
}

/// The same grate with no void.
fn grate(s: &mut Session) {
    run(
        s,
        "family",
        json!({"name": "Grate", "category": "generic", "types": {"A": {}},
               "options": {"work_plane_based": true, "always_vertical": false},
               "forms": [{"profile": [[-200, -200], [200, -200], [200, 200], [-200, 200]], "to": 20}]}),
    );
}

fn put(s: &mut Session, family: &str, host: Id, face: &str, at: Value) -> Id {
    id_of(&run(s, "component", json!({"family": family, "at": at, "host": {"face": {"element": host, "face": face}}})))
}

/// A floor 6000 by 4000 of the starter 200 mm type, its top at the level (0).
fn floor(s: &mut Session) -> Id {
    id_of(&run(s, "floor", json!({"boundary": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]], "offset": 0})))
}

/// Whether `owner` is filled in the section from `start` to `end` at (along, z): a point inside one of its fills.
fn filled(s: &mut Session, owner: Id, start: [f64; 2], end: [f64; 2]) -> impl Fn(f64, f64) -> bool + use<> {
    let sec = id_of(&run(s, "section", json!({"start": start, "end": end})));
    let view = render::section_fit(&s.model, sec, 1200, 800);
    let d = render::section(&s.model, sec, view);
    let rings: Vec<Vec<P2>> = d
        .prims
        .iter()
        .zip(&d.owners)
        .filter(|(_, o)| **o == Some(owner))
        .flat_map(|(p, _)| match p {
            Prim::Fill { rings, .. } => rings.iter().map(|r| r.iter().map(|q| view.mm(*q)).collect()).collect(),
            _ => Vec::new(),
        })
        .collect();
    run(s, "delete", json!({"ids": [sec]}));
    move |x, z| rings.iter().any(|r| crate::geom::contains(r, P2(x, z)))
}

/// The section's picture from `start` to `end`: its colour at (along, z).
fn section_pixel(s: &mut Session, start: [f64; 2], end: [f64; 2]) -> impl Fn(f64, f64) -> render::Rgb + use<> {
    let sec = id_of(&run(s, "section", json!({"start": start, "end": end})));
    let view = render::section_fit(&s.model, sec, 1200, 800);
    let pm = tiny_skia::Pixmap::decode_png(&render::section_png(&s.model, sec, view).unwrap()).unwrap();
    run(s, "delete", json!({"ids": [sec]}));
    move |x, z| {
        let (u, v) = view.px(P2(x, z));
        let c = pm.pixel(u as u32, v as u32).unwrap();
        render::Rgb(c.red(), c.green(), c.blue())
    }
}

/// `owner`'s triangles in 3D, as its solids make them.
fn triangles(s: &Session, owner: Id) -> Vec<[[f64; 3]; 3]> {
    crate::view3d::solids(&s.model).iter().filter(|x| x.owner() == owner).flat_map(|x| x.triangles()).collect()
}

/// Where a plumb line through (x, y) crosses `owner`'s triangles, a little off so it runs down no edge.
fn plumb(s: &Session, owner: Id, x: f64, y: f64) -> Vec<f64> {
    let q = P2(x + 0.37, y + 0.13);
    let mut hits: Vec<f64> = triangles(s, owner)
        .iter()
        .filter_map(|[a, b, c]| {
            let (pa, pb, pc) = (P2(a[0], a[1]), P2(b[0], b[1]), P2(c[0], c[1]));
            let area = (pb - pa).cross(pc - pa);
            if area.abs() < 1e-9 {
                return None;
            }
            let (u, v) = ((q - pa).cross(pc - pa) / area, (pb - pa).cross(q - pa) / area);
            (u >= 0.0 && v >= 0.0 && u + v <= 1.0).then(|| a[2] + (b[2] - a[2]) * u + (c[2] - a[2]) * v)
        })
        .collect();
    hits.sort_by(f64::total_cmp);
    hits.dedup_by(|x, y| (*x - *y).abs() < 1e-3);
    hits
}

/// The STEP text of the project as IFC.
fn ifc(s: &mut Session) -> String {
    let path = std::env::temp_dir().join(format!("bimcraft-face-void-{}-{:?}.ifc", std::process::id(), std::thread::current().id()));
    run(s, "save", json!({"path": path}));
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    text
}

/// The entities of `class` that an opening element voids (`IFCRELVOIDSELEMENT(…,#host,#opening)`), counted: an opening
/// whose body is closed face sets (the voids' own shape).
fn voided(text: &str, class: &str) -> usize {
    let hosts: Vec<String> =
        text.lines().filter(|l| l.contains(&format!("={class}("))).map(|l| l.split('=').next().unwrap().to_string()).collect();
    let openings: Vec<String> = text
        .lines()
        .filter(|l| l.contains("=IFCOPENINGELEMENT("))
        .filter(|l| {
            // Its shape, then its representation, whose items are face sets.
            let args: Vec<&str> = l.split(',').collect();
            let shape = entity(text, args[6].trim());
            let rep = shape.split("(#").nth(1).unwrap().split(')').next().unwrap();
            entity(text, &format!("#{rep}")).contains("'Tessellation'")
        })
        .map(|l| l.split('=').next().unwrap().to_string())
        .collect();
    text.lines()
        .filter(|l| l.contains("=IFCRELVOIDSELEMENT("))
        .filter(|l| {
            let args: Vec<&str> = l.trim_end_matches(");").split(',').collect();
            let (host, opening) = (args[4].trim(), args[5].trim());
            hosts.iter().any(|h| h == host) && openings.iter().any(|o| o == opening)
        })
        .count()
}

/// The entity `#n` of STEP text `text`.
fn entity<'a>(text: &'a str, n: &str) -> &'a str {
    text.lines().find(|l| l.starts_with(&format!("{n}="))).unwrap_or_else(|| panic!("no {n}"))
}

/// Whether a line `owner` draws in drawing `d` passes within 2 mm of `p`.
fn drawn_near(d: &render::Drawing, owner: Id, p: P2) -> bool {
    d.prims.iter().zip(&d.owners).filter(|(_, o)| **o == Some(owner)).any(|(prim, _)| match prim {
        Prim::Line { pts, closed, .. } => {
            let pts: Vec<P2> = pts.iter().map(|q| d.view.mm(*q)).collect();
            let n = pts.len();
            let ends = if *closed { n } else { n.saturating_sub(1) };
            (0..ends).any(|i| crate::geom::dist_to_segment(p, pts[i], pts[(i + 1) % n]) < 2.0)
        }
        _ => false,
    })
}

#[test]
fn a_drain_on_a_floors_top_cuts_a_hole_in_section_3d_plan_and_ifc() {
    let mut s = Session::new();
    drain(&mut s);
    let f = floor(&mut s);
    put(&mut s, "Drain", f, "top", json!([3000, 2000]));
    // The section across the floor through the drain: floor either side, the hole through its whole depth.
    let at = filled(&mut s, f, [0.0, 2000.0], [6000.0, 2000.0]);
    assert!(at(1000.0, -100.0) && at(5000.0, -100.0), "floor round it");
    assert!(!at(3000.0, -100.0) && !at(3000.0, -20.0) && !at(3000.0, -180.0), "the hole");
    assert!(!at(3120.0, -100.0) && at(3200.0, -100.0), "300 across");
    let px = section_pixel(&mut s, [0.0, 2000.0], [6000.0, 2000.0]);
    assert_eq!(px(1000.0, -100.0), POCHE);
    assert_ne!(px(3000.0, -100.0), POCHE);
    // In 3D: a plumb line through the hole meets none of the floor; one beside it, its top and bottom.
    assert!(plumb(&s, f, 3000.0, 2000.0).is_empty(), "{:?}", plumb(&s, f, 3000.0, 2000.0));
    let beside = plumb(&s, f, 1000.0, 2000.0);
    assert!(beside.len() == 2 && (beside[0] + 200.0).abs() < 1e-6 && beside[1].abs() < 1e-6, "{beside:?}");
    // The floor's plan, which sees its top: the hole's edge.
    let view = View { scale: 0.2, cx: 3000.0, cy: 2000.0, width: 1600, height: 1200 };
    let d = render::draw(&s.model, LEVEL, view);
    assert!(drawn_near(&d, f, P2(3150.0, 2000.0)) && drawn_near(&d, f, P2(3000.0, 1850.0)), "the hole drawn in plan");
    // IFC: an opening, the void's body, voiding the slab.
    assert_eq!(voided(&ifc(&mut s), "IFCSLAB"), 1);
}

#[test]
fn a_drain_under_a_ceiling_cuts_it_and_its_ceiling_plan_shows_the_hole() {
    let mut s = Session::new();
    drain(&mut s);
    let c = id_of(&run(&mut s, "ceiling", json!({"boundary": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]]})));
    put(&mut s, "Drain", c, "bottom", json!([3000, 2000]));
    let (bottom, top) = {
        let g = s.model.guid(c);
        let at = |face| s.model.face_at(g, face, [1000.0, 1000.0]).unwrap().point[2];
        (at(crate::model::FaceRole::Bottom), at(crate::model::FaceRole::Top))
    };
    assert!(top > bottom, "{bottom} {top}");
    let mid = (bottom + top) / 2.0;
    let at = filled(&mut s, c, [0.0, 2000.0], [6000.0, 2000.0]);
    assert!(at(1000.0, mid) && !at(3000.0, mid), "the ceiling round it, and the hole");
    assert!(plumb(&s, c, 3000.0, 2000.0).is_empty());
    assert_eq!(plumb(&s, c, 1000.0, 2000.0).len(), 2);
    // The ceiling plan, which looks up at its underside: the hole's edge.
    let d = render::ceiling_plan(&s.model, LEVEL, 1600, 1200);
    assert!(drawn_near(&d, c, P2(3150.0, 2000.0)), "the hole drawn in the ceiling plan");
    assert_eq!(voided(&ifc(&mut s), "IFCCOVERING"), 1);
}

#[test]
fn a_drain_on_a_sloped_roof_cuts_it_square_to_the_slope() {
    let mut s = Session::new();
    drain(&mut s);
    let r = id_of(&run(&mut s, "roof", json!({"boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "slope": 30, "overhang": 0})));
    put(&mut s, "Drain", r, "top", json!([4000, 1500]));
    let g = s.model.guid(r);
    let top = |y: f64| s.model.face_at(g, crate::model::FaceRole::Top, [4000.0, y]).unwrap().point[2];
    // A section up the south slope, across the eaves, through the drain: along it is y.
    let (hole, beside) = (top(1500.0) - 60.0, top(2500.0) - 60.0);
    let at = filled(&mut s, r, [4000.0, -500.0], [4000.0, 3000.0]);
    assert!(at(3000.0, beside), "the roof up the slope from it");
    assert!(!at(2000.0, hole), "the hole");
    // The void goes in square to the slope, so its axis leans up the slope as it goes down: a plumb line 65 up the slope
    // from where it enters stays within its 150 all the way through the roof's 300.
    assert!(plumb(&s, r, 4000.0, 1565.0).is_empty(), "{:?}", plumb(&s, r, 4000.0, 1565.0));
    assert_eq!(plumb(&s, r, 4000.0, 2500.0).len(), 2);
    assert_eq!(voided(&ifc(&mut s), "IFCROOF"), 1);
}

#[test]
fn a_drain_on_a_walls_side_cuts_through_the_wall_as_a_doors_voids_do() {
    let mut s = Session::new();
    drain(&mut s);
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "height": 3000}))["ids"][0].as_u64().unwrap();
    // On the exterior (+y), at the plan's cut height: its z out of the wall, its void 400 back into the 200 wall.
    put(&mut s, "Drain", w, "exterior", json!([3000, 100, 1200]));
    let at = filled(&mut s, w, [0.0, 10.0], [6000.0, 10.0]);
    assert!(at(1000.0, 1200.0) && at(3000.0, 1500.0), "the wall round it");
    assert!(!at(3000.0, 1200.0), "the hole");
    assert!(
        crate::view3d::solids(&s.model).iter().any(|x| x.owner() == w && matches!(x, crate::view3d::Solid::Mesh { cut: true, .. })),
        "cut through T22's path, a mesh a section cuts"
    );
    // In plan the cut plane passes through it: the wall open there, poché beside it.
    let view = View { scale: 0.5, cx: 3000.0, cy: 0.0, width: 1200, height: 1200 };
    let pm = tiny_skia::Pixmap::decode_png(&render::png(&render::draw(&s.model, LEVEL, view)).unwrap()).unwrap();
    let pixel = |p: P2| {
        let (x, y) = view.px(p);
        let c = pm.pixel(x as u32, y as u32).unwrap();
        render::Rgb(c.red(), c.green(), c.blue())
    };
    assert_eq!(pixel(P2(1000.0, 0.0)), POCHE);
    assert_ne!(pixel(P2(3000.0, 0.0)), POCHE, "the void's place in the wall is open");
    assert_eq!(voided(&ifc(&mut s), "IFCWALL"), 1);
}

#[test]
fn no_void_no_change_and_a_void_that_reaches_nothing_cuts_nothing() {
    let mut s = Session::new();
    grate(&mut s);
    drain(&mut s);
    let f = floor(&mut s);
    let before = triangles(&s, f);
    put(&mut s, "Grate", f, "top", json!([3000, 2000]));
    assert_eq!(triangles(&s, f), before, "a grate with no void leaves the floor as it was");
    // A drain lifted off the floor by more than its void reaches: nothing cut, the floor not made a mesh.
    let lifted = put(&mut s, "Drain", f, "top", json!([3000, 2000]));
    run(&mut s, "modify", json!({"id": lifted, "offset_from_host": 1000}));
    assert_eq!(triangles(&s, f), before, "out of reach");
    assert!(crate::view3d::solids(&s.model).iter().filter(|x| x.owner() == f).all(|x| !matches!(x, crate::view3d::Solid::Mesh { .. })));
    assert_eq!(voided(&ifc(&mut s), "IFCSLAB"), 0);
}

#[test]
fn the_hole_follows_its_host_and_a_thicker_floor_is_cut_as_far_as_the_void_reaches() {
    let mut s = Session::new();
    drain(&mut s);
    let f = floor(&mut s);
    put(&mut s, "Drain", f, "top", json!([3000, 2000]));
    run(&mut s, "move", json!({"ids": [f], "by": [1000, 0]}));
    assert!(plumb(&s, f, 4000.0, 2000.0).is_empty(), "moved with the floor");
    assert_eq!(plumb(&s, f, 3000.0, 2000.0).len(), 2, "where it was, floor");
    // 300 thick: the void 400 deep still goes through.
    run(&mut s, "type", json!({"category": "floor", "name": "Generic - 200mm", "thickness": 300}));
    let at = filled(&mut s, f, [0.0, 2000.0], [8000.0, 2000.0]);
    assert!(at(2000.0, -250.0) && !at(4000.0, -250.0), "through 300");
    // 600 thick: a recess 400 deep from the top, the floor under it.
    run(&mut s, "type", json!({"category": "floor", "name": "Generic - 200mm", "thickness": 600}));
    let at = filled(&mut s, f, [0.0, 2000.0], [8000.0, 2000.0]);
    assert!(!at(4000.0, -200.0) && at(4000.0, -500.0), "a recess in 600");
    let hits = plumb(&s, f, 4000.0, 2000.0);
    assert!(hits.len() == 2 && (hits[0] + 600.0).abs() < 1e-6 && (hits[1] + 400.0).abs() < 1e-6, "{hits:?}");
    // Its own depth deeper: through again.
    let drain_id = s.model.elements.iter().find(|(_, e)| matches!(e, crate::model::Element::Component(_))).map(|(id, _)| *id).unwrap();
    run(&mut s, "modify", json!({"id": drain_id, "params": {"Void Depth": 800}}));
    assert!(plumb(&s, f, 4000.0, 2000.0).is_empty());
}

#[test]
fn a_compound_floors_layers_follow_the_hole_in_3d_and_section() {
    let mut s = Session::new();
    drain(&mut s);
    run(
        &mut s,
        "type",
        json!({"category": "floor", "name": "Tiled", "layers": [{"material": "Ceramic tile", "thickness": 20}, {"material": "Concrete", "thickness": 180}]}),
    );
    let f = id_of(&run(&mut s, "floor", json!({"boundary": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]], "offset": 0, "type": "Tiled"})));
    put(&mut s, "Drain", f, "top", json!([3000, 2000]));
    // In 3D each layer its own mesh, in its own material's colour, the hole through both.
    let shaded: Vec<_> = crate::view3d::shaded(&s.model).into_iter().filter(|x| x.owner() == f).collect();
    let colours: std::collections::BTreeSet<_> = shaded.iter().map(|x| (x.color().0, x.color().1, x.color().2)).collect();
    assert!(shaded.iter().all(|x| matches!(x, crate::view3d::Solid::Mesh { .. })) && colours.len() == 2, "{colours:?}");
    // The section across the hole: the line between the tile and the concrete, 20 down, either side of the hole and not in it.
    let sec = id_of(&run(&mut s, "section", json!({"start": [0, 2000], "end": [6000, 2000]})));
    let view = render::section_fit(&s.model, sec, 1200, 800);
    let d = render::section(&s.model, sec, view);
    let between = |x: f64| {
        d.prims.iter().zip(&d.owners).filter(|(_, o)| **o == Some(f)).any(|(p, _)| match p {
            Prim::Line { pts, .. } => pts.windows(2).any(|w| {
                let (a, b) = (view.mm(w[0]), view.mm(w[1]));
                (a.1 + 20.0).abs() < 2.0 && (b.1 + 20.0).abs() < 2.0 && a.0.min(b.0) <= x && x <= a.0.max(b.0)
            }),
            _ => false,
        })
    };
    assert!(between(1000.0) && between(5000.0), "the layers' line either side");
    assert!(!between(3000.0), "none across the hole");
}

/// A face-hosted family of a solid 200 square from 0 to 20 and a void 100 square from -300 to 10 that cuts `cuts`, listed
/// after the solid (`first`: before it).
fn recess(s: &mut Session, name: &str, cuts: &str, first: bool) {
    let solid = json!({"profile": [[-100, -100], [100, -100], [100, 100], [-100, 100]], "from": 0, "to": 20});
    let void = json!({"void": true, "cuts": cuts, "profile": [[-50, -50], [50, -50], [50, 50], [-50, 50]], "from": -300, "to": 10});
    let forms = if first { json!([void, solid]) } else { json!([solid, void]) };
    run(
        s,
        "family",
        json!({"name": name, "category": "generic", "types": {"A": {}},
               "options": {"work_plane_based": true, "always_vertical": false}, "forms": forms}),
    );
}

/// F10d's rule on a face, as in a wall: a void that cuts `both` cuts the host whatever its place in the list, so one listed
/// after a solid it overlaps still makes its hole in the floor it is placed on; `host` does too, and `forms` never does.
#[test]
fn a_both_void_listed_after_a_solid_it_overlaps_cuts_the_face_it_stands_on() {
    let mut s = Session::new();
    let floor = floor(&mut s);
    for (k, (name, cuts, first, want)) in
        [("Both", "both", false, 1), ("Host", "host", false, 1), ("Forms", "forms", false, 0), ("BothFirst", "both", true, 1)]
            .into_iter()
            .enumerate()
    {
        recess(&mut s, name, cuts, first);
        let f = s.model.families[name].clone();
        let voids = f.face_voids(&f.values("A", &Default::default()).unwrap()).unwrap();
        assert_eq!(voids.len(), want, "{name}: the family's face voids");
        let c = put(&mut s, name, floor, "top", json!([1000 + 1200 * k, 2000]));
        let crate::model::Element::Component(placed) = &s.model.elements[&c] else { panic!("a component") };
        assert_eq!(s.model.face_voids(placed).len(), want, "{name}: placed on the floor's top");
    }
}
