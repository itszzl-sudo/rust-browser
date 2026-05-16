//! Message types for the Mojo IPC system.
//!
//! This module defines the core message types used to communicate between
//! different components (processes) in the browser. It provides:
//!
//! - [`Message`]: A message that can be sent over a Mojo pipe, containing
//!   a method name, serialized payload data, and transferred handles.
//! - [`Handle`]: An opaque handle (e.g., a pipe endpoint) that can be
//!   transferred in a message.
//! - [`MessageSerializable`]: A trait for serializing/deserializing message
//!   payloads, with a built-in implementation for `String`.

/// Opaque handle that can be transferred in a message.
///
/// Handles represent capabilities such as pipe endpoints, shared memory
/// regions, or other kernel objects that can be transferred between
/// processes via Mojo IPC.
pub struct Handle {
    /// Unique identifier for this handle.
    pub id: u64,
}

/// A message sent over a Mojo pipe.
///
/// Each message consists of:
/// - A method name (or interface method ID) identifying the operation.
/// - A serialized payload (the message body).
/// - A list of transferred handles (e.g., pipe endpoints).
pub struct Message {
    /// Interface method name/ID.
    pub name: &'static str,
    /// Serialized payload.
    pub data: Vec<u8>,
    /// Transferred handles (pipe endpoints, etc.).
    pub handles: Vec<Handle>,
}

impl Message {
    /// Create a new message with the given method name.
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            data: Vec::new(),
            handles: Vec::new(),
        }
    }

    /// Attach serialized data to this message.
    pub fn with_data(mut self, data: Vec<u8>) -> Self {
        self.data = data;
        self
    }

    /// Attach handles to this message.
    pub fn with_handles(mut self, handles: Vec<Handle>) -> Self {
        self.handles = handles;
        self
    }
}

/// Serialization trait for message payloads.
///
/// Types that implement this trait can be serialized into a byte buffer
/// for transmission over a Mojo pipe, and deserialized back on the
/// receiving end.
pub trait MessageSerializable: Send {
    /// Serialize this value into a byte vector.
    fn serialize(&self) -> Vec<u8>;
    /// Deserialize a value from a byte slice.
    fn deserialize(data: &[u8]) -> Self
    where Self: Sized;
}

// --- Built-in serialization implementations ---

impl MessageSerializable for String {
    fn serialize(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }

    fn deserialize(data: &[u8]) -> Self {
        String::from_utf8_lossy(data).to_string()
    }
}

impl MessageSerializable for Vec<u8> {
    fn serialize(&self) -> Vec<u8> {
        self.clone()
    }

    fn deserialize(data: &[u8]) -> Self {
        data.to_vec()
    }
}

impl MessageSerializable for i32 {
    fn serialize(&self) -> Vec<u8> {
        self.to_le_bytes().to_vec()
    }

    fn deserialize(data: &[u8]) -> Self {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(&data[..4.min(data.len())]);
        i32::from_le_bytes(buf)
    }
}

impl MessageSerializable for u64 {
    fn serialize(&self) -> Vec<u8> {
        self.to_le_bytes().to_vec()
    }

    fn deserialize(data: &[u8]) -> Self {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&data[..8.min(data.len())]);
        u64::from_le_bytes(buf)
    }
}

impl MessageSerializable for f64 {
    fn serialize(&self) -> Vec<u8> {
        self.to_le_bytes().to_vec()
    }

    fn deserialize(data: &[u8]) -> Self {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&data[..8.min(data.len())]);
        f64::from_le_bytes(buf)
    }
}

impl MessageSerializable for bool {
    fn serialize(&self) -> Vec<u8> {
        vec![*self as u8]
    }

    fn deserialize(data: &[u8]) -> Self {
        data.first().copied().unwrap_or(0) != 0
    }
}
