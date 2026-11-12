//! Drawing lettering exercised through the same commands as CLI and MCP callers.
#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use serde_json::{Value, json};

    fn grid(label: &str) -> Session {
        let mut session = Session::new();
        session.execute("grid", &json!({"start": [0, 0], "end": [0, 5000], "name": label})).unwrap();
        session
    }

    fn picture(session: &mut Session) -> (Vec<u8>, Value) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plan.png");
        let reply = session.execute("render", &json!({"view": "Level 1", "width": 640, "height": 480, "path": path})).unwrap();
        let image = tiny_skia::Pixmap::load_png(path).unwrap();
        assert_eq!((image.width(), image.height()), (640, 480));
        (image.data().to_vec(), reply)
    }

    #[test]
    fn engineering_marks_are_visible_and_distinct_in_exported_grid_labels() {
        let (question, _) = picture(&mut grid("1?"));
        let (space, _) = picture(&mut grid("1 "));
        let mut marks = Vec::new();
        for label in ["1′", "1″", "1…"] {
            let mut session = grid(label);
            let (pixels, reply) = picture(&mut session);
            assert!(pixels != question, "{label} must not be a question mark");
            assert!(pixels != space, "{label} must not be blank");
            assert!(reply.get("notes").is_none(), "a supported mark needs no diagnostic: {reply}");
            for previous in &marks {
                assert!(&pixels != previous, "engineering marks must remain distinguishable");
            }
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("marks.bimcraft");
            session.execute("save", &json!({"path": path})).unwrap();
            let mut reopened = Session::new();
            reopened.execute("open", &json!({"path": path})).unwrap();
            assert!(picture(&mut reopened).0 == pixels, "{label} must survive native save and reopen");
            marks.push(pixels);
        }
    }

    #[test]
    fn unsupported_grid_letters_are_reported_once_and_survive_native_reopen() {
        let mut session = grid("漢漢ωω");
        let (pixels, reply) = picture(&mut session);
        let notes = reply["notes"].as_array().expect("render must report replaced lettering");
        assert_eq!(notes.len(), 2, "deduplicate repeated labels and both grid heads: {reply}");
        for code in ["U+6F22", "U+03C9"] {
            assert!(notes.iter().any(|n| n.as_str().unwrap().contains(code)), "{code}: {reply}");
        }
        let inline = session.execute("render", &json!({"view": "Level 1", "width": 640, "height": 480})).unwrap();
        assert!(inline["pngBase64"].as_str().is_some_and(|s| !s.is_empty()));
        assert_eq!(inline["notes"], reply["notes"]);
        assert!(pixels == picture(&mut grid("????")).0, "keep the existing fallback output");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("drawing.bimcraft");
        session.execute("save", &json!({"path": path})).unwrap();
        let mut reopened = Session::new();
        reopened.execute("open", &json!({"path": path})).unwrap();
        let (after, reply_after) = picture(&mut reopened);
        assert!(after == pixels, "reopened drawing pixels changed");
        assert_eq!(reply_after["notes"], reply["notes"]);
        assert!(reopened.execute("elements", &json!({"category": "grid"})).unwrap().to_string().contains("漢漢ωω"));
    }

    #[test]
    fn many_unsupported_letters_have_bounded_diagnostics_and_still_render() {
        let label: String = (0x4E00..0x4E40).filter_map(char::from_u32).collect();
        let (_, reply) = picture(&mut grid(&label));
        let notes = reply["notes"].as_array().expect("unsupported lettering diagnostics");
        assert!(!notes.is_empty() && notes.len() <= 32, "{reply}");
        assert!(notes.iter().all(|n| n.as_str().unwrap().len() < 200), "diagnostics must not echo long labels");
    }

    #[test]
    fn export_failure_preserves_labels_and_a_retry_returns_the_diagnostic() {
        let mut session = grid("1漢");
        let before = session.model.clone();
        let dir = tempfile::tempdir().unwrap();
        let missing_parent = dir.path().join("missing").join("plan.png");
        assert!(session.execute("render", &json!({"path": missing_parent})).is_err());
        assert_eq!(*session.model, *before);
        let (_, reply) = picture(&mut session);
        assert!(reply["notes"].as_array().is_some_and(|notes| notes.iter().any(|n| n.as_str().unwrap().contains("U+6F22"))));
    }

    #[test]
    fn non_latin_note_fallback_does_not_get_a_drawing_lettering_warning() {
        for font in ["IBM Plex Sans", "BIMCraft Lettering"] {
            let mut session = Session::new();
            session.execute("text_type", &json!({"name": "Note", "font": font})).unwrap();
            let note = session.execute("text", &json!({"text": "ωΣ", "at": [0, 0], "type": "Note"})).unwrap();
            let (greek, reply) = picture(&mut session);
            assert!(reply.get("notes").is_none(), "the note font pipeline supports Greek: {reply}");
            session.execute("modify", &json!({"id": note["id"], "text": "??"})).unwrap();
            assert!(picture(&mut session).0 != greek, "{font} must preserve its non-Latin fallback");
        }
    }
}
