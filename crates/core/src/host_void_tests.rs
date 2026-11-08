//! A hosted family's voids cut its host: a door's or a window's void that cuts none of its own forms (one listed before
//! them all, say) is the hole it makes in its wall, in the wall's solids (so the 3D view and sections), in plan where the cut
//! plane passes through it, and as the body of the IFC opening, in place of the rectangle of its width and height. A family
//! with no such void still cuts the rectangle.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::Id;
use crate::render::{self, POCHE, Prim, View};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn id_of(v: Value) -> Id {
    v["id"].as_u64().unwrap()
}

/// An arched window: an opening 1000 wide and 1500 tall, whose void is a rectangle 1000 tall under a half circle of radius
/// 500, `Void Depth` (an instance length, 400 unless given) deep across the wall, listed first so it cuts none of the
/// window's own forms; then a sill board.
fn arched(s: &mut Session) {
    let mut profile = vec![json!([-500, 0]), json!([500, 0])];
    for k in 0..=16 {
        let a = std::f64::consts::PI * f64::from(k) / 16.0;
        profile.push(json!([(500.0 * a.cos()).round(), (1000.0 + 500.0 * a.sin()).round()]));
    }
    run(
        s,
        "family",
        json!({"name": "Arched", "category": "window", "types": {"A": {}},
            "parameters": [{"name": "Width", "value": 1000}, {"name": "Height", "value": 1500}, {"name": "Sill Height", "value": 900},
                {"name": "Void Depth", "instance": true, "value": 400}],
            "forms": [
                {"void": true, "plane": "xz", "profile": profile, "from": "-Void Depth / 2", "to": "Void Depth / 2"},
                {"profile": [[-500, -40], [500, -40], [500, 40], [-500, 40]], "from": 0, "to": 40, "material": ""}
            ],
            "symbols": [{"line": [[-500, 0], [500, 0]], "subcategory": "Opening"}]}),
    );
}

/// A wall along x from 0 to 6000 of `thickness` mm, 3000 tall, and an arched window in its middle.
fn wall_with(s: &mut Session, thickness: f64, family: &str) -> (Id, Id) {
    let ty = format!("Test {thickness}");
    run(s, "type", json!({"category": "wall", "name": ty, "thickness": thickness}));
    let wall = run(s, "wall", json!({"points": [[0, 0], [6000, 0]], "type": ty, "height": 3000}))["ids"][0].as_u64().unwrap();
    let window = id_of(run(s, "window", json!({"at": [3000, 0], "family": family})));
    (wall, window)
}

/// Whether the wall is filled in the section along it `across` mm off its middle at (x, z): a point inside one of its fills.
fn wall_filled_at(s: &mut Session, wall: Id, across: f64) -> impl Fn(f64, f64) -> bool + use<> {
    let sec = id_of(run(s, "section", json!({"start": [0, across], "end": [6000, across]})));
    let view = render::section_fit(&s.model, sec, 1200, 800);
    let d = render::section(&s.model, sec, view);
    let rings: Vec<Vec<P2>> = d
        .prims
        .iter()
        .zip(&d.owners)
        .filter(|(_, o)| **o == Some(wall))
        .flat_map(|(p, _)| match p {
            Prim::Fill { rings, .. } => rings.iter().map(|r| r.iter().map(|q| view.mm(*q)).collect()).collect(),
            _ => Vec::new(),
        })
        .collect();
    run(s, "delete", json!({"ids": [sec]}));
    move |x, z| rings.iter().any(|r| crate::geom::contains(r, P2(x, z)))
}

