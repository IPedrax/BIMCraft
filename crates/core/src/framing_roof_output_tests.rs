//! A framed roof's members drawn and written out (`framing/roof.rs`): solids of the roof in 3D and sections, dashed lines in a
//! plan of its level where it shows Roofs > Framing Lines, Roofs > Framing in Visibility/Graphics, the `framingmember`
//! schedule and `IfcMember`s aggregated to their roof. Each through its output.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::framing_roof_hunt_tests::{first, run};
use crate::framing_roof_tests::{X0, X1, YL, YN, YSW, cabin, under};
use crate::geom::P2;
use crate::graphics::Look;
use crate::model::Id;
use crate::render::{self, PAPER, Prim, Rgb};

/// A rafter's centre line in plan x (the 11th from the west), and the middle of the bay east of it.
const RAFTER: f64 = X0 + 10.0 * 1220.0 / 3.0;
const BAY: f64 = RAFTER + 203.0;

#[test]
fn in_3d_a_framed_roofs_rafters_are_its_own_in_their_material_and_hidden_framing_brings_its_layer_back() {
    let mut s = Session::new();
    let (id, _) = cabin(&mut s, json!({}));
    run(&mut s, "material", json!({"name": "Red Pine", "color": [200, 30, 30]}));
    run(&mut s, "type", json!({"category": "framing", "name": "Cabin Roof", "material": "Red Pine"}));
    let n = run(&mut s, "framing", json!({"id": id}))["members"].as_array().map_or(0, Vec::len);
    let drawn = |s: &Session| {
        let look = Look::of_3d(&s.model);
        crate::view3d::shaded(&s.model).into_iter().filter(|x| x.owner() == id && look.forms_show(x.vis(), false)).collect::<Vec<_>>()
    };
    let solids = drawn(&s);
    assert_eq!(solids.iter().filter(|x| x.color() == Rgb(200, 30, 30)).count(), n, "every member the roof's, in its material");
    let timber = s.model.materials["Timber"].color;
    assert_eq!(
        solids.iter().filter(|x| x.color() == Rgb(timber[0], timber[1], timber[2])).count(),
        1,
        "the sheathing only, not the rafters' layer"
    );
    run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["roofframing"]}));
    let whole = drawn(&s);
    assert!(!whole.iter().any(|x| x.color() == Rgb(200, 30, 30)), "framing hidden: no members");
    assert_eq!(whole.len(), 2, "the roof's two layers, as an unframed roof's");
    // Another host's subcategory leaves the rafters.
    run(&mut s, "graphics", json!({"view": "{3D}", "show": ["roofframing"], "hide": ["floorframing", "framing"]}));
    assert_eq!(drawn(&s).iter().filter(|x| x.color() == Rgb(200, 30, 30)).count(), n);
}

#[test]
fn seen_from_below_a_rafter_picks_the_roof_and_the_mesh_and_gltf_draw_what_the_view_does() {
    let mut s = Session::new();
    let (id, _) = cabin(&mut s, json!({}));
    let cam = crate::view3d::Camera {
        azimuth: -90.0,
        elevation: -50.0,
        scale: 0.1,
        target: [RAFTER, 4500.0, 2500.0],
        width: 900,
        height: 700,
        lens: None,
    };
    let scene = crate::view3d::Scene::new(&s.model);
    let image = scene.render_in(&cam, &[], &Look::of_3d(&s.model));
    let (u, v, _) = cam.project([RAFTER, 4500.0, under(4500.0)]);
    assert_eq!(image.pick(u, v), Some(id), "a rafter's underside picks its roof");
    let shown = |s: &Session| {
        let scene = crate::view3d::Scene::new(&s.model);
        let look = Look::of_3d(&s.model);
        let mesh = crate::view3d::gpu::Mesh::new(&scene, &look).unwrap().triangles();
        (scene.shown(&look), mesh, crate::gltf::export(&s.model).unwrap().triangles)
    };
    let framed = shown(&s);
    assert_eq!((framed.1, framed.2), (framed.0, framed.0));
    run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["roofframing"]}));
    let whole = shown(&s);
    assert!(framed.0 > whole.0 + 66 * 11, "each rafter's triangles: {framed:?} {whole:?}");
}

