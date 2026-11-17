//! The user's Python: BIMCraft bundles none. The one File > Settings names, or else the first of `python3`, `python` (and
//! on Windows the `py -3` launcher first) that answers, and is 3.9 or newer. It is asked its version in a process of its
//! own, with a time limit, so a broken install can't hang the window.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The oldest Python a script runs in: what macOS has, and what every supported Linux has or installs.
pub const OLDEST: (u32, u32) = (3, 9);
/// How long Python has to say its version.
const ASK: Duration = Duration::from_secs(15);

/// A Python that answered: how to start it, and its version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Python {
    pub program: PathBuf,
    /// What comes before the script's arguments: `-3` for the Windows launcher.
    pub args: Vec<String>,
    pub version: (u32, u32, u32),
}

impl Python {
    /// The command that starts it, `-u` (its output not held back) and with what is given.
    pub fn command(&self) -> Command {
        let mut c = Command::new(&self.program);
        c.args(&self.args).arg("-u");
        c.env("PYTHONIOENCODING", "utf-8").env("PYTHONDONTWRITEBYTECODE", "1").env("PYTHONUTF8", "1");
        no_window(&mut c);
        c
    }

    /// What the window says it is: `Python 3.12.4 (python3)`.
    pub fn label(&self) -> String {
        let (a, b, c) = self.version;
        let mut how = self.program.display().to_string();
        for arg in &self.args {
            how.push(' ');
            how.push_str(arg);
        }
        format!("Python {a}.{b}.{c} ({how})")
    }
}

