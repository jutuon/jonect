pub mod protocol;

use bytes::BytesMut;
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::{TcpListener, TcpStream, tcp::{ReadHalf, WriteHalf}}, sync::{mpsc, oneshot}, task::JoinHandle};

use std::{collections::HashMap, convert::TryInto, io::{self, ErrorKind}};

use self::protocol::{ClientMessage, ServerInfo, ServerMessage};

use super::{EVENT_CHANNEL_SIZE, ShutdownWatch};

#[derive(Debug)]
pub enum FromDeviceManagerToServerEvent {
    TcpSupportDisabledBecauseOfError(TcpSupportError),
}


#[derive(Debug)]
pub enum TcpSupportError {
    ListenerCreationError(io::Error),
    AcceptError(io::Error),
}

#[derive(Debug)]
pub enum ConnectionEvent {
    SendMessage(ServerMessage),
}

#[derive(Debug)]
pub struct Connection {
    id: ConnectionId,
    connection: TcpStream,
    dm_event_sender: DeviceManagerEventSender,
    receiver: mpsc::Receiver<ConnectionEvent>,
    quit_receiver: oneshot::Receiver<()>,
    _shutdown_watch: ShutdownWatch,
}
// todo

impl Connection {
    pub fn new(
        id: ConnectionId,
        connection: TcpStream,
        dm_event_sender: DeviceManagerEventSender,
        _shutdown_watch: ShutdownWatch,
    ) -> (Self, mpsc::Sender<ConnectionEvent>, oneshot::Sender<()>) {
        let (sender, receiver) = mpsc::channel(EVENT_CHANNEL_SIZE);
        let (quit_sender, quit_receiver) = oneshot::channel();

        let connection = Self {
            id,
            connection,
            dm_event_sender,
            _shutdown_watch,
            receiver,
            quit_receiver,
        };

        (connection, sender, quit_sender)
    }

    pub async fn connection_task(
        mut self,
    ) {
        let (read, write) = self.connection.split();

        let buffer = BytesMut::new();
        let r_task = Self::read_message(buffer, read);
        tokio::pin!(r_task);

        let (w_sender, w_receiver) = mpsc::channel(EVENT_CHANNEL_SIZE);
        let w_task = Self::handle_writing(w_receiver, write);
        tokio::pin!(w_task);

        let message = ServerMessage::ServerInfo(ServerInfo::new("Test server"));
        w_sender.send(message).await.unwrap();

        loop {
            tokio::select!(
                result = &mut self.quit_receiver => {
                    result.unwrap();
                    return;
                }
                event = self.receiver.recv() => {
                    match event.unwrap() {
                        ConnectionEvent::SendMessage(message) => {
                            w_sender.send(message).await.unwrap();
                        }
                    }
                }
                (result, buffer, read) = &mut r_task => {
                    r_task.set(Self::read_message(buffer, read));
                    match result {
                        Ok(message) => {
                            let event = DeviceManagerEvent::ConnectionMessage(self.id, message);
                            self.dm_event_sender.send(event).await;
                        }
                        Err(e) => {
                            let event = DeviceManagerEvent::ConnectionReadError(self.id, e);
                            self.dm_event_sender.send(event).await;
                            break;
                        }
                    }
                }
                (error, w_receiver, write) = &mut w_task => {
                    let event = DeviceManagerEvent::ConnectionWriteError(self.id, error);
                    self.dm_event_sender.send(event).await;
                    break;
                }
            )
        }

        self.quit_receiver.await.unwrap();
    }

    async fn read_message(
        mut buffer: BytesMut,
        mut read_half: ReadHalf<'_>
    ) -> (Result<ClientMessage, ReadError>, BytesMut, ReadHalf<'_>) {
        buffer.clear();

        let message_len = match read_half.read_i32().await.map_err(ReadError::Io) {
            Ok(len) => len,
            Err(e) => return (Err(e), buffer, read_half),
        };

        if message_len.is_negative() {
            return (Err(ReadError::MessageSize(message_len)), buffer, read_half);
        }

        let mut read_half_with_limit = read_half.take(message_len as u64);

        loop {
            match read_half_with_limit.read_buf(&mut buffer).await {
                Ok(0) => {
                    if buffer.len() == message_len as usize {
                        let message = serde_json::from_slice(&buffer).map_err(ReadError::Deserialize);
                        return (message, buffer, read_half_with_limit.into_inner());
                    } else {
                        let error = std::io::Error::new(ErrorKind::UnexpectedEof, "");
                        return (Err(ReadError::Io(error)), buffer, read_half_with_limit.into_inner());
                    }
                }
                Ok(_) => (),
                Err(e) => return (Err(ReadError::Io(e)), buffer, read_half_with_limit.into_inner()),
            }
        }
    }

