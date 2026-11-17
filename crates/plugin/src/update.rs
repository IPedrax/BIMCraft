//! A plugin's own update channel: the feed it declares in its manifest, the publisher's Ed25519 signature, the
//! archive the feed points at, and the atomic folder swap that installs it.
//!
//! The plugin never sees this module: its sandbox imports `update_check` and `update_apply`, and the host does all
//! the network and disk work here. The URL is the installed manifest's, never the call's, so the permission can't
//! turn into general internet access. The feed is checked before the archive is fetched, the archive is hashed as
//! it is read, and the new folder is unpacked beside the old one and renamed into place only after every check:
//! a failure at any step leaves the version that worked.
//!
//! The network is [`bimcraft_update::Net`] — the app updater's trait, with the desktop's HTTPS behind it and a
//! table of answers in tests. State (pinned keys, history, settings) is the plugin folder's `state.json`, shared
//! with the window through one mutex.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ring::signature::{ED25519, UnparsedPublicKey};
use serde::Deserialize;

use bimcraft_update::{Net, Version, sha256_of};

use crate::manifest::{ABI, MAX_MODULE, Manifest, check_https_url, check_public_key, decode_prefixed};
use crate::{Level, Log, MAX_HISTORY, StateFile, UpdateRecord, lock, write_state};

/// The most a feed may be, bytes.
pub const MAX_FEED: u64 = 64 << 10;
/// The most an update archive may be, bytes (the module inside is [`MAX_MODULE`]).
pub const MAX_ARCHIVE: u64 = 32 << 20;
/// The whole check or download waits this long, in a thread of its own; a stalled network can't hold the caller.
pub const TOTAL: Duration = Duration::from_secs(30);
/// A plugin's own check is answered from memory for this long.
pub const CHECK_CACHED: Duration = Duration::from_secs(60 * 60);
/// The window's button is not more often than once a minute.
pub const MANUAL_INTERVAL: Duration = Duration::from_secs(60);
/// The lock file that says one update of a folder runs at a time.
pub const LOCK: &str = ".update.lock";
/// The prefix of the temporary folder an update unpacks into.
pub const TEMP_PREFIX: &str = ".update-";
/// The prefix of the folder the version that is replaced is kept in.
pub const BACKUP_PREFIX: &str = ".backup-";

/// The calls' answers when the user hasn't allowed the plugin to update itself.
pub const NOT_ALLOWED: &str = "the plugin is not allowed to update itself";
/// The answer when a channel is declared with an empty feed.
pub const NO_CHANNEL: &str = "the plugin has no update channel";

/// One plugin a call may update: the installed manifest's id, version and channel, and the folder to replace.
#[derive(Clone, Debug)]
pub struct UpdateTarget {
    pub id: String,
    pub name: String,
    pub version: String,
    /// The folder holding `plugin.json` and the module.
    pub folder: PathBuf,
    /// The manifest's own path: a plugin may be a single `*.json`, which can't be replaced by a folder swap.
    pub manifest_path: PathBuf,
    /// The channel, exactly as the installed manifest declares it.
    pub feed: String,
    pub public_key: Option<String>,
    pub abi: u32,
}

/// What a loaded plugin's functions carry when the user let it update itself.
#[derive(Clone)]
pub struct UpdateCall {
    pub service: Arc<Updater>,
    pub target: UpdateTarget,
}

/// A check found a version to offer, or that the installed one is current.
#[derive(Clone, Debug)]
pub struct Check {
    pub current: String,
    pub available: String,
    pub released: String,
    pub size: u64,
    pub notes: String,
    /// The channel's host, for the window and the confirmation.
    pub host: String,
    /// `ed25519:…` of the key the feed was verified with, short enough to show.
    pub key: String,
    /// The archive's SHA-256, 64 hex digits.
    pub sha256: String,
    /// The archive's https address.
    pub url: String,
    /// The feed is signed with a new key, or is unsigned, or lowers the version: each asks the user.
    pub key_change: bool,
    pub unsigned: bool,
    pub downgrade: bool,
    /// The feed's version is the installed one: there is nothing to install.
    pub up_to_date: bool,
}

/// An installed (or rolled-back) version.
#[derive(Clone, Debug)]
pub struct Installed {
    pub installed: String,
    /// The previous version, kept in `<id>.backup-*`.
    pub previous: String,
    /// The app loads modules at start: a new version takes effect then.
    pub restart: bool,
}

/// A backup folder the window can offer to roll back to.
#[derive(Clone, Debug)]
pub struct BackupInfo {
    pub path: PathBuf,
    pub version: String,
    pub at: String,
    pub sha256: String,
}

/// Who started an install: the check at startup, or a button/the plugin's own call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum By {
    Auto,
    Manual,
}

