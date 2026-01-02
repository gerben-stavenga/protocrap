use crate::{
    Protobuf, ProtobufMut, ProtobufRef,
    base::{Message, Object},
    containers::{Bytes, String},
    google::protobuf::{
        DescriptorProto::ProtoType as DescriptorProto,
        FieldDescriptorProto::{Label, ProtoType as FieldDescriptorProto, Type},
    },
    tables::Table,
    wire,
};

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

pub fn is_repeated(field: &FieldDescriptorProto) -> bool {
    field.label().unwrap() == Label::LABEL_REPEATED
}

pub fn is_message(field: &FieldDescriptorProto) -> bool {
    matches!(
        field.r#type().unwrap(),
        Type::TYPE_MESSAGE | Type::TYPE_GROUP
    )
}

pub fn is_in_oneof(field: &FieldDescriptorProto) -> bool {
    field.has_oneof_index()
}

pub fn needs_has_bit(field: &FieldDescriptorProto) -> bool {
    !is_repeated(field) && !is_message(field) && !is_in_oneof(field)
}

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

pub fn debug_message<'msg, T: Protobuf>(
    msg: &'msg T,
    f: &mut core::fmt::Formatter<'_>,
) -> core::fmt::Result {
    let dynamic_msg = DynamicMessageRef::new(msg);
    core::fmt::Debug::fmt(&dynamic_msg, f)
}

/// Read-only view of a dynamic protobuf message.
/// Used for Debug, Serialize, encode, and field inspection.
pub struct DynamicMessageRef<'pool, 'msg> {
    pub(crate) object: &'msg Object,
    pub(crate) table: &'pool Table,
}

/// Mutable view of a dynamic protobuf message.
/// Used for deserialization and modification.
pub struct DynamicMessage<'pool, 'msg> {
    pub object: &'msg mut Object,
    pub table: &'pool Table,
}

// Deref allows DynamicMessage to use all DynamicMessageRef methods
impl<'pool, 'msg> core::ops::Deref for DynamicMessage<'pool, 'msg> {
    type Target = DynamicMessageRef<'pool, 'msg>;

    fn deref(&self) -> &Self::Target {
        // SAFETY: DynamicMessageRef has the same layout with &Object instead of &mut Object
        // and we're only providing immutable access through the Deref
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
        core::fmt::Debug::fmt(&**self, f)
    }
}

impl<'pool, 'msg> DynamicMessageRef<'pool, 'msg> {
    pub fn new<T>(msg: &'msg T) -> Self
    where
        T: crate::Protobuf,
    {
        DynamicMessageRef {
            object: crate::generated_code_only::as_object(msg),
            table: T::table(),
        }
    }

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
                    let aux = self.table.aux_entry_decode(entry);
                    let slice = self.object.get_slice::<Message>(aux.offset as usize);
                    if slice.is_empty() {
                        return None;
                    }
                    let dynamic_array = DynamicMessageArray {
                        object: slice,
                        table: unsafe { &*aux.child_table },
                    };
                    Some(Value::RepeatedMessage(dynamic_array))
                }
            }
        } else {
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
                    // For oneof message fields, check discriminant first
                    let has_bit_idx = entry.has_bit_idx();
                    if has_bit_idx & 0x80 != 0 {
                        let discriminant_word_idx = (has_bit_idx & 0x7F) as usize;
                        let discriminant = self.object.get::<u32>(discriminant_word_idx * 4);
                        if discriminant != field.number() as u32 {
                            return None;
                        }
                    }
                    let aux_entry = self.table.aux_entry_decode(entry);
                    let offset = aux_entry.offset as usize;
                    let msg = self.object.get::<Message>(offset);
                    if msg.0.is_null() {
                        return None;
                    }
                    let dynamic_msg = DynamicMessageRef {
                        object: unsafe { &*msg.0 },
                        table: unsafe { &*aux_entry.child_table },
                    };
                    return Some(Value::Message(dynamic_msg));
                }
            };
            // Check if field is set
            let has_bit_idx = entry.has_bit_idx();
            let is_set = if has_bit_idx & 0x80 != 0 {
                // Oneof field - check discriminant matches field number
                let discriminant_word_idx = (has_bit_idx & 0x7F) as usize;
                let discriminant = self.object.get::<u32>(discriminant_word_idx * 4);
                discriminant == field.number() as u32
            } else {
                debug_assert!(needs_has_bit(field));
                self.object.has_bit(has_bit_idx as u8)
            };
            if is_set {
                Some(value)
            } else {
                None
            }
        }
    }
}

impl<'pool, 'msg> DynamicMessage<'pool, 'msg> {
    pub fn new<T>(msg: &'msg mut T) -> Self
    where
        T: crate::Protobuf,
    {
        DynamicMessage {
            object: crate::generated_code_only::as_object_mut(msg),
            table: T::table(),
        }
    }

    pub fn as_ref(&self) -> &DynamicMessageRef<'pool, 'msg> {
        // SAFETY: DynamicMessage and DynamicMessageRef have the same layout
        // (one has &mut Object, the other &Object)
        unsafe {
            &*(self as *const DynamicMessage<'pool, 'msg> as *const DynamicMessageRef<'pool, 'msg>)
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
    pub object: &'msg [Message],
    pub table: &'pool Table,
}

impl<'pool, 'msg> core::fmt::Debug for DynamicMessageArray<'pool, 'msg> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list()
            .entries(self.object.iter().map(|msg| DynamicMessageRef {
                object: unsafe { &*msg.0 },
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
        let obj = self.object[index];
        DynamicMessageRef {
            object: unsafe { &*obj.0 },
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
            let obj = self.object[self.index];
            self.index += 1;
            Some(DynamicMessageRef {
                object: unsafe { &*obj.0 },
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
        match self {
            Value::Int32(v) => core::fmt::Debug::fmt(v, f),
            Value::Int64(v) => core::fmt::Debug::fmt(v, f),
            Value::UInt32(v) => core::fmt::Debug::fmt(v, f),
            Value::UInt64(v) => core::fmt::Debug::fmt(v, f),
            Value::Float(v) => core::fmt::Debug::fmt(v, f),
            Value::Double(v) => core::fmt::Debug::fmt(v, f),
            Value::Bool(v) => core::fmt::Debug::fmt(v, f),
            Value::String(v) => core::fmt::Debug::fmt(v, f),
            Value::Bytes(v) => core::fmt::Debug::fmt(v, f),
            Value::Message(v) => core::fmt::Debug::fmt(v, f),
            Value::RepeatedInt32(v) => core::fmt::Debug::fmt(v, f),
            Value::RepeatedInt64(v) => core::fmt::Debug::fmt(v, f),
            Value::RepeatedUInt32(v) => core::fmt::Debug::fmt(v, f),
            Value::RepeatedUInt64(v) => core::fmt::Debug::fmt(v, f),
            Value::RepeatedFloat(v) => core::fmt::Debug::fmt(v, f),
            Value::RepeatedDouble(v) => core::fmt::Debug::fmt(v, f),
            Value::RepeatedBool(v) => core::fmt::Debug::fmt(v, f),
            Value::RepeatedString(v) => core::fmt::Debug::fmt(v, f),
            Value::RepeatedBytes(v) => core::fmt::Debug::fmt(v, f),
            Value::RepeatedMessage(v) => core::fmt::Debug::fmt(v, f),
        }
    }
}
