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
  - a page range;
  - a region: two corners in the view's own coordinates (a plan's model mm, a section's along and up, a sheet's paper mm).

  A region is cut before any writer sees the drawing (`Paged::cropped`, `render::crop`): lines are cut at its edge, fills
  clipped to it, pictures cut to its pixels, and the view moved with it, so a DXF of a region is still in model millimetres.
  It holds for every drawing format, not only paged ones.

  `layout` lays a drawing on as many pages as it needs, and `Paged::place` is where a page puts it. The writers and the
  desktop's preview both use these two, so the preview is what the file holds.
- **File names** (`naming.rs`): templates with tokens (`{SheetNumber} - {SheetName}`, `{ViewName}`, `{Level}`, `{Scale}`,
  `{Format}`, `{Date}` and others: `ModelTokens::NAMES`). A `/` makes folders. Names are made safe for every system, and a
  file already there is numbered unless `overwrite` is set. Project parameters will answer as tokens by their names.

| Family | Formats |
|---|---|
| Document | PDF through krilla (MIT/Apache): sheets in one file, pages laid out, PDF/A-1b, 2b, 2u, 3b or 4, version 1.4 to 2.0, vector or raster pages, cut marks, title, author and subject |
| Vector | SVG |
| Raster | PNG, JPEG (jpeg-encoder), JPEG XL (zune-jpegxl, lossless only: the only lossy encoder in Rust is AGPL) |
| Line | DXF (AC1018, written here), DWG (experimental: the DXF through acadrust, [CAD import](cad-import.md#dwg)) |
| Model | IFC4, IFC4.3 (IFC4X3_ADD2), glTF, OBJ (metres, y up, vertex colours), STL (binary, mm); IFC2x3 and U3D planned |
| Schedule | CSV, XLSX, ODS |
| Coordination | BCF 2.1 (a 3D view's camera, a snapshot, the selected elements and the hidden ones) |

The tests are in `export_tests.rs` (through the commands) and in each format's file, through what it writes. A DXF is read
back with `dxf.rs`, a JPEG XL is decoded pixel for pixel, and a zip is opened. Outside the tests, the DXF was checked with
ezdxf's audit, IFC4.3 with IfcOpenShell (CI validates `coverage-4x3.ifc`), and ODS by opening it in LibreOffice.

- **PDF** (`pdf.rs`) is written with krilla, the library Typst writes its PDFs with. `pdf::Settings` (the `export`
  command's `pdf`) picks an archive standard, which krilla checks as it writes: a file that wouldn't be PDF/A is refused with
  the reasons, not written. PDF/A-1 has nothing see-through, so a picture's clear pixels are laid on white there. Raster
  pages are the pictures the PNG writer makes, at the page setup's dpi. Cut marks (`document::cut_marks`) are hairlines in
  the margin, along each edge of the printable area that meets another page of a tiled drawing. The `a` levels (tagged) are
  not offered: a drawing has no reading order to tag.

- **Pages** (`page::Pages`) are all of them, a range (`[first, last]`), or a typed list (`"1, 3, 5-7"`); a page past the last is
  left out. A drawing no larger than its paper is one page whatever the margin: the margin takes its edge, as a printer's
  border does, rather than tiling an A3 sheet onto four A3 pages.
- **Print sets** (`printset.rs`, the `print_set` command) are Revit's View/Sheet Sets: a named list of views and sheets, any
  kind and any mix, in the order they print. A set is a library entry (`Kind::PrintSet`, `Model::print_sets`): a unique name,
  an id and a GUID; it holds its views by GUID, and a view deleted leaves every set it was in (the engine prunes them after
  each command). `export` with `print_set` writes its views, one PDF of them all at `path`.

## The desktop (`crates/ui-egui/src/export/`)

- **File > Export > Export…** (Ctrl+P) opens the dialog on what the window shows. Its **Destination** is a file or a
  printer: the printers are listed on a thread (`print.rs`: CUPS's `lpstat` on Linux and macOS, PowerShell on Windows). A
  printer prints PDF: Print writes it, as an export does, in a folder of its own, hands it to the spooler (`lp`, with the
  copies and a paper name CUPS knows) and removes it. On Windows it goes through the PDF reader's PrintTo, untested so far.
- **Extents** are all the view shows, or a custom region. Draw region… steps the dialog aside (`Tool::ExportRegion`), the
  view takes two corners (plans in `canvas.rs`, sections in `section.rs`, sheets in `paper.rs`) with a rubber band, and the
  dialog comes back with them; Esc brings it back unchanged. A drawing of no pages (SVG, DXF, DWG) gets the colour and
  hairline settings too. That is the plan, section, sheet or
  schedule, or the active 3D view; a model format from a view that isn't 3D writes the whole project. The format list is
  grouped by family. Formats the source can't be written as are greyed out, and so are planned ones. Experimental ones
  say so.
- **Of**: what the window shows, every sheet (PDF), or a print set, picked by name, with Print Sets… beside it. A set to a
  format that doesn't combine writes a file each, named after the file asked for and the view.
- **The page** is set as a print dialog sets it, a paper of its own size among the papers, a 5 mm margin to start with, with
  the preview beside it. A PDF's Content (vector or raster) and Draw cut marks sit under the colours; its document settings
  (archive, version, title, author, subject) are in the PDF Settings… window under the preview. Pages are all, a range, or
  Custom, a list typed. The preview draws every page, numbers each in light grey when there are several, greys those
  outside the range and draws the drawing on each with `canvas::paint`, in the colours and lines the setup asks for
  (`formats::styled`). The preview's drawing is made on a thread of its own and kept while the source and the project stay
  the same. Under the form, the dialog says how many pages it makes, their size and the scale.
- **Export…** asks for the file in the system's dialog, then writes it with `worker.rs`. The job runs on a thread of its
  own, on a snapshot of the project, so editing goes on. The status bar shows a progress bar with Cancel Export.
  `poll_export` puts what was written, what wasn't and why, and whether it was cancelled in the status box. The file's name
  is the one given, its braces doubled so they aren't tokens. A picture of several pages writes a file a page beside it.
  Each format's settings are kept for the next export while the window is open.
- **Print Sets** (`printsets.rs`; File > Export > Print Sets…, or the dialog's button) is Revit's View/Sheet Set window: Name
  picks a set, the list checks its views and sheets (Show: Sheets, Views; a search; Check All, Check None), and Save, Save
  As…, Revert, Rename… and Delete each run `print_set`. In the **Project Browser** Ctrl picks views one by one and Shift a
  run of them; the right-click menu makes a print set of those picked, or of a group's views, asking its name, and has
  Expand All and Collapse All. Several views picked show in Properties: of one kind, the parameters they share (blank
  where they differ, a change made to all); of several, Multiple Categories and how many of each.
- The other File > Export items (IFC…, View as PNG…, Plan as SVG…, Sheets as PDF…, glTF) still save at once with the
  defaults.
- Tests: `tests/export.rs`. They cover the dialog on a plan and a PDF written while the project is edited (and the dialog no
  taller than its form), a cancelled job (a gate holds the thread until Cancel is pressed), the format list and an IFC of the
  whole project, a region drawn in the plan and the DXF it makes, printing to a fake spooler (`print::Fake`), and a PDF/A-2b
  with cut marks. `tests/printsets.rs` covers picking in the browser with Ctrl and Shift, a set made from its menu, the
  Print Sets window, a set exported as one PDF, and a list of pages.

## Still to come

- The Publisher (View tab): saved export sets, sheets picked by set or parameter, foldered output in one go.
- IFC2x3 (a port of the IFC writer: owner history, IFC4-only entities), U3D and 3D PDF.
- DWG and DXF export setups as Revit's: layers, colours, linetypes, lineweights, real hatch patterns rather than exploded
  lines and solid fills, text, coordinates, paper space (the ROADMAP's row).
- Project-information tokens in file names, from the project parameters.
