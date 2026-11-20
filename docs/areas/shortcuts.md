# BIMCraft keyboard shortcuts

Keyboard shortcuts are application settings, not project settings: they live in `settings.json`, survive New/Open, and are never written into a `.bimcraft` file.

## What can be assigned

A binding is either a **chord** (`Ctrl+D`, `Ctrl+Shift+D`, `Alt+F3`, `F8`) or a **sequence** of two or three keys (`WA`, `MV`, `KS`, `BMM`), typed plainly on the keyboard. One action may have several bindings (up to 32), and all of them work.

Every action has a stable identifier:

- `cmd.<id>`: a core command (`cmd.move`, `cmd.wall`);
- `ui.<area>.<action>`: a window action (`ui.file.save_as`, `ui.view.home`);
- `legacy.<name>`: an old action with no core command behind it.

The label in the interface may be translated; the identifier never is. Files and settings store the identifier.

## Physical keys and layouts

Chords and sequences are read from the **physical key position**, not from the character produced by the active layout. Therefore:

- `Ctrl+D` on a US layout and `Ctrl+В` on a Russian layout are the same key;
- `Ctrl+W, A` is not a chord: `W` and `A` are two plain keys, and Ctrl is held for neither;
- changing the layout while the window is open changes nothing;
- the interface shows the canonical form, followed in parentheses by what the interface language's keyboard layout prints on the same physical keys, when any differ: `Ctrl+Z (Ctrl+Y)` in German or Czech (QWERTZ), `WA (ZQ)` in French (AZERTY), `Ctrl+; (Ctrl+Ñ)` in Spanish, `Ctrl+' (Ctrl+À)` in Italian, `Ctrl+; (Ctrl+Ç)` in Portuguese (Brazil) and `Ctrl+1 (Ctrl+Ă)` in Vietnamese;
- English and Chinese print the US keys, so their hints are the canonical form alone; a binding whose keys all print what the US layout prints never gets parentheses;
- Polish prints the US keys too, but each key's national letter lives on `AltGr`, so its hint names it: `Ctrl+A (AltGr+Ą)`, `Ctrl+Z (AltGr+Ż)`. That says where the letter is on the key, not that `Ctrl+A` types `ą`.

Modifiers are matched **exactly**: `Ctrl+D` and `Ctrl+Shift+D` are different bindings. Holding a key down does not repeat the command.

## Typing a sequence

Sequences are typed plainly, without Ctrl (Cmd on a Mac): `W` and `A` for Wall, a letter at a time. A key held with Ctrl, Alt or Cmd is never part of a sequence: it runs only the chord Keyboard Shortcuts assigned it, and does nothing when none was assigned. Digits and a unit still make a typed length while a tool takes one, and a letter typed after them belongs to the length.

Ctrl+C, Ctrl+X and Ctrl+V stay the system's clipboard: a text field, the note editor and the view all copy, cut and paste with them, and they never type a sequence. The letters `C`, `X` and `V` are typed plainly instead (`CO` Copy, `MV` Move, `TX` Text).

A plain letter that begins no sequence does nothing on the view. A letter that begins one waits for the next for two seconds; if the second makes no binding, the first is dropped and the second waits in its place.

In a text field the letters belong to the field and no sequence is typed; the chords assigned there still act — Save, Save As, Open, New, Find, Settings, Export PDF and Properties.

The interface names a sequence by its keys alone (`WA`, `SA`, `KS`), as Revit does; nothing is held to type it, and the dialog, the tooltips and the Markdown help show the pair.

## Where to edit them

Manage > Shortcuts, the `KS` sequence, or `Ctrl+Shift+K`.

In the dialog:

- search by name, identifier, or binding;
- filters: All, Assigned, Without keys, Changed, Custom, and CantChanged. `Changed` means the action differs from its starting profile; `Custom` means bindings were added where the starting profile had none; `CantChanged` collects only actions that cannot be edited and already carry a binding. Actions that cannot be bound and have no binding are not listed at all;
- filter by category; the profile choice and Apply Profile sit in their own block below the filters, separated from them;
- each action has binding chips: click a chip to edit it, click the small `×` near its right edge to remove it, click `+` to add another. There are no vertical rules between chips; `+` follows the last chip;
- click a column heading to sort by that column; click it again to reverse the order. The arrow at the right of the heading shows the direction. The default is Category, ascending;
- Import…, Export…, and Help (Markdown)…;
- Apply writes the set to `settings.json`; Cancel or `Esc` rolls the changes back.

When a combination is already taken, the dialog reports the conflict and offers Replace, which removes it from the other action.

## Starting sets and presets

A **starting set** is a built-in set of bindings that comes from the code. A **preset** is a named
snapshot of a full set that the user keeps in `settings.json`. Presets are application settings: they
survive New/Open and are never written into a project.

### Starting sets

- **BIMCraft**: the starting set: the familiar two-letter commands (`WA`, `DR`, `WN`, `RM`, `CL`, `DI`, `TG`, `MV`, `CO`, `RO`, `MM`, `DM`, `AL`, `OF`, `DW`, `ZF`, `TL`, `CS`, `PP`, `NP`, `SA`, `VV`, `EH`, `HH`, `KS`, and others) plus the chords `Ctrl+S`, `Ctrl+O`, `Ctrl+N`, `Ctrl+K`, `Ctrl+,`, `Ctrl+P`, `Ctrl+Z`, `F8`, `F2`, `F3`, `Ctrl+0`, and others.
- **Revit-like**: mostly sequences.
- **Archicad-like**: the same, plus `Ctrl+D` for Move.
- **Empty**: no bindings.

These four are read-only: they can be applied, but not renamed or deleted.

### Keyboard presets

The Keyboard Shortcuts dialog has a **Keyboard presets** block, apart from the filters. It lists the
four built-in sets under **Built-in** and the user's snapshots under **My presets**.

- **Save Current as Preset…** keeps the set being edited under a name. The name is trimmed, must not
  be empty and is at most 64 characters; it is unique among all presets, built-in ones included.
  Saving does not change the current set.
- **Apply Preset** replaces the set being edited. If that set has changes, the dialog asks first;
  **Cancel** or `Esc` rolls the whole dialog back, the applied preset included.
- **Apply Changes** is still what writes the set to `settings.json`. Presets themselves are saved as
  soon as they are created, renamed or deleted.
- **Rename Preset…** and **Delete Preset** work on the user's presets only. Deleting the active preset
  leaves the current set as it is and only drops its label.
- A preset is marked **active** while the set matches it and **modified** once a binding differs.
- Limits: 32 presets, 64 characters per name, 4,096 bindings in one preset, 8,192 bindings across
  all presets.

Applying a built-in set sets the starting set the Reset Action command returns to. A user's preset
keeps the starting set it was saved from.

### Home and F8/F11 changes

| Action | Before | Now |
|---|---|---|
| Home / start page | `Ctrl+U` under the earlier plan | `Ctrl+D` (2026-11-12) |
| `Ctrl+D` | free under the earlier plan | Home in BIMCraft; Move in the Archicad-like profile |
| `Ctrl+U` | Home | free |
| Alignment guides | `F8`, `F11` | `F8` only |
| `F11` | alignment guides | free |

`Ctrl+D` does not act as Home while a text field has focus. In the note editor, `Ctrl+U` with Override Formatting remains Underline.

## The `.bks` / `.txt` file format

UTF-8, optional BOM, LF or CRLF. A line starting with `#` is a comment. Blank lines are ignored. One line holds one binding.

