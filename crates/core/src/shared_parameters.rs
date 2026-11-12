//! A shared parameters file, as Revit's Manage > Shared Parameters keeps one: a plain-text file of the user's own, outside any
//! project or family, that lists groups and, in them, parameters, each with a GUID, a name, a data type, a description, whether
//! it is shown and whether the user may change it. A family's parameter shared by that GUID (`Parameter::shared`) is the same
//! parameter in every family and project that holds it.
//!
//! The layout is the one such files are commonly seen with: tab-separated lines, `#` a comment, a header line starting with
//! `*` naming the columns of the lines of its kind after it (`*META`, `*GROUP`, `*PARAM`), and the lines themselves (`META`,
//! `GROUP`, `PARAM`).
//! Version 2 adds `DESCRIPTION` and `USERMODIFIABLE` (and later `HIDEWHENNOVALUE`) to version 1's columns; columns are read
//! by their header's names, so either reads, and a column this version doesn't know is passed over. A file is hostile: its
//! size, its lines and its fields are capped, its numbers are whole and in range, its GUIDs are GUIDs, and what isn't one
//! is refused naming its line.

use std::io::Read;
use std::path::Path;

use crate::family::{Category, Kind, nameable};
use crate::ids::Guid;
use crate::model::MAX_TEXT;

/// The largest file read or written, bytes.
pub const MAX_FILE: u64 = 4 << 20;
/// The most lines a file may have.
pub const MAX_LINES: usize = 100_000;
/// The most tab-separated fields on a line.
pub const MAX_FIELDS: usize = 64;
/// The longest description, in characters.
pub const MAX_DESCRIPTION: usize = 1024;
/// The longest data type or data category, in characters.
const MAX_TYPE: usize = 128;
/// The version this writes, and the oldest a reader must know to read it.
const VERSION: u32 = 2;
const MIN_VERSION: u32 = 1;

/// A group of the file: its number, which its parameters name, and its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileGroup {
    pub id: u32,
    pub name: String,
}

/// A parameter of the file, as the file says it: its data type and category kept as written, so a type BIMCraft has no kind
/// for is written back as it came.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Definition {
    pub guid: Guid,
    pub name: String,
    /// `TEXT`, `LENGTH`, …, or a newer spec id (`autodesk.spec.aec:length-2.0.0`).
    pub datatype: String,
    /// A family type parameter's category, by Revit's number for it; else empty.
    pub category: String,
    pub group: u32,
    pub visible: bool,
    pub description: String,
    /// Whether the user may change its value: the file's to say (`USERMODIFIABLE`).
    pub modifiable: bool,
    pub hide_when_no_value: bool,
    /// The line of the file it was read from; 0, one not read.
    pub line: usize,
}

impl Definition {
    /// The kind it is, and for a family type the category whose types it picks, if the file says one BIMCraft knows; or why
    /// BIMCraft has no kind for it, naming its line.
    pub fn kind(&self) -> Result<(Kind, Option<Category>), String> {
        kind_of(&self.datatype, &self.category).ok_or_else(|| {
            format!(
                "{}shared parameter `{}`: its data type `{}` is none BIMCraft has (text, integer, number, length, area, volume, \
                 angle, yes/no, URL, material, family type)",
                self.at(),
                self.name,
                self.datatype
            )
        })
    }

    /// `line N: ` where it was read from, else nothing.
    pub fn at(&self) -> String {
        if self.line > 0 { format!("line {}: ", self.line) } else { String::new() }
    }
}

/// A shared parameters file: its version as read, its groups and its parameters, in the file's order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedFile {
    pub version: u32,
    pub min_version: u32,
    pub groups: Vec<FileGroup>,
    pub parameters: Vec<Definition>,
}

impl Default for SharedFile {
    fn default() -> SharedFile {
        SharedFile { version: VERSION, min_version: MIN_VERSION, groups: Vec::new(), parameters: Vec::new() }
    }
}

