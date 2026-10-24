# bimcraft-ui-egui

The desktop front end, in egui: a thin shell over the core's `Session`. It reads `session.model` and changes it only through
`Session::execute` (a root rule); what any front end needs (what a plan shows, what is under the cursor) lives in the core. The
map of the modules is in the root `CLAUDE.md`; the write-up of how the window works (docking, settings, picking, snapping, clicks,
startup, tags) is [the desktop](../../docs/areas/desktop.md).

## Adding to the window

- **A tool or a button runs a command.** A new one is a command first (core), then a ribbon button or a tool that passes its
  parameters; its Revit shortcut goes in `SHORTCUTS` (`lib.rs`) and so in Keyboard Shortcuts (KS).
- **Every label has a row in `WORDS`** (`lang.rs`) with its Portuguese and its Spanish, and should have one in `lang/zh.rs` (Chinese, English key
  and the Chinese: a row it lacks shows in English); `nothing_a_translated_interface_shows_is_left_in_english` fails for a label the
  table knows that the window forgot to translate, in all three. The core's messages stay English, because agents read them.
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
- After a change to how the window looks, look at it in both themes: [looking at the result](../../docs/areas/looking-at-results.md).
