//! A family type's graphics: the tree of objects Revit keeps for each type in the project, and how one is read.
//!
//! Each family type has its graphics (the class `GElement`, a `GRep`) written once in a partition, the type's id as a `u64`
//! eight bytes before a `u32` and a `u32`, then the tag of the class `GElement` and the root object, whose first field (its
//! `GInfo`) holds the type's id again as its `m_tag`. That is how a root is found (`roots`): nothing else marks it.
//!
//! The objects are written **breadth first**: the root, then the objects it owns in the order its fields name them, then theirs,
//! a queue. Each object is its class's fields in the order the file's own schema lists them (`Formats/Latest`, [`crate::schema`]),
//! its bases' first. A field's type says how it is written:
//!
//! - a byte (kinds 1 and 2), a `u32` (4 and 5), an `f64` (6 and 7), a `u64` (11);
//! - so many side by side (modifier `0x10` and a count), or a `u32` count and that many (`0x50`, `0x60`);
//! - an array (13) as its inner type, an object by value (14, modifier 0) as its class's fields;
//! - an owned object (14, modifier 1) as a `u32` index, 0 for none, and the `u16` tag of its class: it is written in its turn in
//!   the queue; or, where the index is `0xffffffff`, **inline**, right after the last object its owner owns (after the owner
//!   itself where it owns none);
//! - a weak pointer (14, modifier 3) as a `u32` index alone, the object written elsewhere.
//!
//! A read is bounded: the objects, a list's length, how deep objects by value nest and how many values one tree may take
//! ([`MAX_OBJECTS`], [`MAX_LIST`], [`MAX_DEPTH`], [`MAX_VALUES`]). A tree that breaks a rule (a kind or class the schema does not
//! give, an index owned twice, an inline object that never finds its place) is refused, saying where, never guessed at.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use crate::schema::{Field, Schema, Type};

/// The index of the root object of a tree.
const ROOT: u32 = 2;
/// The most objects one tree may hold.
pub(crate) const MAX_OBJECTS: usize = 200_000;
/// The longest counted list.
pub(crate) const MAX_LIST: u32 = 1_000_000;
/// How deep objects by value may nest.
pub(crate) const MAX_DEPTH: usize = 32;
/// The most values one tree may take, which bounds the time a read takes whatever the counts say.
pub(crate) const MAX_VALUES: usize = 20_000_000;

/// A value as the file writes it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    Int(u64),
    Real(f64),
    List(Vec<Value>),
    /// An object by value: its class's tag and its fields' values, its bases' first.
    Object(u16, Vec<Value>),
    /// An owned object written in its turn: its index and its class's tag.
    Owned(u32, u16),
    /// An owned object written inline: its place in its owner's `inline`.
    Inline(usize),
    Weak(u32),
    Null,
}

impl Value {
    pub(crate) fn real(&self) -> Option<f64> {
        match self {
            Value::Real(v) if v.is_finite() => Some(*v),
            _ => None,
        }
    }

    pub(crate) fn list(&self) -> Option<&[Value]> {
        match self {
            Value::List(l) => Some(l),
            _ => None,
        }
    }

    /// Three finite numbers.
    pub(crate) fn point(&self) -> Option<[f64; 3]> {
        match self.list()? {
            [x, y, z] => Some([x.real()?, y.real()?, z.real()?]),
            _ => None,
        }
    }

    /// Two finite numbers.
    pub(crate) fn pair(&self) -> Option<[f64; 2]> {
        match self.list()? {
            [x, y] => Some([x.real()?, y.real()?]),
            _ => None,
        }
    }

    pub(crate) fn weak(&self) -> Option<u32> {
        match self {
            Value::Weak(i) => Some(*i),
            _ => None,
        }
    }
}

/// An object of a tree: its class's tag, its fields' values (its bases' first) and the objects it owns inline.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Obj {
    pub tag: u16,
    pub values: Vec<Value>,
    pub inline: Vec<Obj>,
}

/// Every class's fields, its bases' first, and the tags of the classes a tree is read by.
pub(crate) struct Layouts<'s> {
    schema: &'s Schema,
    fields: HashMap<u16, Vec<&'s Field>>,
    /// The class of a tree's root.
    pub element: u16,
}

