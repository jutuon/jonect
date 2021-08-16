
use std::{thread::{JoinHandle}, time::Duration};
use super::FromServerToUiSender;

use crate::{server::ui::{UiProtocolFromServerToUi, UiProtocolFromUiToServer}, utils::QuitReceiver};

use tokio::{net::TcpStream, runtime::Runtime, sync::{mpsc, oneshot}};

use crate::{config::{EVENT_CHANNEL_SIZE}, utils::{Connection, ConnectionEvent}};

pub struct ServerConnectionHandle {
    logic_thread: Option<JoinHandle<()>>,
    server_event_sender: mpsc::Sender<UiProtocolFromUiToServer>,
    quit_sender: Option<oneshot::Sender<()>>,
}

impl ServerConnectionHandle {
    pub fn new(sender: FromServerToUiSender) -> Self {
        let (quit_sender, quit_receiver) = oneshot::channel();
        let (server_event_sender, receiver) = mpsc::channel::<UiProtocolFromUiToServer>(EVENT_CHANNEL_SIZE);

        let s = server_event_sender.clone();
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

    pub fn send(&mut self, message: UiProtocolFromUiToServer) {
        // TODO: Do not queue messages if there is no connection to the server?
        // TODO: Clear message queue when new connection is established?
        self.server_event_sender
            .blocking_send(message).unwrap();
    }

    pub fn quit(&mut self) {
        self.quit_sender.take().unwrap().send(()).unwrap();
        self.logic_thread.take().unwrap().join().unwrap();
    }
}


enum QuitReason {
    QuitRequest,
    ConnectionError,
}

struct ServerClient;

impl ServerClient {
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
                            eprintln!("Error: {}", e);
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

    pub async fn handle_connection(
        stream: TcpStream,
        ui_sender: &mut FromServerToUiSender,
        ui_receiver: &mut mpsc::Receiver<UiProtocolFromUiToServer>,
        mut quit_receiver: &mut QuitReceiver,
    ) -> QuitReason {
        let (shutdown_watch, mut shutdown_watch_receiver) = mpsc::channel(1);
        let (read_half, write_half) = stream.into_split();

        let (sender, mut connections_receiver) =
            mpsc::channel::<ConnectionEvent<UiProtocolFromServerToUi>>(EVENT_CHANNEL_SIZE);

        let connection_handle = Connection::spawn_connection_task(
            0,
            read_half,
            write_half,
            sender.into(),
            shutdown_watch
        );

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
                        ConnectionEvent::ReadError(id, error) => {
                            eprintln!("Connection id {} read error {:?}", id, error);
                            ui_sender.send_server_disconnected_message();
                            break QuitReason::ConnectionError;
                        }
                        ConnectionEvent::WriteError(id, error) => {
                            eprintln!("Connection id {} write error {:?}", id, error);
                            ui_sender.send_server_disconnected_message();
                            break QuitReason::ConnectionError;
                        }
                        ConnectionEvent::Message(_, message) => {
                            ui_sender.send(message);
                        }
                    }
                }
            }
        };

        // Quit started. Wait all components to close.

        connection_handle.quit().await;
        let _ = shutdown_watch_receiver.recv().await;

        quit_reason
    }

}
