use std::{mem::MaybeUninit, ops::{Deref, DerefMut}};


#[repr(C)]
pub(crate) struct Stack<T> {
    sp: usize,
    entries: [MaybeUninit<T>],
}

impl<T> Stack<T> {
    pub(crate) fn push(&mut self, entry: T) -> Option<&mut T> {
        if self.sp == 0 {
            return None;
        }
        self.sp -= 1;
        let slot = &mut self.entries[self.sp];
        Some(slot.write(entry))
    }

    pub(crate) fn pop(&mut self) -> Option<T> {
        if self.sp == self.entries.len() {
            return None;
        }
        self.sp += 1;
        Some(unsafe { self.entries[self.sp].assume_init_read() })
    }   

    pub(crate) fn is_empty(&self) -> bool {
        self.sp == self.entries.len()
    }
}

#[repr(C)]
pub(crate) struct StackWithStorage<T, const N: usize> {
    sp: usize,
    entries: [MaybeUninit<T>; N],
}

impl<T, const N: usize> Default for StackWithStorage<T, N> {
    fn default() -> Self {
        Self {
            sp: N,
            entries: [const { MaybeUninit::uninit() }; N],
        }
    }
}

impl<T, const N: usize> Deref for StackWithStorage<T, N> {
    type Target = Stack<T>;

    fn deref(&self) -> &Self::Target {
        unsafe {
            // convert StackWithStorage<T, N> thin ptr to Stack<T> fat ptr
            let fat_ptr = std::ptr::slice_from_raw_parts(self, N) as *const Stack<T>;
            &*fat_ptr
        }
    }
}

impl<T, const N: usize> DerefMut for StackWithStorage<T, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            // convert StackWithStorage<T, N> thin ptr to Stack<T> fat ptr
            let fat_ptr = std::ptr::slice_from_raw_parts_mut(self, N) as *mut Stack<T>;
            &mut *fat_ptr
        }
    }
}
