//! The export engine: every file the project is written out as, other than its own, goes through here.
//!
//! An export is of a *source* (a view, a sheet, a schedule, or the whole project) to a *format*. A format says which kind of
//! content it takes ([`Takes`]): a drawing on pages (PDF, pictures, SVG, DXF), a table (CSV, XLSX), or the model (IFC, OBJ,
//! glTF). The engine makes that content of the source once ([`prepare`]) and hands it to the format's writer, which knows
//! nothing of views. A new file type is a new [`Format`] in [`formats`] (or one an app registers, as the DWG writer is,
//! from a crate the core may not depend on), and every front end gets it: the `export` command, the desktop's Export
//! dialog and its Publisher.
//!
//! A [`Job`] is many exports with a folder, a file name template ([`naming`]) and the page setup ([`page`]); [`run`] works
//! through it on whatever thread calls it, asking [`Ctx::cancelled`] between steps and telling [`Ctx::progress`] where it is,
//! and writes each file whole or not at all. Area: [export](../../../docs/areas/export.md).

pub mod formats;
pub mod naming;
pub mod page;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::geom::P2;
use crate::ids::Guid;
use crate::model::{Element, Id, Model};
use crate::render::Drawing;

pub use formats::builtin;

/// The groups the formats are listed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    /// Documents of pages: PDF.
    Document,
    /// Drawings kept as lines: SVG.
    Vector,
    /// Pictures: PNG, JPEG, JPEG XL.
    Raster,
    /// Line drawings for CAD: DXF, DWG.
    Line,
    /// The model in 3D: IFC, OBJ, STL, glTF, U3D.
    Model,
    /// Tables: CSV, XLSX, ODS.
    Schedule,
    /// Coordination with others: BCF.
    Coordination,
}

/// How far along a format is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Stable,
    /// It writes files, but they have not been proven in the programs that read them.
    Experimental,
    /// Listed, not written yet: an export to it says so.
    Planned,
}

/// The kinds of content a format may take, in the order it prefers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Takes {
    /// A drawing laid on pages (a sheet, a plan, a section, a 3D view's picture, a schedule's table drawn).
    Drawing,
    /// A table's rows (a schedule).
    Table,
    /// The model, or what a 3D view shows of it.
    Model,
}

/// What kind of thing an export is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Plan,
    Section,
    Elevation,
    View3D,
    Sheet,
    Schedule,
    /// The whole project.
    Project,
}

impl Kind {
    /// What a project calls it, in English: `{ViewType}` in a file name.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Plan => "Floor Plan",
            Kind::Section => "Section",
            Kind::Elevation => "Elevation",
            Kind::View3D => "3D View",
            Kind::Sheet => "Sheet",
            Kind::Schedule => "Schedule",
            Kind::Project => "Project",
        }
    }

    /// What content it can be made into.
    pub fn gives(self) -> &'static [Takes] {
        match self {
            Kind::Plan | Kind::Section | Kind::Elevation | Kind::Sheet => &[Takes::Drawing],
            Kind::View3D => &[Takes::Drawing, Takes::Model],
            Kind::Schedule => &[Takes::Table, Takes::Drawing],
            Kind::Project => &[Takes::Model],
        }
    }
}

/// What kind of export element `id` would be, if it is something that exports.
pub fn kind_of(m: &Model, id: Option<Id>) -> Option<Kind> {
    let Some(id) = id else { return Some(Kind::Project) };
    Some(match m.elements.get(id)? {
        Element::Level(_) | Element::PlanView(_) => Kind::Plan,
        Element::Section(_) => Kind::Section,
        Element::Elevation(_) => Kind::Elevation,
        Element::View3D(_) => Kind::View3D,
        Element::Sheet(_) => Kind::Sheet,
        Element::Schedule(_) => Kind::Schedule,
        _ => return None,
    })
}

/// A drawing ready to lay on pages: drawn at 1 : `n` on paper at [`crate::sheet::DENSITY`] px to the mm (its view's
/// `scale`), so a pixel is `n / scale` of its own mm; a sheet at 1 : 1 on its own paper.
pub struct Paged {
    pub drawing: Drawing,
    pub n: f64,
    pub own: Option<P2>,
}

impl Paged {
    /// Its size in its own mm (a plan's, the model's).
    pub fn extent(&self) -> P2 {
        let v = self.drawing.view;
        P2(f64::from(v.width) / v.scale * self.n, f64::from(v.height) / v.scale * self.n)
    }

