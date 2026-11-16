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
- **Writing goes the same way back.** An export to DWG is the core's DXF of a view or sheet (`export::formats::Dxf`), which
  `to_dwg` reads with acadrust and writes as DWG, at least AutoCAD 2004's (AC1018, for true colours), with the drawing
  variables dictionary AutoCAD writes (some readers take a 2004 DWG without it for an empty drawing, as CADCraft found). The
  apps register `writer()` with their session's exports; it writes in a child too (`write_in_child`, `--dxf-to-dwg`: the DXF
  on standard input, the DWG on standard output), which a cancelled export stops. DWG is listed as experimental.
- **Autodesk's files are never committed.** Tests make DWG files from DXF text with `dwg_from_dxf` (the `testing` feature);
  `BIMCRAFT_DWG_SAMPLES` names a folder of real ones, read only where it is set.

## Known limits

- The child has a time limit (300 s) and caps on what it writes, but no memory limit: a hostile drawing can grow it until the
  system's out-of-memory killer picks a victim, which is not always the child.
- `import_cad` waits for the child on the desktop's UI thread, so the window and the control port stand still for as long as the
  read takes, up to the time limit; a project with many linked DWGs reads them one after another when it opens.
- acadrust opens the file with a memory map; a file truncated by another process while it is read can end the child, which the
  parent reports as a failed read.
- A DWG written here is checked by reading it back through acadrust and the core's DXF reader, not yet by a CAD program in CI.
- The tests round-trip drawings made by acadrust's own writer. Files made by CAD programs are read only when
  `BIMCRAFT_DWG_SAMPLES` names a folder of them, which CI does not set.

