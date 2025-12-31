//! Proto JSON serialization support.
//!
//! This module provides `ProtoJsonSerializer` and `ProtoJsonDeserializer` that wrap
//! standard serde serializers/deserializers to apply proto JSON spec transformations:
//!
//! - Float NaN/Infinity as strings: `"NaN"`, `"Infinity"`, `"-Infinity"`
//! - Bytes as base64-encoded strings
//!
//! Well-known type handling (Timestamp, Duration, wrappers) remains in the base
//! `Serialize` impl using `is_human_readable()`.

use base64::Engine;
use serde::ser::{
    SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant,
};

/// A serde Serializer wrapper that applies proto JSON transformations.
///
/// Wraps any serde Serializer and transforms:
/// - `f32::NAN` / `f64::NAN` → `"NaN"`
/// - `f32::INFINITY` / `f64::INFINITY` → `"Infinity"`
/// - `f32::NEG_INFINITY` / `f64::NEG_INFINITY` → `"-Infinity"`
/// - bytes → base64-encoded string
pub struct ProtoJsonSerializer<S> {
    inner: S,
}

impl<S> ProtoJsonSerializer<S> {
    pub fn new(inner: S) -> Self {
        Self { inner }
    }

    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S: serde::Serializer> serde::Serializer for ProtoJsonSerializer<S> {
    type Ok = S::Ok;
    type Error = S::Error;
    type SerializeSeq = ProtoJsonSerializeSeq<S::SerializeSeq>;
    type SerializeTuple = ProtoJsonSerializeTuple<S::SerializeTuple>;
    type SerializeTupleStruct = ProtoJsonSerializeTupleStruct<S::SerializeTupleStruct>;
    type SerializeTupleVariant = ProtoJsonSerializeTupleVariant<S::SerializeTupleVariant>;
    type SerializeMap = ProtoJsonSerializeMap<S::SerializeMap>;
    type SerializeStruct = ProtoJsonSerializeStruct<S::SerializeStruct>;
    type SerializeStructVariant = ProtoJsonSerializeStructVariant<S::SerializeStructVariant>;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_bool(v)
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_i8(v)
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_i16(v)
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_i32(v)
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        // Proto JSON spec: int64 as string for JavaScript precision
        self.inner.serialize_str(&v.to_string())
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_u8(v)
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_u16(v)
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_u32(v)
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        // Proto JSON spec: uint64 as string for JavaScript precision
        self.inner.serialize_str(&v.to_string())
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        if v.is_nan() {
            self.inner.serialize_str("NaN")
        } else if v == f32::INFINITY {
            self.inner.serialize_str("Infinity")
        } else if v == f32::NEG_INFINITY {
            self.inner.serialize_str("-Infinity")
        } else {
            self.inner.serialize_f32(v)
        }
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        if v.is_nan() {
            self.inner.serialize_str("NaN")
        } else if v == f64::INFINITY {
            self.inner.serialize_str("Infinity")
        } else if v == f64::NEG_INFINITY {
            self.inner.serialize_str("-Infinity")
        } else {
            self.inner.serialize_f64(v)
        }
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_char(v)
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_str(v)
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        let encoded = base64::engine::general_purpose::STANDARD.encode(v);
        self.inner.serialize_str(&encoded)
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_none()
    }

    fn serialize_some<T: ?Sized + serde::Serialize>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_unit()
    }

    fn serialize_unit_struct(self, name: &'static str) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_unit_struct(name)
    }

    fn serialize_unit_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_unit_variant(name, variant_index, variant)
    }

    fn serialize_newtype_struct<T: ?Sized + serde::Serialize>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        // Serialize through our wrapper to apply transformations
        self.inner.serialize_newtype_struct(name, &ProtoJsonValue(value))
    }

    fn serialize_newtype_variant<T: ?Sized + serde::Serialize>(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_newtype_variant(name, variant_index, variant, &ProtoJsonValue(value))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(ProtoJsonSerializeSeq {
            inner: self.inner.serialize_seq(len)?,
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(ProtoJsonSerializeTuple {
            inner: self.inner.serialize_tuple(len)?,
        })
    }

    fn serialize_tuple_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Ok(ProtoJsonSerializeTupleStruct {
            inner: self.inner.serialize_tuple_struct(name, len)?,
        })
    }

    fn serialize_tuple_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Ok(ProtoJsonSerializeTupleVariant {
            inner: self.inner.serialize_tuple_variant(name, variant_index, variant, len)?,
        })
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(ProtoJsonSerializeMap {
            inner: self.inner.serialize_map(len)?,
        })
    }

    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(ProtoJsonSerializeStruct {
            inner: self.inner.serialize_struct(name, len)?,
        })
    }

    fn serialize_struct_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Ok(ProtoJsonSerializeStructVariant {
            inner: self.inner.serialize_struct_variant(name, variant_index, variant, len)?,
        })
    }

    fn is_human_readable(&self) -> bool {
        true // Proto JSON is always human-readable
    }
}

