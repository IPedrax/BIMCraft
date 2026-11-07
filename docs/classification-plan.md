# Classification Manager: plan

Branch `feat/classification-manager`, on top of `feat/parameter-manager` (PR #250). Worktree `../BIMCraft-classify`.

The goal: pick a classification system, load its parameters and its tables into the project, then classify the elements
either by hand (a window that follows the selection, as Revit's Interoperability Tools do) or with an AI agent the user
already has (Claude Code, Codex, Ollama, any CLI), whose answers come back for review before anything is written.

## The workflow, as the user sees it

1. **Manage > Classification Manager.** Pick a system (Uniclass 2015, OmniClass, NBR 15965, a table of your own…).
2. **Load Parameters.** A `.bimparams` dictionary of our own with fixed GUIDs (for example `Uniclass Ss Number`,
   `Uniclass Ss Title`) is mounted and imported through PR #250's `dictionary_mount` and `parameter_import`. These are
   type parameters by default.
3. **Load Table.** The system's table, for the chosen edition, is read into the project and kept there, so the project
   opens without the file, as dictionaries are.
4. Then one of these:
   - **By hand.** A modeless window follows the selection. It groups the selection by category and type and shows the
     table filtered to that category's likely sections, with a search box and Show All. A click assigns the code.
     Next Unclassified walks through what's left, and a bar says how much is done (for example 312 of 340 types).
   - **With AI.** Classify with Agent… builds the brief, runs the agent set in Settings > Agent, and opens a review
     table with what it proposes: the code, a confidence and one line of reason. The user ticks rows and clicks Apply.
     It's one command and one undo step.

## Stages

Every stage is commands first, with tests (CLAUDE.md: everything is a command, test first).

### 1. Systems and tables (core, `classification.rs`)
- `Kind::Classification` library entries (id and GUID). Each has a name, an edition, a source, a licence note and its
  items `{code, title, parent}`. Each table has category hints, saying which BIMCraft categories go to which sections
  (walls to `Ss_25`, `EF_25`; doors to `Pr_30_59`…).
- `classification_load {path | builtin, system, edition}` reads `.csv` or `.xlsx` with code, title and parent (or depth)
  columns, matched by heading. It reads kindly, with caps (`MAX_ITEMS`, `MAX_TEXT`, `MAX_FILE`). A newer edition shows
  what it would change first, as a dry run.
- `classification_list` lists the loaded systems and their coverage.

### 2. Parameters (on PR #250)
- Each system names its parameter pair (number and title) and its scope. A built-in dictionary per system is mounted
  with `dictionary_mount` and imported with `parameter_import`, so the GUIDs are the same in every project and schedules
  and IFC line up.

### 3. Assigning (core)
- `classify {system, assign: [{target: guid, code}], dry_run?}`. The target is a type's library GUID (type scope) or an
  element's GUID (instance scope). The command checks that the code is in the loaded table and writes the number and
  title together, through the existing type and instance value paths. The title always comes from the table, never from
  the caller.
- `classification_check` lists what's unclassified, what has a code missing from the table (after an edition change)
  and what doesn't match its category's hints. These are reports, not `Model::check` errors, so a table update never
  makes a project fail to open or edit.

### 4. The by-hand window (`crates/ui-egui/src/classification.rs`)
- It's a dockable, modeless window: the system dropdown, Load Parameters and Load Table with their status, the tree
  filtered by the selection's category, search, assign, Next Unclassified, and coverage.
- It changes the model only through `classify`. It's tested with egui_kittest: select a wall, click a code, check the
  parameter.

### 5. The brief (core, `classification_brief`)
- `classification_brief {system, rows: all | unclassified | selection, by: type | instance}` gives one row per type (or
  per element) with:
  - the key (its GUID), the category, the family and the type name
  - its materials (layers and material parameters)
  - its distinct levels, and its hosts' categories and types
  - size hints (thickness, width, height), the IFC class and any existing code
- With each category it lists its **candidate codes**, the table cut down by the category hints, so a model without
  tools (Ollama) gets only what it needs. The full table also goes out as a file for agents that can read files.
- **By type is the default.** Classifications such as Uniclass Ss and Pr and OmniClass 23 belong to types, and a
  project has around 100× fewer types than elements, which makes the run cheaper and faster. Levels and hosts still go
  in as a summary for each type.
- Because it's a command, an agent on the MCP server (or `mcp --connect`) can do the whole thing with no window:
  `classification_brief`, its own reasoning, then `classify {dry_run: true}`, then `classify`.

### 6. Running the agent (ui, `agent.rs`; settings in `settings.rs`)
- **Settings > Agent:** a preset (Claude Code, Codex, Ollama, Custom), the command (default `claude`), the model
  (optional), extra arguments, a timeout, and **Test**, which runs `--version` and says what it found.
- **The handover** is a fresh temporary folder holding `instructions.md`, `brief.json`, `table.csv` and `schema.json`
  (the answer's JSON Schema). The program runs there, in a thread, with a progress bar and Cancel (which kills the
  child), and its output is capped. Large projects go in batches of about 100 rows per call.
- **The presets** (flags checked against the CLIs installed here on 2026-11-07):

  | Preset | Command | Notes |
  |---|---|---|
  | Claude Code | `claude -p --output-format json --json-schema <schema> --tools Read` (prompt on stdin, in the folder) | Read-only tools. `result` holds the JSON. |
  | Codex | `codex exec --sandbox read-only --skip-git-repo-check --output-schema schema.json -o answer.json -` | Answer read from `answer.json`. |
  | Ollama | `ollama run <model> --format json` (prompt on stdin) | No tools: the candidates go inline, and batches are smaller. |
  | Custom | a template with `{prompt_file}`, `{schema_file}`, `{out_file}`, `{dir}`, `{model}` | For gemini, llm, aider, … |

- On Windows, npm installs `claude.cmd` and `codex.cmd`, and `Command::new("claude")` doesn't find them, so the command
  is resolved through `PATHEXT` first.

### 7. Reading the answer back and reviewing it
- The answer is `{rows: [{key, code, confidence, reason}]}`. It's hostile input: unknown keys, codes missing from the
  table, wrong categories, values that aren't finite and oversized text are each dropped and listed with the reason,
  never applied.
- **The review window:** the proposed code and title, the confidence (coloured), the reason, and an Accept tick (on by
  default above a threshold set in Settings). Any row can be changed with the same picker as the by-hand window. Apply
  runs one `classify` command, which is one undo step. Nothing is written without this review.

### 8. IFC
- Export writes `IfcClassification`, `IfcClassificationReference` and `IfcRelAssociatesClassification` (ISO 16739).
  Import reads them back into the system's parameters, and they're tested through the entity references in the STEP
  file.

### 9. Finish
- `docs/areas/classification.md`, a line in the module map, `pt` translations, the full checks, then the rpm.

## Owner's decisions (2026-11-07)

1. **Every system is supported, none is bundled.** BIMCraft knows how each looks (its tables, how a code is written, which
   parameters it fills and whose they are), and the user loads the tables they hold: Uniclass 2015, OmniClass, UniFormat,
   MasterFormat, NBR 15965, CoClass, CCI, NL-SfB, and a custom table of any other.
2. **COBie is in**, and whose value each field is was decided field by field: its Type sheet's are the type's (make, model,
   warranty), its Component sheet's each element's (serial number, installation and warranty start dates, tag, bar code),
   its Space sheet's each room's. Its Category fields take another system's codes, written `code: title`.
3. Each classification table's scope was decided table by table, with its reason kept beside it (`TableDef::why`): what a
   thing is (product, system, element function, work result) is the type's; a space's is each room's.

Still open: the network rule's sentence in CLAUDE.md for running the agent command (proposed in the PR), and whether
proposals above a confidence threshold may be applied without review (the window ticks them, the user still applies).
