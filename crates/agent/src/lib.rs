//! The AI agent a user already has, run on a task: Claude Code, Codex, Ollama, or a command line of their own.
//!
//! BIMCraft opens no connection here. It writes a task's files into a folder of its own (the instructions, the brief, the
//! tables, the answer's JSON Schema), runs the command the user set in Settings > Agent in that folder, with no shell and
//! read-only tools where the agent has a choice, and reads the answer it prints or writes. What leaves the machine is what
//! that program sends to its provider: the window says so before the first run.
//!
//! What comes back is hostile: its size is capped, it's waited for up to a timeout and can be cancelled, and the caller
//! checks every row of it against the project before anything is written.

use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The most an agent's output is read: what's past it is dropped.
pub const MAX_OUTPUT: usize = 16 << 20;
/// The longest an agent is waited for, whatever the settings say.
pub const MAX_TIMEOUT: u64 = 6 * 3600;
/// The longest a command line the user types.
pub const MAX_COMMAND: usize = 4096;

/// The command lines BIMCraft knows how to call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    /// Anthropic's Claude Code: `claude -p`, its tools held to reading the task's folder.
    #[default]
    Claude,
    /// OpenAI's Codex: `codex exec`, in a read-only sandbox.
    Codex,
    /// A model Ollama runs on this machine (or where it's told): `ollama run`. It reads no files, so the brief goes in the prompt.
    Ollama,
    /// A command line of the user's, with `{prompt_file}`, `{schema_file}`, `{answer_file}`, `{dir}` and `{model}` put in.
    Custom,
}

impl Preset {
    pub const ALL: [Preset; 4] = [Preset::Claude, Preset::Codex, Preset::Ollama, Preset::Custom];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Claude => "Claude Code",
            Preset::Codex => "Codex",
            Preset::Ollama => "Ollama",
            Preset::Custom => "Custom",
        }
    }

    /// The program it runs, unless the user names another.
    pub fn program(self) -> &'static str {
        match self {
            Preset::Claude => "claude",
            Preset::Codex => "codex",
            Preset::Ollama => "ollama",
            Preset::Custom => "",
        }
    }

    /// Whether the agent reads the task's files itself; one that doesn't is given the brief in its prompt.
    pub fn reads_files(self) -> bool {
        !matches!(self, Preset::Ollama)
    }

    /// How many rows of a brief one call is given.
    pub fn batch(self) -> usize {
        match self {
            Preset::Ollama => 20,
            _ => 150,
        }
    }
}

/// What Settings > Agent holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub preset: Preset,
    /// The program: `claude`, a path, or for Custom the whole command line.
    pub command: String,
    /// The model to ask for; empty for the agent's own default. Ollama needs one.
    pub model: String,
    /// More arguments, put after the preset's.
    pub args: String,
    /// How long one call is waited for, in seconds.
    pub timeout: u64,
    /// How many rows of a brief one call is given; 0 for the preset's.
    pub batch: usize,
    /// Whether the user has read what leaves the machine.
    pub consented: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            preset: Preset::Claude,
            command: String::new(),
            model: String::new(),
            args: String::new(),
            timeout: 900,
            batch: 0,
            consented: false,
        }
    }
}

impl Settings {
    /// The rows one call is given.
    pub fn batch(&self) -> usize {
        if self.batch == 0 { self.preset.batch() } else { self.batch.clamp(1, 5000) }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout.clamp(10, MAX_TIMEOUT))
    }

    /// The program to run, as typed or the preset's.
    pub fn program(&self) -> String {
        let typed = self.command.trim();
        if self.preset == Preset::Custom {
            return split_args(typed).into_iter().next().unwrap_or_default();
        }
        if typed.is_empty() { self.preset.program().to_string() } else { typed.to_string() }
    }
}

/// A task for an agent: what it's told and the files it reads.
#[derive(Clone, Debug, Default)]
pub struct Job {
    /// The instructions, in Markdown.
    pub instructions: String,
    /// The answer's JSON Schema.
    pub schema: Value,
    /// The files beside them, by name: the brief, the tables.
    pub files: Vec<(String, String)>,
    /// What an agent that reads no files is given after its instructions: the brief, cut to what it needs.
    pub inline: String,
}

