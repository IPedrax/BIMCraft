# The rule pack seed library

`bimcraft-cli rule-pack seed --out DIR` (the `rule_pack` command, `crates/core/src/rulepack.rs`) writes the frontier-model seed
library: design rules for five building types (office, clinic, hospital, datacenter, airport) that three models proposed and an agent
curated. The spec is `specs/graph-building-generator.md`, slice 2. Nothing here is a code requirement and the generator does not read
it yet; it is the seed a later slice loads as rule packs.

## What a run writes

| File | What it holds |
|---|---|
| `rule-pack-manifest.json` | `schema_version`, `library_id` (`frontier-model-seed-library`), the models, each pack's path, counts and FNV-1a-64 hash, the report's hash |
| `validation-report.json` | the `RP-` checks (PASS or WARN: a FAIL writes nothing), their summary, and `capability_notes` saying what the library cannot vouch for |
| `packs/<archetype>.json` | `status: "seed"`, `origin: "frontier_model_seed"`, `models_consulted`, `prompt_hash`, the rules, the `conflicts` and a `validation` object of counts |

A rule is `id` (`<archetype>.<slug>`, unique across the library), `category`, `subject`, `predicate`, `object`, an optional `condition`,
`classification`, `checkability`, `confidence`, `rationale`, `exceptions` and `provenance` (`model_refs`, `source_proposals`). The
same files come out every time: the data is embedded in the binary, there are no maps of undefined order, and no time or path is written.

## Classification

