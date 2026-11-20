# Project templates: plan

Status: all seven steps built (PR #503); how it works now is in [the desktop](areas/desktop.md#project-templates).

A project template is a `.bimt` file: the same content as a `.bimc` project. Opening one doesn't open the file. It is read
and let go, and its content becomes a new, unsaved project to start work in, as Revit's New Project from a template does.
Save asks where to put the result, so a template is never overwritten by accident.

## What exists already

- `files::TEMPLATE` (`bimt`) is a project extension (`files::PROJECTS`), and Save As already offers it (`files::SAVED`).
- `open` reads a `.bimt` as a project, but keeps its path, so Save writes back into the template. That's the bug this fixes.
- File > Open Recent (`recent.rs`) keeps its list in `recent.json` in the user's configuration folder, beside the favorites.
  The template list follows the same pattern.

## Steps

1. **Core: a template opens as a new project.** `open` of a `.bimt`, or of any project with `as_template: true`, reads the
   file, releases it and leaves the session without a path (`Session::reset(model, None)`), with an empty undo stack. Its
   reply says `template: true`. The CLI, scripts and MCP get this for free. Tests: Save after opening a template asks for a
   path (the session has none), the template file is unchanged, and the model is the template's.
2. **Core: Save as template.** `save {path: "….bimt"}` already writes one. The session keeps the project's own path, so the
   next Save still writes the project, not the template. Tests in `tests.rs`.
3. **Desktop: the template list.** `project_templates.rs` holds the list (`templates.json` beside `recent.json`): an ordered
   list of paths, its file read kindly (missing, damaged or oversized means an empty list) and capped in size. It sits beside
   the favorites, not in a project.
4. **Desktop: New Project shows a small popup.** File > New > Project (Ctrl+N) and the start page's New open a popup listing
   the first `MAX_SHOWN` (10) templates by file name (with the folder when two share a name), plus **Empty project** and
   **Browse…**. A click on a template loads it as in step 1. Browse… opens the system dialog filtered to `.bimt`. Its pick
   loads it and adds its path to the list. A template that is gone is said and taken off the list.
5. **Desktop: Settings > Templates.** A new `Page::Templates`. The full list, uncapped, with Add… (system dialog), Remove, and
   Move up / Move down. A line marks where the popup's first ten end, so it's clear the ones below need moving up to show.
6. **Desktop: Save As > Template.** File > Save As becomes a submenu: Project… and Template…. Template… asks for a `.bimt`
   path, saves, and adds the template to the list.
7. **Docs and translations.** A paragraph in `docs/areas/desktop.md` (and `packaging.md` for `.bimt`), the module's line in
   `crates/ui-egui/CLAUDE.md`, and `cargo i18n update`.

## Tests

- Core: `open` of a `.bimt` leaves no path, `template: true`, and the file unchanged after Save As elsewhere.
- `project_templates.rs`: load and save round trip, order kept, duplicates once, odd entries dropped, oversized file ignored.
- egui_kittest: Ctrl+N shows the popup; a click on a listed template opens an untitled project with its content; only ten are
  listed when twelve are kept; Settings > Templates moves and removes; Save As > Template… adds to the list.