/// A call made ready: the program, its arguments and input, and where its answer will be.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub stdin: String,
    pub dir: PathBuf,
    /// The file the answer is written to, for the agents that write one.
    pub answer_file: Option<PathBuf>,
    pub preset: Preset,
    pub timeout: Duration,
}

impl Call {
    /// The call as a person would type it, for the log.
    pub fn shown(&self) -> String {
        let quote = |a: &str| if a.contains(' ') || a.is_empty() || a.len() > 80 { format!("'{}'", short(a, 80)) } else { a.to_string() };
        std::iter::once(self.program.display().to_string()).chain(self.args.iter().map(|a| quote(a))).collect::<Vec<_>>().join(" ")
    }
}

fn short(t: &str, n: usize) -> String {
    if t.chars().count() <= n { t.to_string() } else { format!("{}…", t.chars().take(n).collect::<String>()) }
}

/// A command line's words: split at spaces, with `"…"` and `'…'` keeping theirs, and `\` before a quote or a space
/// keeping it. No shell reads it: `$HOME`, `*` and `|` are themselves.
pub fn split_args(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (_, '\\') if chars.peek().is_some_and(|n| matches!(n, '"' | '\'' | ' ' | '\\')) => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                    started = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            (_, c) => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}

/// The folders a program is looked for in besides `PATH`: where the agents' installers put them, which an app started
/// from the desktop (macOS's Finder above all) doesn't have on its `PATH`.
fn more_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    if let Some(h) = &home {
        for d in [".local/bin", ".claude/local", ".npm-global/bin", ".bun/bin", ".cargo/bin", ".volta/bin"] {
            dirs.push(h.join(d));
        }
    }
    for d in ["/usr/local/bin", "/opt/homebrew/bin", "/usr/bin", "/snap/bin"] {
        dirs.push(PathBuf::from(d));
    }
    if cfg!(windows) {
        if let Some(a) = std::env::var_os("APPDATA") {
            dirs.push(PathBuf::from(a).join("npm"));
        }
        if let Some(l) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            dirs.push(l.join("Programs").join("Ollama"));
            dirs.push(l.join("Microsoft").join("WindowsApps"));
        }
        if let Some(h) = &home {
            dirs.push(h.join(".local").join("bin"));
        }
    }
    dirs
}

/// The extensions a program may have: on Windows, `PATHEXT`'s (npm installs `claude.cmd`, which `claude` alone doesn't
/// find); elsewhere none.
fn extensions() -> Vec<String> {
    if !cfg!(windows) {
        return vec![String::new()];
    }
    let ext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    std::iter::once(String::new()).chain(ext.split(';').filter(|e| !e.is_empty()).map(|e| e.to_lowercase())).collect()
}

fn runnable(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file())
}

/// Where `program` is: itself, when it's a path; otherwise the first folder of `PATH` (then of where installers put
/// agents) that has it, with any of Windows' extensions.
pub fn resolve(program: &str) -> Option<PathBuf> {
    resolve_in(program, std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default(), more_dirs())
}

fn resolve_in(program: &str, path: Vec<PathBuf>, more: Vec<PathBuf>) -> Option<PathBuf> {
    let program = program.trim();
    if program.is_empty() || program.len() > MAX_COMMAND {
        return None;
    }
    let exts = extensions();
    let with = |base: &Path| exts.iter().map(|e| PathBuf::from(format!("{}{e}", base.display()))).find(|p| runnable(p));
    if program.contains('/') || program.contains('\\') {
        return with(Path::new(program));
    }
    path.into_iter().chain(more).find_map(|d| with(&d.join(program)))
}

/// The prompt given on standard input: where to read the task, or for an agent that reads no files, all of it.
fn prompt(job: &Job, reads_files: bool) -> String {
    if reads_files {
        "Read instructions.md in the current folder and do what it says. The brief is brief.json; the tables are the .csv files \
         beside it. Answer only with the JSON object instructions.md describes."
            .to_string()
    } else {
        format!("{}\n\n## Brief\n\n{}\n", job.instructions, job.inline)
    }
}

