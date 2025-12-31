use serde::ser::{SerializeSeq, SerializeStruct};

use crate::Protobuf;
use crate::base::Object;
use crate::google::protobuf::FieldDescriptorProto::{Label, Type};
use crate::reflection::{DynamicMessageArray, DynamicMessageRef, Value, default_value};
use crate::tables::{AuxTableEntry, Table};

// Well-known type detection and handling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WellKnownType {
    BoolValue,
    Int32Value,
    Int64Value,
    UInt32Value,
    UInt64Value,
    FloatValue,
    DoubleValue,
    StringValue,
    BytesValue,
    Timestamp,
    Duration,
    None,
}

// TODO: This should check the full qualified name (package.Type) instead of just the type name.
// Currently fragile - will incorrectly match user-defined types with the same name.
// Proper implementation would check descriptor's package and construct full name.
fn detect_well_known_type(
    descriptor: &crate::google::protobuf::DescriptorProto::ProtoType,
) -> WellKnownType {
    match descriptor.name() {
        "BoolValue" => WellKnownType::BoolValue,
        "Int32Value" => WellKnownType::Int32Value,
        "Int64Value" => WellKnownType::Int64Value,
        "UInt32Value" => WellKnownType::UInt32Value,
        "UInt64Value" => WellKnownType::UInt64Value,
        "FloatValue" => WellKnownType::FloatValue,
        "DoubleValue" => WellKnownType::DoubleValue,
        "StringValue" => WellKnownType::StringValue,
        "BytesValue" => WellKnownType::BytesValue,
        "Timestamp" => WellKnownType::Timestamp,
        "Duration" => WellKnownType::Duration,
        _ => WellKnownType::None,
    }
}

/// Look up enum value by name from the message descriptor
/// Returns the integer value if found, None otherwise
fn lookup_enum_value(
    descriptor: &crate::google::protobuf::DescriptorProto::ProtoType,
    type_name: &str,
    value_name: &str,
) -> Option<i32> {
    // Extract enum name from type_name (e.g., ".package.Message.EnumName" -> "EnumName")
    let enum_name = type_name.rsplit('.').next()?;

    // Search nested enums in this message
    for enum_type in descriptor.enum_type() {
        if enum_type.name() == enum_name {
            for value in enum_type.value() {
                if value.name() == value_name {
                    return Some(value.number());
                }
            }
        }
    }

    // Search in nested messages
    for nested in descriptor.nested_type() {
        if let Some(v) = lookup_enum_value(nested.as_ref(), type_name, value_name) {
            return Some(v);
        }
    }

    None
}

/// Look up enum name by value from the message descriptor
fn lookup_enum_name<'a>(
    descriptor: &'a crate::google::protobuf::DescriptorProto::ProtoType,
    type_name: &str,
    value: i32,
) -> Option<&'a str> {
    let enum_name = type_name.rsplit('.').next()?;

    for enum_type in descriptor.enum_type() {
        if enum_type.name() == enum_name {
            for enum_value in enum_type.value() {
                if enum_value.number() == value {
                    return Some(enum_value.name());
                }
            }
        }
    }

    for nested in descriptor.nested_type() {
        if let Some(name) = lookup_enum_name(nested.as_ref(), type_name, value) {
            return Some(name);
        }
    }

    None
}

/// Wrapper for serializing a single enum value as its string name.
struct EnumValue<'a> {
    descriptor: &'a crate::google::protobuf::DescriptorProto::ProtoType,
    type_name: &'a str,
    value: i32,
}

impl serde::Serialize for EnumValue<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match lookup_enum_name(self.descriptor, self.type_name, self.value) {
            Some(name) => serializer.serialize_str(name),
            None => serializer.serialize_i32(self.value),
        }
    }
}

/// Wrapper for serializing repeated enum values as string names.
struct RepeatedEnumValue<'a> {
    descriptor: &'a crate::google::protobuf::DescriptorProto::ProtoType,
    type_name: &'a str,
    values: &'a [i32],
}

impl serde::Serialize for RepeatedEnumValue<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.values.len()))?;
        for &v in self.values {
            match lookup_enum_name(self.descriptor, self.type_name, v) {
                Some(name) => seq.serialize_element(name)?,
                None => seq.serialize_element(&v)?,
            }
        }
        seq.end()
    }
}

// Timestamp validation and formatting
fn validate_timestamp(seconds: i64, nanos: i32) -> Result<(), &'static str> {
    // RFC 3339 valid range: 0001-01-01T00:00:00Z to 9999-12-31T23:59:59.999999999Z
    // Corresponds to: -62135596800 to 253402300799 seconds
    if !(-62135596800..=253402300799).contains(&seconds) {
        return Err("Timestamp seconds out of valid range");
    }
    if !(0..=999_999_999).contains(&nanos) {
        return Err("Timestamp nanos must be in range [0, 999999999]");
    }
    Ok(())
}

fn format_timestamp(seconds: i64, nanos: i32) -> Result<std::string::String, &'static str> {
    validate_timestamp(seconds, nanos)?;

    let dt = time::OffsetDateTime::from_unix_timestamp(seconds).map_err(|_| "Invalid timestamp")?;

    // Add nanoseconds
    let dt = dt + time::Duration::nanoseconds(nanos as i64);

    dt.format(&time::format_description::well_known::Rfc3339)
        .map_err(|_| "Format error")
}

fn parse_timestamp(s: &str) -> Result<(i64, i32), &'static str> {
    let dt = time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .map_err(|_| "Invalid RFC 3339 timestamp")?;

    let seconds = dt.unix_timestamp();
    let nanos = dt.nanosecond() as i32;

    validate_timestamp(seconds, nanos)?;
    Ok((seconds, nanos))
}

// Duration validation and formatting
fn validate_duration(seconds: i64, nanos: i32) -> Result<(), &'static str> {
    // Valid range: -315576000000 to +315576000000 seconds (approximately 10,000 years)
    if !(-315576000000..=315576000000).contains(&seconds) {
        return Err("Duration seconds out of valid range");
    }
    if !(-999_999_999..=999_999_999).contains(&nanos) {
        return Err("Duration nanos must be in range [-999999999, 999999999]");
    }
    // Check sign consistency
    if (seconds > 0 && nanos < 0) || (seconds < 0 && nanos > 0) {
        return Err("Duration seconds and nanos must have the same sign");
    }
    Ok(())
}

