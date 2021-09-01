/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! User interface communication protocol.

use serde::{Deserialize, Serialize};
use tokio::{net::{TcpListener, TcpStream}, sync::{mpsc, oneshot}, task::JoinHandle};

use crate::{config::EVENT_CHANNEL_SIZE, utils::{Connection, ConnectionEvent, QuitReceiver, QuitSender, SendDownward, SendUpward, ShutdownWatch}};

#[derive(Debug, Deserialize, Serialize)]
pub enum UiProtocolFromServerToUi {
    Message(String),
}

#[derive(Debug, Deserialize, Serialize)]
pub enum UiProtocolFromUiToServer {
    NotificationTest,
    RunDeviceConnectionPing,
}

enum QuitReason {
    QuitRequest,
    ConnectionError,
}

pub struct UiConnectionManager {
    server_sender: SendUpward<UiProtocolFromUiToServer>,
    ui_receiver: mpsc::Receiver<UiProtocolFromServerToUi>,
    quit_receiver: QuitReceiver,
    shutdown_watch: ShutdownWatch,
}

impl UiConnectionManager {

    pub fn task(shutdown_watch: ShutdownWatch) -> (
        JoinHandle<()>,
        SendDownward<UiProtocolFromServerToUi>,
        mpsc::Receiver<UiProtocolFromUiToServer>,
        QuitSender,
    ) {

        let (ui_sender, ui_receiver) = mpsc::channel(EVENT_CHANNEL_SIZE);
        let (server_sender, server_receiver) = mpsc::channel(EVENT_CHANNEL_SIZE);
        let (quit_sender, quit_receiver) = oneshot::channel();

        let cm = Self {
            server_sender: server_sender.into(),
            ui_receiver,
            quit_receiver,
            shutdown_watch,
        };

        let task = async move {
            cm.run().await;
        };

        let handle = tokio::spawn(task);

        (handle, ui_sender.into(), server_receiver, quit_sender)
    }

    async fn run(mut self) {
        let listener = match TcpListener::bind("127.0.0.1:8081").await {
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

                    match self.handle_connection(socket).await {
                        QuitReason::QuitRequest => return,
                        QuitReason::ConnectionError => (),
                    }
                }
            }
        }
    }

    async fn handle_connection(
        &mut self,
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
            self.shutdown_watch.clone(),
        );

        let quit_reason = loop {
            tokio::select! {
                event = &mut self.quit_receiver => {
                    event.unwrap();
                    break QuitReason::QuitRequest;
                },
                message = self.ui_receiver.recv() => {
                    tokio::select! {
                        result = &mut self.quit_receiver => {
                            result.unwrap();
                            break QuitReason::QuitRequest;
                        }
                        _ = connection_handle.send_down(message) => (),
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
                            tokio::select! {
                                result = &mut self.quit_receiver => {
                                    result.unwrap();
                                    break QuitReason::QuitRequest;
                                }
                                _ = self.server_sender.send_up(message) => (),
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
