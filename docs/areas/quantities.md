# Quantities, steel and cost

What the project's structure comes to: concrete, formwork and steel (`quantities.rs`, `commands/quantities.rs`; tests in
`quantities_tests.rs`), the steel list and the cut plan (`steel.rs`, `commands/bars.rs`; tests in `steel_tests.rs`; the desktop's dialog is
`ui-egui/src/steel.rs`). Cost is planned next and will be written here.

## Areas of walls, floors, ceilings and roofs

Nothing here is stored either: `Model::quantities` works out a planar element's areas and volume when its JSON is made, so
its `elements` keys, its schedule's fields, the IFC quantities and `export_crosscheck` follow one rule. A wall measures one
face; a floor, a ceiling and a roof measure their surface, by the slope of each piece, and a floor or a roof also reports
its plan projection.

| Key | Meaning |
|---|---|
| `area_m2` | net surface: a wall's one face, a floor's, ceiling's or roof's surface by its slope |
| `openings_area_m2` | what its openings take from that surface: a wall's doors, windows and the voids their families cut; a slab's or roof's own openings and the shafts through it |
| `plan_area_m2` | a floor's or a roof's net plan projection; a flat floor's equals its `area_m2` |
| `volume_m3` | net volume, less openings and shafts |

A wall's net side is its net volume over its thickness (`walls::prisms`, `walls::prism_volume`), so an arched window cuts
its arc, not its bounding rectangle; a wall of no known thickness has no area (null, not 0) and `Model::warnings` says
which wall and why. A ceiling is flat, so its surface and plan are the same. A roof whose faces can't be found has no
area, and a warning says so. The gross areas are not exposed: they are only what the two numbers above are worked out
from.

## Changed in this version

`Area` now always means net surface, not a mix of gross and plan:

- a wall's `Area` in a schedule and `elements.area_m2` are new: length by height less its doors, windows and family voids;
- a roof's `elements.area_m2` is its surface by slope; its former plan is `plan_area_m2` (and `Plan Area` in a schedule);
- a sloped floor's `Area` is its surface; `Plan Area` is the projection;
- a roof's `Plan Area` is now net of its openings and the shafts through it;
- a wall, ceiling or roof now has a net `Volume`, and the multi-category schedule shows it;
- `Openings Area` is new for all four, and in a multi-category schedule.

## Concrete, formwork and steel

Nothing here is stored either. A member's `volume_m3`, `formwork_m2`, `rebar_kg` and `rebar_ratio` are worked out when its JSON is made
(`quantities::take`, one function), so a schedule's fields, the `quantities` command and the IFC's `NetVolume` follow one rule and a changed
member changes them. They are this project's own conventions, not Eberick's or any program's: where a rule is a shortcut it says so with a
`shortcut:` comment, and a number an engineer must match to another program is theirs to check. A column and a floor are found once for one
look at many members (`Rooms::quantities`, built the first time a member is asked about). Millimetres in, m³, m² and kg out, to a thousandth,
a hundredth and a hundredth; the ratio is kg per m³, null where there is no concrete.

