//! Arena allocator for protobuf message data.
//!
//! The [`Arena`] provides fast bump allocation for protobuf messages and their
//! contents (strings, bytes, repeated fields, sub-messages). All allocations are
//! freed together when the arena is dropped.
//!
//! # Example
//!
//! ```
//! use protocrap::arena::Arena;
//! use allocator_api2::alloc::Global;
//!
//! let mut arena = Arena::new(&Global);
//!
//! // Allocate raw memory
//! let ptr: *mut u64 = arena.alloc().unwrap(); // Returns Result<*mut u64, Error<LayoutError>>
//! unsafe { *ptr = 42; }
//!
//! // Or place a value directly
//! let value = arena.place(String::from("hello")).unwrap(); // Returns Result<&mut String, Error<LayoutError>>
//! assert_eq!(value, "hello");
//!
//! // All memory freed when arena drops
//! ```
//!
//! # Custom Allocators
//!
//! The arena accepts any `&dyn Allocator`, allowing custom memory placement:
//!
//! ```ignore
//! let mut arena = Arena::new(&my_custom_allocator);
//! ```
//!
//! Since the arena batches small allocations into large blocks, the overhead of
//! dynamic dispatch on the allocator is negligible.

use crate::Allocator;
use core::alloc::Layout;
use core::ptr;
use core::ptr::NonNull;

/// Arena allocator for protobuf message data.
///
/// Provides fast bump-pointer allocation with bulk deallocation. All memory
/// allocated from an arena is freed when the arena is dropped.
///
/// The arena grows automatically, starting with 8KB blocks and doubling up to
/// 1MB. Large allocations (those that would waste significant space in the
/// current block) get their own dedicated blocks.
pub struct Arena<'a> {
    current: *mut MemBlock,
    cursor: *mut u8,
    end: *mut u8,
    allocator: Option<&'a dyn Allocator>,
}

// Mem block is a block of contiguous memory allocated from the allocator
struct MemBlock {
    prev: *mut MemBlock,
    layout: Layout, // Layout of the entire block including header
}

const DEFAULT_BLOCK_SIZE: usize = 8 * 1024; // 8KB initial block
const MAX_BLOCK_SIZE: usize = 1024 * 1024; // 1MB max block

impl<'a> Arena<'a> {
    /// Create a new arena with the given allocator
    pub fn new(allocator: &'a dyn Allocator) -> Self {
        Self {
            current: ptr::null_mut(),
            cursor: ptr::null_mut(),
            end: ptr::null_mut(),
            allocator: Some(allocator),
        }
    }

    /// Create an arena from a pre-allocated memory slice
    pub fn from_slice(data: &'a mut [u8]) -> Self {
        debug_assert!(data.len() >= core::mem::size_of::<MemBlock>());
        Self {
            current: data.as_mut_ptr() as *mut MemBlock,
            cursor: unsafe { data.as_mut_ptr().add(core::mem::size_of::<MemBlock>()) },
            end: unsafe { data.as_mut_ptr().add(data.len()) },
            allocator: None,
        }
    }

    /// Allocate uninitialized memory for type T, returning a raw pointer
    pub fn alloc<T>(&mut self) -> Result<*mut T, crate::Error<core::alloc::LayoutError>> {
        let layout = Layout::new::<T>();
        let ptr = self.alloc_raw(layout)?;
        Ok(ptr.as_ptr() as *mut T)
    }

