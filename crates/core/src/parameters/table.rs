//! Parameter definitions as files: tables (`.xlsx`, `.csv`), a dictionary (`.bimparams`), and Revit's shared parameter files
//! (`.txt`), which are read and never written. Their layout is the one Autodesk's Revit API documentation gives under
//! "Definition File": tab-separated blocks, each headed by a `*` line naming its columns. Columns are found by those names,
//! so one this reader doesn't know is passed over. Every file is hostile: sizes are capped, and a row that can't be read is
//! listed in `left_out` and the rest come in.

use serde_json::Value;

use super::{DICTIONARY_EXTENSION, DICTIONARY_FORMAT, Definition, Dictionary, kind_word};
use crate::ids::Guid;
use crate::model::{MAX_TEXT, ParameterKind};
use crate::xlsx::{self, Cell};

/// The largest file read.
const MAX_FILE: u64 = 64 << 20;
/// The most rows read from a file.
pub const MAX_ROWS: usize = 65_536;
/// The most rows named as left out.
const MAX_LEFT_OUT: usize = 200;

/// The columns of a table, in the order written.
const HEADINGS: [&str; 9] = ["GUID", "Name", "Kind", "Categories", "Group", "Description", "Visible", "User Modifiable", "Hide When Empty"];

/// The dictionary a file is, or stands for.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub name: String,
    pub version: String,
    pub guid: Option<Guid>,
}

/// What a file holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Read {
    pub definitions: Vec<Definition>,
    pub dictionary: Option<Source>,
    pub left_out: Vec<String>,
}

impl Read {
    fn leave_out(&mut self, why: String) {
        if self.left_out.len() < MAX_LEFT_OUT {
            self.left_out.push(why);
        }
    }
}

/// A path's file name, less its extension.
pub fn stem(path: &str) -> String {
    let s = std::path::Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or("Dictionary");
    s.chars().take(MAX_TEXT).collect()
}

fn bytes_of(path: &str) -> Result<Vec<u8>, String> {
    let size = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?.len();
    if size >= MAX_FILE {
        return Err(format!("{path} is {size} bytes; a parameter file holds up to {MAX_FILE}"));
    }
    std::fs::read(path).map_err(|e| format!("{path}: {e}"))
}

/// The definitions in the file at `path`, by its extension.
pub fn read_file(path: &str) -> Result<Read, String> {
    let lower = path.to_lowercase();
    let bytes = bytes_of(path)?;
    if lower.ends_with(".xlsx") {
        let rows = xlsx::read(&bytes).map_err(|e| format!("{path}: {e}"))?;
        return Ok(from_rows(&rows));
    }
    if lower.ends_with(".csv") {
        return Ok(from_rows(&read_csv(&decode(&bytes))));
    }
    if lower.ends_with(".txt") {
        return read_shared(&decode(&bytes), &stem(path)).map_err(|e| format!("{path}: {e}"));
    }
    if lower.ends_with(&format!(".{DICTIONARY_EXTENSION}")) || lower.ends_with(".json") {
        return read_dictionary(&decode(&bytes)).map_err(|e| format!("{path}: {e}"));
    }
    Err(format!("{path}: a path ending .xlsx, .csv, .txt (a Revit shared parameter file) or .{DICTIONARY_EXTENSION}"))
}

/// The dictionary in the file at `path`.
pub fn read_dictionary_file(path: &str) -> Result<Dictionary, String> {
    let read = read_dictionary(&decode(&bytes_of(path)?)).map_err(|e| format!("{path}: {e}"))?;
    let source = read.dictionary.clone();
    Ok(Dictionary {
        format: DICTIONARY_FORMAT,
        guid: source.as_ref().and_then(|s| s.guid),
        name: source.as_ref().map_or_else(|| stem(path), |s| s.name.clone()),
        version: source.map(|s| s.version).unwrap_or_default(),
        parameters: read.definitions,
    })
}

/// Text from what a file holds: UTF-8 or UTF-16 by its mark (Revit writes either), else UTF-8 and then Windows-1252.
fn decode(bytes: &[u8]) -> String {
    if let Some((enc, _)) = encoding_rs::Encoding::for_bom(bytes) {
        return enc.decode(bytes).0.into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned(),
    }
}

