pub mod device;

use std::{sync::Arc, thread, time::Duration};

use self::device::{DeviceManager, DeviceManagerError, DeviceManagerEventSender};

use super::{
    audio::{AudioServerEventSender, AudioThread},
    Event,
};

use crate::{
    config::Config,
    logic::audio::AudioServerEvent,
    ui::gtk_ui::{LogicEventSender, SEND_ERROR},
};

use tokio::sync::{Notify, mpsc::{self, UnboundedReceiver}};

use tokio::runtime::Runtime;

use tokio_stream::{
    StreamExt,
    wrappers::UnboundedReceiverStream,
};


#[derive(Debug)]
pub enum ServerEvent {
    RequestQuit,
    SendMessage,
    QuitProgressCheck,

}

#[derive(Debug)]
pub enum AudioEvent {
    AudioServerInit(AudioServerEventSender),
}

#[derive(Debug)]
pub enum DMEvent {
    /// Device manager will be closed after this event.
    DeviceManagerError(DeviceManagerError),
}

#[derive(Debug, Clone)]
pub struct ServerEventSender {
    sender: mpsc::UnboundedSender<ServerEvent>,
}

impl ServerEventSender {
    pub fn new(sender: mpsc::UnboundedSender<ServerEvent>) -> Self {
        Self {
            sender,
        }
    }

    pub fn send(&mut self, event: ServerEvent) {
        self.sender.send(event).unwrap();
    }
}

#[derive(Debug, Clone)]
pub struct AudioEventSender {
    sender: mpsc::UnboundedSender<AudioEvent>,
}

impl AudioEventSender {
    pub fn new(sender: mpsc::UnboundedSender<AudioEvent>) -> Self {
        Self {
            sender,
        }
    }

    pub fn send(&mut self, event: AudioEvent) {
        self.sender.send(event).unwrap();
    }
}


pub struct ServerStatus {
    audio_server_is_running: bool,
    device_manager_is_running: bool,
}

impl ServerStatus {
    /// Creates new ServerStatus with status running.
    pub fn new() -> Self {
        Self {
            audio_server_is_running: true,
            device_manager_is_running: true
        }
    }

    pub fn set_audio_server_status_to_closed(&mut self) {
        self.audio_server_is_running = false;
    }

    pub fn set_device_manager_status_to_closed(&mut self) {
        self.device_manager_is_running = false;
    }

    pub fn all_server_components_are_closed(&self) -> bool {
        !self.audio_server_is_running && !self.device_manager_is_running
    }
}

/// Close component message.
enum CloseComponent {
    Yes,
    No,
}

pub struct AsyncServer {
    sender: LogicEventSender,
    receiver: UnboundedReceiverStream<ServerEvent>,
    server_event_sender: ServerEventSender,
    config: Config,
}

impl AsyncServer {
    pub fn new(
        sender: LogicEventSender,
        receiver: UnboundedReceiverStream<ServerEvent>,
        server_event_sender: ServerEventSender,
        config: Config,
    ) -> Self {
        Self {
            sender,
            receiver,
            server_event_sender,
            config,
        }
    }