```text
# BIMCraft keyboard shortcuts
version = 2
profile = bimcraft

# Move
cmd.move = Ctrl+Shift+M
cmd.move = MV

# Wall
cmd.wall = WA
cmd.wall = Ctrl+Shift+E

# Door: physical D, captioned В on the Russian layout
cmd.door = Ctrl+В

# Save
cmd.save = Ctrl+S
ui.file.save_as = Ctrl+Shift+S

# Properties
ui.palette.properties = Ctrl+1

# Start page
ui.view.home = Ctrl+D
```

Rules:

- `action` is an identifier or an English/localized name; the identifier wins;
- several bindings for one action are several lines with the same `action`;
- comma-separated lists are not supported: `Ctrl+,` is the Settings key;
- `+` means a chord; two or three letters without `+` mean a sequence;
- `Ctrl`/`Primary` is the platform's primary modifier (Ctrl on Windows/Linux, Cmd on macOS); `Cmd`/`Meta` is the literal Command key;
- `Plus` is the plus key: write `Ctrl+Plus`, not `Ctrl++`;
- punctuation is written as its symbol for readability: `Ctrl+,`, `Ctrl+.`, `Ctrl+/`;
- a Cyrillic letter is translated to a physical key with the Russian layout table: `Ctrl+В` → `Ctrl+D`, `Ctrl+А` → `Ctrl+F`, `Ctrl+М` → `Ctrl+V`.

