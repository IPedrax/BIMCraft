//! DWG drawings on the command line, read as the apps read them: in a child process of the program itself
//! (`bimcraft-cli --dwg-to-dxf`), so a file that brings the decoder down takes only the child with it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const EXE: &str = env!("CARGO_BIN_EXE_bimcraft-cli");

fn pairs(list: &[(i32, &str)]) -> String {
    list.iter().map(|(c, v)| format!("{c:>3}\n{v}\n")).collect()
}

/// A wall 10 m long and a column on layers of their own, drawn in metres (AutoCAD 2004), written as DWG by the bridge's library.
fn dwg(dir: &Path) -> Result<PathBuf, String> {
    let header = pairs(&[(9, "$ACADVER"), (1, "AC1018"), (9, "$INSUNITS"), (70, "6")]);
    let line = pairs(&[
        (0, "LINE"),
        (100, "AcDbEntity"),
        (8, "WALLS"),
        (100, "AcDbLine"),
        (10, "0"),
        (20, "0"),
        (30, "0"),
        (11, "10"),
        (21, "0"),
        (31, "0"),
    ]);
    let circle =
        pairs(&[(0, "CIRCLE"), (100, "AcDbEntity"), (8, "COLS"), (100, "AcDbCircle"), (10, "5"), (20, "3"), (30, "0"), (40, "0.2")]);
    let section = |name: &str, body: &str| format!("{}{body}{}", pairs(&[(0, "SECTION"), (2, name)]), pairs(&[(0, "ENDSEC")]));
    let mut table = pairs(&[(0, "TABLE"), (2, "LAYER"), (70, "2")]);
    for layer in ["WALLS", "COLS"] {
        table += &pairs(&[
            (0, "LAYER"),
            (100, "AcDbSymbolTableRecord"),
            (100, "AcDbLayerTableRecord"),
            (2, layer),
            (70, "0"),
            (62, "7"),
            (6, "Continuous"),
        ]);
    }
    table += &pairs(&[(0, "ENDTAB")]);
    let dxf = section("HEADER", &header) + &section("TABLES", &table) + &section("ENTITIES", &(line + &circle)) + &pairs(&[(0, "EOF")]);
    let path = dir.join("site.dwg");
    std::fs::write(&path, bimcraft_dwg::dwg_from_dxf(&dxf)?).map_err(|e| e.to_string())?;
    Ok(path)
}

fn dir(test: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("bimcraft-cli-dwg-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

#[test]
fn import_cad_reads_a_dwg_through_a_child_of_the_program() {
    let path = dwg(&dir("import").unwrap()).unwrap();
    let cmd = format!("import_cad {}", serde_json::json!({"path": path, "layers": "all"}));
    let out = Command::new(EXE).args(["run", "--cmd", &cmd]).output().unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{said}\n{}", String::from_utf8_lossy(&out.stderr));
    let reply: serde_json::Value = serde_json::from_str(said.lines().last().unwrap()).unwrap();
    assert_eq!(
        (reply["source"].as_str(), reply["unit"].as_str(), reply["entities"].as_u64()),
        (Some("site.dwg"), Some("m"), Some(2)),
        "{reply}"
    );
}

#[test]
fn the_child_gives_dxf_and_a_child_that_fails_or_runs_too_long_says_so() {
    let dir = dir("child").unwrap();
    let path = dwg(&dir).unwrap();
    let read = bimcraft_dwg::in_child(Path::new(EXE), &path, Duration::from_secs(60)).unwrap();
    assert!(String::from_utf8_lossy(&read.dxf).contains("WALLS"));
    // A file that isn't a DWG: the child says why, and the parent passes it on.
    let bad = dir.join("bad.dwg");
    std::fs::write(&bad, "AC1032 not a drawing at all").unwrap();
    let e = bimcraft_dwg::in_child(Path::new(EXE), &bad, Duration::from_secs(60)).unwrap_err();
    assert!(!e.is_empty() && !e.contains("took longer"), "{e}");
    // Out of time: stopped.
    let e = bimcraft_dwg::in_child(Path::new(EXE), &path, Duration::ZERO).unwrap_err();
    assert!(e.contains("took longer"), "{e}");
    // A program that isn't there: the reader couldn't start.
    let e = bimcraft_dwg::in_child(&dir.join("nowhere.exe"), &path, Duration::from_secs(5)).unwrap_err();
    assert!(e.contains("couldn't start"), "{e}");
}

#[test]
fn export_writes_a_plan_as_dwg_through_a_child_and_it_reads_back() {
    let dir = dir("export").unwrap();
    let to = dir.join("plan.dwg");
    let export = format!("export {}", serde_json::json!({"format": "dwg", "view": "Level 1", "path": to}));
    let formats = Command::new(EXE).args(["run", "--sample", "--cmd", &export, "--cmd", "export_formats"]).output().unwrap();
    let said = String::from_utf8_lossy(&formats.stdout);
    assert!(formats.status.success(), "{said}\n{}", String::from_utf8_lossy(&formats.stderr));
    // Listed as written, but experimental.
    let list: serde_json::Value = serde_json::from_str(said.lines().last().unwrap()).unwrap();
    let dwg = list["formats"].as_array().unwrap().iter().find(|f| f["id"] == "dwg").cloned().unwrap();
    assert_eq!(dwg["status"], "experimental");
    let bytes = std::fs::read(&to).unwrap();
    assert!(bytes.starts_with(b"AC1018"), "{:?}", String::from_utf8_lossy(&bytes[..6.min(bytes.len())]));
    // And it opens again as any DWG does, its layers by category.
    let cmd = format!("import_cad {}", serde_json::json!({"path": to, "layers": "all"}));
    let out = Command::new(EXE).args(["run", "--cmd", &cmd]).output().unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{said}\n{}", String::from_utf8_lossy(&out.stderr));
    let reply: serde_json::Value = serde_json::from_str(said.lines().last().unwrap()).unwrap();
    assert_eq!(reply["unit"].as_str(), Some("mm"), "{reply}");
    assert!(reply["entities"].as_u64().unwrap_or(0) > 100, "{reply}");
}

#[test]
fn the_writing_child_says_why_and_stops_when_asked() {
    let dir = dir("write-child").unwrap();
    let e = bimcraft_dwg::write_in_child(Path::new(EXE), b"not a drawing", Duration::from_secs(60), &|| false).unwrap_err();
    assert!(!e.is_empty() && !e.contains("took longer"), "{e}");
    let e = bimcraft_dwg::write_in_child(Path::new(EXE), b"0\nEOF\n", Duration::from_secs(60), &|| true).unwrap_err();
    assert_eq!(e, "cancelled");
    let e = bimcraft_dwg::write_in_child(&dir.join("nowhere.exe"), b"0\nEOF\n", Duration::from_secs(5), &|| false).unwrap_err();
    assert!(e.contains("couldn't start"), "{e}");
}
