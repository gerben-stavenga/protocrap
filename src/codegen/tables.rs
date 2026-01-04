use super::names::{rust_type_tokens, sanitize_field_name};
use super::protocrap;
use anyhow::Result;
use proc_macro2::TokenStream;
use protocrap::google::protobuf::DescriptorProto::ProtoType as DescriptorProto;
use protocrap::google::protobuf::FieldDescriptorProto::ProtoType as FieldDescriptorProto;

use protocrap::reflection::{calculate_tag_with_syntax, is_message};
use quote::{format_ident, quote};

/// Oneof info: field_number -> (discriminant_word_index, oneof_field_name)
/// discriminant_word_index is the index into metadata array where the discriminant is stored
type OneofInfo = std::collections::HashMap<i32, (usize, String)>;

fn generate_aux_entries(
    message: &DescriptorProto,
    oneof_info: &OneofInfo,
    aux_index_map: &mut std::collections::HashMap<i32, usize>,
) -> Result<Vec<TokenStream>> {
    let aux_entries: Vec<_> = message
        .field()
        .iter()
        .filter(|f| is_message(f))
        .map(|field| {
            // For oneof message fields, use the oneof name for offset (union offset)
            let field_offset_name = if let Some((_, oneof_name)) = oneof_info.get(&field.number()) {
                format_ident!("{}", sanitize_field_name(oneof_name))
            } else {
                format_ident!("{}", sanitize_field_name(field.name()))
            };
            let child_table = rust_type_tokens(field);
            let num_aux = aux_index_map.len();
            aux_index_map.insert(field.number(), num_aux);
            quote! {
                protocrap::generated_code_only::AuxTableEntry {
                    offset: core::mem::offset_of!(ProtoType, #field_offset_name) as u32,
                    child_table: &#child_table::TABLE.table,
                }
            }
        })
        .collect();
    Ok(aux_entries)
}

fn generate_encoding_entries(
    message: &DescriptorProto,
    has_bit_map: &std::collections::HashMap<i32, usize>,
    oneof_info: &OneofInfo,
    aux_index_map: &std::collections::HashMap<i32, usize>,
    syntax: Option<&str>,
) -> Result<Vec<TokenStream>> {
    let num_encode_entries = message.field().len();
    let num_aux_entries = aux_index_map.len();

    let max_field_number = message
        .field()
        .iter()
        .map(|f| f.number())
        .max()
        .unwrap_or(0);

    if max_field_number > 2047 {
        return Err(anyhow::anyhow!("Field numbers > 2047 not supported yet"));
    }

    let num_decode_entries = max_field_number as usize + 1;

    let entries: Vec<_> = message.field().iter().map(|field| {
        let kind = field_kind_tokens(field);
        let encoded_tag = calculate_tag_with_syntax(field, syntax);

        // Check oneof first (applies to all field types including message)
        if let Some((discriminant_word_idx, oneof_name)) = oneof_info.get(&field.number()) {
            // Oneof field: has_bit stores discriminant word index with 0x80 flag
            let has_bit = (0x80 | *discriminant_word_idx) as u8;
            let oneof_field_name = format_ident!("{}", sanitize_field_name(oneof_name));

            if is_message(field) {
                // Oneof message field - offset points to aux entry
                let aux_index = *aux_index_map.get(&field.number()).unwrap();
                quote! {
                    protocrap::generated_code_only::EncodeTableEntry {
                        has_bit: #has_bit,
                        kind: #kind,
                        offset: (
                            core::mem::offset_of!(protocrap::generated_code_only::TableWithEntries<#num_encode_entries, #num_decode_entries, #num_aux_entries>, aux_entries) +
                            #aux_index * core::mem::size_of::<protocrap::generated_code_only::AuxTableEntry>() -
                            core::mem::offset_of!(protocrap::generated_code_only::TableWithEntries<#num_encode_entries, #num_decode_entries, #num_aux_entries>, table)
                        ) as u16,
                        encoded_tag: #encoded_tag,
                    }
                }
            } else {
                // Oneof non-message field - offset stores union offset
                quote! {
                    protocrap::generated_code_only::EncodeTableEntry {
                        has_bit: #has_bit,
                        kind: #kind,
                        offset: core::mem::offset_of!(ProtoType, #oneof_field_name) as u16,
                        encoded_tag: #encoded_tag,
                    }
                }
            }
        } else if is_message(field) {
            let aux_index = *aux_index_map.get(&field.number()).unwrap();
            let has_bit = has_bit_map.get(&field.number()).copied().unwrap_or(0) as u8;
            // Regular message field - offset points to aux entry
            quote! {
                protocrap::generated_code_only::EncodeTableEntry {
                    has_bit: #has_bit,
                    kind: #kind,
                    offset: (
                        core::mem::offset_of!(protocrap::generated_code_only::TableWithEntries<#num_encode_entries, #num_decode_entries, #num_aux_entries>, aux_entries) +
                        #aux_index * core::mem::size_of::<protocrap::generated_code_only::AuxTableEntry>() -
                        core::mem::offset_of!(protocrap::generated_code_only::TableWithEntries<#num_encode_entries, #num_decode_entries, #num_aux_entries>, table)
                    ) as u16,
                    encoded_tag: #encoded_tag,
                }
            }
        } else {
            let field_name = format_ident!("{}", sanitize_field_name(field.name()));
            let has_bit = has_bit_map.get(&field.number()).copied().unwrap_or(0) as u8;
            quote! {
                protocrap::generated_code_only::EncodeTableEntry {
                    has_bit: #has_bit,
                    kind: #kind,
                    offset: core::mem::offset_of!(ProtoType, #field_name) as u16,
                    encoded_tag: #encoded_tag,
                }
            }
        }
    }).collect();

    Ok(entries)
}

fn generate_decoding_table(
    message: &DescriptorProto,
    has_bit_map: &std::collections::HashMap<i32, usize>,
    oneof_info: &OneofInfo,
    aux_index_map: &std::collections::HashMap<i32, usize>,
) -> Result<Vec<TokenStream>> {
    // Calculate masked table parameters
    let max_field_number = message
        .field()
        .iter()
        .map(|f| f.number())
        .max()
        .unwrap_or(0);

    if max_field_number > 2047 {
        return Err(anyhow::anyhow!("Field numbers > 2047 not supported yet"));
    }

    let num_decode_entries = max_field_number as usize + 1;

    let num_encode_entries = message.field().len();

    let num_aux_entries = aux_index_map.len();

    // Generate entry table
    let entries: Vec<_> = (0..=max_field_number).map(|field_number| {
        if let Some(field) = message.field().iter().find(|f| f.number() == field_number as i32) {
            let field_kind = field_kind_tokens(field);

            // Check oneof first (applies to all field types including message)
            if let Some((discriminant_word_idx, oneof_name)) = oneof_info.get(&field_number) {
                // Oneof field: has_bit stores discriminant word index with 0x80 flag
                let has_bit = (0x80 | *discriminant_word_idx) as u32;
                let oneof_field_name = format_ident!("{}", sanitize_field_name(oneof_name));

                if is_message(field) {
                    // Oneof message field - offset points to aux entry
                    let aux_index = *aux_index_map.get(&field_number).unwrap();
                    quote! { protocrap::generated_code_only::DecodeTableEntry::new(
                        #field_kind,
                        #has_bit,
                        core::mem::offset_of!(protocrap::generated_code_only::TableWithEntries<#num_encode_entries, #num_decode_entries, #num_aux_entries>, aux_entries) +
                        #aux_index * core::mem::size_of::<protocrap::generated_code_only::AuxTableEntry>() -
                        core::mem::offset_of!(protocrap::generated_code_only::TableWithEntries<#num_encode_entries, #num_decode_entries, #num_aux_entries>, table)
                    ) }
                } else {
                    // Oneof non-message field - offset stores union offset
                    quote! {
                        protocrap::generated_code_only::DecodeTableEntry::new(
                            #field_kind,
                            #has_bit,
                            core::mem::offset_of!(ProtoType, #oneof_field_name)
                        )
                    }
                }
            } else if is_message(field) {
                let aux_index = *aux_index_map.get(&field_number).unwrap();
                // Regular message field - offset points to aux entry
                quote! { protocrap::generated_code_only::DecodeTableEntry::new(
                    #field_kind,
                    0,
                    core::mem::offset_of!(protocrap::generated_code_only::TableWithEntries<#num_encode_entries, #num_decode_entries, #num_aux_entries>, aux_entries) +
                    #aux_index * core::mem::size_of::<protocrap::generated_code_only::AuxTableEntry>() -
                    core::mem::offset_of!(protocrap::generated_code_only::TableWithEntries<#num_encode_entries, #num_decode_entries, #num_aux_entries>, table)
                ) }
            } else {
                let field_name = format_ident!("{}", sanitize_field_name(field.name()));
                let has_bit = has_bit_map.get(&field_number).copied().unwrap_or(0) as u32;

                quote! {
                    protocrap::generated_code_only::DecodeTableEntry::new(
                        #field_kind,
                        #has_bit,
                        core::mem::offset_of!(ProtoType, #field_name)
                    )
                }
            }
        } else {
            quote! { protocrap::generated_code_only::DecodeTableEntry(0) }
        }
    }).collect();

    Ok(entries)
}

pub(crate) fn generate_table(
    message: &DescriptorProto,
    has_bit_map: &std::collections::HashMap<i32, usize>,
    oneof_info: &OneofInfo,
    syntax: Option<&str>,
) -> Result<TokenStream> {
    let mut aux_index_map = std::collections::HashMap::<i32, usize>::new();
    let aux_entries = generate_aux_entries(message, oneof_info, &mut aux_index_map)?;

    let encoding_entries =
        generate_encoding_entries(message, has_bit_map, oneof_info, &aux_index_map, syntax)?;
    let decoding_entries =
        generate_decoding_table(message, has_bit_map, oneof_info, &aux_index_map)?;

    let num_encode_entries = encoding_entries.len();
    let num_decode_entries = decoding_entries.len();
    let num_aux_entries = aux_entries.len();
    Ok(quote! {
        #[allow(clippy::identity_op, clippy::erasing_op)]
        pub static TABLE: protocrap::generated_code_only::TableWithEntries<
            #num_encode_entries,
            #num_decode_entries,
            #num_aux_entries
        > = protocrap::generated_code_only::TableWithEntries {
            encode_entries: [
                #(#encoding_entries),*
            ],
            table: protocrap::generated_code_only::Table {
                num_encode_entries: #num_encode_entries as u16,
                num_decode_entries: #num_decode_entries as u16,
                size: core::mem::size_of::<ProtoType>() as u16,
                descriptor: ProtoType::descriptor_proto(),
            },
            decode_entries: [
                #(#decoding_entries),*
            ],
            aux_entries: [
                #(#aux_entries),*
            ],
        };
    })
}

fn field_kind_tokens(field: &FieldDescriptorProto) -> TokenStream {
    let kind = protocrap::reflection::field_kind_tokens(field);
    let ident = format_ident!("{kind:?}");
    quote! { protocrap::generated_code_only::FieldKind::#ident }
}
