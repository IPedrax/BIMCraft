//! Reference planes and lines (F2, T1, T18): every family's three datums, the planes and lines a family adds, the checks on
//! them, forms hosted on any of them, and files of 17 brought up to 18 building the same triangles.

use std::collections::{BTreeMap, HashMap};

use serde_json::{Value, json};

use crate::engine::Session;
use crate::family::{CENTER_FRONT_BACK, CENTER_LEFT_RIGHT, DATUMS, Family, PlaneRef, REF_LEVEL, Reference};
use crate::model::Model;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn err(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(r) => panic!("{id} {p} should have been refused, and did {r}"),
        Err(e) => e,
    }
}

const SHELF: &str = "1b2c3d4e-5f60-4a71-8b92-a3b4c5d6e7f8";
const RAIL: &str = "2c3d4e5f-6071-4b82-9ca3-b4c5d6e7f809";

/// A family `P` with a length `D` of 30 and one box on Ref. Level.
fn family(s: &mut Session) {
    run(
        s,
        "family",
        json!({"name": "P", "parameters": [{"name": "D", "value": 30}], "types": {"A": {}},
               "forms": [{"profile": [[0, 0], [100, 0], [100, 50], [0, 50]], "to": 10}]}),
    );
}

/// A plane of the family's own, as `ref_planes` takes it.
fn plane(name: &str, start: [f64; 3], end: [f64; 3], up: [f64; 3]) -> Value {
    json!({"name": name, "start": start, "end": end, "up": up})
}

