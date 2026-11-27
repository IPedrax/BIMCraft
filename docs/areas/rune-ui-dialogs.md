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

## Round 2: pickers, and the library managers

Fast-forwarded onto `IPedrax-Test/Rune-GUI` (be7bfa66f, with tools, views and properties merged), then the document-settings dialogs. egui
writes each of Materials, Visibility/Graphics, Parameters, Phasing and the styles as a dialog of its own; here one pattern carries them,
because each is a table of the project and a command of the core, and the generic form already runs the commands.

| Module | What it owns | Tests |
|---|---|---|
| `forms.rs` (`Kind::Pick`, `Class`, `options`) | a field the doc leaves untyped that is named `level`, `top_level`, `base_level`, `material`, `*_material`, `type`, `family`, `phase` or `line_style` is picked from the project's own names (a type is of the command's own category: `wall` offers the wall types) | 1, and the sweep |
| `app/dialogs.rs` (`FormState::options`) | the lists, read when the form opens; a picker with nothing to offer is `Kind::Any` again, so it is typed | 2, no window |
| `library.rs` | the managers, pure: `Source` (Materials, Line Styles, Line Patterns, Fill Patterns, Phases, Phase Filters, View Filters, View Templates, Text Types, Project Parameters, Object Styles), each a table of `Model` as JSON (an entry a file writes as a list, a line pattern's dashes, is `{"value": [..]}`) and one command; `changes`, `deletion` | 3 |
| `app/library.rs`, `ui/library.rs` | the dialog: an entry chosen from a list, its fields that can be typed, a new name, Apply, Delete, New | 5, no window; 2 in `ui/dialog_tests.rs` |

**A manager.** The entries are `serde_json::to_value` of the model's own table (the JSON a file holds). The fields that can be typed are the
entry's scalars that the command's params doc names (`library::editable`: the intersection, so a field the command would refuse is shown
and never offered), and Apply sends the command `{name, field…}` with just the fields that differ (`typeprops::differing`, numbers compared
by value), plus `rename` when a new name is typed and the command has one. Delete is `{name, delete: true}`, offered only where the command
has `delete`; New opens the command's own generic form. Object Styles is the one that is not `{name, …}`: its command takes
`{categories: {name: {projection, cut}}}`, so it has its own `Shape`, and an object style is not renamed or deleted (Reset is the command's
`reset`). The whole dialog is the Type Properties pattern, so a refusal or a field that is no number is said under the rows.

**Where each of the five is.** Materials: the manager (its colour, cut and surface patterns that are plain fields; textures and the
patterns' own settings stay rows). Visibility/Graphics: View Filters and View Templates are managers; the `graphics` command (hide and
show by category, element overrides, a view's own) is the generic form, found as Visibility/Graphics, as is `section_box` and
`graphic_display_options`. Parameters: Project Parameters is a manager; shared parameters, dictionaries, import, export and the batch
commands are generic forms. Phasing: Phases and Phase Filters are managers; design options, renovation and the overrides are generic
forms. Styles: Object Styles, Line Styles, Line Patterns and Fill Patterns are managers; Line Weights is the generic form (`line_weights`
takes lists of widths), as are Halftone and the others.

**Entry points.** The Manage button on the top bar opens Find on the word, which lists the managers first (they are also found by
name: Materials, Phases…) and then the commands the menu puts under Manage; every command stays reachable through Find as a form.

**Not done.** The tables egui draws (a Visibility/Graphics grid of categories by projection, cut, halftone and detail; a materials browser
with a swatch and an asset library; a pattern's drawn preview); colour and pen pickers (a colour is typed as a list of numbers or JSON);
pickers inside a manager's fields (a line style's `pattern` is a text box, not a list of the project's line patterns); nested entries (a
filter's rules, a pattern's lines, a material's textures) are rows, edited through the command's form; the shortcuts. New messages first
written here: Manage, Entry, Nothing here yet: use New, New name, A new name for it, New, There is no entry to change: use New, This kind
of entry is not deleted here.

## Round 3: the project-level dialogs

Coordinates, Classification, Title Blocks, the Publisher and the Start dialog. Four of the five are more managers (`library.rs`, now fifteen),
because each is a table of the project and one command of the core; egui writes each as a window of its own, with a canvas, tabs or a map.
Find lists them under the word Manage, and by name.

| Dialog | In egui | Here |
|---|---|---|
| Coordinates (Location, LO) | `coordinates.rs`: five tabs, a dial, a world map, a map-system search | `Source::Locations`: the project's sites, each with its shared east, north, elevation, angle, scale and map system (`crs`), changed through `location`; **Make active** (`activate`); the forms of `coordinates` (the Project Base Point and Survey Point, reset, acquire), `specify_coordinates` (CP), `rotate_project_north` (TN), `georeference` (its `search` finds a map system) and `convert_coordinates` as buttons (`Source::also`). PC is `spot_coordinate`, a tool command, found by Find. |
| Classification | `classification.rs`: setup, by hand and agent tabs | `Source::Classification`: the loaded tables (system, key, edition, source file, rows), taken out with `remove`; **New** is Load Classification Table (the file's path typed); the forms of `classification_systems`, `classification_parameters`, `classify` (`assign` as JSON), `classification_check`, `classification_brief` and `classification_answer` as buttons. |
| Title Blocks | `titleblocks.rs`: a preview and tabs of fields, texts, lines, size, logos | `Source::TitleBlocks`: the blocks, with `width`, `height` and `font` typed, rename, delete; New is `title_block`'s form (`starter`, `like`, `path` for a DXF, and `fields`, `texts`, `lines` as JSON); `title_values` (the project's values) as a button. |
| Publisher | `publisher.rs`: sets, folders, print sets, Publish with a bar | `Source::PublishSets`: the sets, with `folder` and `overwrite` typed, rename, delete; the folders are rows; the forms of `publish`, `print_set` and `export_formats` as buttons. |
| Start (and templates) | `start.rs`, `project_templates.rs` | `app/start.rs`, `ui/start.rs`, `recent.rs`: New (`new`), Open (the system's dialog), the sample house, the recent files and the templates; the dialog is opened by Find (Start). View Templates, egui's `templates.rs`, is the View Templates manager of round 2. |

**`library.rs` additions.** `Source::also` names the other commands a manager's dialog opens the forms of, as buttons under the rows (`lib-also-{i}`).
`library::deletes` and `deletion` take `remove` where a command has it instead of `delete` (a classification table). A table whose entries a
file writes as a list is `{"value": [..]}`, so every entry is an object. The Classification entries are never typed field by field (a table
is loaded again from its file), and **Make active** shows where the command has `activate` and the entry is not the active one.

**Start's lists.** `recent.json` (the projects opened or saved last, newest first, ten kept) and `templates.json` (the first ten are listed)
are the files egui keeps beside `settings.json`, `{"files": [path, …]}`, so the two apps share them. A file that is missing, over 256 KiB or
damaged is an empty list; an entry that is no path is left out; a write is whole or not at all. `App::open_path` and `save_as` put a file first
(a `.bimt` opened as a template has no file, so it is not noted). Templates are read here and managed in egui's Settings.

**Not done.** The Coordinates window's dial, world map and map-system picker (a system is typed as an EPSG code, searched with `georeference`'s
`search`), the by-hand code search and the AI agent of Classification (it needs the assistant crate, the network rule), the Title Blocks
preview and its fields, texts and lines tabs (JSON in the form), the Publisher's folders tree and its progress bar with Cancel (`publish`
runs in one command), the Start page's tiles with a picture of each project, the update line and the source link, and managing the templates'
list. The Start dialog is not shown at startup: the program still opens on its project. New messages first written here: Related, Make
active, Start, Start a project or open one, Recent, Templates, Sample house.

