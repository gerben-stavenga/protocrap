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

pub(crate) trait UpdateByValue: Sized {
    fn update(&mut self, update: impl FnOnce(Self) -> Self);
}

impl<T> UpdateByValue for T {
    fn update(&mut self, update: impl FnOnce(Self) -> Self) {
        unsafe {
            *self = update(core::ptr::read(self));
        }
    }
}

pub struct Ptr<T: ?Sized>(*const T);

impl<T: ?Sized> Ptr<T> {
    pub fn new(r: &T) -> Self { Ptr(r) }

    // Safe! Invariant enforced by constructor
    pub fn as_ref<'a>(&self) -> &'a T {
        unsafe { &*self.0 }
    }
}


pub struct PtrMut<T: ?Sized>(*mut T);

impl<T: ?Sized> PtrMut<T> {
    pub fn new(r: &mut T) -> Self { PtrMut(r) }

    // Safe! Invariant enforced by constructor
    pub fn as_ref<'a>(&self) -> &'a T {
        unsafe { &*self.0 }
    }

    pub fn as_mut<'a>(&mut self) -> &'a mut T {
        unsafe { &mut *self.0 }
    }
}

pub(crate) fn as_bytes<T>(slice: &[T]) -> &[u8] {
    unsafe {
        core::slice::from_raw_parts(
            slice.as_ptr() as *const u8,
            slice.len() * core::mem::size_of::<T>(),
        )
    }
}
