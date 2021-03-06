/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Connection to the server.

use log::{error};

use super::FromServerToUiSender;
use std::{thread::JoinHandle, time::Duration};

use libjonect::{
    ui::{UiProtocolFromServerToUi, UiProtocolFromUiToServer},
    utils::{QuitReceiver, Connection, ConnectionEvent},
};

use tokio::{
    net::TcpStream,
    runtime::Runtime,
    sync::{mpsc, oneshot},
};

use crate::{
    config::EVENT_CHANNEL_SIZE,
};

/// Server connection thread handle.
pub struct ServerConnectionHandle {
    logic_thread: Option<JoinHandle<()>>,
    server_event_sender: mpsc::Sender<UiProtocolFromUiToServer>,
    quit_sender: Option<oneshot::Sender<()>>,
}

impl ServerConnectionHandle {
    /// Create new `ServerConnectionHandle`.
    pub fn new(sender: FromServerToUiSender) -> Self {
        let (quit_sender, quit_receiver) = oneshot::channel();
        let (server_event_sender, receiver) =
            mpsc::channel::<UiProtocolFromUiToServer>(EVENT_CHANNEL_SIZE);

        let logic_thread = Some(std::thread::spawn(move || {
            let rt = Runtime::new().unwrap();

            let client = ServerClient::run(sender, receiver, quit_receiver);

            rt.block_on(client);
        }));

        Self {
            logic_thread,
            server_event_sender,
            quit_sender: Some(quit_sender),
        }
    }

    /// Send UI protocol message to the server.
    pub fn send(&mut self, message: UiProtocolFromUiToServer) {
        // TODO: Do not queue messages if there is no connection to the server?
        // TODO: Clear message queue when new connection is established?
        self.server_event_sender.blocking_send(message).unwrap();
    }

    /// Quit server connection thread. Blocks until thread is closed.
    pub fn quit(&mut self) {
        self.quit_sender.take().unwrap().send(()).unwrap();
        self.logic_thread.take().unwrap().join().unwrap();
    }
}

/// Reason why `ServerClient::handle_connection` quits.
enum QuitReason {
    /// UI thread requested quit.
    QuitRequest,
    /// Server connection error.
    ConnectionError,
}

/// Connect to the server.
struct ServerClient;

impl ServerClient {
    /// Connect to the server.
    pub async fn run(
        mut ui_sender: FromServerToUiSender,
        mut ui_receiver: mpsc::Receiver<UiProtocolFromUiToServer>,
        mut quit_receiver: QuitReceiver,
    ) {
        loop {
            tokio::select! {
                result = &mut quit_receiver => return result.unwrap(),
                result = TcpStream::connect("127.0.0.1:8081") => {
                    match result {
                        Ok(connection) => {
                            let quit_reason = Self::handle_connection(
                                connection,
                                &mut ui_sender,
                                &mut ui_receiver,
                                &mut quit_receiver
                            ).await;

                            match quit_reason {
                                QuitReason::QuitRequest => return,
                                QuitReason::ConnectionError => (),
                            }
                        }
                        Err(e) => {
                            error!("Error: {}", e);
                            ui_sender.send_connect_to_server_failed();
                        }
                    }
                }
            }

            tokio::select! {
                result = &mut quit_receiver => return result.unwrap(),
                _ = tokio::time::sleep(Duration::from_secs(5)) => (),
            }
        }
    }

    /// Handle server TCP connection.
    pub async fn handle_connection(
        stream: TcpStream,
        ui_sender: &mut FromServerToUiSender,
        ui_receiver: &mut mpsc::Receiver<UiProtocolFromUiToServer>,
        mut quit_receiver: &mut QuitReceiver,
    ) -> QuitReason {
        let (sender, mut connections_receiver) =
            mpsc::channel::<ConnectionEvent<UiProtocolFromServerToUi>>(EVENT_CHANNEL_SIZE);

        let connection_handle =
            Connection::spawn_connection_task(stream, sender.into());

        let quit_reason = loop {
            tokio::select! {
                result = &mut quit_receiver => {
                    result.unwrap();
                    break QuitReason::QuitRequest;
                }
                message = ui_receiver.recv() => {
                    tokio::select! {
                        result = &mut quit_receiver => {
                            result.unwrap();
                            break QuitReason::QuitRequest;
                        }
                        _ = connection_handle.send_down(message) => (),
                    }
                }
                event = connections_receiver.recv() => {
                    match event.unwrap() {
                        ConnectionEvent::ReadError(error) => {
                            error!("UI connection read error {:?}", error);
                            ui_sender.send_server_disconnected_message();
                            break QuitReason::ConnectionError;
                        }
                        ConnectionEvent::WriteError(error) => {
                            error!("UI connection write error {:?}", error);
                            ui_sender.send_server_disconnected_message();
                            break QuitReason::ConnectionError;
                        }
                        ConnectionEvent::Message(message) => {
                            ui_sender.send(message);
                        }
                    }
                }
            }
        };

        // Quit started. Wait all components to close.

        connection_handle.quit().await;

        quit_reason
    }
}
