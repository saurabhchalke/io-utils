//! This module defines the [`CountingIO<I>`] wrapper type for measuring the
//! amount of communication used by a network connection.
//!
//! Much of the code in this module is inspired by the
//! [`count-write`](https://crates.io/crates/count-write) crate.
use core::pin::Pin;
use core::task::{Context, Poll};
use futures::{io, AsyncRead, AsyncWrite};
use std::io::{Read, Write};

/// A wrapper type for measuring the amount of communication used by a network
/// connection.
///
/// `CountingIO` wraps an [`Read`]/[`Write`]/[`AsyncRead`]/[`AsyncWrite`]
/// object and counts the number of bytes passed through it.
///
/// `CountingIO` implements the [`Read`]/[`Write`]/[`AsyncRead`]/[`AsyncWrite`]
/// traits itself, so a wrapped object can be used the same as before.
pub struct CountingIO<I> {
    inner: I,
    count: u64,
}

impl<I> CountingIO<I> {
    /// Constructs a new `CountingIO<I>` object.
    pub fn new(inner: I) -> Self {
        Self { inner, count: 0 }
    }

    /// Returns the total communication amount in bytes.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Returns a reference to the wrapped object.
    pub fn inner(&self) -> &I {
        &self.inner
    }

    /// Returns a mutable reference to the wrapped object.
    pub fn inner_mut(&mut self) -> &mut I {
        &mut self.inner
    }

    /// Consumes the wrapper and returns the wrapped object.
    pub fn into_inner(self) -> I {
        self.inner
    }

    /// Resets the communication amount counter.
    pub fn reset(&mut self) {
        self.count = 0;
    }
}

impl<I: Read> Read for CountingIO<I> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        let bytes = self.inner.read(buf)?;
        self.count += bytes as u64;
        Ok(bytes)
    }
}

impl<I: Write> Write for CountingIO<I> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        let bytes = self.inner.write(buf)?;
        self.count += bytes as u64;
        Ok(bytes)
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        self.inner.flush()
    }
}

impl<I: AsyncRead + Unpin> AsyncRead for CountingIO<I> {
    fn poll_read(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        let Self { inner, count } = self.get_mut();
        let pin = Pin::new(inner);
        let ret = pin.poll_read(ctx, buf);
        if let Poll::Ready(Ok(read)) = &ret {
            *count += *read as u64;
        }
        ret
    }
}

impl<I: AsyncWrite + Unpin> AsyncWrite for CountingIO<I> {
    fn poll_write(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let Self { inner, count } = self.get_mut();
        let pin = Pin::new(inner);
        let ret = pin.poll_write(ctx, buf);
        if let Poll::Ready(Ok(written)) = &ret {
            *count += *written as u64;
        }
        ret
    }

    fn poll_flush(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(ctx)
    }

    fn poll_close(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.get_mut().inner).poll_close(ctx)
    }
}
