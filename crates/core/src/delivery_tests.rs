//! The `cad_profile` command and the delivery drawing rules a plan's DXF keeps with it: closed room outlines, door swings as
//! ARCs, and only a MARK attribute. Read from the DXF text, as a CAD program reads it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::engine::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-delivery-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[derive(Clone, Debug)]
struct Ent {
    kind: String,
    codes: Vec<(String, String)>,
}

impl Ent {
    fn get(&self, c: &str) -> Option<&str> {
        self.codes.iter().find(|(k, _)| k == c).map(|(_, v)| v.as_str())
    }
    fn num(&self, c: &str) -> f64 {
        self.get(c).and_then(|v| v.parse().ok()).unwrap_or(f64::NAN)
    }
    fn layer(&self) -> &str {
        self.get("8").unwrap_or("")
    }
    fn pts(&self) -> Vec<(f64, f64)> {
        let x: Vec<f64> = self.codes.iter().filter(|(k, _)| k == "10").filter_map(|(_, v)| v.parse().ok()).collect();
        let y: Vec<f64> = self.codes.iter().filter(|(k, _)| k == "20").filter_map(|(_, v)| v.parse().ok()).collect();
        x.into_iter().zip(y).collect()
    }
    fn closed(&self) -> bool {
        self.get("70").and_then(|v| v.parse::<i32>().ok()).is_some_and(|f| f & 1 == 1)
    }
}

/// The entities of the ENTITIES section, and each block's by name.
fn parse(text: &str) -> (Vec<Ent>, BTreeMap<String, Vec<Ent>>) {
    let lines: Vec<&str> = text.lines().collect();
    let (mut section, mut cur_block): (String, Option<String>) = (String::new(), None);
    let (mut ents, mut blocks) = (Vec::new(), BTreeMap::<String, Vec<Ent>>::new());
    let mut cur: Option<Ent> = None;
    let mut i = 0;
    let mut flush = |cur: &mut Option<Ent>, section: &str, block: &Option<String>| {
        if let Some(e) = cur.take() {
            match (section, block) {
                ("ENTITIES", _) => ents.push(e),
                ("BLOCKS", Some(b)) if e.kind != "BLOCK" && e.kind != "ENDBLK" => blocks.entry(b.clone()).or_default().push(e),
                _ => {}
            }
        }
    };
    while i + 1 < lines.len() {
        let (c, v) = (lines[i].trim(), lines[i + 1].trim());
        i += 2;
        if c == "0" {
            flush(&mut cur, &section, &cur_block);
            match v {
                "SECTION" => {
                    section = lines.get(i + 1).map(|s| s.trim().to_string()).unwrap_or_default();
                }
                "ENDSEC" => section.clear(),
                "ENDBLK" => cur_block = None,
                _ => cur = Some(Ent { kind: v.to_string(), codes: Vec::new() }),
            }
            continue;
        }
        if let Some(e) = cur.as_mut() {
            if e.kind == "BLOCK" && c == "2" && cur_block.is_none() {
                cur_block = Some(v.to_string());
            }
            e.codes.push((c.to_string(), v.to_string()));
        }
    }
    flush(&mut cur, &section, &cur_block);
    (ents, blocks)
}

fn area(p: &[(f64, f64)]) -> f64 {
    (0..p.len()).map(|i| p[i].0 * p[(i + 1) % p.len()].1 - p[(i + 1) % p.len()].0 * p[i].1).sum::<f64>().abs() / 2.0
}

fn inside(poly: &[(f64, f64)], (x, y): (f64, f64)) -> bool {
    let mut c = false;
    for i in 0..poly.len() {
        let ((x1, y1), (x2, y2)) = (poly[(i + poly.len() - 1) % poly.len()], poly[i]);
        if (y1 > y) != (y2 > y) && x < x1 + (y - y1) * (x2 - x1) / (y2 - y1) {
            c = !c;
        }
    }
    c
}

const HEAD: &str = r##"new {"name":"t"}
room_area {"at":"finish"}
material {"color":"#2f2f2f","name":"Poche"}
type {"category":"wall","layers":[{"function":"structure","material":"Poche","thickness":200}],"name":"W200"}
family {"name":"Single Door","types":{"900 x 2100":{"Height":2100,"Width":900}}}
family {"name":"Double Door","types":{"1500 x 2100":{"Height":2100,"Width":1500}}}
family {"name":"Sliding Door","types":{"2000 x 2100":{"Height":2000,"Width":2000}}}
wall {"closed":true,"points":[[0,0],[8000,0],[8000,4000],[0,4000]],"type":"W200"}
"##;

