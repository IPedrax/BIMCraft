//! F8: a family's Visibility Settings, as Revit's Family Element Visibility Settings. A door-like family, its leaf hidden in
//! plan and at Coarse and its swing a symbolic arc, placed in a wall: what a plan at each detail level, an elevation, a
//! section through it and the 3D view draw; the settings kept through a file; a file of 30 drawn as before; IFC untouched.
//!
//! Driven through `Session::execute`, as every front end drives it, and tested through the pictures.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::Id;
use crate::render::{self, PAPER, Rgb, View};
use crate::view3d::Scene;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn err(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(r) => panic!("{id} {p} should have been refused, and did {r}"),
        Err(e) => e,
    }
}

fn id_of(v: &Value) -> Id {
    v["id"].as_u64().unwrap_or_default()
}

/// The leaf's colour in a shaded 3D view: nothing else in the project is this red.
const LEAF: [u8; 3] = [220, 20, 20];

/// A door family: a frame (two jambs and a head, as a file of 30 has them), a leaf 760 x 2000 inside it whose settings are
/// `leaf`, and a symbolic swing (an arc and the leaf open) in Opening, its settings `swing`.
fn door_family(leaf: Value, swing: Value) -> Value {
    let block = |x: [f64; 2], y: [&str; 2], z: [f64; 2]| json!({"profile": [[x[0], y[0]], [x[1], y[0]], [x[1], y[1]], [x[0], y[1]]], "from": z[0], "to": z[1]});
    let (half_t, minus_half_t) = ("Wall Thickness / 2", "-Wall Thickness / 2");
    let mut leaf_form = json!({"profile": [[-380, -20], [380, -20], [380, 20], [-380, 20]], "to": 2000, "material": "Leaf Red"});
    if !leaf.is_null() {
        leaf_form["visibility"] = leaf;
    }
    let mut arc = json!({"arc": {"centre": [-380, 100], "radius": 760, "from": 0, "to": 90}, "subcategory": "Opening"});
    let mut open = json!({"line": [[-380, 100], [-380, 860]], "subcategory": "Opening"});
    if !swing.is_null() {
        arc["visibility"] = swing.clone();
        open["visibility"] = swing;
    }
    json!({
        "name": "Test Door",
        "category": "door",
        "parameters": [{"name": "Width", "value": 900}, {"name": "Height", "value": 2100}],
        "types": {"900 x 2100": {}},
        "forms": [
            block([-450.0, -400.0], [minus_half_t, half_t], [0.0, 2050.0]),
            block([400.0, 450.0], [minus_half_t, half_t], [0.0, 2050.0]),
            block([-450.0, 450.0], [minus_half_t, half_t], [2050.0, 2100.0]),
            leaf_form
        ],
        "symbols": [arc, open]
    })
}

/// A project: a 200 mm wall along x from 0 to 6000, and in its middle a door of [`door_family`], hung on the wall's centre.
fn project(leaf: Value, swing: Value) -> (Session, Id) {
    let mut s = Session::new();
    run(&mut s, "material", json!({"name": "Leaf Red", "color": LEAF}));
    run(&mut s, "family", door_family(leaf, swing));
    run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]]}));
    let door = id_of(&run(&mut s, "door", json!({"at": [3000, 0], "family": "Test Door", "type": "900 x 2100"})));
    (s, door)
}

/// The leaf as the door is authored in Revit: hidden in plan and at Coarse.
fn leaf_hidden() -> Value {
    json!({"plan": false, "coarse": false})
}

/// Level 1's plan round the door: 0.5 px a mm.
fn plan_view() -> View {
    View { scale: 0.5, cx: 3000.0, cy: 300.0, width: 1200, height: 1200 }
}

/// Level 1's plan at detail level `detail`.
fn plan(s: &mut Session, detail: &str) -> tiny_skia::Pixmap {
    run(s, "detail", json!({"view": "Level 1", "detail": detail}));
    tiny_skia::Pixmap::decode_png(&render::png(&render::draw(&s.model, 100000, plan_view())).unwrap()).unwrap()
}

/// The pixels that aren't paper in the box from model point `lo` to `hi` of `pm`, drawn through `view`.
fn ink(pm: &tiny_skia::Pixmap, view: View, lo: P2, hi: P2) -> usize {
    let (a, b) = (view.px(lo), view.px(hi));
    let (x0, x1) = (a.0.min(b.0) as u32, a.0.max(b.0) as u32);
    let (y0, y1) = (a.1.min(b.1) as u32, a.1.max(b.1) as u32);
    (x0..=x1)
        .flat_map(|x| (y0..=y1).map(move |y| (x, y)))
        .filter(|&(x, y)| pm.pixel(x, y).is_some_and(|c| Rgb(c.red(), c.green(), c.blue()) != PAPER))
        .count()
}