| Rule | What it does | Test (`quantities_tests.rs`) |
|---|---|---|
| Section | width by depth; a round member is a circle; a concrete profile (T, L, I) is its profile's area and perimeter; a footing is its section by its `height`. A steel member (a type of metal, or of no material with a profile) has none of these fields: its `Weight` covers it | `a_beam_with_nothing_around_it_is_its_section_by_its_length`, `a_concrete_t_beam_is_counted_by_its_profile_area`, `a_steel_beam_has_no_concrete_volume` |
| Clear beam length | a beam is counted along its axis less where it lies inside the outline of a concrete column whose height meets its own, so from face to face | `a_beam_between_two_columns_is_counted_to_their_faces`, `a_column_that_does_not_reach_the_beam_takes_nothing_off` |
| Floor over a beam | a rectangular beam is formed on its bottom and two sides. Where a side runs inside a structural floor, the floor and the beam overlap there (their planes, so slopes are right): that height times the stretch is forms the floor does away with, and half the beam's width by it is concrete the floor holds. A slab at a beam's edge takes half | `a_slab_flush_with_a_beams_top_takes_its_thickness_from_the_beam`, `a_slab_on_one_side_of_a_beam_takes_half`, `a_floor_that_is_not_structural_takes_nothing_off_a_beam` |
| Floor over a column | a column's height is less what the structural floors over its centre hold between their undersides and their tops; forms are its perimeter by that height. A beam meeting its head is not taken off | `a_column_under_a_slab_loses_the_slabs_thickness`, `a_column_outside_the_slab_is_whole` |
| Footings | an isolated footing is formed on its sides only (cast on the soil); a strip footing on its two sides along its whole axis, its ends not formed | `a_footing_is_formed_on_its_sides_only`, `a_strip_footing_is_formed_on_its_two_sides` |
| Floor | the volume is `Model::floor_volume` (the slab whole and continuous, less openings and shafts). A structural floor's forms are its soffit, in true area, less the shafts through it and less where a column or beam that carries it lies, plus its free edges (outline and openings, at its depth, less what lies inside a carrying member). A floor that is not structural has its volume and no forms | `a_floors_forms_are_its_soffit_and_its_free_edges`, `a_floors_soffit_is_net_of_the_beams_and_columns_under_it`, `a_floors_edge_is_not_formed_where_a_beam_lies_under_it`, `a_sloped_floors_forms_are_its_true_area`, `a_floor_that_is_not_structure_has_a_volume_and_no_forms` |
| Steel | the weight of the bars that name the member as their host, summed as the steel list sums them (`Bars::weight`); the ratio is that over the member's volume | `steel_per_cubic_metre_is_bar_weight_over_host_volume`, `a_member_with_no_bars_has_no_steel_and_a_ratio_of_nothing`, `a_beam_schedule_shows_the_rebar_and_its_ratio` |

What the rules leave out, each a `shortcut:` in the code: a round or profiled beam loses nothing to a floor over it; two beams that cross are
counted twice at the crossing; a beam that leans to or from a column is cut at the column's outline in plan; the beams meeting a column's head
are not taken off its forms; the edges of a shaft through a floor are not formed; an edge is measured in plan at the floor's depth; a column
counts in its base level. A type with a profile and no material is steel until materials carry a density.

`quantities {level?, ids?, by?, categories?, path?}` (read only) sums them by level in elevation order and by category, splits each by the
value of the project parameter `by` (up to `MAX_GROUPS`, 1,000, refused naming the cap), and gives a line of its own to each level's bars that
name no host, so the project's steel is the steel list's (`the_quantities_command_sums_the_members_of_each_level_and_says_its_rules`,
`a_bar_with_no_host_is_in_its_levels_totals_once`). Its notes say the rules in a sentence and what was left out. `path`
writes the rows as .csv or .xlsx. `schedule {template: "materials_summary", level?}` makes the same, without the split, as a schedule of the project
(`a_materials_summary_adds_up_to_the_elements_of_its_level`; a schedule of several categories has no project parameter among its fields, so it
says to use `quantities` for `by`: `the_materials_summary_says_that_quantities_splits_by_a_parameter`). The IFC writes a beam's, column's, footing's and strip footing's `NetVolume`
from the same function (`ifc::tests::the_ifc_quantities_equal_the_schedule`); the formwork and the rebar weight are not in the file yet.
`cargo test -p bimcraft-core quantities_cost -- --ignored --nocapture` prints the cost for each member at two sizes of the tower.

## Marks and shapes

Nothing here is stored. A mark, a shape and a list are worked out from the bars when asked, so a changed bar changes them and a
file means the same thing to every version (there is no FORMAT step for it).

- **Shape** (`steel::shape`): the legs of a bar (the lengths between its points, mm), the angle it turns after each leg and the
  angle between the plane of each pair of legs and the next (`twists`), which tells a U from a Z of the same legs and a bend out of
  the plane from one in it. A hoop is a bar whose last point is its first, and has one more turn and twist, back to its first leg.
  `Shape::key` is the canonical form two bars share when they are the same bar: lengths to 1 mm, angles to 1 degree, an open bar read
  from either end, a hoop from any corner either way round (a hoop of more than 64 legs only from its first corner), turns and
  twists unsigned. `Shape::label` says the legs (`500 x 300`, `closed 140 x 440 x 140 x 440`); there is no letter code for a shape,
  because a code is a guess at what the engineer calls a bar.
- **Mark**: bars of one grade, one diameter and one shape key share a mark inside their host (a bar with no host, among the bars of
  its level): `N1`, `N2` and so on (`steel::PREFIX` is the `N`). They are numbered by the lowest layer among a mark's bars
  (`bottom`, `bent`, `top`, `skin`, `bars`, `stirrup`, `tie`, any other layer by name, bars drawn by hand last), then the thickest
  first, then the longest first, so reinforcing a host again gives the same marks. The rule is ours, not Eberick's: whether a
  detailer numbers per element or per floor, and whether it reads N1 or 1, is for the engineer to say.
