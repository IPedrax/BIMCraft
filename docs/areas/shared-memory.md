# Shared Obsidian memory and local delegation

The vault is `E:\Program Files\Obsidian\AI Memori`; its `BIMCraft` folder is reached
through the repository's ignored `.memory` Windows junction. Notes are local shared
state, not part of Git and not a replacement for tracked design documents.

## Startup and context recovery

Read `.memory/00_STATE.md`, `01_BLACKBOARD.md`, relevant sections of `02_DECISIONS.md`, then
`graphify-out/GRAPH_REPORT.md`. Find relevant node IDs and relationships in
`graphify-out/graph.json` before inspecting source. The current export reuses the
existing cache; Graphify is not installed. It does not claim a fresh extraction,
complete coverage, current source locations, or a measured 70x speedup.
Missing modules require an explicit fallback reason and current source inspection.

```powershell
python scripts/sync_memory.py --vault 'E:\Program Files\Obsidian\AI Memori'
# Later, refresh the Obsidian export from the existing graph cache:
python scripts/sync_memory.py
```

Setup verifies an existing junction rather than overwriting it. It creates the
three initial notes only when absent. Sync writes generated Graph notes, JSON and
the cache report, keeping user state/decisions and unrelated notes intact. Links
use `[[BIMCraft/...]]`. At most 200 incident links are shown per node; all edges
remain in `Graph/graph.json`. Notes use `<crate>__<module>__<symbol>.md` inside
crate folders: `core/`, `ui/`, `cli/`, `rvt/`, `update/`, `i18n/`; scripts and
documentation use `memory/`. Nested module names join with underscores; Rust
`::` becomes `_`. Case-insensitive collisions receive a four-character hexadecimal
suffix. For Obsidian graph colors use path groups such as `path:BIMCraft/Graph/core`.

After a future Graphify rebuild, rerun sync. A generated-file manifest tracks
renamed/removed nodes. Legacy hash notes and replaced generated notes are first
archived byte-for-byte in `target/memory-graph-backups/*.zip`, then retired when
appropriate. Unrecognized/manual notes are preserved, and their names are reserved
to avoid overwriting them. Concurrent changes abort retirement. Backup ZIPs stay
outside the vault so old notes do not duplicate its graph.

## Token and context budget

- Resolve the latest user request before restoring an old task. A paused goal stays
  paused; documentation or tooling requests do not resume product development.
- At startup/context recovery, read the compact state and current task once. Read
  only relevant decision sections, graph nodes and source ranges thereafter. Do
  not reconstruct the entire conversation or repeatedly reload unchanged guidance.
- Query graph JSON with a local script; return matching nodes and edges, not the
  whole graph. Use `rg -n` and bounded excerpts before broad source reads. When a
  cache is stale or lacks a module, inspect current sources directly and record
  that limitation once.
- Plan output limits before tool calls: summaries, file names, small diff hunks,
  counts and the first relevant diagnostics. Save complete logs on disk. If output
  truncates, narrow the next query rather than printing the same material again.
- Keep one build/test pipeline active. Use the maximum supported terminal wait
  (`exec_command`: 30000 ms on this Windows host; `write_stdin`: 300000 ms when
  supported), never an empty polling loop. If only waiting remains, checkpoint
  its handle, command and log once and yield; do not generate repeated reasoning
  or progress messages about unchanged state. Never infer completion from silence
  or restart a job merely because observation timed out.
- Give Qwen a bounded task with exact APIs, ranges and acceptance checks, not the
  whole chat/repository. Keep its code and compiler repair loop on disk. Bring back
  only changed paths, check results and actionable findings; Codex still reviews
  the actual diff. Avoid cloud subagents for mechanical work already assigned
  locally. Preserve the existing CAD/GPU safeguards.
- Run focused checks while editing, then the required final gates. Reuse results
  for unchanged inputs; rerun checks when changes or failures invalidate them.
  Token economy never justifies weaker acceptance criteria or unverified claims.
