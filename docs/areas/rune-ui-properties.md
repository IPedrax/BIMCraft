# The Properties palette in the Rune UI

The Properties palette of `crates/ui-rune`, to parity with egui's (`crates/ui-egui/src/properties.rs`, 4,892 lines, read and never edited
from here). This file is the inventory of what egui's palette does, the design that carries it over by field kind and by a table of
categories instead of a port line by line, and what is built and what is not. Notes for the crate: [crates/ui-rune/CLAUDE.md](../../crates/ui-rune/CLAUDE.md);
the plan for the whole Rune UI: [the Rune UI](rune-ui.md).

## 1. What egui's palette is

`BimApp::properties` lays out, top to bottom: the **Type Selector**, a **filter row** (what is shown, a count, Edit Type), the
**parameters** under group headers, a stack of **extra panels** for what a table of rows cannot say, and **Apply**. Every edit is a
command, mostly `modify`, so it is checked by `Model::check`, rolled back when it breaks a rule and undone like any change.

### What the palette is about (`Subject`)

| Subject | When | Rows from |
|---|---|---|
| `Element(id)` | one element selected (or a view's own element: a section, a sheet, a callout) | `rows(category)` |
| `Elements(category)` | several selected, all one category | the same rows, values shared or blank |
| `Mixed(n)` | several categories | none; the filter row is a drop-down of the categories, one picked narrows what the palette shows and edits (`palette_filter`), the selection stays |
| `New(category)` | nothing selected, a tool in hand | none: the Type Selector picks the type the tool places |
| `Plan(level)` | nothing selected, a plan | `PLAN` (a level's own plan), `PLAN_VIEW` (a plan view), `PLAN_DEPENDENT`; then the view's graphics, underlay and phasing |
| `CeilingPlan`, `View3D`, `Schedule` | nothing selected in those views | their own extra panels (camera, section box, schedule fields) |

A group's member selected outside Edit Group of it is greyed apart from the parameters that vary by group instance. A family
document's Properties are its own (`family_properties`). Several views picked in the Project Browser show what they share
(`picked_views_properties`, the rows every one has).

### The Type Selector

A 50 point card: an icon of the category, the family (or category) name over the type's name, a drop-down arrow. Its list is
`Model::type_names(category)`; for a door or a window also each family's types as `Family: Type`; for a component every family's
types (the Fixture tool's kinds only, from a fixture tool). A wall's list shows each type's layers as a sample and sets curtain walls
under a heading of their own (`type_sections`). Disabled when the category has no types, or when a floor or ceiling follows a room (`follows_a_room`: its type is the room type's).
Picking applies `modify {ids, type}`, with `family` too for a family pair and `family: null` for a project type of a door or window.
With several elements it shows the type they share, else none. **Edit Type** opens the Type Properties dialog (`edit_type`): one
of `TypeDialog`, `FamilyDialog`, or the dialogs of text, dimension, spot coordinate, grid and filled-region types.

### Groups and rows

A row is `(group, key, label, Kind)` (`type Row`): the group it is under, the field as `elements` names it (a field of a field after
a dot: `view_range.cut`, `values.Opening Area`, `extrusion.from`), its label, and the editor. The groups are: **Constraints**,
**Dimensions**, **Identity Data**, **Structural**, **Graphics**, **Text**, **Camera**, **Extents**, **Quantities**, **Other**, then
per project parameter and per family the groups those name (`Under`), and **Electrical**, **Title Block**, **Tied To**, **Phasing**.
A group header is a bar that folds; the folded set is by the group's name, shared by every element (`collapsed`).

The table, by category (`rows`, about 400 lines): wall, door, window, room, floor, ceiling, shaft, mass, level, propertyline, grid,
dimension, text, spot, spotcoordinate, line, region, detailgroup, modelgroup, repeatingdetail, schedule, view3d, tag, sheet,
viewport, component, column and structuralcolumn, rebar, footing, beam and stripfooting, every run category (`RUN_CATEGORIES`),
stair, ramp, railing, roof, section and elevation, detail, pointcloud, cadimport, image, link. A category not in the table shows no
rows. A wall has 19, a floor 15.

### The editor kinds (`enum Kind`)

| Kind | What it is | Shown as | Read back |
|---|---|---|---|
| `Text` | text | the string | the string |
| `Number` | a number, no unit (an angle, a scale) | to the hundredth | `f64`, finite |
| `Angle` | degrees | `12°` | the number, a trailing `°` allowed |
| `Length`, `LengthIf`, `Height` | a length in the project's units; `LengthIf` has no row when the element has none; `Height` is typed only while the top is unconnected | `Units::with_unit` | `Units::parse` (`10' 6 1/2"`, `3 m`, a bare number in the project's unit) |
| `Limit(hint)` | a length or nothing; the hint names blank (`Same as start`, `No Clip`) | as a length, blank for none | blank is `null` |
| `NumberOr(hint)` | a number or nothing (`Automatic`) | | blank is `null` |
| `Point`, `PaperPoint`, `Outline` | `x, y` in project lengths; on a sheet a note's point is in paper mm; an outline is corners parted by `;`, blank for none | | split at `,` and `;` |
| `Flag` | yes or no | a check box | `bool` |
| `Crop` | cropped or not | a check box | the `crop` command, not `modify` |
| `Level`, `TopLevel`, `UpperLimit` | a level by name; `TopLevel` or none is Unconnected; `UpperLimit` or none is Not Bounded (and keeps a room's top where it was by setting `limit_offset` with it) | a drop-down of the levels | the name, or `null` |
| `LocationLine`, `Justification`, `Choice(&[(value, label)])` | one of a fixed list | a drop-down | the value the command names |
| `LineStyle`, `RegionLines` | one of the project's line styles (a region's also Invisible, which is `null`) | a drop-down | the name |
| `Framing` | None or one of the project's framing types, with a button for its Type Properties | a drop-down | the name or `null` |
| `Attached` | whether a wall's top or base is attached, and a link to what it is attached to | a greyed check box and a link | none (Attach and Detach change it) |
| `Link` | an element it points at (a door's host) | a link that selects it | none |
| `Shown`, `LengthShown`, `AreaShown` (m² in the model), `Mm2Shown`, `VolumeShown`, `MetresShown`, `Values` (a dimension's) | derived, read only | text, in the project's units (`Units::area`, `Units::volume`) | none |
| `LevelName`, `ViewName` | a level or view by name, read only | text | none |
| `Range` | a plan's view range | an Edit… button that opens the View Range dialog | none |

### Which rows show, and what a row turns into

- **Relevant** (`relevant`): a detail line's Center Mark only for an arc or circle, its Revision only for a cloud; a wall's Top Offset
  only with a top constraint; Opening Area only when the element has one; Slope not for an extrusion roof; a floor's Joist Direction
  only when framed.
- **No row for nothing**: a `LengthShown` or `LengthIf` that is `null` for one element (a straight wall's radius); an image shows page,
  paper and scale only for a PDF.
- **A kind rewritten by the element** (`parameters`): `Height` is `LengthShown` once the top is a level; a hosted component's
  `Offset` is shown, not typed; `Point` is `PaperPoint` for a note on a sheet; a floor or ceiling that follows a room shows its level and offset (the room's) and does not take them; a component of a work plane based family shows Schedule Level, Work Plane and Offset
  from Host, and one that is not shows Level (`work_plane_row`).
- **Missing numbers are 0** (`field_of`): the JSON leaves a number out at 0, so a length or number it lacks is 0; a `null` it has is none.
- **Several elements** (`Elements`): each row shows the value they all share, else blank; blank across a selection may be values that
  differ, so no hint is shown; an edit changes all of them, in one `modify` with `ids`.

### How a value is applied

`modify {ids, <field>: value}`, nested through the dots (`nested`: `view_range.cut` is `{view_range: {cut: v}}`, which `modify` merges
into an object field and removes on a `null`). Text and numbers commit on Enter or when the field loses the keyboard, and Escape cancels
(`field`). A value that does not parse says `<key>: <why>` in the status line and changes nothing; a refusal from the core says its reason there too.
Other commands for other things: `crop` (a view's crop), `panel` and `circuit` (an electrical board's and a circuit's fields),
`modify {params: {name: v}}` for a component's family parameters and a project parameter, `modify {title_block | fields}` for a sheet,
and `type` from the Type Properties dialog for a type's own values.

### What else the palette holds

The extra panels, each its own function in egui. **By data, not by rows**: layers of the selected type drawn to thickness (`layer_picture`,
a click sets a wall's location line), a floor's slope arrow and presets (`floor_slope`), the door or window family picker (`opening_picker`),
a sketch's slope and pick, a roof profile's coordinates, **Tied To** (a wall's openings, a roof's or floor's attached walls, the dimensions
holding an element), a stair's flights, landings and turns and a ramp's flights, a curtain wall's part panel, **a component's family
parameters** (by their groups: lengths, materials, yes or no, text, formulas shown), a board's **Electrical** group, a circuit's,
**the project's parameters** for the category (`project_parameters`: a field each, a check box for yes or no, areas and lengths with units,
`vary_by_group`, `read_only`), a sheet's **Title Block**, **Phasing** and a view's phasing, **Graphics** (scale, detail level, discipline,
view template, V/G overrides: `graphics.rs`), **Underlay**, a 3D view's **Camera** and **Section Box**, a schedule's Edit… buttons, a
site point's own panel. Type Properties (everything under `TypeDialog`, lines 1133 to 1610 and 3791 to 4012) is the Type Properties
dialog, which is the dialogs session's work.

## 2. The design in Rune

egui's palette is one 4,900 line file because each panel above is a function of the palette with drawing and logic mixed. Rune keeps
the logic in `crates/ui-rune/src/props/` as pure code, tested without a window, and draws what it says in `ui/properties/`.

```
props.rs            the module: what the rest says, a pointer to each part
props/kind.rs       Kind, field_of, relevant, nested, shown, parse            (units: Units::length, area, volume, parse)
props/table.rs      rows(category): the table, a row for each parameter of each category, the views' rows
props/names.rs      category names (one and several) and group names, through lang
props/panel.rs      Panel::of(model, ids): the groups, rows and editors a selection has, values shared or blank
props/apply.rs      what an edit of a row becomes: a command and its params, or why it is none
props/types.rs      the Type Selector's list (slice 3)
app/properties.rs   App's slice: the panel, the text being typed, what is folded, what choice is open, the seams
ui/properties.rs    the palette: title, filter, type selector, groups   ui/properties/*.rs  a row's editor, the selector
```

- **Generalised by field kind and by category data.** A new category costs a row in `table.rs`. A new kind of editor is a `Kind` and
  an arm in `panel.rs` (what it is as an editor) and `apply.rs` (what it makes of an edit), and nothing in the UI unless it is a new widget.
- **The panel is a value.** `Panel::of` returns `groups: Vec<Group>`, each with `rows: Vec<Row>`, a row with its label, its text as shown
  in the project's units and its `Editor` (`Read`, `Text`, `Check`, `Choice` with each option's label and the value `modify` takes,
  `Link`, `Seam`). The UI draws it and sends back the row's key and what was done (`Edit`); `apply::command` is the only thing that makes
  params. So the whole palette, for every element of the sample house and the tower, is tested without a window.
- **Units**: lengths and areas are shown with `Units::with_unit` and `Units::area`, typed with `Units::parse`; files and commands stay in mm.
- **Never crash**: a hostile element's JSON (a string where a number is, an array of thousands, NaN as text) gives a blank value or a
  short row, never a panic (a test sweeps every element of both samples and a set of hostile values); counts and lengths are capped
  (`MAX_IDS`, `MAX_TEXT`, `MAX_OPTIONS`).
- **Where egui hardcodes what the core could say**: the rows table is in this crate (data, a row per parameter, marked with `msg!` for the
  catalog tool), the category names too (a copy of egui's `lang::category`, marked in `names.rs` as a duplicate to fold into the core). Nothing
  here needed a core change yet.

## 3. What is built

**Slice 1 and 2, the palette for an element (this branch).** `props/` and `ui/properties/`:

- `Panel::of(model, selection, filter, level)` is the whole palette as a value. One element, several of one category (a value they share
  shows, the rest are blank, an edit changes all of them in one `modify`), a mixed selection (the categories it holds, each a button
  that narrows what is shown and edited, the selection staying), and with nothing selected the plan of the level (egui's `PLAN` rows: name,
  associated level, level elevation, crop, section box, view range). Past `MAX_IDS` (1,000) elements it counts and lists no rows.
- Every row of egui's table is carried over (`table.rs` is a copy of `rows`, `PLAN`, `PLAN_VIEW`, `PLAN_DEPENDENT`), with the same rules for which
  row shows (`relevant`, no row for a length an element lacks, a hosted component's offset and a floor's that follows a room shown and not typed,
  work plane rows) and the same kinds. Lengths, points and outlines are shown with `Units::with_unit` and typed with `Units::parse`; areas and
  volumes shown with `Units::area` and `Units::volume`; commands and files stay in mm.
- The editors: read-only text, a field (Enter commits; a value typed over with itself is no edit), a check box, a list opened in place under its
  row (no popup), a link that selects what it points at, and a button for a dialog this window does not have (`seam`: View Range). A refusal, from the
  value typed or from the core, is said under the title of the palette and in the status bar, and changes nothing.
- Tests: `props/tests.rs` sweeps every category of the sample house and of a two-storey tower with its drawings (one element, three of a
  category, a field for each row typed over with itself, a check set the other way, every option of every list), and carries an edit through
  `Session::execute` and reads it back; `ui/properties/tests.rs` drives the window.

**Not built from egui's palette yet** (each is a row of the same shape, a kind, or an extra panel): the extra panels of the inventory above
(layers picture, floor slope presets, opening picker, sketch, ties, stair and ramp flights, curtain part panel, component family parameters,
the electrical group of a board and a circuit, the project's parameters, a sheet's title block, phasing, camera, section box, underlay,
schedule fields, a site point's panel), the greyed rows of a group's member, several views picked in the browser, a field committing when it loses
the keyboard (the kit's `TextInput` gives Enter only), the Framing drop-down's Type Properties button, `Apply` (a field commits on Enter, as in egui),
and the layer sample beside a wall type. The Revit shortcuts wait for the shared registry.

## Round 2: the Options Bar

What a tool in hand lets a person set before the click, so a tool no longer runs on the command's defaults or the first type there is
(egui: `lib.rs` `options`, `canvas.rs` `wall_params` and `run_params`).

- **The data** (`options.rs`, tested without a window): `controls(tool)` is the bar of each tool as a table (`Height`, `Field`, `List`, `Check`);
  `Settings` holds what was set (the top a height goes up to, the typed fields by key, the location line, the justification, the stair line, Chain,
  Gable, and the type picked for each category); `Settings::feed(tool, model, level, params)` joins it to the params the machine made (`tooling.rs`
  is not touched): the type (not over one the machine named, and a family tool takes a family only of its kinds), `top_level` or an unconnected
  `height`, `location`, `justify` and `offset`, `slope`, `overhang`, `gable`, `width`, `line`. A field that is no length, or a slope outside
  0 to 90 degrees, is refused with its text in the message and the click is given back. `items(tool, ...)` is what the bar shows, in the project's
  units; a height list ends with Unconnected, and its field shows only then. Fields default as egui's do (3000, 2600, 500, 30 degrees, the stair's
  1000), written in the project's units.
- **The App slice** (`app/options.rs`): `Bar` holds the settings, a `TextEdit` for each typed field of the tool in hand (made when the tool changes,
  what was typed kept in the settings between tools), and which list is open. `drive` calls `feed_options` on what a click makes, which also turns
  a chain the bar switched off into one wall per pair of clicks.
- **The Type Selector for a tool**: with nothing selected and a tool in hand the palette is about what it places (`Subject::New(category)`, title
  "New Wall"): the selector lists the category's types and a door's or window's family types too, with the pick (or the project's usual type) as the
  current one. Picking one stores it in the settings (`App::pick_type`), not as a change to an element. Selecting something brings the palette
  back to the selection; putting the tool down, to the plan.
- **The widget** (`ui/options_bar.rs`): a row pinned over the plan's top left (under the shape bar), with lists that open under it, fields,
  and Chain or Gable.

## Round 2: the typed length and angle

With a tool in hand and a point picked, digits typed over the plan are a length from that point, Tab goes on to the angle (degrees
counter-clockwise from east, a minus allowed) and Enter picks the point they make, toward the pointer when no angle is typed and as far as
the pointer when no length is (egui's `typed_length` and `drawing.other`).

- **`reading.rs`** (pure, tested without a window): `Typed` (the two fields and which Tab is on; a field starts with a digit or a point, an angle
  also a minus, and once it has something takes any character, so a unit, feet and inches or a fraction go to `Units::parse`; 64 characters at most),
  `point(base, cursor, typed, units)` (a length is above nothing and within `MAX_COORD`, an angle within a turn, the point finite and in
  range, each refusal naming what was typed) and `reading(typed)`, the line the status bar shows while something is typed.
- **`app/reading.rs`**: `type_text` (from the window's `Key::Text`), `typed_tab`, `typed_backspace`, `typed_escape` and `place_typed`, which pushes the
  point and goes through `tool_took_point` as a click there would, so the Options Bar's settings, the core's refusals and a chain all apply as for a
  click. A click, a change of tool and Escape spend what was typed; Backspace takes a typed character before it takes the last point.
- **Which tools**: a wall with its Line shape (a shape's own numbers are in the next section), a run, a stair or ramp's
  second point, a path, Move, Copy and Mirror. Rotate takes the number as an angle about its picked centre: Enter makes the two rays the machine
  turns between and the rotation is the core's `rotate`.

## Round 3: the tools' leftovers

- **A shape's own numbers** (`reading::numbers`, `shape_reading`; `App::numbers`, `place_numbers`): with the Wall tool's shape in hand and its first points
  picked, the digits typed are what the core's `draw::numbers` names for that shape: a rectangle's width and height (Tab goes from one to the other), a circle's or
  a polygon's radius, an arc's radius, a centre-ends arc's angle in degrees. Enter asks `draw::typed` for the next point those numbers make, on the pointer's side, and
  picks it as a click there would; a number that is none is refused with what was typed, and one that cannot be drawn (an arc's radius under half its chord, an
  angle of 180 degrees or more) says so and keeps the points. The status line reads them by name (`Width 3000 | Height _`). A shape with one number has no
  second field, so Tab is not its key.
- **Fillet Arc** (`tooling::fillet`, `fillet_radius_at`, `round`; `Held::Pair`): pick a straight wall, then a second; the click that sizes the arc is the third,
  and the radius is the one whose arc passes nearest it (the core's `curves::fillet_radius`, from where the two lines meet). The radius the Draw panel's shape options
  set would make it at the second pick; the shape options are not on the Options Bar yet, so today the click sizes it, or a number typed once the two walls are held
  (Enter makes `fillet {ids, radius}`; the digits are spent either way, and a refusal, an arc that does not fit, leaves the walls held to be sized again). The ghost is the arc
  and the two walls cut back to it, at the radius typed or the pointer's. egui's detail-line and sketch-line fillets are not here: the Wall tool is the only one
  with shapes.
- **Tangent End Arc** (`tooling::tangent_at`, `Ctx::tangent`, `Tooling::tangent`): the first point must be the end of a wall of the level or a detail line (or the start of one,
  which it leaves away from); the arc leaves the way that line ran. Refused with a message when no line ends there. The end clicked starts the next arc (`Chain::Last`),
  as egui chains it, and the Options Bar's Chain turns that off. The tangent is found when a point is picked and kept for the ghost, so no move of the pointer scans the model.
- **What stands before it is made** (`stand.rs`, pure; `App::stands`, `shape_preview`): a wall's ghost is its outline, the two faces and the ends, as thick as the type the
  Type Selector picked (else the project's usual one) and on the side the Options Bar's location line puts them (`walls::depth`). A door or a window is the element the
  core would make in the wall under the pointer (`opening_of`, fed by the same `Settings::feed` as the click: centred, hinged, facing, flipped by Space), and a
  column is one of the type picked; both are drawn as the plan draws them (`render::sketch`) and lit in the accent colour (`paint::lit`, layer `plan-stands`). Over no wall a door
  has no ghost. Not here: a structural column's, a footing's, a beam's and a component's ghost, and the dimensions egui writes beside them.
- **The shape bar** is two rows now (eleven buttons side by side are wider than the plan), and the Options Bar sits under it.

