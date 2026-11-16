//! A framed floor's members drawn and written out (`framing/floor.rs`): solids of the floor in 3D, section-box caps and
//! sections, hidden lines in a plan where it shows Floors > Framing Lines, Floors > Framing in Visibility/Graphics, the
//! `framingmember` schedule (in `framing_floor_tests`) and `IfcBeam`s aggregated to their slab. Each through its output.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::framing_floor_tests::{FE, FN, FS, FW, YL, cabin};
use crate::geom::P2;
use crate::graphics::Look;
use crate::model::Id;
use crate::render::{self, PAPER, Prim, Rgb};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn first(v: &Value) -> Id {
    v["ids"][0].as_u64().or(v["id"].as_u64()).unwrap_or_else(|| panic!("no id in {v}"))
}

/// The second joist from the west (its centre), and the middle of the bay between it and the edge joist.
const JOIST: f64 = FW + 1220.0 / 3.0;
const BAY: f64 = FW + 213.0;
/// Halfway up the joists.
const MID: f64 = -110.0;

/// The section box's caps covering plan point (x, y) on its top face at `z`, and their colours.
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
fn in_3d_a_framed_floors_joists_are_its_own_in_their_material_and_hidden_framing_brings_its_layer_back() {
    let mut s = Session::new();
    let id = cabin(&mut s);
    run(&mut s, "material", json!({"name": "Red Pine", "color": [200, 30, 30]}));
    run(&mut s, "type", json!({"category": "framing", "name": "Cabin Floor", "material": "Red Pine"}));
    let drawn = |s: &Session| {
        let look = Look::of_3d(&s.model);
        crate::view3d::shaded(&s.model).into_iter().filter(|x| x.owner() == id && look.forms_show(x.vis(), false)).collect::<Vec<_>>()
    };
    let solids = drawn(&s);
    let red = solids.iter().filter(|x| x.color() == Rgb(200, 30, 30)).count();
    assert_eq!(red, 4 + 38, "the rims (each spliced in two) and the joists, the floor's, in the framing's material");
    let timber = s.model.materials["Timber"].color;
    assert_eq!(
        solids.iter().filter(|x| x.color() == Rgb(timber[0], timber[1], timber[2])).count(),
        1,
        "the subfloor only, not the joists' layer"
    );
    // A section box through the joists caps them and nothing between them.
    run(&mut s, "section_box", json!({"min": [-1000, -1000, -1000], "max": [8000, 7000, MID]}));
    assert!(!capped(&s, JOIST, 1500.0, MID).is_empty(), "a joist capped");
    assert!(capped(&s, BAY, 1500.0, MID).is_empty(), "nothing between joists");
    run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["floorframing"]}));
    assert!(!capped(&s, BAY, 1500.0, MID).is_empty(), "framing hidden: the layer capped whole");
    run(&mut s, "section_box", json!({"on": false}));
    let whole = drawn(&s);
    assert!(!whole.iter().any(|x| x.color() == Rgb(200, 30, 30)), "framing hidden: no members");
    assert_eq!(whole.len(), 2, "the floor's two layers, as an unframed floor's");
    // Walls > Framing is another subcategory: hiding it leaves the floor's joists.
    run(&mut s, "graphics", json!({"view": "{3D}", "show": ["floorframing"], "hide": ["framing"]}));
    assert_eq!(drawn(&s).iter().filter(|x| x.color() == Rgb(200, 30, 30)).count(), 42);
}

#[test]
fn seen_from_below_the_joists_pick_the_floor_and_the_mesh_and_gltf_draw_what_the_view_does() {
    let mut s = Session::new();
    let id = cabin(&mut s);
    // Below the beams, looking up at the bay between the edge joist and the next.
    let cam = crate::view3d::Camera {
        azimuth: -90.0,
        elevation: -60.0,
        scale: 0.1,
        target: [3600.0, 3000.0, -200.0],
        width: 900,
        height: 700,
        lens: None,
    };
    let scene = crate::view3d::Scene::new(&s.model);
    let look = Look::of_3d(&s.model);
    let image = scene.render_in(&cam, &[], &look);
    let (u, v, _) = cam.project([JOIST, 1500.0, -202.0]);
    assert_eq!(image.pick(u, v), Some(id), "a joist's underside picks its floor");
    let shown = |s: &Session| {
        let scene = crate::view3d::Scene::new(&s.model);
        let look = Look::of_3d(&s.model);
        let mesh = crate::view3d::gpu::Mesh::new(&scene, &look).unwrap().triangles();
        (scene.shown(&look), mesh, crate::gltf::export(&s.model).unwrap().triangles)
    };
    let framed = shown(&s);
    assert_eq!((framed.1, framed.2), (framed.0, framed.0));
    run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["floorframing"]}));
    let whole = shown(&s);
    assert!(framed.0 > whole.0 + 38 * 12, "{framed:?} {whole:?}");
}

