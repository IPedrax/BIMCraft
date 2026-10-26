//! Build-time catalog validation and translator tooling.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[cfg(any(test, feature = "tooling"))]
mod extract;
#[cfg(any(test, feature = "tooling"))]
pub use extract::extract;

#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub context: String,
    pub id: String,
    pub translation: String,
    pub fuzzy: bool,
    pub comments: Vec<String>,
}

pub type Entries = BTreeMap<(String, String), Entry>;

#[derive(Default)]
pub struct Document {
    pub entries: Entries,
    pub header: String,
    pub header_comments: Vec<String>,
    pub obsolete: Vec<String>,
}

#[derive(Clone, Copy)]
enum Field {
    Context,
    Id,
    Translation,
}

#[derive(Default)]
struct Pending {
    entry: Entry,
    field: Option<Field>,
    context: bool,
    id: bool,
    translation: bool,
}

impl Pending {
    fn finish(&mut self, document: &mut Document) -> Result<(), String> {
        if !self.id && !self.context && !self.translation {
            self.entry.comments.clear();
            return Ok(());
        }
        if !self.id || !self.translation {
            return Err("each entry must have one msgid and one msgstr".into());
        }
        let entry = std::mem::take(&mut self.entry);
        *self = Self::default();
        if entry.id.is_empty() && entry.context.is_empty() {
            if !document.header.is_empty() {
                return Err("duplicate catalog header".into());
            }
            document.header = entry.translation;
            document.header_comments = entry.comments;
            return Ok(());
        }
        if !entry.fuzzy && !entry.translation.trim().is_empty() && entry.id.matches("{}").count() != entry.translation.matches("{}").count()
        {
            return Err(format!("{:?}: translation must preserve the number of {{}} placeholders", entry.id));
        }
        let key = (entry.context.clone(), entry.id.clone());
        if document.entries.insert(key.clone(), entry).is_some() {
            return Err(format!("duplicate message: context {:?}, msgid {:?}", key.0, key.1));
        }
        Ok(())
    }

    fn append(&mut self, text: &str) -> Result<(), String> {
        let value = unquote(text)?;
        match self.field {
            Some(Field::Context) => self.entry.context.push_str(&value),
            Some(Field::Id) => self.entry.id.push_str(&value),
            Some(Field::Translation) => self.entry.translation.push_str(&value),
            None => return Err("quoted continuation without a message field".into()),
        }
        Ok(())
    }

    fn line(&mut self, line: &str, document: &mut Document) -> Result<(), String> {
        if line.is_empty() {
            return self.finish(document);
        }
        if line.starts_with('#') {
            if self.translation {
                self.finish(document)?;
            }
            if line.starts_with("#~") {
                document.obsolete.append(&mut self.entry.comments);
                self.entry.fuzzy = false;
                document.obsolete.push(line.to_string());
            } else {
                if let Some(flags) = line.strip_prefix("#,") {
                    self.entry.fuzzy |= flags.split(',').any(|f| f.trim() == "fuzzy");
                }
                self.entry.comments.push(line.to_string());
            }
            return Ok(());
        }
        if line.starts_with('"') {
            return self.append(line);
        }
        let (name, text) = line.split_once(char::is_whitespace).ok_or("expected a PO field and quoted string")?;
        if self.translation && matches!(name, "msgctxt" | "msgid") {
            self.finish(document)?;
        }
        self.field = Some(match name {
            "msgctxt" if !self.context && !self.id => {
                self.context = true;
                Field::Context
            }
            "msgid" if !self.id => {
                self.id = true;
                Field::Id
            }
            "msgstr" if self.id && !self.translation => {
                self.translation = true;
                Field::Translation
            }
            "msgid_plural" => {
                return Err("plural entries are not supported by the current translation API; use separate category contexts".into());
            }
            _ => return Err(format!("unexpected or repeated PO field {name:?}")),
        });
        self.append(text.trim_start())
    }
}

/// Parse UTF-8 PO syntax, including multiline strings, contexts, flags and obsolete entries.
/// Unsupported plural entries and malformed syntax are errors, never silently discarded.
pub fn document(text: &str) -> Result<Document, String> {
    let mut document = Document::default();
    let mut pending = Pending::default();
    for (index, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        pending.line(line.trim(), &mut document).map_err(|e| format!("line {}: {e}", index + 1))?;
    }
    pending.finish(&mut document)?;
    Ok(document)
}

pub fn parse(text: &str) -> Result<Entries, String> {
    Ok(document(text)?.entries)
}

