//! Bidirectional message pipe for Mojo IPC.
//!
//! This module provides the core transport layer for Mojo-style IPC:
//!
//! - [`MessagePipe`]: A pair of connected [`Port`]s, analogous to Chrome's
//!   `MessagePipe`. Messages written to one endpoint are readable from the
//!   other.
//! - [`Port`]: One endpoint of a message pipe. Supports sending, receiving
//!   (blocking and non-blocking), and closing.
//! - `generate_handle_id()`: Utility for creating unique handle IDs.
//!
//! # Example
//!
//! ```rust
//! use rust_browser::mojo::pipe::MessagePipe;
//! use rust_browser::mojo::message::Message;
//!
//! let pipe = MessagePipe::new("client", "server");
//! pipe.endpoint0.send(Message::new("ping")).unwrap();
//! let msg = pipe.endpoint1.receive().unwrap();
//! assert_eq!(msg.name.as_str(), "ping");
//! ```

use crate::mojo::message::{Handle, Message};
use log::{debug, trace};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};

/// Counter for assigning unique pipe IDs.
static NEXT_PIPE_ID: AtomicU64 = AtomicU64::new(1);
/// Counter for assigning unique handle IDs.
static NEXT_HANDLE_ID: AtomicU64 = AtomicU64::new(1);

/// Generate a new unique handle identifier.
pub fn generate_handle_id() -> u64 {
    NEXT_HANDLE_ID.fetch_add(1, Ordering::SeqCst)
}

// ---------------------------------------------------------------------------
// PortInner – shared state between a Port and its peer
// ---------------------------------------------------------------------------

struct PortInner {
    /// Unique port identifier.
    id: u64,
    /// Human-readable name for debugging.
    name: String,
    /// Weak reference to the peer port (if any).
    peer: Mutex<Option<Weak<PortInner>>>,
    /// Incoming message queue.
    queue: Mutex<VecDeque<Message>>,
    /// Signalled when a new message arrives.
    signal: Condvar,
    /// Set to `true` when this port (or its peer) has been closed.
    closed: AtomicBool,
}

// ---------------------------------------------------------------------------
// Port
// ---------------------------------------------------------------------------

/// One endpoint of a [`MessagePipe`].
///
/// Ports are clonable handles to shared internal state. Cloning a `Port`
/// creates a new reference to the same endpoint (not a new connection).
#[derive(Clone)]
pub struct Port {
    inner: Arc<PortInner>,
}

impl Port {
    fn new(id: u64, name: String) -> Self {
        Self {
            inner: Arc::new(PortInner {
                id,
                name,
                peer: Mutex::new(None),
                queue: Mutex::new(VecDeque::new()),
                signal: Condvar::new(),
                closed: AtomicBool::new(false),
            }),
        }
    }

    /// Return the unique identifier of this port.
    pub fn id(&self) -> u64 {
        self.inner.id
    }

