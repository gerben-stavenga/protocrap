// When bootstrap feature is enabled, the library is empty (uses bootcrap from crates.io)
#![cfg(not(feature = "bootstrap"))]

//! # Protocrap
//!
//! A small, efficient, and flexible protobuf implementation for Rust.
//!
//! ## Overview
//!
//! Protocrap takes a different approach than other protobuf libraries. Instead of
//! generating parsing and serialization code for each message type, it uses a single
//! table-driven implementation. Code generation produces only struct definitions with
//! accessors and static lookup tables.
//!
//! This design yields:
//! - **Small binaries**: No code duplication across message types
//! - **Fast compilation**: No macro expansion or monomorphization explosion
//! - **Flexible memory**: Arena allocation with custom allocator support
//! - **Universal streaming**: Push-based API works with sync and async
//!
//! ## Quick Start
//!
//! ### Code Generation
//!
//! Generate Rust code from `.proto` files using `protocrap-codegen`:
//!
//! ```bash
//! # Create descriptor set with protoc
//! protoc --include_imports --descriptor_set_out=types.bin my_types.proto
//!
//! # Generate Rust code
//! protocrap-codegen types.bin src/types.pc.rs
//! ```
//!
//! Include the generated code in your crate:
//!
//! ```ignore
//! use protocrap;
//! include!("types.pc.rs");
//! ```
//!
//! ### Encoding Messages
//!
//! ```
//! use protocrap::{ProtobufRef, ProtobufMut, arena::Arena};
//! use protocrap::google::protobuf::FileDescriptorProto;
//! use allocator_api2::alloc::Global;
//!
//! let mut arena = Arena::new(&Global);
//! let mut msg = FileDescriptorProto::ProtoType::default();
//! msg.set_name("example.proto", &mut arena);
//! msg.set_package("my.package", &mut arena);
//!
//! // Encode to a Vec<u8>
//! let bytes = msg.encode_vec::<32>().unwrap();
//!
//! // Or encode to a fixed buffer
//! let mut buffer = [0u8; 1024];
//! let encoded = msg.encode_flat::<32>(&mut buffer).unwrap();
//! ```
//!
//! The const generic (`::<32>`) specifies the maximum message nesting depth.
//!
//! ### Decoding Messages
//!
//! ```
//! use protocrap::{ProtobufRef, ProtobufMut, arena::Arena};
//! use protocrap::google::protobuf::FileDescriptorProto;
//! use allocator_api2::alloc::Global;
//!
//! // First encode a message to get some bytes
//! let mut arena = Arena::new(&Global);
//! let mut original = FileDescriptorProto::ProtoType::default();
//! original.set_name("example.proto", &mut arena);
//! let bytes = original.encode_vec::<32>().unwrap();
//!
//! // Decode from a byte slice
//! let mut decoded = FileDescriptorProto::ProtoType::default();
//! decoded.decode_flat::<32>(&mut arena, &bytes);
//! assert_eq!(decoded.name(), "example.proto");
//! ```
//!
//! ### Runtime Reflection
//!
//! Inspect messages dynamically without compile-time knowledge of the schema:
//!
//! ```
//! use protocrap::{Protobuf, ProtobufRef, arena::Arena};
//! use protocrap::google::protobuf::{FileDescriptorProto, DescriptorProto};
//! use protocrap::descriptor_pool::DescriptorPool;
//! use allocator_api2::alloc::Global;
//!
//! // Build descriptor pool from the library's own file descriptor
//! let mut pool = DescriptorPool::new(&Global);
//! let file_desc = FileDescriptorProto::ProtoType::file_descriptor();
//! pool.add_file(file_desc);
//!
//! // Encode a real DescriptorProto (the descriptor for DescriptorProto itself)
//! let descriptor = DescriptorProto::ProtoType::descriptor_proto();
//! let bytes = descriptor.encode_vec::<32>().unwrap();
//!
//! // Decode dynamically using the pool
//! let mut arena = Arena::new(&Global);
//! let msg = pool.decode_message(
//!     "google.protobuf.DescriptorProto",
//!     &bytes,
//!     &mut arena,
//! ).unwrap();
//!
//! // Access fields dynamically
//! for field in msg.descriptor().field() {
//!     if let Some(value) = msg.get_field(field.as_ref()) {
//!         println!("{}: {:?}", field.name(), value);
//!     }
//! }
//! ```
//!
//! ## Architecture
//!
//! ### Arena Allocation
//!
//! All variable-sized data (strings, bytes, repeated fields, sub-messages) is allocated
//! in an [`arena::Arena`]. This provides:
//!
//! - **Speed**: Allocation is a pointer bump in the common case
//! - **Bulk deallocation**: Drop the arena to free all messages at once
//! - **Custom allocators**: Pass any `&dyn Allocator` to control memory placement
//!
//! ```
//! use protocrap::arena::Arena;
//! use allocator_api2::alloc::Global;
//!
//! let mut arena = Arena::new(&Global);
//! // All allocations during decode/set operations use this arena
//! // When arena drops, all memory is freed
//! ```
//!
//! ### Push-Based Streaming
//!
//! The parser uses a push model: you provide data chunks, it returns updated state.
//! This signature `(state, buffer) -> updated_state` enables:
//!
//! - Single implementation for sync and async
//! - No callback traits or complex lifetime requirements
//! - Works in embedded, WASM, and any async runtime
//!
//! ## Generated Code Structure
//!
//! For each protobuf message, the codegen produces a module containing:
//!
//! - `ProtoType`: The message struct with `#[repr(C)]` layout
//! - Accessor methods following protobuf conventions
//!
//! Field accessors follow this pattern:
//!
//! | Proto Type | Getter | Setter | Other |
//! |------------|--------|--------|-------|
//! | Scalar | `field() -> T` | `set_field(T)` | `has_field()`, `clear_field()` |
//! | String/Bytes | `field() -> &str`/`&[u8]` | `set_field(&str, &mut Arena)` | `has_field()`, `clear_field()` |
//! | Message | `field() -> Option<&M>` | `field_mut() -> &mut M` | `has_field()`, `clear_field()` |
//! | Repeated | `field() -> &[T]` | `field_mut() -> &mut RepeatedField<T>` | `add_field(...)` |
//!
//! ## Modules
//!
//! - [`arena`]: Arena allocator for message data
//! - [`base`]: Core message wrapper types ([`base::TypedMessage`], [`base::OptionalMessage`])
//! - [`containers`]: Collection types ([`containers::RepeatedField`], [`containers::String`], [`containers::Bytes`])
//! - [`reflection`]: Runtime message inspection and dynamic decoding
//!
//! ## Feature Flags
//!
//! - `std` (default): Enables `std::io` integration, `Vec`-based encoding
//! - `serde_support` (default): Enables serde serialization via reflection
//! - `nightly`: Use nightly Rust features for slightly better codegen (branch hints)
//!
//! For `no_std` environments, disable default features:
//!
//! ```toml
//! [dependencies]
//! protocrap = { version = "0.1", default-features = false }
//! ```
//!
//! ## Restrictions
//!
//! Protocrap is designed for "sane" schemas:
//!
//! - Up to 256 optional fields per message
//! - Struct sizes up to 64KB
//! - Field numbers 1-2047 (1 or 2 byte wire tags)
//! - Field numbers should be mostly consecutive
//!
//! The following are intentionally unsupported:
//!
//! - **Unknown fields**: Discarded during decoding (no round-trip preservation)
//! - **Extensions**: Proto2 extensions are silently dropped
//! - **Maps**: Decoded as repeated key-value pairs
//! - **Proto3 zero-value omission**: All set fields are serialized