/// The section box's caps covering plan point (x, y) on its top face at `z`.
fn capped(s: &Session, x: f64, y: f64, z: f64) -> usize {
    let look = Look::of_3d(&s.model);
    let caps = crate::view3d::CapSource::new(&s.model).caps(&s.model, &look, (50.0, crate::graphics::Detail::Fine));
    caps.tris
        .iter()
        .filter(|t| t.p.iter().all(|p| (p[2] - z).abs() < 1e-6))
        .filter(|t| crate::geom::contains(&t.p.map(|p| P2(p[0], p[1])), P2(x, y)))
        .count()
}

#[test]
fn a_section_box_through_the_rafters_caps_each_and_nothing_between_them() {
    let mut s = Session::new();
    let _ = cabin(&mut s, json!({}));
    // Level with the rafters' middle at y 4500, over the low pieces, which no lap shifts.
    let mid = under(4500.0) + 100.0;
    run(&mut s, "section_box", json!({"min": [-1000, -4000, 0], "max": [8000, 7000, mid]}));
    assert!(capped(&s, RAFTER, 4500.0, mid) > 0, "a rafter capped");
    assert_eq!(capped(&s, BAY, 4500.0, mid), 0, "nothing between rafters");
    run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["roofframing"]}));
    assert!(capped(&s, BAY, 4500.0, mid) > 0, "framing hidden: the layer capped whole");
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
fn a_section_across_the_rafters_cuts_each_and_the_sheathing_over_them_and_one_down_the_slope_cuts_the_laps() {
    let mut s = Session::new();
    let (id, _) = cabin(&mut s, json!({}));
    let z = under(4500.0) + 100.0;
    let sec = first(&run(&mut s, "section", json!({"start": [X0 - 100.0, 4500.0], "end": [X1 + 100.0, 4500.0]})));
    let at = |x: f64| x - (X0 - 100.0);
    let fills = section_fills(&s, sec, id);
    assert!(fills(at(RAFTER), z), "a rafter cut");
    assert!(!fills(at(BAY), z), "nothing between rafters");
    assert!(fills(at(BAY), under(4500.0) + 235.0 / 0.99875 + 5.0), "the sheathing over them");
    run(&mut s, "graphics", json!({"view": sec, "hide": ["roofframing"]}));
    assert!(section_fills(&s, sec, id)(at(BAY), z), "framing hidden, the layer whole");
    // Up the slope along a rafter, north to south: its low and high pieces, and the middle one beside them not cut.
    let sec = first(&run(&mut s, "section", json!({"start": [RAFTER, YN + 100.0], "end": [RAFTER, -3200.0]})));
    let along = |y: f64| YN + 100.0 - y;
    let fills = section_fills(&s, sec, id);
    assert!(fills(along(YL + 600.0), under(YL + 600.0) + 100.0), "the low piece");
    assert!(fills(along(YSW - 100.0), under(YSW - 100.0) + 100.0), "the high piece over the south wall");
    assert!(!fills(along(1500.0), under(1500.0) + 100.0), "the middle piece lies beside the line");
    // Across them over the south wall: each lapped pair side by side, the middle piece a thickness to its slope's left (east).
    let sec = first(&run(&mut s, "section", json!({"start": [X0 - 100.0, YSW + 100.0], "end": [X1 + 100.0, YSW + 100.0]})));
    let fills = section_fills(&s, sec, id);
    let z = under(YSW + 100.0) + 100.0;
    assert!(fills(at(RAFTER), z) && fills(at(RAFTER + 38.0), z), "the pieces side by side");
}

