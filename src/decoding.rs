use core::mem::MaybeUninit;
use core::ptr::NonNull;

use crate::base::{Message, Object};
use crate::containers::{Bytes, RepeatedField};
use crate::reflection::DynamicMessage;
use crate::tables::Table;
use crate::utils::{Ptr, PtrMut, Stack, StackWithStorage, UpdateByValue};
use crate::wire::{FieldKind, ReadCursor, SLOP_SIZE, zigzag_decode};

#[cfg(feature = "std")]
const TRACE_TAGS: bool = false;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TableEntry(pub u32);

impl TableEntry {
    pub const fn new(kind: FieldKind, has_bit_idx: u32, offset: usize) -> Self {
        TableEntry(((offset & 0xFFFF) as u32) << 16 | has_bit_idx << 8 | (kind as u8 as u32))
    }

    pub(crate) fn kind(&self) -> FieldKind {
        debug_assert!((self.0 as u8) <= FieldKind::RepeatedGroup as u8);
        unsafe { core::mem::transmute(self.0 as u8) }
    }

    pub(crate) fn has_bit_idx(&self) -> u32 {
        (self.0 >> 8) & 0xFF
    }

    pub(crate) fn offset(&self) -> u32 {
        self.0 >> 16
    }

    pub(crate) fn aux_offset(&self) -> u32 {
        self.0 >> 16
    }
}

impl Table {
    #[inline(always)]
    pub(crate) fn entry(&self, field_number: u32) -> Option<TableEntry> {
        let entries = self.decode_entries();
        if field_number >= entries.len() as u32 {
            return None;
        }
        Some(entries[field_number as usize])
    }

    #[inline(always)]
    pub(crate) fn aux_entry_decode(&self, entry: TableEntry) -> (u32, &Table) {
        let offset = entry.aux_offset();
        self.aux_entry(offset as usize)
    }
}

struct StackEntry {
    obj_table: Option<(PtrMut<Object>, Ptr<Table>)>,
    delta_limit_or_group_tag: isize,
}

impl StackEntry {
    fn into_context<'a>(
        self,
        mut limit: isize,
        field_number: Option<u32>,
    ) -> Option<DecodeObjectState<'a>> {
        if let Some(field_number) = field_number {
            if -self.delta_limit_or_group_tag != field_number as isize {
                return None;
            }
        } else {
            if self.delta_limit_or_group_tag < 0 {
                return None;
            }
            limit += self.delta_limit_or_group_tag;
        }
        let Some((mut obj, table)) = self.obj_table else {
            unreachable!("popped stack entry with null obj/table in non-group context");
        };
        Some(DecodeObjectState {
            limit,
            msg: DynamicMessage {
                object: obj.as_mut(),
                table: table.as_ref(),
            },
        })
    }
}

