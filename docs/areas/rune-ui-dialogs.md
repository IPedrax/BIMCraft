# The Rune UI: dialogs, command forms and settings

The dialogs half of the port in [the Rune UI](rune-ui.md) (stage 4 and the rest of stage 2), built on `IPedrax-Test/Rune-GUI-dialogs`
beside the plan tools and the other views. egui's dialogs stay the reference for behaviour (`crates/ui-egui`, untouched). The notes for
the crate are in [crates/ui-rune/CLAUDE.md](../../crates/ui-rune/CLAUDE.md).

## Piece 1: the generic command form, and Find

Every command in `engine::commands()` (282 of them) has an id, a Revit-style shortcut, a params doc written for agents and a JSON `run`.
Instead of one dialog written by hand for each, the dialog is made from that table: a command's doc is read as typed fields, what a
person types is read back as the JSON the command takes, and `Session::execute` runs it. The 18,000 lines of egui's manage dialogs are
the later, hand-built forms (Type Properties, Materials, Parameters: pieces 3 and on) for the commands where a generated form is not
enough; the generated one covers all the rest from the first day.

| Module | What it owns | Tests |
|---|---|---|
| `forms.rs` | the schema layer, pure: `Form::of(&Command)` reads the doc's leading `{…}` into `Field`s (`Kind`: number, text, id, ids, point, points, strings, choice, bool, any), `Form::params` reads the typed text back as JSON | 10, one of them a sweep over every command |
| `finder.rs` | Find, pure: the tools, the commands (by label, id, shortcut and menu) and the levels that have every word typed, best label first, 12 at most | 4 |
| `app/dialogs.rs` | the dialogs' state (`Dialogs` on `App`) and handlers: `toggle_find`, `pick_found`, `open_command`, `choose`, `run_form` | 8, no window |
| `ui/dialog.rs`, `ui/find.rs` | the two modal dialogs, as the kit's `Dialog`, `TextInput` and `Select` | 7, Rune's headless `Runtime` (`ui/dialog_tests.rs`) |

**How a doc is read.** The doc is prose that starts with a group: `{path, as_template?}`, `{at: [x,y], angle?: degrees, justify?:
top|centre|bottom}`. Each item is a field; a name with `?` is optional, anything else is required, and `a | b` names alternatives (none
of them required). What follows the colon decides the kind: `[x,y]` or `[x, y, z]` a point, `[[x,y],…]` a list of points, `[id,…]` ids,
`[name,…]` texts, `a|b|c` a choice (a select), `true|false` or `bool` a yes or no (a select too: unset, yes, no), a number a number box
that starts from that default (`storeys?: 20`), `degrees` or `mm` a number. A few names say more when the doc does not (`id`, `ids`,
`at`, `points`, `closed`, `name`, `height`, `angle`…). Everything else is `Kind::Any`: JSON when the text parses as JSON, else the text.
That is also why a name that reads as a number is never turned into one for a `Text` field (`name`, `rename`, `like`…), and why
`level`, `view`, `host` and `type` (a name or an id) stay `Any`.

A reading that cannot be sure gives a less typed field and never fails. A `…` item (`{id | ids: [id,…], …fields}`) or an item that is no
field's name turns on a box for more parameters, a JSON object merged under the typed fields; a doc with no leading group at all is that
box and nothing else. The sweep test checks that every command has a way to give its parameters, that no field name is repeated, that a
form filled with what each kind reads comes out as an object with every field, and that hostile text in every field is an error or a
value and never a panic. The reading of the first 150 docs was checked by eye in the design: the places it is loose are the quoted
alternatives (`name | "Solid"` is `Any`), a choice whose words are prose (`Any`), and a list of lists of lists (`Any`).

**Caps.** A number is finite and at most 1e12; a coordinate at most `MAX_COORD` (1e9); a list at most 10,000 items and a points list
`MAX_POINTS`; a text 10,000 characters; JSON a MiB (and parsed with serde_json's depth limit, so deeply nested text is text, not a
crash); an id 100 characters. A reply or a refusal is cut before it is shown (4,000 and 2,000 characters).