/// The kind a file's word for one says, and a note when it is only near it; none where a parameter here can't hold it.
fn kind_from(word: &str) -> Option<(ParameterKind, Option<String>)> {
    let raw = word.trim();
    // Revit's newer forms: `autodesk.spec.aec:length-2.0.0`, `autodesk.spec:spec.string-2.0.0`.
    let tail = raw.rsplit(':').next().unwrap_or(raw);
    let tail = tail.split('-').next().unwrap_or(tail);
    let tail = tail.rsplit('.').next().unwrap_or(tail);
    let key: String = tail.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
    Some(match key.as_str() {
        "text" | "string" | "multilinetext" | "url" => (ParameterKind::Text, None),
        "number" | "double" | "real" => (ParameterKind::Number, None),
        "integer" | "int" | "int64" => (ParameterKind::Integer, None),
        "length" => (ParameterKind::Length, None),
        "area" => (ParameterKind::Area, None),
        "volume" => (ParameterKind::Volume, None),
        "angle" => (ParameterKind::Angle, None),
        "yesno" | "yes" | "bool" | "boolean" => (ParameterKind::YesNo, None),
        "familytype" | "reference" | "material" | "image" | "loadclassification" => return None,
        "" => (ParameterKind::Text, None),
        _ => (ParameterKind::Number, Some(format!("`{raw}` has no kind here; it comes in as a number"))),
    })
}

/// A yes or no a table writes; `default` for one it left blank.
fn yes(cell: &str, default: bool) -> bool {
    match cell.trim().to_lowercase().as_str() {
        "" => default,
        "1" | "yes" | "y" | "true" | "x" | "sim" | "si" | "ja" | "oui" => true,
        _ => false,
    }
}

fn clip(t: &str) -> String {
    t.trim().chars().take(MAX_TEXT).collect()
}

/// A GUID a file gives, and none where it gives what isn't one.
fn guid_of(cell: &str) -> Option<Guid> {
    Guid::parse(&cell.trim().trim_matches(['{', '}']).to_lowercase()).filter(|g| g.pending().is_none())
}

/// Definitions from the rows of a table: its first row names its columns, in any order and case.
pub fn from_rows(rows: &[Vec<String>]) -> Read {
    let mut read = Read::default();
    let Some((head, body)) = rows.split_first() else { return read };
    let key = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
    let keys: Vec<String> = head.iter().map(|h| key(h)).collect();
    let at = |names: &[&str]| keys.iter().position(|k| names.contains(&k.as_str()));
    let Some(name_at) = at(&["name", "parametername", "parameter"]) else {
        read.leave_out("the table has no Name column".into());
        return read;
    };
    let guid_at = at(&["guid", "uniqueid"]);
    let kind_at = at(&["kind", "type", "typeofparameter", "datatype"]);
    let cats_at = at(&["categories", "category"]);
    let group_at = at(&["group", "groupunder"]);
    let desc_at = at(&["description", "tooltip"]);
    let vis_at = at(&["visible"]);
    let user_at = at(&["usermodifiable"]);
    let hide_at = at(&["hidewhenempty", "hidewhennovalue"]);
    let cell = |row: &'_ Vec<String>, i: Option<usize>| -> String { i.and_then(|i| row.get(i)).cloned().unwrap_or_default() };
    for (n, row) in body.iter().enumerate().take(MAX_ROWS) {
        let name = clip(&cell(row, Some(name_at)));
        if name.is_empty() {
            continue;
        }
        let word = cell(row, kind_at);
        let Some((kind, note)) = kind_from(&word) else {
            read.leave_out(format!("row {}, `{name}`: `{}` isn't a kind a parameter holds here", n + 2, word.trim()));
            continue;
        };
        if let Some(note) = note {
            read.leave_out(format!("row {}, `{name}`: {note}", n + 2));
        }
        let raw_guid = cell(row, guid_at);
        let guid = guid_of(&raw_guid);
        if guid.is_none() && !raw_guid.trim().is_empty() {
            read.leave_out(format!("row {}, `{name}`: `{}` isn't a GUID; matched by its name", n + 2, raw_guid.trim()));
        }
        read.definitions.push(Definition {
            guid,
            name,
            kind,
            description: clip(&cell(row, desc_at)),
            group: clip(&cell(row, group_at)),
            hidden: !yes(&cell(row, vis_at), true),
            read_only: !yes(&cell(row, user_at), true),
            hide_when_empty: yes(&cell(row, hide_at), false),
            categories: cell(row, cats_at).split([';', '|', '\n']).map(clip).filter(|c| !c.is_empty()).collect(),
        });
    }
    if body.len() > MAX_ROWS {
        read.leave_out(format!("rows past the {MAX_ROWS} a table is read to"));
    }
    read
}