fn format_duration(seconds: i64, nanos: i32) -> Result<std::string::String, &'static str> {
    validate_duration(seconds, nanos)?;

    if seconds == 0 && nanos == 0 {
        return Ok("0s".to_string());
    }

    if nanos == 0 {
        return Ok(format!("{}s", seconds));
    }

    // Work with absolute values to avoid overflow from total_nanos calculation
    let is_negative = seconds < 0 || nanos < 0;
    let abs_seconds = seconds.unsigned_abs();
    let abs_nanos = nanos.unsigned_abs();

    let mut result = if is_negative {
        format!("-{}.{:09}s", abs_seconds, abs_nanos)
    } else {
        format!("{}.{:09}s", abs_seconds, abs_nanos)
    };

    // Trim trailing zeros from fractional part
    result = result
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string();
    if !result.ends_with('s') {
        result.push('s');
    }

    Ok(result)
}

fn parse_duration(s: &str) -> Result<(i64, i32), &'static str> {
    if !s.ends_with('s') {
        return Err("Duration must end with 's'");
    }

    let s = &s[..s.len() - 1]; // Remove 's' suffix

    if let Some(dot_pos) = s.find('.') {
        let (sec_str, frac_str) = s.split_at(dot_pos);
        let frac_str = &frac_str[1..]; // Skip the '.'

        let seconds: i64 = sec_str.parse().map_err(|_| "Invalid seconds")?;

        // Pad or truncate fractional part to 9 digits
        let mut frac_padded = frac_str.to_string();
        frac_padded.truncate(9);
        while frac_padded.len() < 9 {
            frac_padded.push('0');
        }

        let mut nanos: i32 = frac_padded.parse().map_err(|_| "Invalid nanos")?;

        // Apply sign from seconds to nanos
        if seconds < 0 {
            nanos = -nanos;
        }

        Ok((seconds, nanos))
    } else {
        let seconds: i64 = s.parse().map_err(|_| "Invalid seconds")?;
        Ok((seconds, 0))
    }
}

// Helper to serialize wrapper types
fn serialize_wrapper<S, T>(
    msg: &DynamicMessageRef,
    serializer: S,
    extract: impl FnOnce(&Value) -> Option<T>,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
    T: serde::Serialize,
{
    // Find field number 1 (the "value" field in all wrappers)
    let field = msg
        .find_field_descriptor_by_number(1)
        .ok_or_else(|| serde::ser::Error::custom("Wrapper missing 'value' field"))?;

    if let Some(value) = msg.get_field(field) {
        if let Some(unwrapped) = extract(&value) {
            unwrapped.serialize(serializer)
        } else {
            Err(serde::ser::Error::custom("Wrapper value has wrong type"))
        }
    } else {
        // Null value for missing wrapper
        serializer.serialize_none()
    }
}

fn serialize_timestamp<S>(msg: &DynamicMessageRef, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let seconds_field = msg
        .find_field_descriptor_by_number(1)
        .ok_or_else(|| serde::ser::Error::custom("Timestamp missing 'seconds' field"))?;
    let nanos_field = msg
        .find_field_descriptor_by_number(2)
        .ok_or_else(|| serde::ser::Error::custom("Timestamp missing 'nanos' field"))?;

    let seconds = match msg.get_field(seconds_field) {
        Some(Value::Int64(s)) => s,
        _ => 0i64,
    };

    let nanos = match msg.get_field(nanos_field) {
        Some(Value::Int32(n)) => n,
        _ => 0i32,
    };

    let timestamp_str = format_timestamp(seconds, nanos).map_err(serde::ser::Error::custom)?;

    serializer.serialize_str(&timestamp_str)
}

fn serialize_duration<S>(msg: &DynamicMessageRef, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let seconds_field = msg
        .find_field_descriptor_by_number(1)
        .ok_or_else(|| serde::ser::Error::custom("Duration missing 'seconds' field"))?;
    let nanos_field = msg
        .find_field_descriptor_by_number(2)
        .ok_or_else(|| serde::ser::Error::custom("Duration missing 'nanos' field"))?;

    let seconds = match msg.get_field(seconds_field) {
        Some(Value::Int64(s)) => s,
        _ => 0i64,
    };

    let nanos = match msg.get_field(nanos_field) {
        Some(Value::Int32(n)) => n,
        _ => 0i32,
    };

    let duration_str = format_duration(seconds, nanos).map_err(serde::ser::Error::custom)?;

    serializer.serialize_str(&duration_str)
}

impl<'pool, 'msg> serde::Serialize for DynamicMessageRef<'pool, 'msg> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let descriptor = self.descriptor();

        // Check if this is a well-known type
        match detect_well_known_type(descriptor) {
            WellKnownType::BoolValue => serialize_wrapper(self, serializer, |v| match v {
                Value::Bool(b) => Some(*b),
                _ => None,
            }),
            WellKnownType::Int32Value => serialize_wrapper(self, serializer, |v| match v {
                Value::Int32(i) => Some(*i),
                _ => None,
            }),
            WellKnownType::Int64Value => serialize_wrapper(self, serializer, |v| match v {
                Value::Int64(i) => Some(*i),
                _ => None,
            }),
            WellKnownType::UInt32Value => serialize_wrapper(self, serializer, |v| match v {
                Value::UInt32(u) => Some(*u),
                _ => None,
            }),
            WellKnownType::UInt64Value => serialize_wrapper(self, serializer, |v| match v {
                Value::UInt64(u) => Some(*u),
                _ => None,
            }),
            WellKnownType::FloatValue => serialize_wrapper(self, serializer, |v| match v {
                Value::Float(f) => Some(*f),
                _ => None,
            }),
            WellKnownType::DoubleValue => serialize_wrapper(self, serializer, |v| match v {
                Value::Double(d) => Some(*d),
                _ => None,
            }),
            WellKnownType::StringValue => {
                let field = self.find_field_descriptor_by_number(1).ok_or_else(|| {
                    serde::ser::Error::custom("StringValue missing 'value' field")
                })?;
                if let Some(Value::String(s)) = self.get_field(field) {
                    serializer.serialize_str(s)
                } else {
                    serializer.serialize_none()
                }
            }
            WellKnownType::BytesValue => {
                let field = self
                    .find_field_descriptor_by_number(1)
                    .ok_or_else(|| serde::ser::Error::custom("BytesValue missing 'value' field"))?;
                if let Some(Value::Bytes(b)) = self.get_field(field) {
                    serializer.serialize_bytes(b)
                } else {
                    serializer.serialize_none()
                }
            }
            WellKnownType::Timestamp => serialize_timestamp(self, serializer),
            WellKnownType::Duration => serialize_duration(self, serializer),
            WellKnownType::None => {
                // Regular message serialization
                // Count fields first
                let field_count = descriptor
                    .field()
                    .iter()
                    .filter(|f| self.get_field(f.as_ref()).is_some())
                    .count();
                let mut struct_serializer = serializer.serialize_struct("", field_count)?;

                for field in descriptor.field() {
                    let Some(v) = self.get_field(field.as_ref()) else {
                        continue;
                    };
                    // Transmute needed due to serialize_field requiring 'static
                    let json_name: &'static str =
                        unsafe { std::mem::transmute(field.json_name()) };

                    // Check if this is an enum field - serialize as string name
                    if field.r#type() == Some(Type::TYPE_ENUM) {
                        let type_name = field.type_name();
                        match v {
                            Value::Int32(int_val) => {
                                match lookup_enum_name(descriptor, type_name, int_val) {
                                    Some(name) => {
                                        let name: &'static str =
                                            unsafe { std::mem::transmute(name) };
                                        struct_serializer.serialize_field(json_name, name)?
                                    }
                                    None => struct_serializer.serialize_field(json_name, &int_val)?,
                                }
                            }
                            Value::RepeatedInt32(list) => {
                                let enum_vals = RepeatedEnumValue {
                                    descriptor,
                                    type_name,
                                    values: list,
                                };
                                unsafe {
                                    let enum_vals = std::mem::transmute::<
                                        RepeatedEnumValue<'_>,
                                        RepeatedEnumValue<'static>,
                                    >(enum_vals);
                                    struct_serializer.serialize_field(json_name, &enum_vals)?;
                                }
                            }
                            _ => {
                                // Shouldn't happen, but serialize as-is
                                unsafe {
                                    let value =
                                        std::mem::transmute::<Value<'_, '_>, Value<'static, 'msg>>(
                                            v,
                                        );
                                    struct_serializer.serialize_field(json_name, &value)?;
                                }
                            }
                        }
                    } else {
                        unsafe {
                            let value =
                                std::mem::transmute::<Value<'_, '_>, Value<'static, 'msg>>(v);
                            struct_serializer.serialize_field(json_name, &value)?;
                        }
                    }
                }
                struct_serializer.end()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum MapKey {
    Bool(bool),
    Int32(i32),
    Int64(i64),
    UInt32(u32),
    UInt64(u64),
    String(std::string::String),
}

impl<'msg> serde::Serialize for DynamicMessageArray<'static, 'msg> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if self
            .table
            .descriptor
            .options()
            .map(|o| o.map_entry())
            .unwrap_or(false)
        {
            use serde::ser::SerializeMap;
            let mut map_serializer = serializer.serialize_map(Some(self.object.len()))?;

            let mut seen_keys = std::collections::hash_set::HashSet::<MapKey>::new();
            for index in (0..self.object.len()).rev() {
                let entry = self.get(index);
                let key_field = entry
                    .find_field_descriptor_by_number(1)
                    .ok_or_else(|| serde::ser::Error::custom("Map entry missing key field"))?;
                let value_field = entry
                    .find_field_descriptor_by_number(2)
                    .ok_or_else(|| serde::ser::Error::custom("Map entry missing value field"))?;
                let key_val = entry
                    .get_field(key_field)
                    .or_else(|| default_value(key_field))
                    .ok_or_else(|| {
                        serde::ser::Error::custom(
                            "Map entry key field missing and no default value",
                        )
                    })?;
                let value_val = entry
                    .get_field(value_field)
                    .or_else(|| default_value(value_field));
                let map_key = match key_val {
                    Value::Bool(v) => MapKey::Bool(v),
                    Value::Int32(v) => MapKey::Int32(v),
                    Value::Int64(v) => MapKey::Int64(v),
                    Value::UInt32(v) => MapKey::UInt32(v),
                    Value::UInt64(v) => MapKey::UInt64(v),
                    Value::String(v) => MapKey::String(v.to_string()),
                    _ => {
                        return Err(serde::ser::Error::custom(
                            "Invalid map key type; must be scalar",
                        ));
                    }
                };
                if !seen_keys.insert(map_key) {
                    continue; // Skip duplicate keys, keep the last one
                }
                // Check if value is an enum field
                if value_field.r#type() == Some(Type::TYPE_ENUM) {
                    if let Some(Value::Int32(int_val)) = value_val {
                        let enum_val = EnumValue {
                            descriptor: self.table.descriptor,
                            type_name: value_field.type_name(),
                            value: int_val,
                        };
                        map_serializer.serialize_entry(&key_val, &enum_val)?;
                    } else {
                        map_serializer.serialize_entry(&key_val, &value_val)?;
                    }
                } else {
                    map_serializer.serialize_entry(&key_val, &value_val)?;
                }
            }
            return map_serializer.end();
        }
        let mut seq_serializer = serializer.serialize_seq(Some(self.object.len()))?;
        for index in 0..self.object.len() {
            seq_serializer.serialize_element(&self.get(index))?;
        }
        seq_serializer.end()
    }
}