enum DecodeObject<'a> {
    None,
    Message(DynamicMessage<'a, 'a>),
    /// Bytes field with optional UTF-8 validation flag (true = validate as string)
    Bytes(&'a mut Bytes, bool),
    SkipLengthDelimited,
    SkipGroup,
    PackedU64(&'a mut RepeatedField<u64>),
    PackedU32(&'a mut RepeatedField<u32>),
    PackedI64Zigzag(&'a mut RepeatedField<i64>),
    PackedI32Zigzag(&'a mut RepeatedField<i32>),
    PackedBool(&'a mut RepeatedField<bool>),
    PackedFixed64(&'a mut RepeatedField<u64>),
    PackedFixed32(&'a mut RepeatedField<u32>),
}

#[repr(C)]
struct DecodeObjectState<'a> {
    limit: isize, // relative to end
    msg: DynamicMessage<'a, 'a>,
}

fn calc_limited_end(end: NonNull<u8>, limit: isize) -> NonNull<u8> {
    unsafe { end.offset(limit.min(0)) }
}

impl<'a> DecodeObjectState<'a> {
    fn limited_end(&self, end: NonNull<u8>) -> NonNull<u8> {
        calc_limited_end(end, self.limit)
    }

    #[inline(always)]
    fn push_limit(
        &mut self,
        len: isize,
        cursor: ReadCursor,
        end: NonNull<u8>,
        stack: &mut Stack<StackEntry>,
    ) -> Option<NonNull<u8>> {
        let new_limit = cursor - end + len;
        let delta_limit = self.limit - new_limit;
        if delta_limit < 0 {
            return None;
        }
        stack.push(StackEntry {
            obj_table: Some((PtrMut::new(self.msg.object), Ptr::new(self.msg.table))),
            delta_limit_or_group_tag: delta_limit,
        })?;
        self.limit = new_limit;
        Some(self.limited_end(end))
    }

    #[inline(always)]
    fn pop_limit(
        &mut self,
        end: NonNull<u8>,
        stack: &mut Stack<StackEntry>,
    ) -> Option<NonNull<u8>> {
        *self = stack.pop()?.into_context(self.limit, None)?;
        Some(self.limited_end(end))
    }

    #[inline(always)]
    fn push_group(&mut self, field_number: u32, stack: &mut Stack<StackEntry>) -> Option<()> {
        stack.push(StackEntry {
            obj_table: Some((PtrMut::new(self.msg.object), Ptr::new(self.msg.table))),
            delta_limit_or_group_tag: -(field_number as isize),
        })?;
        Some(())
    }

    #[inline(always)]
    fn pop_group(&mut self, field_number: u32, stack: &mut Stack<StackEntry>) -> Option<()> {
        *self = stack.pop()?.into_context(self.limit, Some(field_number))?;
        Some(())
    }

    #[inline(always)]
    fn set<T>(&mut self, entry: TableEntry, field_number: u32, val: T) {
        let has_bit_idx = entry.has_bit_idx();
        if has_bit_idx & 0x80 != 0 {
            // Oneof field: has_bit_idx stores discriminant word index with 0x80 flag
            let discriminant_word_idx = has_bit_idx & 0x7F;
            self.msg
                .object
                .set_oneof(entry.offset(), discriminant_word_idx, field_number, val);
        } else {
            self.msg.object.set(entry.offset(), has_bit_idx, val);
        }
    }

    #[inline(always)]
    fn add<T>(&mut self, entry: TableEntry, val: T, arena: &mut crate::arena::Arena) {
        self.msg.object.add(entry.aux_offset(), val, arena);
    }

    #[inline(always)]
    fn set_bytes<'b>(
        &'b mut self,
        entry: TableEntry,
        field_number: u32,
        slice: &[u8],
        arena: &mut crate::arena::Arena,
    ) -> &'b mut Bytes {
        let has_bit_idx = entry.has_bit_idx();
        if has_bit_idx & 0x80 != 0 {
            // Oneof field
            let discriminant_word_idx = has_bit_idx & 0x7F;
            self.msg.object.set_bytes_oneof(
                entry.offset(),
                discriminant_word_idx,
                field_number,
                slice,
                arena,
            )
        } else {
            self.msg
                .object
                .set_bytes(entry.offset(), has_bit_idx, slice, arena)
        }
    }

    #[inline(always)]
    fn get_or_create_child_object(
        self,
        entry: TableEntry,
        arena: &mut crate::arena::Arena,
    ) -> Result<DynamicMessage<'a, 'a>, crate::Error<core::alloc::LayoutError>> {
        let (offset, child_table) = self.msg.table.aux_entry_decode(entry);
        let field = self.msg.object.ref_mut::<Message>(offset);
        let child = if field.is_null() {
            let child = Object::create(child_table.size as u32, arena)?;
            *field = Message::new(child);
            child
        } else {
            field.as_mut()
        };
        Ok(DynamicMessage {
            object: child,
            table: child_table,
        })
    }

    #[inline(always)]
    fn add_child_object(
        &mut self,
        entry: TableEntry,
        arena: &mut crate::arena::Arena,
    ) -> Result<DynamicMessage<'a, 'a>, crate::Error<core::alloc::LayoutError>> {
        let (offset, child_table) = self.msg.table.aux_entry_decode(entry);
        let field = self
            .msg
            .object
            .ref_mut::<RepeatedField<*mut Object>>(offset);
        let child = Object::create(child_table.size as u32, arena)?;
        field.push(child, arena);
        Ok(DynamicMessage {
            object: child,
            table: child_table,
        })
    }
}

type DecodeLoopResult<'a> = Option<(ReadCursor, isize, DecodeObject<'a>)>;

#[inline(never)]
fn skip_length_delimited<'a>(
    limit: isize,
    mut cursor: ReadCursor,
    end: NonNull<u8>,
    stack: &mut Stack<StackEntry>,
    arena: &mut crate::arena::Arena,
) -> DecodeLoopResult<'a> {
    if limit > SLOP_SIZE as isize {
        cursor.read_slice(SLOP_SIZE as isize - (cursor - end));
        return Some((cursor, limit, DecodeObject::SkipLengthDelimited));
    }
    cursor.read_slice(limit - (cursor - end));
    let stack_entry = stack.pop()?;
    if stack_entry.obj_table.is_none() {
        debug_assert!(stack_entry.delta_limit_or_group_tag >= 0);
        return skip_group(
            limit + stack_entry.delta_limit_or_group_tag,
            cursor,
            end,
            stack,
            arena,
        );
    }
    let ctx = stack_entry.into_context(limit, None)?;
    decode_loop(ctx, cursor, end, stack, arena)
}

#[inline(never)]
fn skip_group<'a>(
    limit: isize,
    mut cursor: ReadCursor,
    end: NonNull<u8>,
    stack: &mut Stack<StackEntry>,
    arena: &mut crate::arena::Arena,
) -> DecodeLoopResult<'a> {
    let limited_end = calc_limited_end(end, limit);
    // loop popping the stack as needed
    loop {
        // inner parse loop
        while cursor < limited_end {
            let tag = cursor.read_tag()?;
            let wire_type = tag & 7;
            let field_number = tag >> 3;
            if field_number == 0 {
                return None;
            }
            #[cfg(feature = "std")]
            if TRACE_TAGS {
                eprintln!(
                    "Skipping unknown field with field number {} and wire type {}",
                    field_number, wire_type
                );
            }
            match wire_type {
                0 => {
                    // varint
                    let _ = cursor.read_varint()?;
                }
                1 => {
                    // fixed64
                    cursor += 8;
                }
                2 => {
                    // length-delimited
                    let len = cursor.read_size()?;
                    debug_assert!(len >= 0);
                    if cursor - limited_end + len <= SLOP_SIZE as isize {
                        cursor.read_slice(len);
                    } else {
                        let new_limit = cursor - end + len;
                        let delta_limit = limit - new_limit;
                        if delta_limit < 0 {
                            return None;
                        }
                        stack.push(StackEntry {
                            obj_table: None,
                            delta_limit_or_group_tag: delta_limit,
                        })?;
                        return Some((cursor, new_limit, DecodeObject::SkipLengthDelimited));
                    }
                }
                3 => {
                    // start group
                    stack.push(StackEntry {
                        obj_table: None,
                        delta_limit_or_group_tag: -(field_number as isize),
                    })?;
                }
                4 => {
                    // end group
                    let stack_entry = stack.pop()?;
                    if -stack_entry.delta_limit_or_group_tag != field_number as isize {
                        return None;
                    }
                    if let Some((mut obj, table)) = stack_entry.obj_table {
                        let ctx = DecodeObjectState {
                            limit,
                            msg: DynamicMessage {
                                object: obj.as_mut(),
                                table: table.as_ref(),
                            },
                        };
                        return decode_loop(ctx, cursor, end, stack, arena);
                    }
                }
                5 => {
                    // fixed32
                    cursor += 4;
                }
                _ => {
                    return None;
                }
            }
        }
        if cursor - end == limit {
            if stack.is_empty() {
                return Some((cursor, limit, DecodeObject::None));
            }
            let stack_entry = stack.pop()?;
            if stack_entry.obj_table.is_none() {
                // We are at a limit but we haven't finished this group, so parse failed
                return None;
            }
            let ctx = stack_entry.into_context(limit, None)?;
            // TODO: this relies on tail call optimization
            return decode_loop(ctx, cursor, end, stack, arena);
        }
        if cursor >= end {
            break;
        }
        if cursor >= limited_end {
            return None;
        }
    }
    Some((cursor, limit, DecodeObject::SkipGroup))
}

