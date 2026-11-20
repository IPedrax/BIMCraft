//! The folder of PyAddins: the Player's scripts, a `.py` file each, and the ribbon's buttons made from them
//! (`buttons.json`). It is the user's, beside the favorites and the settings, and never a project's: a project file carries
//! no code. The window reads it again when it changes, so a script an agent saves (with the MCP server's `pyaddin_save`, or
//! by writing the file) is in the Player a moment later.
//!
//! What is in it is read as hostile as any file: a name is a few safe characters, a script and the buttons file are
//! capped, and a button that is not one is left out.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{Map, Value, json};

/// The longest script kept: a megabyte of Python is a program, not a script.
pub const MAX_SCRIPT: u64 = 1 << 20;
/// The most scripts listed.
pub const MAX_SCRIPTS: usize = 500;
/// The most buttons, and the longest of what a button says.
pub const MAX_BUTTONS: usize = 200;
pub const MAX_LABEL: usize = 40;
pub const MAX_TIP: usize = 300;
/// The longest name of a script.
pub const MAX_NAME: usize = 64;
/// The buttons file's largest size.
const MAX_BUTTONS_FILE: u64 = 1 << 20;
/// The panel a button goes in when it names none.
pub const PANEL: &str = "PyAddins";

const SCRIPTS: &str = "scripts";
const BUTTONS: &str = "buttons.json";
const LIB: &str = "lib";
const SETTINGS: &str = "settings.json";
/// The module scripts import, written beside them.
pub const CLIENT: &str = include_str!("bimcraft.py");

/// The scripts a new folder starts with, by name: examples to run and to read.
pub const SAMPLES: &[(&str, &str)] = &[("Change Wall Type", include_str!("samples/Change Wall Type.py"))];

/// The folder, by default beside the user's favorites; `BIMCRAFT_PYADDINS` puts it elsewhere.
pub fn default_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("BIMCRAFT_PYADDINS").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    bimcraft_core::favorites::default_path().map(|p| p.with_file_name("pyaddins"))
}

/// Whether `name` can name a script: letters, digits, spaces, `_` and `-`, not starting or ending with a space, and at
/// most [`MAX_NAME`] characters. It is the file's name, so nothing that reaches outside the folder.
pub fn check_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.chars().count() <= MAX_NAME
        && name.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-'))
        && !name.starts_with([' ', '-'])
        && !name.ends_with(' ');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "`{name}` can't name a script: use letters, digits, spaces, `_` and `-`, at most {MAX_NAME} characters, starting with a letter or digit"
        ))
    }
}

/// A script in the folder.
#[derive(Clone, Debug, PartialEq)]
pub struct Script {
    pub name: String,
    pub path: PathBuf,
    pub modified: Option<SystemTime>,
    pub size: u64,
}

/// A ribbon button: a script, run with the inputs it keeps.
#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    pub label: String,
    pub script: String,
    pub panel: String,
    pub tip: String,
    /// The values for the script's `main`, by name; one not given takes its default.
    pub inputs: Map<String, Value>,
}

impl Button {
    /// A button from JSON, as an agent or the buttons file gives one: what it is missing or gets wrong is said.
    pub fn from_json(v: &Value) -> Result<Button, String> {
        let o = v.as_object().ok_or("a button is an object: {label, script, panel?, tip?, inputs?}")?;
        let text = |key: &str, max: usize| -> Result<Option<String>, String> {
            match o.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(s)) if s.chars().count() <= max => Ok(Some(s.trim().to_string())),
                Some(Value::String(_)) => Err(format!("a button's `{key}` is at most {max} characters")),
                Some(_) => Err(format!("a button's `{key}` is text")),
            }
        };
        let script = text("script", MAX_NAME)?.ok_or("a button needs `script`, the script it runs")?;
        check_name(&script)?;
        let label = text("label", MAX_LABEL)?.filter(|l| !l.is_empty()).unwrap_or_else(|| script.clone());
        if label.chars().count() > MAX_LABEL {
            return Err(format!("a button's `label` is at most {MAX_LABEL} characters"));
        }
        let panel = text("panel", MAX_LABEL)?.filter(|p| !p.is_empty()).unwrap_or_else(|| PANEL.to_string());
        let tip = text("tip", MAX_TIP)?.unwrap_or_default();
        let inputs = match o.get("inputs") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(m)) => m.clone(),
            Some(_) => return Err("a button's `inputs` is an object: the values of the script's inputs, by name".into()),
        };
        let known = ["label", "script", "panel", "tip", "inputs"];
        if let Some(k) = o.keys().find(|k| !known.contains(&k.as_str())) {
            return Err(format!("a button has no `{k}`: it has {}", known.join(", ")));
        }
        Ok(Button { label, script, panel, tip, inputs })
    }

    pub fn to_json(&self) -> Value {
        json!({"label": self.label, "script": self.script, "panel": self.panel, "tip": self.tip, "inputs": self.inputs})
    }
}

