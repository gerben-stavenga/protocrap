# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Protocrap is a small, efficient protobuf implementation for Rust. It uses a table-driven approach with type erasure instead of generic code generation, arena allocation for memory efficiency, and a push-based streaming API that works with both sync and async code.

**Rust Edition:** 2024 | **MSRV:** 1.91

## Build Commands

```bash
# Bazel (preferred for full test suite)
./bazelisk.sh build //...
./bazelisk.sh test //...

# Cargo
cargo build
cargo build --features codegen    # With code generator
cargo test

# No-std verification
cargo build -p no-std-test --target thumbv7m-none-eabi

# Fuzz testing
cargo +nightly fuzz run decode_fuzz
```

## Code Generation Workflow

```bash
# Generate descriptor from .proto files
protoc --include_imports --descriptor_set_out=descriptor.bin my_types.proto

# Generate Rust code
protocrap-codegen descriptor.bin my_types.pc.rs
```

Update self-hosted descriptor code with `bazel run //:regen_descriptor`.

## Generated API

Each message becomes a module with a `ProtoType` struct. Accessors follow protobuf conventions:

```rust
// Optional scalar fields
msg.has_id() -> bool
msg.id() -> i32                              // Returns 0 if unset
msg.get_id() -> Option<i32>
msg.set_id(value)
msg.set_optional_id(Option<i32>)
msg.clear_id()

// Optional string/bytes fields
msg.has_name() -> bool
msg.name() -> &str                           // Returns "" if unset
msg.get_name() -> Option<&str>
msg.set_name(value, &mut arena)
msg.set_optional_name(Option<&str>, &mut arena)
msg.clear_name()

// Repeated fields
msg.items() -> &[T]
msg.items_mut() -> &mut RepeatedField<T>
msg.add_items(&mut arena) -> &mut T          // For message types

// Submessages (no default - avoids memory for default instances)
msg.has_options() -> bool
msg.options() -> Option<&OptionsType>
msg.options_mut(&mut arena) -> &mut OptionsType  // Creates if unset
msg.clear_options()
```

All fields have explicit presence (unified optionality). The wire format's intrinsic optionality is exposed directly—no proto2/proto3 semantic differences.

## Architecture

### Core Design Principles

1. **Table-Driven**: Generated code produces only struct definitions and static lookup tables. A single non-generic library function handles all parsing/serialization.

2. **Type Erasure**: Uses `dyn` traits instead of generics to avoid monomorphization bloat.

3. **Arena Allocation**: Bump-pointer allocator with bulk deallocation. Supports custom allocators via `&dyn Allocator`.

4. **Push-Based Streaming**: Parser takes `(state, buffer) -> updated_state`, enabling single implementation for sync/async.

### Key Modules

- `arena.rs` - Bump-pointer allocator with 8KB-1MB blocks
- `base.rs` - `TypedMessage<T>`, `OptionalMessage<T>`, type-erased `Message`
- `decoding.rs` - Push-based parser with `ResumeableDecode<STACK_DEPTH>`
- `encoding.rs` - Push-based serializer with `ResumeableEncode<STACK_DEPTH>`
- `reflection.rs` - Runtime introspection via `DynamicMessageRef`/`DynamicMessage`
- `codegen/generator.rs` - Main code generation from FileDescriptorSet

### Message Layout

Messages use `#[repr(C)]` with metadata array for has-bits and oneof discriminants:
```rust
pub struct Message {
    metadata: [u32; N],    // Has-bits (bit 7=0) or oneof discriminants (bit 7=1)
    field1: Type1,
    // ...
}
```

### Trait API

- `ProtobufRef<'pool>` - Read-only: encode, serialize, inspect
- `ProtobufMut<'pool>` - Mutable: decode, deserialize

## Intentional Limitations

- Up to 127 optional fields per message
- Struct sizes up to 1KB
- Field numbers 1-2047 (should be mostly consecutive)
- Unknown fields discarded (no round-trip preservation)
- Maps decoded as repeated key-value pairs
- Proto2 extensions silently dropped

## Testing

- **Conformance tests**: `bazel test //conformance:conformance_test` (2685 passes, 102 expected failures)
- **Unit tests**: `cargo test`
- **Fuzz tests**: `cargo +nightly fuzz run decode_fuzz`

## Feature Flags

- `std` (default) - `std::io` integration, Vec-based encoding
- `serde_support` (default) - Serde via reflection
- `nightly` - Branch hints (`likely`/`unlikely`)
- `codegen` - Full code generation
