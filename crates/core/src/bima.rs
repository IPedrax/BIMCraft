//! The `.bima` file: a project's assets taken out to share with another project, and brought in from one. It's a zip, as a
//! `.bimc` is ([`crate::bimc`]): `assets.json` says what it holds, and each picture is a file of its own beside it,
//! `images/<id>.png` or `images/<id>.jpg`, the file as the project held it, byte for byte: a JPEG stays the JPEG it came in
//! as, and loses nothing a second time.
//!
//! `assets.json` is `{format, materials, fill_types, fill_patterns, line_patterns, line_styles, pictures}`: each kind by
//! name, an asset `{guid, value}` (its value as a project file writes it), a picture `{guid, name, file}`. What goes out
//! takes what it needs with it: a material its fill patterns and picture, a fill type its patterns, a line style its
//! line pattern.
//!
//! A file is read kindly: an asset this version can't read is left out and said, the rest comes in. A file of a newer
//! `format` is refused, and so is one past [`MAX_FILE`] or with more than [`MAX_ASSETS`] assets.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::library::{Entry, Kind};
use crate::model::{Guid, Model};
use crate::texture::Picture;

/// The version of the file this writes; one of a newer is refused.
pub const FORMAT: u32 = 1;
/// The entry that says what it holds.
pub const ENTRY: &str = "assets.json";
/// The largest `.bima` read, bytes.
pub const MAX_FILE: u64 = 64 << 20;
/// The most assets of each kind a file may hold.
pub const MAX_ASSETS: usize = 10_000;

/// The kinds, as a file and the commands name them.
pub use crate::assets::KINDS;

/// What a file says it holds.
#[derive(Default, Serialize, Deserialize)]
struct File {
    format: u32,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    materials: BTreeMap<String, Asset>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    fill_types: BTreeMap<String, Asset>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    fill_patterns: BTreeMap<String, Asset>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    line_patterns: BTreeMap<String, Asset>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    line_styles: BTreeMap<String, Asset>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pictures: BTreeMap<String, PictureAsset>,
}

/// An asset: its GUID, and what a project file writes of it.
#[derive(Serialize, Deserialize)]
struct Asset {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    guid: Option<Guid>,
    value: Value,
}

/// A picture: its GUID, the name it came in with, and its file in the zip.
#[derive(Serialize, Deserialize)]
struct PictureAsset {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    guid: Option<Guid>,
    name: String,
    file: String,
}

/// Which assets of each kind: by kind, the names, or none for all of that kind. An empty selection is every asset.
pub type Selection = BTreeMap<String, Option<BTreeSet<String>>>;

/// What to do with an asset whose name the project has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Existing {
    /// Keep the project's, and what names it uses it.
    Skip,
    /// Put the file's in its place.
    Replace,
    /// Bring the file's in under a free name, ` 2`, ` 3`…
    Rename,
}

impl Existing {
    pub const NAMES: &str = "skip (the default: keep the project's), replace or rename (` 2`, ` 3`…)";

    pub fn parse(s: &str) -> Option<Existing> {
        Some(match s {
            "skip" => Existing::Skip,
            "replace" => Existing::Replace,
            "rename" => Existing::Rename,
            _ => return None,
        })
    }
}

/// The library kind each of [`KINDS`] is.
fn kind_of(kind: &str) -> Kind {
    match kind {
        "materials" => Kind::Material,
        "fill_types" => Kind::FilledRegionType,
        "fill_patterns" => Kind::FillPattern,
        "line_patterns" => Kind::LinePattern,
        "line_styles" => Kind::LineStyle,
        _ => Kind::Picture,
    }
}

/// Whether `sel` takes asset `name` of `kind`: an empty selection takes everything, else only the kinds it names.
fn chosen(sel: &Selection, kind: &str, name: &str) -> bool {
    sel.is_empty() || sel.get(kind).is_some_and(|names| names.as_ref().is_none_or(|n| n.contains(name)))
}