#![cfg_attr(feature = "nightly", feature(likely_unlikely, allocator_api))]
#![cfg_attr(not(feature = "std"), no_std)]

pub mod arena;
pub mod base;
pub mod containers;
pub mod reflection;
#[cfg(feature = "std")]
pub mod descriptor_pool;

// Re-export Allocator trait - use core on nightly, polyfill on stable
#[cfg(feature = "nightly")]
pub use core::alloc::Allocator;
#[cfg(not(feature = "nightly"))]
pub use allocator_api2::alloc::Allocator;

// Internal modules - only accessible within the crate
// Types needed by generated code are re-exported via generated_code_only
pub(crate) mod decoding;
pub(crate) mod encoding;
pub(crate) mod tables;
pub(crate) mod wire;
pub(crate) mod utils;

/// Internal types for generated code. **Do not use directly.**
#[doc(hidden)]
pub mod generated_code_only;

use crate as protocrap;
include!("descriptor.pc.rs");

#[cfg(feature = "serde_support")]
pub mod serde;

#[cfg(feature = "serde_support")]
pub mod proto_json;

#[cfg(feature = "codegen")]
pub mod codegen;

#[derive(Debug)]
pub enum Error<E = ()> {
    TreeTooDeep,
    BufferTooSmall,
    InvalidData,
    MessageNotFound,
    Io(E),
}