/// A box 8000 x 4000 split by a wall at x = 4000 holding a door, its right half split by a room separation line.
fn three_rooms(profile: bool) -> Session {
    let mut s = Session::new();
    let mut script = HEAD.to_string();
    script.push_str(
        r#"wall {"closed":false,"points":[[4000,0],[4000,4000]],"type":"W200"}
door {"at":[4000,2000],"family":"Single Door","type":"900 x 2100","mark":"D1"}
room_separator {"points":[[4000,2000],[8000,2000]]}
room {"name":"Left","number":"101","point":[2000,2000]}
room {"name":"Low","number":"102","point":[6000,1000]}
room {"name":"High","number":"103","point":[6000,3000]}
"#,
    );
    if profile {
        script.push_str("cad_profile {\"name\":\"delivery\"}\n");
    }
    s.script(&script).unwrap();
    s
}

fn plan_dxf(s: &mut Session, test: &str) -> String {
    let path = dir(test).join("plan.dxf");
    run(s, "save", json!({"path": path}));
    std::fs::read_to_string(&path).unwrap()
}

fn one_door(door: &str, test: &str, profile: bool) -> (Session, String) {
    let mut s = Session::new();
    let mut script = HEAD.to_string();
    script.push_str(door);
    script.push('\n');
    if profile {
        script.push_str("cad_profile {\"name\":\"delivery\"}\n");
    }
    s.script(&script).unwrap();
    let t = plan_dxf(&mut s, test);
    (s, t)
}

#[test]
fn the_profile_is_set_cleared_undone_refused_and_saved() {
    let mut s = Session::new();
    assert_eq!(run(&mut s, "cad_profile", json!({})), json!({"profile": "none"}));
    assert_eq!(run(&mut s, "cad_profile", json!({"name": "delivery"})), json!({"profile": "delivery"}));
    assert!(s.model.to_json().unwrap().contains("cad_profile"));
    let path = dir("save").join("p.bimcraft");
    run(&mut s, "save", json!({"path": path}));
    let mut again = Session::new();
    run(&mut again, "open", json!({"path": path}));
    assert_eq!(run(&mut again, "cad_profile", json!({})), json!({"profile": "delivery"}));
    run(&mut s, "undo", json!({}));
    assert_eq!(run(&mut s, "cad_profile", json!({})), json!({"profile": "none"}));
    run(&mut s, "redo", json!({}));
    run(&mut s, "cad_profile", json!({"name": "none"}));
    assert!(!s.model.to_json().unwrap().contains("cad_profile"));
    run(&mut s, "cad_profile", json!({"name": "delivery"}));
    for bad in [json!({"name": "autocad"}), json!({"name": 3}), json!({"nam": "delivery"}), json!({"name": "delivery", "x": 1})] {
        let e = s.execute("cad_profile", &bad).unwrap_err();
        assert!(e.contains("cad_profile"), "{e}");
    }
    assert_eq!(run(&mut s, "cad_profile", json!({})), json!({"profile": "delivery"}));
    // A model that never named one has no key.
    assert!(!Session::new().model.to_json().unwrap().contains("cad_profile"));
}

#[test]
fn with_the_profile_each_room_has_one_closed_outline_and_no_separation_line_is_drawn() {
    let mut s = three_rooms(true);
    let (ents, _) = parse(&plan_dxf(&mut s, "outlines"));
    let bnd: Vec<&Ent> = ents.iter().filter(|e| e.layer() == "A-AREA-BNDY").collect();
    assert_eq!(bnd.len(), 3, "{bnd:?}");
    let mut polys = Vec::new();
    for e in &bnd {
        assert_eq!(e.kind, "LWPOLYLINE");
        assert!(e.closed());
        assert!(e.codes.iter().all(|(k, v)| k != "42" || v.parse::<f64>().unwrap() == 0.0));
        polys.push(e.pts());
    }
    for (point, m2) in [((2000.0, 2000.0), 14.44), ((6000.0, 1000.0), 7.22), ((6000.0, 3000.0), 7.22)] {
        let own: Vec<&Vec<(f64, f64)>> = polys.iter().filter(|p| inside(p, point)).collect();
        assert_eq!(own.len(), 1, "{point:?}");
        assert!((area(own[0]) / 1e6 - m2).abs() < 0.01, "{point:?}: {}", area(own[0]) / 1e6);
    }
    // The two rooms split by the separation line share its edge, along y = 2000.
    let on_line = |p: &Vec<(f64, f64)>| p.iter().filter(|q| (q.1 - 2000.0).abs() < 0.5).count();
    let shared: Vec<_> = polys.iter().filter(|p| inside(p, (6000.0, 1000.0)) || inside(p, (6000.0, 3000.0))).collect();
    assert!(shared.iter().all(|p| on_line(p) >= 2));
}