#[inline(always)]
fn unpack_varint<T>(
    field: &mut RepeatedField<T>,
    mut cursor: ReadCursor,
    limited_end: NonNull<u8>,
    arena: &mut crate::arena::Arena,
    decode_fn: impl Fn(u64) -> T,
) -> Option<ReadCursor> {
    while cursor < limited_end {
        let val = cursor.read_varint()?;
        field.push(decode_fn(val), arena);
    }
    Some(cursor)
}

#[inline(always)]
fn unpack_fixed<T>(
    field: &mut RepeatedField<T>,
    mut cursor: ReadCursor,
    limited_end: NonNull<u8>,
    arena: &mut crate::arena::Arena,
) -> ReadCursor {
    while cursor < limited_end {
        let val = cursor.read_unaligned::<T>();
        field.push(val, arena);
    }
    cursor
}

#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn decode_packed<'a, T>(
    limit: isize,
    field: &'a mut RepeatedField<T>,
    cursor: ReadCursor,
    end: NonNull<u8>,
    stack: &mut Stack<StackEntry>,
    arena: &mut crate::arena::Arena,
    decode_fn: impl Fn(u64) -> T,
    decode_obj: impl Fn(&'a mut RepeatedField<T>) -> DecodeObject<'a>,
) -> DecodeLoopResult<'a> {
    if limit > 0 {
        let cursor = unpack_varint(field, cursor, end, arena, decode_fn)?;
        return Some((cursor, limit, decode_obj(field)));
    }
    let limited_end = calc_limited_end(end, limit);
    let cursor = unpack_varint(field, cursor, limited_end, arena, decode_fn)?;
    let ctx = stack.pop()?.into_context(limit, None)?;
    decode_loop(ctx, cursor, end, stack, arena)
}

#[inline(never)]
fn decode_fixed<'a, T>(
    limit: isize,
    field: &'a mut RepeatedField<T>,
    cursor: ReadCursor,
    end: NonNull<u8>,
    stack: &mut Stack<StackEntry>,
    arena: &mut crate::arena::Arena,
    decode_obj: impl Fn(&'a mut RepeatedField<T>) -> DecodeObject<'a>,
) -> DecodeLoopResult<'a> {
    if limit > 0 {
        let cursor = unpack_fixed(field, cursor, end, arena);
        return Some((cursor, limit, decode_obj(field)));
    }
    let limited_end = calc_limited_end(end, limit);
    let cursor = unpack_fixed(field, cursor, limited_end, arena);
    let ctx = stack.pop()?.into_context(limit, None)?;
    decode_loop(ctx, cursor, end, stack, arena)
}