- **Where a bar shows them**: its `element_json` has `mark`, `host_mark` (the host's `Mark` parameter), `shape` and `shape_label`;
  the `rebar` schedule has the fields Mark, Host Mark and Shape; a bar's tag shows its mark unless it names another field
  (`Tag::usual("rebar")`). One look at many bars (a schedule, `elements`, a tag) shares one index of the bars by host through
  `Rooms::takeoff`, built by a single pass over the elements the first time a bar is asked about.

## The steel list and the cut plan

- `schedule {template: "steel_list" | "steel_summary", host?, level?, show?}` makes a schedule of the project's bars, grouped: the
  list by host and mark (Host Mark, Mark, Diameter, Grade, Shape, Count, Length, Total Length, Weight), the summary by level, grade
  and diameter. `host` and `level` keep one host's or one level's bars and put its id or name in the default name; `show` is
  `count`, `weight` or `both` and only takes a column out. The call is the same one the `schedule` command always takes, with its
  fields, filters and sort filled in, so the result is the project's own schedule, editable like any other.
- `steel_list {level?, ids?, coil_to?, stock_length?, path?}` and `cut_plan {stock_length, level?, ids?, kerf?, min_offcut?,
  coil_to?, path?}` read the bars and change nothing. A list has a row for each host and mark, a summary for each grade and
  diameter, and notes. `coil_to` is the thickest diameter that comes off a coil: such a diameter is listed by weight and count and
  is not planned in stock bars. The cut plan packs each grade and diameter on its own by best fit decreasing (longest piece first,
  into the stock bar whose free length is the smallest that holds it, ties by the lowest bar and bar id), so it is the same plan on
  every run and it claims no optimum. A piece costs its length and one cut (`kerf`), a stock bar holds its length and one cut. A
  leftover of at least `min_offcut` is reusable and not counted as waste. A piece longer than the stock bar is a note and is not
  planned: a lap is design. There is no default `stock_length`, because it is a purchase.
- The caps are named in `steel.rs` (`MAX_ROWS`, `MAX_PIECES`, `MAX_PATTERNS`, `MAX_LABEL_LEGS`); the lists are refused, naming the
  cap, rather than cut. `path` writes a .csv, or an .xlsx workbook for a path ending .xlsx.
- Weights are the circle's area at 7850 kg/m³ (`rebar::weight_kg`). A list that must equal a supplier's invoice needs NBR 7480's
  nominal masses, which are not in the project.

## On the desktop

`ui-egui/src/steel.rs`. The Structure tab's Quantities panel has Materials (QT), Steel List (BL) and Cut Plan (CU), and all three open one
dialog, Quantities, on its page. The dialog runs the page's command and shows what it returns: the level (or the whole project), only the
selected elements, and for each page its own fields. **Materials** (`quantities`) has Split by, a project parameter (or none), and shows a
line with the project's totals and a table of each level and category (and value) with its count, volume, formwork (in the project's area
unit), steel and kg/m³, then the notes that say the rules. **Steel List** (`steel_list`) has the diameter up to which bars come off a coil
and a stock length, **Cut Plan** (`cut_plan`) also the kerf and the shortest leftover worth keeping; the stock length is read in the
project's units, as the other lengths are. The page the dialog opens on is made at once, except a cut plan, which waits for Calculate and a
stock length and says so inside the dialog (a status box in the view's corner can lie over its buttons); moving to a page that has not been
made makes it. Export… writes the page's rows to a .csv or .xlsx file, and Create Schedules makes the Steel List and the Steel Summary (or
the Materials Summary, for the level chosen and not split, which a schedule cannot be) as schedules of the project in one undo step and opens the first. The dialog
draws the first 300 rows; the file has them all. A bar's Properties show its Mark, Host Mark, Length and Shape, and a beam's, column's,
structural column's, footing's, strip footing's and floor's a Quantities group (Volume, Formwork Area, Rebar Weight, Rebar Ratio), read
only. BL, CU and QT are aliases of the commands too, so a script or an agent writes them as well; the desktop answers them with the dialog
(`BimApp::shortcut`), which a listed shortcut must do.
