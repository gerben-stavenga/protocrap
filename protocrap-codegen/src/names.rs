// protocrap-codegen/src/names.rs

use proc_macro2::TokenStream;
use prost_types::field_descriptor_proto::Type;
use prost_types::FieldDescriptorProto;
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

pub fn rust_field_type_tokens(field: &FieldDescriptorProto) -> TokenStream {
    use prost_types::field_descriptor_proto::Label;

    if field.label() == Label::Repeated {
        let element = rust_element_type_tokens(field);
        quote! { protocrap::containers::RepeatedField<#element> }
    } else {
        match field.r#type() {
            Type::Int32 | Type::Sint32 | Type::Sfixed32 => quote! { i32 },
            Type::Int64 | Type::Sint64 | Type::Sfixed64 => quote! { i64 },
            Type::Uint32 | Type::Fixed32 => quote! { u32 },
            Type::Uint64 | Type::Fixed64 => quote! { u64 },
            Type::Float => quote! { f32 },
            Type::Double => quote! { f64 },
            Type::Bool => quote! { bool },
            Type::String => quote! { protocrap::containers::String },
            Type::Bytes => quote! { protocrap::containers::Bytes },
            Type::Message | Type::Group => quote! { protocrap::base::Message },
            Type::Enum => quote! { i32 },
        }
    }
}

pub fn rust_scalar_type_tokens(field: &FieldDescriptorProto) -> TokenStream {
    match field.r#type() {
        Type::Int32 | Type::Sint32 | Type::Sfixed32 => quote! { i32 },
        Type::Int64 | Type::Sint64 | Type::Sfixed64 => quote! { i64 },
        Type::Uint32 | Type::Fixed32 => quote! { u32 },
        Type::Uint64 | Type::Fixed64 => quote! { u64 },
        Type::Float => quote! { f32 },
        Type::Double => quote! { f64 },
        Type::Bool => quote! { bool },
        Type::Enum => quote! { i32 },
        _ => quote! { () },
    }
}

pub fn rust_element_type_tokens(field: &FieldDescriptorProto) -> TokenStream {
    match field.r#type() {
        Type::Message | Type::Group => quote! { protocrap::base::Message },
        Type::Int32 | Type::Sint32 | Type::Sfixed32 => quote! { i32 },
        Type::Int64 | Type::Sint64 | Type::Sfixed64 => quote! { i64 },
        Type::Uint32 | Type::Fixed32 => quote! { u32 },
        Type::Uint64 | Type::Fixed64 => quote! { u64 },
        Type::Float => quote! { f32 },
        Type::Double => quote! { f64 },
        Type::Bool => quote! { bool },
        Type::String => quote! { protocrap::containers::String },
        Type::Bytes => quote! { protocrap::containers::Bytes },
        Type::Enum => quote! { i32 },
    }
}

pub fn rust_type_tokens(field: &FieldDescriptorProto) -> TokenStream {
    // type_name is like ".google.protobuf.FileDescriptorProto"
    let type_name = field.type_name.as_ref().unwrap();

    // Split into parts and convert to identifiers
    let parts: Vec<_> = type_name
        .trim_start_matches('.')
        .split('.')
        .map(|s| format_ident!("{}", s))
        .collect();

    // Build path: google::protobuf::FileDescriptorProto::ProtoType
    quote! { crate::#(#parts)::* }
}