#[inline(never)]
fn decode_string<'a>(
    limit: isize,
    bytes: &'a mut Bytes,
    validate_utf8: bool,
    mut cursor: ReadCursor,
    end: NonNull<u8>,
    stack: &mut Stack<StackEntry>,
    arena: &mut crate::arena::Arena,
) -> DecodeLoopResult<'a> {
    if limit > SLOP_SIZE as isize {
        bytes.append(
            cursor.read_slice(SLOP_SIZE as isize - (cursor - end)),
            arena,
        );
        return Some((cursor, limit, DecodeObject::Bytes(bytes, validate_utf8)));
    }
    bytes.append(cursor.read_slice(limit - (cursor - end)), arena);
    // Validate UTF-8 for string fields
    if validate_utf8 && core::str::from_utf8(bytes.slice()).is_err() {
        return None;
    }
    let ctx = stack.pop()?.into_context(limit, None)?;
    decode_loop(ctx, cursor, end, stack, arena)
}

#[inline(never)]
fn decode_loop<'a>(
    mut ctx: DecodeObjectState<'a>,
    mut cursor: ReadCursor,
    end: NonNull<u8>,
    stack: &mut Stack<StackEntry>,
    arena: &mut crate::arena::Arena,
) -> DecodeLoopResult<'a> {
    let mut limited_end = ctx.limited_end(end);
    // loop popping the stack as needed
    loop {
        // inner parse loop
        'parse_loop: while cursor < limited_end {
            let tag = cursor.read_tag()?;
            let field_number = tag >> 3;
            #[cfg(feature = "std")]
            if TRACE_TAGS {
                let descriptor = ctx.msg.table.descriptor;
                let field = descriptor
                    .field()
                    .iter()
                    .find(|f| f.number() as u32 == field_number);
                if let Some(field) = field {
                    eprintln!(
                        "Msg {} Field number: {}, Field name {}, wire type {}",
                        descriptor.name(),
                        field_number,
                        field.name(),
                        tag & 7
                    );
                } else {
                    // field not found in descriptor, treat as unknown
                    eprintln!(
                        "Msg {} Unknown Field number: {}, wire type {}",
                        descriptor.name(),
                        field_number,
                        tag & 7
                    );
                }
            }
            if let Some(entry) = ctx.msg.table.entry(field_number) {
                'unknown: {
                    match entry.kind() {
                        FieldKind::Varint64 => {
                            if tag & 7 != 0 {
                                break 'unknown;
                            };
                            ctx.set(entry, field_number, cursor.read_varint()?);
                        }
                        FieldKind::Varint32 | FieldKind::Int32 => {
                            if tag & 7 != 0 {
                                break 'unknown;
                            };
                            ctx.set(entry, field_number, cursor.read_varint()? as u32);
                        }
                        FieldKind::Varint64Zigzag => {
                            if tag & 7 != 0 {
                                break 'unknown;
                            };
                            ctx.set(entry, field_number, zigzag_decode(cursor.read_varint()?));
                        }
                        FieldKind::Varint32Zigzag => {
                            if tag & 7 != 0 {
                                break 'unknown;
                            };
                            ctx.set(
                                entry,
                                field_number,
                                zigzag_decode(cursor.read_varint()? as u32 as u64) as i32,
                            );
                        }
                        FieldKind::Bool => {
                            if tag & 7 != 0 {
                                break 'unknown;
                            };
                            let val = cursor.read_varint()?;
                            ctx.set(entry, field_number, val != 0);
                        }
                        FieldKind::Fixed64 => {
                            if tag & 7 != 1 {
                                break 'unknown;
                            };
                            ctx.set(entry, field_number, cursor.read_unaligned::<u64>());
                        }
                        FieldKind::Fixed32 => {
                            if tag & 7 != 5 {
                                break 'unknown;
                            };
                            ctx.set(entry, field_number, cursor.read_unaligned::<u32>());
                        }
                        FieldKind::Bytes | FieldKind::String => {
                            if tag & 7 != 2 {
                                break 'unknown;
                            };
                            let validate_utf8 = entry.kind() == FieldKind::String;
                            let len = cursor.read_size()?;
                            if cursor - limited_end + len <= SLOP_SIZE as isize {
                                let slice = cursor.read_slice(len);
                                if validate_utf8 && core::str::from_utf8(slice).is_err() {
                                    return None;
                                }
                                ctx.set_bytes(entry, field_number, slice, arena);
                            } else {
                                ctx.push_limit(len, cursor, end, stack)?;

                                let DecodeObjectState { limit, msg } = ctx;

                                let has_bit_idx = entry.has_bit_idx();
                                let slice = cursor.read_slice(SLOP_SIZE as isize - (cursor - end));
                                let bytes = if has_bit_idx & 0x80 != 0 {
                                    // Oneof field
                                    let discriminant_word_idx = has_bit_idx & 0x7F;
                                    msg.object.set_bytes_oneof(
                                        entry.offset(),
                                        discriminant_word_idx,
                                        field_number,
                                        slice,
                                        arena,
                                    )
                                } else {
                                    msg.object
                                        .set_bytes(entry.offset(), has_bit_idx, slice, arena)
                                };

                                return Some((
                                    cursor,
                                    limit,
                                    DecodeObject::Bytes(bytes, validate_utf8),
                                ));
                            }
                        }
                        FieldKind::Message => {
                            if tag & 7 != 2 {
                                break 'unknown;
                            };
                            // For oneof message fields, set the discriminant
                            let has_bit_idx = entry.has_bit_idx();
                            if has_bit_idx & 0x80 != 0 {
                                let discriminant_word_idx = has_bit_idx & 0x7F;
                                *ctx.msg.object.ref_mut::<u32>(discriminant_word_idx * 4) =
                                    field_number;
                            }
                            let len = cursor.read_size()?;
                            limited_end = ctx.push_limit(len, cursor, end, stack)?;

                            ctx.update(|ctx| {
                                let limit = ctx.limit;
                                let msg = ctx.get_or_create_child_object(entry, arena).unwrap();
                                DecodeObjectState { limit, msg }
                            });
                        }
                        FieldKind::Group => {
                            if tag & 7 != 3 {
                                break 'unknown;
                            };
                            ctx.push_group(field_number, stack)?;
                            ctx.update(|ctx| {
                                let limit = ctx.limit;
                                let msg = ctx.get_or_create_child_object(entry, arena).unwrap();
                                DecodeObjectState { limit, msg }
                            });
                        }
                        FieldKind::RepeatedVarint64 => {
                            if tag & 7 == 0 {
                                // Unpacked
                                ctx.add(entry, cursor.read_varint()?, arena);
                            } else if tag & 7 == 2 {
                                // Packed
                                let len = cursor.read_size()?;

                                // Fast path: entire packed field fits in buffer
                                if cursor - limited_end + len <= 0 {
                                    let field = ctx
                                        .msg
                                        .object
                                        .ref_mut::<RepeatedField<u64>>(entry.offset());
                                    let end = (cursor + len).0;
                                    cursor = unpack_varint(field, cursor, end, arena, |v| v)?;
                                    if cursor != end {
                                        return None;
                                    }
                                } else {
                                    // Slow path: field spans buffers - transition to resumable parsing
                                    ctx.push_limit(len, cursor, end, stack)?;
                                    let field = ctx
                                        .msg
                                        .object
                                        .ref_mut::<RepeatedField<u64>>(entry.offset());
                                    cursor = unpack_varint(field, cursor, end, arena, |v| v)?;
                                    return Some((
                                        cursor,
                                        ctx.limit,
                                        DecodeObject::PackedU64(field),
                                    ));
                                }
                            } else {
                                break 'unknown;
                            }
                        }
                        FieldKind::RepeatedVarint32 | FieldKind::RepeatedInt32 => {
                            if tag & 7 == 0 {
                                // Unpacked
                                ctx.add(entry, cursor.read_varint()? as u32, arena);
                            } else if tag & 7 == 2 {
                                // Packed
                                let len = cursor.read_size()?;

                                // Fast path: entire packed field fits in buffer
                                if cursor - limited_end + len <= 0 as isize {
                                    let field = ctx
                                        .msg
                                        .object
                                        .ref_mut::<RepeatedField<u32>>(entry.offset());
                                    let end = (cursor + len).0;
                                    cursor =
                                        unpack_varint(field, cursor, end, arena, |v| v as u32)?;
                                    if cursor != end {
                                        return None;
                                    }
                                } else {
                                    // Slow path: field spans buffers - transition to resumable parsing
                                    ctx.push_limit(len, cursor, end, stack)?;
                                    let field = ctx
                                        .msg
                                        .object
                                        .ref_mut::<RepeatedField<u32>>(entry.offset());
                                    cursor =
                                        unpack_varint(field, cursor, end, arena, |v| v as u32)?;
                                    return Some((
                                        cursor,
                                        ctx.limit,
                                        DecodeObject::PackedU32(field),
                                    ));
                                }
                            } else {
                                break 'unknown;
                            }
                        }
                        FieldKind::RepeatedVarint64Zigzag => {
                            if tag & 7 == 0 {
                                // Unpacked
                                ctx.add(entry, zigzag_decode(cursor.read_varint()?), arena);
                            } else if tag & 7 == 2 {
                                // Packed
                                let len = cursor.read_size()?;

                                // Fast path: entire packed field fits in buffer
                                if cursor - limited_end + len <= 0 as isize {
                                    let field = ctx
                                        .msg
                                        .object
                                        .ref_mut::<RepeatedField<i64>>(entry.offset());
                                    let end = (cursor + len).0;
                                    cursor = unpack_varint(field, cursor, end, arena, |v| {
                                        zigzag_decode(v)
                                    })?;
                                    if cursor != end {
                                        return None;
                                    }
                                } else {
                                    // Slow path: field spans buffers - transition to resumable parsing
                                    ctx.push_limit(len, cursor, end, stack)?;
                                    let field = ctx
                                        .msg
                                        .object
                                        .ref_mut::<RepeatedField<i64>>(entry.offset());
                                    cursor = unpack_varint(field, cursor, end, arena, |v| {
                                        zigzag_decode(v)
                                    })?;
                                    return Some((
                                        cursor,
                                        ctx.limit,
                                        DecodeObject::PackedI64Zigzag(field),
                                    ));
                                }
                            } else {
                                break 'unknown;
                            }
                        }
                        FieldKind::RepeatedVarint32Zigzag => {
                            if tag & 7 == 0 {
                                // Unpacked
                                ctx.add(
                                    entry,
                                    zigzag_decode(cursor.read_varint()? as u32 as u64) as i32,
                                    arena,
                                );
                            } else if tag & 7 == 2 {
                                // Packed
                                let len = cursor.read_size()?;

                                // Fast path: entire packed field fits in buffer
                                if cursor - limited_end + len <= 0 {
                                    let field = ctx
                                        .msg
                                        .object
                                        .ref_mut::<RepeatedField<i32>>(entry.offset());
                                    let end = (cursor + len).0;
                                    cursor = unpack_varint(field, cursor, end, arena, |v| {
                                        zigzag_decode(v as u32 as u64) as i32
                                    })?;
                                    if cursor != end {
                                        return None;
                                    }
                                } else {
                                    // Slow path: field spans buffers - transition to resumable parsing
                                    ctx.push_limit(len, cursor, end, stack)?;
                                    let field = ctx
                                        .msg
                                        .object
                                        .ref_mut::<RepeatedField<i32>>(entry.offset());
                                    cursor = unpack_varint(field, cursor, end, arena, |v| {
                                        zigzag_decode(v as u32 as u64) as i32
                                    })?;
                                    return Some((
                                        cursor,
                                        ctx.limit,
                                        DecodeObject::PackedI32Zigzag(field),
                                    ));
                                }
                            } else {
                                break 'unknown;
                            }
                        }
                        FieldKind::RepeatedBool => {
                            if tag & 7 == 0 {
                                // Unpacked
                                let val = cursor.read_varint()?;
                                ctx.add(entry, val != 0, arena);
                            } else if tag & 7 == 2 {
                                // Packed
                                let len = cursor.read_size()?;

                                // Fast path: entire packed field fits in buffer
                                if cursor - limited_end + len <= 0 {
                                    let field = ctx
                                        .msg
                                        .object
                                        .ref_mut::<RepeatedField<bool>>(entry.offset());
                                    let end = (cursor + len).0;
                                    cursor = unpack_varint(field, cursor, end, arena, |v| v != 0)?;
                                    if cursor != end {
                                        return None;
                                    }
                                } else {
                                    // Slow path: field spans buffers - transition to resumable parsing
                                    ctx.push_limit(len, cursor, end, stack)?;
                                    let field = ctx
                                        .msg
                                        .object
                                        .ref_mut::<RepeatedField<bool>>(entry.offset());
                                    cursor = unpack_varint(field, cursor, end, arena, |v| v != 0)?;
                                    return Some((
                                        cursor,
                                        ctx.limit,
                                        DecodeObject::PackedBool(field),
                                    ));
                                }
                            } else {
                                break 'unknown;
                            }
                        }
                        FieldKind::RepeatedFixed64 => {
                            if tag & 7 == 1 {
                                // Unpacked
                                ctx.add(entry, cursor.read_unaligned::<u64>(), arena);
                            } else if tag & 7 == 2 {
                                // Packed
                                let len = cursor.read_size()?;

                                // Fast path: entire packed field fits in buffer
                                if cursor - limited_end + len <= 0 {
                                    let field = ctx
                                        .msg
                                        .object
                                        .ref_mut::<RepeatedField<u64>>(entry.offset());
                                    let end = (cursor + len).0;
                                    cursor = unpack_fixed(field, cursor, end, arena);
                                    if cursor != end {
                                        return None;
                                    }
                                } else {
                                    // Slow path: field spans buffers - transition to resumable parsing
                                    ctx.push_limit(len, cursor, end, stack)?;
                                    let field = ctx
                                        .msg
                                        .object
                                        .ref_mut::<RepeatedField<u64>>(entry.offset());
                                    cursor = unpack_fixed(field, cursor, end, arena);
                                    return Some((
                                        cursor,
                                        ctx.limit,
                                        DecodeObject::PackedFixed64(field),
                                    ));
                                }
                            } else {
                                break 'unknown;
                            }
                        }
                        FieldKind::RepeatedFixed32 => {
                            if tag & 7 == 5 {
                                // Unpacked
                                ctx.add(entry, cursor.read_unaligned::<u32>(), arena);
                            } else if tag & 7 == 2 {
                                // Packed
                                let len = cursor.read_size()?;

                                // Fast path: entire packed field fits in buffer
                                if cursor - limited_end + len <= 0 {
                                    let field = ctx
                                        .msg
                                        .object
                                        .ref_mut::<RepeatedField<u32>>(entry.offset());
                                    let end = (cursor + len).0;
                                    cursor = unpack_fixed(field, cursor, end, arena);
                                    if cursor != end {
                                        return None;
                                    }
                                } else {
                                    // Slow path: field spans buffers - transition to resumable parsing
                                    ctx.push_limit(len, cursor, end, stack)?;
                                    let field = ctx
                                        .msg
                                        .object
                                        .ref_mut::<RepeatedField<u32>>(entry.offset());
                                    cursor = unpack_fixed(field, cursor, end, arena);
                                    return Some((
                                        cursor,
                                        ctx.limit,
                                        DecodeObject::PackedFixed32(field),
                                    ));
                                }
                            } else {
                                break 'unknown;
                            }
                        }
                        FieldKind::RepeatedBytes | FieldKind::RepeatedString => {
                            if tag & 7 != 2 {
                                break 'unknown;
                            };
                            let validate_utf8 = entry.kind() == FieldKind::RepeatedString;
                            let len = cursor.read_size()?;
                            if cursor - limited_end + len <= SLOP_SIZE as isize {
                                let slice = cursor.read_slice(len);
                                if validate_utf8 && core::str::from_utf8(slice).is_err() {
                                    return None;
                                }
                                ctx.msg.object.add_bytes(entry.aux_offset(), slice, arena);
                            } else {
                                ctx.push_limit(len, cursor, end, stack)?;
                                let DecodeObjectState { limit, msg } = ctx;
                                let slice = cursor.read_slice(SLOP_SIZE as isize - (cursor - end));
                                let bytes = msg.object.add_bytes(entry.aux_offset(), slice, arena);
                                return Some((
                                    cursor,
                                    limit,
                                    DecodeObject::Bytes(bytes, validate_utf8),
                                ));
                            }
                        }
                        FieldKind::RepeatedMessage => {
                            if tag & 7 != 2 {
                                break 'unknown;
                            };
                            let len = cursor.read_size()?;
                            limited_end = ctx.push_limit(len, cursor, end, stack)?;
                            ctx.msg = ctx.add_child_object(entry, arena).ok()?;
                        }
                        FieldKind::RepeatedGroup => {
                            if tag & 7 != 3 {
                                break 'unknown;
                            };
                            ctx.push_group(field_number, stack)?;
                            ctx.msg = ctx.add_child_object(entry, arena).ok()?;
                        }
                        FieldKind::Unknown => {
                            break 'unknown;
                        }
                    }
                    continue 'parse_loop;
                }
            };
            // unknown field
            if field_number == 0 {
                if tag == 0 {
                    // 0 byte can used to terminate parsing, but only if stack is empty
                    if stack.is_empty() {
                        return Some((cursor, ctx.limit, DecodeObject::None));
                    }
                    return None;
                }
                // field number 0 is invalid
                return None;
            }
            match tag & 7 {
                0 => {
                    // varint
                    let _ = cursor.read_varint()?;
                }
                1 => {
                    // fixed64
                    cursor += 8;
                }
                2 => {
                    // length-delimited
                    let len = cursor.read_size()?;
                    if cursor - limited_end + len <= SLOP_SIZE as isize {
                        cursor.read_slice(len);
                    } else {
                        ctx.push_limit(len, cursor, end, stack)?;
                        return Some((cursor, ctx.limit, DecodeObject::SkipLengthDelimited));
                    }
                }
                3 => {
                    // start group
                    // push to stack until end group
                    ctx.push_group(field_number, stack)?;
                    return skip_group(ctx.limit, cursor, end, stack, arena);
                }
                4 => {
                    // end group
                    ctx.pop_group(field_number, stack)?;
                }
                5 => {
                    // fixed32
                    cursor += 4;
                }
                _ => {
                    return None;
                }
            }
        }
        if cursor - end == ctx.limit {
            if stack.is_empty() {
                return Some((cursor, ctx.limit, DecodeObject::None));
            }
            limited_end = ctx.pop_limit(end, stack)?;
            continue;
        }
        if cursor >= end {
            break;
        }
        if cursor != limited_end {
            return None;
        }
    }
    Some((cursor, ctx.limit, DecodeObject::Message(ctx.msg)))
}

