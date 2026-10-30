//! Opening a script to edit it. The editor set in PyAddins > Python, or else the system's, *for editing*: a `.py` file's own
//! association often runs it (Python on Windows) or hands it to something that does (Wine registers itself for `.py` on
//! Linux), which would run the script, or seem to do nothing. So Windows asks for its Edit verb, macOS for its text editor
//! (`open -t`), and Linux takes the `.py` handler only when it is an editor, else the one for plain text, and launches it
//! through `gio`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long a launcher has to fail before it is taken to have worked.
const SETTLE: Duration = Duration::from_secs(3);

/// Opens `path` in `editor` (a program, from Settings) or else the system's editor for it.
pub fn open(path: &Path, editor: Option<&str>) -> Result<(), String> {
    if let Some(editor) = editor.map(str::trim).filter(|e| !e.is_empty()) {
        let mut c = Command::new(editor);
        c.arg(path);
        return launch(c).map_err(|e| format!("the editor set, `{editor}`, {e}"));
    }
    system(path)
}

#[cfg(target_os = "windows")]
fn system(path: &Path) -> Result<(), String> {
    // The Edit verb: what Explorer's Edit opens a .py in (IDLE, Notepad, an IDE), never the interpreter.
    let quoted = path.display().to_string().replace('\'', "''");
    let mut c = Command::new("powershell");
    c.args(["-NoProfile", "-NonInteractive", "-Command", &format!("Start-Process -Verb Edit -FilePath '{quoted}'")]);
    hidden(&mut c);
    launch(c).or_else(|_| {
        let mut c = Command::new("notepad");
        c.arg(path);
        launch(c)
    })
}

#[cfg(target_os = "windows")]
fn hidden(c: &mut Command) {
    use std::os::windows::process::CommandExt;
    c.creation_flags(0x0800_0000);
}

#[cfg(target_os = "macos")]
fn system(path: &Path) -> Result<(), String> {
    let mut c = Command::new("open");
    c.arg("-t").arg(path);
    launch(c)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn system(path: &Path) -> Result<(), String> {
    let default = |mime: &str| {
        let mut c = Command::new("xdg-mime");
        c.args(["query", "default", mime]);
        crate::python::output(c, None, SETTLE, 4096).ok().map(|o| o.stdout.trim().to_string()).filter(|d| !d.is_empty())
    };
    let app = editor_of(default("text/x-python"), default("text/plain"))
        .ok_or("the system names no editor for Python or plain text: set one in PyAddins > Python")?;
    let file = desktop_file(&app).ok_or_else(|| format!("the system's editor, {app}, could not be found: set one in PyAddins > Python"))?;
    let mut c = Command::new("gio");
    c.arg("launch").arg(&file).arg(path);
    launch(c).or_else(|_| {
        let mut c = Command::new("gtk-launch");
        c.arg(&app).arg(path);
        launch(c)
    })
}

/// Whether the application a desktop entry names runs a file rather than opening it to edit.
pub fn runs_files(app: &str) -> bool {
    let a = app.to_lowercase();
    a.contains("wine") || (a.contains("python") && !a.contains("idle")) || a.contains("terminal") || a.contains("konsole")
}

/// The application to edit a script in: the `.py` handler when it is an editor, else the plain-text one.
pub fn editor_of(python: Option<String>, text: Option<String>) -> Option<String> {
    python.filter(|p| !runs_files(p)).or(text.filter(|t| !runs_files(t)))
}

/// Where a desktop entry is, in the places the freedesktop standard says (`XDG_DATA_HOME`, `XDG_DATA_DIRS`).
pub fn desktop_file(app: &str) -> Option<PathBuf> {
    if app.contains('/') || app.contains("..") {
        return None;
    }
    let home = std::env::var_os("XDG_DATA_HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    let dirs = std::env::var("XDG_DATA_DIRS").ok().filter(|d| !d.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    home.into_iter()
        .chain(dirs.split(':').map(PathBuf::from))
        .chain([PathBuf::from("/var/lib/flatpak/exports/share")])
        .map(|d| d.join("applications").join(app))
        .find(|p| p.is_file())
}

/// Starts `c`, and takes it to have worked unless it fails within [`SETTLE`]: a launcher that hands the file on ends at once,
/// an editor started itself keeps running.
fn launch(mut c: Command) -> Result<(), String> {
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = c.spawn().map_err(|e| format!("could not start: {e}"))?;
    let start = Instant::now();
    while start.elapsed() < SETTLE {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let mut why = String::new();
                if let Some(mut err) = child.stderr.take() {
                    use std::io::Read;
                    let _ = (&mut err).take(2000).read_to_string(&mut why);
                }
                let why = why.trim();
                return Err(if why.is_empty() { format!("failed ({status})") } else { format!("failed: {why}") });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(e.to_string()),
        }
    }
    // Still running: an editor started itself. It is left to run; a thread waits for it, so it leaves no zombie behind.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_runs_a_script_is_passed_over_for_an_editor() {
        let s = |x: &str| Some(x.to_string());
        assert_eq!(editor_of(s("wine-extension-py.desktop"), s("org.kde.kate.desktop")), s("org.kde.kate.desktop"));
        assert_eq!(editor_of(s("python3.12.desktop"), s("org.gnome.TextEditor.desktop")), s("org.gnome.TextEditor.desktop"));
        assert_eq!(editor_of(s("code.desktop"), s("org.kde.kate.desktop")), s("code.desktop"));
        assert_eq!(editor_of(s("idle-python3.12.desktop"), None), s("idle-python3.12.desktop"));
        assert_eq!(editor_of(None, s("org.kde.kate.desktop")), s("org.kde.kate.desktop"));
        assert_eq!(editor_of(s("wine-extension-py.desktop"), s("wine-extension-txt.desktop")), None);
        assert_eq!(editor_of(None, None), None);
    }

    #[test]
    fn a_desktop_entry_is_looked_for_where_the_standard_says_and_never_outside() {
        assert_eq!(desktop_file("../../etc/passwd"), None);
        assert_eq!(desktop_file("a/b.desktop"), None);
        assert_eq!(desktop_file("no-such-app-anywhere.desktop"), None);
    }

    #[cfg(unix)]
    #[test]
    fn the_editor_set_is_started_with_the_file_and_one_that_fails_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.py");
        std::fs::write(&file, "print(1)\n").unwrap();
        assert_eq!(open(&file, Some("true")), Ok(()));
        let e = open(&file, Some("false")).unwrap_err();
        assert!(e.starts_with("the editor set, `false`, failed"), "{e}");
        let e = open(&file, Some("/no/such/editor")).unwrap_err();
        assert!(e.contains("could not start"), "{e}");
        // One that keeps running (an editor) is taken to have worked, once it hasn't failed for a moment.
        let editor = dir.path().join("editor.sh");
        std::fs::write(&editor, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&editor, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let start = Instant::now();
        assert_eq!(open(&file, Some(&editor.display().to_string())), Ok(()));
        assert!(start.elapsed() < SETTLE * 3);
    }
}