impl<E: core::fmt::Debug> core::fmt::Display for Error<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::TreeTooDeep => write!(f, "message tree too deep"),
            Error::BufferTooSmall => write!(f, "buffer too small"),
            Error::InvalidData => write!(f, "invalid protobuf data"),
            Error::MessageNotFound => write!(f, "message type not found"),
            Error::Io(e) => write!(f, "{:?}", e),
        }
    }
}

impl<E: core::fmt::Debug> core::error::Error for Error<E> {}

impl<E> From<E> for Error<E> {
    fn from(e: E) -> Self {
        Error::Io(e)
    }
}

// One would like to implement Default and Debug for all T: Protobuf via a blanket impl,
// but that is not allowed because Default and Debug are not local to this crate.
pub trait Protobuf: Default + core::fmt::Debug {
    fn table() -> &'static generated_code_only::Table;
    fn descriptor_proto() -> &'static google::protobuf::DescriptorProto::ProtoType {
        Self::table().descriptor
    }
}

/// Read-only protobuf operations (encode, serialize, inspect).
/// The lifetime parameter `'pool` refers to the descriptor/table pool lifetime.
pub trait ProtobufRef<'pool> {
    fn as_dyn<'msg>(&'msg self) -> reflection::DynamicMessageRef<'pool, 'msg>;

    fn encode_flat<'a, const STACK_DEPTH: usize>(
        &self,
        buffer: &'a mut [u8],
    ) -> Result<&'a [u8], Error> {
        let mut resumeable_encode = encoding::ResumeableEncode::<STACK_DEPTH>::new(self.as_dyn());
        let encoding::ResumeResult::Done(buf) = resumeable_encode
            .resume_encode(buffer)
            .ok_or(Error::TreeTooDeep)?
        else {
            return Err(Error::BufferTooSmall);
        };
        Ok(buf)
    }

    #[cfg(feature = "std")]
    fn encode_vec<const STACK_DEPTH: usize>(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = vec![0u8; 1024];
        let mut stack = Vec::new();
        let mut resumeable_encode = encoding::ResumeableEncode::<STACK_DEPTH>::new(self.as_dyn());
        loop {
            match resumeable_encode
                .resume_encode(&mut buffer)
                .ok_or(Error::TreeTooDeep)?
            {
                encoding::ResumeResult::Done(buf) => {
                    let len = buf.len();
                    let end = buffer.len();
                    let start = end - len;
                    buffer.copy_within(start..end, 0);
                    buffer.truncate(len);
                    break;
                }
                encoding::ResumeResult::NeedsMoreBuffer => {
                    let len = buffer.len().min(1024 * 1024);
                    stack.push(core::mem::take(&mut buffer));
                    buffer = vec![0u8; len * 2];
                }
            };
        }
        while let Some(old_buffer) = stack.pop() {
            buffer.extend_from_slice(&old_buffer);
        }
        Ok(buffer)
    }
}

