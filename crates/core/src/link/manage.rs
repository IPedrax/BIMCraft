//! Manage Links, as Revit's dialog lists them: every file the project reads from elsewhere, a tab for each kind, one row for
//! each file (as Revit lists a link's type, its instances under it), with the path the project keeps, whether it's relative
//! to the project's file, where it resolves, its status and the elements that place it.
//!
//! A link (a BIMCraft project or an IFC) is never in the project's file, only its path; a CAD drawing and a scan are, so an
//! imported one is listed as `imported`, with no path to read again.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::ids::Id;
use crate::model::{Element, Model};

/// The tabs, in the order the dialog has them.
pub const KINDS: [&str; 7] = ["projects", "ifc", "cad", "pdf", "images", "clouds", "models"];

/// Their names, for an error.
pub const KIND_NAMES: &str = "projects, ifc, cad, pdf, images, clouds or models";

/// Where `path`, as the project keeps it, is: a relative one beside the project's file `project`, when it has one.
pub fn resolve(project: Option<&Path>, path: &str) -> PathBuf {
    let at = PathBuf::from(path);
    match (project.and_then(Path::parent), at.is_relative()) {
        (Some(dir), true) => dir.join(at),
        _ => at,
    }
}

/// The list for each tab `kind` names, or for every tab: `{projects: [row, …], ifc: […], …}`.
pub fn list(m: &Model, project: Option<&Path>, kind: Option<&str>) -> Value {
    let mut out = serde_json::Map::new();
    for k in KINDS.iter().filter(|k| kind.is_none_or(|want| want == **k)) {
        let rows = match *k {
            "projects" => links(m, project, false),
            "ifc" => links(m, project, true),
            "cad" => cads(m, project),
            "clouds" => clouds(m),
            "models" => models(m),
            "images" => images(m, project, false),
            "pdf" => images(m, project, true),
            _ => Vec::new(),
        };
        out.insert((*k).to_string(), Value::Array(rows));
    }
    Value::Object(out)
}

/// How a row names where its file is: the path kept, whether it's relative and where it resolves.
fn located(row: &mut Value, project: Option<&Path>, path: &str) -> PathBuf {
    let full = resolve(project, path);
    row["path"] = json!(path);
    row["path_type"] = json!(if Path::new(path).is_relative() { "relative" } else { "absolute" });
    row["resolved"] = json!(full.to_string_lossy());
    full
}

/// The links to projects, or with `ifc` to IFC files: one row for each file, its instances the links that read it.
fn links(m: &Model, project: Option<&Path>, ifc: bool) -> Vec<Value> {
    let mut files: Vec<(String, Vec<Id>, Vec<String>)> = Vec::new();
    for (id, e) in m.elements.iter() {
        let Element::Link(l) = e else { continue };
        let is_ifc = Path::new(&l.path).extension().is_some_and(|x| x.eq_ignore_ascii_case("ifc"));
        if is_ifc != ifc {
            continue;
        }
        match files.iter_mut().find(|(p, _, _)| *p == l.path) {
            Some((_, ids, names)) => {
                ids.push(*id);
                names.push(l.name.clone());
            }
            None => files.push((l.path.clone(), vec![*id], vec![l.name.clone()])),
        }
    }
    files
        .into_iter()
        .map(|(path, ids, names)| {
            let name = Path::new(&path).file_stem().map_or_else(|| path.clone(), |s| s.to_string_lossy().into_owned());
            let mut row = json!({"name": name, "instances": ids.len(), "ids": ids, "names": names});
            let full = located(&mut row, project, &path);
            let unloaded = ids.iter().any(|id| matches!(m.elements.get(*id), Some(Element::Link(l)) if l.unloaded));
            let status = if unloaded {
                "unloaded"
            } else if !full.is_file() {
                "not found"
            } else if m.loaded.contains_key(&path) {
                "loaded"
            } else {
                "not loaded"
            };
            row["status"] = json!(status);
            row
        })
        .collect()
}

