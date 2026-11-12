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

#[test]
fn the_limits_hold_the_largest_real_drawings_and_the_dxf_one_is_the_readers() {
    // Measured on real projects: a DWG of 77 MB, and 301 MB of DXF from one of 38 MB.
    assert_eq!(Limits::DEFAULT.dwg_bytes, 128 << 20);
    assert_eq!(Limits::DEFAULT.dxf_bytes, bimcraft_core::dxf::MAX_BYTES);
}