#[test]
fn without_the_profile_the_separation_line_is_an_open_polyline_as_before() {
    let mut s = three_rooms(false);
    let (ents, _) = parse(&plan_dxf(&mut s, "outlines-off"));
    let bnd: Vec<&Ent> = ents.iter().filter(|e| e.layer() == "A-AREA-BNDY").collect();
    assert_eq!(bnd.len(), 1, "{bnd:?}");
    assert!(!bnd[0].closed() && bnd[0].pts().len() == 2);
}

#[test]
fn a_room_hidden_in_the_view_gets_no_outline() {
    let mut s = three_rooms(true);
    let id = s
        .model
        .elements
        .iter()
        .find_map(|(id, e)| matches!(e, crate::model::Element::Room(r) if r.number == "101").then_some(*id))
        .unwrap();
    run(&mut s, "graphics", json!({"hide_elements": [id]}));
    let (ents, _) = parse(&plan_dxf(&mut s, "hidden"));
    assert_eq!(ents.iter().filter(|e| e.layer() == "A-AREA-BNDY").count(), 2);
}

/// The swings of the door block `t` inserts: its ARCs, and its polylines.
fn door_block(t: &str) -> Vec<Ent> {
    let (ents, blocks) = parse(t);
    let ins = ents.iter().find(|e| e.kind == "INSERT" && e.layer() == "A-DOOR").expect("a door insert");
    blocks[ins.get("2").unwrap()].clone()
}

fn arcs(block: &[Ent]) -> Vec<&Ent> {
    block.iter().filter(|e| e.kind == "ARC").collect()
}

fn ends(a: &Ent) -> [(f64, f64); 2] {
    let (c, r) = ((a.num("10"), a.num("20")), a.num("40"));
    let at = |deg: f64| (c.0 + r * deg.to_radians().cos(), c.1 + r * deg.to_radians().sin());
    [at(a.num("50")), at(a.num("51"))]
}

