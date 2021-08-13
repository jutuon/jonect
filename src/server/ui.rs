//! User interface communication protocol.

use std::{convert::TryInto, io::ErrorKind, time::Duration};

use bytes::BytesMut;
use serde::{Deserialize, Serialize};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::{TcpListener, TcpStream, tcp::{OwnedReadHalf, OwnedWriteHalf, ReadHalf, WriteHalf}}, sync::{mpsc, oneshot}, task::JoinHandle};

use super::{EVENT_CHANNEL_SIZE, ShutdownWatch};

#[derive(Debug, Deserialize, Serialize)]
pub enum UiMessage {
    InitStart,
    InitError,
    InitEnd,
    Message(String),
}

#[derive(Debug, Deserialize, Serialize)]
pub enum UiProtocolFromServerToUi {
    UiMessage(UiMessage),
    DisconnectRequest,
    DisconnectRequestResponse,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum UiProtocolFromUiToServer {
    ServerMessage(UiProtocolServerMessage),
    DisconnectRequest,
    DisconnectRequestResponse,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum UiProtocolServerMessage {
    NotificationTest,
}

#[derive(Debug)]
pub enum ConnectionManagerMessage {
    SendMessage(UiMessage),
    /// Quit task.
    RequestQuit,
}

struct QuitRequestReceived;

enum WriteTaskQuitMode {
    SendRequestResponseWithTimeout,
    Normal,
}

pub struct UiConnectionManager {
    server_sender: mpsc::Sender<UiProtocolServerMessage>,
    ui_receiver: mpsc::Receiver<ConnectionManagerMessage>,
    _shutdown_watch: ShutdownWatch,
}

impl UiConnectionManager {
    fn new(shutdown_watch: ShutdownWatch) -> (Self, mpsc::Sender<ConnectionManagerMessage>, mpsc::Receiver<UiProtocolServerMessage>) {
        let (ui_sender, ui_receiver) = mpsc::channel(EVENT_CHANNEL_SIZE);
        let (server_sender, server_receiver) = mpsc::channel(EVENT_CHANNEL_SIZE);

        let manager = Self {
            server_sender,
            ui_receiver,
            _shutdown_watch: shutdown_watch,
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
            tokio::select! {
                event = self.ui_receiver.recv() => {
                    if let ConnectionManagerMessage::RequestQuit = event.unwrap() {
                        return;
                    }
                }
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

    async fn handle_connection(&mut self, connection: TcpStream) -> Option<QuitRequestReceived> {
        let (read, write) = connection.into_split();

        let buffer = BytesMut::new();
        let r_task = Self::handle_reading(buffer, read);
        tokio::pin!(r_task);

        let (w_quit_sender, w_quit_receiver) = oneshot::channel::<WriteTaskQuitMode>();
        let (w_data_sender, w_data_receiver) = mpsc::channel::<UiProtocolFromServerToUi>(EVENT_CHANNEL_SIZE);
        // Spawn task for writing so w_event_sender.send() will not deadlock.
        let w_handle = tokio::spawn(Self::handle_writing(write, w_data_receiver, w_quit_receiver));
        tokio::pin!(w_handle);

        enum QuitMode {
            RequestQuit,
            SendDisconnectRequestResponse,
        }


        let quit_mode;

        loop {
            tokio::select! {
                event = self.ui_receiver.recv() => {
                    match event.unwrap() {
                        ConnectionManagerMessage::RequestQuit => {
                            // Begin quit.
                            quit_mode = QuitMode::RequestQuit;
                            break;
                        }
                        ConnectionManagerMessage::SendMessage(m) => {
                            let m = UiProtocolFromServerToUi::UiMessage(m);
                            w_data_sender.send(m).await.unwrap();
                        }
                    }
                }
                end_result = &mut w_handle => {
                    match end_result.unwrap() {
                        Ok(()) => panic!("Writing ended even if server quit request is not received."),
                        Err(e) => {
                            eprintln!("Writing error: {}", e);
                            return None;
                        }
                    }
                }
                (result, buffer, read_half) = &mut r_task => {
                    r_task.set(Self::handle_reading(buffer, read_half));

                    match result {
                        Ok(message) => {
                            match message {
                                UiProtocolFromUiToServer::DisconnectRequest => {
                                    quit_mode = QuitMode::SendDisconnectRequestResponse;
                                    break;
                                }
                                UiProtocolFromUiToServer::DisconnectRequestResponse => {
                                    eprintln!("Warning: Ui sent DisconnectRequestResponse even if disconnect is not requested.");
                                }
                                UiProtocolFromUiToServer::ServerMessage(m) => {
                                    self.server_sender.send(m).await.unwrap();
                                }
                            }
                        }
                        Err(ReadError::Io(e)) => {
                            eprintln!("Error: {}", e);
                            let _ = w_quit_sender.send(WriteTaskQuitMode::Normal);
                            if let Err(e) = w_handle.await.unwrap() {
                                eprintln!("Error: {}", e);
                            }
                            return None;
                        }
                        Err(ReadError::Deserialize(e)) => {
                            eprintln!("Warning: Message skipped because of deserialize error '{}'", e);
                        }
                    }
                }
            }
        }

        match quit_mode {
            QuitMode::RequestQuit => {
                let client_response_task = async {
                    let m = UiProtocolFromServerToUi::DisconnectRequest;
                    w_data_sender.send(m).await.unwrap();

                    loop {
                        tokio::select! {
                            (result, buffer, read_half) = &mut r_task => {
                                r_task.set(Self::handle_reading(buffer, read_half));

                                match result {
                                    Ok(UiProtocolFromUiToServer::DisconnectRequestResponse) => {
                                        break Ok(());
                                    }
                                    Ok(_) => (),
                                    Err(ReadError::Io(e)) => {
                                        break Err(e);
                                    }
                                    Err(ReadError::Deserialize(e)) => {
                                        eprintln!("Warning: Message skipped because of deserialize error '{}'", e);
                                    }
                                }
                            }
                        }
                    }
                };

                match tokio::time::timeout(Duration::from_secs(1), client_response_task).await {
                    Ok(Ok(())) | Err(_) => (),
                    Ok(Err(e)) => eprintln!("Error: {}", e),
                }

                let _ = w_quit_sender.send(WriteTaskQuitMode::Normal);
                if let Err(e) = w_handle.await.unwrap() {
                    eprintln!("Error: {}", e);
                }
            }
            QuitMode::SendDisconnectRequestResponse => {
                let _ = w_quit_sender.send(WriteTaskQuitMode::SendRequestResponseWithTimeout);
                if let Err(e) = w_handle.await.unwrap() {
                    eprintln!("Error: {}", e);
                }
            },
        }

        if let QuitMode::RequestQuit = quit_mode {
            Some(QuitRequestReceived)
        } else {
            None
        }
    }

    async fn handle_writing(
        mut write_half: OwnedWriteHalf,
        mut write_receiver: mpsc::Receiver<UiProtocolFromServerToUi>,
        quit_receiver: oneshot::Receiver<WriteTaskQuitMode>,
    ) -> Result<(), std::io::Error> {
        tokio::pin!(quit_receiver);

        loop {
            let message = tokio::select! {
                quit_mode = &mut quit_receiver => {
                    match quit_mode.unwrap() {
                        WriteTaskQuitMode::Normal => return Ok(()),
                        WriteTaskQuitMode::SendRequestResponseWithTimeout => break,
                    }
                },
                m = write_receiver.recv() => m.unwrap(),
            };

            let data = match serde_json::to_vec(&message) {
                Ok(data) => data,
                Err(e) => {
                    // TODO: Send error to client?
                    eprintln!("Warning: Message '{:?}' skipped because of serialization error '{}'", message, e);
                    continue;
                }
            };

            let data_len: u32 = match data.len().try_into() {
                Ok(len) => len,
                Err(e) => {
                    eprintln!("Warning: Message skipped because of size limit of u32. Error: '{}'", e);
                    continue;
                }
            };

            let mut quit = false;

            let data_len = data_len.to_be_bytes();
            tokio::select! {
                quit_mode = &mut quit_receiver => {
                    match quit_mode.unwrap() {
                        WriteTaskQuitMode::Normal => return Ok(()),
                        WriteTaskQuitMode::SendRequestResponseWithTimeout => quit = true,
                    }
                }
                result = write_half.write_all(&data_len) => result?,
            }

            tokio::select! {
                quit_mode = &mut quit_receiver => {
                    match quit_mode.unwrap() {
                        WriteTaskQuitMode::Normal => return Ok(()),
                        WriteTaskQuitMode::SendRequestResponseWithTimeout => quit = true,
                    }
                }
                result = write_half.write_all(&data) => result?,
            }

            if quit {
                break;
            }
        };

        let data = serde_json::to_vec(&UiProtocolFromServerToUi::DisconnectRequestResponse).unwrap();
        let data_len: u32 = data.len().try_into().unwrap();
        let data_len = data_len.to_be_bytes();

        match tokio::time::timeout(Duration::from_secs(1), write_half.write_all(&data_len)).await {
            Ok(Ok(())) => (),
            Ok(Err(e)) => return Err(e),    // IoError
            Err(_) => return Ok(()),        // Timeout
        }

        match tokio::time::timeout(Duration::from_secs(1), write_half.write_all(&data)).await {
            Ok(Ok(())) | Err(_) => Ok(()),
            Ok(Err(e)) => Err(e), // IoError
        }
    }

    pub async fn handle_reading(
        mut buffer: BytesMut,
        mut read_half: OwnedReadHalf,
    ) -> (Result<UiProtocolFromUiToServer, ReadError>, BytesMut, OwnedReadHalf) {
        buffer.clear();

        let message_len = match read_half.read_u32().await.map_err(ReadError::Io) {
            Ok(len) => len,
            Err(e) => return (Err(e), buffer, read_half),
        };

        let mut read_half_with_limit = read_half.take(message_len as u64);

        loop {
            match read_half_with_limit.read_buf(&mut buffer).await {
                Ok(_) => (),
                Ok(0) => {
                    if buffer.len() == message_len as usize {
                        let message = serde_json::from_slice(&buffer).map_err(ReadError::Deserialize);
                        return (message, buffer, read_half_with_limit.into_inner());
                    } else {
                        let error = std::io::Error::new(ErrorKind::UnexpectedEof, "");
                        return (Err(ReadError::Io(error)), buffer, read_half_with_limit.into_inner());
                    }
                }
                Err(e) => return (Err(ReadError::Io(e)), buffer, read_half_with_limit.into_inner()),
            }
        }
    }


    /// Return after server requests quit.
    async fn wait_shutdown(&mut self) {
        loop {
            let event = self.ui_receiver.recv().await.unwrap();
            if let ConnectionManagerMessage::RequestQuit = event  {
                return
            }
        }
    }

    pub fn task(shutdown_watch: ShutdownWatch) -> (
        JoinHandle<()>,
        mpsc::Sender<ConnectionManagerMessage>,
        mpsc::Receiver<UiProtocolServerMessage>,
    ) {
        let (mut cm, ui_sender, server_receiver) = UiConnectionManager::new(shutdown_watch);

        let task = async move {
            cm.run().await;
        };

        let handle = tokio::spawn(task);

        (handle, ui_sender, server_receiver)
    }
}


#[derive(Debug)]
pub enum ReadError {
    Io(std::io::Error),
    Deserialize(serde_json::error::Error),
}

#[derive(Debug)]
enum WriteEvent {
    Quit,
    Write(UiProtocolFromServerToUi),
}
