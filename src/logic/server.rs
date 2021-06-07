use std::{thread, time::Duration};

use glib::Sender;

use super::Event;

use crate::ui::gtk_ui::{LogicEventSender, SEND_ERROR};

pub struct Server {
    sender: LogicEventSender,
}

impl Server {
    pub fn new(sender: LogicEventSender) -> Self {
        Self { sender }
    }

    pub fn run(&mut self) {
        loop {
            thread::sleep(Duration::from_secs(1));
            self.sender.send(Event::Message("Test message".to_string()));
        }
    }
}
