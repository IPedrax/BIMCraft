//! Building codes as XML, to hand from one project or office to another.
//!
//! Format 2: one `<building-code>` for one discipline, or several in a `<building-codes>`. Its documents, then a
//! `<section>` for each section, a `<rule>` for each rule: a number, a yes or no or a choice in `value`, anything else (a
//! table, a list of rooms) as JSON in `json`; each with its `source` (or a document cited by `source-doc`, `source-item`,
//! `source-page`) and its `kind`. Format 1, what #386 wrote, is a stairs-only code: its `<stairs>` is read as the
//! architectural section `stairs`. Read kindly: what this version doesn't know is listed, never refused.

use std::collections::BTreeMap;

use serde_json::Value;

use super::{BuildingCode, Citation, Discipline, Document, Kind, Source, section_def};

fn esc(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\n', "&#10;")
}

fn unesc(t: &str) -> String {
    t.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&#10;", "\n").replace("&amp;", "&")
}

/// A source as attributes.
fn source_attrs(s: Option<&Source>) -> String {
    match s {
        None => String::new(),
        Some(Source::Text(t)) => format!(" source=\"{}\"", esc(t)),
        Some(Source::Cited(c)) => {
            let mut out = format!(" source-doc=\"{}\"", esc(&c.doc));
            for (k, v) in [("source-item", &c.item), ("source-page", &c.page), ("source-note", &c.note)] {
                if !v.is_empty() {
                    out.push_str(&format!(" {k}=\"{}\"", esc(v)));
                }
            }
            out
        }
    }
}

fn kind_attr(k: Option<&Kind>) -> String {
    match k.and_then(|k| serde_json::to_value(k).ok()) {
        Some(Value::String(t)) => format!(" kind=\"{t}\""),
        _ => String::new(),
    }
}

/// `codes` as one file: a `<building-code>`, or a `<building-codes>` of several.
pub(super) fn write(codes: &[BuildingCode]) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let several = codes.len() != 1;
    if several {
        out.push_str("<building-codes format=\"2\">\n");
    }
    for c in codes {
        out.push_str(&format!("<building-code format=\"2\" discipline=\"{}\" name=\"{}\"", c.discipline.key(), esc(&c.name)));
        for (k, v) in [("region", &c.region), ("edition", &c.edition), ("based-on", &c.based_on), ("amends", &c.amends)] {
            if !v.is_empty() {
                out.push_str(&format!(" {k}=\"{}\"", esc(v)));
            }
        }
        out.push_str(">\n");
        if !c.note.is_empty() {
            out.push_str(&format!("  <note>{}</note>\n", esc(&c.note)));
        }
        for r in &c.references {
            out.push_str(&format!("  <reference>{}</reference>\n", esc(r)));
        }
        if !c.checked.is_empty() {
            out.push_str(&format!("  <checked>{}</checked>\n", esc(&c.checked)));
        }
        for (id, d) in &c.documents {
            out.push_str(&format!("  <document id=\"{}\" title=\"{}\"", esc(id), esc(&d.title)));
            for (k, v) in [("edition", &d.edition), ("publisher", &d.publisher), ("url", &d.url), ("accessed", &d.accessed)] {
                if !v.is_empty() {
                    out.push_str(&format!(" {k}=\"{}\"", esc(v)));
                }
            }
            out.push_str("/>\n");
        }
        out.push_str("  <!-- Lengths in millimetres. A rule left out is read as the code's it is based on. -->\n");
        for (key, rules) in &c.sections {
            let Some(rules) = rules.as_object() else { continue };
            out.push_str(&format!(
                "  <section name=\"{}\"{}{}>\n",
                esc(key),
                source_attrs(c.sources.get(key)),
                kind_attr(c.kinds.get(key))
            ));
            for (rule, v) in rules {
                let at = format!("{key}.{rule}");
                let said = match v {
                    // A text that would read back as a number or a truth ("7", "true") is written as JSON, so it comes back a text.
                    Value::String(t) if value_of(t) == *v => format!("value=\"{}\"", esc(t)),
                    Value::Number(_) | Value::Bool(_) => format!("value=\"{v}\""),
                    other => format!("json=\"{}\"", esc(&other.to_string())),
                };
                out.push_str(&format!(
                    "    <rule name=\"{}\" {said}{}{}/>\n",
                    esc(rule),
                    source_attrs(c.sources.get(&at)),
                    kind_attr(c.kinds.get(&at))
                ));
            }
            out.push_str("  </section>\n");
        }
        out.push_str("</building-code>\n");
    }
    if several {
        out.push_str("</building-codes>\n");
    }
    out
}