### JSON import

```json
{
  "version": 2,
  "profile": "custom",
  "bindings": [
    {"action": "cmd.move", "keys": ["Ctrl+D", "Ctrl+Shift+M", "MV"]},
    {"action": "cmd.wall", "keys": ["WA", "Ctrl+Shift+E"]}
  ]
}
```

The old JSON form `{"WA": "Wall"}` is read as legacy: each pair becomes a sequence over the current profile. JSON is an import format and the format of `settings.json`; it is no longer a separate export choice.

### Revit XML

Lines like `<Shortcut CommandName="Wall" Shortcuts="WA#W1"/>` are read as sequences. Chords cannot be represented in this format.

## Import and export

- **Import** supports `.txt`, `.bks`, `.json`, and Revit XML. Markdown is reference documentation, not a backup: an `.md` file is refused for import.
- A preview shows how many bindings were read, errors with line numbers, and conflicts before anything is applied.
- Replace everything replaces the set; Add to the current set appends; conflicts are skipped and reported.
- Before replacing, `shortcuts.bak` is written beside the settings file.
- **Export** writes the text `.txt`/`.bks` format. A JSON export button is intentionally not offered.
- Limits: 256 KiB, 10,000 lines, 4,096 bindings.

## Markdown help

Help (Markdown)… in the Keyboard Shortcuts dialog writes `bimcraft-shortcuts.md`:

- every action in the registry, including actions without keys;
- one table per category with exactly two columns: **Command** and **Active**;
- **Active** lists every combination that currently works, including combinations assigned by the user;
- a separate reference table of reserved and system combinations;
- hints in the interface language's layout, such as `Ctrl+Z (Ctrl+Y)` in German, `WA (ZQ)` in French, `Ctrl+1 (Ctrl+Ă)` in Vietnamese or `Ctrl+A (AltGr+Ą)` in Polish.

After saving, the dialog shows one clickable link to the file. The link opens the file with the application associated with `.md` in the operating system (for example MarkText), not in a browser.

The file can be opened in GitHub, VS Code, Obsidian, or printed. It cannot be imported; use `.bks`, `.txt`, or `.json` for a backup.

## Reserved keys

These cannot be assigned: `Esc`, `Enter`, `Tab`, `Backspace`, `Space`, the arrow keys, and the system's clipboard chords `Ctrl+C`, `Ctrl+X` and `Ctrl+V` (`Cmd` on macOS). A sequence may still use those letters (`CO`, `MV`, `TX`); the dialog, an import and a hand-edited `settings.json` all refuse the chord.

These can be assigned with a warning: `Delete`, `F1`–`F12`, `Ctrl+Alt+…` (AltGr), `Ctrl+Shift+Esc`, `Alt+F4`, `Alt+Tab`, `Ctrl+Esc`; on macOS, `Cmd+Tab`, `Cmd+Space`, and other combinations intercepted by the system.

## settings.json

```json
{
  "shortcuts": {
    "version": 2,
    "profile": "custom",
    "bindings": [
      {"action": "cmd.move", "keys": ["Ctrl+D", "MV"]},
      {"action": "cmd.wall", "keys": ["WA"]},
      {"action": "ui.view.home", "keys": ["Ctrl+D"]}
    ],
    "active_preset": "Work keys",
    "presets": [
      {
        "name": "Work keys",
        "base": "archicad",
        "bindings": [
          {"action": "cmd.move", "keys": ["Ctrl+D", "MV"]},
          {"action": "cmd.wall", "keys": ["WA"]}
        ]
      }
    ]
  }
}
```

The file stores the full effective set, not only differences from the starting profile, so an update does not bring back bindings the user deleted. The old `{"WA": "Wall"}` form is migrated on first load.

`presets` and `active_preset` are optional and are ignored by builds that do not know them, which still read `bindings` as before. A built-in set in `active_preset` is written as `builtin:<code>` (`builtin:archicad`); a user's preset is written by its name. Presets are application settings: exporting or importing a `.bks` file changes the current set only, never the preset list.

## How to see every combination

1. Manage > Shortcuts (`KS`).
2. Help (Markdown)… to save the cheat sheet.
3. Or use the dialog table directly with the All filter and search.

`.md` is reference documentation; `.bks`, `.txt`, and `.json` are backups for import.
