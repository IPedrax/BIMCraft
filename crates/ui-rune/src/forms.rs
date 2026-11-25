//! The generic command form: a command's params doc (`Command::params`, written for agents: `{path, as_template?}`,
//! `{at: [x, y], angle?: degrees, justify?: top|centre|bottom}`) read as typed fields, and what a person typed in them read back as
//! the JSON the command takes. No widget here: the dialog (`ui/dialog.rs`) draws a [`Form`], and a test needs no window.
//!
//! The doc is prose with a leading `{…}` group, so the reading is a heuristic and never fails: a field whose shape is not
//! recognised is [`Kind::Any`] (a JSON value, or plain text when what is typed is not JSON), a doc with no leading group is a form
//! of one box for the whole parameters object (`Form::rest`), and a `…` item in the group (the doc's "and the rest") turns that box
//! on beside the typed fields. A command the reading gets wrong is still reachable, and the core's own refusal says what to fix.
//! Everything typed is hostile: numbers must be finite and within a cap, lists and texts have caps, JSON is limited in size.

use bimcraft_core::engine::Command;
use bimcraft_core::model::{MAX_COORD, MAX_POINTS};
use serde_json::{Map, Number, Value};

use crate::props;

/// The largest number a plain number field takes (a count, a factor, a length in millimetres, an angle).
pub const MAX_NUMBER: f64 = 1e12;
/// The longest text, in characters, a text field takes.
pub const MAX_TEXT: usize = 10_000;
/// The longest JSON a field or the rest box takes, bytes.
pub const MAX_JSON: usize = 1 << 20;
/// The most ids or texts in a list.
pub const MAX_ITEMS: usize = MAX_POINTS;
/// The longest an id given as a number is, in digits.
const MAX_DIGITS: usize = 15;
/// The most of a command's prose the dialog shows, characters.
const MAX_PROSE: usize = 1_200;

/// What a field holds, and so how it is typed and read back.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// One finite number.
    Number,
    /// Text, as typed.
    Text,
    /// One element's id (digits, a number) or its GUID or name (a string).
    Id,
    /// A list of ids, separated by commas or spaces.
    Ids,
    /// A point of this many coordinates (2 or 3), in millimetres: `x, y`.
    Point(usize),
    /// A list of points of this many coordinates each: `x, y; x, y`.
    Points(usize),
    /// A list of texts, separated by commas or lines.
    Strings,
    /// One of these, chosen from a list (a number where it reads as one, else a string).
    Choice(Vec<String>),
    /// Yes or no, chosen as the text `true` or `false`.
    Bool,
    /// Anything else: JSON when it parses as JSON, else the text.
    Any,
}

impl Kind {
    /// What to say in an empty box (English; the dialog translates it).
    pub fn hint(&self) -> &'static str {
        match self {
            Kind::Number => "number",
            Kind::Text => "",
            Kind::Id => "id",
            Kind::Ids => "id, id, …",
            Kind::Point(2) => "x, y",
            Kind::Point(_) => "x, y, z",
            Kind::Points(2) => "x, y; x, y; …",
            Kind::Points(_) => "x, y, z; x, y, z; …",
            Kind::Strings => "name, name, …",
            Kind::Choice(_) | Kind::Bool => "",
            Kind::Any => "text or JSON",
        }
    }

    /// Whether the field is chosen from a list (a select) rather than typed.
    pub fn chosen(&self) -> bool {
        matches!(self, Kind::Choice(_) | Kind::Bool)
    }
}

/// One parameter of a command.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    /// The doc does not mark it with `?` and names no alternative to it.
    pub required: bool,
    pub kind: Kind,
    /// What the doc says it is when not given (`storeys?: 20`), to start the box from.
    pub default: String,
}

/// A command's form: its fields in the doc's order, whether the rest box shows, and the doc's prose.
#[derive(Clone, Debug, PartialEq)]
pub struct Form {
    pub id: &'static str,
    pub label: &'static str,
    pub fields: Vec<Field>,
    /// A box for more parameters, as a JSON object, merged under the typed fields.
    pub rest: bool,
    /// What the doc says after its leading group, cut to a few lines.
    pub prose: String,
}

