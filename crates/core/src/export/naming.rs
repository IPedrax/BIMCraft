//! File names from templates: `{SheetNumber} - {SheetName}`, with `/` making folders (`{Format}/{SheetNumber}`).
//!
//! A token is a name in braces, its value from [`Tokens`]; `{{` and `}}` are braces. Each folder and the name are made safe
//! to write on Windows, macOS and Linux ([`safe`]); a template can't climb out of the folder it is written in.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// The longest a template may be, in characters.
pub const MAX_TEMPLATE: usize = 512;
/// The longest a folder's or file's name is made, in characters (well under every system's 255 bytes, room for `(2)` and an
/// extension).
pub const MAX_NAME: usize = 150;
/// The most folders deep a template may go.
pub const MAX_DEPTH: usize = 8;

/// What a template's tokens stand for.
pub trait Tokens {
    /// The token's value, by its name as written (`SheetNumber`); none, a name this doesn't know. A token it knows that has
    /// no value here (a view's `SheetNumber`) is `Some("")`.
    fn token(&self, name: &str) -> Option<String>;
    /// The names it knows, for the error that names an unknown one and for a dialog's list.
    fn names(&self) -> Vec<String>;
}

/// The template with its tokens filled in: a path of folders and a name, each made [`safe`], relative to where it is
/// written. `extension` (without the dot) is put on the name.
pub fn expand(template: &str, tokens: &dyn Tokens, extension: &str) -> Result<PathBuf, String> {
    if template.chars().count() > MAX_TEMPLATE {
        return Err(format!("a file name template is at most {MAX_TEMPLATE} characters"));
    }
    let mut text = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                text.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                text.push('}');
            }
            '{' => {
                let name: String = chars.by_ref().take_while(|c| *c != '}').collect();
                let name = name.trim();
                let value = tokens
                    .token(name)
                    .ok_or_else(|| format!("`{{{name}}}` isn't a token a file name can hold; there are: {}", tokens.names().join(", ")))?;
                // A value is text, never folders: its slashes are not the template's.
                text.push_str(&value.replace(['/', '\\'], "-"));
            }
            '}' => return Err("a `}` in a file name template closes no token; write `}}` for the brace".to_string()),
            '\\' => text.push('/'),
            c => text.push(c),
        }
    }
    let parts: Vec<String> = text.split('/').map(str::trim).filter(|p| !p.is_empty() && *p != ".").map(String::from).collect();
    if parts.iter().any(|p| p == "..") {
        return Err("a file name template can't climb out of the folder it is written in (`..`)".to_string());
    }
    if parts.len() > MAX_DEPTH + 1 {
        return Err(format!("a file name template goes at most {MAX_DEPTH} folders deep"));
    }
    let Some((name, folders)) = parts.split_last() else {
        return Err("the file name template makes an empty name".to_string());
    };
    let mut path: PathBuf = folders.iter().map(|f| safe(f)).collect();
    let extension = safe(extension.trim_start_matches('.'));
    path.push(format!("{}.{extension}", safe(name)));
    Ok(path)
}

/// `name` as a folder's or file's name any system takes: no characters Windows refuses, no control characters, no trailing
/// dot or space, not a name Windows keeps for devices, not empty, and at most [`MAX_NAME`] characters.
pub fn safe(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') { '_' } else { c })
        .take(MAX_NAME)
        .collect();
    while s.ends_with(['.', ' ']) {
        s.pop();
    }
    let s = s.trim_start().to_string();
    let stem = s.split('.').next().unwrap_or_default().to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes().get(3).is_some_and(|d| (b'1'..=b'9').contains(d)));
    match () {
        _ if s.is_empty() => "untitled".to_string(),
        _ if reserved => format!("_{s}"),
        _ => s,
    }
}

/// `path`, or the first of `path (2)`, `path (3)`… that neither `taken` holds nor is on disk under `root`; it is added to
/// `taken`. The names are compared as Windows and macOS do, whatever their case.
pub fn unique(root: &std::path::Path, path: PathBuf, taken: &mut BTreeSet<String>) -> PathBuf {
    let key = |p: &std::path::Path| p.to_string_lossy().to_lowercase();
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let mut candidate = path.clone();
    let mut n = 2u32;
    while taken.contains(&key(&candidate)) || root.join(&candidate).exists() {
        candidate = path.with_file_name(format!("{stem} ({n}){ext}"));
        n = n.saturating_add(1);
        if n == u32::MAX {
            break;
        }
    }
    taken.insert(key(&candidate));
    candidate
}