    async fn handle_writing(
        mut receiver: mpsc::Receiver<ServerMessage>,
        mut write_half: WriteHalf<'_>,
    ) -> (WriteError, mpsc::Receiver<ServerMessage>, WriteHalf<'_>) {
        loop {
            let message = receiver.recv().await.unwrap();

            let data = match serde_json::to_string(&message) {
                Ok(data) => data,
                Err(e) => {
                    return (WriteError::Serialize(e), receiver, write_half);
                }
            };

            let data_len: i32 = match data.len().try_into() {
                Ok(len) => len,
                Err(_) => {
                    return (WriteError::MessageSize(data), receiver, write_half);
                }
            };

            let data_len = data_len.to_be_bytes();
            if let Err(e) = write_half.write_all(&data_len).await {
                return (WriteError::Io(e), receiver, write_half);
            };

            if let Err(e) = write_half.write_all(data.as_bytes()).await {
                return (WriteError::Io(e), receiver, write_half);
            };
        }
    }
}

#[derive(Debug)]
pub enum ReadError {
    Io(std::io::Error),
    Deserialize(serde_json::error::Error),
    /// Negative message size.
    MessageSize(i32),
}

#[derive(Debug)]
pub enum WriteError {
    Io(std::io::Error),
    Serialize(serde_json::error::Error),
    MessageSize(String),
}

#[derive(Debug)]
pub enum DeviceManagerEvent {
    RequestQuit,
    Message(String),
    ConnectionReadError(ConnectionId, ReadError),
    ConnectionWriteError(ConnectionId, WriteError),
    ConnectionMessage(ConnectionId, ClientMessage),
}

#[derive(Debug, Clone)]
pub struct DeviceManagerEventSender {
    sender: mpsc::Sender<DeviceManagerEvent>,
}

impl DeviceManagerEventSender {
    pub fn new(sender: mpsc::Sender<DeviceManagerEvent>) -> Self {
        Self {
            sender,
        }
    }

    pub async fn send(&mut self, event: DeviceManagerEvent) {
        self.sender.send(event).await.unwrap();
    }


}

type DeviceId = String;
type ConnectionId = u64;

pub struct ConnectionHandle {
    id: ConnectionId,
    device_id: Option<DeviceId>,
    task_handle: JoinHandle<()>,
    sender: mpsc::Sender<ConnectionEvent>,
    quit_sender: oneshot::Sender<()>,
}

impl ConnectionHandle {
    pub fn new(
        id: ConnectionId,
        device_id: Option<DeviceId>,
        task_handle: JoinHandle<()>,
        sender: mpsc::Sender<ConnectionEvent>,
        quit_sender: oneshot::Sender<()>,
    ) -> Self {
        Self {
            id,
            device_id,
            task_handle,
            sender,
            quit_sender,
        }
    }
}

pub struct DeviceManager {
    server_sender: mpsc::Sender<FromDeviceManagerToServerEvent>,
    receiver: mpsc::Receiver<DeviceManagerEvent>,
    dm_event_sender: DeviceManagerEventSender,
    connections: HashMap<ConnectionId, ConnectionHandle>,
    next_connection_id: u64,
    _shutdown_watch: ShutdownWatch,
}


impl DeviceManager {
    pub fn new(
        server_sender: mpsc::Sender<FromDeviceManagerToServerEvent>,
        shutdown_watch: ShutdownWatch,
    ) -> (Self, DeviceManagerEventSender) {
        let (dm_event_sender, receiver) = DeviceManager::create_device_event_channel();

        let dm = Self {
            server_sender,
            receiver,
            dm_event_sender: dm_event_sender.clone(),
            connections: HashMap::new(),
            next_connection_id: 0,
            _shutdown_watch: shutdown_watch,
        };

        (dm, dm_event_sender)
    }