/// Mutable protobuf operations (decode, deserialize).
/// Extends ProtobufRef with mutation capabilities.
pub trait ProtobufMut<'pool>: ProtobufRef<'pool> {
    fn as_dyn_mut<'msg>(&'msg mut self) -> reflection::DynamicMessage<'pool, 'msg>;

    #[must_use]
    fn decode_flat<const STACK_DEPTH: usize>(
        &mut self,
        arena: &mut crate::arena::Arena,
        buf: &[u8],
    ) -> bool {
        let mut decoder = decoding::ResumeableDecode::<STACK_DEPTH>::new(self.as_dyn_mut(), isize::MAX);
        if !decoder.resume(buf, arena) {
            return false;
        }
        decoder.finish(arena)
    }

    fn decode<'a, E>(
        &mut self,
        arena: &mut crate::arena::Arena,
        provider: &'a mut impl FnMut() -> Result<Option<&'a [u8]>, E>,
    ) -> Result<(), Error<E>> {
        let mut decoder = decoding::ResumeableDecode::<32>::new(self.as_dyn_mut(), isize::MAX);
        loop {
            let Some(buffer) = provider().map_err(Error::Io)? else {
                break;
            };
            if !decoder.resume(buffer, arena) {
                return Err(Error::InvalidData);
            }
        }
        if !decoder.finish(arena) {
            return Err(Error::InvalidData);
        }
        Ok(())
    }

    fn async_decode<'a, E, F>(
        &'a mut self,
        arena: &mut crate::arena::Arena,
        provider: &'a mut impl FnMut() -> F,
    ) -> impl core::future::Future<Output = Result<(), Error<E>>>
    where
        F: core::future::Future<Output = Result<Option<&'a [u8]>, E>> + 'a,
    {
        async move {
            let mut decoder = decoding::ResumeableDecode::<32>::new(self.as_dyn_mut(), isize::MAX);
            loop {
                let Some(buffer) = provider().await.map_err(Error::Io)? else {
                    break;
                };
                if !decoder.resume(buffer, arena) {
                    return Err(Error::InvalidData);
                }
            }
            if !decoder.finish(arena) {
                return Err(Error::InvalidData);
            }
            Ok(())
        }
    }

    #[cfg(feature = "std")]
    fn decode_from_bufread<const STACK_DEPTH: usize>(
        &mut self,
        arena: &mut crate::arena::Arena,
        reader: &mut impl std::io::BufRead,
    ) -> Result<(), Error<std::io::Error>> {
        let mut decoder = decoding::ResumeableDecode::<STACK_DEPTH>::new(self.as_dyn_mut(), isize::MAX);
        loop {
            let buffer = reader.fill_buf().map_err(Error::Io)?;
            let len = buffer.len();
            if len == 0 {
                break;
            }
            if !decoder.resume(buffer, arena) {
                return Err(Error::InvalidData);
            }
            reader.consume(len);
        }
        if !decoder.finish(arena) {
            return Err(Error::InvalidData);
        }
        Ok(())
    }

    #[cfg(feature = "std")]
    fn decode_from_read<const STACK_DEPTH: usize>(
        &mut self,
        arena: &mut crate::arena::Arena,
        reader: &mut impl std::io::Read,
    ) -> Result<(), Error<std::io::Error>> {
        let mut buf_reader = std::io::BufReader::new(reader);
        self.decode_from_bufread::<STACK_DEPTH>(arena, &mut buf_reader)
    }

    #[cfg(feature = "std")]
    fn decode_from_async_bufread<'a, const STACK_DEPTH: usize>(
        &'a mut self,
        arena: &'a mut crate::arena::Arena<'a>,
        reader: &mut (impl futures::io::AsyncBufRead + Unpin),
    ) -> impl core::future::Future<Output = Result<(), Error<futures::io::Error>>> {
        use futures::io::AsyncBufReadExt;

        async move {
            let mut decoder = decoding::ResumeableDecode::<STACK_DEPTH>::new(self.as_dyn_mut(), isize::MAX);
            loop {
                let buffer = reader.fill_buf().await.map_err(Error::Io)?;
                let len = buffer.len();
                if len == 0 {
                    break;
                }
                if !decoder.resume(buffer, arena) {
                    return Err(Error::InvalidData);
                }
                reader.consume_unpin(len);
            }
            if !decoder.finish(arena) {
                return Err(Error::InvalidData);
            }
            Ok(())
        }
    }

    #[cfg(feature = "std")]
    fn decode_from_async_read<'a, const STACK_DEPTH: usize>(
        &'a mut self,
        arena: &'a mut crate::arena::Arena<'a>,
        reader: &mut (impl futures::io::AsyncRead + Unpin),
    ) -> impl core::future::Future<Output = Result<(), Error<futures::io::Error>>> {
        async move {
            let mut buf_reader = futures::io::BufReader::new(reader);
            self.decode_from_async_bufread::<STACK_DEPTH>(arena, &mut buf_reader)
                .await
        }
    }

    #[cfg(feature = "serde_support")]
    fn serde_deserialize<'arena, 'alloc, 'de, D>(
        &'de mut self,
        arena: &'arena mut crate::arena::Arena<'alloc>,
        deserializer: D,
    ) -> Result<(), D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        serde::serde_deserialize_struct(self.as_dyn_mut(), arena, deserializer)
    }
}

