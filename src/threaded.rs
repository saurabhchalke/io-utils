//! This module defines the [`ThreadedReader`] and [`ThreadedWriter`] wrapper
//! types for cloning a single asynchronous network connection across multiple
//! threads.
//!
//! The [`AsyncRead`]/[`AsyncWrite`] traits are not [`Send`] and can't be
//! shared across threads. The [`ThreadedReader`] and [`ThreadedWriter`]
//! [multiplexes][wikipedia] the stream in order to impl [`Send`].
//!
//! These types split a known amount of data into chunks before
//! sending/receiving rather than providing a streaming interface. As a result,
//! they don't re-implement the [`AsyncRead`]/[`AsyncWrite`] traits of the
//! objects they wrap and using them will require modification to existing
//! codebases.
//!
//! These types use [`IMuxAsync<I>`] objects in order to support efficiently
//! sending large messages over slow networks. However, they do not currently
//! expose interfaces for streams using the [`CountingIO`][counting] wrapper.
//!
//! **This module is still early in development and will most likely contain
//! bugs**
//!
//! [wikipedia]: https://en.wikipedia.org/wiki/Multiplexing
//! [counting]: `crate::counting::CountingIO`

use crate::imux::IMuxAsync;
use async_std::{
    channel,
    sync::{Arc, Mutex},
    task,
};
use futures::{AsyncRead, AsyncWrite};
use std::sync::atomic::{AtomicU8, Ordering};

/// A reader that can be safely cloned and sent between threads.
///
/// This type assumes that the order of clones are in sync with the
/// clones of the corresponding writer.
pub struct ThreadedReader {
    senders: Arc<Mutex<Vec<channel::Sender<Vec<u8>>>>>,
    receiver: channel::Receiver<Vec<u8>>,
    handle: Arc<async_std::task::JoinHandle<()>>,
}

/// A writer that can be safely cloned and sent between threads.
///
/// This type assumes that the order of clones are in sync with the
/// clones of the corresponding writer.
pub struct ThreadedWriter<W: 'static + AsyncWrite + Unpin + Send> {
    num: u8,
    count: Arc<AtomicU8>,
    writer: Arc<Mutex<IMuxAsync<W>>>,
}

// TODO: Better error handling
// TODO: Check edge cases when certain channels die before others

impl ThreadedReader {
    /// Constructs a new `ThreadedReader` object.
    ///
    /// A new thread is spawned containing the wrapped network stream. This
    /// thread will communicate with all cloned readers using channels.
    pub fn new<R: 'static + AsyncRead + Unpin + Send>(reader: IMuxAsync<R>) -> Self {
        // Setup a new channel to be used
        let (sender, receiver) = channel::unbounded();
        let senders = Arc::new(Mutex::new(vec![sender]));
        let senders_clone = senders.clone();
        // Start the background reader thread
        let handle = Arc::new(async_std::spawn(async {
            Self::read_loop(reader, senders_clone).await
        }));
        Self {
            senders,
            receiver,
            handle,
        }
    }

    /// The loop executed by the background reader thread
    async fn read_loop<R: 'static + AsyncRead + Unpin + Send>(
        mut reader: IMuxAsync<R>,
        senders: Arc<Mutex<Vec<channel::Sender<Vec<u8>>>>>,
    ) {
        // Receive messages from the reader in a loop while connection is open
        while let Ok(thread_num) = reader.read().await {
            // The thread number will always be followed by a message
            let msg = reader.read().await.unwrap();
            // Get the lock for the senders and attempt to send
            let senders_lock = senders.lock().await;
            senders_lock[thread_num[0] as usize]
                .send(msg)
                .await
                .unwrap();
        }
    }

    /// Receive a message.
    pub async fn read(&mut self) -> Vec<u8> {
        self.receiver.recv().await.unwrap()
    }
}

