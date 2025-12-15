#![feature(likely_unlikely, allocator_api)]

pub mod arena;
pub mod base;
pub mod containers;
pub mod wire;

pub mod utils;

pub mod decoding;
pub mod encoding;

use crate as protocrap;

include!(concat!(env!("OUT_DIR"), "/descriptor.pc.rs"));

// #[cfg(feature = "serde_support")]
// pub mod serde;

pub trait Protobuf {
    fn encoding_table() -> &'static [encoding::TableEntry];
    fn decoding_table() -> &'static decoding::Table;

    fn as_object(&self) -> &base::Object {
        unsafe { &*(self as *const Self as *const base::Object) }
    }

    fn as_object_mut(&mut self) -> &mut base::Object {
        unsafe { &mut *(self as *mut Self as *mut base::Object) }
    }
}

pub trait ProtobufExt: Protobuf {
    #[must_use]
    fn decode_flat<const STACK_DEPTH: usize>(
        &mut self,
        arena: &mut crate::arena::Arena,
        buf: &[u8],
    ) -> bool {
        let mut decoder = decoding::ResumeableDecode::<STACK_DEPTH>::new(self, isize::MAX);
        if !decoder.resume(buf, arena) {
            return false;
        }
        decoder.finish(arena)
    }

    fn decode<'a, E: std::error::Error + Send + Sync + 'static>(
        &mut self,
        arena: &mut crate::arena::Arena,
        provider: &'a mut impl FnMut() -> Result<Option<&'a [u8]>, E>,
    ) -> anyhow::Result<()> {
        let mut decoder = decoding::ResumeableDecode::<32>::new(self, isize::MAX);
        loop {
            let Some(buffer) = provider()? else {
                break;
            };
            if !decoder.resume(buffer, arena) {
                return Err(anyhow::anyhow!("decode error"));
            }
        }
        if !decoder.finish(arena) {
            return Err(anyhow::anyhow!("decode error"));
        }
        Ok(())
    }

    fn async_decode<'a, E: std::error::Error + Send + Sync + 'static, F>(
        &'a mut self,
        arena: &mut crate::arena::Arena,
        provider: &'a mut impl FnMut() -> F,
    ) -> impl std::future::Future<Output = anyhow::Result<()>>
    where
        F: std::future::Future<Output = Result<Option<&'a [u8]>, E>> + 'a,
    {
        async move {
            let mut decoder = decoding::ResumeableDecode::<32>::new(self, isize::MAX);
            loop {
                let Some(buffer) = provider().await? else {
                    break;
                };
                if !decoder.resume(buffer, arena) {
                    return Err(anyhow::anyhow!("decode error"));
                }
            }
            if !decoder.finish(arena) {
                return Err(anyhow::anyhow!("decode error"));
            }
            Ok(())
        }
    }

    fn decode_from_bufread<const STACK_DEPTH: usize>(
        &mut self,
        arena: &mut crate::arena::Arena,
        reader: &mut impl std::io::BufRead,
    ) -> anyhow::Result<()> {
        let mut decoder = decoding::ResumeableDecode::<STACK_DEPTH>::new(self, isize::MAX);
        loop {
            let buffer = reader.fill_buf()?;
            let len = buffer.len();
            if len == 0 {
                break;
            }
            if !decoder.resume(buffer, arena) {
                return Err(anyhow::anyhow!("decode error"));
            }
            reader.consume(len);
        }
        if !decoder.finish(arena) {
            return Err(anyhow::anyhow!("decode error"));
        }
        Ok(())
    }

    fn decode_from_read<const STACK_DEPTH: usize>(
        &mut self,
        arena: &mut crate::arena::Arena,
        reader: &mut impl std::io::Read,
    ) -> anyhow::Result<()> {
        let mut buf_reader = std::io::BufReader::new(reader);
        self.decode_from_bufread::<STACK_DEPTH>(arena, &mut buf_reader)
    }

    fn decode_from_async_bufread<'a, const STACK_DEPTH: usize>(
        &'a mut self,
        arena: &'a mut crate::arena::Arena<'a>,
        reader: &mut (impl futures::io::AsyncBufRead + Unpin),
    ) -> impl std::future::Future<Output = anyhow::Result<()>> {
        use futures::io::AsyncBufReadExt;

        async move {
            let mut decoder = decoding::ResumeableDecode::<STACK_DEPTH>::new(self, isize::MAX);
            loop {
                let buffer = reader.fill_buf().await?;
                let len = buffer.len();
                if len == 0 {
                    break;
                }
                if !decoder.resume(buffer, arena) {
                    return Err(anyhow::anyhow!("decode error"));
                }
                reader.consume_unpin(len);
            }
            if !decoder.finish(arena) {
                return Err(anyhow::anyhow!("decode error"));
            }
            Ok(())
        }
    }

    fn decode_from_async_read<'a, const STACK_DEPTH: usize>(
        &'a mut self,
        arena: &'a mut crate::arena::Arena<'a>,
        reader: &mut (impl futures::io::AsyncRead + Unpin),
    ) -> impl std::future::Future<Output = anyhow::Result<()>> {
        async move {
            let mut buf_reader = futures::io::BufReader::new(reader);
            self.decode_from_async_bufread::<STACK_DEPTH>(arena, &mut buf_reader)
                .await
        }
    }

    fn encode_flat<'a, const STACK_DEPTH: usize>(
        &self,
        buffer: &'a mut [u8],
    ) -> anyhow::Result<&'a [u8]> {
        let mut resumeable_encode = encoding::ResumeableEncode::<STACK_DEPTH>::new(self);
        let encoding::ResumeResult::Done(buf) = resumeable_encode
            .resume_encode(buffer)
            .ok_or(anyhow::anyhow!("Message tree too deep"))?
        else {
            return Err(anyhow::anyhow!("Buffer too small for message"));
        };
        Ok(buf)
    }
}

impl<T: Protobuf> ProtobufExt for T {}
