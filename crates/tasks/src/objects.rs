//! How the board reads its types from the JSON it holds: as a [`Value`] ([`read`]), every answer
//! of the store (`Store::read`, `Store::transact` and their kin decode through it), an attempt's
//! row, facts and artifacts, the fields the board keeps on a task, the dump's tables and the
//! project file's workflow tables; and as text it drops when it does not read ([`parse`]), the
//! files and lines the board keeps beside its database. The board and its database write each
//! struct of them as an object, so a struct is read from an object alone.
//!
//! `serde_json` reads a struct from an object or from an array of its fields in order, from a
//! value and from text alike, so each derived `Deserialize` it reads links two readers of the
//! struct's fields, the map's and the sequence's, and from text a third and fourth of each, as a
//! reader is compiled for each source it reads. Read through [`Objects`], a struct links its map
//! reader alone, once, as a struct variant does in both. A read whose refusal a caller is shown
//! (`tasks api`'s arguments, an assessment's submission, a file whose line and column are said)
//! is `serde_json`'s own, and keeps its words.
//!
//! What reads differently here, each as no writer of the board writes it:
//!
//! - an array where a struct belongs is refused as the wrong type, as any other value there is;
//! - a map's key is read as the string it is, where `serde_json` parses one for a key that is a
//!   number, a boolean or a derived newtype: such a map is refused, and the board reads none;
//! - text read by [`parse`] is a value first, so an object that says a field twice reads with
//!   the later one, where `serde_json` refuses a struct's duplicate field from text.
//!
//! Everything else reads, and is refused, as `serde_json` reads and refuses it from a value.

use serde::de::{
    self, DeserializeOwned, DeserializeSeed, Deserializer, EnumAccess, IntoDeserializer, MapAccess,
    SeqAccess, Unexpected, VariantAccess, Visitor,
};
use serde_json::{Error, Map, Value};

/// `value` read as `T`, each struct of it from an object.
pub(crate) fn read<T: DeserializeOwned>(value: &Value) -> Result<T, Error> {
    T::deserialize(Objects(value))
}

/// `text` read as `T`, each struct of it from an object, or none where it is not JSON or not a
/// `T`: for what the board drops when it cannot read it, as it says nothing of why.
pub(crate) fn parse<T: DeserializeOwned>(text: impl AsRef<[u8]>) -> Option<T> {
    read(&value(text.as_ref())?).ok()
}

/// `text` as the JSON value it is.
fn value(text: &[u8]) -> Option<Value> {
    serde_json::from_slice(text).ok()
}

/// A JSON value read with each struct from an object alone.
struct Objects<'a>(&'a Value);

/// What `value` is, said in a refusal.
fn unexpected(value: &Value) -> Unexpected<'_> {
    match value {
        Value::Null => Unexpected::Unit,
        Value::Bool(b) => Unexpected::Bool(*b),
        // As `serde_json` says it under `arbitrary_precision`, which `symphony-plugin` turns on
        // for every build of the board.
        Value::Number(_) => Unexpected::Other("number"),
        Value::String(s) => Unexpected::Str(s),
        Value::Array(_) => Unexpected::Seq,
        Value::Object(_) => Unexpected::Map,
    }
}

/// `value` refused where `expected` belongs.
fn refused<T>(value: &Value, expected: &dyn de::Expected) -> Result<T, Error> {
    Err(de::Error::invalid_type(unexpected(value), expected))
}

/// Each method read from the value as `serde_json` reads it.
macro_rules! as_json {
    ($($method:ident)*) => {
        $(
            fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
                self.0.$method(visitor)
            }
        )*
    };
}

