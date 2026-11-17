//! A plugin's manifest: what it is, which host ABI it speaks, what it may do, and the functions it brings.
//! It is untrusted input, so it is read strictly: unknown fields, unknown permissions and bad names are
//! refused, and every text has a limit.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

/// The host ABI this build speaks. A plugin for another one is refused at load.
pub const ABI: u32 = 1;
/// The most a manifest may be, bytes.
pub const MAX_MANIFEST: u64 = 1 << 20;
/// The most a module may be, bytes (the protocol line of the bridge has the same limit).
pub const MAX_MODULE: u64 = 16 << 20;
/// The most functions one plugin may declare.
pub const MAX_FUNCTIONS: usize = 256;
/// The longest id, name or version.
pub const MAX_TEXT: usize = 200;
/// The permissions the ABI knows. Anything else fails the load.
pub const PERMISSIONS: &[&str] = &["files", "update"];

/// A plugin's manifest, as its `plugin.json` holds it.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Stable id: lower-case Latin letters, digits, `-`, `_` and `.`.
    pub id: String,
    pub version: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// The host ABI the module was built for.
    pub abi: u32,
    /// The module file, relative to the manifest's folder.
    #[serde(default = "default_module")]
    pub module: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub link: Option<String>,
    /// What the plugin asks for beyond changing the model: `files` and `update` in ABI 1.
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Where its updates are announced, and the key that signs them. It is present exactly when the manifest asks
    /// for the `update` permission.
    #[serde(default)]
    pub update: Option<UpdateFeed>,
    pub functions: Vec<Function>,
}

/// The channel a plugin is updated through: the URL of its feed, and the publisher's Ed25519 key.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct UpdateFeed {
    /// The https address of the JSON feed. Empty is allowed for a local build, and then no channel is declared.
    #[serde(default)]
    pub feed: String,
    /// `ed25519:BASE64` of the 32-byte publisher key. None: the feed isn't signed, which the user must allow.
    #[serde(default)]
    pub public_key: Option<String>,
}

fn default_module() -> String {
    "plugin.wasm".to_string()
}

/// One function a plugin exposes.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Function {
    /// Stable id within the plugin.
    pub id: String,
    /// The name a person reads, on the ribbon and in the catalog.
    pub label: String,
    #[serde(default)]
    pub description: String,
    /// The JSON parameters, documented for agents, as a command's are.
    #[serde(default)]
    pub params: Value,
    /// The ribbon tab the plugin declares. None: the common Plugins section.
    #[serde(default)]
    pub tab: Option<String>,
    /// The panel of that tab. None: the plugin's own name.
    #[serde(default)]
    pub panel: Option<String>,
    /// Smaller comes first on the ribbon.
    #[serde(default)]
    pub order: Option<i64>,
}

