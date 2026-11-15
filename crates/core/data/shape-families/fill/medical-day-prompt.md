You are Claude Fable 5.2. Write one proposal for the shape-graph family `medical-day`. Do not write product code.

Paths below are relative to the folder you start in, which is laid out like the `synthetic-drawings` repository: `specs/`, `done/` and the `BIMCraft/` checkout.

Write only this file:
BIMCraft/crates/core/data/shape-families/fill/medical-day-claude-fable-5-2.json

Its `model` field is `claude-fable-5-2`. The family file `medical-day.json` is written from your proposal later, by a builder that may not change your values. So the proposal has to be complete: every room type and every link the family needs, with every field.

Read first:
- specs/shape-graph-generator.md: "Family file" (the JSON example, the Fields list and the examples of family rules), "Connection types", "Rooms become clusters and groups", and "Slice 8: Medical Day Family"
- BIMCraft/crates/core/data/graph-rules/medical-day.json: the `kinds` table (each kind's role) and rules MD-001 to MD-011
- done/fixtures/medical-day/README.md, a112-expected-topology.json (44 rooms, 49 links), a112-kinds.json (each room's pack kind) and a112-room-areas.json (each room's measured area). This is the A112 trace, the one real plan
- done/fixtures/shape-graph/examples/medical-day-family.json: the family in the form the layout needs. Keep its structure. Its numbers are guesses and are what you replace
- BIMCraft/crates/core/data/shape-families/fill/hospital-claude-fable-5-2.json: the form of a proposal. Use the same top-level fields (`model`, `family`, `written`, `note`, `inputs`, `pack_id`, `entry`, `rooms`, `links`), the same room and link fields, the same `source` objects, and a `why` on every room type and every link
- BIMCraft/crates/core/data/shape-families/hospital.json: the family file that proposal became

The proposal:
- `model` `claude-fable-5-2`, `family` `medical-day`, `written` `2026-11-15`, `pack_id` `medical-day`, `building_type` `hospital`, `entry` null (a department cropped out of a building, with no outside door; MD-011)
- `inputs` lists the files you read, as the paths above
- every kind in the pack's `kinds` table has at least one room type, with the pack's role for that kind. That includes `shell`, which the example leaves out and A112 has
- each room type has `kind`, `name`, `role`, `place`, `count`, `area_m2`, `max_side_ratio`, `min_side_mm`, `max_links`, and `per` and `weight` where it needs them. Only a `spine` room has a null `max_side_ratio`
- corridors are the spine. There may be several
- every room except the spine rooms has exactly one parent link. A shaft's parent link has `"types": []` and its `max_links` is 0
- connection types are ids from the spec's connection table. A swing door in the trace (`swing`) is `single`; a pair (`double-swing`) is `double` or `unequal_double`; `open` is `open_full` or `cased`

What the rules need, measured on the engine with the example:
- MD-001: a treatment bay is open to a corridor and has no door. Its parent link from the corridor has open types only
- MD-008: staff work is open to its corridor, not joined by a door. The example gets this wrong
- MD-003: every isolation room has its anteroom, a washroom of its own, and a door to a corridor. The example has the first two. Add a link from `corridor` to `isolation` with `"use": "required"` and a door type, and give the isolation room `max_links` of at least 3
- with those two changes, 36 plans of 36 passed all eleven rules, from 20 to 45 rooms on all nine shapes

What the A112 trace has to fit:
- for each kind, the number of A112 rooms of that kind is inside the summed `count` range of that kind's room types
- each A112 room's area is inside the `area_m2` range of a room type of its kind
- each of the 49 links matches a link you list, by the two rooms' kinds and by means: `open`, or the operation of a door type
- A112 has 44 rooms. The family makes plans from 20 to 45 rooms: the fewest rooms a plan can have (the `count` minima added up) is 17 or fewer, and the most (the maxima added up) is 48 or more

Sources:
- every room type has a `source` object with one entry for each of `count`, `area_m2`, `max_side_ratio`, `min_side_mm` and `max_links`. Every link has one `source`
- an entry is `{"from": "trace"}` when the value is exactly what A112 holds, such as a link's connection seen in the trace. It is `{"from": "model", "confidence": "low"|"medium"|"high"}` when you chose it. One trace does not give a range, so a range is `model`, and it contains what A112 holds
- `why` says in a sentence or two what the value rests on, for the person who reviews the family

Names are generic words (`Treatment Bay`, `Clean Utility`). No room number and no room name copied from the trace.

Check your file with python3 before you stop: it parses, and the A112 trace fits it by the four tests above. Do not edit any other file. Do not run git. When the checks pass, stop and say what you were least sure of.