/// Wrapper to serialize a value through ProtoJsonSerializer
struct ProtoJsonValue<'a, T: ?Sized>(&'a T);

impl<T: serde::Serialize + ?Sized> serde::Serialize for ProtoJsonValue<'_, T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(ProtoJsonSerializer::new(serializer))
    }
}

// Wrapper types for compound serializers

pub struct ProtoJsonSerializeSeq<S> {
    inner: S,
}

impl<S: SerializeSeq> SerializeSeq for ProtoJsonSerializeSeq<S> {
    type Ok = S::Ok;
    type Error = S::Error;

    fn serialize_element<T: ?Sized + serde::Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        self.inner.serialize_element(&ProtoJsonValue(value))
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.inner.end()
    }
}

pub struct ProtoJsonSerializeTuple<S> {
    inner: S,
}

impl<S: SerializeTuple> SerializeTuple for ProtoJsonSerializeTuple<S> {
    type Ok = S::Ok;
    type Error = S::Error;

    fn serialize_element<T: ?Sized + serde::Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        self.inner.serialize_element(&ProtoJsonValue(value))
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.inner.end()
    }
}

pub struct ProtoJsonSerializeTupleStruct<S> {
    inner: S,
}

impl<S: SerializeTupleStruct> SerializeTupleStruct for ProtoJsonSerializeTupleStruct<S> {
    type Ok = S::Ok;
    type Error = S::Error;

    fn serialize_field<T: ?Sized + serde::Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        self.inner.serialize_field(&ProtoJsonValue(value))
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.inner.end()
    }
}

pub struct ProtoJsonSerializeTupleVariant<S> {
    inner: S,
}

impl<S: SerializeTupleVariant> SerializeTupleVariant for ProtoJsonSerializeTupleVariant<S> {
    type Ok = S::Ok;
    type Error = S::Error;

    fn serialize_field<T: ?Sized + serde::Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        self.inner.serialize_field(&ProtoJsonValue(value))
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.inner.end()
    }
}

pub struct ProtoJsonSerializeMap<S> {
    inner: S,
}

impl<S: SerializeMap> SerializeMap for ProtoJsonSerializeMap<S> {
    type Ok = S::Ok;
    type Error = S::Error;

    fn serialize_key<T: ?Sized + serde::Serialize>(&mut self, key: &T) -> Result<(), Self::Error> {
        self.inner.serialize_key(&ProtoJsonValue(key))
    }

    fn serialize_value<T: ?Sized + serde::Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        self.inner.serialize_value(&ProtoJsonValue(value))
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.inner.end()
    }
}

pub struct ProtoJsonSerializeStruct<S> {
    inner: S,
}

impl<S: SerializeStruct> SerializeStruct for ProtoJsonSerializeStruct<S> {
    type Ok = S::Ok;
    type Error = S::Error;

    fn serialize_field<T: ?Sized + serde::Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        self.inner.serialize_field(key, &ProtoJsonValue(value))
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.inner.end()
    }
}

pub struct ProtoJsonSerializeStructVariant<S> {
    inner: S,
}

impl<S: SerializeStructVariant> SerializeStructVariant for ProtoJsonSerializeStructVariant<S> {
    type Ok = S::Ok;
    type Error = S::Error;

    fn serialize_field<T: ?Sized + serde::Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        self.inner.serialize_field(key, &ProtoJsonValue(value))
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.inner.end()
    }
}

// ============================================================================
// ProtoJsonDeserializer
// ============================================================================

use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};

/// A serde Deserializer wrapper that handles proto JSON input quirks.
///
/// Wraps any serde Deserializer and transforms:
/// - `"NaN"`, `"Infinity"`, `"-Infinity"` strings → f32/f64
/// - String representations of numbers → integers
/// - Base64 strings → bytes
pub struct ProtoJsonDeserializer<D> {
    inner: D,
}

impl<D> ProtoJsonDeserializer<D> {
    pub fn new(inner: D) -> Self {
        Self { inner }
    }
}