#[test]
fn an_arched_windows_void_cuts_its_wall_in_its_own_shape() {
    let mut s = Session::new();
    arched(&mut s);
    let (wall, _) = wall_with(&mut s, 200.0, "Arched");
    let at = wall_filled_at(&mut s, wall, 10.0);
    assert!(at(1000.0, 1500.0) && at(3000.0, 500.0), "the wall round it");
    assert!(!at(3000.0, 1500.0) && !at(3000.0, 2300.0), "the hole, up into the arch");
    // Off the arch's middle, over its curve but under the rectangle's head: wall, where the rectangle would have cut it.
    assert!(at(2550.0, 2300.0), "the arch's haunch is wall");
    assert!(!at(2550.0, 2050.0), "under the curve, the hole");
    // The section's picture: poché at the haunch.
    let sec = id_of(run(&mut s, "section", json!({"start": [0, 10], "end": [6000, 10]})));
    let view = render::section_fit(&s.model, sec, 1200, 800);
    let pm = tiny_skia::Pixmap::decode_png(&render::section_png(&s.model, sec, view).unwrap()).unwrap();
    let px = |x: f64, z: f64| {
        let (u, v) = view.px(P2(x, z));
        let c = pm.pixel(u as u32, v as u32).unwrap();
        (c.red(), c.green(), c.blue())
    };
    assert_eq!(px(2550.0, 2300.0), (POCHE.0, POCHE.1, POCHE.2));
    assert_ne!(px(3000.0, 2300.0), (POCHE.0, POCHE.1, POCHE.2));
}

#[test]
fn the_wall_solids_round_a_void_are_its_own_shape_in_3d() {
    let mut s = Session::new();
    arched(&mut s);
    let (wall, _) = wall_with(&mut s, 200.0, "Arched");
    let solids = crate::view3d::solids(&s.model);
    let mine: Vec<_> = solids.iter().filter(|x| x.owner() == wall).collect();
    assert!(mine.iter().any(|x| matches!(x, crate::view3d::Solid::Mesh { cut: true, .. })), "cut by the void, a mesh a section cuts");
    // Up through the wall's middle at the haunch: solid to the top; at the arch's crown, open from the sill to the crown.
    let spans = |x: f64| {
        // A little off the void's corners and the wall's middle, so no ray runs down an edge.
        let q = P2(x + 0.37, 13.1);
        let mut zs: Vec<(f64, f64)> = Vec::new();
        for solid in &mine {
            let tris = match solid {
                crate::view3d::Solid::Mesh { tris, .. } => tris.clone(),
                _ => continue,
            };
            let mut hits: Vec<f64> = tris
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
            zs.extend(hits.as_chunks::<2>().0.iter().map(|[a, b]| (*a, *b)));
        }
        zs
    };
    let crown = spans(3000.0);
    assert!(crown.iter().any(|(lo, hi)| (*hi - 900.0).abs() < 1.0 && *lo < 1.0), "wall under the sill: {crown:?}");
    assert!(crown.iter().any(|(lo, _)| (*lo - 2400.0).abs() < 1.0), "wall over the crown at 2400: {crown:?}");
    let haunch = spans(2550.0);
    assert!(haunch.iter().any(|(lo, _)| (*lo - 2118.0).abs() < 5.0), "wall over the curve at about 2118: {haunch:?}");
}

#[test]
fn a_void_deeper_than_the_wall_cuts_right_through_and_an_instance_depth_follows() {
    let mut s = Session::new();
    arched(&mut s);
    let (wall, window) = wall_with(&mut s, 500.0, "Arched");
    // 400 deep in a wall 500 thick: a niche 200 each way of its middle; near the faces, wall.
    let near_face = wall_filled_at(&mut s, wall, 230.0);
    assert!(near_face(3000.0, 1500.0), "400 deep leaves the wall's outer 50");
    assert!(!wall_filled_at(&mut s, wall, 0.0)(3000.0, 1500.0), "and its middle open");
    // This window's own depth past the wall's thickness: right through it.
    run(&mut s, "modify", json!({"id": window, "params": {"Void Depth": 700}}));
    assert!(!wall_filled_at(&mut s, wall, 230.0)(3000.0, 1500.0), "700 deep cuts through 500");
    assert!(!wall_filled_at(&mut s, wall, -230.0)(3000.0, 1500.0), "both faces");
    // Another window of the same family, its own depth as it was: a niche still.
    run(&mut s, "window", json!({"at": [5000, 0], "family": "Arched"}));
    let at = wall_filled_at(&mut s, wall, 230.0);
    assert!(!at(3000.0, 1500.0) && at(5000.0, 1500.0), "each window its own depth");
}

