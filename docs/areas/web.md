# The browser build

BIMCraft runs in a browser: the same window, built for `wasm32-unknown-unknown` and drawn on a page's canvas, so nobody has to
download anything. The page is `apps/bimcraft-web`, made by [Trunk](https://trunkrs.dev) from its `index.html`; it is a static
site (a `.wasm`, its JavaScript glue and the page), served from GitHub Pages by `.github/workflows/web.yml`.

## Building and looking at it

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk            # or: brew install trunk
cd apps/bimcraft-web
trunk serve --release          # http://127.0.0.1:8080/, and http://127.0.0.1:8080/#sample for the sample house
```

`trunk build --release` writes the site to `apps/bimcraft-web/dist`. A Pages site lives under the repository's name, so the
workflow passes `--public-url /<repository>/`. `.cargo/config.toml` gives the wasm target `--cfg getrandom_backend="wasm_js"`:
getrandom takes its random numbers (the GUIDs) from the page only when told to, and the core turns on its `wasm_js` feature for
that target.

To look at a change, serve the page and drive it in a headless browser (`agent-browser`, or any that can click at a point and take
a screenshot): open it with `#sample`, double-click `{3D}` in the Project Browser, save. A headless browser cancels the file chooser
the moment it opens, so to try Open, make the page's `HTMLInputElement.prototype.click` do nothing first, click Open, and upload to
`#rfd-input`: the chooser takes the file once its input changes.

## What is different in a browser

- **Files** (`bimcraft_core::disk`). A page has no file system, so every file a command reads or writes goes through `disk`: the
  computer's files on a computer, and in a browser files held in the page's memory (`disk::Memory`, at most `MAX_HELD` in all).
  Commands still name files by path. Opening goes through rfd's chooser: the picked file is read into the page under its name
  (`/house.ifc`, `disk::put`) and the window gets that path, as it would a path on a computer (`ui-egui/src/web/dialog.rs`). Saving
  asks nothing: the path is the name the window offers, the command writes it into the page, and once a frame each file commands
  wrote (`disk::written`) is taken out and handed to the browser as a download. A project saved once is saved again to the same
  name with Ctrl+S, and downloads again. A new command that reads or writes a file goes through `disk`, never `std::fs`, or it
  works on a computer only.
- **No updater.** Only the desktop phones home: `bimcraft-update` is not a dependency on wasm, `BimApp::updates` is an
  `Option<Infallible>` there, and `ui-egui/src/web/updates.rs` gives the window the updater's names, doing nothing. The page's
  manifest names neither the updater nor the Revit reader (a test reads it).
- **No Revit files.** `bimcraft-rvt` is left out (it is Apache-2.0 only, and reads from a file system), so Open offers BIMCraft
  projects and IFC.
- **The clock** is `web_time`'s, which is std's on a computer: std's `Instant::now` and `SystemTime::now` panic in a browser, and
  so does `std::process::id` (`ids::seed` leaves it out there).
- **Threads.** A browser page has one: `std::thread::Builder::spawn` returns an error, which the start page's pictures, the Revit
  reader and autosave already treat as "not this time". rayon runs its work on the calling thread. Nothing calls
  `std::thread::spawn` (which panics) on wasm: the desktop's `dialog.rs` does, and is not compiled there.
- **Panics** abort a wasm module rather than unwind, so `Session::execute`'s `catch_unwind` can't roll a command back from one in
  a browser; a command that returns an error still rolls back. The rules against `unwrap` and `panic!` matter all the more.
- **Not in the page yet:** autosave and recovery, recent files, the settings file and favorites (none of them has anywhere to live
  without a file system; the browser's storage would be the place), the control port for agents, and point clouds big enough to
  need threads to read quickly.

## Checks

CI's `Web (wasm32)` job runs clippy on the core, the window and the page for `wasm32-unknown-unknown`, where the code behind
`cfg(target_arch = "wasm32")` compiles at all, and builds the page with a pinned Trunk. The native tests cover `disk::Memory`,
the page's start (`bimcraft-web`'s tests) and everything else as before.