- Keep `00_STATE.md` a compact current snapshot (target: at most 4 KB), with scope,
  accepted changes, evidence, remaining work and live handles. Replace obsolete
  entries; move useful history to linked archives. The blackboard holds one task;
  decisions hold settled constraints, not running logs. Do not reread archives
  without a concrete question.
- Before continuing, identify the next action that changes code, evidence or a
  required artifact. If none is possible, report the actual dependency once and
  stop spending tokens on restating plans. Report usage as observed accounting,
  never as an invented monetary cost or claimed savings percentage.

## Task protocol

### Blocking delivery (MCP and Stop)

The project config `.codex/config.toml` registers `scripts/qwen_mcp.py` with
`tool_timeout_sec = 600`. The installed Python `mcp` 2.x SDK exposes one stdio
tool, `delegate_qwen`. It starts the existing bounded runner, waits locally for
the entire edit/check cycle, and returns only its compact final report. Full
worker output remains under `target/qwen-mcp/`; per-attempt logs retain their
usual `target/qwen-delegate/` location. No shell code comes from the model.

Use `status: queued` for a direct MCP call. Alternatively, set the task's JSON
status to `PENDING_QWEN` and finish the turn: the synchronous Stop command hook
runs the same worker and returns exit 2 with the final report on stderr, causing
one continuation for diff review. Do not start a direct call for a task already
handed off to Stop. A non-pending board and `stop_hook_active: true` are no-ops;
this prevents recursive wakeups. Existing worker locks are never stolen or used
as proof of completion. Busy workers are not restarted. Paused BIM goals remain
paused; this mechanism authorizes only the explicit blackboard task.

The wrapper has a 570-second overall worker deadline, inside the 600-second MCP
and hook limits. On timeout it terminates the worker process tree and records
an incomplete result; inspect partial edits before manually retrying. A timeout
is not a successful compile. Tasks whose compilation exceeds this budget must
be split or use an explicitly revised execution budget, not repeated polling.

