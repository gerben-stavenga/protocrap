use crate::{
    arena::Arena,
    base::{Message, Object},
    google::protobuf::{
        DescriptorProto::ProtoType as DescriptorProto,
        FieldDescriptorProto::ProtoType as FieldDescriptorProto,
        FileDescriptorProto::ProtoType as FileDescriptorProto,
    },
    reflection::{
        field_kind_tokens, is_in_oneof, is_message, is_repeated, needs_has_bit,
        DynamicMessage,
    },
    tables::Table,
};

pub struct DescriptorPool<'alloc> {
    pub arena: Arena<'alloc>,
    tables: std::collections::HashMap<std::string::String, &'alloc mut Table>,
}

impl<'alloc> DescriptorPool<'alloc> {
    pub fn new(alloc: &'alloc dyn crate::Allocator) -> Self {
        DescriptorPool {
            arena: Arena::new(alloc),
            tables: std::collections::HashMap::new(),
        }
    }

    /// Strip leading dot from type name (protobuf returns ".package.Type", we store "package.Type")
    fn normalize_type_name(type_name: &str) -> &str {
        type_name.strip_prefix('.').unwrap_or(type_name)
    }

    /// Add a FileDescriptorProto to the pool
    pub fn add_file(&mut self, file: &'alloc FileDescriptorProto) {
        let package = if file.has_package() {
            file.package()
        } else {
            ""
        };

        // First pass: build all tables (child table pointers may be null)
        for message in file.message_type() {
            let full_name = if package.is_empty() {
                message.name().to_string()
            } else {
                format!("{}.{}", package, message.name())
            };
            self.add_message(message, &full_name, file.get_syntax());
        }

        // Second pass: patch aux entries with correct child table pointers
        for message in file.message_type() {
            let full_name = if package.is_empty() {
                message.name().to_string()
            } else {
                format!("{}.{}", package, message.name())
            };
            self.patch_message_aux_entries(&full_name);
        }
    }

    fn add_message(
        &mut self,
        message: &'alloc DescriptorProto,
        full_name: &str,
        syntax: Option<&str>,
    ) {
        // Build table from descriptor
        let table = self.build_table_from_descriptor(message, syntax);
        self.tables.insert(full_name.to_string(), table);

        // Add nested types
        for nested in message.nested_type() {
            let nested_full_name = format!("{}.{}", full_name, nested.name());
            self.add_message(nested, &nested_full_name, syntax);
        }
    }

    fn patch_message_aux_entries(&mut self, full_name: &str) {
        use crate::tables::AuxTableEntry;

        let table = match self.tables.get_mut(full_name) {
            Some(t) => &mut **t,
            None => return,
        };

        let descriptor = table.descriptor;

        // Count aux entries (message fields)
        let num_aux_entries = descriptor.field().iter().filter(|f| is_message(f)).count();
        if num_aux_entries == 0 {
            return;
        }

        // Get aux entry pointer - must use same Layout::extend logic as build_table_from_descriptor
        unsafe {
            // Recalculate aux offset using Layout::extend (accounts for padding)
            let table_layout = core::alloc::Layout::new::<Table>();
            let (_, aux_offset_from_table) = table_layout
                .extend(
                    core::alloc::Layout::array::<crate::decoding::TableEntry>(
                        table.num_decode_entries as usize,
                    )
                    .unwrap(),
                )
                .unwrap()
                .0
                .extend(core::alloc::Layout::array::<AuxTableEntry>(num_aux_entries).unwrap())
                .unwrap();

            let aux_ptr =
                (table as *mut Table as *mut u8).add(aux_offset_from_table) as *mut AuxTableEntry;

            // Patch each aux entry with the correct child table pointer
            let mut aux_idx = 0;
            for field in descriptor.field() {
                if is_message(field) {
                    let child_type_name = Self::normalize_type_name(field.type_name());
                    let child_table_ptr = self
                        .tables
                        .get_mut(child_type_name)
                        .map(|t| *t as *mut Table)
                        .unwrap_or(core::ptr::null_mut());

                    if !child_table_ptr.is_null() {
                        (*aux_ptr.add(aux_idx)).child_table = child_table_ptr;
                    }
                    aux_idx += 1;
                }
            }
        }

        // Patch nested types
        for nested in descriptor.nested_type() {
            let nested_full_name = format!("{}.{}", full_name, nested.name());
            self.patch_message_aux_entries(&nested_full_name);
        }
    }

    /// Get a table by message type name
    pub fn get_table(&self, message_type: &str) -> Option<&Table> {
        self.tables.get(message_type).map(|t| &**t)
    }

