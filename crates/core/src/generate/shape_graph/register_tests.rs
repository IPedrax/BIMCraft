//! Built-in families are the files in `data/shape-families`: nothing in the code names one.

use std::collections::BTreeSet;
use std::path::PathBuf;

use super::family::{self, Family};
use super::{Request, build};

fn family_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data").join("shape-families")
}

/// Every `<name>.json` directly in the directory but `shapes.json`, sorted.
pub(crate) fn family_file_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(family_dir())
        .expect("data/shape-families")
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".json").map(str::to_string))
        .filter(|n| n != "shapes")
        .collect();
    names.sort();
    names
}

/// The house family's text, for tests that change it.
pub(crate) fn house_family() -> &'static str {
    family::builtin("house").expect("house.json is a built-in family")
}

#[test]
fn builtin_families_are_the_files() {
    let names = family_file_names();
    assert!(names.iter().any(|n| n == "house"), "{names:?}");
    for n in &names {
        assert!(family::builtin(n).is_some(), "{n} is a file but not built in");
    }
    assert_eq!(family::builtin_names().split(", ").collect::<Vec<_>>(), names);
    let mut fill: Vec<String> = std::fs::read_dir(family_dir().join("fill"))
        .map(|d| d.flatten().filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default();
    fill.iter_mut().for_each(|f| *f = f.trim_end_matches(".json").to_string());
    for n in ["shapes", "fill", "no-such-family"].map(String::from).iter().chain(fill.iter()) {
        assert!(family::builtin(n).is_none(), "{n} is not a family");
    }
}

#[test]
fn every_builtin_parses_and_names_itself() {
    for n in family_file_names() {
        let fam = Family::parse(family::builtin(&n).expect("built in")).unwrap_or_else(|e| panic!("{n}: {e}"));
        assert_eq!(fam.name, n, "the `family` of {n}.json is its file name");
    }
}

fn names_word(name: &str, text: &str) -> bool {
    text.match_indices(name).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + name.len()..].chars().next();
        let word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
        !before.is_some_and(word) && !after.is_some_and(word)
    })
}

#[test]
fn unknown_archetype_names_every_file() {
    let req = Request::new("shapes".into(), 1, 12, 0, None, false);
    let e = build(&req, false).err().expect("shapes is no family");
    assert_eq!(e.code, "unknown_archetype");
    for n in family_file_names() {
        assert!(names_word(&n, &e.message), "{:?} does not name {n}", e.message);
    }
}

#[test]
fn registration_names_no_family_in_src() {
    let sources = [
        ("family.rs", include_str!("family.rs")),
        ("shape_graph.rs", include_str!("../shape_graph.rs")),
        ("deliver.rs", include_str!("../deliver.rs")),
    ];
    for (file, text) in sources {
        for (i, line) in text.lines().enumerate() {
            for (at, _) in line.match_indices("shape-families/") {
                let rest = &line[at + "shape-families/".len()..];
                let stem: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')).collect();
                let named = stem.contains(".json");
                assert!(!named || rest.starts_with("shapes.json"), "{file}:{} names a family file", i + 1);
            }
            for n in family_file_names() {
                assert!(!line.contains(&format!("\"{n}\"")), "{file}:{} quotes the family name {n}", i + 1);
            }
        }
    }
}

fn rust_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).expect("a directory").flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The engine is generic: no room kind of the house, office-pattern or studio-office packs is quoted in its code.
#[test]
fn the_engine_quotes_no_room_kind() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src").join("generate");
    let mut files = vec![src.join("shape_graph.rs")];
    rust_files(&src.join("shape_graph"), &mut files);
    let mut kinds = BTreeSet::new();
    for pack in ["house", "office-pattern", "studio-office"] {
        let pack = crate::graph_rules::Pack::embedded(pack).expect("an embedded pack");
        kinds.extend(pack.kinds.keys().cloned());
    }
    assert!(kinds.len() > 10, "{kinds:?}");
    for f in files {
        let text = std::fs::read_to_string(&f).expect("a source file");
        for (i, line) in text.lines().enumerate() {
            for k in &kinds {
                assert!(!line.contains(&format!("\"{k}\"")), "{}:{} quotes the room kind {k}", f.display(), i + 1);
            }
        }
    }
}