/// What the folder holds now, cheaply: when it changes, the window reads it again.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stamp(Vec<(String, Option<SystemTime>, u64)>);

/// The folder.
#[derive(Clone, Debug, PartialEq)]
pub struct Store {
    pub dir: PathBuf,
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Store {
        Store { dir: dir.into() }
    }

    /// The default folder ([`default_dir`]).
    pub fn default_store() -> Option<Store> {
        default_dir().map(Store::new)
    }

    pub fn scripts_dir(&self) -> PathBuf {
        self.dir.join(SCRIPTS)
    }

    /// Where the script `name` is, or would be.
    pub fn path(&self, name: &str) -> Result<PathBuf, String> {
        check_name(name)?;
        Ok(self.scripts_dir().join(format!("{name}.py")))
    }

    /// The scripts, by name, at most [`MAX_SCRIPTS`].
    pub fn scripts(&self) -> Vec<Script> {
        let Ok(entries) = std::fs::read_dir(self.scripts_dir()) else { return Vec::new() };
        let mut out: Vec<Script> = entries
            .filter_map(Result::ok)
            .filter_map(|e| {
                let path = e.path();
                if path.extension().and_then(|x| x.to_str()) != Some("py") {
                    return None;
                }
                let name = path.file_stem()?.to_str()?.to_string();
                check_name(&name).ok()?;
                let meta = e.metadata().ok().filter(std::fs::Metadata::is_file)?;
                Some(Script { name, path, modified: meta.modified().ok(), size: meta.len() })
            })
            .take(MAX_SCRIPTS)
            .collect();
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.name.cmp(&b.name)));
        out
    }

    /// Writes the [`SAMPLES`] into a folder that has no scripts folder yet: once, so a sample deleted stays deleted.
    pub fn seed(&self) {
        if self.scripts_dir().exists() {
            return;
        }
        for (name, code) in SAMPLES {
            let _ = self.save(name, code);
        }
    }

    /// A script's text.
    pub fn read(&self, name: &str) -> Result<String, String> {
        let path = self.path(name)?;
        let size = std::fs::metadata(&path).map_err(|_| format!("there is no script `{name}`"))?.len();
        if size > MAX_SCRIPT {
            return Err(format!("`{name}` is larger than {} KB, the most a script may be", MAX_SCRIPT / 1024));
        }
        std::fs::read_to_string(&path).map_err(|e| format!("`{name}` could not be read: {e}"))
    }

    /// Writes a script (a new one, or over the one of that name), whole or not at all.
    pub fn save(&self, name: &str, code: &str) -> Result<PathBuf, String> {
        let path = self.path(name)?;
        if code.len() as u64 > MAX_SCRIPT {
            return Err(format!("a script is at most {} KB", MAX_SCRIPT / 1024));
        }
        if self.scripts().len() >= MAX_SCRIPTS && !path.exists() {
            return Err(format!("the folder has {MAX_SCRIPTS} scripts, the most it keeps: delete one first"));
        }
        write_whole(&path, code.as_bytes())?;
        Ok(path)
    }

    /// Deletes a script, and the buttons that run it.
    pub fn delete(&self, name: &str) -> Result<(), String> {
        let path = self.path(name)?;
        std::fs::remove_file(&path).map_err(|_| format!("there is no script `{name}`"))?;
        let mut buttons = self.buttons();
        let before = buttons.len();
        buttons.retain(|b| b.script != name);
        if buttons.len() != before {
            self.save_buttons(&buttons)?;
        }
        Ok(())
    }

    /// The buttons, in the order kept; one that is not a button, names a script twice over, or is past [`MAX_BUTTONS`] is
    /// left out.
    pub fn buttons(&self) -> Vec<Button> {
        let path = self.dir.join(BUTTONS);
        let Ok(meta) = std::fs::metadata(&path) else { return Vec::new() };
        if meta.len() > MAX_BUTTONS_FILE {
            return Vec::new();
        }
        let Some(v) = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else {
            return Vec::new();
        };
        let mut out: Vec<Button> = Vec::new();
        for b in v["buttons"].as_array().into_iter().flatten().filter_map(|b| Button::from_json(b).ok()) {
            if out.len() < MAX_BUTTONS && !out.iter().any(|o| o.label == b.label) {
                out.push(b);
            }
        }
        out
    }

    pub fn save_buttons(&self, buttons: &[Button]) -> Result<(), String> {
        if buttons.len() > MAX_BUTTONS {
            return Err(format!("at most {MAX_BUTTONS} buttons"));
        }
        let v = json!({"buttons": buttons.iter().map(Button::to_json).collect::<Vec<_>>()});
        let text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
        write_whole(&self.dir.join(BUTTONS), text.as_bytes())
    }

    /// Puts a button on the ribbon, or in place of the one with its label; its script must be in the folder.
    pub fn set_button(&self, button: Button) -> Result<(), String> {
        if !self.path(&button.script)?.is_file() {
            return Err(format!("there is no script `{}`: save it first", button.script));
        }
        let mut buttons = self.buttons();
        match buttons.iter().position(|b| b.label == button.label) {
            Some(at) => buttons[at] = button,
            None if buttons.len() >= MAX_BUTTONS => return Err(format!("at most {MAX_BUTTONS} buttons")),
            None => buttons.push(button),
        }
        self.save_buttons(&buttons)
    }

    /// Takes the button with `label` off the ribbon.
    pub fn remove_button(&self, label: &str) -> Result<(), String> {
        let mut buttons = self.buttons();
        let before = buttons.len();
        buttons.retain(|b| b.label != label);
        if buttons.len() == before {
            return Err(format!("there is no button `{label}`"));
        }
        self.save_buttons(&buttons)
    }

    /// The module scripts import, written beside them when it isn't there as this version has it.
    pub fn client(&self) -> Result<PathBuf, String> {
        let path = self.dir.join(LIB).join("bimcraft.py");
        if std::fs::read_to_string(&path).ok().as_deref() != Some(CLIENT) {
            write_whole(&path, CLIENT.as_bytes())?;
        }
        Ok(path)
    }

    /// A setting of PyAddins > Python (`python`, `editor`), if one is set: the window and the MCP server both read it here.
    pub fn setting(&self, key: &str) -> Option<String> {
        self.settings()[key].as_str().map(str::trim).filter(|p| !p.is_empty() && p.len() <= 4096).map(str::to_string)
    }

    fn settings(&self) -> Value {
        let path = self.dir.join(SETTINGS);
        if std::fs::metadata(&path).map_or(true, |m| m.len() > 64 * 1024) {
            return json!({});
        }
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .filter(Value::is_object)
            .unwrap_or(json!({}))
    }

    /// Sets a setting (none: the one found), keeping the others.
    pub fn set_setting(&self, key: &str, value: Option<&str>) -> Result<(), String> {
        let value = value.map(str::trim).filter(|p| !p.is_empty());
        if value.is_some_and(|p| p.len() > 4096) {
            return Err("that path is too long".into());
        }
        let mut all = self.settings();
        all[key] = json!(value);
        write_whole(&self.dir.join(SETTINGS), all.to_string().as_bytes())
    }

    /// The Python set for scripts, if one is.
    pub fn python_setting(&self) -> Option<String> {
        self.setting("python")
    }

    /// Sets the Python (none: find one).
    pub fn set_python(&self, python: Option<&str>) -> Result<(), String> {
        self.set_setting("python", python)
    }

    /// Opens a script in the editor set, or else the system's for it ([`crate::editor`]).
    pub fn edit(&self, name: &str) -> Result<(), String> {
        let path = self.path(name)?;
        if !path.is_file() {
            return Err(format!("there is no script `{name}`"));
        }
        crate::editor::open(&path, self.setting("editor").as_deref())
    }

    /// The Python scripts run in: the one set, or else the first found.
    pub fn python(&self) -> Result<crate::Python, String> {
        crate::python::find(self.python_setting().as_deref())
    }

    /// What the folder holds now: each script's name, time and size, and the buttons file's.
    pub fn stamp(&self) -> Stamp {
        let mut s: Vec<(String, Option<SystemTime>, u64)> = self.scripts().into_iter().map(|sc| (sc.name, sc.modified, sc.size)).collect();
        if let Ok(meta) = std::fs::metadata(self.dir.join(BUTTONS)) {
            s.push((BUTTONS.to_string(), meta.modified().ok(), meta.len()));
        }
        Stamp(s)
    }
}

