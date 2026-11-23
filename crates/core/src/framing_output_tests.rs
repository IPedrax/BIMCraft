//! A framed wall's members drawn and written out (`framing.rs`): solids of the wall in 3D and sections, cut rectangles in a
//! plan at medium and fine detail, Walls > Framing in Visibility/Graphics, the `framingmember` schedule (the cut list) and
//! `IfcMember`s aggregated to their wall. Each through its output: pixels, the display list, the table, the STEP file.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::graphics::Look;
use crate::model::{Element, Id};
use crate::render::{self, PAPER, Prim, Rgb, View};
use crate::view3d::{Camera, Scene};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn first(v: &Value) -> Id {
    v["ids"][0].as_u64().or(v["id"].as_u64()).unwrap_or_else(|| panic!("no id in {v}"))
}

/// The cabin's 7320 mm wall along x from the origin, 2448 tall, framed a third of 1220 apart from its start, of a type
/// whose structure layer (89 mm of timber) is all of it, or (`clad`) between 13 mm of plaster each side.
fn cabin(s: &mut Session, clad: bool) -> Id {
    let timber = json!({"function": "structure", "material": "Timber", "thickness": 89});
    let plaster = json!({"function": "finish", "material": "Plaster", "thickness": 13});
    let layers = if clad { json!([plaster, timber, plaster]) } else { json!([timber]) };
    run(s, "type", json!({"category": "wall", "name": "2x4", "layers": layers}));
    run(s, "type", json!({"category": "framing", "name": "Cabin", "spacing": 1220.0 / 3.0}));
    let w = first(&run(s, "wall", json!({"points": [[0, 0], [7320, 0]], "type": "2x4", "height": 2448})));
    run(s, "frame", json!({"id": w, "type": "Cabin"}));
    w
}

/// Between the first stud (centred at 19) and the second (406.67), and on the second.
const GAP: f64 = 213.0;
const STUD: f64 = 1220.0 / 3.0;

/// The 3D view looking north square at the wall's face, 0.1 px a mm: what it shows at (x, 0, z), and what it picks there.
fn seen(s: &Session) -> impl Fn(f64, f64) -> Option<Id> + use<> {
    let scene = Scene::new(&s.model);
    let look = Look::of_3d(&s.model);
    let cam = Camera { azimuth: -90.0, elevation: 0.0, scale: 0.1, target: [3660.0, 0.0, 1224.0], width: 800, height: 300, lens: None };
    let image = scene.render_in(&cam, &[], &look);
    move |x, z| {
        let (u, v, _) = cam.project([x, 0.0, z]);
        image.pick(u, v)
    }
}

#[test]
fn in_3d_a_framed_walls_studs_are_its_own_with_nothing_between_them_and_hidden_framing_brings_the_layer_back() {
    let mut s = Session::new();
    let w = cabin(&mut s, false);
    let at = seen(&s);
    assert_eq!(at(STUD, 1200.0), Some(w), "a stud picks its wall");
    assert_eq!(at(GAP, 1200.0), None, "between studs, through the wall");
    assert_eq!(at(GAP, 2448.0 - 40.0), Some(w), "the top plates");
    assert_eq!(at(GAP, 19.0), Some(w), "the bottom plate");
    run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["framing"]}));
    let at = seen(&s);
    assert_eq!(at(GAP, 1200.0), Some(w), "framing hidden, the wall is whole again");
    run(&mut s, "unframe", json!({"id": w}));
    assert_eq!(seen(&s)(GAP, 1200.0), Some(w), "unframed, whole");
}

#[test]
fn the_members_take_the_framing_types_material_and_the_structure_layer_is_never_drawn_with_them() {
    let mut s = Session::new();
    let w = cabin(&mut s, true);
    run(&mut s, "material", json!({"name": "Red Pine", "color": [200, 30, 30]}));
    run(&mut s, "type", json!({"category": "framing", "name": "Cabin", "material": "Red Pine"}));
    let solids = crate::view3d::shaded(&s.model);
    let look = Look::of_3d(&s.model);
    let drawn: Vec<_> = solids.iter().filter(|x| x.owner() == w && look.forms_show(x.vis(), false)).collect();
    let red = drawn.iter().filter(|x| x.color() == Rgb(200, 30, 30)).count();
    let timber = s.model.materials["Timber"].color;
    assert!(red > 20, "the studs and plates in the framing's material: {red}");
    assert!(!drawn.iter().any(|x| x.color() == Rgb(timber[0], timber[1], timber[2])), "no timber layer drawn under them");
    assert!(drawn.iter().any(|x| x.color() != Rgb(200, 30, 30)), "the plaster each side still is");
}

