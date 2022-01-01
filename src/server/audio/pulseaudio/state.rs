/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */


use std::{any::Any, collections::VecDeque};

use gtk::glib::{MainContext};

use pulse::{
    callbacks::ListResult,
    context::{introspect::SinkInfo, Context, FlagSet, State},
    proplist::Proplist,
};
use pulse_glib::Mainloop;

use super::{AudioEvent, EventToAudioServerSender, stream::{PARecordingStreamEvent, PAStreamManager}};

use crate::{server::{audio::pulseaudio::AudioServerEvent, device::data::TcpSendHandle, }};


#[derive(Debug)]
pub enum PAEvent {
    ContextStateChanged,
    SinkInfo {
        description: String,
        monitor_source: String,
    },
    OperationCompleted,
    StreamManagerQuitReady,
    RecordingStreamEvent(PARecordingStreamEvent),
}

pub struct PAState {
    // Make sure that Mainloop is not dropped when audio code is running.
    // This is probably not required, but it adds some additional
    // safety as some other objects use reference to this.
    _main_loop: Mainloop,
    context: Context,
    context_ready: bool,
    sender: EventToAudioServerSender,
    current_operation: Option<Box<dyn Any>>,
    stream_manager: PAStreamManager,
    wait_context_event_queue: VecDeque<AudioServerEvent>,
}

impl PAState {
    pub fn new(glib_context: &mut MainContext, sender: EventToAudioServerSender) -> Self {
        let main_loop = pulse_glib::Mainloop::new(Some(glib_context)).unwrap();

        let proplist = Proplist::new().unwrap();

        let mut context = Context::new_with_proplist(&main_loop, "Jonect", &proplist).unwrap();

        let mut s = sender.clone();
        context.set_state_callback(Some(Box::new(move || {
            s.send_pa(PAEvent::ContextStateChanged);
        })));

        context.connect(None, FlagSet::NOFLAGS, None).unwrap();

        let stream_manager = PAStreamManager::new(sender.clone());

        Self {
            _main_loop: main_loop,
            context,
            context_ready: false,
            sender,
            current_operation: None,
            stream_manager,
            wait_context_event_queue: VecDeque::new(),
        }
    }

    fn list_pa_monitors(&mut self) {
        // TODO: Check that Context is ready?
        let mut s = self.sender.clone();
        let operation = self.context.introspect().get_sink_info_list(move |list| {
            match list {
                ListResult::Item(SinkInfo {
                    description: Some(description),
                    monitor_source_name: Some(monitor_name),
                    ..
                }) => {
                    s.send_pa(PAEvent::SinkInfo {
                        description: description.to_string(),
                        monitor_source: monitor_name.to_string(),
                    });
                }
                ListResult::Item(_) => (),
                ListResult::End => {
                    s.send_pa(PAEvent::OperationCompleted);
                }
                ListResult::Error => {
                    // TODO: Send error
                }
            }
        });

        self.current_operation = Some(Box::new(operation) as Box<dyn Any>);
    }

    fn handle_pa_context_state_change(&mut self) {
        let state = self.context.get_state();
        match state {
            State::Ready => {
                self.context_ready = true;
                while let Some(event) = self.wait_context_event_queue.pop_front() {
                    self.sender.send(event);
                }
                self.list_pa_monitors();
            }
            State::Failed => {
                self.context_ready = false;
                eprintln!("PAContext state: Failed");
                // TODO: Send error.
            }
            State::Terminated => {
                eprintln!("PAContext state: Terminated");
                self.sender.send(AudioServerEvent::PAQuitReady);
                self.context_ready = false;
            }
            State::Authorizing | State::Connecting | State::Unconnected => {
                self.context_ready = false
            }
            State::SettingName => (),
        }
    }

    pub fn handle_pa_event(&mut self, event: PAEvent) {
        match event {
            PAEvent::ContextStateChanged => {
                self.handle_pa_context_state_change();
            }
            PAEvent::SinkInfo {
                description,
                monitor_source,
            } => {
                println!(
                    "description: {}, monitor_source: {}",
                    description, monitor_source
                );
            }
            PAEvent::OperationCompleted => {
                self.current_operation.take();
            }
            PAEvent::RecordingStreamEvent(event) => {
                self.stream_manager.handle_recording_stream_event(event);
            }
            PAEvent::StreamManagerQuitReady => {
                self.context.disconnect();
            }
        }
    }

    pub fn start_recording(
        &mut self,
        source_name: Option<String>,
        send_handle: TcpSendHandle,
        encode_opus: bool,
        sample_rate: u32,
    ) {
        if self.context_ready {
            self.stream_manager.request_start_record_stream(
                &mut self.context,
                source_name,
                send_handle,
                encode_opus,
                sample_rate,
            );
        } else {
            self.wait_context_event_queue
                .push_back(AudioServerEvent::AudioEvent(AudioEvent::StartRecording { send_handle, sample_rate }));
        }
    }

    pub fn stop_recording(&mut self) {
        self.stream_manager.stop_recording();
    }

    pub fn request_quit(&mut self) {
        self.stream_manager.request_quit();
    }
}