#[test]
fn a_plan_of_its_level_draws_the_rafters_dashed_only_where_it_shows_framing_lines() {
    let mut s = Session::new();
    let (id, _) = cabin(&mut s, json!({}));
    let n = run(&mut s, "framing", json!({"id": id}))["members"].as_array().map_or(0, Vec::len);
    let view = render::View { angle: 0.0, scale: 0.06, cx: 3600.0, cy: 1800.0, width: 700, height: 800 };
    let dashed = |s: &Session| {
        let d = render::draw(&s.model, 100000, view);
        d.prims.iter().zip(&d.owners).filter(|(p, o)| **o == Some(id) && matches!(p, Prim::Line { dash: Some(_), .. })).count()
    };
    let before = dashed(&s);
    run(&mut s, "graphics", json!({"view": "Level 1", "show": ["roofframingline"]}));
    assert_eq!(dashed(&s), before + n, "each member's outline, dashed");
    let d = render::draw(&s.model, 100000, view);
    let pm = tiny_skia::Pixmap::decode_png(&render::png(&d).unwrap()).unwrap();
    let ink = |x: f64| {
        let (u, _) = view.px(P2(x, 0.0));
        (0..800u32).filter(|v| pm.pixel(u.round() as u32, *v).is_some_and(|c| c.red() < 200)).count()
    };
    assert!(ink(RAFTER - 19.0) > ink(BAY) + 100, "a rafter's side: {} against the bay's {}", ink(RAFTER - 19.0), ink(BAY));
    run(&mut s, "graphics", json!({"view": "Level 1", "hide": ["roof"]}));
    assert_eq!(dashed(&s), 0, "hidden with its roof");
}

#[test]
fn roofs_have_framing_and_framing_lines_subcategories_the_lines_off_unless_shown() {
    let subs: Vec<(&str, bool)> = crate::graphics::subcategories("roof").map(|s| (s.key, s.off)).collect();
    assert_eq!(subs, [("roofframing", false), ("roofframingline", true)]);
}

#[test]
fn the_schedule_lists_each_member_with_the_roof_as_its_host() {
    let mut s = Session::new();
    let (id, _) = cabin(&mut s, json!({}));
    let n = run(&mut s, "framing", json!({"id": id}))["members"].as_array().map_or(0, Vec::len);
    let t = run(&mut s, "schedule", json!({"name": "Roof Members", "category": "framingmembers", "fields": ["Host", "Role", "Host Type"]}));
    let rows: Vec<&Value> = t["rows"].as_array().into_iter().flatten().filter(|row| row[0] == json!(id as f64)).collect();
    assert_eq!(rows.len(), n);
    assert!(rows.iter().all(|row| row[2] == json!("2x10 Roof")));
    assert_eq!(rows.iter().filter(|row| row[1] == json!("rafter")).count(), 66);
}

/// The entities of `class` in a STEP file: each line's text.
fn of_class<'a>(f: &'a str, class: &str) -> Vec<&'a str> {
    f.lines().filter(|l| l.split_once('=').is_some_and(|(_, e)| e.starts_with(&format!("{class}(")))).collect()
}

fn ref_of(line: &str) -> &str {
    line.split_once('=').map_or("", |(r, _)| r)
}

