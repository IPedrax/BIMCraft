# Quantities, steel and cost

What the project's structure comes to: the steel list and the cut plan so far (`steel.rs`, `commands/bars.rs`; tests in
`steel_tests.rs`). Quantities per floor (volume, formwork, rebar weight) and cost are planned next and will be written here.

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
- The shortcuts BL (Steel List) and CU (Cut Plan) and their dialog come after these commands: a shortcut is listed in KS and must do
  something in the window, so neither is an alias yet.
