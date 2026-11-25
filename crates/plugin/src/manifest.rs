//! A plugin's manifest: what it is, which host ABI it speaks, what it may do, and the functions and panels it brings.
//! It is untrusted input, so it is read strictly: unknown fields, unknown permissions and bad names are
//! refused, and every text has a limit.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::{self, Deserializer};
use serde_json::Value;

/// The newest host ABI this build speaks. A plugin for another one is refused at load.
pub const ABI: u32 = 2;
/// The host ABIs this build speaks: a plugin of ABI 1 keeps working, a plugin of ABI 2 may declare panels.
pub const SUPPORTED: &[u32] = &[1, 2];
/// The most a manifest may be, bytes.
pub const MAX_MANIFEST: u64 = 1 << 20;
/// The most a module may be, bytes (the protocol line of the bridge has the same limit).
pub const MAX_MODULE: u64 = 16 << 20;
/// The most functions one plugin may declare.
pub const MAX_FUNCTIONS: usize = 256;
/// The most panels one plugin may declare.
pub const MAX_PANELS: usize = 64;
/// The most widgets one panel may declare, nested ones included.
pub const MAX_WIDGETS: usize = 512;
/// The deepest a widget may nest a row or a group in.
pub const MAX_WIDGET_DEPTH: usize = 8;
/// The longest id, name or version.
pub const MAX_TEXT: usize = 200;
/// The permissions the ABI knows. Anything else fails the load.
pub const PERMISSIONS: &[&str] = &["files"];

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
    /// The host ABI the module was built for: 1 or 2.
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
    /// What the plugin asks for beyond changing the model: `files` alone.
    #[serde(default)]
    pub permissions: Vec<String>,
    /// The palettes the plugin declares: ABI 2 alone.
    #[serde(default)]
    pub panels: Vec<Panel>,
    pub functions: Vec<Function>,
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
    /// The JSON parameters, documented for agents, as a command's are. The window draws a form from them.
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
    /// Where the function is offered: on the ribbon, in a panel's button, or both (the default).
    #[serde(default)]
    pub ui: FunctionUi,
}

/// Where a function is offered.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FunctionUi {
    /// A button on the ribbon, as every function of ABI 1.
    Ribbon,
    /// A button in a panel, and on the ribbon too (the default).
    #[default]
    Both,
    /// A panel's button or a function only a panel calls: no ribbon button.
    Panel,
}

impl FunctionUi {
    /// Whether the function has a button on the ribbon.
    pub fn on_ribbon(self) -> bool {
        self != FunctionUi::Panel
    }
}

/// A panel a plugin declares: a palette the window docks and draws, its fields and buttons its widgets.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Panel {
    /// Stable id within the plugin: `[a-z0-9_.-]`, 1 to 64 characters.
    pub id: String,
    /// The palette's title. The plugin's data: not translated.
    pub title: String,
    /// Smaller comes first in the registry.
    #[serde(default)]
    pub order: Option<i64>,
    /// Where the palette docks when it first opens: right by default.
    #[serde(default)]
    pub dock: Dock,
    /// The function called once, when the palette is first shown.
    #[serde(default)]
    pub on_open: Option<String>,
    /// The function called by Refresh and after each of the panel's buttons.
    #[serde(default)]
    pub refresh: Option<String>,
    /// The fields, buttons and views of the panel, top to bottom.
    #[serde(default)]
    pub widgets: Vec<Widget>,
}

/// The side a plugin's palette docks on when it first opens.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Dock {
    Left,
    #[default]
    Right,
    Bottom,
    /// A window of its own, over the views.
    Float,
}

/// The unit a widget's value is typed and shown in. The call always gets millimetres, square millimetres, cubic
/// millimetres or degrees, as commands do; the window shows the project's units.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    Length,
    Area,
    Volume,
    Angle,
    #[default]
    None,
}

impl Unit {
    /// Its name in a message or an error.
    pub fn name(self) -> &'static str {
        match self {
            Unit::Length => "length",
            Unit::Area => "area",
            Unit::Volume => "volume",
            Unit::Angle => "angle",
            Unit::None => "none",
        }
    }
}

/// How a table's column aligns its cells.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// One entry of a select's list: the value the call gets, and the text a person reads.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub value: Value,
    pub label: String,
}

