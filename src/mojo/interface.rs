//! Interface binding system for Mojo IPC.
//!
//! This module provides high-level abstractions for defining and using Mojo
//! interfaces, modeled after Chrome's `InterfacePtr` / `Binding` pattern:
//!
//! - [`MojoInterface`]: Trait that any Mojo interface must implement.
//! - [`InterfaceProxy`]: A remote proxy that sends messages (like
//!   `InterfacePtr` in Chrome).
//! - [`InterfaceBinding`]: A receiver that handles incoming messages (like
//!   `Binding` in Chrome).
//! - [`PendingRemote`] / [`PendingReceiver`]: Unbound ends of an interface
//!   pipe (like `Remote` / `PendingReceiver` in Chrome).
//! - [`create_interface_pipe`]: Convenience function for creating an
//!   interface pipe from scratch.
//!
//! # Example
//!
//! ```rust
//! use rust_browser::mojo::interface::{InterfaceBinding, InterfaceProxy, create_interface_pipe};
//! use rust_browser::mojo::message::Message;
//!
//! // Create a pipe
//! let (mut remote, mut receiver) = create_interface_pipe("MyInterface");
//! let proxy = remote.bind();
//! let binding = receiver.bind();
//!
//! // Send a message
//! proxy.send_message(Message::new("DoSomething")).unwrap();
//!
//! // Receive it on the other end
//! let msg = binding.wait_for_message().unwrap();
//! assert_eq!(msg.name, "DoSomething");
//! ```

use crate::mojo::message::Message;
use crate::mojo::pipe::{MessagePipe, Port};
use log::trace;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// MojoInterface trait
// ---------------------------------------------------------------------------

/// Trait for a Mojo interface — a named set of related messages that can be
/// sent and received over a pipe.
///
/// Implement this trait on a zero-sized type (e.g. an empty enum) to define
/// a new interface, then use [`InterfaceProxy`] and [`InterfaceBinding`] to
/// communicate.
pub trait MojoInterface: Send + 'static {
    /// The name of this interface (used for debugging and routing).
    fn interface_name() -> &'static str;
}

// ---------------------------------------------------------------------------
// InterfaceProxy
// ---------------------------------------------------------------------------

/// A remote proxy that sends messages to the other end of an interface pipe.
///
/// This is analogous to Chrome's `InterfacePtr` / `Remote<T>`: it owns one
/// endpoint of a message pipe and provides methods for sending messages
/// belonging to the interface.
pub struct InterfaceProxy {
    port: Port,
    interface_name: &'static str,
}

impl InterfaceProxy {
    /// Wrap an existing port as an interface proxy.
    pub fn new(port: Port, name: &'static str) -> Self {
        Self {
            port,
            interface_name: name,
        }
    }

    /// Return a reference to the underlying port.
    pub fn port(&self) -> &Port {
        &self.port
    }

    /// Send a message through this proxy.
    pub fn send_message(&self, msg: Message) -> Result<(), String> {
        trace!(
            "InterfaceProxy[{}] sending: {}",
            self.interface_name,
            msg.name
        );
        self.port.send(msg)
    }

    /// Return `true` if the underlying port has been closed.
    pub fn is_closed(&self) -> bool {
        self.port.is_closed()
    }
}

// ---------------------------------------------------------------------------
// InterfaceBinding
// ---------------------------------------------------------------------------

/// A receiver that handles incoming messages for an interface.
///
/// This is analogous to Chrome's `Binding<T>`: it owns one endpoint of a
/// message pipe and processes incoming messages either through a callback
/// handler or by direct polling (e.g. `wait_for_message`).
pub struct InterfaceBinding {
    port: Port,
    #[allow(dead_code)]
    interface_name: &'static str,
    handler: Arc<Mutex<Option<Box<dyn Fn(Message) + Send>>>>,
}

impl InterfaceBinding {
    /// Wrap an existing port as an interface binding.
    pub fn new(port: Port, name: &'static str) -> Self {
        Self {
            port,
            interface_name: name,
            handler: Arc::new(Mutex::new(None)),
        }
    }

    /// Set a callback handler that will be invoked for each received message
    /// when `dispatch_one()` is called.
    pub fn set_handler<F>(&mut self, handler: F)
    where
        F: Fn(Message) + Send + 'static,
    {
        *self.handler.lock().unwrap() = Some(Box::new(handler));
    }

    /// Return a reference to the underlying port.
    pub fn port(&self) -> &Port {
        &self.port
    }

    /// Block the current thread until a message arrives and return it.
    pub fn wait_for_message(&self) -> Result<Message, String> {
        self.port.receive()
    }

    /// Attempt to receive a message without blocking.
    pub fn try_receive(&self) -> Option<Message> {
        self.port.try_receive()
    }

