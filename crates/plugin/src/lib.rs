//! BIMCraft's plugins: folders of a manifest and a WebAssembly module that run in a sandbox and reach the
//! application only through commands. This crate reads the folder, checks the manifests, compiles the modules,
//! registers their functions as commands of a [`Session`] and runs them under [`runtime`]'s limits.
//!
//! The core knows nothing of WASM: it only has a session's registry of external commands (`engine::ExternalCommand`),
//! which is what [`Plugins::register`] fills. Without a plugin folder nothing changes, and the catalog, the errors and
//! the undo history are exactly as they were.

pub mod manifest;
pub mod runtime;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use bimcraft_core::engine::{ExternalCommand, Session};

pub use manifest::{Function, Manifest};

/// The manifest's file name, in a plugin's folder.
pub const MANIFEST: &str = "plugin.json";
/// The file that keeps the plugins the user switched off, in the plugin folder.
pub const DISABLED: &str = "disabled.json";
/// The most log lines kept, so a plugin that logs in a loop can't grow it without end.
pub const MAX_LOG: usize = 2000;
/// The most disabled ids the state file keeps, and its size.
const MAX_DISABLED: usize = 1000;
const MAX_STATE: u64 = 64 << 10;

/// How important a log line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

/// One line of the plugin log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The plugin's id, or its file's name when the manifest couldn't be read.
    pub plugin: String,
    pub level: Level,
    pub text: String,
}

/// The plugin log: what was loaded, what was refused and why, and what the plugins themselves wrote.
#[derive(Debug, Default)]
pub struct Log {
    entries: Mutex<Vec<Entry>>,
}

impl Log {
    pub fn new() -> Log {
        Log::default()
    }

    /// Adds a line, dropping the oldest when the log is full.
    pub fn note(&self, plugin: &str, level: Level, text: impl Into<String>) {
        if let Ok(mut entries) = self.entries.lock() {
            if entries.len() >= MAX_LOG {
                entries.remove(0);
            }
            entries.push(Entry { plugin: plugin.to_string(), level, text: text.into() });
        }
    }

    pub fn entries(&self) -> Vec<Entry> {
        self.entries.lock().map(|e| e.clone()).unwrap_or_default()
    }

    pub fn of(&self, plugin: &str) -> Vec<Entry> {
        self.entries().into_iter().filter(|e| e.plugin == plugin).collect()
    }
}

/// What became of a plugin at the last read of the folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Its module is loaded and its commands are registered.
    Loaded,
    /// Switched off by the user: its manifest is read, its module is not.
    Disabled,
    /// It couldn't be loaded; the reason says why, and is in the log too.
    Failed(String),
}

impl Status {
    pub fn is_loaded(&self) -> bool {
        matches!(self, Status::Loaded)
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Status::Failed(why) => Some(why),
            _ => None,
        }
    }
}

/// A plugin that couldn't even be read far enough to have a manifest, as the window lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Broken {
    pub name: String,
    pub reason: String,
}

/// A plugin in the folder.
pub struct Plugin {
    pub manifest: Manifest,
    /// The folder its manifest and module are in.
    pub folder: PathBuf,
    pub status: Status,
    runtime: Option<Arc<runtime::Runtime>>,
    log: Arc<Log>,
}

impl Plugin {
    /// The commands this plugin contributes to the catalog, when it is loaded.
    pub fn command_ids(&self) -> Vec<String> {
        self.manifest.functions.iter().map(|f| command_id(&self.manifest.id, &f.id)).collect()
    }

    /// The lines this plugin wrote to the log.
    pub fn log(&self) -> Vec<Entry> {
        self.log.of(&self.manifest.id)
    }
}

/// The command a plugin's function answers to: `plugin:<plugin id>:<function id>`.
pub fn command_id(plugin: &str, function: &str) -> String {
    format!("plugin:{plugin}:{function}")
}

/// A command on the ribbon, as a plugin declared it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RibbonCommand {
    /// The full command id (`plugin:<plugin>:<function>`).
    pub id: String,
    pub label: String,
    pub order: i64,
}

/// A panel of a plugin tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RibbonPanel {
    pub name: String,
    pub commands: Vec<RibbonCommand>,
}

/// A tab a plugin declared. A plugin that declares none has its functions in the common `Plugins` section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RibbonTab {
    /// The plugin's own tab name; none for the common Plugins section.
    pub name: Option<String>,
    pub panels: Vec<RibbonPanel>,
}

/// Where a plugin's folder is: `BIMCRAFT_PLUGINS` if it is set (empty disables plugins), else `plugins`
/// beside the favorites file, in the user's configuration folder.
pub fn default_folder() -> Option<PathBuf> {
    match std::env::var_os("BIMCRAFT_PLUGINS") {
        Some(path) if path.is_empty() => None,
        Some(path) => Some(PathBuf::from(path)),
        None => bimcraft_core::favorites::default_path().and_then(|favorites| favorites.parent().map(|p| p.join("plugins"))),
    }
}

/// The plugin folder of this machine, read. A missing folder is no plugins, not an error.
pub fn load() -> Plugins {
    Plugins::load_from(default_folder().as_deref())
}

