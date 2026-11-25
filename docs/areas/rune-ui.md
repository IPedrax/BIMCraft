# The Rune UI

BIMCraft's desktop is egui today (`crates/ui-egui`: 317 files, 172,000 lines with its tests, 253 of the files naming egui, `BimApp` in
`lib.rs` the hub). `crates/ui-rune` is the start of the same app drawn by [Rune](https://github.com/anomalyco/rune), a pre-release
native Rust UI framework, on the branch `IPedrax-Test/Rune-GUI`. **The egui app stays the shipped UI until Rune reaches parity**, and
`crates/ui-egui` is untouched by this work, so the pull requests that land there keep merging. The rule holds for both: every
feature in both UIs, differing only visually; the BIMCraft and Rev profiles are one app with a shared look. Notes for working in the
crate: [crates/ui-rune/CLAUDE.md](../../crates/ui-rune/CLAUDE.md).

Why: egui is immediate mode and repaints the whole interface when the pointer moves, which pins a CPU on a machine with no graphics
card (PR 628 measured 1.78 of 2 cores on a Windows VM on WARP). Rune keeps a retained tree and redraws what changed.

## What is built (stages 0 and 1, the first slice of 2, and the first four of 3)

A window of the sample house that works end to end, small on purpose:

| Module | What it does | Tests |
|---|---|---|
| `app.rs`, `app/tests.rs` | the state and what its handlers do; the only place that meets the core (`render::draw`, `Drawing::pick`, `Session::execute` for every change: `delete`, `undo`, `redo`, `modify`, `open`, `save`) | 31, no window |
| `ui/` | the plan canvas, the top bar (Open and Save through the system's file dialogs, Undo, Redo), the Tools palette or the ribbon, Properties (a `rune_kit::TextInput` for each field the core may take a `modify` of), the browser of levels, the status bar, as functions of `App` | 17, Rune's headless `Runtime` (dialogs answered by the test) |
| `project.rs`, `profile.rs` | where a project comes from (`--sample`, `--highrise`, a file through `open`), and the two profiles of one app: BIMCraft's palette, Rev's ribbon | 2 |
| `camera.rs` | pan, zoom and Zoom In Region in the core's `View`, hostile numbers refused | 6 |
| `select.rs` | what a click or a box does to the selection: Ctrl adds, Shift takes away, a plain pick replaces; a box left to right takes what it holds, right to left what it touches (stage 3, slice 1) | 3 |
| `paint.rs`, `paint/snap.rs` | a `Drawing` as Rune shapes: strokes, polygons, dashes split, lettering through `text::triangles`, colours in linear light, the box being dragged; and what a pick shows: the points picked, the guides, the glyph of the snap | 7, 6 |
| `snapping.rs` | where a point picked at the pointer lands: the core's `snap::snap` and `AlignmentGuideService` with the pixel reach (10 and 6), the guide held a little longer (12 px to leave, 8 to enter), the name of what it is on (stage 3, slice 4) | 6 |
| `tools.rs` | the table of tools both profiles draw (the seed of the shared registry below) | 2 |
| `props.rs`, `status.rs` | the Properties rows and editable fields from `element_json`, what a typed value becomes; the status line | 3, 3 |
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

The question that started this: egui repaints the whole interface when the pointer moves. Measured on GitHub's runners, which have no
graphics card (Linux: 4 vCPUs, an X server in Xvfb, Mesa's lavapipe as the only Vulkan device; Windows: 4 vCPUs, a 1024x768 desktop, the
Basic Render Driver, which is WARP, and egui's own note on that desktop says "No graphics card found: drawing in software, frame rate
limited"), by `.github/workflows/rune-ui.yml` (jobs `build-*`, `measure` and `measure-windows`; `ci/measure-all.sh`, `ci/measure-ui.sh`,
`ci/measure-ui.ps1`; artifacts `measurements` and `measurements-windows`). The local machine has a card and no software Vulkan, and
builds nothing, so none of it was done here.

**Method.** Each UI starts in a 1280x800 window (the Windows desktop is smaller, so the right-hand column is off screen there; the pointer
stays on the plan), settles for 12 seconds (45 for the tower), and sits idle for 10. Then the pointer moves round an ellipse over the plan
(1,500 moves, about 55 a second; `xdotool` on Linux, `SetCursorPos` on Windows), the wheel turns (200 notches, ten in and ten out, about
30 a second) and the plan is dragged once round a circle (1,000 moves, the middle button for egui, which pans with it, the left for Rune).
The process's CPU time (user and system, every thread, so the software rasterizer's threads too) is read over each stretch and divided by
the events. Every UI ran twice, one after the other; a table gives the range of the two. Idle is 0.00 cores for every UI, egui included.
*Cores* is CPU seconds a second (4 is the machine); the other columns are CPU milliseconds an event, the nearest thing to a frame's cost
that the same method gives both toolkits. Rune is `bimcraft-rune` of this branch: the top bar, a Tools palette of 36 rows (the BIMCraft
profile) or a ribbon (Rev's), the plan, Properties, the levels of the browser and the status bar. egui is `bimcraft` as it is on this
branch's base (ec8b641df) and as main has it since 73aceeae8, the join of PR 628 (frames paced by their cost on a software adapter, one
in flight).

**Linux, the sample house** (12,500 instances in the plan):

| UI | pointer: cores | pointer: CPU ms a move | wheel: ms a notch | drag: ms a move | memory MB |
|---|---|---|---|---|---|
| egui, base of this branch (ec8b641df) | 2.499 to 2.714 | 42.61 to 45.09 | 388.5 to 413.8 | 49.55 to 52.45 | 476 to 495 |
| egui, main since PR 628's join (`BIMCRAFT_SOFTWARE=1`) | 1.23 to 1.362 | 21.05 to 23.32 | 219 to 233.5 | 27.64 to 29.68 | 461 to 480 |
| Rune on lavapipe (its default) | 2.616 to 2.794 | 43.08 to 46.18 | 72.1 to 72.55 | 25.18 to 26.45 | 265 to 267 |
| Rune on its CPU renderer | 0.178 | 2.89 | 42.45 | 22.16 to 22.2 | 91 |
| Rune on its CPU renderer, Rev's ribbon | 0.115 to 0.117 | 1.9 to 1.91 | 40.3 to 40.45 | 21.51 to 21.61 | 91 |
| Rune on lavapipe, Rev's ribbon | 2.601 to 2.773 | 42.77 to 46.06 | 69.2 to 69.25 | 25.04 to 25.8 | 263 |
| Rune on lavapipe, the plan as an image | 3.324 to 3.536 | 55.15 to 58.65 | 102.15 to 104.1 | 28.06 to 29.14 | 259 to 260 |
| Rune on its CPU renderer, the plan as an image | 0.169 to 0.173 | 2.77 to 2.85 | 79.75 to 80.05 | 27.7 to 27.73 | 90 |

**Linux, a big project**: three storeys of the annotated reference tower, saved by `bimcraft-cli` and opened by both (about 30,000
elements; the plan of a level is the dense picture the Rune window shows):

| UI | pointer: cores | pointer: CPU ms a move | wheel: ms a notch | drag: ms a move | memory MB |
|---|---|---|---|---|---|
| egui, base of this branch | 2.068 to 2.081 | 34.65 to 34.81 | 334.1 to 334.15 | 44.83 to 45.74 | 3365 to 3627 |
| egui, main since PR 628's join | 0.995 to 1.025 | 16.61 to 16.96 | 200.85 to 208.35 | 28.41 to 28.66 | 3350 to 3528 |
| Rune on lavapipe | 1.899 to 1.922 | 31.33 to 31.71 | 136.1 to 136.3 | 16.47 to 16.49 | 1352 to 1354 |
| Rune on its CPU renderer | 0.959 to 0.961 | 15.55 to 15.62 | 131.45 to 131.6 | 16.4 to 16.46 | 662 to 663 |

**Windows, the sample house**:

| UI | pointer: cores | pointer: CPU ms a move | wheel: ms a notch | drag: ms a move | memory MB |
|---|---|---|---|---|---|
| egui, main since PR 628's join (it finds the missing card itself) | 3.198 to 3.328 | 102.15 to 107.04 | 76.95 to 85.47 | 58.61 to 62.95 | 179 to 194 |
| the same with `BIMCRAFT_SOFTWARE=1` | 3.3 to 3.324 | 106.43 to 106.95 | 81.02 to 81.56 | 61.17 to 62.83 | 178 to 179 |
| Rune on WARP (its default) | 1.374 to 1.41 | 38.57 to 39 | 83.2 to 84.69 | 58.69 to 58.88 | 390 to 405 |
| Rune on its CPU renderer | 0.1 to 0.151 | 2.99 to 4.29 | 48.36 to 60.78 | 33.28 to 33.83 | 62 to 64 |
| Rune on its CPU renderer, Rev's ribbon | 0.086 to 0.12 | 2.57 to 3.61 | 50.94 to 63.52 | 32.91 to 35 | 62 to 63 |

**Windows, the tower**:

| UI | pointer: cores | pointer: CPU ms a move | wheel: ms a notch | drag: ms a move | memory MB |
|---|---|---|---|---|---|
| egui, main since PR 628's join | 3.109 to 3.176 | 90.09 to 92.55 | 74.22 to 76.33 | 34.22 to 36.52 | 4220 to 4453 |
| Rune on WARP | 3.305 to 3.357 | 95.32 to 97.47 | 130.31 to 130.47 | 102.33 to 103.75 | 4724 to 4926 |
| Rune on its CPU renderer | 0.985 to 0.998 | 27.04 to 27.5 | 47.81 to 48.05 | 42.41 to 42.58 | 978 to 995 |

**What it says.**

- **A pointer moving over the plan, the case of PR 628**: Rune on its CPU renderer costs 3 to 4.5 ms of CPU a move on the house on Linux
  (0.18 cores in the last run) against egui's 43 to 45 (2.5 to 2.7) and 21 to 23 with PR 628's join (1.2 to 1.4): a tenth to a fifteenth of
  egui's, and a fifth to an eighth of the join's. On Windows, 3 to 4 ms (0.1 to 0.15 cores) against egui's 102 to 107 ms (3.2 to 3.3 cores of
  4) with the join and 106 with the variable forced: **a twenty-fifth to a thirty-fifth, on a machine whose adapter is the one that started
  this**. PR 628's join does not make egui cheap per move there.
