# Protocrap Specification

This document defines the design decisions, constraints, and behavior of protocrap.

## Design Principles

1. **No Generics/Macros** - Type erasure at boundaries for smaller binaries and faster compilation
2. **Table-Driven Codec** - Static lookup tables instead of generated per-message code
3. **Arena Allocation** - Pointer increment allocation with bulk deallocation
4. **Push-Based Streaming** - `(state, buffer) -> updated_state` signature for sync/async compatibility

## Memory Layout

### Message Structs

All generated message types use `#[repr(C)]` with a fixed layout:

```rust
#[repr(C)]
pub struct Message {
    metadata: [u32; N],   // Has-bits and oneof discriminants
    field1: Type1,
    field2: Type2,
    // ...
}
```

### Metadata Array

- Array of `u32` at offset 0 of every message
- Stores both has-bits (presence tracking) and oneof discriminants
- Has-bits: each optional field claims one bit, indexed sequentially
- Oneof discriminants: stored as u32 values indicating which field is set

### Metadata Index Encoding

Field metadata indices are stored in a `u8`:
- Bit 7 (MSB): 0 = optional field (has-bit), 1 = oneof field (discriminant)
- Bits 0-6: index into the appropriate part of the metadata array

This allows up to 127 optional fields and 127 oneofs per message.

### Getters and Defaults

Struct fields store the wire value (or zero if not present). Getter methods return the schema-defined default when the has-bit is not set:

```rust
pub fn name(&self) -> &str {
    if self.has_name() { &self.name } else { "default_value" }
}
```

## Wire Format Support

### Fully Supported

| Feature | Proto2 | Proto3 | Notes |
|---------|--------|--------|-------|
| Scalar types | ✓ | ✓ | int32, int64, uint32, uint64, sint32, sint64, fixed32, fixed64, sfixed32, sfixed64, float, double, bool |
| Strings | ✓ | ✓ | UTF-8 validated |
| Bytes | ✓ | ✓ | |
| Enums | ✓ | ✓ | Sign-extended as i32 |
| Nested messages | ✓ | ✓ | |
| Repeated fields | ✓ | ✓ | Both packed and unpacked |
| Optional fields | ✓ | ✓ | Has-bit tracking |
| Default values | ✓ | ✓ | Proto2 custom defaults supported |
| Field merging | ✓ | ✓ | Last value wins for scalars, merge for messages |
| Groups | ✓ | - | Proto2 only |
| Oneof | ✓ | ✓ | Discriminant in metadata array |

### Intentionally Unsupported

These features are **not supported by design** and will not be implemented:

| Feature | Behavior |
|---------|----------|
| Unknown fields | Silently discarded during decoding. Not preserved or round-tripped. |
| Extensions | Dropped (treated as unknown fields). |
| MessageSet encoding | Not supported. |

### Not Yet Implemented

| Feature | Status |
|---------|--------|
| Maps | Parsed as repeated key-value pairs, full map semantics pending |

## Optionality Model

Protocrap uses **unified optionality**:

- All fields (proto2 and proto3) have has-bits
- Fields are serialized if their has-bit is set, even if the value is zero/default
- This differs from proto3 spec which omits default values

## Required Fields

Proto2 `required` fields are treated identically to `optional`:

- No presence validation on decode or encode
- No error if a required field is missing
- Reflection API could add validation if needed

This is by design - required fields are a proto2 mistake that proto3 removed.

## Restrictions

| Restriction | Limit |
|-------------|-------|
| Optional fields per message | 127 (index stored in u8, MSB distinguishes oneof vs optional) |
| Struct size | 1KB max |
| Field numbers | 1..2047 (1-2 byte tags) |
| Field number gaps | Tolerated but create larger tables |

## JSON Format

JSON support is basic and lower priority than binary format.

### Supported
- Primitive types (numbers, strings, bools)
- Nested messages
- Repeated fields
- Duration and Timestamp (partial)

### Not Supported
- Enum string names (integers only)
- Any type
- FieldMask
- Struct/Value/ListValue
- Wrapper types
- Base64url for bytes (standard base64 only)
- Strict validation (overflow, format checks)

## Conformance Status

Against the official protobuf conformance test suite:

| Metric | Count |
|--------|-------|
| Successes | 2685 |
| Expected failures | 102 |
| Unexpected failures | 0 |

### Failure Breakdown

| Category | Reason |
|----------|--------|
| Unknown fields | By design - not preserved |
| Extensions/MessageSet | By design - not supported |
| Proto3 default omission | Unified optionality |
| JSON edge cases | Well-known types, strict validation |

## Panic Safety

Protocrap never panics on malformed input. All errors are returned as `Result` types.