/// The section box's caps that cover plan point (x, y) on its top face at `z`, and their colours.
fn capped(s: &Session, x: f64, y: f64, z: f64) -> Vec<Rgb> {
    let look = Look::of_3d(&s.model);
    let caps = crate::view3d::CapSource::new(&s.model).caps(&s.model, &look, (50.0, crate::graphics::Detail::Fine));
    caps.tris
        .iter()
        .filter(|t| t.p.iter().all(|p| (p[2] - z).abs() < 1e-6))
        .filter(|t| crate::geom::contains(&t.p.map(|p| P2(p[0], p[1])), P2(x, y)))
        .map(|t| t.color)
        .collect()
}

#[test]
fn a_section_box_caps_the_studs_not_the_layer_and_the_3d_mesh_and_gltf_draw_what_the_view_does() {
    let mut s = Session::new();
    cabin(&mut s, true);
    run(&mut s, "section_box", json!({"min": [-1000, -1000, -1000], "max": [8000, 1000, 1200]}));
    assert!(capped(&s, GAP, 0.0, 1200.0).is_empty(), "no cap between studs");
    assert!(!capped(&s, STUD, 0.0, 1200.0).is_empty(), "a stud capped");
    assert!(!capped(&s, GAP, 51.0, 1200.0).is_empty(), "the plaster capped");
    let shown = |s: &Session| {
        let scene = Scene::new(&s.model);
        let look = Look::of_3d(&s.model);
        let mesh = crate::view3d::gpu::Mesh::new(&scene, &look).unwrap().triangles();
        (scene.shown(&look), mesh, crate::gltf::export(&s.model).unwrap().triangles)
    };
    run(&mut s, "section_box", json!({"on": false}));
    let framed = shown(&s);
    run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["framing"]}));
    run(&mut s, "section_box", json!({"min": [-1000, -1000, -1000], "max": [8000, 1000, 1200]}));
    assert!(!capped(&s, GAP, 0.0, 1200.0).is_empty(), "framing hidden: the layer capped");
    run(&mut s, "section_box", json!({"on": false}));
    let whole = shown(&s);
    assert!(framed.0 > whole.0 + 19 * 12, "the members' triangles in place of the layer's: {framed:?} {whole:?}");
    assert_eq!((framed.1, framed.2), (framed.0, framed.0), "the card's mesh and glTF draw what the scene shows");
    assert_eq!((whole.1, whole.2), (whole.0, whole.0));
}

