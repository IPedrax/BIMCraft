# Building codes for all standards: the plan

#386 brought the **building code**: a project names a code, its stairs are held to that code's rules, any rule can be
changed for a state's or a city's own numbers, and a code goes from one project to another as an XML file. Stairs are only
the first case. The electrical and hydraulic work since #290 has gone the same way, one data file at a time
(`load-points.json`, `dr-check.json`, `bath-volumes.json`, `plumbing.json`, and in the open PRs `building-demand.json`
and `entrance-categories.json`). But many of the numbers are still Rust constants and Rust tables. This plan brings all of
them under one Building Code with five tabs: **Architectural, Structural, Electrical, Mechanical, Plumbing**.

It covers what is there today, what contributors have already proposed, the design, the interface, a catalogue of every
parameter a code may hold, the work in order, and the decisions the maintainer needs to make. Nothing in it is built yet.

## The new rule: no hardcoded values

> **No code values in code.** A number that a standard, a utility, a fire brigade, a maker's table or a common practice
> gives is a rule of a building code. It lives in the code's data with its source, and the project can change it. Rust
> holds the method (a formula, a lookup, a check), never the number. Physical constants (g, water's viscosity, copper's
> temperature coefficient) and the safety caps (`MAX_*`, tolerances) stay in Rust.

Because every number is editable in Manage > Building Codes, a project whose office or city reads a rule differently
changes it there. A PR is only needed to change the **default** numbers a code ships with, and the `source` beside each
number makes such a PR quick to review.

The rule goes in `CLAUDE.md`'s Rules, and a test enforces it (see [Enforcement](#enforcement)).

## What is there now

### The Building Code engine (#386)

- `codes.rs`: `BuildingCode { code, name, region, references, note, based_on, sources, checked, stairs: StairRules }`, 26
  stair rules, `stair_findings`, `solve`, and XML in and out (`to_xml`, `from_xml`, `<rule name value source/>`).
- `data/building-codes.json`: generic, NBR 9050, NBR 9077 (both checked by ffishman against the texts), IBC, DIN 18065 and
  Japan (unchecked).
