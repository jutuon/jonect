mod server;

use glib::Sender;
use std::thread::{JoinHandle, Thread};

use crate::{config::Config, settings::SettingsManager, ui::gtk_ui::LogicEventSender};

use self::server::Server;

#[derive(Debug)]
pub enum Event {
    Message(String),
}

pub struct Logic {
    config: Config,
    settings: SettingsManager,
    logic_thread: JoinHandle<()>,
}

impl Logic {
    pub fn new(config: Config, settings: SettingsManager, sender: LogicEventSender) -> Self {
        let logic_thread = std::thread::spawn(move || {
            Server::new(sender).run();
        });

        Self {
            config,
            settings,
            logic_thread,
        }
    }
}