#[test]
fn each_rafter_goes_to_ifc_as_a_rafter_member_of_its_roof_with_its_ridge_and_headers_members() {
    let mut s = Session::new();
    let (id, _) = cabin(&mut s, json!({"bridging": 2000}));
    run(&mut s, "modify", json!({"id": id, "openings": [[[1000, 4000], [2000, 4000], [2000, 5200], [1000, 5200]]]}));
    let r = run(&mut s, "framing", json!({"id": id}));
    let ms = r["members"].as_array().cloned().unwrap_or_default();
    let count = |roles: &[&str]| ms.iter().filter(|m| roles.iter().any(|r| m["role"] == *r)).count();
    let f = crate::ifc::export(&s.model);
    let roof = of_class(&f, "IFCROOF");
    assert_eq!(roof.len(), 1);
    let members = of_class(&f, "IFCMEMBER");
    let rafters: Vec<&str> = members.iter().copied().filter(|l| l.ends_with(",.RAFTER.);")).collect();
    assert_eq!(rafters.len(), count(&["rafter", "trimmer"]));
    let others: Vec<&str> = members.iter().copied().filter(|l| l.contains("'face/") && l.ends_with(",.MEMBER.);")).collect();
    assert_eq!(others.len(), count(&["header", "blocking", "rim", "ridge"]));
    assert!(count(&["blocking"]) > 0 && count(&["header"]) > 0);
    let parts =
        of_class(&f, "IFCRELAGGREGATES").into_iter().find(|l| l.contains(&format!(",{},(", ref_of(roof[0])))).expect("the roof's parts");
    let part = |m: &str| parts.contains(&format!("{},", ref_of(m))) || parts.contains(&format!("{})", ref_of(m)));
    assert!(rafters.iter().chain(&others).all(|m| part(m)));
    let named = |f: &str, key: &str| {
        of_class(f, "IFCMEMBER").iter().find(|l| l.contains(&format!("'{key}'"))).map(|l| l.split('\'').nth(1).unwrap_or("").to_string())
    };
    let far = named(&f, "face/0/rafter/18/0").expect("a rafter far from the opening, tagged by its key");
    assert_eq!(named(&crate::ifc::export(&s.model), "face/0/rafter/18/0").as_deref(), Some(far.as_str()), "the same GlobalId again");
    // IFC2x3: members without a predefined type, as many.
    let old = crate::ifc::export_as(&s.model, Default::default(), crate::ifc::Schema::Ifc2x3);
    assert!(of_class(&old, "IFCMEMBER").iter().all(|l| !l.contains(".RAFTER.")));
    assert_eq!(of_class(&old, "IFCMEMBER").len(), members.len());
}

#[test]
fn a_framed_roofs_ifc_opens_as_one_roof_and_no_stray_members() {
    let mut s = Session::new();
    let (_, _) = cabin(&mut s, json!({}));
    let roofs = s.model.elements.values().filter(|e| e.category() == "roof").count();
    let i = crate::import::ifc(crate::ifc::export(&s.model).as_bytes(), "back").unwrap();
    let count = |c: &str| i.model.elements.values().filter(|e| e.category() == c).count();
    assert_eq!(count("roof"), roofs);
    assert_eq!(count("component"), 0, "skipped: {:?}", i.skipped);
    // The two beams come back as beams; no member does.
    assert_eq!(count("beam"), 2, "skipped: {:?}", i.skipped);
}

#[test]
fn in_ifc2x3_a_framed_roof_keeps_its_body_and_its_members_stand_in_its_storey() {
    // IFC2x3's IfcRoof may be decomposed only when it has no representation of its own (its ShapeRepresentation rule), so
    // there its members aren't its parts: they stand in its storey, and the roof keeps its body. IFC4 makes them its parts.
    let mut s = Session::new();
    let (id, _) = cabin(&mut s, json!({}));
    let members = run(&mut s, "framing", json!({"id": id}))["members"].as_array().map_or(0, Vec::len);
    assert!(members > 20);
    let old = crate::ifc::export_as(&s.model, Default::default(), crate::ifc::Schema::Ifc2x3);
    let roof = of_class(&old, "IFCROOF");
    assert_eq!(roof.len(), 1);
    // Its placement and its body, then its tag.
    let parts: Vec<&str> = roof[0].split(",#").collect();
    let body =
        parts.windows(2).any(|w| w[0].parse::<u64>().is_ok() && w[1].split_once(",'").is_some_and(|(n, _)| n.parse::<u64>().is_ok()));
    assert!(body, "the roof keeps its body: {}", roof[0]);
    let decomposed = |f: &str, r: &str| of_class(f, "IFCRELAGGREGATES").iter().any(|l| l.contains(&format!(",{r},(")));
    assert!(!decomposed(&old, ref_of(roof[0])), "IFC2x3: the roof is decomposed and has a body");
    let rafters: Vec<&str> = of_class(&old, "IFCMEMBER").into_iter().filter(|l| l.contains("'rafter'")).collect();
    assert!(!rafters.is_empty());
    let contained = of_class(&old, "IFCRELCONTAINEDINSPATIALSTRUCTURE");
    let held = |m: &str| contained.iter().any(|l| l.contains(&format!("{},", ref_of(m))) || l.contains(&format!("{})", ref_of(m))));
    assert!(rafters.iter().all(|m| held(m)), "IFC2x3: each rafter stands in a storey");
    let new = crate::ifc::export(&s.model);
    assert!(decomposed(&new, ref_of(of_class(&new, "IFCROOF")[0])), "IFC4: the members are the roof's parts");
}