/// Whether section `sec` fills `owner` at (along, z), mm in its own frame.
fn section_fills(s: &Session, sec: Id, owner: Id) -> impl Fn(f64, f64) -> bool + use<> {
    let view = render::section_fit(&s.model, sec, 1600, 800);
    let d = render::section(&s.model, sec, view);
    let rings: Vec<Vec<P2>> = d
        .prims
        .iter()
        .zip(&d.owners)
        .filter(|(_, o)| **o == Some(owner))
        .flat_map(|(p, _)| match p {
            Prim::Fill { rings, color, .. } if *color != PAPER => rings.iter().map(|r| r.iter().map(|q| view.mm(*q)).collect()).collect(),
            _ => Vec::new(),
        })
        .collect();
    move |x, z| rings.iter().any(|r| crate::geom::contains(r, P2(x, z)))
}

#[test]
fn a_section_across_the_joists_cuts_each_and_the_subfloor_over_them_and_hidden_framing_brings_the_layer_back() {
    let mut s = Session::new();
    let id = cabin(&mut s);
    // West to east at y = 1500, from 100 west of the floor.
    let sec = first(&run(&mut s, "section", json!({"start": [FW - 100.0, 1500.0], "end": [FE + 100.0, 1500.0]})));
    let at = |x: f64| x - (FW - 100.0);
    let fills = section_fills(&s, sec, id);
    assert!(fills(at(JOIST), MID), "a joist cut");
    assert!(!fills(at(BAY), MID), "nothing between joists");
    assert!(fills(at(BAY), -9.0), "the subfloor over them");
    assert!(fills(at(FW + 19.0), MID), "the edge joist");
    run(&mut s, "graphics", json!({"view": sec, "hide": ["floorframing"]}));
    assert!(section_fills(&s, sec, id)(at(BAY), MID), "framing hidden, the layer whole");
    // Along a joist, the middle beam under it.
    let sec = first(&run(&mut s, "section", json!({"start": [JOIST, FS - 100.0], "end": [JOIST, FN + 100.0]})));
    let fills = section_fills(&s, sec, id);
    assert!(fills(1500.0 + 100.0 - FS, MID) && fills(YL + 100.0 - FS + 500.0, MID), "the joist along its length");
}

#[test]
fn a_plan_draws_the_joists_in_hidden_line_only_where_it_shows_framing_lines() {
    let mut s = Session::new();
    let id = cabin(&mut s);
    let n = run(&mut s, "framing", json!({"id": id}))["members"].as_array().unwrap().len();
    let view = render::View { angle: 0.0, scale: 0.08, cx: 3600.0, cy: 3000.0, width: 700, height: 600 };
    let dashed = |s: &Session| {
        let d = render::draw(&s.model, 100000, view);
        d.prims.iter().zip(&d.owners).filter(|(p, o)| **o == Some(id) && matches!(p, Prim::Line { dash: Some(_), .. })).count()
    };
    assert_eq!(dashed(&s), 0, "an ordinary plan isn't cluttered with them");
    run(&mut s, "graphics", json!({"view": "Level 1", "show": ["floorframingline"]}));
    assert_eq!(dashed(&s), n, "each member's outline, dashed");
    // Through the PNG: a dashed line along a joist's side, none in the middle of the bay.
    let d = render::draw(&s.model, 100000, view);
    let pm = tiny_skia::Pixmap::decode_png(&render::png(&d).unwrap()).unwrap();
    let ink = |x: f64| {
        let (u, _) = view.px(P2(x, 0.0));
        (0..600u32).filter(|v| pm.pixel(u.round() as u32, *v).is_some_and(|c| c.red() < 200)).count()
    };
    assert!(ink(JOIST - 19.0) > 100, "the joist's side: {}", ink(JOIST - 19.0));
    assert!(ink(BAY) < 40, "the middle of the bay: {}", ink(BAY));
    run(&mut s, "graphics", json!({"view": "Level 1", "hide": ["floor"]}));
    assert_eq!(dashed(&s), 0, "hidden with its floor");
}

#[test]
fn floors_have_framing_and_framing_lines_subcategories_the_lines_off_unless_shown() {
    let subs: Vec<(&str, bool)> = crate::graphics::subcategories("floor").map(|s| (s.key, s.off)).collect();
    assert_eq!(subs, [("floorframing", false), ("floorframingline", true)]);
}

/// The entities of `class` in a STEP file: each line's text.
fn of_class<'a>(f: &'a str, class: &str) -> Vec<&'a str> {
    f.lines().filter(|l| l.split_once('=').is_some_and(|(_, e)| e.starts_with(&format!("{class}(")))).collect()
}

fn ref_of(line: &str) -> &str {
    line.split_once('=').map_or("", |(r, _)| r)
}

