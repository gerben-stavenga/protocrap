// protocrap-codegen/src/static_gen.rs

use super::protocrap;
use anyhow::Result;
use proc_macro2::{Literal, TokenStream};
use protocrap::{
    google::protobuf::FieldDescriptorProto::{ProtoType as FieldDescriptorProto, Type},
    reflection::{DynamicMessageRef, Value, is_repeated, needs_has_bit},
};
use quote::{ToTokens, format_ident, quote};

/// Convert a fully qualified proto type name to Rust path tokens.
/// Input: ".google.protobuf.FileDescriptorProto" or "google.protobuf.FileDescriptorProto"
/// Output: [google, protobuf, FileDescriptorProto]
fn resolve_type_path(type_name: &str) -> Vec<proc_macro2::Ident> {
    type_name
        .trim_start_matches('.')
        .split('.')
        .map(|s| format_ident!("{}", s))
        .collect()
}

/// Generate the crate prefix token stream (e.g., "protocrap::" or empty for local)
fn crate_prefix(crate_path: &str) -> TokenStream {
    if crate_path.is_empty() {
        quote! {}
    } else {
        let crate_ident = format_ident!("{}", crate_path);
        quote! { #crate_ident:: }
    }
}

/// Generate static initializer for any proto message using runtime reflection.
/// `type_name` should be the fully qualified proto type name like "google.protobuf.FileDescriptorProto"
/// `crate_path` specifies the module prefix (e.g., "protocrap" or "" for local types)
pub(crate) fn generate_static_dynamic(
    value: &DynamicMessageRef,
    type_name: &str,
    crate_path: &str,
) -> Result<TokenStream> {
    // Calculate has_bits
    let has_bits = calculate_has_bits(&value);
    let has_bits_tokens = generate_has_bits_array(&has_bits);

    // Generate field initializers
    let field_inits = generate_field_initializers(value, crate_path)?;

    // Parse type path
    let path_parts = resolve_type_path(type_name);
    let prefix = crate_prefix(crate_path);

    Ok(quote! {
        {
            #prefix #(#path_parts)::* ::ProtoType::from_static(
                #has_bits_tokens,
                #(#field_inits),*
            )
        }
    })
}

fn calculate_has_bits(value: &DynamicMessageRef) -> Vec<u32> {
    let descriptor = value.descriptor();
    let num_has_bits = descriptor
        .field()
        .iter()
        .filter(|field| needs_has_bit(field))
        .count();
    let word_count = (num_has_bits + 31) / 32;
    let mut has_bits = vec![0u32; word_count];

    let mut has_bit_idx = 0;
    for field in descriptor.field() {
        if !needs_has_bit(field) {
            continue;
        }
        if let Some(_) = value.get_field(field.as_ref()) {
            let word_idx = has_bit_idx / 32;
            let bit_idx = has_bit_idx % 32;
            has_bits[word_idx] |= 1u32 << bit_idx;
        }
        has_bit_idx += 1;
    }

    has_bits
}

