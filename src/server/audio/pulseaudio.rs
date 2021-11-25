/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! PulseAudio audio code

mod stream;
mod state;

use std::{thread::JoinHandle, sync::Arc};

use gtk::glib::{MainContext, MainLoop, Sender};

use tokio::sync::{oneshot};

use self::{state::{PAEvent, PAState}, stream::PARecordingStreamEvent};

use super::AudioEvent;

use crate::{
    config::Config,
    server::{
        message_router::{RouterSender},
    },
};

#[derive(Debug)]
pub enum AudioServerEvent {
    AudioEvent(AudioEvent),
    RequestQuit,
    PAEvent(PAEvent),
    PAQuitReady,
}


/// AudioServer which handles connection to the PulseAudio.
///
/// Create a new thread for running an AudioServer. The reason for this is that
/// AudioServer will modify glib thread default MainContext.
pub struct AudioServer {
    server_event_sender: RouterSender,
    config: std::sync::Arc<Config>,
}

impl AudioServer {
    pub fn new(
        server_event_sender: RouterSender,
        config: std::sync::Arc<Config>,
    ) -> Self {
        Self {
            server_event_sender,
            config,
        }
    }

    // TODO: Simplify this code to use glib default MainContext. Currently only
    // PulseAudio uses glib MainContext.

    /// Run audio server code. This method will block until the server is closed.
    ///
    /// This function will modify glib thread default MainContext.
    pub fn run(mut self, init_ok_sender: oneshot::Sender<EventToAudioServerSender>) {
        // Create context for this thread
        let mut context = MainContext::new();
        context.push_thread_default();

        let (sender, receiver) =
            MainContext::channel::<AudioServerEvent>(gtk::glib::PRIORITY_DEFAULT);
        let sender = EventToAudioServerSender::new(sender);

        // Send init event.
        init_ok_sender.send(sender.clone()).unwrap();

        // Init PulseAudio context.
        let mut pa_state = PAState::new(&mut context, sender);

        // Init glib mainloop.
        let glib_main_loop = MainLoop::new(Some(&context), false);

        let glib_main_loop_clone = glib_main_loop.clone();
        receiver.attach(Some(&context), move |event| {
            match event {
                AudioServerEvent::RequestQuit => {
                    pa_state.request_quit();
                }
                AudioServerEvent::AudioEvent(event) => {
                    match event {
                        AudioEvent::StartRecording { send_handle } => {
                            pa_state.start_recording(self.config.pa_source_name.clone(), send_handle);
                        }
                        AudioEvent::StopRecording => {
                            pa_state.stop_recording();
                        }
                        AudioEvent::Message(_) => (),
                    }
                }
                AudioServerEvent::PAEvent(event) => {
                    pa_state.handle_pa_event(event);
                }
                AudioServerEvent::PAQuitReady => {
                    glib_main_loop_clone.quit();
                    return gtk::glib::Continue(false);
                }
            }

            gtk::glib::Continue(true)
        });

        glib_main_loop.run();
    }
}

#[derive(Debug, Clone)]
pub struct EventToAudioServerSender {
    sender: Sender<AudioServerEvent>,
}

impl EventToAudioServerSender {
    fn new(sender: Sender<AudioServerEvent>) -> Self {
        Self { sender }
    }

    pub fn send(&mut self, event: AudioServerEvent) {
        self.sender.send(event).unwrap();
    }

    pub fn send_pa(&mut self, event: PAEvent) {
        self.send(AudioServerEvent::PAEvent(event));
    }

    pub fn send_pa_record_stream_event(&mut self, event: PARecordingStreamEvent) {
        self.send_pa(PAEvent::RecordingStreamEvent(event));
    }
}


pub struct PulseAudioThread {
    audio_thread: Option<JoinHandle<()>>,
    sender: EventToAudioServerSender,
}

impl PulseAudioThread {
    pub async fn start(r_sender: RouterSender, config: Arc<Config>) -> Self {
        let (init_ok_sender, init_ok_receiver) = oneshot::channel();

        let audio_thread = Some(std::thread::spawn(move || {
            AudioServer::new(r_sender, config).run(init_ok_sender);
        }));

        let sender = init_ok_receiver.await.unwrap();

        Self { audio_thread, sender }
    }

    pub fn quit(&mut self) {
        self.sender.send(AudioServerEvent::RequestQuit);

        // TODO: Handle thread panics?
        self.audio_thread.take().unwrap().join().unwrap();
    }

    pub fn send_event(&mut self, a_event: AudioEvent) {
        self.sender.send(AudioServerEvent::AudioEvent(a_event))
    }
}