/// The box family `P`'s form `i` fills, and the volume it encloses (positive: its triangles face out).
fn built(s: &Session, i: usize) -> ([f64; 3], [f64; 3], f64) {
    let f = &s.model.families["P"];
    let values = f.values("A", &BTreeMap::new()).unwrap();
    let piece = f.solid(i, &values, &mut HashMap::new()).unwrap().unwrap();
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in piece.tris.iter().flatten() {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let volume = piece.tris.iter().map(|[a, b, c]| crate::view3d::dot(*a, crate::view3d::cross(*b, *c)) / 6.0).sum();
    (lo.map(|x| x + 0.0), hi.map(|x| x + 0.0), volume)
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
}

/// Each form's own triangles, before any void cuts them, as a number that doesn't depend on the order they come in or which
/// corner each starts at (its turn kept), and the volume the cut pieces enclose: what a family builds, for comparing builds.
fn fingerprint(f: &Family, values: &BTreeMap<String, crate::family::Val>) -> (u64, f64) {
    let q = |x: f64| (x * 1e4).round() as i64;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |x: i64| {
        for b in x.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    let mut read = HashMap::new();
    for i in 0..f.forms.len() {
        let Ok(Some(piece)) = f.solid(i, values, &mut read) else {
            eat(-1);
            continue;
        };
        let mut tris: Vec<[[i64; 3]; 3]> = piece
            .tris
            .iter()
            .map(|t| {
                let t = t.map(|p| p.map(q));
                let k = (0..3).min_by_key(|&k| t[k]).unwrap_or(0);
                [t[k], t[(k + 1) % 3], t[(k + 2) % 3]]
            })
            .collect();
        tris.sort_unstable();
        eat(tris.len() as i64);
        tris.iter().flatten().flatten().for_each(|x| eat(*x));
    }
    let volume = f
        .pieces(values)
        .unwrap_or_default()
        .iter()
        .flat_map(|p| p.tris.iter())
        .map(|[a, b, c]| crate::view3d::dot(*a, crate::view3d::cross(*b, *c)) / 6.0)
        .sum();
    (h, volume)
}

/// Every starter family's types (fixtures and openings with them), each with its fingerprint.
fn fingerprints() -> Vec<(String, String, u64, f64)> {
    let mut out = Vec::new();
    for (name, f) in crate::family::library(crate::language::Language::English) {
        for t in f.types.keys() {
            let values = f.values(t, &BTreeMap::new()).unwrap_or_default();
            let (h, v) = fingerprint(&f, &values);
            out.push((name.clone(), t.clone(), h, v));
        }
    }
    out
}

/// Every starter family's fingerprints as they were before reference planes, when forms named `plane` (xy, xz, yz).
const GOLDEN: &[(&str, &str, u64, f64)] = &[
    ("Air Terminal", "1000", 0x2de093015d7e5069, 245423.6250962893),
    ("Air Terminal", "2000", 0x6ac82befcd41b155, 323459.7539027402),
    ("Air Terminal", "500", 0x4da6678b14762d49, 206405.5606930642),
    ("Base Cabinet", "1200 x 600", 0x41bd84afe1c01099, 622560000.0),
    ("Base Cabinet", "1800 x 600", 0xadc728c607e22be1, 933840000.0),
    ("Base Cabinet", "600 x 600", 0x62053437739b3a25, 311280000.0),
    ("Bathtub", "1500 x 700", 0xdfab65f26af5e27d, 235824000.0),
    ("Bathtub", "1700 x 750", 0xb8390e74813aab81, 276024000.0),
    ("Bed", "1600 x 2000", 0xa5c7ee2d4bfce5ed, 1560960000.0),
    ("Bed", "1800 x 2000", 0x5ec63c5364fc607d, 1758960000.0),
    ("Bed", "900 x 2000", 0xf35f2584c2303be9, 871800000.0),
    ("Casement Window", "1200 x 1500", 0x2bfff8ff670f6c91, 40995360.00000003),
    ("Casement Window", "1800 x 1200", 0x72abfc818fa86c19, 46086960.0),
    ("Casement Window", "600 x 900", 0xbe581a99d9986ef9, 19282560.0),
    ("Ceiling Luminaire", "300", 0x04c17cd8e693405d, 3947067.3950303164),
    ("Ceiling Luminaire", "400", 0xe9d67a3df5661059, 7017008.702276107),
    ("Chair", "450 x 450", 0x70e9f8350d47ee99, 17924000.0),
    ("Data Outlet", "Low 300", 0x5b040b72d88f34f9, 80079.99999999996),
    ("Data Outlet", "Medium 1100", 0xeac2bd10ec62fdb1, 80079.99999999994),
    ("Desk", "1200 x 600", 0x78ef98d8bcc7c989, 50909999.99999998),
    ("Desk", "1400 x 700", 0x7f810fd4a7a0140d, 63900000.0),
    ("Distribution Board", "12 circuits", 0x2aeaa2e16bd7a215, 26086800.0),
    ("Distribution Board", "24 circuits", 0xebc62fead85e6d4d, 45778800.0),
    ("Distribution Board", "36 circuits", 0x01f3a1dd5a8b68f5, 70750800.0),
    ("Double Door", "1200 x 2100", 0x99a73af24d917151, 143564000.0),
    ("Double Door", "1500 x 2100", 0x828ff4288ba2c5ed, 171163999.99999997),
    ("Double Door", "1800 x 2100", 0xdc55689644589929, 198764000.0),
    ("Earthing Electrode", "2400", 0x978ade9bb7efaa21, 479453.97538683726),
    ("Earthing Electrode", "3000", 0x281db04737f8c6b9, 599317.4692335459),
    ("Elbow 90", "110", 0x7c041b305d9ee3c1, 4721185.792790297),
    ("Elbow 90", "25", 0x161d47fb71e6a9f1, 78036.12880645141),
    ("Elbow 90", "50", 0x6ce59221db8859d9, 507234.8372419333),
    ("Fixed Window", "1200 x 1500", 0x05f42c71533be979, 32640000.0),
    ("Fixed Window", "1800 x 1200", 0x4e91960177a95139, 37320000.0),
    ("Fixed Window", "600 x 900", 0x40895ab9338482b9, 15000000.0),
    ("Floor Drain", "100 x 100", 0xfc26458890370b85, 430120.9808890317),
    ("Floor Drain", "150 x 150", 0x5b0f4631c3fe4f69, 868128.8524325796),
    ("Gate Valve", "25", 0x0432e14b9d01680d, 100386.1638224235),
    ("Gate Valve", "50", 0x0377bdc4eea2fe4d, 686035.1173697115),
    ("Inspection Box", "250", 0xb05e3c2b86c11115, 4995872.966188988),
    ("Inspection Box", "300", 0x69d12f682920f4f1, 6299076.317256704),
    ("Kitchen Sink", "1200 x 550", 0xea47eb0a70597a89, 34167081.28981455),
    ("Kitchen Sink", "800 x 500", 0x16ad8469545e1ac5, 24407081.2898145),
    ("Outlet", "High 2100", 0x01e69b7755a0b771, 108294.5806790321),
    ("Outlet", "Low 300", 0xeb00a35cb658c7ed, 108294.58067903231),
    ("Outlet", "Medium 1100", 0x7a9217ad7f2c2d41, 108294.58067903233),
    ("Patch Panel", "24 ports", 0xdd224cd03a332649, 1062600.0),
    ("Patch Panel", "48 ports", 0xdd224cd03a332649, 1062600.0),
    ("Rack", "600 x 1000", 0x8e81c7cba2404605, 1226600000.0000005),
    ("Rack", "600 x 800", 0xe5fd2b5b07859ded, 986600000.0000001),
    ("Rack", "800 x 1000", 0xa7cfea56bcf4008d, 1636099999.9999993),
    ("Recessed Luminaire", "1200 x 300", 0x089e3b984b2dd45d, 12379500.0),
    ("Recessed Luminaire", "600 x 600", 0xe46f667ec11d40f9, 12424500.0),
    ("Shower", "800 x 800", 0xceb11558498dd181, 22301401.780945286),
    ("Shower", "900 x 900", 0x19b6991621433c49, 27151401.78094528),
    ("Single Door", "1000 x 2100", 0x29641aa105881551, 125064000.00000001),
    ("Single Door", "800 x 2100", 0x710aeacb52e2499d, 106663999.99999993),
    ("Single Door", "900 x 2100", 0x4faa272c9c6cab75, 115864000.00000004),
    ("Sliding Door", "1500 x 2100", 0x02d05087c10406c1, 175049999.99999994),
    ("Sliding Door", "2000 x 2100", 0xad20884c590b867d, 221050000.0),
    ("Sliding Door", "2400 x 2100", 0xf0bb98972e91972d, 257850000.00000003),
    ("Sliding Window", "1200 x 1500", 0xa4192e851186c365, 42652800.00000002),
    ("Sliding Window", "1800 x 1200", 0x43b11cf1689d4101, 47260800.00000003),
    ("Sliding Window", "600 x 900", 0x39cf54e43f0942c5, 20260799.999999993),
    ("Sofa", "1600 x 900", 0xcf15e41f80f205e5, 771760000.0000002),
    ("Sofa", "2000 x 900", 0xe715ccafeabaa811, 953360000.0),
    ("Switch", "Low 300", 0xc21c567ff183e699, 128640.0),
    ("Switch", "Medium 1100", 0x520fded95edb3ef1, 128640.0),
    ("Table", "1200 x 800", 0xb231eacca5023a3d, 48624000.0),
    ("Table", "1600 x 900", 0xc074da022d412bc9, 67824000.0),
    ("Table", "2000 x 1000", 0x0b45ca17b8da0215, 90224000.00000001),
    ("Tee", "110", 0xe345e530d7881e05, 5523787.377564635),
    ("Tee", "25", 0xfc57b8c3a4a0bab5, 98764.4755206651),
    ("Tee", "50", 0xfc5eb199203c9d15, 614534.5143508028),
    ("Three-Phase Outlet", "Balanced 1100", 0x7a9217ad7f2c2d41, 108294.58067903233),
    ("Three-Phase Outlet", "Unbalanced 1100", 0x7a9217ad7f2c2d41, 108294.58067903233),
    ("Voice Outlet", "Low 300", 0x615daee58bf22705, 82879.99999999999),
    ("Voice Outlet", "Medium 1100", 0xdffe3b5b9297a9f9, 82879.99999999999),
    ("WC", "400 x 650", 0xa8515cc1da318831, 100133929.17326434),
    ("Wardrobe", "1200 x 600", 0x4bc2dac424357775, 1502800000.0),
    ("Wardrobe", "900 x 600", 0xd6b23ca33d903575, 1126000000.0),
    ("Washbasin", "500 x 400", 0x011427ad2bab8079, 49445756.955970295),
    ("Washbasin", "600 x 450", 0xdf5e9ea02dd6ade5, 60645756.955970295),
];

#[test]
fn every_starter_family_builds_the_triangles_it_built_before_reference_planes() {
    let now = fingerprints();
    assert_eq!(now.len(), GOLDEN.len(), "the starter families' types");
    for ((f, t, h, v), (gf, gt, gh, gv)) in now.iter().zip(GOLDEN) {
        assert_eq!((f.as_str(), t.as_str()), (*gf, *gt));
        assert_eq!(h, gh, "{f} {t}: its forms' triangles");
        assert!((v - gv).abs() <= 1e-6 * gv.abs().max(1.0), "{f} {t}: its volume {v}, was {gv}");
    }
}

#[test]
fn every_family_holds_the_three_datums_first_and_pinned() {
    let mut s = Session::new();
    for (name, f) in s.model.families.iter() {
        let guids: Vec<_> = f.planes.iter().take(3).map(|p| p.guid).collect();
        assert_eq!(guids, DATUMS, "{name}");
    }
    family(&mut s);
    let r = run(&mut s, "family", json!({"name": "P"}));
    let planes = r["family"]["planes"].as_array().unwrap();
    let names: Vec<&str> = planes.iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Ref. Level", "Center (Front/Back)", "Center (Left/Right)"]);
    assert_eq!(planes[0]["level"], json!(true));
    assert_eq!(planes[0]["reference"], json!("bottom"));
    assert_eq!(planes[1]["reference"], json!("center_front_back"));
    assert_eq!(planes[2]["reference"], json!("center_left_right"));
    assert_eq!(r["family"]["forms"][0]["host"], json!({"plane": REF_LEVEL.to_string()}), "no host is Ref. Level");
    // A family read without them gets them; one that moved them in its file gets them back as they are.
    let bare: Family = serde_json::from_value(json!({"types": {"A": {}}})).unwrap();
    assert_eq!(bare.planes, crate::family::datums());
    let mut moved = serde_json::to_value(&bare).unwrap();
    moved["planes"][1]["start"] = json!([0, 500, 0]);
    let back: Family = serde_json::from_value(moved).unwrap();
    assert_eq!(back.planes, crate::family::datums());
    // Their frames: Center (Front/Back) x along +x, y up, normal -y; Center (Left/Right) x along +y, y up, normal +x.
    let fb = bare.frame(&PlaneRef::Plane(CENTER_FRONT_BACK)).unwrap();
    assert_eq!((fb.x, fb.y, fb.z), ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]));
    let lr = bare.frame(&PlaneRef::Plane(CENTER_LEFT_RIGHT)).unwrap();
    assert_eq!((lr.x, lr.y, lr.z), ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]));
    // Pinned: neither moved, changed nor deleted through the command, though given back as they are they're kept.
    let datum = REF_LEVEL.to_string();
    assert!(err(&mut s, "family", json!({"name": "P", "ref_plane": {"guid": datum, "start": [0, 0, 100]}})).contains("pinned"));
    assert!(err(&mut s, "family", json!({"name": "P", "delete_ref_plane": [datum]})).contains("pinned"));
    let mut planes = r["family"]["planes"].clone();
    planes[2]["name"] = json!("Middle");
    assert!(err(&mut s, "family", json!({"name": "P", "ref_planes": planes})).contains("pinned"));
    run(&mut s, "family", json!({"name": "P", "ref_planes": r["family"]["planes"]}));
    assert_eq!(s.model.families["P"].planes, crate::family::datums());
}

