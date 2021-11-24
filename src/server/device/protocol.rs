/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Jonect command protocol
//!
//! # Data transfer protocol
//! 1. Send JSON message size as i32 (big endian).
//! 2. Send JSON message (UTF-8).
//!
//! # Protocol messages
//! 1. Server sends ServerMessage::ServerInfo message to the client.
//! 2. Client sends ClientMessage::ClientInfo message to the server.
//! 3. Client and server communicate with different protocol messages.
//!    There is no need to have a specific message sending order between
//!    the server and client because messages are processed as they
//!    are received.

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerInfo {
    pub version: String,
    pub id: String,
}

impl ServerInfo {
    pub fn new<T: Into<String>>(id: T) -> Self {
        Self {
            version: "0.1".to_string(),
            id: id.into(),
        }
    }
}

pub enum AudioFormat {
    // 16-bit little endian PCM samples.
    Pcm,
}

impl AudioFormat {
    pub fn as_json_value(&self) -> &'static str {
        "pcm-s16le"
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AudioStreamInfo {
    /// Possible values:
    /// * pcm-s16le - 16-bit little endian PCM samples.
    format: String,
    channels: u8,
    rate: u32,
    pub port: u16,
}

impl AudioStreamInfo {
    pub fn new(format: AudioFormat, channels: u8, rate: u32, port: u16) -> Self {
        Self {
            format: format.as_json_value().to_string(),
            channels,
            rate,
            port,
        }
    }
}

/// Message from server to client.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    ServerInfo(ServerInfo),
    /// Client should respond with ClientMessage::PingResponse
    /// when it receives this message.
    Ping,
    /// Server should send this when client sends ClientMessage::Ping.
    PingResponse,
    PlayAudioStream(AudioStreamInfo),
}

/// Message from client to server.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    ClientInfo(ClientInfo),
    // Send ping request to server. Server should respond with
    // ServerMessage::PingResponse.
    Ping,
    // Client sends this message to server when it receives ServerMessage::Ping.
    PingResponse,
    AudioStreamPlayError(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClientInfo {
    pub version: String,
    pub id: String,
}

impl ClientInfo {
    pub fn new<T: Into<String>>(id: T) -> Self {
        Self {
            version: "0.1".to_string(),
            id: id.into(),
        }
    }
}
