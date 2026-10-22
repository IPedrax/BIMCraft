# How to look at the result of a change

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

- Desktop: `cargo run -p bimcraft -- --sample` (it needs a display; egui 0.36 needs Rust 1.95). After a UI change, look at a
  screenshot of the window, in both themes (View > Dark Mode). Without a display, egui_kittest's `wgpu` feature renders a `Harness` to an image; with no GPU,
  `WGPU_BACKEND=gl` falls back to Mesa's software renderer.
- The 3D view on the graphics card: `cargo test -p bimcraft-ui-egui --release gpu_speed -- --ignored --nocapture` times the tower's
  frame on the card and on the processor and the first pick, and `the_card_draws_what_the_processor_does` compares the two pictures (set
  `DUMP` to a folder for both as raw RGBA and the pixels that differ). To look at the window with the card's picture in it, render a
  Harness as above, with `Gpu::new(&state)` given the same fresh `RenderState` the Harness's `WgpuTestRenderer` was made from, and
  `app.card` set to `wanted`; look in both themes, and at File > Settings > Performance.
- Updates: the banner, the File menu's items and the status notes are drawn by tests that give a `BimApp` an `Updater` on a
  `testing::Fake` network, made `.inline()` (its work on the calling thread) and `.target(Some(Target::Windows))` (a copy that can
  update itself), with `.handing_over(..)` standing in for the install; `tests::updates` in `crates/ui-egui`. The Installing box
  has a spinner, which asks for frame after frame, so a test that sees it uses real threads, a hand-over that waits at a gate, and
  `step`, never `run`. Look at the banner and the box in both themes by rendering a Harness (above): the offer, the Download
  variant, the progress (it needs a real thread and a network that blocks), the ready state, the Installing box, and the File menu. A live check of the
  real network code is a throwaway program in a scratch folder that calls `net::Ureq` and `check` against api.github.com; run it
  after touching `net.rs`, since a fake can't show what ureq or GitHub do (ureq's body limit once failed a real 8.9 MB download at
  its last byte, which only a real or a loopback server shows).
- Plans: `bimcraft-cli run --sample --save /tmp/plan.png`, then read the PNG. After any drawing change, look at it. A level's
  reflected ceiling plan: `render {"view": "Level 1", "ceiling": true, "path": "/tmp/rcp.png"}`.
- 3D: `bimcraft-cli run --sample --cmd 'render {"view": "{3D}", "path": "/tmp/3d.png"}'` (`azimuth` and `elevation`
  turn it). After touching `view3d.rs`, look at it. Sections and elevations render by name the same way: `"view": "Section 1"`.
- Scale, detail level and style: `bimcraft-cli run --sample --cmd 'scale {"view": "Level 1", "scale": 100}' --cmd 'render {"view": "Level 1",
  "path": "/tmp/plan.png"}'` (annotation follows the scale at that picture's zoom, so the one that fits is not the one a sheet prints; a fit leaves room for it, `fit_by`,
  which finds the largest scale whose view and annotation both fit: render at 1 : 20 and 1 : 500 and look for what's cut at the edge),
  `detail {"view": "Level 1", "detail": "coarse"}`, and `visual_style {"style": "wireframe"}` then `render {"view": "{3D}", ...}`. After
  touching `Drawing::grow`, a layer or opening symbol's detail, or `Scene::wireframe`, look at them. In the desktop, the bar's menus in both themes.
- Schedules: `bimcraft-cli run --sample --cmd 'schedule {"category": "rooms"}'` returns its columns, rows and totals (and
  `available`, the fields there are); one placed on a sheet prints as a table: render the sheet and look at it.
- Sheets: `bimcraft-cli run --sample --save /tmp/sheets.pdf` prints every sheet; read them back with poppler (`pdfinfo`,
  `pdftoppm -png`) when it's installed. `render {"view": "A101"}` draws one sheet as a PNG.
- IFC: `bimcraft-cli run --sample --save /tmp/house.ifc`. After touching `ifc.rs`, validate with IfcOpenShell:
  `ifcopenshell.validate.validate(f, logger, express_rules=True)` (needs pytest installed), then build every product's shape
  with `ifcopenshell.geom.create_shape` and compare wall and roof volumes with a hand calculation. `python ci/validate_ifc.py x.ifc`
  does the first two, as CI's IFC job does on the sample and on `ci/ifc-coverage.txt` (add a line there for a new kind of element).
- Structure: after touching `runs.rs` or how a run, a footing or a structural column goes to IFC, export beams (level, sloped, round), footings, a strip
  footing, columns and a structural floor, validate with IfcOpenShell (`express_rules=True`), and compare each product's volume and bounding box
  with a hand calculation (a round section is a polygon in IfcOpenShell's tessellation: a fraction of a percent short). Look at the plan, a
  section across a beam, and the 3D view in both themes.
- Discipline tabs (Plumbing, Electrical, SPDA, Data): after touching `fixtures.rs`, `runs::IFC`, `Category::ifc` or the run arm of `ifc.rs`, script
  one of every run category (sloped, upright, each justification, in a system) and one of every starter family, `--save` the IFC, validate it with
  IfcOpenShell (`express_rules=True`: 0 statements), build every product's shape and compare the runs' volumes and boxes with a hand calculation,
  check the `IfcSystem`s with the runs assigned to each and the proxies' object types, then read it back (`run x.ifc --cmd 'elements {"category": "pipe"}'`)
  as written, with its bodies turned into triangle meshes by IfcOpenShell, and with BIMCraft's `BIMCraft_Run` property sets taken out. Look at each
  tab, a run tool's preview, a fixture tool with its Type Selector open, Edit Type, a section across a pipe and a tray, and the 3D view with the
  walls hidden (`graphics {"view": "{3D}", "hide": ["wall"]}`), in both themes.
- IFC import: after touching `import.rs` or `step.rs`, open the sample's IFC (`run house.ifc --cmd 'elements {}'`) and
  compare it with the sample, then the same file with its bodies turned into triangle meshes by IfcOpenShell, and a
  building written with IfcOpenShell's API. Render what came in and look at it. After touching how phases and design options
  cross (`ifc::PHASING`), do it with a phased sample too: `run --sample --script phased.txt --save p.ifc`, `convert p.ifc
  p.bimcraft`, and compare `phases`, `phase_filters`, `phase_overrides`, `option_sets` and each element's `params` with the project's.