impl Form {
    /// The form of a command, read from its params doc.
    pub fn of(cmd: &Command) -> Form {
        let (fields, spread, after) = match lead(cmd.params) {
            Some((inner, after)) => {
                let (fields, spread) = fields_of(inner);
                (fields, spread, after)
            }
            None => (Vec::new(), true, cmd.params),
        };
        // A group with no field in it and no `…` is `{}`: a command with nothing to give. Anything else the reading could not
        // use is one box for the whole object.
        let nothing = lead(cmd.params).is_some_and(|(inner, _)| inner.trim().is_empty());
        let rest = spread || (fields.is_empty() && !nothing);
        Form { id: cmd.id, label: cmd.label, fields, rest, prose: cut(after.trim(), MAX_PROSE) }
    }

    /// The form of the command with this id or alias.
    pub fn named(name: &str) -> Option<Form> {
        bimcraft_core::engine::find(name).map(Form::of)
    }

    /// The JSON the command takes, from the text of each field (a chosen field's choice, empty for none) and the rest box. A blank
    /// optional field is left out; a blank required one, a number that is none, a list of the wrong shape or too long is an error that
    /// names the field. The rest box's keys are overridden by the typed fields.
    pub fn params(&self, texts: &[String], rest: &str) -> Result<Value, String> {
        let mut map = Map::new();
        let extra = rest.trim();
        if !extra.is_empty() {
            if extra.len() > MAX_JSON {
                return Err("more parameters: too long".into());
            }
            match serde_json::from_str::<Value>(extra) {
                Ok(Value::Object(m)) => map = m,
                _ => return Err("more parameters: write a JSON object, like {\"key\": 1}".into()),
            }
        }
        for (i, field) in self.fields.iter().enumerate() {
            let text = texts.get(i).map_or("", String::as_str);
            match value(&field.kind, text).map_err(|why| format!("{}: {why}", field.name))? {
                Some(v) => {
                    map.insert(field.name.clone(), v);
                }
                None if field.required && !map.contains_key(&field.name) => return Err(format!("{} is required", field.name)),
                None => {}
            }
        }
        Ok(Value::Object(map))
    }
}

/// `text` held to `max` characters, ending in `…` when cut.
pub fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut short: String = text.chars().take(max.saturating_sub(1)).collect();
    short.push('…');
    short
}

/// The inside of the doc's leading `{…}` and what follows it, or none when the doc does not start with a brace. Braces in quotes
/// do not count (`"{3D}"`). A group that never closes runs to the end.
fn lead(doc: &str) -> Option<(&str, &str)> {
    let inner = doc.trim_start().strip_prefix('{')?;
    let (mut depth, mut quoted) = (1usize, false);
    for (i, c) in inner.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '{' if !quoted => depth += 1,
            '}' if !quoted => {
                depth -= 1;
                if depth == 0 {
                    return Some((&inner[..i], &inner[i + 1..]));
                }
            }
            _ => {}
        }
    }
    Some((inner, ""))
}

/// `s` split at each `sep` outside quotes and outside every bracket.
fn split_top(s: &str, sep: char) -> Vec<&str> {
    let (mut parts, mut start, mut depth, mut quoted) = (Vec::new(), 0, 0usize, false);
    for (i, c) in s.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '{' | '[' | '(' if !quoted => depth += 1,
            '}' | ']' | ')' if !quoted => depth = depth.saturating_sub(1),
            c if c == sep && depth == 0 && !quoted => {
                parts.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// `s` cut at its first top-level `sep`: what is before it and what is after.
fn split_once_top(s: &str, sep: char) -> Option<(&str, &str)> {
    let first = *split_top(s, sep).first()?;
    (first.len() < s.len()).then(|| (first, &s[first.len() + sep.len_utf8()..]))
}

/// The inside of the leading `[…]` of `s` and what follows its closing bracket.
fn bracket(s: &str) -> Option<(&str, &str)> {
    let inner = s.trim_start().strip_prefix('[')?;
    let (mut depth, mut quoted) = (1usize, false);
    for (i, c) in inner.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '[' if !quoted => depth += 1,
            ']' if !quoted => {
                depth -= 1;
                if depth == 0 {
                    return Some((&inner[..i], &inner[i + 1..]));
                }
            }
            _ => {}
        }
    }
    Some((inner, ""))
}

