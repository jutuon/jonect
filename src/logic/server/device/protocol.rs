//! Multidevice command protocol
//!
//! 1. Send JSON message size as i32 (little endian).
//! 2. Send JSON message (UTF-8).

use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerInfo {
    pub speaker_audio_stream_port: u16,
}

pub struct ProtocolDeserializer {
    message_size: i32,
    buffer: Vec<u8>,
}
