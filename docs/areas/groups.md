# Groups: model groups, detail groups and flexible groups

Revit's groups, in `crates/core/src/groups.rs` and `groups/`: a group type is a set of elements kept about an origin, and each
group placed is an instance of it whose members are ordinary elements of the project, drawn, scheduled, exported and picked as any
other. A change to the type is a change to every instance. Model groups and detail groups are separate, as in Revit's Project
Browser:

- **Model groups** (`Model::model_groups`, `Element::ModelGroup`) hold model elements: walls (and their doors and windows), floors,
  ceilings, roofs, shafts, rooms and room separation lines, columns, footings, beams, pipes, ducts and the other runs, stairs,
  ramps, railings, masses, rebar, property lines and components that stand on a level. A model group stands on a level.
- **Detail groups** (`Model::detail_groups`, `Element::DetailGroup`) hold what is drawn in one view: detail lines, filled regions,
  text notes and dimensions. A detail group stands in a view.

A selection all of one kind can be grouped (`groups::kind_of`); one with both kinds, or with something that is in no kind (levels,
grids, views, sheets, tags, links, scans, CAD), cannot, and says what is in the way, unless `create_group {only: model | detail}`
says which kind to make of it: `groups::split` keeps that kind's elements (for detail, those in the first one's view), leaves out the
rest (the other kind, what no group holds, what is grouped already) and the command lists it as `left_out`. A member that points at another element
(a door at its wall, a railing at its stair, a wall attached to a roof, a room at its own floor, a bar at its host, a dimension at
what it measures) may be grouped only with that element: a dimension of a detail group measures only the group's own lines and
regions. Elements outside a group may point into it (a tag on a grouped door, a dimension to a grouped wall): members keep their
GUIDs, so those keep pointing.

## How a type holds its members

A type's members (`Member`, parts with an id and a GUID, as before) are elements in the group's own frame: about its origin,
unturned, and with what they point at made tokens: another member by that member's GUID, the group's levels by
`groups::level_token(k)` and, for a detail group, its view and that view's level by `VIEW` and `LEVEL`. A model group type lists
its levels (`GroupType::levels`): for each token, how many storeys above the group's own level it is (`above`) and how far above
it (`height`, mm). Members keep the parameters they were made with.