fn is_name(n: &str) -> bool {
    n.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The fields of the group's inside, and whether it says "and the rest" (a `…` item, or an item that is no field's name).
fn fields_of(inner: &str) -> (Vec<Field>, bool) {
    let (mut fields, mut spread) = (Vec::<Field>::new(), false);
    for raw in split_top(inner, ',') {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        if raw.starts_with('…') || raw.starts_with("..") {
            spread = true;
            continue;
        }
        let (names, spec) = split_once_top(raw, ':').map_or((raw, ""), |(n, s)| (n.trim(), s.trim()));
        let alternatives: Vec<&str> = names.split('|').map(|n| n.trim().trim_end_matches('?').trim()).collect();
        if !alternatives.iter().all(|n| is_name(n)) {
            spread = true;
            continue;
        }
        let required = alternatives.len() == 1 && !names.ends_with('?');
        for (j, name) in alternatives.iter().enumerate() {
            if fields.iter().any(|f| f.name == *name) {
                continue;
            }
            // The spec is the last alternative's (`id | ids: [id, …]` is about the ids).
            let (kind, default) =
                if spec.is_empty() || j + 1 < alternatives.len() { (by_name(name), String::new()) } else { classify(spec, name) };
            fields.push(Field { name: (*name).to_string(), required, kind, default });
        }
    }
    (fields, spread)
}

/// What a field of this name is when the doc says no more.
fn by_name(name: &str) -> Kind {
    match name {
        "id" => Kind::Id,
        "ids" => Kind::Ids,
        "at" | "about" | "toward" | "by" | "start" | "end" => Kind::Point(2),
        "points" | "boundary" => Kind::Points(2),
        "closed" | "copy" | "dry_run" | "compress" | "overwrite" | "mirrored" | "absolute" | "delete" | "reset" | "clear" => Kind::Bool,
        "name" | "rename" | "like" | "label" | "title" | "text" | "note" | "mark" | "prefix" | "suffix" | "find" | "comment" => Kind::Text,
        "height" | "width" | "thickness" | "depth" | "elevation" | "offset" | "end_offset" | "base_offset" | "top_offset" | "angle"
        | "distance" | "radius" | "factor" | "diameter" | "spacing" | "count" | "step" | "storeys" | "fall" | "slope" => Kind::Number,
        _ if name.ends_with("_id") => Kind::Id,
        _ => Kind::Any,
    }
}

/// An axis or an offset along one: a coordinate's name in a doc's `[x, y]`.
fn is_axis(s: &str) -> bool {
    matches!(s, "x" | "y" | "z" | "dx" | "dy" | "dz" | "de" | "dn" | "r" | "g" | "b")
}

