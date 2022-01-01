/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

mod pulseaudio;

use std::{sync::Arc};

use tokio::sync::oneshot;

use self::pulseaudio::PulseAudioThread;
use super::device::data::TcpSendHandle;
use super::message_router::MessageReceiver;
use super::message_router::RouterSender;
use crate::config::Config;
use crate::utils::QuitReceiver;
use crate::utils::QuitSender;

pub use pulseaudio::EventToAudioServerSender;

#[derive(Debug)]
pub enum AudioEvent {
    Message(String),
    StopRecording,
    StartRecording { send_handle: TcpSendHandle, sample_rate: u32 },
}


pub struct AudioManager {
    r_sender: RouterSender,
    quit_receiver: QuitReceiver,
    audio_receiver: MessageReceiver<AudioEvent>,
    config: Arc<Config>,
}

impl AudioManager {
    pub fn task(
        r_sender: RouterSender,
        audio_receiver: MessageReceiver<AudioEvent>,
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
        let mut at = PulseAudioThread::start(self.r_sender, self.config).await;

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
