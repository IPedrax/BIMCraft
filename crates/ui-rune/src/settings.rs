//! `settings.json`, the user's own settings and never a project's: the interface's language, the theme, the profile and the window's
//! size. It is the file the egui front end keeps (`crates/ui-egui/src/settings.rs`, beside the favorites in the user's configuration
//! folder) and the keys have the same names and values, so one choice made in either shows in both. This front end keeps four keys of
//! that file and leaves every other as it found it, since the file holds a hundred things only the egui app reads.
//!
//! A file is read kindly (a missing or damaged one is none, a key it cannot read keeps what it was, an unknown key is left alone) and
//! written strictly (through a file beside it, so a crash never leaves half of one). Nothing here knows a window, so a test needs none.

use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::profile::Profile;

/// The file's name, in the folder of the favorites.
pub const FILE: &str = "settings.json";
/// The most of it that is read: the egui app's file, with its 4096 shortcuts, is far below this.
const MAX_FILE: u64 = 1 << 20;
/// The sizes believed for a window, points: never smaller than this, nor larger than the biggest screens.
pub const SIZES: RangeInclusive<f32> = 300.0..=16_000.0;

/// The interface's languages, each by its code (also the name of its catalog) and in its own words.
pub const LANGUAGES: [(&str, &str); 12] = [
    ("en", "English"),
    ("pt", "Português"),
    ("es", "Español"),
    ("pl", "Polski"),
    ("cs", "Čeština"),
    ("fr", "Français"),
    ("de", "Deutsch"),
    ("nl", "Nederlands"),
    ("it", "Italiano"),
    ("vi", "Tiếng Việt"),
    ("zh", "简体中文"),
    ("zh_TW", "繁體中文"),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
    /// What the system says. This front end cannot ask it yet and draws dark, as it always has.
    #[default]
    System,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::Dark, Theme::Light, Theme::System];

    pub fn code(self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Light => "light",
            Theme::System => "system",
        }
    }

    pub fn from_code(code: &str) -> Option<Theme> {
        Theme::ALL.into_iter().find(|t| t.code() == code)
    }

    /// Whether the interface is drawn dark.
    pub fn dark(self) -> bool {
        self != Theme::Light
    }
}

fn profile_code(p: Profile) -> &'static str {
    match p {
        Profile::Bimcraft => "bimcraft",
        Profile::Rev => "rev",
    }
}

fn profile_from(code: &str) -> Option<Profile> {
    [Profile::Bimcraft, Profile::Rev].into_iter().find(|p| profile_code(*p) == code)
}

/// How the window was left: its size in points and whether it was maximized. Only the size is kept, never the place (Wayland tells an
/// app none) and a window starts in the middle of its screen.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Window {
    pub size: Option<[f32; 2]>,
    pub maximized: bool,
}

impl Window {
    /// Whether `size` is one a window can be.
    pub fn believed(size: [f32; 2]) -> bool {
        size.iter().all(|n| n.is_finite() && SIZES.contains(n))
    }

    fn from_json(v: &Value, base: Window) -> Window {
        let side = |key: &str| v[key].as_f64().map(|n| n as f32).filter(|n| n.is_finite() && SIZES.contains(n));
        Window {
            size: side("width").zip(side("height")).map(|(w, h)| [w, h]).or(base.size),
            maximized: v["maximized"].as_bool().unwrap_or(base.maximized),
        }
    }

    fn to_json(self) -> Value {
        match self.size {
            Some([width, height]) => json!({"width": width, "height": height, "maximized": self.maximized}),
            None => json!({"maximized": self.maximized}),
        }
    }
}

/// What this front end keeps of the settings file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    /// A language's code, or none to follow the system's.
    pub language: Option<String>,
    pub theme: Theme,
    /// A profile the user chose, or none for what the arguments and the default say.
    pub profile: Option<Profile>,
    pub window: Window,
}

impl Settings {
    /// `base` with what `v` says over it: a key that is missing, of the wrong kind or not understood keeps `base`'s.
    pub fn from_json(v: &Value, base: Settings) -> Settings {
        let language = v["language"].as_str().filter(|c| LANGUAGES.iter().any(|(code, _)| code == c)).map(str::to_string);
        Settings {
            language: language.or(base.language),
            theme: v["theme"].as_str().and_then(Theme::from_code).unwrap_or(base.theme),
            profile: v["profile"].as_str().and_then(profile_from).or(base.profile),
            window: Window::from_json(&v["window"], base.window),
        }
    }

    /// `file`, the settings file as it was, with this front end's four keys set and every other key as it was. A file that was no
    /// object is replaced by one.
    pub fn merged_into(&self, file: Value) -> Value {
        let mut map = match file {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        if let Some(code) = &self.language {
            map.insert("language".into(), json!(code));
        }
        map.insert("theme".into(), json!(self.theme.code()));
        if let Some(p) = self.profile {
            map.insert("profile".into(), json!(profile_code(p)));
        }
        // The window's own keys are kept under `window`, as the egui app has them.
        map.insert("window".into(), self.window.to_json());
        Value::Object(map)
    }
}

/// Where the settings file is: beside the favorites, in the user's own configuration folder.
pub fn default_path() -> Option<PathBuf> {
    bimcraft_core::favorites::default_path().map(|favorites| favorites.with_file_name(FILE))
}

/// The settings file as JSON: none when it is not there, is too big or is not JSON.
pub fn read(path: &Path) -> Option<Value> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut text = String::new();
    file.take(MAX_FILE + 1).read_to_string(&mut text).ok()?;
    if text.len() as u64 > MAX_FILE {
        return None;
    }
    serde_json::from_str(&text).ok()
}