#[test]
fn a_family_refuses_planes_that_break_its_rules() {
    let mut s = Session::new();
    family(&mut s);
    let x = |name: &str, y: f64| plane(name, [0.0, y, 0.0], [1000.0, y, 0.0], [0.0, 0.0, 1000.0]);
    let refused = |s: &mut Session, planes: Value, says: &str| {
        let e = err(s, "family", json!({"name": "P", "ref_planes": planes}));
        assert!(e.contains(says), "{planes}: {e}");
    };
    refused(&mut s, json!([x("Front", 100.0), x("Front", 200.0)]), "named `Front`");
    let mut left = x("A", 100.0);
    left["reference"] = json!("left");
    let mut left2 = x("B", 200.0);
    left2["reference"] = json!("left");
    refused(&mut s, json!([left.clone(), left2]), "already");
    let mut bottom = x("C", 300.0);
    bottom["reference"] = json!("bottom");
    refused(&mut s, json!([bottom]), "Ref. Level");
    let mut o1 = x("O1", 100.0);
    o1["defines_origin"] = json!(true);
    let mut o2 = x("O2", 200.0);
    o2["defines_origin"] = json!(true);
    refused(&mut s, json!([o1.clone(), o2]), "defines the origin");
    let mut skew = plane("S", [0.0, 0.0, 0.0], [1000.0, 1000.0, 0.0], [0.0, 0.0, 1.0]);
    skew["defines_origin"] = json!(true);
    refused(&mut s, json!([skew]), "square to x, y or z");
    refused(&mut s, json!([plane("F", [0.0, 0.0, 0.0], [1e300, 0.0, 0.0], [0.0, 0.0, 1.0])]), "within");
    refused(&mut s, json!([plane("F", [5.0, 5.0, 5.0], [5.0, 5.0, 5.0], [0.0, 0.0, 1.0])]), "one point");
    refused(&mut s, json!([plane("F", [0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [-3.0, 0.0, 0.0])]), "along its line");
    let mut level = x("L", 100.0);
    level["level"] = json!(true);
    refused(&mut s, json!([level]), "only Ref. Level");
    // One plane may take the origin in a direction over from its datum; Left is one plane's.
    run(&mut s, "family", json!({"name": "P", "ref_planes": [o1, left]}));
    let f = &s.model.families["P"];
    let flags: Vec<bool> = f.planes.iter().map(|p| p.defines_origin).collect();
    assert_eq!(flags, [true, false, true, true, false], "Center (Front/Back) gave the origin in y to O1");
    assert_eq!(f.planes[4].reference, Reference::Left);
    // A line takes a named reference too, and lies on its host.
    let line = |reference: &str, z: f64| json!({"start": [0, 0, z], "end": [500, 0, z], "reference": reference});
    let e = err(&mut s, "family", json!({"name": "P", "ref_lines": [line("left", 0.0)]}));
    assert!(e.contains("already"), "{e}");
    let e = err(&mut s, "family", json!({"name": "P", "ref_lines": [line("weak", 50.0)]}));
    assert!(e.contains("doesn't lie on"), "{e}");
}

#[test]
fn a_form_is_drawn_on_a_plane_of_the_family_and_swept_along_its_normal() {
    let mut s = Session::new();
    family(&mut s);
    // A level plane 500 up: its origin the point of it over the family's.
    let mut shelf = plane("Shelf", [-200.0, 300.0, 500.0], [800.0, 300.0, 500.0], [0.0, 1000.0, 0.0]);
    shelf["guid"] = json!(SHELF);
    let form = s.model.families["P"].forms[0].id;
    run(&mut s, "family", json!({"name": "P", "ref_planes": [shelf], "form": {"id": form, "host": {"plane": SHELF}, "to": "D"}}));
    let (lo, hi, v) = built(&s, 0);
    assert!(near(lo, [0.0, 0.0, 500.0]) && near(hi, [100.0, 50.0, 530.0]) && v > 0.0, "{lo:?} {hi:?} {v}");
    // By the plane's id, as a command may name it.
    let id = s.model.families["P"].planes[3].id;
    assert!(id >= crate::ids::FIRST_ID, "a plane of its own is a part, with an id");
    run(&mut s, "family", json!({"name": "P", "form": {"id": form, "host": {"plane": id}}}));
    assert_eq!(s.model.families["P"].forms[0].host, PlaneRef::Plane(crate::ids::Guid::parse(SHELF).unwrap()));
    // An upright plane, its normal +x: profile x along +y, y up.
    let wall = plane("Side", [300.0, 0.0, 0.0], [300.0, 1000.0, 0.0], [0.0, 0.0, 1000.0]);
    let mut wall = wall;
    wall["guid"] = json!(RAIL);
    let kept = serde_json::to_value(&s.model.families["P"].planes[3]).unwrap();
    run(
        &mut s,
        "family",
        json!({"name": "P", "ref_planes": [kept, wall],
        "forms": [{"host": {"plane": RAIL}, "profile": [[0, 0], [100, 0], [100, 50], [0, 50]], "from": 0, "to": 20}]}),
    );
    let (lo, hi, v) = built(&s, 0);
    assert!(near(lo, [300.0, 0.0, 0.0]) && near(hi, [320.0, 100.0, 50.0]) && v > 0.0, "{lo:?} {hi:?} {v}");
    // A host the family has no plane of is refused.
    let e = err(
        &mut s,
        "family",
        json!({"name": "P", "forms": [{"host": {"plane": "3d4e5f60-7182-4c93-8db4-c5d6e7f8091a"},
        "profile": [[0, 0], [1, 0], [1, 1]], "to": 1}]}),
    );
    assert!(e.contains("isn't one of its planes"), "{e}");
    let e = err(
        &mut s,
        "family",
        json!({"name": "P", "forms": [{"host": {"plane": REF_LEVEL.to_string()}, "plane": "xz", "profile": [[0, 0], [1, 0], [1, 1]], "to": 1}]}),
    );
    assert!(e.contains("not both"), "{e}");
}

#[test]
fn a_form_is_drawn_on_each_of_a_reference_lines_four_planes() {
    let mut s = Session::new();
    family(&mut s);
    run(&mut s, "family", json!({"name": "P", "ref_lines": [{"guid": RAIL, "start": [0, 0, 0], "end": [1000, 0, 0]}]}));
    let square = json!([[0, 0], [100, 0], [100, 50], [0, 50]]);
    for (face, lo, hi) in [
        ("horizontal", [0.0, 0.0, 0.0], [100.0, 50.0, 10.0]),
        ("vertical", [0.0, -10.0, 0.0], [100.0, 0.0, 50.0]),
        ("start", [0.0, 0.0, 0.0], [10.0, 100.0, 50.0]),
        ("end", [1000.0, 0.0, 0.0], [1010.0, 100.0, 50.0]),
    ] {
        run(&mut s, "family", json!({"name": "P", "forms": [{"host": {"line": RAIL, "face": face}, "profile": square, "to": 10}]}));
        let (l, h, v) = built(&s, 0);
        assert!(near(l, lo) && near(h, hi) && v > 0.0, "{face}: {l:?} {h:?} {v}");
    }
    // A line drawn on another line's plane, and a form on it: the second line upright at the first's end.
    run(
        &mut s,
        "family",
        json!({"name": "P", "ref_lines": [{"guid": RAIL, "start": [0, 0, 0], "end": [1000, 0, 0]},
        {"guid": SHELF, "start": [1000, 0, 0], "end": [1000, 0, 400], "host": {"line": RAIL, "face": "vertical"}}],
        "forms": [{"host": {"line": SHELF, "face": "end"}, "profile": square, "to": 10}]}),
    );
    let (l, h, _) = built(&s, 0);
    assert!((l[2] - 400.0).abs() < 1e-6 && (h[2] - 410.0).abs() < 1e-6, "{l:?} {h:?}");
    // One drawn on a line listed after it is refused.
    let e = err(
        &mut s,
        "family",
        json!({"name": "P", "ref_lines": [
        {"start": [1000, 0, 0], "end": [1000, 0, 400], "host": {"line": RAIL, "face": "vertical"}},
        {"guid": RAIL, "start": [0, 0, 0], "end": [1000, 0, 0]}]}),
    );
    assert!(e.contains("listed after it"), "{e}");
}

#[test]
fn a_plane_or_line_something_is_drawn_on_is_not_deleted() {
    let mut s = Session::new();
    family(&mut s);
    let mut shelf = plane("Shelf", [0.0, 0.0, 500.0], [1000.0, 0.0, 500.0], [0.0, 1000.0, 0.0]);
    shelf["guid"] = json!(SHELF);
    run(
        &mut s,
        "family",
        json!({"name": "P", "ref_planes": [shelf],
        "ref_lines": [{"guid": RAIL, "start": [0, 0, 500], "end": [100, 0, 500], "host": {"plane": SHELF}}],
        "forms": [{"host": {"plane": SHELF}, "profile": [[0, 0], [10, 0], [10, 10]], "to": 1},
                  {"host": {"line": RAIL, "face": "start"}, "profile": [[0, 0], [10, 0], [10, 10]], "to": 1}]}),
    );
    let e = err(&mut s, "family", json!({"name": "P", "delete_ref_plane": [SHELF]}));
    assert!(e.contains("form 1") && e.contains("line 1"), "{e}");
    let e = err(&mut s, "family", json!({"name": "P", "delete_ref_line": [RAIL]}));
    assert!(e.contains("form 2"), "{e}");
    let e = err(&mut s, "family", json!({"name": "P", "delete_ref_plane": [CENTER_LEFT_RIGHT.to_string()]}));
    assert!(e.contains("pinned"), "{e}");
    // Moved off them in the same command, they go.
    run(
        &mut s,
        "family",
        json!({"name": "P", "forms": [{"profile": [[0, 0], [10, 0], [10, 10]], "to": 1}],
        "delete_ref_line": [RAIL], "delete_ref_plane": [SHELF]}),
    );
    let f = &s.model.families["P"];
    assert_eq!((f.planes.len(), f.lines.len()), (3, 0));
}

#[test]
fn planes_lines_and_hosts_come_back_from_a_file_and_a_copy_keeps_its_own() {
    let mut s = Session::new();
    family(&mut s);
    let mut shelf = plane("Shelf", [0.0, 0.0, 500.0], [1000.0, 0.0, 500.0], [0.0, 1000.0, 0.0]);
    shelf["guid"] = json!(SHELF);
    shelf["reference"] = json!("top");
    shelf["subcategory"] = json!("Shelves");
    run(
        &mut s,
        "family",
        json!({"name": "P", "ref_planes": [shelf],
        "ref_lines": [{"guid": RAIL, "start": [0, 0, 500], "end": [100, 0, 500], "host": {"plane": SHELF}, "reference": "strong"}],
        "forms": [{"host": {"plane": SHELF}, "profile": [[0, 0], [10, 0], [10, 10]], "to": 1},
                  {"host": {"line": RAIL, "face": "vertical"}, "profile": [[0, 0], [10, 0], [10, 10]], "to": 1}]}),
    );
    let path = std::env::temp_dir().join(format!("bimcraft-test-{}-planes.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": path}));
    let mut back = Session::new();
    run(&mut back, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(back.model.families["P"], s.model.families["P"]);
    // Saved as a family file and loaded again under another name: the copy's plane and line take GUIDs of their own,
    // and its forms follow them.
    let file = std::env::temp_dir().join(format!("bimcraft-test-{}-planes.json", std::process::id()));
    run(&mut s, "family", json!({"name": "P", "save": file}));
    run(&mut s, "family", json!({"name": "Q", "path": file}));
    let _ = std::fs::remove_file(&file);
    let (p, q) = (&s.model.families["P"], &s.model.families["Q"]);
    assert_eq!(p.planes[3].guid.to_string(), SHELF, "the first holder keeps it");
    assert_ne!(q.planes[3].guid, p.planes[3].guid);
    assert_eq!(q.forms[0].host, PlaneRef::Plane(q.planes[3].guid));
    assert_eq!(q.lines[0].host, PlaneRef::Plane(q.planes[3].guid));
    assert_eq!(q.forms[1].host, PlaneRef::Line(q.lines[0].guid, crate::family::Face::Vertical));
    assert_eq!(s.model.check(), Ok(()));
}

/// Every form of a family in a file, as a file of 17 holds it: `plane` where it says `host`, a form on Center (Front/Back)
/// swept along +y; no `planes`.
fn as_17(family: &mut Value) {
    let neg = |e: &Value| match e {
        Value::String(f) => match f.strip_prefix("-(").and_then(|f| f.strip_suffix(')')) {
            Some(inner) => json!(inner),
            None => json!(format!("-({f})")),
        },
        Value::Number(_) => json!(if e.as_f64() == Some(0.0) { 0.0 } else { -e.as_f64().unwrap() }),
        _ => json!(0.0),
    };
    let o = family.as_object_mut().unwrap();
    o.remove("planes");
    for form in o.get_mut("forms").and_then(Value::as_array_mut).into_iter().flatten() {
        let f = form.as_object_mut().unwrap();
        let Some(host) = f.remove("host") else { continue };
        let plane = match host["plane"].as_str().and_then(crate::ids::Guid::parse) {
            Some(REF_LEVEL) => "xy",
            Some(CENTER_FRONT_BACK) => "xz",
            Some(CENTER_LEFT_RIGHT) => "yz",
            _ => panic!("a starter form on {host}"),
        };
        if plane == "xz" && !f.contains_key("axis") {
            let (from, to) = (f.get("from").cloned().unwrap_or(json!(0.0)), f.get("to").cloned().unwrap_or(json!(0.0)));
            f.insert("from".into(), neg(&to));
            f.insert("to".into(), neg(&from));
        }
        f.insert("plane".into(), json!(plane));
    }
}

#[test]
fn a_file_of_17_opens_with_its_forms_on_the_datums_building_what_they_built() {
    let mut s = Session::new();
    // A family with forms across xz (numbers and formulas, and a revolve) and yz, as a command of 17 made it.
    run(
        &mut s,
        "family",
        json!({"name": "Old", "parameters": [{"name": "D", "value": 30}], "types": {"A": {}}, "forms": [
        {"plane": "xz", "profile": [[0, 0], [100, 0], [100, 50], [0, 50]], "from": 10, "to": "D"},
        {"plane": "yz", "profile": [[0, 0], [200, 0], [200, 40]], "from": 5, "to": 25},
        {"plane": "xz", "profile": [[100, 0], [300, 0], [300, 500], [100, 500]], "axis": [[0, 0], [0, 1]], "from": 0, "to": 90}]}),
    );
    let f = &s.model.families["Old"];
    assert_eq!(f.forms[0].host, PlaneRef::Plane(CENTER_FRONT_BACK));
    assert_eq!((&f.forms[0].from, &f.forms[0].to), (&crate::family::Expr::Formula("-(D)".into()), &crate::family::Expr::Number(-10.0)));
    assert_eq!((&f.forms[1].host, &f.forms[1].from), (&PlaneRef::Plane(CENTER_LEFT_RIGHT), &crate::family::Expr::Number(5.0)));
    assert_eq!(f.forms[2].to, crate::family::Expr::Number(90.0), "a revolve's angles are as they were");
    let values = f.values("A", &BTreeMap::new()).unwrap();
    let boxes = f.shapes(&values);
    let close = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
    assert!(close(boxes[0].min, [0.0, 10.0, 0.0]) && close(boxes[0].max, [100.0, 30.0, 50.0]), "{:?}", boxes[0]);
    assert!(close(boxes[1].min, [5.0, 0.0, 0.0]) && close(boxes[1].max, [25.0, 200.0, 40.0]), "{:?}", boxes[1]);
    // The whole project written as 17 writes it opens as it is now, every starter family with it.
    let mut file: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    file["format"] = json!(17);
    for family in file["families"].as_object_mut().unwrap().values_mut() {
        as_17(family);
    }
    let (opened, left_out) = Model::read(&file.to_string()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(opened.format, crate::model::FORMAT);
    for (name, f) in s.model.families.iter() {
        assert_eq!(&opened.families[name], f, "{name}");
        for t in f.types.keys() {
            let values = f.values(t, &BTreeMap::new()).unwrap();
            assert_eq!(fingerprint(&opened.families[name], &values), fingerprint(f, &values), "{name} {t}");
        }
    }
}

#[test]
#[ignore = "prints the table `GOLDEN` holds, as the starter families build now"]
fn print_fingerprints() {
    for (f, t, h, v) in fingerprints() {
        println!("    ({f:?}, {t:?}, {h:#018x}, {v:?}),");
    }
}
