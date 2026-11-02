# bimcraft-ui-egui

The desktop front end, in egui: a thin shell over the core's `Session`. It reads `session.model` and changes it only through
`Session::execute` (a root rule); what any front end needs (what a plan shows, what is under the cursor) lives in the core. The
map of the modules is in the root `CLAUDE.md`; the write-up of how the window works (docking, settings, picking, snapping, clicks,
startup, tags) is [the desktop](../../docs/areas/desktop.md).

## Adding to the window

- **A tool or a button runs a command.** A new one is a command first (core), then a ribbon button or a tool that passes its
  parameters; its Revit shortcut goes in `SHORTCUTS` (`lib.rs`) and so in Keyboard Shortcuts (KS).
- **Labels use English source messages**, translated through `lang::t`, `lang::tf` or `lang::tc`. Mark indirectly selected labels
  with `bimcraft_i18n::msg!`; run `cargo i18n update` after changing messages. Translations live in `translations/ui/*.po`;
  unfinished entries fall back to English. See [translations](../../docs/areas/translations.md).
  `nothing_a_translated_interface_shows_is_left_in_english` catches known translations the window forgot to use. Core diagnostics stay English.
- **A ribbon tool that drops choices down** goes through `ribbon::choices(ui, &response, open, content)`, not `Popup::menu`: the
  tool may be a row in its panel's list, where a popup of its own would close the list (egui keeps one popup open).
- **An application dialog** uses `popup::Modal` with a title, `popup::body` for the scrolling form, and `popup::footer` for its
  bottom actions. Observe the returned `should_close()` so the corner close button and Escape follow Cancel/Close.
- **A palette or a window** that docks or floats goes through `dock.rs`; settings the user keeps go in `settings.rs`
  (`settings.json`), never in a project.
- **The 3D view has two painters**: the core's software picture and the card's (`gpu.rs`, `gpu.wgsl`; the design is in
  [views and graphics](../../docs/areas/views-and-graphics.md)). `orbit.rs` picks one for each picture and picks through `Picker`, so a
  change to what the 3D view shows or how it picks goes through both. A test gives the app a card with `gpu::headless()`.

## Testing

- The app is driven as a user drives it (egui_kittest): `app(script)` builds one on a script's model, then `click(&mut h, P2(x, y))` at
  a model point, `press(&mut h, "Label")` for a button, `shortcut(&mut h, "EH")`, `key`, `type_into(&mut h, "Label:", "text")`, `pick`
  for a check box or radio button, and `h.state()` to read the app. New behaviour gets a test in `tests.rs` or, for an area, in `tests/<area>.rs`.
- Filter while you work: `cargo test -p bimcraft-ui-egui <part of a name>` (`dock`, `browser`, `autosave`, `unhide`, `tag_by_category`).
  The whole crate runs in about 35 seconds.
- Traps: `press("X")` panics when two nodes carry the label (a ribbon tab and a button; a palette and a window): check
  `h.state()` or set the state; a status box in the view's corner can lie over a modal's buttons, so a modal says its errors
  inside itself; a field inside a `Grid` comes out narrow, so use `ui.add_sized`.
- A texture's pixels (the 3D view's picture, a section's) are not in the context, and the harness hands each frame's uploads to a renderer
  that keeps none: `tests/light_canvas.rs`'s `Uploads` is one that does (`Harness::builder().renderer(..)`).
- After a change to how the window looks, look at it in the three looks (Light, Dark, Dark UI with a Light Canvas): [looking at the result](../../docs/areas/looking-at-results.md).

## Background view work

Sections/elevations use `view_worker.rs` (bounded latest-request mailboxes) and `section_worker.rs` (snapshot-bound
geometry, pictures and overlays). The UI installs each picture and its picking/display list together, and reprojects
that completed generation during navigation. Never fall back to full geometry work on the UI thread when a job is late.
Keep selected-level grip endpoints in the worker result too; `render::level_ends` builds whole-model work.
The worker prepares with `SectionWork::new_deferred_cancellable` and fits with `section_fit_cancellable`; a job stopped part
way publishes nothing, and what the renderer keeps of it is whole.

Ordinary UI harnesses use inline section execution for deterministic tool tests. `section::worker_tests`,
`section_worker::tests` and `view_worker::tests` exercise actual threaded publication, cancellation and navigation picking.
New worker lifecycle behavior needs a real-worker test, not only an inline harness assertion.