fn is_ident(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// A number, or two joined by `..`, a hyphen or an en dash (`1..100`, `0-100`).
fn is_range(p: &str) -> bool {
    if p.parse::<f64>().is_ok_and(f64::is_finite) {
        return true;
    }
    let skip = p.chars().next().map_or(0, char::len_utf8);
    ["..", "\u{2013}", "-"].iter().any(|sep| {
        p[skip..].find(sep).is_some_and(|i| {
            let (a, b) = (&p[..i + skip], &p[i + skip + sep.len()..]);
            a.parse::<f64>().is_ok() && b.parse::<f64>().is_ok()
        })
    })
}

/// What a doc's `[…]` holds.
fn list_kind(inner: &str) -> Kind {
    let items: Vec<&str> = split_top(inner, ',').into_iter().map(str::trim).collect();
    let first = items.first().copied().unwrap_or_default();
    if first.starts_with('[') {
        // `[[x,y],…]`: points. Three brackets deep is a line, or a list of them: JSON.
        let Some((point, _)) = bracket(first) else { return Kind::Any };
        let axes = split_top(point, ',').len();
        return if point.trim_start().starts_with('[') || !(2..=3).contains(&axes) { Kind::Any } else { Kind::Points(axes) };
    }
    if (2..=3).contains(&items.len()) && items.iter().all(|i| is_axis(i)) {
        return Kind::Point(items.len());
    }
    if first.split_whitespace().next() == Some("id") {
        return Kind::Ids;
    }
    if !first.is_empty() && first.split('|').all(|p| is_ident(p.trim())) { Kind::Strings } else { Kind::Any }
}

/// A choice's piece as its value: no spaces, quotes taken off, nothing that says "and so on".
fn token(piece: &str) -> Option<(String, bool)> {
    let piece = piece.split(" (").next().unwrap_or_default().trim();
    let (word, quoted) = match piece.strip_prefix('"').and_then(|p| p.strip_suffix('"')) {
        Some(inner) => (inner, true),
        None => (piece, false),
    };
    let fine = !word.is_empty() && !word.contains(char::is_whitespace) && !word.contains('…') && !word.contains("..");
    fine.then(|| (word.to_string(), quoted))
}

/// What the doc's spec after the colon says a field is, and the default it names.
fn classify(spec: &str, name: &str) -> (Kind, String) {
    let spec = spec.trim();
    if spec.starts_with('[') {
        return match bracket(spec) {
            Some((inner, after)) if after.trim().is_empty() => (list_kind(inner), String::new()),
            _ => (Kind::Any, String::new()),
        };
    }
    if spec.starts_with('{') {
        return (Kind::Any, String::new());
    }
    let pieces = split_top(spec, '|');
    if pieces.len() > 1 {
        let tokens: Option<Vec<(String, bool)>> = pieces.iter().map(|p| token(p)).collect();
        let Some(tokens) = tokens else { return (Kind::Any, String::new()) };
        let quoted = tokens.iter().filter(|(_, q)| *q).count();
        let words: Vec<String> = tokens.into_iter().map(|(w, _)| w).collect();
        let typed = |w: &str| {
            matches!(w.to_lowercase().as_str(), "id" | "guid" | "name" | "number" | "text" | "string" | "ref" | "value" | "null" | "view")
        };
        if (quoted > 0 && quoted < words.len()) || words.iter().any(|w| typed(w.as_str())) {
            return (Kind::Any, String::new());
        }
        if words.len() == 2 && words.contains(&"true".to_string()) && words.contains(&"false".to_string()) {
            return (Kind::Bool, String::new());
        }
        return (Kind::Choice(words), String::new());
    }
    let single = token(spec).map(|(w, _)| w).unwrap_or_default();
    match single.as_str() {
        "true" | "false" => (Kind::Bool, String::new()),
        "bool" => (Kind::Bool, String::new()),
        "string" => (Kind::Text, String::new()),
        "degrees" | "mm" | "mm²" | "px" | "number" => (Kind::Number, String::new()),
        s if s.parse::<f64>().is_ok_and(f64::is_finite) => (Kind::Number, s.to_string()),
        s if !s.is_empty() && is_range(s) => (Kind::Number, String::new()),
        _ => (by_name(name), String::new()),
    }
}

/// A finite number within `cap`.
fn num(t: &str, cap: f64) -> Result<f64, String> {
    let n: f64 = t.parse().map_err(|_| format!("{t:?} is not a number"))?;
    if !n.is_finite() {
        return Err(format!("{t:?} is not a number"));
    }
    if n.abs() > cap {
        return Err(format!("{t:?} is too large"));
    }
    Ok(n)
}

fn json_number(n: f64) -> Result<Value, String> {
    if n.fract() == 0.0 && n.abs() < 9e15 {
        return Ok(Value::from(n as i64));
    }
    Number::from_f64(n).map(Value::Number).ok_or_else(|| format!("{n} is not a number"))
}

/// The numbers in `t`, however they are separated or bracketed (`1, 2`, `[1,2]`, `1 2; 3 4`).
fn numbers(t: &str) -> Result<Vec<f64>, String> {
    let spaced: String = t.chars().map(|c| if matches!(c, ',' | ';' | '[' | ']' | '(' | ')') { ' ' } else { c }).collect();
    let mut all = Vec::new();
    for word in spaced.split_whitespace() {
        if all.len() >= MAX_ITEMS * 3 {
            return Err("too many numbers".into());
        }
        all.push(num(word, MAX_COORD)?);
    }
    Ok(all)
}

/// An id as the core reads one: digits are the number, anything else (a GUID, a name) the string.
fn id(t: &str) -> Result<Value, String> {
    if t.chars().count() > 100 {
        return Err("too long".into());
    }
    if t.len() <= MAX_DIGITS && t.chars().all(|c| c.is_ascii_digit()) {
        return Ok(t.parse::<u64>().map_or_else(|_| Value::String(t.to_string()), Value::from));
    }
    Ok(Value::String(t.to_string()))
}

/// What was typed in a field of this kind, as its JSON value; none for a blank.
pub fn value(kind: &Kind, typed: &str) -> Result<Option<Value>, String> {
    let t = typed.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let v = match kind {
        Kind::Number => json_number(num(t, MAX_NUMBER)?)?,
        Kind::Text => {
            if t.chars().count() > MAX_TEXT {
                return Err(format!("longer than {MAX_TEXT} characters"));
            }
            Value::String(t.to_string())
        }
        Kind::Id => id(t)?,
        Kind::Ids => {
            let ids: Vec<&str> = t.split(|c: char| c == ',' || c == ';' || c.is_whitespace()).filter(|p| !p.is_empty()).collect();
            if ids.len() > MAX_ITEMS {
                return Err("too many".into());
            }
            Value::Array(ids.into_iter().map(id).collect::<Result<_, _>>()?)
        }
        Kind::Point(n) => {
            let nums = numbers(t)?;
            if nums.len() != *n {
                return Err(format!("needs {n} numbers, like {}", Kind::Point(*n).hint()));
            }
            Value::Array(nums.into_iter().map(json_number).collect::<Result<_, _>>()?)
        }
        Kind::Points(n) => {
            let nums = numbers(t)?;
            if nums.is_empty() || !nums.len().is_multiple_of(*n) {
                return Err(format!("needs groups of {n} numbers, like {}", Kind::Points(*n).hint()));
            }
            if nums.len() / n > MAX_POINTS {
                return Err(format!("more than {MAX_POINTS} points"));
            }
            let points: Result<Vec<Value>, String> =
                nums.chunks(*n).map(|c| c.iter().map(|x| json_number(*x)).collect::<Result<Vec<_>, _>>().map(Value::Array)).collect();
            Value::Array(points?)
        }
        Kind::Strings => {
            let items: Vec<&str> = t.split([',', '\n']).map(str::trim).filter(|p| !p.is_empty()).collect();
            if items.len() > MAX_ITEMS || items.iter().any(|i| i.chars().count() > MAX_TEXT) {
                return Err("too many or too long".into());
            }
            Value::Array(items.into_iter().map(|i| Value::String(i.to_string())).collect())
        }
        Kind::Choice(options) => {
            if !options.iter().any(|o| o == t) {
                return Err(format!("{t:?} is not one of {}", options.join(", ")));
            }
            t.parse::<i64>().map_or_else(|_| Value::String(t.to_string()), Value::from)
        }
        Kind::Bool => props::parse(props::Kind::Bool, t)?,
        Kind::Any => {
            if t.len() > MAX_JSON {
                return Err("too long".into());
            }
            serde_json::from_str::<Value>(t).unwrap_or_else(|_| Value::String(t.to_string()))
        }
    };
    Ok(Some(v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn form(doc: &'static str) -> Form {
        fn nothing(_: &mut bimcraft_core::engine::Session, _: &Value) -> Result<Value, String> {
            Ok(Value::Null)
        }
        let cmd = Command {
            id: "probe",
            label: "Probe",
            aliases: &[],
            params: doc,
            run: nothing,
            undoable: true,
            menu: None,
            files: bimcraft_core::engine::Files::None,
        };
        Form::of(&cmd)
    }

    fn kinds(f: &Form) -> Vec<(&str, bool, &Kind)> {
        f.fields.iter().map(|f| (f.name.as_str(), f.required, &f.kind)).collect()
    }

    #[test]
    fn a_params_doc_is_read_as_typed_fields() {
        let f = form("{path, as_template?} a project");
        assert_eq!(kinds(&f), [("path", true, &Kind::Any), ("as_template", false, &Kind::Any)]);
        assert_eq!(f.prose, "a project");
        assert!(!f.rest);

        let f = form("{at: [x,y], angle?: degrees, justify?: top|centre|bottom, closed?, storeys?: 20, fall: 1.5}");
        assert_eq!(
            kinds(&f),
            [
                ("at", true, &Kind::Point(2)),
                ("angle", false, &Kind::Number),
                ("justify", false, &Kind::Choice(vec!["top".into(), "centre".into(), "bottom".into()])),
                ("closed", false, &Kind::Bool),
                ("storeys", false, &Kind::Number),
                ("fall", true, &Kind::Number),
            ]
        );
        assert_eq!(f.fields[4].default, "20");
        assert_eq!(f.fields[5].default, "1.5");
    }

    #[test]
    fn lists_points_ids_and_alternatives_have_their_own_kinds() {
        let f = form("{points: [[x,y],…], at?: [x, y, z], ids: [id,…], names?: [name, …], line: [[x,y],[x,y]], rows: [{a}], ids | id}");
        let k = kinds(&f);
        assert_eq!(k[0], ("points", true, &Kind::Points(2)));
        assert_eq!(k[1], ("at", false, &Kind::Point(3)));
        assert_eq!(k[2], ("ids", true, &Kind::Ids));
        assert_eq!(k[3], ("names", false, &Kind::Strings));
        assert_eq!(k[4], ("line", true, &Kind::Points(2)));
        assert_eq!(k[5], ("rows", true, &Kind::Any), "a list of objects is JSON");
        assert_eq!(f.fields.len(), 7, "`ids` was already there, so the alternative adds only `id`");
        assert!(k[2].1, "and an alternative that came second does not make the first optional");

        let f = form("{id | ids: [id,…], …fields}");
        assert_eq!(kinds(&f), [("id", false, &Kind::Id), ("ids", false, &Kind::Ids)], "an alternative is not required");
        assert!(f.rest, "`…fields` is the rest box");

        let f = form("{kinds?: [type|family|material, …], widths: [16 widths]}");
        assert_eq!(f.fields[0].kind, Kind::Strings);
        assert_eq!(f.fields[1].kind, Kind::Any);
    }

    #[test]
    fn choices_are_told_from_prose_and_from_a_type_in_words() {
        let f = form(
            "{a?: on|off, b?: true | false (true unless given), c?: 1|2|3, d: id|guid, e?: name | \"Solid\", f?: view | null, g?: 1 | … | 100, h?: \"{3D}\" | a 3D view's name or id}",
        );
        let k = kinds(&f);
        assert_eq!(k[0].2, &Kind::Choice(vec!["on".into(), "off".into()]));
        assert_eq!(k[1].2, &Kind::Bool);
        assert_eq!(k[2].2, &Kind::Choice(vec!["1".into(), "2".into(), "3".into()]));
        assert_eq!(k[3].2, &Kind::Any, "id|guid says what the value is, not what to choose");
        assert_eq!(k[4].2, &Kind::Any, "a quoted word beside a bare one is prose");
        assert_eq!(k[5].2, &Kind::Any, "null is a way to clear, typed as JSON");
        assert_eq!(k[6].2, &Kind::Any, "a range written with … is not a list of choices");
        assert_eq!(k[7].2, &Kind::Any);
        assert_eq!(f.fields.len(), 8, "braces in quotes do not end the group early");
    }

    #[test]
    fn a_doc_that_cannot_be_read_is_one_box_for_the_whole_object() {
        for doc in ["Manage > Link Model", "no group at all", "{a curve: `points` [[x, y], …], and prose}", ""] {
            let f = form(doc);
            assert!(f.rest, "{doc:?}");
        }
        let f = form("{}");
        assert!(f.fields.is_empty() && !f.rest, "`{{}}` is a command with nothing to give");
        assert_eq!(form("{a, b").fields.len(), 2, "a group that never closes still reads");
    }

    #[test]
    fn what_is_typed_is_read_back_by_kind() {
        let f = form("{id, at: [x,y], points?: [[x,y],…], ids?: [id,…], n?: 20, on?: true|false, how?: a|b, tag?: string, any?}");
        let typed = |texts: &[&str]| f.params(&texts.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "");
        assert_eq!(
            typed(&["100234", "1500.5, -20", "0,0; 3000,0; 3000 3000", "100001, 100002", "7", "true", "a", "x", "[1,2]"]).unwrap(),
            json!({"id": 100234, "at": [1500.5, -20], "points": [[0, 0], [3000, 0], [3000, 3000]], "ids": [100001, 100002], "n": 7, "on": true,
                   "how": "a", "tag": "x", "any": [1, 2]})
        );
        assert_eq!(typed(&["a1b2c3d4-0000-4000-8000-000000000000", "1 2"]).unwrap()["id"], json!("a1b2c3d4-0000-4000-8000-000000000000"));
        assert_eq!(typed(&["1", "(1, 2)"]).unwrap()["at"], json!([1, 2]));
        assert_eq!(typed(&["", "1,2"]), Err("id is required".into()));
        assert_eq!(typed(&["1", ""]), Err("at is required".into()));
        assert!(typed(&["1", "1,2,3"]).unwrap_err().starts_with("at: needs 2 numbers"));
        assert!(typed(&["1", "1,2", "1,2,3"]).unwrap_err().starts_with("points: needs groups of 2"));
        assert!(typed(&["1", "1,2", "", "", "abc"]).unwrap_err().starts_with("n: "));
        assert!(typed(&["1", "1,2", "", "", "", "", "c"]).unwrap_err().contains("not one of a, b"));
    }

    #[test]
    fn the_rest_box_is_merged_under_the_typed_fields() {
        let f = form("{id, name?, …fields}");
        let texts = vec!["5".to_string(), "Hall".to_string()];
        assert_eq!(f.params(&texts, r#"{"height": 2700, "name": "old"}"#).unwrap(), json!({"id": 5, "name": "Hall", "height": 2700}));
        assert_eq!(
            f.params(&["".to_string(), "".to_string()], r#"{"id": 9}"#).unwrap(),
            json!({"id": 9}),
            "a required field the box gives is given"
        );
        for bad in ["[1]", "3", "{", "text"] {
            assert!(f.params(&texts, bad).unwrap_err().starts_with("more parameters"), "{bad}");
        }
    }

    #[test]
    fn hostile_text_is_refused_or_held_to_its_caps() {
        let n = |t: &str| value(&Kind::Number, t);
        for bad in ["NaN", "inf", "-inf", "1e999", "1e13", "x", "--1", "1,5"] {
            assert!(n(bad).is_err(), "{bad:?}");
        }
        assert_eq!(n("   "), Ok(None));
        assert_eq!(n("2.5"), Ok(Some(json!(2.5))));
        assert_eq!(n("-3"), Ok(Some(json!(-3))));
        assert!(value(&Kind::Point(2), "1e10, 2").is_err(), "a coordinate is held to MAX_COORD");
        assert!(value(&Kind::Text, &"x".repeat(MAX_TEXT + 1)).is_err());
        assert!(value(&Kind::Any, &"[".repeat(MAX_JSON + 1)).is_err());
        assert!(value(&Kind::Any, &"[".repeat(5_000)).is_ok(), "deeply nested JSON is only text, never a crash");
        let many = vec!["1"; MAX_ITEMS + 1].join(",");
        assert!(value(&Kind::Ids, &many).is_err());
        assert!(value(&Kind::Strings, &many).is_err());
        let pts = vec!["1,2"; MAX_POINTS + 1].join(";");
        assert!(value(&Kind::Points(2), &pts).is_err());
        assert!(value(&Kind::Id, &"9".repeat(200)).is_err());
        assert_eq!(
            value(&Kind::Id, "12345678901234567890"),
            Ok(Some(json!("12345678901234567890"))),
            "too many digits for a number: kept as text"
        );
        assert_eq!(value(&Kind::Choice(vec!["1".into(), "4-40".into()]), "1"), Ok(Some(json!(1))));
        assert_eq!(value(&Kind::Choice(vec!["1".into(), "4-40".into()]), "4-40"), Ok(Some(json!("4-40"))));
        assert!(value(&Kind::Bool, "maybe").is_err());
        assert_eq!(value(&Kind::Bool, "false"), Ok(Some(json!(false))));
    }

    /// What a person might type in a field of this kind so that it reads back.
    fn sample(kind: &Kind) -> String {
        match kind {
            Kind::Number => "1".into(),
            Kind::Text => "x".into(),
            Kind::Id => "100001".into(),
            Kind::Ids => "100001, 100002".into(),
            Kind::Point(2) => "1, 2".into(),
            Kind::Point(_) => "1, 2, 3".into(),
            Kind::Points(2) => "1, 2; 3, 4".into(),
            Kind::Points(_) => "1, 2, 3; 4, 5, 6".into(),
            Kind::Strings => "a, b".into(),
            Kind::Choice(o) => o.first().cloned().unwrap_or_default(),
            Kind::Bool => "true".into(),
            Kind::Any => "1".into(),
        }
    }

    #[test]
    fn every_command_has_a_form_that_reads_back_and_never_panics() {
        let all = bimcraft_core::engine::commands();
        assert!(all.len() > 250, "the table is the core's own: {} commands", all.len());
        let mut boxed = 0;
        for cmd in all {
            let f = Form::of(cmd);
            assert_eq!(f.id, cmd.id);
            let empty_group = cmd.params.trim_start().strip_prefix('{').is_some_and(|r| r.trim_start().starts_with('}'));
            assert!(!f.fields.is_empty() || f.rest || empty_group, "{} has no way to give its parameters: {:?}", cmd.id, cmd.params);
            let mut names: Vec<&str> = f.fields.iter().map(|f| f.name.as_str()).collect();
            let n = names.len();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), n, "{}: a name twice", cmd.id);
            assert!(f.prose.chars().count() <= MAX_PROSE);
            if f.fields.is_empty() && f.rest {
                boxed += 1;
            }
            // Filled with what each kind reads, the form gives an object holding every field; left blank it names the first field
            // it needs, or gives an empty object. Neither panics.
            let full: Vec<String> = f.fields.iter().map(|f| sample(&f.kind)).collect();
            let object = f.params(&full, "").unwrap_or_else(|why| panic!("{}: {why}", cmd.id));
            assert_eq!(object.as_object().map(Map::len), Some(f.fields.len()), "{}", cmd.id);
            let blank = vec![String::new(); f.fields.len()];
            match f.params(&blank, "") {
                Ok(v) => assert!(f.fields.iter().all(|f| !f.required) && v.as_object().is_some_and(Map::is_empty), "{}", cmd.id),
                Err(why) => assert!(f.fields.iter().any(|f| f.required && why.starts_with(f.name.as_str())), "{}: {why}", cmd.id),
            }
            // Hostile text in every field of every kind: an error or a value, never a panic.
            let big = "9".repeat(5_000);
            for hostile in ["NaN", "1e999", "\u{0}", "é\u{301}", "[[[[", "\"", "{\"a\":", big.as_str()] {
                let texts = vec![hostile.to_string(); f.fields.len()];
                let _ = f.params(&texts, hostile);
            }
        }
        assert!(boxed < all.len() / 4, "{boxed} of {} commands are only a box: the reading is failing", all.len());
    }

    #[test]
    fn commands_found_by_id_or_shortcut_have_the_forms_a_person_expects() {
        let wall = Form::named("modify").expect("modify is a command");
        assert!(wall.rest, "modify takes `…fields`");
        assert_eq!(wall.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["id", "ids"]);
        let tower = Form::named("tower").expect("tower");
        assert_eq!(tower.fields.iter().find(|f| f.name == "storeys").map(|f| &f.kind), Some(&Kind::Number));
        assert_eq!(
            tower.fields.iter().find(|f| f.name == "plate").map(|f| &f.kind),
            Some(&Kind::Choice(vec!["reference".into(), "small".into()]))
        );
        assert!(Form::named("no such command").is_none());
    }
}
