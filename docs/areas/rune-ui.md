# The Rune UI

BIMCraft's desktop is egui today (`crates/ui-egui`: 317 files, 172,000 lines with its tests, 253 of the files naming egui, `BimApp` in
`lib.rs` the hub). `crates/ui-rune` is the start of the same app drawn by [Rune](https://github.com/anomalyco/rune), a pre-release
native Rust UI framework, on the branch `IPedrax-Test/Rune-GUI`. **The egui app stays the shipped UI until Rune reaches parity**, and
`crates/ui-egui` is untouched by this work, so the pull requests that land there keep merging. The rule holds for both: every
feature in both UIs, differing only visually; the BIMCraft and Rev profiles are one app with a shared look. Notes for working in the
crate: [crates/ui-rune/CLAUDE.md](../../crates/ui-rune/CLAUDE.md).

Why: egui is immediate mode and repaints the whole interface when the pointer moves, which pins a CPU on a machine with no graphics
card (issue 628 measured 1.78 of 2 cores on a Windows VM on WARP). Rune keeps a retained tree and redraws what changed.

## What is built (stages 0 and 1)

A window of the sample house that works end to end, small on purpose:

| Module | What it does | Tests |
|---|---|---|
| `app.rs` | the state and what its handlers do; the only place that meets the core (`render::draw`, `Drawing::pick`, `Session::execute`) | 6, no window |
| `ui/` | the plan canvas, the Properties panel, the status bar, as functions of `App` | 5, Rune's headless `Runtime` |
| `camera.rs` | pan and zoom in the core's `View`, hostile numbers refused | 4 |
| `paint.rs` | a `Drawing` as Rune shapes: strokes, polygons, dashes split, lettering through `text::triangles` | 5 |
| `props.rs`, `status.rs` | the Properties rows from `element_json`; the status line | 2, 3 |
| `lang.rs` | `t`, `tf`, `tc` over the catalogs egui reads (`translations/ui/*.po`), English fallback | 3 |

The pointer moves over the plan without rebuilding it: the drawing is one element kept by `memo` while the drawing is the same, the
pointer only changes the small hover layer, and a drag or a wheel draws again through the core. Every change (Delete, Undo, Redo) is
`Session::execute`; the UI reads `session.model` and never edits it (a test deletes an element through the Delete button and
brings it back through Undo). Not drawn yet, and counted rather than drawn wrong (`Painted::skipped`): pictures (`Prim::Image`).
Strokes are round-capped and round-joined where egui and the core's own painter mitre the corners of thick lines.

## One workspace or two

Rune's README asks for `[patch.crates-io]` forks of winit, sctk-adwaita, parley_engine, wgpu-hal, naga and three accesskit crates in
the workspace root, and cargo applies a patch to every crate of the workspace. eframe 0.36 builds on stock winit and wgpu, so one
workspace would build one of the two UIs wrong.

**Chosen: `crates/ui-rune` is a workspace of its own**, excluded from the root (`exclude` in the root `Cargo.toml`), with its own
`Cargo.lock` and a pinned toolchain (Rune's, 1.99.1, in `rust-toolchain.toml`). It depends on `bimcraft-core` and `bimcraft-i18n` by
path, and nothing depends on it. Against the other option, a branch that drops egui: that one cannot merge to main while egui ships,
and it forks every outside contribution to the UI. The cost of a workspace of its own is that `cargo test --workspace` does not reach
it, so `.github/workflows/rune-ui.yml` runs fmt, clippy and tests on Linux, Windows and macOS (a path filter: this crate, the core,
the i18n crate, the catalogs), and a second lockfile has to be kept (`cargo generate-lockfile` inside the folder).

Checked: the manifest test (`only_the_desktop_app_can_reach_the_network`) reads every `crates/*/Cargo.toml` and passes with this one
(no `ureq`, `rustls`, `reqwest` or `hyper` line, and no crate of the core, the apps or the CLI depends on this one). Beyond the test,
`cargo tree -i` finds none of `ureq`, `reqwest`, `hyper`, `rustls`, `native-tls`, `openssl`, `h2` or `curl` anywhere in the 492 packages
of the Rune graph: Rune brings no HTTP client. Licences: 484 of the packages are MIT, Apache-2.0, Zlib, BSD or Unicode-3.0 (or their
combinations); `spellbook` is MPL-2.0 (Rune compiles it for the web only), `r-efi` offers LGPL beside MIT and Apache-2.0. Rune's
`ogg-audio` default feature is left off because it brings MPL-2.0 audio decoders on macOS and BIMCraft plays no sound. `NOTICE` carries
Rune's MIT notice and Inter's (OFL) with its text in `assets/fonts/inter/OFL.txt`; a package of a build with the Rune UI must carry that
file as `fonts/inter/OFL.txt`, as it carries the Plex ones.

Rune is pinned to revision `80cfdbc1a31d8f088d44cb1e4fa36cbbae01e047` of `dev` (2026-11-24). It never rewrites `dev`. It has no API
promise (the root `Cargo.toml` lists 56 crates and the unreleased changelog already holds more than 1,200 entries), takes issues and no
pull requests, and changes daily. Bumping it is one revision in five places (the crate's `CLAUDE.md`) and a rebuilt lockfile.

## What costs what with no graphics card

MEASUREMENTS

## The unknowns

Read from Rune's sources and docs at the pinned revision; what a run on the CI machines showed is marked (run).

**Docking panels.** `rune-kit` has `SplitView` (a sidebar and an inspector, resizable by their edge, collapsible, widths in a
`PaneState` the app keeps and saves), `Sidebar`, `Toolbar`, `Page` and `TabStrip` (tabs that open and close on springs, reorder by
drag, tear out, close with a middle click). It has no docking of palettes into any edge and no floating palettes inside the window.
A floating palette is a second OS window (`App::window`, with its own state, sharing the one GPU device). So `dock.rs` (1,557 lines,
palettes that dock to an edge or float) is rebuilt over `SplitView`, `TabStrip` and `El::on_drag`, with the layout as data (a
`DockLayout` in the core, saved in `settings.json`, which both UIs read).

**Tables.** `rune_kit::Table` is virtualised (100,000 rows scroll at 0.4 ms a frame, 3.4 ms when rows mount, on Apple silicon, by
Rune's own figures, not checked here), keeps selection, sort and column widths in a `TableState`, resizes columns by their header
edge, and exposes `grid`, `row`, `columnheader` and `gridcell` to screen readers. Cells are elements, so an editable cell is a
`TextInput` in the cell. No frozen columns, grouping or cell selection: BIMCraft's schedules (`schedule::Table` is already in the core)
need frozen headings and grouped rows, built over `Table`'s row hook. `Tree` and `FileTree` (50,000 files at 0.4 ms) are the Project
Browser's base.

**Rich text input.** `TextEdit::rich()` holds marks (bold, italic, underline, strike, code, link, colour), block attributes (bullets,
numbers, quotes, code), inline pills, HTML paste and export, undo with a history across a restart, IME for fields and for an editor an
app draws itself (`ime_cursor`, `App::on_ime`), and `allow_marks` to keep a field to what its store holds. That covers a dialog's text
field and the Assistant's composer. The note a person writes on the plan is the core's own layout (`text.rs`: font, size, runs), which
Rune's marks do not hold (no size or family per run), so notes keep the core's run model and use Rune for the caret, selection and IME
of an editor drawn on the canvas.

**A ribbon of the density BIMCraft needs.** There is none in the kit. It has `Button` in five `ControlSize`s (Mini to ExtraLarge),
`IconButton`, `SegmentedControl`, `Menu`, `Popover`, `Select`, `Tooltip` and `Keybind`, and Taffy's flexbox, which folds a panel when
its row runs out of room better than egui's immediate layout does. The ribbon (`ribbon.rs`, 827 lines) is built from those, from a
table of tabs, panels and buttons that lives in the core and that both UIs draw (see below). Not measured: the density of a full
Revit-style ribbon at 1x on a small laptop.

**Fonts.** Rune embeds Inter (Regular, Medium, SemiBold, OFL) and takes `AppSpec::fonts` and `AppSpec::families` to replace it, so the
window can stay in IBM Plex Sans and Mono as the egui app is (the two files are in `assets/fonts`). A character the app's fonts lack is
asked of the platform's own fallback when it is first laid out (fontconfig in process on Linux, DirectWrite on Windows, CoreText on
macOS; no folder is walked and no process is started), so Chinese needs no font search of ours (egui's `lang/font.rs`, 228 lines,
does one). Vietnamese and Polish are Latin Extended: Inter and Plex carry them. A text type that names a system font
(`TextStyle::family(Family::Named("Arial"))`) loads the installed face on first use and falls back to the sans slot when it is not
there; BIMCraft's notes on the plan do not depend on it, since the core reads the fonts and draws their outlines itself. FONTS

**Window, file dialogs, language.** The window is winit's (patched), with a title, a size, a minimum size and, on macOS, `Chrome` for a
full-size titlebar; Rune has no remembered window geometry off macOS (`rune_mac::remember_frame` is macOS only), so the size and
position keep going in `settings.json` as `window.rs` does now. File dialogs are the `native-file-dialog` feature (not enabled yet):
`rfd` on Windows and macOS, and on Linux Rune calls the XDG desktop portal's FileChooser itself over zbus and returns an error when the
desktop has no portal, where egui's `rfd` falls back to GTK; the asks are `tx.open_file`, `tx.save_file` and `tx.open_folder` with a
callback, and a headless run answers them from a script (`Runtime::resolve_file_dialog`), which makes Open and Save testable. The
system language stays `sys-locale` through `lang.rs`; Rune has its own catalog crate (`rune-i18n`, ICU4X) that BIMCraft does not need.

**Windows and macOS builds.** Rune's README: on Windows the MSVC toolchain needs only the Visual Studio C++ build tools; the GNU
toolchain also needs MinGW-w64's binutils for `dlltool`. On macOS the default Metal backend is Rune's own (objc2), with shader libraries
checked in (Xcode's Metal toolchain only to rebuild them). On Linux the `-dev` packages of `libxkbcommon` and `libwayland` at build
time, `libxkbcommon-x11-0` to open a window, and a software Vulkan driver (Mesa's lavapipe) on a machine with no card. The `check` jobs
of `rune-ui.yml` build and test this crate on `ubuntu-24.04`, `windows-latest` and `macos-15` (run): CHECKS

**What else the read turned up.** Rune is not GPU-only: it has a CPU renderer (`render::cpu_boot`, `CpuPainter`, tiles of 64x32, only the
tiles whose inputs changed repainted, one comparison of the instance list for a frame that changed nothing) that draws a window's first
frame before any GPU exists and keeps it live; a GPU that cannot be made leaves the window on the CPU (three tries, 30 seconds apart),
and `RUNE_CPU_FIRST=only` never starts the GPU. That landed on Windows on 2026-11-12 and on Linux on 2026-11-24, in `dev`, unreleased.
Its README says it is 'not measured on a real GPU or a real desktop'. Accessibility is AccessKit on all three systems (UI Automation,
NSAccessibility, AT-SPI), the same family egui's `accesskit` feature uses, and `Runtime::accessibility_json` makes the tree testable.

## The modules, and the order

The target layout of `crates/ui-rune/src`, so the next hundred modules have a place and no file becomes the next `lib.rs`:

```
app.rs            the hub, small: App holds one slice per area, no widget, no toolkit
app/<slice>.rs    a slice owns its state and its handlers: project, canvas, selection, panels, dialogs, docks, settings
canvas/           camera, paint, pick, grips, tools/<family>.rs (one file per family of tools)
ui/shell/         window, dock (SplitView, TabStrip), status bar, ribbon and the Tools palette
ui/panels/        Properties, Project Browser, palettes        ui/dialogs/   one file per dialog
ui/forms/         the form widgets the core's schemas are drawn with      ui/tables/   schedules and lists
ui/view3d/        the 3D view         ui/icons/   the icons, painted from the core's description
```

A test keeps it: no file under `crates/ui-rune/src` passes 1,200 lines (`tests/layout.rs`), as the root `CLAUDE.md` is held to 3,000
words.

**What egui's modules become**, with their size (lines, the tests aside; the tests are 55,800 of the 172,700 and are written again
against Rune's `Runtime`, scenario by scenario, as each area arrives):

| egui | Lines | Rune |
|---|---|---|
| `lib.rs` (`BimApp`: ribbon, menus, tools, tabs, files) | 8,000 | split into `app/` slices and `ui/shell/` |
| `canvas.rs`, `retained.rs`, `motion.rs`, `guide_refs.rs`, `draw.rs`, grips, zoom, minimap | 11,600 | `canvas/`; `retained.rs` and `motion.rs` are egui painter work (kept triangles, a motion mesh) and are not ported: `memo` and shape layers do their job |
| `ribbon.rs`, `dock.rs`, `style.rs`, `titlebar.rs`, `icons.rs`, `popup.rs`, `window.rs`, `settings.rs`, `shortcuts.rs`, `profile.rs`, `start.rs`, `find.rs`, `context.rs` | 12,500 | `ui/shell/`, `style/` (the Rev and BIMCraft profiles as tokens), the shared tables below |
| `properties.rs`, `browser.rs`, `footer.rs`, `filter.rs` | 6,400 | `ui/panels/` over `Table`, `Tree` and the core's forms |
| the manage dialogs (Building Codes, Parameters, Coordinates, Graphics, Materials, Line Weights, Object Styles, Standards, Templates, Phasing, Links, Type Properties and the rest) | 18,200 | `ui/dialogs/` from form schemas the core builds |
| the 3D view (`orbit.rs`, `gpu.rs`, `triad.rs`, `fly.rs`, `spacemouse.rs`, `section_box.rs`, `camera.rs`) | 8,900 | `ui/view3d/`: first the core's software picture as `rune::Pixels` (no GPU needed, as today's fallback), then a mesh pass through Rune's `Compositor` |
| sections, sheets, underlays, view range, schedules on screen, title blocks | 4,200 | `canvas/` and `ui/tables/`; the two workers move to the core |
| export, print sets, the Publisher | 2,900 | `ui/dialogs/` over the core's export registry |
| the Family Editor and adaptive families | 23,600 | last: the biggest single area, drawn on the same canvas |
| the Assistant, Co-Work, the updater banner | 2,400 | `ui/panels/`; the three network crates stay as they are |
| PyAddins, Plugins, the Markdown subset of a report, Run Script | 3,700 | `ui/dialogs/`; `markdown.rs` is pure and moves to the core |

**What stays shared in the core, because both UIs need it.** Each of these is extracted first, in a pull request of its own to main
that touches `crates/ui-egui` as little as it can (the egui side adopts it when it is ready, and a contributor's change to the old
copy keeps merging until then):

1. **The tool and command registry**: tabs, panels and buttons with their command ids, icon ids, Revit shortcuts and the BIMCraft and Rev
   profiles, as data. Both UIs draw the same table, and one test fails when a command has an entry in one UI and not the other, which
   is how the parity rule is kept by a machine and not by memory. It is the most valuable piece and the one the 4,187 lines of
   `shortcuts.rs` and the ribbon's code are part of.
2. **Form schemas** for the dialogs and Properties (a field is a name, a kind, a value in millimetres or a choice, a unit, a rule;
   `Units::length` and `Units::parse` apply in the core): the 18,000 lines of dialogs stop being written twice.
3. **The icons as a drawing description** (strokes and polygons in a 24-point box): `icons.rs` paints them through egui calls today.
4. **Tool state machines** (what a click, a move or a key does in the Wall tool, the sketch, the Dimension tool, with their previews
   and the command they end in), now mixed into `canvas.rs` with egui events; the core already has the shapes (`draw.rs`, `snap.rs`,
   `alignment.rs`) and gets the machines.
5. **The context menu** (`Action`, `context_items`, `act`), **the workers** (`view_worker.rs`, `section_worker.rs`, `opening.rs`, the
   export threads), **settings and window geometry** as data, **the Properties rows and the Project Browser tree as data**, and the pure
   part of **`lang`** (`Lang`, `t`, `tf`, `tc`: today beside egui's CJK font code).

**The order.**

| Stage | What | Done when |
|---|---|---|
| 0 and 1 (this branch) | the spike, the measurements, the first window, the layout and its guard | the numbers are in this file and CI is green |
| 2 | the shell: `settings.json` and the window's geometry, the two profiles as tokens, open and save (`native-file-dialog`, a big file read on a worker with Cancel), docking and tabs, the Project Browser, editable Properties through `modify`, undo and redo, the shared tool registry drawn as the ribbon and the Tools palette | a person can open a project, look at any level, change a parameter and save it |
| 3 | the plan canvas: selection (box, Tab, filters), grips, snapping and its guides, temporary dimensions, the Draw and Modify tools by family, the context menu, sections and elevations, zoom regions | the tests of `tests/guides.rs` and the tool tests replayed on Rune |
| 4 | the dialogs and managers, most used first: Type Properties, Settings, Materials, Parameters, Visibility/Graphics, Phasing | each from a form schema |
| 5 | the 3D view, sheets and schedules on screen, export and print | the sample house exports and prints from Rune |
| 6 | the Family Editor, the MEP and structure tabs, Co-Work, the Assistant, PyAddins, Plugins, Building Codes | parity: the registry test passes with no entry missing |

Before Rune becomes the default, one run on a machine with no graphics card must show it cheaper than egui on that machine with #628's
join, on a big project (the tower sample), not only the sample house, and a person must have used it for a day of real work. Until
then `bimcraft` stays egui, the Rune binary is `bimcraft-rune`, and packages do not carry it.

## What the maintainer decides

DECISIONS