/// What the file at `path` keeps, over the defaults (the defaults for a file that is not there or is damaged).
pub fn load(path: &Path) -> Settings {
    read(path).map_or_else(Settings::default, |v| Settings::from_json(&v, Settings::default()))
}

/// Writes `settings` into the file at `path`: its folder made if need be, every key it holds that is not ours left as it is, through a
/// file beside it so a crash never leaves half of one.
pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let file = settings.merged_into(read(path).unwrap_or(Value::Null));
    let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    let staged = path.with_extension("json.new");
    std::fs::write(&staged, text).map_err(|e| e.to_string())?;
    std::fs::rename(&staged, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bimcraft-rune-settings-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a folder");
        dir
    }

    #[test]
    fn a_file_is_read_kindly_a_key_it_cannot_read_keeps_what_was() {
        let base = Settings { language: Some("pt".into()), theme: Theme::Light, profile: Some(Profile::Rev), window: Window::default() };
        let read = |v: Value| Settings::from_json(&v, base.clone());
        assert_eq!(read(json!({})), base, "an empty file changes nothing");
        assert_eq!(read(json!(7)), base, "nor does one that is no object");
        let v = read(
            json!({"language": "zh_TW", "theme": "dark", "profile": "bimcraft", "window": {"width": 1500, "height": 900, "maximized": true}}),
        );
        assert_eq!(v.language.as_deref(), Some("zh_TW"));
        assert_eq!((v.theme, v.profile), (Theme::Dark, Some(Profile::Bimcraft)));
        assert_eq!(v.window, Window { size: Some([1500.0, 900.0]), maximized: true });
        let bad =
            read(json!({"language": "klingon", "theme": 3, "profile": "x", "window": {"width": 1e9, "height": -4, "maximized": "yes"}}));
        assert_eq!(bad, base, "a language with no catalog, a wrong kind, a size no window can be: all kept as they were");
        let nan = read(json!({"window": {"width": 100, "height": 100}}));
        assert_eq!(nan.window.size, None, "a window below the least is not believed");
    }

    #[test]
    fn writing_keeps_every_key_that_is_not_ours() {
        let settings = Settings {
            language: Some("pl".into()),
            theme: Theme::Dark,
            profile: None,
            window: Window { size: Some([1280.0, 800.0]), maximized: false },
        };
        let existing =
            json!({"shortcuts": {"version": 2}, "accent": [1, 2, 3], "theme": "light", "profile": "rev", "assistant_model": "m"});
        let merged = settings.merged_into(existing);
        assert_eq!(merged["shortcuts"], json!({"version": 2}), "the egui app's keys are left alone");
        assert_eq!(merged["accent"], json!([1, 2, 3]));
        assert_eq!(merged["assistant_model"], json!("m"));
        assert_eq!(merged["theme"], json!("dark"), "ours are set");
        assert_eq!(merged["language"], json!("pl"));
        assert_eq!(merged["profile"], json!("rev"), "no choice of ours leaves the file's own");
        assert_eq!(merged["window"]["width"], json!(1280.0));
        assert_eq!(Settings::default().merged_into(json!("not an object"))["theme"], json!("system"));
    }

    #[test]
    fn the_file_round_trips_is_written_whole_and_a_bad_one_is_ignored() {
        let dir = folder("file");
        let path = dir.join("nested").join(FILE);
        assert_eq!(load(&path), Settings::default(), "no file is the defaults");
        let settings = Settings {
            language: Some("de".into()),
            theme: Theme::Light,
            profile: Some(Profile::Rev),
            window: Window { size: Some([1100.0, 700.0]), maximized: true },
        };
        save(&path, &settings).expect("written");
        assert_eq!(load(&path), settings);
        assert!(!path.with_extension("json.new").exists(), "the file beside it is renamed away");

        std::fs::write(&path, r#"{"accent": [9, 9, 9], "language": "fr"}"#).expect("a file of the egui app's");
        save(&path, &Settings { language: None, ..settings.clone() }).expect("written");
        let after = read(&path).expect("json");
        assert_eq!(after["accent"], json!([9, 9, 9]), "what the egui app wrote is still there");
        assert_eq!(after["language"], json!("fr"), "and a choice we did not make is not taken from it");

        std::fs::write(&path, "{ not json").expect("a damaged file");
        assert_eq!(load(&path), Settings::default(), "a damaged file is ignored");
        save(&path, &settings).expect("and written over");
        assert_eq!(load(&path), settings);

        std::fs::write(&path, vec![b' '; (MAX_FILE + 2) as usize]).expect("a huge file");
        assert!(read(&path).is_none(), "a file too big to believe is not read");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_themes_and_languages_have_their_codes() {
        for t in Theme::ALL {
            assert_eq!(Theme::from_code(t.code()), Some(t));
        }
        assert!(Theme::Dark.dark() && Theme::System.dark() && !Theme::Light.dark());
        assert_eq!(Theme::from_code("sepia"), None);
        assert!(LANGUAGES.iter().all(|(code, name)| !name.is_empty() && crate::lang::code_of(code) == *code), "each has a catalog");
        assert!(Window::believed([800.0, 600.0]) && !Window::believed([f32::NAN, 600.0]) && !Window::believed([800.0, 20_000.0]));
    }
}
