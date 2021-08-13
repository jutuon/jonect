//! User interface communication protocol.

use bytes::BytesMut;
use serde::{Deserialize, Serialize};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::{TcpListener, TcpStream, tcp::WriteHalf}, sync::{mpsc}, task::JoinHandle};


use crate::server::utils::JsonMessageConnectionReader;

use super::EVENT_CHANNEL_SIZE;

#[derive(Debug, Deserialize, Serialize)]
pub enum Event {
    InitStart,
    InitError,
    InitEnd,
    Message(String),
    /// Server requested disconnect from the UI.
    DisconnectRequest,
    DisconnectRequestResponse,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum FromUiToServerEvent {
    /// UI requested disconnect from the server.
    DisconnectRequest,
    DisconnectRequestResponse,
    NotificationTest,
    //QuitProgressCheck,
    //AudioServerStateChange,
    //DMStateChange,
}



pub enum InternalFromServerToUiEvent {
    SendMessage(Event),
    /// Quit task.
    RequestQuit,
}

pub struct UiConnectionManager {
    server_sender: mpsc::Sender<FromUiToServerEvent>,
    ui_receiver: mpsc::Receiver<InternalFromServerToUiEvent>,
}

impl UiConnectionManager {
    fn new() -> (Self, mpsc::Sender<InternalFromServerToUiEvent>, mpsc::Receiver<FromUiToServerEvent>) {
        let (ui_sender, ui_receiver) = mpsc::channel(EVENT_CHANNEL_SIZE);
        let (server_sender, server_receiver) = mpsc::channel(EVENT_CHANNEL_SIZE);

        let manager = Self {
            server_sender,
            ui_receiver,
        };

        (manager, ui_sender, server_receiver)
    }

    async fn run(&mut self) {

        let listener = match TcpListener::bind("127.0.0.1:8081").await {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("UI connection disabled. Error: {:?}", e);
                self.wait_shutdown().await;
                return;
            }
        };

        loop {
            // TODO: Check shutdown events.
            let listener_result = listener.accept().await;
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

    async fn handle_connection(&mut self, mut connection: TcpStream) {
        let mut buffer = BytesMut::new();

        let (mut read, mut write) = connection.split();

        let mut reader = JsonMessageConnectionReader::new();


        //let r_task = reader.read_stream(read);

        //tokio::pin!(r_task);



        let mut w_task = Self::handle_writing(&mut write, &mut self.ui_receiver);
        tokio::pin!(w_task);

        loop {
            tokio::select! {
                quit_reason = &mut w_task => {
                    match quit_reason {
                        QuitReason::QuitRequested => {
                            // Request disconnect.
                        },
                        QuitReason::Error(e) => {
                            eprintln!()
                        }
                    }

                    return;
                }
            }
        }



       // self.self.connection.
    }

    async fn handle_writing<'a>(
        write_half: &mut WriteHalf<'a>,
        ui_receiver: &mut mpsc::Receiver<InternalFromServerToUiEvent>
    ) -> QuitReason {
        loop {
            let event = ui_receiver.recv().await.unwrap();

            match event {
                InternalFromServerToUiEvent::RequestQuit => {
                    return QuitReason::QuitRequested;
                }
                InternalFromServerToUiEvent::SendMessage(m) => {
                    let data = match serde_json::to_vec(&m) {
                        Ok(data) => data,
                        Err(e) => {
                            // TODO: Send error to client?
                            eprintln!("Warning: Message '{:?}' skipped because of serialization error '{}'", m, e);
                            continue;
                        }
                    };

                    if let Err(e) = write_half.write_all(&data).await {
                        return QuitReason::Error(e);
                    }
                }
            }
        }
    }

    async fn wait_shutdown(&mut self) {
        loop {
            let event = self.ui_receiver.recv().await.unwrap();
            if let InternalFromServerToUiEvent::RequestQuit = event  {
                return
            }
        }
    }

    pub fn task() -> (
        JoinHandle<()>,
        mpsc::Sender<InternalFromServerToUiEvent>,
        mpsc::Receiver<FromUiToServerEvent>,
    ) {
        let (mut cm, ui_sender, server_receiver) = UiConnectionManager::new();

        let task = async move {
            cm.run().await;
        };

        let handle = tokio::spawn(task);

        (handle, ui_sender, server_receiver)
    }
}

enum QuitReason {
    QuitRequested,
    Error(std::io::Error),
}