/// The rows of a CSV: comma or semicolon (as a spreadsheet in much of the world writes it), by which its first line has more
/// of; quoted cells, doubled quotes; and the `'` a schedule's CSV puts before text a spreadsheet would run, taken off.
pub fn read_csv(text: &str) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let first = text.lines().next().unwrap_or("");
    let sep = if first.matches(';').count() > first.matches(',').count() { ';' } else { ',' };
    let (mut rows, mut row, mut cell) = (Vec::new(), Vec::new(), String::new());
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let tidy = |c: String| -> String {
        match c.strip_prefix('\'') {
            Some(rest) if rest.starts_with(['=', '+', '-', '@']) => rest.to_string(),
            _ => c,
        }
    };
    while let Some(c) = chars.next() {
        if rows.len() > MAX_ROWS {
            break;
        }
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            } else {
                cell.push(c);
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() => quoted = true,
            '\n' | '\r' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(tidy(std::mem::take(&mut cell)));
                rows.push(std::mem::take(&mut row));
            }
            c if c == sep => row.push(tidy(std::mem::take(&mut cell))),
            c => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(tidy(cell));
        rows.push(row);
    }
    rows
}

/// A Revit shared parameter file. Its GUIDs are kept, which is why it is worth reading. It says nothing of categories, so each
/// row comes in with none, for the import to give it.
pub fn read_shared(text: &str, name: &str) -> Result<Read, String> {
    let mut read = Read { dictionary: Some(Source { name: name.to_string(), version: String::new(), guid: None }), ..Read::default() };
    let mut groups: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut columns: Vec<String> = Vec::new();
    let mut group_columns: Vec<String> = Vec::new();
    let mut saw_param_block = false;
    for (n, line) in text.lines().enumerate() {
        let line = line.trim_start_matches('\u{feff}');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let cells: Vec<&str> = line.split('\t').collect();
        match cells.first().copied().unwrap_or("") {
            "*GROUP" => group_columns = cells.iter().map(|s| s.trim().to_string()).collect(),
            "*PARAM" => {
                saw_param_block = true;
                columns = cells.iter().map(|s| s.trim().to_string()).collect();
            }
            "GROUP" => {
                let at = |k: &str| group_columns.iter().position(|c| c == k).and_then(|i| cells.get(i)).copied();
                if let (Some(id), Some(g)) = (at("ID"), at("NAME")) {
                    groups.insert(id.trim().to_string(), clip(g));
                }
            }
            "PARAM" => {
                if read.definitions.len() >= MAX_ROWS {
                    read.leave_out(format!("parameters past the {MAX_ROWS} a file is read to"));
                    break;
                }
                let at = |k: &str| columns.iter().position(|c| c == k).and_then(|i| cells.get(i)).copied().unwrap_or("");
                let name = clip(at("NAME"));
                let Some(guid) = guid_of(at("GUID")) else {
                    read.leave_out(format!("line {}, `{name}`: `{}` isn't a GUID", n + 1, at("GUID")));
                    continue;
                };
                if name.is_empty() {
                    read.leave_out(format!("line {}: a parameter with no name", n + 1));
                    continue;
                }
                let Some((kind, note)) = kind_from(at("DATATYPE")) else {
                    read.leave_out(format!("`{name}`: `{}` isn't a kind a parameter holds here", at("DATATYPE")));
                    continue;
                };
                if let Some(note) = note {
                    read.leave_out(format!("`{name}`: {note}"));
                }
                read.definitions.push(Definition {
                    guid: Some(guid),
                    name,
                    kind,
                    description: clip(at("DESCRIPTION")),
                    group: groups.get(at("GROUP").trim()).cloned().unwrap_or_default(),
                    hidden: !yes(at("VISIBLE"), true),
                    read_only: !yes(at("USERMODIFIABLE"), true),
                    hide_when_empty: yes(at("HIDEWHENNOVALUE"), false),
                    categories: Vec::new(),
                });
            }
            _ => {}
        }
    }
    if !saw_param_block {
        return Err("it isn't a shared parameter file: it has no *PARAM line".into());
    }
    Ok(read)
}

