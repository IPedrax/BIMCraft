//! Standard colours: collections of named colours a material can be made of in a few clicks (`standard_color`). BIMCraft ships
//! the ones whose data is open: RAL Classic ([`crate::ral`]) and the CSS named colours of the W3C's CSS Color specification.
//! Any other standard is the user's to bring: a table they are licensed to use, imported from a CSV file into a file of their
//! own, next to their favorites, never into a project and never shipped.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::commands::{bad, flag, only, string};
use crate::engine::Session;
use crate::model::{MAX_TEXT, Material, Pattern};

/// The most collections a user's file holds.
pub const MAX_COLLECTIONS: usize = 64;
/// The most colours one collection holds.
pub const MAX_COLORS: usize = 10_000;
/// The largest CSV file imported, and the largest file of collections read, bytes.
const MAX_CSV: u64 = 4 << 20;
const MAX_FILE: u64 = 64 << 20;

/// What the CSS collection says of where it is from.
pub const CSS_SOURCE: &str = "the named colors of the W3C's CSS Color Module specification.";

/// A colour of a collection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Color {
    pub code: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub rgb: [u8; 3],
}

/// A collection of colours, and where they are from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collection {
    /// What commands call it, and what starts the names of the materials made of it.
    pub id: String,
    pub source: String,
    /// Whether the user brought it, and can take it away.
    pub imported: bool,
    pub colors: Vec<Color>,
}

/// A collection in the user's file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Imported {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    pub colors: Vec<Color>,
}

/// The user's file of collections.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kept {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub collections: BTreeMap<String, Imported>,
}

/// Where the user's collections are kept: beside their favorites.
pub fn default_path() -> Option<PathBuf> {
    Some(crate::favorites::default_path()?.with_file_name("standard_colors.json"))
}

