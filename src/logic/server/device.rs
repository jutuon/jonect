pub mod protocol;

use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::{TcpListener, TcpStream, tcp::{OwnedReadHalf, OwnedWriteHalf}}, sync::{mpsc::{self, Receiver, UnboundedReceiver}, Notify}};
use tokio_stream::{Stream, StreamExt, wrappers::UnboundedReceiverStream};
use async_stream::stream;

use std::{collections::HashMap, convert::TryInto, io, num::TryFromIntError, sync::Arc};

use crate::logic::server::DMEvent;

use self::protocol::{ClientInfo, ClientMessage, ProtocolDeserializer, ProtocolDeserializerError, ServerInfo, ServerMessage};

use super::{CloseComponent, ServerEvent, ServerEventSender};


#[derive(Debug)]
pub enum DeviceConnectionError {
    MessageSerializationError(serde_json::Error),
    MessageSendError(io::Error),
    MessageSendDataLengthError(TryFromIntError),
    MessageReceiveError(io::Error),
    MessageReceiveMessageSizeError,
    MessageReceiveProtocolDeserializeError(ProtocolDeserializerError),
}

pub struct DeviceConnection {
    read_half: OwnedReadHalf,
    write_half: OwnedWriteHalf,
}

impl DeviceConnection {
    fn new(connection: TcpStream) -> Self {
        let (read_half, write_half) = connection.into_split();

        Self {
            read_half,
            write_half,
        }
    }

    pub async fn send_message(&mut self, message: ServerMessage) -> Result<(), DeviceConnectionError> {
        let data = serde_json::to_vec(&message)
            .map_err(DeviceConnectionError::MessageSerializationError)?;

        let data_len: i32 = data
            .len()
            .try_into()
            .map_err(DeviceConnectionError::MessageSendDataLengthError)?;

        let data_len = data_len as u32;

        self.write_half.write_all(&data_len.to_be_bytes())
            .await
            .map_err(DeviceConnectionError::MessageSendError)?;

        self.write_half.write_all(&data)
            .await
            .map_err(DeviceConnectionError::MessageSendError)
    }

    pub async fn receive_message(&mut self) -> Result<ClientMessage, DeviceConnectionError> {
        let message_len = self.read_half
            .read_u32()
            .await
            .map_err(DeviceConnectionError::MessageReceiveError)?;

        if message_len > i32::max_value() as u32 {
            return Err(DeviceConnectionError::MessageReceiveMessageSizeError);
        }

        let mut deserializer = ProtocolDeserializer::new();
        let message = deserializer
            .read_client_message(&mut self.read_half, message_len)
            .await
            .map_err(DeviceConnectionError::MessageReceiveProtocolDeserializeError)?;

        Ok(message)
    }
}


#[derive(Debug)]
pub enum DeviceError {
    DeviceConnectionError(DeviceConnectionError),
    UnknownFirstProtocolMessage,
}

pub struct Device {
    device_connection: DeviceConnection,
    info: ClientInfo,
}

impl Device {
    pub async fn new(mut device_connection: DeviceConnection) -> Result<Self, DeviceError> {
        let message = ServerMessage::ServerInfo(ServerInfo::new("Test server"));
        device_connection.send_message(message).await.map_err(DeviceError::DeviceConnectionError)?;

        let message = device_connection.receive_message().await.map_err(DeviceError::DeviceConnectionError)?;

        let info = match message  {
            ClientMessage::ClientInfo(info) => info,
            _ => return Err(DeviceError::UnknownFirstProtocolMessage),
        };

        Ok(Device {
            device_connection,
            info,
        })
    }

    pub fn device_id(&self) -> &str {
        &self.info.id
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
}

type DeviceId = String;

pub struct DeviceManager {
    devices: HashMap<DeviceId, Device>,
    listener: TcpListener,
}

impl DeviceManager {
    pub async fn new() -> Result<Self, DeviceManagerError> {
        let listener = TcpListener::bind("127.0.0.1:8080")
            .await
            .map_err(DeviceManagerError::SocketListenerCreationError)?;

        Ok(Self {
            devices: HashMap::new(),
            listener,
        })
    }

    async fn accept_next_device_connection(&mut self) -> Result<(), DeviceManagerError> {
        loop {
            let (socket, _) = self.listener.accept().await
                .map_err(DeviceManagerError::SocketListenerAcceptError)?;

            match Device::new(DeviceConnection::new(socket)).await {
                Ok(device) => {
                    let id = device.device_id().to_string();
                    println!("New device connection. {:?}", device.info);
                    self.devices.insert(id, device);
                    return Ok(());
                },
                Err(e) => {
                    eprintln!("Error: {:?}", e);
                }
            }
        }
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
            // TODO: Wait new connection.
            //       This select will only run once so it is possible that
            //       there is no working device connection after this select.
            tokio::select! {
                e_next = receiver.next() => {
                    let e = e_next.expect("Logic bug: server task channel broken.");
                    if let CloseComponent::Yes = self.handle_event(e).await {
                        return;
                    }
                }
                connection_result = self.accept_next_device_connection() => {
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
