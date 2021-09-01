/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

pub mod protocol;
pub mod state;
pub mod data;

use bytes::BytesMut;
use serde::Serialize;
use tokio::{io::{AsyncReadExt, AsyncWrite, AsyncWriteExt}, net::{TcpListener, TcpStream, tcp::{ReadHalf, WriteHalf}}, sync::{mpsc, oneshot}, task::JoinHandle};

use std::{collections::HashMap, convert::TryInto, fmt::{self, Debug}, io::{self, ErrorKind}, pin, time::Duration};

use crate::{server::device::data::DataConnectionEvent, utils::{Connection, ConnectionEvent, ConnectionHandle, ConnectionId, SendDownward, ShutdownWatch}};

use self::{data::{DataConnectionHandle, TcpSendHandle}, protocol::{ClientMessage, ServerInfo, ServerMessage}, state::{DeviceEvent, DeviceState}};

use crate::config::{EVENT_CHANNEL_SIZE};

#[derive(Debug)]
pub enum FromDeviceManagerToServerEvent {
    TcpSupportDisabledBecauseOfError(TcpSupportError),
    DataConnection(TcpSendHandle),
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
    RunDeviceConnectionPing,
}

type DeviceId = String;

pub struct DeviceManager {
    server_sender: mpsc::Sender<FromDeviceManagerToServerEvent>,
    receiver: mpsc::Receiver<DeviceManagerEvent>,
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

        let mut connections = HashMap::<ConnectionId, DeviceState>::new();
        let (connections_sender, mut connections_receiver) =
            mpsc::channel::<ConnectionEvent<ClientMessage>>(EVENT_CHANNEL_SIZE);
        let (device_event_sender, mut device_event_receiver) =
            mpsc::channel::<(ConnectionId, DeviceEvent)>(EVENT_CHANNEL_SIZE);

        let mut ping_timer = tokio::time::interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                result = listener.accept(), if tcp_listener_enabled => {
                    match result {
                        Ok((stream, address)) => {
                            let id = self.next_connection_id;
                            self.next_connection_id = match id.checked_add(1) {
                                Some(new_next_id) => new_next_id,
                                None => {
                                    eprintln!("Warning: Couldn't handle a new connection because there is no new connection ID numbers.");
                                    continue;
                                }
                            };

                            let (read_half, write_half) = stream.into_split();
                            let connection_handle = Connection::spawn_connection_task(
                                id,
                                read_half,
                                write_half,
                                connections_sender.clone().into(),
                                self._shutdown_watch.clone(),
                            );

                            let device_state = DeviceState::new(
                                connection_handle,
                                device_event_sender.clone().into(),
                                address,
                                self._shutdown_watch.clone(),
                            ).await;

                            connections.insert(id, device_state);
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
                        DeviceManagerEvent::RunDeviceConnectionPing => {
                            for connection in connections.values_mut() {
                                connection.send_ping_message().await;
                            }
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
                            connections.get_mut(&id).unwrap().handle_client_message(message).await;
                        }
                    }
                }
                event = device_event_receiver.recv() => {
                    let (id, event) = event.unwrap();
                    match event {
                        DeviceEvent::Test => (),
                        DeviceEvent::DataConnection(DataConnectionEvent::NewConnection(handle)) => {
                            let e = FromDeviceManagerToServerEvent::DataConnection(handle);
                            self.server_sender.send(e).await.unwrap();
                        }
                        DeviceEvent::DataConnection(event) => {
                            connections.get_mut(&id).unwrap().handle_data_connection_message(event).await;
                        }
                    }
                }
                _ = ping_timer.tick() => {
                    for connection in connections.values_mut() {
                        connection.send_ping_message().await;
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
