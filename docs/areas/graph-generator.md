# The graph building generator

`bimcraft-cli generate --archetype office|clinic|hospital --seed N (--area M2 | --area-sqft SQFT) --out DIR` (the `generate`
command, `crates/core/src/generate.rs`)
turns a brief into a run directory. Deterministic code owns the geometry and the pass/fail; nothing in it is asked of a model. The
spec is `specs/graph-building-generator.md`; the first shipped slices cover office, clinic and a larger hospital archetype.

## What a run writes

| File | What it holds |
|---|---|
| `program-graph.json` | the spaces the building must hold (kind, role, least and target area) and the access each needs |
| `topology-graph.json` | nodes (id, kind, role, gross `area_m2`) and edges: `door` (a room to the corridor) and `adjacent` (neighbours in a band) |
| `building-ir.json` | the placed building in millimetres: footprint, corridor, each space's rectangle, walls and openings |
| `script.bimcraft.txt` | the building-ir compiled to one command to a line, for `bimcraft-cli run --script` (no file path in it) |
| `report.json` | the checks (stable ids), warnings, capability notes, scores and each room's net area as BIMCraft measured it |
| `manifest.json` | generator version, archetype, integer seed, requested and gross area, each file's name and FNV-1a-64 hash, the topology hash |

Only `door` and `open` edges are access; `adjacent` and `near` are softer and no rule reads them. The same brief gives the same bytes:
whole millimetres, integer seeds, a splitmix `Rng` (`generate::Rng`), no maps with undefined order, no times or paths in a file.

## The layout

One storey, with a seeded layout family (`corridor_bar`, `central_commons`, `clustered_commons`, or `atrium_commons`), boundary shape
(`rectangle`, `l_shape`, `stepped_bar`, `t_shape`, or `bar_with_wings`) and corridor strategy (`double_loaded_corridor`,
`single_loaded_corridor`, or `offset_corridor`). The family, strategy and boundary shape are chosen from the seed and exported in the
manifest, program graph, topology, building graph and report. Corridor-bar plans keep the older long-bar proportion; open families
use a more compact aspect ratio and a wider shared circulation zone, with `atrium_commons` reserving the largest shared zone. The
program (`program.rs`) grows with the area and takes a few of its counts from the seed; the lobby is the first room with the way in on
the west wall; every room has one door to the shared/circulation zone. Rooms are dealt into the strategy's band(s), in an order the
seed shuffles, each as wide as its least area asks. Non-rectangular boundaries shorten, center, or step room bands against the
shared/circulation bar, and the footprint polygon is used for the exterior walls, floor command and slab boundary.

Gridlines come from the same geometry: footprint extents, corridor/shared-zone edges, boundary vertices and room/module breaks.
Vertical grids are numbered and horizontal grids are lettered, so larger or more subdivided plans carry denser coordination grids in
the script and exported drawings instead of only four outside grids.

## Door and window symbols

Each brief takes an `opening_symbol_profile` from its seed and archetype (`generate/openings.rs`): `classic_punched`,
`storefront_ribbon`, `service_secure` or `slot_clerestory`. It is exported in the manifest, program graph, topology graph, building
graph and report (the manifest and the report also count `opening_symbols`, doors and windows apart). A profile fixes the symbol of
each opening by the room's kind and role:

| Doors | Symbol | Made as |
|---|---|---|
| the way in (lobby, west wall) | `main_entry` | double or single leaf by profile, as wide as the corridor allows |
| an exit from a back-of-house room with no window | `back_door`, `service_exit` | one leaf, 1000 and 1200 wide; every building has at least one |
| a room to the corridor | `single_swing`, `double_swing`, `service`, `secure` | 900, 1600 (public and wide rooms), 1100, 1000 wide |

Windows are `punched`, `ribbon`, `storefront`, `clerestory` or `narrow_slot`, each with its own width, height and sill.
Every building-graph opening carries `role` (`main_entry`, `back_door`, `service_exit`, `room_access`, `window`), `symbol`, `type`,
`width_mm`, `height_mm`, `sill_mm`, `operation` and `flip_hand`. The script makes one `type` command for each symbol used, with the
width, height, sill and operation, then each `door` and `window` names its `type` (`LIFT-007` checks BIMCraft holds what the graph says). Only the
way in and the exits have no topology edge; a room's door is still the lift of its `door` edge, which carries its `opening_symbol`.
BIMCraft's plan draws a door's width and one swing arc, and a window's width: a double leaf, and a clerestory or a slot, differ in
the graphs, the IFC operation and the schedule, not in the picture.

## Refusals

