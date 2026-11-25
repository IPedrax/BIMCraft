# Rebar details

The drawing a detailer issues for a reinforced member. Code: `crates/core/src/rebar_detail.rs` (the element, its check, the sets of
bars, the tag text, the shapes) and `crates/core/src/sheet/detail.rs` (the layout and the drawing); the command is in
`crates/core/src/commands/rebar_detail.rs`, tests in `rebar_detail_tests.rs`. A beam, a column, a footing and a floor have one.

## What is stored, and what is not

`Element::RebarDetail` (category `rebardetail`) is a view of its own, as a panel schedule is: `name`, `host` (a GUID), `scale` (1 : n,
default 50), `stations` (where its sections are cut, mm along the beam or column; none means the default) and `list` (whether its steel list is
in). It took no FORMAT step of its own: it shares 21's. Everything it shows is worked out from the member and its bars each time it is
drawn, so reinforcing the member again changes it with no command. Deleting the member deletes it and any viewport of it. Nothing may be
annotated in it; a note goes on the sheet.

## Sets

A *set* is bars that are copies of one another moved by one step, to 0.5 mm: the stirrups of a zone, a row of bottom bars. Bars are
grouped by layer, grade, diameter and `steel::Shape::key`, put in order by their first point, and cut into maximal runs of equal step.
Where two runs of different step meet, the bar between them goes to the run with the closer spacing. That is why `reinforce`'s ends
rule reads 7, 31 and 7 stirrups on a 6 m beam and not 7, 32 and 6: its closer end zones keep their bars and the wider middle starts one
spacing in. A bar of another shape or diameter is never in a set, and a bar drawn by hand is a set of one. The counts of a beam's sets
add up to its bars. A set's mark is the host's (`N2`), so the zones of one stirrup share it.

## The tag

`{mark} {count} ø{diameter} c/{spacing} C={length}`: no count for a set of one, and `c/` only where the set's bars spread along the
beam in the picture (the stirrups, not three bottom bars lying behind one another). Lengths are in the project's unit and decimals
(`Model::length`), so a project in centimetres reads `c/10 C=118`; the diameter is always millimetres. A tag in a section never has
the spacing. In a beam's or a column's picture, sets of one mark that say no spacing (a column's bars lie in rows of 3, 2 and 3, a
round one's are single) are one tag with their bars added, `N1 8 ø16 C=2950`, as a detailer writes it; the zones of a stirrup do not
join, since each says where it is, and a mesh's two directions do not either (`tag_count`, `pend` in `sheet/detail.rs`). The reply's
`sets` still list each set with its own tag.

## The drawing

One drawing per beam, laid out as a display list in pixels (`sheet::detail::layout`) that `draw` puts on a sheet. Its picture is the
member's and its bars' own, not `render::section_in`: a section of the model hides a bar beyond its cut under the concrete, and a
detail doesn't. From the top: the elevation (the concrete's outline, every set's bars as a heavy line, bars lying on one another once,
a zone of three or more stirrups as its first and its last with a thin line between), a column of tags to its right (one tag a row,
ordered by where its anchor is, so none overlaps), section marks A, B, C, the beam's length and each zone dimensioned; under that the
sections, each at **twice the scale** of the elevation and lettered with its own, with the stirrup of the zone it is in, each bar it
cuts as a dot, and the width dimensioned; under those the steel list (`steel::rows` for the beam) with each bar's shape sketched, its
columns as wide as their headings and cells need. A curved bar (a round hoop is 24 chords) is sketched without its legs' lengths and
angles, which would pile up; the row's length says it.
Default stations of a beam are the middle of each zone of stirrups, or of the beam where it has none.

**A column** is drawn standing: the same elevation turned up (its length on y), its tags to the right, its zones and its height
dimensioned in columns to its left, and its sections (a circle for a round one) under it at the foot, the middle and the head of its
stirrups, which are its default stations. A tie, seen end on in the elevation, is a dot. **A footing** is drawn in plan, a to the right
and b up (its own axes, so a turned footing is drawn square): the outline, each mesh layer as its first bar and its last with a thin
line between, tagged with its spacing, since those bars spread across the plan; its section A-A is under it, lined up with it, the bars
projected as they lie (a layer's bars along the cut as a line, those across it as dots), with its thickness dimensioned. **A floor** is
the plan alone, its outline and openings in its own x and y; it has no section (`section` draws a floor with its bars as dots already).
A footing and a floor take no stations. A floor whose bars differ in length (an outline that isn't a rectangle) has a set, so a tag,
for each length: that is the sets rule, bars that are copies of one another, and not a shortcut that hides them.

Stated shortcuts, each one function to change: tags stack in a column (no smarter placer), a bar's dot is never under 0.4 mm on the
paper, a spatial bar's sketch is its projection and says so, there are no support ticks, and a drawing stops at 50,000 lines and says
it did. The look (line weights, the order of tags, the zone drawn as a first and a last) is general detailing practice, not
Eberick's.

## The command

`rebar_detail {host | hosts, stations?, scale?, list?, sheet?, replace?}`: makes a detail for each member (up to 1,000), or changes the
one it has and keeps its name and its place; `sheet` places it through `place`; `replace` takes the values back to the defaults. A
member with no bars is refused, saying to reinforce it; so is `stations` for a footing or a floor. The reply lists each view's `sets` (mark, layer, diameter, count, spacing,
length, tag, the ids of the bars), so an agent can read what the drawing says. `render {"view": <its name>}` draws it. Its shortcut is DD.

## The desktop

`crates/ui-egui/src/rebar_detail.rs`. The Structure tab's Detail (DD) opens the Reinforcement Detail dialog for the members selected (all
of the selection must be beams, columns, footings or floors: it says what to select otherwise): Stations in the project's unit, the scale as a 1 : n, whether the
steel list is in, and where it goes (no sheet, a sheet, or a new one). Make runs `rebar_detail` (with a `sheet` command before it for a
new one, numbered A101, A102 and so on, in one undo step) and opens what it made: the sheet, or the detail's own tab. A refusal (a
beam with no bars, a station that is not a length) is the command's words, said inside the dialog. The tab is a `Sheet::Table`, as a
panel schedule's is: the drawing from `sheet::draw_view` at the detail's scale on a sheet of paper the size it takes, which pans and
zooms, nothing in it picked or moved. The Project Browser lists the details under Reinforcement Details, with a right-click Delete.
