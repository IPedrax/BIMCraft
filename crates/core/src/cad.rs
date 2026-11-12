//! CAD drawings brought into a project: Revit's Import CAD and Link CAD. A project holds each drawing once, as a [`Cad`]: the
//! DXF it was read from, kept whole (deflated, in base64) so a file holds what the user brought in and reads it again as this
//! build does, and the drawing [`crate::dxf`] reads of it. A `CadImport` element places one on a level, in every plan of it or
//! in one view only. A linked one remembers its file, and `reload_cad` (and opening the project) reads it again.

use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};

use crate::dxf::{self, Color, Dxf, Entity, Shape};
use crate::geom::P2;
use crate::pen::Pen;
use crate::render::Rgb;

/// What reads a DWG drawing for a session: the DXF the file at a path becomes, and notes on what the read left out, or why it
/// can't be read. The core reads no DWG of its own; the apps hand their `Session` one (`bimcraft-dwg`, which reads it in a
/// child process), and a session without one says to save the drawing as DXF.
pub type DwgReader = Arc<dyn Fn(&std::path::Path) -> Result<(Vec<u8>, Vec<String>), String> + Send + Sync>;

/// What writes a DWG for a session: the DWG bytes an ASCII DXF the core wrote ([`crate::dxf_out`]) becomes, or why it can't. The
/// core writes no DWG of its own; the apps hand their `Session` one (`bimcraft-dwg`'s), and a session without one says to save
/// as DXF.
pub type DwgWriter = Arc<dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync>;

/// Drawings one project may hold.
pub const MAX_CADS: usize = 64;
/// What a project's drawings may hold among them, bytes of DXF.
pub const MAX_HELD: usize = 512 << 20;
/// Entities a drawing needs before any of it counts as far from the rest: a sparse one is all bulk.
const BULK_FROM: usize = 20;
/// How many times the middle of a drawing's span an entity may lie beyond it before it's far from the rest.
const FAR: f64 = 20.0;

/// How an import's colours are drawn: as the drawing has them, inverted (light on dark becomes dark on light), or all in the
/// view's ink.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Colors {
    #[default]
    Preserve,
    Invert,
    Black,
}

impl Colors {
    pub fn from_name(name: &str) -> Option<Colors> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "preserve" => Colors::Preserve,
            "invert" => Colors::Invert,
            "black" | "black and white" | "black_and_white" => Colors::Black,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Colors::Preserve => "preserve",
            Colors::Invert => "invert",
            Colors::Black => "black",
        }
    }
}

/// A drawing as a project holds it. Cheap to clone: the drawing is shared.
#[derive(Clone)]
pub struct Cad {
    /// The file's name, for people to know it by.
    source: String,
    /// Where it was read from, when it's linked: `reload_cad`, and opening the project, read it again from there.
    path: Option<String>,
    bytes: Arc<Vec<u8>>,
    dxf: Arc<Dxf>,
    /// The box round its bulk, drawing units: all of it but what lies far from the rest.
    bulk: Option<[P2; 2]>,
    /// How many of its entities lie far from the rest.
    far: usize,
    /// What a project file holds of it, made the first time it's saved.
    packed: Arc<OnceLock<CadFile>>,
}

impl std::fmt::Debug for Cad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Cad({:?}, {} entities)", self.source, self.dxf.entities.len())
    }
}

impl PartialEq for Cad {
    fn eq(&self, other: &Cad) -> bool {
        self.source == other.source && self.path == other.path && (Arc::ptr_eq(&self.bytes, &other.bytes) || self.bytes == other.bytes)
    }
}

