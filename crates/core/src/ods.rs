//! A spreadsheet of one table as OpenDocument (`.ods`, OASIS ODF 1.2), written by hand as `xlsx.rs` writes a workbook: the
//! `mimetype` first and stored, as ODF asks, then the manifest and the content, deflated. The first row in bold; numbers as
//! numbers a spreadsheet can sum; text with its runs of spaces kept. Nothing is read back.

use crate::xlsx::{Cell, Part, escape, sheet_name, zip};

const MIMETYPE: &str = "application/vnd.oasis.opendocument.spreadsheet";

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2"><manifest:file-entry manifest:full-path="/" manifest:version="1.2" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/></manifest:manifest>"#;

/// A spreadsheet of one table named `name` (cut to what a sheet name may hold), its first row `headings` in bold, then `rows`.
pub fn spreadsheet(name: &str, headings: &[String], rows: &[Vec<Cell>]) -> Vec<u8> {
    let content = content(&sheet_name(name), headings, rows);
    zip(&[
        Part { name: "mimetype", data: MIMETYPE.as_bytes(), store: true },
        Part { name: "META-INF/manifest.xml", data: MANIFEST.as_bytes(), store: false },
        Part { name: "content.xml", data: content.as_bytes(), store: false },
    ])
}

/// Text as a paragraph holds it: escaped, a run of spaces (which XML would fold into one) kept as `text:s`.
fn paragraph(s: &str) -> String {
    let mut out = String::from("<text:p>");
    let mut spaces = 0;
    let flush = |out: &mut String, n: usize| match n {
        0 => {}
        1 => out.push(' '),
        n => out.push_str(&format!(" <text:s text:c=\"{}\"/>", n - 1)),
    };
    for c in escape(s).chars() {
        if c == ' ' {
            spaces += 1;
            continue;
        }
        flush(&mut out, spaces);
        spaces = 0;
        out.push(c);
    }
    flush(&mut out, spaces);
    out.push_str("</text:p>");
    out
}

fn content(name: &str, headings: &[String], rows: &[Vec<Cell>]) -> String {
    let columns = rows.iter().map(Vec::len).chain([headings.len()]).max().unwrap_or(0).max(1);
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" office:version="1.2"><office:automatic-styles><style:style style:name="bold" style:family="table-cell"><style:text-properties fo:font-weight="bold"/></style:style></office:automatic-styles><office:body><office:spreadsheet>"#,
    );
    out.push_str(&format!("<table:table table:name=\"{}\">", escape(name)));
    out.push_str(&format!("<table:table-column table:number-columns-repeated=\"{columns}\"/>"));
    out.push_str("<table:table-row>");
    for h in headings {
        out.push_str(&format!(
            "<table:table-cell table:style-name=\"bold\" office:value-type=\"string\">{}</table:table-cell>",
            paragraph(h)
        ));
    }
    out.push_str("</table:table-row>");
    for row in rows {
        out.push_str("<table:table-row>");
        for cell in row {
            match cell {
                Cell::Text(s) => {
                    out.push_str(&format!("<table:table-cell office:value-type=\"string\">{}</table:table-cell>", paragraph(s)));
                }
                Cell::Number(x) if x.is_finite() => {
                    out.push_str(&format!(
                        "<table:table-cell office:value-type=\"float\" office:value=\"{x}\">{}</table:table-cell>",
                        paragraph(&x.to_string())
                    ));
                }
                Cell::Number(_) | Cell::Empty => out.push_str("<table:table-cell/>"),
            }
        }
        out.push_str("</table:table-row>");
    }
    out.push_str("</table:table></office:spreadsheet></office:body></office:document-content>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spreadsheet_starts_with_its_mimetype_stored_and_holds_its_table() {
        let rows = vec![
            vec![Cell::Text("Living  room <1>".into()), Cell::Number(24.5), Cell::Empty],
            vec![Cell::Text("Total".into()), Cell::Number(f64::NAN)],
        ];
        let b = spreadsheet("Rooms: [all]", &["Name".into(), "Area".into(), "Note".into()], &rows);
        // What ODF asks first: the mimetype, stored, its name at byte 30 and its text right after.
        assert_eq!(&b[..4], &[0x50, 0x4b, 0x03, 0x04]);
        assert_eq!(u16::from_le_bytes([b[8], b[9]]), 0, "stored");
        assert_eq!(&b[30..38], b"mimetype");
        assert_eq!(&b[38..38 + MIMETYPE.len()], MIMETYPE.as_bytes());
        let parts = crate::xlsx::unzip(&b);
        assert_eq!(parts.keys().collect::<Vec<_>>(), ["META-INF/manifest.xml", "content.xml", "mimetype"]);
        let c = &parts["content.xml"];
        assert!(c.contains("table:name=\"Rooms all\""), "a sheet name holds none of []:*?/ {c}");
        assert!(c.contains("table:style-name=\"bold\" office:value-type=\"string\"><text:p>Name</text:p>"));
        assert!(c.contains("<text:p>Living <text:s text:c=\"1\"/>room &lt;1&gt;</text:p>"), "{c}");
        assert!(c.contains("office:value-type=\"float\" office:value=\"24.5\""));
        assert_eq!(c.matches("<table:table-row>").count(), 3);
        assert!(!c.contains("NaN"));
    }
}
