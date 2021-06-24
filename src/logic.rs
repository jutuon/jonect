mod audio;
mod server;
pub mod client;

use std::{
    sync::mpsc::{self, Sender},
    thread::{JoinHandle, Thread},
};

use crate::{config::Config, settings::SettingsManager, ui::gtk_ui::LogicEventSender};

use self::server::{Server, ServerEvent, ServerEventSender};

#[derive(Debug)]
pub enum Event {
    InitStart,
    InitError,
    InitEnd,
    Message(String),
    CloseProgram,
}

pub struct Logic {
    config: Config,
    settings: SettingsManager,
    logic_thread: Option<JoinHandle<()>>,
    server_event_sender: ServerEventSender,
}

impl Logic {
    pub fn new(config: Config, settings: SettingsManager, sender: LogicEventSender) -> Self {
        let (server_event_sender, receiver) = Server::create_server_event_channel();

        let s = server_event_sender.clone();
        let c = config.clone();
        let logic_thread = Some(std::thread::spawn(move || {
            Server::run(sender, receiver, s, c);
        }));

        Self {
            config,
            settings,
            logic_thread,
            server_event_sender,
        }
    }

    pub fn send_message(&mut self) {
        self.server_event_sender
            .send(ServerEvent::SendMessage);
    }

    pub fn request_quit(&mut self) {
        self.server_event_sender
            .send(ServerEvent::RequestQuit);
    }

    pub fn join_logic_thread(&mut self) {
        self.logic_thread.take().unwrap().join().unwrap();
    }
}