    pub fn create_message<'pool, 'msg>(
        &'pool self,
        message_type: &str,
        arena: &mut Arena<'msg>,
    ) -> Result<DynamicMessage<'pool, 'msg>, crate::Error> {
        let table = &**self
            .tables
            .get(message_type)
            .ok_or(crate::Error::MessageNotFound)?;

        // Allocate object with proper alignment (8 bytes for all protobuf types)
        let layout = core::alloc::Layout::from_size_align(table.size as usize, 8).unwrap();
        let ptr = arena.alloc_raw(layout).as_ptr() as *mut Object;
        assert!((ptr as usize) & 7 == 0);
        let object = unsafe {
            // Zero-initialize the object
            core::ptr::write_bytes(ptr as *mut u8, 0, table.size as usize);
            &mut *ptr
        };

        Ok(DynamicMessage { object, table })
    }

    /// Create a DynamicMessage by decoding bytes with the given message type
    pub fn decode_message<'pool, 'msg>(
        &'pool self,
        message_type: &str,
        bytes: &[u8],
        arena: &'msg mut Arena,
    ) -> Result<DynamicMessage<'pool, 'msg>, crate::Error> {
        let table = &**self
            .tables
            .get(message_type)
            .ok_or(crate::Error::MessageNotFound)?;

        // Allocate object with proper alignment (8 bytes for all protobuf types)
        let layout = core::alloc::Layout::from_size_align(table.size as usize, 8).unwrap();
        let ptr = arena.alloc_raw(layout).as_ptr() as *mut Object;
        assert!((ptr as usize) & 7 == 0);
        let object = unsafe {
            // Zero-initialize the object
            core::ptr::write_bytes(ptr as *mut u8, 0, table.size as usize);
            &mut *ptr
        };

        // Decode
        self.decode_into(object, table, bytes, arena)?;

        Ok(DynamicMessage { object, table })
    }

    // TODO: improve lifetime annotations
    #[allow(clippy::mut_from_ref)]
    fn build_table_from_descriptor(
        &mut self,
        descriptor: &'alloc DescriptorProto,
        syntax: Option<&str>,
    ) -> &'alloc mut Table {
        use crate::{
            decoding, encoding,
            reflection::calculate_tag_with_syntax,
            tables::AuxTableEntry,
        };

        // Calculate sizes
        let num_fields = descriptor.field().len();
        let num_has_bits = descriptor
            .field()
            .iter()
            .filter(|f| needs_has_bit(f))
            .count();
        let has_bits_words = num_has_bits.div_ceil(32);
        let oneof_count = descriptor.oneof_decl().len();
        let metadata_words = has_bits_words + oneof_count;
        let metadata_size = (metadata_words * 4) as u32;

        // Calculate max field number for sparse decode table
        let max_field_number = descriptor
            .field()
            .iter()
            .map(|f| f.number())
            .max()
            .unwrap_or(0);

        if max_field_number > 2047 {
            panic!("Field numbers > 2047 not supported yet");
        }

        let num_decode_entries = (max_field_number + 1) as usize;

        // Group fields by oneof_index and calculate union sizes
        let mut oneof_sizes: std::vec::Vec<(usize, usize)> = vec![(0, 1); oneof_count]; // (size, align)
        for field in descriptor.field() {
            if is_in_oneof(field) {
                let oneof_idx = field.oneof_index() as usize;
                let field_size = self.field_size(field) as usize;
                let field_align = self.field_align(field) as usize;
                if field_size > oneof_sizes[oneof_idx].0 {
                    oneof_sizes[oneof_idx].0 = field_size;
                }
                if field_align > oneof_sizes[oneof_idx].1 {
                    oneof_sizes[oneof_idx].1 = field_align;
                }
            }
        }

        // Calculate field offsets and total size using Layout::extend for proper padding
        // Start with metadata layout (always u32 array, so alignment is 4)
        let mut layout =
            core::alloc::Layout::from_size_align(metadata_size as usize, 4).unwrap();

        // First pass: calculate offsets for regular fields (not in oneof)
        // Store in a map by field number
        let mut regular_field_offsets = std::collections::HashMap::<i32, u32>::new();
        for field in descriptor.field() {
            if is_in_oneof(field) {
                continue; // Skip oneof fields, handled separately
            }
            let field_size = self.field_size(field);
            let field_align = self.field_align(field);
            let field_layout =
                core::alloc::Layout::from_size_align(field_size as usize, field_align as usize)
                    .unwrap();

            let (new_layout, offset) = layout.extend(field_layout).unwrap();
            regular_field_offsets.insert(field.number(), offset as u32);
            layout = new_layout;
        }