    pub fn place<T>(&mut self, val: T) -> Result<&'a mut T, crate::Error<core::alloc::LayoutError>> {
        let p = self.alloc::<T>()?;
        unsafe {
            p.write(val);
            Ok(&mut *p)
        }
    }

    /// Allocate an uninitialized slice of T with given length
    pub fn alloc_slice<T>(&mut self, len: usize) -> Result<*mut [T], crate::Error<core::alloc::LayoutError>> {
        let layout = Layout::array::<T>(len)?;
        let ptr = self.alloc_raw(layout)?;

        Ok(ptr::slice_from_raw_parts_mut(ptr.as_ptr() as *mut T, len))
    }

    /// Allocate raw memory with given size and alignment (uninitialized)
    #[inline]
    pub fn alloc_raw(&mut self, layout: Layout) -> Result<NonNull<u8>, crate::Error<core::alloc::LayoutError>> {
        let size = layout.size();
        let align = layout.align();

        // Align the cursor to the required alignment
        let cursor_addr = self.cursor as usize;
        let aligned_addr = (cursor_addr + align - 1) & !(align - 1);
        let aligned_cursor = aligned_addr as *mut u8;

        // Check if we have enough space: end - aligned_cursor >= size
        let available = self.end as isize - aligned_cursor as isize;
        if crate::utils::likely(available >= size as isize) {
            // Fits in current block - use it regardless of size
            self.cursor = unsafe { aligned_cursor.add(size) };
            return unsafe { Ok(NonNull::new_unchecked(aligned_cursor)) };
        }

        // Doesn't fit - need new allocation strategy
        self.alloc_outlined(layout, available as usize).ok_or(crate::Error::ArenaAllocationFailed)
    }

    /// Get total bytes allocated by this arena
    pub fn bytes_allocated(&self) -> usize {
        let mut total = 0;
        let mut current = self.current;

        unsafe {
            while !current.is_null() {
                total += (*current).layout.size();
                current = (*current).prev;
            }
        }

        total
    }

    /// Allocate a new memory block - never inlined to keep fast path small
    #[inline(never)]
    fn alloc_outlined(&mut self, layout: Layout, available: usize) -> Option<NonNull<u8>> {
        const SIGNIFICANT_SPACE_THRESHOLD: usize = 512; // 512 bytes is "significant"

        if available >= SIGNIFICANT_SPACE_THRESHOLD {
            // Significant free space left, which implies this is a large allocation
            // Keep the free space and just allocate a dedicated block for this allocation
            // and keep the current block for future allocations.
            self.alloc_dedicated(layout)
        } else {
            // Little space left - allocate new block sized for this allocation + future allocations
            self.allocate_new_block(layout)
        }
    }

    /// Allocate a new memory block
    fn allocate_new_block(&mut self, alloc_layout: Layout) -> Option<NonNull<u8>> {
        let Some(allocator) = self.allocator else {
            return None;
        };

        // Calculate block size - grow exponentially but respect min_size

        let (layout, offset) = Layout::new::<MemBlock>()
            .extend(alloc_layout)
            .expect("Layout overflow");
        let layout = layout.pad_to_align();

        let new_block_size = if self.current.is_null() {
            DEFAULT_BLOCK_SIZE
        } else {
            let current_block_size = unsafe { (*self.current).layout.size() };
            (current_block_size * 2).min(MAX_BLOCK_SIZE)
        };

        let (layout, block_start) = layout
            .extend(Layout::array::<u8>(new_block_size).expect("Layout overflow"))
            .expect("Layout overflow");
        let layout = layout.pad_to_align();

        let ptr = allocator.allocate(layout).ok()?.as_ptr() as *mut MemBlock;

        unsafe {
            // Initialize the MemBlock header
            (*ptr).prev = self.current;
            (*ptr).layout = layout;

            // Update arena state - this becomes the new active block
            self.current = ptr;
            self.cursor = (ptr as *mut u8).add(block_start);
            self.end = (ptr as *mut u8).add(layout.size());
            Some(NonNull::new_unchecked((ptr as *mut u8).add(offset)))
        }
    }

    /// Allocate a dedicated (large) memory directly from allocator (dedicated block)
    fn alloc_dedicated(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        let Some(allocator) = self.allocator else {
            return None;
        };
        // Use layout extend for proper alignment
        let memblock_layout = Layout::new::<MemBlock>();
        let (extended_layout, data_offset) =
            memblock_layout.extend(layout).expect("Layout overflow");
        let final_layout = extended_layout.pad_to_align();

        let ptr = allocator.allocate(final_layout).ok()?.as_ptr() as *mut MemBlock;

        unsafe {
            (*ptr).layout = final_layout;

            // Insert just after current head, keeping current as head
            if !self.current.is_null() {
                // Insert between current and current.prev
                (*ptr).prev = (*self.current).prev;
                (*self.current).prev = ptr;
            } else {
                // No blocks yet, this becomes the only block
                (*ptr).prev = ptr::null_mut();
                self.current = ptr;
                // Still no active bump allocation (cursor/end remain null)
            }

            // Return aligned data pointer after header
            let data_ptr = (ptr as *mut u8).add(data_offset);
            Some(NonNull::new_unchecked(data_ptr))
        }
    }
}

impl<'a> Drop for Arena<'a> {
    fn drop(&mut self) {
        let Some(allocator) = self.allocator else {
            return;
        };
        unsafe {
            let mut current = self.current;
            while !current.is_null() {
                let prev = (*current).prev;
                let layout = (*current).layout;

                // Deallocate this block with correct size
                let ptr = NonNull::new_unchecked(current as *mut u8);
                allocator.deallocate(ptr, layout);

                current = prev;
            }
        }
    }
}

// Safety: Arena can be sent between threads if the allocator supports it
unsafe impl<'a> Send for Arena<'a> where &'a dyn Allocator: Send {}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(feature = "nightly"))]
    use allocator_api2::alloc::Global;
    #[cfg(feature = "nightly")]
    use std::alloc::Global;

    #[test]
    fn test_basic_allocation() {
        let mut arena = Arena::new(&Global);

        let ptr1: *mut u32 = arena.alloc().unwrap();
        let ptr2: *mut u64 = arena.alloc().unwrap();

        unsafe {
            *ptr1 = 42;
            *ptr2 = 1337;

            assert_eq!(*ptr1, 42);
            assert_eq!(*ptr2, 1337);
        }
    }

    #[test]
    fn test_slice_allocation() {
        let mut arena = Arena::new(&Global);

        let slice_ptr: *mut [u32] = arena.alloc_slice(100).unwrap();

        unsafe {
            let slice = &mut *slice_ptr;
            slice[0] = 1;
            slice[99] = 2;

            assert_eq!(slice.len(), 100);
            assert_eq!(slice[0], 1);
            assert_eq!(slice[99], 2);
        }
    }

    #[test]
    fn test_alignment() {
        let mut arena = Arena::new(&Global);

        // Allocate types with different alignment requirements
        let _u8_ptr: *mut u8 = arena.alloc().unwrap();
        let u64_ptr: *mut u64 = arena.alloc().unwrap();

        // Check that u64 is properly aligned
        assert_eq!(u64_ptr as usize % core::mem::align_of::<u64>(), 0);
    }

    #[test]
    fn test_large_allocation() {
        let mut arena = Arena::new(&Global);

        // Allocate something larger than default block size
        let large_slice_ptr: *mut [u8] = arena.alloc_slice(DEFAULT_BLOCK_SIZE * 2).unwrap();

        unsafe {
            let large_slice = &mut *large_slice_ptr;
            large_slice[0] = 1;
            large_slice[large_slice.len() - 1] = 2;

            assert_eq!(large_slice[0], 1);
            assert_eq!(large_slice[large_slice.len() - 1], 2);
        }
    }
}