impl<'de> Deserializer<'de> for Objects<'de> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Value::Array(_) => self.deserialize_seq(visitor),
            Value::Object(_) => self.deserialize_map(visitor),
            other => other.deserialize_any(visitor),
        }
    }

    as_json! {
        deserialize_bool deserialize_i8 deserialize_i16 deserialize_i32 deserialize_i64
        deserialize_i128 deserialize_u8 deserialize_u16 deserialize_u32 deserialize_u64
        deserialize_u128 deserialize_f32 deserialize_f64 deserialize_char deserialize_str
        deserialize_string deserialize_bytes deserialize_byte_buf deserialize_unit
        deserialize_identifier
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Value::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.0.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let Value::Array(items) = self.0 else {
            return refused(self.0, &visitor);
        };
        let mut seq = Items(items.iter());
        let read = visitor.visit_seq(&mut seq)?;
        match seq.0.len() {
            0 => Ok(read),
            _ => Err(de::Error::invalid_length(
                items.len(),
                &"fewer elements in array",
            )),
        }
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, _: usize, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: usize,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let Value::Object(map) = self.0 else {
            return refused(self.0, &visitor);
        };
        let mut entries = Entries::of(map);
        let read = visitor.visit_map(&mut entries)?;
        match entries.entries.len() {
            0 => Ok(read),
            _ => Err(de::Error::invalid_length(
                map.len(),
                &"fewer elements in map",
            )),
        }
    }

    /// A struct from an object alone: never its fields in an array's order.
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self.0 {
            Value::String(variant) => visitor.visit_enum(variant.as_str().into_deserializer()),
            Value::Object(map) => {
                let mut entries = map.iter();
                match (entries.next(), entries.next()) {
                    (Some((variant, value)), None) => {
                        visitor.visit_enum(Variant { variant, value })
                    }
                    _ => Err(de::Error::invalid_value(
                        Unexpected::Map,
                        &"map with a single key",
                    )),
                }
            }
            other => refused(other, &"string or map"),
        }
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_unit()
    }
}

/// An array's elements, each read through [`Objects`].
struct Items<'a>(std::slice::Iter<'a, Value>);

impl<'de> SeqAccess<'de> for Items<'de> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        self.0
            .next()
            .map(|item| seed.deserialize(Objects(item)))
            .transpose()
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.0.len())
    }
}

/// An object's entries, each value read through [`Objects`].
struct Entries<'a> {
    entries: serde_json::map::Iter<'a>,
    /// The value of the key read last.
    value: Option<&'a Value>,
}

impl<'a> Entries<'a> {
    fn of(map: &'a Map<String, Value>) -> Entries<'a> {
        Entries {
            entries: map.iter(),
            value: None,
        }
    }
}

impl<'de> MapAccess<'de> for Entries<'de> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        let Some((key, value)) = self.entries.next() else {
            return Ok(None);
        };
        self.value = Some(value);
        seed.deserialize(key.as_str().into_deserializer()).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        match self.value.take() {
            Some(value) => seed.deserialize(Objects(value)),
            None => Err(de::Error::custom("value is missing")),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.entries.len())
    }
}

/// An externally tagged variant, `{"variant": value}`.
struct Variant<'a> {
    variant: &'a str,
    value: &'a Value,
}

impl<'de> EnumAccess<'de> for Variant<'de> {
    type Error = Error;
    type Variant = Objects<'de>;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Objects<'de>), Error> {
        let variant = seed.deserialize(self.variant.into_deserializer())?;
        Ok((variant, Objects(self.value)))
    }
}

/// A variant's value, of `{"variant": value}`.
impl<'de> VariantAccess<'de> for Objects<'de> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        match self.0 {
            Value::Null => Ok(()),
            other => refused(other, &"unit"),
        }
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Error> {
        seed.deserialize(self)
    }

    fn tuple_variant<V: Visitor<'de>>(self, _: usize, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Value::Array(items) if items.is_empty() => visitor.visit_unit(),
            Value::Array(_) => self.deserialize_seq(visitor),
            other => refused(other, &"tuple variant"),
        }
    }

    /// A struct variant from an object alone, as a struct.
    fn struct_variant<V: Visitor<'de>>(
        self,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self.0 {
            Value::Object(_) => self.deserialize_map(visitor),
            other => refused(other, &"struct variant"),
        }
    }
}

#[cfg(test)]
#[path = "objects_tests.rs"]
mod tests;
