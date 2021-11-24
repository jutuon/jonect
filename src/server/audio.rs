/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::{
    thread::JoinHandle,
};

use self::audio_server::{AudioServer};

use crate::config::Config;
use crate::{config::EVENT_CHANNEL_SIZE};

use tokio::sync::{mpsc, oneshot};

mod audio_server;


/*
struct AudioServerStateWaitingEventSender {
    thread: AudioThread,
}

impl AudioServerStateWaitingEventSender {
    fn new(thread: AudioThread) -> Self {
        Self {
            thread,
        }
    }

    fn handle_event(self, event: EventFromAudioServer) -> AudioServerStateRunning {
        if let EventFromAudioServer::Init(sender) = event {
            AudioServerStateRunning { sender, thread: self.thread }
        } else {
            panic!("Error: First event from AudioServer must be AudioServerInit.");
        }
    }
}

enum ComponentEventHandlingResult<C, E> {
    Running(C),
    NormalQuit,
    FatalError(E),
}

struct AudioServerStateRunning {
    sender: EventToAudioServerSender,
    thread: AudioThread,
}

impl AudioServerStateRunning {
    fn handle_event(mut self, event: EventFromAudioServer) -> ComponentEventHandlingResult<AudioServerStateRunning, ()> {
        match event {
            EventFromAudioServer::Init(_) => {
                panic!("Error: multiple AudioServerInit events detected");
            }
            EventFromAudioServer::AudioServerClosed => {
                self.thread.join();
                ComponentEventHandlingResult::NormalQuit
            }
        }
    }

    fn send_event(&mut self, event: AudioServerEvent) {
        self.sender.send(event);
    }
}


enum AudioServerState {
    WaitingEventSender {
        server_state: AudioServerStateWaitingEventSender,
        quit_requested: bool,
    },
    Running(AudioServerStateRunning),
    Closed,
}


impl AudioServerState {
    fn new(server_state: AudioServerStateWaitingEventSender) -> Self {
        Self::WaitingEventSender { server_state, quit_requested: false }
    }

    fn handle_event(mut self, audio_event: EventFromAudioServer, server_sender: &mut ServerEventSender) -> Self {
        match self {
            Self::WaitingEventSender {
                server_state,
                quit_requested,
            } => {
                let mut new_state = server_state.handle_event(audio_event);
                if quit_requested {
                    new_state.send_event(AudioServerEvent::RequestQuit);
                }
                self = AudioServerState::Running(new_state);
                server_sender.send(FromUiToServerEvent::AudioServerStateChange);
            }
            Self::Running(server_state) => {
                match server_state.handle_event(audio_event) {
                    ComponentEventHandlingResult::Running(server_state) => {
                        self = AudioServerState::Running(server_state);
                    }
                    ComponentEventHandlingResult::FatalError(e) => {
                        eprintln!("Audio server fatal error: {:?}", e);
                        self = AudioServerState::Closed;
                        server_sender.send(FromUiToServerEvent::AudioServerStateChange);

                    }
                    ComponentEventHandlingResult::NormalQuit => {
                        self = AudioServerState::Closed;
                        server_sender.send(FromUiToServerEvent::AudioServerStateChange);
                    }
                }
            }
            Self::Closed => {
                panic!("Error: Audio event received even if audio server is closed.");
            }
        }

        self
    }

    fn running(&self) -> bool {
        if let Self::Running(_) = self {
            true
        } else {
            false
        }
    }

    fn closed(&self) -> bool {
        if let Self::Closed = self {
            true
        } else {
            false
        }
    }

    fn send_event_if_running(&mut self, event: AudioServerEvent) {
        if let Self::Running(state) = self {
            state.send_event(event);
        }
    }

    fn request_quit(&mut self) {
        match self {
            Self::WaitingEventSender { quit_requested, ..} => {
                *quit_requested = true;
            }
            Self::Running(server_state) => {
                server_state.send_event(AudioServerEvent::RequestQuit);
            }
            Self::Closed => (),
        }
    }
}

 */

pub use audio_server::{EventToAudioServerSender};
pub use audio_server::AudioServerEvent;

use super::message_router::RouterSender;

use std::sync::Arc;

pub struct AudioThread {
    audio_thread: Option<JoinHandle<()>>,
}

impl AudioThread {
    pub async fn start(r_sender: RouterSender, config: Arc<Config>) -> Self  {
        let (init_ok_sender, mut init_ok_receiver) = mpsc::channel(1);

        let audio_thread = Some(std::thread::spawn(move || {
            AudioServer::new(r_sender, init_ok_sender, config).run();
        }));

        init_ok_receiver.recv().await.unwrap();

        let thread = Self {
            audio_thread,
        };

        thread
    }

    pub fn join(&mut self) {
        // TODO: Handle thread panics?
        self.audio_thread.take().unwrap().join().unwrap();
    }
}
