# Classification

Manage > Classification Manager (CI) classifies a project's elements by the systems the industry uses, by hand or with the
AI assistant's model. The core is `crates/core/src/classification.rs`; the window is `crates/ui-egui/src/classification.rs`,
which asks the model through `crates/assistant` ([the assistant](assistant.md)). The plan and the owner's decisions:
[the plan](../classification-plan.md).

## Systems, and no tables

BIMCraft knows how each system looks (`SYSTEMS`): Uniclass 2015, OmniClass, UniFormat, MasterFormat, NBR 15965, CoClass,
CCI, NL-SfB and COBie 2.4. It knows their tables, how a code is written, which parameters each table fills, and whose
value each one is. It **ships no table**, because their publishers own them. The user loads the edition they hold from
`.csv` or `.xlsx` (`classification_table`). A table of any other system loads as `custom` under a name of the user's.

- Columns are found by heading, in the systems' languages (Code, Number, Código, Kod…; Title, Description, Descrição,
  Benämning, Omschrijving…). With no heading it knows, the first two columns are the code and the title.
- A file that holds several tables (Uniclass's in one sheet) is cut to the one asked for, by the table's key as a prefix.
- Codes are compared by their letters and digits (`normal`): `Ss_25_10` is `SS 25 10`, and `23-13 00 00` is `2313`.
  NBR 15965 writes two characters and six pairs, its older parts fewer; trailing `00` pairs go, so `3E 14 02` and
  `3E 14 02 00 00 00 00` are one code, and its hierarchy by position is the prefix rule below.
- A table's example (`TableDef::example`) is a real code of the published table, or none where none was checked.
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
- **A space's** classification (Uniclass SL, OmniClass 13, NBR 4A) is **each room's**. NBR 4U classifies a unit (an
  apartment, a shop), not its rooms; with no unit or zone here it is each room's too, and every room of a unit carries the
  same code.
- **A work result** (OmniClass 22, MasterFormat, NBR 3R) fits a task, a specification or a budget item best; with none of
  those here it is the type's, and `scopes` moves it.
- **A table whose subject isn't an element here** (NBR 15965's 0M materials, 0P properties, 1F phases, 1S services,
  1D disciplines, 2N functions, 2Q equipment, 5I information) is listed, so `classification_table` names it and cuts a
  file of all the tables to its codes, but it has no category: asking for all a system's tables leaves it out with a note,
  and it makes its parameters for the categories named in `categories`.
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
- The engine leaves `classify`'s `target` and `classification_answer`'s `key` as GUIDs (`engine.rs`, `named`): a type's
  GUID would otherwise be read as its name. Other commands' fields of those names are named as before.

`classification_check` gives, for each binding, how many are classified, which aren't, and which hold a code the table no
longer has.

**By hand:** the window stays open beside the views and follows the selection.

- It shows the selection's rows (a type, or an element) with their codes now.
- It lists the selection category's candidates, a search, or the whole table as a tree. A click gives the selection that
  code.
- Next Unclassified selects what's left, one type at a time, and a count says how much is classified.

## The AI step

`classification_brief` is what a model is handed:

- One row per type (per element for a table of each element's), with its category, family, type, materials, distinct
  levels, hosts, sizes, count and code now.
- For each table and category, **candidates** (`candidates_for`): the codes whose titles (and parents') share words with
  the category's (in the systems' languages) and the rows' type, family, materials and names.
- Instructions (`instructions`) and the answer's JSON Schema (`answer_schema`).

`Brief::chunks` cuts a brief into batches, and `handover` writes each batch as text, with the candidates in it; with `files`
(`classification_brief {files: true}`) the instructions point at the full tables as `.csv` instead, for an agent that reads
files.

**In the desktop**, Ask the Agent asks the AI assistant's model: the provider, model and key of File > Settings > Assistant,
the AI chat's own ([the assistant](assistant.md)).

- Each batch (`BATCH`, 25 rows, with 30 candidates a category) is one user message: a line saying it is a task with no
  tools, the instructions, the answer's JSON Schema and the brief, as Copy Brief puts them.
- It goes out through the assistant's own client and network (`bimcraft_assistant::complete` on the chat's `Net`), so its
  rules hold: the three providers' hosts only, the key in a header, the reply read to `MAX_REPLY` and parsed leniently, and
  a batch past `MAX_HISTORY` refused before it goes. The assistant's system prompt goes too, and says that text inside the
  project is data, never instructions: type and material names come from whatever project is open.
