//! What a project file holds that this BIMCraft doesn't know, found so that it can be left out.
//!
//! The model's types refuse fields they don't know (`deny_unknown_fields`), which is what catches a misspelled key in a
//! command's params. A file is read more kindly: a field a newer BIMCraft added (without changing the file's format) is
//! left out and named, and the rest of the project opens. [`unknown`] walks a JSON value as `T` would read it, and lists
//! every key a struct on the way doesn't have; the caller takes those out and reads the value as strictly as ever.
//! It sees into a struct wherever serde asks for one by its fields: not inside an internally tagged or untagged enum,
//! which serde reads as a whole, so [`Element`](crate::model::Element) is looked into by its category (`element`).

use serde::de::{self, DeserializeSeed, Deserializer, EnumAccess, IntoDeserializer, MapAccess, SeqAccess, VariantAccess, Visitor};
use serde_json::{Map, Value};
use std::cell::RefCell;

type Error = serde_json::Error;

/// The keys of `v` that no struct of `T` has, as JSON pointers (`/types/walls/Generic/finish`), in the order read.
pub fn unknown<'de, T: de::Deserialize<'de>>(v: &Value) -> Vec<String> {
    let found = RefCell::new(Vec::new());
    // What else is wrong with it is for the strict reading to say.
    let _ = T::deserialize(Walk { v: v.clone(), at: String::new(), found: &found });
    found.into_inner()
}

/// The keys of an element's JSON (its `category`, and `guid`, taken out) that its kind doesn't have; none for a category
/// this BIMCraft doesn't know, which the strict reading refuses.
pub fn element(category: &str, v: &Value) -> Vec<String> {
    use crate::model::*;
    macro_rules! kinds {
        ($($name:literal => $t:ty),* $(,)?) => {
            match category {
                $($name => unknown::<$t>(v),)*
                _ => Vec::new(),
            }
        };
    }
    kinds! {
        "level" => Level, "grid" => Grid, "wall" => Wall, "floor" => Floor, "ceiling" => Ceiling, "shaft" => Shaft,
        "mass" => Mass, "door" => Opening, "window" => Opening, "room" => Room, "dimension" => Dimension, "tag" => Tag,
        "section" => Section, "elevation" => Section, "sheet" => Sheet, "viewport" => Viewport, "column" => Column,
        "structuralcolumn" => Column, "footing" => Column, "beam" => Run, "stripfooting" => Run, "pipe" => Run,
        "conduit" => Run, "cabletray" => Run, "wire" => Run, "downconductor" => Run, "groundconductor" => Run,
        "cable" => Run, "component" => Component, "stair" => Stair, "ramp" => Ramp, "railing" => Railing, "roof" => Roof,
        "text" => Note, "line" => DetailLine, "region" => FilledRegion, "spot" => Spot, "schedule" => Schedule, "view3d" => View3D,
        "planview" => PlanView, "link" => Link,
    }
}

/// Takes out of `v` the key a pointer from [`unknown`] names.
pub fn remove(v: &mut Value, pointer: &str) {
    let Some((parent, key)) = pointer.rsplit_once('/') else { return };
    let key = key.replace("~1", "/").replace("~0", "~");
    if let Some(Value::Object(o)) = v.pointer_mut(parent) {
        o.remove(&key);
    }
}

fn step(at: &str, key: &str) -> String {
    format!("{at}/{}", key.replace('~', "~0").replace('/', "~1"))
}

/// A value read as the type asks, noting the keys its structs don't have.
struct Walk<'a> {
    v: Value,
    at: String,
    found: &'a RefCell<Vec<String>>,
}

impl<'a> Walk<'a> {
    fn map(self, o: Map<String, Value>, fields: Option<&'static [&'static str]>) -> Entries<'a> {
        let mut items = Vec::new();
        for (k, v) in o {
            let at = step(&self.at, &k);
            match fields {
                Some(f) if !f.contains(&k.as_str()) => self.found.borrow_mut().push(at),
                _ => items.push((k, Walk { v, at, found: self.found })),
            }
        }
        items.reverse();
        Entries { items, value: None }
    }

    fn seq(self, a: Vec<Value>) -> Items<'a> {
        let at = self.at;
        let found = self.found;
        let mut items: Vec<Walk<'a>> = a.into_iter().enumerate().map(|(i, v)| Walk { v, at: format!("{at}/{i}"), found }).collect();
        items.reverse();
        Items { items }
    }
}

macro_rules! plain {
    ($($f:ident)*) => {
        $(fn $f<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
            self.v.$f(visitor)
        })*
    };
}

