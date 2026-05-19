//! Thread-safe ZeroMQ socket wrapper.
//!
//! The upstream `zmq` crate implements `Send` for [`zmq::Socket`] but not
//! `Sync`, because the underlying `*mut c_void` is not automatically
//! recognised as thread-safe.  ZeroMQ sockets are documented as fully
//! thread-safe¹, so this wrapper soundly adds `Sync`.
//!
//! ¹ <https://zguide.zeromq.org/docs/chapter2/#Multithreading-with-ZMQ>

use std::ops::{Deref, DerefMut};

/// ZeroMQ socket wrapper that is both `Send` and `Sync`.
///
/// # Safety
///
/// The `zmq::Socket` API uses internal locking and is safe to share
/// across threads.  The underlying C library does not impose thread
/// restrictions.
#[repr(transparent)]
pub struct SendSocket {
    inner: zmq::Socket,
}

impl SendSocket {
    /// Wrap a `zmq::Socket`.
    pub fn new(sock: zmq::Socket) -> Self {
        Self { inner: sock }
    }
}

// SAFETY: ZeroMQ sockets are documented as thread-safe.  The upstream
// zmq crate implements `Send` but omits `Sync` solely because of the
// `*mut c_void` raw-pointer field in its struct.
unsafe impl Send for SendSocket {}
unsafe impl Sync for SendSocket {}

impl Deref for SendSocket {
    type Target = zmq::Socket;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for SendSocket {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
