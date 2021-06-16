mod audio;
mod server;

use std::{
    sync::mpsc::{self, Sender},
    thread::{JoinHandle, Thread},
};

use crate::{config::Config, settings::SettingsManager, ui::gtk_ui::LogicEventSender};

use self::server::{Server, ServerEvent};

#[derive(Debug)]
pub enum Event {
    InitStart,
    InitEnd,
    Message(String),
    CloseProgram,
}

pub struct Logic {
    config: Config,
    settings: SettingsManager,
    logic_thread: Option<JoinHandle<()>>,
    server_event_sender: Sender<ServerEvent>,
}

impl Logic {
    pub fn new(config: Config, settings: SettingsManager, sender: LogicEventSender) -> Self {
        let (server_event_sender, receiver) = mpsc::channel();

        let s = server_event_sender.clone();
        let logic_thread = Some(std::thread::spawn(move || {
            Server::new(sender, receiver, s).run();
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
            .send(ServerEvent::SendMessage)
            .unwrap();
    }

    pub fn request_quit(&mut self) {
        self.server_event_sender
            .send(ServerEvent::RequestQuit)
            .unwrap();
    }

    pub fn join_logic_thread(&mut self) {
        self.logic_thread.take().unwrap().join().unwrap();
    }
}
