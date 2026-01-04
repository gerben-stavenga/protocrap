//! Test utilities for protocrap - available to downstream crates for testing.

use crate::ProtobufMut;

#[cfg(not(feature = "nightly"))]
use allocator_api2::alloc::Global;
#[cfg(feature = "nightly")]
use std::alloc::Global;

/// Assert that a message can be encoded and decoded without loss.
pub fn assert_roundtrip<'a, T: ProtobufMut<'a> + Default>(msg: &T) {
    let data = msg.encode_vec::<32>().expect("msg should encode");

    let mut arena = crate::arena::Arena::new(&Global);
    let mut roundtrip_msg = T::default();
    assert!(roundtrip_msg.decode_flat::<32>(&mut arena, &data));

    println!(
        "Encoded {} ({} bytes)",
        msg.as_dyn().descriptor().name(),
        data.len()
    );

    let roundtrip_data = roundtrip_msg.encode_vec::<32>().expect("msg should encode");

    assert_eq!(roundtrip_data, data);
}

use crate::tables::Table;
use std::collections::HashSet;

/// Recursively compare static and dynamic tables for equality.
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
        "{}: size mismatch",
        type_name
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

    let dynamic_encode = dynamic_table.encode_entries();
    let static_encode = static_table.encode_entries();

    let mut aux_offsets = Vec::new();
    for (i, (dyn_entry, static_entry)) in
        dynamic_encode.iter().zip(static_encode.iter()).enumerate()
    {
        let field_name = dynamic_table.descriptor.field()[i].name();
        assert_eq!(
            dyn_entry.offset, static_entry.offset,
            "{}.{}: offset",
            type_name, field_name
        );
        assert_eq!(
            dyn_entry.has_bit, static_entry.has_bit,
            "{}.{}: has_bit",
            type_name, field_name
        );
        assert_eq!(
            dyn_entry.encoded_tag, static_entry.encoded_tag,
            "{}.{}: tag",
            type_name, field_name
        );
        assert_eq!(
            dyn_entry.kind, static_entry.kind,
            "{}.{}: kind",
            type_name, field_name
        );

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
        assert_eq!(dyn_aux.0, static_aux.0, "{} aux@{}", type_name, offset);
        compare_tables_rec(static_aux.1, dyn_aux.1, seen);
    }
}