#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct DisabledFile {
    #[serde(default)]
    disabled: Vec<String>,
}

/// The plugins of one folder, with what became of each and the log of the last read.
pub struct Plugins {
    pub folder: Option<PathBuf>,
    pub plugins: Vec<Plugin>,
    /// Manifests that couldn't be read at all.
    pub broken: Vec<Broken>,
    log: Arc<Log>,
    disabled: BTreeSet<String>,
}

impl Default for Plugins {
    fn default() -> Self {
        Plugins::empty()
    }
}

impl Plugins {
    /// No folder at all: no plugins, and nothing changes.
    pub fn empty() -> Plugins {
        Plugins { folder: None, plugins: Vec::new(), broken: Vec::new(), log: Arc::new(Log::new()), disabled: BTreeSet::new() }
    }

    /// The folder at `folder`, read; none is no plugins.
    pub fn load_from(folder: Option<&Path>) -> Plugins {
        let log = Arc::new(Log::new());
        Plugins::load_into(folder, log)
    }

    fn load_into(folder: Option<&Path>, log: Arc<Log>) -> Plugins {
        let mut plugins =
            Plugins { folder: folder.map(Path::to_path_buf), plugins: Vec::new(), broken: Vec::new(), log, disabled: BTreeSet::new() };
        let Some(folder) = folder else { return plugins };
        plugins.disabled = read_disabled(folder, &plugins.log);
        let list = match std::fs::read_dir(folder) {
            Ok(list) => list,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return plugins,
            Err(e) => {
                plugins.log.note("plugins", Level::Error, format!("{}: {e}", folder.display()));
                return plugins;
            }
        };
        let mut paths: Vec<PathBuf> = list.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                let manifest = path.join(MANIFEST);
                if manifest.is_file() {
                    plugins.add(&manifest);
                }
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) && path.file_name().is_none_or(|n| n != DISABLED) {
                plugins.add(&path);
            }
        }
        plugins.log.note("plugins", Level::Info, format!("{} plugin(s) in {}", plugins.plugins.len(), folder.display()));
        plugins
    }

    /// Reads one manifest and its module, adding it to the list or saying why not.
    fn add(&mut self, manifest_path: &Path) {
        let name = manifest_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let manifest = match Manifest::read(manifest_path) {
            Ok(m) => m,
            Err(e) => {
                self.log.note(&name, Level::Error, e.clone());
                self.broken.push(Broken { name, reason: e });
                return;
            }
        };
        let id = manifest.id.clone();
        if self.plugins.iter().any(|p| p.manifest.id == id) {
            let why = format!("another plugin is already `{id}`");
            self.log.note(&id, Level::Error, why.clone());
            self.broken.push(Broken { name: manifest.name, reason: why });
            return;
        }
        let folder = manifest_path.parent().map(Path::to_path_buf).unwrap_or_default();
        let disabled = self.disabled.contains(&id);
        let status = if disabled { Status::Disabled } else { Status::Loaded };
        let mut plugin = Plugin { manifest, folder, status, runtime: None, log: Arc::clone(&self.log) };
        if disabled {
            self.log.note(&id, Level::Info, "switched off: its commands aren't registered");
            self.plugins.push(plugin);
            return;
        }
        match load_runtime(&plugin.manifest, manifest_path) {
            Ok(rt) => {
                plugin.runtime = Some(Arc::new(rt));
                self.log.note(&id, Level::Info, format!("loaded `{}` {}", plugin.manifest.name, plugin.manifest.version));
            }
            Err(e) => {
                plugin.status = Status::Failed(e.clone());
                self.log.note(&id, Level::Error, e);
            }
        }
        self.plugins.push(plugin);
    }

    /// Reads the folder again, keeping the log.
    pub fn refresh(&mut self) {
        let log = Arc::clone(&self.log);
        *self = Plugins::load_into(self.folder.as_deref(), log);
    }

    /// Switches a plugin on or off, keeps it in the folder's state file, and reads the folder again.
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), String> {
        if enabled {
            self.disabled.remove(id);
        } else {
            self.disabled.insert(id.to_string());
        }
        self.save_disabled()?;
        self.refresh();
        Ok(())
    }

    fn save_disabled(&self) -> Result<(), String> {
        let Some(folder) = &self.folder else { return Ok(()) };
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        let path = folder.join(DISABLED);
        let list = self.disabled.iter().take(MAX_DISABLED).cloned().collect();
        let text = serde_json::to_string_pretty(&DisabledFile { disabled: list }).map_err(|e| e.to_string())?;
        let part = path.with_extension("json.part");
        std::fs::write(&part, text).map_err(|e| format!("{}: {e}", part.display()))?;
        std::fs::rename(&part, &path).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Registers every loaded plugin's functions in `session`, as commands of it. A session keeps the commands
    /// of the plugins of the moment; a plugin switched off since the last call has none.
    pub fn register(&self, session: &mut Session) {
        session.clear_external();
        for plugin in self.plugins.iter().filter(|p| p.status.is_loaded()) {
            let Some(rt) = &plugin.runtime else { continue };
            for f in &plugin.manifest.functions {
                let id = command_id(&plugin.manifest.id, &f.id);
                let params = serde_json::to_string(&f.params).unwrap_or_else(|_| "{}".to_string());
                let menu = f.tab.as_ref().map(|tab| format!("{tab} > {}", plugin.manifest.panel_name(f)));
                let step = format!("{}: {}", plugin.manifest.name, f.label);
                let (plugin_id, function, files, log) =
                    (plugin.manifest.id.clone(), f.id.clone(), plugin.manifest.files(), Arc::clone(&self.log));
                let rt = Arc::clone(rt);
                let run = move |session: &mut Session, params: &Value| {
                    session.transaction(&step, |s| rt.invoke(s, &plugin_id, &function, files, params, &log))
                };
                let command = ExternalCommand {
                    id,
                    label: f.label.clone(),
                    aliases: Vec::new(),
                    params,
                    menu,
                    plugin: plugin.manifest.id.clone(),
                    run: Arc::new(run),
                };
                if let Err(e) = session.register(command) {
                    self.log.note(&plugin.manifest.id, Level::Error, e);
                }
            }
        }
    }

    /// The log of the folder: load failures and what the plugins wrote.
    pub fn log(&self) -> Vec<Entry> {
        self.log.entries()
    }

    /// The ribbon tabs the loaded plugins declare, in the order the plugins were read. The common `Plugins`
    /// section comes last.
    pub fn tabs(&self) -> Vec<RibbonTab> {
        let mut tabs: Vec<RibbonTab> = Vec::new();
        let mut common = Vec::new();
        let mut commands: Vec<(Option<String>, String, RibbonCommand)> = Vec::new();
        for plugin in self.plugins.iter().filter(|p| p.status.is_loaded()) {
            for f in &plugin.manifest.functions {
                let tab = plugin.manifest.tab_name(f);
                let panel = plugin.manifest.panel_name(f);
                let order = f.order.unwrap_or(0);
                let command = RibbonCommand { id: command_id(&plugin.manifest.id, &f.id), label: f.label.clone(), order };
                match &tab {
                    Some(_) => commands.push((tab, panel, command)),
                    None => common.push((panel, command)),
                }
            }
        }
        for (tab_name, panel_name, command) in commands {
            let index = match tabs.iter().position(|t| t.name == tab_name) {
                Some(i) => i,
                None => {
                    tabs.push(RibbonTab { name: tab_name, panels: Vec::new() });
                    tabs.len() - 1
                }
            };
            push_command(&mut tabs[index].panels, panel_name, command);
        }
        if !common.is_empty() {
            let mut panels = Vec::new();
            for (panel, command) in common {
                push_command(&mut panels, panel, command);
            }
            tabs.push(RibbonTab { name: None, panels });
        }
        for tab in &mut tabs {
            for panel in &mut tab.panels {
                panel.commands.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase())));
            }
        }
        tabs
    }

    /// A map from a command id to the plugin that brings it, for the window's lists.
    pub fn owners(&self) -> BTreeMap<String, String> {
        let mut owners = BTreeMap::new();
        for p in self.plugins.iter().filter(|p| p.status.is_loaded()) {
            for f in &p.manifest.functions {
                owners.insert(command_id(&p.manifest.id, &f.id), p.manifest.name.clone());
            }
        }
        owners
    }
}