// Blanket impl for static protobuf types
impl<T: Protobuf> ProtobufRef<'static> for T {
    fn as_dyn<'msg>(&'msg self) -> reflection::DynamicMessageRef<'static, 'msg> {
        reflection::DynamicMessageRef::new(self)
    }
}

impl<T: Protobuf> ProtobufMut<'static> for T {
    fn as_dyn_mut<'msg>(&'msg mut self) -> reflection::DynamicMessage<'static, 'msg> {
        reflection::DynamicMessage::new(self)
    }
}

#[cfg(feature = "std")]
pub mod tests {
    use crate::{Protobuf, ProtobufMut, ProtobufRef};

    #[cfg(feature = "nightly")]
    use std::alloc::Global;
    #[cfg(not(feature = "nightly"))]
    use allocator_api2::alloc::Global;

    pub fn assert_roundtrip<T: Protobuf>(msg: &T) {
        let data = msg.encode_vec::<32>().expect("msg should encode");

        let mut arena = crate::arena::Arena::new(&Global);
        let mut roundtrip_msg = T::default();
        assert!(roundtrip_msg.decode_flat::<32>(&mut arena, &data));

        println!(
            "Encoded {} ({} bytes)",
            T::table().descriptor.name(),
            data.len()
        );
        // println!("Roundtrip message: {:#?}", roundtrip_msg);

        let roundtrip_data = roundtrip_msg.encode_vec::<32>().expect("msg should encode");

        assert_eq!(roundtrip_data, data);
    }

    #[test]
    fn descriptor_accessors() {
        use crate::Protobuf;
        let file_descriptor =
            crate::google::protobuf::FileDescriptorProto::ProtoType::file_descriptor();
        let message_descriptor =
            crate::google::protobuf::DescriptorProto::ProtoType::descriptor_proto();
        let nested_descriptor =
            crate::google::protobuf::DescriptorProto::ExtensionRange::ProtoType::descriptor_proto();

        assert!(file_descriptor.name().ends_with("descriptor.proto"));
        println!("File descriptor name: {}", file_descriptor.name());
        assert_eq!(message_descriptor.name(), "DescriptorProto");
        assert_eq!(nested_descriptor.name(), "ExtensionRange");
    }

    #[test]
    fn file_descriptor_roundtrip() {
        assert_roundtrip(
            crate::google::protobuf::FileDescriptorProto::ProtoType::file_descriptor(),
        );
    }

    #[test]
    fn dynamic_file_descriptor_roundtrip() {
        let mut pool = crate::descriptor_pool::DescriptorPool::new(&Global);
        let file_descriptor =
            crate::google::protobuf::FileDescriptorProto::ProtoType::file_descriptor();
        pool.add_file(&file_descriptor);

        let bytes = file_descriptor.encode_vec::<32>().expect("should encode");
        let mut arena = crate::arena::Arena::new(&Global);
        let dynamic_file_descriptor = pool
            .decode_message("google.protobuf.FileDescriptorProto", &bytes, &mut arena)
            .expect("should decode");

        let roundtrip = dynamic_file_descriptor
            .encode_vec::<32>()
            .expect("should encode");

        assert_eq!(bytes, roundtrip);
    }

    #[test]
    fn invalid_utf8_string_rejected() {
        // FileDescriptorProto field 1 is "name" (string type)
        // Wire format: tag (field 1, wire type 2) = 0x0a, then length, then bytes
        // 0xFF is invalid UTF-8
        let invalid_utf8_name: &[u8] = &[0x0a, 0x03, 0x61, 0xFF, 0x62]; // "a<invalid>b"

        let mut arena = crate::arena::Arena::new(&Global);
        let mut msg = crate::google::protobuf::FileDescriptorProto::ProtoType::default();
        let result = msg.decode_flat::<32>(&mut arena, invalid_utf8_name);

        assert!(!result, "decoding invalid UTF-8 in string field should fail");
    }
}
