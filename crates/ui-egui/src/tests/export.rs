//! File > Export… (Ctrl+P): the dialog, its preview, and an export written on a thread of its own while the status bar shows
//! it and Cancel stops it.

use super::*;
use egui_kittest::kittest::NodeT;
use std::sync::Arc;
use web_time::Duration;

/// Frames until `done` says so, a few seconds at most: the preview and the export are made on threads of their own.
fn until(h: &mut Harness<BimApp>, done: impl Fn(&BimApp) -> bool) {
    for _ in 0..2000 {
        if done(h.state()) {
            return;
        }
        h.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("it never came: {}", h.state().status.text);
}

fn answer(h: &mut Harness<BimApp>, path: &std::path::Path) {
    let tx = crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).expect("a file dialog was begun");
    tx.send(Some(vec![path.to_path_buf()])).unwrap();
    h.run();
}

#[test]
fn ctrl_p_opens_the_export_dialog_on_the_plan_and_export_writes_the_pdf_on_a_thread_while_editing_goes_on() {
    let mut h = app(SAMPLE);
    assert!(matches!(h.state().sheet, Sheet::Plan(_)), "the sample opens on a plan");
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    h.step(); // The preview is still rendering; `until` waits for it below.
    let d = h.state().export_dialog.as_ref().expect("Ctrl+P opens the Export dialog");
    assert_eq!(d.format, "pdf");
    assert_eq!(d.source, h.state().export_source());
    until(&mut h, |app| app.export_dialog.as_ref().is_some_and(|d| d.preview_ready()));
    // The plan at its own scale is one page; fitted to A4 it is one too, at the scale that fills it.
    assert!(h.query_by_label_contains("1 page").is_some(), "the dialog says how many pages");
    // As tall as its form, not the window: the button sits under the form, not at the screen's foot.
    let (top, bottom) = (h.get_by_label("Format").rect().top(), h.get_by_label("Export…").rect().bottom());
    assert!(bottom - top < 600.0, "the dialog is {} tall", bottom - top);
    h.get_by_role_and_label(Role::RadioButton, "Fit to page").click_accesskit();
    h.step();
    assert_eq!(h.state().export_dialog.as_ref().unwrap().options.page.scale, bimcraft_core::export::page::Scale::Fit);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("plan {1}.pdf");
    h.get_by_label("Export…").click_accesskit();
    h.step();
    assert!(h.state().export_dialog.is_none(), "Export… closes the dialog and asks where");
    let tx = crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).expect("a file dialog was begun");
    tx.send(Some(vec![file.clone()])).unwrap();
    h.step();
    assert!(h.state().exporting(), "the file is written on a thread of its own");
    // The project can be edited while it is written: the export is of the project as it was.
    h.state_mut().session.execute("level", &json!({"elevation": 9000})).unwrap();
    until(&mut h, |app| !app.exporting());
    let bytes = std::fs::read(&file).unwrap_or_else(|e| panic!("{e}: {}", h.state().status.text));
    assert!(bytes.starts_with(b"%PDF"), "a PDF, named as the dialog gave it, braces and all");
    assert!(!h.state().status.error && h.state().status.text.contains("plan {1}.pdf"), "{}", h.state().status.text);
    // Its settings are kept for the next export.
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    h.step();
    assert_eq!(h.state().export_dialog.as_ref().unwrap().options.page.scale, bimcraft_core::export::page::Scale::Fit);
}