/// A table's column: the key by which the plugin's answer names the cells, its heading, its width and its alignment.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Column {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub width: Option<f32>,
    #[serde(default)]
    pub align: Align,
}

/// A button's condition: enabled while the widget `widget` holds `equals`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EnabledWhen {
    pub widget: String,
    pub equals: Value,
}

/// A widget of a panel. The window draws it; the plugin never does.
#[derive(Clone, Debug, PartialEq)]
pub enum Widget {
    /// A line of text.
    Label { text: String },
    /// Widgets side by side.
    Row { widgets: Vec<Widget> },
    /// A titled group, which may fold away.
    Group { title: String, widgets: Vec<Widget>, collapsed: bool },
    /// A line of text the user types.
    Text { id: String, label: String, readonly: bool, multiline: bool, unit: Unit },
    /// A number the user types, in the project's units.
    Number { id: String, label: String, unit: Unit, min: Option<f64>, max: Option<f64>, decimals: Option<u8> },
    /// A box to tick.
    Checkbox { id: String, label: String },
    /// One of a list of choices.
    Select { id: String, label: String, options: Vec<Choice> },
    /// A level of the project, by name.
    Level { id: String, label: String },
    /// A name from the project's library, of the kind `kind` (as `type:wall`).
    Library { id: String, label: String, kind: String },
    /// What is selected, read only: it is not sent with a call.
    Selection { id: String, label: String },
    /// A button that calls a function of this plugin.
    Button { id: String, label: String, function: String, enabled_when: Option<EnabledWhen> },
    /// A table the plugin's answer fills.
    Table { id: String, columns: Vec<Column> },
    /// A block of text the plugin's answer fills, `height` points tall.
    TextPanel { id: String, height: Option<f32> },
    /// A line across the panel.
    Separator,
}

/// The widget types a manifest may name, for messages.
const WIDGET_TYPES: &[&str] = &[
    "label",
    "row",
    "group",
    "text",
    "number",
    "checkbox",
    "select",
    "level",
    "library",
    "selection",
    "button",
    "table",
    "text_panel",
    "separator",
];

impl Widget {
    /// The id its value is kept and sent by: none for a label, a row, a group or a separator.
    pub fn id(&self) -> Option<&str> {
        match self {
            Widget::Text { id, .. }
            | Widget::Number { id, .. }
            | Widget::Checkbox { id, .. }
            | Widget::Select { id, .. }
            | Widget::Level { id, .. }
            | Widget::Library { id, .. }
            | Widget::Selection { id, .. }
            | Widget::Button { id, .. }
            | Widget::Table { id, .. }
            | Widget::TextPanel { id, .. } => Some(id),
            Widget::Label { .. } | Widget::Row { .. } | Widget::Group { .. } | Widget::Separator => None,
        }
    }

    /// The widgets it holds, top to bottom: itself alone for all but a row and a group.
    pub fn children(&self) -> &[Widget] {
        match self {
            Widget::Row { widgets } | Widget::Group { widgets, .. } => widgets,
            _ => &[],
        }
    }