        // Then add unions for each oneof
        let mut oneof_offsets = std::vec::Vec::new();
        for (oneof_idx, &(size, align)) in oneof_sizes.iter().enumerate() {
            if size > 0 {
                let union_layout = core::alloc::Layout::from_size_align(size, align).unwrap();
                let (new_layout, offset) = layout.extend(union_layout).unwrap();
                oneof_offsets.push((oneof_idx, offset as u32));
                layout = new_layout;
            }
        }

        // Build field_offsets in proto definition order (matching codegen)
        let mut field_offsets = std::vec::Vec::new();
        for field in descriptor.field() {
            let offset = if is_in_oneof(field) {
                let oneof_idx = field.oneof_index() as usize;
                oneof_offsets
                    .iter()
                    .find(|(idx, _)| *idx == oneof_idx)
                    .map(|(_, off)| *off)
                    .unwrap_or(0)
            } else {
                regular_field_offsets[&field.number()]
            };
            field_offsets.push((*field, offset));
        }

        // Pad to struct alignment
        let layout = layout.pad_to_align();
        let total_size = layout.size() as u32;

        // Count message fields for aux entries
        let num_aux_entries = descriptor
            .field()
            .iter()
            .filter(|f| is_message(&**f))
            .count();

        // Allocate table with entries - use Layout::extend to handle padding correctly
        let encode_layout = core::alloc::Layout::array::<encoding::TableEntry>(num_fields).unwrap();
        let (layout, table_offset) = encode_layout
            .extend(core::alloc::Layout::new::<Table>())
            .unwrap();
        let (layout, decode_offset) = layout
            .extend(core::alloc::Layout::array::<decoding::TableEntry>(num_decode_entries).unwrap())
            .unwrap();
        let (layout, aux_offset) = layout
            .extend(core::alloc::Layout::array::<AuxTableEntry>(num_aux_entries).unwrap())
            .unwrap();

        let base_ptr = self.arena.alloc_raw(layout).as_ptr();
        let encode_ptr = base_ptr as *mut encoding::TableEntry;
        let table_ptr = unsafe { base_ptr.add(table_offset) as *mut Table };
        let decode_ptr = unsafe { base_ptr.add(decode_offset) as *mut decoding::TableEntry };
        let aux_ptr = unsafe { base_ptr.add(aux_offset) as *mut AuxTableEntry };

