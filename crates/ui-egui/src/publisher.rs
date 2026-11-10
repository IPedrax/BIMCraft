//! The Publisher (File > Export > Publisher…): the project's publishing sets on the left, and the one picked on the right,
//! its destination and its folders, each holding print sets with the format each is written as. A print set is in a folder
//! once for each format. Publish writes the set on the export's thread ([`crate::export::worker`]), with its bar and Cancel.
//! Every change is a `publish_set` command; print sets are made and changed in the Print Sets window, opened from here.
//! An entry's file names are a template of tokens, put together in File Names…; its format's settings (the page, a PDF's)
//! are the Export dialog's form.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use bimcraft_core::export::{self, Family, Format, ModelTokens, Options, Registry, Status as Ready, naming};
use bimcraft_core::model::Model;
use bimcraft_core::publish::{self, Entry, Folder, PublishSet};
use egui::{Align, ComboBox, Context, Layout, RichText, ScrollArea, TextEdit, Ui};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::BimApp;
use crate::export::{family_name, format_name, format_options, pdf_settings};
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::pal;

/// How wide the list of sets is.
const SETS: f32 = 200.0;
/// How tall the window's two sides are.
const HEIGHT: f32 = 380.0;

enum TransferChoice {
    Import,
    Export,
}

#[derive(Serialize, Deserialize)]
#[serde(rename = "BIMCraftPublisher")]
struct Transfer {
    #[serde(rename = "@version")]
    version: u32,
    #[serde(rename = "PrintSet", default)]
    print_sets: Vec<TransferPrintSet>,
    #[serde(rename = "PublishingSet", default)]
    publishing_sets: Vec<TransferPublishingSet>,
}

#[derive(Serialize, Deserialize)]
struct TransferPrintSet {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "View", default)]
    views: Vec<TransferView>,
}

#[derive(Serialize, Deserialize)]
struct TransferView {
    #[serde(rename = "@guid")]
    guid: String,
    #[serde(rename = "@title")]
    title: String,
}

#[derive(Serialize, Deserialize)]
struct TransferPublishingSet {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "$text")]
    definition: String,
}

pub(crate) fn transfer_xml(model: &Model) -> Result<String, String> {
    let referenced: BTreeSet<&str> =
        model.publish_sets.values().flat_map(|s| &s.folders).flat_map(|f| &f.entries).map(|e| e.print_set.as_str()).collect();
    let print_sets = model
        .print_sets
        .iter()
        .filter(|(name, _)| referenced.contains(name.as_str()))
        .map(|(name, set)| TransferPrintSet {
            name: name.clone(),
            views: set
                .views
                .iter()
                .filter_map(|g| {
                    let id = model.elements.id(*g)?;
                    let kind = export::kind_of(model, Some(id))?;
                    Some(TransferView { guid: g.to_string(), title: export::title(model, Some(id), kind) })
                })
                .collect(),
        })
        .collect();
    let publishing_sets = model
        .publish_sets
        .iter()
        .map(|(name, set)| {
            Ok(TransferPublishingSet { name: name.clone(), definition: serde_json::to_string(set).map_err(|e| e.to_string())? })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let xml = quick_xml::se::to_string(&Transfer { version: 1, print_sets, publishing_sets }).map_err(|e| e.to_string())?;
    Ok(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{xml}\n"))
}

fn free_import_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_string();
    }
    (2..).map(|n| format!("{name} ({n})")).find(|n| !taken(n)).unwrap_or_else(|| name.to_string())
}

/// What a name being typed is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Naming {
    New,
    Duplicate,
    Rename,
}

/// An entry's file names, being put together.
pub(crate) struct Rule {
    /// The folder and the entry, by place.
    pub(crate) at: (usize, usize),
    pub(crate) template: String,
    /// What goes between two tokens picked one after the other.
    pub(crate) separator: String,
    /// All the set's views in one file, and that file's name.
    pub(crate) combine: bool,
    pub(crate) combined: String,
    find: String,
}

/// An entry's format settings, being changed.
pub(crate) struct Settings {
    pub(crate) at: (usize, usize),
    /// What they were, for Cancel.
    was: Options,
    ratio: f64,
    list: String,
}

/// The Publisher, while it is open.
#[derive(Default)]
pub(crate) struct Dialog {
    /// The set shown, by name.
    pub(crate) current: Option<String>,
    /// The set as it is being changed, and as it was when it was shown.
    pub(crate) draft: PublishSet,
    loaded: PublishSet,
    pub(crate) naming: Option<(Naming, String)>,
    pub(crate) error: Option<String>,
    pub(crate) rule: Option<Rule>,
    pub(crate) settings: Option<Settings>,
    /// A row waiting for the print set being made in the Print Sets window, and the print sets there were before.
    awaiting: Option<((usize, usize), BTreeSet<String>)>,
}

impl Dialog {
    /// Shows set `name` as it is saved; none, nothing.
    fn show_set(&mut self, m: &Model, name: Option<String>) {
        self.draft = name.as_ref().and_then(|n| m.publish_sets.get(n)).cloned().unwrap_or_default();
        self.loaded = self.draft.clone();
        self.current = name;
        (self.rule, self.settings, self.awaiting) = (None, None, None);
    }