impl<'de, D: serde::Deserializer<'de>> serde::Deserializer<'de> for ProtoJsonDeserializer<D> {
    type Error = D::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_any(ProtoJsonVisitor(visitor))
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_bool(visitor)
    }

    fn deserialize_i8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_any(FlexibleIntVisitor(visitor))
    }

    fn deserialize_i16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_any(FlexibleIntVisitor(visitor))
    }

    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_any(FlexibleIntVisitor(visitor))
    }

    fn deserialize_i64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_any(FlexibleIntVisitor(visitor))
    }

    fn deserialize_u8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_any(FlexibleIntVisitor(visitor))
    }

    fn deserialize_u16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_any(FlexibleIntVisitor(visitor))
    }

    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_any(FlexibleIntVisitor(visitor))
    }

    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_any(FlexibleIntVisitor(visitor))
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_any(FlexibleF32Visitor(visitor))
    }

    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_any(FlexibleFloatVisitor(visitor))
    }

    fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_char(visitor)
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_str(visitor)
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_string(visitor)
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_any(FlexibleBytesVisitor(visitor))
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_any(FlexibleBytesVisitor(visitor))
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_option(ProtoJsonOptionVisitor(visitor))
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_unit(visitor)
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_unit_struct(name, visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_newtype_struct(name, ProtoJsonVisitor(visitor))
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_seq(ProtoJsonSeqVisitor(visitor))
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_tuple(len, ProtoJsonSeqVisitor(visitor))
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_tuple_struct(name, len, ProtoJsonSeqVisitor(visitor))
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_map(ProtoJsonMapVisitor(visitor))
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_struct(name, fields, ProtoJsonMapVisitor(visitor))
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_enum(name, variants, ProtoJsonEnumVisitor(visitor))
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_identifier(visitor)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_ignored_any(visitor)
    }

    fn is_human_readable(&self) -> bool {
        true
    }
}

// Visitor wrappers

struct ProtoJsonVisitor<V>(V);

impl<'de, V: Visitor<'de>> Visitor<'de> for ProtoJsonVisitor<V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.0.expecting(formatter)
    }

    fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
        self.0.visit_bool(v)
    }

    fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
        self.0.visit_i64(v)
    }

    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
        self.0.visit_u64(v)
    }

    fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
        self.0.visit_f64(v)
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
        self.0.visit_str(v)
    }

    fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
        self.0.visit_string(v)
    }

    fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
        self.0.visit_bytes(v)
    }

    fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
        self.0.visit_byte_buf(v)
    }

    fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        self.0.visit_none()
    }

    fn visit_some<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        self.0.visit_some(ProtoJsonDeserializer::new(deserializer))
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        self.0.visit_unit()
    }

    fn visit_newtype_struct<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        self.0.visit_newtype_struct(ProtoJsonDeserializer::new(deserializer))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        self.0.visit_seq(ProtoJsonSeqAccess(seq))
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        self.0.visit_map(ProtoJsonMapAccess(map))
    }
}

struct FlexibleIntVisitor<V>(V);

impl<'de, V: Visitor<'de>> Visitor<'de> for FlexibleIntVisitor<V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.0.expecting(formatter)
    }

    fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
        self.0.visit_i64(v)
    }

    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
        self.0.visit_u64(v)
    }

    fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
        // JSON numbers can be floats, but must be whole numbers for int fields
        if v.fract() != 0.0 {
            return Err(E::custom("expected integer, got non-integer float"));
        }
        self.0.visit_i64(v as i64)
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
        // Try parsing as i64 first, then u64
        if let Ok(i) = v.parse::<i64>() {
            return self.0.visit_i64(i);
        }
        if let Ok(u) = v.parse::<u64>() {
            return self.0.visit_u64(u);
        }
        // Handle exponential notation like "1e2" - only if contains 'e' or 'E'
        // (plain overflows should fail, not silently truncate via f64)
        if v.contains('e') || v.contains('E') {
            if let Ok(f) = v.parse::<f64>() {
                if f.fract() != 0.0 {
                    return Err(E::custom(format!("'{}' is not a whole number", v)));
                }
                // Exponential values within f64's exact integer range are safe
                if f >= i64::MIN as f64 && f <= i64::MAX as f64 {
                    return self.0.visit_i64(f as i64);
                }
                if f >= 0.0 && f <= u64::MAX as f64 {
                    return self.0.visit_u64(f as u64);
                }
            }
        }
        Err(E::custom(format!("cannot parse '{}' as integer", v)))
    }
}

struct FlexibleFloatVisitor<V>(V);

impl<'de, V: Visitor<'de>> Visitor<'de> for FlexibleFloatVisitor<V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.0.expecting(formatter)
    }

    fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
        self.0.visit_f64(v)
    }

    fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
        self.0.visit_f64(v as f64)
    }

    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
        self.0.visit_f64(v as f64)
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
        match v {
            "NaN" => self.0.visit_f64(f64::NAN),
            "Infinity" => self.0.visit_f64(f64::INFINITY),
            "-Infinity" => self.0.visit_f64(f64::NEG_INFINITY),
            _ => match v.parse::<f64>() {
                Ok(f) => self.0.visit_f64(f),
                Err(_) => Err(E::custom(format!("cannot parse '{}' as float", v))),
            },
        }
    }
}