#[test]
fn each_joist_rim_trimmer_and_header_goes_to_ifc_as_a_joist_of_its_slab() {
    let mut s = Session::new();
    let id = cabin(&mut s);
    run(&mut s, "type", json!({"category": "framing", "name": "Cabin Floor", "bridging": 2000}));
    run(&mut s, "modify", json!({"id": id, "openings": [[[1300, 1000], [2300, 1000], [2300, 2200], [1300, 2200]]]}));
    let r = run(&mut s, "framing", json!({"id": id}));
    let ms = r["members"].as_array().unwrap();
    let count = |roles: &[&str]| ms.iter().filter(|m| roles.iter().any(|r| m["role"] == *r)).count();
    let f = crate::ifc::export(&s.model);
    let slab = of_class(&f, "IFCSLAB");
    assert_eq!(slab.len(), 1);
    let joists: Vec<&str> = of_class(&f, "IFCBEAM").into_iter().filter(|l| l.ends_with(",.JOIST.);")).collect();
    assert_eq!(joists.len(), count(&["joist", "rim", "trimmer", "header"]));
    let blocks: Vec<&str> = of_class(&f, "IFCMEMBER").into_iter().filter(|l| l.contains("'blocking'")).collect();
    assert_eq!(blocks.len(), count(&["blocking"]));
    assert!(!blocks.is_empty());
    // Parts of the slab, in no storey of their own.
    let parts =
        of_class(&f, "IFCRELAGGREGATES").into_iter().find(|l| l.contains(&format!(",{},(", ref_of(slab[0])))).expect("the slab's parts");
    let part = |m: &str| parts.contains(&format!("{},", ref_of(m))) || parts.contains(&format!("{})", ref_of(m)));
    assert!(joists.iter().chain(&blocks).all(|m| part(m)));
    let contained = of_class(&f, "IFCRELCONTAINEDINSPATIALSTRUCTURE");
    assert!(
        !contained.iter().any(|l| joists.iter().any(|m| l.contains(&format!("{},", ref_of(m))) || l.contains(&format!("{})", ref_of(m)))))
    );
    // GlobalIds by key: an opening moved keeps the far joists'.
    let named = |f: &str, key: &str| {
        of_class(f, "IFCBEAM").iter().find(|l| l.contains(&format!("'{key}'"))).map(|l| l.split('\'').nth(1).unwrap_or("").to_string())
    };
    let far = named(&f, "joist/2/1").expect("a joist far from the opening, tagged by its key");
    run(&mut s, "modify", json!({"id": id, "openings": [[[1300, 1100], [2300, 1100], [2300, 2300], [1300, 2300]]]}));
    assert_eq!(named(&crate::ifc::export(&s.model), "joist/2/1"), Some(far));
    // IFC2x3: beams without a predefined type.
    let old = crate::ifc::export_as(&s.model, Default::default(), crate::ifc::Schema::Ifc2x3);
    assert!(of_class(&old, "IFCBEAM").iter().all(|l| !l.contains(".JOIST.")));
    assert_eq!(of_class(&old, "IFCBEAM").len(), of_class(&crate::ifc::export(&s.model), "IFCBEAM").len());
}

#[test]
fn in_ifc2x3_every_placement_a_framed_floor_writes_places_a_product() {
    // IFC2x3's IfcLocalPlacement places exactly one product (PlacesObject SET [1:1]); a placement that only others are
    // placed relative to is invalid there.
    let mut s = Session::new();
    let id = cabin(&mut s);
    run(&mut s, "frame", json!({"id": id}));
    let f = crate::ifc::export_as(&s.model, Default::default(), crate::ifc::Schema::Ifc2x3);
    let placements: Vec<&str> = of_class(&f, "IFCLOCALPLACEMENT").into_iter().map(ref_of).collect();
    assert!(placements.len() > 10);
    let orphans: Vec<&&str> = placements
        .iter()
        .filter(|p| {
            !f.lines().any(|l| {
                let entity = l.split_once('=').map_or("", |(_, e)| e);
                !entity.starts_with("IFCLOCALPLACEMENT(") && (l.contains(&format!("{p},")) || l.contains(&format!("{p})")))
            })
        })
        .collect();
    assert!(orphans.is_empty(), "placements that place nothing: {orphans:?}");
}

#[test]
fn a_framed_floors_ifc_opens_as_one_floor_and_no_beams() {
    let mut s = Session::new();
    run(
        &mut s,
        "type",
        json!({"category": "floor", "name": "2x8", "layers": [{"function": "structure", "material": "Timber", "thickness": 184}]}),
    );
    let id = first(&run(
        &mut s,
        "floor",
        json!({"boundary": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]], "type": "2x8",
                                                 "openings": [[[1000, 1000], [2000, 1000], [2000, 2000], [1000, 2000]]]}),
    ));
    run(&mut s, "frame", json!({"id": id}));
    let i = crate::import::ifc(crate::ifc::export(&s.model).as_bytes(), "back").unwrap();
    let count = |c: &str| i.model.elements.values().filter(|e| e.category() == c).count();
    assert_eq!((count("floor"), count("beam"), count("component")), (1, 0, 0), "skipped: {:?}", i.skipped);
}
