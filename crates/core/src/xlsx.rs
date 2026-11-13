//! A workbook of one sheet as Office Open XML (`.xlsx`), written by hand: the five parts a spreadsheet needs, deflated into a
//! zip with `flate2`. Text goes in as inline strings, so there is no shared-string table; numbers as numbers; the first row
//! in bold. [`read`] reads the first sheet of any workbook back, as text.

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
    zip(&parts)
}

fn workbook_xml(name: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="{}" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        escape(name)
    )
}

/// The most bytes the parts of a workbook read (its shared strings and its first sheet) inflate to, together.
const MAX_PART: u64 = 256 << 20;
/// The most rows, and columns, read from a sheet.
const MAX_ROWS: usize = 65_536;
const MAX_COLUMNS: usize = 256;

/// The first sheet of a workbook, as rows of text: a spreadsheet's own, with its shared strings, or one of ours. A number comes
/// back as it was written and a date as its serial number. Cells a row skips are empty. A workbook is hostile: every offset is
/// checked, a part inflates only so far, and a file that isn't one is an error.
pub fn read(bytes: &[u8]) -> Result<Vec<Vec<String>>, String> {
    let parts = zip_entries(bytes, MAX_PART)?;
    let part = |name: &str| parts.iter().find(|(n, _)| n == name).map(|(_, b)| String::from_utf8_lossy(b).into_owned());
    let mut sheets: Vec<&String> =
        parts.iter().map(|(n, _)| n).filter(|n| n.starts_with("xl/worksheets/") && n.ends_with(".xml")).collect();
    // `sheet2` before `sheet10`.
    sheets.sort_by_key(|n| (n.len(), n.as_str()));
    let sheet = sheets.first().and_then(|n| part(n)).ok_or("it has no worksheet")?;
    let shared: Vec<String> = part("xl/sharedStrings.xml").map_or_else(Vec::new, |x| {
        tag_bodies(&x, "si").map(|(_, si)| tag_bodies(si, "t").map(|(_, t)| unescape(t)).collect::<String>()).take(MAX_ROWS * 16).collect()
    });
    let mut rows = Vec::new();
    for (_, row) in tag_bodies(&sheet, "row").take(MAX_ROWS) {
        let mut cells: Vec<String> = Vec::new();
        for (head, cell) in tag_bodies(row, "c") {
            let at = attribute(head, "r").map_or(cells.len(), column_index);
            if at >= MAX_COLUMNS {
                continue;
            }
            let value = tag_bodies(cell, "v").next().map(|(_, v)| unescape(v));
            let text = match attribute(head, "t") {
                Some("s") => value.and_then(|v| v.trim().parse::<usize>().ok()).and_then(|i| shared.get(i).cloned()).unwrap_or_default(),
                Some("inlineStr") => tag_bodies(cell, "t").map(|(_, t)| unescape(t)).collect(),
                _ => value.unwrap_or_default(),
            };
            if cells.len() <= at {
                cells.resize(at + 1, String::new());
            }
            if let Some(slot) = cells.get_mut(at) {
                *slot = text;
            }
        }
        rows.push(cells);
    }
    Ok(rows)
}

/// A cell reference's column: `A1` is 0, `AA3` is 26.
fn column_index(reference: &str) -> usize {
    let n = reference
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .fold(0usize, |n, c| n.saturating_mul(26).saturating_add(usize::from(c.to_ascii_uppercase() as u8 - b'A') + 1));
    n.saturating_sub(1)
}