impl<'msg> serde::Serialize for Value<'static, 'msg> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match *self {
            Value::Bool(v) => serializer.serialize_bool(v),
            Value::Int32(v) => serializer.serialize_i32(v),
            Value::Int64(v) => serializer.serialize_i64(v),
            Value::UInt32(v) => serializer.serialize_u32(v),
            Value::UInt64(v) => serializer.serialize_u64(v),
            Value::Float(v) => serializer.serialize_f32(v),
            Value::Double(v) => serializer.serialize_f64(v),
            Value::String(v) => serializer.serialize_str(v),
            Value::Bytes(v) => serializer.serialize_bytes(v),
            Value::Message(ref msg) => msg.serialize(serializer),
            Value::RepeatedBool(list) => list.serialize(serializer),
            Value::RepeatedInt32(list) => list.serialize(serializer),
            Value::RepeatedInt64(list) => list.serialize(serializer),
            Value::RepeatedUInt32(list) => list.serialize(serializer),
            Value::RepeatedUInt64(list) => list.serialize(serializer),
            Value::RepeatedFloat(list) => list.serialize(serializer),
            Value::RepeatedDouble(list) => list.serialize(serializer),
            Value::RepeatedString(list) => list.serialize(serializer),
            Value::RepeatedBytes(list) => list.serialize(serializer),
            Value::RepeatedMessage(ref list) => list.serialize(serializer),
        }
    }
}

impl serde::Serialize for crate::containers::Bytes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_bytes(self.as_ref())
    }
}

impl serde::Serialize for crate::containers::String {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_ref())
    }
}

pub struct SerdeDeserialize<'arena, 'alloc, T>(
    &'arena mut crate::arena::Arena<'alloc>,
    core::marker::PhantomData<T>,
);

impl<'arena, 'alloc, T> SerdeDeserialize<'arena, 'alloc, T> {
    pub fn new(arena: &'arena mut crate::arena::Arena<'alloc>) -> Self {
        SerdeDeserialize(arena, core::marker::PhantomData)
    }
}