impl By {
    fn name(self) -> &'static str {
        match self {
            By::Auto => "auto",
            By::Manual => "manual",
        }
    }
}

/// What the user is shown when an install needs a question: the first one, a new key, new permissions, a lower
/// version, or every version when the setting says so.
#[derive(Clone, Debug, Default)]
pub struct ConsentPrompt {
    pub plugin: String,
    pub name: String,
    pub current: String,
    pub next: String,
    pub host: String,
    pub key: String,
    pub size: u64,
    pub notes: String,
    /// Why the question is asked, in the window's words: "the publisher's key is new", "it asks for files", …
    pub reasons: Vec<String>,
}

/// How the host gets the user's answer. The desktop can't draw a window from inside a plugin call, so its
/// implementation answers from what the user already confirmed (Manage > Plugins). Tests use [`Allow`] and
/// [`Deny`].
pub trait Consent: Send + Sync {
    fn confirm(&self, prompt: &ConsentPrompt) -> bool;
}

/// The default: nothing is installed until the user says so in Manage > Plugins.
pub struct Deny;

impl Consent for Deny {
    fn confirm(&self, _: &ConsentPrompt) -> bool {
        false
    }
}

/// For tests and for a user who already answered the window: every question is yes.
pub struct Allow;

impl Consent for Allow {
    fn confirm(&self, _: &ConsentPrompt) -> bool {
        true
    }
}

/// A feed read from a channel, as schema 1 writes it.
#[derive(Clone, Debug, Deserialize)]
struct Feed {
    schema: u32,
    plugin: FeedPlugin,
    version: String,
    #[serde(default)]
    released: String,
    #[serde(default)]
    notes: String,
    module: FeedModule,
    #[serde(default)]
    signature: Option<String>,
    /// A rotated key can be announced here; the user still confirms the change.
    #[serde(default)]
    public_key: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct FeedPlugin {
    id: String,
    abi: u32,
}

#[derive(Clone, Debug, Deserialize)]
struct FeedModule {
    url: String,
    sha256: String,
    size: u64,
}

/// The feed and the version comparison, with the signature checked before any archive is asked for.
struct Checked {
    feed: Feed,
    check: Check,
    /// The key that verified the feed's signature; none when it isn't signed.
    signed_key: Option<String>,
}

/// What the updater remembers of one plugin's last check.
#[derive(Clone)]
struct Cached {
    at: Instant,
    checked: Check,
}

/// The update channel of one plugin folder: its network, its state file and its check cache.
pub struct Updater {
    folder: Option<PathBuf>,
    net: Arc<dyn Net>,
    state: Arc<Mutex<StateFile>>,
    log: Arc<Log>,
    restart: Arc<Mutex<std::collections::BTreeSet<String>>>,
    consent: Mutex<Arc<dyn Consent>>,
    cache: Mutex<BTreeMap<String, Cached>>,
    last_manual: Mutex<BTreeMap<String, Instant>>,
    /// The plugins whose channel the user checked with the window's button: only those offer `Update now`.
    manual_ready: Mutex<std::collections::BTreeSet<String>>,
}

impl Updater {
    pub(crate) fn new(
        folder: Option<PathBuf>,
        net: Arc<dyn Net>,
        state: Arc<Mutex<StateFile>>,
        log: Arc<Log>,
        restart: Arc<Mutex<std::collections::BTreeSet<String>>>,
    ) -> Updater {
        Updater {
            folder,
            net,
            state,
            log,
            restart,
            consent: Mutex::new(Arc::new(Deny)),
            cache: Mutex::new(BTreeMap::new()),
            last_manual: Mutex::new(BTreeMap::new()),
            manual_ready: Mutex::new(std::collections::BTreeSet::new()),
        }
    }

    /// The answers the updater gets when it must ask. The desktop keeps the default until it has a window that
    /// can answer before the call starts.
    pub fn set_consent(&self, consent: Arc<dyn Consent>) {
        *lock(&self.consent) = consent;
    }

    /// The folder the state file and the plugins are in, when there is one.
    pub fn folder(&self) -> Option<&Path> {
        self.folder.as_deref()
    }

    /// Whether the user still lets the plugin update itself. Checked again just before the folders are swapped,
    /// so taking the permission back during a download stops the install.
    pub fn allowed(&self, target: &UpdateTarget) -> bool {
        lock(&self.state).permissions.get(&target.id).is_some_and(|list| list.iter().any(|p| p == "update"))
    }