/// Each element `name` in `xml`: its opening tag's attributes, and what it holds (nothing for one that closes itself).
fn tag_bodies<'a>(xml: &'a str, name: &'a str) -> impl Iterator<Item = (&'a str, &'a str)> {
    let mut rest = xml;
    std::iter::from_fn(move || {
        loop {
            let start = rest.find('<')?;
            let after = rest.get(start + 1..)?;
            let name_end = after.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(after.len());
            let found = after.get(..name_end)?;
            // A prefixed tag (`x:row`) is the same tag.
            let bare = found.rsplit(':').next().unwrap_or(found);
            if bare != name {
                rest = after;
                continue;
            }
            let close = after.find('>')?;
            let head = after.get(name_end..close)?;
            if head.ends_with('/') {
                rest = after.get(close + 1..)?;
                return Some((head, ""));
            }
            let body = after.get(close + 1..)?;
            let closing = format!("</{found}>");
            let Some(end) = body.find(&closing) else {
                rest = "";
                return Some((head, body));
            };
            rest = body.get(end + closing.len()..)?;
            return Some((head, body.get(..end)?));
        }
    })
}

/// Attribute `key` of an opening tag.
fn attribute<'a>(head: &'a str, key: &str) -> Option<&'a str> {
    let mut rest = head;
    loop {
        let eq = rest.find('=')?;
        let name = rest.get(..eq)?.trim();
        let after = rest.get(eq + 1..)?.trim_start();
        let quote = after.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        let body = after.get(1..)?;
        let end = body.find(quote)?;
        if name == key {
            return body.get(..end);
        }
        rest = body.get(end + 1..)?;
    }
}

/// XML's entities and character references read.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(rest.get(..at).unwrap_or(""));
        let tail = rest.get(at..).unwrap_or("");
        let Some(end) = tail.find(';').filter(|e| *e <= 12) else {
            out.push('&');
            rest = tail.get(1..).unwrap_or("");
            continue;
        };
        let name = tail.get(1..end).unwrap_or("");
        let c = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => name
                .strip_prefix("#x")
                .map_or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()), |h| u32::from_str_radix(h, 16).ok())
                .and_then(char::from_u32),
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = tail.get(end + 1..).unwrap_or("");
            }
            None => {
                out.push('&');
                rest = tail.get(1..).unwrap_or("");
            }
        }
    }
    out.push_str(rest);
    out
}

/// The parts of a zip a workbook's first sheet needs, by name: its shared strings and its first worksheet (`sheet2` before
/// `sheet10`), found in its central directory before anything is inflated; stored or deflated, the two together no more
/// than `budget` bytes. Every other part, the other sheets among them, is passed over unread.
fn zip_entries(b: &[u8], budget: u64) -> Result<Vec<(String, Vec<u8>)>, String> {
    const NOT: &str = "it isn't a workbook";
    let u16_at = |at: usize| -> Option<usize> { Some(usize::from(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]))) };
    let u32_at = |at: usize| -> Option<usize> {
        Some(u32::from_le_bytes([*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?]) as usize)
    };
    // The end of central directory record is within the last 64 KB, comment and all.
    let from = b.len().saturating_sub(65_557);
    let tail = b.get(from..).ok_or(NOT)?;
    let eocd = tail.windows(4).rposition(|w| w == b"PK\x05\x06").map(|i| i + from).ok_or(NOT)?;
    let count = u16_at(eocd + 10).ok_or(NOT)?;
    let mut at = u32_at(eocd + 16).ok_or(NOT)?;
    // (name, method, compressed size, local header) of the shared strings and of each worksheet.
    let mut listed: Vec<(String, usize, usize, usize)> = Vec::new();
    for _ in 0..count {
        if b.get(at..at.saturating_add(4)) != Some(b"PK\x01\x02") {
            return Err(NOT.into());
        }
        let method = u16_at(at + 10).ok_or(NOT)?;
        let compressed = u32_at(at + 20).ok_or(NOT)?;
        let (name_len, extra, comment) = (u16_at(at + 28).ok_or(NOT)?, u16_at(at + 30).ok_or(NOT)?, u16_at(at + 32).ok_or(NOT)?);
        let local = u32_at(at + 42).ok_or(NOT)?;
        let name = String::from_utf8_lossy(b.get(at + 46..at + 46 + name_len).ok_or(NOT)?).into_owned();
        at = at + 46 + name_len + extra + comment;
        if name == "xl/sharedStrings.xml" || (name.starts_with("xl/worksheets/") && name.ends_with(".xml")) {
            listed.push((name, method, compressed, local));
        }
    }
    let first = listed.iter().map(|e| &e.0).filter(|n| n.starts_with("xl/worksheets/")).min_by_key(|n| (n.len(), n.as_str())).cloned();
    let mut left = budget;
    let mut out = Vec::new();
    for (name, method, compressed, local) in listed {
        if name != "xl/sharedStrings.xml" && Some(&name) != first.as_ref() {
            continue;
        }
        if b.get(local..local.saturating_add(4)) != Some(b"PK\x03\x04") {
            return Err(NOT.into());
        }
        let start = local + 30 + u16_at(local + 26).ok_or(NOT)? + u16_at(local + 28).ok_or(NOT)?;
        let data = b.get(start..start.saturating_add(compressed)).ok_or(NOT)?;
        let mut body = Vec::new();
        match method {
            // Stored, it is no bigger than the file: the check below still counts it.
            0 => body.extend_from_slice(data),
            8 => {
                use std::io::Read;
                flate2::read::DeflateDecoder::new(data)
                    .take(left.saturating_add(1))
                    .read_to_end(&mut body)
                    .map_err(|e| format!("{NOT}: {e}"))?;
            }
            other => return Err(format!("{name} is compressed in a way this reader doesn't know ({other})")),
        }
        let size = body.len() as u64;
        if size > left {
            return Err(format!("its first sheet and its shared strings come to more than the {} MB a workbook is read to", budget >> 20));
        }
        left -= size;
        out.push((name, body));
    }
    Ok(out)
}

