use std::{ops::{Add, AddAssign, Index, Sub}, ptr::NonNull};

use crate::repeated_field::Vec;

pub mod repeated_field;

pub struct LocalCapture<'a, T> {
    value: std::mem::ManuallyDrop<T>,
    origin: &'a mut T,
}

impl<'a, T> LocalCapture<'a, T> {
    pub fn new(origin: &'a mut T) -> Self {
        Self { value: std::mem::ManuallyDrop::new(unsafe { std::ptr::read(origin) }), origin }
    }
}

impl<'a, T> std::ops::Deref for LocalCapture<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<'a, T> std::ops::DerefMut for LocalCapture<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl<'a, T> Drop for LocalCapture<'a, T> {
    fn drop(&mut self) {
        unsafe {
            std::ptr::write(self.origin, std::mem::ManuallyDrop::take(&mut self.value));
        }
    }
}

fn zigzag_decode(n: u64) -> i64 {
    ((n >> 1) as i64) ^ (-((n & 1) as i64))
}

#[derive(Clone, Copy)]
struct Cursor(NonNull<u8>);

impl Cursor {
    fn as_ptr(&self) -> *mut u8 {
        self.0.as_ptr()
    }

    fn read_varint(&mut self) -> Option<u64> {
        let mut result = 0;
        let mut extra = 0;
        for i in 0..10 {
            let b = self[i];
            if i == 9 && b != 1 {
                return None;
            }
            result ^= (b as u64) << (7 * i);
            if b < 0x80 {
                *self += i + 1;
                return Some(result ^ extra);
            }
            extra ^= 0x80 << (7 * i);
        }
        None
    }

    fn read_tag(&mut self) -> Option<u32> {
        let mut result = 0;
        let mut extra = 0;
        for i in 0..5 {
            let b = self[i];
            if i == 4 && (b == 0 || b > 15) {
                return None;
            }
            result ^= (b as u32) << (7 * i);
            if b < 0x80 {
                *self += i + 1;
                return Some(result ^ extra);
            }
            extra ^= 0x80 << (7 * i);
        }
        None
    }

    // Reads a isize varint limited to i32::MAX (used for lengths)
    fn read_size(&mut self) -> Option<isize> {
        let mut result = 0;
        let mut extra = 0;
        for i in 0..5 {
            let b = self[i];
            if i == 4 && (b == 0 || b > 7) {
                return None;
            }
            result ^= (b as isize) << (7 * i);
            if b < 0x80 {
                *self += i + 1;
                return Some(result ^ extra);
            }
            extra ^= 0x80 << (7 * i);
        }
        None
    }

    fn read_unaligned<T>(&mut self) -> T {
        let p = self.0.as_ptr();
        let value = unsafe { std::ptr::read_unaligned(p as *const T) };
        *self += std::mem::size_of::<T>() as isize;
        value
    }

    fn read_slice(&mut self, len: isize) -> &[u8] {
        let p = self.0.as_ptr();
        let slice = unsafe { std::slice::from_raw_parts(p, len as usize) };
        *self += len;
        slice
    }
}

impl PartialEq<NonNull<u8>> for Cursor {
    fn eq(&self, other: &NonNull<u8>) -> bool {
        self.as_ptr() == other.as_ptr()
    }
}

impl PartialOrd<NonNull<u8>> for Cursor {
    fn partial_cmp(&self, other: &NonNull<u8>) -> Option<std::cmp::Ordering> {
        Some(self.0.cmp(other))
    }
}

impl Add<isize> for Cursor {
    type Output = Cursor;
    fn add(self, rhs: isize) -> Self::Output {
        Cursor(unsafe { self.0.offset(rhs) })
    }
}

impl AddAssign<isize> for Cursor {
    fn add_assign(&mut self, rhs: isize) {
        *self = *self + rhs;
    }
}

impl Sub<NonNull<u8>> for Cursor {
    type Output = isize;
    fn sub(self, rhs: NonNull<u8>) -> Self::Output {
        self.0.as_ptr() as isize - rhs.as_ptr() as isize
    }
}

impl Index<isize> for Cursor {
    type Output = u8;
    fn index(&self, index: isize) -> &Self::Output {
        unsafe { &*self.0.as_ptr().offset(index) }
    }
}


const SLOP_SIZE: usize = 16;

struct TableEntry {
    tag: u8,
    kind: u8,
    data_offset: u16,
}

struct AuxTableEntry {
    offset: u32,
    child_table: *const Table,
}