- **Rune's default path is only as cheap as the adapter lets it be.** It hands the window to the GPU as soon as a device exists. On
  lavapipe that costs 43 to 46 ms a move (2.6 to 2.8 cores), about egui's; on WARP 39 ms (1.4 cores) against egui's 105, which is 2.7 times
  less but ten times the CPU renderer's. What is cheap is Rune's CPU renderer, which repaints the tiles that changed (here the hover layer
  and the status text), not the window.
- **A denser chrome costs the CPU renderer nothing**: with Rev's ribbon (a tab's 13 tools and the 10 of Modify, each a button with its icon and name)
  over the same window it is 1.9 ms a move on Linux and 2.6 to 3.6 on Windows; the GPU path stays where it was.
- **A big plan is where Rune's lead narrows**: on the tower the CPU renderer costs 15.6 ms a move on Linux (1.0 cores) against egui's 35
  and 17 with the join, and 27 ms on Windows against 90 to 93. One cost in this crate grows with the plan, the hover pick, which walks
  every primitive of the drawing (`Drawing::pick`, the core's, which egui calls the same way); it was not isolated here. A spatial index in
  the core would serve both UIs. Rune on the GPU path with the tower is 31 ms on Linux and 95 to 97 on Windows (3.3 cores).
- **Memory**: Rune on its CPU renderer 91 MB on the house and 660 MB on the tower, against egui's 460 to 495 and 3.4 to 3.6 GB; on Windows
  62 MB and 1 GB against 180 to 195 MB and 4.2 to 4.5 GB, and Rune on WARP with the tower 4.7 to 4.9 GB.
- **The plan as the core's own raster in one image** (`BIMCRAFT_RUNE_PLAN=pixels`, `render::raster`) is no better: the same 3 ms of hover
  on the CPU renderer, 55 to 59 ms on lavapipe, and on the CPU renderer a wheel notch 1.9 times dearer than as shapes. The shapes stay.
- **The wheel and a drag**: Rune 42 ms a notch on the CPU renderer (72 on lavapipe), egui 389 to 414 (219 to 233 with the join); a drag
  22 ms a move, egui 50 to 52 (28 to 30). Not to be taken at face value: a drag costs the same 22 ms in every Rune variant, which points at
  the one thing they share, the core's `render::draw` of the plan, run once per event on the UI thread. That is a naive loop; egui avoids
  it by moving the last drawing and asking a worker for the exact one. Rune's answer is `live_offset`, `live_scale` and `tx.live_only()`, a
  camera that skips the view, the diff and the layout (its own bench: 13.7 ms a frame down to 2.0 at 6,000 elements); stage 3 does the same.
- **The live camera, measured (stage 3, slice 3, run 45525844245 at 3dcf6079b; two runs each, the pan now with the middle button as egui's).**
  `App::glide` moves the last drawing through `live_offset` and `live_scale` and has the core draw it exactly 150 ms after the camera
  stops. CPU time a wheel notch / a pan step, before (the same code with the core drawing again each time) and after:

  | | wheel, before | wheel, after | pan, before | pan, after |
  |---|---:|---:|---:|---:|
  | Windows, WARP, CPU renderer, house | 48 to 61 | 1.8 to 2.6 | 33 | 12.6 to 13.4 |
  | Windows, CPU renderer, tower | 48 | 52 to 67 | 42.5 | 39 |
  | Windows, default GPU path, house | 83 to 85 | 87 to 93 | 59 | 80 to 81 |
  | Linux, lavapipe, CPU renderer, house | 42 | 95 | 22 | 11.8 to 11.9 |
  | Linux, CPU renderer, tower | 131 | 85 to 97 | 16.4 | 21.8 to 22.1 |
  | Linux, lavapipe GPU path, house | 72 | 298 to 307 | 25 to 26 | 30.8 |
  | Linux, lavapipe GPU path, tower | 136 | 283 to 297 | 16.5 | 26 |

  The pointer over the plan (2.8 ms a move on the CPU renderer) and the idle cost did not move. So the live camera is a large win where it
  was meant for, a Windows machine with no card on the CPU renderer over a plan of the house's size (a wheel notch a twenty-fifth, a
  pan step two fifths), level over the tower there, and mixed or worse on Linux and on any software-GPU path, where Rune seems to draw
  a scaled subtree again in full each frame (the cost sits in the live frames, not in the core). I have not profiled why. The window
  therefore opens in `CameraMode::Live` on Windows and `Exact` elsewhere (`BIMCRAFT_RUNE_CAMERA=live` or `exact` chooses), to be measured
  again when Rune's Linux CPU renderer is released. A real card, which composites a scaled layer for nothing, was not measured.

  Two more runs say how far to trust the table. The Windows house, `Live` again (runs 45532642548 and 45538998003): a wheel notch 2.8 to
  3.4 and then 4.2 to 5.6 ms, a pan step 13.6 and then 18.1 to 18.4 against 33 for `Exact`: the runners differ by a third, the win does not.
  And a trap on Linux: with `Exact` still on, but the layers inside the wrapper element bound to the live offset and scale at rest (run
  45532642548), the house's wheel notch cost 80 to 84 ms against 42 and its pan step 17 against 22, so the wrapper alone, doing nothing,
  changes the cost on that renderer in both directions (not profiled). `Exact` therefore has no wrapper; the layers are the canvas's own
  children, as before, and every Linux number came back to the commit before the live camera's (run 45538998003: house wheel notch 43,
  pan 22.2, pointer 2.8 to 3.0 ms; tower 133 and 16.5).