/// The CAD drawings the project holds: a linked one by its file, an imported one by what it holds; their instances the
/// imports that place them.
fn cads(m: &Model, project: Option<&Path>) -> Vec<Value> {
    m.cads
        .iter()
        .map(|(name, cad)| {
            let ids: Vec<Id> = placing(m, |e| matches!(e, Element::CadImport(ci) if ci.cad == *name));
            let mut row = json!({"name": name, "source": cad.source(), "instances": ids.len(), "ids": ids, "path": null});
            let status = match cad.path() {
                None => "imported",
                Some(path) if located(&mut row, project, path).is_file() => "loaded",
                Some(_) => "not found",
            };
            row["status"] = json!(status);
            row
        })
        .collect()
}

/// The scans the project holds, and the point clouds that place them.
fn clouds(m: &Model) -> Vec<Value> {
    m.clouds
        .iter()
        .map(|(name, cloud)| {
            let ids: Vec<Id> = placing(m, |e| matches!(e, Element::PointCloud(pc) if pc.cloud == *name));
            json!({"name": name, "source": cloud.source(), "instances": ids.len(), "ids": ids, "path": null, "status": "imported"})
        })
        .collect()
}

/// The 3D models families were made from, one row for each file: its families and the components that place them.
fn models(m: &Model) -> Vec<Value> {
    let mut files: Vec<(String, Vec<String>)> = Vec::new();
    for (name, f) in m.families.iter() {
        let Some(src) = &f.source else { continue };
        match files.iter_mut().find(|(p, _)| *p == src.path) {
            Some((_, names)) => names.push(name.clone()),
            None => files.push((src.path.clone(), vec![name.clone()])),
        }
    }
    files
        .into_iter()
        .map(|(path, families)| {
            let ids = placing(m, |e| matches!(e, Element::Component(c) if families.contains(&c.family)));
            let name = Path::new(&path).file_name().map_or_else(|| path.clone(), |s| s.to_string_lossy().into_owned());
            let mut row = json!({"name": name, "families": families, "instances": ids.len(), "ids": ids});
            let status = if located(&mut row, None, &path).is_file() { "loaded" } else { "not found" };
            row["status"] = json!(status);
            row
        })
        .collect()
}

/// The images placed in plans, or with `pdf` the PDFs' pages, one row for each picture and file: linked ones by their file,
/// imported ones by the picture the project holds.
fn images(m: &Model, project: Option<&Path>, pdf: bool) -> Vec<Value> {
    // A picture and the file it's linked to, if it is: the images placed from it.
    type Placed = ((String, Option<String>), Vec<Id>);
    let mut files: Vec<Placed> = Vec::new();
    for (id, e) in m.elements.iter() {
        let Element::Image(im) = e else { continue };
        if im.page.is_some() != pdf {
            continue;
        }
        let key = (im.picture.clone(), im.path.clone());
        match files.iter_mut().find(|(k, _)| *k == key) {
            Some((_, ids)) => ids.push(*id),
            None => files.push((key, vec![*id])),
        }
    }
    files
        .into_iter()
        .map(|((picture, path), ids)| {
            let label = m.images.get(&picture).map_or_else(|| picture.clone(), |p| p.name().to_string());
            let mut row = json!({"name": label, "picture": picture, "instances": ids.len(), "ids": ids, "path": null});
            let status = match &path {
                None => "imported",
                Some(path) if located(&mut row, project, path).is_file() => "loaded",
                Some(_) => "not found",
            };
            if let Some(path) = &path {
                row["name"] = json!(Path::new(path).file_name().map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned()));
            }
            row["status"] = json!(status);
            row
        })
        .collect()
}

/// The elements `places` says yes to, in id order.
fn placing(m: &Model, places: impl Fn(&Element) -> bool) -> Vec<Id> {
    m.elements.iter().filter(|(_, e)| places(e)).map(|(id, _)| *id).collect()
}
