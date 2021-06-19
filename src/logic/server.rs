use std::{thread, time::Duration};

use super::{
    audio::{AudioServerEventSender, AudioThread},
    Event,
};

use crate::{
    config::Config,
    logic::audio::AudioServerEvent,
    ui::gtk_ui::{LogicEventSender, SEND_ERROR},
};

use tokio::sync::mpsc;

use tokio::runtime::Runtime;

#[derive(Debug)]
pub enum ServerEvent {
    RequestQuit,
    SendMessage,
    AudioServerInit(AudioServerEventSender),
    AudioServerClosed,
    QuitProgressCheck,
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

pub struct ServerStatus {
    audio_server: bool,
}

impl ServerStatus {
    /// Creates new ServerStatus with status running.
    pub fn new() -> Self {
        Self { audio_server: true }
    }

    pub fn set_audio_server_status_to_closed(&mut self) {
        self.audio_server = false;
    }

    pub fn all_threads_are_closed(&self) -> bool {
        !self.audio_server
    }
}

pub struct AsyncServer {
    sender: LogicEventSender,
    receiver: mpsc::UnboundedReceiver<ServerEvent>,
    server_event_sender: ServerEventSender,
    config: Config,
}

impl AsyncServer {
    pub fn new(
        sender: LogicEventSender,
        receiver: mpsc::UnboundedReceiver<ServerEvent>,
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
        let mut audio_thread = AudioThread::run(self.server_event_sender.clone());

        let mut audio_event_sender = loop {
            let event = self
                .receiver.recv().await
                .expect("Error: ServerEvent channel broken, no senders");

            if let ServerEvent::AudioServerInit(e) = event {
                break e;
            }
        };

        self.sender.send(Event::InitEnd);

        let mut thread_status = ServerStatus::new();

        if let Some(source_name) = self.config.pa_source_name.clone() {
            audio_event_sender.send(AudioServerEvent::StartRecording { source_name });
        }

        loop {
            let event = self
                .receiver.recv().await
                .expect("Error: ServerEvent channel broken, no senders");

            //println!("{:?}", event);

            match event {
                ServerEvent::RequestQuit => {
                    audio_event_sender.send(AudioServerEvent::RequestQuit);
                }
                ServerEvent::AudioServerClosed => {
                    audio_thread.join();
                    thread_status.set_audio_server_status_to_closed();
                    self.server_event_sender
                        .send(ServerEvent::QuitProgressCheck);
                }
                ServerEvent::QuitProgressCheck => {
                    if thread_status.all_threads_are_closed() {
                        self.sender.send(Event::CloseProgram);
                        return;
                    }
                }
                ServerEvent::SendMessage => {
                    self.sender.send(Event::Message("Test message".to_string()));
                }
                ServerEvent::AudioServerInit(_) => {
                    panic!("Error: multiple AudioServerInit events detected")
                }
            }
        }
    }
}

pub struct Server;

impl Server {
    pub fn run(
        mut sender: LogicEventSender,
        receiver: mpsc::UnboundedReceiver<ServerEvent>,
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

    pub fn create_server_event_channel() -> (ServerEventSender, mpsc::UnboundedReceiver<ServerEvent>) {
        let (sender, receiver) = mpsc::unbounded_channel();

        (ServerEventSender::new(sender), receiver)
    }
}