/// A UTC date `YYYY-MM-DD` and time `HH-MM` (a colon can't be in a Windows file name) from seconds since 1970.
pub fn date_and_time(secs: u64) -> (String, String) {
    let stamp = crate::ifc::utc(secs);
    let date = stamp.get(..10).unwrap_or_default().to_string();
    let time = stamp.get(11..16).unwrap_or_default().replace(':', "-");
    (date, time)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct Map(BTreeMap<&'static str, &'static str>);
    impl Tokens for Map {
        fn token(&self, name: &str) -> Option<String> {
            self.0.get(name).map(|v| v.to_string())
        }
        fn names(&self) -> Vec<String> {
            self.0.keys().map(|k| k.to_string()).collect()
        }
    }

    fn tokens() -> Map {
        Map(BTreeMap::from([("SheetNumber", "A-101"), ("SheetName", "Ground Floor: Plan"), ("Format", "PDF"), ("Empty", "")]))
    }

    #[test]
    fn tokens_are_filled_in_and_made_safe() {
        let p = expand("{SheetNumber} - {SheetName}", &tokens(), "pdf").unwrap();
        assert_eq!(p, PathBuf::from("A-101 - Ground Floor_ Plan.pdf"));
        assert_eq!(expand("{{x}} {Empty}", &tokens(), "pdf").unwrap(), PathBuf::from("{x}.pdf"));
    }

    #[test]
    fn slashes_make_folders_but_values_never_do() {
        let p = expand("{Format}/{SheetNumber}", &tokens(), "pdf").unwrap();
        assert_eq!(p, ["PDF", "A-101.pdf"].iter().collect::<PathBuf>());
        let slashed = Map(BTreeMap::from([("Name", "a/b\\c")]));
        assert_eq!(expand("{Name}", &slashed, "dxf").unwrap(), PathBuf::from("a-b-c.dxf"));
        assert_eq!(expand("x\\y", &tokens(), "png").unwrap(), ["x", "y.png"].iter().collect::<PathBuf>());
    }

    #[test]
    fn bad_templates_say_what_is_wrong() {
        assert!(expand("{Nope}", &tokens(), "pdf").unwrap_err().contains("SheetNumber"));
        assert!(expand("../{SheetName}", &tokens(), "pdf").unwrap_err().contains(".."));
        assert!(expand("a}b", &tokens(), "pdf").is_err());
        assert!(expand("/ / /", &tokens(), "pdf").is_err());
        assert!(expand(&"a/".repeat(20), &tokens(), "pdf").is_err());
        assert!(expand(&"x".repeat(MAX_TEMPLATE + 1), &tokens(), "pdf").is_err());
    }

    #[test]
    fn names_windows_refuses_are_changed() {
        assert_eq!(safe("CON"), "_CON");
        assert_eq!(safe("com3.txt"), "_com3.txt");
        assert_eq!(safe("COM0"), "COM0");
        assert_eq!(safe("plan. "), "plan");
        assert_eq!(safe("a?b*c\u{7}"), "a_b_c_");
        assert_eq!(safe("   "), "untitled");
        assert_eq!(safe(&"é".repeat(400)).chars().count(), MAX_NAME);
    }

    #[test]
    fn a_name_taken_gets_a_number() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("A.pdf"), b"").unwrap();
        let mut taken = BTreeSet::new();
        assert_eq!(unique(dir.path(), "A.pdf".into(), &mut taken), PathBuf::from("A (2).pdf"));
        // Names this export already took are compared whatever their case, as Windows and macOS would.
        assert_eq!(unique(dir.path(), "a (2).PDF".into(), &mut taken), PathBuf::from("a (2) (2).PDF"));
        assert_eq!(unique(dir.path(), "B.pdf".into(), &mut taken), PathBuf::from("B.pdf"));
    }

    #[test]
    fn dates_are_utc_and_file_safe() {
        // 2026-11-09 10:05 UTC.
        assert_eq!(date_and_time(1_794_218_700), ("2026-11-09".to_string(), "10-05".to_string()));
    }
}