pub struct Table {
    num_entries: u32,
    size: u32,
    create_fn: fn() -> &'static mut Object,
}

impl Table {
    fn entry(&self, field_number: u32) -> Option<&TableEntry> {
        if field_number >= self.num_entries {
            return None;
        }
        unsafe {
            let entries_ptr = (self as *const Table).add(1) as *const TableEntry;
            Some(&*entries_ptr.add(field_number as usize))
        }
    }

    fn aux_entry(&self, offset: u32) -> &AuxTableEntry {
        unsafe { 
            let aux_table_ptr = (self as *const Table as *const u8).add(offset as usize) as *const AuxTableEntry;
            &*aux_table_ptr
        }
    }
}

#[derive(Default, Clone, Copy)]
struct StackEntry {
    obj: *mut Object,
    table: *const Table,
    delta_limit_or_group_tag: isize,
}

const STACK_DEPTH: usize = 100;

struct ParseContext {
    obj: *mut Object,
    table: *const Table,
    limit: isize,
    depth: usize,
    stack: [StackEntry; STACK_DEPTH],
}

pub struct Object;

impl Object {
    fn ref_mut<T>(&mut self, offset: u32) -> &mut T {
        unsafe { &mut *((self as *mut Object as *mut u8).add(offset as usize) as *mut T) }
    }

    fn get_or_create_child_object<'a>(&mut self, aux_entry: &AuxTableEntry) -> (&'a mut Object, &'a Table) {
        let field = self.ref_mut::<*mut Object>(aux_entry.offset);
        let child_table = unsafe { &*aux_entry.child_table };
        let child = if (*field).is_null() {
            let child = (child_table.create_fn)();
            *field = child;
            child
        } else {
            unsafe { &mut **field }
        };
        (child, child_table)
    }
}

impl ParseContext {
    fn push_limit(&mut self, ptr: Cursor, len: isize, end: NonNull<u8>, obj: &mut Object, table: &Table) -> Option<NonNull<u8>> {
        let new_limit = ptr - end + len;
        let delta_limit = self.limit - new_limit;
        if delta_limit < 0 {
            return None;
        }
        let depth = self.depth;
        if depth == 0 {
            return None;
        }
        let depth = depth - 1;
        self.depth = depth;
        self.stack[depth] = StackEntry {
            obj,
            table,
            delta_limit_or_group_tag: delta_limit,
        };
        self.limit = new_limit;
        Some(unsafe { end.offset(new_limit.min(0)) })
    }

    fn pop_limit<'a>(&mut self) -> Option<(&'a mut Object, &'a Table)> {
        let depth = self.depth;
        if depth == STACK_DEPTH {
            return None;
        }
        self.depth = depth + 1;
        let StackEntry { obj, table, delta_limit_or_group_tag } = self.stack[depth];
        self.limit += delta_limit_or_group_tag;
        unsafe { Some((&mut *obj, &*table)) }
    }

    fn push_group(&mut self, field_number: u32, obj: &mut Object, table: &Table) -> Option<()> {
        let depth = self.depth;
        if depth == 0 {
            return None;
        }
        let depth = depth - 1;
        self.depth = depth;
        self.stack[depth] = StackEntry {
            obj,
            table,
            delta_limit_or_group_tag: -(field_number as isize),
        };
        Some(())
    }

    fn pop_group<'a>(&mut self, field_number: u32) -> Option<(&'a mut Object, &'a Table)> {
        let depth = self.depth;
        if depth == STACK_DEPTH {
            return None;
        }
        self.depth = depth + 1;
        let StackEntry { obj, table, delta_limit_or_group_tag } = self.stack[depth];
        if field_number != -delta_limit_or_group_tag as u32 {
            return None;
        }
        unsafe { Some((&mut *obj, &*table)) }
    }
}