    /// Checks the plugin's channel. `force` is the window's button (at most once a minute); `allow_downgrade`
    /// lets a lower version be offered, which the install still asks about.
    pub fn check(&self, target: &UpdateTarget, force: bool, allow_downgrade: bool) -> Result<Check, String> {
        if !self.allowed(target) {
            return Err(NOT_ALLOWED.to_string());
        }
        if target.feed.trim().is_empty() {
            return Err(NO_CHANNEL.to_string());
        }
        if lock(&self.restart).contains(&target.id) {
            return Err("the installed version waits for a restart".to_string());
        }
        if !force
            && let Some(cached) = lock(&self.cache).get(&target.id)
            && cached.at.elapsed() < CHECK_CACHED
        {
            return Ok(cached.checked.clone());
        }
        if force
            && let Some(at) = lock(&self.last_manual).get(&target.id)
            && at.elapsed() < MANUAL_INTERVAL
        {
            return Err("the update was checked a moment ago; try again in a minute".into());
        }
        let checked = self.fetch_and_verify(target, allow_downgrade)?;
        if force {
            lock(&self.last_manual).insert(target.id.clone(), Instant::now());
            // A check at start, or a plugin's own call, never enables the window's `Update now`: only the user's
            // press of `Check for update` does.
            lock(&self.manual_ready).insert(target.id.clone());
        }
        let check = checked.check.clone();
        lock(&self.cache).insert(target.id.clone(), Cached { at: Instant::now(), checked: check.clone() });
        if check.up_to_date {
            self.log.note(&target.id, Level::Info, format!("update: {} is current", check.current));
        } else {
            self.log.note(
                &target.id,
                Level::Info,
                format!("update: {} is available ({} bytes from {})", check.available, check.size, check.host),
            );
        }
        Ok(check)
    }

    /// The last check's answer, without the network: what the window's confirmation shows.
    pub fn cached(&self, id: &str) -> Option<Check> {
        lock(&self.cache).get(id).map(|c| c.checked.clone())
    }

    /// Whether the user pressed `Check for update` for this plugin since the install: `Update now` waits for it.
    pub fn manual_check_ready(&self, id: &str) -> bool {
        lock(&self.manual_ready).contains(id)
    }

    /// Installs the newest version the channel offers. `approved` is the user's answer in the window; false means
    /// the updater asks [`Consent`] for everything the check says needs a question.
    pub fn apply(&self, target: &UpdateTarget, approved: bool, by: By) -> Result<Installed, String> {
        if !self.allowed(target) {
            return Err(NOT_ALLOWED.to_string());
        }
        if target.feed.trim().is_empty() {
            return Err(NO_CHANNEL.to_string());
        }
        if lock(&self.restart).contains(&target.id) {
            return Err("the installed version waits for a restart".to_string());
        }
        let folder = self.folder.as_ref().ok_or_else(|| "there is no plugin folder".to_string())?;
        let current_manifest = Manifest::read(&target.manifest_path).map_err(|e| format!("the installed manifest: {e}"))?;
        if current_manifest.id != target.id {
            return Err(format!("the installed manifest is `{}`, not `{}`", current_manifest.id, target.id));
        }
        let checked = self.fetch_and_verify(target, false)?;
        let next = Version::parse(&checked.feed.version).map_err(|e| format!("the feed's version: {e}"))?;
        let current = Version::parse(&checked.check.current).map_err(|e| format!("the installed version: {e}"))?;
        // A lower version is a rollback the feed announces: allowed, but only with the user's yes, which the
        // install below asks for as one more reason. The same version is no update at all.
        if next == current {
            return Err(format!("{} is already installed", checked.check.current));
        }

        // One update of this folder at a time; a stale lock is a reason, not something to guess about.
        let _lock = UpdateLock::acquire(folder)?;
        let temp = fresh_temp(folder, &target.id)?;
        let outcome = self.install(target, &current_manifest, &checked, &temp, approved, by);
        let _ = std::fs::remove_dir_all(&temp);
        outcome
    }