    /// Reads one widget, and those it holds, from its JSON. Strict: an unknown type, an unknown field or a wrong one
    /// is refused with a message that names the widget.
    pub fn from_value(v: &Value) -> Result<Widget, String> {
        let Some(object) = v.as_object() else {
            return Err(format!("a widget must be an object, not {v}"));
        };
        let kind = match object.get("type") {
            Some(Value::String(kind)) => kind.as_str(),
            Some(other) => return Err(format!("a widget's `type` must be a string, not {other}")),
            None => return Err("a widget has no `type`".to_string()),
        };
        let name = object.get("id").and_then(Value::as_str).unwrap_or(kind);
        let at = format!("the widget `{name}`");
        let widget = match kind {
            "label" => {
                only(&at, object, &["type", "text"])?;
                Widget::Label { text: text(&at, v, "text")? }
            }
            "row" => {
                only(&at, object, &["type", "widgets"])?;
                Widget::Row { widgets: children(&at, v)? }
            }
            "group" => {
                only(&at, object, &["type", "title", "widgets", "collapsed"])?;
                Widget::Group { title: text(&at, v, "title")?, widgets: children(&at, v)?, collapsed: flag(&at, v, "collapsed")? }
            }
            "text" => {
                only(&at, object, &["type", "id", "label", "readonly", "multiline", "unit"])?;
                Widget::Text {
                    id: identifier(&at, v, "id")?,
                    label: text(&at, v, "label")?,
                    readonly: flag(&at, v, "readonly")?,
                    multiline: flag(&at, v, "multiline")?,
                    unit: unit(&at, v)?,
                }
            }
            "number" => {
                only(&at, object, &["type", "id", "label", "unit", "min", "max", "decimals"])?;
                let decimals = match object.get("decimals") {
                    None | Some(Value::Null) => None,
                    Some(n) => Some(n.as_u64().filter(|d| *d <= 6).ok_or_else(|| format!("{at}: `decimals` is 0 to 6, not {n}"))? as u8),
                };
                Widget::Number {
                    id: identifier(&at, v, "id")?,
                    label: text(&at, v, "label")?,
                    unit: unit(&at, v)?,
                    min: number(&at, v, "min")?,
                    max: number(&at, v, "max")?,
                    decimals,
                }
            }
            "checkbox" => {
                only(&at, object, &["type", "id", "label"])?;
                Widget::Checkbox { id: identifier(&at, v, "id")?, label: text(&at, v, "label")? }
            }
            "select" => {
                only(&at, object, &["type", "id", "label", "options"])?;
                let options = match object.get("options") {
                    Some(Value::Array(list)) if !list.is_empty() => list
                        .iter()
                        .map(|o| {
                            serde_json::from_value::<Choice>(o.clone()).map_err(|e| format!("{at}: a choice isn't `{{value, label}}`: {e}"))
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    Some(Value::Array(_)) => return Err(format!("{at}: `options` is empty")),
                    Some(other) => return Err(format!("{at}: `options` must be an array, not {other}")),
                    None => return Err(format!("{at}: `options` is required")),
                };
                Widget::Select { id: identifier(&at, v, "id")?, label: text(&at, v, "label")?, options }
            }
            "level" => {
                only(&at, object, &["type", "id", "label"])?;
                Widget::Level { id: identifier(&at, v, "id")?, label: text(&at, v, "label")? }
            }
            "library" => {
                only(&at, object, &["type", "id", "label", "kind"])?;
                Widget::Library { id: identifier(&at, v, "id")?, label: text(&at, v, "label")?, kind: text(&at, v, "kind")? }
            }
            "selection" => {
                only(&at, object, &["type", "id", "label"])?;
                Widget::Selection { id: identifier(&at, v, "id")?, label: text(&at, v, "label")? }
            }
            "button" => {
                only(&at, object, &["type", "id", "label", "function", "enabled_when"])?;
                let enabled_when = match object.get("enabled_when") {
                    None | Some(Value::Null) => None,
                    Some(w) => Some(serde_json::from_value::<EnabledWhen>(w.clone()).map_err(|e| format!("{at}: `enabled_when`: {e}"))?),
                };
                Widget::Button {
                    id: identifier(&at, v, "id")?,
                    label: text(&at, v, "label")?,
                    function: identifier(&at, v, "function")?,
                    enabled_when,
                }
            }
            "table" => {
                only(&at, object, &["type", "id", "columns"])?;
                let columns = match object.get("columns") {
                    Some(Value::Array(list)) if !list.is_empty() => list
                        .iter()
                        .map(|c| {
                            serde_json::from_value::<Column>(c.clone())
                                .map_err(|e| format!("{at}: a column isn't `{{key, label, width, align}}`: {e}"))
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    Some(Value::Array(_)) => return Err(format!("{at}: `columns` is empty")),
                    Some(other) => return Err(format!("{at}: `columns` must be an array, not {other}")),
                    None => return Err(format!("{at}: `columns` is required")),
                };
                Widget::Table { id: identifier(&at, v, "id")?, columns }
            }
            "text_panel" => {
                only(&at, object, &["type", "id", "height"])?;
                let height = match object.get("height") {
                    None | Some(Value::Null) => None,
                    Some(n) => Some(
                        n.as_f64()
                            .filter(|h| h.is_finite() && *h > 0.0 && *h <= 4000.0)
                            .ok_or_else(|| format!("{at}: `height` is 1 to 4000, not {n}"))? as f32,
                    ),
                };
                Widget::TextPanel { id: identifier(&at, v, "id")?, height }
            }
            "separator" => {
                only(&at, object, &["type"])?;
                Widget::Separator
            }
            other => {
                return Err(format!(
                    "{at} has the type `{other}`, which isn't known: use {}",
                    WIDGET_TYPES.iter().map(|t| format!("`{t}`")).collect::<Vec<_>>().join(", ")
                ));
            }
        };
        Ok(widget)
    }
}

/// Reads a widget from its JSON, the widget reading itself.
impl<'de> Deserialize<'de> for Widget {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Widget, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Widget::from_value(&value).map_err(de::Error::custom)
    }
}

fn only(at: &str, object: &serde_json::Map<String, Value>, allowed: &[&str]) -> Result<(), String> {
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!(
                "{at} has the field `{key}`, which a `{}` hasn't",
                object.get("type").and_then(Value::as_str).unwrap_or("widget")
            ));
        }
    }
    Ok(())
}