    pub async fn run(&mut self) {
        let (a_event_sender, mut a_event_receiver) = Server::create_audio_event_channel();

        let mut audio_thread = AudioThread::run(a_event_sender);

        let mut audio_server_event_sender = loop {
            let event = a_event_receiver.next().await
                .expect("Error: ServerEvent channel broken, no senders");

            if let AudioEvent::AudioServerInit(e) = event {
                break e;
            }
        };

        let (mut dm_sender, dm_receiver) = DeviceManager::create_device_event_channel();
        let dm_result = DeviceManager::new().await;
        let device_manager = match dm_result {
            Err(e) => {
                self.sender.send(Event::InitError);
                eprintln!("Device manager error: {:?}", e);
                return;
            }
            Ok(dm) => dm,
        };

        self.sender.send(Event::InitEnd);

        let mut component_status = ServerStatus::new();

        if let Some(source_name) = self.config.pa_source_name.clone() {
            audio_server_event_sender.send(AudioServerEvent::StartRecording { source_name });
        }

        let dm_event_stream = device_manager.event_stream(dm_receiver);
        tokio::pin!(dm_event_stream);

        loop {
            tokio::select! {
                Some(e) = self.receiver.next() => {
                    let close_server = self.handle_server_event(e, &mut audio_server_event_sender, &mut dm_sender, &mut component_status);
                    if let CloseComponent::Yes = close_server {
                        break;
                    }
                }
                e_next = dm_event_stream.next() => {
                    match e_next {
                        Some(e) => {
                            self.handle_dm_event(e, &mut audio_server_event_sender, &mut dm_sender);
                        }
                        None => {
                            // Device manager closed.
                            component_status.set_device_manager_status_to_closed();
                            self.server_event_sender
                                .send(ServerEvent::QuitProgressCheck);
                        }
                    }
                }
                e_next = a_event_receiver.next() => {
                    match e_next {
                        Some(e) => {
                            self.handle_audio_event(e, &mut audio_server_event_sender, &mut dm_sender);
                        }
                        None => {
                            // Audio server closed.
                            component_status.set_audio_server_status_to_closed();
                            self.server_event_sender
                                .send(ServerEvent::QuitProgressCheck);
                        }
                    }
                }
                else => panic!("Logic bug: self.receiver channel closed."),
            };
        }

        // All server components are now closed.
        self.sender.send(Event::CloseProgram);
    }

    fn handle_server_event(
        &mut self,
        e: ServerEvent,
        audio_server_event_sender: &mut AudioServerEventSender,
        dm_sender: &mut DeviceManagerEventSender,
        thread_status: &mut ServerStatus,
    ) -> CloseComponent {
        match e {
            ServerEvent::RequestQuit => {
                audio_server_event_sender.send(AudioServerEvent::RequestQuit);
                dm_sender.send(device::DeviceManagerEvent::RequestQuit);
            }
            ServerEvent::QuitProgressCheck => {
                if thread_status.all_server_components_are_closed() {
                    return CloseComponent::Yes;
                }
            }
            ServerEvent::SendMessage => {
                self.sender.send(Event::Message("Test message".to_string()));
            }
        }

        CloseComponent::No
    }

    fn handle_audio_event(
        &mut self,
        e: AudioEvent,
        audio_server_event_sender: &mut AudioServerEventSender,
        dm_sender: &mut DeviceManagerEventSender,
    ) {
        match e {
            AudioEvent::AudioServerInit(_) => {
                panic!("Error: multiple AudioServerInit events detected")
            }
        }
    }

    fn handle_dm_event(
        &mut self,
        e: DMEvent,
        audio_server_event_sender: &mut AudioServerEventSender,
        dm_sender: &mut DeviceManagerEventSender,
    ) {
        match e {
            DMEvent::DeviceManagerError(e) => {
                eprintln!("DeviceManagerError {:?}", e);
            }
        }
    }
}

pub struct Server;

impl Server {
    pub fn run(
        mut sender: LogicEventSender,
        receiver: UnboundedReceiverStream<ServerEvent>,
        server_event_sender: ServerEventSender,
        config: Config,
    ) {
        sender.send(Event::InitStart);

        let rt = match Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                sender.send(Event::InitError);
                eprintln!("{}", e);
                return;
            }
        };

        let mut server = AsyncServer::new(sender, receiver, server_event_sender, config);

        rt.block_on(server.run());
    }

    pub fn create_server_event_channel() -> (ServerEventSender, UnboundedReceiverStream<ServerEvent>) {
        let (sender, receiver) = mpsc::unbounded_channel();

        (ServerEventSender::new(sender), UnboundedReceiverStream::new(receiver))
    }

    pub fn create_audio_event_channel() -> (AudioEventSender, UnboundedReceiverStream<AudioEvent>) {
        let (sender, receiver) = mpsc::unbounded_channel();

        (AudioEventSender::new(sender), UnboundedReceiverStream::new(receiver))
    }
}
