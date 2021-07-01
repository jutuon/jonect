pub mod device;

use std::{sync::Arc, thread, time::Duration};

use self::device::{DeviceManager, DeviceManagerError, DeviceManagerEvent, DeviceManagerEventSender, DmTcpSupportDisabled};

use super::{
    audio::{AudioServerEventSender, AudioThread},
    Event,
};

use crate::{
    config::Config,
    logic::audio::AudioServerEvent,
    ui::gtk_ui::{LogicEventSender, SEND_ERROR},
};

use tokio::{sync::{Notify, mpsc::{self, UnboundedReceiver}}, task::JoinHandle};

use tokio::runtime::Runtime;

use tokio_stream::{
    StreamExt,
    wrappers::UnboundedReceiverStream,
};


#[derive(Debug)]
pub enum ServerEvent {
    RequestQuit,
    SendMessage,
    QuitProgressCheck,
    AudioServerStateChange,
    DMStateChange,
    DMEvent(DMEvent),
    AudioEvent(AudioEvent),
}

#[derive(Debug)]
pub enum AudioEvent {
    AudioServerInit(AudioServerEventSender),
    AudioServerClosed,
}

#[derive(Debug)]
pub enum DMEvent {
    /// Device manager will be closed after this event.
    DeviceManagerError(DeviceManagerError),
    TcpSupportDisabledBecauseOfError(DmTcpSupportDisabled),
    DMClosed,
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

#[derive(Debug, Clone)]
pub struct AudioEventSender {
    sender: mpsc::UnboundedSender<ServerEvent>,
}

impl AudioEventSender {
    pub fn new(sender: mpsc::UnboundedSender<ServerEvent>) -> Self {
        Self {
            sender,
        }
    }

    pub fn send(&mut self, event: AudioEvent) {
        self.sender.send(ServerEvent::AudioEvent(event)).unwrap();
    }
}

#[derive(Debug, Clone)]
pub struct DMEventSender {
    sender: mpsc::UnboundedSender<ServerEvent>,
}

impl DMEventSender {
    pub fn new(sender: mpsc::UnboundedSender<ServerEvent>) -> Self {
        Self {
            sender,
        }
    }

    pub fn send(&mut self, event: DMEvent) {
        self.sender.send(ServerEvent::DMEvent(event)).unwrap();
    }
}


pub struct ServerStatus {
    audio_server_is_running: bool,
    device_manager_is_running: bool,
}

impl ServerStatus {
    /// Creates new ServerStatus with status running.
    pub fn new() -> Self {
        Self {
            audio_server_is_running: true,
            device_manager_is_running: true
        }
    }

    pub fn set_audio_server_status_to_closed(&mut self) {
        self.audio_server_is_running = false;
    }

    pub fn set_device_manager_status_to_closed(&mut self) {
        self.device_manager_is_running = false;
    }

    pub fn all_server_components_are_closed(&self) -> bool {
        !self.audio_server_is_running && !self.device_manager_is_running
    }
}

/// Close component message.
enum CloseComponent {
    Yes,
    No,
}

struct AudioServerStateWaitingEventSender {
    thread: AudioThread,
}

impl AudioServerStateWaitingEventSender {
    fn new(thread: AudioThread) -> Self {
        Self {
            thread,
        }
    }

