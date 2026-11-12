# Co-Work

Live co-editing of one project: a host's window holds the model, guests connect to it, and everyone sees the others' edits,
cursors, cameras and selections, and shares one AI chat. The whole design, the decisions and the build order are in
[COWORK-PLAN.md](../../COWORK-PLAN.md). This page says what is built.

## Built: the engine's side (slice 1)

`crates/core/src/cowork.rs`, with its tests in `cowork_tests.rs`. Nothing here touches the network.

- **Deltas.** `cowork::delta(before, after)` says what a command changed, for a guest's copy that has `before`: the elements
  alone (`Delta::Elements`: each one as it is now with its id and GUID, each id gone, the next id) when nothing else changed,
  else the whole model as a file holds it (`Delta::Whole`). It reads `Model::changed_since`. `cowork::apply(copy, delta)` takes
  one in and checks the result as a command's model is checked; a delta that doesn't fit (an element the copy never had,
  another element's GUID, a door left with no wall, a next id behind an element's or past `LINKED`, a model that doesn't read)
  is an error and leaves the copy as it was. A `Delta` is JSON with `deny_unknown_fields`, so a host is read as strictly as a
  command. The test that guards it runs a session of walls, doors, moves, a type, undo, redo and delete, and holds a copy fed
  only the deltas, through JSON, to the host's model file for file.
- **A guest's command on the host.** `cowork::run_for_guest(session, guest, command, params, made_in, edit)` runs it as the
  guest (`Session::author`), in the guest's phase and option, and gives the session back as it was. It refuses what would
  replace the project, and while it runs `Session::execute` refuses any command that reads or writes a file (what
  `touches_files` marks, or any `path` or `save` given), a plugin's nested commands too, so a guest never reaches the host's
  disk; a guest editing a link's `path` is refused by the same rule. With `edit` false (the host lets guests only look) an
  edit is refused with `cowork::LOOK` and what reads still answers.
- **Each person undoes their own edits.** While `Session::author` is set (a session's host sets its own name, and
  `run_for_guest` the guest's), every step records who made it (`Step::by`), and `undo` and `redo` take back the author's own
  (`Session::undo_own`) instead of the top of the history. Outside a session they are as they always were.
  - Undo picks the author's last edit still standing, wherever it is, and puts what it changed back as it was before it, as a
    new step on top named `Undo <label>`. Redo picks their last undo still standing, unless an edit of theirs came after it,
    and makes `Redo <label>`. `count` takes several, all or nothing.
  - It is refused, and nothing changes, when anything the step changed is no longer as the step left it: an element someone
    changed, removed or gave back since, or the project's settings when the step changed them and they have changed since.
    Comparing what is there now with what the step left handles undos and redos made by others in between without special
    cases. The error names the last step after it that touched those elements, with its author and label, and the elements:
    "can't undo your Wall: Bo changed wall 100234 after it (Bo's Move)".
  - It is refused too when the model it would leave fails the check (Bo put a door in Ana's wall, and her undo would take the
    wall away); then the error lists whose steps came after it.
  - Settings are compared as a whole (`Model::same_settings`, which `Model::diff` uses): two steps that both changed any
    setting conflict even when they changed different types. That is safe and sometimes too strict.

## Built: the interface's funnel

`BimApp::exec` and `exec_batch` (`crates/ui-egui/src/lib.rs`) are the one way the desktop changes the project: `run` goes
through `exec`, and the dialogs that ran several commands as one step (Visibility Set, Fill Patterns, Line Styles, Electrical
Settings, Draw's runs) go through `exec_batch`. `tests/cowork.rs` reads the interface's sources and fails on any other
`self.session.execute`, `transaction` or `script`. It is where a guest's commands will go to the host.

## Not built yet

The transport (`crates/cowork`), hosting and joining in the desktop, presence, and the shared AI chat: slices 2 to 5 of the
plan.