Configuration takes effect when Codex loads the project MCP/hook definitions.
Non-managed hooks require the user's exact-definition trust review through
`/hooks`; do not write trust hashes or approve the hook on the user's behalf.
Definitions and local protocol tests do not prove that this already running
chat has reloaded the tool or trusted the hook. See official
[hooks](https://learn.chatgpt.com/docs/hooks) and
[MCP configuration](https://learn.chatgpt.com/docs/extend/mcp).

The blackboard has exactly one fenced `qwen-task` JSON object. Archive the previous
task in `.memory/History/` before replacing it. Set `status: queued`, a short stable
`id`, `mode: edit` or `review`, and precise `instructions` (real APIs, expected
behavior, constraints). Define 1–8 target files with **one editable inclusive line
range per file**, full-file `sha256`, and matching Graphify `graph_nodes`.

```json
{
  "id": "bounded-test",
  "status": "queued",
  "mode": "edit",
  "instructions": "Implement the specified behavior test using the supplied APIs.",
  "targets": [{
    "path": "crates/core/src/example_tests.rs",
    "start_line": 10,
    "end_line": 30,
    "sha256": "CURRENT_SHA256_OF_THE_ENTIRE_FILE",
    "graph_nodes": [],
    "graph_reason": "This new test module is absent from the cached graph."
  }]
}
```

Compute the hash from bytes: `(Get-FileHash PATH -Algorithm SHA256).Hash.ToLower()`.
New files require `create: true`, `sha256: null`, and range 1..0. An empty insertion
range in an existing file has `end_line = start_line - 1`. Context is capped at
48 KB; include relevant signatures in the task if outside the editable ranges.

```powershell
python scripts/delegate_qwen.py
python scripts/delegate_qwen.py --interactive-3d # explicitly defers generation
```

This is agent-triggered automatic delegation, not a background daemon: Codex or
OpenCode enqueues a task and invokes the script without manual model copy/paste.
The script checks `/v1/models` for `coder-iq1_m`, ignores proxy settings for localhost,
and allows only one invocation through `target/qwen-delegate.lock`.
Each request receives the shared state, approved decisions, scoped blackboard task
and selected Graphify node metadata. Memory notes are limited to 12 KB each; an
oversized note must be compressed rather than silently truncated. Expanded prompts
are capped at 110 KB. Edits use low reasoning; independent review uses medium.
No model-generated shell command is executed. Model output is JSON with `summary`
and `edits: [{path, content}]`; content replaces only that file's declared range.
Paths outside the repository, instruction files, Git metadata and `.memory` are
not editable by the helper. Current byte content is checked before writes, and
unrelated lines/CRLF are preserved. Responses with non-`stop` finish reasons fail.

After each edit, the script runs:

```text
cargo --config profile.dev.package.bimcraft-core.opt-level=0 check --quiet --message-format=short --jobs 2
```

Compiler diagnostics return to the local model, for at most **three requests total**
(initial plus two repairs), with a five-second pause between failed attempts.
`review` mode writes findings, never source edits, and runs one check. It can review
Codex changes as well as local changes. Default request/check timeouts are 600/900s;
flags allow bounded overrides. Original bytes, requests, responses and check logs
stay under ignored `target/qwen-delegate/`; the board records only a short result.
Edits after a failed check/timeout remain available for review; they are not
silently accepted or rolled back over concurrent user work.
The board records current target hashes/ranges after applying a response. Refresh
the queued task's hashes and ranges before retrying an interrupted task. Cancelling
or changing the board during generation prevents application of the late response.
For Python tasks, Codex also runs the relevant Python tests: cargo check does not
validate Python code.

States: `queued → running → needs_review | compile_failed | blocked`. Exit codes:
0 means no queued task or ready for review, 1 means checks failed, 2 means blocked.
Codex must inspect `git diff` (new files via `git diff --no-index -- NUL FILE`), run
meaningful tests and required project checks, then record acceptance and update
state/decisions. Compilation is not final approval. A changed blackboard is not
overwritten; status is preserved in the task log as `status-conflict.json` instead.

## Resource limits and recovery

Before every request and before applying a response, Windows process detection
checks Renga, nanoCAD, BIMCraft, ArtCraft and Blender. Known headless test executables
are excluded. Other interactive viewers require `--interactive-3d` or agent-side
deferral. Strata's **Leave 20% for my work** cannot currently be verified from a
stable API, so detected CAD blocks generation rather than pretending a sequential
request reserves GPU capacity. No switch to Full performance is made by the script.
The main agent may continue suitable work if local generation is blocked.

After interruption, inspect the board, logs and actual processes before requeueing.
If a crashed process left the lock, verify its PID is no longer running before
removing that single lock file. Never delete the junction recursively to reset
memory; preserve the vault and reconnect only after checking the existing target.

Script checks: `python -m unittest discover -s scripts/tests -q`. On restricted
Windows runners set TEMP/TMP to `E:\BIMCraft\target\test-temp` first. These tests
use fake model/check functions, including the bounded repair loop, so they do not
load the GPU or require a live model.

### Cargo по завершению хода

Для нового итогового прогона выполнить `python scripts/cargo_stop_hook.py fmt clippy test`
и завершить ход. Проектный Stop-хук забирает заявку, запускает проверки
последовательно и возвращает код завершения и путь к журналу в
`target/cargo-stop/`. Одна заявка на ход, до трёх шагов из
`fmt`, `check`, `build`, `clippy`, `test`. Для Qwen использовать
`PENDING_QWEN` по регламенту выше.

Для остальных долгих действий использовать ожидание завершения в самом
инструменте. Для другого чата это `wait_threads` с курсором, для уже
запущенной команды — одно длительное ожидание её сеанса. Stop-хук запускается
при завершении хода и обрабатывает явно поставленные заявки. У процессов,
запущенных вне Codex, должен быть собственный сигнал завершения. При таймауте
проверить журнал и состояние процесса перед повторным запуском.
