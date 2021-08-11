use std::{
    thread::JoinHandle,
};

use self::audio_server::{AudioServer};

use super::server::{EVENT_CHANNEL_SIZE, FromUiToServerEvent, ServerEventSender, ShutdownWatch};


use tokio::sync::mpsc;

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

pub use audio_server::{FromAudioServerToServerEvent, EventToAudioServerSender};
pub use audio_server::AudioServerEvent;

pub type AudioEventSender = mpsc::Sender<FromAudioServerToServerEvent>;

pub struct AudioThread {
    audio_thread: Option<JoinHandle<()>>,
    receiver: mpsc::Receiver<FromAudioServerToServerEvent>,
    init_ready: bool,
    sender: Option<EventToAudioServerSender>,
}

impl AudioThread {
    pub async fn start(shutdown_watch: ShutdownWatch) -> (Self, EventToAudioServerSender)  {
        let (sender, mut receiver) = mpsc::channel(EVENT_CHANNEL_SIZE);

        let audio_thread = Some(std::thread::spawn(move || {
            AudioServer::new(sender).run();
            drop(shutdown_watch);
        }));

        let ae_sender = if let FromAudioServerToServerEvent::Init(sender) = receiver.recv().await.unwrap() {
            sender
        } else {
            panic!("The first event from AudioServer is not Init.");
        };

        let thread = Self {
            audio_thread,
            receiver,
            init_ready: false,
            sender: None,
        };

        (thread, ae_sender)
    }

    // If None then the server is closed.
    pub async fn next_event(&mut self) -> Option<FromAudioServerToServerEvent> {
        self.receiver.recv().await
    }

    pub fn join(&mut self) {
        // TODO: Handle thread panics?
        self.audio_thread.take().unwrap().join().unwrap();
    }
}
