use std::time::Instant;

use crate::utils::{ConnectionHandle, ConnectionId, SendDownward, SendUpward};

use super::protocol::{ClientMessage, ServerInfo, ServerMessage};



#[derive(Debug)]
pub enum DeviceEvent {
    Test,
}

pub struct DeviceState {
    play_stream: bool,
    connection_handle: ConnectionHandle<ServerMessage>,
    sender: SendUpward<(ConnectionId, DeviceEvent)>,
    ping_state: Option<Instant>,
}


impl DeviceState {
    pub async fn new(
        connection_handle: ConnectionHandle<ServerMessage>,
        sender: SendUpward<(ConnectionId, DeviceEvent)>,
    ) -> Self {
        let message = ServerMessage::ServerInfo(ServerInfo::new("Test server"));
        connection_handle.send_down(message).await;

        Self {
            play_stream: false,
            connection_handle,
            sender,
            ping_state: None,
        }
    }

    pub async fn handle_client_message(&mut self, message: ClientMessage) {
        match message {
            ClientMessage::ClientInfo(info) => {
                println!("ClientInfo {:?}", info);
            }
            ClientMessage::Ping => {
                self.connection_handle.send_down(ServerMessage::PingResponse).await;
            }
            ClientMessage::PingResponse => {
                if let Some(time) = self.ping_state.take() {
                    let time = Instant::now().duration_since(time).as_millis();
                    println!("Ping time {} ms", time);
                }
            }
            ClientMessage::AudioStreamPlayError(error) => {
                eprintln!("AudioStreamPlayError {:?}", error);
            }
        }
    }

    pub async fn send_ping_message(&mut self) {
        if self.ping_state.is_none() {
            self.connection_handle.send_down(ServerMessage::Ping).await;
            self.ping_state = Some(Instant::now());
        }
    }

    pub async fn quit(self) {
        self.connection_handle.quit().await;
    }
}
