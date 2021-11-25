//! Message router for sending messages between components.

use tokio::sync::mpsc;

use crate::{config, utils};

use super::{
    audio::{AudioEvent},
    device::DeviceManagerEvent,
    ui::UiEvent,
};

#[derive(Debug)]
pub enum RouterEvent {
    SendQuitRequest,
}

#[derive(Debug)]
pub enum RouterMessage {
    AudioServer(AudioEvent),
    DeviceManager(DeviceManagerEvent),
    Ui(UiEvent),
    Router(RouterEvent),
}

#[derive(Debug)]
pub struct Router {
    r_receiver: mpsc::Receiver<RouterMessage>,
    audio_sender: mpsc::Sender<AudioEvent>,
    device_manager_sender: mpsc::Sender<DeviceManagerEvent>,
    ui_sender: mpsc::Sender<UiEvent>,
}

impl Router {
    pub fn new() -> (
        Self,
        RouterSender,
        MessageReceiver<DeviceManagerEvent>,
        MessageReceiver<UiEvent>,
        MessageReceiver<AudioEvent>,
    ) {
        let (r_sender, r_receiver) = mpsc::channel(config::EVENT_CHANNEL_SIZE);

        let sender = RouterSender { sender: r_sender };

        let (device_manager_sender, device_manager_receiver) =
            mpsc::channel(config::EVENT_CHANNEL_SIZE);
        let (ui_sender, ui_receiver) = mpsc::channel(config::EVENT_CHANNEL_SIZE);
        let (audio_sender, audio_receiver) = mpsc::channel(config::EVENT_CHANNEL_SIZE);

        let router = Self {
            r_receiver,
            audio_sender,
            device_manager_sender,
            ui_sender,
        };

        (
            router,
            sender,
            MessageReceiver::new(device_manager_receiver),
            MessageReceiver::new(ui_receiver),
            MessageReceiver::new(audio_receiver),
        )
    }

    pub async fn run(mut self, mut quit_receiver: utils::QuitReceiver) {
        loop {
            tokio::select! {
                result = &mut quit_receiver => break result.unwrap(),
                Some(event) = self.r_receiver.recv() => {
                    match event {
                        RouterMessage::AudioServer(event) => {
                            tokio::select! {
                                result = &mut quit_receiver => break result.unwrap(),
                                result = self.audio_sender.send(event) => result.unwrap(),
                            }
                        }
                        RouterMessage::DeviceManager(event) => {
                            tokio::select! {
                                result = &mut quit_receiver => break result.unwrap(),
                                result = self.device_manager_sender.send(event) => result.unwrap(),
                            }
                        }
                        RouterMessage::Ui(event) => {
                            tokio::select! {
                                result = &mut quit_receiver => break result.unwrap(),
                                result = self.ui_sender.send(event) => result.unwrap(),
                            }
                        }
                        RouterMessage::Router(RouterEvent::SendQuitRequest) => {
                            // TODO: Should router send quit message to components?
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct RouterSender {
    sender: mpsc::Sender<RouterMessage>,
}

impl RouterSender {
    pub async fn send_ui_event(&mut self, event: UiEvent) {
        self.sender.send(RouterMessage::Ui(event)).await.unwrap()
    }

    pub async fn send_audio_server_event(&mut self, event: AudioEvent) {
        self.sender
            .send(RouterMessage::AudioServer(event))
            .await
            .unwrap()
    }

    pub async fn send_device_manager_event(&mut self, event: DeviceManagerEvent) {
        self.sender
            .send(RouterMessage::DeviceManager(event))
            .await
            .unwrap()
    }

    pub fn send_router_blocking(&mut self, event: RouterEvent) {
        self.sender
            .blocking_send(RouterMessage::Router(event))
            .unwrap()
    }
}

#[derive(Debug)]
pub struct MessageReceiver<T> {
    receiver: mpsc::Receiver<T>,
}

impl<T> MessageReceiver<T> {
    pub fn new(receiver: mpsc::Receiver<T>) -> Self {
        Self { receiver }
    }

    pub async fn recv(&mut self) -> T {
        self.receiver.recv().await.unwrap()
    }
}
