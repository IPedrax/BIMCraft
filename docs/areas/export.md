# Export

Every file the project is written out as, other than its own `.bimcraft`, goes through one engine: `crates/core/src/export/`.
The `export` command, `save` to an `.ifc` or a `.pdf`, and the desktop's File > Export… all call it, so a format added there
reaches every front end.

## The engine (`bimcraft-core`)

- **A source and a format.** A source is a view (a plan, a section, an elevation, a 3D view), a sheet, a schedule, or the
  whole project (`Kind`). A format (`Format`, one file each in `export/formats/`) says what content it takes (`Takes`: a
  drawing on pages, a table, the model) in the order it prefers. `prepare` makes that content of the source once, and the
  writer knows nothing of views. `Registry` holds the built-in formats and any an app registers. The DWG writer is one: it
  is in `crates/dwg`, which the core may not depend on.
- **A job** (`Job`) is many items (a source's GUID and a format), a folder, a file name template, folders by format or not,
  combined files (a PDF of many sheets) and the options. `run` works through it on whatever thread calls it. It asks
  `Ctx::cancelled` between sources, pages and long writers' steps and tells `Ctx::progress` where it is. Each file is
  written whole or not at all (`write_file`: a temporary file beside it, then renamed, with the user's umask). A source that
  fails is reported, and the rest go on.
- **Pages** (`page.rs`). `PageSetup` holds the following:
  - paper (named, its own size, or the drawing's);
  - orientation;
  - margin;
  - scale (original, fit to page, or 1 : n);
  - a nine-way anchor;
  - colour, greyscale or black and white;
  - hairlines;
  - dpi;
  - a page range.

  `layout` lays a drawing on as many pages as it needs, and `Paged::place` is where a page puts it. The writers and the
  desktop's preview both use these two, so the preview is what the file holds.
- **File names** (`naming.rs`): templates with tokens (`{SheetNumber} - {SheetName}`, `{ViewName}`, `{Level}`, `{Scale}`,
  `{Format}`, `{Date}` and others: `ModelTokens::NAMES`). A `/` makes folders. Names are made safe for every system, and a
  file already there is numbered unless `overwrite` is set. Project parameters will answer as tokens by their names.

| Family | Formats |
|---|---|
| Document | PDF (sheets in one file, pages laid out) |
| Vector | SVG |
| Raster | PNG, JPEG (jpeg-encoder), JPEG XL (zune-jpegxl, lossless only: the only lossy encoder in Rust is AGPL) |
| Line | DXF (AC1018, written here), DWG (experimental: the DXF through acadrust, [CAD import](cad-import.md#dwg)) |
| Model | IFC4, IFC4.3 (IFC4X3_ADD2), glTF, OBJ (metres, y up, vertex colours), STL (binary, mm); IFC2x3 and U3D planned |
| Schedule | CSV, XLSX, ODS |
| Coordination | BCF 2.1 (a 3D view's camera, a snapshot, the selected elements and the hidden ones) |

The tests are in `export_tests.rs` (through the commands) and in each format's file, through what it writes. A DXF is read
back with `dxf.rs`, a JPEG XL is decoded pixel for pixel, and a zip is opened. Outside the tests, the DXF was checked with
ezdxf's audit, IFC4.3 with IfcOpenShell (CI validates `coverage-4x3.ifc`), and ODS by opening it in LibreOffice.

## The desktop (`crates/ui-egui/src/export/`)

- **File > Export > Export…** (Ctrl+P) opens the dialog on what the window shows. That is the plan, section, sheet or
  schedule, or the active 3D view; a model format from a view that isn't 3D writes the whole project. The format list is
  grouped by family. Formats the source can't be written as are greyed out, and so are planned ones. Experimental ones
  say so.
- **The page** is set as a print dialog sets it, with the preview beside it. The preview draws every page, greys those
  outside the range and draws the drawing on each with `canvas::paint`, in the colours and lines the setup asks for
  (`formats::styled`). The preview's drawing is made on a thread of its own and kept while the source and the project stay
  the same. Under the form, the dialog says how many pages it makes, their size and the scale.
- **Export…** asks for the file in the system's dialog, then writes it with `worker.rs`. The job runs on a thread of its
  own, on a snapshot of the project, so editing goes on. The status bar shows a progress bar with Cancel Export.
  `poll_export` puts what was written, what wasn't and why, and whether it was cancelled in the status box. The file's name
  is the one given, its braces doubled so they aren't tokens. A picture of several pages writes a file a page beside it.
  Each format's settings are kept for the next export while the window is open.
- The other File > Export items (IFC…, View as PNG…, Plan as SVG…, Sheets as PDF…, glTF) still save at once with the
  defaults.
- Tests: `tests/export.rs`. They cover the dialog on a plan and a PDF written while the project is edited, a cancelled job
  (a gate holds the thread until Cancel is pressed), and the format list and an IFC of the whole project.

## Still to come

- The Publisher (View tab): saved export sets, sheets picked by set or parameter, foldered output in one go.
- IFC2x3 (a port of the IFC writer: owner history, IFC4-only entities), U3D and 3D PDF.
- Project-information tokens in file names, from the project parameters.