impl Cad {
    /// The drawing a DXF file's `bytes` hold, known by `source`, linked to `path` if it's given; or why it can't be held.
    pub fn new(source: &str, path: Option<String>, bytes: Vec<u8>) -> Result<Cad, String> {
        let dxf = dxf::read(&bytes)?;
        if dxf.entities.is_empty() {
            return Err("it draws nothing in model space".into());
        }
        let (bulk, far) = bulk(&dxf);
        Ok(Cad {
            source: source.chars().take(crate::model::MAX_TEXT).collect(),
            path: path.map(|p| p.chars().take(4096).collect()),
            bytes: Arc::new(bytes),
            dxf: Arc::new(dxf),
            bulk,
            far,
            packed: Arc::default(),
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// The file it's linked to, if it is.
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// The same drawing, linked to `path` or not.
    pub fn linked(mut self, path: Option<String>) -> Cad {
        self.path = path;
        self.packed = Arc::default();
        self
    }

    pub fn dxf(&self) -> &Dxf {
        &self.dxf
    }

    /// Bytes of DXF it holds.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Whether `bytes` are what it was read from.
    pub fn same_bytes(&self, bytes: &[u8]) -> bool {
        self.bytes.as_slice() == bytes
    }

    /// The box round its bulk, drawing units: what centring it and fitting a view to it go by.
    pub fn bulk(&self) -> Option<[P2; 2]> {
        self.bulk
    }

    /// How many of its entities lie far from the rest: the stray lines that make a drawing's extents huge.
    pub fn far(&self) -> usize {
        self.far
    }

    fn file(&self) -> &CadFile {
        self.packed.get_or_init(|| CadFile {
            source: self.source.clone(),
            path: self.path.clone(),
            dxf: crate::commands::base64(&crate::cloud::deflate(&self.bytes)),
        })
    }

    /// The colour an entity is drawn in when `colors` says how, or none for the view's ink: a colour by layer is its layer's,
    /// by block outside a block the ink's, index 7 the ink.
    pub fn color(&self, e: &Entity, colors: Colors) -> Option<Rgb> {
        let own = match e.color {
            Color::ByLayer => self.dxf.layer(&e.layer).map_or(Color::Index(7), |l| l.color),
            c => c,
        };
        let rgb = own.rgb()?;
        match colors {
            Colors::Preserve => Some(rgb),
            Colors::Invert => Some(Rgb(255 - rgb.0, 255 - rgb.1, 255 - rgb.2)),
            Colors::Black => None,
        }
    }

    /// The pen an entity is drawn with: its weight's, else its layer's, else the finest.
    pub fn pen(&self, e: &Entity) -> Pen {
        e.weight.or_else(|| self.dxf.layer(&e.layer).and_then(|l| l.weight)).filter(|w| *w > 0.0).map_or(Pen::FINE, Pen::nearest)
    }

    /// An entity's dash and gap, drawing units, when its linetype (its own, else its layer's) has dashes; times `ltscale`.
    pub fn dash(&self, e: &Entity) -> Option<[f64; 2]> {
        let name = e.linetype.as_deref().or_else(|| self.dxf.layer(&e.layer).map(|l| l.linetype.as_str()))?;
        let lt = self.dxf.linetypes.iter().find(|l| l.name.eq_ignore_ascii_case(name))?;
        let dash = lt.dashes.iter().copied().find(|d| *d > 0.0);
        let gap = lt.dashes.iter().copied().find(|d| *d < 0.0)?.abs();
        let k = self.dxf.ltscale;
        // A dot is drawn as a short dash.
        Some([dash.unwrap_or(gap / 4.0) * k, gap * k])
    }

    /// Whether an entity is drawn when the layers `hidden` are.
    pub fn shows(&self, e: &Entity, hidden: &BTreeSet<String>) -> bool {
        !hidden.contains(&e.layer)
    }
}

/// What a shape draws as lines, drawing units: each run of points, and whether it closes. A circle or an arc is points on it; a
/// point and a text draw none.
pub fn paths(shape: &Shape) -> Vec<(Vec<P2>, bool)> {
    match shape {
        Shape::Line(a, b) => vec![(vec![*a, *b], false)],
        Shape::Polyline { pts, closed } => vec![(pts.clone(), *closed)],
        Shape::Circle { c, r } => {
            let mut pts = dxf::arc_points(*c, *r, 0.0, 360.0);
            pts.pop();
            vec![(pts, true)]
        }
        Shape::Arc { c, r, start, end } => vec![(dxf::arc_points(*c, *r, *start, end - start), false)],
        Shape::Fill { rings, .. } => rings.iter().map(|r| (r.clone(), true)).collect(),
        Shape::Point(_) | Shape::Text { .. } => Vec::new(),
    }
}

/// What kind of entity a shape came from, as Query names it.
pub fn kind(shape: &Shape) -> &'static str {
    match shape {
        Shape::Line(..) => "line",
        Shape::Polyline { .. } => "polyline",
        Shape::Circle { .. } => "circle",
        Shape::Arc { .. } => "arc",
        Shape::Point(_) => "point",
        Shape::Text { .. } => "text",
        Shape::Fill { .. } => "hatch",
    }
}

/// How far a shape is from `p`, drawing units: from its lines, a point or a text's point; 0 inside a fill.
fn distance(shape: &Shape, p: P2) -> f64 {
    match shape {
        Shape::Point(q) | Shape::Text { at: q, .. } => q.dist(p),
        Shape::Fill { rings, .. } if rings.iter().filter(|r| crate::geom::contains(r, p)).count() % 2 == 1 => 0.0,
        _ => paths(shape)
            .iter()
            .flat_map(|(pts, closed)| {
                let close = (*closed).then(|| pts.last().copied().zip(pts.first().copied())).flatten();
                pts.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).chain(close).collect::<Vec<_>>()
            })
            .map(|(a, b)| crate::geom::dist_to_segment(p, a, b))
            .fold(f64::INFINITY, f64::min),
    }
}

/// The entity of a placed drawing nearest `at` in plan within `reach` mm, of those it shows, and how far it is, mm.
pub fn nearest<'c>(cad: &'c Cad, ci: &crate::model::CadImport, at: P2, reach: f64) -> Option<(&'c Entity, f64)> {
    let q = crate::model::cad_unplace(ci, at);
    let reach = reach / ci.scale;
    cad.dxf()
        .entities
        .iter()
        .filter(|e| cad.shows(e, &ci.hidden))
        .filter(|e| {
            let [lo, hi] = e.shape.bounds();
            q.0 >= lo.0 - reach && q.0 <= hi.0 + reach && q.1 >= lo.1 - reach && q.1 <= hi.1 + reach
        })
        .map(|e| (e, distance(&e.shape, q)))
        .filter(|(_, d)| *d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(e, d)| (e, d * ci.scale))
}

