# bimcraft-agent

Runs the AI agent a user already has (Claude Code, Codex, Ollama, or a command line of their own) on a task's files, and reads
its answer. The desktop's Classification Manager and `bimcraft-cli classify` use it; `bimcraft-core` never does, and the core
makes the task (the brief, the instructions, the tables, the answer's schema) without knowing who runs it. The write-up is
[classification](../../docs/areas/classification.md).

- **BIMCraft opens no connection here.** The agent's connections are its own; the window asks before the first run and says what
  is sent. Nothing runs unless the user asks.
- **No shell.** The command line is split by `split_args` and run as a program with arguments: `$HOME`, `*` and `|` are themselves.
  A file a task names is a plain name in its own folder, never a path out of it.
- **What comes back is hostile.** Output is capped (`MAX_OUTPUT`), a run has a timeout and a Cancel, and stopping one stops its
  process group (an agent's tools, a shell's children). The caller checks every row of an answer against the project.
- **Each preset's flags were checked against the installed CLIs** (`claude --help`, `codex exec --help`): read-only tools or
  sandbox, a JSON Schema for the answer, no saved session. A new preset says where its flags come from.
- Tests run fake agents (shell scripts, on Unix) and never a real one: `cargo test -p bimcraft-agent`.
