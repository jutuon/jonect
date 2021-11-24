/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

mod audio_server;

use std::{
    thread::JoinHandle,
    sync::Arc,
};

use tokio::sync::{mpsc};

use self::audio_server::{AudioServer};
use crate::config::Config;
use super::message_router::RouterSender;

pub use audio_server::{EventToAudioServerSender};
pub use audio_server::AudioServerEvent;

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

        Self {
            audio_thread,
        }
    }

    pub fn join(&mut self) {
        // TODO: Handle thread panics?
        self.audio_thread.take().unwrap().join().unwrap();
    }
}
