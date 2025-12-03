
// Arena allocates memory for protobuf objects. Which can be freed all at once.
// This is useful for short lived objects that are created and destroyed together.
// We need arena to be a non-generic type to avoid code bloat, but at the same time
// we want users to have full control over the allocator used by the arena. Because
// arena is batching small allocations into sporadic large allocations, we can
// allocate large blocks using the dyn Allocator trait object without too much
// overhead.
pub struct Arena<'a> {
    current: *mut MemBlock,
    allocator: &'a dyn std::alloc::Allocator,
}

// Mem block is a block of contiguous memory allocated from the 
struct MemBlock {
    prev: *mut MemBlock,
    data: [u8],
}
