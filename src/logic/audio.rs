use std::{
    any::Any,
    collections::VecDeque,
    thread::JoinHandle,
    time::{Duration, Instant},
};

use glib::{MainContext, MainLoop, Sender};

use pulse::{
    callbacks::ListResult,
    context::{introspect::SinkInfo, Context, FlagSet, State},
    operation::Operation,
    proplist::Proplist,
    sample::Spec,
    stream::Stream,
};
use pulse_glib::Mainloop;

use super::server::{ServerEvent, ServerEventSender};

pub struct AudioThread {
    audio_thread: Option<JoinHandle<()>>,
}

impl AudioThread {
    pub fn run(sender: ServerEventSender) -> Self {
        let audio_thread = Some(std::thread::spawn(move || {
            AudioServer::new(sender).run();
        }));

        Self { audio_thread }
    }

    pub fn join(&mut self) {
        // TODO: Handle thread panics?
        self.audio_thread.take().unwrap().join().unwrap();
    }
}

#[derive(Debug, Clone)]
pub struct AudioServerEventSender {
    sender: Sender<AudioServerEvent>,
}

impl AudioServerEventSender {
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
    sender: AudioServerEventSender,
    bytes_per_second: u64,
    data_count_time: Option<Instant>,
}

impl PAStreamManager {
    fn new(sender: AudioServerEventSender) -> Self {
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
    sender: AudioServerEventSender,
    current_operation: Option<Box<dyn Any>>,
    stream_manager: PAStreamManager,
    wait_context_event_queue: VecDeque<AudioServerEvent>,
}

impl PAState {
    fn new(glib_context: &mut MainContext, sender: AudioServerEventSender) -> Self {
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

pub struct AudioServer {
    context: MainContext,
    server_event_sender: ServerEventSender,
}

impl AudioServer {
    fn new(server_event_sender: ServerEventSender) -> Self {
        let mut context = MainContext::new();
        if !context.acquire() {
            panic!("context.acquire() failed");
        }

        let server = Self {
            context,
            server_event_sender,
        };

        server
    }

    fn run(&mut self) {
        let (sender, receiver) = MainContext::channel::<AudioServerEvent>(glib::PRIORITY_DEFAULT);
        let sender = AudioServerEventSender::new(sender);

        // Send init event.
        let init_event = ServerEvent::AudioServerInit(sender.clone());
        self.server_event_sender.send(init_event);

        // Init PulseAudio context.
        let mut pa_state = PAState::new(&mut self.context, sender);

        // Init glib mainloop.
        let glib_main_loop = MainLoop::new(Some(&self.context), false);

        let glib_main_loop_clone = glib_main_loop.clone();
        receiver.attach(Some(&self.context), move |event| {
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
                    return glib::Continue(false);
                }
            }

            glib::Continue(true)
        });

        glib_main_loop.run();

        self.server_event_sender
            .send(ServerEvent::AudioServerClosed);
    }
}
