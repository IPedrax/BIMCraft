//! Publishing sets, the Publisher's presets: what is written out together, again and again, at one press. A set is folders,
//! and a folder holds print sets ([`crate::printset`]), each with the format it is written as and that format's settings:
//! its file names ([`crate::export::naming`]), its page, its PDF. A print set may be in a folder more than once, each time
//! as another format. A set is a library entry ([`crate::library::Kind::PublishSet`]) that names its print sets (a file
//! holds their GUIDs, so a print set renamed is still the one meant); a print set deleted leaves every folder it was in
//! ([`Model::prune_publish_sets`]).
//!
//! [`run`] publishes one: an [`export::Job`] for each entry, in order, under the set's folder. Area:
//! [export](../../../docs/areas/export.md).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::export::{self, Ctx, Format, Item, Job, Options, Registry, Report, naming};
use crate::model::Model;

/// The most publishing sets a project holds.
pub const MAX_SETS: usize = 1000;
/// The most folders one set holds.
pub const MAX_FOLDERS: usize = 100;
/// The most print sets one folder holds.
pub const MAX_ENTRIES: usize = 200;
/// The longest name of a set or a folder, in characters.
pub const MAX_NAME: usize = 128;
/// The longest path of a set's destination, in characters.
pub const MAX_PATH: usize = 1024;

/// A print set in a folder, and what it is written as.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Entry {
    /// The print set, by name (its GUID in a file).
    pub print_set: String,
    /// The format's id: `pdf`, `dwg`, `ifc4`.
    pub format: String,
    /// The files' names, a template of tokens; none, each kind's own ([`export::default_name`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The name of the one file all the set's views go into, for a format that holds several (PDF); none, a file each.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub combine: Option<String>,
    /// The page, the PDF's settings and the other things the format's writer is asked.
    pub options: Options,
    /// The last complete export of this entry, used to estimate the next one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_export: Option<ExportTime>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportTime {
    pub started_ms: u64,
    pub ended_ms: u64,
    pub duration_ms: u64,
}

/// Timing of one completed publishing entry, identified by its folder, print set and format.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimedEntry {
    pub folder: String,
    pub print_set: String,
    pub format: String,
    pub time: ExportTime,
}

/// A folder of a publishing set: where its print sets' files go, under the set's destination.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Folder {
    /// Its name; a `/` puts it inside another (`Issue/PDF`), and none is the destination itself.
    pub name: String,
    pub entries: Vec<Entry>,
}

/// A publishing set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PublishSet {
    /// Where it is written: a folder, beside the project's file when it isn't absolute; none, asked for when it is published.
    pub folder: String,
    pub folders: Vec<Folder>,
    /// Whether a file already there is written over (a set published again replaces what it wrote); otherwise the new one
    /// is numbered.
    pub overwrite: bool,
}

impl Default for PublishSet {
    fn default() -> PublishSet {
        PublishSet { folder: String::new(), folders: Vec::new(), overwrite: true }
    }
}

/// The folders a folder's name makes, each safe to write; an error for one that climbs out or goes too deep.
fn parts(name: &str) -> Result<Vec<String>, String> {
    let parts: Vec<&str> = name.split(['/', '\\']).map(str::trim).filter(|p| !p.is_empty() && *p != ".").collect();
    if parts.contains(&"..") {
        return Err(format!("folder `{name}` can't climb out of where the set is written (`..`)"));
    }
    if parts.len() > naming::MAX_DEPTH {
        return Err(format!("folder `{name}` goes at most {} folders deep", naming::MAX_DEPTH));
    }
    Ok(parts.into_iter().map(naming::safe).collect())
}

impl Folder {
    /// Where it is, under the set's destination.
    pub fn path(&self) -> Result<PathBuf, String> {
        Ok(parts(&self.name)?.iter().collect())
    }
}

impl PublishSet {
    /// Save timings for entries still present in this set. Editing during an export may remove an entry.
    pub fn record_times(&mut self, timings: &[TimedEntry]) {
        for timed in timings {
            if let Some(entry) = self
                .folders
                .iter_mut()
                .find(|f| f.name == timed.folder)
                .and_then(|f| f.entries.iter_mut().find(|e| e.print_set == timed.print_set && e.format == timed.format))
            {
                entry.last_export = Some(timed.time.clone());
            }
        }
    }