- **Frame cost, by each toolkit's own clock.** egui's `--bench` (frames forced as if the user kept moving, on the real window): a median
  frame of 22 to 29 ms idle and zoomed, 33 ms panned, on two different runners; with the join of PR 628, 48 to 82 ms, because it paces
  on cost (14 to 15 frames a second panned or zoomed against 31 to 32: PR 628 buys its CPU with frame rate). Rune headless on lavapipe
  (`--script`, CPU time of update, build and submit): a hover frame at a median of 42 to 65 ms over at least four runs of the same binary, and a zoom
  scene that leaves 832 instances in view costs 44 to 67 ms too, so the GPU path's cost on a software adapter is a floor per frame and not
  a function of the content. One full-window repaint on the CPU renderer (1280x800 at 2x, 12,516 instances) is 19.5 ms of CPU, and 44 of its
  4,096,000 pixels differ from the GPU's frame, by 9 of 255 at most.
- **Repeatability**: the two runs of a variant agree within 10% in one run. Across the runs of 2026-11-24 the same variant moved by 8% for
  the others and from 2.9 to 4.5 ms for Rune's CPU renderer (the runners differ), so the ratios above are the finding, not the third
  digit.

**What could not be measured here, and bears on the answer.**

1. **dansity's own Windows VM.** A GitHub Windows runner is the same class of machine (no card, WARP, 4 vCPUs) and egui itself says so, but
   it is not that VM: `bimcraft-rune --software` on it is the check that settles the case, and the artifact `bimcraft-rune-windows` of
   the run is the binary to give them.