A brief that can't be met writes nothing and the error starts with a code: `invalid_params`, `unknown_archetype`, `invalid_area`
(above 0, at most 5000 m2), `unsatisfiable_area` (the minimum program doesn't fit: the message says it is too small and the least area
that works), `unplaceable_graph` (a house no shell fits),  `rule_violation` (a check failed: a bug in the generator, not in the brief), `generation_failed`, `output_unwritable`.
The files are made only after every check has passed, and a file that can't be written takes the earlier ones away again. The
CLI prints a refusal as a JSON object, `{"error": CODE, "message": ...}`, on stderr.

## The checks

Before anything is written the script is run on a scratch session: every command must run, every room must be bounded (a `room`
reply with a `warning` fails `RT-002`) and `Model::check` must pass. `GR-001..006` and `PR-001` are the rules on the graph (reach, no
private to private, no clinical to clinical, no private to clinical, connected from the entry, the minimum program); `SC-` the
script; `RT-` what BIMCraft did with it; `GM-` the geometry. The code owns each predicate (`slice1-hard-rules/1`) over roles and edge
kinds; since Slice 11 the parameters (the role pairs, the minimum program's groups) are read from the packs in
`crates/core/data/graph-rules/` ([graph rule packs](rule-packs.md#graph-rule-packs-and-the-checker)), and `report.json` is byte for
byte what it was (its `capability_notes` still say "hard-coded"; they are part of the pinned bytes).

## Looking at the result

```sh
bimcraft-cli generate --archetype hospital --seed 10 --area 1800 --out /tmp/hospital-1
cd /tmp/hospital-1 && bimcraft-cli run --script script.bimcraft.txt --save model.bimcraft --save plan.png --save plan.dxf
bimcraft-cli generate --archetype office --seed 2 --area-sqft 10000 --out /tmp/office-10000sf
```

Open `plan.png`: named, numbered rooms with their areas, a door in each room's wall to the corridor, windows on the outside wall,
tags, three dimensions and a note. The sheet `A101` holds the plan (`--save` a `.pdf` for it).

## The topology extractor

`topology` (`topology.rs`; `bimcraft-cli topology --model FILE.bimcraft --out FILE.json`) is the reverse: a model's 2D graph from its
geometry alone, in the shape of `topology-graph.json` with `graph_kind: "extracted"`. Nodes are the enclosed rooms (id: the room
number if unique, else `room-<element id>`; boundary, holes, area, level). Every edge is found by sampling points either side of a
line and asking which room holds each (`Spaces::room_at`):

- `door`: the rooms either side of a door's host wall (at the wall's centre ± half its thickness + 150 mm), with the door's family
  (a typed door's type), type, `operation` and `width_mm` from `Model::opening_dims`. A room on one side only is an exterior door
  (`to: "exterior"`). `entry` is the room of the exterior door that is a swing door, then the widest, then the lowest id.
  A swing or double-swing door also carries `opens_into` (the room on its swing side: the wall's left, or the right with
  `flip_facing`; `exterior` where no room is) and `hinge` (`start`, or `end` with `flip_hand`), on its edge and its `exterior_doors`
  entry. Any other operation has neither key: absent, not null.
- `open`: rooms sharing at least `MIN_SHARED` (300 mm) of room separation line; a shorter touch (a separator's end grazing a third
  room) is no link, and a separator with a room on one side only gives nothing.
- `adjacent`: rooms sharing at least 300 mm of wall, with no door or open edge between them.
- `footprint`: the largest outline of the lowest level's bounding lines at the wall centres, dangling stubs trimmed, corners only.
- `diagnostics`, each with a `code`: `room_not_enclosed` and `room_shares_space` (the room is no node: no boundary is made up),
  `door_without_room`, `door_type_unknown`. Written even then. Nodes carry no kind: the house rule checker will need one.

It gives the same graph measured at the wall centres or faces, and the same bytes for the same model.

## The trace contract

`topology --expect FILE.json` (`topology/expect.rs`; `bimcraft-cli topology --model M --expect E [--out G]`) compares the extracted
graph with a traced plan's expected file, the same for any building type. The file is `rooms` (`{id, name?, kind}`; `id` is the room
number), `links` (`{from, to, via}`, an unordered pair; `via` is `open` or a door operation) and `entry` (`{room, via?}`), with
optional `building`, `review` and `source`. It is read strictly (unknown keys, a duplicate or unknown room, an unknown `via`, an empty
`kind` and an oversized file are errors that name the field). Compared: the room ids, the `door` and `open` edges that aren't exterior
(by pair and via), `entry.room`, and that `diagnostics` is empty. Not compared: kinds, `building`, `review`, `source`, `entry.via`.

The fields are named from the model's side, which reads backwards at first: `missing_links` (and `missing_rooms`) are in the model and
not in the expected file; `extra_links` (and `extra_rooms`) are in the file and not in the model. `entry` is null, or
`{expected, extracted}`. The command replies with these under `comparison` (with `agree`); a disagreement is still an `Ok` reply
there, but the CLI exits non-zero with `{"error": "topology_mismatch", ...six fields}` on stderr. `path` is written before the
comparison and never holds it, so `--out` gives the same bytes with or without `--expect`.

## House: graph first

`generate --archetype house` (`generate/house.rs`, `house/{sample,place,emit}.rs`) goes the other way round from the corridor
archetypes: a graph first, then geometry for it. It writes the same six files; `manifest.json` also has `shell`, and
`topology-graph.json` has no `adjacent` edges (the extractor finds them, and its comparison reads them only when the generated graph
has any).

- **The graph** (`sample.rs`) is sampled by the seed inside the house pack (`data/graph-rules/house.json`): living and kitchen, a
  dining room at 100 m2 and up, a hall joined to one of them (open or swing), one to three bedrooms by area, each with a closet
  (sliding or swing) and an en-suite bath it may have, a bath off the hall, a closet or laundry at the hall's end, a porch off a
  shared room (sliding or swing), and one exterior swing door into a living room or kitchen that is not the porch's. A dining room is
  joined only openly; a porch has room separation lines for its sides, so a house with a porch has an open edge, and a house with
  no open edge has no separation line. Every node id is the room's number in the project.
- **The shell** (`place.rs`) is a shared band (living, kitchen, dining, dealt along it by area) and a private band, a hall strip with
  a column of rooms each side and a closet or laundry at its end. The shell is where the private band lies against the shared one:
  `bar` (as wide), `l_shape`, `corner_notch` and `narrow` (flush with an end, by how much is left), `t_shape` (inset or overhanging
  both ends), `side_nub` (a third along) and `stepped` (past the end). The seed turns and mirrors the house last. `court`,
  `opposite_wings` and the other shapes of the first catalogue are not made yet.
- **A try** is a shell, whether the shared rooms lie in reverse, which side the columns swap to, a proportion and which end of the
  shared band the private band lies against, in a fixed order starting from the seed's shell. Every `door` and `open` edge must land
  on a wall the two rooms share, long enough for the door (the door's width and 400 mm); a try that cannot is skipped. A try that
  can is built in a scratch project, read back by the topology extractor and judged by the house rules (`graph_rules`); it is
  kept only when the extracted graph is the generated one (the same rooms, doors by pair, operation and width, open edges, entry,
  one exterior door, no diagnostics) and both graphs pass. If none is, `unplaceable_graph`.
- **Walls** come from the room rectangles: every edge is cut where another room's corner lies on it; a stretch with a room on one
  side only is exterior (300 mm), one between two rooms an interior wall (100 mm) unless the graph opens the pair, when it is a
  room separation line. Doors are the built-in `Single Door` and `Sliding Door` families (the sliding family gets a 1200 type), so
  a sliding door is drawn as one; a door sits at the middle of its shared run.
- **Swing** (`house/swing.rs`, `data/house-swing.json`): `swing::orient` finds each swing door's host wall in the layout and the room
  either side of it, then sets `flip_facing` (to open into the rule's room) and `flip_hand` (hinge `end`) from the traced house's
  rules: `SW-001` to `SW-004`, all `exploratory`, one for each kind pair (a sorted pair, `opens_into` a kind, `hinge`) and one for the
  exterior door. The exterior object applies to every exterior swing door whatever its room's kind: `opens_into: exterior` means it
  opens out, any other value (here `kitchen`) opens in, toward its own room; `either` is not enforced. A pair with no rule keeps the
  default (left side, `start`). The predicted fields go on the generated graph and `emit::compare` fails a try whose extracted
  fields differ. The checker (`rule_check`, house) appends one `SW-` check for each rule to the pack's own; a swing door without both
  fields is not judged, so graphs from before them still pass. The list in the file is `swing`, not `rules`, so it is not a pack.
  A door tag that a neighbour's swung leaf would hide under (its mask clips the leaf) is moved: `swing::tag_moves`, then a `tag` command
  with `ids` and `offset` before the closing `tag {}`; the door ids come from a first run of the script in the scratch project.
- **The area** is the centre-line gross, porch included, within 6% of the request (the shared band is sized once, then again for
  what the first try came to); `unsatisfiable_area` below 45 m2.
- **Above 160 m2 (`LARGE_AREA`) the house is of the Unit 1 family** (`done/fixtures/houses/unit1-house`, the second approved trace).
  `sample::large` draws every choice from salts of its own (`SALT + 4` to `+ 6`), and `place.rs` gates each change on `large`, so
  houses at 160 m2 and below are byte for byte what they were (scripts, topology graphs, building IR; the program file's hall range
  and the report's notes changed with the pack). The family: living, kitchen and dining always; an **entry hall** (`entry`, kind
  `hall`) that is the way in, the public rooms open to it along the shared band (its link to a neighbour is open, sometimes a swing
  door), and the **hall of the bedroom wing** opening into it (`hall`-`hall` is open, the only means the pack allows); a **garage**
  beside the entry hall by a swing door (always from 240 m2), a **deck** off living and dining (two sliding doors, or living alone)
  in about two houses in three, and **folding** doors (the `Folding Door` family, defined in the script by `emit::folding_family`:
  `operation: folding`, no `opens_into` or `hinge`) on bedroom closets and on the closet or laundry at the hall's end. Every bedroom
  has a closet, at most two have an en-suite bath, and from 215 m2 there are four bedrooms (the pack allows six). The entry hall's
  cell is pinned to the hall strip over it (500 mm of it to the west, 600 to the east) and the rooms either side take what is left;
  the garage is whatever width that leaves the shared band's end (16.5 to 44 m2 net). The deck is the porch mechanism with one or
  two rooms: a rectangle south of the band across the living|dining wall, closed off by room separation lines, each room with a run
  of at least 1800 mm for its door. A house of this family has no porch. The bedrooms grow with the area (net m2 plus 0.05 for each
  m2 over 160) and are at most 4 m wide; the private band's hall may be up to about 16 m2 net (the pack's hall maximum).
  What is *not* made: a garage vehicle door (the one exterior door stays the entry's), and shells other than the seven above.
- **Swing rules across two traces.** `swing::rules()` (the traced house's, `data/house-swing.json`) is what `swing::orient` sets doors
  by, so generated houses are unchanged. The checker judges by `swing::judged_rules()` instead: the same four rules with each half
  (`opens_into`, `hinge`) left to `either` where Unit 1 (`data/house-swing-unit1.json`, read off its script by the same definition)
  says something else. In practice SW-001 and SW-002 do not enforce the hinge (Unit 1 hangs its bedroom doors from both jambs, the
  bath's from the other jamb), and still enforce which side the door opens into; SW-003 (an exterior door opens in, hinged at the
  end) and SW-004 are as traced. The pairs only Unit 1 has (ensuite, garage) are recorded in its file as SW-005 and SW-006 and not
  judged yet. Generated houses hang their doors by the traced rules and so pass the judged ones.

## Studio office: graph first

`generate --archetype studio-office` is its own family, not an alias of `office`: the program is the approved trace
`done/fixtures/studio-office/traced-studio`, a reception, a corridor with a meeting room and a store off it, a passage of WCs, and
a large studio. `generate/studio.rs` samples the graph by the seed (`studio/sample.rs`: 2 to 5 WCs, 1 to 2 meeting rooms and stores
in a shuffled order, the corridor 1500, 2000 or 2400 mm wide, double-swing doors where a door is wide enough, a studio way out or
not), `studio/place.rs` finds geometry (the studio the south band, the core of the other rooms against it: `bar`, `flush_west`,
`flush_east` or `inset`, turned and mirrored by the seed; windows are Fixed Windows on the studio, reception and meeting
rooms' outside walls) and `studio/emit.rs` builds each try in a scratch project, reads it back with the topology extractor and
compares: rooms, doors (operation, width, side, jamb), the open passage-studio link, the ways out and the entry. The reception's
way in is the widest door and the first, so the extractor takes it for the entry. The least area is 150 m2; below it the brief is
refused as `unsatisfiable_area`. The generated graph, and the extracted one with the generated kinds, must pass the studio-office
pack ([rule packs](rule-packs.md#graph-rule-packs-and-the-checker)).

## Office pattern: graph first

`generate --archetype office-pattern` follows the pattern of `generated/office-variations-20261112/generate.py`: a lobby at one end
(the only way in from outside, a Double Door 1800 in the end wall), a corridor the length of the plan, rooms off one side or both
(private offices, conference rooms, restrooms, a pantry, on some seeds focus rooms in the north band) and an open office at the
far end, the full depth of the plan. `generate/pattern/place.rs` draws the dimensions from the seed and scales them by the area
(`draw`: depths, corridor, lobby and open office widths, chamfers), solves the length from the area (rounded to 100 mm, so the
gross area is within 5% of the request), and rejects a draw whose length to depth is outside 1.6 to 7, whose rooms won't make a
run (4 rooms of 3000 mm for a single-loaded plan), or whose chamfer cuts into a room; up to 12 draws are tried in a fixed order.
Bands are dealt in rooms of 3000 to 5200 mm; the plan is then forced to hold a restroom, a pantry, a conference room and a
private office. Ids are `lobby`, `corridor`, `open_office`, `private_office_1`, `conference_1`, … and names `Lobby`, `Corridor`,
`Open Office`, `Private Office 1`, …; the room number is the id. A chamfer cuts the lobby's south-west corner and the open
office's north-east one at 45 degrees (the shell is a polygon, one wall for each side; rooms keep their rectangles for picking
and door sides). The seed then turns and mirrors the plan; it is never sheared, so every coordinate stays an integer. Doors are
swing, or double-swing where the corridor (or a conference room 4200 mm wide) has the room; `flip_hand` is never set. Windows
are Fixed Windows centred in side rooms at least 3200 mm wide (not restrooms) and 1 to 3 on the open office's end wall.
`pattern/emit.rs` builds each try in a scratch project, reads it back with the topology extractor and compares it as the studio
does; the generated graph, and the extracted one with the generated kinds, must pass the office-pattern pack
([rule packs](rule-packs.md#the-office-pattern-pack)). The manifest adds `loaded` (single or double), `focus`, `chamfers`,
`quarter_turns`, `mirror` and `shear` (0). The least area is 170 m2 (at 160 m2 six seeds of 40 are refused); below it the brief
is refused as `unsatisfiable_area`, and a brief no draw fits as `unplaceable_graph`. The tests in `generate/pattern/tests.rs` hold
a port of the done check's geometry rules (the open office at the far end and deep, nothing past it, the lobby at the other end)
and run it on the plan read back from each of the 20 seeds of the done check.

## Medical day: graph first

`generate --archetype medical-day` follows `specs/medical-day-pattern-generator.md`: a department, not a building, so no exterior door
and `entry` is null. Bands across the plan: a window band of treatment bays (cubicles, stations, one or two washrooms, sometimes a
shell space) open to the patient corridor; the core (9 to 12 m deep) of blocks, each with a cross corridor on its west side (open to
the patient corridor, one door to the staff corridor); an office band over the blocks on some seeds; the staff corridor. An end
corridor closes the far end (a pair of doors from the patient corridor, open to the staff corridor); the start has nothing, a head
row (a waiting room open to the patient corridor) or a second end corridor. A treatment block (medication, clean and soiled utility
against the cross corridor, treatment rooms with a door to the patient corridor, staff work open to it, sometimes a second open to
the first) and an isolation block (a pair of doors to the patient corridor, an anteroom to the cross corridor, a washroom of its
own, storage, an alcove and a staff washroom) are always there; a service block (staff work, shells and plant rooms with doors to
the end corridor) is last when a seed has one. `generate/medical/place.rs` draws the dimensions (`draw`: bands, blocks, widths
solved from the area so the gross area is within 5%), makes every room a rectangle tiling the plan and derives walls and room
separation lines from the shared edges (`assemble`: an open link is a separator over its run and never a wall; a door sits at the
middle of its run with 400 mm clear each side, and a try whose run is too short is dropped); up to 24 draws are tried. The seed
turns and mirrors the plan, never shears it. Ids are `1-09NN` (corridors `C1-09NN`), as A112's. The script is layout only: `new`,
one `drawing_property` (`building_type` = `hospital`), `room_area`, `level`, walls, floor, door families, doors, separators, rooms.
`medical/emit.rs` builds each try in a scratch project and compares the extractor's graph with the generated one (the same doors,
the same open links, no exterior door, null entry, no diagnostics); both pass the medical-day pack
([rule packs](rule-packs.md#the-medical-day-pack)). The manifest adds `service`, `office_band`, `blocks` (an order string such as
`TIS`), `head_row`, `second_end_corridor`, `quarter_turns`, `mirror` and `shear` (0). The least area is 400 m2; below it the brief is
refused as `unsatisfiable_area`, and a brief no draw fits as `unplaceable_graph`. Windows, grids, dimensions, symbols and a title
block are later slices.

## Shape graph engine

`generate --archetype house --seed N --rooms R [--rooms-tol T] [--shape ID] [--interior aligned|jogged|mixed] [--bumps on|off]` (spec: `specs/shape-graph-generator.md`,
Slice 1) is a second engine beside the `--area` ones. `--rooms` selects it; it has no area input (`--rooms` with an area is refused as
`invalid_params`, and so is any of its options without `--rooms`). The engine's code holds no room kind: what rooms a family has, their
counts, areas, sides, links and entry are a family file (`crates/core/data/shape-families/<archetype>.json`, compiled in, read strictly by
`shape_graph/family.rs` with the rooms' order kept, and refused as `invalid_family` when it widens the graph rule pack it names). The nine
outlines are `shapes.json` (corners, the depth levels of each side's pieces, raise ranges, the proportion each is recognised by).

A built-in family is any `crates/core/data/shape-families/<name>.json` other than `shapes.json` (not the `fill/` directory's files). `crates/core/build.rs`
compiles them in, sorted by name, into `family::BUILTIN`; `builtin`, `builtin_names`, `--archetype` with `--rooms`, and `deliver`'s rooms batches
all follow that table, and no code names a family. Adding a family is adding the file, whose `family` is its file name; taking one away is deleting it
(`shape_graph/register_tests.rs` keeps the code from naming one).

**Jogs and bumps** (Slice 3; `shape_graph/layout/interior.rs`). `--interior` is `aligned`, `jogged` or `mixed` (the default: the seed
chooses, and only a mixed call draws a number for it, so an aligned plan is what it was before the option); `--bumps` is `on` (the
default) or `off`. `deliver` and `generate` take the defaults from one place (`Request::new`). The cells the layout places are kept
(`Cell`), and after the trial `verify` the outline is the *base*. A jog sets a row cell's front back 300 to 1,200 mm, refitted shallower,
and the spine room it faced takes the strip; a jog is accepted only if the number of pairs a check would count (neighbours that share
300 mm of wall and face the same spine room from lines 300 to 1,200 mm apart, `jog_pairs`) rises, so the last count is the plan's. A bump
moves a cell's back wall 300 to 1,200 mm out (the ground and 600 mm round it empty) or in; it is accepted only when the outline minus
the base and the base minus the outline are exactly one rectangle per bump, each on one side (`bump_parts`), which rules out two inward
bumps side by side and an inward bump at a corner. Both run on the trial's throwaway random source for the links and checks, and use
the real one for their choices only when on. The manifest gains `interior` (resolved: `aligned` or `jogged`), `bumps` (`room` is the
room's id, `side` N/E/S/W after the mirror and turns, `offset_mm` positive outward) and `base_outline` (corner points in the plan's frame).

`--family-file FILE` (Slice 2; `family_file` in the command's params) reads the family from a file in place of `--archetype`: the two
exclude each other (`invalid_params`, as is neither) and `family_file` needs `--rooms`. The file is read up to 1 MiB (never trusting its
size: `/dev/zero` is cut off), parsed by `Family::parse` like a built-in family and checked against the pack it names (`invalid_family`);
a family with `pack_id: null` has no rule checks. An unreadable path is `invalid_params`. Every refusal comes before anything is written, and
the file's path reaches none of the six files: only the family's name does (`manifest.family`, `report.archetype`). A family file for
`house` gives the same six files, byte for byte, as `--archetype house`. Slice 2 found one engine bug the house never hit: `emit::orient`
skipped `double-swing` doors, so a generated double door lacked the `opens_into` and `hinge` the extractor reports; it now sets them.
Not yet generalised: `swing::rule_for` still keys its swing rules on house room kinds (a later slice).

`shape_graph/` is a port of the reference layout in `done/fixtures/shape-graph/reference_layout.py`: `plan.rs` (room program, parent graph,
clusters and groups), `fit.rs` (fitting a cluster in a cell, `share`, the attempt's random numbers: attempt `k` draws from the seed and `k`),
`layout.rs` (the profile along a spine, depths, lengths, rectangles, mirror and turn), `links.rs` (shared walls, links, the outside door,
`verify`: no overlap, one loop with the shape's corner count, every room inside its limits and reached), `emit.rs` (walls, doors, separators,
the script, the scratch run read back by the topology extractor, the graph and the pack's rules on both graphs). With no shape pinned the seed
orders the nine shapes and each gets 80 attempts; 400 attempts at most, then `unplaceable_shape` or `unplaceable_graph`. Three read-backs that
disagree with the plan also end it. Swing doors take their side and jamb from `house/swing.rs`'s rules by room kind, read from the family.

Two things a script of this engine does that others don't: its `new` carries a `seed`, which makes the project's uid and every GUID the
seed's, so a script saves the same file again; and its wall material has no solid cut pattern, because that lays paper over the wall lines
and a plan's DXF then has no `A-WALL` entities.
**Connection types** (Slice 4; `shape_graph/doors.rs`). All nine are drawn: `open_full` (a separation line the length of the shared edge),
`cased` (the pair's longest shared wall is two walls with a gap of 900 to 2400 mm, at least 600 mm shorter than the wall and leaving at least
300 mm of wall each side, and one `room_separator` across the gap; the pair's other shared edges stay solid; `links::measure` takes no wall
loss along the gap), `single`, `double`, `unequal_double`, `sliding`, `double_sliding`, `pocket` and `bifold`. A door's centre is anywhere
along its wall that leaves 150 mm of wall each side (`doors::door_centre`): a fraction per link is drawn at the end of `links::connect`,
after every type pick, so the picks' random stream is unchanged, and stored in `Linked::place` (the entry door's in `entry_at`). The script
defines the folding, Unequal Double (leaves two thirds and a third of the clear width, so its ARC radii differ by at least 200 mm), Double
Sliding (two bi-parting leaves on one track) and Pocket (a leaf on the wall's centre line, the pocket marked at one jamb) door families
(`doors::definition`); the others are BIMCraft's. A family file's `entry.types` must be door types.

**Room tags that do not fit** (Slice 14; `shape_graph/tags.rs`). A room's tag (name, number, area: `render::room_tag_box`, 0.6 H a character
wide and 4.5 H high, H the name's height, 260 mm at 1 : 100) is drawn in the room when it fits the room's cell shrunk by 150 mm on every
side. The cell is the largest rectangle of the room's outline (as the topology extractor reads it back from the scratch project) out to the wall
and separation centre lines, found as the done check finds it. A room that does not fit gets `tag: [x, y]` on its `room` command: the
three lines stand there, outside the plan, 1,000 mm or more clear of its outline on the side nearest the room, and one straight leader
(`render::room_tag_leader`) runs from the tag's nearest edge to the room's `point`, which the generator then sets to the centre of the
cell. Tags go in room-number order, each in the first place that overlaps no tag, whose leader crosses no tag and no leader, tried on the
rows 1,100 mm, then 1,100 + box + 300 mm, ... from the nearest side and sliding along it; if one has no place the other order is tried, and then
the attempt is given up (`Trouble::Plan`). `emit::verify` runs the script twice when any room has a tag: the first run measures the areas
the tag lines need, the second is the script as written. A plan with no leader is byte for byte what it was.

**Required links and the two ends** (Slice 6; `links::connect`, `layout::steer_target`). A `"use": "required"` link is made whenever both
room types are in the plan: `connect` joins each pair of rooms of its types (after the parent links, before the extra ones) and fails the
attempt when they share no wall long enough. When a required link joins the start end room (`entry_end`) and a room in the far end room's
cluster (`spine_end`), the plan is steered, whatever the family: the start end room always stands across the end of the bar, the far end
room stands alone on one side of the core as one cell the length of the core (`Mode::Span`; the side must be level 0 in its last piece and
in every piece but a raised first one, which the start end room covers), every other room goes to the other side, and the cluster is fitted
with a goal (`Cl::goal`, `Fitter::fit`): the room named must touch the cell's start-facing wall, so a child of the far end room (a passage)
reaches the start end room. The far end cluster's fronts are scanned to 40 m, not 14. A plan that is not steered draws no number more than it
did. The reference layout has no such rule; the spec's reference wins where they disagree, so this is an addition to ask the spec's owner
about. The studio-office family is the one that uses it (the reception to the passage, SO-011).

Every `--rooms` script has one `cad_profile {"name":"delivery"}` line, straight after `drawing_property`: its DXF and DWG keep the
delivery drawing rules ([CAD export](cad-import.md#export)). The other generators' scripts have none and export as before.

### Delivery rules for the existing generators

`generate --archetype A --area M2 --delivery on` (also `--area-sqft`; `off` is the default, and with `--rooms` it is `invalid_params`) draws the same
plan, walls, doors, windows and rooms as without it, to the delivery drawing rules (`generate/delivery.rs`, run after the generator's own build and
checks, so the default files keep their bytes). `delivery::apply` rewrites the script (one `cad_profile` line after `drawing_property`; no
`dimension`; each room numbered `101` upward in the order of the graph's nodes; doors and windows marked `D1`, `W1`…; a door of a type by `operation`
alone becomes a door of the family that draws that operation, Single, Double or Sliding, so its swing is one quarter-circle ARC, two, or none), puts each
node's `number` in `topology-graph.json`, and gives `manifest.json` `delivery: true` and a `building`. The graph's `entry` is the room the
saved model's entry is (the corridor generators name the lobby, while their main door opens into the corridor). In the DXF, gridline letters go on
`A-ANNO-GRID` (as TEXT) and a block keeps both its attributes with an empty `TYPE`, so the entities are those of the plain export; the
plain notes are TEXT without a typeface run. `deliver` takes batches by `area: [lo, hi]` (square metres): a seed's whole-number area is
`lo + mix(seed ^ AREA_SALT) % (hi - lo + 1)`, pinned by a test, and its plan is what `generate --delivery on` writes.

### Gridlines

`generate --gridlines on|off`, in both forms. With `--rooms` the default is `off`; `on` puts a `grid` line at each distinct x of the
outline's corners (numbered 1.. left to right) and at each distinct y (lettered A.. up), 1,500 mm past the bounding box. The corners come
from the exterior walls (`script::segment_corners`: a point where a horizontal and a vertical exterior segment meet, so the collinear joins
where an interior wall meets the shell are not corners). The lines go after the types and families, before the first wall, in
`emit::compile`; grids change neither the topology, nor `building`, nor the tags. The manifest has `gridlines`.

With `--area` the default is what the archetype draws (on, except medical-day: `Archetype::draws_grid`). A value equal to the default
touches nothing, so the default files stay byte for byte. A value that departs goes through `generate/gridlines.rs`, a post-pass run before
`delivery::apply`: `off` takes the `grid` lines out of the script, `on` (medical-day) puts the outline's grid after the `level` line from
the `floor` boundary (`script::ring_corners`). It runs the result in a scratch session, leaves the three graph files alone, adds a note to
`report.json` and re-hashes the manifest. Only then does the manifest gain `gridlines` (the default `--area` manifests are pinned and gain no key).

## Delivery folder

`bimcraft-cli deliver --brief BRIEF.json --delivery-id ID --rights (FILE | emailed) --out ROOT [--rerun N] [--jobs J]` (`generate/deliver.rs`)
writes `ROOT/<generator_version>/ID/` and `ID-rerun/`, each with `manifest.csv`, `RIGHTS.txt` (none with `emailed`) and `dwg/<archetype>_s<seed>.dwg`,
and prints one JSON summary (`folder`, `rerun_folder`, `generator_version`, `files`, `buildings`, `average_file_bytes`, `skipped`, `sources`).

- **Version.** `Session::generator_version` is `bimcraft-<release>+<commit>[.dirty]`, put into the CLI by `apps/bimcraft-cli/build.rs` (`git rev-parse`,
  `git status --porcelain --untracked-files=no`; `BIMCRAFT_COMMIT` for a build with no `.git`). Without it, or without a DWG writer, `deliver` refuses
  with `invalid_params`. Commit before the release build you deliver from, or the version says `.dirty`.
- **A seed** is built in memory (`shape_graph::build`), its script run in a fresh `Session` and saved as DWG, so its bytes are what `generate` then `run` write.
  A batch's `rooms: [lo, hi]` gives each seed `lo + mix(seed ^ SALT) % (hi - lo + 1)` rooms: the seed alone, pinned by a test, because past deliveries
  depend on it. A seed `build` refuses is in `skipped` with the refusal's code and has no row and no file. A batch by `area` makes the seed's plan with `build_for_delivery`; `sources` lists only the families of batches by `rooms`.
- **`building`** is `<family>-` and 12 hex digits of the SHA-256 of the script's `type`, `family`, `wall`, `door` (without `mark`), `room_separator` and
  `room` lines, each kind sorted: the same layout is the same building however the script numbered its doors, and `generate`'s `manifest.json` carries it.
- **Threads.** `--jobs` plain threads pull seeds from an atomic counter (the reproducible-GUID state is thread-local, so no rayon here); results go by position
  and the manifest is written when every seed is done, so the tree does not depend on J. DWGs stream to their files.
- **The rerun** repeats `k = min(N, rows)` seeds, index `i` being row `floor((2i+1)·rows/2k)`, made again from the brief; a different byte is `nondeterministic`.
- **Refusals** (`invalid_brief`, `missing_rights`, `delivery_exists`, `invalid_params`) are all checked before anything is created, and a failure later removes
  what the run made. The command can run for minutes, also through MCP.
- Not done by `deliver`: writing `RIGHTS.txt`, uploading, telling the receiver the counts.

## Not yet

No stairs, shafts, structure or MEP; one storey; no datacenters or airports;
no batch runs beyond `deliver`'s, and the rules are still hard-coded: the seed rule packs of [rule packs](rule-packs.md) are written but not read. `report.json` says so in `capability_notes`.
