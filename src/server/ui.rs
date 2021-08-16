//! User interface communication protocol.

use serde::{Deserialize, Serialize};
use tokio::{net::{TcpListener, TcpStream}, sync::{mpsc, oneshot}, task::JoinHandle};

use crate::{config::EVENT_CHANNEL_SIZE, utils::{Connection, ConnectionEvent, QuitReceiver, QuitSender, SendDownward, SendUpward, ShutdownWatch}};

#[derive(Debug, Deserialize, Serialize)]
pub enum UiProtocolFromServerToUi {
    InitStart,
    InitError,
    InitEnd,
    Message(String),
}

#[derive(Debug, Deserialize, Serialize)]
pub enum UiProtocolFromUiToServer {
    NotificationTest,
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

                    self.handle_connection(socket).await;
                }
            }
        }
    }

    async fn handle_connection(
        &mut self,
        connection: TcpStream,
    ) {
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

        loop {
            tokio::select! {
                event = &mut self.quit_receiver => return event.unwrap(),
                event = self.ui_receiver.recv() => {
                    connection_handle.send_down(event.unwrap()).await;
                }
                event = connections_receiver.recv() => {
                    match event.unwrap() {
                        ConnectionEvent::ReadError(id, error) => {
                            eprintln!("Connection id {} read error {:?}", id, error);
                            break;
                        }
                        ConnectionEvent::WriteError(id, error) => {
                            eprintln!("Connection id {} write error {:?}", id, error);
                            break;
                        }
                        ConnectionEvent::Message(_, message) => {
                            self.server_sender.send_up(message).await;
                        }
                    }
                }

            }
        }

        connection_handle.quit().await;
    }
}
