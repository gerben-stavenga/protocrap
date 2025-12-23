# Protocrap TODO List

## Critical - Blocking Conformance Tests

### Codegen: Package/Module Deduplication
- [x] **FIXED: Merge FileDescriptorProtos with same package** (2024-12-23)
  - Implemented package tree structure in `codegen/src/generator.rs`
  - Properly handles hierarchical packages (e.g., `protobuf_test_messages.proto2` and `protobuf_test_messages.proto3`)
  - Also handles enum aliasing by deduplicating enum variants with same value
  - Conformance tests now compile successfully!

### Build Script Integration
- [x] **FIXED: build.rs cargo deadlock issue** (2024-12-23)
  - Implemented workaround: Find pre-built codegen binary instead of `cargo run`
  - Pattern works: looks for binary in target/release or target/debug
  - Future improvement: Create `protocrap-build` crate with compile_protos() API (similar to `prost-build`)
  - See: `conformance/build.rs` for working implementation

## High Priority

### Conformance Tests
- [ ] **Complete conformance test setup** (95% done - ready to run!)
  - ✅ Test protocol implementation (parse → serialize roundtrip)
  - ✅ Varint read/write for test runner protocol
  - ✅ Binary compiled and runs
  - ✅ Package deduplication fixed - test messages compile
  - ✅ Generated code uses proper module structure
  - ❌ Need to download/build Google's conformance_test_runner
  - ❌ Need to run actual conformance tests
  - Location: `conformance/` directory, binary at `target/debug/conformance-protocrap`

- [ ] **Proto Package → Rust Crate Design** (architectural decision needed)
  - Option 1: Create `protocrap-types` crate for Google well-known types (recommended)
  - Option 2: Merge packages in codegen (current blocker fix)
  - Option 3: One crate per package root (too many crates)
  - Option 4: External types flag `--external-types protocrap_types`
  - See discussion: impedance mismatch between proto packages and Rust crates
  - Recommendation: Short-term fix #2, long-term implement #1 + #4

### Code Quality & Completeness
- [ ] **Enum default values** (`codegen/src/generator.rs:428`)
  - Implement support for enum default values in code generation
  - Currently returns `None` for enum defaults

- [ ] **Better error handling** (`src/containers.rs:72`)
  - Replace panic on allocation failure with proper error propagation
  - Consider using Result types or custom error handling strategy

- [ ] **Serde null handling** (`src/serde.rs:412`)
  - Handle null values properly when deserializing message fields
  - Currently has TODO comment but may silently fail

### Code Generation Improvements
- [ ] **Name resolution hardcoding** (`codegen/src/static_gen.rs:13`)
  - Replace hardcoded name resolution in `full_name()` function
  - Should use proper scope/package resolution from descriptor
  - Currently has manual mapping for ExtensionRange, ReservedRange, etc.

- [ ] **Suppress false dead_code warnings in codegen**
  - Add `#![allow(dead_code)]` to codegen modules
  - Functions like `generate_file_set`, `generate_message`, etc. ARE used but Rust's analysis misses them
  - These are internal implementation details called through the public `generate()` API

### Performance
- [ ] **Optimize write_tag** (`src/wire.rs:241`)
  - Current implementation just calls write_varint
  - Could be optimized for common tag sizes

## Medium Priority

### Feature Completeness
- [ ] **Map support**
  - README states maps aren't supported (treated as repeated key/value pairs)
  - Decide: fully implement map syntax or document current limitation better
  - MessageOptions.map_entry field exists in descriptor but not used in codegen

- [ ] **Unknown fields handling**
  - Currently skipped during parsing (documented design decision)
  - Consider if there are use cases that need this

- [ ] **Extensions support**
  - Currently not supported (documented design decision)
  - Verify this is acceptable for target use cases

### Documentation
- [ ] **API documentation**
  - Add rustdoc comments to public APIs
  - Document design patterns (arena usage, push-based parsing, etc.)
  - Add examples to README

- [ ] **Usage guide**
  - Expand README with real-world usage examples
  - Document the reflection API and DescriptorPool
  - Add guide for async usage patterns

- [ ] **Migration guide**
  - Document differences from prost/protobuf
  - Explain when to use protocrap vs alternatives

## Low Priority

### Testing
- [ ] **Expand test coverage**
  - More edge cases in codegen-tests
  - Test with larger/more complex schemas
  - Async parsing tests

- [ ] **Benchmarks**
  - Complete benchmark suite comparing to prost
  - Document performance characteristics
  - Memory usage profiling

### Tooling
- [ ] **Better error messages**
  - Improve decode error reporting (currently just "decode error")
  - Add context about what failed and why

- [ ] **Cargo integration**
  - Consider build.rs helper for proto compilation
  - Publishing to crates.io checklist

## Completed ✓
- [x] Fix codegen package/module deduplication (2024-12-23)
  - Implemented package tree structure for hierarchical packages
  - Fixed duplicate module errors in conformance tests
- [x] Fix enum aliasing in codegen (2024-12-23)
  - Deduplicate enum variants with same numeric value
  - Rust doesn't support aliased enums like protobuf does
- [x] Fix build.rs cargo deadlock (2024-12-23)
  - Use pre-built codegen binary instead of `cargo run`
- [x] Conformance test infrastructure (2024-12-23)
  - Binary compiles and runs successfully
  - Ready for Google conformance test runner
- [x] Fix alignment bugs in has_bit operations (2024-12-22)
- [x] Fix decode table has_bit calculation (2024-12-22)
- [x] Support latest descriptor.proto (2024-12-22)
- [x] Bootstrap mode in generate_descriptor.sh (2024-12-22)

## Notes

### Design Decisions (Not TODOs)
These are intentional limitations per the design philosophy:
- No generic-heavy code (type erasure preferred)
- Max 64 optional fields per message
- Field numbers 1-2047 only
- No extensions or unknown field preservation
- Struct sizes up to 1KB

### Won't Fix
- Complex macro-based codegen (against design principles)
- Generic explosion for type safety (performance/compile-time trade-off)
