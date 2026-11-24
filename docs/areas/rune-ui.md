# The Rune UI

BIMCraft's desktop is egui today (`crates/ui-egui`: 317 files, 172,000 lines with its tests, 253 of the files naming egui, `BimApp` in
`lib.rs` the hub). `crates/ui-rune` is the start of the same app drawn by [Rune](https://github.com/anomalyco/rune), a pre-release
native Rust UI framework, on the branch `IPedrax-Test/Rune-GUI`. **The egui app stays the shipped UI until Rune reaches parity**, and
`crates/ui-egui` is untouched by this work, so the pull requests that land there keep merging. The rule holds for both: every
feature in both UIs, differing only visually; the BIMCraft and Rev profiles are one app with a shared look. Notes for working in the
crate: [crates/ui-rune/CLAUDE.md](../../crates/ui-rune/CLAUDE.md).

Why: egui is immediate mode and repaints the whole interface when the pointer moves, which pins a CPU on a machine with no graphics
card (PR 628 measured 1.78 of 2 cores on a Windows VM on WARP). Rune keeps a retained tree and redraws what changed.

## What is built (stages 0 and 1)

A window of the sample house that works end to end, small on purpose:

| Module | What it does | Tests |
|---|---|---|
| `app.rs` | the state and what its handlers do; the only place that meets the core (`render::draw`, `Drawing::pick`, `Session::execute`) | 8, no window |
| `ui/` | the plan canvas, the Tools palette, the Properties panel, the status bar, as functions of `App` | 6, Rune's headless `Runtime` |
| `camera.rs` | pan and zoom in the core's `View`, hostile numbers refused | 4 |
| `paint.rs` | a `Drawing` as Rune shapes: strokes, polygons, dashes split, lettering through `text::triangles`, colours in linear light | 6 |
| `tools.rs` | the Tools palette's table (the seed of the shared registry below) | 2 |
| `props.rs`, `status.rs` | the Properties rows from `element_json`; the status line | 2, 3 |
| `lang.rs` | `t`, `tf`, `tc` over the catalogs egui reads (`translations/ui/*.po`), English fallback | 3 |
| `software.rs` | `--software`: starts again with `RUNE_CPU_FIRST=only` (see "Choosing the CPU renderer") | 1 |
| `tests/layout.rs` | no source file passes 1,200 lines | 1 |

`props.rs`, `status.rs` and `tools.rs` stay in this crate, though the root rule is that what any front end needs belongs in the core:
they are a hundred lines each and nothing in egui could use them yet. Each moves to the core, as a pull request of its own, the first
time egui adopts it (see "what stays shared").

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
promise (25 library crates, and the unreleased changelog already holds more than 1,200 entries), takes issues and no
pull requests, and changes daily. Bumping it is one revision in five places (the crate's `CLAUDE.md`) and a rebuilt lockfile.

## What costs what with no graphics card

The question that started this: egui repaints the whole interface when the pointer moves. Measured on GitHub's `ubuntu-24.04`
runner, which has no graphics card (4 vCPUs, an X server in Xvfb, Mesa's lavapipe as the only Vulkan device), by
`.github/workflows/rune-ui.yml` (jobs `build-*` and `measure`, `ci/measure-all.sh`, `ci/measure-ui.sh`; the run of 2026-11-24 on
`IPedrax-Test/Rune-GUI`, artifact `measurements`). The local machine has a card and no software Vulkan, and builds nothing, so this was
not done here.

**Method.** Each UI starts on the sample house in a 1280x800 window, settles for 12 seconds, and sits idle for 10. Then `xdotool`
moves the pointer round an ellipse over the plan (1,500 moves, about 55 a second), turns the wheel (200 notches, ten in and ten out,
about 30 a second) and drags the plan once round a circle (1,000 moves, with the middle button for egui, which pans with it, and the
left for Rune). The process's CPU time (user and system, every thread, so lavapipe's rasterizer threads too) is read over each
stretch and divided by the number of events. Every UI ran twice, one after the other on the same machine; the table gives the range
of the two. egui is `bimcraft --sample` as it is on this branch's base (ec8b641df), and as main has it since 73aceeae8, the join of PR 628 (run with
`BIMCRAFT_SOFTWARE=1`: frames paced by their cost on a software adapter, one in flight). Rune is `bimcraft-rune`, the window of this
branch: the Tools palette (36 rows), the plan, Properties and the status bar.

| UI | runs | idle cores | pointer: cores | CPU ms per move | wheel: CPU ms per notch | drag: CPU ms per move | RSS MB |
|---|---|---|---|---|---|---|---|
| egui, base of this branch (ec8b641df) | 2 | 0.00 | 2.438 to 2.545 | 43.07 to 45.01 | 391.1 to 391.9 | 50.21 to 50.4 | 414 to 420 |
| egui, main since the join of PR 628 (73aceeae8, BIMCRAFT_SOFTWARE=1) | 2 | 0.00 | 1.21 to 1.246 | 20.63 to 21.37 | 222.25 to 223 | 28.04 to 28.16 | 405 to 407 |
| Rune, shapes, on lavapipe (its default) | 2 | 0.00 | 2.605 to 2.623 | 46.23 to 46.4 | 131.95 to 132.65 | 17.63 to 17.65 | 266 to 267 |
| Rune, shapes, CPU renderer (RUNE_CPU_FIRST=only) | 2 | 0.00 | 0.277 to 0.278 | 4.49 to 4.51 | 71.55 to 75.65 | 17.03 to 17.04 | 94 |
| Rune, plan as an image, on lavapipe | 2 | 0.00 | 3.269 to 3.317 | 57.34 to 58.56 | 226.25 to 233.7 | 17.45 to 17.48 | 263 to 266 |
| Rune, plan as an image, CPU renderer | 2 | 0.00 | 0.277 to 0.292 | 4.49 to 4.74 | 167.95 to 176.8 | 17.23 to 17.31 | 93 |

*Cores* is CPU seconds a second of the stretch (4 is the machine). *CPU ms per move* is the process's CPU time over the stretch
divided by the events, the nearest thing to a frame's cost the same method gives both toolkits.

**What it says.**

- **Idle is free everywhere**: no UI uses CPU at rest (0.00 cores), egui included.
- **A pointer moving over the plan, the case of PR 628**: Rune on its CPU renderer costs 4.5 ms of CPU a move (0.28 cores), a tenth
  of egui's 43 to 45 ms (2.4 to 2.5 cores) and a fifth of egui with the join of PR 628 (21 ms, 1.2 cores). The same Rune on its default
  path, which hands the window to the GPU as soon as a device exists, and on a software adapter that is lavapipe or WARP, costs 46 ms
  (2.6 cores): as much as egui before PR 628. So **Rune is not cheaper than egui on software Vulkan, and is much cheaper on its own CPU
  renderer**, which repaints the tiles that changed (here the hover layer and the status text) and not the window.
- **The plan as the core's own raster in one image** (`BIMCRAFT_RUNE_PLAN=pixels`, `render::raster`) is no better: the same 4.5 ms of
  hover on the CPU renderer, 57 to 59 ms on lavapipe, and on the CPU renderer a wheel notch 2.3 times dearer than as shapes. The shapes stay.
- **The wheel and a drag**: Rune 72 to 76 ms a notch on the CPU renderer (132 on lavapipe) against egui's 391 (222 with the join),
  and 17 to 18 ms a drag move against egui's 50 (28). Not to be taken at face value: a drag costs the same 17 ms in every Rune variant,
  on the GPU path or the CPU renderer, as shapes or as an image, which points at the one thing they share, the core's `render::draw` of
  the plan, run once per event on the UI thread. That is a naive loop; egui avoids it by moving the last drawing and asking a worker for
  the exact one. Rune's own answer is `live_offset`, `live_scale` and `tx.live_only()`, a camera that skips the view, the diff and the
  layout (its own bench: 13.7 ms a frame down to 2.0 at 6,000 elements); stage 3 does the same here.
- **Memory**: Rune on the CPU renderer 94 MB, on lavapipe 266 MB, egui 405 to 420 MB.
- **Frame cost, by each toolkit's own clock.** egui's `--bench` (frames forced as if the user kept moving, on the real window): a median
  frame of 28 to 29 ms idle and zoomed, 33 ms panned; with the join of PR 628, 60 to 82 ms, because it paces on cost (14 to 15 frames a second panned or
  zoomed against 31 to 32: PR 628 buys its CPU with frame rate). Rune headless on lavapipe (`--script`, CPU time of update, build and submit): a
  hover frame at a median of 42 to 65 ms over three runs of the same binary, and the zoom scene, which leaves 832 instances in view, costs
  67 ms too, so the GPU path's cost on a software adapter is a floor per frame and not a function of the content. One full-window
  repaint on the CPU renderer (1280x800 at 2x, 12,516 instances) is 19.5 ms of CPU, and 44 of its 4,096,000 pixels differ from the GPU's
  frame, by 9 of 255 at most.
- **The two runs of every UI agree** within 5% (the table's ranges), but the headless frame times of one binary moved by half between
  runs, so read those as an order of magnitude.

**What could not be measured here, and bears on the answer.**

1. **A real Windows VM on WARP**, the machine of PR 628. This is Linux and lavapipe on a shared 4 vCPU runner. The direction
   should hold (Rune's CPU renderer landed on Windows on 2026-11-12, on Linux on 2026-11-24, both unreleased; WARP is a software adapter
   as lavapipe is), the numbers will not. dansity's VM is the test: `bimcraft-rune --software`.
2. **A chrome as dense as egui's.** egui here is the whole app (Tools, Inspector, Navigator, view bar, status bar); Rune's window has
   a palette, the plan and a panel (about 100 elements). The CPU renderer's cost should follow what changes, not what exists, and the GPU
   path's should not; that is a claim until stage 2 has the ribbon and the browser.
3. **A big project.** Only the sample house (12,500 instances). The tower sample is the real test of both.
4. **A real card.** Neither UI was measured on one; nothing here says Rune is faster or slower than egui with a GPU.
5. **Pan and zoom as a finished UI would have them** (above), **macOS** (Metal, no software path), the **CJK fallback with no CJK font
   installed** (the runner had Noto CJK, which `fonts.txt` lists), and **a screen reader** actually used.

**Choosing the CPU renderer.** Rune reads `RUNE_CPU_FIRST` from the environment when it makes a window, and a program cannot set its
own variable without `unsafe` (`std::env::set_var` is, in edition 2024), which this workspace forbids. `bimcraft-rune --software`
(or `BIMCRAFT_SOFTWARE=1`, the variable the egui app has since PR 628) therefore starts the program again with the variable set
(`software.rs`: `exec` on Unix, a child on Windows, a test of when it does). What is still open is the choice itself. Rune
does not pick the CPU renderer for a software adapter: lavapipe and WARP make a device, so the window goes to them and costs what the
table says. The window has to be told, by a flag, a setting, or a detection (the egui app notices a software adapter after wgpu has made it;
Rune would have to be asked before its window exists). The proper fix is upstream, an `App` option and a software-adapter rule; Rune takes
issues and no pull requests, so that is an issue to file, whose text is under "What the maintainer decides".

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
there; BIMCraft's notes on the plan do not depend on it, since the core reads the fonts and draws their outlines itself. (run) On the runner, with `fonts-noto-cjk` installed, `LC_ALL=zh_CN.UTF-8`
draws 墙, 门, 窗, 属性, 删除, 撤销 and 重做 in the palette and Properties through the system fallback, beside Inter's Latin where the
catalog has no Chinese ("Curtain Wall"); `vi_VN` draws the stacked Vietnamese marks (Cột kết cấu, Ống luồn dây, Thuộc tính) in Inter;
`pl_PL` draws ł, ś and ó (Właściwości, Ściana, Usuń). Not tried: no CJK font installed, Korean and Japanese, right to left.

**Window, file dialogs, language.** The window is winit's (patched), with a title, a size, a minimum size and, on macOS, `Chrome` for a
full-size titlebar. `tx.resize_window` and `WindowOptions::position` set size and place; Wayland can neither see nor set a window's
position (winit's own limit, egui has it too), and the frame Rune remembers across launches (`rune_mac::remember_frame`) is macOS
only, so size and position keep going in `settings.json` as `window.rs` does now. File dialogs are the `native-file-dialog` feature
(not enabled yet): `rfd` on Windows and macOS, and on Linux Rune calls the XDG desktop portal's FileChooser itself over zbus and
returns an error when the desktop has no portal, where egui's `rfd` falls back to GTK; the asks are `tx.open_file`, `tx.save_file` and
`tx.open_folder` with a callback, and a headless run answers them from a script (`Runtime::resolve_file_dialog`), which makes Open and
Save testable. The system language stays `sys-locale` through `lang.rs`; Rune has its own catalog crate (`rune-i18n`, ICU4X) that
BIMCraft does not need. Rune's web target (WebGPU, experimental, `WEB.md`) is not a goal: BIMCraft is a desktop app.

**Windows and macOS builds.** Rune's README: on Windows the MSVC toolchain needs only the Visual Studio C++ build tools; the GNU
toolchain also needs MinGW-w64's binutils for `dlltool`. On macOS the default Metal backend is Rune's own (objc2), with shader libraries
checked in (Xcode's Metal toolchain only to rebuild them). On Linux the `-dev` packages of `libxkbcommon` and `libwayland` at build
time, `libxkbcommon-x11-0` to open a window, and a software Vulkan driver (Mesa's lavapipe) on a machine with no card. The `check` jobs
of `rune-ui.yml` build and test this crate on `ubuntu-24.04`, `windows-latest` and `macos-15` (run): fmt, `clippy -D warnings`, 34 unit tests (35 with `software.rs`) and the layout guard pass on all three on
the commit that carries the measurement. Times with the build cache warm: Linux clippy 45 s and tests 6 min, Windows 1 and 8 min,
macOS 2 and 11 min; Linux with no cache, the Rune graph and the core included: clippy 2 min 16 s, the test build 8 min 30 s.

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

Before Rune becomes the default, one run on a machine with no graphics card must show it cheaper than egui on that machine with PR 628's
join, on a big project (the tower sample), not only the sample house, and a person must have used it for a day of real work. Until
then `bimcraft` stays egui, the Rune binary is `bimcraft-rune`, and packages do not carry it.

## What the maintainer decides

1. **Go on past stage 1, or stop and take Iced?** The numbers do not say stop: with no card, Rune's CPU renderer costs a tenth of what
   egui costs when the pointer moves over the plan, and 4.7 times less than egui with the join of PR 628. They do not say Rune is simply
   faster either: on its default path on a software adapter it costs the same as egui. The case for Rune is one feature, the CPU
   renderer, that is a few days old (Windows 2026-11-12, Linux 2026-11-24), unreleased, off by default, switched by an environment
   variable, and whose only measurement I know of is its author's. My recommendation is **stage 2 only, with three gates**: a run of
   `bimcraft-rune --software` on a real Windows VM on WARP (dansity's, who measured PR 628) with the table's method; the same measurement again
   once the chrome is as dense as egui's and on the tower sample; and an answer upstream to the issue below. If a gate fails, the
   fallback is Iced (0.14, tiny-skia, incremental rendering), not measured here.
2. **A pre-release dependency with no API promise.** Rune is `0.0.1`, pinned to a revision of `dev`; it changes daily (more than 1,200
   unreleased changelog entries), takes no pull requests, and one company writes it. A bump is one revision in five places and a rebuilt
   lock, plus whatever its API changed; the crate keeps Rune out of `app.rs`, `camera.rs`, `paint.rs`, `props.rs`, `status.rs` and
   `lang.rs` (the logic, tested without a window) so a bump touches `ui/` only. Is that risk acceptable for a front end that may one
   day be the only one?
3. **The issue to file at github.com/anomalyco/rune** (the maintainer's to post; nothing was sent): "Linux and Windows: choose the CPU
   renderer from the app, and by default when the adapter is a software one. `RUNE_CPU_FIRST=only` keeps a window on `CpuPainter`, which
   costs a tenth of the GPU path on lavapipe when the pointer moves over a 12,500-instance scene (4.5 ms against 46 ms of CPU a move,
   Mesa lavapipe, Xvfb, 4 vCPUs). An app that forbids `unsafe` cannot set the variable; an `App` option, and a rule that treats lavapipe
   and WARP (`DXGI_ADAPTER_FLAG_SOFTWARE`) as no GPU, would let a machine with no card draw on the CPU without a flag." Plus the question
   whether `CpuPainter` is meant to be used for a whole session, and what it does not draw.
4. **One workspace or two**, decided here and open to reversal: two (`crates/ui-rune` excluded from the root), at the cost that
   `cargo test --workspace` does not reach it, a second `Cargo.lock`, and a workflow of its own. The alternative, a branch that drops
   egui, cannot merge while egui ships.
5. **The extractions.** The plan above moves five pieces into the core before Rune uses them (the tool registry first). Each is a
   pull request to main that touches `crates/ui-egui` a little, among the 25 open ones. Who does them, and in what order relative to
   the contributors' PRs, is the maintainer's call; stage 2 needs the registry and the form schemas before its ribbon and dialogs.
6. **Packaging.** Nothing ships Rune yet and `bimcraft` stays egui. A package with the Rune UI needs `fonts/inter/OFL.txt` beside
   `NOTICE`, Rune's MIT notice (both are in the repository), and the MPL-2.0 `spellbook` crate named if it is ever compiled in.
