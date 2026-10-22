//! The updater as the desktop drives it: a check at startup or by hand, an offer, a download, the install. The work runs on
//! threads of its own and comes back through a channel the window polls each frame, so nothing waits on the network or the
//! disk, and a panic in what it runs is an error, not a check that never finishes.

use std::ffi::OsString;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use semver::Version;

use crate::net::Ureq;
use crate::prefs::Prefs;
use crate::{Install, Net, Release, Target, check, detect, download, install};

/// Set (to anything but nothing), this keeps the app from phoning home at all: no check, no menu items.
const NO_CHECK: &str = "BIMCRAFT_NO_UPDATE_CHECK";

type Job = Box<dyn FnOnce() + Send>;

/// What an install is given: the downloaded update, which has passed its checks, and what it is for.
#[derive(Clone, Debug, PartialEq)]
pub struct Handover {
    pub version: Version,
    /// The file, in a folder an update made.
    pub file: PathBuf,
    pub sha256: [u8; 32],
    /// The project open now, which the new version opens again: an absolute path, since the new version may start somewhere else.
    pub project: Option<PathBuf>,
}

/// What puts an update in place: [`Install::hand_over`], or in a test a stand-in.
type HandOver = Arc<dyn Fn(&Handover) -> Result<(), String> + Send + Sync>;

/// Where the updater stands, which the window draws.
#[derive(Clone, Debug)]
pub enum State {
    /// Nothing to say.
    Idle,
    /// A newer release exists, and the user is asked.
    Available(Release),
    /// Its installer is coming down: how many bytes are in.
    Downloading { release: Release, done: Arc<AtomicU64> },
    /// Its installer is here and has passed its checks.
    Ready { release: Release, file: PathBuf },
    /// It is being put in place, which takes a moment, and nothing may change until it has gone through or failed.
    Installing { release: Release, file: PathBuf },
}

/// What the window says once, in its status box.
#[derive(Clone, Debug, PartialEq)]
pub enum Note {
    /// A check by hand found nothing newer.
    UpToDate,
    /// A check by hand failed, and why.
    CheckFailed(String),
    /// A download failed or didn't pass its checks, and why.
    DownloadFailed(String),
    /// The update is in, and the new version has been started (on Windows, will be once this one has exited): the window closes.
    Installed,
    /// The update could not be put in place, and why. It is ready to try again.
    InstallFailed(String),
}

enum Msg {
    Checked { manual: bool, result: Result<Option<Release>, String> },
    Downloaded(Result<PathBuf, String>),
    Installed(Result<(), String>),
}

pub struct Updater {
    net: Arc<dyn Net>,
    current: Version,
    prefs: Prefs,
    prefs_path: Option<PathBuf>,
    state: State,
    checking: bool,
    /// The last check found nothing newer than this version (or only a version the user skipped).
    up_to_date: bool,
    target: Option<Target>,
    /// What an update replaces, when this copy can be updated in place.
    install: Option<Arc<Install>>,
    /// Where downloads get their folders.
    temp: PathBuf,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// Asks the window for a frame, when work comes back.
    wake: Arc<dyn Fn() + Send + Sync>,
    spawn: fn(Job),
    hand_over: HandOver,
}

/// `work`'s result, or an error if it panicked.
fn guarded<T>(work: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(work)).unwrap_or_else(|_| Err("the updater failed unexpectedly".into()))
}

impl Updater {
    /// An updater of `current` on `net`, with its preferences read from `prefs_path` (kept there when changed), that calls
    /// `wake` when work comes back for the window to take.
    pub fn new(net: Arc<dyn Net>, current: Version, prefs_path: Option<PathBuf>, wake: impl Fn() + Send + Sync + 'static) -> Updater {
        let (tx, rx) = channel();
        let mut updater = Updater {
            net,
            current,
            prefs: prefs_path.as_deref().map(Prefs::load).unwrap_or_default(),
            prefs_path,
            state: State::Idle,
            checking: false,
            up_to_date: false,
            target: None,
            install: None,
            temp: std::env::temp_dir(),
            tx,
            rx,
            wake: Arc::new(wake),
            spawn: |job| {
                std::thread::spawn(job);
            },
            hand_over: Arc::new(|_| Err("this copy can't be updated in place".into())),
        };
        if let Some(install) = Install::detect() {
            updater = updater.found(install);
        }
        updater
    }