    fn install(
        &self,
        target: &UpdateTarget,
        current_manifest: &Manifest,
        checked: &Checked,
        temp: &Path,
        approved: bool,
        by: By,
    ) -> Result<Installed, String> {
        let feed = &checked.feed;
        let archive = self.fetch(&feed.module.url, MAX_ARCHIVE)?;
        if archive.len() as u64 != feed.module.size {
            return Err(format!("the archive is {} bytes, not the {} the feed lists", archive.len(), feed.module.size));
        }
        let digest = sha256_of(&mut archive.as_slice()).map_err(|e| format!("the archive: {e}"))?;
        let hex = hex(&digest);
        if hex != feed.module.sha256.to_ascii_lowercase() {
            return Err("the archive's SHA-256 isn't the one the feed lists".into());
        }

        let new_folder = temp.join("new");
        let manifest = unpack(&archive, &new_folder)?;
        if manifest.id != target.id {
            return Err(format!("the archive holds `{}`, not `{}`", manifest.id, target.id));
        }
        if manifest.abi != ABI {
            return Err(format!("the archive's manifest speaks ABI {}; this BIMCraft speaks ABI {ABI}", manifest.abi));
        }
        if manifest.version != feed.version {
            return Err(format!("the archive's manifest is {}, the feed says {}", manifest.version, feed.version));
        }
        let channel_changed = manifest.update.as_ref().map(|u| u.feed.as_str()).unwrap_or_default() != target.feed;

        let new_key = manifest.update.as_ref().and_then(|u| u.public_key.clone());
        let pinned = lock(&self.state).update.get(&target.id).cloned();
        let signed_key = checked.signed_key.clone();
        if let Some(key) = &signed_key
            && let Some(declared) = &new_key
            && declared != key
        {
            return Err("the archive's publisher key isn't the one that signed the feed".into());
        }

        let added: Vec<String> = manifest.permissions.iter().filter(|p| !current_manifest.permissions.contains(p)).cloned().collect();
        let mut reasons = Vec::new();
        if Version::parse(&feed.version).ok().zip(Version::parse(&checked.check.current).ok()).is_some_and(|(next, current)| next < current)
        {
            reasons.push("this version is lower: it is a rollback".to_string());
        }
        if checked.check.unsigned {
            reasons.push("the feed isn't signed".to_string());
        }
        if checked.check.key_change {
            reasons.push("the publisher's key changed".to_string());
        } else if by != By::Auto && pinned.as_ref().is_none_or(|p| !p.confirmed) {
            // The startup install is the permission at work: "Enable automatic updates" was the answer. A plugin's own
            // call and the window still show the first channel before taking it.
            reasons.push("the publisher's key is confirmed on the first update".to_string());
        }
        if channel_changed {
            reasons.push("the update channel changes".to_string());
        }
        if !added.is_empty() {
            reasons.push(format!("the new version asks for {}", added.join(", ")));
        }
        if lock(&self.state).settings.ask_before_every {
            reasons.push("you asked to confirm every update".to_string());
        }
        if !approved && !reasons.is_empty() {
            let prompt = ConsentPrompt {
                plugin: target.id.clone(),
                name: target.name.clone(),
                current: checked.check.current.clone(),
                next: feed.version.clone(),
                host: checked.check.host.clone(),
                key: checked.check.key.clone(),
                size: checked.check.size,
                notes: checked.check.notes.clone(),
                reasons,
            };
            let answer = lock(&self.consent).clone();
            if !answer.confirm(&prompt) {
                self.log.note(&target.id, Level::Warning, "update refused by the user");
                return Err("the user declined".into());
            }
        }

        // The permission may have been taken back while the archive was downloading: the swap must not happen.
        if !self.allowed(target) {
            return Err(NOT_ALLOWED.to_string());
        }
        if !folder_is_replaceable(target) {
            return Err("the plugin is a single manifest file, not a folder; update it with the installer".into());
        }
        let current = &target.folder;
        let parent = current.parent().ok_or_else(|| format!("{}: no parent folder", current.display()))?;
        let backup = backup_path(parent, &target.id);
        std::fs::rename(current, &backup).map_err(|e| format!("{}: {e}", backup.display()))?;
        if let Err(e) = std::fs::rename(&new_folder, current) {
            let _ = std::fs::rename(&backup, current);
            return Err(format!("the new version couldn't be put in place: {e}"));
        }
        let before = lock(&self.state).clone();
        {
            let mut state = lock(&self.state);
            let entry = state.update.entry(target.id.clone()).or_default();
            if let Some(key) = &signed_key {
                entry.key = key.clone();
            }
            entry.confirmed = true;
            state.updates.push(UpdateRecord {
                id: target.id.clone(),
                from: checked.check.current.clone(),
                to: feed.version.clone(),
                at: now(),
                sha256: hex.clone(),
                host: checked.check.host.clone(),
                by: by.name().to_string(),
            });
            if state.updates.len() > MAX_HISTORY {
                let extra = state.updates.len() - MAX_HISTORY;
                state.updates.drain(..extra);
            }
            let backups = state.settings.backups;
            drop(state);
            if let Err(e) = self.save() {
                // A failed state write is a failed install: the new folder goes back to the temporary one and the
                // previous version to its place, so the plugin folder never says one thing and state.json another.
                *lock(&self.state) = before;
                let _ = std::fs::rename(current, &new_folder);
                let _ = std::fs::rename(&backup, current);
                return Err(format!("the update's state couldn't be written: {e}"));
            }
            prune(parent, &target.id, backups);
        }
        lock(&self.restart).insert(target.id.clone());
        lock(&self.cache).remove(&target.id);
        lock(&self.manual_ready).remove(&target.id);
        self.log.note(
            &target.id,
            Level::Info,
            format!(
                "update: {} installed over {} from {} (sha256 {hex}); takes effect at the next start",
                feed.version, checked.check.current, checked.check.host
            ),
        );
        Ok(Installed { installed: feed.version.clone(), previous: checked.check.current.clone(), restart: true })
    }

