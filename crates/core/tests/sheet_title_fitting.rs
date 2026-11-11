//! The built-in title block through the same commands used by the CLI and MCP.
#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use bimcraft_core::geom::P2;
    use bimcraft_core::model::paper;
    use serde_json::json;
    use std::path::Path;

    fn project(project: &str, name: &str, number: &str, standard: &str, size: &str) -> Session {
        let mut s = Session::new();
        s.execute("new", &json!({"name": project})).unwrap();
        s.execute("standard", &json!({"set": standard, "adapt_all": false})).unwrap();
        s.execute("sheet", &json!({"name": name, "number": number, "size": size})).unwrap();
        s
    }

    fn render(s: &mut Session, number: &str, path: &Path) -> Vec<u8> {
        s.execute("render", &json!({"view": number, "path": path})).unwrap();
        std::fs::read(path).unwrap()
    }

    fn assert_changed_ink_stays_in_cell(field: usize, standard: &str, size: &str) {
        let dir = tempfile::tempdir().unwrap();
        let long = "SYNTHETIC REGIONAL OPERATIONS AND MAINTENANCE CAMPUS DOCUMENTATION PROJECT";
        let mut values = ["H", "H", "H"];
        let mut reference = project(values[0], values[1], values[2], standard, size);
        let before = render(&mut reference, values[2], &dir.path().join("before.png"));
        values[field] = long;
        let mut s = project(values[0], values[1], values[2], standard, size);
        let after = render(&mut s, values[2], &dir.path().join("after.png"));
        let before = tiny_skia::Pixmap::decode_png(&before).unwrap();
        let after = tiny_skia::Pixmap::decode_png(&after).unwrap();
        let P2(w, h) = paper(size).unwrap();
        let m = s.model.standard().sheet_margins(size);
        let x1 = w - m.right;
        let x0 = x1 - m.title_width.min(w - m.left - m.right);
        let (left, right, bottom, top) = match field {
            0 => (x0 + 4.0, x1 - 4.0, m.bottom + 29.0, m.bottom + 37.0),
            1 => (x0 + 4.0, x1 - 54.0, m.bottom + 14.0, m.bottom + 20.0),
            _ => (x1 - 46.0, x1 - 4.0, m.bottom + 8.0, m.bottom + 18.0),
        };
        let mut changed = 0;
        for y in 0..after.height() {
            for x in 0..after.width() {
                if before.pixel(x, y) == after.pixel(x, y) {
                    continue;
                }
                changed += 1;
                let (px, py) = (f64::from(x) / 5.0, h - f64::from(y) / 5.0);
                assert!(
                    px >= left - 0.4 && px <= right + 0.4 && py >= bottom && py <= top,
                    "field {field}, {standard}/{size}: changed ink outside its cell at ({px}, {py}), cell {left}..{right}, {bottom}..{top}"
                );
            }
        }
        assert!(changed > 30, "the lettering must still be visible");
    }

    #[test]
    fn long_project_names_stay_inside_the_title_block() {
        for standard in ["nbr", "iso", "ansi", "eu", "jis"] {
            assert_changed_ink_stays_in_cell(0, standard, "A4");
        }
    }

    #[test]
    fn long_sheet_names_stay_clear_of_the_number_cell() {
        for size in ["A0", "A1", "A2", "A3", "A4"] {
            assert_changed_ink_stays_in_cell(1, "nbr", size);
        }
    }

    #[test]
    fn long_sheet_numbers_stay_inside_their_cell() {
        assert_changed_ink_stays_in_cell(2, "iso", "A3");
    }

    #[test]
    fn a_fitted_project_name_draws_every_letter() {
        for (name, expected) in [("H".repeat(80), 80), ("ß".repeat(80), 160)] {
            let dir = tempfile::tempdir().unwrap();
            let mut s = project(&name, "Plan", "A101", "iso", "A3");
            let bytes = render(&mut s, "A101", &dir.path().join("all-letters.png"));
            let image = tiny_skia::Pixmap::decode_png(&bytes).unwrap();
            // The H/S crossbars are separate ink runs through the project row's centre.
            // Include antialiased one-pixel strokes split across adjacent pixel rows.
            let y = ((297.0 - (10.0 + 33.0)) * 5.0) as u32;
            let mut letters = 0;
            let mut ink = false;
            for x in (234 * 5)..(406 * 5) {
                let next = image.pixel(x, y).unwrap().red() < 200;
                letters += usize::from(next && !ink);
                ink = next;
            }
            assert_eq!(letters, expected, "all capitals must remain visible, with gaps between them");
        }
    }
    #[test]
    fn fitted_titles_keep_their_names_and_print_identically_after_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let project_name = "SYNTHETIC REGIONAL OPERATIONS AND MAINTENANCE CAMPUS DOCUMENTATION PROJECT";
        let name = "GROUND FLOOR GENERAL ARRANGEMENT AND OPERATIONAL COORDINATION PLAN";
        let number = "SYNTHETIC-A-2026-001-REVISION-A";
        let mut s = project(project_name, name, number, "iso", "A3");
        let before = render(&mut s, number, &dir.path().join("before.png"));
        let native = dir.path().join("drawing.bimcraft");
        let pdf = dir.path().join("before.pdf");
        s.execute("save", &json!({"path": native})).unwrap();
        s.execute("save", &json!({"path": pdf})).unwrap();
        let mut reopened = Session::new();
        let result = reopened.execute("open", &json!({"path": native})).unwrap();
        assert!(result.get("left_out").is_none(), "{result}");
        assert_eq!(reopened.model.name, project_name);
        let sheet = reopened
            .model
            .elements
            .values()
            .find_map(|e| match e {
                bimcraft_core::model::Element::Sheet(sh) => Some(sh),
                _ => None,
            })
            .unwrap();
        assert_eq!((&*sheet.name, &*sheet.number), (name, number));
        assert_eq!(before, render(&mut reopened, number, &dir.path().join("after.png")));
        let after_pdf = dir.path().join("after.pdf");
        reopened.execute("save", &json!({"path": after_pdf})).unwrap();
        assert_eq!(std::fs::read(pdf).unwrap(), std::fs::read(after_pdf).unwrap());
    }
}
