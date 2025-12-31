//! Collection types for protobuf messages.
//!
//! This module provides arena-allocated containers used by generated protobuf code:
//!
//! - [`RepeatedField<T>`]: A growable array for repeated fields
//! - [`String`]: UTF-8 string (equivalent to protobuf `string`)
//! - [`Bytes`]: Byte array (equivalent to protobuf `bytes`)
//!
//! These types are designed for arena allocation and do not implement `Drop`.
//! Memory is freed when the arena is dropped.
//!
//! # Example
//!
//! ```
//! use protocrap::{arena::Arena, containers::{RepeatedField, String}};
//! use allocator_api2::alloc::Global;
//!
//! let mut arena = Arena::new(&Global);
//!
//! // RepeatedField for integers
//! let mut numbers = RepeatedField::<i32>::new();
//! numbers.push(1, &mut arena);
//! numbers.push(2, &mut arena);
//! assert_eq!(&numbers[..], &[1, 2]);
//!
//! // String from a str
//! let s = String::from_str("hello", &mut arena);
//! assert_eq!(s.as_str(), "hello");
//! ```

use core::alloc::Layout;
use core::fmt::Debug;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::ptr;

#[repr(C)]
#[derive(Copy, Clone)]
pub(super) struct RawVec {
    ptr: *mut u8,
    cap: usize,
}

unsafe impl Send for RawVec {}
unsafe impl Sync for RawVec {}

impl RawVec {
    const fn new() -> Self {
        // `NonNull::dangling()` doubles as "unallocated" and "zero-sized allocation"
        RawVec {
            ptr: core::ptr::null_mut(),
            cap: 0,
        }
    }

    #[inline(never)]
    fn grow(mut self, new_cap: usize, layout: Layout, arena: &mut crate::arena::Arena) -> Self {
        // since we set the capacity to usize::MAX when T has size 0,
        // getting to here necessarily means the Vec is overfull.
        assert!(layout.size() != 0, "capacity overflow");

        let (new_cap, new_layout) = if self.cap == 0 {
            if new_cap == 0 {
                (1, layout)
            } else {
                let new_layout =
                    Layout::from_size_align(layout.size() * new_cap, layout.align()).unwrap();
                (new_cap, new_layout)
            }
        } else {
            // This can't overflow because we ensure self.cap <= isize::MAX.
            let new_cap = if new_cap == 0 {
                2 * self.cap
            } else {
                assert!(new_cap > self.cap);
                new_cap
            };

            let new_layout =
                Layout::from_size_align(layout.size() * new_cap, layout.align()).unwrap();

            (new_cap, new_layout)
        };

        // Ensure that the new allocation doesn't exceed `isize::MAX` bytes.
        assert!(
            new_layout.size() <= isize::MAX as usize,
            "Allocation too large"
        );

        let new_ptr = if self.cap == 0 {
            arena.alloc_raw(new_layout).as_ptr()
        } else {
            let new_ptr = arena.alloc_raw(new_layout).as_ptr();
            unsafe { core::ptr::copy_nonoverlapping(self.ptr, new_ptr, layout.size() * self.cap) };
            new_ptr
        };

        // If allocation fails, `new_ptr` will be null, in which case we abort.
        if new_ptr.is_null() {
            // TODO: use a better error handling strategy
            panic!("allocation failed");
        }
        self.ptr = new_ptr;
        self.cap = new_cap;
        self
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub unsafe fn push_uninitialized(
        &mut self,
        len: &mut usize,
        layout: Layout,
        arena: &mut crate::arena::Arena,
    ) -> *mut u8 {
        let l = *len;
        if l == self.cap {
            *self = self.grow(0, layout, arena);
        }

        // Can't overflow, we'll OOM first.
        *len = l + 1;

        unsafe { self.ptr.add(l * layout.size()) }
    }

    pub unsafe fn pop(&mut self, len: &mut usize, layout: Layout) -> Option<*mut u8> {
        let l = *len;
        if l == 0 {
            None
        } else {
            let l = l - 1;
            let ptr = unsafe { self.ptr.add(l * layout.size()) };
            *len = l;
            Some(ptr)
        }
    }

    pub fn reserve(&mut self, new_cap: usize, layout: Layout, arena: &mut crate::arena::Arena) {
        if new_cap > self.cap {
            *self = self.grow(new_cap, layout, arena);
        }
    }
}

#[repr(C)]
pub struct RepeatedField<T> {
    buf: RawVec,
    len: usize,
    phantom: core::marker::PhantomData<T>,
}

impl<T: PartialEq> PartialEq for RepeatedField<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_ref() == other.as_ref()
    }
}

impl<T: PartialEq> PartialEq<&[T]> for RepeatedField<T> {
    fn eq(&self, other: &&[T]) -> bool {
        self.as_ref() == *other
    }
}