**The dialog.** `Dialog::new` over the kit, `fit` and a scrolling body of rows (the name, with `*` when required, and the control), the
prose of the doc above them, then the error and the reply, then Close and Run. Enter in a text box runs. A refusal, whether the form's
(`elevation is required`) or the core's, is shown inside the dialog and never in the status bar, where it could sit under a modal's
buttons; what the command replied is shown there too, as JSON, and the dialog stays open, so a query's answer can be read and a
command run again. A command that replaces the project (`engine::REPLACES`) starts the view again, as Open does. The form is
millimetres and degrees as the commands are: a project's units (`Units::parse`) are not applied by the generic form, only by the
dialogs that are written by hand.

**Find.** Ctrl+K (or the Find button on the top bar) opens a field and the rows under it; the arrow keys move, Enter or a press takes a
row, Escape or a press outside closes it. A tool is put in hand through `App::set_tool` (and left there when it already is), a command
opens its form, a level shows its plan. While a dialog is open the window's own keys (Delete, Ctrl+Z, Tab) are not read, so a key typed
in a field cannot delete the selection under it. egui's Find also lists the plugins' functions and the family document's own commands:
not here yet. The Revit shortcuts are matched as text (typing `WA` finds Wall) but are not bound to keys: they wait for the shared
registry.

**Words.** The dialogs' own messages are first written in this crate, and `cargo i18n` does not read it, so they show in English until
the tool learns this folder: Find, Close, Run, yes, no, More parameters, Find a tool, a view or a command, Nothing by that name, Tool, View, A
JSON object, like {"key": 1}. The form's errors (`elevation is required`) are English, as the Properties panel's are. A command's label is
the core's own, so it is translated where the core's catalog is.

**Not done.** Command-specific controls (a level picker where the doc says a level, a material picker, a file dialog for a `path`);
the form does not learn a command's parameters from the model (the types a family has, for example); undo and redo are the same
commands as any, not the top bar's buttons; the Find list does not show the keys a command is bound to.

## Piece 2: settings.json, the window's size, and the Settings dialog

| Module | What it owns | Tests |
|---|---|---|
| `settings.rs` | the user's `settings.json`, pure and the file's IO: `Settings` (language, theme, profile, window), read kindly, written atomically, `default_path()` beside the favorites | 4, in a folder of their own |
| `app/settings.rs` | the state (`Prefs` on `App`, the Settings dialog in `Dialogs`) and handlers: `use_settings`, `set_language`, `set_theme`, `set_profile`, `set_units`, the window's size kept once it has held still | 7, no window |
| `ui/settings.rs` | the dialog: four selects (language, units, theme, profile) and a Project Information button | 4, Rune's headless `Runtime` |

**The file.** It is the file the egui app keeps (`settings::beside` the favorites: `favorites::default_path()` with the file name
`settings.json`), and the four keys have the same names and values there (`language`: a catalog code; `theme`: `dark`, `light` or
`system`; `profile`: `bimcraft` or `rev`; `window`: `width`, `height`, `maximized`), so a choice made in either UI shows in both. This UI
reads that file as JSON, sets its four keys and writes the whole of it back, so every key it does not know (shortcuts, the assistant's,
colours: a hundred) is left as it was. The egui app writes its whole struct, so a key only this UI added would be dropped by it, and
this UI adds none. A missing, damaged or over-large (1 MiB) file is the defaults; a key it cannot read keeps what it was; a write goes
through `settings.json.new` and a rename. A test holds each of those. The file is read once, at the window's start (`lib.rs`'s `start`,
`size`); tests never reach the real one (`App::use_settings` takes a path, and the tests give it a folder of their own).

**What each choice does.** Language: `lang::set`, at once and at start; the dialog lists the twelve catalogs in their own words.
Theme: `Dark`, `Light` or `System`, which draws dark because this front end cannot ask the system yet; `tok::theme` and the kit's
palette follow it (`App::dark`), so every panel that takes the theme does, and the plan's own drawing (the core's colours) does not
change. Profile: BIMCraft's palette or Rev's ribbon, unless the arguments asked (`--rev`, `--bimcraft` win over the file). Units: the
*project's* (`Model::units`), so it goes through the core's `units` command and into the project's file, not into `settings.json`; the
core's refusal is said in the dialog. Manage > Project Information is the command `project_info` as the generic form (it has every
field the command takes, none of egui's tabs).

