//! Test that protocrap works in a no_std environment.
//!
//! This crate verifies that the core protocrap functionality compiles
//! and works without the standard library, including encode/decode.
//!
//! Build with: cargo build -p no-std-test --target thumbv7m-none-eabi

#![no_std]

use protocrap::arena::Arena;
use protocrap::google::protobuf::FileDescriptorProto;
use protocrap::{Allocator, ProtobufMut, ProtobufRef};

/// Test encoding works in no_std
pub fn test_encode(alloc: &dyn Allocator) -> bool {
    let mut arena = Arena::new(alloc);

    // Create a simple FileDescriptorProto
    let mut msg = FileDescriptorProto::ProtoType::default();
    msg.set_name("test.proto", &mut arena);
    msg.set_package("test.package", &mut arena);

    // Encode to a fixed buffer
    let mut buffer = [0u8; 256];
    match msg.encode_flat::<16>(&mut buffer) {
        Ok(encoded) => !encoded.is_empty(),
        Err(_) => false,
    }
}

/// Test decoding works in no_std
pub fn test_decode(alloc: &dyn Allocator) -> bool {
    let mut arena = Arena::new(alloc);

    // Wire format for FileDescriptorProto with name="test.proto"
    // Field 1 (name) = string, tag = 0x0a, length = 10, "test.proto"
    let data: &[u8] = &[
        0x0a, 0x0a, b't', b'e', b's', b't', b'.', b'p', b'r', b'o', b't', b'o',
    ];

    let mut msg = FileDescriptorProto::ProtoType::default();
    if msg.decode_flat::<16>(&mut arena, data) {
        msg.name() == "test.proto"
    } else {
        false
    }
}

/// Test round-trip encode/decode works in no_std
pub fn test_roundtrip(alloc: &dyn Allocator) -> bool {
    let mut arena = Arena::new(alloc);

    // Create and populate a message
    let mut original = FileDescriptorProto::ProtoType::default();
    original.set_name("roundtrip.proto", &mut arena);
    original.set_package("my.package", &mut arena);

    // Encode
    let mut buffer = [0u8; 256];
    let encoded = match original.encode_flat::<16>(&mut buffer) {
        Ok(e) => e,
        Err(_) => return false,
    };

    // Copy encoded data (encode_flat returns slice at end of buffer)
    let mut encoded_copy = [0u8; 256];
    let len = encoded.len();
    encoded_copy[..len].copy_from_slice(encoded);

    // Decode into new message
    let mut decoded = FileDescriptorProto::ProtoType::default();
    if !decoded.decode_flat::<16>(&mut arena, &encoded_copy[..len]) {
        return false;
    }

    // Verify
    decoded.name() == "roundtrip.proto" && decoded.package() == "my.package"
}
