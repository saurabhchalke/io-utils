//! This module defines the [`IMuxSync<I>`] and [`IMuxAsync<I>`] wrapper types
//! for efficiently sending/receiving large messages over slow networks.
//!
//! Sending large messages over a single network stream in Rust is
//! unnecessarily slow (most likely due to underlying OS default settings for
//! TCP buffer sizes, see [here][stack_overflow]).
//!
//! The [`IMuxSync<I>`] and [`IMuxAsync<I>`] types mitigate this slowdown
//! by combining a collection of network connections into an
//! [inverse multiplexer][wikipedia].
//!
//! These types split a known amount of data into chunks before
//! sending/receiving rather than providing a streaming interface. As a result,
//! they don't re-implement the [`Read`]/[`Write`]/[`AsyncRead`]/[`AsyncWrite`]
//! traits of the objects they wrap and using them will require modification to
//! existing codebases.
//!
//! Both types are compatible with the [`CountingIO`] wrapper and expose the
//! underlying [`count`] and [`reset`] functions.
//!
//! [stack_overflow]: https://stackoverflow.com/questions/65731653/how-to-efficiently-send-large-files-across-a-single-network-connection
//! [wikipedia]: https://en.wikipedia.org/wiki/Inverse_multiplexer
//! [`count`]: `CountingIO::count`
//! [`reset`]: `CountingIO::reset`

use crate::counting::CountingIO;
use crossbeam_utils::thread;
use futures::{io, prelude::*, stream::FuturesUnordered, AsyncRead, AsyncWrite};
use std::{
    cmp::max,
    io::{Read, Write},
};

/// The default chunk size
const MIN_CHUNK_SIZE: usize = 8192;

/// An inverse multiplexer for asynchronous network streams.
///
/// Sending/receiving is done across each stream in parallel using a different
/// thread for each stream.
pub struct IMuxSync<I> {
    channels: Vec<I>,
}

/// An inverse multiplexer for asynchronous network streams.
///
/// Sending/receiving is done across each stream concurrently on a single
/// thread.
pub struct IMuxAsync<I> {
    channels: Vec<I>,
}

impl<I> IMuxSync<I> {
    /// Constructs a new `IMuxSync<I>` object.
    pub fn new(channels: Vec<I>) -> Self {
        Self { channels }
    }

    /// Consumes the inverse multiplexer and returns the underlying streams.
    pub fn into_inner(self) -> Vec<I> {
        self.channels
    }

    /// Returns a list of references to the underlying streams.
    pub fn get_ref(&self) -> Vec<&I> {
        self.channels.iter().collect()
    }

    /// Returns a list of references to the underlying streams.
    pub fn get_mut_ref(&mut self) -> Vec<&mut I> {
        self.channels.iter_mut().collect()
    }

    fn chunk_size(&self, len: usize) -> usize {
        max(
            MIN_CHUNK_SIZE,
            (len as f64 / self.channels.len() as f64).ceil() as usize,
        )
    }
}

impl<I> IMuxAsync<I> {
    /// Constructs a new `IMuxAsync<I>` object.
    pub fn new(channels: Vec<I>) -> Self {
        Self { channels }
    }

    /// Consumes the inverse multiplexer and returns the underlying streams.
    pub fn into_inner(self) -> Vec<I> {
        self.channels
    }

    /// Returns a list of references to the underlying streams.
    pub fn get_ref(&self) -> Vec<&I> {
        self.channels.iter().collect()
    }

    /// Returns a list of mutable references to the underlying streams.
    pub fn get_mut_ref(&mut self) -> Vec<&mut I> {
        self.channels.iter_mut().collect()
    }

    fn chunk_size(&self, len: usize) -> usize {
        max(
            MIN_CHUNK_SIZE,
            (len as f64 / self.channels.len() as f64).ceil() as usize,
        )
    }
}

impl<I> IMuxSync<CountingIO<I>> {
    /// Returns the total communication amount of the inverse multiplexer in
    /// bytes.
    pub fn count(&self) -> u64 {
        self.channels.iter().map(CountingIO::count).sum()
    }

