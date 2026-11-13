//! CAD drawings in the window, driven as a user drives them: Insert > Import CAD and Link CAD bring a DXF in through their
//! box, the Properties palette shows one and changes its colours, and Visibility/Graphics' Imported Categories hides the
//! category or a drawing's layers in a view.

use std::path::PathBuf;

use bimcraft_core::cad::Colors;

use super::*;

/// Pairs as a DXF writes them: a code line, then its value.
fn pairs(list: &[(i32, &str)]) -> String {
    list.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect()
}

/// A plan of a room 10 m by 6 m in metres: its walls on WALLS (red), a table on FURN (off in its file).
fn drawing(test: &str, name: &str) -> PathBuf {
    let line = |layer: &str, a: (f64, f64), b: (f64, f64)| {
        let (ax, ay, bx, by) = (a.0.to_string(), a.1.to_string(), b.0.to_string(), b.1.to_string());
        pairs(&[(0, "LINE"), (8, layer), (10, &ax), (20, &ay), (11, &bx), (21, &by)])
    };
    let walls: String = [((0.0, 0.0), (10.0, 0.0)), ((10.0, 0.0), (10.0, 6.0)), ((10.0, 6.0), (0.0, 6.0)), ((0.0, 6.0), (0.0, 0.0))]
        .iter()
        .map(|(a, b)| line("WALLS", *a, *b))
        .collect();
    let entities = walls + &line("FURN", (1.0, 1.0), (2.0, 1.0));
    let header = pairs(&[(9, "$INSUNITS"), (70, "6")]);
    let mut table = pairs(&[(0, "TABLE"), (2, "LAYER")]);
    for (layer, color) in [("WALLS", "1"), ("FURN", "-3")] {
        table += &pairs(&[(0, "LAYER"), (2, layer), (70, "0"), (62, color), (6, "Continuous")]);
    }
    table += &pairs(&[(0, "ENDTAB")]);
    let text = format!(
        "0\nSECTION\n2\nHEADER\n{header}0\nENDSEC\n0\nSECTION\n2\nTABLES\n{table}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n"
    );
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-cad-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn imports(h: &Harness<BimApp>) -> Vec<Id> {
    let m = &h.state().session.model;
    m.elements.iter().filter(|(_, e)| matches!(e, Element::CadImport(_))).map(|(id, _)| *id).collect()
}

fn import(h: &Harness<BimApp>, id: Id) -> bimcraft_core::model::CadImport {
    let Some(Element::CadImport(ci)) = h.state().session.model.elements.get(id) else { panic!("{id} isn't an import") };
    ci.clone()
}

#[test]
fn import_cad_brings_the_file_picked_in_through_its_box_and_selects_it() {
    let mut h = app(BOX);
    press(&mut h, "Insert");
    assert!(h.query_by_label("Link CAD").is_some(), "Link CAD, beside Link Model");
    // The system's dialog gives the file; a test hands it over as it would.
    h.state_mut().import_cad_path(drawing("import", "ground.dxf"), false);
    h.run();
    assert!(h.query_by_label("ground.dxf").is_some());
    assert_eq!(combo_value(&h, "Import units"), "Auto-Detect");
    type_into(&mut h, "Name", "ground floor");
    choose(&mut h, "Colors", "Black and White");
    choose(&mut h, "Layers", "All");
    pick(&mut h, Role::RadioButton, "Auto - Origin to Origin");
    pick(&mut h, Role::CheckBox, "Current view only");
    press(&mut h, "OK");
    let placed = imports(&h);
    assert_eq!(placed.len(), 1);
    let ci = import(&h, placed[0]);
    assert_eq!((ci.cad.as_str(), ci.colors, ci.hidden.is_empty()), ("ground floor", Colors::Black, true), "all its layers");
    assert!(ci.view.is_some(), "in this plan only");
    let m = &h.state().session.model;
    assert!(!m.cads["ground floor"].path().is_some(), "imported, not linked");
    // Origin to Origin: its corner on the project's origin, its metres read as such.
    let [lo, hi] = m.cad_box(placed[0]).unwrap();
    assert!(lo.0.abs() < 1.0 && lo.1.abs() < 1.0 && (hi.0 - 10_000.0).abs() < 1.0, "{lo:?} {hi:?}");
    assert_eq!(h.state().selection, placed, "what was brought in is selected, as Revit does");
    assert!(h.query_by_label("Import units").is_none(), "the box closed");
}

#[test]
fn link_cad_remembers_the_file_and_center_to_center_puts_its_middle_on_the_origin() {
    let mut h = app(BOX);
    h.state_mut().import_cad_path(drawing("link", "site.dxf"), true);
    h.run();
    assert!(h.query_by_label("Link CAD").is_some(), "the box says what it does");
    press(&mut h, "OK");
    let placed = imports(&h);
    let m = &h.state().session.model;
    assert!(m.cads["site"].path().is_some());
    let ci = import(&h, placed[0]);
    assert!(ci.view.is_none() && ci.hidden.contains("FURN"), "every plan of its level; the layers its file has off are hidden");
    let [lo, hi] = m.cad_box(placed[0]).unwrap();
    assert!((lo.0 + hi.0).abs() < 1.0 && (lo.1 + hi.1).abs() < 1.0, "{lo:?} {hi:?}");
}

#[test]
fn a_drawing_that_cant_come_in_says_why_in_its_box() {
    let mut h = app(BOX);
    let dir = drawing("dwg", "plan.dxf");
    let dwg = dir.with_file_name("plan.dwg");
    std::fs::write(&dwg, "AC1032 not a dxf").unwrap();
    h.state_mut().import_cad_path(dwg, false);
    h.run();
    press(&mut h, "OK");
    assert!(imports(&h).is_empty());
    assert!(h.query_by_label_contains("save it as DXF").is_some(), "the reason shows in the box, which stays open");
    press(&mut h, "Cancel");
    assert!(h.query_by_label("plan.dwg").is_none());
}

#[test]
fn properties_show_an_import_and_change_its_colours() {
    let path = drawing("properties", "plan.dxf");
    let mut h = app(&format!("import_cad {}", json!({"path": path})));
    let id = imports(&h)[0];
    h.state_mut().selection = vec![id];
    h.run();
    for label in ["Drawing", "Entities", "Position", "Linked File"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    assert_eq!(combo_value(&h, "Colors"), "Preserve");
    choose(&mut h, "Colors", "Invert");
    assert_eq!(import(&h, id).colors, Colors::Invert);
}

#[test]
fn imported_categories_hides_a_drawings_layers_or_the_category_in_the_view() {
    let path = drawing("vg", "plan.dxf");
    let mut h = app(&format!("{BOX}\nimport_cad {}", json!({"path": path, "layers": "all"})));
    key(&mut h, Key::Escape);
    shortcut(&mut h, "VV");
    press(&mut h, "Imported Categories");
    assert!(h.query_by_label("Model Categories").is_some(), "the tabs stay");
    // Its layers, under the drawing: WALLS cleared hides it in this view.
    pick(&mut h, Role::CheckBox, "WALLS");
    press(&mut h, "OK");
    let g = plan_graphics(&h, 100000);
    assert_eq!(g.layers.get("plan").map(|l| l.iter().cloned().collect::<Vec<_>>()), Some(vec!["WALLS".to_string()]));
    // Shown again, and the category hidden as a whole.
    shortcut(&mut h, "VV");
    press(&mut h, "Imported Categories");
    pick(&mut h, Role::CheckBox, "WALLS");
    pick(&mut h, Role::CheckBox, "CAD Imports");
    press(&mut h, "OK");
    let g = plan_graphics(&h, 100000);
    assert!(g.layers.is_empty(), "{:?}", g.layers);
    assert!(g.categories.get("cadimport").is_some_and(|o| o.hidden), "{g:?}");
}

#[test]
fn a_right_click_reloads_a_linked_drawing_from_its_file() {
    let path = drawing("reload", "site.dxf");
    let mut h = app(&format!("import_cad {}", json!({"path": path, "link": true, "layers": "all"})));
    let id = imports(&h)[0];
    let count = |h: &Harness<BimApp>| h.state().session.model.cads["site"].dxf().entities.len();
    assert_eq!(count(&h), 5);
    // The file changes: a line more.
    let text = std::fs::read_to_string(&path).unwrap();
    let more = pairs(&[(0, "LINE"), (8, "WALLS"), (10, "5"), (20, "0"), (11, "5"), (21, "6")]);
    std::fs::write(&path, text.replacen("0\nENDSEC\n0\nEOF", &format!("{more}0\nENDSEC\n0\nEOF"), 1)).unwrap();
    // Not offered for what isn't a linked drawing; offered for one.
    let at = h.state().screen(P2(0.0, 0.0));
    assert!(offered(&mut h, at, &["Reload CAD"]).is_empty());
    key(&mut h, Key::Escape);
    h.state_mut().selection = vec![id];
    h.run();
    assert_eq!(offered(&mut h, at, &["Reload CAD"]), ["Reload CAD"]);
    choose_item(&mut h, "Reload CAD");
    assert_eq!(count(&h), 6, "read again");
}

#[test]
fn a_dwg_comes_in_through_the_box_with_the_reader_the_app_is_given_and_its_notes_on_the_status_bar() {
    let mut h = app(BOX);
    // The desktop hands its session `bimcraft-dwg`'s reader; a test hands it one that gives a DXF's text.
    let dxf = std::fs::read(drawing("dwg-reader", "plan.dxf")).unwrap();
    h.state_mut().session.dwg = Some(std::sync::Arc::new(move |_: &std::path::Path| {
        Ok((dxf.clone(), vec!["3 of the DWG's object records couldn't be read and were left out".into()]))
    }));
    let dwg = drawing("dwg-reader", "plan.dxf").with_file_name("site.dwg");
    std::fs::write(&dwg, "AC1032").unwrap();
    h.state_mut().import_cad_path(dwg, false);
    h.run();
    press(&mut h, "OK");
    let placed = imports(&h);
    assert_eq!(placed.len(), 1);
    assert_eq!(h.state().session.model.cads["site"].source(), "site.dwg");
    let status = &h.state().status;
    assert!(!status.error && status.text.contains("3 of the DWG's object records"), "{status:?}");
}

#[test]
fn by_shared_coordinates_puts_the_drawing_where_the_active_location_puts_it_or_acquires_from_it() {
    let mut h = app(BOX);
    // The site's survey says the project's origin is at E 1000 m, N 2000 m: a drawing in those numbers lands 1 km off.
    h.state_mut().run("location", json!({"name": "Internal", "east": 1_000_000.0, "north": 2_000_000.0}));
    h.state_mut().import_cad_path(drawing("shared", "survey.dxf"), false);
    h.run();
    let acquire = h.get_by_role_and_label(Role::CheckBox, "Acquire coordinates from it");
    assert!(egui_kittest::kittest::NodeT::accesskit_node(&acquire).is_disabled(), "for shared coordinates only");
    pick(&mut h, Role::RadioButton, "Auto - By Shared Coordinates");
    pick(&mut h, Role::CheckBox, "Acquire coordinates from it");
    press(&mut h, "OK");
    let placed = imports(&h);
    assert_eq!(placed.len(), 1, "{}", h.state().status.text);
    // Acquired: its middle (E 5 m, N 3 m) is now the project's origin.
    let l = h.state().session.model.frames().location();
    assert!((l.east - 5_000.0).abs() < 1.0 && (l.north - 3_000.0).abs() < 1.0, "{l:?}");
}