#[test]
fn a_plan_cuts_the_wall_where_the_void_is_at_its_cut_plane() {
    let mut s = Session::new();
    arched(&mut s);
    let (_, window) = wall_with(&mut s, 500.0, "Arched");
    let view = View { scale: 0.5, cx: 3000.0, cy: 0.0, width: 1200, height: 1200 };
    let pixel = |s: &Session, p: P2| {
        let pm = tiny_skia::Pixmap::decode_png(&render::png(&render::draw(&s.model, 100000, view)).unwrap()).unwrap();
        let (x, y) = view.px(p);
        let c = pm.pixel(x as u32, y as u32).unwrap();
        render::Rgb(c.red(), c.green(), c.blue())
    };
    // A niche 400 deep in 500: the wall's middle open, poché left near its faces; away from the window, all poché.
    assert_eq!(pixel(&s, P2(1000.0, 0.0)), POCHE);
    assert_ne!(pixel(&s, P2(3000.0, 0.0)), POCHE, "the void's place in the wall is open");
    assert_eq!(pixel(&s, P2(3000.0, 230.0)), POCHE, "past the niche, wall");
    run(&mut s, "modify", json!({"id": window, "params": {"Void Depth": 700}}));
    assert_ne!(pixel(&s, P2(3000.0, 230.0)), POCHE, "through, open to the face");
    // Nothing past the wall's faces is cleared: what lies there still shows.
    let d = render::draw(&s.model, 100000, view);
    let paper_past = d
        .prims
        .iter()
        .zip(&d.owners)
        .filter(|(p, o)| {
            **o == Some(window)
                && matches!(p, Prim::Fill { rings, color } if *color == render::PAPER
                    && rings.iter().flatten().any(|q| view.mm(*q).1.abs() > 260.0))
        })
        .count();
    assert_eq!(paper_past, 0, "the gap stops at the wall's faces");
}

/// The STEP text of the project as IFC.
fn ifc(s: &mut Session) -> String {
    let path = std::env::temp_dir().join(format!("bimcraft-host-void-{}-{:?}.ifc", std::process::id(), std::thread::current().id()));
    run(s, "save", json!({"path": path}));
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    text
}

/// The entity `#n` of STEP text `text`.
fn entity<'a>(text: &'a str, n: &str) -> &'a str {
    text.lines().find(|l| l.starts_with(&format!("{n}="))).unwrap_or_else(|| panic!("no {n}"))
}

/// The body items of the opening element: the entity types its representation's items are.
fn opening_items(text: &str) -> Vec<String> {
    let opening = text.lines().find(|l| l.contains("IFCOPENINGELEMENT(") && l.contains("'Opening'")).unwrap();
    // IFCOPENINGELEMENT(guid,$,'Opening',$,$,#place,#shape,…)
    let args: Vec<&str> = opening.split(',').collect();
    let shape = args[6].trim();
    let reps = entity(text, shape);
    let rep = reps.split("(#").nth(1).unwrap().split(')').next().unwrap();
    let rep = entity(text, &format!("#{rep}"));
    // …,'Body','Tessellation',(#a,#b)); its items, the last list.
    let items = rep.rsplit('(').next().unwrap().split(')').next().unwrap();
    items
        .split(',')
        .map(|r| {
            let e = entity(text, r.trim());
            e.split('=').nth(1).unwrap().split('(').next().unwrap().to_string()
        })
        .collect()
}

#[test]
fn the_ifc_openings_body_is_the_void() {
    let mut s = Session::new();
    arched(&mut s);
    wall_with(&mut s, 200.0, "Arched");
    let text = ifc(&mut s);
    let items = opening_items(&text);
    assert_eq!(items, ["IFCTRIANGULATEDFACESET"], "the void's own shape: {items:?}");
    assert_eq!(text.matches("IFCRELVOIDSELEMENT(").count(), 1);
    assert_eq!(text.matches("IFCRELFILLSELEMENT(").count(), 1);
}

