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
