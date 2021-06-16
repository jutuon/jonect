use std::{thread, time::Duration};

use super::{
    audio::{AudioServerEventSender, AudioThread},
    Event,
};

use crate::{
    logic::audio::AudioServerEvent,
    ui::gtk_ui::{LogicEventSender, SEND_ERROR},
};

use std::sync::mpsc::{self, Receiver, Sender};

#[derive(Debug)]
pub enum ServerEvent {
    RequestQuit,
    SendMessage,
    AudioServerInit(AudioServerEventSender),
    AudioServerClosed,
    QuitProgressCheck,
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

pub struct Server {
    sender: LogicEventSender,
    receiver: Receiver<ServerEvent>,
    server_event_sender: Sender<ServerEvent>,
}

impl Server {
    pub fn new(
        sender: LogicEventSender,
        receiver: Receiver<ServerEvent>,
        server_event_sender: Sender<ServerEvent>,
    ) -> Self {
        Self {
            sender,
            receiver,
            server_event_sender,
        }
    }

    pub fn run(&mut self) {
        let mut audio_thread = AudioThread::run(self.server_event_sender.clone());

        self.sender.send(Event::InitStart);

        let mut audio_event_sender = loop {
            let event = self
                .receiver
                .recv()
                .expect("Error: ServerEvent channel broken, no senders");

            if let ServerEvent::AudioServerInit(e) = event {
                break e;
            }
        };

        self.sender.send(Event::InitEnd);

        let mut thread_status = ServerStatus::new();

        loop {
            let event = self
                .receiver
                .recv()
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
                        .send(ServerEvent::QuitProgressCheck)
                        .unwrap();
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