/// What a sheet may be called: no `[]:*?/\`, not empty, at most 31 characters.
fn sheet_name(name: &str) -> String {
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
fn escape(s: &str) -> String {
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

/// The parts as a zip, each deflated.
fn zip(parts: &[(&str, String)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, body) in parts {
        let data = body.as_bytes();
        let mut crc = flate2::Crc::new();
        crc.update(data);
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        // Writing to a Vec doesn't fail.
        let packed = enc.write_all(data).and_then(|_| enc.finish()).unwrap_or_default();
        let offset = out.len() as u32;
        let header = |sig: u32, central: bool| {
            let mut h = Vec::new();
            h.extend(sig.to_le_bytes());
            if central {
                h.extend(20u16.to_le_bytes()); // made by
            }
            h.extend(20u16.to_le_bytes()); // version needed
            h.extend(0x0800u16.to_le_bytes()); // names in UTF-8
            h.extend(8u16.to_le_bytes()); // deflated
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
        let mut text = String::new();
        flate2::read::DeflateDecoder::new(&b[start..start + packed]).read_to_string(&mut text).unwrap();
        let mut c = flate2::Crc::new();
        c.update(text.as_bytes());
        assert_eq!((c.sum(), text.len()), (crc, size), "{name}");
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
    fn only_the_first_sheet_and_the_shared_strings_inflate_and_within_one_budget() {
        // Two huge sheets after the first (`sheet10` sorts after `sheet2`) are never inflated.
        let big = "a".repeat(1 << 20);
        let b = zip(&[
            ("xl/worksheets/sheet2.xml", big.clone()),
            ("xl/worksheets/sheet10.xml", big),
            ("xl/worksheets/sheet1.xml", "<row/>".into()),
        ]);
        let parts = zip_entries(&b, 1024).unwrap();
        assert_eq!(parts.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["xl/worksheets/sheet1.xml"]);
        // The first sheet and the shared strings share one budget.
        let b = zip(&[("xl/worksheets/sheet1.xml", "x".repeat(600)), ("xl/sharedStrings.xml", "y".repeat(600))]);
        assert!(zip_entries(&b, 1024).unwrap_err().contains("more than"));
        assert_eq!(zip_entries(&b, 2048).unwrap().len(), 2);
    }

    #[test]
    fn columns_past_z_take_two_letters_and_a_name_cut_to_what_a_sheet_holds() {
        assert_eq!([column(0), column(25), column(26), column(27), column(701), column(702)], ["A", "Z", "AA", "AB", "ZZ", "AAA"]);
        assert_eq!(sheet_name(&"x".repeat(40)).len(), MAX_NAME);
        assert_eq!(sheet_name("[]"), "Sheet1");
        assert_eq!(escape("a\u{1}b\tc"), "ab\tc");
    }

    /// A zip of `parts`, stored: what a spreadsheet writes, in the way a reader must take it.
    fn zipped(parts: &[(&str, &str)]) -> Vec<u8> {
        let (mut out, mut central) = (Vec::new(), Vec::new());
        for (name, body) in parts {
            let at = out.len() as u32;
            let (n, b) = (name.len() as u16, body.len() as u32);
            out.extend(b"PK\x03\x04\x14\0\0\0\0\0\0\0\0\0\0\0\0\0");
            out.extend(b.to_le_bytes());
            out.extend(b.to_le_bytes());
            out.extend(n.to_le_bytes());
            out.extend(0u16.to_le_bytes());
            out.extend(name.as_bytes());
            out.extend(body.as_bytes());
            central.extend(b"PK\x01\x02\x14\0\x14\0\0\0\0\0\0\0\0\0\0\0\0\0");
            central.extend(b.to_le_bytes());
            central.extend(b.to_le_bytes());
            central.extend(n.to_le_bytes());
            central.extend([0u8; 10]);
            central.extend([0u8; 2]);
            central.extend(at.to_le_bytes());
            central.extend(name.as_bytes());
        }
        let start = out.len() as u32;
        let size = central.len() as u32;
        out.extend(central);
        out.extend(b"PK\x05\x06\0\0\0\0");
        out.extend((parts.len() as u16).to_le_bytes());
        out.extend((parts.len() as u16).to_le_bytes());
        out.extend(size.to_le_bytes());
        out.extend(start.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out
    }

    #[test]
    fn a_spreadsheets_own_workbook_is_read_with_its_shared_strings_and_the_cells_it_skips() {
        let shared = r#"<sst xmlns="x"><si><t>Name</t></si><si><t xml:space="preserve">Fire &amp; Smoke</t></si><si><r><t>Ri</t></r><r><rPr><b/></rPr><t>ch</t></r></si></sst>"#;
        let sheet = r#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="C1" t="s"><v>2</v></c></row><row r="2" spans="1:3"><c r="A2" s="1" t="s"><v>1</v></c><c r="B2"/><c r="C2"><v>12.5</v></c><c r="D2" t="inlineStr"><is><t>in&#x41;</t></is></c></row><row r="3"/></sheetData></worksheet>"#;
        let book = zipped(&[
            ("xl/workbook.xml", "<workbook/>"),
            ("xl/sharedStrings.xml", shared),
            ("xl/worksheets/sheet10.xml", "<worksheet/>"),
            ("xl/worksheets/sheet1.xml", sheet),
        ]);
        let rows = read(&book).unwrap();
        assert_eq!(rows[0], ["Name", "", "Rich"]);
        assert_eq!(rows[1], ["Fire & Smoke", "", "12.5", "inA"]);
    }

    #[test]
    fn what_isnt_a_workbook_is_an_error_and_ours_reads_back() {
        for junk in [&b""[..], b"PK", b"PK\x05\x06", &[0xFF; 100]] {
            assert!(read(junk).is_err());
        }
        let bad_offset = zipped(&[("xl/worksheets/sheet1.xml", "<x/>")]);
        let cut = bad_offset.get(..bad_offset.len() - 30).unwrap();
        assert!(read(cut).is_err(), "cut short");
        let ours = workbook("S", &["A".to_string(), "B".to_string()], &[vec![Cell::Text("x".into()), Cell::Number(2.0)]]);
        assert_eq!(read(&ours).unwrap()[1], ["x", "2"]);
    }
}