2. **A chrome as dense as egui's.** egui here is the whole app (Tools, Inspector, Navigator, view bar, status bar); Rune's window now has
   a top bar, a palette or a ribbon, Properties, a browser and a status bar (about 150 elements). It costs the CPU renderer nothing, but
   the Family Editor's panels and the dialogs are not in it.
3. **The hundred-storey tower**, which the core's own work is measured on: three annotated storeys were used. A real card was not used by
   either UI. macOS (Metal, no software path), a screen reader in use, and the CJK fallback with no CJK font installed were not measured.
4. **Pan and zoom as a finished UI would have them** (above).

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
of `rune-ui.yml` build and test this crate on `ubuntu-24.04`, `windows-latest` and `macos-15` (run): fmt, `clippy -D warnings`, 35 unit tests and the layout guard pass on all three (34 and the guard on the commit the
measurement ran on, before `software.rs`). Times with the build cache warm: Linux clippy 45 s and tests 6 min, Windows 1 and 8 min,
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
| 2 (first slice built, see "Where stage 2 stands") | the shell: `settings.json` and the window's geometry, the two profiles as tokens, open and save (`native-file-dialog`, a big file read on a worker with Cancel), docking and tabs, the Project Browser, editable Properties through `modify`, undo and redo, the shared tool registry drawn as the ribbon and the Tools palette | a person can open a project, look at any level, change a parameter and save it |
| 3 | the plan canvas: selection (box, Tab, filters), grips, snapping and its guides, temporary dimensions, the Draw and Modify tools by family, the context menu, sections and elevations, zoom regions | the tests of `tests/guides.rs` and the tool tests replayed on Rune |
| 4 | the dialogs and managers, most used first: Type Properties, Settings, Materials, Parameters, Visibility/Graphics, Phasing | each from a form schema |
| 5 | the 3D view, sheets and schedules on screen, export and print | the sample house exports and prints from Rune |
| 6 | the Family Editor, the MEP and structure tabs, Co-Work, the Assistant, PyAddins, Plugins, Building Codes | parity: the registry test passes with no entry missing |

