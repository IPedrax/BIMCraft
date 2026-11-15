//! A plan's doors, windows and components as `INSERT`s of blocks: one `BLOCK` for each kind of them, built from what the plan
//! draws for it, and an `INSERT` for each instance at its origin, turned by its angle, scale 1, on its element's layer. The
//! block holds the strokes (jambs, leaf and swing, glass, outline) in the instance's own frame; a `MARK` and a `TYPE`
//! attribute, invisible, say which one it is. Instances of a family type that draw the same share a block; one that draws
//! another way (a hand or a facing flipped, a parameter changed) has a block of its own.

use std::collections::{BTreeMap, BTreeSet};

use super::*;

/// The most blocks written; a drawing with more keeps the rest stroked.
const MAX_BLOCKS: usize = 5_000;

/// The most inserts written.
const MAX_INSERTS: usize = 50_000;

/// The most ways of drawing one family type that get a block; an instance past them keeps its strokes.
const MAX_VARIANTS: usize = 16;

/// How far two drawings of a kind may differ, mm, and still be one block: pixels are `f32`, so identical pieces of furniture
/// come back from the plan to model millimetres a little apart.
const SAME: f64 = 0.05;

/// The height of an attribute's letters, mm; it is never seen.
const ATTRIBUTE_HEIGHT: f64 = 100.0;

/// Where an instance stands and how it is turned, which is the frame its block is drawn in.
#[derive(Clone, Copy)]
struct Frame {
    origin: P2,
    /// Degrees counter-clockwise, 0 to 360.
    angle: f64,
    cos: f64,
    sin: f64,
}

impl Frame {
    /// The frame of door, window or component `id`, none for another element or one that stands nowhere.
    fn of(m: &Model, id: Id, shift: P2) -> Option<Frame> {
        let (origin, angle) = match m.elements.get(id)? {
            Element::Door(o) | Element::Window(o) => {
                let axis = walls::Axis::of(m.wall(o.host)?);
                let t = axis.tangent(o.offset);
                (axis.at(o.offset, 0.0), t.1.atan2(t.0).to_degrees())
            }
            Element::Component(c) => (c.at, c.angle),
            _ => return None,
        };
        if !(origin.0.is_finite() && origin.1.is_finite() && angle.is_finite()) {
            return None;
        }
        let angle = angle.rem_euclid(360.0);
        let (sin, cos) = angle.to_radians().sin_cos();
        Some(Frame { origin: origin + shift, angle, cos, sin })
    }

    /// `p`, model mm, in this frame.
    fn local(&self, p: P2) -> P2 {
        let d = p - self.origin;
        P2(d.0 * self.cos + d.1 * self.sin, d.1 * self.cos - d.0 * self.sin)
    }

    fn ink(&self, ink: &Ink) -> Ink {
        let each = |pts: &[P2]| pts.iter().map(|p| self.local(*p)).collect::<Vec<P2>>();
        let shape = match &ink.shape {
            Shape::Line { pts, closed, weight } => Shape::Line { pts: each(pts), closed: *closed, weight: *weight },
            Shape::Fill { rings } => Shape::Fill { rings: rings.iter().map(|r| each(r)).collect() },
            Shape::Arc { centre, radius, from, to, weight } => Shape::Arc {
                centre: self.local(*centre),
                radius: *radius,
                from: (from - self.angle).rem_euclid(360.0),
                to: (to - self.angle).rem_euclid(360.0),
                weight: *weight,
            },
        };
        Ink { layer: ink.layer.clone(), ltype: ink.ltype.clone(), color: ink.color, shape }
    }
}

/// What a kind of instance is called and says of itself, and who it is.
struct Kind {
    family: String,
    type_name: String,
    mark: String,
}

impl Kind {
    fn of(m: &Model, id: Id) -> Option<Kind> {
        match m.elements.get(id)? {
            Element::Door(o) | Element::Window(o) => {
                Some(Kind { family: o.family.clone().unwrap_or_default(), type_name: o.type_name.clone(), mark: o.mark.clone() })
            }
            Element::Component(c) => {
                let mark = match c.params.get("Mark") {
                    Some(serde_json::Value::String(s)) => s.clone(),
                    Some(serde_json::Value::Number(n)) => n.to_string(),
                    _ => String::new(),
                };
                Some(Kind { family: c.family.clone(), type_name: c.type_name.clone(), mark })
            }
            _ => None,
        }
    }

    /// `family: type`, or the type alone.
    fn label(&self) -> String {
        match (self.family.trim(), self.type_name.trim()) {
            ("", t) => t.to_string(),
            (f, "") => f.to_string(),
            (f, t) => format!("{f}: {t}"),
        }
    }
}

/// A block's definition: what it draws, in its frame.
struct Def {
    name: String,
    record: u64,
    begin: u64,
    end: u64,
    /// The `MARK` and `TYPE` attribute definitions' handles.
    attdefs: [u64; 2],
    layer: String,
    body: String,
    ink: Vec<Ink>,
}