impl<'de, 'arena, 'alloc, T: Protobuf + 'alloc> serde::de::DeserializeSeed<'de>
    for SerdeDeserialize<'arena, 'alloc, T>
{
    type Value = T;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Deserialization logic to be implemented
        let SerdeDeserialize(arena, _) = self;
        let mut msg = T::default();
        serde_deserialize_struct(crate::as_object_mut(&mut msg), T::table(), arena, deserializer)?;
        Ok(msg)
    }
}

pub struct ProtobufVisitor<'arena, 'alloc, 'b, 'pool> {
    obj: &'b mut Object,
    table: &'pool Table,
    arena: &'arena mut crate::arena::Arena<'alloc>,
}

impl<'de, 'arena, 'alloc, 'b, 'pool> serde::de::DeserializeSeed<'de>
    for ProtobufVisitor<'arena, 'alloc, 'b, 'pool>
{
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let ProtobufVisitor { obj, table, arena } = self;
        serde_deserialize_struct(obj, table, arena, deserializer)?;
        Ok(())
    }
}

pub struct Optional<T>(T);

/// DeserializeSeed for enum values - accepts both integers and string names
struct EnumSeed<'a> {
    descriptor: &'a crate::google::protobuf::DescriptorProto::ProtoType,
    type_name: &'a str,
}

impl<'de, 'a> serde::de::DeserializeSeed<'de> for EnumSeed<'a> {
    type Value = i32;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }
}

impl<'de, 'a> serde::de::Visitor<'de> for EnumSeed<'a> {
    type Value = i32;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        formatter.write_str("enum value (integer or string name)")
    }

    fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
        Ok(v as i32)
    }

    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
        Ok(v as i32)
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
        lookup_enum_value(self.descriptor, self.type_name, v).ok_or_else(|| {
            E::custom(format!(
                "unknown enum value '{}' for type '{}'",
                v, self.type_name
            ))
        })
    }
}

/// DeserializeSeed for repeated enum values
struct EnumArraySeed<'a> {
    descriptor: &'a crate::google::protobuf::DescriptorProto::ProtoType,
    type_name: &'a str,
}

impl<'de, 'a> serde::de::DeserializeSeed<'de> for EnumArraySeed<'a> {
    type Value = Vec<i32>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_seq(self)
    }
}

impl<'de, 'a> serde::de::Visitor<'de> for EnumArraySeed<'a> {
    type Value = Vec<i32>;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        formatter.write_str("array of enum values")
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut values = Vec::new();
        while let Some(v) = seq.next_element_seed(EnumSeed {
            descriptor: self.descriptor,
            type_name: self.type_name,
        })? {
            values.push(v);
        }
        Ok(values)
    }
}

impl<'de, T> serde::de::DeserializeSeed<'de> for Optional<T>
where
    T: serde::de::DeserializeSeed<'de>,
{
    type Value = Option<T::Value>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_option(self)
    }
}

impl<'de, T> serde::de::Visitor<'de> for Optional<T>
where
    T: serde::de::DeserializeSeed<'de>,
{
    type Value = Option<T::Value>;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        formatter.write_str("optional value")
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(None)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Some(self.0.deserialize(deserializer)?))
    }
}

pub fn serde_deserialize_struct<'arena, 'alloc, 'b, 'de, 'pool, D>(
    obj: &'b mut Object,
    table: &'pool Table,
    arena: &'arena mut crate::arena::Arena<'alloc>,
    deserializer: D,
) -> Result<(), D::Error>
where
    D: serde::Deserializer<'de>,
{
    let visitor = ProtobufVisitor { obj, table, arena };

    // For well-known types, use appropriate deserialize method
    match detect_well_known_type(table.descriptor) {
        WellKnownType::BytesValue => return deserializer.deserialize_bytes(visitor),
        WellKnownType::None => {}
        _ => return deserializer.deserialize_any(visitor),
    }

    let fields = table.descriptor.field();
    let field_names: Vec<&str> = fields.iter().map(|f| f.name()).collect();
    let field_names_slice = field_names.as_slice();
    let field_names_static = unsafe {
        std::mem::transmute::<&[&'static str], &'static [&'static str]>(field_names_slice)
    };
    deserializer.deserialize_struct(table.descriptor.name(), field_names_static, visitor)
}

struct StructKeyVisitor<'a>(&'a std::collections::HashMap<&'static str, usize>);

impl<'de> serde::de::DeserializeSeed<'de> for StructKeyVisitor<'_> {
    type Value = usize;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_identifier(self)
    }
}

impl<'de> serde::de::Visitor<'de> for StructKeyVisitor<'_> {
    type Value = usize;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        formatter.write_str("a valid field name")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.0
            .get(v)
            .copied()
            .ok_or_else(|| serde::de::Error::unknown_field(v, &[]))
    }
}

struct ProtobufArrayfVisitor<'arena, 'alloc, 'b> {
    rf: &'b mut crate::containers::RepeatedField<crate::base::Message>,
    table: &'static Table,
    arena: &'arena mut crate::arena::Arena<'alloc>,
}

impl<'de, 'arena, 'alloc, 'b> serde::de::DeserializeSeed<'de>
    for ProtobufArrayfVisitor<'arena, 'alloc, 'b>
{
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_seq(self)
    }
}

impl<'de, 'arena, 'alloc, 'b> serde::de::Visitor<'de>
    for ProtobufArrayfVisitor<'arena, 'alloc, 'b>
{
    type Value = ();

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        formatter.write_str(&format!("an array of {}", self.table.descriptor.name()))
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let ProtobufArrayfVisitor { rf, table, arena } = self;
        loop {
            let msg_obj = Object::create(table.size as u32, arena);

            let seed = ProtobufVisitor {
                obj: msg_obj,
                table,
                arena,
            };

            match seq.next_element_seed(seed)? {
                Some(()) => {
                    rf.push(crate::base::Message(msg_obj as *mut Object), arena);
                }
                None => {
                    return Ok(());
                }
            }
        }
    }
}

struct ProtobufMapVisitor<'arena, 'alloc, 'b> {
    rf: &'b mut crate::containers::RepeatedField<crate::base::Message>,
    table: &'static Table,
    arena: &'arena mut crate::arena::Arena<'alloc>,
}

impl<'de, 'arena, 'alloc, 'b> serde::de::DeserializeSeed<'de>
    for ProtobufMapVisitor<'arena, 'alloc, 'b>
{
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(self)
    }
}

impl<'de, 'arena, 'alloc, 'b> serde::de::Visitor<'de> for ProtobufMapVisitor<'arena, 'alloc, 'b> {
    type Value = ();

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        formatter.write_str(&format!(
            "a map with {} entries",
            self.table.descriptor.name()
        ))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let ProtobufMapVisitor { rf, table, arena } = self;

