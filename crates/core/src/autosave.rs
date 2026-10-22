//! Background snapshots of immutable models. A single worker orders writes and removals, so a
//! late autosave can never resurrect a file removed by Save/Discard. No UI toolkit or model lock.
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak, mpsc};

use crate::engine::Session;
use crate::model::Model;

pub const SUFFIX: &str = ".autosave.bimcraft";

/// Append, rather than replace, the extension so differently named projects cannot collide.
pub fn backup_path(project: &Path) -> PathBuf {
    let mut name = project.as_os_str().to_owned();
    name.push(SUFFIX);
    PathBuf::from(name)
}

/// Flush a complete file in the destination directory, then replace the old one atomically.
/// Never fall back to truncating the destination, including on Windows.
pub fn atomic_write(path: &Path, text: &str) -> Result<(), String> {
    let write = || -> Result<(), Box<dyn std::error::Error>> {
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        staged.write_all(text.as_bytes())?;
        staged.as_file().sync_all()?;
        staged.persist(path)?;
        // The file contents were synced before replacement. Persist the directory entry too on Unix.
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        Ok(())
    };
    write().map_err(|e| format!("{}: {e}", path.display()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backup {
    pub path: PathBuf,
    /// None for an untitled document. Recovery of it must ask for Save As.
    pub project: Option<PathBuf>,
}

#[derive(Debug)]
pub enum Notice {
    Saved { path: PathBuf, snapshot: Weak<Model>, result: Result<(), String> },
    Removed { path: PathBuf, result: Result<(), String> },
    Discovered { generation: u64, result: Result<Vec<Backup>, String> },
}

enum Job {
    Save(PathBuf, Arc<Model>),
    Remove(PathBuf),
    Discover(Option<PathBuf>, u64),
    Release(PathBuf),
}

pub struct Worker {
    jobs: mpsc::Sender<Job>,
    notes: mpsc::Receiver<Notice>,
    busy: bool,
}

impl Worker {
    /// Starts no work on the caller's thread. Dropping the handle never joins the thread: it
    /// drains already accepted jobs, then exits and releases its leases.
    pub fn start(temp: PathBuf, wake: impl Fn() + Send + 'static) -> Result<Self, String> {
        let (jobs, inbox) = mpsc::channel();
        let (outbox, notes) = mpsc::channel();
        std::thread::Builder::new()
            .name("bimcraft-autosave".into())
            .spawn(move || {
                let mut leases = BTreeMap::new();
                for job in inbox {
                    let note = match job {
                        Job::Save(path, model) => {
                            let snapshot = Arc::downgrade(&model);
                            let result = (|| {
                                if path.parent() == Some(temp.as_path()) {
                                    std::fs::create_dir_all(&temp).map_err(|e| e.to_string())?;
                                }
                                lease(&path, &mut leases)?;
                                let mut session = Session::new();
                                session.model = model;
                                session.execute("autosave", &serde_json::json!({"path": path})).map(|_| ())
                            })();
                            Notice::Saved { path, snapshot, result }
                        }
                        Job::Remove(path) => {
                            let result = (|| {
                                // Removing a missing backup needs no lease (its folder may not exist yet).
                                if !path.try_exists().map_err(|e| e.to_string())? {
                                    return Ok(());
                                }
                                lease(&path, &mut leases)?;
                                std::fs::remove_file(&path).map_err(|e| e.to_string())
                            })();
                            if result.is_ok() {
                                leases.remove(&path);
                            }
                            Notice::Removed { path, result }
                        }
                        Job::Release(path) => {
                            leases.remove(&path);
                            continue;
                        }
                        Job::Discover(project, generation) => {
                            let result = discover(&temp, project, &mut leases);
                            Notice::Discovered { generation, result }
                        }
                    };
                    if outbox.send(note).is_err() {
                        // Even after the window is gone, finish queued cleanup in order.
                        continue;
                    }
                    wake();
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { jobs, notes, busy: false })
    }

    /// False while one snapshot is outstanding: typing never grows a queue of obsolete models.
    pub fn save(&mut self, path: PathBuf, model: Arc<Model>) -> Result<bool, String> {
        if self.busy {
            return Ok(false);
        }
        self.send(Job::Save(path, model))?;
        self.busy = true;
        Ok(true)
    }

    pub fn remove(&self, path: PathBuf) -> Result<(), String> {
        self.send(Job::Remove(path))
    }

    pub fn discover(&self, project: Option<PathBuf>, generation: u64) -> Result<(), String> {
        self.send(Job::Discover(project, generation))
    }

    /// Leave a postponed backup in place but let another window recover it after this document closes.
    pub fn release(&self, path: PathBuf) -> Result<(), String> {
        self.send(Job::Release(path))
    }

    fn send(&self, job: Job) -> Result<(), String> {
        self.jobs.send(job).map_err(|_| "the autosave worker stopped".to_owned())
    }

    pub fn poll(&mut self) -> Option<Notice> {
        let note = self.notes.try_recv().ok()?;
        if matches!(note, Notice::Saved { .. }) {
            self.busy = false;
        }
        Some(note)
    }
}

/// An OS lock is released even after a killed process. The small lock file deliberately remains:
/// unlinking one could allow another process to lock a different inode under the same name.
fn lease(path: &Path, leases: &mut BTreeMap<PathBuf, File>) -> Result<(), String> {
    if leases.contains_key(path) {
        return Ok(());
    }
    let mut lock = path.as_os_str().to_owned();
    lock.push(".lock");
    let file =
        OpenOptions::new().read(true).write(true).create(true).truncate(false).open(PathBuf::from(lock)).map_err(|e| e.to_string())?;
    file.try_lock().map_err(|e| format!("recovery file is in use by another BIMCraft window: {e}"))?;
    leases.insert(path.to_path_buf(), file);
    Ok(())
}

fn discover(temp: &Path, project: Option<PathBuf>, leases: &mut BTreeMap<PathBuf, File>) -> Result<Vec<Backup>, String> {
    let mut found = Vec::new();
    if let Some(project) = project {
        let path = backup_path(&project);
        if path.try_exists().map_err(|e| e.to_string())? && lease(&path, leases).is_ok() {
            found.push(Backup { path, project: Some(project) });
        }
        return Ok(found);
    }
    let entries = match std::fs::read_dir(temp) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(found),
        Err(e) => return Err(e.to_string()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().is_some_and(|n| n.to_string_lossy().ends_with(SUFFIX))
            && entry.file_type().is_ok_and(|t| t.is_file())
            && lease(&path, leases).is_ok()
        {
            found.push(Backup { path, project: None });
            if found.len() >= 128 {
                break;
            }
        }
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(found)
}