/// The blocks a drawing writes and their inserts.
#[derive(Default)]
pub(super) struct Blocks {
    /// The `INSERT`s not yet written, each with its attributes, by the element they stand for.
    inserts: BTreeMap<Id, String>,
    defs: Vec<Def>,
    /// The elements whose strokes a block holds.
    held: BTreeSet<Id>,
}

/// The attributes a block carries: its `MARK` and `TYPE`. Under the delivery profile the `TYPE` is empty, so a block says only its
/// mark but keeps the entities it has without the profile.
const TAGS: [&str; 2] = ["MARK", "TYPE"];

/// Whether two ways of drawing are one, to within [`SAME`].
fn same(a: &[Ink], b: &[Ink]) -> bool {
    let near = |p: &[P2], q: &[P2]| p.len() == q.len() && p.iter().zip(q).all(|(p, q)| p.dist(*q) <= SAME);
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.layer == b.layer
                && a.ltype.as_ref().map(|l| &l.0) == b.ltype.as_ref().map(|l| &l.0)
                && a.color == b.color
                && match (&a.shape, &b.shape) {
                    (Shape::Line { pts: p, closed: c, weight: w }, Shape::Line { pts: q, closed: d, weight: v }) => {
                        c == d && w == v && near(p, q)
                    }
                    (Shape::Fill { rings: p }, Shape::Fill { rings: q }) => p.len() == q.len() && p.iter().zip(q).all(|(p, q)| near(p, q)),
                    (
                        Shape::Arc { centre: c, radius: r, from: f, to: t, weight: w },
                        Shape::Arc { centre: c2, radius: r2, from: f2, to: t2, weight: w2 },
                    ) => w == w2 && c.dist(*c2) <= SAME && (r - r2).abs() <= SAME && (f - f2).abs() <= SAME && (t - t2).abs() <= SAME,
                    _ => false,
                }
        })
}

