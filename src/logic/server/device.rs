pub mod protocol;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc::{self, Receiver, UnboundedReceiver}, Notify},
};
use tokio_stream::{Stream, StreamExt, wrappers::UnboundedReceiverStream};
use async_stream::stream;

use std::{convert::TryInto, io, num::TryFromIntError, sync::Arc};

use crate::logic::server::DMEvent;

use self::protocol::{ServerInfo};

use super::{CloseComponent, ServerEvent, ServerEventSender};


pub struct Device {
    connection: TcpStream,
}

impl Device {
    fn new(connection: TcpStream) -> Self {
        Self {
            connection,
        }
    }

    pub async fn send_message(&mut self, message: ServerInfo) -> Result<(), DeviceManagerError> {
        let data = serde_json::to_vec(&message)
            .map_err(DeviceManagerError::MessageSerializationError)?;

        let data_len: i32 = data
            .len()
            .try_into()
            .map_err(DeviceManagerError::MessageSendDataLengthError)?;

        let data_len = data_len as u32;

        self.connection.write_all(&data_len.to_be_bytes())
            .await
            .map_err(DeviceManagerError::MessageSendError)?;

        self.connection.write_all(&data)
            .await
            .map_err(DeviceManagerError::MessageSendError)
    }
}

#[derive(Debug)]
pub enum DeviceManagerEvent {
    RequestQuit,
    Message(String),
}

#[derive(Debug, Clone)]
pub struct DeviceManagerEventSender {
    sender: mpsc::UnboundedSender<DeviceManagerEvent>,
}

impl DeviceManagerEventSender {
    pub fn new(sender: mpsc::UnboundedSender<DeviceManagerEvent>) -> Self {
        Self {
            sender,
        }
    }

    pub fn send(&mut self, event: DeviceManagerEvent) {
        self.sender.send(event).unwrap();
    }
}

#[derive(Debug)]
pub enum DeviceManagerError {
    SocketListenerCreationError(io::Error),
    SocketListenerAcceptError(io::Error),
    MessageSerializationError(serde_json::Error),
    MessageSendError(io::Error),
    MessageSendDataLengthError(TryFromIntError),
}

pub struct DeviceManager {
    device: Option<Device>,
    listener: TcpListener,
}

impl DeviceManager {
    pub async fn new() -> Result<Self, DeviceManagerError> {
        let listener = TcpListener::bind("127.0.0.1:8080")
            .await
            .map_err(DeviceManagerError::SocketListenerCreationError)?;

        Ok(Self {
            device: None,
            listener,
        })
    }

    async fn accept_next_connection(&mut self) -> Result<&mut Device, DeviceManagerError> {
        let (socket, _) = self.listener.accept().await
            .map_err(DeviceManagerError::SocketListenerAcceptError)?;

        self.device = Some(Device::new(socket));

        Ok(self.device.as_mut().unwrap())
    }

    async fn start_device_connection(&mut self) -> Result<(), DeviceManagerError> {
        let device = self.accept_next_connection().await?;
        device.send_message(ServerInfo {
            speaker_audio_stream_port: 1234,
        }).await?;

        Ok(())
    }

    async fn handle_event(&mut self, event: DeviceManagerEvent) -> CloseComponent {
        match event {
            DeviceManagerEvent::RequestQuit => {
                return CloseComponent::Yes;
            }
            DeviceManagerEvent::Message(_) => (),
        }

        CloseComponent::No
    }

    pub fn event_stream(mut self, mut receiver: UnboundedReceiverStream<DeviceManagerEvent>) -> impl Stream<Item = DMEvent> {
        stream! {
            tokio::select! {
                e_next = receiver.next() => {
                    let e = e_next.expect("Logic bug: server task channel broken.");
                    if let CloseComponent::Yes = self.handle_event(e).await {
                        return;
                    }
                }
                connection_result = self.start_device_connection() => {
                    match connection_result {
                        Err(e) => {
                            yield DMEvent::DeviceManagerError(e);
                            // Close device manager.
                            return;
                        }
                        Ok(()) => (),
                    }
                }
            };

            loop {
                tokio::select! {
                    e_next = receiver.next() => {
                        let e = e_next.expect("Logic bug: server task channel broken.");
                        if let CloseComponent::Yes = self.handle_event(e).await {
                            return;
                        }
                    }
                }
            }
        }
    }

    pub fn create_device_event_channel() -> (DeviceManagerEventSender, UnboundedReceiverStream<DeviceManagerEvent>) {
        let (sender, receiver) = mpsc::unbounded_channel();

        (DeviceManagerEventSender::new(sender), UnboundedReceiverStream::new(receiver))
    }
}