    /// Resets the communication amount counter.
    pub fn reset(&mut self) {
        self.channels.iter_mut().for_each(CountingIO::reset);
    }
}

impl<I> IMuxAsync<CountingIO<I>> {
    /// Returns the total communication amount of the inverse multiplexer in
    /// bytes.
    pub fn count(&self) -> u64 {
        self.channels.iter().map(CountingIO::count).sum()
    }

    /// Resets the communication amount counter.
    pub fn reset(&mut self) {
        self.channels.iter_mut().for_each(CountingIO::reset);
    }
}

impl<I: Read + Send> IMuxSync<I> {
    /// Receive a message over the inverse multiplexer.
    pub fn read(&mut self) -> Result<Vec<u8>, io::Error> {
        // Read the length of the incoming buffer
        let mut len_buf = [0u8; 8];
        self.channels[0].read_exact(&mut len_buf)?;
        let len: u64 = u64::from_le_bytes(len_buf);

        // Read the message in chunks
        let mut buf = vec![0u8; len as usize];
        let chunk_size = self.chunk_size(buf.len());
        thread::scope(|s| {
            for (chunk, reader) in buf.chunks_mut(chunk_size).zip(self.channels.iter_mut()) {
                s.spawn(move |_| reader.read_exact(chunk));
            }
        })
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("Error occured while reading {:?}", e),
            )
        })?;
        Ok(buf)
    }
}

impl<I: Write + Send> IMuxSync<I> {
    /// Send a message over the inverse multiplexer.
    pub fn write(&mut self, buf: &[u8]) -> Result<(), io::Error> {
        // Send the total buffer length
        self.channels[0].write_all(&(buf.len() as u64).to_le_bytes())?;

        // Send `msg` in chunks
        let chunk_size = self.chunk_size(buf.len());
        thread::scope(|s| {
            for (chunk, writer) in buf.chunks(chunk_size).zip(self.channels.iter_mut()) {
                s.spawn(move |_| writer.write_all(chunk));
            }
        })
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("Error occured while writing {:?}", e),
            )
        })?;
        Ok(())
    }

    /// Flush the inverse multiplexer.
    pub fn flush(&mut self) -> Result<(), io::Error> {
        self.channels
            .iter_mut()
            .map(std::io::Write::flush)
            .collect::<Vec<_>>()
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        Ok(())
    }
}

impl<I: AsyncRead + Unpin> IMuxAsync<I> {
    /// Receive a message over the inverse multiplexer.
    pub async fn read(&mut self) -> Result<Vec<u8>, io::Error> {
        // Read the length of the incoming buffer
        let mut len_buf = [0u8; 8];
        self.channels[0].read_exact(&mut len_buf).await?;
        let len: u64 = u64::from_le_bytes(len_buf);

        // Read the message in chunks
        let mut buf = vec![0u8; len as usize];
        let chunk_size = self.chunk_size(buf.len());
        buf.chunks_mut(chunk_size)
            .zip(self.channels.iter_mut())
            .map(|(chunk, r)| async move { r.read_exact(chunk).await })
            .collect::<FuturesUnordered<_>>()
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        Ok(buf)
    }
}

impl<I: AsyncWrite + Unpin> IMuxAsync<I> {
    /// Send a message over the inverse multiplexer.
    pub async fn write(&mut self, buf: &[u8]) -> Result<(), io::Error> {
        // Send the total buffer length
        self.channels[0]
            .write_all(&(buf.len() as u64).to_le_bytes())
            .await?;

        // Send `msg` in chunks
        let chunk_size = self.chunk_size(buf.len());
        buf.chunks(chunk_size)
            .zip(self.channels.iter_mut())
            .map(|(chunk, w)| async move { w.write_all(chunk).await })
            .collect::<FuturesUnordered<_>>()
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        Ok(())
    }

    /// Flush the inverse multiplexer.
    pub async fn flush(&mut self) -> Result<(), io::Error> {
        self.channels
            .iter_mut()
            .map(io::AsyncWriteExt::flush)
            .collect::<FuturesUnordered<_>>()
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        Ok(())
    }
}
