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
  bottom actions. Observe the returned `should_close()` so the corner close button and Escape follow Cancel/Close. A dialog is as tall as
  what it holds (at least its `min_height`, at most the window less a margin, the body scrolling past that), measured each
  frame, so it shrinks when its content does: never size it from what its area took last frame.
- **A palette or a window** that docks or floats goes through `dock.rs`; settings the user keeps go in `settings.rs`
  (`settings.json`), never in a project.
- **The 3D view has two painters**: the core's software picture and the card's (`gpu.rs`, `gpu.wgsl`; the design is in
  [views and graphics](../../docs/areas/views-and-graphics.md)). `orbit.rs` picks one for each picture and picks through `Picker`, so a
  change to what the 3D view shows or how it picks goes through both. A test gives the app a card with `gpu::headless()`.

## Testing

- `app(script)` builds the window in Rev, the ribbon's profile, with Revit's palette layout: the tests of what the tools do were written
  round the ribbon. `bimcraft(script)` builds it in BIMCraft's own (the Tools palette, no ribbon) for what BIMCraft's chrome does.
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
- After a change to how the window looks, look at it in the three looks (Light, Dark, Dark UI with a Light Canvas) and in both profiles
  (BIMCraft and Rev, `style::Profile`): [looking at the result](../../docs/areas/looking-at-results.md).

## The 2D canvas on the card

A plan's triangles are egui's own, made once from `canvas::paint_to`'s shapes and kept on the card (`retained.rs`), so a frame that
changes nothing is a draw call. A shape's look changes in `canvas::paint_prim`, for both paths. A new thing that decides what a plan
looks like goes in `retained::Look`, or the canvas shows it late. The window's renderer's options must stay what `retained::Pipeline`
assumes: no multisampling, no depth buffer, dithering on. The tests compare pixels with egui's renderer on a real card; a harness has no
card, so it stays on `canvas::paint`. While a view moves, the frame shows the last drawing moved (`canvas::Shown`, which also picks through
it, so use its `pick`s and `place`, not the view's pixels, for anything read from the drawing) and `PlanCache::moving` asks a worker for the
exact one; an edit or another level is made in the frame. Plan and numbers: [canvas acceleration](../../docs/canvas-acceleration-plan.md).

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

## Module map

One line per module: what it owns. Read the module's file and, for the areas it names, the write-up in `docs/areas/`.

- `lib.rs`: the app: ribbon, menus, tools, tabs, files. `canvas.rs`: the plan canvas (picks, tools, grips); `retained.rs`: its triangles kept on the card. `orbit.rs`: the 3D view; `gpu.rs` and `gpu.wgsl`: drawing it on the graphics card. `section.rs`, `paper.rs`: sections and sheets; `section_worker.rs`, `view_worker.rs`: views made off the interface's thread. `properties.rs`: the Properties palette. `browser.rs`: the Project Browser. `dock.rs`: docking palettes and floating views. `settings.rs`: File > Settings. `lineweights.rs`, `objectstyles.rs`, `linestyles.rs`, `fillpatterns.rs`: Manage > Line Weights, Object Styles, Line Styles and Fill Patterns. `sketch.rs`: sketch mode (Edit Boundary, Finish and Cancel); `sketch3d.rs`: the sketch in the 3D view, on its element's plane. `standards.rs`: Manage > Technical Standards. `shape.rs`: Shape Editing's actions, from a plan or the 3D view. `electrical.rs`: Manage > Electrical Settings. `notes.rs`: text notes written on the canvas, and Text Type Properties. `dimtypes.rs`: Dimension Type Properties. `typeprops.rs`: the look every Type Properties dialog shares. `draw.rs`: the Draw panel and its Options Bar. `groups.rs`: Create Group, Place Detail Group, Repeating Detail, Ungroup. `start.rs`: the start page. `context.rs`: the right-click menu. `find.rs`: Find (Ctrl+K), every tool and view by what is typed. `presets.rs`: the project's scale, the tools left out (Edit Tools) and named presets of the layout. `dialog.rs`: the file dialogs, on a thread of their own. `popup.rs`: movable dialog frames, scrolling forms and fixed bottom actions. `window.rs`: the window's size, kept and fitted to its screen. `control.rs`: the control port, agents working on the window's project.
- `background_grid.rs`: the view/template grid editor and canvas layer.
- `minimap.rs`: the Navigator's minimap. `display.rs`: Graphic Display Options (GD), the 3D view's edges and ambient shadows.
- `graphics.rs`, `templates.rs`, `footer.rs`, `phasing.rs`: Visibility/Graphics, View Templates (and Assign View Template), the view control bar, Phasing. `filter.rs`: Filter on the selection. `inquiry.rs`, `recent.rs`, `autosave.rs`, `updates.rs`, `revit.rs`: Manage > Inquiry, Open Recent, autosave and recovery, the update banner, Revit files.
- `assistant.rs`: the Assistant palette (AI) and its Settings page, the provider, model and key ([the assistant](../../docs/areas/assistant.md)).
- `links.rs`: Link Model. `clouds.rs`: Insert > Point Cloud. `cad.rs`: Insert > Import CAD and Link CAD. `section_box.rs`: the 3D view's section box, its Properties and grips, and Linked 3D View. `triad.rs`: the 3D view's navigation triad. `spacemouse.rs`: the 3D mouse, read on a thread of its own, moving the active 3D view. `camera.rs`: a 3D view's camera in Properties, bound to the view the 3D tab shows or one picked on a plan (perspective, lens, eye, and a camera view's name, eye and target). `structure.rs`, `mep.rs`: the Structure tab and the discipline tabs. `reinforce.rs`: the Reinforce dialog. `editor.rs`, `editor/document.rs`, `editor/association.rs`, `editor/extrusion.rs`, `editor/exists.rs`, `editor/types_dialog.rs`, `editor/temp_hide.rs`, `editor/views.rs`, `editor/options.rs`, `editor/sketch.rs`, `editor/dimensions.rs`, `favorites.rs`: family drafts, the family document every family opens as, parameter associations, a selected form's Properties, Family Already Exists, Family Types, the document's Temporary Hide/Isolate, its plans and elevations, the family's options (Family Category and Parameters, Properties' Other), sketch mode, their dimensions and Align, and Favorites; `adaptive_*.rs`: adaptive families. `style.rs`: look and feel, the BIMCraft and Rev profiles, light and dark, theme presets and the user's accent, floating cards or compact. `ribbon.rs`: the ribbon's panels, their lists, and folding them to fit. `titlebar.rs`: the window's own title bar and frame. `icons.rs`: the icons, drawn in code (`--example icon_catalog` exports them). `lang.rs`: language selection and the interface PO catalogs; `lang/font.rs`: Chinese and Korean font support. `tests.rs`, `tests/`: the app driven as a user does.