/// `job`'s files written into `dir`, and the call that runs the agent on them.
pub fn prepare(settings: &Settings, job: &Job, dir: &Path) -> Result<Call, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let write = |name: &str, text: &str| -> Result<PathBuf, String> {
        // Only a plain file name: nothing a brief names can land outside the folder.
        if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
            return Err(format!("`{name}` isn't a file name"));
        }
        let p = dir.join(name);
        std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))?;
        Ok(p)
    };
    let prompt_file = write("instructions.md", &job.instructions)?;
    let schema_text = serde_json::to_string(&job.schema).map_err(|e| e.to_string())?;
    let schema_file = write("schema.json", &schema_text)?;
    for (name, text) in &job.files {
        write(name, text)?;
    }
    let answer = dir.join("answer.json");
    let _ = std::fs::remove_file(&answer);
    let program_name = settings.program();
    let program = resolve(&program_name).ok_or_else(|| {
        if program_name.is_empty() {
            "no command is set: set one in Settings > Agent".to_string()
        } else {
            format!(
                "`{program_name}` isn't installed, or isn't where BIMCraft looks: install it, or give its full path in Settings > Agent"
            )
        }
    })?;
    let model = settings.model.trim().to_string();
    let extra = split_args(&settings.args);
    let mut args: Vec<String> = Vec::new();
    let mut answer_file = None;
    let preset = settings.preset;
    match preset {
        Preset::Claude => {
            args.extend(
                ["-p", "--output-format", "json", "--tools", "Read,Grep,Glob", "--strict-mcp-config", "--no-session-persistence"]
                    .map(String::from),
            );
            args.push("--json-schema".into());
            args.push(schema_text.clone());
            if !model.is_empty() {
                args.extend(["--model".into(), model.clone()]);
            }
        }
        Preset::Codex => {
            args.extend(["exec", "--sandbox", "read-only", "--skip-git-repo-check", "--ephemeral", "--color", "never"].map(String::from));
            args.extend(["--output-schema".into(), schema_file.display().to_string()]);
            args.extend(["-o".into(), answer.display().to_string()]);
            if !model.is_empty() {
                args.extend(["-m".into(), model.clone()]);
            }
            answer_file = Some(answer.clone());
        }
        Preset::Ollama => {
            if model.is_empty() {
                return Err("Ollama runs the model you name: set one in Settings > Agent (`ollama list` shows yours)".into());
            }
            args.extend(["run".into(), model.clone(), "--format".into(), "json".into()]);
        }
        Preset::Custom => {
            let words = split_args(settings.command.trim());
            let put = |w: &str| {
                w.replace("{prompt_file}", &prompt_file.display().to_string())
                    .replace("{schema_file}", &schema_file.display().to_string())
                    .replace("{answer_file}", &answer.display().to_string())
                    .replace("{dir}", &dir.display().to_string())
                    .replace("{model}", &model)
            };
            args.extend(words.iter().skip(1).map(|w| put(w)));
            answer_file = Some(answer.clone());
        }
    }
    args.extend(extra);
    if preset == Preset::Codex {
        // The prompt comes after the options, from standard input.
        args.push("-".into());
    }
    Ok(Call {
        program,
        args,
        stdin: prompt(job, preset.reads_files()),
        dir: dir.to_path_buf(),
        answer_file,
        preset,
        timeout: settings.timeout(),
    })
}

/// What a run ended with.
#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    /// The answer's text: what the agent wrote to its answer file, or printed.
    pub answer: String,
    /// What it said on its error stream, cut short.
    pub log: String,
    pub took: Duration,
}

fn read_capped(mut r: impl Read) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 65536];
    while let Ok(n) = r.read(&mut buf) {
        if n == 0 {
            break;
        }
        if out.len() < MAX_OUTPUT {
            let room = MAX_OUTPUT - out.len();
            out.extend_from_slice(&buf[..n.min(room)]);
        }
    }
    out
}

#[cfg(windows)]
fn detach(c: &mut Command) {
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW: a console program run from the window opens no console of its own.
    c.creation_flags(0x0800_0000);
}

/// A group of its own, so stopping it stops what it started too (an agent's tools, a shell's children).
#[cfg(unix)]
fn detach(c: &mut Command) {
    use std::os::unix::process::CommandExt;
    c.process_group(0);
}

#[cfg(not(any(unix, windows)))]
fn detach(_: &mut Command) {}