fn push_command(panels: &mut Vec<RibbonPanel>, panel: String, command: RibbonCommand) {
    match panels.iter_mut().find(|p| p.name == panel) {
        Some(p) => p.commands.push(command),
        None => panels.push(RibbonPanel { name: panel, commands: vec![command] }),
    }
}

fn load_runtime(manifest: &Manifest, manifest_path: &Path) -> Result<runtime::Runtime, String> {
    let module = manifest.module_path(manifest_path);
    let size = std::fs::metadata(&module).map_err(|e| format!("{}: {e}", module.display()))?.len();
    if size > manifest::MAX_MODULE {
        return Err(format!("{} is too large ({size} bytes; a module is up to {} MB)", module.display(), manifest::MAX_MODULE >> 20));
    }
    let bytes = std::fs::read(&module).map_err(|e| format!("{}: {e}", module.display()))?;
    runtime::Runtime::new(&bytes)
}

fn read_disabled(folder: &Path, log: &Log) -> BTreeSet<String> {
    let path = folder.join(DISABLED);
    let text = match std::fs::metadata(&path) {
        Ok(m) if m.len() > MAX_STATE => {
            log.note("plugins", Level::Warning, format!("{} is too large: the switched-off list is ignored", path.display()));
            return BTreeSet::new();
        }
        Ok(_) => match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                log.note("plugins", Level::Warning, format!("{}: {e}", path.display()));
                return BTreeSet::new();
            }
        },
        Err(_) => return BTreeSet::new(),
    };
    match serde_json::from_str::<DisabledFile>(&text) {
        Ok(file) => file.disabled.into_iter().take(MAX_DISABLED).collect(),
        Err(e) => {
            log.note("plugins", Level::Warning, format!("{} isn't a switched-off list: {e}", path.display()));
            BTreeSet::new()
        }
    }
}