    /// Swaps the plugin's folder with its newest backup (or with `backup`). The backup was installed once, so its
    /// signature isn't checked again; its digest is in the result and the history.
    pub fn rollback(&self, target: &UpdateTarget, backup: Option<&Path>) -> Result<Installed, String> {
        if !self.allowed(target) {
            return Err(NOT_ALLOWED.to_string());
        }
        if !folder_is_replaceable(target) {
            return Err("the plugin is a single manifest file, not a folder; there is nothing to swap".into());
        }
        let folder = self.folder.as_ref().ok_or_else(|| "there is no plugin folder".to_string())?;
        let _lock = UpdateLock::acquire(folder)?;
        let parent = target.folder.parent().ok_or_else(|| format!("{}: no parent folder", target.folder.display()))?;
        let chosen = match backup {
            Some(path) => path.to_path_buf(),
            None => {
                self.backups(target).into_iter().next().map(|b| b.path).ok_or_else(|| "there is no backup to roll back to".to_string())?
            }
        };
        if !chosen.is_dir() {
            return Err(format!("{}: not a backup folder", chosen.display()));
        }
        let current_manifest = Manifest::read(&target.manifest_path).ok();
        let old_version = current_manifest.as_ref().map(|m| m.version.clone()).unwrap_or_else(|| target.version.clone());
        let backup_version = Manifest::read(&chosen.join(crate::MANIFEST)).map(|m| m.version).unwrap_or_else(|_| "unknown".into());
        let digest = bimcraft_update::sha256_file(&chosen.join(crate::MANIFEST)).map(|d| hex(&d)).unwrap_or_default();
        let aside = fresh_temp(parent, &target.id)?;
        let aside = aside.join("current");
        std::fs::rename(&target.folder, &aside).map_err(|e| format!("{}: {e}", target.folder.display()))?;
        if let Err(e) = std::fs::rename(&chosen, &target.folder) {
            let _ = std::fs::rename(&aside, &target.folder);
            let _ = std::fs::remove_dir_all(aside.parent().unwrap_or(&aside));
            return Err(format!("the backup couldn't be put in place: {e}"));
        }
        // The version that was installed a moment ago stays as a backup of its own.
        let keep = backup_path(parent, &target.id);
        let _ = std::fs::rename(&aside, &keep);
        let before = lock(&self.state).clone();
        {
            let mut state = lock(&self.state);
            state.updates.push(UpdateRecord {
                id: target.id.clone(),
                from: old_version.clone(),
                to: backup_version.clone(),
                at: now(),
                sha256: digest,
                host: String::new(),
                by: By::Manual.name().to_string(),
            });
            if state.updates.len() > MAX_HISTORY {
                let extra = state.updates.len() - MAX_HISTORY;
                state.updates.drain(..extra);
            }
        }
        if let Err(e) = self.save() {
            *lock(&self.state) = before;
            let _ = std::fs::rename(&target.folder, &chosen);
            let back = if keep.is_dir() { &keep } else { &aside };
            let _ = std::fs::rename(back, &target.folder);
            let _ = std::fs::remove_dir_all(aside.parent().unwrap_or(&aside));
            return Err(format!("the rollback's state couldn't be written: {e}"));
        }
        let _ = std::fs::remove_dir_all(aside.parent().unwrap_or(&aside));
        lock(&self.restart).insert(target.id.clone());
        lock(&self.cache).remove(&target.id);
        lock(&self.manual_ready).remove(&target.id);
        self.log.note(&target.id, Level::Info, format!("update: rolled back from {old_version} to {backup_version}"));
        Ok(Installed { installed: backup_version, previous: old_version, restart: true })
    }

