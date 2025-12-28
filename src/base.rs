//! Core message wrapper types.
//!
//! This module provides typed wrappers for message fields in generated code:
//!
//! - [`TypedMessage<T>`]: Non-null pointer to a message, used in repeated message fields
//! - [`OptionalMessage<T>`]: Nullable pointer to a message, used for singular message fields
//!
//! These wrappers provide type safety while maintaining `#[repr(transparent)]` layout
//! compatible with the table-driven codec.
//!
//! # Example
//!
//! ```ignore
//! // Generated code uses these types:
//! pub struct Parent {
//!     // Singular message field - may or may not be present
//!     child: OptionalMessage<Child>,
//!     // Repeated message field - each element is always present
//!     children: RepeatedField<TypedMessage<Child>>,
//! }
//!
//! // Access singular message
//! if let Some(child) = parent.child() {
//!     println!("Child name: {}", child.name());
//! }
//!
//! // Or get/initialize it
//! let child = parent.child_mut(&mut arena);
//! child.set_name("New child", &mut arena);
//!
//! // Iterate repeated messages
//! for child in parent.children() {
//!     println!("Child: {}", child.name());
//! }
//! ```

use core::alloc::Layout;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};

use crate::{
    Protobuf,
    arena::Arena,
    containers::{Bytes, RepeatedField},
};

/// Type-erased message pointer for table-driven code.
/// Used internally by the codec; prefer `OptionalMessage<T>` in generated code.
#[derive(Debug, Default, Clone, Copy)]
#[repr(C)]
pub struct Message(pub *mut Object);

unsafe impl Send for Message {}
unsafe impl Sync for Message {}

impl Message {
    pub const fn new<T>(msg: &T) -> Self {
        Message(msg as *const T as *mut T as *mut Object)
    }
}

/// A typed non-null message pointer for repeated fields.
/// Implements `Deref<Target=T>` so `&[TypedMessage<T>]` can be used like `&[&T]`.
///
/// This is `#[repr(transparent)]` over `*mut T`, making it compatible with
/// table-driven codec that treats it as `*mut Object`.
#[repr(transparent)]
pub struct TypedMessage<T: Protobuf> {
    ptr: *mut Object,
    _marker: PhantomData<T>,
}

// Safety: TypedMessage is just a pointer, safe to send/sync like Message
unsafe impl<T: Protobuf> Send for TypedMessage<T> {}
unsafe impl<T: Protobuf> Sync for TypedMessage<T> {}

impl<T: Protobuf> Clone for TypedMessage<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Protobuf> Copy for TypedMessage<T> {}

// Note: No Default impl - TypedMessage must always point to a valid message

impl<T: Protobuf> core::fmt::Debug for TypedMessage<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "TypedMessage({:?})", self.deref())
    }
}

impl<T: Protobuf> Deref for TypedMessage<T> {
    type Target = T;

    fn deref(&self) -> &T {
        // Safety: TypedMessage invariant - ptr is always valid and non-null
        unsafe { &*(self.ptr as *const T) }
    }
}

impl<T: Protobuf> DerefMut for TypedMessage<T> {
    fn deref_mut(&mut self) -> &mut T {
        // Safety: TypedMessage invariant - ptr is always valid and non-null
        unsafe { &mut *(self.ptr as *mut T) }
    }
}

impl<T: Protobuf> TypedMessage<T> {
    /// Create a new message allocated in the arena, initialized to default.
    pub fn new_in(arena: &mut Arena) -> Self {
        let obj = Object::create(core::mem::size_of::<T>() as u32, arena);
        Self {
            ptr: obj,
            _marker: PhantomData,
        }
    }

    /// Create from a static reference (for static initializers).
    pub const fn from_static(msg: &'static T) -> Self {
        Self {
            ptr: msg as *const T as *mut T as *mut Object,
            _marker: PhantomData,
        }
    }

    /// Convert to type-erased Message for table-driven code.
    pub const fn as_message(&self) -> Message {
        Message(self.ptr as *mut Object)
    }

    /// Get a reference to the underlying message (const-compatible).
    pub const fn as_ref(&self) -> &T {
        unsafe { &*(self.ptr as *const T) }
    }
}

/// A typed optional message field. Wraps a nullable pointer to T.
///
/// This is `#[repr(transparent)]` over `*mut Object`, making it compatible with
/// table-driven codec that treats it as `*mut Object`.
#[repr(transparent)]
pub struct OptionalMessage<T: Protobuf> {
    ptr: *mut Object,
    _marker: PhantomData<T>,
}

// Safety: OptionalMessage is just a pointer, safe to send/sync like Message
unsafe impl<T: Protobuf> Send for OptionalMessage<T> {}
unsafe impl<T: Protobuf> Sync for OptionalMessage<T> {}

impl<T: Protobuf> Clone for OptionalMessage<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Protobuf> Copy for OptionalMessage<T> {}

impl<T: Protobuf> Default for OptionalMessage<T> {
    fn default() -> Self {
        Self::none()
    }
}

impl<T: Protobuf> core::fmt::Debug for OptionalMessage<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.get() {
            Some(msg) => write!(f, "Some({:?})", msg),
            None => write!(f, "None"),
        }
    }
}

