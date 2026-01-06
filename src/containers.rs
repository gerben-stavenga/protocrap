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
//! let s = String::from_str("hello", &mut arena).unwrap();
//! assert_eq!(s.as_str(), "hello");
//! ```

use core::alloc::Layout;
use core::fmt::Debug;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::ptr;
use core::ptr::NonNull;

#[repr(C)]
#[derive(Copy, Clone)]
pub(super) struct RawVec {
    ptr: *mut u8,
    cap: usize,
}

unsafe impl Send for RawVec {}
unsafe impl Sync for RawVec {}

struct RawVecGrown {
    ptr: NonNull<u8>,
    cap: usize,
}

// assert Result<RawVecGrown, crate::Error<LayoutError>> is same size as RawVec
const _: () = assert!(core::mem::size_of::<Result<RawVecGrown, crate::Error<core::alloc::LayoutError>>>() == core::mem::size_of::<RawVec>());

impl RawVec {
    const fn new() -> Self {
        RawVec {
            ptr: core::ptr::null_mut(),
            cap: 0,
        }
    }

    #[inline(always)]
    fn grow(&mut self, new_cap: usize, layout: Layout, arena: &mut crate::arena::Arena) -> Result<(), crate::Error<core::alloc::LayoutError>> {
        let RawVecGrown { ptr, cap } = self.grow_outline(new_cap, layout, arena)?;
        self.ptr = ptr.as_ptr();
        self.cap = cap;
        Ok(())
    }

    #[inline(never)]
    fn grow_outline(self, new_cap: usize, layout: Layout, arena: &mut crate::arena::Arena) -> Result<RawVecGrown, crate::Error<core::alloc::LayoutError>> {
        // since we set the capacity to usize::MAX when T has size 0,
        // getting to here necessarily means the Vec is overfull.
        assert!(layout.size() != 0, "capacity overflow");

        let (new_cap, new_layout) = if self.cap == 0 {
            if new_cap == 0 {
                (1, layout)
            } else {
                let new_layout =
                    Layout::from_size_align(layout.size() * new_cap, layout.align())?;
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
                Layout::from_size_align(layout.size() * new_cap, layout.align())?;

            (new_cap, new_layout)
        };

        // Ensure that the new allocation doesn't exceed `isize::MAX` bytes.
        assert!(
            new_layout.size() <= isize::MAX as usize,
            "Allocation too large"
        );

        let new_ptr = if self.cap == 0 {
            arena.alloc_raw(new_layout)?
        } else {
            let new_ptr = arena.alloc_raw(new_layout)?;
            unsafe { core::ptr::copy_nonoverlapping(self.ptr, new_ptr.as_ptr(), layout.size() * self.cap) };
            new_ptr
        };

        Ok(RawVecGrown { ptr: new_ptr, cap: new_cap })
    }

    #[inline(always)]
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

    #[inline(always)]
    pub fn reserve(&mut self, new_cap: usize, layout: Layout, arena: &mut crate::arena::Arena) -> Result<(), crate::Error<core::alloc::LayoutError>> {
        if new_cap > self.cap {
            self.grow(new_cap, layout, arena)?;
        }
        Ok(())
    }


}

/// Like `Vec<T>` but arena-allocated and never drops elements.
/// Only suitable for trivial (Copy) types.
#[repr(C)]
pub struct RepeatedField<T> {
    buf: RawVec,
    len: usize,
    phantom: core::marker::PhantomData<T>,
}

impl<T: PartialEq> PartialEq for RepeatedField<T> {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.as_ref() == other.as_ref()
    }
}

impl<T: PartialEq> PartialEq<&[T]> for RepeatedField<T> {
    #[inline(always)]
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
    #[inline(always)]
    const fn ptr(&self) -> *mut T {
        self.buf.ptr as *mut T
    }

    #[inline(always)]
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