impl<'s> Layouts<'s> {
    /// The layouts of a schema that has the class `GElement`.
    pub(crate) fn of(schema: &'s Schema) -> Option<Layouts<'s>> {
        let element = schema.tag("GElement")?;
        let mut fields = HashMap::new();
        for class in &schema.classes {
            let mut chain = vec![class];
            while let Some(base) = chain.last().and_then(|c| c.base).and_then(|b| schema.by_tag(b)) {
                if chain.len() > 64 || chain.iter().any(|c| c.tag == base.tag) {
                    break;
                }
                chain.push(base);
            }
            let flat: Vec<&Field> = chain.iter().rev().flat_map(|c| c.fields.iter()).collect();
            fields.insert(class.tag, flat);
        }
        Some(Layouts { schema, fields, element })
    }

    /// The name of a class.
    pub(crate) fn name(&self, tag: u16) -> &str {
        self.schema.by_tag(tag).map_or("", |c| c.name.as_str())
    }

    /// The value of the field `name` of an object of class `tag` whose values are `values`.
    pub(crate) fn field<'v>(&self, tag: u16, values: &'v [Value], name: &str) -> Option<&'v Value> {
        let at = self.fields.get(&tag)?.iter().position(|f| f.name == name)?;
        values.get(at)
    }

    /// The value of an object's field `name`.
    pub(crate) fn get<'v>(&self, o: &'v Obj, name: &str) -> Option<&'v Value> {
        self.field(o.tag, &o.values, name)
    }
}

type Step<T> = std::result::Result<T, String>;

struct Decoder<'b, 'l, 's> {
    bytes: &'b [u8],
    at: usize,
    values: usize,
    layouts: &'l Layouts<'s>,
    /// The class of each index named so far.
    kinds: BTreeMap<u32, u16>,
    /// What the object being read owns, in turn and inline.
    children: Vec<u32>,
    inline: Vec<u16>,
}