impl<T: Protobuf> OptionalMessage<T> {
    /// Create an empty (None) optional message.
    pub const fn none() -> Self {
        Self {
            ptr: core::ptr::null_mut(),
            _marker: PhantomData,
        }
    }

    /// Create from a static reference (for static initializers).
    pub const fn from_static(msg: &'static T) -> Self {
        Self {
            ptr: msg as *const T as *mut T as *mut Object,
            _marker: PhantomData,
        }
    }

    /// Check if the message is present.
    pub const fn is_some(&self) -> bool {
        !self.ptr.is_null()
    }

    /// Check if the message is absent.
    pub const fn is_none(&self) -> bool {
        self.ptr.is_null()
    }

    /// Get a reference to the message if present.
    pub const fn get(&self) -> Option<&T> {
        if self.ptr.is_null() {
            None
        } else {
            // Safety: ptr is non-null and points to a valid T allocated in arena
            Some(unsafe { &*(self.ptr as *const T) })
        }
    }

    /// Get a mutable reference to the message if present.
    pub fn get_mut(&mut self) -> Option<&mut T> {
        if self.ptr.is_null() {
            None
        } else {
            // Safety: ptr is non-null and points to a valid T allocated in arena
            Some(unsafe { &mut *(self.ptr as *mut T) })
        }
    }

    pub fn get_or_init(&mut self, arena: &mut Arena) -> &mut T {
        if self.ptr.is_null() {
            let obj = Object::create(core::mem::size_of::<T>() as u32, arena);
            self.ptr = obj;
        }
        // Safety: ptr is now non-null and points to a valid T allocated in arena
        unsafe { &mut *(self.ptr as *mut T) }
    }

    /// Clear the message (set to None).
    pub fn clear(&mut self) {
        self.ptr = core::ptr::null_mut();
    }

    /// Convert to type-erased Message for table-driven code.
    pub const fn as_message(&self) -> Message {
        Message(self.ptr as *mut Object)
    }
}

pub struct Object;

impl Object {
    pub fn create(size: u32, arena: &mut Arena) -> &'static mut Object {
        unsafe {
            let buffer = arena
                .alloc_raw(Layout::from_size_align_unchecked(
                    size as usize,
                    core::mem::align_of::<u64>(),
                ))
                .as_ptr();
            core::ptr::write_bytes(buffer, 0, size as usize);
            &mut *(buffer as *mut Object)
        }
    }

    pub const fn ref_at<T>(&self, offset: usize) -> &T {
        unsafe { &*((self as *const Self as *const u8).add(offset) as *const T) }
    }

    pub(crate) fn ref_mut<T>(&mut self, offset: u32) -> &mut T {
        unsafe { &mut *((self as *mut Object as *mut u8).add(offset as usize) as *mut T) }
    }

    pub const fn has_bit(&self, has_bit_idx: u8) -> bool {
        let has_bit_word = has_bit_idx as usize / 32;
        let has_bit_idx = has_bit_idx % 32;
        (*self.ref_at::<u32>(has_bit_word * core::mem::size_of::<u32>())) & (1 << has_bit_idx) != 0
    }

    pub fn set_has_bit(&mut self, has_bit_idx: u32) {
        let has_bit_word = has_bit_idx / 32;
        let has_bit_idx = has_bit_idx % 32;
        *self.ref_mut::<u32>(has_bit_word * 4) |= 1 << has_bit_idx;
    }

    pub fn clear_has_bit(&mut self, has_bit_idx: u32) {
        let has_bit_word = has_bit_idx / 32;
        let has_bit_idx = has_bit_idx % 32;
        *self.ref_mut::<u32>(has_bit_word * 4) &= !(1 << has_bit_idx);
    }

    pub(crate) fn get<T: Copy>(&self, offset: usize) -> T {
        *self.ref_at::<T>(offset)
    }

    pub(crate) fn get_slice<T>(&self, offset: usize) -> &[T] {
        self.ref_at::<RepeatedField<T>>(offset).as_ref()
    }

    pub(crate) fn set<T>(&mut self, offset: u32, has_bit_idx: u32, val: T) -> &mut T {
        self.set_has_bit(has_bit_idx);
        let field = self.ref_mut::<T>(offset);
        *field = val;
        field
    }

    pub(crate) fn add<T>(&mut self, offset: u32, val: T, arena: &mut Arena) {
        let field = self.ref_mut::<RepeatedField<T>>(offset);
        field.push(val, arena);
    }

    pub(crate) fn bytes(&self, offset: usize) -> &[u8] {
        self.ref_at::<Bytes>(offset).as_ref()
    }

    pub(crate) fn set_bytes(
        &mut self,
        offset: u32,
        has_bit_idx: u32,
        bytes: &[u8],
        arena: &mut Arena,
    ) -> &mut Bytes {
        self.set_has_bit(has_bit_idx);
        let field = self.ref_mut::<Bytes>(offset);
        field.assign(bytes, arena);
        field
    }

    pub(crate) fn add_bytes(&mut self, offset: u32, bytes: &[u8], arena: &mut Arena) -> &mut Bytes {
        let field = self.ref_mut::<RepeatedField<Bytes>>(offset);
        let b = Bytes::from_slice(bytes, arena);
        field.push(b, arena);
        field.last_mut().unwrap()
    }
}
