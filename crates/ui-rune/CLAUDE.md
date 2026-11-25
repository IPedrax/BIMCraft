# bimcraft-ui-rune

The desktop front end in [Rune](https://github.com/anomalyco/rune), a thin shell over the core's `Session`, begun on `IPedrax-Test/Rune-GUI`.
Experimental: **the egui front end (`crates/ui-egui`) is the shipped UI** until this one reaches parity, and the two stay in step
(every feature in both, differing only visually). Why Rune, the measurements, the migration order and what is shared in the core:
[the Rune UI](../../docs/areas/rune-ui.md). The root rules hold: never crash, parameters are hostile, test first, the UI reads
`session.model` and changes it only through `Session::execute`, words through `lang::t`, `tf` and `tc` with English fallback.

## A workspace of its own

`crates/ui-rune` is `exclude`d from the root workspace and has its own `Cargo.lock`. Rune needs `[patch.crates-io]` forks of winit,
wgpu-hal, naga, parley_engine, sctk-adwaita and the accesskit crates, and a patch applies to every crate of a workspace, so eframe's
winit and wgpu would be replaced too. So here, inside `crates/ui-rune`:

- `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` (the root's `--workspace` does not reach it); CI is
  `.github/workflows/rune-ui.yml`. The toolchain is pinned in `rust-toolchain.toml` (Rune's own), not `stable`.
- **Bumping Rune** is one revision in five places: `rune` and the eight `[patch.crates-io]` lines of `Cargo.toml`, all the same `rev`.
  Read Rune's root `Cargo.toml` at the new revision for its patch list first, and rebuild `Cargo.lock`. Rune never rewrites `dev`.
- The network rule holds: nothing in the graph is an HTTP client (`cargo tree -i ureq` and `reqwest`, `hyper`, `rustls` find
  nothing), and this crate is no dependency of `bimcraft-core`, the apps or the CLI. The root's manifest test reads this manifest too.
- Rune's `ogg-audio` default feature is left off (it brings MPL-2.0 audio decoders on macOS). Rune's licence (MIT) and Inter (OFL) are in
  the root `NOTICE`, and the OFL text in `assets/fonts/inter/OFL.txt`.

- **Words** go through `lang::t`, `tf` and `tc` (English fallback), over the catalogs the egui crate reads. `cargo i18n` extracts only
  `crates/ui-egui`, `crates/core` and `apps/bimcraft`, so a message first written here is not in `translations/ui/bimcraft-ui.pot`
  and shows in English until the tool learns this folder (`catalog/extract.rs`'s roots); the ones a test reads are already there.

## Testing

- The app is driven headless through Rune's own `Runtime` (`src/ui/tests.rs`): `Runtime::new(App::sample(), view, size, Env::new(..))`,
  `rt.frame(0.0)`, a pointer at window points (`rt.find(key)`, `pointer_down`, `pointer_up`, `pointer_move`, `wheel_event`), then the
  state and the elements read back (`rt.state`, `rt.elements()`). A window is never opened in a test. Ctrl and Shift held for the
  presses that follow: `rt.physical_input(PhysicalInput::Modifiers(..))` (a handler reads them as `tx.mods()`); the middle button:
  `rt.button_down(x, y, PointerButton::Middle)`; the window's keys: `rt.on_key = Some(ui::on_key)` first, then `rt.key(..)`. A press moves
  no pointer (`pointer_down` sets the place and runs no hover), so a test that wants hover moves first. The poll: `rt.poll = Some(ui::poll)`,
  and `rt.frame(dt)` steps the clock by `dt`, so a test of what waits for the camera to be still frames 0.2 s.
- A tool in hand is `app.set_tool("wall")` or the palette's `tool-wall`; the snap is then `app.landing`, and a click (`release` with no travel, or a tap)
  picks `landing.at` into `app.picked`. A test that wants a place far from every wall and grid uses a model point such as (-30000, -30000), where the
  snap is the nearest 10 mm, and one that wants a guide from the plan sets `app.snapper.snaps.guides_from_whole_model = true`.
- What needs no window is tested without one: `app.rs` (state and handlers), `camera.rs`, `paint.rs`, `props.rs`, `status.rs`, `lang.rs`.
- Rune looks at itself: `bimcraft-rune --snap out.png --size 1280x800`, `--tree out.json`, `--script s.json --frames N` render and
  drive it with no display, on whatever Vulkan the machine has (lavapipe with no card): [looking at the result](../../docs/areas/looking-at-results.md).

## Module map

One line per module. A new module adds its line here, not to the root.

- `lib.rs`: the modules, and `run()` (the window). `main.rs`: the `bimcraft-rune` binary. `software.rs`: `--software`, which starts the
  program again with `RUNE_CPU_FIRST=only` so a machine with no card draws on Rune's CPU renderer (it cannot set the variable itself, safely).
- `app.rs`: the state (`App`) and what its handlers do, the one place that meets the core: `render::draw`, `Drawing::pick`,
  `Session::execute` for every change. It holds no widget, so it is tested without a window (`app/tests.rs`). In `CameraMode::Live` (the
  Windows default; `BIMCRAFT_RUNE_CAMERA=live|exact` chooses, and the doc has the measurements behind the default) the plan on screen is the
  last drawing (`drawn`) moved by the camera through `live_offset` and `live_scale`, which the plan binds, and drawn again exactly once the
  camera has been still for `SETTLE` (`settle_if_still`, from `ui::poll`) or something needs the exact drawing (a release, a command).
- `ui/`: the elements, a function of `App`: `mod.rs` the window (laid out by profile) and its keys, `plan.rs` the canvas (the drawing kept as
  one element while it is the same, the pointer and wheel handlers, the primary button's click and box, the middle button's pan, the hover, selection and box layers), `topbar.rs` (Open and Save through
  the system's file dialogs, Undo, Redo, the zoom buttons), `tools.rs` the BIMCraft profile's palette, `ribbon.rs` Rev's, `properties.rs` (the palette: the panel's groups, each folded by its header;
  `properties/editors.rs` a row's editor: text, a field, a check box, a list opened in place, a link; `properties/selector.rs` the Type Selector and Edit Type), `browser.rs` (the levels), `status_bar.rs`.
- `profile.rs`: the two profiles (BIMCraft's palette, Rev's ribbon), one table drawn two ways. `project.rs`: where a project comes from
  (`--sample`, `--highrise`, a file through the core's `open`) and what the arguments ask. `tools.rs`: the table of tools both profiles draw.
- `select.rs`: what a click or a box does to the selection (`How`: replace, add, take away; `Rubber`, the box being dragged, a window
  left to right and crossing right to left), the same rules as egui's canvas.
- `snapping.rs`: where a point picked at the pointer lands (`Snapper::land`: the core's `snap::snap` and `AlignmentGuideService`, the pixel reach,
  the held guide; `Landing::tag`, the words). `paint/snap.rs`: what a pick shows, in one shape (the points picked, the guides, the snap's glyph). The state is
  `App::landing`, `picked` and `snapper`; it is made only with a tool in hand (`App::reland`, from `rehover` and after a command), and the plan's snap layer is
  only there then. What egui's canvas has and this does not is listed in the doc's slice 4.
- `camera.rs`: pan, zoom and Zoom In Region of the plan, in the core's `View` (the history Previous Pan/Zoom walks back is `App`'s). `paint.rs`: a `Drawing`'s primitives as Rune shapes (dashes split,
  lettering through `text::triangles`, what cannot be drawn counted). `props.rs` and `props/`: the Properties palette's logic with no window ([the palette](../../docs/areas/rune-ui-properties.md)): `kind.rs` the editor
  kinds, shown and typed in the project's units; `table.rs` the rows of each category as data; `names.rs` the categories' names; `panel.rs` `Panel::of`, the groups,
  rows and editors a selection has (values shared or blank); `apply.rs` what an edit becomes (`modify`, or `crop`); `types.rs` the Type Selector's list; `view.rs` the plan's own Graphics rows.
  `app/properties.rs`: the palette's slice of `App` (the panel, the text typed, what is folded or open, the handlers a row calls, the seams for dialogs).
  `status.rs`: the status line. `lang.rs`: `t`, `tf`, `tc` over `translations/ui/*.po`, the catalogs egui reads.