    pub fn from_slice(slice: &[T], arena: &mut crate::arena::Arena) -> Result<Self, crate::Error<core::alloc::LayoutError>>
    where
        T: Copy,
    {
        let mut rf = Self::new();
        rf.append(slice, arena)?;
        Ok(rf)
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

    #[inline(always)]
    pub const fn slice(&self) -> &[T] {
        if self.cap() == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.ptr(), self.len) }
        }
    }

    #[inline(always)]
    pub fn slice_mut(&mut self) -> &mut [T] {
        if self.cap() == 0 {
            &mut []
        } else {
            unsafe { core::slice::from_raw_parts_mut(self.ptr(), self.len) }
        }
    }

    #[inline(always)]
    pub fn push(&mut self, elem: T, arena: &mut crate::arena::Arena) -> Result<&mut T, crate::Error<core::alloc::LayoutError>> {
        let l = self.len;
        if l == self.cap() {
            self.buf.grow(0, Layout::new::<T>(), arena)?;
        }
        let res = unsafe {
            let p = self.ptr().add(l); 
            p.write(elem);
            &mut *p
        };

        // Can't overflow, we'll OOM first.
        self.len = l + 1;
        Ok(res)
    }

    #[inline(always)]
    pub fn pop(&mut self) -> Option<T> {
        unsafe {
            self.buf
                .pop(&mut self.len, Layout::new::<T>())
                .map(|ptr| ptr.cast::<T>().read())
        }
    }

    #[inline(always)]
    pub fn insert(&mut self, index: usize, elem: T, arena: &mut crate::arena::Arena) -> Result<(), crate::Error<core::alloc::LayoutError>> {
        assert!(index <= self.len, "index out of bounds");
        let len = self.len;
        if len == self.cap() {
            self.buf.grow(0, Layout::new::<T>(), arena)?;
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
        Ok(())
    }

    #[inline(always)]
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

    #[inline(always)]
    pub fn clear(&mut self) {
        self.len = 0
    }

    #[inline(always)]
    pub fn reserve(&mut self, new_cap: usize, arena: &mut crate::arena::Arena) -> Result<(), crate::Error<core::alloc::LayoutError>> {
        self.buf.reserve(new_cap, Layout::new::<T>(), arena)
    }

    #[inline(always)]
    pub fn assign(&mut self, slice: &[T], arena: &mut crate::arena::Arena) -> Result<(), crate::Error<core::alloc::LayoutError>>
    where
        T: Copy,
    {
        self.clear();
        self.append(slice, arena)
    }

    #[inline(always)]
    pub fn append(&mut self, slice: &[T], arena: &mut crate::arena::Arena) -> Result<(), crate::Error<core::alloc::LayoutError>>
    where
        T: Copy,
    {
        let old_len = self.len;
        self.reserve(old_len + slice.len(), arena)?;
        unsafe {
            self.ptr()
                .add(old_len)
                .copy_from_nonoverlapping(slice.as_ptr(), slice.len());
        }
        self.len = old_len + slice.len();
        Ok(())
    }
}

impl<T> Deref for RepeatedField<T> {
    type Target = [T];
    #[inline(always)]
    fn deref(&self) -> &[T] {
        self.slice()
    }
}

impl<T> DerefMut for RepeatedField<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut [T] {
        self.slice_mut()
    }
}

/// Alias for `RepeatedField<u8>`, used for protobuf `bytes` fields.
pub type Bytes = RepeatedField<u8>;

/// Arena-allocated UTF-8 string for protobuf `string` fields.
#[repr(C)]
#[derive(Default, PartialEq, Eq)]
pub struct String(Bytes);

impl core::fmt::Debug for String {
    #[inline(always)]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.as_str().fmt(f)
    }
}
impl String {
    pub const fn new() -> Self {
        String(RepeatedField::new())
    }

    pub fn from_str(s: &str, arena: &mut crate::arena::Arena) -> Result<Self, crate::Error<core::alloc::LayoutError>> {
        Ok(String(RepeatedField::from_slice(s.as_bytes(), arena)?))
    }

    pub const fn from_static(s: &'static str) -> Self {
        String(RepeatedField::from_static(s.as_bytes()))
    }

    #[inline(always)]
    pub const fn as_str(&self) -> &str {
        debug_assert!(core::str::from_utf8(self.0.slice()).is_ok());
        unsafe { core::str::from_utf8_unchecked(self.0.slice()) }
    }


    #[inline(always)]
    pub fn assign(&mut self, s: &str, arena: &mut crate::arena::Arena) -> Result<(), crate::Error<core::alloc::LayoutError>> {
        self.0.assign(s.as_bytes(), arena)
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

impl Deref for String {
    type Target = str;
    #[inline(always)]
    fn deref(&self) -> &str {
        self.as_str()
    }
}
