use allocator_api2::alloc::Global;
use anyhow::{Result, bail};
use protocrap; // Keep this for generated code
use protocrap::ProtobufMut;
use protocrap::reflection::DescriptorPool;

// Include all generated code from conformance_all.proto
// This includes conformance.proto, test_messages_proto2.proto, and test_messages_proto3.proto
#[cfg(not(bazel))]
include!(concat!(env!("OUT_DIR"), "/conformance_all.pc.rs"));
#[cfg(bazel)]
include!("conformance_all.pc.rs");

// Include descriptor bytes for reuse in main.rs and tests
#[cfg(not(bazel))]
pub const CONFORMANCE_DESCRIPTOR_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/conformance_all.bin"));
#[cfg(bazel)]
pub const CONFORMANCE_DESCRIPTOR_BYTES: &[u8] =
    include_bytes!(env!("CONFORMANCE_DESCRIPTOR_SET"));

pub static GLOBAL_ALLOC: Global = Global;

pub fn load_descriptor_pool() -> Result<DescriptorPool<'static>> {
    use protocrap::google::protobuf::FileDescriptorSet;

    let mut pool = DescriptorPool::new(&GLOBAL_ALLOC);

    // Load the FileDescriptorSet from the build output
    let descriptor_bytes = CONFORMANCE_DESCRIPTOR_BYTES;

    let mut fds = FileDescriptorSet::ProtoType::default();
    if !fds.decode_flat::<32>(&mut pool.arena, descriptor_bytes) {
        bail!("Failed to decode FileDescriptorSet");
    }

    let fds = pool.arena.place(fds);

    // Build pool
    for file in fds.file() {
        pool.add_file(file.as_ref());
    }

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use protocrap::ProtobufRef;
    use protocrap::tables::{AuxTableEntry, Table};

    use crate::conformance::{ConformanceRequest, ConformanceResponse, WireFormat};

    use super::*;

    #[test]
    fn test_static_vs_dynamic_tables() {
        let pool = load_descriptor_pool().unwrap();

        // Test TestAllTypesProto3
        test_message_table(
            &pool,
            "protobuf_test_messages.proto3.TestAllTypesProto3",
            <protobuf_test_messages::proto3::TestAllTypesProto3::ProtoType as protocrap::Protobuf>::table(),
        );

        // Test TestAllTypesProto2
        test_message_table(
            &pool,
            "protobuf_test_messages.proto2.TestAllTypesProto2",
            <protobuf_test_messages::proto2::TestAllTypesProto2::ProtoType as protocrap::Protobuf>::table(),
        );
    }

    fn test_message_table(pool: &DescriptorPool, type_name: &str, static_table: &'static Table) {
        let dynamic_table = pool
            .get_table(type_name)
            .unwrap_or_else(|| panic!("Dynamic table not found for {}", type_name));
        let mut seen = std::collections::hash_set::HashSet::new();
        test_message_table_rec(static_table, dynamic_table, &mut seen);
    }

    fn test_message_table_rec(
        static_table: &Table,
        dynamic_table: &Table,
        seen: &mut std::collections::hash_set::HashSet<*const Table>,
    ) {
        assert_eq!(
            dynamic_table.descriptor.name(),
            static_table.descriptor.name()
        );
        let type_name = dynamic_table.descriptor.name();
        if !seen.insert(dynamic_table as *const Table) {
            // Already seen this table, avoid infinite recursion
            return;
        }
        eprintln!("\nTesting {}", type_name);
        eprintln!("  Static table size: {}", static_table.size);
        eprintln!("  Dynamic table size: {}", dynamic_table.size);

        // Compare basic properties
        assert_eq!(
            dynamic_table.size, static_table.size,
            "{}: size mismatch - dynamic={} static={}",
            type_name, dynamic_table.size, static_table.size
        );

        assert_eq!(
            dynamic_table.num_encode_entries, static_table.num_encode_entries,
            "{}: num_encode_entries mismatch",
            type_name
        );

        assert_eq!(
            dynamic_table.num_decode_entries, static_table.num_decode_entries,
            "{}: num_decode_entries mismatch",
            type_name
        );

        // Compare encode entries
        let dynamic_encode = unsafe {
            let ptr = (dynamic_table as *const Table as *const protocrap::encoding::TableEntry)
                .sub(dynamic_table.num_encode_entries as usize);
            core::slice::from_raw_parts(ptr, dynamic_table.num_encode_entries as usize)
        };
        let static_encode = unsafe {
            let ptr = (static_table as *const Table as *const protocrap::encoding::TableEntry)
                .sub(static_table.num_encode_entries as usize);
            core::slice::from_raw_parts(ptr, static_table.num_encode_entries as usize)
        };

        let mut aux_offsets = Vec::new();
        for (i, (dyn_entry, static_entry)) in
            dynamic_encode.iter().zip(static_encode.iter()).enumerate()
        {
            let field_name = dynamic_table.descriptor.field()[i].name();

            assert_eq!(
                dyn_entry.offset, static_entry.offset,
                "{} field #{} '{}': offset mismatch - dynamic={} static={}",
                type_name, i, field_name, dyn_entry.offset, static_entry.offset
            );

            assert_eq!(
                dyn_entry.has_bit, static_entry.has_bit,
                "{} field #{} '{}': has_bit mismatch",
                type_name, i, field_name
            );

            assert_eq!(
                dyn_entry.encoded_tag, static_entry.encoded_tag,
                "{} field #{} '{}': tag mismatch",
                type_name, i, field_name
            );

            assert_eq!(
                dyn_entry.kind, static_entry.kind,
                "{} field #{} '{}': kind mismatch",
                type_name, i, field_name
            );

            if dyn_entry.kind == protocrap::wire::FieldKind::Message
                || static_entry.kind == protocrap::wire::FieldKind::RepeatedMessage
            {
                aux_offsets.push(dyn_entry.offset as usize);
            }
        }

        // Compare decode entries
        let dynamic_decode = unsafe {
            let ptr =
                (dynamic_table as *const Table).add(1) as *const protocrap::decoding::TableEntry;
            core::slice::from_raw_parts(ptr, dynamic_table.num_decode_entries as usize)
        };
        let static_decode = unsafe {
            let ptr =
                (static_table as *const Table).add(1) as *const protocrap::decoding::TableEntry;
            core::slice::from_raw_parts(ptr, static_table.num_decode_entries as usize)
        };
        for (i, (dyn_entry, static_entry)) in
            dynamic_decode.iter().zip(static_decode.iter()).enumerate()
        {
            if dyn_entry.0 != static_entry.0 {
                eprintln!(
                    "  Decode entry #{}: offset mismatch - dynamic={} static={}",
                    i, dyn_entry.0, static_entry.0
                );
            }

            assert_eq!(
                dyn_entry.0, static_entry.0,
                "{} decode entry #{}: offset mismatch - dynamic={} static={}",
                type_name, i, dyn_entry.0, static_entry.0
            );
        }

        let mut childs = Vec::new();
        // Compare aux entries
        for offset in aux_offsets {
            let dyn_aux_ptr = unsafe {
                (dynamic_table as *const Table as *const u8).add(offset) as *const AuxTableEntry
            };
            let dyn_aux = unsafe { &*dyn_aux_ptr };

            let static_aux_ptr = unsafe {
                (static_table as *const Table as *const u8).add(offset) as *const AuxTableEntry
            };
            let static_aux = unsafe { &*static_aux_ptr };

            childs.push(unsafe { (&*static_aux.child_table, &*dyn_aux.child_table) });

            assert_eq!(
                dyn_aux.offset, static_aux.offset,
                "{} aux entry at offset {}: offset mismatch - dynamic={} static={}",
                type_name, offset, dyn_aux.offset, static_aux.offset
            );
        }
        for (static_child, dynamic_child) in childs {
            test_message_table_rec(static_child, dynamic_child, seen);
        }

        eprintln!("  ✓ All {} encode entries match!", dynamic_encode.len());
    }
}
