# Copy and paste

Revit's Clipboard panel: Copy to Clipboard (Ctrl+C), Cut to Clipboard (Ctrl+X) and Paste (Ctrl+V) with its six ways of
placing what's pasted. Anything selected in any view can be copied and pasted into any view it can stand in: a plan, a
ceiling plan, a section, an elevation, a 3D view; in this project, or in another one open in another BIMCraft window.

## What Revit does (the behaviour we follow)

- **Copy to Clipboard** keeps the selection, in any view. **Cut** keeps it and deletes it, in one undo step.
- **Paste** (Ctrl+V, Paste from Clipboard) places it by hand: the elements follow the cursor, a click puts them down. In a plan,
  model elements land on the level of the view; in a section or an elevation, on the view's plane, the cursor's height taking
  them up or down. The base point is the middle of what was copied. In a 3D view, Ctrl+V asks: in the same place (Aligned to
  Same Place), or by hand on the view's work plane: the copy's box follows the cursor over it, and a click puts the middle
  there in plan, the model elements on the level that point stands on (the highest at or below it).
- **Aligned to Selected Levels**: model elements, onto each level picked from a list, each at the same place in plan and the same
  height over its level as over the level it was on.
- **Aligned to Selected Views**: view-specific elements (detail lines, filled regions, notes, dimensions, tags, detail groups,
  spots), into each view picked from a list of the views like the one they were copied from (plans for a plan, parallel
  sections and elevations for a section or an elevation).
- **Aligned to Current View**: view-specific elements into the open view, where they were in theirs; model elements onto the
  open plan's level, at the same place in plan.
- **Aligned to Same Place**: everything where it was: the same place in plan, the same level (by name in another project).
- **Aligned to Picked Level** (a section or an elevation): a level line is clicked; model elements go onto it, keeping their
  places along the view.
- A view-specific element can't be pasted into a 3D view, nor a model element onto a sheet; model elements and view-specific
  ones copied together paste together where both can (by hand, or aligned to the current view).
- Hosted elements: a door or a window copied with its wall comes with it; copied without it, it is pasted into a wall of the
  target at that place, else refused. A tag or a dimension comes with what it names or measures, else it is left out.
- A group is pasted as a group, its members placed anew from its type (copied with their group, they are not pasted on their own); a
  curtain wall comes with the walls in its cells and their doors, as the Copy tool takes them.
- Names and numbers that are taken get the next (`Wall 2`, room `102`, door mark `D3`); types, materials and families the target
  hasn't come with the elements (the target's own, by name, where it has them).

## Between views of other orientations

What a view-specific element's points mean depends on its view: plan millimetres in a plan, (along, up) in a section or an
elevation, paper millimetres on a sheet. Pasted into another view it is taken by the two views' frames:

- **Same orientation** (plan to plan; a section to a parallel section or elevation looking the same way): by the model's
  coordinates, so a note on a wall in one section lands on the same wall in the other.
- **Another orientation** (plan to section, a section to one at an angle, a sheet to a view and back): by the view's own
  coordinates, the view's origin to the view's origin (a plan's project origin, a section's start at its level 0), which is
  Revit's "view orientation".
- By hand, the view's coordinates of the copy's middle follow the cursor in either case.

## Between BIMCraft windows

The clipboard is text: a small project of its own (`bimcraft clipboard` JSON) holding what was copied, what it needs that
isn't a view or a host (its levels, types, families, materials, line styles…), and where it was copied from (the project's
uid, the view and its frame, the level). The window puts it on the system clipboard, so another BIMCraft window (another
process, another project) pastes it as this one does; Paste reads it through `insert`, Insert from File's engine, so a
project's library comes over as Insert from File brings it.

## Commands

- `copy_to_clipboard {ids, view?}` → `{clipboard, count}`: the text, also kept by the session for a `paste` without one.
- `cut_to_clipboard {ids, view?}` → the same, and the elements deleted.
- `paste {clipboard?, mode, view?, levels?, views?, at?, by?, level?}` → `{ids, left_out}`; `mode` is `manual` (with `by`, the
  move in the target view's coordinates, or `at`, where the middle goes; in a 3D view, with `level`, the level the model
  elements go onto), `current_view`, `same_place`, `levels`, `views` or
  `picked_level`.

## Where the code lives

- `crates/core/src/clipboard.rs`: the clipboard's text (written, read, checked), the views' frames, and the paste.
- `crates/ui-egui/src/clipboard.rs`: the Clipboard panel, Ctrl+C / Ctrl+X / Ctrl+V, the system clipboard, the paste preview
  that follows the cursor, and the Selected Levels and Selected Views dialogs.

## Status

Done:

- The core: `copy_to_clipboard`, `cut_to_clipboard` and `paste` with all six modes; view-specific elements between views by
  location (same orientation) or by the view's own terms (another); levels matched by height (or by name, in place), a top
  this project has no level for kept as a height; doors and windows without their wall put into a wall there; ties to what
  wasn't copied (attachments, a roof's join, a room's finishes) dropped, and what can't be without it (tags, dimensions,
  railings, rebar, hosted components) left out, saying why; marks and room numbers made the next where taken. Tests:
  `clipboard_tests.rs`, and the hostile sweep's payloads.
- The window: Modify > Clipboard (Paste and its options, Cut to Clipboard, Copy to Clipboard); Ctrl+C, Ctrl+X and Ctrl+V,
  which the window's system hands over as copy, cut and paste events, so Ctrl+V reads another window's copy from the system
  clipboard (the ribbon's Paste asks for it with `RequestPaste`, and takes the window's own copy when nothing comes); Paste
  from Clipboard following the cursor in plans (the clipboard's project drawn as a plan draws it, once a view), sections and
  elevations and sheets (its box); Cut, Copy and Paste in the right-click menu; the panel yields its Cut and Copy to its list
  before another panel folds, and its list (the panel's name) holds them one under another; Aligned to Picked Level by a click
  on a level line in a section or an elevation, the level under the cursor lit (a level line, though a wall lies nearer); the
  Selected Levels and Selected Views lists; Ctrl+V in a 3D view asking Same Place or Position on Work Plane, and the paste by
  hand there, its box over the work plane. Tests: `crates/ui-egui/src/tests/clipboard.rs`.
- A family's tab: the same panel (`clipboard::panel`, which both draw) and the same Copy, Cut and Paste events and ribbon
  paste, handed to the family's clipboard, whose text is marked as a family's (`family::clipboard::KIND`), so a project's
  paste doesn't take it for its own: it refuses it, saying to paste it in a family tab, rather than paste the project's older
  copy ([families](families.md)). A
  paste the ribbon asked for on one tab and answered on another is given up (`paste_asked` keeps the tab). Tests: `crates/ui-egui/src/tests/family_clipboard.rs`.

Still to do:

- A paste by hand in a 3D view on a work plane that isn't level puts the copy level, on the level under the point picked.
- Ctrl+V's preview in a section or an elevation drawn element by element rather than as a box.
- A view-specific element pasted from a view onto a sheet keeps its size in millimetres rather than taking the view's scale.