fn parse_loop_chunk(mut ptr: Cursor, end: NonNull<u8>, obj: &mut Object, table: &Table, expected_end: u32) -> Option<Cursor> {
    while ptr < end {
        let tag = ptr.read_tag()?;
        if tag == 0 {
            // Zero tag is invalid for ending a submessage
            return None;
        }
        if (tag & 7) == 4 {
            if tag != expected_end {
                return None;
            }
            return Some(ptr);
        }
        let field_number = tag >> 3;
        let entry = table.entry(field_number)?;
        if entry.tag != tag as u8 {
            return None;
        }
        let offset = entry.data_offset as u32;
        match entry.kind {
            0 => { // varint64
                let value = ptr.read_varint()?;
                *obj.ref_mut(offset) = value;
            }
            1 => { // varint32
                let value = ptr.read_varint()?;
                *obj.ref_mut(offset) = value as u32;
            }
            2 => { // varint64 zigzag
                let value = ptr.read_varint()?;
                *obj.ref_mut(offset) = zigzag_decode(value);
            }
            3 => { // varint32 zigzag
                let value = ptr.read_varint()?;
                *obj.ref_mut(offset) = zigzag_decode(value) as u32;
            }
            4 => { // fixed64
                let value = ptr.read_unaligned::<u64>();
                *obj.ref_mut(offset) = value;
            }
            5 => { // fixed32
                let value = ptr.read_unaligned::<u32>();
                *obj.ref_mut(offset) = value;
            }
            6 => { // bytes
                let len = ptr.read_size()?;
                if ptr - end + len > 0 {
                    return None;
                }
                obj.ref_mut::<Vec<u8>>(offset).assign(ptr.read_slice(len));
            }
            7 => { // message
                let len = ptr.read_size()?;
                if ptr - end + len > 0 {
                    return None;
                }
                let aux_entry = table.aux_entry(offset);
                let (child, child_table) = obj.get_or_create_child_object(aux_entry);
                let new_end = (ptr + len).0;
                ptr = parse_loop_chunk(ptr, new_end, child, child_table, 0)?;
            }
            8 => { // start group
                let group_tag = tag + 1;
                let aux_entry = table.aux_entry(offset);
                let (child, child_table) = obj.get_or_create_child_object(aux_entry);
                ptr = parse_loop_chunk(ptr, end, child, child_table, group_tag + 1)?;
            }
            _ => {
                unreachable!()
            }
        }
    }
    if expected_end != 0 || ptr != end {
        return None;
    }
    Some(ptr)
}

fn parse_loop(mut ptr: Cursor, end: NonNull<u8>, ctx: &mut ParseContext) -> Option<Cursor> {
    let mut obj = unsafe { &mut *ctx.obj };
    let mut table = unsafe { &*ctx.table };
    loop {
        let mut limited_end = unsafe { end.offset(ctx.limit.min(0)) };
        while ptr < limited_end {
            let tag = ptr.read_tag()?;
            if tag == 0 {
                return Some(ptr);
            }
            let field_number = tag >> 3;
            if (tag & 7) == 4 {
                (obj, table) = ctx.pop_group(field_number)?;
                continue;
            }
            let entry = table.entry(field_number)?;
            if entry.tag != tag as u8 {
                return None;
            }
            let offset = entry.data_offset as u32;
            match entry.kind {
                0 => { // varint64
                    let value = ptr.read_varint()?;
                    *obj.ref_mut(offset) = value;
                }
                1 => { // varint32
                    let value = ptr.read_varint()?;
                    *obj.ref_mut(offset) = value as u32;
                }
                2 => { // varint64 zigzag
                    let value = ptr.read_varint()?;
                    *obj.ref_mut(offset) = zigzag_decode(value);
                }
                3 => { // varint32 zigzag
                    let value = ptr.read_varint()?;
                    *obj.ref_mut(offset) = zigzag_decode(value) as u32;
                }
                4 => { // fixed64
                    let value = ptr.read_unaligned::<u64>();
                    *obj.ref_mut(offset) = value;
                }
                5 => { // fixed32
                    let value = ptr.read_unaligned::<u32>();
                    *obj.ref_mut(offset) = value;
                }
                6 => { // bytes
                    let len = ptr.read_size()?;
                    if ptr - limited_end + len <= SLOP_SIZE as isize {
                        obj.ref_mut::<Vec<u8>>(offset).assign(ptr.read_slice(len));
                    } else {
                        ctx.push_limit(ptr, len, end, obj, table)?;
                        let s = obj.ref_mut::<Vec<u8>>(offset);
                        s.assign(ptr.read_slice(len - (ptr - end) + SLOP_SIZE as isize));
                        ctx.obj = s as *mut _ as *mut Object;
                        ctx.table = 1 as *const Table;
                        return Some(ptr);
                    }
                }
                7 => { // message
                    let len = ptr.read_size()?;
                    let aux_entry = table.aux_entry(offset);
                    let (child, child_table) = obj.get_or_create_child_object(aux_entry);
                    if ptr - limited_end + len <= 0 {
                        let new_end = (ptr + len).0;
                        ptr = parse_loop_chunk(ptr, new_end, child, child_table, 0)?;
                    } else {
                        limited_end = ctx.push_limit(ptr, len, end, obj, table)?;
                        (obj, table) = (child, child_table);
                    }
                }
                8 => { // start group
                    ctx.push_group(field_number, obj, table)?;
                    (obj, table) = obj.get_or_create_child_object(table.aux_entry(offset));
                }
                _ => {
                    unreachable!()
                }
            }
        }
        if ptr >= end {
            break;
        }
        if ptr != limited_end {
            return None;
        }
        (obj, table) = ctx.pop_limit()?;
    }
    Some(ptr)
}

