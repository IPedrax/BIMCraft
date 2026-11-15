# Copy and paste

Revit's Clipboard panel: Copy to Clipboard (Ctrl+C), Cut to Clipboard (Ctrl+X) and Paste (Ctrl+V) with its six ways of
placing what's pasted. Anything selected in any view can be copied and pasted into any view it can stand in: a plan, a
ceiling plan, a section, an elevation, a 3D view; in this project, or in another one open in another BIMCraft window.

## What Revit does (the behaviour we follow)

- **Copy to Clipboard** keeps the selection, in any view. **Cut** keeps it and deletes it, in one undo step.
- **Paste** (Ctrl+V, Paste from Clipboard) places it by hand: the elements follow the cursor, a click puts them down. In a plan,
  model elements land on the level of the view; in a section or an elevation, on the view's plane, the cursor's height taking
  them up or down. The base point is the middle of what was copied.
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
  move in the target view's coordinates, or `at`, where the middle goes), `current_view`, `same_place`, `levels`, `views` or
  `picked_level`.

## Where the code lives

- `crates/core/src/clipboard.rs`: the clipboard's text (written, read, checked), the views' frames, and the paste.
- `crates/ui-egui/src/clipboard.rs`: the Clipboard panel, Ctrl+C / Ctrl+X / Ctrl+V, the system clipboard, the paste preview
  that follows the cursor, and the Selected Levels and Selected Views dialogs.

## Status

Work in progress on `feat/copy-paste`; this file says what is done as it lands.
