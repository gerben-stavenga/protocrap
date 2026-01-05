# Protocrap 🦀
* Also known as Crap'n proto ⚓

A small efficient and flexible protobuf implementation for rust

## Why Protocrap?

Unlike other protobuf implementations that focus on feature completeness, Protocrap prioritizes:
- **Tiny footprint** - Minimal code size for library, generated code, and binaries
- **Lightning-fast compilation** - No macro-heavy codegen or generic explosion
- **Ultimate flexibility** - Custom allocators, async-ready, streaming support
- **Blazing performance** - Zero-cost abstractions where it matters

## Design Philosophy

### TL;DR 🚀
- **🚫 No Generics/Macros**: Type erasure at boundaries → smaller binaries, faster compilation
- **📊 Table-Driven**: Static lookup tables instead of generated code → tiny footprint  
- **🏗️ Arena Allocation**: Pointer increment + bulk deallocation → speed + flexibility
- **🌊 Push-Based Streaming**: Pure functions instead of pull APIs → works everywhere (sync/async)

---

This protobuf implementation prioritizes four key design goals:
- Small code size for the library, generated code, and resulting binaries
- Fast compile times  
- Flexibility for niche use cases, especially around memory allocation
- Uncompromising efficiency in parsing and serialization

These goals drive the following architectural decisions:

### No Generics, No Rust Macros

Generics create significant code bloat and compilation slowdowns—Serde exemplifies this problem perfectly. We achieve the same flexibility through type erasure using `dyn` traits and type punning. By carefully placing this type erasure at interface boundaries, we eliminate performance overhead while often improving instruction cache utilization through code deduplication. Our Arena allocation and parsing APIs demonstrate this approach effectively.

The few generic functions in our API are small, inline-only helpers that encapsulate dispatch logic to non-generic library functions.

### Table-Driven Implementation

Rather than generating parsing and serialization code, we handle these operations through non-generic library functions. Code generation from proto IDL files produces only Rust struct definitions with their accessors, plus extremely efficient static lookup tables that drive the parsing and serialization logic.

### Arena Allocation

Arena allocation serves two critical purposes in this library:

**Efficiency**: While modern allocators are extremely fast, arena allocation reduces allocation to a simple pointer increment in the common case—and this gets inlined at the call site. More importantly, it eliminates the need to drop individual nodes in object trees. Traditional dropping requires a costly post-order traversal of often cold memory, whereas arenas allow us to deallocate entire object trees by dropping just a few large blocks.

**Flexibility**: Since actual allocator calls only happen when requesting new blocks, we can use `&dyn Allocator` without significant performance impact. This gives users complete control over memory placement while serving as a highly efficient type eraser.

**Fallible Allocation**: All arena operations return `Result`, allowing custom allocators to enforce memory limits and reject allocations. This gives you explicit control over memory budgets—your allocator decides how much memory to admit, and protocrap properly propagates those decisions as errors rather than panicking.

### A Push API for Stream Parsing and Serialization

Most serialization frameworks limit themselves to flat buffers as sources and sinks—a restrictive approach. The Rust standard library's Read/Write and BufRead/BufWrite traits provide better abstractions, but they're generic traits that force code duplication for each implementation. Using dynamic dispatch for individual operations touching just a few bytes creates unacceptable overhead.

The standard solution uses some `&dyn BufferStream` streaming buffer chunks, such that the cost of dynamic dispatch is amortized over many operations. This is a pull (callback) API where the protobuf library requests buffers as needed—the approach used by Google's protobuf implementation.

However, pull APIs lack the flexibility our design goals demand. When parsing and the next buffer isn't ready from disk or network, the buffer stream must block. This breaks async task systems. We could define an async stream, but then the parser must `.await` buffers, making the entire parser async and unusable outside of async contexts. Creating separate sync and async parsers contradicts our design principles, see code bloat.

Our solution: a push API. Both parser and serializer become pure functions with the signature `(internal_state, buffer) -> updated_state`. Instead of the protobuf pulling buffers in its inner loops, users push chunks in an outer loop by calling the parse function. This approach supports both synchronous and asynchronous streams without requiring separate implementations. It also eliminates trait compatibility issues—no more situations where third-party crate streams lack the necessary traits and Rust's orphan rules prevent you from implementing them.

## Restrictions

Every framework comes with some limits (often unspecified) to where you can push things. For instance template instantiation recursion limits of a c++ compiler. For a serialization framework these involve how many fields can a schema have, etc.. We are very principled here, we support only _sane_ schemas, so no thousands of fields with arbitrary field numbers in the tens of millions. We support
1) up to 127 optional fields (and 127 oneofs)
2) Struct sizes up to 1kb
3) Field numbers up to 1..2047, these are all 1 or 2 byte tags. 

Field numbers should just be assigned consecutive 1 ... max field. We do tolerate holes, but we do not compress field numbers, assigning a single field struct a field number of 2000. Will lead to a very big table. These restrictions simplify code and allows compression of has bit index and offset of fields into a single 16 bit integer, which makes for compact tables.