    /// The desktop app's: this version, GitHub over HTTPS, the user's own settings file at `prefs_path` ([`prefs::beside`] the favorites;
    /// none keeps nothing). None when the environment variable `BIMCRAFT_NO_UPDATE_CHECK` is set, for a machine that must not phone
    /// home, and in a Flatpak, which has no network.
    pub fn desktop(prefs_path: Option<PathBuf>, wake: impl Fn() + Send + Sync + 'static) -> Option<Updater> {
        let flatpak = detect::in_flatpak(std::env::var_os("FLATPAK_ID"), Path::new("/.flatpak-info").exists());
        if !phones_home(std::env::var_os(NO_CHECK), flatpak) {
            return None;
        }
        let current = Version::parse(env!("CARGO_PKG_VERSION")).ok()?;
        Some(Updater::new(Arc::new(Ureq::new()), current, prefs_path, wake))
    }

    /// For tests: does its work on the thread that asks, so a result is there to poll at once.
    pub fn inline(mut self) -> Updater {
        self.spawn = |job| job();
        self
    }

    /// This copy is `install`: its target decides the file an update takes, and it is what an install replaces. `new` does this with
    /// what it detects; a test does it with any.
    pub fn found(mut self, install: Install) -> Updater {
        self.target = Some(install.target);
        let install = Arc::new(install);
        self.install = Some(Arc::clone(&install));
        self.hand_over = Arc::new(move |job| install.hand_over(job));
        self
    }

    /// For tests: the kind of install this copy is, which decides the file an update takes (none: it takes none).
    pub fn target(mut self, target: Option<Target>) -> Updater {
        self.target = target;
        self
    }

    /// For tests: `install` calls this, with the download, its digest and the project to reopen, instead of replacing this program.
    pub fn handing_over(mut self, f: impl Fn(&Handover) -> Result<(), String> + Send + Sync + 'static) -> Updater {
        self.hand_over = Arc::new(f);
        self
    }

    /// For tests: downloads go in folders under `temp`.
    pub fn in_folder(mut self, temp: PathBuf) -> Updater {
        self.temp = temp;
        self
    }

