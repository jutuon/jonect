/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

mod pulseaudio;

use std::{sync::Arc, thread::JoinHandle};

use tokio::sync::oneshot;

use self::pulseaudio::AudioServer;
use super::message_router::MessageReceiver;
use super::message_router::RouterSender;
use crate::config::Config;
use crate::utils::QuitReceiver;
use crate::utils::QuitSender;

pub use pulseaudio::AudioServerEvent;
pub use pulseaudio::EventToAudioServerSender;

pub struct AudioThread {
    audio_thread: Option<JoinHandle<()>>,
    sender: EventToAudioServerSender,
}

impl AudioThread {
    pub async fn start(r_sender: RouterSender, config: Arc<Config>) -> Self {
        let (init_ok_sender, init_ok_receiver) = oneshot::channel();

        let audio_thread = Some(std::thread::spawn(move || {
            AudioServer::new(r_sender, config).run(init_ok_sender);
        }));

        let sender = init_ok_receiver.await.unwrap();

        Self { audio_thread, sender }
    }

    pub fn quit(&mut self) {
        self.send_event(AudioServerEvent::RequestQuit);

        // TODO: Handle thread panics?
        self.audio_thread.take().unwrap().join().unwrap();
    }

    pub fn send_event(&mut self, a_event: AudioServerEvent) {
        self.sender.send(a_event)
    }
}


pub struct AudioManager {
    r_sender: RouterSender,
    quit_receiver: QuitReceiver,
    audio_receiver: MessageReceiver<AudioServerEvent>,
    config: Arc<Config>,
}

impl AudioManager {
    pub fn task(
        r_sender: RouterSender,
        audio_receiver: MessageReceiver<AudioServerEvent>,
        config: Arc<Config>,
    ) -> (tokio::task::JoinHandle<()>, QuitSender) {
        let (quit_sender, quit_receiver) = oneshot::channel();

        let audio_manager = Self {
            r_sender,
            audio_receiver,
            quit_receiver,
            config,
        };

        let task = async move {
            audio_manager.run().await;
        };

        let handle = tokio::spawn(task);

        (handle, quit_sender)
    }

    async fn run(mut self) {
        let mut at = AudioThread::start(self.r_sender, self.config).await;

        loop {
            tokio::select! {
                result = &mut self.quit_receiver => break result.unwrap(),
                event = self.audio_receiver.recv() => {
                    at.send_event(event)
                }
            }
        }

        at.quit()
    }
}