        let key_field = table.descriptor.field()[0];
        let value_field = table.descriptor.field()[1];
        let key_entry = table
            .entry(1)
            .ok_or_else(|| serde::de::Error::custom("Map entry missing key field in table"))?;
        let value_entry = table
            .entry(2)
            .ok_or_else(|| serde::de::Error::custom("Map entry missing value field in table"))?;

        while let Some(key_str) = map.next_key::<std::string::String>()? {
            let entry_obj = Object::create(table.size as u32, arena);

            match key_field.r#type().unwrap() {
                Type::TYPE_BOOL => {
                    let key_val: bool = key_str.parse().map_err(serde::de::Error::custom)?;
                    entry_obj.set::<bool>(key_entry.offset(), key_entry.has_bit_idx(), key_val);
                }
                Type::TYPE_INT32 | Type::TYPE_SINT32 | Type::TYPE_SFIXED32 => {
                    let key_val: i32 = key_str.parse().map_err(serde::de::Error::custom)?;
                    entry_obj.set::<i32>(key_entry.offset(), key_entry.has_bit_idx(), key_val);
                }
                Type::TYPE_INT64 | Type::TYPE_SINT64 | Type::TYPE_SFIXED64 => {
                    let key_val: i64 = key_str.parse().map_err(serde::de::Error::custom)?;
                    entry_obj.set::<i64>(key_entry.offset(), key_entry.has_bit_idx(), key_val);
                }
                Type::TYPE_UINT32 | Type::TYPE_FIXED32 => {
                    let key_val: u32 = key_str.parse().map_err(serde::de::Error::custom)?;
                    entry_obj.set::<u32>(key_entry.offset(), key_entry.has_bit_idx(), key_val);
                }
                Type::TYPE_UINT64 | Type::TYPE_FIXED64 => {
                    let key_val: u64 = key_str.parse().map_err(serde::de::Error::custom)?;
                    entry_obj.set::<u64>(key_entry.offset(), key_entry.has_bit_idx(), key_val);
                }
                Type::TYPE_STRING => {
                    let s = crate::containers::String::from_str(&key_str, arena);
                    entry_obj.set::<crate::containers::String>(
                        key_entry.offset(),
                        key_entry.has_bit_idx(),
                        s,
                    );
                }
                _ => {
                    return Err(serde::de::Error::custom(format!(
                        "Unsupported map key type: {:?}",
                        key_field.r#type()
                    )));
                }
            }

            match value_field.r#type().unwrap() {
                Type::TYPE_BOOL => {
                    let v: bool = map.next_value()?;
                    entry_obj.set::<bool>(value_entry.offset(), value_entry.has_bit_idx(), v);
                }
                Type::TYPE_INT32 | Type::TYPE_SINT32 | Type::TYPE_SFIXED32 => {
                    let v: i32 = map.next_value()?;
                    entry_obj.set::<i32>(value_entry.offset(), value_entry.has_bit_idx(), v);
                }
                Type::TYPE_ENUM => {
                    let seed = EnumSeed {
                        descriptor: table.descriptor,
                        type_name: value_field.type_name(),
                    };
                    let v: i32 = map.next_value_seed(seed)?;
                    entry_obj.set::<i32>(value_entry.offset(), value_entry.has_bit_idx(), v);
                }
                Type::TYPE_INT64 | Type::TYPE_SINT64 | Type::TYPE_SFIXED64 => {
                    let v: i64 = map.next_value()?;
                    entry_obj.set::<i64>(value_entry.offset(), value_entry.has_bit_idx(), v);
                }
                Type::TYPE_UINT32 | Type::TYPE_FIXED32 => {
                    let v: u32 = map.next_value()?;
                    entry_obj.set::<u32>(value_entry.offset(), value_entry.has_bit_idx(), v);
                }
                Type::TYPE_UINT64 | Type::TYPE_FIXED64 => {
                    let v: u64 = map.next_value()?;
                    entry_obj.set::<u64>(value_entry.offset(), value_entry.has_bit_idx(), v);
                }
                Type::TYPE_FLOAT => {
                    let v: f32 = map.next_value()?;
                    entry_obj.set::<f32>(value_entry.offset(), value_entry.has_bit_idx(), v);
                }
                Type::TYPE_DOUBLE => {
                    let v: f64 = map.next_value()?;
                    entry_obj.set::<f64>(value_entry.offset(), value_entry.has_bit_idx(), v);
                }
                Type::TYPE_STRING => {
                    let v: std::string::String = map.next_value()?;
                    let s = crate::containers::String::from_str(&v, arena);
                    entry_obj.set::<crate::containers::String>(
                        value_entry.offset(),
                        value_entry.has_bit_idx(),
                        s,
                    );
                }
                Type::TYPE_BYTES => {
                    let v: BytesBuf = map.next_value()?;
                    let b = crate::containers::Bytes::from_slice(&v.0, arena);
                    entry_obj.set::<crate::containers::Bytes>(
                        value_entry.offset(),
                        value_entry.has_bit_idx(),
                        b,
                    );
                }
                Type::TYPE_MESSAGE | Type::TYPE_GROUP => {
                    let value_aux = table.aux_entry_decode(value_entry);
                    let value_child_table = unsafe { &*value_aux.child_table };
                    let child_obj = Object::create(value_child_table.size as u32, arena);
                    let seed = ProtobufVisitor {
                        obj: child_obj,
                        table: value_child_table,
                        arena,
                    };
                    map.next_value_seed(seed)?;
                    *entry_obj.ref_mut::<crate::base::Message>(value_aux.offset) =
                        crate::base::Message(child_obj);
                }
            }

            rf.push(crate::base::Message(entry_obj as *mut Object), arena);
        }
        Ok(())
    }
}

// Wrapper deserialization helpers
fn deserialize_wrapper<'de, A, T>(
    obj: &mut Object,
    table: &Table,
    mut map: A,
) -> Result<(), A::Error>
where
    A: serde::de::MapAccess<'de>,
    T: serde::de::Deserialize<'de> + Copy,
{
    let entry = table
        .entry(1)
        .ok_or_else(|| serde::de::Error::custom("Wrapper missing 'value' field in table"))?;

    while let Some(key) = map.next_key::<std::string::String>()? {
        if key == "value" {
            let val: T = map.next_value()?;
            obj.set::<T>(entry.offset(), entry.has_bit_idx(), val);
            return Ok(());
        } else {
            map.next_value::<serde::de::IgnoredAny>()?;
        }
    }
    Ok(())
}