    /// How it lies on the pages `setup` asks for.
    pub fn layout(&self, setup: &page::PageSetup) -> Result<page::Layout, String> {
        page::layout(setup, self.extent(), self.n, self.own)
    }

    /// Where a page of `layout` puts it: paper mm to its pixel, its pixel (0, 0) from the page's top left corner, and the
    /// printable area it is cut to.
    pub fn place(&self, layout: &page::Layout, tile: &page::Tile) -> crate::pdf::Place {
        let unit = self.n / self.drawing.view.scale;
        let m = layout.margin;
        crate::pdf::Place {
            k: layout.factor * unit,
            at: P2(m - tile.at.0 * layout.factor, m - tile.at.1 * layout.factor),
            clip: Some([m, m, layout.paper.0 - 2.0 * m, layout.paper.1 - 2.0 * m]),
        }
    }
}

/// What a writer is given, made of a source.
pub enum Content {
    /// Boxed: a drawing's display list is far bigger than the others.
    Drawing(Box<Paged>),
    Table(crate::schedule::Table),
    /// The model as its links make it, and the 3D view it is seen through, if it is one's.
    Model {
        model: Arc<Model>,
        view: Option<Id>,
    },
}

/// One source made ready for a writer: its content, its name, and the kind it is.
pub struct Prepared {
    pub title: String,
    pub kind: Kind,
    pub content: Content,
}

/// What a writer makes: a file's bytes, and the words put after its name when one source makes several (a page's number).
#[derive(Debug)]
pub struct Out {
    pub suffix: Option<String>,
    pub bytes: Vec<u8>,
}

/// What the writers are asked to do besides the page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub page: page::PageSetup,
    /// A JPEG's quality, 1 to 100; none, 90.
    pub quality: Option<u8>,
}

/// What a running export is told and asked.
pub struct Ctx<'a> {
    /// Whether to stop: asked between files and pages, so it stops soon after.
    pub cancelled: &'a (dyn Fn() -> bool + Sync),
    /// Where it is: steps done of the steps there are, and what it is on now.
    pub progress: &'a (dyn Fn(usize, usize, &str) + Sync),
}

impl Ctx<'static> {
    /// One that is never stopped and tells no one.
    pub fn quiet() -> Ctx<'static> {
        Ctx { cancelled: &|| false, progress: &|_, _, _| {} }
    }
}

/// A file type the project can be exported as.
pub trait Format: Send + Sync {
    /// Its id, which commands and saved export sets name it by: `pdf`, `ifc4`, `dxf`.
    fn id(&self) -> &'static str;
    /// What the dialog lists it as.
    fn label(&self) -> &'static str;
    /// The extension of the files it writes, without the dot.
    fn extension(&self) -> &'static str;
    fn family(&self) -> Family;
    fn status(&self) -> Status {
        Status::Stable
    }
    /// The content it takes, in the order it prefers.
    fn takes(&self) -> &'static [Takes];
    /// Whether it can put several sources in one file (a PDF's pages); otherwise each is a file of its own.
    fn combines(&self) -> bool {
        false
    }
    /// Whether it lays drawings on pages, so the page setup applies.
    fn paged(&self) -> bool {
        false
    }
    /// The files of `items`, one or more (a picture a page). `items` is one, unless the format [`combines`](Format::combines).
    fn write(&self, items: &[Prepared], options: &Options, ctx: &Ctx) -> Result<Vec<Out>, String>;
}

/// The formats a session can export to: the built-in ones, and any an app registers.
#[derive(Clone)]
pub struct Registry {
    formats: Vec<Arc<dyn Format>>,
}

impl Default for Registry {
    fn default() -> Registry {
        Registry { formats: builtin() }
    }
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.formats.iter().map(|x| x.id())).finish()
    }
}

impl Registry {
    /// `format` added, or put in the place of the one of its id.
    pub fn register(&mut self, format: Arc<dyn Format>) {
        match self.formats.iter_mut().find(|f| f.id() == format.id()) {
            Some(slot) => *slot = format,
            None => self.formats.push(format),
        }
    }

    pub fn all(&self) -> &[Arc<dyn Format>] {
        &self.formats
    }