    pub async fn run(mut self) {

        let listener = match TcpListener::bind("127.0.0.1:8080").await {
            Ok(listener) => listener,
            Err(e) => {
                let e = TcpSupportError::ListenerCreationError(e);
                let e = FromDeviceManagerToServerEvent::TcpSupportDisabledBecauseOfError(e);
                self.server_sender.send(e).await.unwrap();
                self.wait_quit_event().await;
                return;
            }
        };

        let mut tcp_listener_enabled = true;

        loop {
            tokio::select! {
                result = listener.accept(), if tcp_listener_enabled => {
                    match result {
                        Ok((stream, _)) => {
                            let id = self.next_connection_id;
                            self.next_connection_id = match id.checked_add(1) {
                                Some(new_next_id) => new_next_id,
                                None => {
                                    eprintln!("Warning: Couldn't handle a new connection because there is no new connection ID numbers.");
                                    continue;
                                }
                            };

                            let (connection, connection_sender, quit_sender) = Connection::new(
                                id,
                                stream,
                                self.dm_event_sender.clone(),
                                self._shutdown_watch.clone(),
                            );

                            let task_handle = tokio::spawn(async move {
                                connection.connection_task().await
                            });

                            let connection_handle = ConnectionHandle::new(
                                id,
                                None,
                                task_handle,
                                connection_sender,
                                quit_sender,
                            );

                            self.connections.insert(id, connection_handle);
                        }
                        Err(e) => {
                            let e = TcpSupportError::AcceptError(e);
                            let e = FromDeviceManagerToServerEvent::TcpSupportDisabledBecauseOfError(e);
                            self.server_sender.send(e).await.unwrap();

                            tcp_listener_enabled = false;
                        }
                    }
                }
                event = self.receiver.recv() => {
                    match event.unwrap() {
                        DeviceManagerEvent::Message(_) => {

                        }
                        DeviceManagerEvent::ConnectionReadError(id, error) => {
                            eprintln!("Connection id {} read error {:?}", id, error);

                            let connection = self.connections.remove(&id).unwrap();
                            connection.quit_sender.send(()).unwrap();
                            connection.task_handle.await.unwrap();
                        }
                        DeviceManagerEvent::ConnectionWriteError(id, error) => {
                            eprintln!("Connection id {} write error {:?}", id, error);

                            let connection = self.connections.remove(&id).unwrap();
                            connection.quit_sender.send(()).unwrap();
                            connection.task_handle.await.unwrap();
                        }
                        DeviceManagerEvent::ConnectionMessage(id, message) => {
                            println!("Connection id {} message {:?}", id, message);
                        }
                        DeviceManagerEvent::RequestQuit => {
                            break;
                        }
                    }
                }
            }
        }


        // Quit

        for handle in self.connections.into_values() {
            handle.quit_sender.send(()).unwrap();
            handle.task_handle.await.unwrap();
        }
    }

    async fn wait_quit_event(&mut self) {
        loop {
            let event = self.receiver.recv().await.unwrap();
            if let DeviceManagerEvent::RequestQuit = event {
                return;
            }
        }
    }

    fn create_device_event_channel() -> (DeviceManagerEventSender, mpsc::Receiver<DeviceManagerEvent>) {
        let (sender, receiver) = mpsc::channel(EVENT_CHANNEL_SIZE);

        (DeviceManagerEventSender::new(sender), receiver)
    }
}

pub struct DeviceManagerTask;


impl DeviceManagerTask {
    pub fn task(shutdown_watch: ShutdownWatch) -> (
        JoinHandle<()>,
        DeviceManagerEventSender,
        mpsc::Receiver<FromDeviceManagerToServerEvent>) {
        let (sender, receiver) = mpsc::channel(EVENT_CHANNEL_SIZE);

        let (dm, dm_sender) = DeviceManager::new(sender, shutdown_watch);

        let task = async move {
            dm.run().await;
        };

        let handle = tokio::spawn(task);

        (handle, dm_sender, receiver)
    }
}
