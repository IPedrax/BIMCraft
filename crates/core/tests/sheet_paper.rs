//! Named paper dimensions are checked in the PDF written by save, not through paper().
use bimcraft_core::engine::Session;
use serde_json::json;

fn save_pdf(session: &mut Session, path: &std::path::Path) -> Result<Vec<u8>, String> {
    session.execute("save", &json!({"path": path}))?;
    std::fs::read(path).map_err(|e| e.to_string())
}

/// Whether the PDF's one page is `want` points wide and high (`"1368.000 936.000"`), whatever spacing and digits the writer
/// gives its MediaBox.
fn media_box_is(text: &str, want: &str) -> bool {
    let got: Vec<f64> = text
        .split("/MediaBox")
        .nth(1)
        .and_then(|r| r.split(']').next())
        .map(|b| b.trim_start_matches([' ', '[']).split_whitespace().filter_map(|x| x.parse().ok()).collect())
        .unwrap_or_default();
    let want: Vec<f64> = [0.0, 0.0].into_iter().chain(want.split_whitespace().filter_map(|x| x.parse().ok())).collect();
    got.len() == 4 && want.len() == 4 && got.iter().zip(&want).all(|(a, b)| (a - b).abs() < 0.01)
}

#[test]
fn named_paper_prints_exact_media_boxes_and_survives_reopening() {
    // Points (1/72 inch), independently tabulated from the paper dimensions; PDF writes three decimals.
    let cases: &[(&[&str], &str)] = &[
        (&["A0", "ISO A0", "ISOA0"], "3370.394 2383.937"),
        (&["A1", "ISO A1", "ISOA1"], "2383.937 1683.780"),
        (&["A2", "ISO A2", "ISOA2"], "1683.780 1190.551"),
        (&["A3", "ISO A3", "ISOA3"], "1190.551 841.890"),
        (&["A4", "ISO A4", "ISOA4"], "841.890 595.276"),
        (&["B1", "ISO B1", "ISOB1"], "2834.646 2004.094"),
        (&["B2", "ISO B2", "ISOB2"], "2004.094 1417.323"),
        (&["B3", "ISO B3", "ISOB3"], "1417.323 1000.630"),
        (&["B4", "ISO B4", "ISOB4"], "1000.630 708.661"),
        (&["ARCH A", "ARCHA"], "864.000 648.000"),
        (&["ARCH B", "ARCHB"], "1296.000 864.000"),
        (&["ARCH C", "ARCHC"], "1728.000 1296.000"),
        (&["ARCH D", "ARCHD"], "2592.000 1728.000"),
        (&["ARCH E", "ARCHE"], "3456.000 2592.000"),
        (&["ARCH E1", "ARCHE1"], "3024.000 2160.000"),
        (&["ARCH E2", "ARCHE2"], "2736.000 1872.000"),
        (&["ARCH E3", "ARCHE3"], "2808.000 1944.000"),
        (&["ANSI A", "ANSIA", "LETTER"], "792.000 612.000"),
        (&["ANSI B", "ANSIB", "TABLOID", "LEDGER"], "1224.000 792.000"),
        (&["ANSI C", "ANSIC"], "1584.000 1224.000"),
        (&["ANSI D", "ANSID"], "2448.000 1584.000"),
        (&["ANSI E", "ANSIE"], "3168.000 2448.000"),
        (&["SUPER B", "SUPERB", "SUPER-B", " super b "], "1368.000 936.000"),
    ];
    let dir = tempfile::tempdir().unwrap();
    for (names, media_box) in cases {
        for name in *names {
            let mut session = Session::new();
            session.execute("sheet", &json!({"size": name})).unwrap();
            let bytes = save_pdf(&mut session, &dir.path().join("paper.pdf")).unwrap();
            let text = String::from_utf8_lossy(&bytes);
            assert_eq!(text.matches("/MediaBox").count(), 1, "{name}");
            assert!(media_box_is(&text, media_box), "{name}: expected {media_box}");
            let file = dir.path().join("paper.bimcraft");
            session.execute("save", &json!({"path": file})).unwrap();
            let mut reopened = Session::new();
            reopened.execute("open", &json!({"path": file})).unwrap();
            let id = reopened.model.sheet_numbered("A101").unwrap();
            assert_eq!(reopened.model.sheet(id).unwrap().size, name.trim().to_ascii_uppercase());
            assert_eq!(save_pdf(&mut reopened, &dir.path().join("reopened.pdf")).unwrap(), bytes, "{name}");
        }
    }
}

#[test]
fn iso_aliases_keep_the_bare_formats_margins_and_folding_marks() {
    let dir = tempfile::tempdir().unwrap();
    for standard in ["iso", "nbr"] {
        for size in ["A0", "A3", "A4"] {
            let mut bare = Session::new();
            bare.execute("standard", &json!({"set": standard})).unwrap();
            bare.execute("sheet", &json!({"size": size})).unwrap();
            let expected = save_pdf(&mut bare, &dir.path().join("bare.pdf")).unwrap();
            for alias in [format!("ISO {size}"), format!("ISO{size}")] {
                let mut session = Session::new();
                session.execute("standard", &json!({"set": standard})).unwrap();
                session.execute("sheet", &json!({"size": alias})).unwrap();
                assert_eq!(save_pdf(&mut session, &dir.path().join("alias.pdf")).unwrap(), expected, "{standard}: {alias}");
            }
        }
    }
}