#[test]
fn a_family_with_no_void_of_its_own_still_cuts_the_rectangle() {
    for family in ["Fixed Window", "Casement Window"] {
        let mut s = Session::new();
        let (wall, _) = wall_with(&mut s, 200.0, family);
        // Its opening 1200 x 1500 at a sill of 900: open right up to its head's corners.
        let at = wall_filled_at(&mut s, wall, 10.0);
        assert!(!at(2450.0, 2350.0) && !at(3000.0, 1500.0), "{family}: the rectangle");
        assert!(at(2350.0, 1500.0) && at(3000.0, 2450.0), "{family}: wall round it");
        let solids = crate::view3d::solids(&s.model);
        assert!(solids.iter().filter(|x| x.owner() == wall).all(|x| matches!(x, crate::view3d::Solid::Prism { .. })), "{family}");
        let items = opening_items(&ifc(&mut s));
        assert_eq!(items, ["IFCEXTRUDEDAREASOLID"], "{family}: {items:?}");
    }
}

#[test]
fn a_hosted_familys_void_that_cuts_none_of_its_forms_is_no_longer_said_to_cut_nothing() {
    let mut s = Session::new();
    arched(&mut s);
    let r = run(&mut s, "family", json!({"name": "Arched"}));
    let warnings = r["warnings"].to_string();
    assert!(!warnings.contains("cuts nothing"), "{warnings}");
    // A generic model's still is.
    let r = s
        .execute(
            "family",
            &json!({"name": "Lone", "category": "generic", "types": {"A": {}}, "forms": [
                {"void": true, "profile": [[0, 0], [100, 0], [100, 100], [0, 100]], "to": 100},
                {"profile": [[500, 0], [600, 0], [600, 100], [500, 100]], "to": 100}
            ]}),
        )
        .unwrap();
    assert!(r["warnings"].to_string().contains("cuts nothing"), "{r}");
}

#[test]
fn a_compound_walls_layers_follow_the_void_in_3d_and_section() {
    let mut s = Session::new();
    arched(&mut s);
    let wall =
        run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "type": "Exterior - 300mm", "height": 3000}))["ids"][0].as_u64().unwrap();
    let window = id_of(run(&mut s, "window", json!({"at": [3000, 0], "family": "Arched"})));
    run(&mut s, "modify", json!({"id": window, "params": {"Void Depth": 600}}));
    let layers = s.model.type_layers("wall", "Exterior - 300mm").iter().filter(|l| l.thickness > 0.0).count();
    assert!(layers > 1);
    // Each layer of the pieces the void cuts its own mesh, in its own material's colour (the wall, all one straight piece,
    // is all cut).
    let shaded = crate::view3d::shaded(&s.model);
    let cut: Vec<_> = shaded.iter().filter(|x| x.owner() == wall && matches!(x, crate::view3d::Solid::Mesh { .. })).collect();
    let colours: std::collections::BTreeSet<_> = cut.iter().map(|x| (x.color().0, x.color().1, x.color().2)).collect();
    let materials: std::collections::BTreeSet<_> =
        s.model.type_layers("wall", "Exterior - 300mm").iter().filter(|l| l.thickness > 0.0).map(|l| l.material.clone()).collect();
    assert!(cut.len() >= layers && colours.len() == materials.len(), "{} pieces in {colours:?}", cut.len());
    // The arch is open through every layer: the section along each face's side still finds the hole and the haunch.
    for across in [-120.0, 0.0, 120.0] {
        let at = wall_filled_at(&mut s, wall, across);
        assert!(!at(3000.0, 2300.0) && at(2550.0, 2300.0), "{across}");
    }
}

/// F10d: a window whose opening's void is listed after its frame, a box round the opening 80 deep it overlaps: `cuts`
/// as given (none: both).
fn framed(s: &mut Session, cuts: Option<&str>) {
    let mut void =
        json!({"void": true, "plane": "xz", "profile": [[-500, 0], [500, 0], [500, 1500], [-500, 1500]], "from": -300, "to": 300});
    if let Some(c) = cuts {
        void["cuts"] = json!(c);
    }
    run(
        s,
        "family",
        json!({"name": "Framed", "category": "window", "types": {"A": {}},
            "parameters": [{"name": "Width", "value": 1000}, {"name": "Height", "value": 1500}, {"name": "Sill Height", "value": 900}],
            "forms": [
                {"plane": "xz", "profile": [[-500, 0], [500, 0], [500, 50], [-500, 50]], "from": -40, "to": 40, "material": ""},
                void
            ],
            "symbols": [{"line": [[-500, 0], [500, 0]], "subcategory": "Opening"}]}),
    );
}

