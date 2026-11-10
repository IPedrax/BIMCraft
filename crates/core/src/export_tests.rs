//! The `export` and `export_formats` commands, driven as an agent drives them, and `save`'s exports, which go through the same
//! formats. The engine's own parts (page layout, file name templates, each writer) are tested in `export/`.

use crate::engine::Session;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A box of walls with a room, the room schedule, and sheet A101 holding the Level 1 plan.
fn project() -> Session {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           room {"point": [4000, 3000], "name": "Living"}
           sheet {"name": "Plans"}
           place {"sheet": "A101", "view": "Level 1"}"#,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    s
}

/// A folder of its own for a test's files.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-export-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The paths `export` says it wrote, relative to `dir`, with `/` between folders.
fn written(r: &Value, dir: &Path) -> Vec<String> {
    let all = r["written"].as_array().cloned().unwrap_or_default();
    all.iter()
        .filter_map(|w| w["path"].as_str().map(PathBuf::from))
        .map(|p| p.strip_prefix(dir).unwrap_or(&p).iter().map(|c| c.to_string_lossy().to_string()).collect::<Vec<_>>().join("/"))
        .collect()
}

#[test]
fn export_formats_lists_each_format_and_what_it_takes() {
    let mut s = project();
    let r = run(&mut s, "export_formats", json!({}));
    let formats = r["formats"].as_array().cloned().unwrap_or_default();
    let of = |id: &str| formats.iter().find(|f| f["id"] == id).cloned().unwrap_or_else(|| panic!("{id} is listed: {r}"));
    assert_eq!(of("pdf")["status"], "stable");
    assert_eq!((of("pdf")["combines"].as_bool(), of("pdf")["paged"].as_bool()), (Some(true), Some(true)));
    assert_eq!(of("ifc4")["takes"], json!(["model"]));
    assert_eq!(of("csv")["family"], "schedule");
    // Listed before it is written, so a dialog shows it and an agent knows it's coming.
    assert_eq!(of("u3d")["status"], "planned");
    assert_eq!(of("jxl")["status"], "stable");
    for token in ["SheetNumber", "SheetName", "ViewName", "ProjectName", "Date"] {
        assert!(r["tokens"].as_array().is_some_and(|t| t.contains(&json!(token))), "{token}: {r}");
    }
}

#[test]
fn a_sheet_exports_to_one_pdf_at_path() {
    let mut s = project();
    let dir = folder("pdf");
    let to = dir.join("plans.pdf");
    let r = run(&mut s, "export", json!({"format": "pdf", "view": "A101", "path": to}));
    assert_eq!(written(&r, &dir), ["plans.pdf"], "{r}");
    assert_eq!(r["written"][0]["of"], json!(["A101 - Plans"]));
    let bytes = std::fs::read(&to).unwrap_or_default();
    assert!(bytes.starts_with(b"%PDF"));
    // By extension too, and written over at `path`: one file, as the agent asked.
    run(&mut s, "export", json!({"format": ".PDF", "view": "A101", "path": to}));
    assert_eq!(std::fs::read_dir(&dir).map(Iterator::count).unwrap_or(0), 1);
    let _ = std::fs::remove_dir_all(dir);
}