/// A value written in `value`: a yes or no, a whole number, a number, else a text.
fn value_of(t: &str) -> Value {
    match t.trim() {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        t => {
            if !t.contains(['.', 'e', 'E'])
                && let Ok(n) = t.parse::<i64>()
            {
                return Value::from(n);
            }
            match t.parse::<f64>() {
                Ok(x) if x.is_finite() => serde_json::json!(x),
                _ => Value::String(t.to_string()),
            }
        }
    }
}

/// The rules a section of discipline `d` may have, where BIMCraft knows them: the keys of the library's own section.
/// None for a section of rules for reference, which takes any.
fn known(d: Discipline, section: &str) -> Option<Vec<String>> {
    let def = section_def(d, section)?;
    // The electrical settings are named by the user (a voltage, a system, a wire type): any name is one of their rules.
    if def.read_by.is_empty() || (d == Discipline::Electrical && crate::electrical::SECTIONS.contains(&section)) {
        return None;
    }
    // Every code that holds the section, not only the first: a utility's `unit_demand` has parts another's hasn't.
    let mut keys: Vec<String> = Vec::new();
    for lib in super::of(d).filter(|c| c.sections.contains_key(section)) {
        // As the section types it: every rule it has, those with a default too.
        let mine: Vec<String> = match (def.read)(&lib.sections[section]) {
            Ok(Value::Object(o)) => o.keys().cloned().collect(),
            _ => lib.section(section).map(|s| s.keys().cloned().collect()).unwrap_or_default(),
        };
        for k in mine {
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
    }
    (!keys.is_empty()).then_some(keys)
}

/// The codes the file holds and what it holds that this version doesn't know.
pub(super) fn read(text: &str) -> Result<(Vec<BuildingCode>, Vec<String>), String> {
    let mut codes: Vec<BuildingCode> = Vec::new();
    let mut left_out = Vec::new();
    let leave = |what: String, left: &mut Vec<String>| {
        if left.len() < 100 {
            left.push(what);
        }
    };
    // The code being read, and the section of it.
    let mut open: Option<BuildingCode> = None;
    let mut section: Option<String> = None;
    // Inside a section this version doesn't know: its rules go with it, said once.
    let mut skipping = false;
    let mut rest = text;
    while let Some(at) = rest.find('<') {
        let after = rest.get(at + 1..).unwrap_or("");
        // A declaration or a comment: past it.
        let skip = if after.starts_with("!--") {
            Some("-->")
        } else if after.starts_with('?') {
            Some("?>")
        } else {
            None
        };
        if let Some(end) = skip {
            rest = after.find(end).and_then(|i| after.get(i + end.len()..)).unwrap_or("");
            continue;
        }
        let Some(close) = after.find('>') else { return Err("a tag of the file never closes".into()) };
        let tag = after.get(..close).unwrap_or("").trim_end_matches('/').trim();
        rest = after.get(close + 1..).unwrap_or("");
        if let Some(name) = tag.strip_prefix('/') {
            match name.trim() {
                "building-code" => {
                    if let Some(c) = open.take() {
                        codes.push(c);
                    }
                    section = None;
                }
                "section" | "stairs" => {
                    section = None;
                    skipping = false;
                }
                _ => {}
            }
            continue;
        }
        let (name, attrs) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
        // name="value" pairs, in either quote.
        let mut pairs: Vec<(String, String)> = Vec::new();
        let mut a = attrs.trim_start();
        while let Some((key, value)) = a.split_once('=') {
            let value = value.trim_start();
            let Some(q) = value.chars().next().filter(|q| *q == '"' || *q == '\'') else { break };
            let Some(end) = value.get(1..).and_then(|v| v.find(q)) else { break };
            pairs.push((key.trim().to_string(), unesc(value.get(1..=end).unwrap_or(""))));
            a = value.get(end + 2..).unwrap_or("").trim_start();
            if pairs.len() > 64 {
                break;
            }
        }
        let attr = |key: &str| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        let source = || match (attr("source"), attr("source-doc")) {
            (_, Some(doc)) if !doc.trim().is_empty() => Some(Source::Cited(Citation {
                doc,
                item: attr("source-item").unwrap_or_default(),
                page: attr("source-page").unwrap_or_default(),
                note: attr("source-note").unwrap_or_default(),
            })),
            (Some(t), _) if !t.trim().is_empty() => Some(Source::Text(t.trim().to_string())),
            _ => None,
        };
        let kind = || attr("kind").and_then(|k| serde_json::from_value::<Kind>(Value::String(k)).ok());
        let inner = || unesc(rest.split('<').next().unwrap_or("").trim());
        match (name, open.as_mut()) {
            ("building-codes", _) => {}
            ("building-code", _) => {
                if let Some(c) = open.take() {
                    codes.push(c);
                }
                let discipline = match attr("discipline") {
                    None => Some(Discipline::Architectural),
                    Some(d) => Discipline::parse(&d),
                };
                let Some(discipline) = discipline else {
                    leave(format!("a code of the discipline `{}`", attr("discipline").unwrap_or_default()), &mut left_out);
                    continue;
                };
                if codes.len() >= super::MAX_OWN {
                    return Err(format!("the file holds more than {} building codes", super::MAX_OWN));
                }
                open = Some(BuildingCode {
                    discipline,
                    name: attr("name").unwrap_or_default(),
                    region: attr("region").unwrap_or_default(),
                    edition: attr("edition").unwrap_or_default(),
                    based_on: attr("based-on").unwrap_or_default(),
                    amends: attr("amends").unwrap_or_default(),
                    ..BuildingCode::default()
                });
                section = None;
            }
            (_, None) => leave(format!("<{}>", name.chars().take(40).collect::<String>()), &mut left_out),
            ("note", Some(c)) => c.note = inner(),
            ("checked", Some(c)) => c.checked = inner(),
            ("reference", Some(c)) if c.references.len() < 50 => c.references.push(inner()),
            ("document", Some(c)) if c.documents.len() < 200 => {
                let id = attr("id").unwrap_or_default();
                let d = Document {
                    title: attr("title").unwrap_or_default(),
                    edition: attr("edition").unwrap_or_default(),
                    publisher: attr("publisher").unwrap_or_default(),
                    url: attr("url").unwrap_or_default(),
                    accessed: attr("accessed").unwrap_or_default(),
                };
                if !id.trim().is_empty() {
                    c.documents.insert(id, d);
                }
            }
            // Format 1: the stairs' rules, the only section it had.
            ("stairs", Some(c)) => {
                section = Some("stairs".into());
                c.sections.entry("stairs".into()).or_insert_with(|| Value::Object(Default::default()));
            }
            ("section", Some(c)) => {
                let key = attr("name").unwrap_or_default();
                if section_def(c.discipline, &key).is_none() {
                    leave(format!("section `{key}`"), &mut left_out);
                    section = None;
                    skipping = true;
                    continue;
                }
                if let Some(s) = source() {
                    c.sources.insert(key.clone(), s);
                }
                if let Some(k) = kind() {
                    c.kinds.insert(key.clone(), k);
                }
                c.sections.entry(key.clone()).or_insert_with(|| Value::Object(Default::default()));
                section = Some(key);
            }
            ("rule", Some(_)) if skipping => {}
            ("rule", Some(c)) => {
                let Some(key) = section.clone() else {
                    leave("a rule outside a section".into(), &mut left_out);
                    continue;
                };
                let rule = attr("name").unwrap_or_default();
                if known(c.discipline, &key).is_some_and(|k| !k.contains(&rule)) || rule.is_empty() {
                    leave(format!("rule `{rule}`"), &mut left_out);
                    continue;
                }
                let v = match (attr("json"), attr("value")) {
                    (Some(j), _) => match serde_json::from_str::<Value>(&j) {
                        Ok(v) => v,
                        Err(_) => {
                            leave(format!("rule `{rule}`, whose JSON doesn't read"), &mut left_out);
                            continue;
                        }
                    },
                    (None, Some(t)) => value_of(&t),
                    (None, None) => {
                        leave(format!("rule `{rule}`, which has no value"), &mut left_out);
                        continue;
                    }
                };
                let at = format!("{key}.{rule}");
                if let Some(s) = source() {
                    c.sources.insert(at.clone(), s);
                }
                if let Some(k) = kind() {
                    c.kinds.insert(at, k);
                }
                if let Some(Value::Object(o)) = c.sections.get_mut(&key) {
                    o.insert(rule, v);
                }
            }
            (other, Some(_)) => leave(format!("<{}>", other.chars().take(40).collect::<String>()), &mut left_out),
        }
    }
    if let Some(c) = open.take() {
        codes.push(c);
    }
    if codes.is_empty() {
        return Err("the file is no building code: it has no <building-code>".into());
    }
    // Sources the file gives for nothing it has are dropped.
    for c in &mut codes {
        let has: BTreeMap<String, ()> = c
            .sections
            .iter()
            .flat_map(|(k, v)| {
                let rules = v.as_object().map(|o| o.keys().map(|r| (format!("{k}.{r}"), ())).collect::<Vec<_>>()).unwrap_or_default();
                std::iter::once((k.clone(), ())).chain(rules)
            })
            .collect();
        c.sources.retain(|k, _| has.contains_key(k));
        c.kinds.retain(|k, _| has.contains_key(k));
    }
    Ok((codes, left_out))
}
