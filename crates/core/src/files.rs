//! The extensions of BIMCraft's files: what a save writes, and what an open still reads (discussion 310).
//!
//! A project is a `.bimc`, its backup a `.bimb`, a template a `.bimt` and a family a `.bimf`; a template and a backup hold the
//! same JSON as a project. They are short because a long extension eats into a path where systems limit its length. What
//! earlier versions wrote still opens: a project's `.bimcraft` (and the `.revcraft` before it), and a family's `.json`.
use std::path::{Path, PathBuf};

/// A project's, which Save As offers first.
pub const PROJECT: &str = "bimc";
/// A project's backup, the recovery copy autosave keeps.
pub const BACKUP: &str = "bimb";
pub const TEMPLATE: &str = "bimt";
pub const FAMILY: &str = "bimf";

/// A project before the short names, and before that.
pub const OLD_PROJECT: &str = "bimcraft";
pub const OLDER_PROJECT: &str = "revcraft";
/// A family before the short names.
pub const OLD_FAMILY: &str = "json";

/// Every extension a project is read under, the default first.
pub const PROJECTS: [&str; 5] = [PROJECT, TEMPLATE, BACKUP, OLD_PROJECT, OLDER_PROJECT];
/// What Save As offers: a project or a template.
pub const SAVED: [&str; 2] = [PROJECT, TEMPLATE];
/// What a family dialog lists, the one Save Family writes first.
pub const FAMILIES: [&str; 2] = [FAMILY, OLD_FAMILY];

fn has<S: AsRef<str>>(path: &Path, extensions: &[S]) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| extensions.iter().any(|x| e.eq_ignore_ascii_case(x.as_ref())))
}

/// Whether `path` is named as a project of any of its names.
pub fn is_project(path: &Path) -> bool {
    has(path, &PROJECTS)
}

/// Whether `path` is named as a family at sight. A `.json` is not: it may be anything, and one that holds a project opens as one.
pub fn is_family(path: &Path) -> bool {
    has(path, &[FAMILY])
}

/// The file name a dialog offers for `stem`: no path in it, and `.ext`.
pub fn file_name(stem: &str, ext: &str) -> String {
    format!("{}.{ext}", stem.replace(['/', '\\'], "-"))
}

/// `path` as typed in a save dialog: kept if it ends in one of `extensions`, else with the first.
pub fn named<S: AsRef<str>>(path: PathBuf, extensions: &[S]) -> PathBuf {
    match extensions.first() {
        Some(first) if !has(&path, extensions) => path.with_extension(first.as_ref()),
        _ => path,
    }
}