#[test]
fn a_cancelled_export_writes_nothing_more_and_says_so() {
    let mut h = app(SAMPLE);
    let dir = tempfile::tempdir().unwrap();
    let m = &h.state().session.model;
    let items = m
        .sheets()
        .into_iter()
        .chain(m.levels().into_iter().map(|(id, _)| id))
        .map(|id| bimcraft_core::export::Item { view: Some(m.guid(id)), format: "png".into() })
        .collect::<Vec<_>>();
    assert!(items.len() > 1);
    let job = bimcraft_core::export::Job { items, folder: dir.path().to_path_buf(), ..Default::default() };
    let (open, gate) = std::sync::mpsc::channel();
    assert!(h.state_mut().start_export_after(job, Some(gate)));
    // While it runs the status bar shows it, with its Cancel. (A frame at a time: the window looks again while it runs.)
    h.step();
    h.get_by_label("Cancel Export").click();
    h.step();
    h.step();
    assert!(h.query_by_label("Cancelling…").is_some());
    open.send(()).unwrap();
    until(&mut h, |app| !app.exporting());
    let status = &h.state().status.text;
    assert!(status.starts_with("Export cancelled"), "{status}");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "nothing was written: it was stopped before the first");
}

#[test]
fn the_format_list_offers_what_the_view_can_be_written_as_and_a_model_format_writes_the_project() {
    let mut h = app(SAMPLE);
    h.state_mut().open_export(Some("dxf"));
    h.run();
    assert_eq!(h.state().export_dialog.as_ref().unwrap().format, "dxf", "a plan is a line drawing");
    // A table format can't take a plan, so the dialog doesn't open on it but on one that can.
    h.state_mut().export_dialog = None;
    h.state_mut().open_export(Some("csv"));
    h.run();
    assert_ne!(h.state().export_dialog.as_ref().unwrap().format, "csv");
    // IFC from a plan is the whole project.
    h.state_mut().export_dialog = None;
    h.state_mut().open_export(Some("ifc"));
    h.run();
    assert_eq!(h.state().export_dialog.as_ref().unwrap().format, "ifc4");
    assert!(h.query_by_label_contains("The whole project").is_some());
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("house.ifc");
    press(&mut h, "Export…");
    answer(&mut h, &file);
    until(&mut h, |app| !app.exporting());
    assert!(std::fs::read_to_string(&file).unwrap().contains("FILE_SCHEMA(('IFC4'))"));
}

