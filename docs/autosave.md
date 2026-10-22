# Background autosave and recovery

The desktop takes a recovery snapshot every two minutes when the committed model has unsaved changes. File > Settings >
Files offers an interval of 1–120 minutes and an off switch, persisted in the user's settings. Recovery discovery still
runs when automatic saving is off. The interval is measured with a monotonic clock and does not reset on every edit:
continuous modeling must not starve autosave. There are no periodic dialogs, focus changes, or successful-save notices.

## Approach and research

- Rust's [Arc documentation](https://doc.rust-lang.org/std/sync/struct.Arc.html) describes shared immutable ownership and
  copy-on-write. BIMCraft already uses `Arc<Model>` for undo and rollback. The UI passes a reference-counted snapshot to
  one worker; serialization and every recovery-file operation run there. There is no model mutex, UI-thread model copy,
  or wait for a writer. At most one snapshot is outstanding, so a slow disk cannot accumulate obsolete full models.
- Qt's [QSaveFile](https://doc.qt.io/qt-6/qsavefile.html) documents writing a temporary file and committing it to avoid
  partial documents. BIMCraft follows that pattern using Rust's
  [NamedTempFile::persist](https://docs.rs/tempfile/latest/tempfile/struct.NamedTempFile.html#method.persist): write beside
  the destination, sync the complete file, then atomically replace the old one, without a direct-write fallback.
  Unix also syncs the containing directory. Normal project Save uses the same replacement helper.
- Recovery files are independent of the saved project. Saving a snapshot leaves the project path, dirty state, selection,
  undo history, and saved project untouched. The background worker executes the ordinary `autosave {path}` command on
  its snapshot. Recovery uses `recover {path, project?}`, with the same migrations and validation as Open.

These are full snapshots, rather than a new command journal: the existing native project reader can validate and migrate
them, including after a process is killed. The worker retains a bounded amount of snapshot work, though serialization
still consumes CPU and memory in proportion to model size. This does not implement incremental persistence.

## Files and lifecycle

- A saved project `house.bimcraft` uses `house.bimcraft.autosave.bimcraft` in the same directory. No numbered history is
  accumulated. An in-progress replacement cannot truncate the previous complete recovery copy.
- Untitled work uses `<system temp>/bimcraft-autosave/untitled-<GUID>.autosave.bimcraft`. The name is unique per document.
  On first Save, its temporary snapshot is removed and subsequent snapshots go beside the saved project. The operating
  system may clear its temporary directory, so untitled recovery cannot promise survival of a reboot or system cleanup.
- A small adjacent `.lock` file holds an OS file lock for each claimed backup. Another live window's copies are skipped;
  process termination releases its locks automatically. Lock files deliberately remain to avoid unlink/relock races.
- Save, Save As, New, Open, explicit Don't Save, and normal close queue cleanup after outstanding writes, so a late writer
  cannot recreate a discarded snapshot. Merely destroying an app object does not delete snapshots. A failed Save leaves
  recovery intact. Exports leave the native project and recovery lifecycle alone.

Opening a native project checks for its adjacent recovery copy even if the original is damaged. Startup also scans for
orphaned untitled snapshots. No timestamp comparison suppresses a discovered backup: the user decides its relevance.

| Choice | Result |
|---|---|
| Recover | Validate and open the backup as unsaved work. Save targets the original project, or asks for Save As for untitled recovery. The backup remains until saved or explicitly discarded. |
| Discard | Delete only the chosen recovery snapshot, leaving the saved project and current model alone. Failed deletion keeps it available. |
| Postpone, Escape, or dismiss | Continue using the current model and preserve the copy. File > Recover Autosave reopens the choices; reopening the project offers them again. |

A postponed copy must not be overwritten by subsequent editing or Save. While it is unresolved, new autosaves use a
separate temporary snapshot. Save As also checks the destination for an existing recovery copy. Recovery of corrupt or
unsupported data reports the error and keeps both the current project and recovery choices available. Recovering over
new unsaved edits first uses the usual Save / Don't Save / Cancel prompt.

## Verification

Core tests cover snapshot consistency, unchanged original/undo state, ordered save/remove operations, bounded scheduling,
a deliberately blocked worker, corrupt data, live-window leases, and recovery after forcibly killing a subprocess.
Desktop tests cover temporary-to-adjacent saving, all three recovery choices, postponed-copy preservation through editing
and Save, corrupt backups, a damaged original, existing recovery data at a Save As destination, and untitled recovery.

A local smoke measurement with 10,000 columns (a 4.14 MB native snapshot) took about 3 microseconds to enqueue the snapshot,
33 ms for a validated model edit while the worker saved, and 55 ms to complete the snapshot. This is one generated model
on the development machine, rather than a performance guarantee for projects with complex geometry or large images.