/// The assets of `m` that `sel` chooses, with what they need: as `(kind, name)`.
fn closure(m: &Model, sel: &Selection) -> BTreeSet<(&'static str, String)> {
    let mut out = BTreeSet::new();
    for kind in KINDS {
        for name in crate::assets::names(m, kind) {
            if chosen(sel, kind, &name) {
                out.insert((kind, name));
            }
        }
    }
    let needs: Vec<(&'static str, String)> = out
        .iter()
        .flat_map(|(kind, name)| {
            let mut n: Vec<(&'static str, String)> = Vec::new();
            match *kind {
                "materials" => {
                    if let Some(x) = m.materials.get(name) {
                        n.extend([&x.cut_pattern, &x.surface_pattern].into_iter().flatten().map(|p| ("fill_patterns", p.clone())));
                        if let Some(tx) = x.texture.as_ref().filter(|tx| !tx.image.is_empty()) {
                            n.push(("pictures", tx.image.clone()));
                        }
                    }
                }
                "fill_types" => {
                    if let Some(x) = m.region_types.get(name) {
                        n.extend([&x.foreground, &x.background].into_iter().flatten().map(|p| ("fill_patterns", p.clone())));
                    }
                }
                "line_styles" => {
                    if let Some(p) = m.line_styles.get(name).and_then(|x| x.pattern.as_ref()) {
                        n.push(("line_patterns", p.clone()));
                    }
                }
                _ => {}
            }
            n
        })
        .collect();
    // Only what the project holds: a material's built-in pattern is no fill pattern of its own.
    out.extend(needs.into_iter().filter(|(kind, name)| crate::assets::names(m, kind).contains(name)));
    out
}

/// The `.bima` of the assets of `m` that `sel` chooses, with what they need; and how many of each kind it holds.
pub fn export(m: &Model, sel: &Selection) -> Result<(Vec<u8>, BTreeMap<&'static str, usize>), String> {
    let take = closure(m, sel);
    let mut file = File { format: FORMAT, ..File::default() };
    let mut images: Vec<(String, Vec<u8>)> = Vec::new();
    let mut counts: BTreeMap<&'static str, usize> = KINDS.iter().map(|k| (*k, 0)).collect();
    for (kind, name) in &take {
        let guid = m.library.guid(&kind_of(kind), name);
        let value = match *kind {
            "materials" => m.materials.get(name).map(serde_json::to_value),
            "fill_types" => m.region_types.get(name).map(serde_json::to_value),
            "fill_patterns" => m.fill_patterns.get(name).map(serde_json::to_value),
            "line_patterns" => m.line_patterns.get(name).map(serde_json::to_value),
            "line_styles" => m.line_styles.get(name).map(serde_json::to_value),
            _ => {
                let Some(p) = m.images.get(name) else { continue };
                let entry = format!("images/{name}.{}", p.extension());
                images.push((entry.clone(), p.file().to_vec()));
                file.pictures.insert(name.clone(), PictureAsset { guid, name: p.name().to_string(), file: entry });
                *counts.entry(kind).or_default() += 1;
                continue;
            }
        };
        let Some(value) = value else { continue };
        let asset = Asset { guid, value: value.map_err(|e| e.to_string())? };
        let to = match *kind {
            "materials" => &mut file.materials,
            "fill_types" => &mut file.fill_types,
            "fill_patterns" => &mut file.fill_patterns,
            "line_patterns" => &mut file.line_patterns,
            _ => &mut file.line_styles,
        };
        to.insert(name.clone(), asset);
        *counts.entry(kind).or_default() += 1;
    }
    let json = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    let mut entries: Vec<(&str, &[u8], bool)> = vec![(ENTRY, json.as_bytes(), true)];
    entries.extend(images.iter().map(|(n, b)| (n.as_str(), b.as_slice(), false)));
    Ok((crate::bimc::zip_files(&entries)?, counts))
}

/// What an import did: each asset's `(kind, name)` added, replaced, skipped, renamed `(kind, from, to)`, or left out with why.
#[derive(Default)]
pub struct Imported {
    pub added: Vec<(&'static str, String)>,
    pub replaced: Vec<(&'static str, String)>,
    pub skipped: Vec<(&'static str, String)>,
    pub renamed: Vec<(&'static str, String, String)>,
    pub left_out: Vec<(&'static str, String, String)>,
}

impl Imported {
    pub fn json(&self) -> Value {
        let pairs = |v: &[(&str, String)]| v.iter().map(|(k, n)| json!({"kind": k, "name": n})).collect::<Vec<_>>();
        json!({
            "added": pairs(&self.added),
            "replaced": pairs(&self.replaced),
            "skipped": pairs(&self.skipped),
            "renamed": self.renamed.iter().map(|(k, a, b)| json!({"kind": k, "from": a, "to": b})).collect::<Vec<_>>(),
            "left_out": self.left_out.iter().map(|(k, n, why)| json!({"kind": k, "name": n, "why": why})).collect::<Vec<_>>(),
        })
    }
}

/// The assets of the `.bima` `bytes` that `sel` chooses, brought into `m`; a name `m` has, as `existing` says.
pub fn import(m: &mut Model, bytes: &[u8], sel: &Selection, existing: Existing) -> Result<Imported, String> {
    if bytes.len() as u64 > MAX_FILE {
        return Err(format!("it's {} bytes; an assets file is up to {MAX_FILE}", bytes.len()));
    }
    let files: BTreeMap<String, Vec<u8>> = crate::bimc::unzip_files(bytes, MAX_FILE * 4)?.into_iter().collect();
    let json = files.get(ENTRY).ok_or_else(|| format!("it is a zip without an `{ENTRY}`: not a BIMCraft assets file"))?;
    let raw: Value = serde_json::from_slice(json).map_err(|e| format!("its `{ENTRY}` isn't JSON: {e}"))?;
    let format = raw.get("format").and_then(Value::as_u64).ok_or_else(|| format!("its `{ENTRY}` has no `format`"))?;
    if format > u64::from(FORMAT) {
        return Err(format!("it is an assets file of format {format}, newer than this BIMCraft reads ({FORMAT}): update BIMCraft"));
    }
    let file: File = serde_json::from_value(raw).map_err(|e| format!("its `{ENTRY}`: {e}"))?;
    for (kind, n) in [
        ("materials", file.materials.len()),
        ("fill_types", file.fill_types.len()),
        ("fill_patterns", file.fill_patterns.len()),
        ("line_patterns", file.line_patterns.len()),
        ("line_styles", file.line_styles.len()),
        ("pictures", file.pictures.len()),
    ] {
        if n > MAX_ASSETS {
            return Err(format!("it holds {n} {kind}; an assets file holds up to {MAX_ASSETS} of each"));
        }
    }
    let mut out = Imported::default();
    // What each of the file's names goes by here, by kind, for the assets that name them.
    let mut here: BTreeMap<&'static str, BTreeMap<String, String>> = BTreeMap::new();

    // Pictures first: a picture is known by its pixels, so one the project holds is the project's.
    for (key, p) in &file.pictures {
        if !chosen(sel, "pictures", key) && !needed(&file, "pictures", key, sel) {
            continue;
        }
        let Some(png) = files.get(&p.file) else {
            out.left_out.push(("pictures", key.clone(), format!("its file `{}` isn't in the zip", p.file)));
            continue;
        };
        match Picture::from_file(&p.name, png.clone()) {
            Ok(picture) => {
                let id = picture.id();
                here.entry("pictures").or_default().insert(key.clone(), id.clone());
                if m.images.contains_key(&id) {
                    out.skipped.push(("pictures", id));
                } else {
                    m.images.insert(id.clone(), picture);
                    keep_guid(m, &Kind::Picture, &id, p.guid);
                    out.added.push(("pictures", id));
                }
            }
            Err(why) => out.left_out.push(("pictures", key.clone(), why)),
        }
    }
    // Then what others need, then what needs them.
    for kind in ["fill_patterns", "line_patterns", "fill_types", "line_styles", "materials"] {
        let assets = match kind {
            "materials" => &file.materials,
            "fill_types" => &file.fill_types,
            "fill_patterns" => &file.fill_patterns,
            "line_patterns" => &file.line_patterns,
            _ => &file.line_styles,
        };
        for (name, asset) in assets {
            if !chosen(sel, kind, name) && !needed(&file, kind, name, sel) {
                continue;
            }
            let mut value = asset.value.clone();
            rename_refs(kind, &mut value, &here);
            let taken = crate::assets::names(m, kind).contains(name);
            let to = match (taken, existing) {
                (false, _) => name.clone(),
                (true, Existing::Skip) => {
                    here.entry(kind).or_default().insert(name.clone(), name.clone());
                    out.skipped.push((kind, name.clone()));
                    continue;
                }
                (true, Existing::Replace) => name.clone(),
                (true, Existing::Rename) => {
                    let names = crate::assets::names(m, kind);
                    (2..).map(|k| format!("{name} {k}")).find(|n| !names.contains(n)).unwrap_or_default()
                }
            };
            if let Err(why) = put(m, kind, &to, value) {
                out.left_out.push((kind, name.clone(), why));
                continue;
            }
            here.entry(kind).or_default().insert(name.clone(), to.clone());
            if !taken {
                keep_guid(m, &kind_of(kind), &to, asset.guid);
                out.added.push((kind, to));
            } else if existing == Existing::Replace {
                out.replaced.push((kind, to));
            } else {
                keep_guid(m, &kind_of(kind), &to, asset.guid);
                out.renamed.push((kind, name.clone(), to));
            }
        }
    }
    m.sync_library();
    Ok(out)
}

/// Whether asset `name` of `kind` is one that an asset `sel` chooses from `file` needs.
fn needed(file: &File, kind: &str, name: &str, sel: &Selection) -> bool {
    if sel.is_empty() {
        return true;
    }
    let names = |v: &Value, keys: &[&str]| keys.iter().any(|k| v.get(*k).and_then(Value::as_str) == Some(name));
    match kind {
        "fill_patterns" => {
            file.materials.iter().any(|(n, a)| chosen(sel, "materials", n) && names(&a.value, &["cut_pattern", "surface_pattern"]))
                || file.fill_types.iter().any(|(n, a)| chosen(sel, "fill_types", n) && names(&a.value, &["foreground", "background"]))
        }
        "line_patterns" => file.line_styles.iter().any(|(n, a)| chosen(sel, "line_styles", n) && names(&a.value, &["pattern"])),
        "pictures" => file.materials.iter().any(|(n, a)| {
            chosen(sel, "materials", n) && a.value.get("texture").and_then(|t| t.get("image")).and_then(Value::as_str) == Some(name)
        }),
        _ => false,
    }
}

/// The names asset `value` of `kind` holds of others, as they go by here.
fn rename_refs(kind: &str, value: &mut Value, here: &BTreeMap<&'static str, BTreeMap<String, String>>) {
    let swap = |v: Option<&mut Value>, of: &str| {
        if let Some(Value::String(n)) = v
            && let Some(to) = here.get(of).and_then(|m| m.get(n.as_str()))
        {
            n.clone_from(to);
        }
    };
    match kind {
        "materials" => {
            swap(value.get_mut("cut_pattern"), "fill_patterns");
            swap(value.get_mut("surface_pattern"), "fill_patterns");
            swap(value.get_mut("texture").and_then(|t| t.get_mut("image")), "pictures");
        }
        "fill_types" => {
            swap(value.get_mut("foreground"), "fill_patterns");
            swap(value.get_mut("background"), "fill_patterns");
        }
        "line_styles" => swap(value.get_mut("pattern"), "line_patterns"),
        _ => {}
    }
}

/// Asset `value` of `kind` put in `m` as `name`; or why it can't be read.
fn put(m: &mut Model, kind: &str, name: &str, value: Value) -> Result<(), String> {
    let why = |e: serde_json::Error| format!("this BIMCraft can't read it: {e}");
    match kind {
        "materials" => {
            let x = serde_json::from_value(value).map_err(why)?;
            m.materials.insert(name.to_string(), x);
        }
        "fill_types" => {
            m.region_types.insert(name.to_string(), serde_json::from_value(value).map_err(why)?);
        }
        "fill_patterns" => {
            m.fill_patterns.insert(name.to_string(), serde_json::from_value(value).map_err(why)?);
        }
        "line_patterns" => {
            m.line_patterns.insert(name.to_string(), serde_json::from_value(value).map_err(why)?);
        }
        _ => {
            m.line_styles.insert(name.to_string(), serde_json::from_value(value).map_err(why)?);
        }
    }
    Ok(())
}

/// The library entry of `name` of `kind`, new to `m`, given the file's GUID where the project has no entry of that GUID.
fn keep_guid(m: &mut Model, kind: &Kind, name: &str, guid: Option<Guid>) {
    let Some(g) = guid else { return };
    if m.library.id(kind, name).is_some() || m.library.by_guid(g).is_some() {
        return;
    }
    let id = m.fresh_library_id();
    // ponytail: an entry that can't go in leaves the name to `sync_library`, which gives it a GUID of its own.
    let _ = m.library.insert(id, Entry { kind: kind.clone(), name: name.to_string(), guid: g });
}
