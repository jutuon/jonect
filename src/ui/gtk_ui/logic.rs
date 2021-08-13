
use std::{
    thread::{JoinHandle},
};

use crate::{config::Config, settings::SettingsManager, ui::gtk_ui::FromServerToUiSender};

use self::server::{Server, FromUiToServerEvent, ServerEventSender};


pub struct Logic {
    config: Config,
    settings: SettingsManager,
    logic_thread: Option<JoinHandle<()>>,
    server_event_sender: ServerEventSender,
}

impl Logic {
    pub fn new(config: Config, settings: SettingsManager, sender: FromServerToUiSender) -> Self {
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
            .blocking_send(FromUiToServerEvent::SendMessage);
    }

    pub fn request_quit(&mut self) {
        self.server_event_sender
            .blocking_send(FromUiToServerEvent::RequestQuit);
    }

    pub fn join_logic_thread(&mut self) {
        self.logic_thread.take().unwrap().join().unwrap();
    }
}
