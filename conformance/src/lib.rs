use allocator_api2::alloc::Global;
use anyhow::{Result, bail};
use protocrap::ProtobufMut;
use protocrap::descriptor_pool::DescriptorPool;

// Re-export all generated types from test_protos
pub use test_protos::*;

pub static GLOBAL_ALLOC: Global = Global;

pub fn load_descriptor_pool() -> Result<DescriptorPool<'static>> {
    use protocrap::google::protobuf::FileDescriptorSet;

    let mut pool = DescriptorPool::new(&GLOBAL_ALLOC);

    let mut fds = FileDescriptorSet::ProtoType::default();
    if !fds.decode_flat::<32>(&mut pool.arena, test_protos::DESCRIPTOR_BYTES) {
        bail!("Failed to decode FileDescriptorSet");
    }

    let fds = pool.arena.place(fds)?;

    for file in fds.file() {
        pool.add_file(file.as_ref())?;
    }

    Ok(pool)
}
