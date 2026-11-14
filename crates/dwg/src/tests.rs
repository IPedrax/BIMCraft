//! The bridge driven as the apps drive it: DWG files made in the tests (a DXF written as DWG by the same library, since
//! Autodesk's own files are never committed) read back as DXF the core reads, and hostile files refused with a reason.

use bimcraft_core::dxf::{self, Shape};

use super::*;

/// Pairs as a DXF writes them: a code line, then its value.
fn pairs(list: &[(i32, &str)]) -> String {
    list.iter().map(|(c, v)| format!("{c:>3}\n{v}\n")).collect()
}

/// A room 10 m by 6 m in metres (an AutoCAD 2004 file): its walls on WALLS (red), a column as a circle on COLS, a label.
pub(crate) fn room_dxf() -> String {
    let header = pairs(&[(9, "$ACADVER"), (1, "AC1018"), (9, "$INSUNITS"), (70, "6")]);
    let mut table = pairs(&[(0, "TABLE"), (2, "LAYER"), (70, "2")]);
    for (layer, color) in [("WALLS", "1"), ("COLS", "5")] {
        table += &pairs(&[
            (0, "LAYER"),
            (100, "AcDbSymbolTableRecord"),
            (100, "AcDbLayerTableRecord"),
            (2, layer),
            (70, "0"),
            (62, color),
            (6, "Continuous"),
        ]);
    }
    table += &pairs(&[(0, "ENDTAB")]);
    let line = |a: (&str, &str), b: (&str, &str)| {
        pairs(&[
            (0, "LINE"),
            (100, "AcDbEntity"),
            (8, "WALLS"),
            (100, "AcDbLine"),
            (10, a.0),
            (20, a.1),
            (30, "0"),
            (11, b.0),
            (21, b.1),
            (31, "0"),
        ])
    };
    let mut entities =
        line(("0", "0"), ("10", "0")) + &line(("10", "0"), ("10", "6")) + &line(("10", "6"), ("0", "6")) + &line(("0", "6"), ("0", "0"));
    entities +=
        &pairs(&[(0, "CIRCLE"), (100, "AcDbEntity"), (8, "COLS"), (100, "AcDbCircle"), (10, "5"), (20, "3"), (30, "0"), (40, "0.2")]);
    entities += &pairs(&[
        (0, "TEXT"),
        (100, "AcDbEntity"),
        (8, "WALLS"),
        (100, "AcDbText"),
        (10, "1"),
        (20, "1"),
        (30, "0"),
        (40, "0.3"),
        (1, "Sala"),
        (100, "AcDbText"),
    ]);
    let section = |name: &str, body: &str| format!("{}{body}{}", pairs(&[(0, "SECTION"), (2, name)]), pairs(&[(0, "ENDSEC")]));
    section("HEADER", &header) + &section("TABLES", &table) + &section("ENTITIES", &entities) + &pairs(&[(0, "EOF")])
}

fn room_dwg() -> Vec<u8> {
    dwg_from_dxf(&room_dxf()).unwrap()
}

#[test]
fn a_dwg_comes_back_as_dxf_the_core_reads_with_its_layers_shapes_and_unit() {
    let dwg = room_dwg();
    assert!(is_dwg(&dwg), "{:?}", dwg.get(..6));
    let read = to_dxf(&dwg, Limits::DEFAULT).unwrap();
    assert!(read.notes.is_empty(), "{:?}", read.notes);
    let d = dxf::read(&read.dxf).unwrap();
    assert_eq!(d.unit, "m");
    let lines: Vec<_> = d.entities.iter().filter(|e| matches!(e.shape, Shape::Line(..))).collect();
    assert_eq!(lines.len(), 4);
    assert!(lines.iter().all(|e| e.layer == "WALLS"));
    assert!(d.entities.iter().any(|e| e.layer == "COLS" && matches!(e.shape, Shape::Circle { .. })));
    assert!(d.entities.iter().any(|e| matches!(&e.shape, Shape::Text { text, .. } if text.contains("Sala"))));
    let walls = d.layers.iter().find(|l| l.name == "WALLS").unwrap();
    assert_eq!(walls.color, dxf::Color::Index(1));
}