        unsafe {
            // Initialize Table header
            (*table_ptr).num_encode_entries = num_fields as u16;
            (*table_ptr).num_decode_entries = num_decode_entries as u16;
            (*table_ptr).size = total_size as u16;
            // SAFETY: descriptor lives in arena with 'alloc lifetime, which outlives the table usage
            (*table_ptr).descriptor = core::mem::transmute::<
                &'alloc DescriptorProto,
                &'static DescriptorProto,
            >(descriptor);

            // Build aux index map for message fields and has_bit index map
            let mut aux_index_map = std::collections::HashMap::<i32, usize>::new();
            let mut has_bit_index_map = std::collections::HashMap::<i32, u32>::new();
            let mut aux_idx = 0;
            let mut has_bit_idx = 0u32;
            for field in descriptor.field() {
                if is_message(field) {
                    aux_index_map.insert(field.number(), aux_idx);
                    aux_idx += 1;
                }
                if needs_has_bit(field) {
                    has_bit_index_map.insert(field.number(), has_bit_idx);
                    has_bit_idx += 1;
                }
            }

            // Build encode entries
            let mut has_bit_idx = 0u8;
            for (i, &(field, offset)) in field_offsets.iter().enumerate() {
                let has_bit = if is_in_oneof(&field) {
                    // Oneof field: has_bit = 0x80 | discriminant_word_idx
                    let oneof_idx = field.oneof_index() as usize;
                    (0x80 | (has_bits_words + oneof_idx)) as u8
                } else if needs_has_bit(&field) {
                    let bit = has_bit_idx;
                    has_bit_idx += 1;
                    bit
                } else {
                    0
                };

                let entry_offset = if is_message(&field) {
                    // For message fields, offset points to aux entry
                    let aux_index = aux_index_map[&field.number()];
                    let aux_offset =
                        (aux_ptr as usize) + aux_index * core::mem::size_of::<AuxTableEntry>();
                    let table_addr = table_ptr as usize;
                    (aux_offset - table_addr) as u16
                } else {
                    offset as u16
                };

                encode_ptr.add(i).write(encoding::TableEntry {
                    has_bit,
                    kind: field_kind_tokens(&field),
                    offset: entry_offset,
                    encoded_tag: calculate_tag_with_syntax(&field, syntax),
                });
            }

            // Build decode entries - sparse array indexed by field number
            for field_number in 0..=max_field_number {
                if let Some(field) = descriptor
                    .field()
                    .iter()
                    .find(|f| f.number() == field_number)
                {
                    let offset = field_offsets
                        .iter()
                        .find(|(f, _)| f.number() == field_number)
                        .map(|(_, o)| *o)
                        .unwrap_or(0);

                    // Check oneof first (applies to all field types including message)
                    let entry = if is_in_oneof(&**field) {
                        // Oneof field: has_bit = 0x80 | discriminant_word_idx
                        let oneof_idx = field.oneof_index() as usize;
                        let has_bit = (0x80 | (has_bits_words + oneof_idx)) as u32;

                        if is_message(&**field) {
                            // Oneof message field - offset points to aux entry
                            let aux_index = aux_index_map[&field_number];
                            let aux_offset =
                                (aux_ptr as usize) + aux_index * core::mem::size_of::<AuxTableEntry>();
                            let table_addr = table_ptr as usize;
                            decoding::TableEntry::new(
                                field_kind_tokens(field),
                                has_bit,
                                aux_offset - table_addr,
                            )
                        } else {
                            decoding::TableEntry::new(
                                field_kind_tokens(field),
                                has_bit,
                                offset as usize,
                            )
                        }
                    } else if is_message(&**field) {
                        // Regular message field - offset points to aux entry
                        let aux_index = aux_index_map[&field_number];
                        let aux_offset =
                            (aux_ptr as usize) + aux_index * core::mem::size_of::<AuxTableEntry>();
                        let table_addr = table_ptr as usize;
                        decoding::TableEntry::new(
                            field_kind_tokens(field),
                            0, // has_bit not used for message fields
                            aux_offset - table_addr,
                        )
                    } else {
                        let has_bit = if needs_has_bit(&**field) {
                            has_bit_index_map[&field_number]
                        } else {
                            0
                        };
                        decoding::TableEntry::new(
                            field_kind_tokens(field),
                            has_bit,
                            offset as usize,
                        )
                    };
                    decode_ptr.add(field_number as usize).write(entry);
                } else {
                    // Empty entry for unused field number
                    decode_ptr
                        .add(field_number as usize)
                        .write(decoding::TableEntry(0));
                }
            }

            // Build aux entries for message fields
            for (aux_index, &(field, offset)) in field_offsets
                .iter()
                .filter(|(f, _)| is_message(&**f))
                .enumerate()
            {
                let child_type_name = Self::normalize_type_name(field.type_name());
                let child_table_ptr = self
                    .tables
                    .get_mut(child_type_name)
                    .map(|t| *t as *mut Table)
                    .unwrap_or(core::ptr::null_mut());

                aux_ptr.add(aux_index).write(AuxTableEntry {
                    offset,
                    child_table: child_table_ptr,
                });
            }

            &mut *table_ptr
        }
    }

    fn field_size(&self, field: &FieldDescriptorProto) -> u32 {
        use crate::google::protobuf::FieldDescriptorProto::Type::*;

        if is_repeated(field) {
            return core::mem::size_of::<crate::containers::RepeatedField<u8>>() as u32;
        }

        match field.r#type().unwrap() {
            TYPE_BOOL => 1,
            TYPE_INT32 | TYPE_UINT32 | TYPE_SINT32 | TYPE_FIXED32 | TYPE_SFIXED32 | TYPE_FLOAT
            | TYPE_ENUM => 4,
            TYPE_INT64 | TYPE_UINT64 | TYPE_SINT64 | TYPE_FIXED64 | TYPE_SFIXED64 | TYPE_DOUBLE => {
                8
            }
            TYPE_STRING | TYPE_BYTES => core::mem::size_of::<crate::containers::String>() as u32,
            TYPE_MESSAGE | TYPE_GROUP => core::mem::size_of::<Message>() as u32,
        }
    }

    fn field_align(&self, field: &FieldDescriptorProto) -> u32 {
        use crate::google::protobuf::FieldDescriptorProto::Type::*;

        if is_repeated(field) {
            return core::mem::align_of::<crate::containers::RepeatedField<u8>>() as u32;
        }

        match field.r#type().unwrap() {
            TYPE_BOOL => 1,
            TYPE_INT32 | TYPE_UINT32 | TYPE_SINT32 | TYPE_FIXED32 | TYPE_SFIXED32 | TYPE_FLOAT
            | TYPE_ENUM => 4,
            TYPE_INT64 | TYPE_UINT64 | TYPE_SINT64 | TYPE_FIXED64 | TYPE_SFIXED64 | TYPE_DOUBLE => {
                8
            }
            TYPE_STRING | TYPE_BYTES => core::mem::align_of::<crate::containers::String>() as u32,
            TYPE_MESSAGE | TYPE_GROUP => core::mem::align_of::<Message>() as u32,
        }
    }

    fn decode_into(
        &self,
        object: &mut Object,
        table: &Table,
        bytes: &[u8],
        arena: &mut Arena,
    ) -> Result<(), crate::Error> {
        use crate::decoding::ResumeableDecode;

        let mut decoder = ResumeableDecode::<32>::new_from_table(object, table, isize::MAX);
        if !decoder.resume(bytes, arena) {
            return Err(crate::Error::InvalidData);
        }
        if !decoder.finish(arena) {
            return Err(crate::Error::InvalidData);
        }
        Ok(())
    }
}

