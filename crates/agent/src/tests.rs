use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::json;

use super::*;

fn job() -> Job {
    Job {
        instructions: "# Do it".into(),
        schema: json!({"type": "object"}),
        files: vec![("brief.json".into(), "{\"rows\": []}".into()), ("Uniclass-Ss.csv".into(), "Code,Title\n".into())],
        inline: "{\"rows\": []}".into(),
    }
}

#[test]
fn a_command_line_is_split_as_a_person_types_it_and_no_shell_reads_it() {
    assert_eq!(split_args(r#"my-agent --flag "two words" 'x y' a\ b $HOME"#), ["my-agent", "--flag", "two words", "x y", "a b", "$HOME"]);
    assert_eq!(split_args("  "), Vec::<String>::new());
    assert_eq!(split_args(r#"a "" b"#), ["a", "", "b"]);
}

#[test]
fn a_program_is_found_on_the_path_or_where_installers_put_it_and_a_path_is_itself() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let name = if cfg!(windows) { "fake-agent.cmd" } else { "fake-agent" };
    std::fs::write(bin.join(name), "x").unwrap();
    assert_eq!(resolve_in("fake-agent", vec![], vec![bin.clone()]), Some(bin.join(name)), "found in the installers' folders");
    assert_eq!(resolve_in("fake-agent", vec![bin.clone()], vec![]), Some(bin.join(name)));
    assert_eq!(resolve_in("missing-agent", vec![bin.clone()], vec![]), None);
    assert_eq!(resolve_in(&bin.join(name).display().to_string(), vec![], vec![]), Some(bin.join(name)));
    assert_eq!(resolve_in("", vec![bin], vec![]), None);
}

/// A fake agent: a script that prints `printed` and writes `file` to answer.json when given.
#[cfg(unix)]
fn fake(dir: &std::path::Path, body: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("agent.sh");
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p.display().to_string()
}

#[cfg(unix)]
#[test]
fn claude_is_asked_with_read_only_tools_and_its_structured_output_is_the_answer() {
    let dir = tempfile::tempdir().unwrap();
    let out = r#"{"type":"result","is_error":false,"result":"done","structured_output":{"rows":[{"key":"k"}]}}"#;
    let agent = fake(dir.path(), &format!("cat > /dev/null; echo '{out}'"));
    let s = Settings { preset: Preset::Claude, command: agent, model: "sonnet".into(), ..Settings::default() };
    let call = prepare(&s, &job(), &dir.path().join("task")).unwrap();
    assert!(call.args.windows(2).any(|w| w == ["--tools", "Read,Grep,Glob"]), "{:?}", call.args);
    assert!(call.args.windows(2).any(|w| w == ["--model", "sonnet"]));
    assert!(call.stdin.contains("instructions.md"));
    assert!(dir.path().join("task/Uniclass-Ss.csv").exists());
    let o = run(&call, &Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&o.answer).unwrap()["rows"][0]["key"], "k");
}

#[cfg(unix)]
#[test]
fn codex_writes_its_answer_to_a_file_and_reads_its_prompt_from_standard_input() {
    let dir = tempfile::tempdir().unwrap();
    // The fake writes what follows -o.
    let agent = fake(dir.path(), r#"cat > /dev/null; while [ "$1" != "-o" ]; do shift; done; echo '{"rows":[]}' > "$2"; echo noise"#);
    let s = Settings { preset: Preset::Codex, command: agent, ..Settings::default() };
    let call = prepare(&s, &job(), dir.path()).unwrap();
    assert_eq!(call.args.last().map(String::as_str), Some("-"));
    assert!(call.args.contains(&"read-only".to_string()));
    let o = run(&call, &Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(o.answer.trim(), r#"{"rows":[]}"#);
}

#[cfg(unix)]
#[test]
fn ollama_is_given_the_brief_in_its_prompt_and_needs_a_model() {
    let dir = tempfile::tempdir().unwrap();
    let agent = fake(dir.path(), "cat");
    let mut s = Settings { preset: Preset::Ollama, command: agent, ..Settings::default() };
    assert!(prepare(&s, &job(), dir.path()).unwrap_err().contains("model"));
    s.model = "qwen2.5".into();
    let call = prepare(&s, &job(), dir.path()).unwrap();
    assert_eq!(call.args[..4], ["run", "qwen2.5", "--format", "json"]);
    let o = run(&call, &Arc::new(AtomicBool::new(false))).unwrap();
    assert!(o.answer.contains("# Do it") && o.answer.contains("## Brief"), "the fake echoes its prompt: {}", o.answer);
}

#[cfg(unix)]
#[test]
fn a_custom_command_has_the_tasks_files_put_in() {
    let dir = tempfile::tempdir().unwrap();
    let agent = fake(dir.path(), r#"cat "$1" > /dev/null && echo '{"rows":[1]}' > "$2""#);
    let s = Settings { preset: Preset::Custom, command: format!("{agent} {{prompt_file}} {{answer_file}}"), ..Settings::default() };
    let call = prepare(&s, &job(), dir.path()).unwrap();
    assert!(call.args[0].ends_with("instructions.md"));
    let o = run(&call, &Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(o.answer.trim(), r#"{"rows":[1]}"#);
}

#[cfg(unix)]
#[test]
fn an_agent_that_fails_takes_too_long_or_is_cancelled_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let failing = fake(dir.path(), "echo 'not logged in' >&2; exit 3");
    let s = Settings { preset: Preset::Custom, command: failing, ..Settings::default() };
    let e = run(&prepare(&s, &job(), dir.path()).unwrap(), &Arc::new(AtomicBool::new(false))).unwrap_err();
    assert!(e.contains("not logged in"), "{e}");
    let dir2 = tempfile::tempdir().unwrap();
    let slow = fake(dir2.path(), "sleep 30");
    let s = Settings { preset: Preset::Custom, command: slow, ..Settings::default() };
    let mut call = prepare(&s, &job(), dir2.path()).unwrap();
    call.timeout = Duration::from_millis(300);
    assert!(run(&call, &Arc::new(AtomicBool::new(false))).unwrap_err().contains("no answer after"));
    call.timeout = Duration::from_secs(60);
    let cancel = Arc::new(AtomicBool::new(false));
    let c2 = Arc::clone(&cancel);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        c2.store(true, Ordering::Relaxed);
    });
    let t = Instant::now();
    assert_eq!(run(&call, &cancel).unwrap_err(), "cancelled");
    assert!(t.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_missing_agent_and_a_bad_file_name_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let s = Settings { preset: Preset::Claude, command: "surely-no-such-agent-here".into(), ..Settings::default() };
    assert!(prepare(&s, &job(), dir.path()).unwrap_err().contains("isn't installed"));
    let mut j = job();
    j.files.push(("../escape.txt".into(), "x".into()));
    assert!(prepare(&s, &j, dir.path()).unwrap_err().contains("isn't a file name"));
    assert!(!dir.path().parent().unwrap().join("escape.txt").exists());
}

#[test]
fn claudes_error_result_is_an_error() {
    assert!(unwrap_claude(r#"{"is_error":true,"result":"Credit balance is too low"}"#).unwrap_err().contains("Credit"));
    assert_eq!(unwrap_claude(r#"{"result":"{\"rows\":[]}"}"#).unwrap(), r#"{"rows":[]}"#);
    assert_eq!(unwrap_claude("plain").unwrap(), "plain");
}