fn unquote(text: &str) -> Result<String, String> {
    let mut chars = text.strip_prefix('"').ok_or("expected opening quote")?.chars();
    let mut result = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' if chars.as_str().is_empty() => return Ok(result),
            '"' => return Err("unexpected text after closing quote".into()),
            '\\' => result.push(match chars.next().ok_or("unfinished escape")? {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'a' => '\u{7}',
                'b' => '\u{8}',
                'f' => '\u{c}',
                'v' => '\u{b}',
                '\\' => '\\',
                '"' => '"',
                '\'' => '\'',
                c => return Err(format!("unsupported escape \\{c}")),
            }),
            c => result.push(c),
        }
    }
    Err("missing closing quote".into())
}

pub fn quote(text: &str) -> String {
    let mut result = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '\u{7}' => result.push_str("\\a"),
            '\u{8}' => result.push_str("\\b"),
            '\u{c}' => result.push_str("\\f"),
            '\u{b}' => result.push_str("\\v"),
            c => result.push(c),
        }
    }
    result.push('"');
    result
}

pub fn read(path: &Path) -> Result<Document, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    document(&text).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn render(document: &Document) -> String {
    let mut text = document.header_comments.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(&format!("msgid \"\"\nmsgstr {}\n\n", quote(&document.header)));
    for entry in document.entries.values() {
        for comment in &entry.comments {
            text.push_str(comment);
            text.push('\n');
        }
        if !entry.context.is_empty() {
            text.push_str(&format!("msgctxt {}\n", quote(&entry.context)));
        }
        text.push_str(&format!("msgid {}\nmsgstr {}\n\n", quote(&entry.id), quote(&entry.translation)));
    }
    if !document.obsolete.is_empty() {
        text.push_str(&document.obsolete.join("\n"));
        text.push('\n');
    }
    text
}

/// Merge by exact context and English source. Changed messages remain untranslated instead of
/// guessing a translation. Translator comments, flags, metadata and obsolete work are retained.
pub fn merge(mut old: Document, template: &Document) -> Document {
    // A message removed in one version and restored in another can reuse its old translation.
    let restored_text = old.obsolete.iter().map(|line| line.strip_prefix("#~ ").unwrap_or(line)).collect::<Vec<_>>().join("\n");
    if let Ok(mut obsolete) = document(&restored_text) {
        let mut restored = false;
        for key in template.entries.keys() {
            if !old.entries.contains_key(key)
                && let Some(entry) = obsolete.entries.remove(key)
            {
                old.entries.insert(key.clone(), entry);
                restored = true;
            }
        }
        if restored {
            old.obsolete.clear();
            for entry in obsolete.entries.values() {
                append_obsolete(&mut old.obsolete, entry);
            }
        }
    }
    let mut entries = Entries::new();
    for (key, source) in &template.entries {
        let mut entry = old.entries.remove(key).unwrap_or_else(|| source.clone());
        entry.comments.retain(|c| !c.starts_with("#:") && !c.starts_with("#."));
        entry.comments.extend(source.comments.iter().filter(|c| c.starts_with("#:") || c.starts_with("#.")).cloned());
        entries.insert(key.clone(), entry);
    }
    for entry in old.entries.values() {
        append_obsolete(&mut old.obsolete, entry);
    }
    old.entries = entries;
    old
}

fn append_obsolete(lines: &mut Vec<String>, entry: &Entry) {
    lines.extend(entry.comments.iter().cloned());
    if !entry.context.is_empty() {
        lines.push(format!("#~ msgctxt {}", quote(&entry.context)));
    }
    lines.push(format!("#~ msgid {}", quote(&entry.id)));
    lines.push(format!("#~ msgstr {}", quote(&entry.translation)));
}