impl Blocks {
    /// The blocks and inserts of the doors, windows and components `clip` (the drawing's strokes, see [`clipped`]) draws, moved
    /// by `shift`, in `space`. Empty for a view that cannot place text, which keeps every stroke as drawn. Handles and names come
    /// from `sh`, which the file's other drawings draw on too; what a block draws goes on its layers and in its line types.
    pub fn of(
        d: &Drawing,
        m: &Model,
        (texts, delivery): (bool, bool),
        clip: &[(usize, Cow<'_, Prim>)],
        sh: &mut Shared,
        space: u64,
        shift: P2,
    ) -> Blocks {
        let mut out = Blocks::default();
        if !texts {
            return out;
        }
        let Shared { w, layers, ltypes, names } = sh;
        let mut frames: BTreeMap<Id, Option<Frame>> = BTreeMap::new();
        let mut drawn: BTreeMap<Id, Vec<Ink>> = BTreeMap::new();
        // Under the delivery profile, the chords of an opening's swing are the arc they stand for.
        let mut swings: BTreeMap<Id, Vec<(P2, Vec<P2>)>> = BTreeMap::new();
        for (i, prim) in clip {
            let Some(owner) = d.owners.get(*i).copied().flatten() else { continue };
            let part = d.parts.get(*i).copied().unwrap_or_default();
            // The lettering is the TEXT beside; its strokes are not part of a block.
            if part == Part::Text {
                continue;
            }
            let Some(frame) = *frames.entry(owner).or_insert_with(|| Frame::of(m, owner, shift)) else { continue };
            let Some(ink) = ink(d, prim.as_ref(), part, layer(m, Some(owner))) else { continue };
            let ink = match delivery {
                true => {
                    let arcs = swings.entry(owner).or_insert_with(|| super::delivery::swings(m, owner));
                    super::delivery::as_arc(&ink, arcs, shift).unwrap_or(ink)
                }
                false => ink,
            };
            let ink = frame.ink(&ink);
            let held = drawn.entry(owner).or_default();
            // The pieces of one swing, cut apart, are each the whole arc: keep it once.
            if matches!(ink.shape, Shape::Arc { .. }) && held.iter().any(|h| same(std::slice::from_ref(h), std::slice::from_ref(&ink))) {
                continue;
            }
            held.push(ink);
        }
        let mut variants: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
        let mut inserts = 0;
        for (owner, ink) in drawn {
            let (Some(Some(frame)), Some(kind)) = (frames.get(&owner), Kind::of(m, owner)) else { continue };
            if inserts >= MAX_INSERTS {
                break;
            }
            let of_kind = variants.entry((kind.family.clone(), kind.type_name.clone())).or_default();
            let found = of_kind.iter().copied().find(|k| out.defs.get(*k).is_some_and(|def| same(&def.ink, &ink)));
            let k = match found {
                Some(k) => k,
                None => {
                    if of_kind.len() >= MAX_VARIANTS || out.defs.len() >= MAX_BLOCKS {
                        continue;
                    }
                    let name = safe(&kind.label(), "BIM-BLOCK", &names.blocks);
                    names.blocks.insert(name.to_uppercase());
                    let record = w.handle();
                    let mut def = Def {
                        name,
                        record,
                        begin: w.handle(),
                        end: w.handle(),
                        attdefs: [w.handle(), w.handle()],
                        layer: layer(m, Some(owner)).to_string(),
                        body: String::new(),
                        ink: Vec::new(),
                    };
                    for ink in &ink {
                        layers.insert(ink.layer.clone());
                        if let Some((name, dashes)) = &ink.ltype {
                            ltypes.insert(name.clone(), dashes.clone());
                        }
                        put(&mut def.body, w.handle(), record, ink);
                    }
                    def.ink = ink;
                    out.defs.push(def);
                    let k = out.defs.len() - 1;
                    of_kind.push(k);
                    k
                }
            };
            let Some(def) = out.defs.get(k) else { continue };
            layers.insert(def.layer.clone());
            let label = kind.label();
            let label = if label.is_empty() { def.name.clone() } else { label };
            let mut text = String::new();
            insert(&mut text, w, space, def, frame, &[kind.mark.as_str(), if delivery { "" } else { label.as_str() }], &TAGS);
            out.inserts.insert(owner, text);
            out.held.insert(owner);
            inserts += 1;
        }
        out
    }

    /// Whether a block holds what `owner` draws, so the drawing does not.
    pub fn holds(&self, owner: Id) -> bool {
        self.held.contains(&owner)
    }

    /// `owner`'s `INSERT`, once: written where its first stroke stood, so what draws over it still does.
    pub fn take(&mut self, owner: Id) -> Option<String> {
        self.inserts.remove(&owner)
    }

    /// The `INSERT`s no stroke has asked for.
    pub fn rest(&mut self) -> String {
        std::mem::take(&mut self.inserts).into_values().collect()
    }

    /// Its blocks' records, for the `BLOCK_RECORD` table.
    pub fn block_records(&self, s: &mut String, owner: u64) {
        for b in &self.defs {
            pair(s, 0, "BLOCK_RECORD");
            record(s, 5, b.record, owner, "AcDbBlockTableRecord", &b.name);
        }
    }

    /// How many blocks there are.
    pub fn len(&self) -> usize {
        self.defs.len()
    }

    /// Its blocks, for the `BLOCKS` section.
    pub fn block_section(&self, s: &mut String) {
        for b in &self.defs {
            pair(s, 0, "BLOCK");
            hex(s, 5, b.begin);
            hex(s, 330, b.record);
            pair(s, 100, "AcDbEntity");
            pair(s, 8, "0");
            pair(s, 100, "AcDbBlockBegin");
            pair(s, 2, &b.name);
            // Bit 2: it holds attribute definitions.
            pair(s, 70, 2);
            point3(s, 10, P2(0.0, 0.0));
            pair(s, 3, &b.name);
            pair(s, 1, "");
            s.push_str(&b.body);
            for (tag, h) in TAGS.iter().zip(b.attdefs) {
                pair(s, 0, "ATTDEF");
                hex(s, 5, h);
                hex(s, 330, b.record);
                pair(s, 100, "AcDbEntity");
                pair(s, 8, &b.layer);
                pair(s, 100, "AcDbText");
                point3(s, 10, P2(0.0, 0.0));
                pair(s, 40, num(ATTRIBUTE_HEIGHT));
                pair(s, 1, "");
                pair(s, 100, "AcDbAttributeDefinition");
                pair(s, 280, 0);
                pair(s, 3, *tag);
                pair(s, 2, *tag);
                // Invisible.
                pair(s, 70, 1);
            }
            pair(s, 0, "ENDBLK");
            hex(s, 5, b.end);
            hex(s, 330, b.record);
            pair(s, 100, "AcDbEntity");
            pair(s, 8, "0");
            pair(s, 100, "AcDbBlockEnd");
        }
    }
}

/// `def`'s `INSERT` at `frame` with its `MARK` and `TYPE` (`values`), then the `SEQEND` that closes them.
fn insert(s: &mut String, w: &mut Out, space: u64, def: &Def, frame: &Frame, values: &[&str], tags: &[&str]) {
    let layer = &def.layer;
    pair(s, 0, "INSERT");
    owned(s, w.handle(), space);
    pair(s, 8, layer);
    pair(s, 100, "AcDbBlockReference");
    pair(s, 66, 1);
    pair(s, 2, &def.name);
    point3(s, 10, frame.origin);
    for code in [41, 42, 43] {
        pair(s, code, 1);
    }
    pair(s, 50, num(frame.angle));
    for (tag, value) in tags.iter().zip(values) {
        let value: String = escaped(value, false).chars().take(200).collect();
        pair(s, 0, "ATTRIB");
        owned(s, w.handle(), space);
        pair(s, 8, layer);
        pair(s, 100, "AcDbText");
        point3(s, 10, frame.origin);
        pair(s, 40, num(ATTRIBUTE_HEIGHT));
        pair(s, 1, value);
        pair(s, 50, num(frame.angle));
        pair(s, 100, "AcDbAttribute");
        pair(s, 2, *tag);
        // Invisible: the plan's own lettering is the TEXT beside.
        pair(s, 70, 1);
    }
    pair(s, 0, "SEQEND");
    owned(s, w.handle(), space);
    pair(s, 8, layer);
}