fn deserialize_wrapper_string<'de, 'alloc, A>(
    obj: &mut Object,
    table: &Table,
    arena: &mut crate::arena::Arena<'alloc>,
    mut map: A,
) -> Result<(), A::Error>
where
    A: serde::de::MapAccess<'de>,
{
    let entry = table
        .entry(1)
        .ok_or_else(|| serde::de::Error::custom("StringValue missing 'value' field in table"))?;

    while let Some(key) = map.next_key::<std::string::String>()? {
        if key == "value" {
            let val: std::string::String = map.next_value()?;
            let s = crate::containers::String::from_str(&val, arena);
            obj.set::<crate::containers::String>(entry.offset(), entry.has_bit_idx(), s);
            return Ok(());
        } else {
            map.next_value::<serde::de::IgnoredAny>()?;
        }
    }
    Ok(())
}

fn deserialize_wrapper_bytes<'de, 'alloc, A>(
    obj: &mut Object,
    table: &Table,
    arena: &mut crate::arena::Arena<'alloc>,
    mut map: A,
) -> Result<(), A::Error>
where
    A: serde::de::MapAccess<'de>,
{
    let entry = table
        .entry(1)
        .ok_or_else(|| serde::de::Error::custom("BytesValue missing 'value' field in table"))?;

    while let Some(key) = map.next_key::<std::string::String>()? {
        if key == "value" {
            let val: BytesBuf = map.next_value()?;
            let b = crate::containers::Bytes::from_slice(&val.0, arena);
            obj.set::<crate::containers::Bytes>(entry.offset(), entry.has_bit_idx(), b);
            return Ok(());
        } else {
            map.next_value::<serde::de::IgnoredAny>()?;
        }
    }
    Ok(())
}

fn deserialize_timestamp<'de, A>(
    obj: &mut Object,
    table: &Table,
    mut map: A,
) -> Result<(), A::Error>
where
    A: serde::de::MapAccess<'de>,
{
    let seconds_entry = table
        .entry(1)
        .ok_or_else(|| serde::de::Error::custom("Timestamp missing 'seconds' field in table"))?;
    let nanos_entry = table
        .entry(2)
        .ok_or_else(|| serde::de::Error::custom("Timestamp missing 'nanos' field in table"))?;

    while let Some(key) = map.next_key::<std::string::String>()? {
        match key.as_str() {
            "seconds" => {
                let s: i64 = map.next_value()?;
                obj.set::<i64>(seconds_entry.offset(), seconds_entry.has_bit_idx(), s);
            }
            "nanos" => {
                let n: i32 = map.next_value()?;
                obj.set::<i32>(nanos_entry.offset(), nanos_entry.has_bit_idx(), n);
            }
            _ => {
                map.next_value::<serde::de::IgnoredAny>()?;
            }
        }
    }

    // Validate after parsing
    let seconds = obj.get::<i64>(seconds_entry.offset() as usize);
    let nanos = obj.get::<i32>(nanos_entry.offset() as usize);
    validate_timestamp(seconds, nanos).map_err(serde::de::Error::custom)?;

    Ok(())
}

fn deserialize_duration<'de, A>(obj: &mut Object, table: &Table, mut map: A) -> Result<(), A::Error>
where
    A: serde::de::MapAccess<'de>,
{
    let seconds_entry = table
        .entry(1)
        .ok_or_else(|| serde::de::Error::custom("Duration missing 'seconds' field in table"))?;
    let nanos_entry = table
        .entry(2)
        .ok_or_else(|| serde::de::Error::custom("Duration missing 'nanos' field in table"))?;

    while let Some(key) = map.next_key::<std::string::String>()? {
        match key.as_str() {
            "seconds" => {
                let s: i64 = map.next_value()?;
                obj.set::<i64>(seconds_entry.offset(), seconds_entry.has_bit_idx(), s);
            }
            "nanos" => {
                let n: i32 = map.next_value()?;
                obj.set::<i32>(nanos_entry.offset(), nanos_entry.has_bit_idx(), n);
            }
            _ => {
                map.next_value::<serde::de::IgnoredAny>()?;
            }
        }
    }

    // Validate after parsing
    let seconds = obj.get::<i64>(seconds_entry.offset() as usize);
    let nanos = obj.get::<i32>(nanos_entry.offset() as usize);
    validate_duration(seconds, nanos).map_err(serde::de::Error::custom)?;

    Ok(())
}