/// Revit's numbers for the categories a family type parameter may pick from, where BIMCraft has the category: the public
/// API's built-in category values.
const CATEGORY_IDS: &[(&str, Category)] = &[
    ("-2000080", Category::Furniture),
    ("-2001000", Category::Casework),
    ("-2001160", Category::Plumbing),
    ("-2008049", Category::PipeFitting),
    ("-2008055", Category::PipeAccessory),
    ("-2001040", Category::Equipment),
    ("-2001060", Category::Outlet),
    ("-2001120", Category::Lighting),
    ("-2000151", Category::Generic),
    ("-2000023", Category::Door),
    ("-2000014", Category::Window),
];

/// The kind a data type is, and a family type's category from its data category; none, a type BIMCraft has no kind for.
pub fn kind_of(datatype: &str, category: &str) -> Option<(Kind, Option<Category>)> {
    let kind = match datatype {
        "TEXT" => Kind::Text,
        "INTEGER" => Kind::Integer,
        "NUMBER" => Kind::Number,
        "LENGTH" => Kind::Length,
        "AREA" => Kind::Area,
        "VOLUME" => Kind::Volume,
        "ANGLE" => Kind::Angle,
        "YESNO" => Kind::YesNo,
        "URL" => Kind::Url,
        "MATERIAL" => Kind::Material,
        "FAMILYTYPE" => {
            let of = CATEGORY_IDS.iter().find(|(id, _)| *id == category.trim()).map(|(_, c)| *c);
            return Some((Kind::FamilyType, of));
        }
        spec => {
            // `namespace:name-major.minor.patch`: the version left out.
            let (space, rest) = spec.split_once(':')?;
            let name = match rest.rsplit_once('-') {
                Some((name, v)) if !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit() || b == b'.') => name,
                _ => rest,
            };
            match (space, name) {
                ("autodesk.spec", "spec.string") => Kind::Text,
                ("autodesk.spec", "spec.int64" | "spec.int32") => Kind::Integer,
                ("autodesk.spec", "spec.double") | ("autodesk.spec.aec", "number") => Kind::Number,
                ("autodesk.spec", "spec.bool") => Kind::YesNo,
                ("autodesk.spec.aec", "length") => Kind::Length,
                ("autodesk.spec.aec", "area") => Kind::Area,
                ("autodesk.spec.aec", "volume") => Kind::Volume,
                ("autodesk.spec.aec", "angle") => Kind::Angle,
                ("autodesk.spec.string", "url") => Kind::Url,
                ("autodesk.spec.reference", "material") => Kind::Material,
                _ => return None,
            }
        }
    };
    Some((kind, None))
}

/// The data type and data category a parameter of `kind` is written with (`of`, a family type's category); refused for a family
/// type of a category whose number BIMCraft doesn't know, which a file can't say.
pub fn datatype(kind: Kind, of: Option<Category>) -> Result<(String, String), String> {
    let name = match kind {
        Kind::Text => "TEXT",
        Kind::Integer => "INTEGER",
        Kind::Number => "NUMBER",
        Kind::Length => "LENGTH",
        Kind::Area => "AREA",
        Kind::Volume => "VOLUME",
        Kind::Angle => "ANGLE",
        Kind::YesNo => "YESNO",
        Kind::Url => "URL",
        Kind::Material => "MATERIAL",
        Kind::FamilyType => {
            let of = of.ok_or("a family type parameter needs `of`, the category of the families whose types it picks")?;
            let id = CATEGORY_IDS.iter().find(|(_, c)| *c == of).map(|(id, _)| *id).ok_or_else(|| {
                let known: Vec<String> = CATEGORY_IDS
                    .iter()
                    .map(|(_, c)| serde_json::to_value(c).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default())
                    .collect();
                format!(
                    "a family type parameter of `{}` can't be written: the file names its category by Revit's number, which \
                     BIMCraft knows for {} only",
                    category_name(of),
                    known.join(", ")
                )
            })?;
            return Ok(("FAMILYTYPE".into(), id.into()));
        }
    };
    Ok((name.into(), String::new()))
}