/// Where the leaf's outline is in plan: across the opening, short of the jambs, its faces at y = ±20.
const LEAF_LO: P2 = P2(2700.0, -30.0);
const LEAF_HI: P2 = P2(3300.0, 30.0);

/// A point on the swing's arc, in the model: 45 degrees round from its hinge at (2620, 100).
fn on_arc() -> P2 {
    let r = 760.0 * std::f64::consts::FRAC_1_SQRT_2;
    P2(2620.0 + r, 100.0 + r)
}

/// The leaf and the swing in Level 1's plan at `detail`: whether each is drawn.
fn leaf_and_swing(s: &mut Session, detail: &str) -> (bool, bool) {
    let pm = plan(s, detail);
    let view = plan_view();
    let swing = ink(&pm, view, on_arc() - P2(6.0, 6.0), on_arc() + P2(6.0, 6.0));
    (ink(&pm, view, LEAF_LO, LEAF_HI) > 0, swing > 0)
}

#[test]
fn a_door_with_its_leaf_hidden_in_plan_shows_its_swing_in_its_place_at_medium_and_fine_and_neither_at_coarse() {
    let (mut s, _) = project(leaf_hidden(), Value::Null);
    assert_eq!(leaf_and_swing(&mut s, "fine"), (false, true), "Fine: the swing, not the leaf");
    assert_eq!(leaf_and_swing(&mut s, "medium"), (false, true), "Medium: the swing, not the leaf");
    assert_eq!(leaf_and_swing(&mut s, "coarse"), (false, false), "Coarse: a symbol with no settings is left out, as ever");
    // The same door without settings draws its leaf too, at Medium and Fine, as a file of 30 does.
    let (mut old, _) = project(Value::Null, Value::Null);
    assert_eq!(leaf_and_swing(&mut old, "fine"), (true, true));
    assert_eq!(leaf_and_swing(&mut old, "coarse"), (false, false));
}

#[test]
fn a_symbols_detail_levels_and_a_form_ticked_for_coarse_are_drawn_at_coarse() {
    let (mut s, _) = project(json!({"medium": false, "fine": false}), json!({"coarse": true, "medium": false, "fine": false}));
    assert_eq!(leaf_and_swing(&mut s, "coarse"), (true, true), "Coarse: the leaf and the swing, as their settings tick it");
    assert_eq!(leaf_and_swing(&mut s, "fine"), (false, false), "Fine: neither");
    // A symbol takes only detail levels: Plan/RCP and the rest are refused, naming them.
    let mut f = door_family(Value::Null, json!({"plan": false}));
    f["name"] = json!("Other Door");
    let e = err(&mut s, "family", f);
    assert!(e.contains("symbol 1") && e.contains("only in plan"), "{e}");
}

/// An elevation of the wall's south face, looking north: Front/Back of the door. The picture through `view`, and the view.
fn elevation(s: &mut Session) -> (Id, View) {
    let id = id_of(&run(s, "elevation", json!({"start": [1000, -3000], "end": [5000, -3000], "name": "South"})));
    let view = render::section_fit(&s.model, id, 800, 600);
    (id, view)
}

/// What a section or an elevation picks at (`along` its line from its start, `z`).
fn pick(s: &Session, id: Id, view: View, along: f64, z: f64) -> Option<Id> {
    let (cam, clip) = render::section_camera(&s.model, id, view).unwrap();
    let image = Scene::new(&s.model).cut_in(&cam, &clip, &[], &crate::graphics::Look::of(&s.model, id));
    let (px, py) = view.px(P2(along, z));
    image.pick(f64::from(px), f64::from(py))
}

