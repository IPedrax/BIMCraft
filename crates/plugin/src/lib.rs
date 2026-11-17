//! BIMCraft's plugins: folders of a manifest and a WebAssembly module that run in a sandbox and reach the
//! application only through commands. This crate reads the folder, checks the manifests, compiles the modules,
//! registers their functions as commands of a [`Session`] and runs them under [`runtime`]'s limits.
//!
//! The core knows nothing of WASM: it only has a session's registry of external commands (`engine::ExternalCommand`),
//! which is what [`Plugins::register`] fills. Without a plugin folder nothing changes, and the catalog, the errors and
//! the undo history are exactly as they were.

pub mod manifest;
pub mod runtime;
pub mod testing;
pub mod update;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use bimcraft_core::engine::{ExternalCommand, Session};

pub use manifest::{Function, Manifest};

/// The manifest's file name, in a plugin's folder.
pub const MANIFEST: &str = "plugin.json";
/// The file that keeps the plugins the user switched off and the permissions they granted, in the plugin folder.
pub const STATE: &str = "state.json";
/// The state file's former name, which an older version wrote: it kept the switched-off list alone, and is
/// still read when `state.json` isn't there.
pub const LEGACY_STATE: &str = "disabled.json";
/// The most log lines kept, so a plugin that logs in a loop can't grow it without end.
pub const MAX_LOG: usize = 2000;
/// The most disabled ids the state file keeps, and its size.
const MAX_DISABLED: usize = 1000;
/// The most permissions one plugin's state keeps.
const MAX_GRANTS: usize = 8;
/// The most update events the state file keeps: the newest ones, oldest dropped.
pub const MAX_HISTORY: usize = 50;
/// The most backup copies kept for one plugin unless the user says otherwise.
pub const DEFAULT_BACKUPS: usize = 3;
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
    /// The manifest's own path: a plugin may also be a single `*.json` in the plugin folder.
    pub manifest_path: PathBuf,
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StateFile {
    #[serde(default)]
    pub(crate) disabled: Vec<String>,
    /// What the user let each plugin do, by plugin id: `{"sample": ["files"]}`. A manifest may only ask;
    /// the grant is the user's, and none is granted until they do it in Manage > Plugins.
    #[serde(default)]
    pub(crate) permissions: BTreeMap<String, Vec<String>>,
    /// The publisher's key each plugin was first installed with: fixed on the first update (TOFU), and kept here
    /// beside the permission. A new key is a new publisher until the user says otherwise.
    #[serde(default)]
    pub(crate) update: BTreeMap<String, PinnedKey>,
    /// The newest update events, all plugins together: at most [`MAX_HISTORY`].
    #[serde(default)]
    pub(crate) updates: Vec<UpdateRecord>,
    /// What the user chose about updating plugins.
    #[serde(default)]
    pub(crate) settings: UpdateSettings,
}

/// The key a plugin's updates were first accepted with, and whether the user has confirmed the channel.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinnedKey {
    /// `ed25519:BASE64`, or empty when the feed isn't signed and the user allowed that.
    #[serde(default)]
    pub key: String,
    /// The user saw the channel, its key, a version and a size and agreed; later installs need no question.
    #[serde(default, rename = "update_confirmed")]
    pub confirmed: bool,
}

/// One update that was installed or rolled back, as the history keeps it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRecord {
    pub id: String,
    pub from: String,
    pub to: String,
    /// UTC date and time, `YYYY-MM-DDTHH:MM:SSZ`.
    #[serde(default)]
    pub at: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub host: String,
    /// `auto` for a check at startup, `manual` for a button or a plugin's own call.
    #[serde(default)]
    pub by: String,
}

/// What the user chose about plugins that update themselves.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateSettings {
    /// Ask before every installed version, not only the first one and the unusual ones. Off by default.
    #[serde(default)]
    pub ask_before_every: bool,
    /// Let a feed without a signature install anything. Off by default, and asked every time.
    #[serde(default)]
    pub allow_unsigned: bool,
    /// Let a feed live at a loopback or link-local address, for local development. Off by default.
    #[serde(default)]
    pub allow_private: bool,
    /// How many `<id>.backup-*` folders to keep for one plugin.
    #[serde(default = "default_backups")]
    pub backups: usize,
}

