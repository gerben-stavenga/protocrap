//! Runtime reflection for protobuf messages.
//!
//! This module provides dynamic access to protobuf messages without compile-time
//! knowledge of their schema. Use reflection when you need to:
//!
//! - Inspect message fields at runtime
//! - Implement generic message processing (logging, diff, transform)
//! - Work with messages whose types are only known at runtime
//!
//! # Key Types
//!
//! - [`DynamicMessageRef`]: Read-only view of a message for inspection and encoding
//! - [`DynamicMessage`]: Mutable view for decoding and modification
//! - [`Value`]: Enum representing any protobuf field value
//!
//! # Example
//!
//! ```
//! use protocrap::ProtobufRef;
//! use protocrap::google::protobuf::FileDescriptorProto;
//!
//! let file_desc = FileDescriptorProto::ProtoType::file_descriptor();
//!
//! // Access dynamically via reflection
//! let dynamic = file_desc.as_dyn();
//! for field in dynamic.descriptor().field() {
//!     if let Some(value) = dynamic.get_field(field.as_ref()) {
//!         println!("{}: {:?}", field.name(), value);
//!     }
//! }
//! ```

use crate::{
    ProtobufMut, ProtobufRef,
    base::{Message, Object},
    containers::{Bytes, String},
    google::protobuf::{
        DescriptorProto::ProtoType as DescriptorProto,
        FieldDescriptorProto::{Label, ProtoType as FieldDescriptorProto, Type},
    },
    tables::Table,
    wire,
};

#[doc(hidden)]
pub fn field_kind_tokens(field: &FieldDescriptorProto) -> wire::FieldKind {
    if field.label().unwrap() == Label::LABEL_REPEATED {
        match field.r#type().unwrap() {
            Type::TYPE_INT32 => wire::FieldKind::RepeatedInt32,
            Type::TYPE_UINT32 => wire::FieldKind::RepeatedVarint32,
            Type::TYPE_INT64 | Type::TYPE_UINT64 => wire::FieldKind::RepeatedVarint64,
            Type::TYPE_SINT32 => wire::FieldKind::RepeatedVarint32Zigzag,
            Type::TYPE_SINT64 => wire::FieldKind::RepeatedVarint64Zigzag,
            Type::TYPE_FIXED32 | Type::TYPE_SFIXED32 | Type::TYPE_FLOAT => {
                wire::FieldKind::RepeatedFixed32
            }
            Type::TYPE_FIXED64 | Type::TYPE_SFIXED64 | Type::TYPE_DOUBLE => {
                wire::FieldKind::RepeatedFixed64
            }
            Type::TYPE_BOOL => wire::FieldKind::RepeatedBool,
            Type::TYPE_STRING => wire::FieldKind::RepeatedString,
            Type::TYPE_BYTES => wire::FieldKind::RepeatedBytes,
            Type::TYPE_MESSAGE => wire::FieldKind::RepeatedMessage,
            Type::TYPE_GROUP => wire::FieldKind::RepeatedGroup,
            Type::TYPE_ENUM => wire::FieldKind::RepeatedInt32,
        }
    } else {
        match field.r#type().unwrap() {
            Type::TYPE_INT32 => wire::FieldKind::Int32,
            Type::TYPE_UINT32 => wire::FieldKind::Varint32,
            Type::TYPE_INT64 | Type::TYPE_UINT64 => wire::FieldKind::Varint64,
            Type::TYPE_SINT32 => wire::FieldKind::Varint32Zigzag,
            Type::TYPE_SINT64 => wire::FieldKind::Varint64Zigzag,
            Type::TYPE_FIXED32 | Type::TYPE_SFIXED32 | Type::TYPE_FLOAT => wire::FieldKind::Fixed32,
            Type::TYPE_FIXED64 | Type::TYPE_SFIXED64 | Type::TYPE_DOUBLE => {
                wire::FieldKind::Fixed64
            }
            Type::TYPE_BOOL => wire::FieldKind::Bool,
            Type::TYPE_STRING => wire::FieldKind::String,
            Type::TYPE_BYTES => wire::FieldKind::Bytes,
            Type::TYPE_MESSAGE => wire::FieldKind::Message,
            Type::TYPE_GROUP => wire::FieldKind::Group,
            Type::TYPE_ENUM => wire::FieldKind::Int32,
        }
    }
}

