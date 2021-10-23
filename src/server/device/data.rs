/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Device data connection.

use std::net::SocketAddr;

use tokio::{net::TcpListener, sync::{mpsc, oneshot}, task::JoinHandle};

use crate::{config::EVENT_CHANNEL_SIZE, utils::{ConnectionId, ConnectionShutdownWatch, SendDownward, SendUpward, ShutdownWatch}};

use super::{DeviceId, state::DeviceEvent};


#[derive(Debug)]
pub struct TcpSendHandle {
    tcp_stream: std::net::TcpStream,
    _shutdown_watch: ConnectionShutdownWatch,
}

impl std::io::Write for TcpSendHandle {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.tcp_stream.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.tcp_stream.flush()
    }
}



#[derive(Debug)]
pub struct TcpSendConnection {
    shutdown_watch_receiver: mpsc::Receiver<()>,
}

impl TcpSendConnection {
    pub fn new(tcp_stream: std::net::TcpStream) -> Result<(Self, TcpSendHandle), std::io::Error> {
        tcp_stream.set_nonblocking(true)?;
        let (_shutdown_watch, shutdown_watch_receiver) = mpsc::channel::<()>(1);

        let handle = TcpSendHandle {
            tcp_stream,
            _shutdown_watch,
        };

        let connection = Self {
            shutdown_watch_receiver,
        };

        Ok((connection, handle))
    }

    pub async fn wait_quit(mut self) {
        let _ = self.shutdown_watch_receiver.recv().await;
    }
}

#[derive(Debug)]
pub enum DataConnectionEvent {
    TcpListenerBindError(std::io::Error),
    GetPortNumberError(std::io::Error),
    AcceptError(std::io::Error),
    SendConnectionError(std::io::Error),
    PortNumber(u16),
    NewConnection(TcpSendHandle),
}

impl From<DataConnectionEvent> for DeviceEvent {
    fn from(event: DataConnectionEvent) -> Self {
        DeviceEvent::DataConnection(event)
    }
}



#[derive(Debug)]
pub enum DataConnectionEventFromDevice {
    Test,
}


pub struct DataConnectionHandle {
    command_connection_id: ConnectionId,
    task_handle: JoinHandle<()>,
    event_sender: SendDownward<DataConnectionEventFromDevice>,
    quit_sender: oneshot::Sender<()>,
}

impl DataConnectionHandle {
    pub fn id(&self) -> ConnectionId {
        self.command_connection_id
    }

    pub async fn quit(self) {
        self.quit_sender.send(()).unwrap();
        self.task_handle.await.unwrap();
    }

    pub async fn send_down(&self, message: DataConnectionEventFromDevice) {
        self.event_sender.send_down(message).await
    }
}

pub struct DataConnection {
    _shutdown_watch: ShutdownWatch,
    command_connection_id: ConnectionId,
    sender: SendUpward<(ConnectionId, DeviceEvent)>,
    receiver: mpsc::Receiver<DataConnectionEventFromDevice>,
    quit_receiver: oneshot::Receiver<()>,
    accept_from: SocketAddr,
    connection: Option<TcpSendConnection>,
}

impl DataConnection {
    pub fn task(
        command_connection_id: ConnectionId,
        sender: SendUpward<(ConnectionId, DeviceEvent)>,
        accept_from: SocketAddr,
        _shutdown_watch: ShutdownWatch,
    ) -> DataConnectionHandle {
        let (event_sender, receiver) = mpsc::channel(EVENT_CHANNEL_SIZE);
        let (quit_sender, quit_receiver) = oneshot::channel();

        let manager = Self {
            _shutdown_watch,
            command_connection_id,
            sender,
            receiver,
            quit_receiver,
            accept_from,
            connection: None,
        };

        let task_handle = tokio::spawn(manager.run());

        DataConnectionHandle {
            command_connection_id,
            task_handle,
            event_sender: event_sender.into(),
            quit_sender,
        }
    }


    pub async fn run(mut self) {
        let audio_out = match TcpListener::bind("127.0.0.1:8082").await {
            Ok(listener) => listener,
            Err(e) => {
                let e = DataConnectionEvent::TcpListenerBindError(e);
                self.sender.send_up((self.command_connection_id, e.into())).await;
                self.quit_receiver.await.unwrap();
                return;
            }
        };

        match audio_out.local_addr() {
            Ok(address) => {
                let event = DataConnectionEvent::PortNumber(address.port());
                self.sender.send_up((self.command_connection_id, event.into())).await;
            }
            Err(e) => {
                let e = DataConnectionEvent::GetPortNumberError(e);
                self.sender.send_up((self.command_connection_id, e.into())).await;
                self.quit_receiver.await.unwrap();
                return;
            }
        }

        loop {
            tokio::select! {
                result = &mut self.quit_receiver => break result.unwrap(),
                event = self.receiver.recv() => {
                    match event.unwrap() {
                        DataConnectionEventFromDevice::Test => (),
                    }
                }
                result = audio_out.accept() => {
                    match result {
                        Ok((connection, address)) => {
                            if address.ip() != self.accept_from.ip() {
                                continue;
                            }

                            let event = match connection.into_std().map(TcpSendConnection::new) {
                                Ok(Ok((connection, handle))) => {
                                    self.connection = Some(connection);
                                    DataConnectionEvent::NewConnection(handle)
                                }
                                Ok(Err(e)) | Err(e) => DataConnectionEvent::SendConnectionError(e),
                            };

                            self.sender.send_up((self.command_connection_id, event.into())).await;
                        }
                        Err(e) => {
                            let e = DataConnectionEvent::AcceptError(e);
                            self.sender.send_up((self.command_connection_id, e.into())).await;
                        }
                    }
                }
            }
        }

        if let Some(connection) = self.connection.take() {
            connection.wait_quit().await;
        }
    }
}