#[test]
fn an_elevation_draws_the_leaf_by_front_back_and_its_detail_level() {
    let (mut s, door) = project(leaf_hidden(), Value::Null);
    let (id, view) = elevation(&mut s);
    // The leaf's middle, 2000 along from the elevation's start at x = 1000; through the opening beyond it, nothing.
    assert_eq!(pick(&s, id, view, 2000.0, 1000.0), Some(door), "Fine, Front/Back: the leaf");
    run(&mut s, "detail", json!({"view": id, "detail": "coarse"}));
    assert_eq!(pick(&s, id, view, 2000.0, 1000.0), None, "Coarse: the leaf is left out, the opening seen through");
    assert_eq!(pick(&s, id, view, 1600.0, 1000.0), Some(door), "and the jamb stays");
    run(&mut s, "detail", json!({"view": id, "detail": "medium"}));
    assert_eq!(pick(&s, id, view, 2000.0, 1000.0), Some(door));
    // Front/Back off: not in this elevation, which looks along the door's y.
    let leaf = s.model.families["Test Door"].forms[3].id;
    run(&mut s, "family", json!({"name": "Test Door", "form": {"id": leaf, "visibility": {"front_back": false}}}));
    assert_eq!(pick(&s, id, view, 2000.0, 1000.0), None, "Front/Back off");
    // Left/Right off instead (a patch merges key by key): shown again here.
    run(&mut s, "family", json!({"name": "Test Door", "form": {"id": leaf, "visibility": {"front_back": true, "left_right": false}}}));
    assert_eq!(pick(&s, id, view, 2000.0, 1000.0), Some(door), "Left/Right doesn't touch a Front/Back view");
}

/// A section across the door, looking west along its x axis (Left/Right), and its picture as a PNG.
fn section(s: &mut Session) -> (Id, View) {
    let id = id_of(&run(s, "section", json!({"start": [3000, -1500], "end": [3000, 1500]})));
    let view = render::section_fit(&s.model, id, 800, 800);
    (id, view)
}

/// Whether the pixel at (`along`, `z`) of the section's PNG is poché or ink (the leaf where the line cuts it), not paper.
fn cut_at(s: &Session, id: Id, view: View, along: f64, z: f64) -> bool {
    let png = render::section_png(&s.model, id, view).unwrap();
    let pm = tiny_skia::Pixmap::decode_png(&png).unwrap();
    let (x, y) = view.px(P2(along, z));
    pm.pixel(x as u32, y as u32).is_some_and(|c| c.red() < 200 && c.green() < 200 && c.blue() < 200)
}

#[test]
fn a_section_cuts_the_leaf_by_left_right_and_its_detail_level() {
    let (mut s, _) = project(leaf_hidden(), Value::Null);
    let (id, view) = section(&mut s);
    // The leaf lies across the line at 1500 along it (y = 0), from 0 to 2000 up.
    assert!(cut_at(&s, id, view, 1500.0, 1000.0), "Fine: the leaf's cut, in poché");
    run(&mut s, "detail", json!({"view": id, "detail": "coarse"}));
    assert!(!cut_at(&s, id, view, 1500.0, 1000.0), "Coarse: no leaf to cut");
    run(&mut s, "detail", json!({"view": id, "detail": "fine"}));
    let leaf = s.model.families["Test Door"].forms[3].id;
    run(&mut s, "family", json!({"name": "Test Door", "form": {"id": leaf, "visibility": {"left_right": false}}}));
    assert!(!cut_at(&s, id, view, 1500.0, 1000.0), "Left/Right off: this section, along the door's x, doesn't cut it");
    // Only shown when cut: cut here, as it is.
    run(&mut s, "family", json!({"name": "Test Door", "form": {"id": leaf, "visibility": {"left_right": true, "only_when_cut": true}}}));
    assert!(cut_at(&s, id, view, 1500.0, 1000.0), "only when cut: cut, so shown");
}

/// The leaf's red pixels in the default 3D view, from the south-west and above.
fn red_in_3d(s: &Session) -> usize {
    let look = crate::graphics::Look::of_3d(&s.model);
    let scene = Scene::new(&s.model);
    let cam = scene.fit_in(-60.0, 15.0, 600, 400, &look);
    let image = scene.render_in(&cam, &[], &look);
    image.rgba.chunks(4).filter(|p| (p[0] as i32 - LEAF[0] as i32).abs() < 60 && p[1] < 90 && p[2] < 90 && p[0] > 100).count()
}

