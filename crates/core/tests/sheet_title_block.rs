//! Inspect the actual PDF streams: an empty unframed sheet paints nothing; placed content survives.
use std::io::Read as _;

use bimcraft_core::engine::Session;
use bimcraft_core::model::FORMAT;
use flate2::read::ZlibDecoder;
use serde_json::{Value, json};

fn printed(session: &mut Session) -> Result<(Vec<u8>, String), String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let path = dir.path().join("sheet.pdf");
    session.execute("save", &json!({"path": path}))?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let start = bytes.windows(7).position(|b| b == b"stream\n").ok_or("PDF has no stream")?;
    let header = String::from_utf8_lossy(&bytes[..start]);
    let length: usize = header
        .rsplit("/Length ")
        .next()
        .and_then(|s| s.split_whitespace().next())
        .ok_or("PDF stream has no length")?
        .parse::<usize>()
        .map_err(|e| e.to_string())?;
    let compressed = bytes.get(start + 7..start + 7 + length).ok_or("PDF stream is truncated")?;
    let mut contents = String::new();
    ZlibDecoder::new(compressed).read_to_string(&mut contents).map_err(|e| e.to_string())?;
    Ok((bytes, contents))
}

fn fixture(title_block: Option<bool>, decorated: bool) -> Result<(Session, u64), String> {
    let mut session = Session::new();
    let mut params = json!({"number": "S101", "name": "Example sheet"});
    if let Some(on) = title_block {
        params["title_block"] = json!(on);
    }
    let id = session.execute("sheet", &params)?["id"].as_u64().ok_or("sheet has no id")?;
    if decorated {
        session.execute("wall", &json!({"points": [[0,0],[4000,0]]}))?;
        session.execute("place", &json!({"sheet": "S101", "view": "Level 1", "at": [150,160], "scale": 100}))?;
        session.execute("text", &json!({"view": "S101", "text": "Example note", "at": [20,40]}))?;
        session.execute("detail_line", &json!({"view": "S101", "start": [20,20], "end": [80,20]}))?;
    }
    Ok((session, id))
}

#[test]
fn no_title_block_means_no_stock_pdf_painting_and_on_remains_the_default() {
    let (mut off, _) = fixture(Some(false), false).unwrap();
    let (_, empty) = printed(&mut off).unwrap();
    // Exactly the page transform and line-cap/join setup: no paths, fills, text or images at all.
    assert_eq!(empty.lines().count(), 1, "unexpected content: {empty}");
    assert!(empty.ends_with("cm 1 J 0 j\n"), "{empty}");
    for decorated in [false, true] {
        let (mut default, _) = fixture(None, decorated).unwrap();
        let (mut on, _) = fixture(Some(true), decorated).unwrap();
        let (default_pdf, default_stream) = printed(&mut default).unwrap();
        assert_eq!(printed(&mut on).unwrap().0, default_pdf, "omitting the option must keep its previous appearance");
        assert!(default_stream.contains("h S\n"), "the default still prints its closed frame");
    }
}

#[test]
fn hiding_the_title_block_keeps_viewports_and_sheet_annotations_and_round_trips() {
    let (mut session, id) = fixture(None, true).unwrap();
    let (on_pdf, on) = printed(&mut session).unwrap();
    let views = session.model.on_sheet(id);
    session.execute("modify", &json!({"id": id, "title_block": false})).unwrap();
    let (off_pdf, off) = printed(&mut session).unwrap();
    let (_, empty) = printed(&mut fixture(Some(false), false).unwrap().0).unwrap();
    let retained = off.strip_prefix(&empty).unwrap();
    assert!(!retained.is_empty() && retained.contains(" S\n"), "the note, line and view must print");
    assert!(on.ends_with(retained), "all viewport and annotation operators must be unchanged");
    assert_ne!(off_pdf, on_pdf);
    assert_eq!(session.model.on_sheet(id), views);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("sheet.bimcraft");
    session.execute("save", &json!({"path": file})).unwrap();
    let stored: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(stored["format"], FORMAT);
    assert_eq!(stored["elements"][id.to_string()]["title_block"], false);
    let mut reopened = Session::new();
    let opened = reopened.execute("open", &json!({"path": file})).unwrap();
    assert!(opened.get("left_out").is_none(), "{opened}");
    assert_eq!(printed(&mut reopened).unwrap().0, off_pdf);
    reopened.execute("modify", &json!({"id": id, "title_block": true})).unwrap();
    assert_eq!(printed(&mut reopened).unwrap().0, on_pdf);
}

#[test]
fn old_sheets_default_to_a_title_block_and_bad_options_roll_back() {
    let (mut session, _) = fixture(None, true).unwrap();
    let before = printed(&mut session).unwrap().0;
    let mut old: Value = serde_json::from_str(&session.model.to_json().unwrap()).unwrap();
    old["format"] = json!(17);
    for element in old["elements"].as_object_mut().unwrap().values_mut() {
        element.as_object_mut().unwrap().remove("title_block");
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("old.bimcraft");
    std::fs::write(&file, old.to_string()).unwrap();
    let mut reopened = Session::new();
    reopened.execute("open", &json!({"path": file})).unwrap();
    assert_eq!(printed(&mut reopened).unwrap().0, before);
    for bad in [json!("false"), json!(0), json!([]), json!({})] {
        let model = session.model.to_json().unwrap();
        assert!(session.execute("sheet", &json!({"title_block": bad})).is_err());
        assert_eq!(session.model.to_json().unwrap(), model);
    }
}

#[test]
fn a_sheets_title_block_is_reported_even_when_the_file_omits_the_default() {
    for option in [None, Some(true), Some(false)] {
        let (mut session, _) = fixture(option, false).unwrap();
        let listed = session.execute("elements", &json!({"category": "sheet"})).unwrap();
        assert_eq!(listed["elements"][0]["title_block"], option.unwrap_or(true));
    }
}

#[test]
fn a_sheets_title_block_creation_and_edits_undo_and_redo_in_the_printed_output() {
    let (mut session, id) = fixture(Some(false), false).unwrap();
    let off = printed(&mut session).unwrap().0;
    session.execute("undo", &json!({})).unwrap();
    assert!(session.model.sheet(id).is_none());
    session.execute("redo", &json!({})).unwrap();
    assert!(!session.model.sheet(id).unwrap().title_block);
    assert_eq!(printed(&mut session).unwrap().0, off);
    session.execute("modify", &json!({"id": id, "title_block": true})).unwrap();
    let on = printed(&mut session).unwrap().0;
    assert_ne!(on, off);
    session.execute("undo", &json!({})).unwrap();
    assert_eq!(printed(&mut session).unwrap().0, off);
    session.execute("redo", &json!({})).unwrap();
    assert_eq!(printed(&mut session).unwrap().0, on);
    session.execute("undo", &json!({})).unwrap();
    assert_eq!(printed(&mut session).unwrap().0, off);
}