    /// What is wrong with set `name`, if anything.
    pub fn check(&self, name: &str) -> Result<(), String> {
        if name.trim().is_empty() || name.chars().count() > MAX_NAME {
            return Err(format!("a publishing set's name is 1 to {MAX_NAME} characters, not `{name}`"));
        }
        if self.folder.chars().count() > MAX_PATH || self.folder.contains('\0') {
            return Err(format!("publishing set `{name}`: its destination is a path of up to {MAX_PATH} characters"));
        }
        if self.folders.len() > MAX_FOLDERS {
            return Err(format!("publishing set `{name}` holds {} folders; a set holds up to {MAX_FOLDERS}", self.folders.len()));
        }
        let mut folders = BTreeSet::new();
        for f in &self.folders {
            if f.name.chars().count() > MAX_NAME {
                return Err(format!("publishing set `{name}`: a folder's name is up to {MAX_NAME} characters"));
            }
            let path = parts(&f.name).map_err(|e| format!("publishing set `{name}`: {e}"))?;
            // Compared as Windows and macOS do, whatever their case.
            if !folders.insert(path.join("/").to_lowercase()) {
                return Err(format!("publishing set `{name}` has folder `{}` twice", f.name));
            }
            if f.entries.len() > MAX_ENTRIES {
                return Err(format!("publishing set `{name}`: folder `{}` holds up to {MAX_ENTRIES} print sets", f.name));
            }
            let mut seen = BTreeSet::new();
            for e in &f.entries {
                let said = format!("publishing set `{name}`, folder `{}`", f.name);
                if e.print_set.trim().is_empty() {
                    return Err(format!("{said}: an entry names no print set"));
                }
                let format = e.format.trim().to_ascii_lowercase();
                if format.is_empty() || format.chars().count() > 32 {
                    return Err(format!("{said}: print set `{}` names no format (`pdf`, `dwg`, `ifc4`…)", e.print_set));
                }
                if !seen.insert((e.print_set.clone(), format)) {
                    return Err(format!(
                        "{said}: print set `{}` is there as {} twice; a print set is in a folder once for each format",
                        e.print_set, e.format
                    ));
                }
                for template in [&e.name, &e.combine].into_iter().flatten() {
                    naming::check(template).map_err(|why| format!("{said}, print set `{}`: {why}", e.print_set))?;
                }
            }
        }
        Ok(())
    }

