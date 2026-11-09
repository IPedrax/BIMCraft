//! A workbook of one sheet as Office Open XML (`.xlsx`), written by hand: the five parts a spreadsheet needs, deflated into a
//! zip with `flate2`. Text goes in as inline strings, so there is no shared-string table; numbers as numbers; the first row
//! in bold. Nothing is read back.

use std::io::Write;

use flate2::Compression;
use flate2::write::DeflateEncoder;

/// A cell: text, or a number a spreadsheet can sum.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Text(String),
    Number(f64),
    Empty,
}

/// The longest name a sheet may have.
const MAX_NAME: usize = 31;

/// A workbook of one sheet named `name` (cut to what a sheet name may hold), its first row `headings` in bold, then `rows`.
pub fn workbook(name: &str, headings: &[String], rows: &[Vec<Cell>]) -> Vec<u8> {
    let parts = [
        ("[Content_Types].xml", CONTENT_TYPES.to_string()),
        ("_rels/.rels", RELS.to_string()),
        ("xl/workbook.xml", workbook_xml(&sheet_name(name))),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS.to_string()),
        ("xl/styles.xml", STYLES.to_string()),
        ("xl/worksheets/sheet1.xml", sheet(headings, rows)),
    ];
    let parts: Vec<Part> = parts.iter().map(|(name, body)| Part { name, data: body.as_bytes(), store: false }).collect();
    zip(&parts)
}

/// A part of a zip: its name, its bytes, and whether it goes in as it is (stored) rather than deflated.
pub(crate) struct Part<'a> {
    pub name: &'a str,
    pub data: &'a [u8],
    pub store: bool,
}

fn workbook_xml(name: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="{}" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        escape(name)
    )
}

/// What a sheet may be called: no `[]:*?/\`, not empty, at most 31 characters.
pub(crate) fn sheet_name(name: &str) -> String {
    let n: String = name.chars().filter(|c| !"[]:*?/\\".contains(*c) && !c.is_control()).take(MAX_NAME).collect();
    let n = n.trim().trim_matches('\'').to_string();
    if n.is_empty() { "Sheet1".into() } else { n }
}

/// A column's letters: 0 is A, 25 Z, 26 AA.
fn column(mut i: usize) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (i % 26) as u8);
        if i < 26 {
            break;
        }
        i = i / 26 - 1;
    }
    s.reverse();
    String::from_utf8_lossy(&s).into_owned()
}

fn sheet(headings: &[String], rows: &[Vec<Cell>]) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
    );
    let head: Vec<Cell> = headings.iter().map(|h| Cell::Text(h.clone())).collect();
    for (r, row) in std::iter::once(&head).chain(rows).enumerate() {
        let bold = r == 0;
        out.push_str(&format!("<row r=\"{}\">", r + 1));
        for (c, cell) in row.iter().enumerate() {
            let at = format!("{}{}", column(c), r + 1);
            let style = if bold { " s=\"1\"" } else { "" };
            match cell {
                Cell::Text(t) => {
                    out.push_str(&format!("<c r=\"{at}\"{style} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>", escape(t)))
                }
                Cell::Number(x) if x.is_finite() => out.push_str(&format!("<c r=\"{at}\"{style}><v>{x}</v></c>")),
                Cell::Number(_) | Cell::Empty => {}
            }
        }
        out.push_str("</row>");
    }
    out.push_str("</sheetData></worksheet>");
    out
}

/// Text as XML holds it: the five entities escaped, and the control characters XML 1.0 can't hold left out.
pub(crate) fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