#[test]
fn a_file_that_isnt_a_dwg_or_is_too_large_or_broken_says_why_and_never_panics() {
    assert!(to_dxf(b"", Limits::DEFAULT).unwrap_err().contains("isn't a DWG"));
    assert!(to_dxf(b"  0\nSECTION\n", Limits::DEFAULT).unwrap_err().contains("isn't a DWG"));
    let small = Limits { dwg_bytes: 10, ..Limits::DEFAULT };
    assert!(to_dxf(&room_dwg(), small).unwrap_err().contains("larger than"));
    let tight = Limits { dxf_bytes: 1000, ..Limits::DEFAULT };
    assert!(to_dxf(&room_dwg(), tight).unwrap_err().contains("more than"));
    // Its header and nothing more, or garbage after it.
    assert!(to_dxf(b"AC1032 this is not a dwg file at all", Limits::DEFAULT).is_err());
    let dwg = room_dwg();
    for cut in [7, 64, 256, dwg.len() / 2, dwg.len() - 1] {
        let _ = to_dxf(&dwg[..cut.min(dwg.len())], Limits::DEFAULT);
    }
}

#[test]
fn bytes_changed_anywhere_in_a_dwg_never_panic() {
    let dwg = room_dwg();
    let mut seed = 0x2545_f491_u32;
    for _ in 0..200 {
        let mut bad = dwg.clone();
        for _ in 0..8 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let at = 6 + seed as usize % (bad.len() - 6);
            bad[at] = (seed >> 8) as u8;
        }
        let _ = to_dxf(&bad, Limits::DEFAULT);
    }
}