impl Decoder<'_, '_, '_> {
    fn take<const N: usize>(&mut self) -> Step<[u8; N]> {
        let end = self.at.checked_add(N).filter(|e| *e <= self.bytes.len());
        let bytes = end.and_then(|e| self.bytes.get(self.at..e)).ok_or_else(|| format!("the stream ends at {:#x}", self.at))?;
        let mut out = [0; N];
        out.copy_from_slice(bytes);
        self.at += N;
        Ok(out)
    }

    fn u16(&mut self) -> Step<u16> {
        Ok(u16::from_le_bytes(self.take()?))
    }

    fn u32(&mut self) -> Step<u32> {
        Ok(u32::from_le_bytes(self.take()?))
    }

    fn many(&mut self, n: u32, t: &Type, modifier: u8, class: Option<u16>, depth: usize) -> Step<Value> {
        if n > MAX_LIST {
            return Err(format!("a list of {n} at {:#x}", self.at));
        }
        let mut out = Vec::new();
        for _ in 0..n {
            out.push(self.value(t, modifier, class, depth)?);
        }
        Ok(Value::List(out))
    }

    /// A value of type `t` written with `modifier` (`t`'s own, or its low nibble for each item of a list), `class` the class it
    /// names.
    fn value(&mut self, t: &Type, modifier: u8, class: Option<u16>, depth: usize) -> Step<Value> {
        self.values += 1;
        if self.values > MAX_VALUES {
            return Err(format!("more than {MAX_VALUES} values"));
        }
        if depth > MAX_DEPTH {
            return Err(format!("objects by value nest deeper than {MAX_DEPTH} at {:#x}", self.at));
        }
        let at = self.at;
        match modifier & 0xf0 {
            0x10 => {
                let n = t.count.ok_or_else(|| format!("an array with no count at {at:#x}"))?;
                return self.many(n, t, modifier & 0x0f, class, depth);
            }
            0x50 | 0x60 => {
                let n = self.u32()?;
                if t.kind == 0x0d {
                    let inner = t.inner.as_deref().ok_or_else(|| format!("an array with no inner type at {at:#x}"))?;
                    return self.many(n, inner, inner.modifier, class.or(inner.class), depth);
                }
                return self.many(n, t, modifier & 0x0f, class, depth);
            }
            _ => {}
        }
        Ok(match t.kind {
            1 | 2 => Value::Int(u64::from(self.take::<1>()?[0])),
            4 | 5 => Value::Int(u64::from(self.u32()?)),
            6 | 7 => Value::Real(f64::from_le_bytes(self.take()?)),
            11 => Value::Int(u64::from_le_bytes(self.take()?)),
            8 => Value::Int(u64::from(self.u16()?)),
            0x0d => {
                let inner = t.inner.as_deref().ok_or_else(|| format!("an array with no inner type at {at:#x}"))?;
                self.value(inner, inner.modifier, class.or(inner.class), depth)?
            }
            0x0e => match modifier & 0x0f {
                0 => {
                    let class = class.ok_or_else(|| format!("an object by value of no class at {at:#x}"))?;
                    Value::Object(class, self.object(class, depth + 1)?)
                }
                1 => self.owned()?,
                3 => Value::Weak(self.u32()?),
                m => return Err(format!("an object of modifier {m} at {at:#x}")),
            },
            k => return Err(format!("a field of kind {k} at {at:#x}")),
        })
    }

    fn owned(&mut self) -> Step<Value> {
        let index = self.u32()?;
        if index == 0 {
            return Ok(Value::Null);
        }
        let at = self.at;
        let tag = self.u16()?;
        if !self.layouts.fields.contains_key(&tag) {
            return Err(format!("an object of class {tag}, which the schema does not have, at {at:#x}"));
        }
        if index == u32::MAX {
            self.inline.push(tag);
            return Ok(Value::Inline(self.inline.len() - 1));
        }
        if self.kinds.insert(index, tag).is_some() {
            return Err(format!("object {index} is owned twice, at {at:#x}"));
        }
        self.children.push(index);
        Ok(Value::Owned(index, tag))
    }

    /// The fields of an object of class `tag`.
    fn object(&mut self, tag: u16, depth: usize) -> Step<Vec<Value>> {
        let layouts = self.layouts;
        let fields = layouts.fields.get(&tag).ok_or_else(|| format!("an object of class {tag}, which the schema does not have"))?;
        let mut out = Vec::with_capacity(fields.len());
        for f in fields {
            out.push(self.value(&f.ty, f.ty.modifier, f.ty.class, depth)?);
        }
        Ok(out)
    }
}

/// A tree read: its objects by index, the root's at index 2.
pub(crate) struct Tree {
    pub objects: BTreeMap<u32, Obj>,
}

impl Tree {
    pub(crate) fn get(&self, index: u32) -> Option<&Obj> {
        self.objects.get(&index)
    }
}

/// Reads the tree whose root object starts at `at` (right after the tag of the class `GElement`).
pub(crate) fn read(bytes: &[u8], at: usize, layouts: &Layouts) -> Step<Tree> {
    let mut d = Decoder {
        bytes,
        at,
        values: 0,
        layouts,
        kinds: BTreeMap::from([(ROOT, layouts.element)]),
        children: Vec::new(),
        inline: Vec::new(),
    };
    let mut queue = VecDeque::from([ROOT]);
    let mut objects: BTreeMap<u32, Obj> = BTreeMap::new();
    // The inline objects to read after the object of an index, and whose they are.
    let mut pending: BTreeMap<u32, Vec<(u32, u16)>> = BTreeMap::new();
    while let Some(i) = queue.pop_front() {
        if objects.len() >= MAX_OBJECTS {
            return Err(format!("more than {MAX_OBJECTS} objects"));
        }
        let tag = d.kinds.get(&i).copied().ok_or_else(|| format!("object {i} has no class"))?;
        d.children.clear();
        d.inline.clear();
        let values = d.object(tag, 0)?;
        let mut kids = std::mem::take(&mut d.children);
        let inline = std::mem::take(&mut d.inline);
        if !inline.is_empty() {
            let after = kids.last().copied().unwrap_or(i);
            pending.entry(after).or_default().extend(inline.into_iter().map(|t| (i, t)));
        }
        objects.insert(i, Obj { tag, values, inline: Vec::new() });
        for (owner, t) in pending.remove(&i).unwrap_or_default() {
            d.children.clear();
            d.inline.clear();
            let values = d.object(t, 0)?;
            if !d.inline.is_empty() {
                return Err(format!("an inline object of class {t} holds one of its own, at {:#x}", d.at));
            }
            kids.append(&mut d.children);
            if let Some(o) = objects.get_mut(&owner) {
                o.inline.push(Obj { tag: t, values, inline: Vec::new() });
            }
        }
        queue.extend(kids);
    }
    if !pending.is_empty() {
        return Err("an inline object comes after an object the tree does not hold".to_string());
    }
    Ok(Tree { objects })
}

