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
        picture_of(session, "Level 1", 640, 480)
    }

    fn picture_of(session: &mut Session, view: &str, width: u32, height: u32) -> (Vec<u8>, Value) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("view.png");
        let reply = session.execute("render", &json!({"view": view, "width": width, "height": height, "path": path})).unwrap();
        let image = tiny_skia::Pixmap::load_png(path).unwrap();
        assert_eq!((image.width(), image.height()), (width, height));
        (image.data().to_vec(), reply)
    }

    /// Eighteen by ten metres of walls with a room in them, named `name`.
    fn room(name: &str) -> Session {
        let mut session = Session::new();
        session.execute("wall", &json!({"points": [[0, 0], [18000, 0], [18000, 10000], [0, 10000]], "closed": true})).unwrap();
        session.execute("room", &json!({"point": [9000, 5000], "name": name})).unwrap();
        session
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
    fn unsupported_grid_letters_fall_back_silently_and_survive_native_reopen() {
        let mut session = grid("漢漢ωω");
        let (pixels, reply) = picture(&mut session);
        // Grid names are lettered by the text pipeline, whose fonts fall back letter by letter and say nothing: a name the
        // project's font has no glyph for is still drawn, not replaced by a question mark.
        assert!(reply.get("notes").is_none(), "the text pipeline falls back silently: {reply}");
        let inline = session.execute("render", &json!({"view": "Level 1", "width": 640, "height": 480})).unwrap();
        assert!(inline["pngBase64"].as_str().is_some_and(|s| !s.is_empty()));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("drawing.bimcraft");
        session.execute("save", &json!({"path": path})).unwrap();
        let mut reopened = Session::new();
        reopened.execute("open", &json!({"path": path})).unwrap();
        let (after, reply_after) = picture(&mut reopened);
        assert!(after == pixels, "reopened drawing pixels changed");
        assert!(reply_after.get("notes").is_none(), "{reply_after}");
        assert!(reopened.execute("elements", &json!({"category": "grid"})).unwrap().to_string().contains("漢漢ωω"));
    }

    #[test]
    fn many_unsupported_letters_still_render_without_diagnostics() {
        let label: String = (0x4E00..0x4E40).filter_map(char::from_u32).collect();
        let (pixels, reply) = picture(&mut grid(&label));
        assert!(!pixels.is_empty());
        assert!(reply.get("notes").is_none(), "the text pipeline falls back silently: {reply}");
    }

    #[test]
    fn export_failure_preserves_labels_and_a_retry_draws_them() {
        let mut session = grid("1漢");
        let before = session.model.clone();
        let dir = tempfile::tempdir().unwrap();
        let missing_parent = dir.path().join("missing").join("plan.png");
        assert!(session.execute("render", &json!({"path": missing_parent})).is_err());
        assert_eq!(*session.model, *before);
        let (drawn, reply) = picture(&mut session);
        assert!(!drawn.is_empty());
        assert!(reply.get("notes").is_none(), "{reply}");
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

    #[test]
    fn ukrainian_room_names_are_lettered_on_the_plan_without_a_question_mark() {
        let (drawn, reply) = picture(&mut room("Кухня-вітальня"));
        assert!(reply.get("notes").is_none(), "Ukrainian letters need no diagnostic: {reply}");
        // The letters are drawn, not the question mark the missing glyphs used to become, and they are not blank.
        let (question, question_reply) = picture(&mut room("Кухня-в?тальня"));
        assert!(question_reply.get("notes").is_none(), "{question_reply}");
        assert!(drawn != question, "і must be drawn, not replaced by a question mark");
        let (blank, _) = picture(&mut room("Кухня-в тальня"));
        assert!(drawn != blank, "і must be inked, not left as a space");
    }

    #[test]
    fn ukrainian_letters_are_lettered_in_a_section_without_a_question_mark() {
        let section = |level: &str| {
            let mut session = Session::new();
            session.execute("level", &json!({"name": level, "elevation": 1500})).unwrap();
            session.execute("wall", &json!({"points": [[0, 0], [8000, 0]]})).unwrap();
            // A section looks to the left of its line's direction: at y = -1000 it looks north at the wall on y = 0.
            session.execute("section", &json!({"start": [-1000, -1000], "end": [9000, -1000], "name": "Розріз"})).unwrap();
            session
        };
        // A section names its levels in the drawing lettering, so a Ukrainian name is a lettering path too.
        let (drawn, reply) = picture_of(&mut section("Рівень Ї"), "Розріз", 640, 480);
        assert!(reply.get("notes").is_none(), "Ukrainian letters need no diagnostic: {reply}");
        let (question, _) = picture_of(&mut section("Рівень ?"), "Розріз", 640, 480);
        assert!(drawn != question, "Ї must be drawn, not replaced by a question mark");
    }

    #[test]
    fn a_sheet_and_its_pdf_letter_ukrainian_names_without_a_question_mark() {
        let mut session = room("Кухня-вітальня");
        session.execute("sheet", &json!({"number": "S-02", "name": "Кухня-вітальня", "size": "A3"})).unwrap();
        session.execute("place", &json!({"sheet": "S-02", "view": "Level 1", "scale": 100, "at": [210, 165]})).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("ukrainian-sheet.png");
        let reply = session.execute("render", &json!({"view": "S-02", "path": png})).unwrap();
        assert!(reply.get("notes").is_none(), "the sheet's view of the room needs no diagnostic: {reply}");
        assert!(tiny_skia::Pixmap::load_png(&png).unwrap().data().iter().any(|b| *b != 0), "the sheet is not blank");
        let pdf = dir.path().join("ukrainian-sheet.pdf");
        let reply = session.execute("render", &json!({"view": "S-02", "path": pdf})).unwrap();
        assert!(reply.get("notes").is_none(), "{reply}");
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF-"), "the sheet writes a PDF");
    }

    #[test]
    fn ukrainian_lettering_survives_native_save_and_reopen_pixel_for_pixel() {
        let mut session = room("Кухня-вітальня");
        let (before, reply) = picture(&mut session);
        assert!(reply.get("notes").is_none(), "{reply}");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ukrainian.bimcraft");
        session.execute("save", &json!({"path": path})).unwrap();
        let mut reopened = Session::new();
        reopened.execute("open", &json!({"path": path})).unwrap();
        let (after, reply) = picture(&mut reopened);
        assert!(after == before, "reopened Ukrainian lettering changed");
        assert!(reply.get("notes").is_none(), "{reply}");
    }

    #[test]
    fn a_ukrainian_grid_head_is_lettered_in_our_own_lettering() {
        let axis = |name: &str| {
            let mut session = Session::new();
            session.execute("grid_type", &json!({"name": "Lettering", "font": "BIMCraft Lettering"})).unwrap();
            session.execute("grid", &json!({"start": [0, 0], "end": [0, 5000], "type": "Lettering", "name": name})).unwrap();
            session
        };
        let (drawn, reply) = picture(&mut axis("Ґ"));
        assert!(reply.get("notes").is_none(), "{reply}");
        let (question, _) = picture(&mut axis("?"));
        assert!(drawn != question, "Ґ must be drawn, not replaced by a question mark");
    }
}