    /// Return the human-readable name of this port.
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// Return `true` if this port (or its peer) has been closed.
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::SeqCst)
    }

    /// Send a message to the peer port.
    ///
    /// Returns `Err` if the port is closed, the peer has been dropped,
    /// or no peer is connected.
    pub fn send(&self, msg: Message) -> Result<(), String> {
        if self.inner.closed.load(Ordering::SeqCst) {
            return Err("Port is closed".to_string());
        }

        let peer_opt = self.inner.peer.lock().unwrap().clone();
        match peer_opt {
            Some(peer_weak) => {
                let peer = peer_weak
                    .upgrade()
                    .ok_or_else(|| "Peer has been dropped".to_string())?;
                let mut q = peer.queue.lock().unwrap();
                q.push_back(msg);
                peer.signal.notify_one();
                trace!("Port {} sent message to {}", self.inner.name, peer.name);
                Ok(())
            }
            None => Err("No peer connected".to_string()),
        }
    }

    /// Block the current thread until a message is available, then return it.
    ///
    /// Returns `Err` if the port is closed while waiting.
    pub fn receive(&self) -> Result<Message, String> {
        if self.inner.closed.load(Ordering::SeqCst) {
            return Err("Port is closed".to_string());
        }

        let mut q = self.inner.queue.lock().unwrap();
        while q.is_empty() && !self.inner.closed.load(Ordering::SeqCst) {
            q = self.inner.signal.wait(q).unwrap();
        }
        if q.is_empty() {
            return Err("Port closed while waiting".to_string());
        }
        q.pop_front().ok_or_else(|| "Queue empty".to_string())
    }

    /// Attempt to receive a message without blocking.
    ///
    /// Returns `None` if no message is available or the port is closed.
    pub fn try_receive(&self) -> Option<Message> {
        if self.inner.closed.load(Ordering::SeqCst) {
            return None;
        }
        let mut q = self.inner.queue.lock().unwrap();
        q.pop_front()
    }

    /// Close this port and notify its peer.
    ///
    /// After closing, the peer will also be marked as closed and any waiting
    /// `receive()` calls on either side will return `Err`.
    pub fn close(&self) {
        self.inner.closed.store(true, Ordering::SeqCst);
        self.inner.signal.notify_all();

        // Detach from the peer and mark it as closed too.
        if let Some(peer_weak) = self.inner.peer.lock().unwrap().take() {
            if let Some(peer) = peer_weak.upgrade() {
                peer.closed.store(true, Ordering::SeqCst);
                peer.signal.notify_all();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// MessagePipe
// ---------------------------------------------------------------------------

/// A pair of connected ports, analogous to Chrome's `MessagePipe`.
///
/// Messages written to `endpoint0` can be read from `endpoint1`, and vice
/// versa. This is the fundamental building block for Mojo-style IPC.
pub struct MessagePipe {
    /// First endpoint.
    pub endpoint0: Port,
    /// Second endpoint.
    pub endpoint1: Port,
}

impl MessagePipe {
    /// Create a new connected pair of ports with the given debug names.
    ///
    /// The two ports are wired together: messages sent via `endpoint0` are
    /// received via `endpoint1`, and vice versa.
    pub fn new(name0: &str, name1: &str) -> Self {
        let id = NEXT_PIPE_ID.fetch_add(1, Ordering::SeqCst);
        let p0 = Port::new(id * 2, name0.to_string());
        let p1 = Port::new(id * 2 + 1, name1.to_string());

        // Connect them by exchanging weak references.
        let weak_p0 = Arc::downgrade(&p0.inner);
        let weak_p1 = Arc::downgrade(&p1.inner);
        *p0.inner.peer.lock().unwrap() = Some(weak_p1);
        *p1.inner.peer.lock().unwrap() = Some(weak_p0);

        debug!("MessagePipe created: {} <-> {}", name0, name1);

        Self {
            endpoint0: p0,
            endpoint1: p1,
        }
    }

    /// Close both endpoints of this pipe.
    pub fn close(&self) {
        self.endpoint0.close();
        self.endpoint1.close();
    }
}

// ---------------------------------------------------------------------------
// Handle conversion
// ---------------------------------------------------------------------------

impl From<Port> for Handle {
    fn from(port: Port) -> Self {
        Handle { id: port.id() }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_send_receive() {
        let pipe = MessagePipe::new("a", "b");
        pipe.endpoint0.send(Message::new("hello")).unwrap();
        let msg = pipe.endpoint1.receive().unwrap();
        assert_eq!(msg.name.as_str(), "hello");
    }

    #[test]
    fn test_try_receive_empty() {
        let pipe = MessagePipe::new("a", "b");
        assert!(pipe.endpoint0.try_receive().is_none());
    }

    #[test]
    fn test_close() {
        let pipe = MessagePipe::new("a", "b");
        pipe.endpoint0.close();
        assert!(pipe.endpoint0.is_closed());
        assert!(pipe.endpoint1.is_closed());
        assert!(pipe.endpoint0.send(Message::new("x")).is_err());
    }

    #[test]
    fn test_bidirectional() {
        let pipe = MessagePipe::new("left", "right");
        pipe.endpoint0.send(Message::new("to_right")).unwrap();
        pipe.endpoint1.send(Message::new("to_left")).unwrap();

        assert_eq!(pipe.endpoint1.receive().unwrap().name.as_str(), "to_right");
        assert_eq!(pipe.endpoint0.receive().unwrap().name.as_str(), "to_left");
    }
}