#[test]
fn in_ifc2x3_every_placement_a_framed_roof_writes_places_a_product() {
    // IFC2x3's IfcLocalPlacement places exactly one product (PlacesObject [1:1]): the members were placed in a placement of
    // the framing's frame that placed nothing itself.
    let mut s = Session::new();
    let (id, _) = cabin(&mut s, json!({"bridging": 2000}));
    run(&mut s, "modify", json!({"id": id, "openings": [[[1000, 4000], [2000, 4000], [2000, 5200], [1000, 5200]]]}));
    let gable = first(&run(
        &mut s,
        "roof",
        json!({"boundary": [[20000, 0], [28000, 0], [28000, 5000], [20000, 5000]], "level": "Level 1", "offset": 2400, "slope": 30,
               "gables": [1, 3], "type": "2x10 Roof"}),
    ));
    run(&mut s, "frame", json!({"id": gable, "type": "Cabin Roof"}));
    for schema in [crate::ifc::Schema::Ifc2x3, crate::ifc::Schema::Ifc4] {
        let f = crate::ifc::export_as(&s.model, Default::default(), schema);
        let placements: Vec<&str> = of_class(&f, "IFCLOCALPLACEMENT").into_iter().map(ref_of).collect();
        // What each placement places: the lines other than placements that name it.
        let refers =
            |line: &str, r: &str| line.match_indices(r).any(|(i, _)| !line[i + r.len()..].starts_with(|c: char| c.is_ascii_digit()));
        let others: Vec<&str> = f.lines().filter(|l| l.contains('=') && !l.contains("=IFCLOCALPLACEMENT(")).collect();
        for p in placements {
            let n = others.iter().filter(|l| refers(l.split_once('=').map_or("", |(_, e)| e), p)).count();
            assert_eq!(n, 1, "{schema:?}: placement {p} places {n} products");
        }
    }
    // The members where they were: a rafter's placement at its slope's frame's origin, in plan, in the roof's (the gable's
    // alone framed, so its keys are the only ones).
    run(&mut s, "unframe", json!({"id": id}));
    let f = crate::ifc::export(&s.model);
    let r = run(&mut s, "framing", json!({"id": gable}));
    let m = r["members"].as_array().and_then(|ms| ms.iter().find(|m| m["role"] == "rafter")).cloned().expect("a rafter");
    let line =
        of_class(&f, "IFCMEMBER").into_iter().find(|l| l.contains(&format!("'{}'", m["key"].as_str().unwrap_or("")))).expect("its member");
    let place = line.split(",#").nth(1).and_then(|t| t.split(',').next()).expect("its placement");
    let at = |r: &str| f.lines().find(|l| l.starts_with(&format!("#{r}="))).unwrap_or("").to_string();
    let axes = at(at(place).split(",#").nth(1).unwrap_or("").trim_end_matches(");"));
    let point = at(axes.split("(#").nth(1).and_then(|t| t.split(',').next()).unwrap_or(""));
    let xyz: Vec<f64> = point
        .split("((")
        .nth(1)
        .unwrap_or("")
        .trim_end_matches("));")
        .split(',')
        .map(|v| v.trim_end_matches('.').parse().unwrap_or(f64::NAN))
        .collect();
    let o = &m["frame"]["origin"];
    let want = [o[0].as_f64().unwrap_or(f64::NAN), o[1].as_f64().unwrap_or(f64::NAN)];
    assert!((xyz[0] - want[0]).abs() < 0.01 && (xyz[1] - want[1]).abs() < 0.01, "{point} isn't at {want:?}");
}