**Stage 3 so far.** Slice 1, selection, is built (`select.rs`, `App::sweep`, `release`, `step`, `escape`, the box layer in
`ui/plan.rs`): a click replaces the selection, Ctrl adds and Shift takes away (read as `tx.mods()`); a drag with nothing in hand draws a
box, solid and blue left to right (what it holds), dashed and green right to left (what it touches), through the core's `Drawing::in_box`;
Tab and Shift+Tab step through `Drawing::pick_all` at the pointer and a click takes the one Tab reached; Escape puts the tool down, then
clears the selection. The middle button pans (`El::pointer_buttons`), as in egui, so the primary drag no longer does, except while a tool is
in hand (a placeholder until the tools take the button). Slice 2, the zoom commands, is written too: Zoom to Fit, Zoom In and Out about the middle, Zoom In Region (armed from the top bar, one box, drawn as a
window whichever way it goes, a click leaves it armed, Escape disarms it) and Previous Pan/Zoom (twenty steps of what the commands left, as in egui's
`zoom.rs`; the wheel and a pan are no steps). Their Revit shortcuts (ZF, ZR, ZO, ZP) wait for the shared registry. Slice 3, the live camera, is the
answer to the naive wheel and drag of "What costs what", and its measurement there is mixed: a wheel notch or a pan moves the last drawing through
`live_offset` and `live_scale` (`App::glide`; hover and picks map the pointer back into the drawing), and the core draws it exactly once the camera has been
still for 150 ms (`ui::poll`) or a click, a box or a command needs it. It is the default on Windows only (`CameraMode`). Rune's `Live` is a lock-free 128-bit atomic and asserts that the CPU has them, so the
crate would stop at start on one that does not (x86_64 before 2007, riscv64); Rune's own platforms have them. Slice 4, snapping, is built
(`snapping.rs`, `paint/snap.rs`, `App::landing`, `picked`, `pick_point`): with a tool in hand the pointer lands as egui's does, through the core's
`snap::snap` (what the plan has there, an angle step from the last point picked, else the nearest 10 mm) and the temporary alignment guides
(`AlignmentGuideService`), and the plan shows the glyph of the snap, its name beside it, the guide it follows (a dashed run, and a blue square on the
reference it lines up with) and the points picked so far with the segment on to the pointer. A click with a tool in hand picks the landing point
(at most `MAX_POINTS`); Shift, read at each pointer move, keeps the step horizontal or vertical; Escape gives up the points, then the tool, then the
selection. The tools do not take the points yet, so they are only the chain the plan shows. The layer is there only while a tool is in hand, so the
plan's elements, which the measurements were made on, are as they were with none. **Not in the slice, from egui's canvas:** the lines of an underlay,
a camera's eye, a sketch's lines, the background grid, Tab's wall face, the marks a rest on a point leaves, the wall the chain began on and the grids
near the cursor as guide sources (the guides come from the points picked and, with `Snaps::guides_from_whole_model`, from the whole plan), the
glyph's refined look, the badge, and Manage > Snaps (the switches are `Snapper::snaps`, all on). A Shift pressed without the pointer moving is read
at the next move. Not measured: what a snap costs per pointer move on the tower, with or without a card (the core's `Scene::of` runs for each).
Not in the slices: the Filter dialog (stage 4), grips, temporary dimensions, the Draw and Modify tools, the context menu, sections. The tools and grips
lean on the tool state machines, which are still inside egui's `canvas.rs` (extraction 4 above), so they wait on that pull request.

Before Rune becomes the default, one run on a machine with no graphics card must show it cheaper than egui on that machine with PR 628's
join, on a big project (the tower sample), not only the sample house, and a person must have used it for a day of real work. Until
then `bimcraft` stays egui, the Rune binary is `bimcraft-rune`, and packages do not carry it.

## What the maintainer decides

**Where stage 2 stands.** Done, with tests, on all three systems: the two profiles from one table, project sources, Open and Save through
the system's dialogs (the feature builds on Windows and macOS), levels in a browser, editable Properties through `modify`, and the
measurements the gates asked for. Not done: `settings.json` and the window's remembered size, docking and tabs (`SplitView` and `TabStrip`
are in the kit, nothing here uses them), views in the browser besides levels, and all five extractions into the core (a pull request each
to main, the tool registry first). The gates: the **Windows** gate is met on a GitHub Windows runner (WARP), not on dansity's VM; the **dense
chrome and big project** gate is met with a ribbon, a palette, a browser and a three-storey tower (not the hundred); the **upstream
issue** was filed on 2026-11-24 as [anomalyco/rune#2068](https://github.com/anomalyco/rune/issues/2068), with no answer yet.

1. **Go on to stage 3, or stop and take Iced?** The gates ran. With no card, Rune's CPU renderer costs a tenth to a fifteenth of
   egui's CPU when the pointer moves over the plan on Linux, and a twenty-fifth to a thirty-fifth on a Windows runner (WARP), where egui
   with PR 628's join is still at 3.2 cores; on a three-storey tower the lead narrows to a half (Linux, against the join) or a third
   (Windows). Rune's default path, which takes a software GPU, is as expensive as egui on lavapipe and 2.7 times cheaper on WARP. The case
   for Rune is one feature, the CPU renderer, that is a few days old (Windows 2026-11-12, Linux 2026-11-24), unreleased, off by default
   and switched by an environment variable. My recommendation is **to go on, with what is left of the gates**: a run of
   `bimcraft-rune --software` on dansity's own VM (the binary is the artifact `bimcraft-rune-windows` of the latest `rune-ui.yml` run), the
   hundred-storey tower, and an answer upstream to the issue below. If one fails, the fallback is Iced (0.14, tiny-skia, incremental
   rendering), not measured here.
2. **A pre-release dependency with no API promise.** Rune is `0.0.1`, pinned to a revision of `dev`; it changes daily (more than 1,200
   unreleased changelog entries), takes no pull requests, and one company writes it. A bump is one revision in five places and a rebuilt
   lock, plus whatever its API changed; the crate keeps Rune out of `app.rs`, `camera.rs`, `paint.rs`, `props.rs`, `status.rs` and
   `lang.rs` (the logic, tested without a window) so a bump touches `ui/` only. Is that risk acceptable for a front end that may one
   day be the only one?
3. **The issue at github.com/anomalyco/rune**, filed 2026-11-24 as [#2068](https://github.com/anomalyco/rune/issues/2068) (its body names no
   project and links nothing of ours; it points at their #296 and #301, which are the same machine class). Its first draft read: "Linux and Windows: choose the CPU
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
