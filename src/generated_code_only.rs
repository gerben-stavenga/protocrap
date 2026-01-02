//! Internal types for generated code. **Do not use directly.**
//!
//! This module re-exports implementation details that generated code needs access to.
//! These are not part of the stable public API and may change without notice.

// Re-export table types
pub use crate::tables::{AuxTableEntry, Table, TableWithEntries};

// Re-export codec table entries
pub use crate::decoding::TableEntry as DecodeTableEntry;
pub use crate::encoding::TableEntry as EncodeTableEntry;

// Re-export wire types needed by codegen
pub use crate::wire::FieldKind;

// Re-export type-erased message types
pub use crate::base::{Message, Object};

pub const fn as_object<T: crate::Protobuf>(msg: &T) -> &crate::base::Object {
    unsafe { &*(msg as *const T as *const crate::base::Object) }
}

pub const fn as_object_mut<T: crate::Protobuf>(msg: &mut T) -> &mut crate::base::Object {
    unsafe { &mut *(msg as *mut T as *mut crate::base::Object) }
}