impl<T: Eq> Eq for RepeatedField<T> where T: Eq {}

impl<T> Default for RepeatedField<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Debug for RepeatedField<T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.as_ref().fmt(f)
    }
}

impl<T> RepeatedField<T> {
    const fn ptr(&self) -> *mut T {
        self.buf.ptr as *mut T
    }

    const fn cap(&self) -> usize {
        self.buf.cap
    }

    pub const fn new() -> Self {
        RepeatedField {
            buf: RawVec::new(),
            len: 0,
            phantom: core::marker::PhantomData,
        }
    }

    pub fn from_slice(slice: &[T], arena: &mut crate::arena::Arena) -> Self
    where
        T: Copy,
    {
        let mut rf = Self::new();
        rf.append(slice, arena);
        rf
    }

    pub const fn from_static(slice: &'static [T]) -> Self {
        RepeatedField {
            buf: RawVec {
                ptr: slice.as_ptr() as *mut u8,
                cap: slice.len(),
            },
            len: slice.len(),
            phantom: PhantomData,
        }
    }

    pub const fn slice(&self) -> &[T] {
        if self.cap() == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.ptr(), self.len) }
        }
    }

    pub fn slice_mut(&mut self) -> &mut [T] {
        if self.cap() == 0 {
            &mut []
        } else {
            unsafe { core::slice::from_raw_parts_mut(self.ptr(), self.len) }
        }
    }

    #[inline(always)]
    pub fn push(&mut self, elem: T, arena: &mut crate::arena::Arena) {
        let l = self.len;
        if l == self.cap() {
            self.buf = self.buf.grow(0, Layout::new::<T>(), arena);
        }
        unsafe { self.ptr().add(l).write(elem) };

        // Can't overflow, we'll OOM first.
        self.len = l + 1;
    }

    pub fn pop(&mut self) -> Option<T> {
        unsafe {
            self.buf
                .pop(&mut self.len, Layout::new::<T>())
                .map(|ptr| ptr.cast::<T>().read())
        }
    }

    pub fn insert(&mut self, index: usize, elem: T, arena: &mut crate::arena::Arena) {
        assert!(index <= self.len, "index out of bounds");
        let len = self.len;
        if len == self.cap() {
            self.buf = self.buf.grow(0, Layout::new::<T>(), arena);
        }

        unsafe {
            ptr::copy(
                self.ptr().add(index),
                self.ptr().add(index + 1),
                len - index,
            );
            ptr::write(self.ptr().add(index), elem);
        }

        self.len = len + 1;
    }

    pub fn remove(&mut self, index: usize) -> T {
        let len = self.len;
        assert!(index < len, "index out of bounds");

        let len = len - 1;

        unsafe {
            let result = ptr::read(self.ptr().add(index));
            ptr::copy(
                self.ptr().add(index + 1),
                self.ptr().add(index),
                len - index,
            );
            self.len = len;
            result
        }
    }

    pub fn clear(&mut self) {
        unsafe { core::ptr::drop_in_place(self.as_mut()) }
        self.len = 0
    }

    pub fn reserve(&mut self, new_cap: usize, arena: &mut crate::arena::Arena) {
        self.buf.reserve(new_cap, Layout::new::<T>(), arena);
    }

    pub fn assign(&mut self, slice: &[T], arena: &mut crate::arena::Arena)
    where
        T: Copy,
    {
        self.clear();
        self.append(slice, arena);
    }

    pub fn append(&mut self, slice: &[T], arena: &mut crate::arena::Arena)
    where
        T: Copy,
    {
        let old_len = self.len;
        self.reserve(old_len + slice.len(), arena);
        unsafe {
            self.ptr()
                .add(old_len)
                .copy_from_nonoverlapping(slice.as_ptr(), slice.len());
        }
        self.len = old_len + slice.len();
    }
}

impl<T> Deref for RepeatedField<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        self.slice()
    }
}

impl<T> DerefMut for RepeatedField<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        self.slice_mut()
    }
}

pub type Bytes = RepeatedField<u8>;

#[repr(C)]
#[derive(Default, PartialEq, Eq)]
pub struct String(Bytes);

impl core::fmt::Debug for String {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}
impl String {
    pub const fn new() -> Self {
        String(RepeatedField::new())
    }

    pub fn from_str(s: &str, arena: &mut crate::arena::Arena) -> Self {
        String(RepeatedField::from_slice(s.as_bytes(), arena))
    }

    pub const fn from_static(s: &'static str) -> Self {
        String(RepeatedField::from_static(s.as_bytes()))
    }

    pub const fn as_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(self.0.slice()) }
    }

    pub fn assign(&mut self, s: &str, arena: &mut crate::arena::Arena) {
        self.0.assign(s.as_bytes(), arena);
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

impl Deref for String {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}