/// A section from `a` to `b` (each [x, y]).
fn section(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Id {
    first(&run(s, "section", json!({"start": a, "end": b})))
}

/// Whether section `sec` fills `wall` at (along, z), mm in its own frame.
fn section_fills(s: &Session, sec: Id, wall: Id) -> impl Fn(f64, f64) -> bool + use<> {
    let view = render::section_fit(&s.model, sec, 1600, 800);
    let d = render::section(&s.model, sec, view);
    let rings: Vec<Vec<P2>> = d
        .prims
        .iter()
        .zip(&d.owners)
        .filter(|(_, o)| **o == Some(wall))
        .flat_map(|(p, _)| match p {
            Prim::Fill { rings, color, .. } if *color != PAPER => rings.iter().map(|r| r.iter().map(|q| view.mm(*q)).collect()).collect(),
            _ => Vec::new(),
        })
        .collect();
    move |x, z| rings.iter().any(|r| crate::geom::contains(r, P2(x, z)))
}

#[test]
fn a_section_along_a_framed_wall_cuts_its_studs_apart_and_one_across_cuts_its_plates() {
    let mut s = Session::new();
    let w = cabin(&mut s, true);
    // Along the middle of the structure layer, from 100 before the wall's start.
    let sec = section(&mut s, [-100.0, 0.0], [7500.0, 0.0]);
    let along = section_fills(&s, sec, w);
    assert!(along(100.0 + STUD, 1200.0), "a stud");
    assert!(!along(100.0 + GAP, 1200.0), "nothing between studs");
    assert!(along(100.0 + GAP, 19.0) && along(100.0 + GAP, 2448.0 - 20.0), "the plates");
    run(&mut s, "graphics", json!({"view": sec, "hide": ["framing"]}));
    assert!(section_fills(&s, sec, w)(100.0 + GAP, 1200.0), "framing hidden, the layer whole again");
    // Across, between two studs, from 500 south: the plaster each side, the plates, and nothing between them.
    let sec = section(&mut s, [GAP, -500.0], [GAP, 500.0]);
    let across = section_fills(&s, sec, w);
    assert!(across(500.0, 19.0) && across(500.0, 2448.0 - 57.0), "the bottom plate and the top plates");
    assert!(!across(500.0, 1200.0), "nothing in the layer between the studs");
    assert!(across(500.0 - 51.0, 1200.0) && across(500.0 + 51.0, 1200.0), "the plaster each side");
    // Across a stud at fine detail, it is cut in the framing type's material's pattern: solid, it is ink.
    let sec = section(&mut s, [STUD, -500.0], [STUD, 500.0]);
    run(&mut s, "detail", json!({"view": sec, "detail": "fine"}));
    let ink = |s: &Session| {
        let view = render::section_fit(&s.model, sec, 1600, 800);
        let pm = tiny_skia::Pixmap::decode_png(&render::section_png(&s.model, sec, view).unwrap()).unwrap();
        let (u, v) = view.px(P2(500.0, 1200.0));
        let dark = (-4..=4).flat_map(|dx| (-4..=4).map(move |dy| (dx, dy)));
        let n = dark.filter(|(dx, dy)| pm.pixel((u as i32 + dx) as u32, (v as i32 + dy) as u32).is_some_and(|c| c.red() < 60)).count();
        n > 70
    };
    assert!(!ink(&s), "timber's diagonal hatch leaves paper between its lines");
    run(&mut s, "material", json!({"name": "Solid Pine", "cut": "solid"}));
    run(&mut s, "type", json!({"category": "framing", "name": "Cabin", "material": "Solid Pine"}));
    assert!(ink(&s), "the framing's own material, solid");
}

/// A plan of Level 1 round the cabin wall's west end: 0.5 px a mm, 1.6 m by 0.6 m (at 1 : 20, so its lines are as thin as
/// a framing plan's: `plan_ink` sets it).
fn plan_view() -> View {
    View { angle: 0.0, scale: 0.5, cx: 800.0, cy: 0.0, width: 800, height: 300 }
}

/// The plan's pixels that aren't paper in the box from model point `lo` to `hi`, the box's edges left out.
fn plan_ink(s: &mut Session) -> impl Fn([f64; 2], [f64; 2]) -> usize + use<> {
    run(s, "scale", json!({"view": "Level 1", "scale": 20}));
    let d = render::draw(&s.model, 100000, plan_view());
    let pm = tiny_skia::Pixmap::decode_png(&render::png(&d).unwrap()).unwrap();
    move |lo, hi| {
        let (a, b) = (plan_view().px(P2(lo[0], lo[1])), plan_view().px(P2(hi[0], hi[1])));
        let (x0, x1) = (a.0.min(b.0).ceil() as u32 + 1, a.0.max(b.0).floor() as u32 - 1);
        let (y0, y1) = (a.1.min(b.1).ceil() as u32 + 1, a.1.max(b.1).floor() as u32 - 1);
        (x0..=x1).flat_map(|x| (y0..=y1).map(move |y| (x, y))).filter(|&(x, y)| pm.pixel(x, y).is_some_and(|c| c.red() < 250)).count()
    }
}

/// Inside the structure layer (44.5 each side of the wall's line), across the stud centred at `c` and between `a` and `b`.
fn stud_box(c: f64) -> ([f64; 2], [f64; 2]) {
    ([c - 19.0, -44.5], [c + 19.0, 44.5])
}

fn gap_box(a: f64, b: f64) -> ([f64; 2], [f64; 2]) {
    ([a, -35.0], [b, 35.0])
}

#[test]
fn a_plan_at_fine_and_medium_detail_cuts_each_stud_in_the_structure_layer_and_at_coarse_draws_the_wall_as_before() {
    let mut s = Session::new();
    cabin(&mut s, true);
    let (stud, gap) = (stud_box(STUD), gap_box(38.0 + 15.0, STUD - 19.0 - 15.0));
    for detail in ["fine", "medium"] {
        run(&mut s, "detail", json!({"view": "Level 1", "detail": detail}));
        let ink = plan_ink(&mut s);
        assert!(ink(stud.0, stud.1) > 100, "{detail}: a stud, cut: {}", ink(stud.0, stud.1));
        assert_eq!(ink(gap.0, gap.1), 0, "{detail}: nothing between studs");
        assert!(ink([GAP - 10.0, 46.0], [GAP + 10.0, 56.0]) > 0, "{detail}: the plaster");
    }
    run(&mut s, "detail", json!({"view": "Level 1", "detail": "coarse"}));
    let ink = plan_ink(&mut s);
    let all = |b: ([f64; 2], [f64; 2])| {
        let (a, c) = (plan_view().px(P2(b.0[0], b.0[1])), plan_view().px(P2(b.1[0], b.1[1])));
        ((c.0 - a.0).abs().floor() as usize - 1) * ((c.1 - a.1).abs().floor() as usize - 1)
    };
    assert!(ink(gap.0, gap.1) * 10 >= all(gap) * 9, "coarse: the wall poché between studs, as an unframed wall's");
    // Framing hidden: the layer as an unframed wall's.
    run(&mut s, "detail", json!({"view": "Level 1", "detail": "medium"}));
    run(&mut s, "graphics", json!({"view": "Level 1", "hide": ["framing"]}));
    let ink = plan_ink(&mut s);
    assert!(ink(gap.0, gap.1) * 10 >= all(gap) * 9, "framing hidden: the structure layer's poché");
}

#[test]
fn a_plan_cuts_the_kings_and_jacks_beside_a_window_and_leaves_its_opening_open() {
    let mut s = Session::new();
    let w = cabin(&mut s, true);
    run(&mut s, "type", json!({"category": "window", "name": "1200 x 1200", "width": 1200, "height": 1200, "sill": 832}));
    run(&mut s, "window", json!({"wall": w, "offset": 1250, "type": "1200 x 1200"}));
    run(&mut s, "detail", json!({"view": "Level 1", "detail": "fine"}));
    let ink = plan_ink(&mut s);
    // The rough opening from 637.5 to 1862.5: the jack beside it, the king beside that, and the gap before them.
    for (what, c) in [("the left jack", 637.5 - 19.0), ("the left king", 637.5 - 57.0)] {
        let b = stud_box(c);
        assert!(ink(b.0, b.1) > 100, "{what}: {}", ink(b.0, b.1));
    }
    let gap = gap_box(STUD + 19.0 + 15.0, 637.5 - 76.0 - 15.0);
    assert_eq!(ink(gap.0, gap.1), 0, "between the last stud and the king");
}

#[test]
fn walls_have_a_framing_subcategory_off_with_its_wall() {
    let subs: Vec<&str> = crate::graphics::subcategories("wall").map(|s| s.key).collect();
    // The curtain walls' panels and mullions are subcategories of walls as well.
    assert!(subs.contains(&"framing"), "{subs:?}");
    let mut s = Session::new();
    run(&mut s, "graphics", json!({"view": "Level 1", "hide": ["wall"]}));
    let g = crate::graphics::graphics_of(&s.model, 100000).unwrap();
    assert!(g.hides("framing"), "hidden with its category");
}

fn table(r: &Value) -> (Vec<String>, Vec<Vec<Value>>) {
    let cols = r["columns"].as_array().unwrap().iter().map(|c| c["heading"].as_str().or(c.as_str()).unwrap().to_string()).collect();
    (cols, r["rows"].as_array().unwrap().iter().map(|r| r.as_array().unwrap().clone()).collect())
}

#[test]
fn a_schedule_of_framing_members_grouped_by_role_section_and_length_is_the_cut_list() {
    let mut s = Session::new();
    let w = cabin(&mut s, false);
    let r = run(
        &mut s,
        "schedule",
        json!({"name": "Cut List", "category": "framingmembers", "fields": ["Role", "Section", "Length", "Stock Length", "Count"],
               "sort": ["Role", "Section", "Length"], "grouped": true}),
    );
    let (cols, rows) = table(&r);
    assert_eq!(cols, ["Role", "Section", "Length", "Stock Length", "Count"]);
    let row = |role: &str| rows.iter().find(|r| r[0] == role).unwrap_or_else(|| panic!("no {role}: {rows:?}")).clone();
    assert_eq!(row("stud")[1..], [json!("38 x 89"), json!(2334.0), json!(2440.0), json!(19.0)]);
    // 7320 long, each plate spliced in two, each piece from stock.
    for (plate, pieces) in [("top_plate", 4.0), ("bottom_plate", 2.0)] {
        let these: Vec<&Vec<Value>> = rows.iter().filter(|r| r[0] == plate).collect();
        assert!(these.iter().all(|r| r[3].as_f64().is_some_and(|st| st > 0.0)), "{these:?}");
        assert_eq!(these.iter().filter_map(|r| r[4].as_f64()).sum::<f64>(), pieces, "{these:?}");
    }
    assert!(rows.iter().all(|r| r[0] == "stud" || r[0] == "top_plate" || r[0] == "bottom_plate"), "{rows:?}");
    // Each row lists its members' wall, which picks it.
    let elements = r["elements"].as_array().unwrap();
    assert!(elements.iter().all(|ids| ids.as_array().unwrap().iter().all(|id| id.as_u64() == Some(w))), "{elements:?}");
    // One row a member: its wall, its types, its key and its level.
    let r = run(
        &mut s,
        "schedule",
        json!({"name": "Members", "category": "framingmember",
               "fields": ["Host", "Host Type", "Framing Type", "Role", "Section", "Level", "Key"],
               "filters": [{"field": "Key", "op": "equals", "value": "stud/1"}]}),
    );
    let (_, rows) = table(&r);
    assert_eq!(
        rows,
        [vec![json!(w as f64), json!("2x4"), json!("Cabin"), json!("stud"), json!("38 x 89"), json!("Level 1"), json!("stud/1")]]
    );
    // A two-ply header is said so.
    run(&mut s, "type", json!({"category": "window", "name": "1200 x 1200", "width": 1200, "height": 1200, "sill": 832}));
    run(&mut s, "window", json!({"wall": w, "offset": 1250, "type": "1200 x 1200"}));
    let r = run(&mut s, "schedule", json!({"name": "Members", "filters": [{"field": "Role", "op": "equals", "value": "header"}]}));
    assert_eq!(table(&r).1[0][4], json!("2 x 38 x 184"));
    // Not a category a multi-category schedule lists.
    let e = s.execute("schedule", &json!({"name": "Mixed", "category": "multicategory", "categories": ["framingmembers"]})).unwrap_err();
    assert!(e.contains("isn't a category a multi-category schedule lists"), "{e}");
}

/// The entities of `class` in a STEP file: each line's text.
fn of_class<'a>(f: &'a str, class: &str) -> Vec<&'a str> {
    f.lines().filter(|l| l.split_once('=').is_some_and(|(_, e)| e.starts_with(&format!("{class}(")))).collect()
}