/// The child stopped, and on Unix its group with it.
fn stop(child: &mut std::process::Child) {
    // The group the child leads (`detach` made it), and only that: never through a `kill` program, which may read `-<pgid>`
    // as something else and signal the caller's own group.
    #[cfg(unix)]
    if let Some(group) = i32::try_from(child.id()).ok().filter(|&p| p > 1).and_then(rustix::process::Pid::from_raw) {
        let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// `call` run: until it ends, its timeout passes, or `cancel` is set, whichever is first; the answer read.
pub fn run(call: &Call, cancel: &Arc<AtomicBool>) -> Result<Output, String> {
    let start = Instant::now();
    let mut cmd = Command::new(&call.program);
    cmd.args(&call.args).current_dir(&call.dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // No colours and no questions: nobody is there to answer them.
    cmd.env("NO_COLOR", "1").env("TERM", "dumb").env("CI", "1");
    detach(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("{} can't be run: {e}", call.program.display()))?;
    if let Some(mut stdin) = child.stdin.take() {
        let text = call.stdin.clone();
        std::thread::spawn(move || {
            let _ = stdin.write_all(text.as_bytes());
        });
    }
    let out = child.stdout.take().map(|o| std::thread::spawn(move || read_capped(o)));
    let err = child.stderr.take().map(|e| std::thread::spawn(move || read_capped(e)));
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Ok(s),
            Ok(None) => {}
            Err(e) => break Err(e.to_string()),
        }
        // Stopped, it isn't waited for: what it started may still hold its output open.
        if cancel.load(Ordering::Relaxed) {
            stop(&mut child);
            return Err("cancelled".to_string());
        }
        if start.elapsed() > call.timeout {
            stop(&mut child);
            return Err(format!("no answer after {} s; it was stopped (Settings > Agent sets how long to wait)", call.timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let stdout = out.and_then(|h| h.join().ok()).unwrap_or_default();
    let stderr = err.and_then(|h| h.join().ok()).unwrap_or_default();
    let log = short(&String::from_utf8_lossy(&stderr), 4000);
    let status = status?;
    let printed = String::from_utf8_lossy(&stdout).into_owned();
    if !status.success() {
        let said = if log.trim().is_empty() { short(printed.trim(), 2000) } else { log.clone() };
        return Err(format!("{} stopped with {status}: {said}", call.program.display()));
    }
    let from_file = call
        .answer_file
        .as_ref()
        .and_then(|f| std::fs::metadata(f).ok().filter(|m| m.len() as usize <= MAX_OUTPUT).and_then(|_| std::fs::read_to_string(f).ok()))
        .filter(|t| !t.trim().is_empty());
    let answer = match from_file {
        Some(t) => t,
        None if call.preset == Preset::Claude => unwrap_claude(&printed)?,
        None => printed,
    };
    Ok(Output { answer, log, took: start.elapsed() })
}

/// Claude Code's `--output-format json` result: its structured output when it gave one, else its text.
pub fn unwrap_claude(printed: &str) -> Result<String, String> {
    let Ok(v) = serde_json::from_str::<Value>(printed.trim()) else { return Ok(printed.to_string()) };
    if v.get("is_error").and_then(Value::as_bool) == Some(true) {
        let why = v.get("result").and_then(Value::as_str).unwrap_or("it gave no reason");
        return Err(format!("Claude Code answered with an error: {}", short(why, 2000)));
    }
    if let Some(s) = v.get("structured_output").filter(|s| s.is_object()) {
        return Ok(s.to_string());
    }
    Ok(v.get("result").and_then(Value::as_str).map_or_else(|| printed.to_string(), String::from))
}

/// What `program --version` says: whether the agent is there, and which.
pub fn version(settings: &Settings) -> Result<String, String> {
    let name = settings.program();
    let program = resolve(&name).ok_or_else(|| format!("`{name}` isn't found on this machine"))?;
    let call = Call {
        program: program.clone(),
        args: vec!["--version".into()],
        stdin: String::new(),
        dir: std::env::temp_dir(),
        answer_file: None,
        preset: Preset::Custom,
        timeout: Duration::from_secs(30),
    };
    let out = run(&call, &Arc::new(AtomicBool::new(false)))?;
    let said = out.answer.trim();
    let said = if said.is_empty() { out.log.trim() } else { said };
    Ok(format!("{}: {}", program.display(), short(said.lines().next().unwrap_or_default(), 200)))
}

#[cfg(test)]
mod tests;