    /// Process one incoming message using the registered handler.
    ///
    /// Returns `Ok(true)` if a message was handled, `Ok(false)` if no message
    /// was available, or `Err` if the port is closed.
    pub fn dispatch_one(&self) -> Result<bool, String> {
        if let Some(msg) = self.port.try_receive() {
            if let Some(ref handler) = *self.handler.lock().unwrap() {
                handler(msg);
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Create a connected (proxy, binding) pair for the given interface name.
    ///
    /// The proxy sends messages that the binding receives.
    pub fn make_pair(name: &'static str) -> (InterfaceProxy, InterfaceBinding) {
        let pipe = MessagePipe::new(&format!("{}_proxy", name), &format!("{}_binding", name));
        let proxy = InterfaceProxy::new(pipe.endpoint0, name);
        let binding = InterfaceBinding::new(pipe.endpoint1, name);
        (proxy, binding)
    }

    /// Same as [`make_pair`](Self::make_pair) but with the proxy and binding
    /// endpoints swapped (proxy on endpoint1, binding on endpoint0).
    pub fn make_pair_swapped(name: &'static str) -> (InterfaceProxy, InterfaceBinding) {
        let pipe = MessagePipe::new(&format!("{}_proxy", name), &format!("{}_binding", name));
        let proxy = InterfaceProxy::new(pipe.endpoint1, name);
        let binding = InterfaceBinding::new(pipe.endpoint0, name);
        (proxy, binding)
    }
}

// ---------------------------------------------------------------------------
// PendingReceiver
// ---------------------------------------------------------------------------

/// A pending receiver that hasn't been bound yet.
///
/// This is analogous to Chrome's `PendingReceiver<T>`: it holds a port that
/// will later be turned into an [`InterfaceBinding`].
pub struct PendingReceiver {
    port: Option<Port>,
    interface_name: &'static str,
}

impl PendingReceiver {
    /// Create a new pending receiver wrapping the given port.
    pub fn new(port: Port, name: &'static str) -> Self {
        Self {
            port: Some(port),
            interface_name: name,
        }
    }

    /// Consume this pending receiver and return a bound [`InterfaceBinding`].
    ///
    /// # Panics
    ///
    /// Panics if the receiver has already been bound.
    pub fn bind(&mut self) -> InterfaceBinding {
        let port = self.port.take().expect("PendingReceiver already bound");
        InterfaceBinding::new(port, self.interface_name)
    }

    /// Returns `true` if this receiver has not yet been bound.
    pub fn is_some(&self) -> bool {
        self.port.is_some()
    }
}

// ---------------------------------------------------------------------------
// PendingRemote
// ---------------------------------------------------------------------------

/// A pending remote that hasn't been connected yet.
///
/// This is analogous to Chrome's `Remote<T>`: it holds a port that will later
/// be turned into an [`InterfaceProxy`].
pub struct PendingRemote {
    port: Option<Port>,
    interface_name: &'static str,
}

impl PendingRemote {
    /// Create a new pending remote wrapping the given port.
    pub fn new(port: Port, name: &'static str) -> Self {
        Self {
            port: Some(port),
            interface_name: name,
        }
    }

    /// Consume this pending remote and return a bound [`InterfaceProxy`].
    ///
    /// # Panics
    ///
    /// Panics if the remote has already been bound.
    pub fn bind(&mut self) -> InterfaceProxy {
        let port = self.port.take().expect("PendingRemote already bound");
        InterfaceProxy::new(port, self.interface_name)
    }

    /// Returns `true` if this remote has not yet been bound.
    pub fn is_some(&self) -> bool {
        self.port.is_some()
    }
}

// ---------------------------------------------------------------------------
// Free functions
// ---------------------------------------------------------------------------

/// Create a Mojo interface pipe, returning a `(PendingRemote, PendingReceiver)` pair.
///
/// This is the standard way to set up a new interface connection. The remote
/// and receiver can be passed to different threads or processes before being
/// bound.
pub fn create_interface_pipe(name: &'static str) -> (PendingRemote, PendingReceiver) {
    let pipe = MessagePipe::new(&format!("{}_remote", name), &format!("{}_receiver", name));
    let remote = PendingRemote::new(pipe.endpoint0, name);
    let receiver = PendingReceiver::new(pipe.endpoint1, name);
    (remote, receiver)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proxy_binding_pair() {
        let (proxy, binding) = InterfaceBinding::make_pair("TestInterface");

        proxy.send_message(Message::new("ping")).unwrap();
        let msg = binding.wait_for_message().unwrap();
        assert_eq!(msg.name, "ping");
    }

    #[test]
    fn test_create_interface_pipe_and_bind() {
        let (mut remote, mut receiver) = create_interface_pipe("MyInterface");

        let proxy = remote.bind();
        let binding = receiver.bind();

        proxy.send_message(Message::new("DoSomething")).unwrap();
        let msg = binding.wait_for_message().unwrap();
        assert_eq!(msg.name, "DoSomething");
    }

    #[test]
    fn test_dispatch_one_with_handler() {
        let handled = Arc::new(Mutex::new(false));
        let handled_clone = handled.clone();

        let (proxy, mut binding) = InterfaceBinding::make_pair("HandlerTest");
        binding.set_handler(move |msg: Message| {
            assert_eq!(msg.name, "handle_me");
            *handled_clone.lock().unwrap() = true;
        });

        proxy.send_message(Message::new("handle_me")).unwrap();

        // dispatch_one should process the message
        assert!(binding.dispatch_one().unwrap());
        assert!(*handled.lock().unwrap());
    }

    #[test]
    fn test_dispatch_one_no_message() {
        let (_, binding) = InterfaceBinding::make_pair("EmptyTest");
        // No message sent — dispatch_one should return Ok(false)
        assert!(!binding.dispatch_one().unwrap());
    }

    #[test]
    fn test_make_pair_swapped() {
        let (proxy, binding) = InterfaceBinding::make_pair_swapped("Swapped");

        proxy.send_message(Message::new("swapped_msg")).unwrap();
        let msg = binding.wait_for_message().unwrap();
        assert_eq!(msg.name, "swapped_msg");
    }

    #[test]
    fn test_pending_is_some() {
        let (mut remote, mut receiver) = create_interface_pipe("PendingTest");

        assert!(remote.is_some());
        assert!(receiver.is_some());

        let _proxy = remote.bind();
        let _binding = receiver.bind();

        assert!(!remote.is_some());
        assert!(!receiver.is_some());
    }
}