/// A dictionary's file, read kindly: what this version doesn't know of it is passed over, a row that can't be read is listed,
/// and a file of a newer format is refused.
pub fn read_dictionary(text: &str) -> Result<Read, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("it isn't a dictionary: {e}"))?;
    let format = v.get("format").and_then(Value::as_u64).ok_or("it isn't a dictionary: it has no `format`")?;
    if format > u64::from(DICTIONARY_FORMAT) {
        return Err(format!("it is a dictionary of format {format}, newer than this BIMCraft's {DICTIONARY_FORMAT}: update BIMCraft"));
    }
    let name =
        v.get("name").and_then(Value::as_str).map(clip).filter(|n| !n.is_empty()).ok_or("it isn't a dictionary: it has no `name`")?;
    let text_of = |k: &str| v.get(k).and_then(Value::as_str).map(clip).unwrap_or_default();
    let guid = guid_of(&text_of("guid"));
    let mut read = Read { dictionary: Some(Source { name, version: text_of("version"), guid }), ..Read::default() };
    let list = v.get("parameters").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    for (i, row) in list.iter().enumerate().take(MAX_ROWS) {
        match serde_json::from_value::<Definition>(row.clone()) {
            Ok(mut d) => {
                d.name = clip(&d.name);
                d.description = clip(&d.description);
                d.group = clip(&d.group);
                d.categories = d.categories.iter().take(256).map(|c| clip(c)).collect();
                d.guid = d.guid.filter(|g| g.pending().is_none());
                if d.name.is_empty() {
                    read.leave_out(format!("parameter {}: it has no name", i + 1));
                } else {
                    read.definitions.push(d);
                }
            }
            Err(e) => read.leave_out(format!("parameter {}: {e}", i + 1)),
        }
    }
    Ok(read)
}

fn row_of(d: &Definition) -> Vec<String> {
    let yes_no = |b: bool| if b { "Yes" } else { "No" }.to_string();
    vec![
        d.guid.map(|g| g.to_string()).unwrap_or_default(),
        d.name.clone(),
        kind_word(d.kind).to_string(),
        d.categories.join("; "),
        d.group.clone(),
        d.description.clone(),
        yes_no(!d.hidden),
        yes_no(!d.read_only),
        yes_no(d.hide_when_empty),
    ]
}

/// A table of `defs` as CSV. Text a spreadsheet would run as a formula goes in with a `'` before it.
pub fn to_csv(defs: &[Definition]) -> String {
    let cell = |s: &str| {
        let s = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{s}") } else { s.to_string() };
        if s.contains([',', ';', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s }
    };
    let mut out = HEADINGS.iter().map(|h| cell(h)).collect::<Vec<_>>().join(",");
    out.push('\n');
    for d in defs {
        out.push_str(&row_of(d).iter().map(|c| cell(c)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

/// A workbook of `defs`.
pub fn to_xlsx(defs: &[Definition]) -> Vec<u8> {
    let headings: Vec<String> = HEADINGS.iter().map(|h| (*h).to_string()).collect();
    let rows: Vec<Vec<Cell>> = defs.iter().map(|d| row_of(d).into_iter().map(Cell::Text).collect()).collect();
    xlsx::workbook("Parameters", &headings, &rows)
}
