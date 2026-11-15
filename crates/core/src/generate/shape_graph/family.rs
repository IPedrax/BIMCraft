//! Family files and the shape catalogue: read strictly, checked against the rule pack they name, and turned into the
//! indexed form the engine works on. The engine holds no room kind of any family: kinds reach it only through these files.

use std::collections::BTreeSet;
use std::fmt;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value as JsonValue;

use crate::graph_rules::Pack;

pub const CLINIC_FAMILY: &str = include_str!("../../../data/shape-families/clinic.json");
pub const HOSPITAL_FAMILY: &str = include_str!("../../../data/shape-families/hospital.json");
pub const HOUSE_FAMILY: &str = include_str!("../../../data/shape-families/house.json");
pub const OFFICE_PATTERN_FAMILY: &str = include_str!("../../../data/shape-families/office-pattern.json");
pub const SCHOOL_FAMILY: &str = include_str!("../../../data/shape-families/school.json");
pub const SHAPES: &str = include_str!("../../../data/shape-families/shapes.json");
pub const STUDIO_OFFICE_FAMILY: &str = include_str!("../../../data/shape-families/studio-office.json");

/// The built-in family files, by archetype.
pub fn builtin(archetype: &str) -> Option<&'static str> {
    [
        ("clinic", CLINIC_FAMILY),
        ("hospital", HOSPITAL_FAMILY),
        ("house", HOUSE_FAMILY),
        ("office-pattern", OFFICE_PATTERN_FAMILY),
        ("school", SCHOOL_FAMILY),
        ("studio-office", STUDIO_OFFICE_FAMILY),
    ]
    .iter()
    .find(|(name, _)| *name == archetype)
    .map(|(_, text)| *text)
}

/// The names of the built-in families, for messages.
pub fn builtin_names() -> &'static str {
    "clinic, hospital, house, office-pattern, school, studio-office"
}

/// A connection type (the spec's table): what it needs of a shared wall and how the extractor reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Conn {
    OpenFull,
    Cased,
    Single,
    Double,
    UnequalDouble,
    Sliding,
    DoubleSliding,
    Pocket,
    Bifold,
}