/// A category as commands name it: `furniture`.
pub fn category_name(c: Category) -> String {
    serde_json::to_value(c).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

/// Bytes as text: UTF-8 or UTF-16 by their byte order mark, UTF-16 without one where its first character says so, else
/// UTF-8, else Latin-1 (what an older editor saves).
pub fn decode(bytes: &[u8]) -> Result<String, String> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(rest.to_vec()).map_err(|e| format!("it isn't UTF-8 as its mark says: {e}"));
    }
    let utf16 = |rest: &[u8], little: bool| -> Result<String, String> {
        let (pairs, odd) = rest.as_chunks::<2>();
        if !odd.is_empty() {
            return Err("it isn't UTF-16: an odd number of bytes".into());
        }
        let units = pairs.iter().map(|b| if little { u16::from_le_bytes(*b) } else { u16::from_be_bytes(*b) });
        char::decode_utf16(units).collect::<Result<String, _>>().map_err(|e| format!("it isn't UTF-16: {e}"))
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, false);
    }
    match bytes {
        [a, 0, ..] if *a != 0 => return utf16(bytes, true),
        [0, b, ..] if *b != 0 => return utf16(bytes, false),
        _ => {}
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => Ok(s.to_string()),
        Err(_) => Ok(bytes.iter().map(|b| char::from(*b)).collect()),
    }
}

/// Text a file may hold in a field: no control character, at most `most` characters; a name not empty.
fn field_ok(what: &str, text: &str, most: usize, empty: bool) -> Result<(), String> {
    if !empty && text.trim().is_empty() {
        return Err(format!("no {what}"));
    }
    if text.chars().any(char::is_control) {
        return Err(format!("its {what} holds a control character"));
    }
    if text.chars().count() > most {
        return Err(format!("its {what} is longer than {most} characters"));
    }
    Ok(())
}

/// A whole number of a field, as the file writes one.
fn whole(what: &str, text: &str) -> Result<u32, String> {
    let t = text.trim();
    if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("its {what} is a whole number, not `{t}`"));
    }
    t.parse().map_err(|_| format!("its {what} {t} is too large"))
}

/// A flag of a field: 1 or 0.
fn flag(what: &str, text: &str) -> Result<bool, String> {
    match text.trim() {
        "1" => Ok(true),
        "0" => Ok(false),
        t => Err(format!("its {what} is 1 or 0, not `{t}`")),
    }
}

impl SharedFile {
    /// A file at `path`, read: refused when it is too large, isn't text or isn't a shared parameters file.
    pub fn read(path: &Path) -> Result<SharedFile, String> {
        let at = path.display();
        let size = std::fs::metadata(path).map_err(|e| format!("{at}: {e}"))?.len();
        if size > MAX_FILE {
            return Err(format!("{at} is too large ({size} bytes; a shared parameters file is at most {MAX_FILE})"));
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path).and_then(|f| f.take(MAX_FILE + 1).read_to_end(&mut bytes)).map_err(|e| format!("{at}: {e}"))?;
        if bytes.len() as u64 > MAX_FILE {
            return Err(format!("{at} is too large (a shared parameters file is at most {MAX_FILE} bytes)"));
        }
        let text = decode(&bytes).map_err(|e| format!("{at}: {e}"))?;
        SharedFile::parse(&text).map_err(|e| format!("{at}: {e}"))
    }