struct FlexibleF32Visitor<V>(V);

impl<'de, V: Visitor<'de>> Visitor<'de> for FlexibleF32Visitor<V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.0.expecting(formatter)
    }

    fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
        // Check f32 range for finite values
        if v.is_finite() && (v > f32::MAX as f64 || v < f32::MIN as f64) {
            return Err(E::custom("float value out of range for f32"));
        }
        self.0.visit_f32(v as f32)
    }

    fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
        self.0.visit_f32(v as f32)
    }

    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
        self.0.visit_f32(v as f32)
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
        match v {
            "NaN" => self.0.visit_f32(f32::NAN),
            "Infinity" => self.0.visit_f32(f32::INFINITY),
            "-Infinity" => self.0.visit_f32(f32::NEG_INFINITY),
            _ => match v.parse::<f64>() {
                Ok(f) => self.visit_f64(f),
                Err(_) => Err(E::custom(format!("cannot parse '{}' as float", v))),
            },
        }
    }
}

struct FlexibleBytesVisitor<V>(V);

impl<'de, V: Visitor<'de>> Visitor<'de> for FlexibleBytesVisitor<V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.0.expecting(formatter)
    }

    fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
        self.0.visit_bytes(v)
    }

    fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
        self.0.visit_byte_buf(v)
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
        use base64::Engine;
        use base64::engine::{GeneralPurpose, GeneralPurposeConfig, DecodePaddingMode};
        // Lenient base64 decoder: accepts standard or URL-safe, with or without padding
        let config = GeneralPurposeConfig::new()
            .with_decode_padding_mode(DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true);
        // Try standard alphabet first, then URL-safe
        let standard = GeneralPurpose::new(&base64::alphabet::STANDARD, config);
        if let Ok(bytes) = standard.decode(v) {
            return self.0.visit_byte_buf(bytes);
        }
        let url_safe = GeneralPurpose::new(&base64::alphabet::URL_SAFE, config);
        if let Ok(bytes) = url_safe.decode(v) {
            return self.0.visit_byte_buf(bytes);
        }
        Err(E::custom("invalid base64"))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        // Accept byte arrays
        self.0.visit_seq(seq)
    }
}

struct ProtoJsonOptionVisitor<V>(V);

impl<'de, V: Visitor<'de>> Visitor<'de> for ProtoJsonOptionVisitor<V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.0.expecting(formatter)
    }

    fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        self.0.visit_none()
    }

    fn visit_some<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        self.0.visit_some(ProtoJsonDeserializer::new(deserializer))
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        self.0.visit_none()
    }
}

struct ProtoJsonSeqVisitor<V>(V);

impl<'de, V: Visitor<'de>> Visitor<'de> for ProtoJsonSeqVisitor<V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.0.expecting(formatter)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        self.0.visit_seq(ProtoJsonSeqAccess(seq))
    }
}

struct ProtoJsonMapVisitor<V>(V);

impl<'de, V: Visitor<'de>> Visitor<'de> for ProtoJsonMapVisitor<V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.0.expecting(formatter)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        self.0.visit_map(ProtoJsonMapAccess(map))
    }
}

struct ProtoJsonEnumVisitor<V>(V);

impl<'de, V: Visitor<'de>> Visitor<'de> for ProtoJsonEnumVisitor<V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.0.expecting(formatter)
    }

    fn visit_enum<A: serde::de::EnumAccess<'de>>(self, data: A) -> Result<Self::Value, A::Error> {
        self.0.visit_enum(data)
    }
}

// SeqAccess and MapAccess wrappers

struct ProtoJsonSeqAccess<A>(A);

impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for ProtoJsonSeqAccess<A> {
    type Error = A::Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error> {
        self.0.next_element_seed(ProtoJsonDeserializeSeed(seed))
    }
}

struct ProtoJsonMapAccess<A>(A);

impl<'de, A: MapAccess<'de>> MapAccess<'de> for ProtoJsonMapAccess<A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error> {
        self.0.next_key_seed(seed) // Keys don't need transformation
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Self::Error> {
        self.0.next_value_seed(ProtoJsonDeserializeSeed(seed))
    }
}

struct ProtoJsonDeserializeSeed<T>(T);

impl<'de, T: DeserializeSeed<'de>> DeserializeSeed<'de> for ProtoJsonDeserializeSeed<T> {
    type Value = T::Value;

    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        self.0.deserialize(ProtoJsonDeserializer::new(deserializer))
    }
}

