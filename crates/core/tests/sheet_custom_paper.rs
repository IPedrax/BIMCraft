//! Custom paper is checked in saved PDFs and actual PNG headers, including its bounds.
use bimcraft_core::engine::Session;
use serde_json::json;

fn save_pdf(session: &mut Session, path: &std::path::Path) -> Result<Vec<u8>, String> {
    session.execute("save", &json!({"path": path}))?;
    std::fs::read(path).map_err(|e| e.to_string())
}

#[test]
fn custom_paper_keeps_its_order_and_exact_media_box_after_reopening() {
    let dir = tempfile::tempdir().unwrap();
    for (size, points) in [
        ("482.6x330.2", "1368.000 936.000"),
        ("482.6 X 330.2", "1368.000 936.000"),
        ("482.6 × 330.2", "1368.000 936.000"),
        // As narrow as ISO's title block and its margins let paper be, and as low as custom paper goes.
        ("210x100", "595.276 283.465"),
        ("210x1219.2", "595.276 3456.000"),
        ("1219.2x100", "3456.000 283.465"),
        ("1219.2x1219.2", "3456.000 3456.000"),
    ] {
        let mut session = Session::new();
        session.execute("sheet", &json!({"size": size})).unwrap();
        let bytes = save_pdf(&mut session, &dir.path().join("custom.pdf")).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert_eq!(text.matches("/MediaBox").count(), 1);
        assert!(text.contains(&format!("/MediaBox [0 0 {points}]")), "{size}");
        let file = dir.path().join("custom.bimcraft");
        session.execute("save", &json!({"path": file})).unwrap();
        let mut reopened = Session::new();
        reopened.execute("open", &json!({"path": file})).unwrap();
        let id = reopened.model.sheet_numbered("A101").unwrap();
        assert_eq!(reopened.model.sheet(id).unwrap().size, size.trim().to_ascii_uppercase());
        assert_eq!(save_pdf(&mut reopened, &dir.path().join("reopened.pdf")).unwrap(), bytes);
    }
}

#[test]
fn invalid_custom_paper_refuses_creation_and_edits_without_changing_the_model() {
    let mut session = Session::new();
    let id = session.execute("sheet", &json!({})).unwrap()["id"].as_u64().unwrap();
    // A positive control shows that modify can edit this sheet's size before testing its refusals.
    session.execute("modify", &json!({"id": id, "size": "B1"})).unwrap();
    assert_eq!(session.model.sheet(id).unwrap().size, "B1");
    for size in [
        "99.9x200",
        "200x99.9",
        "200x1219.2001",
        "1219.2001x200",
        "5000x100",
        "0x300",
        "-100x300",
        "NaNx300",
        "300xinf",
        "1e999x300",
        "300x",
        "x300",
        "300x200x100",
        "300x200×100",
        "wide x tall",
        "300",
        "300in x 200in",
        "300,2x200",
        "ISO100x100",
        "B5",
    ] {
        let before = session.model.to_json().unwrap();
        let error = session.execute("sheet", &json!({"size": size})).unwrap_err();
        assert!(
            error.contains("isn't a paper size") && error.contains("100") && error.contains("1219.2") && error.contains("mm"),
            "{error}"
        );
        assert_eq!(session.model.to_json().unwrap(), before, "create {size}");
        session.execute("modify", &json!({"id": id, "size": size})).unwrap_err();
        assert_eq!(session.model.to_json().unwrap(), before, "edit {size}");
    }
    session.execute("modify", &json!({"id": id, "size": "500×300"})).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let pdf = save_pdf(&mut session, &dir.path().join("edited.pdf")).unwrap();
    assert!(String::from_utf8_lossy(&pdf).contains("/MediaBox [0 0 1417.323 850.394]"));
}

#[test]
fn the_largest_named_and_custom_pages_render_within_the_existing_image_side_limit() {
    let dir = tempfile::tempdir().unwrap();
    for (size, width, height) in [("A0", 5945, 4205), ("ARCH E", 6096, 4572), ("1219.2x1219.2", 6096, 6096), ("210x100", 1050, 500)] {
        let mut session = Session::new();
        session.execute("sheet", &json!({"size": size})).unwrap();
        let path = dir.path().join("paper.png");
        session.execute("render", &json!({"view": "A101", "path": path})).unwrap();
        let png = std::fs::read(path).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let actual = (u32::from_be_bytes(png[16..20].try_into().unwrap()), u32::from_be_bytes(png[20..24].try_into().unwrap()));
        assert_eq!(actual, (width, height), "{size}");
        assert!(actual.0 <= 8192 && actual.1 <= 8192, "{size}");
    }
}

#[test]
fn paper_narrower_than_the_title_block_is_refused_saying_how_wide_it_must_be() {
    // Each standard's title block and the margins either side of it: the least width a sheet's paper may have.
    for (standard, least, short) in
        [("iso", "210", "209.9"), ("eu", "210", "209.9"), ("jis", "210", "209.9"), ("nbr", "210", "209.9"), ("ansi", "211.75", "211.7")]
    {
        let mut session = Session::new();
        session.execute("standard", &json!({"set": standard})).unwrap();
        let id = session.execute("sheet", &json!({})).unwrap()["id"].as_u64().unwrap();
        let before = session.model.to_json().unwrap();
        for size in ["100x100", "150x300", &format!("{short}x297")] {
            let error = session.execute("sheet", &json!({"size": size})).unwrap_err();
            assert!(error.contains("title block") && error.contains(&format!("at least {least} mm wide")), "{standard} {size}: {error}");
            assert_eq!(session.model.to_json().unwrap(), before, "create {standard} {size}");
            let error = session.execute("modify", &json!({"id": id, "size": size})).unwrap_err();
            assert!(error.contains(&format!("at least {least} mm wide")), "{standard} {size}: {error}");
            assert_eq!(session.model.to_json().unwrap(), before, "edit {standard} {size}");
        }
        // Exactly as wide as the title block and its margins, and as low as custom paper goes.
        session.execute("sheet", &json!({"size": format!("{least}x100")})).unwrap();
    }
}

#[test]
fn a_standard_whose_title_block_no_longer_fits_a_sheet_is_refused_naming_the_sheet() {
    let mut session = Session::new();
    session.execute("standard", &json!({"set": "iso"})).unwrap();
    let id = session.execute("sheet", &json!({"size": "210x297", "number": "S1"})).unwrap()["id"].as_u64().unwrap();
    let before = session.model.to_json().unwrap();
    // ANSI's margins are 19.05 and 12.7 mm either side of a 180 mm title block.
    let error = session.execute("standard", &json!({"set": "ansi"})).unwrap_err();
    assert!(error.contains("S1") && error.contains("at least 211.75 mm wide"), "{error}");
    assert_eq!(session.model.to_json().unwrap(), before);
    assert_eq!(session.execute("standard", &json!({})).unwrap()["standard"], "iso");
    session.execute("modify", &json!({"id": id, "size": "ANSI A"})).unwrap();
    session.execute("standard", &json!({"set": "ansi"})).unwrap();
}
