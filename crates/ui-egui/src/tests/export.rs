//! File > Export… (Ctrl+P): the dialog, its preview, and an export written on a thread of its own while the status bar shows
//! it and Cancel stops it.

use super::*;
use std::time::Duration;

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
    h.run();
    let d = h.state().export_dialog.as_ref().expect("Ctrl+P opens the Export dialog");
    assert_eq!(d.format, "pdf");
    assert_eq!(d.source, h.state().export_source());
    until(&mut h, |app| app.export_dialog.as_ref().is_some_and(|d| d.preview_ready()));
    // The plan at its own scale is one page; fitted to A4 it is one too, at the scale that fills it.
    assert!(h.query_by_label_contains("1 page").is_some(), "the dialog says how many pages");
    pick(&mut h, Role::RadioButton, "Fit to page");
    assert_eq!(h.state().export_dialog.as_ref().unwrap().options.page.scale, bimcraft_core::export::page::Scale::Fit);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("plan {1}.pdf");
    press(&mut h, "Export…");
    assert!(h.state().export_dialog.is_none(), "Export… closes the dialog and asks where");
    answer(&mut h, &file);
    assert!(h.state().exporting(), "the file is written on a thread of its own");
    // The project can be edited while it is written: the export is of the project as it was.
    h.state_mut().session.execute("level", &json!({"elevation": 9000})).unwrap();
    until(&mut h, |app| !app.exporting());
    let bytes = std::fs::read(&file).unwrap_or_else(|e| panic!("{e}: {}", h.state().status.text));
    assert!(bytes.starts_with(b"%PDF"), "a PDF, named as the dialog gave it, braces and all");
    assert!(!h.state().status.error && h.state().status.text.contains("plan {1}.pdf"), "{}", h.state().status.text);
    // Its settings are kept for the next export.
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    h.run();
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