    /// Whether the set shown was changed and not saved.
    pub(crate) fn changed(&self) -> bool {
        self.current.is_some() && self.draft != self.loaded
    }

    fn entry_mut(&mut self, at: (usize, usize)) -> Option<&mut Entry> {
        self.draft.folders.get_mut(at.0).and_then(|f| f.entries.get_mut(at.1))
    }
}

/// The formats a publishing set may write: those written already, by family.
fn formats(registry: &Registry) -> BTreeMap<Family, Vec<&Arc<dyn Format>>> {
    let mut families: BTreeMap<Family, Vec<&Arc<dyn Format>>> = BTreeMap::new();
    for f in registry.all().iter().filter(|f| f.status() != Ready::Planned) {
        families.entry(f.family()).or_default().push(f);
    }
    families
}

/// The first entry a folder can still take: a print set and a format it doesn't hold together yet, PDF before the others.
fn next_entry(m: &Model, registry: &Registry, folder: &Folder) -> Option<Entry> {
    let held = |set: &str, format: &str| folder.entries.iter().any(|e| e.print_set == set && e.format == format);
    let mut ids: Vec<&str> = registry.all().iter().filter(|f| f.status() != Ready::Planned).map(|f| f.id()).collect();
    ids.sort_by_key(|id| *id != "pdf");
    m.print_sets.keys().find_map(|set| {
        let format = ids.iter().find(|id| {
            !held(set, id)
                && registry.find(id).is_ok_and(|f| !publish::items(m, set, f.as_ref()).is_empty() || m.print_sets[set].views.is_empty())
        })?;
        Some(Entry { print_set: set.clone(), format: format.to_string(), ..Entry::default() })
    })
}

/// How many files an entry writes: one of them all, or one for each view its format takes.
fn files(m: &Model, registry: &Registry, e: &Entry) -> usize {
    let Ok(format) = registry.find(&e.format) else { return 0 };
    let n = publish::items(m, &e.print_set, format.as_ref()).len();
    if e.combine.is_some() && format.combines() { n.min(1) } else { n }
}

/// What `template` makes of the first view of print set `set` that `format` writes: a sample of its files' names.
fn sample(m: &Model, format: &dyn Format, set: &str, publishing: &str, template: &str, combined: bool) -> Result<String, String> {
    let first = publish::items(m, set, format).first().and_then(|i| i.view).and_then(|g| m.elements.id(g));
    let extra: BTreeMap<String, String> =
        [("PrintSet".to_string(), set.to_string()), ("PublishSet".to_string(), publishing.to_string())].into();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let kind = export::kind_of(m, first).unwrap_or(export::Kind::Sheet);
    let template = if template.trim().is_empty() { export::default_name(kind) } else { template };
    // One file of them all is no view's: only the project's tokens mean anything there.
    let tokens = ModelTokens { model: m, id: first.filter(|_| !combined), format: format.label(), now, extra: &extra };
    naming::expand(template, &tokens, format.extension()).map(|p| p.to_string_lossy().replace('\\', "/"))
}

/// `template` with `token` put on its end, `separator` between it and what was there.
pub(crate) fn with_token(template: &str, token: &str, separator: &str) -> String {
    let had = template.trim_end();
    let between = if had.is_empty() || had.ends_with('/') || had.ends_with(separator.trim_end()) && !separator.trim().is_empty() {
        ""
    } else {
        separator
    };
    format!("{had}{between}{{{token}}}")
}

impl BimApp {
    /// Opens the Publisher on the project's first publishing set, or none.
    pub(crate) fn open_publisher(&mut self) {
        let m = &self.session.model;
        let mut d = Dialog::default();
        d.show_set(m, m.publish_sets.keys().next().cloned());
        self.publisher = Some(d);
    }

    /// The set shown, saved as it is now; whether it was.
    fn save_publish_set(&mut self, d: &mut Dialog) -> bool {
        let Some(name) = d.current.clone() else { return false };
        if let Some(saved) = self.session.model.publish_sets.get(&name) {
            for folder in &mut d.draft.folders {
                for entry in &mut folder.entries {
                    let latest = saved
                        .folders
                        .iter()
                        .find(|f| f.name == folder.name)
                        .and_then(|f| f.entries.iter().find(|e| e.print_set == entry.print_set && e.format == entry.format))
                        .and_then(|e| e.last_export.as_ref());
                    if let Some(latest) = latest
                        && entry.last_export.as_ref().is_none_or(|old| old.ended_ms < latest.ended_ms)
                    {
                        entry.last_export = Some(latest.clone());
                    }
                }
            }
        }
        let params = json!({"name": name, "folder": d.draft.folder, "folders": d.draft.folders, "overwrite": d.draft.overwrite});
        if self.run("publish_set", params).is_none() {
            d.error = Some(std::mem::take(&mut self.status).text);
            return false;
        }
        d.error = None;
        d.show_set(&self.session.model, Some(name));
        true
    }

