# BIMCraft keyboard shortcuts

Keyboard shortcuts are application settings, not project settings: they live in `settings.json`, survive New/Open, and are never written into a `.bimcraft` file.

## What can be assigned

A binding is either a **chord** (`Ctrl+D`, `Ctrl+Shift+D`, `Alt+F3`, `F8`) or a **sequence** of two or three keys (`WA`, `MV`, `KS`, `BMM`). One action may have several bindings (up to 32), and all of them work.

Every action has a stable identifier:

- `cmd.<id>`: a core command (`cmd.move`, `cmd.wall`);
- `ui.<area>.<action>`: a window action (`ui.file.save_as`, `ui.view.home`);
- `legacy.<name>`: an old action with no core command behind it.

The label in the interface may be translated; the identifier never is. Files and settings store the identifier.

## Physical keys and layouts

Chords and sequences are read from the **physical key position**, not from the character produced by the active layout. Therefore:

- `Ctrl+D` on a US layout and `Ctrl+В` on a Russian layout are the same key;
- `WA` works on a Russian layout too (physically the same `W` and `A` keys);
- changing the layout while the window is open changes nothing;
- the interface shows the canonical form, followed by the localized hint `(Ctrl+В)` when the key has a Cyrillic caption.

Modifiers are matched **exactly**: `Ctrl+D` and `Ctrl+Shift+D` are different bindings. Holding a key down does not repeat the command.

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

## Profiles

- **BIMCraft**: the starting set: the familiar two-letter commands (`WA`, `DR`, `WN`, `RM`, `CL`, `DI`, `TG`, `MV`, `CO`, `RO`, `MM`, `DM`, `AL`, `OF`, `DW`, `ZF`, `TL`, `CS`, `PP`, `NP`, `SA`, `VV`, `EH`, `HH`, `KS`, and others) plus the chords `Ctrl+S`, `Ctrl+O`, `Ctrl+N`, `Ctrl+K`, `Ctrl+,`, `Ctrl+P`, `Ctrl+Z`, `F8`, `F2`, `F3`, `Ctrl+0`, and others.
- **Revit-like**: mostly sequences.
- **Archicad-like**: the same, plus `Ctrl+D` for Move.
- **Empty**: no bindings.

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
- localized hints such as `Ctrl+D (Ctrl+В)`.

After saving, the dialog shows one clickable link to the file. The link opens the file with the application associated with `.md` in the operating system (for example MarkText), not in a browser.

The file can be opened in GitHub, VS Code, Obsidian, or printed. It cannot be imported; use `.bks`, `.txt`, or `.json` for a backup.

## Reserved keys

These cannot be assigned: `Esc`, `Enter`, `Tab`, `Backspace`, `Space`, and the arrow keys.

These can be assigned with a warning: `Delete`, `F1`–`F12`, `Ctrl+Alt+…` (AltGr), `Ctrl+Shift+Esc`, `Alt+F4`, `Alt+Tab`, `Ctrl+Esc`; on macOS, `Cmd+Tab`, `Cmd+Space`, and other combinations intercepted by the system.

## settings.json

```json
{
  "shortcuts": {
    "version": 2,
    "profile": "bimcraft",
    "bindings": [
      {"action": "cmd.move", "keys": ["MV"]},
      {"action": "cmd.wall", "keys": ["WA"]},
      {"action": "ui.view.home", "keys": ["Ctrl+D"]}
    ]
  }
}
```

The file stores the full effective set, not only differences from the starting profile, so an update does not bring back bindings the user deleted. The old `{"WA": "Wall"}` form is migrated on first load.

## How to see every combination

1. Manage > Shortcuts (`KS`).
2. Help (Markdown)… to save the cheat sheet.
3. Or use the dialog table directly with the All filter and search.

`.md` is reference documentation; `.bks`, `.txt`, and `.json` are backups for import.
