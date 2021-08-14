//! Multidevice command protocol
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

use serde::{Serialize, Deserialize};
use tokio::io::AsyncReadExt;

use std::io;

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

#[derive(Debug, Serialize, Deserialize)]
pub enum PcmFormat {
    // 16-bit little endian samples.
    S16le,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum AudioFormat {
    Pcm {
        format: PcmFormat,
        channels: u8,
        rate: u32,
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AudioStreamInfo {
    format: AudioFormat,
    port: u16,
}

/// Message from server to client.
#[derive(Debug, Serialize, Deserialize)]
pub enum ServerMessage {
    ServerInfo(ServerInfo),
    /// Client should respond with ClientMessage::PingResponse
    /// when it receives this message.
    Ping,
    /// Server should send this when client sends ClientMessage::Ping.
    PingResponse,
    /// Client should close all connections to the server after receiving this.
    QuitRequest,
    PlayAudioStream(AudioStreamInfo),
}

/// Message from client to server.
#[derive(Debug, Serialize, Deserialize)]
pub enum ClientMessage {
    // Send ping request to server. Server should respond with
    // ServerMessage::PingResponse.
    Ping,
    // Client sends this message to server when it receives ServerMessage::Ping.
    PingResponse,
    /// Server should close all connections to the client after receiving this.
    QuitRequest,
    AudioStreamPlayError(String),
    ClientInfo(ClientInfo),
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

#[derive(Debug)]
pub enum ProtocolDeserializerError {
    IoError(io::Error),
    DeserializerError(serde_json::Error),
}

pub struct ProtocolDeserializer {
    data: Vec<u8>,
}

impl ProtocolDeserializer {
    pub fn new() -> Self {
        Self {
            data: vec![],
        }
    }

    async fn read_async<'a, T: Deserialize<'a>, U: AsyncReadExt + Unpin>(
        &'a mut self,
        data_source: U,
        message_size: i32,
    ) -> Result<T, ProtocolDeserializerError>  {
        self.data = Vec::new();

        let mut buf = [0; 1024];

        if message_size.is_negative() {
            panic!("message_len.is_negative()");
        }

        let mut data_source_with_limit = data_source
                .take(message_size as u64);

        loop {
            let read_count = data_source_with_limit
            .read(&mut buf)
            .await
            .map_err(ProtocolDeserializerError::IoError)?;

            match read_count {
                0 => break,
                data_len => self.data.extend(buf.iter().take(data_len)),
            }
        }

        let message = serde_json::from_slice(&self.data)
            .map_err(ProtocolDeserializerError::DeserializerError)?;

        Ok(message)
    }

    pub async fn read_server_message<U: AsyncReadExt + Unpin>(
        &mut self,
        data_source: U,
        message_size: i32,
    ) -> Result<ServerMessage, ProtocolDeserializerError> {
        self.read_async(data_source, message_size).await
    }

    pub async fn read_client_message<U: AsyncReadExt + Unpin>(
        &mut self,
        data_source: U,
        message_size: i32,
    ) -> Result<ClientMessage, ProtocolDeserializerError> {
        self.read_async(data_source, message_size).await
    }
}
