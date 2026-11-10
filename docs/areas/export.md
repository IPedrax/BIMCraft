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
  file already there is numbered unless `overwrite` is set. Beside the built-in tokens, Project Information answers by
  the names Revit gives its fields (`{Project Number}`, `{Client Name}`, `{Project Status}`…) and every project parameter
  by its own name: the view's or sheet's value when it has one, else the project's (Project Information's); empty when not
  set. A job may add tokens of its own (`Job::tokens`): a publishing set's `{PrintSet}` and `{PublishSet}`.
  `export_formats` lists them all. Views and sheets hold no project parameters yet, so today a parameter's value is the
  project's.

| Family | Formats |
|---|---|
| Document | PDF through krilla (MIT/Apache): sheets in one file, pages laid out, PDF/A-1b, 2b, 2u, 3b or 4, version 1.4 to 2.0, vector or raster pages, cut marks, title, author and subject |
| Vector | SVG |
| Raster | PNG, JPEG (jpeg-encoder), JPEG XL (zune-jpegxl, lossless only: the only lossy encoder in Rust is AGPL) |
| Line | DXF (AC1018, written here), DWG (experimental: the DXF through acadrust, [CAD import](cad-import.md#dwg)) |
| Model | IFC4, IFC4.3 (IFC4X3_ADD2), IFC2x3 (TC1), glTF, OBJ (metres, y up, vertex colours), STL (binary, mm); U3D planned |
| Schedule | CSV, XLSX, ODS |
| Coordination | BCF 2.1 (a 3D view's camera, a snapshot, the selected elements and the hidden ones) |

The tests are in `export_tests.rs` (through the commands) and in each format's file, through what it writes. A DXF is read
back with `dxf.rs`, a JPEG XL is decoded pixel for pixel, and a zip is opened. Outside the tests, the DXF was checked with
ezdxf's audit, IFC4.3 and IFC2x3 with IfcOpenShell (CI validates `coverage-4x3.ifc`, `house-2x3.ifc` and
`coverage-2x3.ifc`), and ODS by opening it in LibreOffice.

- **IFC2x3** (`ifc/v2x3.rs`) is the IFC4 writer's file turned as it is written: `Step::add` passes each entity through
  `v2x3::downgrade`, so an element kind added to the writer goes out in IFC2x3 with no code of its own. `v2x3/table.rs`
  (made by `ci/ifc2x3_table.py` from the two schemas IfcOpenShell carries; run it again when the writer names a new
  entity) says, for each entity, the IFC2x3 class it is, how many attributes that has, whether it has an owner history
  (every rooted entity must, there: one for the file, by Project Information's author) and the values its enumerations
  take. A class only IFC4 has is its nearest IFC2x3 supertype: a pipe segment is an `IfcFlowSegment` whose
  `IfcPipeSegmentType` still says what it is, furniture an `IfcFurnishingElement`, a chimney a proxy; an element short of
  what its IFC2x3 class requires is a proxy too. A door's and a window's type are an `IfcDoorStyle` and an
  `IfcWindowStyle`; a roof's and a footing's have no type object. What differs in kind the writer does where it asks
  `Schema::Ifc2x3`: triangles are a faceted B-rep (no face sets), several materials an `IfcMaterialList`, a beam's,
  column's or footing's material its own (no profile sets), a styled item's styles in an
  `IfcPresentationStyleAssignment`, a see-through colour an `IfcSurfaceStyleRendering`, a swept disk with its ends, and
  the map conversion and its CRS the project's `ePSet_MapConversion` and `ePSet_ProjectedCRS` with the same values, as
  buildingSMART's guide to georeferencing has them and IfcOpenShell reads them. Checked with IfcOpenShell's schema and
  rule validation, a shape built for every product and each shape's extent the same as the IFC4 file's; the file reads
  back through `import.rs`. Not yet opened in Revit or another program that reads only IFC2x3.

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

- **Publishing sets** (`publish.rs`, the `publish_set` and `publish` commands) are the Publisher's presets, ProSheets'
  profiles laid out as Archicad's publisher sets: a named set (a library entry, `Kind::PublishSet`,
  `Model::publish_sets`) with a destination folder, an overwrite flag and folders; a folder holds print sets, each with
  the format it is written as, its file name template, one combined file's name (PDF) and the format's `Options`. A
  print set is in a folder once for each format (`PublishSet::check`). Each completed entry records its start, end and
  elapsed time in the project for the next publish estimate. An entry names its print set (its GUID in a file,
  `Model::lib_refs_mut`), so a rename is followed, and a print set deleted leaves every folder (the engine prunes after
  each command). The destination is beside the project's file unless absolute (`PublishSet::root`); `publish` takes
  another as `folder`. `publish::run` is one `export::Job` for each entry, in order, with the steps of the whole set told
  to `Ctx::progress`; an entry that can't be written (nothing its format takes, a token no project has) is reported and
  the rest go on. A set overwrites what it wrote before unless told not to. Tests: `publish_tests.rs`.

## The desktop (`crates/ui-egui/src/export/`)

- **File > Export** holds Export…, Publisher… and Print Set…, and nothing else: the exports that saved at once with the
  defaults are gone.
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
  the preview beside it. Subtle gray rules separate the form's choices. A PDF's Content (vector or raster) and Draw cut marks
  sit under the colours; its document settings
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
- **Print Sets** (`printsets.rs`; File > Export > Print Set…, the Export dialog's button, or the Publisher) is Revit's View/Sheet Set window: Name
  picks a set, the list checks its views and sheets (Show: Sheets, Views; a search; Check All, Check None), and Save, Save
  As…, Revert, Rename… and Delete each run `print_set`. In the **Project Browser** Ctrl picks views one by one and Shift a
  run of them; the right-click menu makes a print set of those picked, or of a group's views, asking its name, and has
  Expand All and Collapse All. Several views picked show in Properties: of one kind, the parameters they share (blank
  where they differ, a change made to all); of several, Multiple Categories and how many of each.
- **The Publisher** (`publisher.rs`; File > Export > Publisher…, View tab, or Project Browser menu) lists the project's publishing sets on the left (New…,
  Duplicate…, Rename…, Delete, Revert changes, Import… and Export…) and shows the one picked on the right: its destination (Browse…, or typed; empty
  is asked for at Publish), Overwrite existing files, and its folders. Each folder is a separate rounded card with a corner
  X to delete it. A folder has a name (a `/` nests it; none is the destination itself), Add Print Set,
  and a row for each print set: the print set (New Print Set…
  opens Print Sets on a set not made yet, and the row takes the set made), the format (grouped by family; one the print
  set holds nothing for, or one the folder already has for that print set, is greyed), a sample of the file name, File
  Names…, Settings… and Remove. The sample file name sits on a separate line below these controls, truncated to the card's
  width with its full name on hover. File Names… puts a template together: tokens in three groups (views and sheets, Project
  Information, project parameters) are added by a click with the separator between them, the template can be typed, a
  sample shows what it makes, and a format that combines can be one file with its own name. Settings… is the Export
  dialog's form for that format (`export::format_options`, and a PDF's document settings); Cancel puts them back. The
  set is a working copy until Save; Publish saves what changed and writes the set on the export's thread
  (`worker::Work::Publish`), with its bar, Cancel Export and note. Completed entries retain their start and end times,
  and the dialog shows their combined reexport estimate beside the print set and file counts. XML transfer includes the
  publishing sets and their referenced print sets; views match by GUID or, in another project, by unique title. Tests: `tests/publisher.rs`.
- Tests: `tests/export.rs`. They cover the dialog on a plan and a PDF written while the project is edited (and the dialog no
  taller than its form), a cancelled job (a gate holds the thread until Cancel is pressed), the format list and an IFC of the
  whole project, a region drawn in the plan and the DXF it makes, printing to a fake spooler (`print::Fake`), and a PDF/A-2b
  with cut marks. `tests/printsets.rs` covers picking in the browser with Ctrl and Shift, a set made from its menu, the
  Print Sets window, a set exported as one PDF, and a list of pages.

## Still to come

- The Publisher: a tree of nested folders (a folder's name takes a `/` today),
  a set sent to a printer, folder names from tokens, a page preview in an entry's settings, and sheets picked by a
  parameter's value rather than by print set.
- Project parameters on views and sheets, so a file name can take a sheet's own value.
- U3D and 3D PDF.
- DWG and DXF export setups as Revit's: layers, colours, linetypes, lineweights, real hatch patterns rather than exploded
  lines and solid fills, text, coordinates, paper space (the ROADMAP's row).
- Project-information tokens in file names, from the project parameters.