fn default_backups() -> usize {
    DEFAULT_BACKUPS
}

impl Default for UpdateSettings {
    fn default() -> Self {
        UpdateSettings { ask_before_every: false, allow_unsigned: false, allow_private: false, backups: DEFAULT_BACKUPS }
    }
}

/// The plugins of one folder, with what became of each and the log of the last read.
pub struct Plugins {
    pub folder: Option<PathBuf>,
    pub plugins: Vec<Plugin>,
    /// Manifests that couldn't be read at all.
    pub broken: Vec<Broken>,
    log: Arc<Log>,
    /// The folder's `state.json`: switched-off plugins, grants, pinned keys, history and settings. The updater
    /// holds the same file, so an install it makes and a grant the window makes can't overwrite each other.
    state: Arc<Mutex<StateFile>>,
    /// The channels, the network and the checks of this reading of the folder.
    updater: Arc<update::Updater>,
    /// The plugins whose new version is on disk and waits for a restart.
    restart: Arc<Mutex<BTreeSet<String>>>,
    /// The network a test or embedder gave: kept across a re-read of the folder.
    net: Option<Arc<dyn bimcraft_update::Net>>,
}

impl Default for Plugins {
    fn default() -> Self {
        Plugins::empty()
    }
}

impl Plugins {
    /// No folder at all: no plugins, and nothing changes.
    pub fn empty() -> Plugins {
        let log = Arc::new(Log::new());
        let state = Arc::new(Mutex::new(StateFile::default()));
        let restart = Arc::new(Mutex::new(BTreeSet::new()));
        let updater = Arc::new(update::Updater::new(
            None,
            Arc::new(bimcraft_update::net::Ureq::for_feed(false)),
            Arc::clone(&state),
            Arc::clone(&log),
            Arc::clone(&restart),
        ));
        Plugins { folder: None, plugins: Vec::new(), broken: Vec::new(), log, state, updater, restart, net: None }
    }

    /// The folder at `folder`, read; none is no plugins.
    pub fn load_from(folder: Option<&Path>) -> Plugins {
        let log = Arc::new(Log::new());
        Plugins::load_into(folder, log)
    }

    /// The folder at `folder`, read, with `net` in place of the desktop's network: how a test answers updates
    /// from a table and never touches the internet.
    pub fn load_from_with_net(folder: Option<&Path>, net: Arc<dyn bimcraft_update::Net>) -> Plugins {
        let log = Arc::new(Log::new());
        Plugins::load_into_with_net(folder, log, None, Some(net))
    }

    /// The plugins loaded, each with `module` in place of its own module. It is how a test or an embedder runs
    /// a module it builds (a WAT, a file in `target/`) under a manifest in a folder, without a build of that
    /// module beside the manifest: the folder's manifests say what the plugin is, `module` is what runs.
    pub fn load_from_with_module(folder: Option<&Path>, module: Vec<u8>) -> Plugins {
        let log = Arc::new(Log::new());
        let compiled = match runtime::Runtime::new(&module) {
            Ok(rt) => Arc::new(rt),
            Err(e) => {
                log.note("plugins", Level::Error, format!("the module given: {e}"));
                return Plugins::empty();
            }
        };
        // Built once, and what every plugin the folder loads runs: the file its manifest names is never read.
        let mut plugins = Plugins::load_into_with(folder, log, Some(Arc::clone(&compiled)));
        for plugin in plugins.plugins.iter_mut().filter(|p| p.status.is_loaded()) {
            plugin.runtime = Some(Arc::clone(&compiled));
        }
        plugins
    }

    fn load_into(folder: Option<&Path>, log: Arc<Log>) -> Plugins {
        Plugins::load_into_with(folder, log, None)
    }

    fn load_into_with(folder: Option<&Path>, log: Arc<Log>, module: Option<Arc<runtime::Runtime>>) -> Plugins {
        Plugins::load_into_with_net(folder, log, module, None)
    }