impl<'de, 'arena, 'alloc, 'b, 'pool> serde::de::Visitor<'de>
    for ProtobufVisitor<'arena, 'alloc, 'b, 'pool>
{
    type Value = ();

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        formatter.write_str(self.table.descriptor.name())
    }

    // Handle unwrapped wrapper types from JSON
    fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match detect_well_known_type(self.table.descriptor) {
            WellKnownType::BoolValue => {
                let entry = self
                    .table
                    .entry(1)
                    .ok_or_else(|| E::custom("BoolValue missing field 1"))?;
                self.obj.set::<bool>(entry.offset(), entry.has_bit_idx(), v);
                Ok(())
            }
            _ => Err(E::invalid_type(serde::de::Unexpected::Bool(v), &self)),
        }
    }

    fn visit_i32<E>(self, v: i32) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match detect_well_known_type(self.table.descriptor) {
            WellKnownType::Int32Value => {
                let entry = self
                    .table
                    .entry(1)
                    .ok_or_else(|| E::custom("Int32Value missing field 1"))?;
                self.obj.set::<i32>(entry.offset(), entry.has_bit_idx(), v);
                Ok(())
            }
            _ => Err(E::invalid_type(
                serde::de::Unexpected::Signed(v as i64),
                &self,
            )),
        }
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match detect_well_known_type(self.table.descriptor) {
            WellKnownType::Int64Value => {
                let entry = self
                    .table
                    .entry(1)
                    .ok_or_else(|| E::custom("Int64Value missing field 1"))?;
                self.obj.set::<i64>(entry.offset(), entry.has_bit_idx(), v);
                Ok(())
            }
            WellKnownType::Int32Value => self.visit_i32(v as i32),
            WellKnownType::UInt64Value => self.visit_u64(v as u64),
            WellKnownType::UInt32Value => self.visit_u32(v as u32),
            WellKnownType::FloatValue => self.visit_f32(v as f32),
            WellKnownType::DoubleValue => self.visit_f64(v as f64),
            _ => Err(E::invalid_type(serde::de::Unexpected::Signed(v), &self)),
        }
    }

    fn visit_u32<E>(self, v: u32) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match detect_well_known_type(self.table.descriptor) {
            WellKnownType::UInt32Value => {
                let entry = self
                    .table
                    .entry(1)
                    .ok_or_else(|| E::custom("UInt32Value missing field 1"))?;
                self.obj.set::<u32>(entry.offset(), entry.has_bit_idx(), v);
                Ok(())
            }
            _ => Err(E::invalid_type(
                serde::de::Unexpected::Unsigned(v as u64),
                &self,
            )),
        }
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match detect_well_known_type(self.table.descriptor) {
            WellKnownType::UInt64Value => {
                let entry = self
                    .table
                    .entry(1)
                    .ok_or_else(|| E::custom("UInt64Value missing field 1"))?;
                self.obj.set::<u64>(entry.offset(), entry.has_bit_idx(), v);
                Ok(())
            }
            WellKnownType::UInt32Value => self.visit_u32(v as u32),
            WellKnownType::Int64Value => self.visit_i64(v as i64),
            WellKnownType::Int32Value => self.visit_i32(v as i32),
            WellKnownType::FloatValue => self.visit_f32(v as f32),
            WellKnownType::DoubleValue => self.visit_f64(v as f64),
            _ => Err(E::invalid_type(serde::de::Unexpected::Unsigned(v), &self)),
        }
    }

    fn visit_f32<E>(self, v: f32) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match detect_well_known_type(self.table.descriptor) {
            WellKnownType::FloatValue => {
                let entry = self
                    .table
                    .entry(1)
                    .ok_or_else(|| E::custom("FloatValue missing field 1"))?;
                self.obj.set::<f32>(entry.offset(), entry.has_bit_idx(), v);
                Ok(())
            }
            _ => Err(E::invalid_type(
                serde::de::Unexpected::Float(v as f64),
                &self,
            )),
        }
    }

    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match detect_well_known_type(self.table.descriptor) {
            WellKnownType::DoubleValue => {
                let entry = self
                    .table
                    .entry(1)
                    .ok_or_else(|| E::custom("DoubleValue missing field 1"))?;
                self.obj.set::<f64>(entry.offset(), entry.has_bit_idx(), v);
                Ok(())
            }
            WellKnownType::FloatValue => self.visit_f32(v as f32),
            _ => Err(E::invalid_type(serde::de::Unexpected::Float(v), &self)),
        }
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match detect_well_known_type(self.table.descriptor) {
            WellKnownType::BytesValue => {
                let entry = self
                    .table
                    .entry(1)
                    .ok_or_else(|| E::custom("BytesValue missing field 1"))?;
                let b = crate::containers::Bytes::from_slice(v, self.arena);
                self.obj
                    .set::<crate::containers::Bytes>(entry.offset(), entry.has_bit_idx(), b);
                Ok(())
            }
            _ => Err(E::invalid_type(serde::de::Unexpected::Bytes(v), &self)),
        }
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match detect_well_known_type(self.table.descriptor) {
            WellKnownType::StringValue => {
                let entry = self
                    .table
                    .entry(1)
                    .ok_or_else(|| E::custom("StringValue missing field 1"))?;
                let s = crate::containers::String::from_str(v, self.arena);
                self.obj
                    .set::<crate::containers::String>(entry.offset(), entry.has_bit_idx(), s);
                Ok(())
            }
            WellKnownType::Timestamp => {
                let (seconds, nanos) = parse_timestamp(v).map_err(E::custom)?;
                let seconds_entry = self
                    .table
                    .entry(1)
                    .ok_or_else(|| E::custom("Timestamp missing field 1"))?;
                let nanos_entry = self
                    .table
                    .entry(2)
                    .ok_or_else(|| E::custom("Timestamp missing field 2"))?;
                self.obj
                    .set::<i64>(seconds_entry.offset(), seconds_entry.has_bit_idx(), seconds);
                self.obj
                    .set::<i32>(nanos_entry.offset(), nanos_entry.has_bit_idx(), nanos);
                Ok(())
            }
            WellKnownType::Duration => {
                let (seconds, nanos) = parse_duration(v).map_err(E::custom)?;
                let seconds_entry = self
                    .table
                    .entry(1)
                    .ok_or_else(|| E::custom("Duration missing field 1"))?;
                let nanos_entry = self
                    .table
                    .entry(2)
                    .ok_or_else(|| E::custom("Duration missing field 2"))?;
                self.obj
                    .set::<i64>(seconds_entry.offset(), seconds_entry.has_bit_idx(), seconds);
                self.obj
                    .set::<i32>(nanos_entry.offset(), nanos_entry.has_bit_idx(), nanos);
                Ok(())
            }
            WellKnownType::Int64Value => {
                let n: i64 = v.parse().map_err(E::custom)?;
                self.visit_i64(n)
            }
            WellKnownType::UInt64Value => {
                let n: u64 = v.parse().map_err(E::custom)?;
                self.visit_u64(n)
            }
            _ => Err(E::invalid_type(serde::de::Unexpected::Str(v), &self)),
        }
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let ProtobufVisitor { obj, table, arena } = self;

        // Check if this is a well-known type
        match detect_well_known_type(table.descriptor) {
            WellKnownType::BoolValue => return deserialize_wrapper::<A, bool>(obj, table, map),
            WellKnownType::Int32Value => return deserialize_wrapper::<A, i32>(obj, table, map),
            WellKnownType::Int64Value => return deserialize_wrapper::<A, i64>(obj, table, map),
            WellKnownType::UInt32Value => return deserialize_wrapper::<A, u32>(obj, table, map),
            WellKnownType::UInt64Value => return deserialize_wrapper::<A, u64>(obj, table, map),
            WellKnownType::FloatValue => return deserialize_wrapper::<A, f32>(obj, table, map),
            WellKnownType::DoubleValue => return deserialize_wrapper::<A, f64>(obj, table, map),
            WellKnownType::StringValue => {
                return deserialize_wrapper_string(obj, table, arena, map);
            }
            WellKnownType::BytesValue => return deserialize_wrapper_bytes(obj, table, arena, map),
            WellKnownType::Timestamp => return deserialize_timestamp(obj, table, map),
            WellKnownType::Duration => return deserialize_duration(obj, table, map),
            WellKnownType::None => {
                // Continue with regular deserialization
            }
        }

        let mut field_map = std::collections::HashMap::new();
        for (field_index, field) in table.descriptor.field().iter().enumerate() {
            let field_name = field.json_name();
            field_map.insert(field_name, field_index);
        }
        while let Some(idx) = map.next_key_seed(StructKeyVisitor(&field_map))? {
            let field = table.descriptor.field()[idx];
            let entry = table.entry(field.number() as u32).unwrap(); // Safe: field exists in table
            match field.label().unwrap() {
                Label::LABEL_REPEATED => match field.r#type().unwrap() {
                    Type::TYPE_BOOL => {
                        let Some(slice) = map.next_value::<Option<Vec<bool>>>()? else {
                            continue;
                        };
                        for v in slice {
                            obj.add::<bool>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_FIXED64 | Type::TYPE_UINT64 => {
                        let Some(slice) = map.next_value::<Option<Vec<u64>>>()? else {
                            continue;
                        };
                        for v in slice {
                            obj.add::<u64>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_FIXED32 | Type::TYPE_UINT32 => {
                        let Some(slice) = map.next_value::<Option<Vec<u32>>>()? else {
                            continue;
                        };
                        for v in slice {
                            obj.add::<u32>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_SFIXED64 | Type::TYPE_INT64 | Type::TYPE_SINT64 => {
                        let Some(slice) = map.next_value::<Option<Vec<i64>>>()? else {
                            continue;
                        };
                        for v in slice {
                            obj.add::<i64>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_SFIXED32 | Type::TYPE_INT32 | Type::TYPE_SINT32 => {
                        let Some(slice) = map.next_value::<Option<Vec<i32>>>()? else {
                            continue;
                        };
                        for v in slice {
                            obj.add::<i32>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_ENUM => {
                        let seed = EnumArraySeed {
                            descriptor: table.descriptor,
                            type_name: field.type_name(),
                        };
                        let Some(slice) = map.next_value_seed(Optional(seed))? else {
                            continue;
                        };
                        for v in slice {
                            obj.add::<i32>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_FLOAT => {
                        let Some(slice) = map.next_value::<Option<Vec<f32>>>()? else {
                            continue;
                        };
                        for v in slice {
                            obj.add::<f32>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_DOUBLE => {
                        let Some(slice) = map.next_value::<Option<Vec<f64>>>()? else {
                            continue;
                        };
                        for v in slice {
                            obj.add::<f64>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_STRING => {
                        let Some(slice) = map.next_value::<Option<Vec<String>>>()? else {
                            continue;
                        };
                        for v in slice {
                            let s = crate::containers::String::from_str(&v, arena);
                            obj.add::<crate::containers::String>(entry.offset(), s, arena);
                        }
                    }
                    Type::TYPE_BYTES => {
                        let Some(slice) = map.next_value::<Option<Vec<BytesBuf>>>()? else {
                            continue;
                        };
                        for v in slice {
                            let b = crate::containers::Bytes::from_slice(&v.0, arena);
                            obj.add::<crate::containers::Bytes>(entry.offset(), b, arena);
                        }
                    }
                    Type::TYPE_MESSAGE | Type::TYPE_GROUP => {
                        let AuxTableEntry {
                            offset,
                            child_table,
                        } = table.aux_entry_decode(entry);
                        let child_table = unsafe { &*child_table };
                        let rf = obj
                            .ref_mut::<crate::containers::RepeatedField<crate::base::Message>>(
                                offset,
                            );

                        if child_table
                            .descriptor
                            .options()
                            .map(|o| o.map_entry())
                            .unwrap_or(false)
                        {
                            let seed = Optional(ProtobufMapVisitor {
                                rf,
                                table: child_table,
                                arena,
                            });
                            map.next_value_seed(seed)?;
                        } else {
                            let seed = Optional(ProtobufArrayfVisitor {
                                rf,
                                table: child_table,
                                arena,
                            });
                            map.next_value_seed(seed)?;
                        }
                    }
                },
                _ => match field.r#type().unwrap() {
                    Type::TYPE_BOOL => {
                        let Some(v) = map.next_value()? else {
                            continue;
                        };
                        obj.set::<bool>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_FIXED64 | Type::TYPE_UINT64 => {
                        let Some(v) = map.next_value::<Option<u64>>()? else {
                            continue;
                        };
                        obj.set::<u64>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_FIXED32 | Type::TYPE_UINT32 => {
                        let Some(v) = map.next_value::<Option<u32>>()? else {
                            continue;
                        };
                        obj.set::<u32>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_SFIXED64 | Type::TYPE_INT64 | Type::TYPE_SINT64 => {
                        let Some(v) = map.next_value::<Option<i64>>()? else {
                            continue;
                        };
                        obj.set::<i64>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_SFIXED32 | Type::TYPE_INT32 | Type::TYPE_SINT32 => {
                        let Some(v) = map.next_value::<Option<i32>>()? else {
                            continue;
                        };
                        obj.set::<i32>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_ENUM => {
                        let seed = EnumSeed {
                            descriptor: table.descriptor,
                            type_name: field.type_name(),
                        };
                        let Some(v) = map.next_value_seed(Optional(seed))? else {
                            continue;
                        };
                        obj.set::<i32>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_FLOAT => {
                        let Some(v) = map.next_value::<Option<f32>>()? else {
                            continue;
                        };
                        obj.set::<f32>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_DOUBLE => {
                        let Some(v) = map.next_value::<Option<f64>>()? else {
                            continue;
                        };
                        obj.set::<f64>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_STRING => {
                        let Some(v) = map.next_value::<Option<String>>()? else {
                            continue;
                        };
                        let s = crate::containers::String::from_str(&v, arena);
                        obj.set::<crate::containers::String>(
                            entry.offset(),
                            entry.has_bit_idx(),
                            s,
                        );
                    }
                    Type::TYPE_BYTES => {
                        let Some(v) = map.next_value::<Option<BytesBuf>>()? else {
                            continue;
                        };
                        let b = crate::containers::Bytes::from_slice(&v.0, arena);
                        obj.set::<crate::containers::Bytes>(entry.offset(), entry.has_bit_idx(), b);
                    }
                    Type::TYPE_MESSAGE | Type::TYPE_GROUP => {
                        // TODO handle null
                        let AuxTableEntry {
                            offset,
                            child_table,
                        } = table.aux_entry_decode(entry);
                        let child_table = unsafe { &*child_table };
                        let child_obj = Object::create(child_table.size as u32, arena);
                        let seed = Optional(ProtobufVisitor {
                            obj: child_obj,
                            table: child_table,
                            arena,
                        });
                        if map.next_value_seed(seed)?.is_none() {
                            continue;
                        };
                        *obj.ref_mut::<crate::base::Message>(offset) =
                            crate::base::Message(child_obj);
                    }
                },
            }
        }
        Ok(())
    }
}

struct BytesBuf(Vec<u8>);

impl<'de> serde::de::Deserialize<'de> for BytesBuf {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct BytesVisitor;

        impl<'de> serde::de::Visitor<'de> for BytesVisitor {
            type Value = BytesBuf;

            fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
                formatter.write_str("bytes")
            }

            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
                Ok(BytesBuf(v.to_vec()))
            }

            fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
                Ok(BytesBuf(v))
            }
        }

        deserializer.deserialize_bytes(BytesVisitor)
    }
}