/// The `#n` a STEP line defines.
fn ref_of(line: &str) -> &str {
    line.split_once('=').map_or("", |(r, _)| r)
}

#[test]
fn each_member_goes_to_ifc_as_a_member_of_its_wall_with_a_global_id_its_key_keeps() {
    let mut s = Session::new();
    let w = cabin(&mut s, true);
    run(&mut s, "type", json!({"category": "window", "name": "1200 x 1200", "width": 1200, "height": 1200, "sill": 832}));
    let win = first(&run(&mut s, "window", json!({"wall": w, "offset": 1250, "type": "1200 x 1200"})));
    let n = run(&mut s, "framing", json!({"id": w}))["members"].as_array().unwrap().len();
    let f = crate::ifc::export(&s.model);
    let members = of_class(&f, "IFCMEMBER");
    assert_eq!(members.len(), n);
    let kind = |k: &str| members.iter().filter(|l| l.ends_with(&format!(",.{k}.);"))).count();
    let studs = ["stud", "king", "jack", "cripple"];
    let plates = ["bottom_plate", "top_plate", "sill"];
    assert_eq!(kind("STUD"), members.iter().filter(|l| studs.iter().any(|r| l.contains(&format!("'{r}'")))).count());
    assert_eq!(kind("PLATE"), members.iter().filter(|l| plates.iter().any(|r| l.contains(&format!("'{r}'")))).count());
    assert_eq!(kind("MEMBER"), 1, "the header");
    assert_eq!(kind("STUD") + kind("PLATE") + kind("MEMBER"), n);
    // Aggregated to the wall, which keeps its own body; in no storey of their own.
    let wall = of_class(&f, "IFCWALL");
    assert_eq!(wall.len(), 1);
    let parts =
        of_class(&f, "IFCRELAGGREGATES").into_iter().find(|l| l.contains(&format!(",{},(", ref_of(wall[0])))).expect("the wall's parts");
    assert!(members.iter().all(|m| parts.contains(&format!("{},", ref_of(m))) || parts.contains(&format!("{})", ref_of(m)))));
    assert!(
        !of_class(&f, "IFCRELCONTAINEDINSPATIALSTRUCTURE").iter().any(|l| members.iter().any(|m| l.contains(&format!("{},", ref_of(m)))))
    );
    // Each in the framing's material.
    assert!(of_class(&f, "IFCRELASSOCIATESMATERIAL").iter().any(|l| members.iter().take(3).all(|m| l.contains(ref_of(m)))));
    // Their GlobalIds: one each, and by key: moving the window keeps the far studs'.
    let ids =
        |f: &str| -> Vec<String> { of_class(f, "IFCMEMBER").iter().map(|l| l.split('\'').nth(1).unwrap_or("").to_string()).collect() };
    let before = ids(&f);
    assert_eq!(before.iter().collect::<std::collections::BTreeSet<_>>().len(), n, "one GlobalId each");
    let named = |f: &str, key: &str| {
        of_class(f, "IFCMEMBER").iter().find(|l| l.contains(&format!("'{key}'"))).map(|l| l.split('\'').nth(1).unwrap_or("").to_string())
    };
    let far = named(&f, "stud/15").expect("a stud far from the window, tagged by its key");
    run(&mut s, "move", json!({"ids": [win], "by": [300, 0]}));
    let f2 = crate::ifc::export(&s.model);
    assert_eq!(named(&f2, "stud/15"), Some(far));
    // IFC2x3: an IfcMember without a predefined type.
    let old = crate::ifc::export_as(&s.model, Default::default(), crate::ifc::Schema::Ifc2x3);
    let m2 = of_class(&old, "IFCMEMBER");
    assert_eq!(m2.len(), of_class(&f2, "IFCMEMBER").len());
    assert!(m2.iter().all(|l| !l.contains(".STUD.") && !l.contains(".PLATE.")), "{}", m2[0]);
}