An instance (`ModelGroup`, `DetailGroup`) is its type's name, its origin `at`, its `angle` and `mirrored`, the level it stands on
(or the view), the levels its tokens stand for (`levels`, worked out when it is placed: the level that many storeys above its own)
and its `members`: for each member of the type, the element that is it in this instance (`Slot`), or that it is excluded.
`groups::place` makes an instance's member from the type's: tokens to the instance's elements and levels, then moved, turned and
mirrored as `commands::transform` takes elements, a door's place along its wall and its hand flipped by a mirror as Mirror does.
Marks and room numbers are each instance's own (`groups::OWN`): placed anew, and never a change to the group. So are a member's
values of the project parameters that vary by group instance (`vary_by_group`, `groups::varying`; [parameters](parameters.md#in-groups)):
set without Edit Group, kept when the group is placed again, taken by its copies. The rest are aligned per group type.

## A group changes as a whole

After every command (`groups::keep`, in `Session::perform` after constraints and rooms), each instance the command touched is
looked at:

- **Its placement changed** (moved, turned, mirrored, copied, given another level or type): its members are placed again from the
  type, keeping their ids and GUIDs. Move, Copy, Rotate, Mirror and Delete given an instance take its members with it.
- **A member changed, or went, outside Edit Group**: a conflict. The command is refused with an error that starts
  `groups::CONFLICT` and names the group and what changed, and that says how to go on: `resolve_group {fix, command, params}`
  runs the same command again with `fix: ungroup` (the instances it changes are ungrouped first, their members kept as they
  are) or `fix: new_type` (each changed instance becomes a group type of its own, as it now stands). The desktop asks with
  Revit's three choices: Ungroup, Create New Type, Cancel.
- **A level it stands on moved**: a group's members don't move with a level they don't stand on, but they may be taller. A
  group that isn't flexible keeps its storeys: a level of it whose height above the group's level isn't the type's is a
  conflict too. A **flexible** group (`flexible: true`, an instance property: Properties' Flexible check box) lets a
  member stretch: a level that is only some member's top (a wall to Level 2, Level 2 moved up) may be at any height, so long as
  everything else of the member stays where it was: the level it stands on, its plan and its offsets. A level a member stands on,
  moved, still moves all of it, and is still a conflict.

Placed on another level (`model_group {level}` or `{levels: [...]}`, Revit's Paste Aligned to Selected Levels), a group's
tokens go that many storeys up from there. A group that isn't flexible is placed only where those storeys are the type's
heights; a flexible one is placed wherever the levels it stands on are, its tops reaching the levels there, however tall.

## Picking, Tab and excluding

A click on a member picks its group (`Model::group_picks`, which front ends run on what a pick finds): the instance comes first,
its member second, so Tab steps from the group to the element in it. Selecting an instance highlights its members
(`Model::with_members`). `exclude_member {ids}` takes members out of their instances (Revit's Exclude): the elements go, the
slot remembers them, and `restore_excluded {id}` puts every excluded member of an instance back as its type has it.

## Edit Group

`edit_group {id}` starts editing an instance (Revit's Edit Group): while it is on, its members change freely, `edit_group {add:
[ids]}` and `{remove: [ids]}` add elements to it and take members out (the elements stay, ungrouped), `{finish: true}` makes the
type what the instance now is and places every other instance again, and `{cancel: true}` puts the project back as it was when
the edit started. A member removed may be added back before Finish. `Session::group_scope` (`groups::Scope`) is what a front end may
pick meanwhile: the group's members, less those removed, with those added or drawn; `group_picks` and `as_picked` given it pick only
those, and `Scope::addable` is what Add may take (of the group's kind, in its view for a detail group, in no other group).

## Files

FORMAT 33 (past 20: 21 to 32 were the Family Editor's unreleased steps, still refused as newer, `model::UNRELEASED`). A
detail group of a file of 20 or before had no members of its own (its type's were drawn in its place): once the file is read
and its names are names again, `Model::place_old_groups` places them, so each becomes an instance with real members.
Repeating details still draw their type's members, as before.

## On the desktop

The Modify tab's Create panel has Create Group (GP), always there: a selection that can't make one group asks which kind to
make of it (`groups::group_split_window`: Model elements only, Detail elements only, each with how many it takes, or Cancel), and
runs `create_group {only}`. Model Group (Place Model Group, with the type and Flexible on the Options Bar) is there once there is
a model group. Flexible's hover note (`groups::FLEXIBLE_TIP`, on the Options Bar and on the Properties row) says what it does. With a
group selected, the Group panel has Edit Group, Ungroup (UG) and Restore All Excluded; with a member selected (Tab), Exclude, and
Properties sets only its parameters that vary by group instance, the rest greyed (`groups::held`).

In Edit Group the window says so: a frame in the edit's colour round every view and a banner at its top naming the group and what
may be changed (`edit_group_frame`), and the prompt line. Only the group's own picks, in plans, sections and the 3D view
(`groups::picks`, through `Session::group_scope`), and the selection keeps only those whatever else selects (`keep_to_group`). A
plan draws the rest greyed: washed with the paper's colour and the group's members drawn again over it (`canvas::grey_outside`);
sections and the 3D view tint the rest nearly the paper's colour (`groups::greyed`, `groups::grey`), their shading and edges
still showing it. A panel beside Select on every tab has Add and Remove, which are picks (`GroupPick`): with
Add on, what may join is what picks, and a click or a box adds it at once; with Remove on, a click takes a member out (Remove
with members selected takes those at once). Esc or the button again stops picking. Finish Edit Group and Cancel Edit Group end
it. The right-click menu offers the same. A selected group lights its members, with a dashed frame round them and a cross at its
origin (`canvas::group_frames`); sections and the 3D view pick a member's group too, and light its members. A group under the
pointer lights its members in the hover colour (`groups::hover_lit`; in a plan, `canvas::paint_hovered`), so what a click takes
is seen before Tab steps into it. A command the core refuses as a group
conflict opens Group Changed (`groups::Conflict`): Ungroup, Create New Type or Cancel, which run it again through
`resolve_group`. `BIMCRAFT_GROUPS_DUMP=<folder>` with the test
`group_screenshots` writes pictures of these, light and dark.