    /// Publishes set `name` under `root`, on the export's thread.
    pub(crate) fn publish_to(&mut self, name: &str, root: PathBuf) -> bool {
        self.start_publish(name.to_string(), root)
    }

    pub(crate) fn import_publisher_xml(&mut self, path: &std::path::Path) -> Result<(Option<String>, usize), String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if bytes.len() > 10 * 1024 * 1024 {
            return Err("The Publisher XML file is too large".to_string());
        }
        let xml = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
        let transfer: Transfer = quick_xml::de::from_str(xml).map_err(|e| format!("Invalid Publisher XML: {e}"))?;
        if transfer.version != 1 {
            return Err(format!("Unsupported Publisher XML version {}", transfer.version));
        }
        if transfer.publishing_sets.is_empty() {
            return Err("The Publisher XML has no publishing sets".to_string());
        }
        let mut names = BTreeSet::new();
        for set in &transfer.print_sets {
            if !names.insert(set.name.clone()) {
                return Err(format!("Print set `{}` is in the XML twice", set.name));
            }
        }
        names.clear();
        for set in &transfer.publishing_sets {
            if !names.insert(set.name.clone()) {
                return Err(format!("Publishing set `{}` is in the XML twice", set.name));
            }
        }
        let model = &self.session.model;
        let mut print_names = BTreeMap::<String, String>::new();
        let mut taken: BTreeSet<String> = model.print_sets.keys().cloned().collect();
        let mut print_commands = Vec::<Value>::new();
        let mut missing = 0;
        for set in &transfer.print_sets {
            let mut views = Vec::new();
            for view in &set.views {
                let by_guid = bimcraft_core::ids::Guid::parse(&view.guid).and_then(|g| model.elements.id(g));
                let by_title = || {
                    let mut matches = model.elements.iter().filter_map(|(id, _)| {
                        let kind = export::kind_of(model, Some(*id))?;
                        (export::title(model, Some(*id), kind) == view.title).then_some(*id)
                    });
                    let first = matches.next()?;
                    matches.next().is_none().then_some(first)
                };
                if let Some(id) = by_guid.or_else(by_title) {
                    if !views.contains(&id) {
                        views.push(id);
                    }
                } else {
                    missing += 1;
                }
            }
            let existing = model
                .print_sets
                .get(&set.name)
                .is_some_and(|was| was.views.iter().filter_map(|g| model.elements.id(*g)).collect::<Vec<_>>() == views);
            let name = if existing { set.name.clone() } else { free_import_name(&set.name, |n| taken.contains(n)) };
            if !existing {
                taken.insert(name.clone());
                print_commands.push(json!({"name": name, "views": views}));
            }
            print_names.insert(set.name.clone(), name);
        }
        let mut publish_commands = Vec::<Value>::new();
        taken = model.publish_sets.keys().cloned().collect();
        let mut selected = None;
        for source in &transfer.publishing_sets {
            let mut set: PublishSet =
                serde_json::from_str(&source.definition).map_err(|e| format!("Publishing set `{}`: {e}", source.name))?;
            for entry in set.folders.iter_mut().flat_map(|f| &mut f.entries) {
                entry.print_set = print_names
                    .get(&entry.print_set)
                    .cloned()
                    .ok_or_else(|| format!("Publishing set `{}` refers to missing print set `{}`", source.name, entry.print_set))?;
            }
            let name = free_import_name(&source.name, |n| taken.contains(n));
            set.check(&name)?;
            taken.insert(name.clone());
            selected = Some(name.clone());
            publish_commands.push(json!({"name": name, "folder": set.folder, "folders": set.folders, "overwrite": set.overwrite}));
        }
        self.session.transaction("Import Publishing Sets", |session| {
            for command in print_commands {
                session.execute("print_set", &command)?;
            }
            for command in publish_commands {
                session.execute("publish_set", &command)?;
            }
            Ok(())
        })?;
        Ok((selected, missing))
    }

    /// The Publisher, while it is open.
    pub(crate) fn publisher_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.publisher.take() else { return };
        let model = Arc::clone(&self.session.model);
        let registry = self.session.exports.clone();
        // The print set made for a row, once the Print Sets window has closed.
        if self.print_sets.is_none()
            && let Some((at, before)) = d.awaiting.take()
            && let Some(new) = model.print_sets.keys().find(|n| !before.contains(*n))
            && let Some(entry) = d.entry_mut(at)
        {
            entry.print_set = new.clone();
        }
        // A set gone (deleted, renamed elsewhere) is no choice; one changed elsewhere and not here is shown as it is now.
        if d.current.as_ref().is_some_and(|n| !model.publish_sets.contains_key(n)) {
            d.show_set(&model, model.publish_sets.keys().next().cloned());
        }
        if let Some(saved) = d.current.as_ref().and_then(|n| model.publish_sets.get(n))
            && !d.changed()
            && *saved != d.loaded
        {
            let name = d.current.clone();
            d.show_set(&model, name);
        }
        // A print set deleted leaves the folders being changed too.
        for f in &mut d.draft.folders {
            f.entries.retain(|e| model.print_sets.contains_key(&e.print_set));
        }
        let changed = d.changed();
        let busy = self.exporting();
        let mut run: Vec<Value> = Vec::new();
        let mut then: Option<Option<String>> = None;
        let (mut close, mut save, mut go, mut browse, mut edit_sets) = (false, false, false, false, false);
        let mut transfer = None;
        let mut new_set: Option<(usize, usize)> = None;
        let r = Modal::new(egui::Id::new("publisher"), t("Publisher")).width(940.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(SETS);
                        Self::publisher_sets(ui, &mut d, &model, changed, &mut run, &mut then, &mut transfer);
                    });
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        if d.current.is_none() {
                            ui.add_space(40.0);
                            ui.label(RichText::new(t("Make a publishing set with New… to choose what it writes.")).color(pal().muted));
                            return;
                        }
                        ui.horizontal(|ui| {
                            let l = ui.label(t("Destination"));
                            let hint = t("Asked when published");
                            let edit = TextEdit::singleline(&mut d.draft.folder).char_limit(publish::MAX_PATH).hint_text(hint);
                            ui.add_sized([420.0, 20.0], edit)
                                .labelled_by(l.id)
                                .on_hover_text(t("A folder. One that isn't absolute is beside the project's file."));
                            browse = ui.button(t("Browse…")).clicked();
                        });
                        ui.checkbox(&mut d.draft.overwrite, t("Overwrite existing files"))
                            .on_hover_text(t("Otherwise a file already there is kept and the new one is numbered"));
                        ui.separator();
                        ScrollArea::vertical()
                            .id_salt("publisher-folders")
                            .max_height(HEIGHT - 110.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| Self::publisher_folders(ui, &mut d, &model, &registry, &mut new_set));
                        ui.separator();
                        ui.horizontal(|ui| {
                            let room = d.draft.folders.len() < publish::MAX_FOLDERS;
                            if ui.add_enabled(room, egui::Button::new(t("Add Folder"))).clicked() {
                                // A name no folder has: PDF first, as most sets start.
                                let taken = |n: &str| d.draft.folders.iter().any(|f| f.name.eq_ignore_ascii_case(n));
                                let name = std::iter::once("PDF".to_string())
                                    .chain((2..).map(|n| tf("Folder {}", &[&n.to_string()])))
                                    .find(|n| !taken(n))
                                    .unwrap_or_default();
                                d.draft.folders.push(Folder { name, entries: Vec::new() });
                            }
                            let count: usize = d.draft.folders.iter().flat_map(|f| &f.entries).map(|e| files(&model, &registry, e)).sum();
                            let said = tf("{} print sets · {} files", &[&d.draft.len().to_string(), &count.to_string()]);
                            ui.label(RichText::new(said).color(pal().muted));
                            let times: Vec<u64> = d
                                .draft
                                .folders
                                .iter()
                                .flat_map(|f| &f.entries)
                                .filter_map(|e| e.last_export.as_ref().map(|time| time.duration_ms))
                                .collect();
                            if !times.is_empty() {
                                let seconds = times.iter().sum::<u64>() as f64 / 1000.0;
                                let duration = if seconds < 60.0 {
                                    format!("{seconds:.1}s")
                                } else {
                                    format!("{}m {:02}s", seconds as u64 / 60, seconds as u64 % 60)
                                };
                                let estimate = tf("Estimated reexport: {}", &[&duration]);
                                ui.label(RichText::new(estimate).color(pal().muted))
                                    .on_hover_text(t("Based on the last completed export of each print set"));
                            }
                        });
                    });
                });
                if let Some(e) = &d.error {
                    ui.label(RichText::new(e.as_str()).color(pal().error));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    edit_sets = ui.button(t("Print Sets…")).on_hover_text(t("Make and change the sets of views and sheets")).clicked();
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        let ready = d.current.is_some() && !d.draft.is_empty() && !busy;
                        let button = ui.add_enabled(ready, egui::Button::new(t("Publish")));
                        let button = if busy {
                            button.on_disabled_hover_text(t("An export is being written"))
                        } else {
                            button.on_disabled_hover_text(t("Add a folder and a print set first"))
                        };
                        go = button.clicked();
                        save = ui.add_enabled(changed, egui::Button::new(t("Save"))).clicked();
                        close = ui.button(t("Close")).clicked();
                    });
                });
            });
        });
        self.publisher_rule(ctx, &mut d, &model, &registry);
        self.publisher_settings(ctx, &mut d, &registry);
        // The list's commands: new, duplicate, rename, delete.
        let mut failed = None;
        for params in run {
            if self.run("publish_set", params).is_none() {
                failed = Some(std::mem::take(&mut self.status).text);
                break;
            }
        }
        match failed {
            Some(e) => d.error = Some(e),
            None => {
                if let Some(name) = then {
                    let m = &self.session.model;
                    let name = name.filter(|n| m.publish_sets.contains_key(n)).or_else(|| m.publish_sets.keys().next().cloned());
                    d.naming = None;
                    d.error = None;
                    d.show_set(m, name);
                }
            }
        }
        if save {
            self.save_publish_set(&mut d);
        }
        if edit_sets {
            self.open_print_sets(None);
        }
        if let Some(at) = new_set {
            d.awaiting = Some((at, model.print_sets.keys().cloned().collect()));
            self.open_new_print_set();
        }
        if browse {
            let dialog = rfd::AsyncFileDialog::new().set_title(t("Destination"));
            self.ask_folder(ctx, dialog, |app, path| {
                if let Some(d) = &mut app.publisher {
                    d.draft.folder = path.to_string_lossy().into_owned();
                }
            });
        }
        if matches!(transfer, Some(TransferChoice::Export)) && (!d.changed() || self.save_publish_set(&mut d)) {
            let dialog = rfd::AsyncFileDialog::new().add_filter("Publisher XML", &["xml"]).set_file_name("bimcraft-publishing-sets.xml");
            self.ask_file(ctx, dialog, true, |app, path| {
                let result = transfer_xml(&app.session.model).and_then(|xml| export::write_file(&path, xml.as_bytes()));
                if let Err(error) = result {
                    if let Some(d) = &mut app.publisher {
                        d.error = Some(error);
                    }
                } else {
                    app.status = crate::Status { text: tf("Publishing sets saved to {}", &[&path.display().to_string()]), error: false };
                }
            });
        }
        if matches!(transfer, Some(TransferChoice::Import)) && (!d.changed() || self.save_publish_set(&mut d)) {
            let dialog = rfd::AsyncFileDialog::new().add_filter("Publisher XML", &["xml"]);
            self.ask_file(ctx, dialog, false, |app, path| match app.import_publisher_xml(&path) {
                Ok((name, missing)) => {
                    if let Some(d) = &mut app.publisher {
                        d.error = None;
                        d.show_set(&app.session.model, name);
                    }
                    let text = if missing == 0 {
                        t("Publishing sets imported").to_string()
                    } else {
                        tf("Publishing sets imported; {} views were not found in this project", &[&missing.to_string()])
                    };
                    app.status = crate::Status { text, error: missing > 0 };
                }
                Err(error) => {
                    if let Some(d) = &mut app.publisher {
                        d.error = Some(error);
                    }
                }
            });
        }
        if go && (!d.changed() || self.save_publish_set(&mut d)) {
            let name = d.current.clone().unwrap_or_default();
            match d.draft.root(self.session.path.as_deref(), None) {
                Ok(root) => {
                    self.publish_to(&name, root);
                }
                // No destination of its own, or one beside a project not saved yet: asked for.
                Err(_) => {
                    let dialog = rfd::AsyncFileDialog::new().set_title(t("Publish to"));
                    self.ask_folder(ctx, dialog, move |app, path| {
                        app.publish_to(&name, path);
                    });
                }
            }
        }
        if !(close || r.should_close()) {
            self.publisher = Some(d);
        }
    }

    /// The left side: the project's publishing sets, and what is done with one.
    fn publisher_sets(
        ui: &mut Ui,
        d: &mut Dialog,
        model: &Model,
        changed: bool,
        run: &mut Vec<Value>,
        then: &mut Option<Option<String>>,
        transfer: &mut Option<TransferChoice>,
    ) {
        ui.label(RichText::new(t("Publishing sets")).strong());
        egui::Frame::new().stroke(egui::Stroke::new(1.0, pal().border)).inner_margin(4.0).show(ui, |ui| {
            ScrollArea::vertical().id_salt("publisher-sets").max_height(HEIGHT - 150.0).auto_shrink([false, false]).show(ui, |ui| {
                ui.set_width(SETS - 10.0);
                if model.publish_sets.is_empty() {
                    ui.label(RichText::new(t("None yet")).color(pal().muted));
                }
                for name in model.publish_sets.keys() {
                    let here = d.current.as_ref() == Some(name);
                    let shown = if here && changed { format!("{name} *") } else { name.clone() };
                    if ui.selectable_label(here, shown).clicked() && !here {
                        *then = Some(Some(name.clone()));
                    }
                }
            });
        });
        let current = d.current.clone();
        let mut drop_naming = false;
        if let Some((naming, name)) = &mut d.naming {
            let l = ui.label(match naming {
                Naming::New => t("New name"),
                Naming::Duplicate => t("Name of the copy"),
                Naming::Rename => t("Rename to"),
            });
            let edit = TextEdit::singleline(name).char_limit(publish::MAX_NAME).desired_width(SETS - 8.0);
            let r = ui.add(edit).labelled_by(l.id);
            r.request_focus();
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                let go = match naming {
                    Naming::New => t("Create"),
                    Naming::Duplicate => t("Duplicate"),
                    Naming::Rename => t("Rename"),
                };
                let typed = name.trim().to_string();
                if (ui.add_enabled(!typed.is_empty(), egui::Button::new(go)).clicked() || enter) && !typed.is_empty() {
                    match (*naming, &current) {
                        (Naming::New, _) => run.push(json!({"name": typed})),
                        (Naming::Duplicate, Some(from)) => run.push(json!({"name": from, "duplicate": typed})),
                        (Naming::Rename, Some(from)) => run.push(json!({"name": from, "rename": typed})),
                        _ => {}
                    }
                    *then = Some(Some(typed));
                }
                drop_naming = ui.button(t("Cancel")).clicked();
            });
        } else {
            let half = |ui: &mut Ui, text: &str, enabled: bool| {
                ui.add_enabled(enabled, egui::Button::new(t(text)).min_size(egui::vec2((SETS - 8.0) / 2.0, 0.0))).clicked()
            };
            let one = current.is_some();
            ui.horizontal(|ui| {
                if half(ui, "New…", true) {
                    d.naming = Some((Naming::New, String::new()));
                }
                if half(ui, "Duplicate…", one) {
                    d.naming = Some((Naming::Duplicate, tf("{} copy", &[current.as_deref().unwrap_or_default()])));
                }
            });
            ui.horizontal(|ui| {
                if half(ui, "Rename…", one) {
                    d.naming = Some((Naming::Rename, current.clone().unwrap_or_default()));
                }
                if half(ui, "Delete", one)
                    && let Some(name) = &current
                {
                    run.push(json!({"name": name, "delete": true}));
                    *then = Some(None);
                }
            });
            if ui.add_enabled(changed, egui::Button::new(t("Revert changes")).min_size(egui::vec2(SETS - 2.0, 0.0))).clicked() {
                *then = Some(current);
            }
            ui.horizontal(|ui| {
                if half(ui, "Import…", true) {
                    *transfer = Some(TransferChoice::Import);
                }
                if half(ui, "Export…", !model.publish_sets.is_empty()) {
                    *transfer = Some(TransferChoice::Export);
                }
            });
        }
        if drop_naming {
            d.naming = None;
        }
    }

    /// The right side's folders, each with its print sets and what they are written as.
    fn publisher_folders(ui: &mut Ui, d: &mut Dialog, model: &Model, registry: &Registry, new_set: &mut Option<(usize, usize)>) {
        if d.draft.folders.is_empty() {
            ui.label(RichText::new(t("No folders yet: Add Folder makes one to put print sets in.")).color(pal().muted));
        }
        let families = formats(registry);
        let (mut drop_folder, mut drop_entry, mut rule, mut settings) = (None, None, None, None);
        let publishing = d.current.clone().unwrap_or_default();
        for (fi, folder) in d.draft.folders.iter_mut().enumerate() {
            ui.push_id(("publisher-folder", fi), |ui| {
                let fill = if crate::style::dark() { pal().bar } else { pal().surface };
                egui::Frame::new()
                    .fill(fill)
                    .stroke(egui::Stroke::new(1.0, pal().border))
                    .corner_radius(8.0)
                    .inner_margin(8.0)
                    .outer_margin(egui::Margin::symmetric(0, 4))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let l = ui.label(RichText::new(t("Folder")).strong());
                            let edit =
                                TextEdit::singleline(&mut folder.name).char_limit(publish::MAX_NAME).hint_text(t("The destination itself"));
                            ui.add_sized([220.0, 20.0], edit).labelled_by(l.id).on_hover_text(t("A / puts it inside another: Issue/PDF"));
                            let next = next_entry(model, registry, folder).filter(|_| folder.entries.len() < publish::MAX_ENTRIES);
                            let add = ui.add_enabled(next.is_some(), egui::Button::new(t("Add Print Set")));
                            let add = if model.print_sets.is_empty() {
                                add.on_disabled_hover_text(t("The project has no print sets yet: Print Sets… makes one"))
                            } else {
                                add.on_disabled_hover_text(t("Every print set is here as every format already"))
                            };
                            if add.clicked()
                                && let Some(entry) = next
                            {
                                folder.entries.push(entry);
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(egui::Button::new("×").min_size(egui::vec2(22.0, 20.0)))
                                    .on_hover_text(t("Delete Folder"))
                                    .clicked()
                                {
                                    drop_folder = Some(fi);
                                }
                            });
                        });
                        if folder.entries.is_empty() {
                            return;
                        }
                        egui::Grid::new("publisher-entries").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
                            for head in ["Print set", "Format", "File names"] {
                                ui.label(RichText::new(t(head)).color(pal().muted));
                            }
                            ui.end_row();
                            for ei in 0..folder.entries.len() {
                                // What the other rows of the folder hold: a print set is here once for each format.
                                let others: Vec<(String, String)> = folder
                                    .entries
                                    .iter()
                                    .enumerate()
                                    .filter(|(k, _)| *k != ei)
                                    .map(|(_, e)| (e.print_set.clone(), e.format.clone()))
                                    .collect();
                                let Some(e) = folder.entries.get_mut(ei) else { continue };
                                let held = |set: &str, format: &str| others.iter().any(|(s, f)| s == set && f == format);
                                ComboBox::from_id_salt(("publisher-set", ei)).width(190.0).selected_text(e.print_set.clone()).show_ui(
                                    ui,
                                    |ui| {
                                        for name in model.print_sets.keys() {
                                            let free = !held(name, &e.format);
                                            let pick = ui.add_enabled(free, egui::Button::selectable(e.print_set == *name, name.as_str()));
                                            if pick.on_disabled_hover_text(t("Already in this folder as this format")).clicked() {
                                                e.print_set = name.clone();
                                            }
                                        }
                                        ui.separator();
                                        if ui.button(t("New Print Set…")).clicked() {
                                            *new_set = Some((fi, ei));
                                        }
                                    },
                                );
                                let format = registry.find(&e.format).ok().cloned();
                                let shown = format.as_ref().map_or_else(|| e.format.clone(), |f| format_name(f.as_ref()));
                                ComboBox::from_id_salt(("publisher-format", ei)).width(150.0).selected_text(shown).show_ui(ui, |ui| {
                                    for (family, list) in &families {
                                        ui.label(RichText::new(t(family_name(*family))).color(pal().muted));
                                        for f in list {
                                            let fits = !publish::items(model, &e.print_set, f.as_ref()).is_empty();
                                            let free = !held(&e.print_set, f.id());
                                            let label = format_name(f.as_ref());
                                            let pick = ui.add_enabled(fits && free, egui::Button::selectable(e.format == f.id(), label));
                                            let why = if free {
                                                t("The print set holds nothing this format writes")
                                            } else {
                                                t("Already in this folder as this format")
                                            };
                                            if pick.on_disabled_hover_text(why).clicked() {
                                                e.format = f.id().to_string();
                                                // One file of them all only where the format holds several.
                                                if !f.combines() {
                                                    e.combine = None;
                                                }
                                            }
                                        }
                                    }
                                });
                                let combined = e.combine.is_some() && format.as_ref().is_some_and(|f| f.combines());
                                let template = if combined { e.combine.clone() } else { e.name.clone() };
                                let said = match format.as_ref() {
                                    Some(f) => {
                                        sample(model, f.as_ref(), &e.print_set, &publishing, template.as_deref().unwrap_or(""), combined)
                                    }
                                    None => Err(tf("{} isn't a format", &[&e.format])),
                                };
                                let (text, colour) = match said {
                                    Ok(name) => (name, pal().muted),
                                    Err(why) => (why, pal().error),
                                };
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        if ui.button(t("File Names…")).clicked() {
                                            rule = Some(Rule {
                                                at: (fi, ei),
                                                template: e.name.clone().unwrap_or_default(),
                                                separator: " - ".to_string(),
                                                combine: combined,
                                                combined: e.combine.clone().unwrap_or_else(|| "{PrintSet}".to_string()),
                                                find: String::new(),
                                            });
                                        }
                                        if ui
                                            .button(t("Settings…"))
                                            .on_hover_text(t("The page, the colours and the format's own settings"))
                                            .clicked()
                                        {
                                            let ratio = match e.options.page.scale {
                                                export::page::Scale::Custom(n) => n,
                                                _ => 100.0,
                                            };
                                            settings = Some(Settings { at: (fi, ei), was: e.options.clone(), ratio, list: String::new() });
                                        }
                                        if ui.button(t("Remove")).clicked() {
                                            drop_entry = Some((fi, ei));
                                        }
                                    });
                                    // The preview has its own line and a fixed width, so a long file name cannot
                                    // widen the folder card or the Publisher. Hover shows the complete name.
                                    ui.add_sized([280.0, 18.0], egui::Label::new(RichText::new(text.clone()).color(colour)).truncate())
                                        .on_hover_text(text);
                                });
                                ui.end_row();
                            }
                        });
                        ui.add_space(6.0);
                    });
            });
        }
        if let Some((fi, ei)) = drop_entry
            && let Some(f) = d.draft.folders.get_mut(fi)
            && ei < f.entries.len()
        {
            f.entries.remove(ei);
        }
        if let Some(fi) = drop_folder.filter(|fi| *fi < d.draft.folders.len()) {
            d.draft.folders.remove(fi);
        }
        if rule.is_some() {
            d.rule = rule;
        }
        if settings.is_some() {
            d.settings = settings;
        }
    }

    /// File Names: an entry's template, put together of tokens, with a sample of what it makes.
    fn publisher_rule(&mut self, ctx: &Context, d: &mut Dialog, model: &Model, registry: &Registry) {
        let Some(mut rule) = d.rule.take() else { return };
        let Some(entry) = d.entry_mut(rule.at).map(|e| e.clone()) else { return };
        let Ok(format) = registry.find(&entry.format).cloned() else { return };
        let publishing = d.current.clone().unwrap_or_default();
        let (mut ok, mut cancel) = (false, false);
        let r = Modal::new(egui::Id::new("publisher-rule"), t("File Names")).width(620.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(RichText::new(format!("{} · {}", entry.print_set, format_name(format.as_ref()))).color(pal().muted));
                let combines = format.combines();
                if combines {
                    ui.checkbox(&mut rule.combine, t("Combine into one file")).on_hover_text(t("Every view of the print set, a page each"));
                }
                let combined = combines && rule.combine;
                ui.horizontal(|ui| {
                    let l = ui.label(if combined { t("File name") } else { t("Name of each file") });
                    let (text, hint) = if combined {
                        (&mut rule.combined, "{PrintSet}".to_string())
                    } else {
                        let kind = publish::items(model, &entry.print_set, format.as_ref())
                            .first()
                            .and_then(|i| i.view)
                            .and_then(|g| export::kind_of(model, model.elements.id(g)));
                        (&mut rule.template, export::default_name(kind.unwrap_or(export::Kind::Sheet)).to_string())
                    };
                    let edit = TextEdit::singleline(text).char_limit(naming::MAX_TEMPLATE).hint_text(hint);
                    ui.add_sized([440.0, 20.0], edit).labelled_by(l.id).on_hover_text(t("A / makes a folder; {{ and }} are braces"));
                });
                let template = if combined { rule.combined.clone() } else { rule.template.clone() };
                match sample(model, format.as_ref(), &entry.print_set, &publishing, &template, combined) {
                    Ok(name) => ui.label(RichText::new(tf("For example: {}", &[&name])).color(pal().muted)),
                    Err(why) => ui.label(RichText::new(why).color(pal().error)),
                };
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(t("Add a parameter"));
                    ui.add(TextEdit::singleline(&mut rule.find).hint_text(t("Search…")).desired_width(200.0));
                    let l = ui.label(t("Separator"));
                    ui.add(TextEdit::singleline(&mut rule.separator).char_limit(8).desired_width(50.0)).labelled_by(l.id);
                    if ui.button(t("Clear")).clicked() {
                        if combined { rule.combined.clear() } else { rule.template.clear() }
                    }
                });
                let words = crate::browser::words(&rule.find);
                let groups: [(&str, Vec<String>); 3] = [
                    ("Views and sheets", ModelTokens::NAMES.iter().map(|n| n.to_string()).collect()),
                    ("Project Information", ModelTokens::information().iter().map(|n| n.to_string()).collect()),
                    ("Project parameters", ModelTokens::parameters(model)),
                ];
                egui::Frame::new().stroke(egui::Stroke::new(1.0, pal().border)).inner_margin(4.0).show(ui, |ui| {
                    ScrollArea::vertical().id_salt("publisher-tokens").max_height(190.0).auto_shrink([false, false]).show(ui, |ui| {
                        for (group, names) in &groups {
                            let shown: Vec<&String> =
                                names.iter().filter(|n| words.iter().all(|w| crate::browser::spans(n, w).is_some())).collect();
                            if shown.is_empty() {
                                continue;
                            }
                            ui.label(RichText::new(t(group)).color(pal().muted));
                            ui.horizontal_wrapped(|ui| {
                                for name in shown {
                                    if ui.button(name.as_str()).clicked() {
                                        let text = if combined { &mut rule.combined } else { &mut rule.template };
                                        *text = with_token(text, name, &rule.separator);
                                    }
                                }
                            });
                        }
                    });
                });
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button(t("Cancel")).clicked();
                    ok = ui.button(t("OK")).clicked();
                });
            });
        });
        if ok {
            let combined = format.combines() && rule.combine;
            let given = |text: &str| Some(text.trim().to_string()).filter(|t| !t.is_empty());
            if let Some(e) = d.entry_mut(rule.at) {
                e.name = given(&rule.template);
                e.combine = if combined { given(&rule.combined).or_else(|| Some("{PrintSet}".to_string())) } else { None };
            }
            return;
        }
        if !(cancel || r.should_close()) {
            d.rule = Some(rule);
        }
    }

    /// Settings: what an entry's format is asked, as the Export dialog asks it.
    fn publisher_settings(&mut self, ctx: &Context, d: &mut Dialog, registry: &Registry) {
        let Some(mut s) = d.settings.take() else { return };
        let Some(entry) = d.entry_mut(s.at) else { return };
        let Ok(format) = registry.find(&entry.format).cloned() else { return };
        let (mut ok, mut cancel) = (false, false);
        let title = tf("{} Settings", &[format.label()]);
        let r = Modal::new(egui::Id::new("publisher-settings"), title).width(460.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(RichText::new(entry.print_set.as_str()).color(pal().muted));
                ui.push_id("publisher-format-options", |ui| {
                    format_options(ui, format.as_ref(), &mut entry.options, &mut s.ratio, &mut s.list, None);
                    if format.id() == "pdf" {
                        ui.add_space(8.0);
                        ui.label(RichText::new(t("Document")).strong());
                        pdf_settings(ui, &mut entry.options.pdf);
                    }
                    let plain = !format.paged()
                        && !format.takes().contains(&export::Takes::Drawing)
                        && !format.id().starts_with("ifc")
                        && format.id() != "jpeg";
                    if plain {
                        ui.label(RichText::new(t("This format has no settings.")).color(pal().muted));
                    }
                });
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button(t("Cancel")).clicked();
                    ok = ui.button(t("OK")).clicked();
                });
            });
        });
        if cancel || r.should_close() {
            entry.options = s.was;
            return;
        }
        if !ok {
            d.settings = Some(s);
        }
    }
}