struct ResumeableState<'a> {
    limit: isize,
    object: DecodeObject<'a>,
    overrun: isize,
}

impl<'a> ResumeableState<'a> {
    fn go_decode(
        mut self,
        buf: &[u8],
        stack: &mut Stack<StackEntry>,
        arena: &mut crate::arena::Arena,
    ) -> Option<Self> {
        let len = buf.len() as isize;
        self.limit -= len;
        if self.overrun >= len {
            self.overrun -= len;
            return Some(self);
        }
        let (mut cursor, end) = ReadCursor::new(buf);
        cursor += self.overrun;
        let (new_cursor, new_limit, new_object) = match self.object {
            DecodeObject::Message(msg) => {
                let ctx = DecodeObjectState {
                    limit: self.limit,
                    msg,
                };
                decode_loop(ctx, cursor, end, stack, arena)?
            }
            DecodeObject::Bytes(bytes, validate_utf8) => {
                decode_string(self.limit, bytes, validate_utf8, cursor, end, stack, arena)?
            }
            DecodeObject::SkipLengthDelimited => {
                skip_length_delimited(self.limit, cursor, end, stack, arena)?
            }
            DecodeObject::SkipGroup => skip_group(self.limit, cursor, end, stack, arena)?,
            DecodeObject::PackedU64(field) => decode_packed(
                self.limit,
                field,
                cursor,
                end,
                stack,
                arena,
                |v| v,
                DecodeObject::PackedU64,
            )?,
            DecodeObject::PackedU32(field) => decode_packed(
                self.limit,
                field,
                cursor,
                end,
                stack,
                arena,
                |v| v as u32,
                DecodeObject::PackedU32,
            )?,
            DecodeObject::PackedI64Zigzag(field) => decode_packed(
                self.limit,
                field,
                cursor,
                end,
                stack,
                arena,
                zigzag_decode,
                DecodeObject::PackedI64Zigzag,
            )?,
            DecodeObject::PackedI32Zigzag(field) => decode_packed(
                self.limit,
                field,
                cursor,
                end,
                stack,
                arena,
                |v| zigzag_decode(v as u32 as u64) as i32,
                DecodeObject::PackedI32Zigzag,
            )?,
            DecodeObject::PackedBool(field) => decode_packed(
                self.limit,
                field,
                cursor,
                end,
                stack,
                arena,
                |v| v != 0,
                DecodeObject::PackedBool,
            )?,
            DecodeObject::PackedFixed64(field) => {
                decode_fixed(self.limit, field, cursor, end, stack, arena, |f| {
                    DecodeObject::PackedFixed64(f)
                })?
            }
            DecodeObject::PackedFixed32(field) => {
                decode_fixed(self.limit, field, cursor, end, stack, arena, |f| {
                    DecodeObject::PackedFixed32(f)
                })?
            }
            DecodeObject::None => unreachable!(),
        };
        self.limit = new_limit;
        self.object = new_object;
        self.overrun = new_cursor - end;
        Some(self)
    }
}

