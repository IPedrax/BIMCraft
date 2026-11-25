//! A spreadsheet of one table as OpenDocument (`.ods`, OASIS ODF 1.2), written by hand as `xlsx.rs` writes a workbook: the
//! `mimetype` first and stored, as ODF asks, then the manifest and the content, deflated. The first row in bold; numbers as
//! numbers a spreadsheet can sum; text with its runs of spaces kept. Catalogue imports read the first sheet with size limits.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use quick_xml::{Reader, events::Event};
use zip::ZipArchive;

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

pub const MAX_ROWS: usize = 20_000;
pub const MAX_CELL: usize = 16_384;
const MAX_COLUMNS: usize = 64;
const MAX_XML: u64 = 32 << 20;

pub(crate) fn workbook_path(path: &Path) -> Result<PathBuf, String> {
    let path = std::path::absolute(path).map_err(|e| e.to_string())?;
    if !path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("ods")) || path.as_os_str().len() > 4096 {
        return Err("Catalogue path must name an .ods file and be at most 4096 bytes".into());
    }
    Ok(path)
}

pub(crate) fn workbook_exists(path: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Ok(true),
        Ok(_) => Err("Catalogue path must refer to a regular file, not a link or special file".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn read_workbook(path: &Path) -> Result<Vec<u8>, String> {
    if !workbook_exists(path)? {
        return Err(format!("Catalogue file does not exist: {}", path.display()));
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let file = {
        use rustix::fs::{Mode, OFlags, open};
        std::fs::File::from(open(path, OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW, Mode::empty()).map_err(|e| e.to_string())?)
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > MAX_XML {
        return Err("Catalogue must be a regular file of at most 32 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_XML + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_XML {
        return Err("Catalogue must be a regular file of at most 32 MiB".into());
    }
    Ok(bytes)
}

/// Export catalogue rows as literal strings through the common ODS writer.
pub(crate) fn write(rows: &[Vec<String>]) -> Result<Vec<u8>, String> {
    let (headings, rows) = rows.split_first().ok_or("ODS catalogue needs a header")?;
    if rows.len() >= MAX_ROWS {
        return Err("ODS catalogue has too many rows".into());
    }
    let mut xml_bytes = 4096_u64;
    for row in std::iter::once(headings).chain(rows) {
        if row.len() > MAX_COLUMNS {
            return Err("ODS catalogue has too many columns".into());
        }
        xml_bytes += 64;
        for cell in row {
            if cell.len() > MAX_CELL || cell.chars().any(|c| c < ' ' && !matches!(c, '\n' | '\r' | '\t')) {
                return Err("ODS cell is too long or contains control characters".into());
            }
            xml_bytes += 160 + 8 * cell.len() as u64;
        }
        if xml_bytes > MAX_XML {
            return Err("ODS catalogue exceeds the size limit".into());
        }
    }
    let cells: Vec<_> = rows.iter().map(|row| row.iter().cloned().map(Cell::Text).collect()).collect();
    Ok(spreadsheet("Catalogue", headings, &cells))
}

/// Read the first sheet, omitting empty rows and trailing cells without expanding office-editor padding.
pub fn read(bytes: &[u8]) -> Result<Vec<Vec<String>>, String> {
    if bytes.len() as u64 > MAX_XML {
        return Err("ODS file exceeds the size limit".into());
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if archive.len() > 256 {
        return Err("ODS archive has too many entries".into());
    }
    let entry = archive.by_name("content.xml").map_err(|e| e.to_string())?;
    let mut xml = String::new();
    entry.take(MAX_XML + 1).read_to_string(&mut xml).map_err(|e| e.to_string())?;
    if xml.len() as u64 > MAX_XML {
        return Err("ODS content exceeds the size limit".into());
    }
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().expand_empty_elements = true;
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut scalar = None;
    let (mut sheet, mut in_cell) = (false, false);
    let (mut row_repeat, mut cell_repeat) = (1, 1);
    let mut expanded_bytes = 0_u64;
    let mut empty_columns = 0;
    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"table" => sheet = true,
            Event::Start(e) if sheet && e.local_name().as_ref() == b"table-row" => {
                row.clear();
                empty_columns = 0;
                row_repeat = repeated(&e, b"number-rows-repeated", 1_048_576)?;
            }
            Event::Start(e) if sheet && e.local_name().as_ref() == b"table-cell" => {
                cell.clear();
                scalar = cell_scalar(&e)?;
                in_cell = true;
                cell_repeat = repeated(&e, b"number-columns-repeated", 1_048_576)?;
            }
            Event::Start(e) | Event::Empty(e) if in_cell && e.local_name().as_ref() == b"s" => {
                let count = repeated(&e, b"c", MAX_CELL)?;
                cell.extend(std::iter::repeat_n(' ', count));
            }
            Event::Start(e) | Event::Empty(e) if in_cell && e.local_name().as_ref() == b"tab" => cell.push('\t'),
            Event::Start(e) | Event::Empty(e) if in_cell && e.local_name().as_ref() == b"line-break" => cell.push('\n'),
            Event::CData(e) if in_cell => cell.push_str(&e.decode().map_err(|e| e.to_string())?),
            Event::Text(e) if in_cell => {
                let text = e.xml_content(quick_xml::XmlVersion::Implicit1_0).map_err(|e| e.to_string())?;
                cell.push_str(&quick_xml::escape::unescape(&text).map_err(|e| e.to_string())?);
                if cell.len() > MAX_CELL {
                    return Err("ODS cell is too long".into());
                }
            }
            Event::GeneralRef(e) if in_cell => {
                let name = e.decode().map_err(|e| e.to_string())?;
                cell.push_str(&quick_xml::escape::unescape(&format!("&{name};")).map_err(|e| e.to_string())?);
                if cell.len() > MAX_CELL {
                    return Err("ODS cell is too long".into());
                }
            }
            Event::End(e) if sheet && e.local_name().as_ref() == b"table-cell" => {
                in_cell = false;
                if cell.is_empty() && scalar.is_none() {
                    empty_columns = (empty_columns + cell_repeat).min(MAX_COLUMNS + 1);
                } else {
                    if row.len() + empty_columns + cell_repeat > MAX_COLUMNS {
                        return Err("ODS row has too many cells".into());
                    }
                    row.resize(row.len() + empty_columns, String::new());
                    empty_columns = 0;
                    for _ in 0..cell_repeat {
                        let heading = rows.first().and_then(|header: &Vec<String>| header.get(row.len())).map(String::as_str);
                        let value = match (heading, scalar.as_ref()) {
                            (
                                Some("Width_mm" | "Depth_mm" | "Height_mm" | "Mass_kg" | "ClearWidth_mm" | "ClearDepth_mm"),
                                Some(("float", value)),
                            )
                            | (Some("Round"), Some(("boolean", value))) => value,
                            (_, Some((_, value))) if cell.is_empty() => value,
                            _ => &cell,
                        };
                        row.push(value.clone());
                    }
                }
            }
            Event::End(e) if sheet && e.local_name().as_ref() == b"table-row" => {
                if row.is_empty() {
                    continue;
                }
                if rows.len() + row_repeat > MAX_ROWS {
                    return Err("ODS catalogue has too many rows".into());
                }
                let row_bytes = std::mem::size_of::<Vec<String>>() as u64
                    + row.iter().map(|cell| cell.len() as u64 + std::mem::size_of::<String>() as u64).sum::<u64>();
                expanded_bytes += row_bytes * row_repeat as u64;
                if expanded_bytes > MAX_XML {
                    return Err("ODS expanded cells exceed the size limit".into());
                }
                rows.extend(std::iter::repeat_n(row.clone(), row_repeat));
            }
            Event::End(e) if sheet && e.local_name().as_ref() == b"table" => return Ok(rows),
            Event::DocType(_) => return Err("ODS document types are not supported".into()),
            Event::Eof => return Err("ODS first sheet is missing or incomplete".into()),
            _ => {}
        }
        if cell.len() > MAX_CELL {
            return Err("ODS cell is too long".into());
        }
    }
}

/// ODF 1.3, 19.388–19.389: stored scalars are independent of localized display text.
fn cell_scalar(e: &quick_xml::events::BytesStart<'_>) -> Result<Option<(&'static str, String)>, String> {
    let (mut kind, mut number, mut boolean) = (None, None, None);
    for attr in e.attributes() {
        let attr = attr.map_err(|e| e.to_string())?;
        let name = attr.key.local_name();
        let target = match name.as_ref() {
            b"value-type" => &mut kind,
            b"value" => &mut number,
            b"boolean-value" => &mut boolean,
            _ => continue,
        };
        let value = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0).map_err(|e| e.to_string())?;
        if value.len() > MAX_CELL {
            return Err("ODS cell is too long".into());
        }
        *target = Some(value.trim().to_owned());
    }
    match kind.as_deref() {
        Some("float") => Ok(number.map(|value| ("float", value))),
        Some("boolean") => boolean
            .map(|value| match value.as_str() {
                "true" | "1" => Ok(("boolean", "true".into())),
                "false" | "0" => Ok(("boolean", "false".into())),
                _ => Err("ODS boolean value must be true, false, 1 or 0".into()),
            })
            .transpose(),
        _ => Ok(None),
    }
}

fn repeated(e: &quick_xml::events::BytesStart<'_>, name: &[u8], max: usize) -> Result<usize, String> {
    for attr in e.attributes() {
        let attr = attr.map_err(|e| e.to_string())?;
        if attr.key.local_name().as_ref() == name {
            let count = std::str::from_utf8(&attr.value).map_err(|e| e.to_string())?.parse::<usize>().map_err(|e| e.to_string())?;
            if count == 0 || count > max {
                return Err("ODS repetition exceeds the limit".into());
            }
            return Ok(count);
        }
    }
    Ok(1)
}

const CATEGORIES: [&str; 4] = ["conduit", "cabletray", "wire", "cable"];
const HEADINGS: [&str; 7] = ["GUID", "Category", "Name", "Width_mm", "Depth_mm", "Round", "Material"];

pub(crate) fn catalogue_rows(model: &crate::model::Model) -> Result<Vec<Vec<String>>, String> {
    let mut rows = vec![HEADINGS.map(String::from).to_vec()];
    for category in CATEGORIES {
        let kind = crate::library::Kind::Type(category.into());
        for (name, ty) in model.types.runs.get(category).into_iter().flatten() {
            if rows.len() >= MAX_ROWS {
                return Err("ODS catalogue has too many rows".into());
            }
            let guid = model.library.guid(&kind, name).ok_or_else(|| format!("catalogue type `{name}` has no library GUID"))?;
            rows.push(vec![
                guid.to_string(),
                category.into(),
                name.clone(),
                ty.width.to_string(),
                ty.depth.to_string(),
                ty.round.to_string(),
                ty.material.clone(),
            ]);
        }
    }
    Ok(rows)
}

pub(crate) fn apply_catalogue(model: &mut crate::model::Model, rows: &[Vec<String>]) -> Result<usize, String> {
    if rows.first().is_none_or(|r| r.iter().map(String::as_str).ne(HEADINGS)) {
        return Err("ODS catalogue headings do not match the exported schema".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut count = 0;
    for (index, row) in rows.iter().enumerate().skip(1) {
        if row.iter().all(String::is_empty) {
            continue;
        }
        let line = index + 1;
        let [guid, category, name, width, depth, round, rest @ ..] = row.as_slice() else {
            return Err(format!("ODS row {line} needs GUID, category, name, width, depth and round columns"));
        };
        if rest.len() > 1 || !CATEGORIES.contains(&category.as_str()) || name.trim().is_empty() {
            return Err(format!("ODS row {line} needs a supported category, a type name and at most seven columns"));
        }
        if !seen.insert((category, name)) {
            return Err(format!("ODS catalogue repeats type `{name}` in `{category}`"));
        }
        let kind = crate::library::Kind::Type(category.clone());
        if guid.is_empty() {
            if model.library.id(&kind, name).is_some() {
                return Err(format!("ODS type `{name}` already exists; keep its GUID"));
            }
        } else {
            let guid: crate::ids::Guid =
                serde_json::from_value(serde_json::Value::String(guid.clone())).map_err(|e| format!("ODS row {line}: {e}"))?;
            let (_, entry) = model.library.by_guid(guid).ok_or_else(|| format!("ODS row {line} has a GUID outside this project"))?;
            if entry.kind != kind || entry.name != *name {
                return Err(format!("ODS row {line} GUID identifies a different category or type"));
            }
        }
        let dimension = |value: &str| -> Result<f64, String> {
            value
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite() && *n > 0.0 && *n <= crate::model::MAX_COORD)
                .ok_or_else(|| format!("ODS row {line} dimensions must be positive and within the coordinate limit"))
        };
        let (width, depth) = (dimension(width)?, dimension(depth)?);
        let round = round.parse().map_err(|_| format!("ODS row {line} Round must be true or false"))?;
        if round && width != depth {
            return Err(format!("ODS row {line} round type needs equal width and depth"));
        }
        let material = rest.first().cloned().unwrap_or_default();
        let ty = model.types.runs.entry(category.clone()).or_default().entry(name.clone()).or_default();
        ty.width = width;
        ty.depth = depth;
        ty.round = round;
        ty.material = material;
        count += 1;
    }
    Ok(count)
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