/// What window `id`'s family builds of it: its pieces' triangles, in all.
fn built_tris(s: &Session, id: Id) -> usize {
    let e = s.model.elements.get(id).unwrap();
    s.model.opening_pieces(e).iter().map(|p| p.tris.len()).sum()
}

#[test]
fn a_hosted_familys_void_cuts_the_wall_whatever_its_place_in_the_list() {
    // F10d: listed after the sill member it overlaps, the void still cuts the wall (before, only one listed before every
    // form it overlaps did), and still cuts the member, as a void cuts the forms listed before it.
    let mut s = Session::new();
    framed(&mut s, None);
    let f = s.model.families["Framed"].clone();
    let values = f.values("A", &Default::default()).unwrap();
    assert_eq!(f.host_voids(&values).unwrap().len(), 1, "it cuts the wall");
    let (wall, window) = wall_with(&mut s, 200.0, "Framed");
    let at = wall_filled_at(&mut s, wall, 10.0);
    assert!(!at(3000.0, 1500.0) && at(2000.0, 1500.0), "the void's hole in the wall");
    assert_eq!(built_tris(&s, window), 0, "the member, wholly inside the void, is cut away");
    // `host`: the wall alone; the member stands.
    let mut s = Session::new();
    framed(&mut s, Some("host"));
    let (wall, window) = wall_with(&mut s, 200.0, "Framed");
    let at = wall_filled_at(&mut s, wall, 10.0);
    assert!(!at(3000.0, 1500.0) && at(2000.0, 1500.0), "host: the hole");
    assert!(built_tris(&s, window) >= 12, "host: the member stands");
    let r = run(&mut s, "family", json!({"name": "Framed"}));
    assert!(!r["warnings"].to_string().contains("cuts nothing"), "{r}");
    // `forms`: the member alone; the wall keeps the rectangle.
    let mut s = Session::new();
    framed(&mut s, Some("forms"));
    let f = s.model.families["Framed"].clone();
    assert!(f.host_voids(&f.values("A", &Default::default()).unwrap()).unwrap().is_empty(), "forms: not the wall");
    let (_, window) = wall_with(&mut s, 200.0, "Framed");
    assert_eq!(built_tris(&s, window), 0, "forms: the member cut away");
    let items = opening_items(&ifc(&mut s));
    assert_eq!(items, ["IFCEXTRUDEDAREASOLID"], "forms: the rectangle, {items:?}");
}

#[test]
fn cuts_is_a_voids_and_a_file_of_31_keeps_what_its_voids_cut() {
    let mut s = Session::new();
    let e = s
        .execute(
            "family",
            &json!({"name": "Solid", "category": "generic", "types": {"A": {}}, "forms": [
                {"profile": [[0, 0], [100, 0], [100, 100], [0, 100]], "to": 100, "cuts": "host"}
            ]}),
        )
        .unwrap_err();
    assert!(e.contains("`cuts` says what a void cuts"), "{e}");
    // FORMAT 31 to 32: a file of 31 whose void, listed after the member it overlaps, cut the member and not the wall, reads
    // `cuts: forms` and cuts what it did; one listed first, which cut the wall, reads as it was.
    framed(&mut s, None);
    arched(&mut s);
    let mut file: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    file["format"] = json!(31);
    let dir = std::env::temp_dir().join(format!("bimcraft-f10d-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("old.bimcraft");
    std::fs::write(&path, file.to_string()).unwrap();
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    let _ = std::fs::remove_dir_all(&dir);
    let framed = &t.model.families["Framed"];
    assert_eq!(framed.forms[1].cuts, Some(crate::family::Cuts::Forms));
    assert!(framed.host_voids(&framed.values("A", &Default::default()).unwrap()).unwrap().is_empty());
    let arched = &t.model.families["Arched"];
    assert_eq!(arched.forms[0].cuts, None, "the arch's void cut the wall in 31 too");
    assert_eq!(arched.host_voids(&arched.values("A", &Default::default()).unwrap()).unwrap().len(), 1);
    let saved: Value = serde_json::from_str(&t.model.to_json().unwrap()).unwrap();
    assert_eq!(saved["format"], json!(crate::model::FORMAT));
}
