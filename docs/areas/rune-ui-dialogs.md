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
