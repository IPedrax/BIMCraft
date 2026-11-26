use serde_json::json;

use super::*;

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-rune-export-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    dir
}

fn open_first(app: &mut App, group: &str) -> ViewKind {
    let kind = app.views.groups.iter().find(|g| g.label == group).and_then(|g| g.entries.first()).map(|e| e.kind).expect("a view");
    assert!(app.open_view(kind));
    kind
}

#[test]
fn a_sheet_is_a_pdf_and_a_plan_a_png_through_the_export_command() {
    let dir = folder("files");
    let mut app = App::sample();
    open_first(&mut app, "Sheets (all)");
    assert_eq!(app.export_file_name("pdf"), "A101 - Ground Floor.pdf");
    let pdf = dir.join("sheet.pdf");
    assert!(app.export_view("pdf", pdf.clone()), "refused: {:?}", app.message);
    assert!(std::fs::read(&pdf).is_ok_and(|b| b.starts_with(b"%PDF")), "a PDF file");
    assert!(app.message.as_deref().is_some_and(|m| m.starts_with("Written to ")), "{:?}", app.message);

    let level = app.level.expect("a level");
    assert!(app.open_view(ViewKind::Plan(level)));
    let png = dir.join("plan.png");
    assert!(app.export_view("png", png.clone()), "refused: {:?}", app.message);
    assert!(std::fs::read(&png).is_ok_and(|b| b.starts_with(&[0x89, b'P', b'N', b'G'])), "a PNG file");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_view_the_command_cannot_name_or_a_format_it_does_not_have_is_said_and_writes_nothing() {
    let dir = folder("refused");
    let mut app = App::sample();
    let level = app.level.expect("a level");
    assert!(app.open_view(ViewKind::Ceiling(level)));
    assert!(app.export_target().is_none());
    let file = dir.join("ceiling.pdf");
    assert!(!app.export_view("pdf", file.clone()) && !file.exists());
    assert_eq!(app.message.as_deref(), Some("This view cannot be exported yet."));

    assert!(app.open_view(ViewKind::Plan(level)));
    assert!(!app.export_view("nonsense", dir.join("x.nonsense")), "an unknown format is refused");
    assert!(app.message.as_deref().is_some_and(|m| !m.is_empty()), "{:?}", app.message);
    assert!(app.open_view(ViewKind::View3D(None)));
    assert_eq!(app.export_target(), Some(json!("{3D}")), "the default 3D view is named");
    assert_eq!(app.export_file_name("png"), "3D.png", "a file name has no braces");
    assert!(app.open_view(ViewKind::Schedule("room")));
    assert_eq!(app.export_target(), Some(json!("Room Schedule")));
    std::fs::remove_dir_all(&dir).ok();
}