fn generate_has_bits_array(has_bits: &[u32]) -> TokenStream {
    let values: Vec<_> = has_bits
        .iter()
        .map(|&v| Literal::u32_unsuffixed(v))
        .collect();
    quote! { [#(#values),*] }
}

fn generate_field_initializers(value: &DynamicMessageRef, crate_path: &str) -> Result<Vec<TokenStream>> {
    let mut inits = Vec::new();

    let mut fields = Vec::from(value.descriptor().field());
    fields.sort_by_key(|f| f.number());

    for field in fields {
        let field_value = value.get_field(field.as_ref());
        let init = if let Some(val) = field_value {
            generate_field_value(val, field.as_ref(), crate_path)?.0
        } else {
            generate_default_value(field.as_ref())
        };

        inits.push(init);
    }

    Ok(inits)
}

fn generate_repeated_scalar<T: Copy + ToTokens>(
    values: &[T],
) -> Result<(TokenStream, TokenStream)> {
    let mut elements = Vec::new();
    let type_name = format_ident!("{}", std::any::type_name::<T>());

    for &value in values {
        let elem_init = quote! {
            #value
        };
        elements.push(elem_init);
    }

    let len = elements.len();
    Ok((
        quote! {
            {
                static ELEMENTS: [#type_name; #len] = [
                    #(#elements),*
                ];
                protocrap::containers::RepeatedField::from_static(&ELEMENTS)
            }
        },
        quote! { protocrap::containers::RepeatedField<#type_name> },
    ))
}

fn generate_field_value(
    value: Value,
    field: &FieldDescriptorProto,
    crate_path: &str,
) -> Result<(TokenStream, TokenStream)> {
    let prefix = crate_prefix(crate_path);
    match value {
        Value::Bool(b) => Ok((quote! { #b }, quote! { bool })),
        Value::Int32(v) => {
            let lit = Literal::i32_unsuffixed(v);
            Ok((quote! { #lit }, quote! { i32 }))
        }
        Value::Int64(v) => {
            let lit = Literal::i64_unsuffixed(v);
            Ok((quote! { #lit }, quote! { i64 }))
        }
        Value::UInt32(v) => {
            let lit = Literal::u32_unsuffixed(v);
            Ok((quote! { #lit }, quote! { u32 }))
        }
        Value::UInt64(v) => {
            let lit = Literal::u64_unsuffixed(v);
            Ok((quote! { #lit }, quote! { u64 }))
        }
        Value::Float(v) => {
            let lit = Literal::f32_unsuffixed(v);
            Ok((quote! { #lit }, quote! { f32 }))
        }
        Value::Double(v) => {
            let lit = Literal::f64_unsuffixed(v);
            Ok((quote! { #lit }, quote! { f64 }))
        }
        Value::String(s) => Ok((
            quote! {
                protocrap::containers::String::from_static(#s)
            },
            quote! { protocrap::containers::String },
        )),
        Value::Bytes(b) => {
            let bytes: Vec<_> = b.iter().map(|&byte| Literal::u8_unsuffixed(byte)).collect();
            Ok((
                quote! {
                    protocrap::containers::Bytes::from_static(&[#(#bytes),*])
                },
                quote! { protocrap::containers::Bytes },
            ))
        }
        Value::Message(msg) => {
            let type_name = field.type_name();
            let static_ref = generate_nested_message(&msg, type_name, crate_path)?;
            Ok((
                quote! { protocrap::generated_code_only::OptionalMessage::from_static(#static_ref) },
                quote! { protocrap::generated_code_only::OptionalMessage },
            ))
        }
        Value::RepeatedBool(list) => generate_repeated_scalar(list),
        Value::RepeatedInt32(list) => generate_repeated_scalar(list),
        Value::RepeatedInt64(list) => generate_repeated_scalar(list),
        Value::RepeatedUInt32(list) => generate_repeated_scalar(list),
        Value::RepeatedUInt64(list) => generate_repeated_scalar(list),
        Value::RepeatedFloat(list) => generate_repeated_scalar(list),
        Value::RepeatedDouble(list) => generate_repeated_scalar(list),
        Value::RepeatedString(list) => {
            let mut elements = Vec::new();
            for s in list {
                let s_str = s.as_str();
                let elem_init = quote! {
                    protocrap::containers::String::from_static(#s_str)
                };
                elements.push(elem_init);
            }
            let len = elements.len();
            Ok((
                quote! {
                    {
                        static ELEMENTS: [protocrap::containers::String; #len] = [
                            #(#elements),*
                        ];
                        protocrap::containers::RepeatedField::from_static(&ELEMENTS)
                    }
                },
                quote! { protocrap::containers::RepeatedField<protocrap::containers::String> },
            ))
        }
        Value::RepeatedBytes(list) => {
            let mut elements = Vec::new();
            for b in list {
                let bytes: Vec<_> = b.iter().map(|&byte| Literal::u8_unsuffixed(byte)).collect();
                let elem_init = quote! {
                    protocrap::containers::Bytes::from_static(&[#(#bytes),*])
                };
                elements.push(elem_init);
            }
            let len = elements.len();
            Ok((
                quote! {
                    {
                        static ELEMENTS: [protocrap::containers::Bytes; #len] = [
                            #(#elements),*
                        ];
                        protocrap::containers::RepeatedField::from_static(&ELEMENTS)
                    }
                },
                quote! { protocrap::containers::RepeatedField<protocrap::containers::Bytes> },
            ))
        }
        Value::RepeatedMessage(list) => {
            let type_name = field.type_name();
            let path_parts = resolve_type_path(type_name);
            let mut elements = Vec::new();
            for msg in list.iter() {
                let static_ref = generate_nested_message(&msg, type_name, crate_path)?;
                elements.push(quote! { protocrap::TypedMessage::from_static(#static_ref) });
            }
            let len = elements.len();
            Ok((
                quote! {
                    {
                        static ELEMENTS: [protocrap::TypedMessage<#prefix #(#path_parts)::* ::ProtoType>; #len] = [
                            #(#elements),*
                        ];
                        protocrap::containers::RepeatedField::from_static(&ELEMENTS)
                    }
                },
                quote! { protocrap::containers::RepeatedField<protocrap::TypedMessage<#prefix #(#path_parts)::* ::ProtoType>> },
            ))
        }
    }
}

fn generate_nested_message(msg: &DynamicMessageRef, type_name: &str, crate_path: &str) -> Result<TokenStream> {
    let nested_initializer = generate_static_dynamic(msg, type_name, crate_path)?;
    let path_parts = resolve_type_path(type_name);
    let prefix = crate_prefix(crate_path);

    Ok(quote! {
        {
            static PROTO_TYPE: #prefix #(#path_parts)::* ::ProtoType = #nested_initializer;
            &PROTO_TYPE
        }
    })
}

fn generate_default_value(field: &FieldDescriptorProto) -> TokenStream {
    if is_repeated(field) {
        return quote! { protocrap::containers::RepeatedField::new() };
    }

    match field.r#type().unwrap() {
        Type::TYPE_STRING => quote! { protocrap::containers::String::new() },
        Type::TYPE_BYTES => quote! { protocrap::containers::Bytes::new() },
        Type::TYPE_MESSAGE | Type::TYPE_GROUP => quote! {
            protocrap::generated_code_only::OptionalMessage::none()
        },
        Type::TYPE_BOOL => quote! { false },
        Type::TYPE_INT32 | Type::TYPE_SINT32 | Type::TYPE_SFIXED32 | Type::TYPE_ENUM => {
            quote! { 0i32 }
        }
        Type::TYPE_INT64 | Type::TYPE_SINT64 | Type::TYPE_SFIXED64 => quote! { 0i64 },
        Type::TYPE_UINT32 | Type::TYPE_FIXED32 => quote! { 0u32 },
        Type::TYPE_UINT64 | Type::TYPE_FIXED64 => quote! { 0u64 },
        Type::TYPE_FLOAT => quote! { 0.0f32 },
        Type::TYPE_DOUBLE => quote! { 0.0f64 },
    }
}
