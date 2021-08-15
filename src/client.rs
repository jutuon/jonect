//! Test client

use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpStream, runtime::Runtime, sync::mpsc, signal};

use crate::{config::{EVENT_CHANNEL_SIZE, TestClientConfig}, server::device::protocol::ServerMessage, utils::{Connection, ConnectionEvent}};

use crate::server::device::{protocol::{ClientInfo, ClientMessage}};

use std::convert::TryInto;

pub struct TestClient {
    config: TestClientConfig,
}



impl TestClient {
    pub fn new(config: TestClientConfig) -> Self {
        TestClient {
            config
        }
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
        Self {
            config,
        }
    }

    pub async fn run(self) {
        let (shutdown_watch, mut shutdown_watch_receiver) = mpsc::channel(1);

        let stream = TcpStream::connect(self.config.address).await.unwrap();
        let (read_half, write_half) = stream.into_split();

        let (sender, mut connections_receiver) =
            mpsc::channel::<ConnectionEvent<ServerMessage>>(EVENT_CHANNEL_SIZE);

        let connection_handle = Connection::spawn_connection_task(
            0,
            read_half,
            write_half,
            sender.into(),
            shutdown_watch
        );

        let message = ClientMessage::ClientInfo(ClientInfo::new("test"));
        connection_handle.send_down(message).await;

        let mut ctrl_c_listener_enabled = true;

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

        connection_handle.quit().await;
        let _ = shutdown_watch_receiver.recv().await;
    }
}