#[test]
fn a_framed_walls_ifc_opens_as_one_wall_and_its_window() {
    let mut s = Session::new();
    let w = cabin(&mut s, true);
    run(&mut s, "type", json!({"category": "window", "name": "1200 x 1200", "width": 1200, "height": 1200, "sill": 832}));
    run(&mut s, "window", json!({"wall": w, "offset": 1250, "type": "1200 x 1200"}));
    let i = crate::import::ifc(crate::ifc::export(&s.model).as_bytes(), "back").unwrap();
    let count = |c: &str| i.model.elements.values().filter(|e| e.category() == c).count();
    assert_eq!((count("wall"), count("window"), count("beam"), count("component")), (1, 1, 0, 0), "skipped: {:?}", i.skipped);
}

#[test]
fn a_framed_walls_json_names_its_framing_type_and_keeps_its_own_types_guid() {
    let mut s = Session::new();
    let w = cabin(&mut s, false);
    let e = crate::commands::element_json(&s.model, w);
    assert_eq!(e["framing"], "Cabin");
    let guid = |kind: &str, name: &str| json!(s.model.library.guid(&crate::library::Kind::Type(kind.into()), name).unwrap());
    assert_eq!(e["type_guid"], guid("wall", "2x4"), "the wall type's, not the framing type's");
    assert_eq!(e["framing_guid"], guid("framing", "Cabin"));
    let q = run(&mut s, "elements", json!({"category": "wall"}));
    assert!(q.to_string().contains("\"framing\":\"Cabin\""), "{q}");
    assert!(matches!(s.model.elements.get(w), Some(Element::Wall(_))));
}
