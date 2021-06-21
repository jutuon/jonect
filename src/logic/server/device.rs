mod protocol;

use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::{TcpListener, TcpStream}, sync::mpsc::{self, Receiver, UnboundedReceiver}};

use std::{convert::TryInto, io, num::TryFromIntError};

use self::protocol::{ServerInfo};

use super::{ServerEvent, ServerEventSender};


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

        self.connection.write_all(&data_len.to_le_bytes())
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
    server_event_sender: ServerEventSender,
    receiver: UnboundedReceiver<DeviceManagerEvent>,
}

impl DeviceManager {
    pub async fn new(
        server_event_sender: ServerEventSender,
        receiver: UnboundedReceiver<DeviceManagerEvent>
    ) -> Result<Self, DeviceManagerError> {
        let listener = TcpListener::bind("127.0.0.1:8080")
            .await
            .map_err(DeviceManagerError::SocketListenerCreationError)?;

        Ok(Self {
            device: None,
            listener,
            server_event_sender,
            receiver,
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

    pub async fn run(mut self) {
        /*
        match self.start_device_connection().await {
            Err(e) => {
                self.server_event_sender.send(ServerEvent::DeviceManagerError(e));
                self.server_event_sender.send(ServerEvent::DeviceManagerClosed);
                return;
            }
            Ok(()) => (),
        }
         */

        loop {
            match self.receiver.recv().await.unwrap() {
                DeviceManagerEvent::RequestQuit => {
                    self.server_event_sender.send(ServerEvent::DeviceManagerClosed);
                    return;
                }
                DeviceManagerEvent::Message(_) => (),
            }
        }
    }

    pub fn create_device_event_channel() -> (DeviceManagerEventSender, mpsc::UnboundedReceiver<DeviceManagerEvent>) {
        let (sender, receiver) = mpsc::unbounded_channel();

        (DeviceManagerEventSender::new(sender), receiver)
    }
}