pub fn parse(obj: &mut Object, table: &Table, buf: &[u8]) -> Option<()> {
    let mut patch_buffer = [0u8; SLOP_SIZE * 2];
    if buf.len() <= SLOP_SIZE {
        patch_buffer[..buf.len()].copy_from_slice(buf);
        let start = Cursor(NonNull::from_ref(&patch_buffer[0]));
        let end = (start + buf.len() as isize).0;
        parse_loop_chunk(start, end, obj, table, 0)?;
        Some(())
    } else {
        let mut ctx = ParseContext {
            obj,
            table,
            limit: buf.len() as isize,
            depth: 100,
            stack: [Default::default(); 100],
        };
        let start = Cursor(NonNull::from_ref(&buf[0]));
        let end = (start + buf.len() as isize).0;
        let overrun = parse_loop(start, end, &mut ctx)? - end;
        assert!(overrun >= 0 && overrun <= SLOP_SIZE as isize);
        patch_buffer[..SLOP_SIZE].copy_from_slice(buf[buf.len() - SLOP_SIZE..].as_ref());
        let start = Cursor(NonNull::from_ref(&patch_buffer[0]));
        let end = (start + SLOP_SIZE as isize).0;
        if parse_loop(start + overrun, end, &mut ctx)? == end {
            Some(())
        } else {
            None
        }
    }
}

pub struct ResumeableParse {
    overrun: isize,
    patch_buffer: [u8; SLOP_SIZE * 2],
    ctx: ParseContext,
}

impl ResumeableParse {
    pub fn new(obj: &mut Object, table: &Table, limit: isize) -> Self {
        let patch_buffer = [0u8; SLOP_SIZE * 2];
        let ctx = ParseContext {
            obj,
            table,
            limit,
            depth: 100,
            stack: [Default::default(); 100],
        };
        Self {
            overrun: SLOP_SIZE as isize,
            patch_buffer,
            ctx,
        }
    }

    pub fn resume(&mut self, buf: &[u8]) -> Option<()> {
        let size = buf.len();
        if buf.len() > SLOP_SIZE {
            self.patch_buffer[SLOP_SIZE..].copy_from_slice(&buf[..SLOP_SIZE]);
            let overrun = Self::go_parse(self.overrun, &self.patch_buffer[..SLOP_SIZE], &mut self.ctx)?;
            self.overrun = Self::go_parse(overrun, &buf[..size - SLOP_SIZE], &mut self.ctx)?;
            self.patch_buffer[..SLOP_SIZE].copy_from_slice(&buf[size - SLOP_SIZE..]);
        } else {
            self.patch_buffer[SLOP_SIZE..SLOP_SIZE + size].copy_from_slice(buf);
            self.overrun = Self::go_parse(self.overrun, &self.patch_buffer[..size], &mut self.ctx)?;
            self.patch_buffer.copy_within(size..size + SLOP_SIZE, 0);
        }
        Some(())
    }

    pub fn finish(&mut self) -> Option<()> {
        let overrun = Self::go_parse(self.overrun, &self.patch_buffer[..SLOP_SIZE], &mut self.ctx)?;
        if overrun == 0 {
            Some(())
        } else {
            None
        }
    }

    fn go_parse(overrun: isize, buf: &[u8], ctx: &mut ParseContext) -> Option<isize> {
        if overrun < buf.len() as isize {
            let start = Cursor(NonNull::from_ref(&buf[0]));
            let end = (start + buf.len() as isize).0;
            let overrun = parse_loop(start + overrun, end, ctx)? - end;
            assert!(overrun >= 0 && overrun <= SLOP_SIZE as isize);
            Some(overrun)
        } else {
            Some(overrun - buf.len() as isize)
        }
    }
}