    /// Reads `folder` with `module` for every plugin the caller gave one: the plugin's module is compiled from it,
    /// and its own file isn't read. `net` is the updater's network; none: the desktop's HTTPS.
    fn load_into_with_net(
        folder: Option<&Path>,
        log: Arc<Log>,
        module: Option<Arc<runtime::Runtime>>,
        net: Option<Arc<dyn bimcraft_update::Net>>,
    ) -> Plugins {
        let mut plugins = Plugins::empty();
        plugins.folder = folder.map(Path::to_path_buf);
        plugins.log = Arc::clone(&log);
        plugins.net = net.clone();
        let Some(folder) = folder else {
            plugins.updater = Arc::new(update::Updater::new(
                None,
                net.unwrap_or_else(|| Arc::new(bimcraft_update::net::Ureq::for_feed(false))),
                Arc::clone(&plugins.state),
                log,
                Arc::clone(&plugins.restart),
            ));
            return plugins;
        };
        let state = read_state(folder, &plugins.log);
        let allow_private = state.settings.allow_private;
        *lock(&plugins.state) = state;
        let net = net.unwrap_or_else(|| Arc::new(bimcraft_update::net::Ureq::for_feed(allow_private)));
        plugins.updater = Arc::new(update::Updater::new(
            Some(folder.to_path_buf()),
            net,
            Arc::clone(&plugins.state),
            Arc::clone(&plugins.log),
            Arc::clone(&plugins.restart),
        ));
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
                // The updater's own folders aren't plugins: a backup holds the previous version's `plugin.json`
                // with the same id, and reading it would say "another plugin is already `<id>`" on every start.
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
                if is_update_work(name) {
                    continue;
                }
                let manifest = path.join(MANIFEST);
                if manifest.is_file() {
                    plugins.add(&manifest, module.as_ref());
                }
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json"))
                && path.file_name().is_none_or(|n| n != STATE && n != LEGACY_STATE)
            {
                plugins.add(&path, module.as_ref());
            }
        }
        plugins.log.note("plugins", Level::Info, format!("{} plugin(s) in {}", plugins.plugins.len(), folder.display()));
        plugins
    }

    /// Reads one manifest and its module, adding it to the list or saying why not. `given` is the module the caller
    /// supplied for every plugin ([`Plugins::load_from_with_module`]): it runs in place of the plugin's own, which
    /// isn't read.
    fn add(&mut self, manifest_path: &Path, given: Option<&Arc<runtime::Runtime>>) {
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
        let disabled = lock(&self.state).disabled.iter().any(|d| d == &id);
        let status = if disabled { Status::Disabled } else { Status::Loaded };
        let mut plugin =
            Plugin { manifest, folder, manifest_path: manifest_path.to_path_buf(), status, runtime: None, log: Arc::clone(&self.log) };
        if disabled {
            self.log.note(&id, Level::Info, "switched off: its commands aren't registered");
            self.plugins.push(plugin);
            return;
        }
        let loaded = match given {
            Some(rt) => Ok(Arc::clone(rt)),
            None => load_runtime(&plugin.manifest, manifest_path).map(Arc::new),
        };
        match loaded {
            Ok(rt) => {
                plugin.runtime = Some(rt);
                self.log.note(&id, Level::Info, format!("loaded `{}` {}", plugin.manifest.name, plugin.manifest.version));
                if plugin.manifest.asks_for_files() {
                    self.log.note(
                        &id,
                        Level::Info,
                        "asks to read and write files; the user hasn't allowed it until they do in Manage > Plugins",
                    );
                }
                if plugin.manifest.asks_for_update() {
                    self.log.note(&id, Level::Info, "asks to update itself; the user hasn't allowed it until they do in Manage > Plugins");
                }
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
        let folder = self.folder.clone();
        // A version installed earlier still waits for the restart: reading the folder again doesn't undo that.
        let restart = lock(&self.restart).clone();
        *self = Plugins::load_into_with_net(folder.as_deref(), log, None, self.net.clone());
        *lock(&self.restart) = restart;
    }

    /// Switches a plugin on or off, keeps it in the folder's state file, and reads the folder again.
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), String> {
        {
            let mut state = lock(&self.state);
            if enabled {
                state.disabled.retain(|d| d != id);
            } else if !state.disabled.iter().any(|d| d == id) {
                state.disabled.push(id.to_string());
            }
        }
        self.save_state()?;
        self.refresh();
        Ok(())
    }

    fn save_state(&self) -> Result<(), String> {
        let Some(folder) = &self.folder else { return Ok(()) };
        write_state(folder, &lock(&self.state))
    }

    /// Whether the user granted `permission` to the plugin `id`. A manifest only asks for one; none is granted
    /// until the user says so in Manage > Plugins, and then it is kept in the folder's state file.
    pub fn permission_granted(&self, id: &str, permission: &str) -> bool {
        lock(&self.state).permissions.get(id).is_some_and(|list| list.iter().any(|p| p == permission))
    }

    /// Grants or takes back `permission` for the plugin `id`, keeps it in the state file, and reads the folder
    /// again so the plugin's commands carry the new answer. A plugin that doesn't ask for it in its manifest
    /// can't be granted it: the manifest is the request, the user the grant.
    pub fn set_permission(&mut self, id: &str, permission: &str, granted: bool) -> Result<(), String> {
        if granted {
            let asks = self.plugins.iter().find(|p| p.manifest.id == id).is_some_and(|p| p.manifest.asks_for(permission));
            if !asks {
                return Err(format!("`{id}` doesn't ask for the `{permission}` permission in its manifest, so it can't be granted"));
            }
        }
        if granted {
            let mut state = lock(&self.state);
            let list = state.permissions.entry(id.to_string()).or_default();
            if !list.iter().any(|p| p == permission) {
                list.push(permission.to_string());
            }
        } else {
            let mut state = lock(&self.state);
            if let Some(list) = state.permissions.get_mut(id) {
                list.retain(|p| p != permission);
                if list.is_empty() {
                    state.permissions.remove(id);
                }
            }
        }
        self.save_state()?;
        self.log.note(
            id,
            Level::Info,
            if granted {
                format!("the user granted the `{permission}` permission")
            } else {
                format!("the user took back the `{permission}` permission")
            },
        );
        self.refresh();
        Ok(())
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
                // The manifest asks for the permission; only the user's grant lets the call through.
                let files = plugin.manifest.asks_for_files() && self.permission_granted(&plugin.manifest.id, "files");
                let update = self.update_call(plugin);
                let (plugin_id, function, log) = (plugin.manifest.id.clone(), f.id.clone(), Arc::clone(&self.log));
                let rt = Arc::clone(rt);
                let run = move |session: &mut Session, params: &Value| {
                    let call = runtime::Call { files, update: update.clone(), params, log: &log };
                    session.transaction(&step, |s| rt.invoke_call(s, &plugin_id, &function, call))
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

    /// The update call a loaded plugin's functions carry, when the user let it update itself. No grant: none,
    /// and the host functions answer that the plugin isn't allowed.
    fn update_call(&self, plugin: &Plugin) -> Option<Arc<update::UpdateCall>> {
        let m = &plugin.manifest;
        if !m.asks_for_update() || !self.permission_granted(&m.id, "update") {
            return None;
        }
        let feed = m.update.as_ref().map(|u| u.feed.clone()).unwrap_or_default();
        let public_key = m.update.as_ref().and_then(|u| u.public_key.clone());
        Some(Arc::new(update::UpdateCall {
            service: Arc::clone(&self.updater),
            target: update::UpdateTarget {
                id: m.id.clone(),
                name: m.name.clone(),
                version: m.version.clone(),
                folder: plugin.folder.clone(),
                manifest_path: plugin.manifest_path.clone(),
                feed,
                public_key,
                abi: m.abi,
            },
        }))
    }

    /// The update settings the user chose: asked before every install, unsigned feeds, local addresses, backups.
    pub fn update_settings(&self) -> UpdateSettings {
        lock(&self.state).settings.clone()
    }

    /// Keeps the user's update settings in `state.json`.
    pub fn set_update_settings(&self, settings: UpdateSettings) -> Result<(), String> {
        lock(&self.state).settings = settings;
        self.save_state()
    }

    /// The updater, for the window's checks: the same network, folder and state the plugins' calls use.
    pub fn updater(&self) -> &Arc<update::Updater> {
        &self.updater
    }

    /// Whether the plugin's new version is installed on disk and waits for a restart.
    pub fn restart_required(&self, id: &str) -> bool {
        lock(&self.restart).iter().any(|p| p == id)
    }

    /// The plugins whose installed version takes effect at the next start.
    pub fn restarts(&self) -> Vec<String> {
        lock(&self.restart).iter().cloned().collect()
    }

    /// The update history of one plugin, newest last.
    pub fn update_history(&self, id: &str) -> Vec<UpdateRecord> {
        lock(&self.state).updates.iter().filter(|r| r.id == id).cloned().collect()
    }

    /// Checks one plugin's channel. `force` is the window's button, once a minute; a plugin's own call is cached
    /// for an hour. Only the button's check enables [`Plugins::manual_check_ready`].
    pub fn check_update(&self, id: &str, force: bool) -> Result<update::Check, String> {
        let target = self.update_target(id)?;
        self.updater.check(&target, force, false)
    }

    /// Whether the user pressed Check for update for this plugin: Update now waits for that, while the startup
    /// check alone never lights the button.
    pub fn manual_check_ready(&self, id: &str) -> bool {
        self.updater.manual_check_ready(id)
    }

    /// Installs the version a check found. `approved` is the user's answer in the window; a plugin's own call
    /// leaves it false and the updater asks [`Consent`](update::Consent) when it must.
    pub fn apply_update(&self, id: &str, approved: bool) -> Result<update::Installed, String> {
        let target = self.update_target(id)?;
        self.updater.apply(&target, approved, update::By::Manual)
    }

    /// Swaps a plugin's folder with its newest backup (or `backup`, when given) and marks the restart.
    pub fn rollback_update(&self, id: &str, backup: Option<&Path>) -> Result<update::Installed, String> {
        let target = self.update_target(id)?;
        self.updater.rollback(&target, backup)
    }

    /// The backup folders of a plugin, newest first.
    pub fn backups(&self, id: &str) -> Vec<update::BackupInfo> {
        match self.update_target(id) {
            Ok(target) => self.updater.backups(&target),
            Err(_) => Vec::new(),
        }
    }

    fn update_target(&self, id: &str) -> Result<update::UpdateTarget, String> {
        let plugin = self.plugins.iter().find(|p| p.manifest.id == id).ok_or_else(|| format!("there is no plugin `{id}`"))?;
        let m = &plugin.manifest;
        let feed = m.update.as_ref().map(|u| u.feed.clone()).unwrap_or_default();
        let public_key = m.update.as_ref().and_then(|u| u.public_key.clone());
        Ok(update::UpdateTarget {
            id: m.id.clone(),
            name: m.name.clone(),
            version: m.version.clone(),
            folder: plugin.folder.clone(),
            manifest_path: plugin.manifest_path.clone(),
            feed,
            public_key,
            abi: m.abi,
        })
    }

    /// Whether any loaded plugin both asks for and was granted updating: the start's check has work to do.
    pub fn has_update_grants(&self) -> bool {
        self.plugins
            .iter()
            .any(|p| p.status.is_loaded() && p.manifest.asks_for_update() && self.permission_granted(&p.manifest.id, "update"))
    }

    /// The once-a-start check: each plugin the user allowed to update is checked in turn and a newer version is
    /// installed without a question. "Enable automatic updates" is the answer to the first channel too; a changed key, new
    /// permissions, an unsigned feed or the "ask every time" setting still stop it. No timer follows. Each failure
    /// is one line in the log, never a modal window.
    pub fn updates_on_start(&self) -> Vec<String> {
        let mut done = Vec::new();
        let ids: Vec<String> = self
            .plugins
            .iter()
            .filter(|p| p.status.is_loaded() && p.manifest.asks_for_update() && self.permission_granted(&p.manifest.id, "update"))
            .map(|p| p.manifest.id.clone())
            .collect();
        for id in ids {
            let Ok(target) = self.update_target(&id) else { continue };
            match self.updater.check(&target, false, false) {
                Ok(check) if check.available != check.current => {
                    // No button is pressed at start: the permission is the answer, and the version is installed
                    // for the next launch. Only a changed key, new permissions, an unsigned feed or "ask every
                    // time" can still refuse, and then the automatic install stops with one log line.
                    match self.updater.apply(&target, false, update::By::Auto) {
                        Ok(installed) => done.push(format!("{id}: {} installed, restart required", installed.installed)),
                        Err(e) => self.log.note(&id, Level::Info, format!("update: {e}")),
                    }
                }
                Ok(_) => {}
                Err(e) => self.log.note(&id, Level::Warning, format!("update check: {e}")),
            }
        }
        done
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

/// Whether a folder in the plugin folder belongs to the updater rather than to a plugin: an unpacking
/// `.update-<id>-…` folder, or a `<id>.backup-<date>-<time>` kept by [`Plugins::rollback_update`].
fn is_update_work(name: &str) -> bool {
    if name.starts_with(update::TEMP_PREFIX) {
        return true;
    }
    let Some((id, stamp)) = name.rsplit_once(update::BACKUP_PREFIX) else { return false };
    !id.is_empty() && is_stamp(stamp)
}

/// `YYYY-MM-DD-HH-MM-SS`, optionally followed by `-`, or by `-<n>` when two backups fell in one second. Older
/// versions wrote a trailing `-` where the date's `Z` was; it is still recognized.
fn is_stamp(stamp: &str) -> bool {
    let bytes = stamp.as_bytes();
    if bytes.len() < 19 {
        return false;
    }
    for (i, byte) in bytes[..19].iter().enumerate() {
        let good = match i {
            4 | 7 | 10 | 13 | 16 => *byte == b'-',
            _ => byte.is_ascii_digit(),
        };
        if !good {
            return false;
        }
    }
    match &bytes[19..] {
        [] | [b'-'] => true,
        [b'-', rest @ ..] => !rest.is_empty() && rest.iter().all(u8::is_ascii_digit),
        _ => false,
    }
}

/// The folder's state: the plugins switched off and the permissions the user granted. A folder written before
/// the state kept permissions has `disabled.json`, read as the switched-off list alone; a `state.json` beside it
/// is what is saved from now on.
fn read_state(folder: &Path, log: &Log) -> StateFile {
    let state = folder.join(STATE);
    let (path, legacy) = if state.is_file() { (state, false) } else { (folder.join(LEGACY_STATE), true) };
    let text = match std::fs::metadata(&path) {
        Ok(m) if m.len() > MAX_STATE => {
            log.note("plugins", Level::Warning, format!("{} is too large: the plugins' state is ignored", path.display()));
            return StateFile::default();
        }
        Ok(_) => match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                log.note("plugins", Level::Warning, format!("{}: {e}", path.display()));
                return StateFile::default();
            }
        },
        Err(_) => return StateFile::default(),
    };
    match serde_json::from_str::<StateFile>(&text) {
        Ok(file) => limited(file),
        // An older file had the disabled list alone; it is still one.
        Err(e) if legacy => match serde_json::from_str::<LegacyDisabled>(&text) {
            Ok(file) => StateFile { disabled: file.disabled, ..StateFile::default() },
            Err(_) => {
                log.note("plugins", Level::Warning, format!("{} isn't a plugin state: {e}", path.display()));
                StateFile::default()
            }
        },
        Err(e) => {
            log.note("plugins", Level::Warning, format!("{} isn't a plugin state: {e}", path.display()));
            StateFile::default()
        }
    }
}

/// A state file read, held to the limits: long lists are cut, the newest history stays.
fn limited(mut file: StateFile) -> StateFile {
    file.disabled.truncate(MAX_DISABLED);
    file.permissions.retain(|_, list| !list.is_empty());
    file.permissions = file
        .permissions
        .into_iter()
        .take(MAX_DISABLED)
        .map(|(id, mut list)| {
            list.truncate(MAX_GRANTS);
            (id, list)
        })
        .collect();
    file.update = file.update.into_iter().take(MAX_DISABLED).collect();
    if file.updates.len() > MAX_HISTORY {
        let extra = file.updates.len() - MAX_HISTORY;
        file.updates.drain(..extra);
    }
    file.settings.backups = file.settings.backups.min(10);
    file
}

/// The state file, written whole and atomically: a crash leaves the previous one, never half a file.
pub(crate) fn write_state(folder: &Path, state: &StateFile) -> Result<(), String> {
    std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let path = folder.join(STATE);
    let state = limited(state.clone());
    let text = serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?;
    let part = path.with_extension("json.part");
    std::fs::write(&part, text).map_err(|e| format!("{}: {e}", part.display()))?;
    std::fs::rename(&part, &path).map_err(|e| format!("{}: {e}", path.display()))
}

/// A lock that a panic can't leave poisoned: the state is a plain file, and a half-written one is refused by
/// the parsers anyway.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// The state file an older version wrote: the switched-off list alone.
#[derive(Deserialize)]
struct LegacyDisabled {
    #[serde(default)]
    disabled: Vec<String>,
}
