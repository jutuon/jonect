//! Multidevice command protocol
//!
//! 1. Send JSON message size as u32 (big endian).
//!    Message size max value is max value of i32.
//! 2. Send JSON message (UTF-8).

use serde::{Serialize, Deserialize};
use tokio::io::AsyncReadExt;

use std::io;

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerInfo {
    pub speaker_audio_stream_port: u16,
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

    pub async fn read_async<'a, T: Deserialize<'a>, U: AsyncReadExt + Unpin>(
        &'a mut self,
        data_source: U,
        message_size: u32,
    ) -> Result<T, ProtocolDeserializerError>  {
        self.data = Vec::new();

        let mut buf = [0; 1024];

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
}