- `Model::building_code` (the code's name), `Model::stair_rules` (the project's changes) and `Model::building_codes` (the
  project's own codes).
- Manage > Building Codes (`ui-egui/src/codes.rs`): one code selector, Import / Export / Save as / Delete, an Overview tab and
  a Stairs & Railings tab with min/max pairs, a source beside each number, "Changed: the code says …" and Reset.
- The rule that a check is fast and spatial: `docs/areas/building-codes.md`.

### Numbers that are data already, but outside the Building Code

| File | Read by | What |
|---|---|---|
| `data/electrical.json` | `electrical.rs` | voltages and their tolerances, distribution systems, wire types and their sizes |
| `data/load-points.json` | `loads.rs` | NBR 5410 9.5.2: lighting VA and general-use outlets by room |
| `data/dr-check.json` | `dr.rs` | NBR 5410: where a 30 mA RCD is required |
| `data/bath-volumes.json` | `bath.rs` | NBR 5410 9.1: the volumes round a bath or shower |
| `data/plumbing.json` | `plumbing.rs`, `water.rs`, `drainage.rs` | five fixtures: flows, weights, UHC, drain DN |
| `data/building-demand.json` (#458, #483) | `building_demand.rs` | a building's demand: Celesc, Cemig, Fecoergs, Hidropan, Spain, Portugal, France, NEC |
| `data/entrance-categories.json` (#484) | `entrance.rs` | entrance categories of Fecoergs, Hidropan, Energisa, Equatorial, Copel |
| `data/arei-symbols.json` | `symbol_set.rs` | plan symbols. This is drafting, not a code, so it stays where it is. |

### Numbers that are still in Rust

| Module | Constants and tables | Tab |
|---|---|---|
| `model.rs` | `RISER`, `TREAD`, `STAIR_WIDTH` (#386 takes them from the code), `RAMP_SLOPE` (1:12), `RAMP_WIDTH` 1200 | Architectural |
| `nbr5410.rs`, `nbr5410/tables.rs` | `DROP_LIMIT` 4 %, `REACTANCE` 0.1 Ω/km, `SYSTEMS`, the installation `METHODS`, Tabelas 36, 37, 40, 41, 42, 45, 48, `FLEX_OVER_RIGID` | Electrical |
| `dps.rs` | AQ regions, `CONDUCTOR_LENGTH` 0.5 m, the discharge and impulse currents, Uc (Tabela 49), `SIGNAL_ENTRY_WITHSTAND_KV` | Electrical |
| `conduit.rs`, `conduit/tables.rs` | the 53 / 31 / 40 % fill, makers' conduit families, `CABLE_OD` | Electrical (the fill); catalogue (the makers) |
| `demand.rs` | CPFL GED 13 Tabelas 1 to 15, Enel's annexes and Celesc's table as Rust tables (#474 updates them to their new editions) | Electrical (utility) |
| `water.rs` | `MAX_SPEED` 3 m/s, `MIN_USE_KPA` 10, the fitting equivalent lengths (Tigre), the PVC table | Plumbing |
| `reservoir.rs` | `MIN_DAYS` 1, `MAX_DAYS` 3, `REFILL_HOURS` 6 and 3, `MIN_PUMPS`, `MIN_COMPARTMENTS`, `USUAL_LOWER_PERCENT` 60 | Plumbing |
| `drainage.rs` | NBR 8160 Tabelas 1, 2, 3, 5, 6, 7, 8, `MAIN_MIN_DN` 100 | Plumbing |
| `storm.rs` | `DEFAULT_RAIN` 150 mm/h up to `DEFAULT_RAIN_MAX_AREA` 100 m², `MIN_DOWNPIPE_MM` 70, `MIN_SLOPE` 0.5 %, `N_PVC`, the curves fitted to the abacuses | Plumbing, and Site & Climate (the rain) |
| `hydrant.rs` | CBPMESP IT 22/2025 Tabelas 1 to 3, `MAX_SPEED` 5, `MAX_NOZZLE_HEAD` 100, the least DN | Plumbing (fire protection) |
| `reinforce/norm.rs` | NBR 6118 Tabela 7.2 cover, ρ max 4 %, ρ min 0.15 %, stirrups ≥ 5 mm and ≤ b/10, spacing min(0.6 d, 300), skin bars 0.10 % | Structural |
| `rebar.rs`, `reinforce.rs` | bar diameters and masses, bend diameters, default covers | Structural |
| `timber.rs`, framing PRs #461, #463, #467 | stud spacing, plates, blocking heights, stock lengths, kept in the framing **type** | Structural (code limits); the type keeps its own choices |

Some numbers are not code values and stay where they are:
- **catalogues**: `profiles.rs` (steel sections), the conduit makers' families, `CABLE_OD`, the PVC pipe sizes;
- **drafting**: `standards.rs` (pens, sheets, object styles);
- **defaults of a new element** that no code asks for: `CEILING_HEIGHT`, `DEFAULT_HEIGHT`, `RAILING_INSET`.

Catalogues are discussed under [Decisions](#decisions-for-the-maintainer).

## What contributors have already proposed

The design takes each of these on:

- **dansity, discussion #387**: prefilled codes (DIN, AIA, NBR) for opening sizes, stairs, roof slopes and similar. Families'
  parameters are matched to the code's numbers. A rule broken gives a **warning**, never a block.
- **acordobac, #387**: flag a dimension outside the code while modelling, showing the actual value, the limit and the code
  section. Save **code profiles with local adjustments**, picked by the project's **location and building use**.
- **dronov-dmitry, #387**: switch between countries' tables in the program's settings.
- **ffishman, #386** (the review of NBR 9050 and 9077):
  - one entry per **use** (9050 for an accessible route, 9077 for an emergency exit), never the stricter of both;
  - a `source` for every value, down to the item;
  - the forgiving numbers marked as forgiving;
  - the state fire brigade's instruction rules over 9077 (São Paulo's IT 11, Santa Catarina's IN 09).
- **ffishman, #474**:
  - below the country there is a second level, **who issues the rule** (a utility or a cooperative), with its document and
    edition; a project picks its utility much as it picks its country;
  - a source goes **down to the table and page**;
  - the diversity methods fit one mechanism with a `kind` field: an equivalent count, a coefficient by band, `a + b·n`, or
    load bands;
  - he offered to move the electrical data into the Building Code himself once #386 is in.
- **renanem, discussion #436**:
  - one fixture library for `water_size`, `drain_size` and the commands to come, which a project can override;
  - a consumption table by building type;
  - every answer marks each item as one of **four kinds**: current requirement, historical reference, engineering practice,
    or configurable assumption;
  - a missing input leaves its check `pending` and never stops the others.

## Design

### Concepts

- **Discipline**: one of `architectural`, `structural`, `electrical`, `mechanical`, `plumbing`. Each is chosen, changed,
  imported and exported on its own.
- **Code**: an entry for one discipline, with a name, a region, the documents it is from, a note, who checked it, and its
  sections. Examples: "ABNT NBR 9050 (accessible route)", "IBC 2024 + ICC A117.1", "Eurocodes (recommended values)",
  "NBR 5410:2004".
- **Section**: a group of rules within a discipline, such as Stairs, Ramps, Doors or Conductors. Each is a typed struct in
  the core, beside the module that reads it, and is registered in one table (`codes::SECTIONS`) with its discipline, key,
  rules, units and ranges. `StairRules` is the first.
- **Rule**: one value. It may be a length, a ratio, a percent, an angle, a count, a flow, a pressure, a speed, a load, a
  temperature, a yes/no or a choice. Every rule carries:
  - `source`: the document, its edition, the item, and the table or page;
  - `kind`: `requirement` (the text's), `recommendation`, `practice` (a maker's table or a handbook), `assumption` (the
    project's to justify), `historical` (an older edition kept for reference), or `forgiving` (no document gives it).
- **Table**: a rule whose value is a table: an ampacity table, a fixture-unit table, demand factors, entrance categories,
  covers by exposure class. A table has typed columns (each with a unit) and rows, and one source for the table, page
  included.
- **Use class**: a rule may depend on the use. Examples: a private stair and a common stair (UK); a residential and an
  egress stair (IBC and IRC); 9050's accessible route and 9077's emergency exit. A section may hold **cases**, each a full
  set of its rules keyed by a use. The element picks its case: a stair type's *Use*, a room's *Use*, a door's *Egress*. The
  project's code says which case a use maps to.
- **Layers**: the rules in force for a discipline are, last one winning:
  1. the **base code** (for example IBC 2024);
  2. any **amendments**, in order (California's CBC changes; São Paulo's IT 11 over NBR 9077; the utility CPFL GED 13 under
     NBR 5410; a municipal Código de Obras);
  3. the **project's own changes**.

  Each value shown says which layer it comes from. An amendment is a code entry that sets only what it changes
  (`"amends": "nbr-9077"`). This answers #387's "local rules" and #474's "who issues the rule" with one mechanism. A utility
  is an amendment of the Electrical code that fills the Demand & Utility section.
- **Site & Climate**: the numbers a code's maps give for the project's location: rainfall intensity, ground snow load,
  basic wind speed, seismic zone, frost depth, climate zone, thunderstorm days (AQ1 to AQ3), outdoor design temperatures.
  They are project data shared by every tab (storm drains read the rain; `dps_check` reads the AQ class). They are shown on
  the Overview.
- **Occupancy vocabulary**: a short, neutral list of uses that BIMCraft knows: residential (single, multi), office, retail,
  assembly, education, healthcare, industrial, storage, parking, and a few more. Each code maps its own classes onto it
  (IBC groups A to U, NBR 9077 groups A to M, the UK purpose groups). A room's `use` and the building's main use then drive
  egress, fixture counts and ventilation, whichever codes the five tabs hold.

### Data layout

```
crates/core/data/codes/
  architectural/generic.json  nbr-9050.json  nbr-9077.json  ibc-2024.json  irc-2024.json  uk-adk-adm.json  din.json  japan.json  ...
  structural/   generic.json  nbr-6118.json  eurocode.json  aci-318.json ...
  electrical/   generic.json  nbr-5410.json  nec-2023.json  iec-60364.json
                utilities/cpfl-ged13.json  enel-gri0017.json  celesc.json  cemig.json  fecoergs.json  ...
  mechanical/   generic.json  ashrae-621.json  nbr-16401.json  en-16798.json
  plumbing/     generic.json  nbr-5626-8160-10844.json  ipc-2024.json  en-806-12056.json
                amendments/cbpmesp-it22.json ...
  site/         (maps: none are bundled; the project types its own numbers, with the map's source)
```

- One file per code keeps PRs that add a country small, and keeps contributors from conflicting with each other.
- `codes/data.rs` lists them with `include_str!`. A test fails when a file in the folder is not listed.
- Each file is self-describing:

  ```json
  {"code": "nbr-5410", "discipline": "electrical", "name": "ABNT NBR 5410:2004", "region": "Brasil",
   "references": [...], "checked": "...", "note": "...", "amends": null,
   "sections": {"conductors": {"drop_limit": {"value": 4, "source": "NBR 5410:2004 6.2.7.1 c)", "kind": "requirement"}, ...}}}
  ```

  A rule may be given bare (`"drop_limit": 4`) where its source is the section's.
- The existing data files move in with their numbers unchanged. Their `about` text becomes the section's documentation.
  Each existing rule set becomes a code (`load-points.json`'s `NBR 5410:2004` becomes `electrical/nbr-5410.json` →
  `sections.dwelling_loads`).

### In the core

- `codes.rs` becomes a folder:
  - `codes.rs`: the selection, layers and resolution, `Model::rules::<S>()`;
  - `codes/schema.rs`: the section registry and rule documentation;
  - `codes/value.rs`: `Rule`, `Table`, `Source`, `Kind`, `Quantity`;
  - `codes/xml.rs`: the file format;
  - `codes/data.rs`: the bundled codes;
  - `codes/stairs.rs`: today's stair rules, findings and solver, moved and unchanged.
- **Resolution** merges the layers' JSON per section, then deserializes into the section's typed struct (`StairRules`,
  `ConductorRules`, …), so a consumer reads typed fields, never strings. The result is cached per model revision, because
  checks run every frame.
- **Consumers stay pure.** `nbr5410::size(input, &ConductorRules)` takes its rules as an argument. The command looks the rules
  up from the project, or from the `standard` / `utility` it is given. The existing tests pin the numbers and pass unchanged.
- **Quantities**: rules are stored in SI (mm, kPa, L/s, m/s, kN/m², VA, °C). `codes/quantity.rs` shows and parses them in the
  project's units (psi, gpm, ft/s, psf and °F for imperial projects), as `Units::length` does for lengths.
- **Model**: `Model::codes: CodeSelection`, with for each discipline `{ base, amendments, changes }`, plus Site & Climate and
  the project's own codes (`Model::building_codes`, now each with a `discipline`). This replaces `building_code` and
  `stair_rules`. That is a **FORMAT bump** with a `Model::migrate` step: `building_code` becomes the architectural base, and
  `stair_rules` becomes the architectural changes in `stairs`. An older file opens unchanged.
- **Validation** stays in one place: each section's `check()` (ranges not upside down, finite, in its unit's bounds),
  called from `Model::check` for the project's own codes and changes.

### Commands

- `building_code`: `{discipline?, set?, amendments?, rules?: {section: {rule: value}}, save?, delete?}`.
  - With no discipline it is architectural, so #386's `{set, stairs}` still works as it did.
  - Returns the resolved rules, each with its value, layer, source and kind.
- `building_code_export`: `{discipline? | all, path, name?}`. One discipline, or a bundle of all five.
- `building_code_import`: `{path}`. Reads one discipline, a bundle, or #386's format 1 (stairs, which goes to
  architectural). Unknown rules are listed as `left_out`.
- `code_rules`: `{discipline?, section?, rule?}`. Read only: what is in force and why. This is for agents (MCP) and scripts.
- `site_climate`: `{rain?, snow?, wind?, ...}`. The project's Site & Climate numbers, each with the map it was read from.
- `code_check`: `{ids? | all, discipline?}`. Every finding of every section for the elements given, as `stair_check` gives
  them for stairs. `stair_check` and `stair_solve` stay.

### The file format (XML, format 2)

```xml
<building-code format="2" discipline="electrical" name="NBR 5410:2004 (office)" region="Brasil" based-on="nbr-5410">
  <reference>ABNT NBR 5410:2004 (versão corrigida 2008)</reference>
  <checked>…</checked>
  <section name="conductors">
    <rule name="drop_limit" value="4" unit="%" kind="requirement" source="NBR 5410:2004 6.2.7.1"/>
    <table name="ampacity_cu_pvc" source="NBR 5410:2004 Tabela 36" page="101">
      <column name="section" unit="mm2"/><column name="A1_2" unit="A"/> …
      <row>1.5 14.5 13.5 …</row>
    </table>
  </section>
</building-code>
```

- A bundle is `<building-codes format="2">` holding up to five `<building-code>`.
- Format 1 files (#386) still read.
- What a file holds that this version doesn't know is listed as `left_out`, as `open` does for a project.
- The reader keeps #386's hostile-input caps: 1 MB a file (raised to 8 MB for a bundle with tables), a text length cap, and
  a cap on the rows and columns of a table.

### Speed

Unchanged from `docs/areas/building-codes.md`: every check is fast and spatial, timed on the highrise. Resolution is cached
per revision, so reading a rule is a field access.

## The interface

Manage > Building Codes becomes a larger dialog, about 980 × 680 and resizable, with six tabs. Every tab shows the same
discipline header, so import, export, presets and Save As work **per discipline**, as asked.

### A discipline tab (here Electrical)

```
┌ Building Codes ────────────────────────────────────────────────────────────────────────────────────────────────┐
│ Overview │ Architectural │ Structural │ [Electrical] │ Mechanical │ Plumbing                                   │
├────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ Code      [ ABNT NBR 5410:2004 (Brasil)                ▾]  ✔ checked by a contributor (#386)                   │
│ Amendments [ CPFL GED 13 v46.0 ✕ ] [ + Add… ]                       ● Custom: 3 rules changed for this project │
│ [Import…] [Export…] [Save As…] [Delete]                                         [Reset Tab to the Code's Rules] │
├──────────────────────────────┬─────────────────────────────────────────────────────────────────────────────────┤
│ 🔍 [ find a rule…          ] │ Conductors and Protection                                         NBR 5410:2004 │
│                              │ ─────────────────────────────────────────────────────────────────────────────── │
│ Supply and Systems           │ Rule                          Value               From      Source               │
│ Conductors and Protection ●3 │ Voltage drop, branch circuit  [ 4   ] %           code      6.2.7.1 c)        ⓘ │
│ Voltage Drop                 │ Reactance                     [ 0.1 ] Ω/km        code      practice          ⓘ │
│ Conduit Fill                 │ Least section, lighting       [ 1.5 ] mm²         code      Tabela 47         ⓘ │
│ Dwelling Loads and Outlets   │ Least section, power          [ 2.5 ] mm²         ● project  code says 2.5  Reset│
│ RCDs                         │ Ampacity, Cu PVC 70 °C        [ table 24 × 12 ]   code      Tabela 36, p. 101 Edit│
│ Surge Protection (DPS)       │ Temperature correction        [ table 13 × 3  ]   code      Tabela 40       Edit│
│ Bathroom Volumes             │ …                                                                               │
│ Demand and Utility      CPFL │                                                                                 │
│ Equipment Working Space   ◌  │                                                                                 │
│ Lighting Levels           ◌  │                                                                                 │
│ Lightning Protection      ◌  │ ┌ Least section, power ─────────────────────────────────────────────────────┐   │
│ Fire Alarm                ◌  │ │ ABNT NBR 5410:2004, 6.2.6.1.1, Tabela 47 · requirement · checked           │   │
│                              │ │ The least copper section of a power circuit's phase conductors.            │   │
│ ◌ reference only: nothing    │ │ Read by: circuit                                                           │   │
│   checks these yet           │ └────────────────────────────────────────────────────────────────────────────┘   │
├──────────────────────────────┴─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                           [OK] [Cancel] [Apply] │
└────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

- **The header** picks the base code. Amendments show as chips, in order, and are added from a list filtered to what amends
  that base. An orange "Custom" shows while the project's changes are unsaved as a code. **Save As** keeps the tab as a
  code of the project's own. **Import** and **Export** read and write this discipline only. **Reset Tab** clears the
  project's changes to this discipline.
- **The section list** on the left:
  - a count of the project's changes, shown as an orange dot;
  - the amendment that fills the section ("CPFL");
  - ◌ for a section that nothing checks yet. Its values are still shown, as a reference.
  - The search box filters rules across every section of the tab, by name, by key and by source.
- **The rules grid** keeps #386's layout: min and max side by side; a tick for a yes/no; a drop-down for a choice; a number
  field in the project's units. The **From** column says which layer the value comes from (code, an amendment's short name,
  or the project). The **Source** is truncated, with its full text on hover. A changed rule shows "code says …" and Reset,
  as now.
  - **Use cases** (Stairs: Private / Common / Egress / Accessible) show as a row of buttons over the grid. Each case is a full
    column of values.
- **Tables** open in an editor below the grid:
  - the header names each column with its unit;
  - cells are edited in place;
  - rows can be added and removed;
  - **Copy** and **Paste** as tab-separated text, so a table goes to and from a spreadsheet;
  - the table's own source and page.
- **The rule card** at the bottom shows the selected rule:
  - its full source, its kind, and whether it was checked;
  - a line of what it means;
  - **Read by**: the commands and checks that use it, from the registry. This is how a user sees that a ◌ rule is not yet
    checked.

### Overview

```
┌ Overview ──────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ Region preset [ Brasil, São Paulo                  ▾]  sets all five tabs     [Import All…] [Export All…]       │
│                                                                                                                │
│ ┌ Architectural ───────────────┐ ┌ Structural ─────────────────┐ ┌ Electrical ─────────────────┐               │
│ │ NBR 9050 + IT 11 (CBPMESP)   │ │ NBR 6118 / 6120 / 8681      │ │ NBR 5410 + CPFL GED 13      │               │
│ │ ✔ checked · 2 changes        │ │ ⚠ unchecked numbers         │ │ ✔ checked · 3 changes       │               │
│ │ 4 elements outside a rule  → │ │                             │ │                             │               │
│ └──────────────────────────────┘ └─────────────────────────────┘ └─────────────────────────────┘               │
│ ┌ Mechanical ──────────────────┐ ┌ Plumbing ───────────────────┐                                               │
│ │ NBR 16401                    │ │ NBR 5626 / 8160 / 10844     │                                               │
│ │ ⚠ unchecked numbers          │ │ + CBPMESP IT 22/2025        │                                               │
│ └──────────────────────────────┘ └─────────────────────────────┘                                               │
│                                                                                                                │
│ Site and Climate                                                     Main use of the building [ Residential ▾] │
│ Rainfall intensity [150] mm/h  return 5 years  source [NBR 10844 Tabela 5, São Paulo        ]                  │
│ Thunderstorm days  [ AQ2 ▾]    Wind speed V0 [40] m/s (NBR 6123 isopleths)   Ground snow [0] kN/m²            │
│ Climate zone      [ ZB3 ▾]     Frost depth   [0] mm   Seismic zone [0 ▾]     Altitude [760] m                  │
└────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

- A **region preset** is a small bundle that names a base and amendments for each tab. Picking one sets all five tabs, and
  each can then be changed on its own tab.
- Each card links to its tab and to the elements outside a rule (`code_check`).
- **Site and Climate** holds the project's map values, each with a source field.

### Where findings show

These keep #386's pattern for each element a section checks:
- a sign in Properties that opens the Building Code Check window (generalized from the stair's to any element, with advice
  per rule);
- live while drawing and dragging;
- `code_findings` in the element's JSON.

### Testing the interface

The tests use egui_kittest, in the style of `tests/codes.rs`:
- switch tabs;
- change a rule and Reset it;
- add and remove an amendment;
- Export and Import one tab and leave the others alone;
- Save As;
- edit a table cell and paste rows;
- search.

Screenshots in the three looks, as `docs/areas/looking-at-results.md` asks.

## The parameter catalogue

Every parameter found in the codes looked at, grouped by tab and section. **Status**:
- **today**: a module reads it now and will read it from the code;
- **check**: an element in the model can be checked against it, and this plan adds the check;
- **ref**: shown as a reference until a tool reads it.

The typical values are for orientation only. They were typed from public summaries and were **not checked** against the
texts. The data files carry each value's source, and a value stays `checked: ""` until someone checks it, as #386 does.
Abbreviations: G = generic, BR = Brasil (ABNT), US = ICC (IBC, IRC, IPC, IMC) / ADA / NFPA / NEC, EU = Eurocodes and EN,
UK = the Approved Documents, DE = DIN.

### Architectural

**A1. Stairs** (today, #386: the 26 rules, plus what the #386 review asked for). Cases: private, common, egress, accessible.

| Rule | What | Unit | Typical (unchecked) | Status |
|---|---|---|---|---|
| riser, going, `riser_min`…`going_max`, `step_min/max`, `width_min`, `headroom`, `flight_rise_max`, `flight_risers_max`, landings, winders, walk line, nosing, open risers, uniformity | as #386 | | US 102–178 / 279; IRC ≤ 197 / ≥ 254; UK private 150–220 / 220–300; BR 160–180 / 280–320 | today |
| `flight_risers_min` | fewest risers in a flight | count | BR 9077 3; UK 1 | check |
| `pitch_max` | steepest pitch | ° | UK private 42 | check |
| `uniformity_flights` | most two successive flights' risers differ | mm | BR 9077 5 | check |
| `handrail_both_sides_width` | width past which both sides need a handrail | mm | UK 1000; US by egress | check |
| `intermediate_handrail_width` | width past which a middle handrail is due | mm | US: every 1524 (60 in) of required width | ref |
| `handrail_extension_top/bottom` | how far a handrail runs past the top and bottom risers | mm | US 305 top, one going at the bottom; BR 300 | check (railings) |
| `spiral_*` | a spiral's width, going at the walk line, riser and headroom | mm | IBC 660 / 191 at 305 / 241 / 1982 | check |
| `tread_slope_max` | a tread's slope | % | US 2 | ref |

**A2. Ramps** (today: `RAMP_SLOPE`, `RAMP_WIDTH`). Cases: accessible, vehicle.

| Rule | What | Unit | Typical | Status |
|---|---|---|---|---|
| `slope` | a new ramp's slope | % | 8.33 | today |
| `slope_by_rise` | the steepest slope for a run's rise (table) | % / mm | BR 9050: 5 % up to 1500, 6.25 % up to 1000, 8.33 % up to 800; UK AD M: 1:20 over 10 m, 1:15 over 5 m, 1:12 over 2 m | check |
| `run_rise_max`, `run_length_max` | most rise and length of one run before a landing | mm | US 760; UK 500 rise | check |
| `width_min` | least clear width | mm | US 915; BR 1200; UK 1500 | check |
| `landing_length_min`, `turn_landing_min` | landings straight on and at a turn | mm | US 1525; BR 1200 / 1500 | check |
| `cross_slope_max` | cross slope | % | US 2.08 (1:48); BR 2 | ref |
| `handrails_from_rise` | rise past which handrails are due both sides | mm | US 150 | check |
| `edge_protection_min` | a curb or rail at an open edge | mm | US 100; BR 50 | ref |
| `vehicle_slope_max`, `vehicle_transition` | parking ramps | % | 20 straight, 15 curved (practice); transitions at half the slope | ref |

**A3. Guards and handrails** (today, inside stairs; it moves here, shared by stairs, ramps and railings).

| Rule | What | Unit | Typical | Status |
|---|---|---|---|---|
| `guard_required_drop` | drop past which a guard is due | mm | US 762; UK 600 (dwellings 600, stairs 2 risers); BR — | check |
| `guard_min` by case | guard height: dwelling, other, external high | mm | US 1067 (IRC 914); UK 900 / 1100; BR 1050, 920 internal, 1300 external > 12 m | check |
| `guard_opening_max` | the sphere that must not pass | mm | US 102 (152 at a stair's triangle); UK 100; BR 110 (practice) | check (balusters) |
| `guard_climbable` | horizontal rails allowed | yes/no | UK no (dwellings) | ref |
| `handrail_min/max` | height over the nosings or the floor | mm | US 864–965; UK 900–1000; BR 700 / 920 (two rails) | check |
| `handrail_grip_min/max`, `handrail_clearance_min` | grip size and clearance to the wall | mm | US 32–51, 38; BR 30–45, 40 | ref |
| `window_sill_guard_height` | an openable window's sill below which a guard is due | mm | US 610 (where the drop is over 1830) | check (windows) |

**A4. Doors and openings** (check: doors and windows are in the model).

| Rule | What | Unit | Typical | Status |
|---|---|---|---|---|
| `clear_width_min` by case (any, accessible, egress) | clear width | mm | US 813; BR 800; UK 775 to 825 by approach | check |
| `clear_height_min` | clear height | mm | US 2032; BR 2100; UK 2000 | check |
| `leaf_width_max` | widest leaf on an egress door | mm | US 1219 | check |
| `maneuvering_pull_latch`, `maneuvering_push_latch` | clear floor beside the latch | mm | US 457 / 305; BR 600 (practice) | ref |
| `threshold_max` | threshold height | mm | US 13; BR 5 (20 bevelled) | ref |
| `hardware_height_min/max` | handle height | mm | US 865–1220; BR 800–1100 | ref |
| `opening_force_max` | interior door force | N | US 22 | ref |
| `swing_with_egress_from` | occupant load from which a door swings the way out | people | US 50 | ref |
| `doors_in_series_min` | space between two doors in a row | mm | US 1219 + the door's swing | ref |
| `vision_panel_zone` | where a vision panel is due | mm | UK 500–1500 | ref |

**A5. Egress (means of escape)** (ref; checks later, once escape paths are a model query).

| Rule | What | Unit | Typical | Status |
|---|---|---|---|---|
| `occupant_load_factor` | area per occupant, by use (table) | m²/person | US Table 1004.5; BR 9077 Tabela 5 | ref, read by A5, P2, M2 |
| `capacity_per_person_stairs/other` | width per occupant | mm/person | US 7.6 / 5.1 (5.1 / 3.8 with sprinklers) | ref |
| `unit_of_passage`, `capacity_per_unit` | BR 9077's unit and its people per unit, by use | mm, people | BR 550; table | ref |
| `corridor_width_min` by load | corridor width | mm | US 1118 (914 under 50 people); BR 1100 / 1200 | check |
| `exits_by_load` | number of exits by occupant load (table) | count | US 2 up to 500, 3 up to 1000, then 4 | ref |
| `travel_distance_max` by use, sprinklered or not | travel distance (table) | m | US 61–76 (91 with sprinklers); BR 9077 Tabela 6 | ref |
| `common_path_max`, `dead_end_max` | | m | US 23 / 6.1 (15.2 with sprinklers) | ref |
| `exit_separation` | fraction of the diagonal | ratio | US 1/2 (1/3 with sprinklers) | ref |
| `egress_headroom_min` | ceiling height on an egress path | mm | US 2286 | check |
| `refuge_space` | a wheelchair space in a refuge area | mm | US 760 × 1220 | ref |

**A6. Rooms and habitability** (check: rooms are in the model, with area, perimeter and height). Cases by room kind
(bedroom, living, kitchen, bath, service, corridor, storage, garage).

| Rule | What | Unit | Typical | Status |
|---|---|---|---|---|
| `area_min` by kind | least area | m² | US IRC 6.5 habitable (one room 11.1); BR: the municipal Código de Obras (SP: dormitory 8, the second 6, …) | check |
| `dimension_min` by kind | least side, or a circle that fits inside | mm | US 2134; BR 2400 (bedroom, varies by city) | check |
| `ceiling_height_min` by kind | least ceiling height | mm | US 2134; BR 2500 (2300 for baths, corridors, larders); EU 2400–2500 | check |
| `sloped_ceiling_rule` | share of the floor that may be lower, and to how low | %, mm | US 50 % ≥ 1524 | ref |
| `light_ratio_min` by kind | glazed area / floor area | ratio | US 8 %; BR 1/6 habitable, 1/8 other | check (windows in rooms) |
| `vent_ratio_min` by kind | openable area / floor area | ratio | US 4 %; BR half the light area | check |
| `room_depth_max` | deepest room for its window's head height | ratio | 2.5 × head (practice, EU) | ref |
| `escape_window` | emergency escape opening: net area, least width, least height, highest sill | m², mm | US 0.53 m², 508, 610, 1118 | check (sleeping rooms) |
| `illuminance_min` (performance) | BR 15575's daylight | lx | BR 60 / 90 / 120 | ref |

**A7. Accessibility** (beyond A1, A2 and A4).

| Rule | What | Unit | Typical | Status |
|---|---|---|---|---|
| `route_width_min`, `passing_space` | accessible route, and a passing space every so often | mm, m | US 915, 1525 × 1525 every 61 m; BR 1200 (900 short) | ref |
| `turning_diameter` | turning circle | mm | US 1525; BR 1500 | check (rooms tagged accessible) |
| `clear_floor_space` | wheelchair module | mm | US 760 × 1220; BR 800 × 1200 | ref |
| `reach_high/low` | reach range | mm | US 1220 / 380; BR 1200 / 400 (practice) | ref (switches, outlets: E6) |
| `wc_*` | toilet: room size, the WC's centreline to the wall, seat height, grab bars' length and height | mm | US 405–455 centreline, 430–485 seat, 840–915 bars | ref |
| `counter_height_max`, `knee_clearance_min` | | mm | US 865 / 685 | ref |
| `accessible_parking_ratio`, `accessible_stall` | how many, and their size | count, mm | US 1 in 25; 2440 + aisle 1525 | ref |
| `accessible_units_ratio` | share of accessible dwellings | % | by code | ref |

**A8. Fire safety (passive).** Ref; a check of wall and floor types' fire rating later.

| Rule | What | Unit | Typical | Status |
|---|---|---|---|---|
| `construction_types` | the fire resistance of each element by construction type (table) | h | US Table 601; BR IT 08 TRRF | ref |
| `area_max`, `height_max`, `storeys_max` by use and type (table) | allowable size | m², m | US Tables 504 and 506 | ref |
| `compartment_area_max` | | m² | BR IT 09 | ref |
| `separation_openings` | unprotected openings by separation distance (table) | % | US Table 705.5 | ref |
| `shaft_rating` | a shaft enclosure's rating by storeys connected | h | US 1 (fewer than 4 storeys), 2 | ref |
| `fire_door_rating` | a fire door's rating by the wall's rating (table) | h | US Table 716.1 | ref |
| `extinguisher_travel_max`, `extinguisher_area_per_unit` | | m, m² | BR IT 21: 15–25 m | ref |

**A9. Envelope and energy** (ref; check wall, roof and window types' U-values later).

| Rule | What | Unit | Typical | Status |
|---|---|---|---|---|
| `climate_zone` | from Site & Climate | | US 1–8; BR ZB1–ZB8 | ref |
| `u_max` by element and zone (table) | wall, roof, floor, window, door | W/m²K | US IECC R402.1.2; BR 15575 / 15220; DE GEG; UK Part L | ref |
| `shgc_max`, `wwr_max` | solar gain; window-to-wall ratio | ratio | US by zone | ref |
| `air_tightness_max` | | m³/h·m² at 50 Pa | UK 8; DE 3 / 1.5 | ref |

**A10. Acoustics.** `rw_min` between dwellings and `lnw_max` impact (dB): BR 15575 Rw ≥ 45; US STC/IIC 50; DE 4109. Ref,
read by wall and floor types later.

**A11. Site, zoning and parking** (municipal; ref).
- Setbacks (front, side, rear), coverage, floor-area ratio, height limit: property boundaries are in `site.rs`.
- Parking: stall 2500 × 5000, aisle 5000 to 6000, headroom 2100 to 2300, bays per dwelling or per m² by use.

**A12. Occupancy and use.** The code's classes mapped to BIMCraft's use vocabulary (table), and which case of each
section a use takes. Read by A5, P2, M2 and E8.

### Structural

**S1. Basis of design** (ref, later read by load combinations).

| Rule | What | Unit | Typical | Status |
|---|---|---|---|---|
| `gamma_g`, `gamma_q` | partial factors on permanent and variable actions | | EU 1.35 / 1.5; BR 1.4 / 1.4; US 1.2D + 1.6L | ref |
| `psi_0/1/2` by load category (table) | combination factors | | EU Table A1.1 | ref |
| `design_life` | design working life | years | 50 | ref |
| `importance` / `consequence_class` | | | | ref |

**S2. Loads.**

| Rule | What | Unit | Typical | Status |
|---|---|---|---|---|
| `unit_weights` (table) | densities of concrete, steel, masonry, timber | kN/m³ | 25 / 78.5 / 18 / 5 | ref (later read by takeoffs) |
| `imposed_by_use` (table) | live loads by use | kN/m² | EU A 1.5–2.0, B 2.0–3.0, C 3.0–5.0, D 4.0–5.0, E 7.5; US 1.92 residential, 2.40 office, 4.79 assembly; BR NBR 6120 1.5 residential | ref |
| `concentrated_by_use` (table) | | kN | EU A 2.0 | ref |
| `partition_allowance` | | kN/m² | EU 0.5–1.2; US 0.72 | ref |
| `barrier_line_load` by use | horizontal load on a guard | kN/m | EU 0.5–3.0; US 0.73 (50 plf) + 0.89 kN | ref |
| `live_reduction` | area and storey reduction rule | choice | | ref |
| `roof_imposed` | | kN/m² | EU H 0.4; US 0.96 | ref |
| wind, snow and seismic | method and coefficients; the site's numbers come from Site & Climate | | NBR 6123, EN 1991-1-4, ASCE 7 | ref |

**S3. Concrete** (today: `reinforce/norm.rs`).

| Rule | What | Unit | Typical | Status |
|---|---|---|---|---|
| `exposure_classes` (table) | classes and the least concrete class and w/c for each | | BR CAA I–IV: C20 to C40; EU X0 to XS3 | today (the class) |
| `cover_nominal` by class and member (table) | nominal cover | mm | BR Tabela 7.2: beam and column 25 / 30 / 40 / 50, slab 20 / 25 / 35 / 45 | today |
| `cover_tolerance` | Δc | mm | BR 10 (5 with strict control); EU 10 | today |
| `rho_long_max`, `rho_long_min` (table by fck) | longitudinal steel ratio | % | BR 4 / 0.15 up to C30 | today |
| `column_*` | least side, least bar, ρ min and max | mm, % | BR 190 (140 with γn), 10, 0.4, 8 (4 outside laps) | check (columns with bars) |
| `stirrup_dia_min`, `stirrup_dia_max_ratio`, `stirrup_spacing_max` | | mm, ratio | BR 5, b/10, min(0.6 d, 300) | today |
| `skin_ratio`, `skin_from_depth`, `skin_spacing_max` | | %, mm | BR 0.10 per face, 600, min(d/3, 200) | today |
| `slab_thickness_min` by use | | mm | BR 70 roof, 80 floor, 100 cantilever and vehicles under 30 kN, 120 over | check (floors) |
| `beam_width_min`, `wall_thickness_min` | | mm | BR 120 / 100 | check |
| `bar_spacing_min` | clear spacing between bars | mm | max(20, φ, 1.2 dmax) | today (bar lists) |
| `slab_bar_spacing_max` | | mm | BR min(2h, 200) | today |
| `bend_diameter` by φ and use (table) | mandrel diameter | ×φ | BR Tabela 9.1: 5φ under 20 mm, 8φ at 20 and over (CA-50) | today (rebar shapes) |
| `anchorage`, `lap_factor` | the bond method's coefficients | | BR η1 η2 η3; EU | ref |
| `crack_width_max` by class | | mm | 0.4 / 0.3 / 0.2 | ref |
| `deflection_max` | span ratio | 1/n | 250 total, 500 after partitions | ref |

**S4. Reinforcing steel.** Grades and their fyk (CA-50 500 MPa, CA-60, B500B, Grade 60), Es, and the bar catalogue
(diameter, mass per metre: NBR 7480, EN 10080, ASTM A615). The bar catalogue is data the code ships, because the bar
sizes a country sells are part of its standard. Today, in `rebar.rs`.

**S5. Structural steel** (ref): γM0, γM1 (EU 1.0 / 1.0; BR 1.10), grades, the most slenderness (200 in compression, 300 in
tension), deflections (L/250, L/350, L/180 for roofs), the bolts' edge and end distances, the least weld sizes. The
profiles stay a catalogue.

**S6. Timber** (with the framing PRs #461, #463 and #467).

| Rule | What | Unit | Typical | Status |
|---|---|---|---|---|
| `stud_spacing_max` by stud size and storeys (table) | | mm | US IRC Table R602.3(5): 406 / 610 | check (framed walls) |
| `joist_span_max` by size, species, spacing and load (table) | | mm | US IRC R502.3.1 | check (framed floors) |
| `rafter_span_max` (table) | | mm | US IRC R802.4.1 | check (framed roofs) |
| `header_span_max` (table) | | mm | US IRC R602.7 | check (openings) |
| `bearing_min` | | mm | US 38 on wood, 76 on masonry | check |
| `notch_max_ratio`, `hole_max_ratio`, `hole_edge_min` | | ratio, mm | US 1/6, 1/3, 51 | ref |
| `fireblocking_spacing_max` | | mm | US 3048 | ref |
| `gamma_m`, `kmod` (table), service classes | | | EU 1.3; BR 1.4 | ref |
| `deflection_max` | floors and roofs | 1/n | US L/360 floors, L/240 roofs | ref |

The framing **type** keeps what the designer chooses (spacing, plates, stock lengths). The code holds the limits that
type is checked against.

**S7. Masonry** (ref): least wall thickness, the most slenderness h/t, lintel bearing, control joint spacing.

**S8. Foundations.**

| Rule | What | Unit | Typical | Status |
|---|---|---|---|---|
| `frost_depth` | from Site & Climate | mm | | check (footings) |
| `footing_width_min` by soil and storeys (table) | | mm | US IRC R403.1 | check (strip footings: `glue.rs`) |
| `footing_thickness_min`, `embedment_min` | | mm | US 152 / 305 | check |
| `presumptive_bearing` by soil (table) | | kPa | US IBC 1806.2: 72 (clay) to 576 (rock) | ref |
| `cover_cast_against_earth` | | mm | US 76; BR 45–50 | today (with S3) |

**S9. Serviceability** (ref): a table of deflection limits by member and case, the least floor frequency (3–5 Hz), and
drift (H/500 under wind; 0.020 hsx in an earthquake).

### Electrical

**E1. Supply and systems** (today: `electrical.json`): voltages and their tolerance band; distribution systems (phases,
wires, voltages, earthing); frequency (50 / 60 Hz); the earthing systems allowed (TN-S, TN-C-S, TT, IT); the wire types
and their sizes (a catalogue that the code's standard defines).

**E2. Conductors and protection** (today: `nbr5410.rs`).

| Rule | What | Unit | Typical | Status |
|---|---|---|---|---|
| `ampacity_*` (tables) | by insulation, method and loaded conductors | A | BR Tabelas 36–39; US NEC 310.16; IEC 60364-5-52 B.52 | today |
| `temperature_correction` (table), `grouping_correction` (tables), `soil_resistivity_correction` (table) | | factor | BR Tabelas 40–45 | today |
| `reference_air`, `reference_soil` | | °C | 30 / 20 | today |
| `installation_methods` (table) | reference methods | | BR Tabela 33 | today |
| `section_min` by use | lighting, power, signal | mm² | BR 1.5 / 2.5 / 0.5; US 14 AWG on 15 A | today |
| `neutral_rule`, `pe_rule` (tables) | | mm² | BR Tabelas 48, 58 | today |
| `aluminium_min` | the least aluminium section | mm² | BR 16 | ref |
| `breaker_ratings` | the standard ratings | A | 6, 10, 16, 20, 25, 32, 40, 50, 63, … | today |
| `i2_factor` | conventional trip / In | | 1.45 | today |
| `reactance` | | Ω/km | 0.1 (practice) | today |
| `disconnection_time` by system and Uo (table) | | s | IEC 0.4 at 230 V (TN) | ref |

**E3. Voltage drop** (today: `DROP_LIMIT`): the branch limit (BR 4 %; US 3 % branch and 5 % total, informational; UK 3 %
lighting and 5 % other), the total from the transformer (BR 7 %), the default power factor, and the temperature the
resistance is taken at.

**E4. Conduit and raceway** (today: `conduit.rs`): the most fill for 1, 2 and 3 or more conductors (53 / 31 / 40 %, in
NBR and NEC chapter 9); the least conduit size; the most bends between pull points (US 360°, BR 270°); the longest
straight run between boxes (BR 15 m, less 3 m per bend).

**E5. RCDs** (today: `dr-check.json`): the locations where 30 mA is required (BR: bathrooms, outdoors, kitchens, laundries,
wet areas; US NEC 210.8 GFCI list and 210.12 AFCI), the most residual current, the exemptions.

**E6. Surge protection** (today: `dps.rs`): AQ thresholds (25 days of thunderstorms a year), the least In by mode (5 / 10 /
20 kA), the least Iimp (12.5 / 25 / 50 kA), the connecting conductors (4 / 16 mm²), the longest connection (0.5 m), Uc by
system (Tabela 49), the withstand at a signal entry (1.5 kV). The AQ class itself comes from Site & Climate.

**E7. Bathroom volumes** (today: `bath-volumes.json`): the volumes' dimensions (0, 1, 2; 600 mm, 2250 mm), the IP rating
and equipment allowed in each, and the RCD. UK BS 7671 zones; US NEC 406.9(C) and 410.10(D).

**E8. Dwelling loads and outlets** (today: `load-points.json`).
- Today: lighting VA by area; outlets by room (count by perimeter, least, counters); VA per outlet.
- To add:
  - US NEC 210.52: 1.8 m from any point of a wall (so 3.6 m apart), counters every 1.2 m and within 600 mm of an end,
    wall spaces of 610 mm and more;
  - dedicated circuits (washer, dryer, oven, AC; US 2 × 20 A small-appliance circuits);
  - the most load per circuit;
  - mounting heights (outlets low, middle and high; switches; accessible reach from A7).

  Checking outlets against the walls of rooms is a check.

**E9. Demand and utility** (today: `demand.rs`, #458, #483, #484).
- Demand factors by load class (tables).
- The building's diversity: the method `kind` (`equivalent_count`, `per_dwelling`, `a_plus_bn`, `bands`) with its tables.
- The dwelling's demand by area or as declared.
- The utility's entrance categories (by system and entrance type), the power factor, the most single-phase load.
- This section is filled by the utility **amendment** (CPFL, Enel, Celesc, Cemig, Fecoergs and its members, Hidropan,
  Energisa, Equatorial, Copel, …). Each table carries its document, edition, item and page, as #458 and #484 already do.

**E10. Equipment working space** (check: panels are in the model): the depth in front of a panel by voltage and condition
(US NEC 110.26: 914 / 1067 / 1219), the width (762, or the equipment's), the headroom (1981), the dedicated space above.

**E11. Lighting levels** (ref, later checked against rooms' luminaires): maintained lux by room or task (EN 12464-1 / NBR
ISO 8995-1: office 500, corridor 100, stairs 150); emergency lighting: least on the path (EU 1 lx; BR 10898 3 / 5 lx),
duration (1 h / 3 h), the spacing of exit signs.

**E12. Lightning protection** (ref): protection level I–IV, the rolling sphere radius (20 / 30 / 45 / 60 m), the mesh size
(5 × 5 / 10 × 10 / 15 × 15 / 20 × 20 m), the down conductors' spacing (10 / 10 / 15 / 20 m), the protection angles
(table). NBR 5419 / IEC 62305.

**E13. Fire alarm** (ref): detector spacing and area by type (US NFPA 72: 9.1 m; BR 17240), the longest walk to a manual
call point (US 61 m; BR 30 m), sounder levels.

### Mechanical

**M1. Design conditions** (ref): the indoor setpoints (cooling 23–26 °C, 40–65 % RH; heating 20–22 °C). The outdoor
design temperatures (0.4 % or 1 % cooling, 99.6 % heating) come from Site & Climate.

**M2. Ventilation** (check: rooms with a use and an area; outdoor air per room, once air terminals carry a flow).

| Rule | What | Unit | Typical | Status |
|---|---|---|---|---|
| `rates_by_use` (table) | outdoor air per person (Rp) and per area (Ra) | L/s·person, L/s·m² | US 62.1 Table 6-1: office 2.5 / 0.3, classroom 5 / 0.6; BR NBR 16401-3 levels 1–3; EU EN 16798-1 categories I–IV | check |
| `density_by_use` (table) | default occupancy | people/100 m² | US 62.1 | check |
| `ez` by air distribution (table) | zone effectiveness | | 1.0 / 0.8 | ref |
| `exhaust_by_room` (table) | toilets, kitchens, garages | L/s | US 25 / 35 per WC (intermittent / continuous) | check |
| `residential_rate` | whole-dwelling rate | formula | US 62.2: 0.15 L/s·m² + 3.5 L/s × (bedrooms + 1) | check |
| `co2_above_outdoor_max` | | ppm | BR 700 | ref |
| `natural_vent_ratio` | shared with A6 | | | — |

**M3. Ducts** (ref, until ducts are sized): the most velocity by duct kind and noise class (mains 6–8, branches 4–5, outlets
2–3 m/s), the most friction (0.8–1 Pa/m), the pressure classes, insulation by temperature and climate, the most aspect ratio
(4:1), leakage class, fire dampers at rated walls (with A8).

**M4. HVAC piping** (ref): the most velocity by DN (1.2–3 m/s), the most friction (400 Pa/m), insulation by temperature
(ASHRAE 90.1 Table 6.8.3).

**M5. Equipment efficiency** (ref): the least efficiencies by equipment (SEER, EER, COP; 90.1 Tables 6.8.1; BR PBE classes),
the economizer and energy recovery thresholds by climate zone, fan power limits.

**M6. Clearances** (ref): service space in front of equipment (US IMC 306: 762), outdoor units' clearances, plant rooms.

**M7. Kitchen exhaust** (ref): hood overhang (152), flow per metre of hood by duty, grease duct velocity.

**M8. Smoke control** (ref): stair pressurization (50 Pa; BR IT 13), the most door opening force with pressure.

**M9. Refrigerants** (ref): the most charge for a room's volume (ASHRAE 15 / EN 378 RCL).

### Plumbing

**P1. Fixtures** (today: `plumbing.json`, `water.rs`, `drainage.rs`; #436's library). For each fixture:

| Field | Unit | Typical |
|---|---|---|
| `cold_flow`, `hot_flow` | L/s | Tigre's table (BR practice) |
| `weight` (P), `wsfu` (US), `loading_unit` (EN 806) | | |
| `uhc` (BR) / `dfu` (US) / `du` (EN 12056) | | BR Tabela 3; US IPC 709.1 |
| `drain_dn_min`, `supply_dn_min`, `trap_dn` | mm | |
| `pressure_min` | kPa | BR 10; US 55 (138 for a flushometer) |

The fixture list grows past today's five: bidet, washing machine, dishwasher, urinal, drinking fountain, floor drain, service
sink. A project can add its own fixtures.

**P2. Least fixtures by use** (check: rooms and Plumbing components): WCs, lavatories, urinals, drinking fountains and
service sinks per so many occupants by use and sex (US IPC Table 403.1; BR NR 24 for workplaces; UK BS 6465), the share of
accessible fixtures, the longest walk to a toilet (US 152 m), the occupant load from which separate facilities are due.

**P3. Cold water** (today: `water.rs`, `reservoir.rs`).

| Rule | What | Unit | Typical | Status |
|---|---|---|---|---|
| `flow_method` | weights (`0.3 √ΣP`), Hunter, EN 806-3 | choice + coefficient | BR 0.3 | today |
| `velocity_max` | | m/s | BR 3; US 2.4; EU 2 | today |
| `dynamic_pressure_min` | | kPa | BR 10; US Table 604.3 | today |
| `static_pressure_max` | | kPa | BR 400; US 552 (then a PRV) | check |
| `surge_max` | | kPa | BR 200 | ref |
| `service_dn_min` | | mm | US 19 | ref |
| `consumption_by_use` (table) | per capita | L/person/day | 150–200 dwelling (practice), 50 office | today (`reservoir_size`) |
| `reserve_days_min/max`, `refill_hours` (house and other), `compartments_min`, `pumps_min`, `lower_share` | | days, h, count, % | 1 / 3, 6 / 3, 2, 2, 60 (practice) | today |
| `air_gap_min` | backflow | ×DN, mm | 2 × DN, 25 | ref |
| `fitting_lengths` (table) | equivalent lengths | m | Tigre (practice) | today (as a catalogue the code names) |

**P4. Hot water** (ref; #436's `hot_water_size` comes next):
- the flow method (K √ΣP);
- storage and delivery temperatures (≥ 60 °C stored against legionella; ≤ 49 °C at a lavatory, ≤ 43 °C at a bath);
- the longest dead leg (US 15 m; IPC Table 607.1.1 by DN);
- recirculation from a length;
- insulation thickness by DN;
- heater storage by occupants.

**P5. Sanitary drainage** (today: `drainage.rs`).

| Rule | What | Unit | Typical | Status |
|---|---|---|---|---|
| `branch_capacity`, `stack_capacity`, `main_capacity` by slope (tables) | | UHC/DFU | BR Tabelas 5, 6, 7; US IPC 710.1 | today |
| `slope_min` by DN (table) | | % | BR 2 (DN ≤ 75), 1 (DN ≥ 100); US 2.08 / 1.04 / 0.52 | check (sloped pipes) |
| `main_dn_min`, `wc_dn_min` | | mm | BR 100 / 100; US 76 | today |
| `trap_arm_max` by DN (table) | | m | BR Tabela 1; US IPC 909.1 | today |
| `trap_seal_min` | | mm | 50 | ref |
| `cleanout_spacing_max` | | m | BR 25 (inspection chambers); US 30 | check (pipe network) |
| `grease_interceptor_required` | where | yes/no | | ref |

**P6. Vents** (today: `vent_size`): vent tables (BR Tabelas 2, 8; US IPC 916.1); terminal height over a roof (BR 300, or
2000 where the roof is used; US 152); distance from openings (US 3 m across or 0.9 m above); whether air admittance valves
are allowed; relief vents every so many branch intervals (US 10).

**P7. Storm drainage** (today: `storm.rs`).

| Rule | What | Unit | Typical | Status |
|---|---|---|---|---|
| `rain_intensity` | from Site & Climate, with `return_period` by area kind (table) | mm/h, years | BR 150 up to 100 m² (5.1.4); return periods 1 / 5 / 25 | today |
| `wall_fraction` | share of a wall's area that drains to the roof | % | BR 50; US 50 | ref |
| `gutter_slope_min`, `drain_slope_min` | | % | BR 0.5 / 0.5 | today |
| `manning_n` by material (table) | | | PVC 0.011 | today |
| `downpipe_min` | | mm | BR 70 | today |
| `downpipe_curves` (table) | the curves fitted to NBR 10844's Figure 3 | | BR | today (method data) |
| `overflow_required` | a secondary or overflow drain | yes/no | US IPC 1108 | ref |

**P8. Fire protection water** (today: `hydrant.rs`, as the CBPMESP IT 22/2025 amendment).
- Hydrants and hose reels: the system types (table), the reserve by use and area (table), Hazen-Williams C by material
  (table), the most speed (5 m/s), the most head at a nozzle (100 mca), the least DN (65, or 50 for types 1 and 2), and the
  number of jets in the design flow.
- Sprinklers (ref): hazard classes, density (mm/min) and design area (m²), the most area per head and spacing (light hazard
  21 m², 4.6 m), the distance from walls. NFPA 13 / EN 12845 / BR IT 23.

**P9. Fuel gas** (ref):
- pipe sizing tables (capacity by length and DN);
- the most pressure drop;
- combustion air and room volume for appliances (BR NBR 13103; US IFGC 304);
- ventilation openings;
- flue clearances;
- where the meter stands.

**P10. Rainwater reuse** (ref): reservoir sizing method and first-flush volume (NBR 15527).

### Site and Climate (Overview; shared)

These are project inputs that the codes' maps give:
- rainfall intensity and its return period;
- thunderstorm days and the AQ class;
- basic wind speed;
- ground snow load;
- seismic zone or ag;
- frost depth;
- climate zone;
- outdoor design temperatures;
- altitude;
- the soil's class and presumptive bearing.

Each carries the map or table it was read from. None is bundled, because the maps are per place. A region preset may fill
a capital's numbers as an example, marked as such.

## The codes to ship

Each one is an entry per discipline. "Checked" only when a contributor has checked the numbers against the text, as for NBR
9050 and 9077 in #386.

| Region preset | Architectural | Structural | Electrical | Mechanical | Plumbing |
|---|---|---|---|---|---|
| Generic | forgiving ranges (today's generic) | EU recommended values | IEC 60364 (no utility) | EN 16798 | EN 806 / 12056 |
| Brasil | NBR 9050, NBR 9077, IT 11 (SP) as an amendment, Código de Obras SP as an amendment | NBR 6118, 6120, 8681, 8800, 7190 | NBR 5410 + utilities (CPFL, Enel, Celesc, Cemig, Fecoergs…, Copel, Energisa, Equatorial) | NBR 16401 | NBR 5626, 8160, 10844 + IT 22 (SP) |
| USA | IBC 2024 + ICC A117.1 / ADA 2010, IRC 2024 | ASCE 7-22, ACI 318-19, AISC 360, NDS + IRC span tables | NEC 2023 | IMC 2024 / ASHRAE 62.1 | IPC 2024 (UPC as an alternative) |
| UK | Approved Documents K, M, B, F | Eurocodes + UK NA | BS 7671 | Approved Document F | Approved Document H, BS EN 12056 |
| Germany | DIN 18065, MBO, DIN 18040 | Eurocodes + DE NA | DIN VDE 0100 | DIN EN 16798 | DIN 1988, DIN 1986-100 |
| Japan | the existing stairs entry | | | | |

Only today's numbers are moved as they are. Every new entry starts unchecked and says so in its `checked`. The words in the
data are ours: only the numbers, the document's name, item, table and page come from the standards, as #458 and #484 do.

## The work, in order

Each step is one commit or a short run of commits on `feat/building-code`. Each keeps `cargo fmt --check`, clippy and
`cargo test --workspace` green, and each step's tests are written first.

### Step 0. The branch, and what merges into it

1. `feat/building-code` starts from #386's head (`ac749874`), because the engine and the dialog are there. This PR is
   stacked on #386: merge #386 first, and this one then shows only its own work.
2. Merge `main` (6 commits ahead of #386). Only the translation catalogues conflict; they are regenerated with
   `cargo i18n update`.
3. Merge the electrical PRs, in their dependency order:
   1. #483, which carries #458;
   2. #484;
   3. #474.

   A trial merge showed no code conflict, only the two module-map lines in `CLAUDE.md`, the paragraphs in
   `docs/areas/structure-and-mep.md`, and the translations. Their data then moves into the Building Code in step 3.
4. Tell ffishman on #474 and #484 that their branches are carried here. He offered to do the electrical move himself; he
   can take E2 to E9 of step 3 as commits on this branch, or as PRs into it.

If any of them lands on `main` first, its merge here becomes a no-op.

### Step 1. The engine

1. `codes/` folder, `Rule`, `Source`, `Kind`, `Table`, `Quantity`, the section registry, with `StairRules` registered
   as `architectural.stairs`. No behaviour change.
2. Disciplines, layers (base, amendments, changes) and resolution, cached per revision.
3. `Model::codes` with a FORMAT bump and the `Model::migrate` step from #386's `building_code`, `stair_rules` and
   `building_codes`. A test opens a #386 file and gets the same rules.
4. The commands: `building_code` generalized (with #386's params still working), `code_rules`, `site_climate`, and
   `code_check` (stairs only at first).
5. XML format 2, per discipline and as a bundle, with format 1 still read. Hostile-input caps for tables.
6. `hostile_params_never_panic_or_break_the_model` covers the new commands.

### Step 2. The dialog

1. The tab bar, the per-discipline header, the section list, the rules grid with From and Source columns, and the rule card.
2. The table editor (edit in place, add and remove rows, copy and paste as tab-separated text).
3. Use cases (buttons over the grid), the search box, and ◌ for sections that are not yet read.
4. The Overview: region preset, cards, and Site & Climate.
5. Kittest tests; screenshots in the three looks; translations (`cargo i18n update`, then fill pt and es as the
   electrical PRs do).

### Step 3. Rewire what is there, numbers unchanged

Each item moves a module's numbers into its section and changes the module to read them. The module's existing tests pin
the numbers and pass unchanged. Each also adds a test that changing the rule in the project changes the answer.

1. **Architectural**: stairs' guards and handrails into A3 (shared); ramps (`RAMP_SLOPE`, `RAMP_WIDTH`) into A2.
2. **Electrical**:
   - `electrical.json` → E1;
   - `nbr5410` tables, `DROP_LIMIT` and `REACTANCE` → E2 and E3;
   - the conduit fill → E4;
   - `dr-check.json` → E5;
   - `dps.rs` → E6;
   - `bath-volumes.json` → E7;
   - `load-points.json` → E8;
   - `demand.rs`'s Rust tables (at #474's editions), `building-demand.json` and `entrance-categories.json` → E9, each
     utility an amendment.

   Today's default (`NBR 5410:2004`) becomes the project's electrical code.
3. **Plumbing**:
   - `plumbing.json` and the fixtures → P1;
   - `water.rs` and `reservoir.rs` → P3;
   - `drainage.rs` → P5 and P6;
   - `storm.rs` → P7, the rain to Site & Climate;
   - `hydrant.rs` → P8 as the IT 22 amendment.
4. **Structural**: `reinforce/norm.rs` → S3; the bar catalogue and bend diameters → S4.
5. **Site & Climate**: `dps_check` and `storm_size` read the project's AQ class and rain when the params don't give them.

### Step 4. The rule, enforced

1. `CLAUDE.md` gets the rule "No code values in code", one sentence under Rules, and the module map's lines for `codes/`.
2. A test, `code_values_live_in_codes`, reads the modules that consume codes (a list in the test) and fails on a numeric
   `const` or `static` table that is not in its allowlist of physical constants and caps. A new consumer module joins the
   list when it is registered.
3. A data test for every bundled code:
   - every rule has a source and a kind;
   - every table names its page;
   - ranges are not upside down;
   - an amendment only sets rules its base has;
   - every file is listed.
4. `docs/areas/building-codes.md` is rewritten for five disciplines, layers and tables, with how to add a section and how
   to add a country.

### Step 5. New checks, cheap and local

Each check is fast and spatial, timed on the highrise, with a test. Each shows in Properties and in `code_check`.
- **Ramps**: slope by rise, runs, landings, width, handrails.
- **Railings and guards**: height, openings, handrail extensions.
- **Doors**: clear width and height; leaf width on egress.
- **Windows**: the sill guard; the escape opening in sleeping rooms.
- **Rooms**:
  - least area and least side by kind;
  - ceiling height;
  - light and ventilation ratios from the room's windows;
  - turning circle where the room is tagged accessible.
- **Electrical panels**: working space against walls and other elements.
- **Floors**: least slab thickness by use (S3).

The rest stays reference until its tool exists:
- egress paths and travel distance;
- fixture counts by occupancy;
- ventilation by air terminals;
- U-values on types;
- fire ratings;
- framing span tables. These come with #461, #463 and #467, whose author can read S6 once this is in.

### Step 6. Data

1. The Brasil preset from the data moved in step 3, plus the sections still to fill:
   - A4 to A6 from NBR 9050 and the São Paulo Código de Obras;
   - S2 from NBR 6120;
   - M2 from NBR 16401.

   All of them are unchecked until a contributor checks them.
2. USA, UK, Germany and Generic, as in [The codes to ship](#the-codes-to-ship). They are filled by research, each value
   with its source and `checked: ""`.
3. An issue asks contributors to check entries, one per code, as ffishman did for NBR 9050 and 9077.

Steps 0 to 4 are this PR's core. Steps 5 and 6 can run in parallel once the engine is in, and can be split into follow-up
PRs if the maintainer prefers smaller reviews.

## Enforcement

- **The rule** in `CLAUDE.md`, under Rules: "No code values in code: a number a standard, utility or practice gives is a
  rule of a building code, with its source; physical constants and caps stay in Rust."
- **The test** `code_values_live_in_codes`, described in step 4.
- **The data test**, described in step 4.
- **The review**: a PR that adds a calculation names the section it reads.

## Decisions for the maintainer

1. **Stacking on #386.** This PR is based on #386 and needs it merged first. The other way is to fold #386 into this PR,
   which makes one very large PR. **Recommended: stack.**
2. **Carrying #458, #483, #484 and #474.** Merge them into this branch now so their data can move, while they stay open
   on their own. ffishman asked to keep #474 separate, so it should go to `main` first if the maintainer can merge it
   soon. **Recommended: carry them, tell ffishman.**
3. **The default for a new project.**
   - **Recommended:** today's behaviour as the defaults: Generic architectural, NBR 5410 for electrical, and NBR 5626 /
     8160 / 10844 for plumbing, because those are the only full methods there are. A region preset is offered at the
     start.
   - The alternative is to pick the preset from the interface language. That guesses wrong for many users.
4. **Catalogues.** Pipe sizes, conduit makers, cable diameters and steel profiles are not code values.
   - **Recommended:** keep them as data files outside the Building Code, as a Catalogues library later. The exception is a
     table a code prints itself (NBR 5410's ampacities, NBR 8160's tables, a country's rebar sizes), which is in the code.
5. **Where three topics go.**
   - Envelope and energy: under Architectural, because the checks are on wall, roof and window types.
   - Fire protection water (hydrants, sprinklers): under Plumbing.
   - Fuel gas: under Plumbing, as the UPC and Brazilian practice place it. In the ICC family it is the IFGC, alongside
     the mechanical code.
   - **Recommended:** as listed. Moving one later is a data change.
6. **XML or JSON for import and export.** #386 chose XML and people may already have files.
   - **Recommended:** keep XML (format 2) and read format 1.
   - Tables are carried as rows of numbers; Copy and Paste in the dialog covers spreadsheets.
7. **Scope of this PR.** Steps 0 to 4 at least.
   - **Recommended:** steps 0 to 5, with step 6 for Brasil only. The other countries' data follow as one PR each, which
     keeps reviews readable and gives contributors from each country a place to check.

## Sources consulted

The in-repo sources:
- PRs #386, #458, #474, #483 and #484, with the review comments on #386 and #474;
- discussions #387 (Building Code) and #436 (hydraulic sizing);
- the modules and data files listed under [What is there now](#what-is-there-now).

Public summaries, used only to build the list of parameters. Their numbers are unchecked, and every value is checked against
its text before it is marked checked.

ICC:
- IBC chapter 10 (means of egress): [Data Driven AEC](https://datadrivenaec.com/insights/ibc-egress-corridor-requirements),
  [doors](https://datadrivenaec.com/insights/ibc-door-requirements),
  [open-exam-prep](https://open-exam-prep.com/study-guides/icc-cbo/architectural-plan-review/cbo-9-5)
- IPC: [fixture units and slopes](https://open-exam-prep.com/study-guides/icc-cbo/building-systems/cbo-11-2),
  [Table 403.1](https://open-exam-prep.com/study-guides/icc-plumbing-plans-examiner/fixtures-occupant-load/occupant-load-table-403-1)
- IMC and ASHRAE 62.1: [IMC Table 403.3.1.1](https://open-exam-prep.com/study-guides/icc-commercial-mechanical/ventilation-rates/table-403-rates),
  [62.1 rates](https://www.datadrivenaec.com/insights/ashrae-62-1-ventilation-rates)

US accessibility and electrical:
- ADA 2010 and ICC A117.1: [Access Board, chapter 3](https://www.access-board.gov/ada/guides/chapter-3-clear-floor-or-ground-space-and-turning-space)
- NEC 210.52: [Mike Holt](https://www.mikeholt.com/files/PDF/23_UNEC1_210.52.pdf),
  [EC&M](https://ecmweb.com/national-electrical-code/code-basics/article/20901808/receptacles-in-dwellings)

UK:
- Approved Document K: [specifiedby](https://www.specifiedby.com/building-regulations/approved-document-k-protection-falling-collision-impact),
  [Designing Buildings](https://www.designingbuildings.co.uk/wiki/Stair_design)

Europe:
- EN 1992-1-1 Table 4.4N: [blueprints](https://blueprints.readthedocs.io/en/stable/API%20reference/codes/eurocode/en_1992_1_1_2004/chapter_4_durability_and_cover/table_4_4n/),
  [Structville](https://structville.com/2017/11/durability-of-structures-and-how-to-calculate-concrete-cover-eurocode-2.html)
- EN 1991-1-1: [DK NA](https://www.ds.dk/media/mv5dsp34/ds-en-1991-1-1-dk-na-2013-english.pdf),
  [Structville](https://structville.com/live-loads-imposed-loads-on-floors)

Brasil:
- NBR 15575 and the municipal codes: [LabEEE NT 15575](https://labeee.ufsc.br/sites/default/files/publicacoes/notas_tecnicas/NT_15575_FINAL.pdf),
  [norma de desempenho](https://lageportilhojardim.com.br/blog/norma-de-desempenho/)

Automated code checking, for how others structure rules:
- [ITcon 2025 review](https://itcon.org/papers/2025_02-ITcon-Alnuzha.pdf)
- [W78 2024, IDS for accessibility](https://architektur-informatik.scix.net/pdfs/w78-2024-paper_21.pdf)