#[test]
fn the_3d_view_shows_the_leaf_whichever_way_it_looks_by_its_own_detail_level() {
    let (mut s, _) = project(leaf_hidden(), Value::Null);
    assert!(red_in_3d(&s) > 50, "Fine: the leaf, though hidden in plan");
    let made = run(&mut s, "detail", json!({"view": "{3D}", "detail": "coarse"}));
    assert_eq!(made["detail"], "coarse");
    assert_eq!(red_in_3d(&s), 0, "Coarse: no leaf");
    run(&mut s, "detail", json!({"view": "{3D}", "detail": "medium"}));
    assert!(red_in_3d(&s) > 50, "Medium: the leaf");
    assert_eq!(run(&mut s, "detail", json!({"view": "{3D}"}))["detail"], "medium", "the view keeps its level");
    // Only shown when cut: never in 3D.
    let leaf = s.model.families["Test Door"].forms[3].id;
    run(&mut s, "family", json!({"name": "Test Door", "form": {"id": leaf, "visibility": {"only_when_cut": true}}}));
    assert_eq!(red_in_3d(&s), 0, "only when cut: not in 3D");
}

/// How many of each IFC entity the project writes.
fn ifc_entities(s: &Session) -> std::collections::BTreeMap<String, usize> {
    let file = crate::ifc::export(&s.model);
    let mut by = std::collections::BTreeMap::new();
    for line in file.lines().filter(|l| l.starts_with('#')) {
        if let Some(name) = line.split_once('=').map(|(_, r)| r.trim_start().split('(').next().unwrap_or_default().to_string()) {
            *by.entry(name).or_insert(0) += 1;
        }
    }
    by
}

#[test]
fn ifc_and_the_model_keep_the_leaf_whatever_the_views_draw() {
    let (hidden, door) = project(
        json!({"plan": false, "front_back": false, "left_right": false, "coarse": false, "medium": false, "fine": false}),
        Value::Null,
    );
    let (shown, _) = project(Value::Null, Value::Null);
    assert_eq!(ifc_entities(&hidden), ifc_entities(&shown), "the same entities: the leaf's body is written");
    let leaf_corner = "380.";
    let file = crate::ifc::export(&hidden.model);
    assert!(file.contains(leaf_corner), "the leaf's corner is in the door's body");
    // The model: the door's pieces still hold the leaf.
    let e = &hidden.model.elements[&door];
    assert_eq!(hidden.model.opening_pieces(e).len(), 4, "jambs, head and leaf");
}