#[test]
fn a_custom_region_is_drawn_in_the_plan_while_the_dialog_steps_aside_and_the_export_holds_only_it() {
    let mut h = app(SAMPLE);
    h.state_mut().open_export(Some("dxf"));
    h.run();
    // A CAD drawing takes the colour setting too, though it has no pages.
    assert!(h.query_by_label("Greyscale").is_some());
    pick(&mut h, Role::RadioButton, "Custom region");
    assert!(h.query_by_label("No region drawn yet").is_some());
    press(&mut h, "Draw region…");
    assert!(h.state().export_dialog.as_ref().is_some_and(|d| d.picking), "the dialog steps aside");
    assert!(h.query_by_label("Export…").is_none());
    assert!(matches!(h.state().tool, Tool::ExportRegion(None)));
    // Two corners, opposite: the second brings the dialog back with them.
    click(&mut h, P2(-500.0, 2500.0));
    assert!(matches!(h.state().tool, Tool::ExportRegion(Some(_))));
    click(&mut h, P2(6200.0, 7800.0));
    let d = h.state().export_dialog.as_ref().expect("the dialog is back");
    assert!(!d.picking && d.custom);
    let r = d.region().expect("the region drawn");
    // Where the clicks were, give or take the snap.
    for (got, want) in r.iter().zip([-500.0, 2500.0, 6200.0, 7800.0]) {
        assert!((got - want).abs() < 300.0, "{r:?}");
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("kitchen.dxf");
    press(&mut h, "Export…");
    answer(&mut h, &file);
    until(&mut h, |app| !app.exporting());
    let dxf = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{e}: {}", h.state().status.text));
    // Its extents are the region's, in the model's millimetres.
    let after = |key: &str| -> f64 {
        let at = dxf.find(key).unwrap();
        dxf[at..].lines().nth(2).unwrap().trim().parse().unwrap()
    };
    assert!(after("$EXTMIN") >= r[0] - 1.0 && after("$EXTMAX") <= r[2] + 1.0, "{} to {}", after("$EXTMIN"), after("$EXTMAX"));
    // Esc while drawing brings the dialog back as it was.
    h.state_mut().open_export(Some("pdf"));
    h.run();
    pick(&mut h, Role::RadioButton, "Custom region");
    press(&mut h, "Draw region…");
    key(&mut h, Key::Escape);
    h.run();
    assert!(h.query_by_label("Export…").is_some(), "back, the region still to draw");
    assert!(!h.state().export_dialog.as_ref().unwrap().picking);
}

#[test]
fn a_printer_prints_the_pdf_the_page_setup_makes_and_the_status_says_where_it_went() {
    let mut h = app(SAMPLE);
    let spooler = Arc::new(crate::export::print::Fake { names: vec!["Office".into(), "Plotter".into()], ..Default::default() });
    h.state_mut().spooler = spooler.clone();
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    h.run();
    until(&mut h, |app| app.export_dialog.as_ref().is_some_and(|d| d.printers_found() && d.preview_ready()));
    // The printers are listed under the file.
    h.get_all_by_role(Role::ComboBox).find(|n| n.value().as_deref() == Some("File")).expect("the destination").click_accesskit();
    h.run();
    assert!(h.query_by_label("Plotter").is_some());
    h.get_by_label("Office").click();
    h.run();
    let d = h.state().export_dialog.as_ref().unwrap();
    assert_eq!(d.destination, crate::export::Destination::Printer("Office".into()));
    assert_eq!(d.format, "pdf", "a printer prints PDF");
    h.state_mut().export_dialog.as_mut().unwrap().copies = 2;
    h.state_mut().export_dialog.as_mut().unwrap().options.page.paper = Some("A4".into());
    h.state_mut().export_dialog.as_mut().unwrap().options.page.scale = bimcraft_core::export::page::Scale::Fit;
    press(&mut h, "Print");
    assert!(h.state().export_dialog.is_none(), "Print asks for no file");
    until(&mut h, |app| !app.exporting());
    let jobs = spooler.jobs.lock().unwrap().clone();
    assert_eq!(jobs.len(), 1, "{}", h.state().status.text);
    let (printer, copies, media, head) = &jobs[0];
    assert_eq!((printer.as_str(), *copies, media.as_deref()), ("Office", 2, Some("A4")));
    assert!(head.starts_with(b"%PDF"));
    assert_eq!(h.state().status.text, "Sent to Office.");
}

#[test]
fn a_pdf_can_be_an_archive_with_cut_marks_and_the_dialog_says_so() {
    let mut h = app(SAMPLE);
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    h.run();
    until(&mut h, |app| app.export_dialog.as_ref().is_some_and(|d| d.preview_ready()));
    // One page: cut marks are for a drawing across several.
    assert!(h.get_by_label("Draw cut marks").accesskit_node().is_disabled());
    {
        let d = h.state_mut().export_dialog.as_mut().unwrap();
        d.options.page.paper = Some("A4".into());
        d.options.page.scale = bimcraft_core::export::page::Scale::Custom(20.0);
        d.options.page.margin = 10.0;
        d.options.pdf.archive = bimcraft_core::pdf::Archive::A2b;
    }
    h.run();
    pick(&mut h, Role::CheckBox, "Draw cut marks");
    assert!(h.state().export_dialog.as_ref().unwrap().options.pdf.cut_marks);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("archive.pdf");
    press(&mut h, "Export…");
    answer(&mut h, &file);
    until(&mut h, |app| !app.exporting());
    let pdf = std::fs::read(&file).unwrap_or_else(|e| panic!("{e}: {}", h.state().status.text));
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("<pdfaid:part>2</pdfaid:part>") && text.contains("<pdfaid:conformance>B</pdfaid:conformance>"));
    assert!(text.matches("/Type/Page/").count() > 1, "tiled at 1 : 20");
}