    fn handle_event(self, event: AudioEvent) -> AudioServerStateRunning {
        if let AudioEvent::AudioServerInit(sender) = event {
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
    sender: AudioServerEventSender,
    thread: AudioThread,
}

impl AudioServerStateRunning {
    fn handle_event(mut self, event: AudioEvent) -> ComponentEventHandlingResult<AudioServerStateRunning, ()> {
        match event {
            AudioEvent::AudioServerInit(_) => {
                panic!("Error: multiple AudioServerInit events detected");
            }
            AudioEvent::AudioServerClosed => {
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

    fn handle_event(mut self, audio_event: AudioEvent, server_sender: &mut ServerEventSender) -> Self {
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
                server_sender.send(ServerEvent::AudioServerStateChange);
            }
            Self::Running(server_state) => {
                match server_state.handle_event(audio_event) {
                    ComponentEventHandlingResult::Running(server_state) => {
                        self = AudioServerState::Running(server_state);
                    }
                    ComponentEventHandlingResult::FatalError(e) => {
                        eprintln!("Audio server fatal error: {:?}", e);
                        self = AudioServerState::Closed;
                        server_sender.send(ServerEvent::AudioServerStateChange);

                    }
                    ComponentEventHandlingResult::NormalQuit => {
                        self = AudioServerState::Closed;
                        server_sender.send(ServerEvent::AudioServerStateChange);
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


enum DMState {
    Running { handle: JoinHandle<()>, dm_sender: DeviceManagerEventSender },
    Closed,
}

impl DMState {
    fn new(dm: DeviceManager, dm_sender: DeviceManagerEventSender) -> Self {
        DMState::Running {
            handle: tokio::spawn(dm.run()),
            dm_sender,
        }
    }

    async fn handle_dm_event(
        &mut self,
        e: DMEvent,
        server_sender: &mut ServerEventSender,
    ) {
        match self {
            Self::Running { handle, .. } => {
                match e {
                    DMEvent::DeviceManagerError(e) => {
                        eprintln!("DeviceManagerError {:?}", e);
                        *self = Self::Closed;
                        server_sender.send(ServerEvent::DMStateChange);
                    }
                    DMEvent::TcpSupportDisabledBecauseOfError(e) => {
                        eprintln!("TcpSupportDisabledBecauseOfError {:?}", e);
                    }
                    DMEvent::DMClosed => {
                        handle.await.unwrap();
                        *self = Self::Closed;
                        server_sender.send(ServerEvent::DMStateChange);
                    }
                }
            }
            Self::Closed => (),
        }
    }

    fn closed(&self) -> bool {
        if let Self::Closed = self {
            true
        } else {
            false
        }
    }

    fn send_event_if_running(&mut self, event: DeviceManagerEvent) {
        if let Self::Running { dm_sender, ..} = self {
            dm_sender.send(event);
        }
    }

    fn request_quit(&mut self) {
        self.send_event_if_running(DeviceManagerEvent::RequestQuit);
    }
}

pub struct AsyncServer {
    sender: LogicEventSender,
    receiver: UnboundedReceiverStream<ServerEvent>,
    server_event_sender: ServerEventSender,
    config: Config,
}

impl AsyncServer {
    pub fn new(
        sender: LogicEventSender,
        receiver: UnboundedReceiverStream<ServerEvent>,
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
        let a_event_sender = Server::create_audio_event_channel(self.server_event_sender.clone());

        let audio_thread = AudioThread::run(a_event_sender);
        let state = AudioServerStateWaitingEventSender::new(audio_thread);
        let mut audio_server_state = AudioServerState::new(state);

        let (dm_sender, dm_receiver) = DeviceManager::create_device_event_channel();
        let dm_server_sender = Server::create_dm_event_channel(self.server_event_sender.clone());
        let dm = DeviceManager::new(dm_server_sender, dm_receiver, dm_sender.clone());
        let mut dm_state = DMState::new(dm, dm_sender);

        let mut quit_requested = false;

        loop {
            let event = self.receiver
                .next()
                .await
                .expect("Logic bug: self.receiver channel closed.");

            match event {
                ServerEvent::AudioEvent(e) => {
                    audio_server_state = audio_server_state.handle_event(e, &mut self.server_event_sender);
                }
                ServerEvent::DMEvent(e) => {
                    dm_state.handle_dm_event(e, &mut self.server_event_sender).await;
                }
                ServerEvent::DMStateChange => {
                    if dm_state.closed() && quit_requested {
                        self.server_event_sender.send(ServerEvent::QuitProgressCheck);
                    }
                }
                ServerEvent::AudioServerStateChange => {
                    match audio_server_state {
                        AudioServerState::WaitingEventSender {..} => (),
                        AudioServerState::Running(ref mut state) => {
                            if let Some(source_name) = self.config.pa_source_name.clone() {
                                state.send_event(AudioServerEvent::StartRecording { source_name })
                            }

                            self.sender.send(Event::InitEnd);
                        }
                        AudioServerState::Closed => {
                            if quit_requested {
                                self.server_event_sender.send(ServerEvent::QuitProgressCheck);
                            }
                        }
                    }
                }
                ServerEvent::RequestQuit => {
                    quit_requested = true;
                    audio_server_state.request_quit();
                    dm_state.request_quit();
                    // Send quit progress check event to handle case when all
                    // components are already closed.
                    self.server_event_sender.send(ServerEvent::QuitProgressCheck);
                }
                ServerEvent::QuitProgressCheck => {
                    if audio_server_state.closed() && dm_state.closed() {
                        break;
                    }
                }
                ServerEvent::SendMessage => {
                    self.sender.send(Event::Message("Test message".to_string()));
                }
            }
        }

        // All server components are now closed.
        self.sender.send(Event::CloseProgram);
    }
}

pub struct Server;

impl Server {
    pub fn run(
        mut sender: LogicEventSender,
        receiver: UnboundedReceiverStream<ServerEvent>,
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

    pub fn create_server_event_channel() -> (ServerEventSender, UnboundedReceiverStream<ServerEvent>) {
        let (sender, receiver) = mpsc::unbounded_channel();

        (ServerEventSender::new(sender), UnboundedReceiverStream::new(receiver))
    }

    pub fn create_audio_event_channel(server_event_sender: ServerEventSender) -> AudioEventSender {
        AudioEventSender::new(server_event_sender.sender)
    }

    pub fn create_dm_event_channel(server_event_sender: ServerEventSender) -> DMEventSender {
        DMEventSender::new(server_event_sender.sender)
    }
}