fn text(at: &str, v: &Value, key: &str) -> Result<String, String> {
    match v.get(key) {
        Some(Value::String(s)) => {
            let count = s.chars().count();
            if count > MAX_TEXT {
                return Err(format!("{at}: `{key}` is {count} characters; it may be {MAX_TEXT}"));
            }
            Ok(s.clone())
        }
        Some(other) => Err(format!("{at}: `{key}` must be a string, not {other}")),
        None => Err(format!("{at}: `{key}` is required")),
    }
}

fn identifier(at: &str, v: &Value, key: &str) -> Result<String, String> {
    let id = text(at, v, key)?;
    check_id(&format!("{at}: its `{key}`"), &id)?;
    Ok(id)
}

fn flag(at: &str, v: &Value, key: &str) -> Result<bool, String> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(other) => Err(format!("{at}: `{key}` must be true or false, not {other}")),
    }
}

fn number(at: &str, v: &Value, key: &str) -> Result<Option<f64>, String> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n.as_f64().filter(|x| x.is_finite()).map(Some).ok_or_else(|| format!("{at}: `{key}` isn't a number")),
        Some(other) => Err(format!("{at}: `{key}` must be a number, not {other}")),
    }
}

fn unit(at: &str, v: &Value) -> Result<Unit, String> {
    match v.get("unit") {
        None | Some(Value::Null) => Ok(Unit::None),
        Some(unit) => serde_json::from_value::<Unit>(unit.clone())
            .map_err(|_| format!("{at}: `unit` is `length`, `area`, `volume`, `angle` or `none`, not {unit}")),
    }
}