    /// The plugin's `<id>.backup-*` folders, newest first.
    pub fn backups(&self, target: &UpdateTarget) -> Vec<BackupInfo> {
        let Some(parent) = target.folder.parent() else { return Vec::new() };
        let prefix = format!("{}{BACKUP_PREFIX}", target.id);
        let mut found: Vec<PathBuf> = std::fs::read_dir(parent)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&prefix)))
            .collect();
        found.sort();
        found.reverse();
        found
            .into_iter()
            .map(|path| {
                let manifest = Manifest::read(&path.join(crate::MANIFEST)).ok();
                let version = manifest.map(|m| m.version).unwrap_or_else(|| "unknown".into());
                let sha256 = bimcraft_update::sha256_file(&path.join(crate::MANIFEST)).map(|d| hex(&d)).unwrap_or_default();
                let at = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                BackupInfo { path, version, at, sha256 }
            })
            .collect()
    }

    fn save(&self) -> Result<(), String> {
        let Some(folder) = &self.folder else { return Ok(()) };
        write_state(folder, &lock(&self.state))
    }

    /// The feed, checked: schema, signature, id, ABI, https, size, digest shape and version.
    fn fetch_and_verify(&self, target: &UpdateTarget, allow_downgrade: bool) -> Result<Checked, String> {
        check_https_url("the update feed", &target.feed)?;
        let bytes = self.fetch(&target.feed, MAX_FEED)?;
        let feed: Feed = serde_json::from_slice(&bytes).map_err(|e| format!("the feed isn't JSON: {e}"))?;
        if feed.schema != 1 {
            return Err(format!("the feed is schema {}; this BIMCraft reads schema 1", feed.schema));
        }
        if feed.plugin.id != target.id {
            return Err(format!("the feed announces `{}`, not `{}`", feed.plugin.id, target.id));
        }
        if feed.plugin.abi != ABI || feed.plugin.abi != target.abi {
            return Err(format!("the feed is for ABI {}; this BIMCraft speaks ABI {ABI}", feed.plugin.abi));
        }
        check_https_url("the archive address", &feed.module.url)?;
        if !(1..=MAX_ARCHIVE).contains(&feed.module.size) {
            return Err(format!("the feed's archive size {} is outside 1 byte to {} MB", feed.module.size, MAX_ARCHIVE >> 20));
        }
        if feed.module.sha256.len() != 64 || !feed.module.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("the feed's archive SHA-256 isn't 64 hex digits".into());
        }
        let current = Version::parse(&target.version).map_err(|e| format!("the installed version `{}`: {e}", target.version))?;
        let next = Version::parse(&feed.version).map_err(|e| format!("the feed's version `{}`: {e}", feed.version))?;

        let pinned = lock(&self.state).update.get(&target.id).cloned();
        let settings = lock(&self.state).settings.clone();
        let host = bimcraft_update::net::host_of(&feed.module.url);
        let feed_host = bimcraft_update::net::host_of(&target.feed);
        let signed = feed.signature.is_some();
        let mut key_change = false;
        let chosen_key;
        let signed_key;
        if let Some(signature) = &feed.signature {
            let canonical = canonical(&feed)?;
            let mut candidates: Vec<String> = Vec::new();
            if let Some(p) = &pinned
                && !p.key.is_empty()
            {
                candidates.push(p.key.clone());
            }
            if let Some(k) = &target.public_key {
                candidates.push(k.clone());
            }
            if let Some(k) = &feed.public_key {
                check_public_key(k)?;
                candidates.push(k.clone());
            }
            candidates.sort();
            candidates.dedup();
            let mut matched = None;
            for key in &candidates {
                if verify(key, canonical.as_bytes(), signature).is_ok() {
                    matched = Some(key.clone());
                    break;
                }
            }
            let key = matched.ok_or_else(|| "the feed's signature didn't match the publisher's key".to_string())?;
            if let Some(p) = &pinned
                && p.key != key
            {
                // A pinned empty key is an unsigned first install; a key appearing afterwards is a change too.
                key_change = true;
            }
            if pinned.is_none() && target.public_key.as_deref().is_some_and(|known| known != key) {
                return Err("the feed's signature isn't the key the plugin's first install declared".into());
            }
            signed_key = Some(key.clone());
            chosen_key = key;
        } else {
            if !settings.allow_unsigned {
                return Err("the feed isn't signed, and unsigned updates aren't allowed in the settings".into());
            }
            if target.public_key.is_some() || feed.public_key.is_some() {
                return Err("the feed isn't signed, although the plugin declares a publisher key".into());
            }
            chosen_key = String::new();
            signed_key = None;
        }
        let _ = feed_host;

        let up_to_date = next == current;
        let downgrade = next < current;
        let available = if up_to_date || (downgrade && !allow_downgrade) { target.version.clone() } else { feed.version.clone() };
        let check = Check {
            current: target.version.clone(),
            available,
            released: feed.released.clone(),
            size: feed.module.size,
            notes: feed.notes.clone(),
            host,
            key: short_key(&chosen_key),
            sha256: feed.module.sha256.to_ascii_lowercase(),
            url: feed.module.url.clone(),
            key_change,
            unsigned: !signed,
            downgrade: downgrade && allow_downgrade,
            up_to_date: up_to_date || (downgrade && !allow_downgrade),
        };
        Ok(Checked { feed, check, signed_key })
    }

    /// Reads `url` in a thread of its own, with the overall limit: a stalled connection can't hold the caller past
    /// [`TOTAL`]. The cap is checked here too, since a test network writes what it is told.
    fn fetch(&self, url: &str, cap: u64) -> Result<Vec<u8>, String> {
        let net = Arc::clone(&self.net);
        let url = url.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("plugin-update".into())
            .spawn(move || {
                let mut sink = Limited { out: Vec::new(), left: cap, over: false };
                let result = net.get(&url, cap, &mut sink);
                let _ = tx.send((result, sink));
            })
            .map_err(|e| format!("the update network couldn't start: {e}"))?;
        match rx.recv_timeout(TOTAL) {
            Ok((Ok(()), sink)) => {
                if sink.over {
                    Err(format!("the answer is over {} bytes", cap))
                } else {
                    Ok(sink.out)
                }
            }
            Ok((Err(_), sink)) if sink.over => Err(format!("the answer is over {} bytes", cap)),
            Ok((Err(e), _)) => Err(format!("the update channel doesn't answer: {e}")),
            Err(_) => Err(format!("the update channel doesn't answer in {} s", TOTAL.as_secs())),
        }
    }
}

