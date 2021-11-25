/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

pub mod data;
pub mod protocol;
pub mod state;

use tokio::{net::TcpListener, sync::mpsc, task::JoinHandle};

use std::{
    collections::HashMap,
    fmt::Debug,
    io::{self},
    time::Duration,
};

use crate::{
    config,
    server::{audio::AudioEvent, device::data::DataConnectionEvent},
    utils::{Connection, ConnectionEvent, ConnectionId},
};

use self::{
    data::TcpSendHandle,
    protocol::ClientMessage,
    state::{DeviceEvent, DeviceState},
};

use crate::config::EVENT_CHANNEL_SIZE;

use super::{
    message_router::{MessageReceiver, RouterSender},
    ui::UiEvent,
};

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
    r_sender: RouterSender,
    receiver: MessageReceiver<DeviceManagerEvent>,
    next_connection_id: u64,
}

impl DeviceManager {
    pub fn new(r_sender: RouterSender, receiver: MessageReceiver<DeviceManagerEvent>) -> Self {
        Self {
            r_sender,
            receiver,
            next_connection_id: 0,
        }
    }

    pub async fn run(mut self) {
        let listener = match TcpListener::bind(config::DEVICE_SOCKET_ADDRESS).await {
            Ok(listener) => listener,
            Err(e) => {
                let e = TcpSupportError::ListenerCreationError(e);
                let e = UiEvent::TcpSupportDisabledBecauseOfError(e);
                self.r_sender.send_ui_event(e).await;
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
                            );

                            let device_state = DeviceState::new(
                                connection_handle,
                                device_event_sender.clone().into(),
                                address,
                            ).await;

                            connections.insert(id, device_state);
                        }
                        Err(e) => {
                            let e = TcpSupportError::AcceptError(e);
                            let e = UiEvent::TcpSupportDisabledBecauseOfError(e);
                            self.r_sender.send_ui_event(e).await;

                            tcp_listener_enabled = false;
                        }
                    }
                }
                event = self.receiver.recv() => {
                    match event {
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
                            self.r_sender.send_audio_server_event(AudioEvent::StartRecording {
                                send_handle: handle,
                            }).await;
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
            let event = self.receiver.recv().await;
            if let DeviceManagerEvent::RequestQuit = event {
                return;
            }
        }
    }
}

pub struct DeviceManagerTask;

impl DeviceManagerTask {
    pub fn task(
        r_sender: RouterSender,
        receiver: MessageReceiver<DeviceManagerEvent>,
    ) -> JoinHandle<()> {
        let dm = DeviceManager::new(r_sender, receiver);

        let task = async move {
            dm.run().await;
        };

        tokio::spawn(task)
    }
}
