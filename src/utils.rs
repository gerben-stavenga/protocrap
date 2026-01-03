use core::{
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
};

// Branch prediction hints - use core::hint on nightly, no-op on stable
#[cfg(feature = "nightly")]
pub use core::hint::likely;

#[cfg(not(feature = "nightly"))]
#[inline(always)]
pub const fn likely(b: bool) -> bool { b }

#[repr(C)]
pub(crate) struct Stack<T> {
    pub sp: usize,
    entries: [MaybeUninit<T>],
}

impl<T> Stack<T> {
    #[must_use]
    pub(crate) fn push(&mut self, entry: T) -> Option<&mut T> {
        // println!("Stack push: {:?}", &entry);
        let sp = *core::hint::black_box(&self.sp);
        if sp == 0 {
            return None;
        }
        let sp = sp - 1;
        self.sp = sp;
        let slot = &mut self.entries[sp];
        Some(slot.write(entry))
    }

    #[must_use]
    pub(crate) fn pop(&mut self) -> Option<T> {
        let sp = *core::hint::black_box(&self.sp);
        if sp == self.entries.len() {
            return None;
        }
        self.sp = sp + 1;
        let x = unsafe { self.entries[sp].assume_init_read() };
        // println!("Stack pop: {:?}", &x);
        Some(x)
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
            let fat_ptr = core::ptr::slice_from_raw_parts(self, N) as *const Stack<T>;
            &*fat_ptr
        }
    }
}

impl<T, const N: usize> DerefMut for StackWithStorage<T, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            // convert StackWithStorage<T, N> thin ptr to Stack<T> fat ptr
            let fat_ptr = core::ptr::slice_from_raw_parts_mut(self, N) as *mut Stack<T>;
            &mut *fat_ptr
        }
    }
}