    /// The format of id `name`, or the first ready one that writes files with that extension (`ifc` is IFC4), whatever the
    /// case and with or without its dot.
    pub fn find(&self, name: &str) -> Result<&Arc<dyn Format>, String> {
        let name = name.trim().trim_start_matches('.').to_ascii_lowercase();
        let ready = |f: &&Arc<dyn Format>| f.status() != Status::Planned;
        self.formats
            .iter()
            .find(|f| f.id() == name)
            .or_else(|| self.formats.iter().filter(ready).find(|f| f.extension() == name))
            .or_else(|| self.formats.iter().find(|f| f.extension() == name))
            .ok_or_else(|| {
                let ids: Vec<&str> = self.formats.iter().map(|f| f.id()).collect();
                format!("`{name}` isn't a format this exports to; there are: {}", ids.join(", "))
            })
    }
}

/// What a source makes for a format that takes `takes`: its drawing, table or model. `project` is the project itself, what a
/// model's writer is given (a writer that wants its links' elements too asks `link::seen`); `m` is it as its links make it,
/// what its views draw.
pub fn prepare(project: &Arc<Model>, m: &Arc<Model>, id: Option<Id>, takes: Takes) -> Result<Prepared, String> {
    let kind = kind_of(m, id).ok_or_else(|| match id {
        Some(id) => format!("element {id} isn't a view, a sheet or a schedule to export"),
        None => "nothing to export".to_string(),
    })?;
    let title = title(m, id, kind);
    let content = match (takes, id) {
        (Takes::Model, _) => Content::Model { model: project.clone(), view: id.filter(|_| kind == Kind::View3D) },
        (Takes::Table, Some(id)) => match m.elements.get(id) {
            Some(Element::Schedule(sc)) => Content::Table(crate::schedule::table(m, sc)?),
            _ => return Err(format!("{title} has no table to write")),
        },
        (Takes::Drawing, Some(id)) if kind == Kind::Sheet => {
            let own = m.sheet(id).and_then(|s| crate::model::paper(&s.size));
            let drawing = crate::sheet::draw(m, id).ok_or_else(|| format!("sheet {title} can't be drawn"))?;
            Content::Drawing(Box::new(Paged { drawing, n: 1.0, own }))
        }
        (Takes::Drawing, Some(id)) => {
            // A schedule keeps its own size; a view is drawn at its scale.
            let n = if kind == Kind::Schedule { 1.0 } else { crate::graphics::scale_of(m, id) };
            let n = if n.is_finite() && n > 0.0 { n } else { 100.0 };
            let drawing = crate::sheet::draw_view(m, id, n).ok_or_else(|| format!("{title} has nothing to draw"))?;
            Content::Drawing(Box::new(Paged { drawing, n, own: None }))
        }
        (_, None) => return Err("the whole project exports only as a model (IFC, glTF, OBJ, STL)".to_string()),
    };
    Ok(Prepared { title, kind, content })
}

/// What a source is called: a sheet's number and name, a view's name, the project's.
pub fn title(m: &Model, id: Option<Id>, kind: Kind) -> String {
    match (id, kind) {
        (Some(id), Kind::Sheet) => m.sheet(id).map(|s| format!("{} - {}", s.number, s.name)).unwrap_or_default(),
        (Some(id), _) => m.view_name(id).unwrap_or_default().to_string(),
        (None, _) => m.name.clone(),
    }
}

/// What a file name template's tokens stand for, for one source of a project.
pub struct ModelTokens<'a> {
    pub model: &'a Model,
    pub id: Option<Id>,
    pub format: &'a str,
    /// Seconds since 1970, UTC.
    pub now: u64,
}

impl ModelTokens<'_> {
    pub const NAMES: [&'static str; 10] =
        ["ProjectName", "ViewName", "ViewType", "SheetNumber", "SheetName", "Level", "Scale", "Format", "Date", "Time"];
}

impl naming::Tokens for ModelTokens<'_> {
    fn token(&self, name: &str) -> Option<String> {
        let m = self.model;
        let kind = kind_of(m, self.id);
        let sheet = self.id.and_then(|id| m.sheet(id));
        let (date, time) = naming::date_and_time(self.now);
        Some(match name {
            "ProjectName" => m.name.clone(),
            "ViewName" => kind.map(|k| title(m, self.id, k)).unwrap_or_default(),
            "ViewType" => kind.map(|k| k.name().to_string()).unwrap_or_default(),
            "SheetNumber" => sheet.map(|s| s.number.clone()).unwrap_or_default(),
            "SheetName" => sheet.map(|s| s.name.clone()).unwrap_or_default(),
            "Level" => self
                .id
                .and_then(|id| match m.elements.get(id) {
                    Some(Element::Level(l)) => Some(l.name.clone()),
                    Some(Element::PlanView(v)) => m.level(v.level).map(|l| l.name.clone()),
                    _ => None,
                })
                .unwrap_or_default(),
            "Scale" => match kind {
                Some(Kind::Plan | Kind::Section | Kind::Elevation | Kind::View3D) => {
                    self.id.map(|id| format!("1-{}", crate::graphics::scale_of(m, id).round())).unwrap_or_default()
                }
                _ => String::new(),
            },
            "Format" => self.format.to_string(),
            "Date" => date,
            "Time" => time,
            // Project parameters, by name, will answer here.
            _ => return None,
        })
    }

    fn names(&self) -> Vec<String> {
        Self::NAMES.iter().map(|n| n.to_string()).collect()
    }
}

