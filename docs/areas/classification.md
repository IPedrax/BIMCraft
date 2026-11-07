# Classification

Manage > Classification Manager classifies a project's elements by the systems the industry uses, by hand or with the
user's own AI agent. The core is `crates/core/src/classification.rs`; the window and Settings > AI Agent are
`crates/ui-egui/src/classification.rs`; the agent is run by `crates/agent`; `bimcraft-cli classify` does the agent's part
with no window (`apps/bimcraft-cli/src/classify.rs`). The plan and the owner's decisions: [the plan](../classification-plan.md).

## Systems, and no tables

BIMCraft knows how each system looks (`SYSTEMS`): Uniclass 2015, OmniClass, UniFormat, MasterFormat, NBR 15965, CoClass,
CCI, NL-SfB and COBie 2.4. It knows their tables, how a code is written, which parameters each table fills, and whose
value each one is. It **ships no table**, because their publishers own them. The user loads the edition they hold from
`.csv` or `.xlsx` (`classification_table`). A table of any other system loads as `custom` under a name of the user's.

- Columns are found by heading, in the systems' languages (Code, Number, Código, Kod…; Title, Description, Descrição,
  Benämning, Omschrijving…). With no heading it knows, the first two columns are the code and the title.
- A file that holds several tables (Uniclass's in one sheet) is cut to the one asked for, by the table's key as a prefix.
- Codes are compared by their letters and digits (`normal`): `Ss_25_10` is `SS 25 10`, and `23-13 00 00` is `2313`.
- A code's parent is its file's Parent column, or else the longest other code of the table it begins with. This one rule
  serves every system's way of writing a hierarchy (pairs of digits, letters, dots).
- Loading again with a newer edition replaces the table, and `dry_run` says what it adds, removes and retitles.
- A loaded table is kept in the project (`Model::classification`, a `Kind::Classification` library entry), so the project
  opens without the file.

## Whose value a code is

Decided table by table, with the reason kept beside each (`TableDef::why`, shown as the window's tooltip):

- **What a thing is** (a product, a system, an element's function, a work result) is the same for every element of a type,
  so it's a **type** parameter, and a type is classified once. Stairs and ramps have no types here, so type tables leave
  them out, and the parameters command says so.
- **A space's** classification (Uniclass SL, OmniClass 13, NBR 4A and 4U) is **each room's**.
- **COBie**:
  - The Type sheet's fields are the type's: maker, model, warranty, expected life, nominal sizes.
  - The Component sheet's fields are each element's: serial number, installation date, warranty start date, tag, bar
    code, asset identifier. Dates are text, `YYYY-MM-DD`.
  - The Space sheet's fields are each room's.
  - The Type and Component fields are for doors, windows and components, the assets a facility manager keeps.
  - The Category fields take another system's codes, from the table `cobie` names, written `code: title`.

The window lets a table's scope be changed before Load Parameters (`scopes`).

## Parameters and bindings

`classification_parameters` makes a system's parameters through PR #250's `parameters::plan` and `apply`.

- Each table makes `<table> Number` and `<table> Title` (`Uniclass Ss Number`), and COBie makes `COBie <Sheet> <Column>`.
- Their GUIDs come from their names (`parameter_guid`), so they are the same in every project, and schedules, dictionaries
  and IFC line up.
- A *binding* ties a parameter (and its title's) to the table it takes codes from. Bindings are held by name in memory and
  by GUID in a file, and a rename follows them.

## Classifying

`classify` writes codes, all or none:

- A code must be in its table, written as the table writes it, and its title is always the table's.
- A target is an element (id or GUID) or a type (its library GUID). A type parameter's code goes on the element's type.
- The engine leaves `target` and `key` as GUIDs: a type's GUID would otherwise be read as its name.

`classification_check` gives, for each binding, how many are classified, which aren't, and which hold a code the table no
longer has.

**By hand:** the window stays open beside the views and follows the selection.

- It shows the selection's rows (a type, or an element) with their codes now.
- It lists the selection category's candidates, a search, or the whole table as a tree. A click gives the selection that
  code.
- Next Unclassified selects what's left, one type at a time, and a count says how much is classified.

## The agent

`classification_brief` is what an agent is handed:

- One row per type (per element for a table of each element's), with its category, family, type, materials, distinct
  levels, hosts, sizes, count and code now.
- For each table and category, **candidates** (`candidates_for`): the codes whose titles (and parents') share words with
  the category's (in the systems' languages) and the rows' type, family, materials and names.
- Instructions (`instructions`) and the answer's JSON Schema (`answer_schema`).

An agent that reads files gets the full table as `.csv` too, and searches it when no candidate fits. One that doesn't
(Ollama) gets the candidates in its prompt, in smaller batches. `Brief::chunks` cuts a brief into batches, and `handover`
writes each batch's files.

`crates/agent` runs the command the user set in Settings > AI Agent, in a fresh folder holding those files:

- **Claude Code:** `claude -p --output-format json --tools Read,Grep,Glob --strict-mcp-config --no-session-persistence
  --json-schema …`. Its structured output is the answer.
- **Codex:** `codex exec --sandbox read-only --skip-git-repo-check --ephemeral --output-schema schema.json -o answer.json -`.
- **Ollama:** `ollama run <model> --format json`, with the brief in the prompt.
- **Custom:** a command line with `{prompt_file}`, `{schema_file}`, `{answer_file}`, `{dir}` and `{model}`.

How the agent is run:

- No shell reads the command.
- The program is found on `PATH` and where installers put agents (`~/.local/bin`, `/opt/homebrew/bin`, npm's folder…),
  with `PATHEXT` on Windows, where npm installs `claude.cmd`.
- Output is capped. A timeout or Cancel stops the agent's whole process group.
- The folder is removed when the run is done.
- Test runs `--version`.

The Claude desktop chat app installs no `claude` command; Claude Code is its own install. Ollama's installer puts `ollama`
on `PATH`.

**The answer is hostile:**
- `json_in` finds the JSON in the agent's text.
- `read_answer` keeps only rows whose key is an element or type of the project, whose table is loaded and bound, and whose
  code the table holds. Everything else is listed as left out.
- The window shows the proposals with their confidence and reasons, ticks those over a threshold, and writes the ticked
  ones with one `classify`, which is one undo step. Nothing is written without that review.
- Copy Brief and Paste Answer do the same for an agent BIMCraft can't run, such as a chat window.

**What leaves the machine.** BIMCraft opens no connection: the agent sends what it's given to its own provider. The window
asks before the first run (`Settings::consented`) and says what is sent. The root rule that only `crates/update` phones
home is about BIMCraft's own connections; running the user's agent is the user's act, through `crates/agent`, off by
default.

## IFC

- **Export** (`ifc::classify`, ISO 16739's classification resource):
  - an `IfcClassification` for each system (its publisher, the table's edition, its name)
  - an `IfcClassificationReference` for each code held (code, title, system)
  - an `IfcRelAssociatesClassification` from each code to the elements holding it, their own or their type's
- IfcOpenShell validates the file and reads the references back (`ifcopenshell.util.classification.get_references`).
- **Import** reads each element's references, through references of references, into the system's own parameters
  (`imported_parameters`): `Uniclass Pr Number` for `Pr_…`, `OmniClass 23 Number` for `23-…`. Another program's system
  comes in as `<name> Number` and `<name> Title`.

## Left for later

- A Job sheet (maintenance tasks and their frequencies) and the Floor and Facility sheets of COBie. Levels and the project
  aren't parameter categories yet.
- Type objects in IFC: codes go on occurrences, as the project's type parameters do.
- Writing a COBie spreadsheet: the values are parameters, and a schedule exports them.
