pub mod protocol;

use bytes::BytesMut;
use serde::Serialize;
use tokio::{io::{AsyncReadExt, AsyncWrite, AsyncWriteExt}, net::{TcpListener, TcpStream, tcp::{ReadHalf, WriteHalf}}, sync::{mpsc, oneshot}, task::JoinHandle};

use std::{collections::HashMap, convert::TryInto, fmt::{self, Debug}, io::{self, ErrorKind}, pin};

use crate::utils::{Connection, ConnectionEvent, ConnectionHandle, ConnectionId, SendDownward, ShutdownWatch};

use self::protocol::{ClientMessage, ServerInfo, ServerMessage};

use crate::config::{EVENT_CHANNEL_SIZE};

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
pub enum DeviceManagerEvent {
    RequestQuit,
    Message(String),
}

type DeviceId = String;

pub struct DeviceManager {
    server_sender: mpsc::Sender<FromDeviceManagerToServerEvent>,
    receiver: mpsc::Receiver<DeviceManagerEvent>,
    dm_event_sender: mpsc::Sender<DeviceManagerEvent>,
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

        let mut connections = HashMap::new();
        let (connections_sender, mut connections_receiver) =
            mpsc::channel::<ConnectionEvent<ClientMessage>>(EVENT_CHANNEL_SIZE);

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
                                connections_sender.clone().into(),
                                self._shutdown_watch.clone(),
                            );

                            let task_handle = tokio::spawn(async move {
                                connection.connection_task().await
                            });

                            let connection_handle = ConnectionHandle::new(
                                id,
                                task_handle,
                                connection_sender,
                                quit_sender,
                            );

                            let message = ServerMessage::ServerInfo(ServerInfo::new("Test server"));
                            connection_handle.send_down(message).await;

                            connections.insert(id, connection_handle);
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
                        DeviceManagerEvent::RequestQuit => {
                            break;
                        }
                    }
                }
                event = connections_receiver.recv() => {
                    match event.unwrap() {
                        ConnectionEvent::ReadError(id, error) => {
                            eprintln!("Connection id {} read error {:?}", id, error);

                            connections.remove(&id).unwrap().quit().await;
                        }
                        ConnectionEvent::WriteError(id, error) => {
                            eprintln!("Connection id {} write error {:?}", id, error);

                            connections.remove(&id).unwrap().quit().await;
                        }
                        ConnectionEvent::Message(id, message) => {
                            println!("Connection id {} message {:?}", id, message);
                        }
                    }
                }
            }
        }


        // Quit

        for connection in connections.into_values() {
            connection.quit().await;
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
