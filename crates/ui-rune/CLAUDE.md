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

## Testing

- The app is driven headless through Rune's own `Runtime` (`src/ui/tests.rs`): `Runtime::new(App::sample(), view, size, Env::new(..))`,
  `rt.frame(0.0)`, a pointer at window points (`rt.find(key)`, `pointer_down`, `pointer_up`, `pointer_move`, `wheel_event`), then the
  state and the elements read back (`rt.state`, `rt.elements()`). A window is never opened in a test.
- What needs no window is tested without one: `app.rs` (state and handlers), `camera.rs`, `paint.rs`, `props.rs`, `status.rs`, `lang.rs`.
- Rune looks at itself: `bimcraft-rune --snap out.png --size 1280x800`, `--tree out.json`, `--script s.json --frames N` render and
  drive it with no display, on whatever Vulkan the machine has (lavapipe with no card): [looking at the result](../../docs/areas/looking-at-results.md).

## Module map

One line per module. A new module adds its line here, not to the root.

- `lib.rs`: the modules, and `run()` (the window). `main.rs`: the `bimcraft-rune` binary.
- `app.rs`: the state (`App`) and what its handlers do, the one place that meets the core: `render::draw`, `Drawing::pick`,
  `Session::execute` for every change. It holds no widget, so it is tested without a window.
- `ui/`: the elements, a function of `App`: `mod.rs` the window and its keys, `plan.rs` the canvas (the drawing kept as one element
  while it is the same, the pointer, wheel and drag handlers, the hover and selection layers), `properties.rs`, `status_bar.rs`.
- `camera.rs`: pan and zoom of the plan, in the core's `View`. `paint.rs`: a `Drawing`'s primitives as Rune shapes (dashes split,
  lettering through `text::triangles`, what cannot be drawn counted). `props.rs`: the Properties rows from `element_json`.
  `status.rs`: the status line. `lang.rs`: `t`, `tf`, `tc` over `translations/ui/*.po`, the catalogs egui reads.