#[doc(hidden)]
pub fn calculate_tag_with_syntax(field: &FieldDescriptorProto, syntax: Option<&str>) -> u32 {
    let is_repeated = field.label().unwrap() == Label::LABEL_REPEATED;

    // Determine if packed encoding should be used (only for repeated primitive fields)
    let is_packed = is_repeated
        && if let Some(opts) = field.options() {
            if opts.has_packed() {
                // Explicit packed option takes precedence
                opts.packed()
            } else {
                // No explicit option - use default based on syntax
                // Proto3 defaults to packed for repeated primitive fields
                // Proto2 defaults to unpacked
                syntax == Some("proto3")
            }
        } else {
            // No options at all - use syntax-based default
            syntax == Some("proto3")
        };

    let wire_type = match field.r#type().unwrap() {
        Type::TYPE_INT32
        | Type::TYPE_INT64
        | Type::TYPE_UINT32
        | Type::TYPE_UINT64
        | Type::TYPE_SINT32
        | Type::TYPE_SINT64
        | Type::TYPE_BOOL
        | Type::TYPE_ENUM => {
            if is_packed { 2 } else { 0 } // Packed encoding for repeated primitives
        }
        Type::TYPE_FIXED64 | Type::TYPE_SFIXED64 | Type::TYPE_DOUBLE => {
            if is_packed { 2 } else { 1 } // Packed encoding for repeated fixed64
        }
        Type::TYPE_STRING | Type::TYPE_BYTES | Type::TYPE_MESSAGE => 2,
        Type::TYPE_GROUP => 3,
        Type::TYPE_FIXED32 | Type::TYPE_SFIXED32 | Type::TYPE_FLOAT => {
            if is_packed { 2 } else { 5 } // Packed encoding for repeated fixed32
        }
    };

    (field.number() as u32) << 3 | wire_type
}

#[doc(hidden)]
pub fn is_repeated(field: &FieldDescriptorProto) -> bool {
    field.label().unwrap() == Label::LABEL_REPEATED
}

#[doc(hidden)]
pub fn is_message(field: &FieldDescriptorProto) -> bool {
    matches!(
        field.r#type().unwrap(),
        Type::TYPE_MESSAGE | Type::TYPE_GROUP
    )
}

#[doc(hidden)]
pub fn is_in_oneof(field: &FieldDescriptorProto) -> bool {
    field.has_oneof_index()
}

#[doc(hidden)]
pub fn needs_has_bit(field: &FieldDescriptorProto) -> bool {
    !is_repeated(field) && !is_message(field) && !is_in_oneof(field)
}

#[doc(hidden)]
pub fn default_value<'a>(field: &'a FieldDescriptorProto) -> Option<Value<'a, 'a>> {
    use Type::*;

    match field.r#type()? {
        TYPE_BOOL => Some(Value::Bool(false)),
        TYPE_INT32 | TYPE_UINT32 | TYPE_ENUM => Some(Value::Int32(0)),
        TYPE_INT64 | TYPE_UINT64 => Some(Value::Int64(0)),
        TYPE_SINT32 => Some(Value::Int32(0)),
        TYPE_SINT64 => Some(Value::Int64(0)),
        TYPE_FIXED32 | TYPE_SFIXED32 | TYPE_FLOAT => Some(Value::Int32(0)),
        TYPE_FIXED64 | TYPE_SFIXED64 | TYPE_DOUBLE => Some(Value::Int64(0)),
        TYPE_STRING => Some(Value::String("")),
        TYPE_BYTES => Some(Value::Bytes(&[])),
        TYPE_MESSAGE | TYPE_GROUP => None,
    }
}

/// Read-only view of a protobuf message for dynamic inspection.
///
/// Provides access to message fields without knowing the concrete type at compile time.
/// Obtained via [`ProtobufRef::as_dyn()`](crate::ProtobufRef::as_dyn) or from a
/// [`DescriptorPool`](crate::descriptor_pool::DescriptorPool).
///
/// # Lifetimes
///
/// - `'pool`: Lifetime of the descriptor/table data (often `'static` for generated types)
/// - `'msg`: Lifetime of the message data being inspected
pub struct DynamicMessageRef<'pool, 'msg> {
    pub(crate) object: &'msg Object,
    pub(crate) table: &'pool Table,
}

/// Mutable view of a protobuf message for dynamic modification.
///
/// Extends [`DynamicMessageRef`] with mutation capabilities. Use for decoding
/// messages or setting field values dynamically.
///
/// Implements `Deref<Target = DynamicMessageRef>` so all read methods are available.
pub struct DynamicMessage<'pool, 'msg> {
    pub(crate) object: &'msg mut Object,
    pub(crate) table: &'pool Table,
}

