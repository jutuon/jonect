use bytes::BytesMut;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::{io::AsyncReadExt, io::AsyncWriteExt, net::tcp::{OwnedReadHalf, ReadHalf, WriteHalf}};
use tokio_stream::Stream;

use async_stream::stream;

use std::convert::TryInto;



pub enum ReadError {
    Io(std::io::Error),
    Deserialize(serde_json::error::Error),
    UnexpectedEnding,
}

pub enum WriteError {
    Io(std::io::Error),
    Serialize(serde_json::error::Error),
}

pub struct JsonMessageConnectionReader {
    buffer: BytesMut,
}


impl JsonMessageConnectionReader {
    pub fn new() -> Self {
        Self {
            buffer: BytesMut::new(),
        }
    }


    pub fn message_len(&self) -> Option<u32> {
        let (len, _) = self.buffer.split_at(4);

        let len: [u8; 4] = match len.try_into() {
            Ok(len) => len,
            Err(e) => return None,
        };

        let len: u32 = u32::from_be_bytes(len);
        Some(len)
    }

    pub fn buffer_contains_a_message(&self) -> bool {
        if let Some(len) = self.message_len() {
            // TODO: Handle integer overflow.
            let required_buffer_len = usize::checked_add(len as usize, self.buffer.len()).unwrap();

            self.buffer.len() >= required_buffer_len
        } else {
            false
        }
    }

    /// If Some() is returned then parsed message is removed from the buffer.
    ///
    /// Panics if there is not enough data in the buffer.
    pub fn parse_message<D: DeserializeOwned>(&mut self) -> Result<D, ReadError> {
        let len = self.message_len().unwrap();
        let message_bytes = self.buffer.split_to(len as usize);
        let parse_result = serde_json::from_slice(&message_bytes).map_err(ReadError::Deserialize);
        parse_result
    }

    pub async fn read<D: DeserializeOwned>(&mut self, read_half: &mut ReadHalf<'_>) -> Result<D, ReadError> {


        // self.buffer.clear();

        // ErrorKind::UnexpectedEof
        // let message_len = read_half.read_u32().await.map_err(ReadError::Io)?;

        // let mut read_half_with_limit = read_half.take(message_len as u64);

        loop {
            if self.buffer_contains_a_message() {
                break;
            }

            if 0 == read_half.read_buf(&mut self.buffer).await.map_err(ReadError::Io)? {
                return Err(ReadError::UnexpectedEnding);
            }
        }

        self.parse_message()
    }



    pub async fn read_stream<'a, D: DeserializeOwned + 'a >(mut self, mut read_half: ReadHalf<'a>) -> impl Stream<Item=Result<D, ReadError>> + 'a {
        stream! {
            loop {
                yield self.read(&mut read_half).await;
            }
        }
    }

}

pub struct JsonMessageConnectionWriter;

impl JsonMessageConnectionWriter {
    pub async fn write<'a, S: Serialize>(message: &S, mut write_half: WriteHalf<'a>) -> Result<(), WriteError> {
        let data = match serde_json::to_vec(&message) {
            Ok(data) => data,
            Err(e) => {
                // TODO: Send error to client?
                //eprintln!("Warning: Message '{:?}' skipped because of serialization error '{}'", m, e);
                return Err(WriteError::Serialize(e));
            }
        };

        if let Err(e) = write_half.write_all(&data).await {
            return Err(WriteError::Io(e));
        }

        Ok(())
    }
}
