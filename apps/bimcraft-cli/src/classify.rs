//! `bimcraft-cli classify`: a project's types (and the elements of tables of each element's) classified by the user's AI
//! agent, as the desktop's Classification Manager does it, with no window: the brief made and cut into batches, each run by
//! the agent, its answer checked against the project, and what it proposes with confidence enough written as one step.

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use bimcraft_agent::{self as agent, Preset};
use bimcraft_core::classification::{self as cls, BriefAsk};
use bimcraft_core::engine::Session;
use serde_json::{Value, json};

/// What `classify` is told after its file.
struct Options {
    settings: agent::Settings,
    tables: Vec<String>,
    all: bool,
    min: f64,
    dry_run: bool,
    save: Option<String>,
}

fn options(args: &[String], usage: &str) -> Result<Options, String> {
    let mut o = Options { settings: agent::Settings::default(), tables: Vec::new(), all: false, min: 0.7, dry_run: false, save: None };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value\n\n{usage}"));
        match arg.as_str() {
            "--agent" => {
                let v = value()?;
                o.settings.preset = match v.to_lowercase().as_str() {
                    "claude" => Preset::Claude,
                    "codex" => Preset::Codex,
                    "ollama" => Preset::Ollama,
                    "custom" => Preset::Custom,
                    other => return Err(format!("--agent is claude, codex, ollama or custom, not `{other}`")),
                };
            }
            "--command" => o.settings.command = value()?,
            "--model" => o.settings.model = value()?,
            "--args" => o.settings.args = value()?,
            "--timeout" => o.settings.timeout = value()?.parse().map_err(|_| "--timeout is seconds")?,
            "--batch" => o.settings.batch = value()?.parse().map_err(|_| "--batch is a number of rows")?,
            "--table" => o.tables.push(value()?),
            "--all" => o.all = true,
            "--min" => {
                o.min = value()?.parse::<f64>().ok().filter(|x| x.is_finite() && (0.0..=1.0).contains(x)).ok_or("--min is from 0 to 1")?;
            }
            "--dry-run" => o.dry_run = true,
            "--save" => o.save = Some(value()?),
            other => return Err(format!("unknown option `{other}`\n\n{usage}")),
        }
    }
    Ok(o)
}

/// Classifies the project `s` holds as `args` say, saying how each batch went on `out`.
pub fn run(s: &mut Session, args: &[String], out: &mut impl Write, usage: &str) -> Result<(), String> {
    let o = options(args, usage)?;
    let say = |out: &mut dyn Write, v: Value| writeln!(out, "{v}").map_err(|e| format!("stdout: {e}"));
    let ask = BriefAsk { tables: o.tables.clone(), rows: Some(if o.all { "all" } else { "unclassified" }.into()), ..BriefAsk::default() };
    let brief = cls::brief(&s.model, &ask)?;
    if brief.rows.is_empty() {
        return say(out, json!({"classified": 0, "note": "nothing to classify: everything asked for has a code"}));
    }
    let reads = o.settings.preset.reads_files();
    let parts = brief.chunks(o.settings.batch());
    let base = std::env::temp_dir().join(format!("bimcraft-cli-classify-{}", std::process::id()));
    let cancel = Arc::new(AtomicBool::new(false));
    let (mut proposals, mut refused) = (Vec::new(), Vec::new());
    for (n, part) in parts.iter().enumerate() {
        let h = cls::handover(&s.model, part, reads);
        let job = agent::Job { instructions: h.instructions, schema: cls::answer_schema(), files: h.files, inline: h.inline };
        let call = agent::prepare(&o.settings, &job, &base.join(format!("batch-{n:03}")));
        let done = call.and_then(|c| agent::run(&c, &cancel));
        let output = match done {
            Ok(x) => x,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&base);
                return Err(format!("batch {} of {}: {e}", n + 1, parts.len()));
            }
        };
        let Some(answer) = cls::json_in(&output.answer) else {
            refused.push(format!("batch {}: the answer held no JSON", n + 1));
            continue;
        };
        let read = cls::read_answer(&s.model, &answer);
        say(
            out,
            json!({"batch": n + 1, "of": parts.len(), "rows": part.rows.len(), "proposals": read.proposals.len(),
                        "refused": read.refused.len(), "seconds": output.took.as_secs()}),
        )?;
        proposals.extend(read.proposals);
        refused.extend(read.refused);
    }
    let _ = std::fs::remove_dir_all(&base);
    let chosen: Vec<&cls::Proposal> =
        proposals.iter().filter(|p| p.code.is_some() && p.confidence >= o.min && p.code != p.current).collect();
    let assign: Vec<Value> = chosen.iter().map(|p| json!({"target": p.key, "table": p.table, "code": p.code})).collect();
    let mut summary = json!({"proposals": proposals, "refused": refused, "chosen": chosen.len(), "min": o.min});
    if !o.dry_run && !assign.is_empty() {
        let r = s.execute("classify", &json!({"assign": assign}))?;
        summary["written"] = json!(r["rows"].as_array().map_or(0, Vec::len));
    }
    if let Some(path) = &o.save {
        s.execute("save", &json!({"path": path}))?;
    }
    say(out, summary)
}
