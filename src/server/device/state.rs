/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::{net::SocketAddr, time::Instant};

use crate::utils::{ConnectionHandle, ConnectionId, SendDownward, SendUpward};

use super::{data::{DataConnection, DataConnectionEvent, DataConnectionHandle}, protocol::{AudioFormat, AudioStreamInfo, ClientMessage, ServerInfo, ServerMessage}};



#[derive(Debug)]
pub enum DeviceEvent {
    Test,
    DataConnection(DataConnectionEvent),
}

pub struct DeviceState {
    address: SocketAddr,
    play_stream: bool,
    connection_handle: ConnectionHandle<ServerMessage>,
    sender: SendUpward<(ConnectionId, DeviceEvent)>,
    ping_state: Option<Instant>,
    audio_out: Option<DataConnectionHandle>,
}


impl DeviceState {
    pub async fn new(
        connection_handle: ConnectionHandle<ServerMessage>,
        sender: SendUpward<(ConnectionId, DeviceEvent)>,
        address: SocketAddr,
    ) -> Self {
        let message = ServerMessage::ServerInfo(ServerInfo::new("Test server"));
        connection_handle.send_down(message).await;

        Self {
            play_stream: false,
            connection_handle,
            sender,
            ping_state: None,
            address,
            audio_out: None,
        }
    }

    pub async fn handle_client_message(&mut self, message: ClientMessage) {
        match message {
            ClientMessage::ClientInfo(info) => {
                println!("ClientInfo {:?}", info);

                let handle = DataConnection::task(
                    self.connection_handle.id(),
                    self.sender.clone(),
                    self.address,
                );

                self.audio_out = Some(handle);
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

    pub async fn handle_data_connection_message(&mut self, message: DataConnectionEvent) {
        match message {
            DataConnectionEvent::NewConnection(_) => {
                panic!("DataConnectionEvent::NewConnection should be handled before this method.");
            }
            DataConnectionEvent::PortNumber(tcp_port) => {
                let info = AudioStreamInfo::new(AudioFormat::Pcm, 2u8, 44100u32, tcp_port);

                self.connection_handle.send_down(ServerMessage::PlayAudioStream(info)).await;
            }
            e @ DataConnectionEvent::TcpListenerBindError(_) |
            e @ DataConnectionEvent::GetPortNumberError(_) |
            e @ DataConnectionEvent::AcceptError(_) |
            e @ DataConnectionEvent::SendConnectionError(_) => {
                eprintln!("Error: {:?}", e);
            }
        }
    }

    pub async fn quit(mut self) {
        if let Some(audio) = self.audio_out.take() {
            audio.quit().await;
        }
        self.connection_handle.quit().await;
    }
}