impl Manifest {
    /// The manifest at `path`, checked: its folder is the base of the module's path.
    pub fn read(path: &Path) -> Result<Manifest, String> {
        let size = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
        if !path.is_file() {
            return Err(format!("{}: not a manifest file", path.display()));
        }
        if size > MAX_MANIFEST {
            return Err(format!("{} is too large ({size} bytes; a manifest is up to {MAX_MANIFEST})", path.display()));
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let manifest: Manifest = serde_json::from_str(&text).map_err(|e| format!("{} isn't a plugin manifest: {e}", path.display()))?;
        manifest.check().map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(manifest)
    }

    /// The manifest held in `text`, checked, as if it were the one at `path`.
    pub fn parse(text: &str, path: &Path) -> Result<Manifest, String> {
        let manifest: Manifest = serde_json::from_str(text).map_err(|e| format!("{} isn't a plugin manifest: {e}", path.display()))?;
        manifest.check().map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(manifest)
    }

    /// Whether it is sound: names are names, the ABI is this one, the permissions are known, no function is
    /// declared twice.
    pub fn check(&self) -> Result<(), String> {
        check_id("a plugin id", &self.id)?;
        check_version(&self.version)?;
        check_text("the plugin's name", &self.name)?;
        for (field, value) in [
            ("description", Some(self.description.as_str())),
            ("author", Some(self.author.as_str())),
            ("license", self.license.as_deref()),
            ("link", self.link.as_deref()),
        ] {
            if let Some(value) = value {
                check_text(field, value)?;
            }
        }
        if self.abi != ABI {
            return Err(format!("host ABI {}, this BIMCraft speaks ABI {ABI}: rebuild the plugin for it", self.abi));
        }
        check_relative("the module", &self.module)?;
        if self.functions.len() > MAX_FUNCTIONS {
            return Err(format!("it declares {} functions; a plugin may declare {MAX_FUNCTIONS}", self.functions.len()));
        }
        for p in &self.permissions {
            if !PERMISSIONS.contains(&p.as_str()) {
                return Err(format!(
                    "unknown permission `{p}`: the host ABI 1 knows {}",
                    PERMISSIONS.iter().map(|p| format!("`{p}`")).collect::<Vec<_>>().join(", ")
                ));
            }
        }
        let asks_update = self.asks_for("update");
        match (&self.update, asks_update) {
            (Some(update), true) => update.check()?,
            (None, true) => {
                return Err("it asks for the `update` permission but has no `update` block: the feed is where updates are read from".into());
            }
            (Some(_), false) => {
                return Err("it has an `update` block but doesn't ask for the `update` permission".into());
            }
            (None, false) => {}
        }
        let mut seen = Vec::new();
        for f in &self.functions {
            check_id("a function id", &f.id)?;
            check_text("a function's label", &f.label)?;
            check_text("a function's description", &f.description)?;
            for (field, value) in [("tab", f.tab.as_deref()), ("panel", f.panel.as_deref())] {
                if let Some(value) = value {
                    check_text(&format!("a function's {field}"), value)?;
                }
            }
            if seen.iter().any(|id| id == &f.id) {
                return Err(format!("two functions are named `{}`", f.id));
            }
            seen.push(f.id.clone());
        }
        Ok(())
    }

    /// The module the manifest names, beside the manifest at `path`.
    pub fn module_path(&self, manifest: &Path) -> PathBuf {
        match manifest.parent() {
            Some(folder) => folder.join(&self.module),
            None => PathBuf::from(&self.module),
        }
    }

    /// Whether the manifest asks for `permission`. It is a request, not a grant: the user gives one in
    /// Manage > Plugins, and it is kept in the folder's state file.
    pub fn asks_for(&self, permission: &str) -> bool {
        self.permissions.iter().any(|p| p == permission)
    }

    /// Whether the manifest asks to reach files outside the model.
    pub fn asks_for_files(&self) -> bool {
        self.asks_for("files")
    }

    /// Whether the manifest asks to update itself from its channel.
    pub fn asks_for_update(&self) -> bool {
        self.asks_for("update")
    }

    /// The channel the plugin would update through, when it declares one: an empty feed is no channel.
    pub fn update_feed(&self) -> Option<&UpdateFeed> {
        self.update.as_ref().filter(|u| !u.feed.trim().is_empty())
    }

    /// Whether the plugin declared no ribbon tab: its functions go in the common Plugins section.
    pub fn tab_name(&self, f: &Function) -> Option<String> {
        f.tab.clone().filter(|t| !t.trim().is_empty())
    }

    pub fn panel_name(&self, f: &Function) -> String {
        f.panel.clone().filter(|p| !p.trim().is_empty()).unwrap_or_else(|| self.name.clone())
    }
}

impl UpdateFeed {
    /// Whether the channel is sound: an https address without a login, a fragment or a space, and an Ed25519 key of
    /// the right length when one is given. An empty feed is allowed only for a local build.
    pub fn check(&self) -> Result<(), String> {
        check_text("the update feed", &self.feed)?;
        if !self.feed.trim().is_empty() {
            check_https_url("the update feed", &self.feed)?;
        }
        if let Some(key) = &self.public_key {
            check_text("the update public key", key)?;
            check_public_key(key)?;
        }
        Ok(())
    }

    /// The key with the `ed25519:` prefix, or an empty string when the feed isn't signed.
    pub fn key(&self) -> &str {
        self.public_key.as_deref().unwrap_or_default()
    }
}

/// A URL the update channel may be read from: https, a host, no login and no fragment.
pub(crate) fn check_https_url(what: &str, url: &str) -> Result<(), String> {
    if url.chars().any(char::is_whitespace) || url.chars().any(char::is_control) {
        return Err(format!("{what} `{url}` has a space or a control character"));
    }
    if !url.starts_with("https://") {
        return Err(format!("{what} `{url}` isn't https: an update is only read over HTTPS"));
    }
    let rest = &url["https://".len()..];
    if rest.contains('#') {
        return Err(format!("{what} `{url}` has a fragment"));
    }
    let authority = rest.split(['/', '?']).next().unwrap_or_default();
    if authority.is_empty() {
        return Err(format!("{what} `{url}` has no host"));
    }
    if authority.contains('@') {
        return Err(format!("{what} `{url}` has a login in it"));
    }
    Ok(())
}

/// An Ed25519 public key as `ed25519:BASE64`, of exactly 32 bytes.
pub(crate) fn check_public_key(key: &str) -> Result<(), String> {
    let bytes = decode_prefixed(key, "ed25519:")?;
    if bytes.len() != 32 {
        return Err(format!("the update public key is {} bytes; an Ed25519 key is 32", bytes.len()));
    }
    Ok(())
}

/// The bytes of a `prefix:BASE64` field.
pub(crate) fn decode_prefixed(field: &str, prefix: &str) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    let body = field
        .strip_prefix(prefix)
        .ok_or_else(|| format!("`{}` doesn't start with `{prefix}`", field.chars().take(24).collect::<String>()))?;
    base64::engine::general_purpose::STANDARD
        .decode(body)
        .map_err(|e| format!("`{}` isn't base64: {e}", field.chars().take(24).collect::<String>()))
}