/// Validate catalogs and generate sorted static Rust tables for a domain. Only translated,
/// non-fuzzy entries are embedded, so every other lookup falls back to its English source.
pub fn compile(directory: &Path, output: &Path) -> Result<(), String> {
    println!("cargo:rerun-if-changed={}", directory.display());
    let mut paths = fs::read_dir(directory)
        .map_err(|e| format!("{}: {e}", directory.display()))?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    paths.sort();
    let mut generated = String::from("pub const CATALOGS: bimcraft_i18n::Catalogs = &[\n");
    for path in paths.iter().filter(|p| p.extension().is_some_and(|e| e == "po")) {
        let language = path.file_stem().and_then(|s| s.to_str()).ok_or("invalid catalog filename")?;
        let document = read(path)?;
        generated.push_str(&format!("({language:?}, &[\n"));
        for entry in document.entries.values().filter(|e| !e.fuzzy && !e.translation.trim().is_empty()) {
            generated.push_str(&format!("({:?}, {:?}, {:?}),\n", entry.context, entry.id, entry.translation));
        }
        generated.push_str("]),\n");
    }
    generated.push_str("];\n");
    fs::write(output, generated).map_err(|e| format!("{}: {e}", output.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn po_files_support_context_unicode_multiline_comments_and_fuzzy_entries() {
        let entries = parse(concat!(
            "msgid \"\"\nmsgstr \"Content-Type: text/plain; charset=UTF-8\\n\"\n\n",
            "#. Button\n#: window.rs:12\nmsgctxt \"verb\"\nmsgid \"Save \\\"\"\n\"{}\\n\"\nmsgstr \"Salvar \\\"{}\\n\"\n\n",
            "#, fuzzy\nmsgid \"Open\"\nmsgstr \"打开\"\n\n",
            "#~ msgid \"Old\"\n#~ msgstr \"Antigo\"\n",
        ))
        .unwrap();
        let saved = &entries[&("verb".into(), "Save \"{}\n".into())];
        assert_eq!(saved.translation, "Salvar \"{}\n");
        assert!(entries[&("".into(), "Open".into())].fuzzy);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn invalid_catalogs_are_errors_instead_of_silent_bad_translations() {
        for text in [
            "msgid \"Open\"\nmsgstr missing-quotes\n",
            "msgid \"Open\"\n",
            "msgstr \"Abrir\"\n",
            "msgid \"Open\"\nmsgstr \"Abrir\"\n\nmsgid \"Open\"\nmsgstr \"Abierto\"\n",
            "msgid \"Count {}\"\nmsgstr \"Contagem\"\n",
            "msgid \"Open\"\nmsgstr \"Abrir\"\ngarbage\n",
            "msgid \"Bad\\q\"\nmsgstr \"Ruim\"\n",
        ] {
            assert!(parse(text).is_err(), "accepted {text:?}");
        }
    }

    #[test]
    fn compilation_skips_untranslated_fuzzy_and_obsolete_entries_and_sorts_catalogs() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("pt.po"),
            concat!(
                "msgid \"Save\"\nmsgstr \"Salvar\"\n\n",
                "msgid \"Blank\"\nmsgstr \"\"\n\n",
                "#, fuzzy\nmsgid \"Count {}\"\nmsgstr \"Contagem\"\n\n",
                "#~ msgid \"Old\"\n#~ msgstr \"Antigo\"\n",
            ),
        )
        .unwrap();
        fs::write(directory.path().join("es.po"), "msgid \"Save\"\nmsgstr \"Guardar\"\n").unwrap();
        let output = directory.path().join("catalogs.rs");
        compile(directory.path(), &output).unwrap();
        let generated = fs::read_to_string(output).unwrap();
        syn::parse_file(&generated).unwrap();
        assert!(generated.contains("Salvar") && generated.contains("Guardar"));
        assert!(!generated.contains("Blank") && !generated.contains("Count") && !generated.contains("Antigo"));
        assert!(generated.find("es").unwrap() < generated.find("pt").unwrap());
    }

    #[test]
    fn updating_preserves_translator_work_and_is_repeatable() {
        let old = document(concat!(
            "# Translator-owned header\nmsgid \"\"\nmsgstr \"Language: pt\\n\"\n\n",
            "# Keep this advice\n#. Old developer comment\n#: old.rs:1\n#, fuzzy\nmsgid \"Save\"\nmsgstr \"Salvar\"\n\n",
            "# Keep removed work too\nmsgid \"Old\"\nmsgstr \"Antigo\"\n",
        ))
        .unwrap();
        let template = document("#. New advice\n#: window.rs:2\nmsgid \"Save\"\nmsgstr \"\"\n\nmsgid \"New\"\nmsgstr \"\"\n").unwrap();
        let updated = render(&merge(old, &template));
        for expected in [
            "Translator-owned header",
            "Language: pt",
            "Keep this advice",
            "New advice",
            "window.rs:2",
            "fuzzy",
            "Salvar",
            "Keep removed work too",
            "#~ msgid \"Old\"",
            "Antigo",
        ] {
            assert!(updated.contains(expected), "missing {expected:?}: {updated}");
        }
        assert!(!updated.contains("old.rs:1") && !updated.contains("Old developer comment"));
        let again = render(&merge(document(&updated).unwrap(), &template));
        assert_eq!(updated, again);
        let restored = document("msgid \"Old\"\nmsgstr \"\"\n").unwrap();
        let restored = merge(document(&updated).unwrap(), &restored);
        assert_eq!(restored.entries[&("".into(), "Old".into())].translation, "Antigo");
        assert!(restored.entries[&("".into(), "Old".into())].comments.iter().any(|c| c.contains("Keep removed work too")));
        assert!(!render(&restored).contains("#~ msgid \"Old\""));
    }
}