pub mod test_util {
    use crate::tables::Table;
    use std::collections::HashSet;

    pub fn compare_tables_rec(
        static_table: &Table,
        dynamic_table: &Table,
        seen: &mut HashSet<*const Table>,
    ) {
        let type_name = dynamic_table.descriptor.name();
        if !seen.insert(dynamic_table as *const Table) {
            return;
        }

        assert_eq!(
            dynamic_table.size, static_table.size,
            "{}: size mismatch", type_name
        );
        assert_eq!(
            dynamic_table.num_encode_entries, static_table.num_encode_entries,
            "{}: num_encode_entries mismatch", type_name
        );
        assert_eq!(
            dynamic_table.num_decode_entries, static_table.num_decode_entries,
            "{}: num_decode_entries mismatch", type_name
        );

        let dynamic_encode = dynamic_table.encode_entries();
        let static_encode = static_table.encode_entries();

        let mut aux_offsets = Vec::new();
        for (i, (dyn_entry, static_entry)) in
            dynamic_encode.iter().zip(static_encode.iter()).enumerate()
        {
            let field_name = dynamic_table.descriptor.field()[i].name();
            assert_eq!(dyn_entry.offset, static_entry.offset, "{}.{}: offset", type_name, field_name);
            assert_eq!(dyn_entry.has_bit, static_entry.has_bit, "{}.{}: has_bit", type_name, field_name);
            assert_eq!(dyn_entry.encoded_tag, static_entry.encoded_tag, "{}.{}: tag", type_name, field_name);
            assert_eq!(dyn_entry.kind, static_entry.kind, "{}.{}: kind", type_name, field_name);

            if dyn_entry.kind == crate::wire::FieldKind::Message
                || dyn_entry.kind == crate::wire::FieldKind::RepeatedMessage
            {
                aux_offsets.push(dyn_entry.offset as usize);
            }
        }

        let dynamic_decode = dynamic_table.decode_entries();
        let static_decode = static_table.decode_entries();
        for (i, (dyn_entry, static_entry)) in
            dynamic_decode.iter().zip(static_decode.iter()).enumerate()
        {
            assert_eq!(dyn_entry.0, static_entry.0, "{} decode[{}]", type_name, i);
        }

        for offset in aux_offsets {
            let dyn_aux = dynamic_table.aux_entry(offset);
            let static_aux = static_table.aux_entry(offset);
            assert_eq!(dyn_aux.offset, static_aux.offset, "{} aux@{}", type_name, offset);
            compare_tables_rec(
                unsafe { &*static_aux.child_table },
                unsafe { &*dyn_aux.child_table },
                seen,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated_code_only::Protobuf;
    use allocator_api2::alloc::Global;
    use std::collections::HashSet;

    #[test]
    fn test_static_vs_dynamic_tables() {
        let mut pool = DescriptorPool::new(&Global);
        let file_descriptor =
            crate::google::protobuf::FileDescriptorProto::ProtoType::file_descriptor();
        pool.add_file(file_descriptor);

        let static_table =
            <crate::google::protobuf::FileDescriptorSet::ProtoType as Protobuf>::table();
        let dynamic_table = pool
            .get_table("google.protobuf.FileDescriptorSet")
            .expect("FileDescriptorSet not found in pool");

        let mut seen = HashSet::new();
        test_util::compare_tables_rec(static_table, dynamic_table, &mut seen);
    }
}