**The window.** The size is read from the file when the window opens (`AppSpec::size`, held to 300 to 16,000 points) and kept again
once it has held still for half a second (the window's root reports its layout: `on_layout`; `ui::poll` writes it). Only the size is
kept, as in egui (Wayland tells an app no place). Not done: maximized is read and written but not applied (`AppSpec` has no option for
it), a window too big for a smaller screen is not brought within it (Rune does not say the monitor's size), and the place.

**Keys.** Ctrl+K is Find and Ctrl+, is Settings (both also buttons on the top bar, and Settings is in Find). The Revit shortcuts wait
for the shared registry.

**Words.** Written first here, so English until the tool learns this folder: Settings (in the catalogs already), Language, Units,
Theme, Profile, Dark, Light, Project Information and Close are in them (egui's), so those show translated; System, BIMCraft, Rev and Dialog
may not be.

## Piece 3: Type Properties

| Module | What it owns | Tests |
|---|---|---|
| `typeprops.rs` | the project's types as a form, pure: `all(&Model)` (every type of `Model::types` with its fields as a file writes them, the runs by their own categories), `of_element`, `fields`, `changes` | 3 |
| `app/types.rs` | the state (`TypeDialog` in `Dialogs`) and handlers: `open_type`, `open_type_of_selection`, `switch_type`, `apply_type` | 3, no window |
| `ui/types.rs` | the dialog: the type's name in a select of its category's types, its scalar fields in boxes, the rest as rows, Apply | 2, in `ui/dialog_tests.rs` |

The Properties panel's Edit Type button (and Find, "Type Properties") opens the dialog on the type of the selected element. There is
no per-category code: `Model::types` is written as JSON (the same JSON a project file holds), so a category's types are whatever the
core has (walls, floors, doors, windows, columns, roofs, ceilings, railings, rooms, the runs, curtain panels, framing types…), a type's
fields are its JSON object, and the scalars among them (numbers, text, yes or no: `props::editable`) are typed; layers and the other
structures are shown as rows and left to the commands made for them. Apply sends one `type {category, name, …}` with only the fields
that differ (numbers compared by value, so `200` typed is the `200.0` the type has), which "creates the type, or changes it everywhere it's used";
the dialog then shows the type as the project now holds it. A field that is no number, or a refusal of the core (a type's field the
`type` command does not take under that name), is said under the rows. A type is not renamed, duplicated or deleted here: those are
`rename_type`, `type` with `name` and `delete_type`, which Find opens as the generic form.

**Not done in this piece.** Materials and Parameters (the generic form carries `material` and `parameter` for now, and Find opens it); a
layer editor for compound types; a picker for a field that names another type or a material; units in the boxes (millimetres, as the
type is held); the Type Properties of a family's types.

## Where it stands, and what piece 4 needs

Pieces 1 to 3 are on `IPedrax-Test/Rune-GUI-dialogs` with the Rune UI checks green on Linux, Windows and macOS (run 45652167699 at
938f8e7ef: fmt, clippy, 130 or so tests, the layout guard, and the two no-card measure jobs). The first run of piece 1 alone (45638982950)
had 4 failing UI tests: a dialog opens on a spring, so a press at once finds it still opening, and `rt.frame(0.0)` does not move the clock;
the dialog tests settle the clock before each press (as the kit's own dialog tests do).

**Piece 4, docking and tabs (`SplitView`, `TabStrip`), is not started, on purpose.** `SplitView` is an element that holds the sidebar,
the middle and the inspector, so using it puts a wrapper around the plan canvas, which is the one thing `rune-ui.md` records as having
doubled Linux's wheel cost (the wrapper alone, doing nothing, with `Exact` camera). It needs a measurement run on both machines before
it lands, and it edits the shared window layout in `ui/mod.rs` that the tools and views branches also change. A smaller step that
carries no such risk is `TabStrip` for the right-hand column (Properties and Levels as tabs), which does not wrap the plan.

**Run the Rune UI checks for this branch** with `gh workflow run rune-ui.yml --ref IPedrax-Test/Rune-GUI-dialogs`: the workflow's push
trigger names only `main` and `IPedrax-Test/Rune-GUI`, so a push starts the main CI alone. A second dispatch on the same ref cancels
the first (the concurrency group is per ref).