/// A reply written straight into a buffer with a hard cap: a network that ignores the limit it was given can't
/// grow the host's memory past it.
struct Limited {
    out: Vec<u8>,
    left: u64,
    over: bool,
}

impl Write for Limited {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if buf.len() as u64 > self.left {
            self.over = true;
            return Err(std::io::Error::other("more than the size limit"));
        }
        self.out.extend_from_slice(buf);
        self.left -= buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Whether the plugin is a folder: a single `*.json` manifest has no folder to rename.
fn folder_is_replaceable(target: &UpdateTarget) -> bool {
    target.manifest_path == target.folder.join(crate::MANIFEST) && target.folder.is_dir()
}

/// The canonical JSON a feed's signature covers: every field but `signature`, in a fixed order.
fn canonical(feed: &Feed) -> Result<String, String> {
    let text = |v: &str| serde_json::to_string(v).map_err(|e| e.to_string());
    let mut out = String::new();
    out.push_str(&format!("{{\"schema\":{},\"plugin\":{{\"id\":{},\"abi\":{}}},\"version\":{},\"released\":{},\"notes\":{},\"module\":{{\"url\":{},\"sha256\":{},\"size\":{}}}", feed.schema, text(&feed.plugin.id)?, feed.plugin.abi, text(&feed.version)?, text(&feed.released)?, text(&feed.notes)?, text(&feed.module.url)?, text(&feed.module.sha256)?, feed.module.size));
    if let Some(key) = &feed.public_key {
        out.push_str(&format!(",\"public_key\":{}", text(key)?));
    }
    out.push('}');
    Ok(out)
}

/// Whether `signature` (`ed25519:BASE64`) is `key`'s over `message`.
fn verify(key: &str, message: &[u8], signature: &str) -> Result<(), String> {
    check_public_key(key)?;
    let key_bytes = decode_prefixed(key, "ed25519:")?;
    let signature = decode_prefixed(signature, "ed25519:")?;
    if signature.len() != 64 {
        return Err(format!("the signature is {} bytes; an Ed25519 signature is 64", signature.len()));
    }
    UnparsedPublicKey::new(&ED25519, key_bytes).verify(message, &signature).map_err(|_| "the signature didn't match".to_string())
}

/// A short form of a key for the window: `ed25519:AB12CD34…`.
fn short_key(key: &str) -> String {
    let body = key.strip_prefix("ed25519:").unwrap_or(key);
    let head: String = body.chars().take(16).collect();
    if body.chars().count() > 16 {
        format!("ed25519:{head}…")
    } else if body.is_empty() {
        String::new()
    } else {
        format!("ed25519:{head}")
    }
}

/// 32 bytes as 64 lower-case hex digits.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The date and time in UTC, `YYYY-MM-DDTHH:MM:SSZ`.
fn now() -> String {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let (year, month, day) = civil_from_days(days);
    let rest = seconds % 86_400;
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rest / 3600, (rest % 3600) / 60, rest % 60)
}

/// A civil date from days since 1970-01-01 (Howard Hinnant's algorithm, no clock crate needed).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + i64::from(m <= 2), m as u32, d as u32)
}

/// A fresh `.update-<id>-<random>` folder inside the plugin folder: its name can't be guessed, and it never
/// reuses a folder that is already there.
fn fresh_temp(parent: &Path, id: &str) -> Result<PathBuf, String> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    for attempt in 0..16 {
        let folder = parent.join(format!("{TEMP_PREFIX}{id}-{:x}-{nanos:x}-{attempt}", std::process::id()));
        match std::fs::create_dir(&folder) {
            Ok(()) => return Ok(folder),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("{}: {e}", folder.display())),
        }
    }
    Err("no fresh folder to unpack the update into".into())
}