/// How the files of a job are put in folders under its folder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Folders {
    /// All in the folder, as the template names them.
    #[default]
    Flat,
    /// A folder for each format (`PDF/`, `DWG/`).
    ByFormat,
}

/// One export of a job: a source (an element's id; none, the whole project) to a format (its id or extension).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    #[serde(default)]
    pub view: Option<Guid>,
    pub format: String,
}

/// Many exports at once.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Job {
    pub items: Vec<Item>,
    /// Where the files go; it is made if it isn't there.
    pub folder: PathBuf,
    /// The files' names ([`naming`]); none, `{SheetNumber} - {SheetName}` for a sheet, `{ViewName}` for a view and
    /// `{ProjectName}` for the project.
    pub name: Option<String>,
    pub folders: Folders,
    /// The name of the one file all of a format's sources go into, for formats that can hold several (PDF); none, a file
    /// each.
    pub combine: Option<String>,
    /// Whether a file already there is written over; otherwise the new one is numbered.
    pub overwrite: bool,
    pub options: Options,
}

/// A file a job wrote.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Written {
    pub path: PathBuf,
    pub format: String,
    /// What it is of.
    pub of: Vec<String>,
}

/// An export of a job that wrote nothing, and why.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Failed {
    pub of: String,
    pub format: String,
    pub error: String,
}

/// What a job did.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Report {
    pub written: Vec<Written>,
    pub failed: Vec<Failed>,
    /// Whether it was stopped before the end; what it wrote by then is whole and kept.
    pub cancelled: bool,
}

/// The default file name template of a source of `kind`.
pub fn default_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Sheet => "{SheetNumber} - {SheetName}",
        Kind::Project => "{ProjectName}",
        _ => "{ViewName}",
    }
}

/// `bytes` written to `path` whole or not at all: to a file beside it first, then put in its place.
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write as _;
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    staged.write_all(bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    staged.persist(path).map_err(|e| format!("{}: {}", path.display(), e.error))?;
    Ok(())
}

/// The job's exports, written under its folder. A source that can't be exported is reported and the rest go on; the job
/// fails only when it can't start (no folder, an unknown format, a bad template).
/// One format and the sources it writes, together where it combines them.
type Group = (Arc<dyn Format>, Vec<Option<Id>>);