/// An export is readable as any file the user makes, not only by its owner as a temporary file is.
#[cfg(unix)]
#[test]
fn an_exported_file_is_made_as_any_file_is() {
    use std::os::unix::fs::PermissionsExt;
    let mut s = project();
    let dir = folder("mode");
    std::fs::create_dir_all(&dir).unwrap();
    let plain = dir.join("plain.txt");
    std::fs::File::create(&plain).unwrap();
    let to = dir.join("plan.svg");
    run(&mut s, "export", json!({"format": "svg", "view": "Level 1", "path": to}));
    let mode = |p: &Path| std::fs::metadata(p).map(|m| m.permissions().mode() & 0o777).unwrap_or(0);
    assert_eq!(mode(&to), mode(&plain));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn views_export_to_a_folder_named_by_a_template_and_numbered_rather_than_written_over() {
    let mut s = project();
    let dir = folder("folder");
    let p = json!({"format": "svg", "views": ["Level 1", "A101"], "folder": dir, "name": "{ViewType}/{ViewName} {SheetNumber}"});
    let r = run(&mut s, "export", p.clone());
    let mut got = written(&r, &dir);
    got.sort();
    assert_eq!(got, ["Floor Plan/Level 1.svg", "Sheet/Plans A101.svg"].map(String::from), "{r}");
    // Again: beside them, not over them.
    let r = run(&mut s, "export", p);
    assert!(written(&r, &dir).contains(&"Floor Plan/Level 1 (2).svg".to_string()), "{r}");
    // By format, in a folder each.
    let r = run(
        &mut s,
        "export",
        json!({"format": "png", "view": "Level 1", "folder": dir, "name": "{ViewName}", "folders": "by_format", "page": {"dpi": 36}}),
    );
    assert_eq!(written(&r, &dir), ["PNG/Level 1.png"], "{r}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn schedules_export_as_tables_and_a_plan_as_a_table_says_why_not() {
    let mut s = project();
    let dir = folder("tables");
    run(&mut s, "schedule", json!({"name": "Rooms", "category": "rooms", "fields": ["Name", "Area"]}));
    let name = "Rooms";
    for format in ["csv", "xlsx", "ods"] {
        let r = run(&mut s, "export", json!({"format": format, "view": name, "folder": dir}));
        assert_eq!(r["written"].as_array().map(Vec::len), Some(1), "{format}: {r}");
    }
    let csv = std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).find(|e| e.path().extension().is_some_and(|x| x == "csv"));
    assert!(std::fs::read_to_string(csv.unwrap().path()).unwrap_or_default().contains("Living"));
    let e = s.execute("export", &json!({"format": "csv", "view": "Level 1", "folder": dir})).unwrap_err();
    assert!(e.contains("nothing was written") && e.contains("schedules"), "{e}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_project_exports_as_ifc_from_the_origin_asked_for() {
    let mut s = project();
    let dir = folder("ifc");
    let to = dir.join("house.ifc");
    run(&mut s, "export", json!({"format": "ifc4", "path": to, "ifc_origin": "survey"}));
    let text = std::fs::read_to_string(&to).unwrap_or_default();
    assert!(text.contains("FILE_SCHEMA(('IFC4'))") && text.contains("IFCWALL"), "{text}");
    // As `save` writes it, from the same origin: the same file but for its time stamp.
    let saved = dir.join("saved.ifc");
    assert_eq!(run(&mut s, "save", json!({"path": saved, "ifc_origin": "survey"}))["format"], "IFC4");
    let data = |t: &str| t.split("DATA;").nth(1).map(String::from).unwrap_or_default();
    assert_eq!(data(&text), data(&std::fs::read_to_string(&saved).unwrap_or_default()));
    for c in ["export", "save"] {
        let p = if c == "export" {
            json!({"format": "ifc", "path": to, "ifc_origin": "moon"})
        } else {
            json!({"path": saved, "ifc_origin": "moon"})
        };
        assert!(s.execute(c, &p).unwrap_err().contains("internal, base, survey or shared"), "{c}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_project_exports_as_ifc4x3_with_the_attributes_it_added() {
    // The sample house, furnished: its families' shapes are meshes, each a point list.
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap_or_else(|e| panic!("{e}"));
    run(&mut s, "room_separator", json!({"points": [[4000, 0], [4000, 6000]]}));
    let dir = folder("ifc4x3");
    let to = dir.join("house.ifc");
    run(&mut s, "export", json!({"format": "ifc4x3", "path": to}));
    let text = std::fs::read_to_string(&to).unwrap_or_default();
    assert!(text.contains("FILE_SCHEMA(('IFC4X3_ADD2'))"), "{text}");
    // A point list's tags, none; a room separation line a boundary.
    let lists: Vec<&str> = text.lines().filter(|l| l.contains("=IFCCARTESIANPOINTLIST3D(")).collect();
    assert!(!lists.is_empty() && lists.iter().all(|l| l.ends_with(",$);")), "{lists:?}");
    assert!(text.lines().any(|l| l.contains("=IFCVIRTUALELEMENT(") && l.ends_with(",.BOUNDARY.);")), "{text}");
    // IFC4's has neither.
    run(&mut s, "export", json!({"format": "ifc", "path": to}));
    let text = std::fs::read_to_string(&to).unwrap_or_default();
    assert!(text.contains("FILE_SCHEMA(('IFC4'))") && !text.contains(".BOUNDARY."));
    let _ = std::fs::remove_dir_all(dir);
}

/// The entities of `class` in a STEP file's text.
fn entities<'a>(text: &'a str, class: &str) -> Vec<&'a str> {
    text.lines().filter(|l| l.split_once('=').is_some_and(|(_, e)| e.starts_with(&format!("{class}(")))).collect()
}

#[test]
fn the_project_exports_as_ifc2x3_with_what_that_schema_has() {
    // The sample house, furnished (its families' shapes are meshes), with a room separation line, on a map.
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap_or_else(|e| panic!("{e}"));
    run(&mut s, "room_separator", json!({"points": [[4000, 0], [4000, 6000]]}));
    run(&mut s, "georeference", json!({"crs": "EPSG:31983", "eastings": 333_000_000, "northings": 7_394_000_000i64, "map_zone": "23S"}));
    let dir = folder("ifc2x3");
    let to = dir.join("house.ifc");
    run(&mut s, "export", json!({"format": "ifc2x3", "path": to}));
    let text = std::fs::read_to_string(&to).unwrap_or_default();
    assert!(text.contains("FILE_SCHEMA(('IFC2X3'))") && text.contains("ViewDefinition [CoordinationView_V2.0]"), "{text}");
    // Nothing only IFC4 has: its face sets, material sets, map conversion and type objects.
    for class in [
        "IFCTRIANGULATEDFACESET",
        "IFCCARTESIANPOINTLIST3D",
        "IFCMATERIALCONSTITUENTSET",
        "IFCMATERIALPROFILESET",
        "IFCMAPCONVERSION",
        "IFCPROJECTEDCRS",
        "IFCDOORTYPE",
        "IFCWINDOWTYPE",
        "IFCROOFTYPE",
        "IFCFURNITURE",
    ] {
        assert!(entities(&text, class).is_empty(), "{class} isn't IFC2x3's");
    }
    // One owner history, which every rooted entity has: the walls, their type, the relations.
    let [owner] = entities(&text, "IFCOWNERHISTORY")[..] else { panic!("one owner history: {text}") };
    let owner = format!(",#{},", owner.split_once('=').map_or("", |(n, _)| n.trim_start_matches('#')));
    for class in ["IFCWALL", "IFCWALLTYPE", "IFCRELAGGREGATES", "IFCPROPERTYSET", "IFCPROJECT", "IFCBUILDINGSTOREY", "IFCDOOR"] {
        let all = entities(&text, class);
        assert!(!all.is_empty() && all.iter().all(|l| l.contains(&owner)), "{class}: {all:?}");
    }
    // A wall has no predefined type there, a door no operation: the door's is its style's.
    assert!(entities(&text, "IFCWALL").iter().all(|l| !l.contains(".STANDARD.")));
    let styles = entities(&text, "IFCDOORSTYLE");
    assert!(!styles.is_empty() && styles.iter().all(|l| l.ends_with(",.NOTDEFINED.,.F.,.F.);")), "{styles:?}");
    assert!(!entities(&text, "IFCWINDOWSTYLE").is_empty());
    // Furniture is a furnishing element, its mesh a faceted B-rep, its styles in an assignment.
    assert!(!entities(&text, "IFCFURNISHINGELEMENT").is_empty() && text.contains("'Body','Brep'"));
    assert!(!text.contains("'Tessellation'"));
    assert_eq!(entities(&text, "IFCSTYLEDITEM").len(), entities(&text, "IFCPRESENTATIONSTYLEASSIGNMENT").len());
    // Where it lies on a map is the project's two property sets, with the map conversion's own values (metres).
    assert!(text.contains("'ePSet_MapConversion'") && text.contains("'ePSet_ProjectedCRS'"), "{text}");
    assert!(text.contains("IFCPROPERTYSINGLEVALUE('Eastings',$,IFCLENGTHMEASURE(333000.),$)"), "{text}");
    assert!(text.contains("IFCPROPERTYSINGLEVALUE('MapZone',$,IFCIDENTIFIER('23S'),$)"), "{text}");
    // It reads back as the project it was: its levels and as many walls, doors and windows.
    let count = |s: &Session, category: &str| s.model.elements.values().filter(|e| e.category() == category).count();
    let mut back = Session::new();
    run(&mut back, "open", json!({"path": to}));
    for category in ["level", "wall", "door", "window", "floor", "roof"] {
        assert_eq!(count(&back, category), count(&s, category), "{category}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_bar_and_a_structural_frame_export_as_ifc2x3() {
    // A swept disk says where it starts and ends there; a beam's and a footing's material is its own, with no profile set.
    let mut s = Session::new();
    s.script(include_str!("../../../ci/ifc-coverage.txt")).unwrap_or_else(|e| panic!("{e}"));
    let dir = folder("ifc2x3-frame");
    let to = dir.join("frame.ifc");
    run(&mut s, "export", json!({"format": "ifc2x3", "path": to}));
    let text = std::fs::read_to_string(&to).unwrap_or_default();
    let disks = entities(&text, "IFCSWEPTDISKSOLID");
    assert!(!disks.is_empty() && disks.iter().all(|l| l.ends_with(",$,0.,1.);")), "{:?}", disks.first());
    let bars = entities(&text, "IFCREINFORCINGBAR");
    assert!(!bars.is_empty() && bars.iter().all(|l| l.ends_with(",.NOTDEFINED.,.PLAIN.);")), "{:?}", bars.first());
    // IFC4's pipe, duct and cable segments are flow segments, each still of its own segment type.
    assert!(entities(&text, "IFCPIPESEGMENT").is_empty() && !entities(&text, "IFCFLOWSEGMENT").is_empty());
    assert!(!entities(&text, "IFCPIPESEGMENTTYPE").is_empty() && !entities(&text, "IFCBEAMTYPE").is_empty());
    assert!(entities(&text, "IFCFOOTINGTYPE").is_empty() && !entities(&text, "IFCFOOTING").is_empty());
    assert!(!text.contains("IFCMATERIALPROFILE"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn save_prints_every_sheet_through_the_pdf_format() {
    let mut s = project();
    run(&mut s, "sheet", json!({"name": "More"}));
    let dir = folder("save-pdf");
    std::fs::create_dir_all(&dir).unwrap();
    let to = dir.join("set.pdf");
    let r = run(&mut s, "save", json!({"path": to}));
    assert_eq!((r["format"].as_str(), r["pages"].as_u64()), (Some("PDF"), Some(2)), "{r}");
    let pdf = String::from_utf8_lossy(&std::fs::read(&to).unwrap_or_default()).to_string();
    // A page each, and a bookmark each, in the order of their numbers.
    assert_eq!(pdf.matches("/Type/Page/").count(), 2, "{pdf}");
    assert!(pdf.contains("/Outlines"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn what_export_cant_do_it_says() {
    let mut s = project();
    let dir = folder("refused");
    let to = dir.join("x.pdf");
    let refused = [
        (json!({"view": "A101", "path": to}), "`format` is required"),
        (json!({"format": "doc", "view": "A101", "path": to}), "doc"),
        (json!({"format": "u3d", "path": to}), "isn't written yet"),
        (json!({"format": "pdf", "view": "Nowhere", "path": to}), "no view or sheet `Nowhere`"),
        (json!({"format": "pdf", "view": "A101"}), "`path` (one file) or `folder` (a file each) is required"),
        (json!({"format": "pdf", "view": "A101", "path": to, "folder": dir}), "not both"),
        (json!({"format": "pdf", "view": "A101", "views": ["A101"], "path": to}), "not both"),
        (json!({"format": "pdf", "views": [], "path": to}), "1 to 10000"),
        (json!({"format": "pdf", "view": "A101", "folder": dir, "name": "../{SheetNumber}"}), ".."),
        (json!({"format": "pdf", "view": "A101", "folder": dir, "name": "{Nope}"}), "SheetNumber"),
        (json!({"format": "pdf", "view": "A101", "path": to, "page": {"paper": "A99"}}), "A99"),
        (json!({"format": "pdf", "view": "A101", "path": to, "page": {"colour": "pink"}}), "`page`"),
        (json!({"format": "pdf", "view": "A101", "path": to, "quality": 0}), "1 to 100"),
        (json!({"format": "ifc4", "view": "A101", "path": to}), "the model or a 3D view"),
        (json!({"format": "pdf", "path": to}), "project can't be written as PDF: it takes views and sheets"),
    ];
    for (p, says) in refused {
        let e = s.execute("export", &p).map(|r| r.to_string()).unwrap_or_else(|e| e);
        assert!(e.contains(says), "{p}: {e}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// What the hostile sweep (`tests::hostile_params_never_panic_or_break_the_model`) runs against `export`: its real keys with
/// values wrong in the ways its checks look for, writing (where they write at all) under `dir`.
pub(crate) fn sweep_payloads(dir: &Path) -> Vec<(&'static str, Value)> {
    let to = dir.join("sweep.pdf");
    let pages = [
        json!({"paper": "A3", "size": [1e300, 1e300]}),
        json!({"size": [-1, 5], "margin": 1e300}),
        json!({"size": [f64::MAX, 1], "scale": {"custom": 1e-300}}),
        json!({"scale": {"custom": -5}, "dpi": 4_000_000_000u64}),
        json!({"scale": {"custom": 1e300}, "pages": [5, 1]}),
        json!({"scale": "fit", "pages": [0, u32::MAX], "anchor": "nowhere"}),
        json!({"dpi": 1200, "paper": "A0", "scale": "fit"}),
        json!({"orientation": 5, "hairlines": "yes", "colour": null}),
        json!({"unknown": true}),
        json!("A3"),
    ];
    let mut out: Vec<(&'static str, Value)> = pages
        .into_iter()
        .flat_map(|page| {
            [
                ("export", json!({"format": "pdf", "view": "A101", "path": to, "page": page})),
                ("export", json!({"format": "png", "view": "Level 1", "path": dir.join("sweep.png"), "page": page})),
            ]
        })
        .collect();
    out.extend([
        ("export", json!({"format": "pdf", "views": vec!["Level 1"; 10_001], "path": to})),
        ("export", json!({"format": "pdf", "views": [1e300, -1, "x", null, {}], "path": to})),
        ("export", json!({"format": "svg", "views": ["Level 1", "{3D}", "A101"], "folder": dir, "name": "{{".repeat(400)})),
        ("export", json!({"format": "svg", "view": "Level 1", "folder": dir, "name": "a/".repeat(100)})),
        ("export", json!({"format": "svg", "view": "Level 1", "folder": dir, "name": "CON\u{7}<>:|?*. "})),
        ("export", json!({"format": "svg", "view": "Level 1", "folder": dir, "name": "x".repeat(100_000)})),
        ("export", json!({"format": "pdf", "views": ["A101", "Level 1"], "folder": dir, "combine": "{ViewName}"})),
        ("export", json!({"format": "pdf", "view": "A101", "folder": "", "overwrite": "yes"})),
        ("export", json!({"format": "glb", "view": "Level 1", "path": dir.join("sweep.glb")})),
        ("export", json!({"format": "jpeg", "view": "A101", "path": dir.join("sweep.jpg"), "quality": 100, "page": {"dpi": 1200, "paper": "A0"}})),
        ("export", json!({"format": "jpeg", "view": "Level 1", "path": dir.join("sweep.jpg"), "quality": 0.5, "page": {"dpi": 36, "colour": "black"}})),
        ("export", json!({"format": "jxl", "view": "Level 1", "path": dir.join("sweep.jxl"), "page": {"dpi": 36, "size": [20, 20]}})),
        ("export", json!({"format": "dxf", "views": ["Level 1", "{3D}", "A101"], "folder": dir, "page": {"colour": "grey", "hairlines": true}})),
        ("export", json!({"format": "obj", "view": "{3D}", "path": dir.join("sweep.obj")})),
        ("export", json!({"format": "stl", "path": dir.join("sweep.stl")})),
        ("export", json!({"format": "ods", "view": "Room Schedule", "path": dir.join("sweep.ods")})),
        ("export", json!({"format": "ifc4", "path": dir.join("sweep.ifc"), "ifc_origin": 5})),
        ("export", json!({"format": "csv", "view": "{3D}", "path": dir.join("sweep.csv"), "quality": 1e300})),
        ("export", json!({"format": "x".repeat(100_000), "view": "A101", "path": to})),
        ("export_formats", json!({"anything": 1e300})),
    ]);
    out
}

#[test]
fn a_region_of_a_view_is_all_an_export_holds_and_a_region_outside_it_is_refused() {
    let mut s = project();
    let dir = folder("region");
    let whole = run(&mut s, "export", json!({"format": "png", "view": "Level 1", "path": dir.join("whole.png"), "page": {"dpi": 72}}));
    // The box's west half: its pixels a little over half as wide, the plan's height.
    let half = run(
        &mut s,
        "export",
        json!({"format": "png", "view": "Level 1", "path": dir.join("half.png"), "page": {"dpi": 72, "region": [-1000, -1000, 4000, 7000]}}),
    );
    let size = |r: &Value| {
        let path = r["written"][0]["path"].as_str().unwrap().to_string();
        let img = tiny_skia::Pixmap::decode_png(&std::fs::read(path).unwrap()).unwrap();
        (img.width(), img.height())
    };
    let ((ww, _), (hw, _)) = (size(&whole), size(&half));
    assert!(hw < ww * 3 / 4, "{hw} of {ww}");
    // A DXF of it is in the model's millimetres, inside the region.
    let r = run(
        &mut s,
        "export",
        json!({"format": "dxf", "view": "Level 1", "path": dir.join("half.dxf"), "page": {"region": [-1000, -1000, 4000, 7000]}}),
    );
    let dxf = std::fs::read_to_string(r["written"][0]["path"].as_str().unwrap()).unwrap();
    let max_x: f64 = dxf[dxf.find("$EXTMAX").unwrap()..].lines().nth(2).unwrap().trim().parse().unwrap();
    assert!(max_x <= 4000.0 + 1e-6, "{max_x}");
    let e = s
        .execute("export", &json!({"format": "png", "view": "Level 1", "folder": dir, "page": {"region": [90000, 90000, 99000, 99000]}}))
        .unwrap_err();
    assert!(e.contains("holds nothing of the view"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_pdf_is_written_as_an_archive_or_as_pictures_as_its_settings_say() {
    let mut s = project();
    let dir = folder("pdfa");
    let r = run(
        &mut s,
        "export",
        json!({"format": "pdf", "view": "A101", "path": dir.join("a.pdf"), "pdf": {"archive": "a2b", "title": "Plans", "author": "Office"}}),
    );
    let pdf = String::from_utf8_lossy(&std::fs::read(r["written"][0]["path"].as_str().unwrap()).unwrap()).into_owned();
    assert!(pdf.contains("<pdfaid:part>2</pdfaid:part>") && pdf.contains("/OutputIntents") && pdf.contains("Office"));
    let r = run(
        &mut s,
        "export",
        json!({"format": "pdf", "view": "A101", "path": dir.join("r.pdf"), "pdf": {"raster": true}, "page": {"dpi": 50}}),
    );
    let pdf = String::from_utf8_lossy(&std::fs::read(r["written"][0]["path"].as_str().unwrap()).unwrap()).into_owned();
    assert!(pdf.contains("/Subtype/Image"), "the page is a picture");
    let e = s.execute("export", &json!({"format": "pdf", "view": "A101", "folder": dir, "pdf": {"archive": "a9"}})).unwrap_err();
    assert!(e.contains("`pdf`"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}