impl<'pool, 'msg> core::ops::Deref for DynamicMessage<'pool, 'msg> {
    type Target = DynamicMessageRef<'pool, 'msg>;

    fn deref(&self) -> &Self::Target {
        // Safety: This is unsound but works.
        unsafe {
            &*(self as *const DynamicMessage<'pool, 'msg> as *const DynamicMessageRef<'pool, 'msg>)
        }
    }
}

impl<'pool, 'msg> core::fmt::Debug for DynamicMessageRef<'pool, 'msg> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut debug_struct = f.debug_struct(self.table.descriptor.name());
        for field in self.table.descriptor.field() {
            if let Some(value) = self.get_field(field) {
                debug_struct.field(field.name(), &value);
            }
        }
        debug_struct.finish()
    }
}

impl<'pool, 'msg> core::fmt::Debug for DynamicMessage<'pool, 'msg> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Delegate to DynamicMessageRef's Debug impl via Deref
        self.as_ref().fmt(f)
    }
}

impl<'pool, 'msg> DynamicMessageRef<'pool, 'msg> {
    pub fn descriptor(&self) -> &'pool DescriptorProto {
        self.table.descriptor
    }

    pub fn find_field_descriptor(&self, field_name: &str) -> Option<&'pool FieldDescriptorProto> {
        self.table
            .descriptor
            .field()
            .iter()
            .find(|f| f.name() == field_name)
            .map(|f| &**f)
    }

    pub fn find_field_descriptor_by_number(
        &self,
        field_number: i32,
    ) -> Option<&'pool FieldDescriptorProto> {
        self.table
            .descriptor
            .field()
            .iter()
            .find(|f| f.number() == field_number)
            .map(|f| &**f)
    }

    pub fn get_field(&self, field: &'pool FieldDescriptorProto) -> Option<Value<'pool, 'msg>> {
        let entry = self.table.entry(field.number() as u32).unwrap();
        if field.label().unwrap() == Label::LABEL_REPEATED {
            // Repeated field
            match field.r#type().unwrap() {
                Type::TYPE_INT32 | Type::TYPE_SINT32 | Type::TYPE_SFIXED32 | Type::TYPE_ENUM => {
                    let slice = self.object.get_slice::<i32>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedInt32(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_INT64 | Type::TYPE_SINT64 | Type::TYPE_SFIXED64 => {
                    let slice = self.object.get_slice::<i64>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedInt64(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_UINT32 | Type::TYPE_FIXED32 => {
                    let slice = self.object.get_slice::<u32>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedUInt32(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_UINT64 | Type::TYPE_FIXED64 => {
                    let slice = self.object.get_slice::<u64>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedUInt64(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_FLOAT => {
                    let slice = self.object.get_slice::<f32>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedFloat(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_DOUBLE => {
                    let slice = self.object.get_slice::<f64>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedDouble(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_BOOL => {
                    let slice = self.object.get_slice::<bool>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedBool(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_STRING => {
                    let slice = self.object.get_slice::<String>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedString(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_BYTES => {
                    let slice = self.object.get_slice::<Bytes>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedBytes(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_MESSAGE | Type::TYPE_GROUP => {
                    let (offset, child_table) = self.table.aux_entry_decode(entry);
                    let slice = self.object.get_slice::<Message>(offset as usize);
                    if slice.is_empty() {
                        return None;
                    }
                    let dynamic_array = DynamicMessageArray {
                        object: slice,
                        table: child_table,
                    };
                    Some(Value::RepeatedMessage(dynamic_array))
                }
            }
        } else {
            // Check if field is set BEFORE accessing field data
            // This is critical for oneofs where unset variants may contain garbage
            let has_bit_idx = entry.has_bit_idx();
            if has_bit_idx & 0x80 != 0 {
                // Oneof field - check discriminant matches field number
                let discriminant_word_idx = (has_bit_idx & 0x7F) as usize;
                let discriminant = self.object.get::<u32>(discriminant_word_idx * 4);
                if discriminant != field.number() as u32 {
                    return None;
                }
            } else if needs_has_bit(field) {
                // Field has a has_bit - check it
                if !self.object.has_bit(has_bit_idx as u8) {
                    return None;
                }
            }
            // Proto3 scalar fields without has_bits are always "present" (may be default)

            let value = match field.r#type().unwrap() {
                Type::TYPE_INT32 | Type::TYPE_SINT32 | Type::TYPE_SFIXED32 | Type::TYPE_ENUM => {
                    Value::Int32(self.object.get(entry.offset() as usize))
                }
                Type::TYPE_INT64 | Type::TYPE_SINT64 | Type::TYPE_SFIXED64 => {
                    Value::Int64(self.object.get(entry.offset() as usize))
                }
                Type::TYPE_UINT32 | Type::TYPE_FIXED32 => {
                    Value::UInt32(self.object.get(entry.offset() as usize))
                }
                Type::TYPE_UINT64 | Type::TYPE_FIXED64 => {
                    Value::UInt64(self.object.get(entry.offset() as usize))
                }
                Type::TYPE_FLOAT => Value::Float(self.object.get(entry.offset() as usize)),
                Type::TYPE_DOUBLE => Value::Double(self.object.get(entry.offset() as usize)),
                Type::TYPE_BOOL => Value::Bool(self.object.get(entry.offset() as usize)),
                Type::TYPE_STRING => Value::String(
                    self.object
                        .ref_at::<crate::containers::String>(entry.offset() as usize)
                        .as_str(),
                ),
                Type::TYPE_BYTES => {
                    Value::Bytes(self.object.get_slice::<u8>(entry.offset() as usize))
                }
                Type::TYPE_MESSAGE | Type::TYPE_GROUP => {
                    let (offset, child_table) = self.table.aux_entry_decode(entry);
                    let msg = self.object.ref_at::<Message>(offset as usize);
                    if msg.is_null() {
                        return None;
                    }
                    let dynamic_msg = DynamicMessageRef {
                        object: msg.as_ref(),
                        table: child_table,
                    };
                    return Some(Value::Message(dynamic_msg));
                }
            };
            Some(value)
        }
    }
}

impl<'pool, 'msg> DynamicMessage<'pool, 'msg> {
    pub fn as_ref<'a>(&'a self) -> DynamicMessageRef<'pool, 'a> {
        DynamicMessageRef {
            object: self.object,
            table: self.table,
        }
    }

    /// Zeroes all fields of this message.
    pub fn clear(&mut self) {
        unsafe {
            core::ptr::write_bytes(
                self.object as *mut Object as *mut u8,
                0,
                self.table.size as usize,
            );
        }
    }
}

impl<'pool, 'msg> From<DynamicMessage<'pool, 'msg>> for DynamicMessageRef<'pool, 'msg> {
    fn from(dynamic: DynamicMessage<'pool, 'msg>) -> Self {
        DynamicMessageRef {
            object: dynamic.object,
            table: dynamic.table,
        }
    }
}

// ProtobufRef implementation for DynamicMessageRef
impl<'pool, 'msg> ProtobufRef<'pool> for DynamicMessageRef<'pool, 'msg> {
    fn as_dyn<'a>(&'a self) -> DynamicMessageRef<'pool, 'a> {
        DynamicMessageRef {
            object: self.object,
            table: self.table,
        }
    }
}

impl<'pool, 'msg> ProtobufRef<'pool> for DynamicMessage<'pool, 'msg> {
    fn as_dyn<'a>(&'a self) -> DynamicMessageRef<'pool, 'a> {
        DynamicMessageRef {
            object: self.object,
            table: self.table,
        }
    }
}

// ProtobufMut implementation for DynamicMessage
impl<'pool, 'msg> ProtobufMut<'pool> for DynamicMessage<'pool, 'msg> {
    fn as_dyn_mut<'a>(&'a mut self) -> DynamicMessage<'pool, 'a> {
        DynamicMessage {
            object: self.object,
            table: self.table,
        }
    }
}

pub struct DynamicMessageArray<'pool, 'msg> {
    pub(crate) object: &'msg [Message],
    pub(crate) table: &'pool Table,
}

impl<'pool, 'msg> core::fmt::Debug for DynamicMessageArray<'pool, 'msg> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list()
            .entries(self.object.iter().map(|msg| DynamicMessageRef {
                object: msg.as_ref(),
                table: self.table,
            }))
            .finish()
    }
}

impl<'pool, 'msg> DynamicMessageArray<'pool, 'msg> {
    pub fn len(&self) -> usize {
        self.object.len()
    }

    pub fn is_empty(&self) -> bool {
        self.object.is_empty()
    }

    pub fn get(&self, index: usize) -> DynamicMessageRef<'pool, 'msg> {
        let obj = &self.object[index];
        DynamicMessageRef {
            object: obj.as_ref(),
            table: self.table,
        }
    }

    pub fn iter<'a>(&'a self) -> DynamicMessageArrayIter<'pool, 'a>
    where
        'msg: 'a,
    {
        DynamicMessageArrayIter {
            object: self.object,
            table: self.table,
            index: 0,
        }
    }
}

// Iterator struct - holds the data directly instead of borrowing the array
pub struct DynamicMessageArrayIter<'pool, 'a> {
    object: &'a [Message],
    table: &'pool Table,
    index: usize,
}

impl<'pool, 'a> Iterator for DynamicMessageArrayIter<'pool, 'a> {
    type Item = DynamicMessageRef<'pool, 'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.object.len() {
            let obj = &self.object[self.index];
            self.index += 1;
            Some(DynamicMessageRef {
                object: obj.as_ref(),
                table: self.table,
            })
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.object.len() - self.index;
        (remaining, Some(remaining))
    }
}

impl<'pool, 'a> ExactSizeIterator for DynamicMessageArrayIter<'pool, 'a> {
    fn len(&self) -> usize {
        self.object.len() - self.index
    }
}

// IntoIterator for easy use with for loops
impl<'pool, 'msg> IntoIterator for &DynamicMessageArray<'pool, 'msg> {
    type Item = DynamicMessageRef<'pool, 'msg>;
    type IntoIter = DynamicMessageArrayIter<'pool, 'msg>;

    fn into_iter(self) -> Self::IntoIter {
        DynamicMessageArrayIter {
            object: self.object,
            table: self.table,
            index: 0,
        }
    }
}

/// A dynamically-typed protobuf field value.
///
/// Returned by [`DynamicMessageRef::get_field()`] to represent any field value
/// without compile-time type knowledge. Variants cover all protobuf scalar types,
/// messages, and their repeated versions.
///
/// # Example
///
/// ```
/// use protocrap::ProtobufRef;
/// use protocrap::reflection::Value;
/// use protocrap::google::protobuf::FileDescriptorProto;
///
/// let file_desc = FileDescriptorProto::ProtoType::file_descriptor();
/// let dynamic = file_desc.as_dyn();
///
/// // Find and inspect the "name" field
/// if let Some(field) = dynamic.find_field_descriptor("name") {
///     match dynamic.get_field(field) {
///         Some(Value::String(s)) => println!("name = {}", s),
///         _ => println!("name not set"),
///     }
/// }
/// ```
pub enum Value<'pool, 'msg> {
    Int32(i32),
    Int64(i64),
    UInt32(u32),
    UInt64(u64),
    Float(f32),
    Double(f64),
    Bool(bool),
    String(&'msg str),
    Bytes(&'msg [u8]),
    Message(DynamicMessageRef<'pool, 'msg>),
    RepeatedInt32(&'msg [i32]),
    RepeatedInt64(&'msg [i64]),
    RepeatedUInt32(&'msg [u32]),
    RepeatedUInt64(&'msg [u64]),
    RepeatedFloat(&'msg [f32]),
    RepeatedDouble(&'msg [f64]),
    RepeatedBool(&'msg [bool]),
    RepeatedString(&'msg [String]),
    RepeatedBytes(&'msg [Bytes]),
    RepeatedMessage(DynamicMessageArray<'pool, 'msg>),
}

impl core::fmt::Debug for Value<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Value::Int32(v) => v.fmt(f),
            Value::Int64(v) => v.fmt(f),
            Value::UInt32(v) => v.fmt(f),
            Value::UInt64(v) => v.fmt(f),
            Value::Float(v) => v.fmt(f),
            Value::Double(v) => v.fmt(f),
            Value::Bool(v) => v.fmt(f),
            Value::String(v) => v.fmt(f),
            Value::Bytes(v) => v.fmt(f),
            Value::Message(ref v) => v.fmt(f),
            Value::RepeatedInt32(v) => v.fmt(f),
            Value::RepeatedInt64(v) => v.fmt(f),
            Value::RepeatedUInt32(v) => v.fmt(f),
            Value::RepeatedUInt64(v) => v.fmt(f),
            Value::RepeatedFloat(v) => v.fmt(f),
            Value::RepeatedDouble(v) => v.fmt(f),
            Value::RepeatedBool(v) => v.fmt(f),
            Value::RepeatedString(v) => v.fmt(f),
            Value::RepeatedBytes(v) => v.fmt(f),
            Value::RepeatedMessage(ref v) => v.fmt(f),
        }
    }
}
