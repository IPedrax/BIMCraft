// Portions of this file are ported from rvt-rs (https://github.com/DrunkOnJava/rvt-rs), Apache-2.0,
// Copyright 2026 Griffin Long, at commit b6086d41b9758fd9b92f42f0a47b8345b9c58056: src/formats.rs (`schema_classes` and
// its `GrammarReader`: the record grammar of `Formats/Latest`, which rvt-rs credits to STE1200, Discussion #112).
// Changed: rewritten for BIMCraft; the class list is kept with each field's name and type (kind, modifier, count, inner type,
// class), every read is bounds-checked, and nesting is capped by `MAX_NESTING`.
//! `Formats/Latest`: the schema Revit embeds in every file.
//!
//! The stream lists every class Revit serializes, with its fields. A class is known by its **tag**, its place in the list
//! (12 for the first class, one more for each definition after it, nested ones included), and a record in a partition
//! names its class by that tag. The tags change from one release to the next, so a reader finds a class by name in the
//! file's own schema and never assumes a number.
//!
//! The grammar (rvt-rs, from STE1200's measurements across 2014 to 2026 schemas, and checked here on four samples):
//! records are separated by a zero `u16`; a class is a counted name (`u16`), a base reference (`u16`, bit 15 set when the
//! base is defined inline right after a zero `u16`), a `u32` version, a `u32` field count, the fields (a `u32`-counted name
//! and a type), and a `u32`-counted list of 16-byte GUIDs. A type is a `u32` kind word (a kind byte and a modifier byte):
//! modifier high nibble `0x10` adds a `u32` array count, kind `0x0d` adds `01 00 00 00 20` and an inner type and, when that
//! has one, a class reference, and kind `0x0e` by value adds a class reference. A class reference is a `u16` with bit 15 set
//! when the class is defined inline. The tag is not stored; an inline reference repeats it, which the reader checks, so a
//! misread stops the parse instead of shifting every later tag.

const FIRST_TAG: u16 = 12;
const MAX_NESTING: usize = 64;
const MAX_FIELDS: u32 = 2_000;
const MAX_ARRAY: u32 = 65_536;
const MAX_GUIDS: usize = 60_000;

/// A field's type as the schema writes it: the kind byte (`1` and `2` a byte, `4` and `5` a `u32`, `6` and `7` an `f64`, `11`
/// a `u64`, `13` an array of `inner`, `14` an object), the modifier byte (high nibble `0x10` so many side by side, `0x50` and
/// `0x60` a `u32`-counted list; low nibble, for an object, `0` by value, `1` owned and `3` weak), a fixed array's count, an
/// array's inner type, and the class an object or an array of objects names (none for a pointer, whose class is written with it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Type {
    pub kind: u8,
    pub modifier: u8,
    pub count: Option<u32>,
    pub inner: Option<Box<Type>>,
    pub class: Option<u16>,
}

/// One field of a class: its name and type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub ty: Type,
}

/// One class of the schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Class {
    pub tag: u16,
    pub name: String,
    /// The tag of the class it derives from.
    pub base: Option<u16>,
    pub version: u32,
    pub field_count: u32,
    /// Its own fields, in the order an object of it is written (its base's come before them).
    pub fields: Vec<Field>,
}

/// Every class of a schema, by tag order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Schema {
    pub classes: Vec<Class>,
    /// Bytes read before the stream ended or a record broke the grammar.
    pub parsed_bytes: usize,
    /// Why reading stopped early; none when it reached the end.
    pub stopped: Option<String>,
}

impl Schema {
    /// The class with this tag.
    pub fn by_tag(&self, tag: u16) -> Option<&Class> {
        let first = self.classes.first()?.tag;
        let class = self.classes.get(usize::from(tag.checked_sub(first)?))?;
        (class.tag == tag).then_some(class)
    }

    /// The tag of the class with this name.
    pub fn tag(&self, name: &str) -> Option<u16> {
        self.classes.iter().find(|c| c.name == name).map(|c| c.tag)
    }

    /// Reads the inflated, page-stripped `Formats/Latest`. Classes before a record that breaks the grammar are kept, and
    /// `stopped` says why reading ended there.
    pub fn parse(data: &[u8]) -> Schema {
        let mut reader = Reader { data, next_tag: FIRST_TAG, classes: Vec::new() };
        let mut at = 0;
        let stopped = loop {
            if at + 2 > data.len() {
                break None;
            }
            if reader.u16(at) != Some(0) {
                break Some(format!("record separator is not 0 at {at:#x}"));
            }
            at += 2;
            if at >= data.len() || data[at..].iter().all(|&b| b == 0) {
                at = data.len();
                break None;
            }
            let (tag, count) = (reader.next_tag, reader.classes.len());
            match reader.class(at, 0) {
                Ok((_, next)) => at = next,
                Err(reason) => {
                    reader.classes.truncate(count);
                    reader.next_tag = tag;
                    break Some(reason);
                }
            }
        };
        reader.classes.sort_by_key(|class| class.tag);
        Schema { classes: reader.classes, parsed_bytes: at, stopped }
    }
}

type Step<T> = std::result::Result<T, String>;

impl Type {
    /// Whether the schema writes a class reference after it: an object by value (or a list or array of them).
    fn names_class(&self) -> bool {
        self.kind == 0x0e && self.modifier & 0x0f == 0
    }
}

struct Reader<'a> {
    data: &'a [u8],
    next_tag: u16,
    classes: Vec<Class>,
}

