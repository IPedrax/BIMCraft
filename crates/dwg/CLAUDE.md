# bimcraft-dwg

The DWG bridge: a DWG drawing as the ASCII DXF `bimcraft-core`'s `dxf.rs` reads, through the acadrust crate (MPL-2.0, used
unmodified as a separate library; NOTICE carries it and ACadSharp's MIT notice). The write-up is
[CAD import](../../docs/areas/cad-import.md#dwg).

- **Keep acadrust behind the bridge.** Nothing outside this crate sees its types; what comes out is DXF bytes and notes. The
  core never depends on this crate: the apps hand a `Session` its `reader()`, behind their default `dwg` feature.
- **Never change acadrust's files.** Its MPL-2.0 copyleft covers them; a fix goes upstream, or round the library here, as
  `Capped` and the size checks do.
- **A file is hostile, and the decoder can't be interrupted.** `to_dxf` checks `Limits` before and while it converts and runs
  under `catch_unwind`, but a file that makes the decoder allocate past memory aborts the process, so the apps read a DWG in a
  child of their own (`in_child`, answered by `child_main`, which every app's `main` calls first).
- **Writing is in process.** `writer()` (Save as DWG) turns a DXF the core wrote into DWG with acadrust's writer under
  `catch_unwind`, in the app's own process: what it reads is the core's own DXF, not a hostile file.
- **Autodesk's files are never committed.** Tests make DWG files from DXF text with `dwg_from_dxf` (the `testing` feature);
  `BIMCRAFT_DWG_SAMPLES` names a folder of real ones, read only where it is set.

## Known limits

- The child has a time limit (300 s) and caps on what it writes, but no memory limit: a hostile drawing can grow it until the
  system's out-of-memory killer picks a victim, which is not always the child.
- `import_cad` waits for the child on the desktop's UI thread, so the window and the control port stand still for as long as the
  read takes, up to the time limit; a project with many linked DWGs reads them one after another when it opens.
- acadrust opens the file with a memory map; a file truncated by another process while it is read can end the child, which the
  parent reports as a failed read.
- The tests round-trip drawings made by acadrust's own writer. Files made by CAD programs are read only when
  `BIMCRAFT_DWG_SAMPLES` names a folder of them, which CI does not set.

