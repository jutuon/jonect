use std::{thread, time::Duration};

use super::Event;

use crate::ui::gtk_ui::{LogicEventSender, SEND_ERROR};

use std::sync::mpsc::{self, Receiver, Sender};

pub enum ServerEvent {
    RequestQuit,
    SendMessage,
}

pub struct Server {
    sender: LogicEventSender,
    receiver: Receiver<ServerEvent>,
}

impl Server {
    pub fn new(sender: LogicEventSender, receiver: Receiver<ServerEvent>) -> Self {
        Self { sender, receiver }
    }

    pub fn run(&mut self) {
        loop {
            let event = self
                .receiver
                .recv()
                .expect("Error: ServerEvent channel broken, no senders");

            match event {
                ServerEvent::RequestQuit => {
                    self.sender.send(Event::CloseProgram);
                    return;
                }
                ServerEvent::SendMessage => {
                    self.sender.send(Event::Message("Test message".to_string()));
                }
            }
        }
    }
}