/// Writes a file whole or not at all: to a file beside it, then over it.
fn write_whole(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("no folder to write in")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{} could not be made: {e}", dir.display()))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let part = dir.join(format!(".{name}.{}.part", std::process::id()));
    std::fs::write(&part, bytes).map_err(|e| format!("{} could not be written: {e}", path.display()))?;
    std::fs::rename(&part, path).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        format!("{} could not be written: {e}", path.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_a_few_safe_characters_and_never_a_path() {
        for ok in ["Studs", "room numbers 2", "a_b-c", "Сходы", "门窗"] {
            assert_eq!(check_name(ok), Ok(()), "{ok}");
        }
        let long = "x".repeat(MAX_NAME + 1);
        for bad in ["", " lead", "trail ", "-flag", "../up", "a/b", "a\\b", "x.py", "nul\0", "tab\t", long.as_str()] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn scripts_are_saved_listed_read_and_deleted_and_take_their_buttons_with_them() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        assert!(store.scripts().is_empty(), "no folder yet is no scripts");
        store.save("b script", "print('b')\n").unwrap();
        store.save("A script", "print('a')\n").unwrap();
        // What is not a script of ours is not listed: another extension, a bad name, a folder.
        std::fs::write(store.scripts_dir().join("notes.txt"), "x").unwrap();
        std::fs::write(store.scripts_dir().join(".hidden.py"), "x").unwrap();
        std::fs::create_dir(store.scripts_dir().join("dir.py")).unwrap();
        let names: Vec<String> = store.scripts().into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["A script", "b script"]);
        assert_eq!(store.read("A script").unwrap(), "print('a')\n");
        assert!(store.read("nope").unwrap_err().contains("no script"));
        assert!(store.save("../out", "x").is_err());
        assert!(store.save("big", &"#".repeat(MAX_SCRIPT as usize + 1)).unwrap_err().contains("at most"));
        // Saved again, it is replaced, and nothing half-written is left beside it.
        store.save("A script", "print('again')\n").unwrap();
        assert_eq!(store.read("A script").unwrap(), "print('again')\n");
        assert!(std::fs::read_dir(store.scripts_dir()).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".part")));

        let button = |label: &str, script: &str| Button::from_json(&json!({"label": label, "script": script})).unwrap();
        store.set_button(button("Run A", "A script")).unwrap();
        store.set_button(button("Run B", "b script")).unwrap();
        assert!(store.set_button(button("Run C", "missing")).unwrap_err().contains("save it first"));
        assert_eq!(store.buttons().len(), 2);
        assert_eq!(store.buttons()[0].panel, PANEL);
        // The same label replaces the button.
        let mut again = button("Run A", "b script");
        again.panel = "Rooms".into();
        store.set_button(again).unwrap();
        assert_eq!(
            store.buttons().iter().map(|b| (b.label.as_str(), b.script.as_str())).collect::<Vec<_>>(),
            [("Run A", "b script"), ("Run B", "b script")]
        );
        store.delete("b script").unwrap();
        assert!(store.buttons().is_empty(), "the buttons went with their script");
        assert!(store.delete("b script").is_err());
        store.set_button(button("Run A", "A script")).unwrap();
        store.remove_button("Run A").unwrap();
        assert!(store.remove_button("Run A").is_err());
    }

    #[test]
    fn a_new_folder_starts_with_the_samples_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        store.seed();
        let names: Vec<String> = store.scripts().into_iter().map(|s| s.name).collect();
        assert_eq!(names, SAMPLES.iter().map(|(n, _)| n.to_string()).collect::<Vec<_>>());
        // Deleted, a sample stays deleted.
        store.delete("Change Wall Type").unwrap();
        store.seed();
        assert!(store.scripts().is_empty());
    }

    #[test]
    fn a_button_says_what_is_wrong_with_it_and_the_file_keeps_only_buttons() {
        let b = Button::from_json(&json!({"script": "Studs", "inputs": {"spacing": 900}})).unwrap();
        assert_eq!((b.label.as_str(), b.panel.as_str(), b.inputs["spacing"].clone()), ("Studs", PANEL, json!(900)));
        let err = |v: Value| Button::from_json(&v).unwrap_err();
        assert!(err(json!([])).contains("object"));
        assert!(err(json!({"label": "x"})).contains("`script`"));
        assert!(err(json!({"script": "../x"})).contains("can't name"));
        assert!(err(json!({"script": "x", "label": "y".repeat(MAX_LABEL + 1)})).contains("at most"));
        assert!(err(json!({"script": "x", "inputs": [1]})).contains("`inputs`"));
        assert!(err(json!({"script": "x", "colour": "red"})).contains("no `colour`"));
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        for text in ["", "{", "[]", r#"{"buttons": 5}"#] {
            std::fs::write(dir.path().join(BUTTONS), text).unwrap();
            assert!(store.buttons().is_empty(), "{text}");
        }
        let text = json!({"buttons": [{"script": "a"}, {"bogus": 1}, {"script": "a"}, {"script": "b", "label": "B"}]});
        std::fs::write(dir.path().join(BUTTONS), text.to_string()).unwrap();
        assert_eq!(
            store.buttons().iter().map(|b| b.label.as_str()).collect::<Vec<_>>(),
            ["a", "B"],
            "the bad one and the twin are left out"
        );
    }

    #[test]
    fn the_stamp_changes_when_a_script_or_the_buttons_do_and_the_client_is_written_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let empty = store.stamp();
        store.save("one", "x = 1\n").unwrap();
        let one = store.stamp();
        assert_ne!(empty, one);
        assert_eq!(one, store.stamp(), "nothing changed, the same stamp");
        store.save("one", "x = 22\n").unwrap();
        assert_ne!(one, store.stamp(), "a script rewritten");
        let before = store.stamp();
        store.set_button(Button::from_json(&json!({"script": "one"})).unwrap()).unwrap();
        assert_ne!(before, store.stamp(), "the buttons");
        assert_eq!(store.python_setting(), None);
        store.set_python(Some(" /opt/py/bin/python3 ")).unwrap();
        assert_eq!(store.python_setting().as_deref(), Some("/opt/py/bin/python3"));
        store.set_setting("editor", Some("kate")).unwrap();
        assert_eq!(store.python_setting().as_deref(), Some("/opt/py/bin/python3"), "one setting keeps the other");
        store.set_python(None).unwrap();
        assert_eq!(store.python_setting(), None);
        assert_eq!(store.setting("editor").as_deref(), Some("kate"));
        let client = store.client().unwrap();
        assert_eq!(std::fs::read_to_string(&client).unwrap(), CLIENT);
        std::fs::write(&client, "old").unwrap();
        store.client().unwrap();
        assert_eq!(std::fs::read_to_string(&client).unwrap(), CLIENT, "an old one is replaced");
    }
}