    /// The file's text read; each refusal names its line.
    pub fn parse(text: &str) -> Result<SharedFile, String> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        if text.split('\n').nth(MAX_LINES).is_some() {
            return Err(format!("it has more than {MAX_LINES} lines"));
        }
        let mut file = SharedFile::default();
        let mut meta = false;
        let mut meta_cols: Option<Vec<String>> = None;
        let mut group_cols: Option<Vec<String>> = None;
        let mut param_cols: Option<Vec<String>> = None;
        for (i, line) in text.split('\n').enumerate() {
            let n = i + 1;
            let said = |e: String| format!("line {n}: {e}");
            let line = line.strip_suffix('\r').unwrap_or(line);
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() > MAX_FIELDS {
                return Err(said(format!("more than {MAX_FIELDS} fields")));
            }
            let record = fields.first().copied().unwrap_or_default();
            // The value of column `name` on this line, by the header of its kind.
            let get = |cols: &[String], name: &str| cols.iter().position(|c| c == name).and_then(|k| fields.get(k + 1)).copied();
            let need = |cols: &[String], name: &str| get(cols, name).ok_or_else(|| said(format!("no {name}")));
            match record {
                "*META" | "*GROUP" | "*PARAM" => {
                    let cols: Vec<String> = fields[1..].iter().map(|c| c.trim().to_string()).collect();
                    let (slot, required): (&mut Option<Vec<String>>, &[&str]) = match record {
                        "*META" => (&mut meta_cols, &["VERSION", "MINVERSION"]),
                        "*GROUP" => (&mut group_cols, &["ID", "NAME"]),
                        _ => (&mut param_cols, &["GUID", "NAME", "DATATYPE", "GROUP"]),
                    };
                    if slot.is_some() {
                        return Err(said(format!("a second {record} header")));
                    }
                    if let Some(missing) = required.iter().find(|r| !cols.iter().any(|c| c == *r)) {
                        return Err(said(format!("the {record} header has no {missing} column")));
                    }
                    *slot = Some(cols);
                }
                "META" => {
                    let cols = meta_cols.as_deref().ok_or_else(|| said("META before its *META header".into()))?;
                    if meta {
                        return Err(said("a second META line".into()));
                    }
                    meta = true;
                    file.version = whole("VERSION", need(cols, "VERSION")?).map_err(said)?;
                    file.min_version = whole("MINVERSION", need(cols, "MINVERSION")?).map_err(said)?;
                    if file.min_version > VERSION {
                        return Err(said(format!(
                            "a file of version {}, which a reader of version {} at least reads; BIMCraft reads up to {VERSION}",
                            file.version, file.min_version
                        )));
                    }
                }
                "GROUP" => {
                    let cols = group_cols.as_deref().ok_or_else(|| said("GROUP before its *GROUP header".into()))?;
                    let id = whole("ID", need(cols, "ID")?).map_err(said)?;
                    if id == 0 {
                        return Err(said("a group's ID is 1 or more".into()));
                    }
                    let name = need(cols, "NAME")?.to_string();
                    field_ok("NAME", &name, MAX_TEXT, false).map_err(said)?;
                    if file.groups.iter().any(|g| g.id == id) {
                        return Err(said(format!("a second group {id}")));
                    }
                    if file.groups.iter().any(|g| g.name == name) {
                        return Err(said(format!("a second group named `{name}`")));
                    }
                    file.groups.push(FileGroup { id, name });
                }
                "PARAM" => {
                    let cols = param_cols.as_deref().ok_or_else(|| said("PARAM before its *PARAM header".into()))?;
                    let g = need(cols, "GUID")?.trim();
                    let guid = Guid::parse(g)
                        .filter(|g| g.pending().is_none() && !g.is_nil())
                        .ok_or_else(|| said(format!("its GUID is xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx, not `{g}`")))?;
                    let name = need(cols, "NAME")?.to_string();
                    field_ok("NAME", &name, MAX_TEXT, false).map_err(said)?;
                    let datatype = need(cols, "DATATYPE")?.trim().to_string();
                    field_ok("DATATYPE", &datatype, MAX_TYPE, false).map_err(said)?;
                    let category = get(cols, "DATACATEGORY").unwrap_or_default().trim().to_string();
                    field_ok("DATACATEGORY", &category, MAX_TYPE, true).map_err(said)?;
                    let group = whole("GROUP", need(cols, "GROUP")?).map_err(said)?;
                    let visible = get(cols, "VISIBLE").map_or(Ok(true), |v| flag("VISIBLE", v)).map_err(said)?;
                    let description = get(cols, "DESCRIPTION").unwrap_or_default().to_string();
                    field_ok("DESCRIPTION", &description, MAX_DESCRIPTION, true).map_err(said)?;
                    let modifiable = get(cols, "USERMODIFIABLE").map_or(Ok(true), |v| flag("USERMODIFIABLE", v)).map_err(said)?;
                    let hide = get(cols, "HIDEWHENNOVALUE").map_or(Ok(false), |v| flag("HIDEWHENNOVALUE", v)).map_err(said)?;
                    if let Some(q) = file.parameters.iter().find(|q| q.guid == guid) {
                        return Err(said(format!("its GUID {guid} is `{}`'s, on line {}: one GUID is one parameter", q.name, q.line)));
                    }
                    file.parameters.push(Definition {
                        guid,
                        name,
                        datatype,
                        category,
                        group,
                        visible,
                        description,
                        modifiable,
                        hide_when_no_value: hide,
                        line: n,
                    });
                }
                other => {
                    let shown: String = other.chars().take(40).collect();
                    return Err(said(format!("`{shown}` is none of *META, META, *GROUP, GROUP, *PARAM, PARAM, or a # comment")));
                }
            }
        }
        if !meta {
            return Err("it has no META line: it isn't a shared parameters file".into());
        }
        if let Some(p) = file.parameters.iter().find(|p| !file.groups.iter().any(|g| g.id == p.group)) {
            return Err(format!("line {}: parameter `{}` is in group {}, which the file doesn't have", p.line, p.name, p.group));
        }
        Ok(file)
    }

    /// The file's text, in version 2's layout: lines ended as Windows ends them, the parameters in their groups' order.
    pub fn to_text(&self) -> String {
        let mut out = String::from("# Shared parameters, written by BIMCraft: tab-separated, one header line for each kind of line.\r\n");
        out.push_str(&format!("*META\tVERSION\tMINVERSION\r\nMETA\t{VERSION}\t{MIN_VERSION}\r\n*GROUP\tID\tNAME\r\n"));
        for g in &self.groups {
            out.push_str(&format!("GROUP\t{}\t{}\r\n", g.id, g.name));
        }
        out.push_str("*PARAM\tGUID\tNAME\tDATATYPE\tDATACATEGORY\tGROUP\tVISIBLE\tDESCRIPTION\tUSERMODIFIABLE\tHIDEWHENNOVALUE\r\n");
        let bit = |b: bool| if b { 1 } else { 0 };
        for g in &self.groups {
            for p in self.parameters.iter().filter(|p| p.group == g.id) {
                out.push_str(&format!(
                    "PARAM\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\r\n",
                    p.guid,
                    p.name,
                    p.datatype,
                    p.category,
                    p.group,
                    bit(p.visible),
                    p.description,
                    bit(p.modifiable),
                    bit(p.hide_when_no_value)
                ));
            }
        }
        out
    }

    /// Written to `path`, UTF-8 with a byte order mark; refused when it would be too large to read back.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        let text = self.to_text();
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend(text.as_bytes());
        if bytes.len() as u64 > MAX_FILE {
            return Err(format!("{}: it would be larger than {MAX_FILE} bytes", path.display()));
        }
        std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Its group named `name`.
    pub fn group(&self, name: &str) -> Option<&FileGroup> {
        self.groups.iter().find(|g| g.name == name)
    }

    /// A new group named `name`, numbered after the highest: its number.
    pub fn add_group(&mut self, name: &str) -> Result<u32, String> {
        field_ok("name", name, MAX_TEXT, false).map_err(|e| format!("a group: {e}"))?;
        if self.group(name).is_some() {
            return Err(format!("the file has a group `{name}` already"));
        }
        let id =
            self.groups.iter().map(|g| g.id).max().unwrap_or(0).checked_add(1).ok_or("the file's groups are numbered as far as they go")?;
        self.groups.push(FileGroup { id, name: name.to_string() });
        Ok(id)
    }

    /// A new parameter: a name a family parameter can have, one the file hasn't, a GUID it hasn't, in a group it has.
    pub fn add(&mut self, p: Definition) -> Result<(), String> {
        if !nameable(&p.name) {
            return Err(format!(
                "shared parameter `{}`: a name starts with a letter and holds letters, digits, `_` and spaces, up to {MAX_TEXT}",
                p.name
            ));
        }
        field_ok("description", &p.description, MAX_DESCRIPTION, true).map_err(|e| format!("shared parameter `{}`: {e}", p.name))?;
        if p.guid.pending().is_some() || p.guid.is_nil() {
            return Err(format!("shared parameter `{}`: its GUID is xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx, not {}", p.name, p.guid));
        }
        if let Some(q) = self.parameters.iter().find(|q| q.guid == p.guid) {
            return Err(format!("{}the file has GUID {} already, as `{}`", q.at(), p.guid, q.name));
        }
        if let Some(q) = self.parameters.iter().find(|q| q.name == p.name) {
            return Err(format!("{}the file has a parameter `{}` already, as {}", q.at(), p.name, q.guid));
        }
        if !self.groups.iter().any(|g| g.id == p.group) {
            return Err(format!("the file has no group {}", p.group));
        }
        self.parameters.push(p);
        Ok(())
    }
}