    /// How many print sets it holds, over all its folders.
    pub fn len(&self) -> usize {
        self.folders.iter().map(|f| f.entries.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Where it is written: `given` when there is one, else its own folder, beside the project's file (`project`) when it
    /// isn't absolute.
    pub fn root(&self, project: Option<&Path>, given: Option<&Path>) -> Result<PathBuf, String> {
        if let Some(given) = given.filter(|g| !g.as_os_str().is_empty()) {
            return Ok(given.to_path_buf());
        }
        let own = Path::new(self.folder.trim());
        if own.as_os_str().is_empty() {
            return Err("the publishing set has no destination: give `folder`, where its files go".to_string());
        }
        if own.is_absolute() {
            return Ok(own.to_path_buf());
        }
        let beside = project.and_then(Path::parent).ok_or_else(|| {
            format!("`{}` is beside the project's file, and the project isn't saved yet: save it, or give `folder`", self.folder)
        })?;
        Ok(beside.join(own))
    }
}

impl Model {
    /// Whether a publishing set names a print set the project no longer has.
    pub fn publish_sets_dangle(&self) -> bool {
        self.publish_sets.values().flat_map(|s| &s.folders).flat_map(|f| &f.entries).any(|e| !self.print_sets.contains_key(&e.print_set))
    }

    /// Takes out of every publishing set the print sets the project no longer has.
    pub fn prune_publish_sets(&mut self) {
        let Model { publish_sets, print_sets, .. } = self;
        for f in publish_sets.values_mut().flat_map(|s| &mut s.folders) {
            f.entries.retain(|e| print_sets.contains_key(&e.print_set));
        }
    }
}

/// The views of print set `name` that `format` can write, as a job's items.
pub fn items(m: &Model, name: &str, format: &dyn Format) -> Vec<Item> {
    let Some(set) = m.print_sets.get(name) else { return Vec::new() };
    set.views
        .iter()
        .filter(|g| {
            let kind = m.elements.id(**g).and_then(|id| export::kind_of(m, Some(id)));
            kind.is_some_and(|k| format.takes().iter().any(|t| k.gives().contains(t)))
        })
        .map(|g| Item { view: Some(*g), format: format.id().to_string() })
        .collect()
}

/// One entry of a set made ready: what it is of, and its job, or why it has none.
pub struct Planned {
    /// The folder's name and the print set's.
    pub folder: String,
    pub print_set: String,
    pub format: String,
    pub job: Result<Job, String>,
}

/// The jobs publishing set `name` is, one for each of its entries, in order, written under `root`.
pub fn plan(m: &Model, registry: &Registry, name: &str, root: &Path) -> Result<Vec<Planned>, String> {
    let set = m.publish_sets.get(name).ok_or_else(|| format!("there's no publishing set `{name}`"))?;
    let mut out = Vec::new();
    for f in &set.folders {
        let folder = root.join(f.path()?);
        for e in &f.entries {
            let job = registry.find(&e.format).map(Arc::clone).and_then(|format| {
                let items = items(m, &e.print_set, format.as_ref());
                if items.is_empty() {
                    return Err(format!("print set `{}` holds nothing {} writes", e.print_set, format.label()));
                }
                // One file of them all only where the format holds several.
                let combine = e.combine.clone().filter(|_| format.combines());
                Ok(Job {
                    items,
                    folder: folder.clone(),
                    name: e.name.clone(),
                    folders: export::Folders::Flat,
                    combine,
                    overwrite: set.overwrite,
                    options: e.options.clone(),
                    tokens: [("PrintSet".to_string(), e.print_set.clone()), ("PublishSet".to_string(), name.to_string())].into(),
                })
            });
            out.push(Planned { folder: f.name.clone(), print_set: e.print_set.clone(), format: e.format.clone(), job });
        }
    }
    Ok(out)
}

/// Publishes set `name` under `root`: each entry's files, in order. An entry that can't be written is reported and the rest
/// go on; it fails only when it can't start (no such set, nothing in it).
pub fn run(model: &Arc<Model>, registry: &Registry, name: &str, root: &Path, ctx: &Ctx) -> Result<Report, String> {
    let planned = plan(model, registry, name, root)?;
    if planned.is_empty() {
        return Err(format!("publishing set `{name}` holds no print sets: there is nothing to publish"));
    }
    let total: usize = planned.iter().map(|p| p.job.as_ref().map_or(1, |j| j.items.len())).sum();
    let mut report = Report::default();
    let mut done = 0;
    for p in planned {
        if (ctx.cancelled)() {
            report.cancelled = true;
            break;
        }
        let failed = |error: String| export::Failed { of: p.print_set.clone(), format: p.format.clone(), error };
        let job = match p.job {
            Ok(job) => job,
            Err(error) => {
                report.failed.push(failed(error));
                done += 1;
                continue;
            }
        };
        let steps = job.items.len();
        // The job counts its own steps: here they are steps of the whole set.
        let progress = |at: usize, _: usize, what: &str| (ctx.progress)(done + at.min(steps), total, what);
        let inner = Ctx { cancelled: ctx.cancelled, progress: &progress };
        let started_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        let start = Instant::now();
        match export::run(model, registry, &job, &inner) {
            Ok(r) => {
                if !r.cancelled && r.failed.is_empty() {
                    let ended_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(started_ms, |d| d.as_millis() as u64);
                    report.published.push(TimedEntry {
                        folder: p.folder.clone(),
                        print_set: p.print_set.clone(),
                        format: p.format.clone(),
                        time: ExportTime { started_ms, ended_ms, duration_ms: start.elapsed().as_millis() as u64 },
                    });
                }
                report.written.extend(r.written);
                report.failed.extend(r.failed);
                report.cancelled |= r.cancelled;
            }
            Err(error) => report.failed.push(failed(error)),
        }
        done += steps;
        if report.cancelled {
            break;
        }
    }
    (ctx.progress)(done, total, "");
    Ok(report)
}