fn children(at: &str, v: &Value) -> Result<Vec<Widget>, String> {
    match v.get("widgets") {
        Some(Value::Array(list)) => list.iter().map(Widget::from_value).collect(),
        Some(other) => Err(format!("{at}: `widgets` must be an array, not {other}")),
        None => Err(format!("{at}: `widgets` is required")),
    }
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

    /// Whether it is sound: names are names, the ABI is one this build speaks, the permissions are known, no function
    /// is declared twice, and each panel is made of widgets that name this plugin's own functions and unique ids.
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
        if !SUPPORTED.contains(&self.abi) {
            return Err(format!("host ABI {}, this BIMCraft speaks ABI {}: rebuild the plugin for it", self.abi, spoken()));
        }
        check_relative("the module", &self.module)?;
        if self.functions.len() > MAX_FUNCTIONS {
            return Err(format!("it declares {} functions; a plugin may declare {MAX_FUNCTIONS}", self.functions.len()));
        }
        for p in &self.permissions {
            if !PERMISSIONS.contains(&p.as_str()) {
                return Err(format!(
                    "unknown permission `{p}`: the host ABI {} knows {}",
                    spoken(),
                    PERMISSIONS.iter().map(|p| format!("`{p}`")).collect::<Vec<_>>().join(", ")
                ));
            }
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
        if !self.panels.is_empty() && self.abi < 2 {
            return Err(format!("a plugin of ABI {} can't declare panels: make it `abi: 2` and build it for the newer host", self.abi));
        }
        if self.panels.len() > MAX_PANELS {
            return Err(format!("it declares {} panels; a plugin may declare {MAX_PANELS}", self.panels.len()));
        }
        let mut named = Vec::new();
        for panel in &self.panels {
            check_id("a panel id", &panel.id)?;
            check_text("a panel's title", &panel.title)?;
            if named.iter().any(|id| id == &panel.id) {
                return Err(format!("two panels are named `{}`", panel.id));
            }
            named.push(panel.id.clone());
            for (what, function) in [("on_open", &panel.on_open), ("refresh", &panel.refresh)] {
                if let Some(function) = function
                    && !self.functions.iter().any(|f| &f.id == function)
                {
                    return Err(format!("the panel `{}` names the function `{function}` in `{what}`, which this plugin hasn't", panel.id));
                }
            }
            panel.check_widgets(self)?;
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

    /// Whether the plugin declared no ribbon tab: its functions go in the common Plugins section.
    pub fn tab_name(&self, f: &Function) -> Option<String> {
        f.tab.clone().filter(|t| !t.trim().is_empty())
    }

    pub fn panel_name(&self, f: &Function) -> String {
        f.panel.clone().filter(|p| !p.trim().is_empty()).unwrap_or_else(|| self.name.clone())
    }
}

/// The ABIs this build speaks, as a sentence: `1 and 2`.
fn spoken() -> String {
    match SUPPORTED {
        [one] => format!("{one}"),
        [rest @ .., last] => format!("{} and {last}", rest.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", ")),
        [] => "none".to_string(),
    }
}

impl Panel {
    /// Whether its widgets are sound: within the limits, each id once, each button and condition naming what exists.
    fn check_widgets(&self, manifest: &Manifest) -> Result<(), String> {
        let mut widgets = 0;
        let mut ids: Vec<String> = Vec::new();
        self.walk(1, &mut |widget, depth| {
            widgets += 1;
            if depth > MAX_WIDGET_DEPTH {
                return Err(format!("the panel `{}` nests its widgets {depth} deep; the most is {MAX_WIDGET_DEPTH}", self.id));
            }
            if let Some(id) = widget.id() {
                if ids.iter().any(|q| q == id) {
                    return Err(format!("two widgets of the panel `{}` are named `{id}`", self.id));
                }
                ids.push(id.to_string());
            }
            if let Widget::Button { function, id, .. } = widget
                && !manifest.functions.iter().any(|f| &f.id == function)
            {
                return Err(format!(
                    "the button `{id}` of the panel `{}` calls the function `{function}`, which this plugin hasn't",
                    self.id
                ));
            }
            if widgets > MAX_WIDGETS {
                return Err(format!("the panel `{}` has more than {MAX_WIDGETS} widgets", self.id));
            }
            Ok(())
        })?;
        // A condition may name a widget declared after it.
        self.walk(1, &mut |widget, _| {
            if let Widget::Button { id, enabled_when: Some(when), .. } = widget
                && !ids.iter().any(|q| q == &when.widget)
            {
                return Err(format!(
                    "the button `{id}` of the panel `{}` waits on the widget `{}`, which the panel hasn't",
                    self.id, when.widget
                ));
            }
            Ok(())
        })
    }

    fn walk(&self, depth: usize, f: &mut impl FnMut(&Widget, usize) -> Result<(), String>) -> Result<(), String> {
        for widget in &self.widgets {
            walk(widget, depth, f)?;
        }
        Ok(())
    }
}

fn walk(widget: &Widget, depth: usize, f: &mut impl FnMut(&Widget, usize) -> Result<(), String>) -> Result<(), String> {
    f(widget, depth)?;
    for child in widget.children() {
        walk(child, depth + 1, f)?;
    }
    Ok(())
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

    /// The panel of the specification's example.
    fn panel_manifest() -> serde_json::Value {
        serde_json::json!({
            "id": "rooms",
            "version": "0.2.0",
            "name": "Rooms",
            "abi": 2,
            "panels": [{
                "id": "areas",
                "title": "Area Summary",
                "order": 10,
                "dock": "right",
                "on_open": "summary",
                "refresh": "summary",
                "widgets": [
                    {"type": "label", "text": "Totals by level"},
                    {"type": "level", "id": "level", "label": "Level"},
                    {"type": "number", "id": "min_area", "label": "Min area", "unit": "area", "min": 0},
                    {"type": "checkbox", "id": "outer", "label": "Outer walls only"},
                    {"type": "select", "id": "mode", "label": "Mode",
                     "options": [{"value": "sum", "label": "Sum"}, {"value": "list", "label": "List"}]},
                    {"type": "library", "id": "wall_type", "label": "Wall type", "kind": "type:wall"},
                    {"type": "selection", "id": "ids", "label": "Elements"},
                    {"type": "button", "id": "run", "label": "Summarise", "function": "summary"},
                    {"type": "table", "id": "rows", "columns": [
                        {"key": "level", "label": "Level", "width": 120},
                        {"key": "area", "label": "Area", "width": 90, "align": "right"}
                    ]},
                    {"type": "text", "id": "note", "label": "Note", "readonly": true}
                ]
            }],
            "functions": [{"id": "summary", "label": "Area Summary", "tab": "Areas", "panel": "Rooms", "order": 1,
                           "params": {"type": "object", "properties": {}, "required": []}}]
        })
    }

    #[test]
    fn a_manifest_is_read_strictly() {
        assert!(check(sample()).is_ok());
        // An unknown field is refused, as an unknown command parameter is.
        let mut v = sample();
        v["extra"] = serde_json::json!(1);
        assert!(check(v).is_err());
        // A wrong ABI says which ones this app speaks.
        let mut v = sample();
        v["abi"] = serde_json::json!(3);
        assert!(check(v).unwrap_err().contains("ABI 1 and 2"));
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

    #[test]
    fn a_plugin_of_abi_one_cannot_declare_panels() {
        let mut v = panel_manifest();
        v["abi"] = serde_json::json!(1);
        let why = check(v).unwrap_err();
        assert!(why.contains("can't declare panels"), "{why}");
    }

    #[test]
    fn a_manifest_of_abi_two_without_panels_loads_as_before() {
        let mut v = sample();
        v["abi"] = serde_json::json!(2);
        let m = check(v).unwrap();
        assert!(m.panels.is_empty());
        assert_eq!(m.functions[0].ui, FunctionUi::Both);
    }

    #[test]
    fn a_panel_naming_an_unknown_function_is_refused() {
        let mut v = panel_manifest();
        v["panels"][0]["widgets"][7]["function"] = serde_json::json!("elsewhere");
        let why = check(v).unwrap_err();
        assert!(why.contains("elsewhere") && why.contains("hasn't"), "{why}");
        let mut v = panel_manifest();
        v["panels"][0]["refresh"] = serde_json::json!("elsewhere");
        assert!(check(v).unwrap_err().contains("refresh"));
    }

    #[test]
    fn an_unknown_widget_type_is_refused_and_names_the_widget() {
        let mut v = panel_manifest();
        v["panels"][0]["widgets"][1] = serde_json::json!({"type": "chart", "id": "level", "label": "Level"});
        let why = check(v).unwrap_err();
        assert!(why.contains("chart") && why.contains("level"), "{why}");
        // An unknown field of a widget is refused too.
        let mut v = panel_manifest();
        v["panels"][0]["widgets"][1]["extra"] = serde_json::json!(1);
        assert!(check(v).unwrap_err().contains("extra"));
    }

    #[test]
    fn two_widgets_with_one_id_are_refused() {
        let mut v = panel_manifest();
        v["panels"][0]["widgets"][2]["id"] = serde_json::json!("level");
        assert!(check(v).unwrap_err().contains("two widgets"));
    }

    #[test]
    fn the_panel_of_the_specification_reads() {
        let m = check(panel_manifest()).unwrap();
        let p = &m.panels[0];
        assert_eq!((p.id.as_str(), p.title.as_str(), p.dock), ("areas", "Area Summary", Dock::Right));
        assert_eq!(p.refresh.as_deref(), Some("summary"));
        assert_eq!(p.widgets.len(), 10);
        assert_eq!(m.functions[0].ui, FunctionUi::Both);
    }

    #[test]
    fn a_widget_nested_too_deep_or_one_too_many_is_refused() {
        let mut deep = serde_json::json!({"type": "label", "text": "x"});
        for _ in 0..MAX_WIDGET_DEPTH {
            deep = serde_json::json!({"type": "row", "widgets": [deep]});
        }
        let mut v = panel_manifest();
        v["panels"][0]["widgets"] = serde_json::json!([deep]);
        assert!(check(v).unwrap_err().contains("nests"));
        let many = Value::Array((0..=MAX_WIDGETS).map(|_| serde_json::json!({"type": "separator"})).collect());
        let mut v = panel_manifest();
        v["panels"][0]["widgets"] = many;
        assert!(check(v).unwrap_err().contains("more than"));
    }
}