Every rule is `exploratory` or `soft`, never `hard`: a model's opinion is not a requirement. A rule is `soft` only when a generator check
enforces it, and then it names that check as `deterministic_checker` (`KNOWN_CHECKERS`, today `GR-002`; a test runs the generator and
reads `report.json` to see that every named check is really run). At least 85% of a pack must stay exploratory (the gate's line is 80%).
`checkability` says how a rule could be checked (`deterministic`, `heuristic`, `human_review`), which is not the same thing: most
deterministic rules have no checker yet, and `RP-013` warns of the count.

## Where the rules came from

`crates/core/data/rule-packs/` holds, in the order they were made:

1. `seed-prompt.md`, the prompt given to each of three models, alone, with no sight of the others' work.
2. `proposals/claude-{opus,sonnet,haiku}.json`, what each model's run wrote to its own file; the curator did not edit them. Their keys are the
   proposal names: a reference is `claude-opus#airport/landside-airside-split`, model, archetype and key.
3. `seed-prompt-followup.md`, a second round to each model: the first round gave the hospital nothing on outpatient exam rooms,
   so each was asked for three to five more. They were appended to the model's own file, not written by the curator.
4. `<archetype>.json`, the curated seeds: an agent merged proposals that say the same thing, cited each of them in
   `source_proposals`, capped a rule's `confidence` at the highest of its sources, and kept the disagreements as `conflicts`.

The library is built from these files and nothing else. It does not call a model, so a build is the same any day.

**Provenance caveat.** The three "models" are all Anthropic models, asked by the aliases opus, sonnet and haiku in three separate
runs; the exact versions were not recorded, and the labels `claude-*` are the alias. Their agreement is therefore weak evidence:
they share a provider, probably training data and some of the same blind spots. The curator was an AI agent too. No rule was reviewed by
a person or checked against a published standard, and none has corpus evidence. `capability_notes` says all of this in every report.
`prompt_hash` is the FNV-1a-64 of the two prompts and the archetype: it detects a changed prompt, not a changed model.

## Conflicts

Where models disagreed the pack keeps both positions (`positions`, each a proposal ref and what it said), the rules concerned and a
`status`: `resolved_in_pack` with the reason, or `open` when both stay as separate rules. Examples: the airport's security predicate
(one model had it running the wrong way, and its own rationale disagreed), how strictly a hospital separates clean from dirty,
where a datacenter's operations centre sits. Open conflicts show as a `WARN` in `RP-011`.

## The checks

`RP-001` inputs parse strictly (`deny_unknown_fields`; a bad file is `invalid_seed`, naming the file) · `002` three models per pack ·
`003` size (8, 8, 10, 10, 10 rules) and topics (five words a pack must speak of) · `004` ids · `005` classification · `006` fields ·
`007` exploratory by default · `008` sources exist, for the pack's own archetype · `009` confidence capped · `010` conflicts name rules
and proposals that exist · `011` open conflicts (WARN) · `012` every model contributes (WARN) · `013` deterministic rules without a
checker (WARN). A failure refuses the library with `rule_violation` and the first failing check's id and why; nothing is written.
The other codes are `invalid_params` and `output_unwritable`; a file that can't be written takes the earlier ones away again, and the
manifest is written last.

## Looking at the result

```sh
bimcraft-cli rule-pack seed --out /tmp/rule-packs
jq '.checks[] | select(.status != "PASS")' /tmp/rule-packs/validation-report.json
jq '.rules[] | {id, classification, confidence}' /tmp/rule-packs/packs/airport.json
```

Run it twice into two directories and `diff -r` them: there is no difference. To change a rule, edit the curated seed and its
`source_proposals`; to change what models said, a new run of them is a new proposals file, which this slice does not make.

## Graph rule packs and the checker

Apart from the seed library above (a frontier model's proposals, which nothing reads), `crates/core/data/graph-rules/` holds the
packs a checker reads: `house.json`, `office.json`, `clinic.json`, `hospital.json` (`graph_rules.rs`, embedded, parsed once, strictly).
`bimcraft-cli rule-pack check --archetype A --graph G.json` (the `rule_check` command; `graph` may also be the graph itself) prints each
rule's PASS or FAIL and exits 0 when none fails; otherwise it exits 1 with `rule_violation` and the failed ids on stderr. The code owns
each rule's predicate, the pack owns its parameters (`check`).

- **The house pack** (`archetype: house`, `origin: authored_from_trace`, `HR-001` to `HR-013`, every rule exploratory). It was written from
  one reference plan, `done/fixtures/houses/traced-house`, and says so (`reference_plans`, `reference_plan_count: 1`): its kinds (the
  eleven of the spec and `library`, which the trace has) hold a role, a count range and an area range, widened from that plan's numbers;
  `connections` lists which kind pairs may be joined by `swing`, `sliding` or `open` (`double-swing` is swing, `double-sliding` is
  sliding, `fixed` is allowed nowhere, and `folding` only where the Unit 1 paragraph below says). The rules: HR-001 every node's kind is known · 002 edges name spaces ·
  003 counts · 004 areas · 005 connections · 006 a closet has one door and one neighbour · 007 a porch opens only into living, kitchen or
  dining · 008 every bedroom is reached from the entry without crossing a bedroom or a bath (no entry fails) · 009 the entry is a
  living, kitchen, dining or hall. The extractor gives a node no `kind`; whoever checks a traced graph adds it.
  After these the checker appends `SW-001` to `SW-004`, from `generate/data/house-swing.json` (not part of the pack, so HR ids stay
  the pack's): which room kind a swing door opens into and which jamb it hangs from, by the kind pair, with the exterior door judged
  by its own rule. Edges without `opens_into` and `hinge` are not judged. See [the graph generator](graph-generator.md).
- **The house pack has two sources** since the Unit 1 trace (`done/fixtures/houses/unit1-house`): `sources` names both traces, while
  `reference_plans` and `reference_plan_count: 1` still say what HR-001 to HR-009 were authored from (the slice-11 check wants the
  pack to say it has one reference plan; the two are kept apart on purpose). Unit 1 added the kinds `garage` (service, 0 to 1, 15 to
  45 m2) and `deck` (public, 0 to 1, 6 to 30 m2), widened `hall` to 16 m2, added the means `folding` (allowed between a bedroom or
  hall and a closet or laundry only; listed under `folding_connections`, not `connections`, because the slice-11 check reads
  `connections` for the three means of the first trace) and the connections garage-hall (swing), deck-living/dining/kitchen (sliding), hall-hall (open)
  and bedroom-closet open, and four exploratory rules, which a house without the room they judge passes: **HR-010** a garage is reached
  from the entry without crossing a bedroom or a bath and is next to a hall or the entry · **HR-011** a deck opens only into living,
  kitchen or dining, by at least one sliding door · **HR-012** a folding door joins a closet or a laundry to a bedroom or a hall (an
  exterior folding door, a garage's, joins no two spaces and is not judged) · **HR-013** every hall is reached from the entry without
  crossing a bedroom or a bath. **HR-006** now counts ways in, a door or an open edge, since Unit 1's walk-in robe is open to its
  bedroom (`check: {"kind": "closet", "links": 1}`). The SW- checks judge by both traces: where Unit 1 and the traced house disagree on
  a half of a swing rule (the hinge of SW-001 and SW-002) that half is not enforced.
- **The closet rule departs from the spec's wording.** The spec sends a closet's door "to a bedroom or a hall", but the traced entry closet
  opens off the kitchen. HR-006 counts only; which kinds a closet joins is the connection table's, which allows closet and kitchen by a
  sliding door because the trace does (and the rule's `exceptions` say so).
- **The older packs** hold `GR-001` to `GR-006` and `PR-001` (`origin: slice1_hard_rules`, `hard`, `approved_by` the spec's "Hard Rules For
  Slice 1"): the generator's own report reads its role pairs and minimum groups from them now (`generate/report.rs`, `graph_checks`), and a test pins
  the reports of sixteen briefs to the bytes they had when the rules were hard-coded. Whether the spec counts as Slice 2's explicit
  human approval is the reviewer's call. Their checker needs a `kind` and a `role` on each node and an entry, and refuses a graph without.
- **A pack that fails to load** makes `PACK-001` fail in a report; it never panics.

```sh
bimcraft-cli rule-pack check --archetype house --graph topology.json   # topology.json with kinds added
jq '.checks[] | select(.status == "FAIL")' <<< "$(bimcraft-cli rule-pack check --archetype house --graph g.json 2>&1)"
```

### The studio-office pack

`data/graph-rules/studio-office.json` (SO-001 to SO-012, all exploratory, from `done/fixtures/studio-office/traced-studio`): kinds
reception, hall, storage, meeting, bath and office, and rules for the means of each link (double-swing kept apart from swing),
the entry being the reception, services reached without crossing the studio, WCs off a passage that is open to the studio, the
corridor serving the meeting rooms and stores, the reception joining both halls, and the studio being the largest space. The
evaluators are in `graph_rules/studio.rs`; a passage is a hall with a WC off it, a corridor a hall without. Graphs need a `kind`
on each node and no `role`.

### The office-pattern pack

`data/graph-rules/office-pattern.json` (OP-001 to OP-012, all exploratory, from `generated/office-variations-20261112/generate.py`,
which no trace backs): kinds lobby, hall, private_office, conference, restroom, pantry, focus_room and open_office, one swing means
for every link (double-swing counts as swing here), and rules for the counts, the areas, exactly one way in from outside and on
the lobby (OP-006, counted by edges with `exterior`), the entry being the lobby, the lobby joining the hall, every side room and
every private office having a door (not an open link) to the hall (OP-009, OP-010), the open office joining the hall and neither
the lobby nor a side room (OP-011), and every space being reached from the entry. The evaluators are in `graph_rules/pattern.rs`.
Graphs need a `kind` on each node and no `role`. `generate --archetype office-pattern` ([graph generator](graph-generator.md#office-pattern-graph-first))
checks its graph and the extracted one against it.

### The medical-day pack

`data/graph-rules/medical-day.json` (MD-001 to MD-011, exploratory, from `specs/medical-day-pattern-generator.md`; the frozen A112
trace's 44 rooms, given kinds, pass all eleven): kinds treatment_bay, treatment_room, isolation, anteroom, washroom, medication,
clean_utility, soiled_utility, staff_work, office, meeting, storage, alcove, waiting, shell, plant, shaft and corridor. Bays open to a
corridor with no door; treatment rooms, offices and utilities with a door to a corridor; isolation with an anteroom and a washroom
of its own; clean and soiled apart; medication with one link; staff work open and doorless; the corridors one set; every room but a
shaft reaching a corridor; and no link with a room on one side only (MD-011, from `exterior` edges, edges naming a node the graph
lacks, and a top-level `exterior_doors` list). The evaluators are in `graph_rules/medical.rs` and follow the done check's own
literally: links count one by one, a door is any operation but `open`. A node with no kind, or one outside the eighteen, is refused
rather than failed. Graphs need a `kind` on each node and no `role` or `entry`. `generate --archetype medical-day`
([graph generator](graph-generator.md#medical-day-graph-first)) checks its graph and the extracted one against it.