impl Reader<'_> {
    fn u16(&self, at: usize) -> Option<u16> {
        let b = self.data.get(at..at.checked_add(2)?)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&self, at: usize) -> Option<u32> {
        let b = self.data.get(at..at.checked_add(4)?)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn need<T>(value: Option<T>, what: &str, at: usize) -> Step<T> {
        value.ok_or_else(|| format!("{what} past the end at {at:#x}"))
    }

    /// A counted ASCII name whose count is `width` (2 or 4) bytes; returns it and where it ends.
    fn name(&self, at: usize, width: usize) -> Step<(String, usize)> {
        let len = if width == 2 {
            usize::from(Self::need(self.u16(at), "name length", at)?)
        } else {
            Self::need(self.u32(at), "name length", at)? as usize
        };
        let start = at + width;
        let raw = (1..=250)
            .contains(&len)
            .then(|| self.data.get(start..start + len))
            .flatten()
            .ok_or_else(|| format!("bad name length {len} at {at:#x}"))?;
        if !raw.iter().all(|b| (0x20..0x7f).contains(b)) {
            return Err(format!("name is not ASCII at {at:#x}"));
        }
        Ok((String::from_utf8_lossy(raw).into_owned(), start + len))
    }

    fn inline_class(&mut self, at: usize, reference: u16, depth: usize) -> Step<(u16, usize)> {
        if self.u16(at) != Some(0) {
            return Err(format!("inline pad is not 0 at {at:#x}"));
        }
        let (tag, next) = self.class(at + 2, depth + 1)?;
        if tag != reference & 0x7fff {
            return Err(format!("inline tag {tag} does not match reference {} at {at:#x}", reference & 0x7fff));
        }
        Ok((tag, next))
    }

    /// A class reference: the tag it names and where it ends.
    fn class_ref(&mut self, at: usize, depth: usize) -> Step<(u16, usize)> {
        let reference = Self::need(self.u16(at), "class reference", at)?;
        if reference & 0x8000 == 0 {
            return Ok((reference, at + 2));
        }
        self.inline_class(at + 2, reference, depth)
    }

    /// Reads a type; returns it and where it ends.
    fn field_type(&mut self, at: usize, depth: usize) -> Step<(Type, usize)> {
        if depth > MAX_NESTING {
            return Err(format!("types nest too deep at {at:#x}"));
        }
        let word = Self::need(self.u32(at), "type word", at)?;
        let mut at = at + 4;
        let (kind, modifier) = ((word & 0xff) as u8, ((word >> 8) & 0xff) as u8);
        if !matches!(modifier & 0xf0, 0x00 | 0x10 | 0x50 | 0x60) {
            return Err(format!("unknown modifier {modifier:#04x} at {:#x}", at - 4));
        }
        let mut ty = Type { kind, modifier, count: None, inner: None, class: None };
        if modifier & 0xf0 == 0x10 {
            let count = Self::need(self.u32(at), "array count", at)?;
            if count > MAX_ARRAY {
                return Err(format!("array count {count} at {at:#x}"));
            }
            ty.count = Some(count);
            at += 4;
        }
        if kind == 0x0d {
            if self.data.get(at..at + 5) != Some(&[1, 0, 0, 0, 0x20][..]) {
                return Err(format!("array filler at {at:#x}"));
            }
            let (inner, mut next) = self.field_type(at + 5, depth + 1)?;
            if inner.names_class() {
                let (class, end) = self.class_ref(next, depth)?;
                ty.class = Some(class);
                next = end;
            }
            ty.inner = Some(Box::new(inner));
            return Ok((ty, next));
        }
        if ty.names_class() {
            let (class, end) = self.class_ref(at, depth)?;
            ty.class = Some(class);
            at = end;
        }
        Ok((ty, at))
    }

    fn class(&mut self, at: usize, depth: usize) -> Step<(u16, usize)> {
        if depth > MAX_NESTING {
            return Err(format!("classes nest too deep at {at:#x}"));
        }
        let (name, mut p) = self.name(at, 2)?;
        let tag = self.next_tag;
        self.next_tag = tag.checked_add(1).filter(|next| *next < 0x8000).ok_or_else(|| format!("too many classes at {at:#x}"))?;
        let reference = Self::need(self.u16(p), "base reference", p)?;
        p += 2;
        let base = if reference & 0x8000 != 0 {
            let (base, next) = self.inline_class(p, reference, depth)?;
            p = next;
            Some(base)
        } else {
            (reference != 0).then_some(reference)
        };
        let version = Self::need(self.u32(p), "version", p)?;
        let field_count = Self::need(self.u32(p + 4), "field count", p + 4)?;
        p += 8;
        if field_count > MAX_FIELDS {
            return Err(format!("field count {field_count} at {:#x}", p - 4));
        }
        let mut fields = Vec::new();
        for _ in 0..field_count {
            let (name, next) = self.name(p, 4)?;
            let (ty, end) = self.field_type(next, depth)?;
            fields.push(Field { name, ty });
            p = end;
        }
        let guids = Self::need(self.u32(p), "GUID count", p)? as usize;
        p += 4;
        p = guids
            .checked_mul(16)
            .and_then(|len| p.checked_add(len))
            .filter(|end| *end <= self.data.len() && guids <= MAX_GUIDS)
            .ok_or_else(|| format!("GUID count {guids} at {:#x}", p - 4))?;
        self.classes.push(Class { tag, name, base, version, field_count, fields });
        Ok((tag, p))
    }
}

impl crate::Revit {
    /// Reads `Formats/Latest`. The schema may have stopped early ([`Schema::stopped`]); a stream with no class is an error.
    pub fn schema(&mut self) -> crate::Result<Schema> {
        let schema = Schema::parse(&self.inflate("Formats/Latest")?.bytes);
        if schema.classes.is_empty() {
            return Err(crate::Error::Format(format!(
                "Formats/Latest holds no class the reader can read ({})",
                schema.stopped.as_deref().unwrap_or("it is empty")
            )));
        }
        Ok(schema)
    }
}