impl<'de> Deserializer<'de> for Walk<'_> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match &self.v {
            Value::Object(_) => self.deserialize_map(visitor),
            Value::Array(_) => self.deserialize_seq(visitor),
            _ => self.v.deserialize_any(visitor),
        }
    }

    plain!(deserialize_bool deserialize_i8 deserialize_i16 deserialize_i32 deserialize_i64 deserialize_i128
        deserialize_u8 deserialize_u16 deserialize_u32 deserialize_u64 deserialize_u128 deserialize_f32 deserialize_f64
        deserialize_char deserialize_str deserialize_string deserialize_bytes deserialize_byte_buf deserialize_unit
        deserialize_identifier deserialize_ignored_any);

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.v {
            Value::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(self, name: &'static str, visitor: V) -> Result<V::Value, Error> {
        self.v.deserialize_unit_struct(name, visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(self, _: &'static str, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: Visitor<'de>>(mut self, visitor: V) -> Result<V::Value, Error> {
        match std::mem::take(&mut self.v) {
            Value::Array(a) => visitor.visit_seq(self.seq(a)),
            v => v.deserialize_seq(visitor),
        }
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, _: usize, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(self, _: &'static str, _: usize, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(mut self, visitor: V) -> Result<V::Value, Error> {
        match std::mem::take(&mut self.v) {
            Value::Object(o) => visitor.visit_map(self.map(o, None)),
            v => v.deserialize_map(visitor),
        }
    }

    fn deserialize_struct<V: Visitor<'de>>(
        mut self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match std::mem::take(&mut self.v) {
            Value::Object(o) => visitor.visit_map(self.map(o, Some(fields))),
            v => v.deserialize_struct(name, fields, visitor),
        }
    }

    fn deserialize_enum<V: Visitor<'de>>(
        mut self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match std::mem::take(&mut self.v) {
            Value::Object(o) if o.len() == 1 => {
                let Some((k, v)) = o.into_iter().next() else { return Err(de::Error::custom("an empty enum")) };
                let at = step(&self.at, &k);
                visitor.visit_enum(Variant { name: k, walk: Walk { v, at, found: self.found } })
            }
            v => v.deserialize_enum(name, variants, visitor),
        }
    }
}

struct Entries<'a> {
    items: Vec<(String, Walk<'a>)>,
    value: Option<Walk<'a>>,
}

impl<'de> MapAccess<'de> for Entries<'_> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, Error> {
        match self.items.pop() {
            Some((k, w)) => {
                self.value = Some(w);
                seed.deserialize(Key(k)).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<S: DeserializeSeed<'de>>(&mut self, seed: S) -> Result<S::Value, Error> {
        match self.value.take() {
            Some(w) => seed.deserialize(w),
            None => Err(de::Error::custom("a value with no key")),
        }
    }
}

/// A map's key, which JSON writes as a string whatever it is: a number reads back from it as `serde_json` reads one.
struct Key(String);

macro_rules! number {
    ($($f:ident $v:ident $t:ty)*) => {
        $(fn $f<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
            match self.0.parse::<$t>() {
                Ok(n) => visitor.$v(n),
                Err(_) => visitor.visit_string(self.0),
            }
        })*
    };
}

impl<'de> Deserializer<'de> for Key {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_string(self.0)
    }

    number!(deserialize_i8 visit_i64 i64 deserialize_i16 visit_i64 i64 deserialize_i32 visit_i64 i64
        deserialize_i64 visit_i64 i64 deserialize_u8 visit_u64 u64 deserialize_u16 visit_u64 u64
        deserialize_u32 visit_u64 u64 deserialize_u64 visit_u64 u64 deserialize_f32 visit_f64 f64
        deserialize_f64 visit_f64 f64);

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        Value::String(self.0).deserialize_enum(name, variants, visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(self, _: &'static str, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    serde::forward_to_deserialize_any! {
        bool i128 u128 char str string bytes byte_buf option unit unit_struct seq tuple tuple_struct map struct
        identifier ignored_any
    }
}

struct Items<'a> {
    items: Vec<Walk<'a>>,
}

impl<'de> SeqAccess<'de> for Items<'_> {
    type Error = Error;

    fn next_element_seed<S: DeserializeSeed<'de>>(&mut self, seed: S) -> Result<Option<S::Value>, Error> {
        self.items.pop().map(|w| seed.deserialize(w)).transpose()
    }
}

struct Variant<'a> {
    name: String,
    walk: Walk<'a>,
}

impl<'de, 'a> EnumAccess<'de> for Variant<'a> {
    type Error = Error;
    type Variant = Walk<'a>;

    fn variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<(S::Value, Walk<'a>), Error> {
        let name: de::value::StringDeserializer<Error> = self.name.into_deserializer();
        Ok((seed.deserialize(name)?, self.walk))
    }
}

impl<'de> VariantAccess<'de> for Walk<'_> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        de::Deserialize::deserialize(self.v)
    }

    fn newtype_variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<S::Value, Error> {
        seed.deserialize(self)
    }

    fn tuple_variant<V: Visitor<'de>>(self, _: usize, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_seq(visitor)
    }

    fn struct_variant<V: Visitor<'de>>(self, fields: &'static [&'static str], visitor: V) -> Result<V::Value, Error> {
        self.deserialize_struct("", fields, visitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Element;
    use serde_json::json;

    #[test]
    fn every_category_of_element_is_looked_into() {
        // serde names every variant when it meets one it doesn't know.
        let e = serde_json::from_value::<Element>(json!({"category": "nothing"})).unwrap_err().to_string();
        let names: Vec<&str> = e.split('`').skip(3).step_by(2).collect();
        assert!(names.len() > 30, "{e}");
        for name in names {
            assert_eq!(element(name, &json!({"made_later": 1})), ["/made_later"], "{name}");
        }
        assert_eq!(element("hologram", &json!({"made_later": 1})), Vec::<String>::new());
    }

    #[test]
    fn keys_are_found_where_they_are_and_taken_out_there() {
        let mut v = json!({"a/b": {"c~d": 1}});
        remove(&mut v, &step(&step("", "a/b"), "c~d"));
        assert_eq!(v, json!({"a/b": {}}));
    }
}
