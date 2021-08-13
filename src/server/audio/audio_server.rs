//! This code runs in a different thread.


use std::{
    any::Any,
    collections::VecDeque,
    time::{Duration, Instant},
};

use gtk::glib::{MainContext, MainLoop, Sender};

use pulse::{callbacks::ListResult, context::{introspect::SinkInfo, Context, FlagSet, State}, proplist::Proplist, sample::Spec, stream::Stream};
use pulse_glib::Mainloop;

use super::{AudioEventSender};


#[derive(Debug)]
pub enum FromAudioServerToServerEvent {
    // The first event from AudioServer.
    Init(EventToAudioServerSender),
    //AudioServerClosed,
}

#[derive(Debug)]
pub enum AudioServerEvent {
    Message(String),
    RequestQuit,
    StartRecording { source_name: String },
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
}

#[derive(Debug)]
pub enum PAPlaybackStreamEvent {
    DataOverflow,
    DataUnderflow,
    StateChange,
}

pub struct PAStreamManager {
    record: Option<Stream>,
    sender: EventToAudioServerSender,
    bytes_per_second: u64,
    data_count_time: Option<Instant>,
}

impl PAStreamManager {
    fn new(sender: EventToAudioServerSender) -> Self {
        Self {
            record: None,
            sender,
            bytes_per_second: 0,
            data_count_time: None,
        }
    }

    fn request_start_record_stream(&mut self, context: &mut Context, source_name: String) {
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
            .connect_record(Some(&source_name), None, pulse::stream::FlagSet::NOFLAGS)
            .unwrap();

        let mut s = self.sender.clone();
        stream.set_state_callback(Some(Box::new(move || {
            s.send_pa_record_stream_event(PARecordingStreamEvent::StateChange);
        })));

        let mut s = self.sender.clone();
        stream.set_read_callback(Some(Box::new(move |size| {
            s.send_pa_record_stream_event(PARecordingStreamEvent::Read(size));
        })));

        self.record = Some(stream);
    }

    fn handle_recording_stream_state_change(&mut self) {
        use pulse::stream::State;

        let state = self.record.as_mut().unwrap().get_state();

        match state {
            State::Failed => {
                eprintln!("Recording stream state: Failed.");
            }
            State::Terminated => {
                self.record = None;
                self.data_count_time = None;
                self.sender.send_pa(PAEvent::StreamManagerQuitReady);
                eprintln!("Recording stream state: Terminated.");
            }
            _ => (),
        }
    }

    fn handle_recording_stream_read(&mut self, read_size: usize) {
        let r = self.record.as_mut().unwrap();

        use pulse::stream::PeekResult;

        loop {
            let peek_result = r.peek().unwrap();
            match peek_result {
                PeekResult::Empty => {
                    break;
                }
                PeekResult::Data(data) => {
                    self.bytes_per_second += data.len() as u64;

                    match self.data_count_time {
                        Some(data_count_time) => {
                            let now = Instant::now();
                            if now.duration_since(data_count_time) >= Duration::from_secs(1) {
                                let speed = (self.bytes_per_second as f64) / 1024.0 / 1024.0;
                                println!("Recording stream data speed: {} MiB/s", speed);
                                self.bytes_per_second = 0;
                                self.data_count_time = Some(Instant::now());
                            }
                        }
                        None => {
                            self.data_count_time = Some(Instant::now());
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
        }
    }

    fn request_quit(&mut self) {
        if let Some(stream) = self.record.as_mut() {
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

    fn start_recording(&mut self, source_name: String) {
        if self.context_ready {
            self.stream_manager
                .request_start_record_stream(&mut self.context, source_name);
        } else {
            self.wait_context_event_queue
                .push_back(AudioServerEvent::StartRecording { source_name });
        }
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
                AudioServerEvent::StartRecording { source_name } => {
                    pa_state.start_recording(source_name);
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
