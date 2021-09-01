/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! This code runs in a different thread.


use std::{
    any::Any,
    collections::VecDeque,
    time::{Duration, Instant},
    io::{Write, ErrorKind},
};

use bytes::{BytesMut, BufMut, Buf};
use gtk::glib::{MainContext, MainLoop, Sender};

use pulse::{callbacks::ListResult, context::{introspect::SinkInfo, Context, FlagSet, State}, proplist::Proplist, sample::Spec, stream::Stream};
use pulse_glib::Mainloop;

use crate::server::device::data::TcpSendHandle;

use super::{AudioEventSender};


#[derive(Debug)]
pub enum FromAudioServerToServerEvent {
    // The first event from AudioServer.
    Init(EventToAudioServerSender),
}

#[derive(Debug)]
pub enum AudioServerEvent {
    Message(String),
    RequestQuit,
    StartRecording { source_name: Option<String>, send_handle: TcpSendHandle },
    StopRecording,
    PAEvent(PAEvent),
    PAQuitReady,
}

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

#[derive(Debug)]
pub enum PARecordingStreamEvent {
    StateChange,
    Read(usize),
    Moved,
}

#[derive(Debug)]
pub enum PAPlaybackStreamEvent {
    DataOverflow,
    DataUnderflow,
    StateChange,
}

pub struct PAStreamManager {
    record: Option<(Stream, TcpSendHandle)>,
    sender: EventToAudioServerSender,
    bytes_per_second: u64,
    data_count_time: Option<Instant>,
    recording_buffer: BytesMut,
    enable_recording: bool,
    quit_requested: bool,
}

impl PAStreamManager {
    fn new(sender: EventToAudioServerSender) -> Self {
        Self {
            record: None,
            sender,
            bytes_per_second: 0,
            data_count_time: None,
            recording_buffer: BytesMut::new(),
            enable_recording: true,
            quit_requested: false,
        }
    }

    fn request_start_record_stream(
        &mut self,
        context: &mut Context,
        source_name: Option<String>,
        send_handle: TcpSendHandle
    ) {
        let spec = Spec {
            format: pulse::sample::Format::S16le,
            channels: 2,
            rate: 44100,
        };

        assert!(spec.is_valid(), "Stream data specification is invalid.");

        let mut stream = Stream::new(
            context,
            "Multidevice recording stream",
            &spec,
            None,
        )
        .expect("Stream creation error");

        stream
            .connect_record(source_name.as_deref(), None, pulse::stream::FlagSet::NOFLAGS)
            .unwrap();

        let mut s = self.sender.clone();
        stream.set_moved_callback(Some(Box::new(move || {
            s.send_pa_record_stream_event(PARecordingStreamEvent::Moved);
        })));

        let mut s = self.sender.clone();
        stream.set_state_callback(Some(Box::new(move || {
            s.send_pa_record_stream_event(PARecordingStreamEvent::StateChange);
        })));

        let mut s = self.sender.clone();
        stream.set_read_callback(Some(Box::new(move |size| {
            s.send_pa_record_stream_event(PARecordingStreamEvent::Read(size));
        })));

        self.recording_buffer.clear();
        self.record = Some((stream, send_handle));
        self.enable_recording = true;
    }

    fn handle_recording_stream_state_change(&mut self) {
        use pulse::stream::State;

        let stream = &self.record.as_ref().unwrap().0;
        let state = stream.get_state();

        match state {
            State::Failed => {
                eprintln!("Recording stream state: Failed.");
            }
            State::Terminated => {
                self.record = None;
                if self.quit_requested {
                    self.sender.send_pa(PAEvent::StreamManagerQuitReady);
                }
                eprintln!("Recording stream state: Terminated.");
            }
            State::Ready => {
                println!("Recording from {:?}", stream.get_device_name());
            }
            _ => (),
        }
    }

    fn handle_data(
        data: &[u8],
        recording_buffer: &mut BytesMut,
        send_handle: &mut TcpSendHandle
    ) -> Result<(), std::io::Error> {
        loop {
            if recording_buffer.has_remaining() {
                match send_handle.write(recording_buffer.chunk()) {
                    Ok(count) => {
                        recording_buffer.advance(count);
                    }
                    Err(e) => {
                        if ErrorKind::WouldBlock == e.kind() {
                            break;
                        } else {
                            return Err(e);
                        }
                    }
                }
            } else {
                break;
            }
        }

        match send_handle.write(data) {
            Ok(count) => {
                if count < data.len() {
                    let remaining_bytes = &data[count..];
                    recording_buffer.extend_from_slice(remaining_bytes);
                    println!("Recording state: buffering {} bytes.", remaining_bytes.len());
                }
            }
            Err(e) => {
                if ErrorKind::WouldBlock == e.kind() {
                    recording_buffer.extend_from_slice(data);
                } else {
                    return Err(e);
                }
            }
        }

        Ok(())
    }