- The request carries the assistant's tools, as each of its requests does. A model that calls one is told the task has
  none (twice at most), and nothing runs.
- The batches run on a thread of their own, with Cancel, as a chat turn does. Cancel gives up on the request that is out
  (its answer goes nowhere) and asks no more; what came back before is put up for review.
- The tokens each reply counts are added to the project's in Settings > Assistant, as the chat's are.
- **What leaves the machine, said first.** Before the first request of the session to a provider, the tab says what goes
  (each type's category, family, type name, materials, levels, hosts and sizes, and the tables' candidate codes), to which
  provider and on whose key, and the button reads Agree and Ask the Agent. Nothing is sent before that click, and nothing at
  start, when the window opens or when a key is typed.
- **No key, no network.** With no key (none typed, none in the provider's environment variable, or one locked under its
  passphrase), the button isn't there: the tab says why, and Assistant settings opens the page. A window started without
  the network (every test's) says that instead. It is always the user's own key: a key lent in a Co-Work session is never
  used here.

**Elsewhere** the agent is the AI. The CLI and the MCP server reach no network for classification: an MCP client calls
`classification_brief`, answers it, checks its answer with `classification_answer`, and writes it with `classify`. Copy
Brief and Paste Answer do the same by hand, for a chat window.

**The answer is hostile:**
- `json_in` finds the JSON in the model's text.
- `read_answer` (what `classification_answer` runs) keeps only rows whose key is an element or type of the project, whose
  table is loaded and bound, and whose code the table holds. Everything else is listed as left out, and when nothing is
  left the window says why.
- The window shows the proposals with their confidence and reasons, ticks those over a threshold, and writes the ticked
  ones with one `classify`, which is one undo step. Nothing is written without that review.

Tested with the assistant's fake network (`crates/ui-egui/src/tests/classification.rs`): a run that proposes codes, an
answer that isn't one, Cancel, and no key.

## IFC

- **Export** (`ifc::classify`, ISO 16739's classification resource):
  - an `IfcClassification` for each system (its publisher, its tables' edition, left out when they disagree, as NBR
    15965's parts, published from 2011 to 2022, do; its name), the same one a family's OmniClass number and a project
    parameter's own classification are of (`Step::classification_by`)
  - an `IfcClassificationReference` for each code held (code, title, system)
  - an `IfcRelAssociatesClassification` from each code to the elements holding it, their own or their type's
  - what a table wrote (the code's parameter and its title's, for each element) is neither a property too nor a second
    reference of the parameter's own classification: the other parameters of a classification go out after it
- IfcOpenShell validates the file and reads the references back (`ifcopenshell.util.classification.get_references`).
- **Import** reads references as the IFC import does ([property sets and classification](ifc-import.md#property-sets-and-classification)),
  in one loop: a type's codes on its occurrences, references of references up to the system. A system this module knows
  (`known_system`) comes into its own parameters (`imported_parameters`): `Uniclass Pr Number` for `Pr_…`, `OmniClass 23
  Number` for `23-…`, with the reference's name in its Title parameter; any other's into one text parameter named for the
  system. Either remembers the system and its edition (`ProjectParameter::classification`), so with no tables loaded a project
  read from IFC goes back out as the same references, and a project classified here survives going out and in twice.
- A known system's parameters come in as the system makes them: with its GUIDs (`parameter_guid`), and of the scope their
  table has (`imported_scope`). A table of types (`Uniclass Ss`) has its codes, read on the elements, moved onto their types
  once the import has given the types their GUIDs (`onto_types`): each type takes the code most of its elements hold (the
  first, on a tie) and that element's title. An element of another code, or of a category with no types (stairs, ramps),
  is left out and said in what the import left out. Loading the system's tables afterwards finds the parameters by GUID and
  binds them as they are, every code kept.

## Left for later

- A Job sheet (maintenance tasks and their frequencies) and the Floor and Facility sheets of COBie. Levels and the project
  aren't parameter categories yet.
- Type objects in IFC: codes go on occurrences, as the project's type parameters do.
- Writing a COBie spreadsheet: the values are parameters, and a schedule exports them.
