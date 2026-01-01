#![no_main]

use allocator_api2::alloc::Global;
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use protocrap::ProtobufMut;

#[derive(Arbitrary, Debug)]
struct ChunkedInput {
    data: Vec<u8>,
    chunk_sizes: Vec<u8>,
}

fuzz_target!(|input: ChunkedInput| {
    let mut arena = protocrap::arena::Arena::new(&Global);
    let mut msg = protocrap::google::protobuf::FileDescriptorProto::ProtoType::default();

    let mut pos = 0;
    let mut chunk_idx = 0;

    let mut provider = || -> Result<Option<&[u8]>, ()> {
        if pos >= input.data.len() {
            return Ok(None);
        }
        let size = input
            .chunk_sizes
            .get(chunk_idx)
            .copied()
            .unwrap_or(16)
            .max(1) as usize;
        let end = (pos + size).min(input.data.len());
        let chunk = &input.data[pos..end];
        pos = end;
        chunk_idx += 1;
        Ok(Some(chunk))
    };

    let _ = msg.decode(&mut arena, &mut provider);
});