#[test]
fn a_file_too_large_is_refused_before_it_is_read() {
    let dir = std::env::temp_dir().join(format!("bimcraft-dwg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("big.dwg");
    std::fs::write(&path, room_dwg()).unwrap();
    let e = read_file(&path, Limits { dwg_bytes: 100, ..Limits::DEFAULT }).unwrap_err();
    assert!(e.contains("larger than"), "{e}");
    assert!(read_file(&dir.join("nowhere.dwg"), Limits::DEFAULT).is_err());
}

#[test]
fn only_a_childs_arguments_make_one() {
    assert!(child_main(&[]).is_none());
    assert!(child_main(&["plan.dwg".into()]).is_none());
    assert!(child_main(&["--sample".into(), "x".into()]).is_none());
}

/// Real drawings, read where `BIMCRAFT_DWG_SAMPLES` names a folder of them (they're never committed): each comes back as DXF
/// the core reads, or says why, and none takes the process down.
#[test]
fn real_drawings_where_a_folder_of_them_is_named() {
    let Some(dir) = std::env::var_os("BIMCRAFT_DWG_SAMPLES") else { return };
    let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(Result::ok).map(|e| e.path()).collect();
    files.retain(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dwg")));
    files.sort();
    for path in files {
        let start = std::time::Instant::now();
        let said = match read_file(&path, Limits::DEFAULT) {
            Ok(read) => match dxf::read(&read.dxf) {
                Ok(d) => format!(
                    "{} entities, unit {}, {} kB of DXF, left out {:?}, notes {:?}",
                    d.entities.len(),
                    d.unit,
                    read.dxf.len() >> 10,
                    d.left_out,
                    read.notes
                ),
                Err(e) => format!("DXF not read: {e}"),
            },
            Err(e) => format!("refused: {e}"),
        };
        eprintln!("{} ({} kB) in {:?}: {said}", path.display(), std::fs::metadata(&path).map_or(0, |m| m.len() >> 10), start.elapsed());
    }
}

/// The core's DXF of a plan of the walls of a box, a door in them, as an export writes it.
fn plan_dxf() -> Vec<u8> {
    use bimcraft_core::export::{Ctx, Format, Options, Takes, prepare};
    let mut s = bimcraft_core::engine::Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           door {"at": [2000, 0]}"#,
    )
    .unwrap();
    let m = bimcraft_core::link::seen(&s.model);
    let p = prepare(&s.model, &m, m.view_named("Level 1"), Takes::Drawing).unwrap();
    bimcraft_core::export::formats::Dxf.write(&[p], &Options::default(), &Ctx::quiet()).unwrap().remove(0).bytes
}

#[test]
fn a_plan_exported_as_dwg_reads_back_with_its_layers_where_it_was() {
    let dxf = plan_dxf();
    let dwg = to_dwg(&dxf, Limits::DEFAULT).unwrap();
    assert!(dwg.starts_with(b"AC1018"), "AutoCAD 2004: {:?}", String::from_utf8_lossy(&dwg[..6]));
    // Read back as any DWG is, then by the core as any DXF.
    let back = to_dxf(&dwg, Limits::DEFAULT).unwrap();
    assert!(back.notes.is_empty(), "{:?}", back.notes);
    let read = dxf::read(&back.dxf).unwrap();
    let before = dxf::read(&dxf).unwrap();
    assert_eq!(read.unit, "mm");
    for layer in ["wall", "door"] {
        let n = |d: &dxf::Dxf| d.entities.iter().filter(|e| e.layer == layer).count();
        assert!(n(&read) > 0 && n(&read) == n(&before), "{layer}: {} of {}", n(&read), n(&before));
    }
    // Where it was: the walls' outer faces just outside 0 and 8000.
    let [lo, hi] = dxf::union(read.entities.iter().filter(|e| e.layer == "wall").map(|e| e.shape.bounds())).unwrap();
    assert!(lo.0 < 0.0 && lo.0 > -400.0 && hi.0 > 8000.0 && hi.0 < 8400.0, "{lo:?} {hi:?}");
    assert!(read.entities.iter().any(|e| matches!(e.shape, Shape::Fill { solid: true, .. })), "the cut walls' fill");
    // The variables dictionary AutoCAD writes, in the DWG's named objects.
    let text = String::from_utf8_lossy(&back.dxf);
    assert!(text.contains("AcDbVariableDictionary") && text.contains("CANNOSCALE"), "the variables dictionary");
}

#[test]
fn what_isnt_a_drawing_or_is_too_large_isnt_written() {
    assert!(to_dwg(b"not a dxf at all", Limits::DEFAULT).is_err());
    let small = Limits { dwg_bytes: 64, dxf_bytes: 64 };
    assert!(to_dwg(&plan_dxf(), small).unwrap_err().contains("more than"));
    let roomy = Limits { dwg_bytes: 64, dxf_bytes: 64 << 20 };
    assert!(to_dwg(&plan_dxf(), roomy).unwrap_err().contains("more than"));
}

/// Contours as a survey draws them (an AutoCAD 2004 file, metres): two LWPOLYLINEs at their elevations and a 3D polyline.
fn contours_dxf() -> String {
    let header = pairs(&[(9, "$ACADVER"), (1, "AC1018"), (9, "$INSUNITS"), (70, "6")]);
    let lw = |z: &str, y: &str| {
        pairs(&[
            (0, "LWPOLYLINE"),
            (100, "AcDbEntity"),
            (8, "CONTOURS"),
            (100, "AcDbPolyline"),
            (90, "2"),
            (70, "0"),
            (38, z),
            (10, "0"),
            (20, y),
            (10, "10"),
            (20, y),
        ])
    };
    let vertex = |x: &str, z: &str| {
        pairs(&[
            (0, "VERTEX"),
            (100, "AcDbEntity"),
            (8, "CONTOURS"),
            (100, "AcDbVertex"),
            (100, "AcDb3dPolylineVertex"),
            (10, x),
            (20, "2"),
            (30, z),
            (70, "32"),
        ])
    };
    let table = pairs(&[(0, "TABLE"), (2, "LAYER"), (70, "1")])
        + &pairs(&[
            (0, "LAYER"),
            (100, "AcDbSymbolTableRecord"),
            (100, "AcDbLayerTableRecord"),
            (2, "CONTOURS"),
            (70, "0"),
            (62, "3"),
            (6, "Continuous"),
        ])
        + &pairs(&[(0, "ENDTAB")]);
    let mut entities = lw("2881", "0") + &lw("2882", "1");
    entities += &pairs(&[
        (0, "POLYLINE"),
        (100, "AcDbEntity"),
        (8, "CONTOURS"),
        (100, "AcDb3dPolyline"),
        (66, "1"),
        (10, "0"),
        (20, "0"),
        (30, "0"),
        (70, "8"),
    ]);
    entities += &(vertex("0", "2883") + &vertex("10", "2883.5"));
    entities += &pairs(&[(0, "SEQEND"), (100, "AcDbEntity"), (8, "CONTOURS")]);
    let section = |name: &str, body: &str| format!("{}{body}{}", pairs(&[(0, "SECTION"), (2, name)]), pairs(&[(0, "ENDSEC")]));
    section("HEADER", &header) + &section("TABLES", &table) + &section("ENTITIES", &entities) + &pairs(&[(0, "EOF")])
}

#[test]
fn a_dwgs_contours_keep_their_elevations() {
    let dwg = dwg_from_dxf(&contours_dxf()).unwrap();
    let read = to_dxf(&dwg, Limits::DEFAULT).unwrap();
    let d = dxf::read(&read.dxf).unwrap();
    let z: Vec<_> = d.entities.iter().filter(|e| e.layer == "CONTOURS").map(|e| e.z.clone()).collect();
    assert_eq!(z, [Some(dxf::Z::Flat(2881.0)), Some(dxf::Z::Flat(2882.0)), Some(dxf::Z::Each(vec![2883.0, 2883.5]))], "{:?}", d.entities);
}
