# Keyboard shortcuts

Two kinds of bindings live side by side:

- **Two-character sequences**, as in Revit (`WA`, `DR`, `KF`). Their list is `SHORTCUTS` in `lib.rs`; Manage > Shortcuts
  (`shortcuts_window` in `shortcuts.rs`) adds, changes and loads the user's own. The user's bindings live in
  `settings.json` (the `shortcuts` field: a two-character key to the English name of an action) and are read into
  `BimApp::custom_shortcuts` at startup (`use_settings`). That dialog cannot assign modifier chords (`Ctrl+…`, `F…`) yet:
  they will come with ТЗ-008.
- **Hard-coded chords** for the window's frequent commands: files, undo, find, the start page and the alignment guides.
  They are checked in `BimApp::keys` (`lib.rs`) and are not configurable.

## Changes from ТЗ-009

ТЗ-009 freed two chords and left one function key for the guides.

| Action | Was | Is |
|---|---|---|
| Home / start page | `Ctrl+D` | `Ctrl+U` (`⌘U` on macOS) |
| `Ctrl+D` | Home | free; it has no default action |
| Alignment guides | `F8` and `F11` | only `F8` |
| `F11` | alignment guides | free; the system may use it for full screen |
| `Ctrl+U` in the note editor | Underline | Underline (unchanged) |
| `Ctrl+Г` on the Russian layout | — | Home: `egui` reports the physical `U` key |

Why: Home is a rare action and `Ctrl+U` is enough for it, while the one-hand-friendly `Ctrl+D` is freed for the user's
commands. `F11` often belongs to the window manager, so the application no longer consumes it.

## Context and priority

- A modal dialog takes the keys first: inside it `Ctrl+U` and `F8` are not global.
- In a text field the keys belong to the field: `Ctrl+U` does not open Home.
- While a note is edited with Override Formatting, `Ctrl+U` stays Underline (`notes.rs`) and Home does not open.
- On an ordinary canvas `Ctrl+U` opens the start page, `Ctrl+D` does nothing by default, and `F8` turns the alignment
  guides on and off.
- `AG` and `OG` are Revit-style legacy sequences; they still toggle the guides. "Only `F8`" applies to function keys.

## Tooltips and translations

Home on the Quick Access Toolbar says `Home (Ctrl+U)` (`Home (⌘U)` on macOS), and Back on the start page says
`Back to the project (Ctrl+U)`. Both are built with `tf`; their templates live in
`translations/ui/bimcraft-ui.pot`. Home's tooltip shows `Ctrl+U` because on the Russian layout the physical `U` key is
labelled `Г`: a user with that layout presses `Ctrl+Г`.