impl Kept {
    /// The collections in the file at `path`; none where there's no file.
    pub fn read(path: &Path) -> Result<Kept, String> {
        let size = match std::fs::metadata(path) {
            Ok(m) => m.len(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Kept::default()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        if size > MAX_FILE {
            return Err(format!("{} is too large ({size} bytes)", path.display()));
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let kept: Kept = serde_json::from_str(&text).map_err(|e| format!("{} isn't a file of colours: {e}", path.display()))?;
        kept.check().map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(kept)
    }

    fn check(&self) -> Result<(), String> {
        if self.collections.len() > MAX_COLLECTIONS {
            return Err(format!("it holds more than {MAX_COLLECTIONS} collections"));
        }
        for (name, c) in &self.collections {
            check_name(name)?;
            if c.source.chars().count() > MAX_TEXT {
                return Err(format!("collection `{name}`: its source is longer than {MAX_TEXT} characters"));
            }
            if c.colors.is_empty() || c.colors.len() > MAX_COLORS {
                return Err(format!("collection `{name}` holds {} colours; it holds 1 to {MAX_COLORS}", c.colors.len()));
            }
            if let Some(x) =
                c.colors.iter().find(|x| x.code.is_empty() || x.code.chars().count() > MAX_TEXT || x.name.chars().count() > MAX_TEXT)
            {
                return Err(format!(
                    "collection `{name}`: colour `{}` needs a code, and a code and a name of up to {MAX_TEXT} characters",
                    x.code
                ));
            }
        }
        Ok(())
    }

    /// Written to `path`, whole or not at all: the folder made if need be.
    fn write(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        if let Some(folder) = path.parent().filter(|f| !f.as_os_str().is_empty()) {
            std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        }
        let part = path.with_extension("json.part");
        std::fs::write(&part, text).map_err(|e| format!("{}: {e}", part.display()))?;
        std::fs::rename(&part, path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

fn check_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() || name.chars().count() > MAX_TEXT {
        return Err(format!("a collection's name is up to {MAX_TEXT} characters, and not empty: `{name}`"));
    }
    Ok(())
}

/// The collections BIMCraft ships.
pub fn built_in() -> Vec<Collection> {
    let ral = crate::ral::ALL.iter().map(|c| Color { code: c.code.to_string(), name: c.name.to_string(), rgb: c.rgb() }).collect();
    let css = CSS.iter().map(|(name, rgb)| Color { code: (*name).to_string(), name: String::new(), rgb: *rgb }).collect();
    vec![
        Collection { id: "RAL".into(), source: crate::ral::SOURCE.into(), imported: false, colors: ral },
        Collection { id: "CSS".into(), source: CSS_SOURCE.into(), imported: false, colors: css },
    ]
}

/// The collections there are: BIMCraft's, then those in the user's file.
pub fn all(kept: &Kept) -> Vec<Collection> {
    let theirs = kept.collections.iter().map(|(id, c)| Collection {
        id: id.clone(),
        source: c.source.clone(),
        imported: true,
        colors: c.colors.clone(),
    });
    built_in().into_iter().chain(theirs).collect()
}

impl Color {
    /// How it is written in its collection: `RAL 3020`.
    pub fn label(&self, collection: &str) -> String {
        format!("{collection} {}", self.code)
    }

    /// The name of the material made of it: `RAL 3020 Traffic red`.
    pub fn material_name(&self, collection: &str) -> String {
        if self.name.is_empty() { self.label(collection) } else { format!("{collection} {} {}", self.code, self.name) }
    }

    /// Whether every word of `query` (lower case) is in its code or its name.
    fn matches(&self, collection: &str, query: &str) -> bool {
        let id = collection.to_lowercase();
        let code = self.code.to_lowercase();
        let text = format!("{id} {code} {id}{code} {}", self.name.to_lowercase());
        query.split_whitespace().all(|word| text.contains(word))
    }
}

impl Collection {
    /// The colours `query` finds by code or by name, in the collection's order; all of them for nothing typed.
    pub fn search(&self, query: &str) -> Vec<&Color> {
        let query = query.to_lowercase();
        self.colors.iter().filter(|c| c.matches(&self.id, &query)).collect()
    }

    /// The colour a code names, with or without the collection's name before it, whatever its case and spacing.
    pub fn find(&self, code: &Value) -> Option<&Color> {
        let typed = match code {
            Value::Number(n) => n.as_u64()?.to_string(),
            Value::String(s) => s.clone(),
            _ => return None,
        };
        let plain = |s: &str| -> String { s.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect() };
        let typed = plain(&typed);
        let id = plain(&self.id);
        let bare = typed.strip_prefix(id.as_str()).filter(|b| !b.is_empty()).unwrap_or(&typed);
        self.colors.iter().find(|c| plain(&c.code) == bare || plain(&c.code) == typed)
    }
}

/// The colour a cell of a table says: `#rrggbb` or `rrggbb`.
fn hex(cell: &str) -> Option<[u8; 3]> {
    let digits = cell.trim().trim_start_matches('#');
    if digits.len() != 6 || !digits.is_ascii() {
        return None;
    }
    let at = |k: usize| digits.get(k..k + 2).and_then(|d| u8::from_str_radix(d, 16).ok());
    Some([at(0)?, at(2)?, at(4)?])
}

/// The colours of a CSV table, a row each: `code, name, #rrggbb`, or `code, name, r, g, b`. A first row that isn't a colour is its
/// heading. What is wrong is said by its row.
pub fn parse_csv(text: &str) -> Result<Vec<Color>, String> {
    let rows = crate::parameters::table::read_csv(text);
    let mut colors: Vec<Color> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let cells: Vec<&str> = row.iter().map(|c| c.trim()).collect();
        if cells.iter().all(|c| c.is_empty()) {
            continue;
        }
        let number = |c: &str| c.parse::<u8>().ok();
        let rgb = match cells.as_slice() {
            [_, _, h] => hex(h),
            [_, _, r, g, b] => number(r).zip(number(g)).zip(number(b)).map(|((r, g), b)| [r, g, b]),
            _ => None,
        };
        let (Some(rgb), Some(code), Some(name)) = (rgb, cells.first(), cells.get(1)) else {
            if i == 0 {
                continue;
            }
            return Err(format!("row {}: it is `code, name, #rrggbb` or `code, name, r, g, b`", i + 1));
        };
        if code.is_empty() || code.chars().count() > MAX_TEXT || name.chars().count() > MAX_TEXT {
            return Err(format!("row {}: a colour needs a code, and a code and a name of up to {MAX_TEXT} characters", i + 1));
        }
        if colors.iter().any(|c| c.code == *code) {
            return Err(format!("row {}: the code `{code}` is there already", i + 1));
        }
        if colors.len() == MAX_COLORS {
            return Err(format!("a collection holds up to {MAX_COLORS} colours"));
        }
        colors.push(Color { code: (*code).to_string(), name: (*name).to_string(), rgb });
    }
    if colors.is_empty() {
        return Err("it holds no colours: each row is `code, name, #rrggbb` or `code, name, r, g, b`".into());
    }
    Ok(colors)
}

/// `standard_color`: the collections, their colours, a material made of one, and the user's own collections.
pub(crate) fn command(s: &mut Session, p: &Value) -> Result<Value, String> {
    let c = "standard_color";
    only(c, p, &["action", "collection", "search", "code", "name", "file", "source", "replace", "path"])?;
    let action =
        string(c, p, "action")?.unwrap_or_else(|| if p.get("code").is_some_and(|v| !v.is_null()) { "create" } else { "list" }.into());
    let path = match string(c, p, "path")? {
        Some(path) => Some(PathBuf::from(path)),
        None => default_path(),
    };
    let need_path = || path.clone().ok_or_else(|| bad(c, "there's no folder to keep colours in here: give `path`"));
    let mut kept = match &path {
        Some(path) => Kept::read(path).map_err(|e| bad(c, e))?,
        None => Kept::default(),
    };
    let wanted = string(c, p, "collection")?.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    let collections = all(&kept);
    let pick = || -> Result<&Collection, String> {
        let id = wanted.as_deref().unwrap_or("RAL");
        collections.iter().find(|x| x.id.eq_ignore_ascii_case(id)).ok_or_else(|| {
            let ids: Vec<&str> = collections.iter().map(|x| x.id.as_str()).collect();
            bad(c, format!("there's no collection `{id}`: one of {}", ids.join(", ")))
        })
    };
    match action.as_str() {
        "list" => {
            let x = pick()?;
            let found = x.search(&string(c, p, "search")?.unwrap_or_default());
            let colors: Vec<Value> = found
                .iter()
                .map(|k| json!({"code": k.label(&x.id), "name": k.name, "rgb": k.rgb, "material": k.material_name(&x.id)}))
                .collect();
            let names: Vec<Value> = collections
                .iter()
                .map(|k| json!({"id": k.id, "count": k.colors.len(), "imported": k.imported, "source": k.source}))
                .collect();
            Ok(json!({"collection": x.id, "count": colors.len(), "colors": colors, "source": x.source, "collections": names}))
        }
        "create" => {
            let x = pick()?;
            let code = p.get("code").filter(|v| !v.is_null()).ok_or_else(|| bad(c, "`code` is required: the colour's, from `list`"))?;
            let Some(color) = x.find(code) else {
                return Err(bad(c, format!("`code`: {code} isn't a colour of {}; `search` lists them", x.id)));
            };
            let name =
                string(c, p, "name")?.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| color.material_name(&x.id));
            let rgb = color.rgb;
            if s.model.materials.contains_key(&name) {
                return Ok(json!({"name": name, "created": false, "color": rgb}));
            }
            let material = Material {
                color: rgb,
                surface: Pattern::Solid,
                surface_color: Some(rgb),
                cut: Pattern::Solid,
                cut_color: Some(rgb),
                ..Material::default()
            };
            s.model_mut().materials.insert(name.clone(), material);
            Ok(json!({"name": name, "created": true, "color": rgb}))
        }
        "import" => {
            let id = wanted.clone().ok_or_else(|| bad(c, "`collection` is required: the name to keep it under"))?;
            check_name(&id).map_err(|e| bad(c, e))?;
            if built_in().iter().any(|x| x.id.eq_ignore_ascii_case(&id)) {
                return Err(bad(c, format!("`{id}` is one of BIMCraft's collections; name it otherwise")));
            }
            if kept.collections.contains_key(&id) && !flag(c, p, "replace")? {
                return Err(bad(c, format!("you have a collection `{id}` already: name it otherwise, or `replace` it")));
            }
            let file = string(c, p, "file")?.map(PathBuf::from).ok_or_else(|| bad(c, "`file` is required: the CSV file to read"))?;
            let size = std::fs::metadata(&file).map_err(|e| bad(c, format!("{}: {e}", file.display())))?.len();
            if size > MAX_CSV {
                return Err(bad(c, format!("{} is too large ({size} bytes)", file.display())));
            }
            let bytes = std::fs::read(&file).map_err(|e| bad(c, format!("{}: {e}", file.display())))?;
            let text = String::from_utf8_lossy(&bytes);
            let colors = parse_csv(text.trim_start_matches('\u{feff}')).map_err(|e| bad(c, format!("{}: {e}", file.display())))?;
            let count = colors.len();
            let source = string(c, p, "source")?.unwrap_or_default();
            kept.collections.insert(id.clone(), Imported { source, colors });
            kept.version = 1;
            kept.check().and_then(|()| kept.write(&need_path()?)).map_err(|e| bad(c, e))?;
            Ok(json!({"imported": id, "count": count}))
        }
        "remove" => {
            let id = wanted.clone().ok_or_else(|| bad(c, "`collection` is required: the one to take away"))?;
            if kept.collections.remove(&id).is_none() {
                return Err(bad(c, format!("you have no collection `{id}`; BIMCraft's own can't be taken away")));
            }
            kept.write(&need_path()?).map_err(|e| bad(c, e))?;
            Ok(json!({"removed": id}))
        }
        other => Err(bad(c, format!("`action` is list, create, import or remove, not `{other}`"))),
    }
}

/// The CSS named colours, by name.
pub const CSS: [(&str, [u8; 3]); 148] = [
    ("aliceblue", [240, 248, 255]),
    ("antiquewhite", [250, 235, 215]),
    ("aqua", [0, 255, 255]),
    ("aquamarine", [127, 255, 212]),
    ("azure", [240, 255, 255]),
    ("beige", [245, 245, 220]),
    ("bisque", [255, 228, 196]),
    ("black", [0, 0, 0]),
    ("blanchedalmond", [255, 235, 205]),
    ("blue", [0, 0, 255]),
    ("blueviolet", [138, 43, 226]),
    ("brown", [165, 42, 42]),
    ("burlywood", [222, 184, 135]),
    ("cadetblue", [95, 158, 160]),
    ("chartreuse", [127, 255, 0]),
    ("chocolate", [210, 105, 30]),
    ("coral", [255, 127, 80]),
    ("cornflowerblue", [100, 149, 237]),
    ("cornsilk", [255, 248, 220]),
    ("crimson", [220, 20, 60]),
    ("cyan", [0, 255, 255]),
    ("darkblue", [0, 0, 139]),
    ("darkcyan", [0, 139, 139]),
    ("darkgoldenrod", [184, 134, 11]),
    ("darkgray", [169, 169, 169]),
    ("darkgreen", [0, 100, 0]),
    ("darkgrey", [169, 169, 169]),
    ("darkkhaki", [189, 183, 107]),
    ("darkmagenta", [139, 0, 139]),
    ("darkolivegreen", [85, 107, 47]),
    ("darkorange", [255, 140, 0]),
    ("darkorchid", [153, 50, 204]),
    ("darkred", [139, 0, 0]),
    ("darksalmon", [233, 150, 122]),
    ("darkseagreen", [143, 188, 143]),
    ("darkslateblue", [72, 61, 139]),
    ("darkslategray", [47, 79, 79]),
    ("darkslategrey", [47, 79, 79]),
    ("darkturquoise", [0, 206, 209]),
    ("darkviolet", [148, 0, 211]),
    ("deeppink", [255, 20, 147]),
    ("deepskyblue", [0, 191, 255]),
    ("dimgray", [105, 105, 105]),
    ("dimgrey", [105, 105, 105]),
    ("dodgerblue", [30, 144, 255]),
    ("firebrick", [178, 34, 34]),
    ("floralwhite", [255, 250, 240]),
    ("forestgreen", [34, 139, 34]),
    ("fuchsia", [255, 0, 255]),
    ("gainsboro", [220, 220, 220]),
    ("ghostwhite", [248, 248, 255]),
    ("gold", [255, 215, 0]),
    ("goldenrod", [218, 165, 32]),
    ("gray", [128, 128, 128]),
    ("green", [0, 128, 0]),
    ("greenyellow", [173, 255, 47]),
    ("grey", [128, 128, 128]),
    ("honeydew", [240, 255, 240]),
    ("hotpink", [255, 105, 180]),
    ("indianred", [205, 92, 92]),
    ("indigo", [75, 0, 130]),
    ("ivory", [255, 255, 240]),
    ("khaki", [240, 230, 140]),
    ("lavender", [230, 230, 250]),
    ("lavenderblush", [255, 240, 245]),
    ("lawngreen", [124, 252, 0]),
    ("lemonchiffon", [255, 250, 205]),
    ("lightblue", [173, 216, 230]),
    ("lightcoral", [240, 128, 128]),
    ("lightcyan", [224, 255, 255]),
    ("lightgoldenrodyellow", [250, 250, 210]),
    ("lightgray", [211, 211, 211]),
    ("lightgreen", [144, 238, 144]),
    ("lightgrey", [211, 211, 211]),
    ("lightpink", [255, 182, 193]),
    ("lightsalmon", [255, 160, 122]),
    ("lightseagreen", [32, 178, 170]),
    ("lightskyblue", [135, 206, 250]),
    ("lightslategray", [119, 136, 153]),
    ("lightslategrey", [119, 136, 153]),
    ("lightsteelblue", [176, 196, 222]),
    ("lightyellow", [255, 255, 224]),
    ("lime", [0, 255, 0]),
    ("limegreen", [50, 205, 50]),
    ("linen", [250, 240, 230]),
    ("magenta", [255, 0, 255]),
    ("maroon", [128, 0, 0]),
    ("mediumaquamarine", [102, 205, 170]),
    ("mediumblue", [0, 0, 205]),
    ("mediumorchid", [186, 85, 211]),
    ("mediumpurple", [147, 112, 219]),
    ("mediumseagreen", [60, 179, 113]),
    ("mediumslateblue", [123, 104, 238]),
    ("mediumspringgreen", [0, 250, 154]),
    ("mediumturquoise", [72, 209, 204]),
    ("mediumvioletred", [199, 21, 133]),
    ("midnightblue", [25, 25, 112]),
    ("mintcream", [245, 255, 250]),
    ("mistyrose", [255, 228, 225]),
    ("moccasin", [255, 228, 181]),
    ("navajowhite", [255, 222, 173]),
    ("navy", [0, 0, 128]),
    ("oldlace", [253, 245, 230]),
    ("olive", [128, 128, 0]),
    ("olivedrab", [107, 142, 35]),
    ("orange", [255, 165, 0]),
    ("orangered", [255, 69, 0]),
    ("orchid", [218, 112, 214]),
    ("palegoldenrod", [238, 232, 170]),
    ("palegreen", [152, 251, 152]),
    ("paleturquoise", [175, 238, 238]),
    ("palevioletred", [219, 112, 147]),
    ("papayawhip", [255, 239, 213]),
    ("peachpuff", [255, 218, 185]),
    ("peru", [205, 133, 63]),
    ("pink", [255, 192, 203]),
    ("plum", [221, 160, 221]),
    ("powderblue", [176, 224, 230]),
    ("purple", [128, 0, 128]),
    ("rebeccapurple", [102, 51, 153]),
    ("red", [255, 0, 0]),
    ("rosybrown", [188, 143, 143]),
    ("royalblue", [65, 105, 225]),
    ("saddlebrown", [139, 69, 19]),
    ("salmon", [250, 128, 114]),
    ("sandybrown", [244, 164, 96]),
    ("seagreen", [46, 139, 87]),
    ("seashell", [255, 245, 238]),
    ("sienna", [160, 82, 45]),
    ("silver", [192, 192, 192]),
    ("skyblue", [135, 206, 235]),
    ("slateblue", [106, 90, 205]),
    ("slategray", [112, 128, 144]),
    ("slategrey", [112, 128, 144]),
    ("snow", [255, 250, 250]),
    ("springgreen", [0, 255, 127]),
    ("steelblue", [70, 130, 180]),
    ("tan", [210, 180, 140]),
    ("teal", [0, 128, 128]),
    ("thistle", [216, 191, 216]),
    ("tomato", [255, 99, 71]),
    ("turquoise", [64, 224, 208]),
    ("violet", [238, 130, 238]),
    ("wheat", [245, 222, 179]),
    ("white", [255, 255, 255]),
    ("whitesmoke", [245, 245, 245]),
    ("yellow", [255, 255, 0]),
    ("yellowgreen", [154, 205, 50]),
];