    fn handle_recording_stream_read(&mut self, read_size: usize) {
        let (r, ref mut send_handle) = self.record.as_mut().unwrap();

        if !self.enable_recording {
            return;
        }

        use pulse::stream::PeekResult;

        loop {
            let peek_result = r.peek().unwrap();
            match peek_result {
                PeekResult::Empty => {
                    break;
                }
                PeekResult::Data(data) => {
                    let result = Self::handle_data(
                        data,
                        &mut self.recording_buffer,
                        send_handle,
                    );

                    match result {
                        Ok(()) => (),
                        Err(e) => {
                            eprintln!("Audio write error: {}", e);
                            r.discard().unwrap();
                            self.enable_recording = false;
                            self.stop_recording();
                            return;
                        }
                    }

                }
                PeekResult::Hole(_) => (),
            }

            r.discard().unwrap();
        }
    }

    fn handle_recording_stream_event(&mut self, event: PARecordingStreamEvent) {
        match event {
            PARecordingStreamEvent::StateChange => {
                self.handle_recording_stream_state_change();
            }
            PARecordingStreamEvent::Read(read_size) => {
                self.handle_recording_stream_read(read_size);
            }
            PARecordingStreamEvent::Moved => {
                if let Some(name) = self.record.as_ref().map(|(s, _)| s.get_device_name()) {
                    println!("Recording stream moved. Device name: {:?}", name);
                }
            }
        }
    }

    fn stop_recording(&mut self) {
        if let Some((stream, _)) = self.record.as_mut() {
            stream.disconnect().unwrap();
        }
    }

    fn request_quit(&mut self) {
        self.quit_requested = true;

        if let Some((stream, _)) = self.record.as_mut() {
            stream.disconnect().unwrap();
        } else {
            self.sender.send_pa(PAEvent::StreamManagerQuitReady);
        }
    }
}

pub struct PAState {
    main_loop: Mainloop,
    context: Context,
    context_ready: bool,
    sender: EventToAudioServerSender,
    current_operation: Option<Box<dyn Any>>,
    stream_manager: PAStreamManager,
    wait_context_event_queue: VecDeque<AudioServerEvent>,
}

impl PAState {
    fn new(glib_context: &mut MainContext, sender: EventToAudioServerSender) -> Self {
        let main_loop = pulse_glib::Mainloop::new(Some(glib_context)).unwrap();

        let mut proplist = Proplist::new().unwrap();

        let mut context = Context::new_with_proplist(&main_loop, "Multidevice", &proplist).unwrap();

        let mut s = sender.clone();
        context.set_state_callback(Some(Box::new(move || {
            s.send_pa(PAEvent::ContextStateChanged);
        })));

        context.connect(None, FlagSet::NOFLAGS, None).unwrap();

        let stream_manager = PAStreamManager::new(sender.clone());

        Self {
            main_loop,
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

    fn handle_pa_event(&mut self, event: PAEvent) {
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

    fn start_recording(&mut self, source_name: Option<String>, send_handle: TcpSendHandle) {
        if self.context_ready {
            self.stream_manager
                .request_start_record_stream(&mut self.context, source_name, send_handle);
        } else {
            self.wait_context_event_queue
                .push_back(AudioServerEvent::StartRecording { source_name, send_handle });
        }
    }

    fn stop_recording(&mut self) {
        self.stream_manager.stop_recording();
    }

    fn request_quit(&mut self) {
        self.stream_manager.request_quit();
    }
}

/// AudioServer which handles connection to the PulseAudio.
///
/// Create a new thread for running an AudioServer. The reason for this is that
/// AudioServer will modify glib thread default MainContext.
pub struct AudioServer {
    server_event_sender: AudioEventSender,
}

impl AudioServer {
    pub fn new(server_event_sender: AudioEventSender) -> Self {
        Self {
            server_event_sender,
        }
    }

    /// Run audio server code. This method will block until the server is closed.
    ///
    /// This function will modify glib thread default MainContext.
    pub fn run(&mut self) {

        // Create context for this thread
        let mut context = MainContext::new();
        context.push_thread_default();

        let (sender, receiver) = MainContext::channel::<AudioServerEvent>(gtk::glib::PRIORITY_DEFAULT);
        let sender = EventToAudioServerSender::new(sender);

        // Send init event.
        let init_event = FromAudioServerToServerEvent::Init(sender.clone());
        self.server_event_sender.blocking_send(init_event).unwrap();

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
                AudioServerEvent::StartRecording { source_name, send_handle } => {
                    pa_state.start_recording(source_name, send_handle);
                }
                AudioServerEvent::StopRecording => {
                    pa_state.stop_recording();
                }
                AudioServerEvent::Message(_) => (),
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