#[repr(C)]
pub struct ResumeableDecode<'a, const STACK_DEPTH: usize> {
    state: MaybeUninit<ResumeableState<'a>>,
    patch_buffer: [u8; SLOP_SIZE * 2],
    stack: StackWithStorage<StackEntry, STACK_DEPTH>,
}

impl<'a, const STACK_DEPTH: usize> ResumeableDecode<'a, STACK_DEPTH> {
    pub fn new<'pool: 'a>(msg: crate::reflection::DynamicMessage<'pool, 'a>, limit: isize) -> Self {
        let object = DecodeObject::Message(msg);
        Self {
            state: MaybeUninit::new(ResumeableState {
                limit,
                object,
                overrun: SLOP_SIZE as isize,
            }),
            patch_buffer: [0; SLOP_SIZE * 2],
            stack: Default::default(),
        }
    }

    #[must_use]
    pub fn resume(&mut self, buf: &[u8], arena: &mut crate::arena::Arena) -> bool {
        self.resume_impl(buf, arena).is_some()
    }

    #[must_use]
    pub fn finish(self, arena: &mut crate::arena::Arena) -> bool {
        let ResumeableDecode {
            state,
            patch_buffer,
            mut stack,
        } = self;
        let state = unsafe { state.assume_init() };
        if matches!(state.object, DecodeObject::None) {
            return false;
        }
        let Some(state) = state.go_decode(&patch_buffer[..SLOP_SIZE], &mut stack, arena) else {
            return false;
        };

        state.overrun == 0 && matches!(state.object, DecodeObject::Message(_)) && stack.is_empty()
    }

