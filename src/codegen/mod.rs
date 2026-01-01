// protocrap codegen module

use allocator_api2::alloc::Global;
use anyhow::Result;

#[cfg(feature = "bootstrap")]
use bootcrap as protocrap;
#[cfg(not(feature = "bootstrap"))]
use crate as protocrap;

use protocrap::ProtobufMut;
use protocrap::google::protobuf::FileDescriptorSet::ProtoType as FileDescriptorSet;

mod generator;
mod names;
mod static_gen;
mod tables;

/// Generate Rust code from protobuf descriptor bytes (FileDescriptorSet binary format)
pub fn generate(descriptor_bytes: &[u8]) -> Result<String> {
    let mut arena = protocrap::arena::Arena::new(&Global);
    let mut file_set = FileDescriptorSet::default();
    if !file_set.decode_flat::<100>(&mut arena, descriptor_bytes) {
        return Err(anyhow::anyhow!("Failed to decode file descriptor set"));
    }

    let tokens = generator::generate_file_set(&file_set)?;

    let syntax_tree = syn::parse2(tokens)?;
    Ok(prettyplease::unparse(&syntax_tree))
}

/// Generate a const initializer expression for embedding protobuf data.
/// Supports both binary (.pb) and JSON (.json) input formats.
/// Returns just the initializer expression suitable for `include!()`.
/// `crate_path` specifies the module path prefix (e.g., "protocrap" or "" for local).
pub fn generate_embed(
    descriptor_bytes: &[u8],
    data: &[u8],
    type_name: &str,
    is_json: bool,
    crate_path: &str,
) -> Result<String> {
    // Decode the descriptor set
    let mut arena = protocrap::arena::Arena::new(&Global);
    let mut file_set = FileDescriptorSet::default();
    if !file_set.decode_flat::<100>(&mut arena, descriptor_bytes) {
        return Err(anyhow::anyhow!("Failed to decode file descriptor set"));
    }

    // Build descriptor pool
    let mut pool = protocrap::descriptor_pool::DescriptorPool::new(&Global);
    for file in file_set.file() {
        pool.add_file(file);
    }

    // Decode the data using the pool
    let mut msg = pool.create_message(type_name, &mut arena)?;
    if is_json {
        let json_str = std::str::from_utf8(data)?;
        let mut deserializer = serde_json::Deserializer::from_str(json_str);
        msg.serde_deserialize(&mut arena, &mut deserializer)
            .map_err(|e| anyhow::anyhow!("JSON deserialization failed: {}", e))?;
    } else if !msg.decode_flat::<100>(&mut arena, data) {
        return Err(anyhow::anyhow!("Failed to decode protobuf data"));
    }

    // Generate the initializer expression
    let tokens = static_gen::generate_static_dynamic(&msg, type_name, crate_path)?;

    // Parse as expression (not file) and format
    let expr: syn::Expr = syn::parse2(tokens)?;
    let wrapped = syn::parse_quote! { fn __fmt() { #expr } };
    let formatted = prettyplease::unparse(&wrapped);

    // Extract just the expression (skip "fn __fmt() {" and trailing "}")
    let start = formatted.find('{').unwrap() + 1;
    let end = formatted.rfind('}').unwrap();
    Ok(formatted[start..end].trim().to_string())
}