#[test]
fn a_single_door_swing_is_one_quarter_circle_arc_however_it_is_flipped() {
    for (flags, name) in
        [("", "a"), (r#","flip_hand":true"#, "b"), (r#","flip_facing":true"#, "c"), (r#","flip_hand":true,"flip_facing":true"#, "d")]
    {
        let door = format!(r#"door {{"at":[0,2000],"family":"Single Door","type":"900 x 2100","mark":"D1"{flags}}}"#);
        let (_, on) = one_door(&door, &format!("swing-{name}"), true);
        let (_, off) = one_door(&door, &format!("swing-off-{name}"), false);
        let block = door_block(&on);
        let a = arcs(&block);
        assert_eq!(a.len(), 1, "{flags}");
        let a = a[0];
        let markers: Vec<&str> = a.codes.iter().filter(|(k, _)| k == "100").map(|(_, v)| v.as_str()).collect();
        assert_eq!(markers, ["AcDbEntity", "AcDbCircle", "AcDbArc"]);
        assert!(((a.num("51") - a.num("50")).rem_euclid(360.0) - 90.0).abs() < 0.01, "{flags}: {} {}", a.num("50"), a.num("51"));
        assert!((a.num("40") - 900.0).abs() < 0.01);
        let c = (a.num("10"), a.num("20"));
        let leaf = block.iter().filter(|e| e.kind == "LWPOLYLINE").any(|e| {
            let p = e.pts();
            p.len() == 2
                && p.iter().any(|q| (q.0 - c.0).hypot(q.1 - c.1) < 0.5)
                && ((p[0].0 - p[1].0).hypot(p[0].1 - p[1].1) - 900.0).abs() < 0.5
        });
        assert!(leaf, "{flags}: a leaf line from the arc's centre");
        assert!(block.iter().all(|e| e.kind != "LWPOLYLINE" || e.pts().len() <= 4));
        // It is the swing the polyline drew: the same two ends.
        let old = door_block(&off);
        let sw = old.iter().find(|e| e.kind == "LWPOLYLINE" && e.pts().len() == 25).expect("the 25-point swing without the profile");
        assert!(arcs(&old).is_empty());
        let p = sw.pts();
        let want = [p[0], p[24]];
        let got = ends(a);
        let near = |u: (f64, f64), v: (f64, f64)| (u.0 - v.0).hypot(u.1 - v.1) < 0.5;
        assert!(
            (near(got[0], want[0]) && near(got[1], want[1])) || (near(got[0], want[1]) && near(got[1], want[0])),
            "{flags}: {got:?} {want:?}"
        );
    }
}

#[test]
fn a_double_door_has_two_equal_arcs_and_a_sliding_door_none() {
    let (_, t) = one_door(r#"door {"at":[0,2000],"family":"Double Door","type":"1500 x 2100","mark":"D1"}"#, "double", true);
    let block = door_block(&t);
    let a = arcs(&block);
    assert_eq!(a.len(), 2);
    assert!((a[0].num("40") - a[1].num("40")).abs() < 0.01 && a[0].num("40") > 300.0);
    assert!(a.iter().all(|a| ((a.num("51") - a.num("50")).rem_euclid(360.0) - 90.0).abs() < 0.01));
    let (_, t) = one_door(r#"door {"at":[0,2000],"family":"Sliding Door","type":"2000 x 2100","mark":"D1"}"#, "sliding", true);
    assert!(arcs(&door_block(&t)).is_empty());
}

#[test]
fn the_profile_does_not_change_the_png() {
    let (mut on, _) = one_door(r#"door {"at":[0,2000],"family":"Single Door","type":"900 x 2100","mark":"D1"}"#, "png-on", true);
    let (mut off, _) = one_door(r#"door {"at":[0,2000],"family":"Single Door","type":"900 x 2100","mark":"D1"}"#, "png-off", false);
    let (a, b) = (dir("png").join("a.png"), dir("png").join("b.png"));
    run(&mut on, "save", json!({"path": a}));
    run(&mut off, "save", json!({"path": b}));
    assert_eq!(std::fs::read(a).unwrap(), std::fs::read(b).unwrap());
}

#[test]
fn with_the_profile_an_attribute_says_only_a_mark() {
    let mut s = three_rooms(true);
    let (ents, blocks) = parse(&plan_dxf(&mut s, "attribs"));
    let atts: Vec<&Ent> = ents.iter().filter(|e| e.kind == "ATTRIB").collect();
    // The same two attributes as without the profile, so the drawing holds the same entities; the type's is empty.
    assert_eq!(atts.len(), 2);
    assert_eq!((atts[0].get("2"), atts[0].get("1")), (Some("MARK"), Some("D1")));
    assert_eq!((atts[1].get("2"), atts[1].get("1")), (Some("TYPE"), Some("")));
    for b in blocks.values() {
        assert!(b.iter().filter(|e| e.kind == "ATTDEF").all(|e| ["MARK", "TYPE"].contains(&e.get("2").unwrap_or(""))));
    }
    // As before without it: a MARK and a TYPE, the type saying what it is.
    let mut s = three_rooms(false);
    let (ents, _) = parse(&plan_dxf(&mut s, "attribs-off"));
    let off: Vec<&Ent> = ents.iter().filter(|e| e.kind == "ATTRIB").collect();
    assert_eq!(off.len(), 2);
    assert_ne!(off[1].get("1"), Some(""));
}

/// The three-room plan's box with two gridlines named 1 and A.
fn gridded(profile: bool) -> Session {
    let mut s = Session::new();
    let mut script = HEAD.to_string();
    script.push_str(
        r#"grid {"start":[1000,-1500],"end":[1000,5500],"name":"1"}
grid {"start":[-1500,1000],"end":[9500,1000],"name":"A"}
room {"name":"Hall","number":"101","point":[2000,2000]}
"#,
    );
    if profile {
        script.push_str("cad_profile {\"name\":\"delivery\"}\n");
    }
    s.script(&script).unwrap();
    s
}

#[test]
fn with_the_profile_gridline_names_are_text_on_the_annotation_layer() {
    let strings = |ents: &[Ent], layer: &str| -> Vec<String> {
        let mut v: Vec<String> = ents
            .iter()
            .filter(|e| e.layer() == layer && ["TEXT", "MTEXT", "ATTRIB"].contains(&e.kind.as_str()))
            .filter_map(|e| e.get("1").map(str::to_string))
            .collect();
        v.sort();
        v.dedup();
        v
    };
    let (on, _) = parse(&plan_dxf(&mut gridded(true), "grid-on"));
    assert_eq!(strings(&on, "A-ANNO-GRID"), ["1", "A"]);
    assert!(on.iter().all(|e| e.layer() != "A-GRID-IDEN"));
    assert!(on.iter().any(|e| e.layer() == "A-GRID" && e.kind != "TEXT"));
    assert!(on.iter().filter(|e| e.layer() == "A-ANNO-GRID").all(|e| e.kind == "TEXT"));
    let (off, _) = parse(&plan_dxf(&mut gridded(false), "grid-off"));
    assert_eq!(strings(&off, "A-GRID-IDEN"), ["1", "A"]);
    assert!(off.iter().all(|e| e.layer() != "A-ANNO-GRID"));
}

#[test]
fn a_section_exported_with_the_profile_on_is_as_without_it() {
    let mut a = three_rooms(true);
    let mut b = three_rooms(false);
    for s in [&mut a, &mut b] {
        run(s, "section", json!({"start": [1000, 2000], "end": [7000, 2000], "name": "S1"}));
    }
    let (pa, pb) = (dir("section").join("a.dxf"), dir("section").join("b.dxf"));
    run(&mut a, "save", json!({"path": pa, "view": "S1"}));
    run(&mut b, "save", json!({"path": pb, "view": "S1"}));
    assert_eq!(std::fs::read(pa).unwrap(), std::fs::read(pb).unwrap());
}
