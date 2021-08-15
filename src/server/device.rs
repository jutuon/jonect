pub mod protocol;

use bytes::BytesMut;
use serde::Serialize;
use tokio::{io::{AsyncReadExt, AsyncWrite, AsyncWriteExt}, net::{TcpListener, TcpStream, tcp::{ReadHalf, WriteHalf}}, sync::{mpsc, oneshot}, task::JoinHandle};

use std::{collections::HashMap, convert::TryInto, fmt::{self, Debug}, io::{self, ErrorKind}, pin};

use self::protocol::{ClientMessage, ServerInfo, ServerMessage};

use super::{EVENT_CHANNEL_SIZE, ShutdownWatch};

#[derive(Debug)]
pub enum FromDeviceManagerToServerEvent {
    TcpSupportDisabledBecauseOfError(TcpSupportError),
}

/// Wrapper for tokio::sync::mpsc::Sender for sending events upwards in the
/// task tree.
#[derive(Debug)]
pub struct SendUpward<T: fmt::Debug> {
    sender: mpsc::Sender<T>,
}

impl <T: fmt::Debug> SendUpward<T> {
    /// Panic if channel is broken.
    pub async fn send_up(&self, data: T) {
        self.sender.send(data).await.expect("Error: broken channel");
    }
}

impl <T: fmt::Debug> From<mpsc::Sender<T>> for SendUpward<T> {
    fn from(sender: mpsc::Sender<T>) -> Self {
        Self {
            sender
        }
    }
}

/// Wrapper for tokio::sync::mpsc::Sender for sending events downwards in the
/// task tree.
#[derive(Debug)]
pub struct SendDownward<T: fmt::Debug> {
    sender: mpsc::Sender<T>,
}

impl <T: fmt::Debug> SendDownward<T> {
    /// Panic if channel is broken.
    pub async fn send_down(&self, data: T) {
        self.sender.send(data).await.expect("Error: broken channel");
    }
}

impl <T: fmt::Debug> From<mpsc::Sender<T>> for SendDownward<T> {
    fn from(sender: mpsc::Sender<T>) -> Self {
        Self {
            sender
        }
    }
}


#[derive(Debug)]
pub enum TcpSupportError {
    ListenerCreationError(io::Error),
    AcceptError(io::Error),
}

#[derive(Debug)]
pub enum ConnectionEvent {
    ReadError(ConnectionId, ReadError),
    WriteError(ConnectionId, WriteError),
    Message(ConnectionId, ClientMessage),
}

impl ConnectionEvent {
    pub fn is_error(&self) -> bool {
        match self {
            Self::ReadError(_,_) | Self::WriteError(_,_) => true,
            Self::Message(_,_) => false,
        }
    }
}

#[derive(Debug)]
pub struct Connection<R: AsyncReadExt + Unpin, W: AsyncWriteExt + Unpin, E: fmt::Debug + From<ConnectionEvent>, M: Serialize + Debug> {
    id: ConnectionId,
    read_half: R,
    write_half: W,
    sender: SendUpward<E>,
    receiver: mpsc::Receiver<M>,
    quit_receiver: oneshot::Receiver<()>,
    _shutdown_watch: ShutdownWatch,
}

impl <R: AsyncReadExt + Unpin, W: AsyncWriteExt + Unpin, E: fmt::Debug + From<ConnectionEvent>, M: Serialize + Debug> Connection<R, W, E, M> {
    /// Returned mpsc::Sender<M> must not be dropped before a running
    /// connection task is dropped.
    pub fn new(
        id: ConnectionId,
        read_half: R,
        write_half: W,
        sender: SendUpward<E>,
        _shutdown_watch: ShutdownWatch,
    ) -> (Self, mpsc::Sender<M>, oneshot::Sender<()>) {
        let (event_sender, receiver) = mpsc::channel(EVENT_CHANNEL_SIZE);
        let (quit_sender, quit_receiver) = oneshot::channel();

        let connection = Self {
            id,
            sender,
            _shutdown_watch,
            receiver,
            quit_receiver,
            read_half,
            write_half,
        };

        (connection, event_sender, quit_sender)
    }

