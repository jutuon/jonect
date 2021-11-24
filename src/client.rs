/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Test client

use tokio::{
    io::AsyncReadExt,
    net::TcpStream,
    runtime::Runtime,
    signal,
    sync::{mpsc, oneshot},
};

use crate::{
    config::{TestClientConfig, EVENT_CHANNEL_SIZE},
    server::device::protocol::ServerMessage,
    utils::{Connection, ConnectionEvent},
};

use crate::server::device::protocol::{ClientInfo, ClientMessage};

use std::{
    net::{SocketAddr, ToSocketAddrs},
    time::{Duration, Instant},
};

pub struct TestClient {
    config: TestClientConfig,
}

impl TestClient {
    pub fn new(config: TestClientConfig) -> Self {
        TestClient { config }
    }

    pub fn run(self) {
        let rt = Runtime::new().unwrap();

        let server = AsyncClient::new(self.config);

        rt.block_on(server.run());
    }
}

struct AsyncClient {
    config: TestClientConfig,
}

impl AsyncClient {
    pub fn new(config: TestClientConfig) -> Self {
        Self { config }
    }

    pub async fn run(self) {
        let address = self
            .config
            .address
            .to_socket_addrs()
            .unwrap()
            .next()
            .unwrap();

        let stream = TcpStream::connect(address).await.unwrap();
        let (read_half, write_half) = stream.into_split();

        let (sender, mut connections_receiver) =
            mpsc::channel::<ConnectionEvent<ServerMessage>>(EVENT_CHANNEL_SIZE);

        let connection_handle =
            Connection::spawn_connection_task(0, read_half, write_half, sender.into());

        let message = ClientMessage::ClientInfo(ClientInfo::new("test"));
        connection_handle.send_down(message).await;

        let mut ctrl_c_listener_enabled = true;

        let mut data_stream = None;

        loop {
            tokio::select! {
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
                        ConnectionEvent::Message(id, message) => {
                            println!("Connection id {} message {:?}", id, message);
                            match message {
                                ServerMessage::Ping => {
                                    connection_handle.send_down(ClientMessage::PingResponse).await;
                                },
                                ServerMessage::PlayAudioStream(info) => {
                                    let data_address = SocketAddr::new(address.ip(), info.port);
                                    let connection = TcpStream::connect(data_address).await.unwrap();

                                    let (quit_sender, quit_receiver) = oneshot::channel();
                                    let task = Self::handle_data_connection(
                                        quit_receiver,
                                        connection,
                                    );
                                    let handle = tokio::spawn(task);

                                    data_stream = Some((handle, quit_sender));
                                }
                                _ => (),
                            }
                        }
                    }
                }
                quit_request = signal::ctrl_c(), if ctrl_c_listener_enabled => {
                    match quit_request {
                        Ok(()) => {
                            break;
                        }
                        Err(e) => {
                            ctrl_c_listener_enabled = false;
                            eprintln!("Failed to listen CTRL+C. Error: {}", e);
                        }
                    }

                }
            }
        }

        // Quit started. Wait all components to close.

        if let Some((handle, quit_sender)) = data_stream.take() {
            quit_sender.send(()).unwrap();
            handle.await.unwrap();
        }

        connection_handle.quit().await;
    }

    async fn handle_data_connection(
        mut quit_receiver: oneshot::Receiver<()>,
        mut connection: TcpStream,
    ) {
        let mut buffer = [0; 1024];
        let mut bytes_per_second: u64 = 0;
        let mut data_count_time: Option<Instant> = None;

        loop {
            tokio::select! {
                result = &mut quit_receiver => return result.unwrap(),
                result = connection.read(&mut buffer) => {
                    match result {
                        Ok(0) => break,
                        Ok(size) => {
                            bytes_per_second += size as u64;

                            match data_count_time {
                                Some(time) => {
                                    let now = Instant::now();
                                    if now.duration_since(time) >= Duration::from_secs(1) {
                                        let speed = (bytes_per_second as f64) / 1024.0 / 1024.0;
                                        println!("Recording stream data speed: {} MiB/s", speed);
                                        bytes_per_second = 0;
                                        data_count_time = Some(Instant::now());
                                    }
                                }
                                None => {
                                    data_count_time = Some(Instant::now());
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Data connection error: {}", e);
                        }
                    }
                }
            }
        }

        quit_receiver.await.unwrap();
    }
}