    fn resume_impl(&mut self, buf: &[u8], arena: &mut crate::arena::Arena) -> Option<()> {
        let size = buf.len();
        let mut state = unsafe { self.state.assume_init_read() };
        if matches!(state.object, DecodeObject::None) {
            // Already finished
            return None;
        }
        if buf.len() > SLOP_SIZE {
            self.patch_buffer[SLOP_SIZE..].copy_from_slice(&buf[..SLOP_SIZE]);
            state = state.go_decode(&self.patch_buffer[..SLOP_SIZE], &mut self.stack, arena)?;
            if matches!(state.object, DecodeObject::None) {
                // TODO: Alter the state to indicate that we've ended on a 0 tag
                // Ended on 0 tag
                return None;
            }
            state = state.go_decode(&buf[..size - SLOP_SIZE], &mut self.stack, arena)?;
            self.patch_buffer[..SLOP_SIZE].copy_from_slice(&buf[size - SLOP_SIZE..]);
        } else {
            self.patch_buffer[SLOP_SIZE..SLOP_SIZE + size].copy_from_slice(buf);
            state = state.go_decode(&self.patch_buffer[..size], &mut self.stack, arena)?;
            self.patch_buffer.copy_within(size..size + SLOP_SIZE, 0);
        }
        self.state.write(state);
        Some(())
    }
}