fn check_text(what: &str, text: &str) -> Result<(), String> {
    let count = text.chars().count();
    if count > MAX_TEXT {
        return Err(format!("{what} is {count} characters; it may be {MAX_TEXT}"));
    }
    Ok(())
}

fn check_version(version: &str) -> Result<(), String> {
    check_text("the version", version)?;
    if version.is_empty() {
        return Err("the version is empty".to_string());
    }
    if !version.chars().all(|c| c.is_ascii_alphanumeric() || ".+-_".contains(c)) {
        return Err(format!("the version `{version}`: use letters, digits, `.`, `+`, `-` and `_`"));
    }
    Ok(())
}

fn check_id(what: &str, id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 64 {
        return Err(format!("{what} is 1 to 64 characters, not {id:?}"));
    }
    let good = id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'));
    if !good || id.starts_with('.') || id.starts_with('-') {
        return Err(format!("{what} `{id}`: use lower-case Latin letters, digits, `-`, `_` and `.`"));
    }
    Ok(())
}

/// A module's path: relative, inside the plugin's folder, without `..`.
fn check_relative(what: &str, path: &str) -> Result<(), String> {
    check_text(what, path)?;
    let p = Path::new(path);
    if p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(format!("{what} `{path}`: it must be a relative path inside the plugin's folder"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;

    use super::*;

    fn sample() -> serde_json::Value {
        serde_json::json!({
            "id": "areas",
            "version": "1.0.0",
            "name": "Areas",
            "abi": 1,
            "functions": [{"id": "summary", "label": "Area Summary"}]
        })
    }

    fn check(v: serde_json::Value) -> Result<Manifest, String> {
        let m: Manifest = serde_json::from_value(v).map_err(|e| e.to_string())?;
        m.check().map(|()| m)
    }

    #[test]
    fn a_manifest_is_read_strictly() {
        assert!(check(sample()).is_ok());
        // An unknown field is refused, as an unknown command parameter is.
        let mut v = sample();
        v["extra"] = serde_json::json!(1);
        assert!(check(v).is_err());
        // A wrong ABI says which one this app speaks.
        let mut v = sample();
        v["abi"] = serde_json::json!(2);
        assert!(check(v).unwrap_err().contains("ABI 1"));
        // An unknown permission is refused.
        let mut v = sample();
        v["permissions"] = serde_json::json!(["net"]);
        assert!(check(v).unwrap_err().contains("unknown permission"));
        // Ids are lower-case Latin.
        let mut v = sample();
        v["id"] = serde_json::json!("Areas!");
        assert!(check(v).is_err());
        // A module may not leave the plugin's folder.
        let mut v = sample();
        v["module"] = serde_json::json!("../evil.wasm");
        assert!(check(v).unwrap_err().contains("relative"));
    }

    #[test]
    fn the_update_permission_and_its_channel_come_together() {
        let key = format!("ed25519:{}", base64::engine::general_purpose::STANDARD.encode([7u8; 32]));
        let mut v = sample();
        v["permissions"] = serde_json::json!(["update"]);
        assert!(check(v.clone()).unwrap_err().contains("no `update` block"));
        v["update"] = serde_json::json!({"feed": "https://example.org/latest.json", "public_key": key});
        assert!(check(v.clone()).is_ok());
        // The block without the permission is refused too.
        let mut v2 = sample();
        v2["update"] = serde_json::json!({"feed": "https://example.org/latest.json"});
        assert!(check(v2).unwrap_err().contains("doesn't ask"));
        // Only https, no login, no fragment.
        for feed in ["http://example.org/latest.json", "https://user:pw@example.org/latest.json", "https://example.org/latest.json#x"] {
            let mut v3 = sample();
            v3["permissions"] = serde_json::json!(["update"]);
            v3["update"] = serde_json::json!({"feed": feed});
            assert!(check(v3).is_err(), "{feed}");
        }
        // A key is Ed25519: 32 bytes in base64.
        for bad in ["ed25519:AAAA", "rsa:AAAA", "not-a-key"] {
            let mut v4 = sample();
            v4["permissions"] = serde_json::json!(["update"]);
            v4["update"] = serde_json::json!({"feed": "https://example.org/latest.json", "public_key": bad});
            assert!(check(v4).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_empty_feed_is_no_channel_and_a_local_build_may_say_so() {
        let mut v = sample();
        v["permissions"] = serde_json::json!(["update"]);
        v["update"] = serde_json::json!({"feed": ""});
        let m = check(v).unwrap();
        assert!(m.asks_for_update());
        assert!(m.update_feed().is_none());
    }

    #[test]
    fn functions_are_named_once() {
        let mut v = sample();
        v["functions"] = serde_json::json!([
            {"id": "summary", "label": "One"},
            {"id": "summary", "label": "Two"}
        ]);
        assert!(check(v).unwrap_err().contains("two functions"));
    }

    #[test]
    fn a_function_without_a_tab_goes_in_the_plugins_section() {
        let m = check(sample()).unwrap();
        let f = &m.functions[0];
        assert_eq!(m.tab_name(f), None);
        assert_eq!(m.panel_name(f), "Areas");
    }
}