    /// The version running.
    pub fn current(&self) -> &Version {
        &self.current
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// Whether a check is on its way.
    pub fn checking(&self) -> bool {
        self.checking
    }

    /// Whether the last check found this version the latest (the start page says so).
    pub fn up_to_date(&self) -> bool {
        self.up_to_date
    }

    /// Whether BIMCraft looks for a newer version when it starts.
    pub fn checks_at_startup(&self) -> bool {
        self.prefs.check
    }

    pub fn set_checks_at_startup(&mut self, on: bool) -> Result<(), String> {
        self.prefs.check = on;
        self.save()
    }

    /// Looks for a newer version, unless the user switched that off, and without asking the window to wait.
    pub fn check_at_startup(&mut self) {
        if self.prefs.check {
            self.start_check(false);
        }
    }

    /// A check the user asked for: it says when there is nothing newer or it failed, and shows a version they skipped.
    pub fn check_now(&mut self) {
        self.start_check(true);
    }

    fn start_check(&mut self, manual: bool) {
        if self.checking {
            return;
        }
        self.checking = true;
        let (net, current, temp, tx, wake) =
            (self.net.clone(), self.current.clone(), self.temp.clone(), self.tx.clone(), self.wake.clone());
        let (target, install) = (self.target, self.install.clone());
        // An update ready to install keeps its folder, however long the banner has been up.
        let keep = match &self.state {
            State::Ready { file, .. } | State::Installing { file, .. } => file.parent().map(Path::to_path_buf),
            _ => None,
        };
        (self.spawn)(Box::new(move || {
            install::sweep(&temp, keep.as_deref());
            let result = guarded(|| Ok(check(&*net, &current, target.as_ref())?.map(|release| fit(release, install.as_deref()))));
            let _ = tx.send(Msg::Checked { manual, result });
            wake();
        }));
    }

    /// Takes what the threads have sent since the last call; a note for the status box when one is due.
    pub fn poll(&mut self) -> Option<Note> {
        let mut note = None;
        while let Ok(msg) = self.rx.try_recv() {
            note = self.apply(msg).or(note);
        }
        note
    }

    fn apply(&mut self, msg: Msg) -> Option<Note> {
        match msg {
            Msg::Checked { manual, result } => {
                self.checking = false;
                // A check that comes back after the download began has nothing to add.
                if matches!(self.state, State::Downloading { .. } | State::Ready { .. } | State::Installing { .. }) {
                    return None;
                }
                match result {
                    Err(e) => manual.then_some(Note::CheckFailed(e)),
                    Ok(None) => {
                        self.up_to_date = true;
                        manual.then_some(Note::UpToDate)
                    }
                    Ok(Some(release)) => {
                        let skipped = self.prefs.skip.as_ref().is_some_and(|s| release.version <= *s);
                        self.up_to_date = false;
                        self.state = if skipped && !manual { State::Idle } else { State::Available(release) };
                        None
                    }
                }
            }
            Msg::Downloaded(result) => match std::mem::replace(&mut self.state, State::Idle) {
                State::Downloading { release, .. } => match result {
                    Ok(file) => {
                        self.state = State::Ready { release, file };
                        None
                    }
                    Err(e) => {
                        self.state = State::Available(release);
                        Some(Note::DownloadFailed(e))
                    }
                },
                other => {
                    self.state = other;
                    None
                }
            },
            Msg::Installed(result) => match std::mem::replace(&mut self.state, State::Idle) {
                State::Installing { release, file } => match result {
                    Ok(()) => Some(Note::Installed),
                    Err(e) => {
                        self.state = State::Ready { release, file };
                        Some(Note::InstallFailed(e))
                    }
                },
                other => {
                    self.state = other;
                    None
                }
            },
        }
    }

    /// Starts downloading the offered release's installer, which is checked as it comes. Nothing happens where the release has no
    /// file for this kind of install (or this copy can't be updated in place, which comes to the same).
    pub fn update_now(&mut self) {
        let State::Available(release) = &self.state else { return };
        let Some(installer) = release.installer.clone() else { return };
        let done = Arc::new(AtomicU64::new(0));
        self.state = State::Downloading { release: release.clone(), done: done.clone() };
        let (net, temp, tx, wake) = (self.net.clone(), self.temp.clone(), self.tx.clone(), self.wake.clone());
        (self.spawn)(Box::new(move || {
            let result = guarded(|| download(&*net, &installer, &temp, &mut |n| done.store(n, Ordering::Relaxed)));
            let _ = tx.send(Msg::Downloaded(result));
            wake();
        }));
    }

    /// Hides the offer for this run: BIMCraft asks again the next time it starts.
    pub fn later(&mut self) {
        if matches!(self.state, State::Available(_) | State::Ready { .. }) {
            self.state = State::Idle;
        }
    }

    /// Hides the offer, and everything up to its version, for good: only a newer release is offered after this.
    pub fn skip(&mut self) -> Result<(), String> {
        let State::Available(release) = &self.state else { return Ok(()) };
        self.prefs.skip = Some(release.version.clone());
        self.state = State::Idle;
        self.save()
    }

    /// Puts the downloaded update in place, on a thread of its own: `poll` says how it went. On `Note::Installed` the caller closes its
    /// window, which an installer needs (it can't replace a program that's running). `project`, the file open now, is opened again
    /// once the new version starts. Nothing happens where no update is ready.
    pub fn install(&mut self, project: Option<&Path>) {
        let State::Ready { release, file } = &self.state else { return };
        let Some(installer) = &release.installer else { return };
        let job = Handover {
            version: release.version.clone(),
            file: file.clone(),
            sha256: installer.sha256,
            project: project.map(|p| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())),
        };
        self.state = State::Installing { release: release.clone(), file: file.clone() };
        let (hand_over, tx, wake) = (self.hand_over.clone(), self.tx.clone(), self.wake.clone());
        (self.spawn)(Box::new(move || {
            let _ = tx.send(Msg::Installed(guarded(|| hand_over(&job))));
            wake();
        }));
    }

    fn save(&self) -> Result<(), String> {
        self.prefs_path.as_deref().map_or(Ok(()), |path| self.prefs.save(path))
    }
}

/// `release` as this copy can use it: where this user can't write beside the program, the update has no file to install, and the release
/// is only told of. It is looked at here, once there is something to offer, because it writes a file to find out.
fn fit(release: Release, install: Option<&Install>) -> Release {
    if install.is_some_and(|install| !install.replaceable()) {
        return Release { installer: None, ..release };
    }
    release
}

/// Whether the desktop app may make requests: not with `no_check` (what `BIMCRAFT_NO_UPDATE_CHECK` holds) set to anything but
/// nothing, and not in a Flatpak, which has no network to make them with.
pub(crate) fn phones_home(no_check: Option<OsString>, flatpak: bool) -> bool {
    !flatpak && !no_check.is_some_and(|v| !v.is_empty())
}