    pub async fn connection_task(
        mut self,
    ) {
        let buffer = BytesMut::new();
        let r_task = Self::read_message(buffer, self.read_half);
        tokio::pin!(r_task);

        let (w_sender, w_receiver) = mpsc::channel(EVENT_CHANNEL_SIZE);
        let w_task = Self::handle_writing(w_receiver, self.write_half);
        tokio::pin!(w_task);

        loop {
            tokio::select!(
                result = &mut self.quit_receiver => {
                    result.unwrap();
                    return;
                }
                event = self.receiver.recv() => {
                    tokio::select!(
                        result = w_sender.send(event.unwrap()) => result.unwrap(),
                        quit = &mut self.quit_receiver => return quit.unwrap(),
                    );
                    continue;
                }
                (result, buffer, read) = &mut r_task => {
                    r_task.set(Self::read_message(buffer, read));

                    match result {
                        Ok(message) => {
                            let event = ConnectionEvent::Message(self.id, message);
                            tokio::select!(
                                _ = self.sender.send_up(event.into()) => (),
                                quit = &mut self.quit_receiver => return quit.unwrap(),
                            );
                        }

                        Err(e) => {
                            let event = ConnectionEvent::ReadError(self.id, e);
                            tokio::select!(
                                _ = self.sender.send_up(event.into()) => (),
                                quit = &mut self.quit_receiver => return quit.unwrap(),
                            );
                            break;
                        }
                    }
                }
                error = &mut w_task => {
                    let event = ConnectionEvent::WriteError(self.id, error);
                    tokio::select!(
                        _ = self.sender.send_up(event.into()) => (),
                        quit = &mut self.quit_receiver => return quit.unwrap(),
                    );
                    break;
                }
            );
        }

        self.quit_receiver.await.unwrap();
    }

    async fn read_message(
        mut buffer: BytesMut,
        mut read_half: R,
    ) -> (Result<ClientMessage, ReadError>, BytesMut, R) {
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
        mut receiver: mpsc::Receiver<M>,
        mut write_half: W,
    ) -> WriteError {
        loop {
            let message = receiver.recv().await.unwrap();

            let data = match serde_json::to_string(&message) {
                Ok(data) => data,
                Err(e) => {
                    return WriteError::Serialize(e);
                }
            };

            let data_len: i32 = match data.len().try_into() {
                Ok(len) => len,
                Err(_) => {
                    return WriteError::MessageSize(data);
                }
            };

            let data_len = data_len.to_be_bytes();
            if let Err(e) = write_half.write_all(&data_len).await {
                return WriteError::Io(e);
            };

            if let Err(e) = write_half.write_all(data.as_bytes()).await {
                return WriteError::Io(e);
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
    ConnectionEvent(ConnectionEvent),
}

impl From<ConnectionEvent> for DeviceManagerEvent {
    fn from(e: ConnectionEvent) -> Self {
        DeviceManagerEvent::ConnectionEvent(e)
    }
}

type DeviceId = String;
type ConnectionId = u64;

pub struct ConnectionHandle {
    id: ConnectionId,
    device_id: Option<DeviceId>,
    task_handle: JoinHandle<()>,
    sender: mpsc::Sender<ServerMessage>,
    quit_sender: oneshot::Sender<()>,
}

impl ConnectionHandle {
    pub fn new(
        id: ConnectionId,
        device_id: Option<DeviceId>,
        task_handle: JoinHandle<()>,
        sender: mpsc::Sender<ServerMessage>,
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
    dm_event_sender: mpsc::Sender<DeviceManagerEvent>,
    connections: HashMap<ConnectionId, ConnectionHandle>,
    next_connection_id: u64,
    _shutdown_watch: ShutdownWatch,
}


impl DeviceManager {
    pub fn new(
        server_sender: mpsc::Sender<FromDeviceManagerToServerEvent>,
        shutdown_watch: ShutdownWatch,
    ) -> (Self, SendDownward<DeviceManagerEvent>) {
        let (dm_event_sender, receiver) = mpsc::channel(EVENT_CHANNEL_SIZE);

        let dm = Self {
            server_sender,
            receiver,
            dm_event_sender: dm_event_sender.clone(),
            connections: HashMap::new(),
            next_connection_id: 0,
            _shutdown_watch: shutdown_watch,
        };

        (dm, dm_event_sender.into())
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

                            let (read_half, write_half) = stream.into_split();
                            let (connection, connection_sender, quit_sender) = Connection::new(
                                id,
                                read_half,
                                write_half,
                                self.dm_event_sender.clone().into(),
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

                            let message = ServerMessage::ServerInfo(ServerInfo::new("Test server"));
                            connection_handle.sender.send(message).await.unwrap();

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
                        DeviceManagerEvent::ConnectionEvent(event) => {
                            match event {
                                ConnectionEvent::ReadError(id, error) => {
                                    eprintln!("Connection id {} read error {:?}", id, error);

                                    let connection = self.connections.remove(&id).unwrap();
                                    connection.quit_sender.send(()).unwrap();
                                    connection.task_handle.await.unwrap();
                                }
                                ConnectionEvent::WriteError(id, error) => {
                                    eprintln!("Connection id {} write error {:?}", id, error);

                                    let connection = self.connections.remove(&id).unwrap();
                                    connection.quit_sender.send(()).unwrap();
                                    connection.task_handle.await.unwrap();
                                }
                                ConnectionEvent::Message(id, message) => {
                                    println!("Connection id {} message {:?}", id, message);
                                }
                            }
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
}

pub struct DeviceManagerTask;


impl DeviceManagerTask {
    pub fn task(shutdown_watch: ShutdownWatch) -> (
        JoinHandle<()>,
        SendDownward<DeviceManagerEvent>,
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
