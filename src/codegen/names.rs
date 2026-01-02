// protocrap-codegen/src/names.rs

use super::protocrap;
use protocrap::reflection::is_in_oneof;
use proc_macro2::TokenStream;
use protocrap::google::protobuf::FieldDescriptorProto::ProtoType as FieldDescriptorProto;
use protocrap::google::protobuf::FieldDescriptorProto::Type;
use quote::{format_ident, quote};

const RUST_KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "async", "await", "dyn",
];

pub fn sanitize_field_name(name: &str) -> String {
    if RUST_KEYWORDS.contains(&name) {
        // Use rust r# syntax for keywords
        format!("r#{}", name)
    } else {
        name.to_string()
    }
}

/// Convert snake_case to PascalCase (for union type names)
pub fn to_pascal_case(name: &str) -> String {
    name.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
}

/// Sanitize a module name by appending underscore for keywords
/// (can't use r# prefix for modules, especially with leading underscores)
pub fn sanitize_module_name(name: &str) -> String {
    if RUST_KEYWORDS.contains(&name) {
        format!("{}_", name)
    } else {
        name.to_string()
    }
}

pub fn rust_field_type_tokens(field: &FieldDescriptorProto) -> TokenStream {
    use protocrap::google::protobuf::FieldDescriptorProto::Label;

    let is_repeated = field.label().unwrap() == Label::LABEL_REPEATED;
    let is_message = matches!(
        field.r#type(),
        Some(Type::TYPE_MESSAGE) | Some(Type::TYPE_GROUP)
    );

    if is_repeated {
        let element = rust_element_type_tokens(field);
        quote! { protocrap::containers::RepeatedField<#element> }
    } else if is_message {
        // Singular message field uses typed OptionalMessage<T>
        let msg_type = rust_type_tokens(field);
        if is_in_oneof(field) {
            // In oneof, use TypedMessage<T> because presence is tracked by the oneof discriminant
            quote! { protocrap::TypedMessage<#msg_type::ProtoType> }
        } else {
            quote! { protocrap::generated_code_only::OptionalMessage<#msg_type::ProtoType> }
        }
    } else {
        rust_element_type_tokens(field)
    }
}

pub fn rust_element_type_tokens(field: &FieldDescriptorProto) -> TokenStream {
    match field.r#type().unwrap() {
        Type::TYPE_MESSAGE | Type::TYPE_GROUP => {
            let msg_type = rust_type_tokens(field);
            quote! { protocrap::TypedMessage<#msg_type::ProtoType> }
        }
        Type::TYPE_INT32 | Type::TYPE_SINT32 | Type::TYPE_SFIXED32 => quote! { i32 },
        Type::TYPE_INT64 | Type::TYPE_SINT64 | Type::TYPE_SFIXED64 => quote! { i64 },
        Type::TYPE_UINT32 | Type::TYPE_FIXED32 => quote! { u32 },
        Type::TYPE_UINT64 | Type::TYPE_FIXED64 => quote! { u64 },
        Type::TYPE_FLOAT => quote! { f32 },
        Type::TYPE_DOUBLE => quote! { f64 },
        Type::TYPE_BOOL => quote! { bool },
        Type::TYPE_STRING => quote! { protocrap::containers::String },
        Type::TYPE_BYTES => quote! { protocrap::containers::Bytes },
        Type::TYPE_ENUM => quote! { i32 },
    }
}

pub fn rust_type_tokens(field: &FieldDescriptorProto) -> TokenStream {
    // type_name is like ".google.protobuf.FileDescriptorProto"
    let type_name = field.type_name();

    // Split into parts and convert to identifiers
    let parts: Vec<_> = type_name
        .trim_start_matches('.')
        .split('.')
        .map(|s| format_ident!("{}", s))
        .collect();

    // Build path: google::protobuf::FileDescriptorProto::ProtoType
    quote! { crate::#(#parts)::* }
}