impl Conn {
    pub fn id(self) -> &'static str {
        match self {
            Conn::OpenFull => "open_full",
            Conn::Cased => "cased",
            Conn::Single => "single",
            Conn::Double => "double",
            Conn::UnequalDouble => "unequal_double",
            Conn::Sliding => "sliding",
            Conn::DoubleSliding => "double_sliding",
            Conn::Pocket => "pocket",
            Conn::Bifold => "bifold",
        }
    }

    /// The shortest shared wall the type needs, mm: its narrowest leaf plus 300 mm, or the opening.
    pub fn need(self) -> i64 {
        match self {
            Conn::OpenFull => 900,
            Conn::Cased | Conn::Double | Conn::UnequalDouble | Conn::DoubleSliding => 1500,
            Conn::Single | Conn::Sliding | Conn::Pocket | Conn::Bifold => 1050,
        }
    }

    /// The pack's means for it.
    pub fn means(self) -> &'static str {
        match self {
            Conn::OpenFull | Conn::Cased => "open",
            Conn::Single | Conn::Double | Conn::UnequalDouble => "swing",
            Conn::Sliding | Conn::DoubleSliding | Conn::Pocket => "sliding",
            Conn::Bifold => "folding",
        }
    }

    pub fn is_open(self) -> bool {
        matches!(self, Conn::OpenFull | Conn::Cased)
    }

    /// The door family, and its operation, of a door type.
    pub fn door(self) -> Option<(&'static str, &'static str)> {
        match self {
            Conn::Single => Some(("Single Door", "swing")),
            Conn::Double => Some(("Double Door", "double-swing")),
            Conn::Sliding => Some(("Sliding Door", "sliding")),
            Conn::UnequalDouble => Some(("Unequal Double Door", "double-swing")),
            Conn::DoubleSliding => Some(("Double Sliding Door", "double-sliding")),
            Conn::Pocket => Some(("Pocket Door", "sliding")),
            Conn::Bifold => Some((super::super::house::FOLDING_FAMILY, "folding")),
            Conn::OpenFull | Conn::Cased => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    Spine,
    EntryEnd,
    SpineEnd,
    OffSpine,
    BesideParent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Use {
    Parent,
    Required,
    Extra,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub status: String,
    /// For the people who review the family.
    #[serde(default)]
    #[allow(dead_code)]
    pub note: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PerFile {
    room: String,
    count: [i64; 2],
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoomFile {
    kind: String,
    #[serde(default)]
    name: Option<String>,
    role: String,
    place: Place,
    count: [i64; 2],
    #[serde(default)]
    per: Option<PerFile>,
    #[serde(default)]
    weight: Option<f64>,
    area_m2: [f64; 2],
    #[serde(default)]
    max_side_ratio: Option<f64>,
    min_side_mm: i64,
    max_links: i64,
    #[serde(default)]
    #[allow(dead_code)]
    source: Option<JsonValue>,
}

/// The rooms object, its order kept: the order of the room types in the file is the order the engine takes them in.
#[derive(Debug)]
struct Ordered(Vec<(String, RoomFile)>);

impl<'de> Deserialize<'de> for Ordered {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Ordered;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object of room types")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Ordered, A::Error> {
                let mut out: Vec<(String, RoomFile)> = Vec::new();
                while let Some((k, v)) = map.next_entry::<String, RoomFile>()? {
                    if out.iter().any(|(o, _)| *o == k) {
                        return Err(serde::de::Error::custom(format!("the room type `{k}` is given twice")));
                    }
                    out.push((k, v));
                }
                Ok(Ordered(out))
            }
        }
        d.deserialize_map(V)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryFile {
    rooms: Vec<String>,
    doors: [i64; 2],
    types: Vec<Conn>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkFile {
    from: String,
    to: String,
    #[serde(rename = "use")]
    use_: Use,
    #[serde(default)]
    weight: Option<f64>,
    #[serde(default)]
    chance: Option<f64>,
    types: Vec<Conn>,
    #[serde(default)]
    #[allow(dead_code)]
    source: Option<JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FamilyFile {
    schema_version: i64,
    family: String,
    pack_id: Option<String>,
    building_type: String,
    review: Review,
    /// The traces and scripts the file was written from: a person reads them before signing a delivery's rights statement.
    #[serde(default)]
    sources: Option<Vec<String>>,
    entry: Option<EntryFile>,
    rooms: Ordered,
    links: Vec<LinkFile>,
}

/// A room type, indexed.
#[derive(Clone, Debug)]
pub struct Ty {
    pub key: String,
    pub kind: String,
    pub name: String,
    pub role: String,
    pub place: Place,
    pub count: (i64, i64),
    /// The owner type's index and how many of this type each owner has.
    pub per: Option<(usize, i64, i64)>,
    pub weight: f64,
    pub area: (f64, f64),
    pub ratio: Option<f64>,
    pub min_side: i64,
    pub max_links: usize,
}

#[derive(Clone, Debug)]
pub struct Lk {
    pub from: usize,
    pub to: usize,
    pub use_: Use,
    pub weight: f64,
    pub chance: f64,
    pub types: Vec<Conn>,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub rooms: Vec<usize>,
    /// How many outside doors; this slice makes one.
    #[allow(dead_code)]
    pub doors: (i64, i64),
    pub types: Vec<Conn>,
}

#[derive(Clone, Debug)]
pub struct Family {
    pub name: String,
    pub pack_id: Option<String>,
    pub building_type: String,
    pub review: Review,
    /// The traces and scripts the file was written from, as the file lists them (a delivery's summary names them).
    pub sources: Vec<String>,
    pub entry: Option<Entry>,
    pub types: Vec<Ty>,
    pub links: Vec<Lk>,
}

/// The receiver's 16 building categories and 5 aliases, one word each.
const CATEGORIES: &str = "residential office commercial mixed-use educational research library assembly sports dining healthcare agricultural industrial utility parking university";
const ALIASES: &str = "clinic hospital house home school";

/// Residential kinds this project writes. Each pair is the family name and the `building_type`: the category plus
/// the kind in brackets. The receiver still accepts any other kind; these are the ones we use. The house family
/// stays the bare category `residential`.
#[allow(dead_code)]
pub const RESIDENTIAL_KINDS: [(&str, &str); 6] = [
    ("residential-single-family", "residential (single family)"),
    ("residential-duplex", "residential (duplex)"),
    ("residential-townhouse", "residential (townhouse)"),
    ("residential-multifamily-low-rise", "residential (multifamily low-rise)"),
    ("residential-multifamily-mid-rise", "residential (multifamily mid-rise)"),
    ("residential-multifamily-high-rise", "residential (multifamily high-rise)"),
];

/// Whether the receiver accepts a `building_type`: a category, optionally with a generic kind in brackets, or an alias.
pub fn building_type_ok(text: &str) -> bool {
    if ALIASES.split(' ').any(|a| a == text) {
        return true;
    }
    let (head, kind) = match text.split_once(" (") {
        Some((head, rest)) => match rest.strip_suffix(')') {
            Some(kind) => (head, Some(kind)),
            None => return false,
        },
        None => (text, None),
    };
    CATEGORIES.split(' ').any(|c| c == head)
        && kind.is_none_or(|k| {
            !k.is_empty()
                && k.starts_with(|c: char| c.is_ascii_lowercase())
                && k.chars().all(|c| c.is_ascii_lowercase() || c == ' ' || c == '-')
        })
}

fn finite_pair(p: [f64; 2]) -> bool {
    p[0].is_finite() && p[1].is_finite() && p[0] > 0.0 && p[0] <= p[1] && p[1] <= 1.0e6
}

impl Family {
    /// A family file read and checked: its schema, and, with a pack, that it does not widen the pack.
    pub fn parse(text: &str) -> Result<Family, String> {
        let file: FamilyFile = serde_json::from_str(text).map_err(|e| format!("the family file: {e}"))?;
        if file.schema_version != 1 {
            return Err(format!("`schema_version` is 1, not {}", file.schema_version));
        }
        if file.family.is_empty() || file.family.len() > 64 {
            return Err("`family` is a name of 1 to 64 characters".into());
        }
        if !building_type_ok(&file.building_type) {
            return Err(format!(
                "`building_type` {:?} is not one the receiver accepts",
                file.building_type.chars().take(40).collect::<String>()
            ));
        }
        if !matches!(file.review.status.as_str(), "reviewed" | "unreviewed") {
            return Err("`review.status` is `reviewed` or `unreviewed`".into());
        }
        let rooms = &file.rooms.0;
        if rooms.is_empty() || rooms.len() > 64 {
            return Err("`rooms` holds 1 to 64 room types".into());
        }
        let index = |key: &str| rooms.iter().position(|(k, _)| k == key);
        let mut types = Vec::new();
        for (key, r) in rooms {
            let bad = |what: &str| format!("room type `{key}`: {what}");
            if r.kind.is_empty() || r.role.is_empty() {
                return Err(bad("`kind` and `role` are not empty"));
            }
            if r.name.as_ref().is_some_and(String::is_empty) {
                return Err(bad("`name` is not empty"));
            }
            if r.count[0] < 0 || r.count[0] > r.count[1] || r.count[1] > 200 {
                return Err(bad("`count` is two whole numbers, fewest first, from 0 to 200"));
            }
            if !finite_pair(r.area_m2) {
                return Err(bad("`area_m2` is smallest and largest, above 0"));
            }
            if r.weight.is_some_and(|w| !w.is_finite() || w <= 0.0) {
                return Err(bad("`weight` is above 0"));
            }
            match r.max_side_ratio {
                Some(x) if !x.is_finite() || x < 1.0 => return Err(bad("`max_side_ratio` is at least 1")),
                None if r.place != Place::Spine => return Err(bad("only a spine room may have a null `max_side_ratio`")),
                _ => {}
            }
            if r.min_side_mm <= 0 || r.min_side_mm > 100_000 || r.max_links < 0 || r.max_links > 1000 {
                return Err(bad("`min_side_mm` is above 0 and `max_links` is not negative"));
            }
            let per = match &r.per {
                None => None,
                Some(p) => {
                    let owner = index(&p.room).filter(|o| rooms[*o].0 != *key).ok_or_else(|| bad("`per.room` names another room type"))?;
                    if p.count[0] < 0 || p.count[0] > p.count[1] || p.count[1] > 200 {
                        return Err(bad("`per.count` is two whole numbers, fewest first"));
                    }
                    Some((owner, p.count[0], p.count[1]))
                }
            };
            types.push(Ty {
                key: key.clone(),
                kind: r.kind.clone(),
                name: r.name.clone().unwrap_or_else(|| title_case(key)),
                role: r.role.clone(),
                place: r.place,
                count: (r.count[0], r.count[1]),
                per,
                weight: r.weight.unwrap_or(1.0),
                area: (r.area_m2[0], r.area_m2[1]),
                ratio: r.max_side_ratio,
                min_side: r.min_side_mm,
                max_links: r.max_links as usize,
            });
        }
        if !types.iter().any(|t| t.place == Place::Spine) {
            return Err("a family has at least one `spine` room type".into());
        }
        let entry = match &file.entry {
            None => None,
            Some(e) => {
                let rooms_ix: Option<Vec<usize>> = e.rooms.iter().map(|k| index(k)).collect();
                let rooms_ix = rooms_ix.filter(|r| !r.is_empty()).ok_or("`entry.rooms` names room types of the family")?;
                if e.doors[0] < 1 || e.doors[0] > e.doors[1] || e.types.is_empty() {
                    return Err("`entry` has `doors` from 1 and at least one connection type".into());
                }
                if rooms_ix.iter().any(|r| types[*r].place != Place::EntryEnd) {
                    return Err("an entry room type has `place` `entry_end`".into());
                }
                Some(Entry { rooms: rooms_ix, doors: (e.doors[0], e.doors[1]), types: e.types.clone() })
            }
        };
        let mut links = Vec::new();
        for (i, l) in file.links.iter().enumerate() {
            let (from, to) =
                index(&l.from).zip(index(&l.to)).ok_or_else(|| format!("link {i} names a room type the family doesn't have"))?;
            if l.types.is_empty() && l.use_ != Use::Parent {
                return Err(format!("link {i}: only a `parent` link may have no types"));
            }
            if l.use_ == Use::Extra && !l.chance.is_some_and(|c| (0.0..=1.0).contains(&c)) {
                return Err(format!("link {i}: an `extra` link has a `chance` from 0 to 1"));
            }
            if l.weight.is_some_and(|w| !w.is_finite() || w <= 0.0) {
                return Err(format!("link {i}: `weight` is above 0"));
            }
            links.push(Lk {
                from,
                to,
                use_: l.use_,
                weight: l.weight.unwrap_or(1.0),
                chance: l.chance.unwrap_or(1.0),
                types: l.types.clone(),
            });
        }
        if let Some(c) = file.entry.iter().flat_map(|e| &e.types).find(|c| c.door().is_none()) {
            return Err(format!("`entry.types`: the connection type `{}` is not a door, and the way in is one", c.id()));
        }
        let fam = Family {
            name: file.family,
            pack_id: file.pack_id,
            building_type: file.building_type,
            review: file.review,
            sources: file.sources.unwrap_or_default(),
            entry,
            types,
            links,
        };
        fam.within_pack()?;
        Ok(fam)
    }

    /// With a pack: each kind is the pack's, counts add up inside its ranges, areas inside its, link means are its.
    fn within_pack(&self) -> Result<(), String> {
        let Some(id) = &self.pack_id else { return Ok(()) };
        let pack = Pack::embedded(id)?;
        for t in &self.types {
            let k =
                pack.kinds.get(&t.kind).ok_or_else(|| format!("room type `{}`: the kind `{}` is not in the `{id}` pack", t.key, t.kind))?;
            if k.role != t.role {
                return Err(format!("room type `{}`: its role is `{}`, the pack's is `{}`", t.key, t.role, k.role));
            }
            let (lo, hi) = k.area_m2;
            if t.area.0 < lo || hi.is_some_and(|h| t.area.1 > h) {
                return Err(format!("room type `{}`: its area range is wider than the pack's", t.key));
            }
        }
        let kinds: BTreeSet<&str> = pack.kinds.keys().map(String::as_str).collect();
        for kind in kinds {
            let lo: i64 = self.types.iter().filter(|t| t.kind == kind).map(|t| t.count.0).sum();
            let hi: i64 = self.types.iter().filter(|t| t.kind == kind).map(|t| t.count.1).sum();
            if let Some(k) = pack.kinds.get(kind)
                && ((lo as f64) < k.count.0 || k.count.1.is_some_and(|m| hi as f64 > m))
            {
                return Err(format!("the counts of kind `{kind}` add up to {lo} to {hi}, which is wider than the pack's"));
            }
        }
        for l in &self.links {
            let (a, b) = (&self.types[l.from].kind, &self.types[l.to].kind);
            for c in &l.types {
                let allowed = pack.connections.iter().chain(&pack.folding_connections).any(|p| {
                    ((p.pair[0] == *a && p.pair[1] == *b) || (p.pair[0] == *b && p.pair[1] == *a)) && p.means.iter().any(|m| m == c.means())
                });
                if !allowed {
                    return Err(format!(
                        "the link from `{}` to `{}` by `{}` is not a means the pack lists",
                        self.types[l.from].key,
                        self.types[l.to].key,
                        c.id()
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn unreviewed(&self) -> bool {
        self.review.status == "unreviewed"
    }

    /// The fewest and the most rooms a plan of the family can have.
    pub fn count_limits(&self) -> (i64, i64) {
        let lo = self.types.iter().map(|t| t.count.0).sum();
        let hi = self
            .types
            .iter()
            .map(|t| match t.per {
                Some((owner, _, most)) => t.count.1.min(most.saturating_mul(self.types[owner].count.1)),
                None => t.count.1,
            })
            .sum();
        (lo, hi)
    }
}

fn title_case(key: &str) -> String {
    key.split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---- the shape catalogue

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Levels {
    pub a: Vec<i64>,
    pub b: Vec<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proportion {
    pub measure: String,
    pub range: [f64; 2],
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shape {
    pub id: String,
    pub corners: usize,
    pub levels: Levels,
    #[serde(default)]
    pub raise: Option<[f64; 2]>,
    pub proportion: Proportion,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShapesFile {
    #[allow(dead_code)]
    schema_version: i64,
    #[allow(dead_code)]
    note: String,
    shapes: Vec<Shape>,
}

/// The nine outlines, in the catalogue's order.
pub fn shapes() -> Result<Vec<Shape>, String> {
    let file: ShapesFile = serde_json::from_str(SHAPES).map_err(|e| format!("the embedded shape catalogue: {e}"))?;
    if file.shapes.iter().any(|s| s.levels.a.is_empty() || s.levels.a.len() != s.levels.b.len()) {
        return Err("the embedded shape catalogue: each side has the same number of pieces".into());
    }
    Ok(file.shapes)
}