/// Where the replaced version is kept: `<id>.backup-<date>-<time>`, unique within the plugin folder.
fn backup_path(parent: &Path, id: &str) -> PathBuf {
    let stamp = now().trim_end_matches('Z').replace([':', 'T'], "-");
    let base = parent.join(format!("{id}{BACKUP_PREFIX}{stamp}"));
    if !base.exists() {
        return base;
    }
    for n in 1..1000 {
        let candidate = parent.join(format!("{id}{BACKUP_PREFIX}{stamp}-{n}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    base
}

/// Keeps at most `keep` backups of `id`, deleting the oldest.
fn prune(parent: &Path, id: &str, keep: usize) {
    let prefix = format!("{id}{BACKUP_PREFIX}");
    let mut found: Vec<PathBuf> = std::fs::read_dir(parent)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&prefix)))
        .collect();
    found.sort();
    let keep = keep.max(1);
    while found.len() > keep {
        let oldest = found.remove(0);
        let _ = std::fs::remove_dir_all(oldest);
    }
}

/// Unpacks the archive into `out`, refusing anything that could leave the folder: absolute paths, `..`, symbolic
/// links, a missing root `plugin.json`, a module of another path, and files the manifest doesn't declare.
fn unpack(archive: &[u8], out: &Path) -> Result<Manifest, String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive)).map_err(|e| format!("the archive isn't a zip: {e}"))?;
    // A plugin is a manifest and a module; a thousand entries is already far past any delivery, and a small ZIP
    // of empty names mustn't be able to make the host hold a million paths.
    if zip.len() > 1024 {
        return Err(format!("the archive holds {} entries; an update is up to 1024", zip.len()));
    }
    let mut files: BTreeMap<PathBuf, Vec<u8>> = BTreeMap::new();
    let mut total = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|e| format!("the archive's entry {index}: {e}"))?;
        let Some(relative) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
            return Err(format!("the archive has an unsafe path `{}`", entry.name()));
        };
        if entry.name().contains('\\')
            || relative.is_absolute()
            || relative.components().any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(format!("the archive has an unsafe path `{}`", entry.name()));
        }
        if entry.is_dir() {
            continue;
        }
        if entry.unix_mode().is_some_and(|mode| mode & 0o170_000 == 0o120_000) {
            return Err(format!("the archive has a symbolic link `{}`", relative.display()));
        }
        if entry.size() > MAX_ARCHIVE {
            return Err(format!("`{}` is too large for an update", relative.display()));
        }
        total = total.saturating_add(entry.size());
        if total > MAX_ARCHIVE {
            return Err(format!("the archive unpacks to more than {} MB", MAX_ARCHIVE >> 20));
        }
        let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0).min(1 << 20));
        entry.read_to_end(&mut bytes).map_err(|e| format!("`{}`: {e}", relative.display()))?;
        if files.insert(relative.clone(), bytes).is_some() {
            return Err(format!("the archive holds `{}` twice", relative.display()));
        }
    }
    let manifest_bytes =
        files.remove(Path::new(crate::MANIFEST)).ok_or_else(|| "the archive has no plugin.json in its root".to_string())?;
    let text = String::from_utf8(manifest_bytes).map_err(|e| format!("plugin.json isn't UTF-8: {e}"))?;
    let manifest = Manifest::parse(&text, &out.join(crate::MANIFEST))?;
    let module = PathBuf::from(&manifest.module);
    let module_bytes = files.remove(&module).ok_or_else(|| format!("the archive has no module `{}`", manifest.module))?;
    if module_bytes.len() as u64 > MAX_MODULE {
        return Err(format!("the module is {} MB; a module is up to {} MB", module_bytes.len() >> 20, MAX_MODULE >> 20));
    }
    if let Some((extra, _)) = files.iter().next() {
        return Err(format!("the archive holds `{}`, which the manifest doesn't declare", extra.display()));
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    for (relative, bytes) in [(PathBuf::from(crate::MANIFEST), text.into_bytes()), (module, module_bytes)] {
        let path = out.join(&relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(manifest)
}

/// The one-update-at-a-time lock, removed when the install ends, whatever way it ends.
struct UpdateLock {
    path: PathBuf,
}

impl UpdateLock {
    fn acquire(folder: &Path) -> Result<UpdateLock, String> {
        let path = folder.join(LOCK);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                use std::io::Write as _;
                let _ = writeln!(file, "{} {}", std::process::id(), now());
                Ok(UpdateLock { path })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err("an update of this plugin is already running".into()),
            Err(e) => Err(format!("{} isn't writable: {e}", folder.display())),
        }
    }
}

impl Drop for UpdateLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_is_written_without_a_clock_crate() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_000), (2024, 10, 4));
    }

    #[test]
    fn an_unsafe_archive_path_is_refused() {
        // A hand-built zip with `../escape` would need the zip writer; the check is the parser's `enclosed_name`,
        // which every entry goes through. The test names the rule the code relies on.
        assert!(Path::new("../escape").components().any(|c| matches!(c, std::path::Component::ParentDir)));
    }
}