See [SPECS.md](SPECS.md) for detailed specifications.

### Intentionally Unsupported Features

- **Unknown fields**: Discarded during parsing. Round-tripping data through parse+serialize may lose unknown fields—but if you're not modifying the data, why reserialize?
- **Extensions**: Proto2 extensions are silently dropped. A confusing feature that causes more pain than it solves.
- **Maps**: Not yet supported. Treated as repeated fields of key/value pairs.

## Quick Example
```rust
use protocrap::*;

// Create a message
let mut arena = arena::Arena::new(&std::alloc::Global);
let mut msg = MyMessage::default();
msg.set_name("Hello", &mut arena);
msg.set_id(42);

// Encode to bytes
let bytes = msg.encode_vec::<32>()?;

// Decode from bytes
let mut decoded = MyMessage::default();
decoded.decode_flat::<32>(&mut arena, &bytes);

// Async decoding
let mut async_msg = MyMessage::default();
async_msg.decode_from_async_bufread::<32>(&mut arena, &mut async_reader).await?;
```

## Examples

See [pc-example](https://github.com/gerben-stavenga/pc-example) for a complete example project using protocrap.

## Code Generation

Generate Rust code from protobuf definitions:

```bash
# Install the code generator
cargo install protocrap --features codegen

# Generate descriptor set from .proto file
protoc --include_imports --descriptor_set_out=descriptor.bin my_types.proto

# Generate Rust code
protocrap descriptor.bin my_types.pc.rs
```

Or build and run from source:

```bash
cargo run --features codegen -- descriptor.bin my_types.pc.rs
```

Include the generated code in your crate:

```rust
use protocrap;
include!("my_types.pc.rs");
```

### Embedding Static Data

Embed protobuf data as compile-time constants - no lazy init, no mutex, just a const:

```bash
# From binary .pb file
protocrap descriptor.bin --embed config.pb:my.package.Config -o config.pc.rs

# From JSON (easier to maintain in version control)
protocrap descriptor.bin --embed config.json:my.package.Config -o config.pc.rs
```

Use in your code:

```rust
const CONFIG: my::package::Config::ProtoType = include!("config.pc.rs");
```

The data goes straight to `.rodata` - zero runtime overhead.

For working on protocrap itself (regenerating descriptor.pc.rs):

```bash
# Regenerate using bootstrap codegen (protocrap from crates.io)
bazel run //:regen_descriptor
```

## Runtime Reflection

Protocrap includes a powerful reflection API for dynamic message inspection:

```rust
use protocrap::reflection::{DescriptorPool, DynamicMessage};

// Create a descriptor pool and load file descriptors
let mut pool = DescriptorPool::new(&std::alloc::Global);
pool.add_file(&file_descriptor);

// Decode a message dynamically
let dynamic_msg = pool.decode_message("package.MessageType", &bytes)?;

// Inspect fields at runtime
for field in dynamic_msg.descriptor().field() {
    if let Some(value) = dynamic_msg.get_field(field) {
        println!("{}: {:?}", field.name(), value);
    }
}
```

## Testing

Protocrap is validated through multiple testing approaches:

- **Conformance tests**: Runs Google's official protobuf conformance test suite in two modes:
  - *Static mode*: Tests generated Rust structs and their encoding/decoding
  - *Dynamic mode*: Tests the runtime reflection descriptor pool
- **Decode fuzzer**: Fuzz testing with `cargo-fuzz` to find edge cases in the decoder
- **Unit tests**: Coverage of core functionality and edge cases

```bash
# Run conformance tests via Bazel
~/bin/bazelisk test //conformance:conformance_test
~/bin/bazelisk test //conformance:conformance_test_dynamic

# Run fuzz tests (requires nightly)
cargo +nightly fuzz run decode_fuzz
```

## Features

- **Serde support**: Optional serde serialization/deserialization via reflection
- **No-std compatible**: Works in embedded environments (with `no_std` feature)
- **Custom allocators**: Full control over memory placement via Arena API
- **Async support**: First-class async/await support without code duplication

## Status

🚧 **Beta** - This library is not yet fully matured. Expect:
- **API changes**: The public API may change in breaking ways between versions
- **Potential bugs**: While tested via conformance tests and fuzzing, edge cases may exist
- **Missing features**: Maps are not implemented and encoding still needs some optimization

Use in production at your own risk. Bug reports and contributions welcome!

### Progress

- [x] Basic parsing and serialization
- [x] Arena allocation with custom allocator support
- [x] Table-driven codec
- [x] Code generation from .proto files
- [x] Runtime reflection API
- [x] Serde integration
- [x] Async/sync streaming support
- [x] Default value support
- [x] Self-hosted (uses own implementation to parse descriptor.proto)
- [x] Conformance test suite (static + dynamic modes)
- [x] Fuzz testing
- [x] Oneof support
- [x] Static data embedding (const protobuf from .pb/.json)
- [x] Full documentation
- [ ] Map support

