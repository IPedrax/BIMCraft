//! Compare the complete piece stream against the direct mesh path on the same machine.
//! Trigonometric functions can differ by an ulp between platforms.

use super::*;
use crate::language::Language;
use serde_json::json;

/// The direct builder used before family forms went through the kernel. Build all forms first so
/// boolean errors and geometry errors keep their ordering, and spend one meter on all the cuts.
fn direct(family: &Family, values: &BTreeMap<String, Val>) -> Result<Vec<Piece>, String> {
    let mut read = HashMap::new();
    let mut pieces = Vec::new();
    let mut voids = Vec::new();
    for (i, form) in family.forms.iter().enumerate() {
        let Some(piece) = family.mesh_form(form, values, &mut read).map_err(|e| format!("form {}: {e}", i + 1))? else { continue };
        if !form.void {
            pieces.push(piece);
        } else if form.cuts().forms() {
            voids.push((i, pieces.len(), piece.tris));
        }
    }
    let mut cuts = csg::Meter::default();
    for (i, count, void) in voids {
        for piece in &mut pieces[..count] {
            let tris = csg::subtract(&piece.tris, &void, &mut cuts).map_err(|e| format!("form {}: {e}", i + 1))?;
            if tris != piece.tris {
                (piece.bottom, piece.top) = heights(&tris);
                piece.caps = tris.len();
                piece.round = false;
                piece.tris = tris;
            }
        }
    }
    pieces.retain(|p| !p.tris.is_empty());
    Ok(pieces)
}

fn families() -> BTreeMap<String, Family> {
    let mut families = library(Language::English);
    let ring = json!([{"start":[0,0],"end":[40,0]}, {"start":[40,0],"end":[40,30]},
                      {"start":[40,30],"end":[0,30]}, {"start":[0,30],"end":[0,0]}]);
    let circle = json!([{"centre":[10,10],"radius":8}]);
    let path = json!([{"start":[100,0],"end":[0,100],"turn":90}]);
    let forms = json!([
        {"sketch":ring,"from":0,"to":"H","material":"Panel","visibility":{"coarse":false}},
        {"sketch":circle,"from":-1,"to":"H + 1","void":true},
        {"sketch":ring,"from":0,"to":10,"void":true,"cuts":"host"},
        {"sketch":ring,"top":circle,"from":0,"to":80,"twist":1},
        {"sketch":path,"sweep":{"profile":ring}},
        {"sketch":path,"sweep":{"profile":ring,"end":circle}},
        {"profile":[[100,0],[140,0],[140,30],[100,30]],"axis":[[0,0],[0,1]],"to":225},
        {"sketch":ring,"to":10,"visible":0},
        {"mesh":[[[0,0,0],[10,0,0],[0,10,0]]]}
    ]);
    let f = serde_json::from_value(json!({"parameters":[{"name":"H","value":20}],"types":{"A":{},"B":{"H":32}},"forms":forms})).unwrap();
    families.insert("All form operations".into(), f);
    families
}

fn bits(piece: &Piece) -> impl Iterator<Item = u64> + '_ {
    piece
        .tris
        .iter()
        .flatten()
        .flatten()
        .copied()
        .chain(piece.outline.iter().chain(piece.holes.iter().flatten()).flat_map(|p| [p.0, p.1]))
        .chain([piece.bottom, piece.top])
        .map(f64::to_bits)
}

#[test]
fn family_kernel_matches_the_direct_mesh_bit_for_bit() {
    let mut built = 0;
    for (name, family) in families() {
        for kind in family.types.keys() {
            let values = family.values(kind, &BTreeMap::new()).unwrap();
            let expected = direct(&family, &values).unwrap_or_else(|e| panic!("{name} {kind}: {e}"));
            let actual = family.pieces(&values).unwrap_or_else(|e| panic!("{name} {kind}: {e}"));
            built += actual.len();
            if name == "All form operations" {
                assert_eq!(actual.len(), 6);
            }
            assert_eq!(actual.len(), expected.len(), "{name} {kind}");
            for (a, b) in actual.iter().zip(&expected) {
                assert_eq!(a, b, "{name} {kind}");
                assert!(bits(a).eq(bits(b)), "{name} {kind}: a floating point bit changed");
            }
        }
    }
    assert!(built > 100, "{built} pieces");
}

/// Compare dispatch and the direct mesh baseline in the same executable. Interleave the order to
/// limit warm-cache bias; each pass builds every type in the corpus once on the calling thread.
#[test]
#[ignore = "timing comparison; run on an idle machine with --nocapture"]
fn time_family_form_kernel_against_direct_mesh() {
    use std::{hint::black_box, time::Instant};

    let families = families();
    let mut cases = Vec::new();
    for family in families.values() {
        for kind in family.types.keys() {
            cases.push((family, family.values(kind, &BTreeMap::new()).unwrap()));
        }
    }
    let run = |direct_mesh: bool| {
        let start = Instant::now();
        for (family, values) in &cases {
            let pieces = if direct_mesh {
                direct(family, values)
            } else {
                family.build(&MeshKernel, values, &mut HashMap::new(), &mut csg::Meter::default())
            };
            black_box(pieces.unwrap());
        }
        start.elapsed()
    };
    run(true);
    run(false);
    for pass in 0..6 {
        let direct_first = pass % 2 == 0;
        let first = run(direct_first);
        let second = run(!direct_first);
        let (mesh, kernel) = if direct_first { (first, second) } else { (second, first) };
        println!("{} family types: direct mesh {mesh:?}, kernel {kernel:?}", cases.len());
    }
}
