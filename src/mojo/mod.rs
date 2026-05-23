//! Mojo IPC system for the Rust browser.
//!
//! This module implements a Chrome-style Mojo IPC (Inter-Process Communication)
//! system, enabling message passing between different components of the browser
//! engine. It is structured into three layers:
//!
//! | Layer | Module | Description |
//! |-------|--------|-------------|
//! | Core  | [`message`] | Message types, handles, and serialization |
//! | Transport | [`pipe`] | Bidirectional message pipes (ports) |
//! | Binding | [`interface`] | Interface proxies, bindings, and pending endpoints |
//!
//! # Architecture
//!
//! The Mojo IPC system is modeled after Chromium's Mojo: two endpoints share
//! a [`MessagePipe`](pipe::MessagePipe), and each endpoint is a [`Port`](pipe::Port).
//! Messages written to one port are queued and can be read from the other.
//!
//! On top of the raw pipe, interfaces provide a higher-level abstraction:
//!
//! - [`InterfaceProxy`](interface::InterfaceProxy) sends messages (like Chrome's `InterfacePtr`).
//! - [`InterfaceBinding`](interface::InterfaceBinding) receives and dispatches messages (like Chrome's `Binding`).
//!
//! # Quick Start
//!
//! ```rust
//! use rust_browser::mojo::interface::{InterfaceBinding, create_interface_pipe};
//! use rust_browser::mojo::message::Message;
//!
//! // 1. Create an interface pipe
//! let (mut remote, mut receiver) = create_interface_pipe("ExampleInterface");
//!
//! // 2. Bind the ends
//! let proxy = remote.bind();
//! let binding = receiver.bind();
//!
//! // 3. Send a message
//! proxy.send_message(Message::new("Ping")).unwrap();
//!
//! // 4. Receive it on the other end
//! let msg = binding.wait_for_message().unwrap();
//! assert_eq!(msg.name.as_str(), "Ping");
//! ```

pub mod interface;
pub mod message;
pub mod pipe;
pub mod process_ipc;

// Re-exports for convenience
pub use interface::{
    create_interface_pipe, InterfaceBinding, InterfaceProxy, MojoInterface, PendingReceiver,
    PendingRemote,
};
pub use message::{Handle, Message, MessageSerializable};
pub use pipe::{generate_handle_id, MessagePipe, Port};