/// The parts as a zip, in their order, each deflated or stored as it says.
pub(crate) fn zip(parts: &[Part]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for &Part { name, data, store } in parts {
        let mut crc = flate2::Crc::new();
        crc.update(data);
        let packed = if store {
            data.to_vec()
        } else {
            let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
            // Writing to a Vec doesn't fail.
            enc.write_all(data).and_then(|_| enc.finish()).unwrap_or_default()
        };
        let offset = out.len() as u32;
        let header = |sig: u32, central: bool| {
            let mut h = Vec::new();
            h.extend(sig.to_le_bytes());
            if central {
                h.extend(20u16.to_le_bytes()); // made by
            }
            h.extend(20u16.to_le_bytes()); // version needed
            h.extend(0x0800u16.to_le_bytes()); // names in UTF-8
            h.extend(if store { 0u16 } else { 8u16 }.to_le_bytes()); // stored or deflated
            h.extend(0u16.to_le_bytes()); // time
            h.extend(0x0021u16.to_le_bytes()); // date: 1980-01-01
            h.extend(crc.sum().to_le_bytes());
            h.extend((packed.len() as u32).to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((name.len() as u16).to_le_bytes());
            h.extend(0u16.to_le_bytes()); // extra
            if central {
                h.extend([0u8; 6]); // comment, disk, internal attributes
                h.extend(0u32.to_le_bytes()); // external attributes
                h.extend(offset.to_le_bytes());
            }
            h.extend(name.as_bytes());
            h
        };
        out.extend(header(0x0403_4b50, false));
        out.extend(&packed);
        central.extend(header(0x0201_4b50, true));
    }
    let (at, size) = (out.len() as u32, central.len() as u32);
    out.extend(central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]); // disks
    out.extend((parts.len() as u16).to_le_bytes());
    out.extend((parts.len() as u16).to_le_bytes());
    out.extend(size.to_le_bytes());
    out.extend(at.to_le_bytes());
    out.extend(0u16.to_le_bytes()); // comment
    out
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>"#;

const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;

const WORKBOOK_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;

/// Two cell formats: the usual one, and bold for the headings.
const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#;

/// The parts of a zip by name, inflated; and checked against the CRC each header gives.
#[cfg(test)]
pub(crate) fn unzip(b: &[u8]) -> std::collections::BTreeMap<String, String> {
    use std::io::Read;
    let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]) as usize;
    let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let mut out = std::collections::BTreeMap::new();
    let mut i = 0;
    while u32_at(i) == 0x0403_4b50 {
        let (crc, packed, size, name_len) = (u32_at(i + 14), u32_at(i + 18) as usize, u32_at(i + 22) as usize, u16_at(i + 26));
        let name = String::from_utf8(b[i + 30..i + 30 + name_len].to_vec()).unwrap();
        let start = i + 30 + name_len + u16_at(i + 28);
        // Stored as it is, or deflated; a binary part (a picture) comes back as text a test can only look for words in.
        let mut raw = Vec::new();
        if u16_at(i + 8) == 0 {
            raw = b[start..start + packed].to_vec();
        } else {
            flate2::read::DeflateDecoder::new(&b[start..start + packed]).read_to_end(&mut raw).unwrap();
        }
        let mut c = flate2::Crc::new();
        c.update(&raw);
        assert_eq!((c.sum(), raw.len()), (crc, size), "{name}");
        let text = String::from_utf8_lossy(&raw).to_string();
        out.insert(name, text);
        i = start + packed;
    }
    // The central directory lists them all, each pointing at its part, and the end record says where it is.
    let end = b.len() - 22;
    assert_eq!(u32_at(end), 0x0605_4b50);
    assert_eq!((u16_at(end + 10), u32_at(end + 16) as usize), (out.len(), i));
    let mut names = Vec::new();
    while u32_at(i) == 0x0201_4b50 {
        let name_len = u16_at(i + 28);
        let at = u32_at(i + 42) as usize;
        assert_eq!(u32_at(at), 0x0403_4b50);
        names.push(String::from_utf8(b[i + 46..i + 46 + name_len].to_vec()).unwrap());
        i += 46 + name_len + u16_at(i + 30) + u16_at(i + 32);
    }
    assert_eq!((i, names.len()), (end, out.len()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workbook_holds_its_headings_in_bold_numbers_as_numbers_and_text_escaped() {
        let rows = vec![
            vec![Cell::Text("A & <B>".into()), Cell::Number(18.5)],
            vec![Cell::Text("=SUM(A1)".into()), Cell::Empty],
            vec![Cell::Empty, Cell::Number(18.5)],
        ];
        let parts = unzip(&workbook("Room: Areas [1]", &["Name".into(), "Area (m²)".into()], &rows));
        assert_eq!(
            parts.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "[Content_Types].xml",
                "_rels/.rels",
                "xl/_rels/workbook.xml.rels",
                "xl/styles.xml",
                "xl/workbook.xml",
                "xl/worksheets/sheet1.xml"
            ]
        );
        assert!(parts["xl/workbook.xml"].contains(r#"<sheet name="Room Areas 1" sheetId="1""#), "a sheet name holds no : [ ]");
        let sheet = &parts["xl/worksheets/sheet1.xml"];
        assert!(sheet.contains(r#"<c r="B1" s="1" t="inlineStr"><is><t xml:space="preserve">Area (m²)</t></is></c>"#), "{sheet}");
        assert!(
            sheet
                .contains(r#"<c r="A2" t="inlineStr"><is><t xml:space="preserve">A &amp; &lt;B&gt;</t></is></c><c r="B2"><v>18.5</v></c>"#)
        );
        assert!(sheet.contains(r#"<t xml:space="preserve">=SUM(A1)</t>"#), "text, never a formula");
        assert!(sheet.contains(r#"<row r="4"><c r="B4"><v>18.5</v></c></row>"#), "an empty cell is left out");
        assert!(!sheet.contains("<f>"));
    }

    #[test]
    fn columns_past_z_take_two_letters_and_a_name_cut_to_what_a_sheet_holds() {
        assert_eq!([column(0), column(25), column(26), column(27), column(701), column(702)], ["A", "Z", "AA", "AB", "ZZ", "AAA"]);
        assert_eq!(sheet_name(&"x".repeat(40)).len(), MAX_NAME);
        assert_eq!(sheet_name("[]"), "Sheet1");
        assert_eq!(escape("a\u{1}b\tc"), "ab\tc");
    }
}