/// The box round a drawing's bulk, and how many of its entities lie far from it: past [`FAR`] times the span of the middle
/// of its entities (the 2nd to the 98th of each hundred, along each axis) beyond it.
fn bulk(d: &Dxf) -> (Option<[P2; 2]>, usize) {
    let boxes: Vec<[P2; 2]> = d.entities.iter().map(|e| e.shape.bounds()).collect();
    if boxes.len() < BULK_FROM {
        return (dxf::union(boxes), 0);
    }
    let mid = |b: &[P2; 2]| (b[0] + b[1]) * 0.5;
    let mut xs: Vec<f64> = boxes.iter().map(|b| mid(b).0).collect();
    let mut ys: Vec<f64> = boxes.iter().map(|b| mid(b).1).collect();
    xs.sort_by(f64::total_cmp);
    ys.sort_by(f64::total_cmp);
    let at = |v: &[f64], q: usize| v.get((v.len() - 1) * q / 100).copied().unwrap_or(0.0);
    let (lo, hi) = (P2(at(&xs, 2), at(&ys, 2)), P2(at(&xs, 98), at(&ys, 98)));
    let reach = ((hi.0 - lo.0).max(hi.1 - lo.1) * FAR).max(1.0);
    let (lo, hi) = (lo - P2(reach, reach), hi + P2(reach, reach));
    let near = |b: &[P2; 2]| b[0].0 <= hi.0 && b[1].0 >= lo.0 && b[0].1 <= hi.1 && b[1].1 >= lo.1;
    let far = boxes.iter().filter(|b| !near(b)).count();
    (dxf::union(boxes.into_iter().filter(near)), far)
}

/// How a drawing goes in a project file: its DXF deflated (zlib) and in base64.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CadFile {
    source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    dxf: String,
}

impl Serialize for Cad {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.file().serialize(s)
    }
}

impl Cad {
    /// A drawing as a file holds it, unpacking no more than `most` bytes.
    fn from_file(f: CadFile, most: usize) -> Result<Cad, String> {
        let bytes = crate::cloud::inflate_of("a CAD drawing", &f.dxf, most.min(dxf::MAX_BYTES))?;
        let cad = Cad::new(&f.source, f.path.clone(), bytes).map_err(|e| format!("CAD drawing `{}`: {e}", f.source))?;
        let _ = cad.packed.set(f);
        Ok(cad)
    }
}

impl<'de> Deserialize<'de> for Cad {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Cad, D::Error> {
        Cad::from_file(CadFile::deserialize(d)?, dxf::MAX_BYTES).map_err(serde::de::Error::custom)
    }
}

/// A project file's drawings (`Model::cads`), read no further than a project may hold: the entry past [`MAX_CADS`] is refused
/// before it's unpacked, and each is unpacked to no more than what the others leave of [`MAX_HELD`].
pub(crate) fn deserialize_cads<'de, D: serde::Deserializer<'de>>(d: D) -> Result<std::collections::BTreeMap<String, Cad>, D::Error> {
    use serde::de::{Error, MapAccess, Visitor};
    struct Cads;
    impl<'de> Visitor<'de> for Cads {
        type Value = std::collections::BTreeMap<String, Cad>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a map of CAD drawings")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
            let (mut cads, mut held) = (Self::Value::new(), 0_usize);
            while let Some(name) = a.next_key::<String>()? {
                if cads.len() >= MAX_CADS {
                    return Err(A::Error::custom(format!("the file holds more than {MAX_CADS} CAD drawings, all a project may hold")));
                }
                let left = MAX_HELD.saturating_sub(held);
                let cad = Cad::from_file(a.next_value()?, left).map_err(A::Error::custom)?;
                held += cad.len();
                cads.insert(name, cad);
            }
            Ok(cads)
        }
    }
    d.deserialize_map(Cads)
}
