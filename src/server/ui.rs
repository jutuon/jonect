/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! User interface communication protocol.

use serde::{Deserialize, Serialize};
use tokio::{net::{TcpListener, TcpStream}, sync::{mpsc, oneshot}, task::JoinHandle};

use crate::{config::{self, EVENT_CHANNEL_SIZE}, server::device::DeviceManagerEvent, utils::{Connection, ConnectionEvent, QuitReceiver, QuitSender, SendDownward, SendUpward, ShutdownWatch}};

use super::{device::TcpSupportError, message_router::{MessageReceiver, RouterSender}};

#[derive(Debug, Deserialize, Serialize)]
pub enum UiProtocolFromServerToUi {
    Message(String),
}

#[derive(Debug, Deserialize, Serialize)]
pub enum UiProtocolFromUiToServer {
    NotificationTest,
    RunDeviceConnectionPing,
}

#[derive(Debug)]
pub enum UiEvent {
    TcpSupportDisabledBecauseOfError(TcpSupportError),
}

enum QuitReason {
    QuitRequest,
    ConnectionError,
}

pub struct UiConnectionManager {
    server_sender: RouterSender,
    ui_receiver: MessageReceiver<UiEvent>,
    quit_receiver: QuitReceiver,
    shutdown_watch: ShutdownWatch,
}

impl UiConnectionManager {

    pub fn task(
        shutdown_watch: ShutdownWatch,
        server_sender: RouterSender,
        ui_receiver: MessageReceiver<UiEvent>,
    ) -> (
        JoinHandle<()>,
        QuitSender,
    ) {

        let (quit_sender, quit_receiver) = oneshot::channel();

        let cm = Self {
            server_sender,
            ui_receiver,
            quit_receiver,
            shutdown_watch,
        };

        let task = async move {
            cm.run().await;
        };

        let handle = tokio::spawn(task);

        (handle, quit_sender)
    }

    async fn run(mut self) {
        let listener = match TcpListener::bind(config::UI_SOCKET_ADDRESS).await {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("UI connection disabled. Error: {:?}", e);
                self.quit_receiver.await.unwrap();
                return;
            }
        };

        loop {
            tokio::select! {
                event = &mut self.quit_receiver => return event.unwrap(),
                listener_result = listener.accept() => {
                    let socket = match listener_result {
                        Ok((socket, _)) => socket,
                        Err(e) => {
                            eprintln!("Error: {:?}", e);
                            continue;
                        }
                    };

                    match Self::handle_connection(
                        &mut self.server_sender,
                        &mut self.ui_receiver,
                        &mut self.quit_receiver,
                        &mut self.shutdown_watch,
                        socket,
                    ).await {
                        QuitReason::QuitRequest => return,
                        QuitReason::ConnectionError => (),
                    }
                }
            }
        }
    }

    async fn handle_connection(
        mut server_sender: &mut RouterSender,
        ui_receiver: &mut MessageReceiver<UiEvent>,
        mut quit_receiver: &mut QuitReceiver,
        shutdown_watch: &mut ShutdownWatch,
        connection: TcpStream,
    ) -> QuitReason {
        let (read_half, write_half) = connection.into_split();

        let (sender, mut connections_receiver) =
            mpsc::channel::<ConnectionEvent<UiProtocolFromUiToServer>>(EVENT_CHANNEL_SIZE);

        let connection_handle = Connection::spawn_connection_task(
            0,
            read_half,
            write_half,
            sender.into(),
            shutdown_watch.clone(),
        );

        tokio::pin!(ui_receiver);

        let quit_reason = loop {
            tokio::select! {
                event = &mut quit_receiver => {
                    event.unwrap();
                    break QuitReason::QuitRequest;
                },
                message = ui_receiver.recv() => {
                    match message {
                        UiEvent::TcpSupportDisabledBecauseOfError(error) => {
                            eprintln!("TCP support disabled {:?}", error);
                            continue;
                        }
                    }

                    tokio::select! {
                        result = &mut quit_receiver => {
                            result.unwrap();
                            break QuitReason::QuitRequest;
                        }
                        _ = connection_handle.send_down(UiProtocolFromServerToUi::Message("test".to_string())) => (),
                    }
                }
                event = connections_receiver.recv() => {
                    match event.unwrap() {
                        ConnectionEvent::ReadError(id, error) => {
                            eprintln!("Connection id {} read error {:?}", id, error);
                            break QuitReason::ConnectionError;
                        }
                        ConnectionEvent::WriteError(id, error) => {
                            eprintln!("Connection id {} write error {:?}", id, error);
                            break QuitReason::ConnectionError;
                        }
                        ConnectionEvent::Message(_, message) => {
                            let sender = &mut server_sender;
                            let handle_message = async move {
                                match message {
                                    UiProtocolFromUiToServer::NotificationTest => {
                                        println!("UI notification");
                                    }
                                    UiProtocolFromUiToServer::RunDeviceConnectionPing => {
                                        sender.send_device_manager_event(DeviceManagerEvent::RunDeviceConnectionPing).await;
                                    }
                                }
                            };
                            tokio::select! {
                                result = &mut quit_receiver => {
                                    result.unwrap();
                                    break QuitReason::QuitRequest;
                                }
                                _ = handle_message => (),
                            };
                        }
                    }
                }

            }
        };

        connection_handle.quit().await;
        quit_reason
    }
}