#[test]
fn settings_are_kept_through_a_file_and_read_back_by_the_family_command() {
    let (mut s, _) = project(leaf_hidden(), json!({"coarse": true}));
    let dir = std::env::temp_dir().join(format!("bimcraft-f8-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("door.bimcraft");
    run(&mut s, "save", json!({"path": path}));
    let text = std::fs::read_to_string(&path).unwrap();
    let format = crate::model::FORMAT;
    assert!(text.contains(&format!("\"format\": {format}")) || text.contains(&format!("\"format\":{format}")), "FORMAT {format}");
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    let f = &t.model.families["Test Door"];
    assert_eq!(f.forms[3].visibility, Some(crate::family::Visibility { plan: false, coarse: false, ..Default::default() }));
    assert_eq!(f.symbols[0].visibility.map(|v| v.coarse), Some(true));
    assert_eq!(f.forms[0].visibility, None, "a form without settings has none");
    assert_eq!(leaf_and_swing(&mut t, "coarse"), (false, true), "drawn as before saving");
    // The command reads them back, writing only what differs from Revit's defaults.
    let read = run(&mut t, "family", json!({"name": "Test Door"}));
    assert_eq!(read["family"]["forms"][3]["visibility"], json!({"plan": false, "coarse": false}), "{}", read["family"]["forms"][3]);
    // `null` takes them away.
    let leaf = t.model.families["Test Door"].forms[3].id;
    run(&mut t, "family", json!({"name": "Test Door", "form": {"id": leaf, "visibility": null}}));
    assert_eq!(t.model.families["Test Door"].forms[3].visibility, None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_of_30_reads_with_no_settings_and_draws_as_it_did() {
    // The starter door's project written as a build of 30 wrote it: no `visibility` anywhere, `format` 30.
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]]}));
    run(&mut s, "door", json!({"at": [3000, 0], "family": "Single Door"}));
    let dir = std::env::temp_dir().join(format!("bimcraft-f8-30-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("thirty.bimcraft");
    run(&mut s, "save", json!({"path": path}));
    let mut file: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    file["format"] = json!(30);
    assert!(!file.to_string().contains("\"visibility\""));
    let read = crate::model::Model::from_json(&file.to_string()).unwrap();
    assert_eq!(read.format, crate::model::FORMAT);
    assert!(read.families.values().all(|f| f.forms.iter().all(|x| x.visibility.is_none())));
    // Each detail level's plan is what the project drew before it was written.
    for detail in ["coarse", "medium", "fine"] {
        let mut before = Session::new();
        before.model = s.model.clone();
        let mut after = Session::new();
        after.model = std::sync::Arc::new(read.clone());
        let (a, b) = (plan(&mut before, detail), plan(&mut after, detail));
        assert!(a.data() == b.data(), "{detail}: the same picture");
    }
    // And, as ever, the starter door's swing is left out at Coarse and drawn at Medium.
    let swing_at = P2(3000.0, 500.0);
    let mut t = Session::new();
    t.model = std::sync::Arc::new(read);
    let coarse = plan(&mut t, "coarse");
    let medium = plan(&mut t, "medium");
    let near = |pm: &tiny_skia::Pixmap| ink(pm, plan_view(), swing_at - P2(400.0, 300.0), swing_at + P2(400.0, 300.0));
    assert_eq!(near(&coarse), 0, "Coarse: no swing");
    assert!(near(&medium) > 0, "Medium: the swing");
}

#[test]
fn a_nested_instances_settings_and_with_its_familys() {
    let (mut s, _) = project(Value::Null, Value::Null);
    // A generic family nesting the door's leaf as a box, its instance hidden in plan: the box is left out of the host's plan.
    run(
        &mut s,
        "family",
        json!({"name": "Host", "category": "generic", "types": {"A": {}},
               "forms": [{"profile": [[0, 0], [100, 0], [100, 100], [0, 100]], "to": 100}],
               "nest": {"name": "Leaf", "definition": {"category": "generic", "types": {"A": {}},
                   "forms": [{"profile": [[0, 0], [800, 0], [800, 40], [0, 40]], "to": 2000}]}}}),
    );
    run(
        &mut s,
        "family",
        json!({"name": "Host", "nested_instance": {"family": "Leaf", "type": "A", "at": [500, 500], "visibility": {"plan": false}}}),
    );
    let host = id_of(&run(&mut s, "component", json!({"family": "Host", "type": "A", "at": [1000, 2000]})));
    let c = s.model.elements.get(host).and_then(|e| if let crate::model::Element::Component(c) = e { Some(c) } else { None }).unwrap();
    let pieces = s.model.component_pieces(c);
    assert_eq!(pieces.len(), 2, "the nested box is built into the host");
    assert_eq!(pieces[1].vis.map(|v| v.plan), Some(false), "with its instance's settings");
    let view = View { scale: 0.5, cx: 1900.0, cy: 2500.0, width: 1200, height: 800 };
    let pm = tiny_skia::Pixmap::decode_png(&render::png(&render::draw(&s.model, 100000, view)).unwrap()).unwrap();
    assert!(ink(&pm, view, P2(990.0, 1990.0), P2(1110.0, 2110.0)) > 0, "the host's own form");
    assert_eq!(ink(&pm, view, P2(1550.0, 2490.0), P2(2250.0, 2550.0)), 0, "the nested box, hidden in plan");
}

#[test]
fn the_leafs_top_under_the_head_draws_as_a_whole_line_not_a_broken_one() {
    // Seen from above, the leaf's 40 mm top is a face one or two pixels across between its front's edge and the head's: it is
    // drawn as edge throughout, never its red a pixel here and there between two dark lines.
    let (s, _) = project(leaf_hidden(), Value::Null);
    let look = crate::graphics::Look::of_3d(&s.model);
    let scene = Scene::new(&s.model);
    for (azimuth, elevation) in [(-70.0, 20.0), (-110.0, 20.0), (-90.0, 15.0), (-60.0, 25.0)] {
        let cam = scene.fit_in(azimuth, elevation, 1200, 800, &look);
        let image = scene.render_in(&cam, &[], &look);
        let px = |x: usize, y: usize| {
            let k = 4 * (y * 1200 + x);
            image.rgba.get(k..k + 3).map(|p| [p[0], p[1], p[2]]).unwrap_or([255; 3])
        };
        let red = |p: [u8; 3]| p[0] > 100 && p[1] < 90 && p[2] < 90;
        let dark = |p: [u8; 3]| p.iter().all(|c| *c < 80);
        let broken = (1..799)
            .flat_map(|y| (0..1200).map(move |x| (x, y)))
            .filter(|&(x, y)| red(px(x, y)) && dark(px(x, y - 1)) && dark(px(x, y + 1)))
            .count();
        assert_eq!(broken, 0, "from {azimuth}, {elevation}: red pixels between two dark lines");
        assert!((0..800).any(|y| (0..1200).any(|x| red(px(x, y)))), "the leaf is drawn");
    }
}