/// A child process on Windows opens no console window of its own.
#[cfg(windows)]
fn no_window(c: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    c.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn no_window(_: &mut Command) {}

/// The Pythons tried, in order, when none is set.
fn candidates() -> Vec<(PathBuf, Vec<String>)> {
    let mut c: Vec<(PathBuf, Vec<String>)> = Vec::new();
    if cfg!(windows) {
        c.push(("py".into(), vec!["-3".into()]));
    }
    c.push(("python3".into(), Vec::new()));
    c.push(("python".into(), Vec::new()));
    c
}

/// The Python to run scripts in: `set` (a path or a program on the PATH, from File > Settings) alone when there is one,
/// else the first of the usual ones that answers and is new enough. The error says how to get one.
pub fn find(set: Option<&str>) -> Result<Python, String> {
    if let Some(set) = set.map(str::trim).filter(|s| !s.is_empty()) {
        return ask(PathBuf::from(set), Vec::new()).map_err(|e| format!("the Python set in Settings, `{set}`, {e}")).and_then(new_enough);
    }
    let mut old = None;
    for (program, args) in candidates() {
        match ask(program, args).map(new_enough) {
            Ok(Ok(p)) => return Ok(p),
            Ok(Err(e)) => old = old.or(Some(e)),
            Err(_) => {}
        }
    }
    Err(old.unwrap_or_else(|| {
        "no Python found: install Python 3.9 or newer (python.org, or your system's packages), or name it in PyAddins > Python".to_string()
    }))
}

fn new_enough(p: Python) -> Result<Python, String> {
    let (major, minor, _) = p.version;
    if (major, minor) >= OLDEST {
        Ok(p)
    } else {
        Err(format!("{} is too old: PyAddins need Python {}.{} or newer", p.label(), OLDEST.0, OLDEST.1))
    }
}

/// Asks `program` its version.
fn ask(program: PathBuf, args: Vec<String>) -> Result<Python, String> {
    let mut c = Command::new(&program);
    c.args(&args).args(["-c", "import sys; print('%d %d %d' % sys.version_info[:3])"]);
    no_window(&mut c);
    let out = output(c, None, ASK, 4096)?;
    if !out.ok {
        return Err(format!("did not answer as Python does: {}", out.stderr.trim()));
    }
    let n: Vec<u32> = out.stdout.split_whitespace().filter_map(|n| n.parse().ok()).collect();
    match n[..] {
        [a, b, c] => Ok(Python { program, args, version: (a, b, c) }),
        _ => Err("did not say a version".into()),
    }
}

/// What a process said, and whether it ended well.
#[derive(Debug)]
pub struct Output {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `c` to its end, `input` on its standard input, within `limit` (it is killed past it), each of its outputs read to
/// `cap` bytes.
pub fn output(mut c: Command, input: Option<&[u8]>, limit: Duration, cap: u64) -> Result<Output, String> {
    c.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn().map_err(|e| format!("could not start: {e}"))?;
    if let (Some(bytes), Some(mut stdin)) = (input, child.stdin.take()) {
        let bytes = bytes.to_vec();
        std::thread::spawn(move || {
            use std::io::Write;
            let _ = stdin.write_all(&bytes);
        });
    }
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = Vec::new();
            if let Some(pipe) = pipe {
                let mut pipe = pipe.take(cap);
                let _ = pipe.read_to_end(&mut text);
                // What is past the cap is read and thrown away, so the process is never stuck writing it.
                let _ = std::io::copy(&mut pipe.into_inner(), &mut std::io::sink());
            }
            String::from_utf8_lossy(&text).into_owned()
        })
    };
    let out = read(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err = read(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < limit => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("took longer than {} seconds and was stopped", limit.as_secs()));
            }
        }
    };
    Ok(Output { ok: status.success(), stdout: out.join().unwrap_or_default(), stderr: err.join().unwrap_or_default() })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The machine's Python, for the tests that need one: CI's runners all have it. Without one, they say so and pass.
    /// `BIMCRAFT_TEST_PYTHON` runs them in another (the oldest, 3.9, say).
    pub(crate) fn python() -> Option<Python> {
        match find(std::env::var("BIMCRAFT_TEST_PYTHON").ok().as_deref()) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("skipped, no Python here: {e}");
                None
            }
        }
    }

    #[test]
    fn the_python_set_is_the_only_one_tried_and_what_is_wrong_with_it_is_said() {
        let e = find(Some("/no/such/python")).unwrap_err();
        assert!(e.starts_with("the Python set in Settings, `/no/such/python`, could not start"), "{e}");
        // A program that isn't Python.
        if cfg!(unix) {
            let e = find(Some("true")).unwrap_err();
            assert!(e.contains("did not say a version"), "{e}");
            let e = find(Some("false")).unwrap_err();
            assert!(e.contains("did not answer"), "{e}");
        }
        let Some(p) = python() else { return };
        assert!((p.version.0, p.version.1) >= OLDEST, "{p:?}");
        assert!(p.label().starts_with("Python 3."), "{}", p.label());
        // Set by its path, it is the one.
        let set = find(Some(&p.program.display().to_string())).unwrap();
        assert_eq!(set.version, p.version);
    }

    #[test]
    fn too_old_is_said_with_the_version_it_is() {
        let p = Python { program: "python".into(), args: Vec::new(), version: (3, 8, 18) };
        let e = new_enough(p).unwrap_err();
        assert!(e.contains("3.8.18") && e.contains("3.9 or newer"), "{e}");
        assert!(new_enough(Python { program: "py".into(), args: vec!["-3".into()], version: (3, 9, 0) }).is_ok());
        assert!(new_enough(Python { program: "p".into(), args: Vec::new(), version: (4, 0, 0) }).is_ok());
    }

    #[test]
    fn a_process_past_its_time_is_stopped_and_its_output_is_capped() {
        if !cfg!(unix) {
            return;
        }
        let mut c = Command::new("sh");
        c.args(["-c", "sleep 5"]);
        let e = output(c, None, Duration::from_millis(200), 100).unwrap_err();
        assert!(e.contains("stopped"), "{e}");
        let mut c = Command::new("sh");
        c.args(["-c", "yes | head -c 100000; echo done >&2"]);
        let out = output(c, None, Duration::from_secs(10), 1000).unwrap();
        assert_eq!(out.stdout.len(), 1000);
        assert_eq!(out.stderr.trim(), "done");
        let mut c = Command::new("cat");
        c.arg("-");
        let out = output(c, Some(b"hello"), Duration::from_secs(10), 1000).unwrap();
        assert_eq!(out.stdout, "hello");
    }
}