pub fn run(model: &Arc<Model>, registry: &Registry, job: &Job, ctx: &Ctx) -> Result<Report, String> {
    if job.items.is_empty() {
        return Err("there is nothing to export: `items` is empty".to_string());
    }
    if job.folder.as_os_str().is_empty() {
        return Err("`folder` is required: where the files go".to_string());
    }
    std::fs::create_dir_all(&job.folder).map_err(|e| format!("{}: {e}", job.folder.display()))?;
    let m = crate::link::seen(model);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    // Each item as a format and a source, checked before anything is written.
    let mut work: Vec<(Arc<dyn Format>, Option<Id>)> = Vec::new();
    for item in &job.items {
        let format = registry.find(&item.format)?.clone();
        let id = match item.view {
            Some(g) => Some(m.elements.id(g).ok_or_else(|| format!("there's no element {g} to export"))?),
            None => None,
        };
        work.push((format, id));
    }
    let mut report = Report::default();
    let mut taken = BTreeSet::new();
    // Combined: each format that can, its sources together, in the order given.
    let mut groups: Vec<Group> = Vec::new();
    for (format, id) in work {
        match groups.iter_mut().find(|(f, _)| job.combine.is_some() && f.combines() && f.id() == format.id()) {
            Some((_, ids)) => ids.push(id),
            None => groups.push((format, vec![id])),
        }
    }
    let total = groups.iter().map(|(_, ids)| ids.len()).sum::<usize>();
    let mut done = 0;
    for (format, ids) in groups {
        if (ctx.cancelled)() {
            report.cancelled = true;
            break;
        }
        let combined = ids.len() > 1;
        let mut prepared = Vec::new();
        for id in &ids {
            let what = kind_of(&m, *id).map_or_else(String::new, |k| title(&m, *id, k));
            (ctx.progress)(done, total, &format!("{what} ({})", format.label()));
            if (ctx.cancelled)() {
                report.cancelled = true;
                break;
            }
            match prepare_for(model, &m, *id, format.as_ref()) {
                Ok(p) => prepared.push(p),
                Err(error) => report.failed.push(Failed { of: what, format: format.id().to_string(), error }),
            }
            done += 1;
        }
        if report.cancelled {
            break;
        }
        // Combined, one at a time otherwise.
        let batches: Vec<Vec<Prepared>> = if combined { vec![prepared] } else { prepared.into_iter().map(|p| vec![p]).collect() };
        for batch in batches.into_iter().filter(|b| !b.is_empty()) {
            if (ctx.cancelled)() {
                report.cancelled = true;
                break;
            }
            let of: Vec<String> = batch.iter().map(|p| p.title.clone()).collect();
            let template = match (&job.combine, combined, &job.name) {
                (Some(name), true, _) => name.clone(),
                (_, _, Some(name)) => name.clone(),
                (..) => default_name(batch.first().map_or(Kind::Project, |p| p.kind)).to_string(),
            };
            // The source of a lone one names it; a combined file's are many, so only the project's tokens mean anything.
            let id = if combined { None } else { batch_id(&m, &batch, &ids) };
            let tokens = ModelTokens { model: &m, id, format: format.label(), now };
            let mut path = naming::expand(&template, &tokens, format.extension())?;
            if job.folders == Folders::ByFormat {
                path = Path::new(&naming::safe(format.label())).join(path);
            }
            let outs = match format.write(&batch, &job.options, ctx) {
                Ok(outs) => outs,
                Err(error) => {
                    if (ctx.cancelled)() {
                        report.cancelled = true;
                        break;
                    }
                    report.failed.push(Failed { of: of.join(", "), format: format.id().to_string(), error });
                    continue;
                }
            };
            for out in outs {
                let path = match &out.suffix {
                    Some(suffix) => {
                        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                        path.with_file_name(format!("{stem} - {}.{}", naming::safe(suffix), format.extension()))
                    }
                    None => path.clone(),
                };
                let path = if job.overwrite {
                    taken.insert(path.to_string_lossy().to_lowercase());
                    path
                } else {
                    naming::unique(&job.folder, path, &mut taken)
                };
                let full = job.folder.join(&path);
                match write_file(&full, &out.bytes) {
                    Ok(()) => report.written.push(Written { path: full, format: format.id().to_string(), of: of.clone() }),
                    Err(error) => report.failed.push(Failed { of: of.join(", "), format: format.id().to_string(), error }),
                }
            }
        }
        if report.cancelled {
            break;
        }
    }
    (ctx.progress)(done, total, "");
    Ok(report)
}

/// The source a batch of one was prepared from, found by its title among the group's (a group that isn't combined is
/// prepared in order, skipping what failed).
fn batch_id(m: &Model, batch: &[Prepared], ids: &[Option<Id>]) -> Option<Id> {
    let p = batch.first()?;
    ids.iter().copied().find(|id| kind_of(m, *id) == Some(p.kind) && title(m, *id, p.kind) == p.title).flatten()
}

/// [`prepare`] in the first kind of content `format` takes that the source gives.
pub fn prepare_for(project: &Arc<Model>, m: &Arc<Model>, id: Option<Id>, format: &dyn Format) -> Result<Prepared, String> {
    if format.status() == Status::Planned {
        return Err(format!("{} export isn't written yet", format.label()));
    }
    let kind = kind_of(m, id).ok_or_else(|| format!("element {} isn't a view, a sheet or a schedule to export", id.unwrap_or(0)))?;
    let takes = format.takes().iter().find(|t| kind.gives().contains(t)).ok_or_else(|| {
        format!("{} can't be written as {}: it takes {}", kind.name().to_lowercase(), format.label(), describe(format.takes()))
    })?;
    prepare(project, m, id, *takes)
}

fn describe(takes: &[Takes]) -> String {
    let words: Vec<&str> = takes
        .iter()
        .map(|t| match t {
            Takes::Drawing => "views and sheets",
            Takes::Table => "schedules",
            Takes::Model => "the model or a 3D view",
        })
        .collect();
    words.join(" or ")
}