/// The id inside an `ElementId` (an object holding an object holding the number).
fn element_id(v: &Value) -> Option<u64> {
    match v {
        Value::Int(id) => Some(*id),
        Value::Object(_, values) => match values.as_slice() {
            [one] => element_id(one),
            _ => None,
        },
        _ => None,
    }
}

/// What a family instance's own tree draws: its root holds one `GInstance` and nothing else, whose `InstanceInfo` names the
/// symbol drawn (`m_symbolId`: the instance's type, or a symbol Revit made for the values of its instance parameters) and the
/// transform it is drawn by (`m_Trf`: the matrix `m_3x3`, whose columns are the instance's axes, and the origin `m_or`).
pub(crate) fn instance(tree: &Tree, layouts: &Layouts) -> Step<(u32, crate::placement::Transform)> {
    let root = tree.objects.get(&ROOT).ok_or("the tree has no root")?;
    let nodes = layouts.get(root, "m_subNodes").and_then(Value::list).ok_or("its root holds no list of nodes")?;
    let node = match nodes {
        [Value::Owned(i, _)] => tree.get(*i).ok_or("its node is not in the tree")?,
        _ => return Err(format!("its root holds {} nodes, not one instance", nodes.len())),
    };
    if layouts.name(node.tag) != "GInstance" {
        return Err(format!("its root holds a {}, not an instance", layouts.name(node.tag)));
    }
    let info = match layouts.get(node, "m_instanceInfo") {
        Some(Value::Inline(k)) => node.inline.get(*k),
        Some(Value::Owned(i, _)) => tree.get(*i),
        _ => None,
    }
    .ok_or("its instance names no instance info")?;
    let symbol =
        layouts.get(info, "m_symbolId").and_then(element_id).and_then(|id| u32::try_from(id).ok()).ok_or("its instance names no symbol")?;
    let Some(Value::Object(trf, values)) = layouts.get(info, "m_Trf") else { return Err("its instance has no transform".to_string()) };
    let rows = layouts.field(*trf, values, "m_3x3").and_then(Value::list).ok_or("its transform has no matrix")?;
    let origin = layouts.field(*trf, values, "m_or").and_then(Value::point).ok_or("its transform has no origin")?;
    let m = match rows {
        [a, b, c] => [a.point(), b.point(), c.point()],
        _ => return Err("its transform's matrix is not three rows".to_string()),
    };
    let [Some(a), Some(b), Some(c)] = m else { return Err("its transform's matrix is not three rows of numbers".to_string()) };
    let axes = [[a[0], b[0], c[0]], [a[1], b[1], c[1]], [a[2], b[2], c[2]]];
    Ok((symbol, crate::placement::Transform { axes, origin }))
}

/// Where the trees of the family types `ids` start in `bytes`: each type's id and the byte after the tag of `GElement`, where the
/// type's id is the `u64` sixteen bytes before the tag and the `m_tag` of the root's `GInfo` (the `u32` eight bytes into it).
pub(crate) fn roots(bytes: &[u8], layouts: &Layouts, ids: &BTreeSet<u32>) -> Vec<(u32, usize)> {
    let tag = layouts.element.to_le_bytes();
    let u32_at = |at: usize| bytes.get(at..at.checked_add(4)?).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes);
    let u64_at = |at: usize| bytes.get(at..at.checked_add(8)?).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes);
    let mut out = Vec::new();
    for p in memchr::memmem::find_iter(bytes, &tag) {
        let (Some(before), Some(id)) = (p.checked_sub(16).and_then(u64_at), u32_at(p + 10)) else { continue };
        if before == u64::from(id) && ids.contains(&id) {
            out.push((id, p + 2));
        }
    }
    out
}