impl<W: 'static + AsyncWrite + Unpin + Send> ThreadedWriter<W> {
    /// Constructs a new `ThreadedWriter` object.
    pub fn new(writer: IMuxAsync<W>) -> Self {
        Self {
            num: 0,
            count: Arc::new(AtomicU8::new(1)),
            writer: Arc::new(Mutex::new(writer)),
        }
    }

    /// Send a message.
    pub async fn write(&mut self, buf: &[u8]) {
        // Acquire lock and write
        let mut writer = self.writer.lock().await;
        writer.write(&[self.num]).await.unwrap();
        writer.write(buf).await.unwrap();
        writer.flush().await.unwrap();
    }
}

impl Clone for ThreadedReader {
    fn clone(&self) -> Self {
        // Create a new channel and add it to the `senders` vector
        let (sender, receiver) = channel::unbounded();
        task::block_on(async {
            let mut senders = self.senders.lock().await;
            senders.push(sender);
        });
        Self {
            senders: self.senders.clone(),
            receiver,
            handle: self.handle.clone(),
        }
    }
}

impl<W: 'static + AsyncWrite + Unpin + Send> Clone for ThreadedWriter<W> {
    fn clone(&self) -> Self {
        ThreadedWriter {
            num: self.count.fetch_add(1, Ordering::SeqCst),
            count: self.count.clone(),
            writer: self.writer.clone(),
        }
    }
}

impl Drop for ThreadedReader {
    fn drop(&mut self) {
        // If this is the last writer still up, wait for the thread to finish
        match Arc::get_mut(&mut self.handle) {
            Some(h) => {
                task::block_on(async { h.await });
            }
            None => {}
        }
    }
}

// This is an alternate writer which uses a background task + channels instead of a mutex but needs
// improvements
// ----------------
///// Note that Writers have two different impls: one which uses a simple mutex, and another which
///// spawns an extra thread and uses channels (commented-out). The channel impl requires that msgs
///// are cloned before writing which leads to overhead for larger messages. The mutex impl may
///// suffer poor performance due to mutex contention. Prefer the channel impl for small, frequent
///// writes, and the mutex impl for large writes.
///// TODO: Do an unsafe impl where you ensure that the calling writer blocks until the message has
///// been sent
//pub struct ThreadedWriter {
//    num: u8,
//    count: Arc<AtomicU8>,
//    sender: mpsc::UnboundedSender<(u8, Vec<u8>)>,
//    handle: Arc<async_std::task::JoinHandle<()>>,
//}
//unsafe impl Send for ThreadedWriter {}
//
//impl ThreadedWriter {
//    pub fn new<W: 'static + AsyncWrite + Unpin + Send>(writer: IMuxAsync<W>) -> Self {
//        // Setup a new channel to be used
//        let (sender, receiver) = mpsc::unbounded();
//        // Start the background thread
//        let count = Arc::new(AtomicU8::new(1));
//        let handle = Arc::new(task::spawn(async {
//            Self::write_loop(writer, receiver).await
//        }));
//        Self { num: 0, count, sender, handle }
//    }
//
//    /// Continuously writes whatever is put onto the queue
//    async fn write_loop<W: 'static + AsyncWrite + Unpin + Send>(
//        mut writer: IMuxAsync<W>,
//        mut channel: mpsc::UnboundedReceiver<(u8, Vec<u8>)>,
//    ) {
//        while let Some((num, msg)) = channel.next().await {
//            writer.write(&[num]).await.unwrap();
//            writer.write(&msg).await.unwrap();
//            writer.flush().await.unwrap();
//        }
//    }
//
//    pub async fn write(&mut self, buf: &[u8]) {
//        self.sender.send((self.num, buf.to_vec())).await.unwrap();
//    }
//}
//
//impl Clone for ThreadedWriter {
//    fn clone(&self) -> Self {
//        ThreadedWriter {
//            num: self.count.fetch_add(1, Ordering::SeqCst),
//            count: self.count.clone(),
//            sender: self.sender.clone(),
//            handle: self.handle.clone(),
//        }
//    }
//}
//
//impl Drop for ThreadedWriter {
//    fn drop(&mut self) {
//        self.sender.disconnect();
//        // If this is the last writer still up, wait for the thread to finish
//        match Arc::get_mut(&mut self.handle) {
//            Some(h) => {
//                task::block_on(async { h.await });
//            },
//            None => {}
//        }
//    }
//}
