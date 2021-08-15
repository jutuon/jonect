use bytes::BytesMut;
use serde::{Serialize, de::DeserializeOwned};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, sync::{mpsc, oneshot}, task::JoinHandle};

use std::{convert::TryInto, fmt::{self, Debug}, io::{ErrorKind}};

use crate::config::EVENT_CHANNEL_SIZE;

/// Drop this type after component is closed.
pub type ShutdownWatch = mpsc::Sender<()>;
pub type QuitSender = oneshot::Sender<()>;

pub type ConnectionId = u64;

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

/// Panic might happen if connection handle is dropped before the task
/// is closed.
pub struct ConnectionHandle<M: Debug> {
    id: ConnectionId,
    task_handle: JoinHandle<()>,
    sender: SendDownward<M>,
    quit_sender: oneshot::Sender<()>,
}

impl <M: Debug> ConnectionHandle<M> {
    pub fn new(
        id: ConnectionId,
        task_handle: JoinHandle<()>,
        sender: SendDownward<M>,
        quit_sender: oneshot::Sender<()>,
    ) -> Self {
        Self {
            id,
            task_handle,
            sender,
            quit_sender,
        }
    }

    pub fn id(&self) -> ConnectionId {
        self.id
    }

    pub async fn quit(self) {
        self.quit_sender.send(()).unwrap();
        self.task_handle.await.unwrap();
    }

    pub async fn send_down(&self, message: M) {
        self.sender.send_down(message).await
    }
}

#[derive(Debug)]
pub enum ConnectionEvent<M> {
    ReadError(ConnectionId, ReadError),
    WriteError(ConnectionId, WriteError),
    Message(ConnectionId, M),
}

impl <M> ConnectionEvent<M> {
    pub fn is_error(&self) -> bool {
        match self {
            Self::ReadError(_,_) | Self::WriteError(_,_) => true,
            Self::Message(_,_) => false,
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
pub struct Connection<
    R: AsyncReadExt + Unpin + Send + 'static,
    W: AsyncWriteExt + Unpin + Send + 'static,
    ReceiveM: DeserializeOwned + Debug + Send + 'static,
    SendM: Serialize + Debug + Send + 'static
> {
    id: ConnectionId,
    read_half: R,
    write_half: W,
    sender: SendUpward<ConnectionEvent<ReceiveM>>,
    receiver: mpsc::Receiver<SendM>,
    quit_receiver: oneshot::Receiver<()>,
    _shutdown_watch: ShutdownWatch,
}

impl <
    R: AsyncReadExt + Unpin + Send + 'static,
    W: AsyncWriteExt + Unpin + Send + 'static,
    ReceiveM: DeserializeOwned + Debug + Send + 'static,
    SendM: Serialize + Debug + Send + 'static,
    > Connection<R, W, ReceiveM, SendM> {

    pub fn spawn_connection_task(
        id: ConnectionId,
        read_half: R,
        write_half: W,
        sender: SendUpward<ConnectionEvent<ReceiveM>>,
        _shutdown_watch: ShutdownWatch,
    ) -> ConnectionHandle<SendM> {
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

        let task_handle = tokio::spawn(async move {
            connection.connection_task().await
        });

        ConnectionHandle::new(
            id,
            task_handle,
            event_sender.into(),
            quit_sender,
        )
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
                                _ = self.sender.send_up(event) => (),
                                quit = &mut self.quit_receiver => return quit.unwrap(),
                            );
                        }

                        Err(e) => {
                            let event = ConnectionEvent::ReadError(self.id, e);
                            tokio::select!(
                                _ = self.sender.send_up(event) => (),
                                quit = &mut self.quit_receiver => return quit.unwrap(),
                            );
                            break;
                        }
                    }
                }
                error = &mut w_task => {
                    let event = ConnectionEvent::WriteError(self.id, error);
                    tokio::select!(
                        _ = self.sender.send_up(event) => (),
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
    ) -> (Result<ReceiveM, ReadError>, BytesMut, R) {
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
        mut receiver: mpsc::Receiver<SendM>,
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
