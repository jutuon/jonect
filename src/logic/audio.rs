use std::{
    any::Any,
    sync::mpsc::{self},
    thread::JoinHandle,
};

use glib::{MainContext, MainLoop, Sender};

use pulse::{
    callbacks::ListResult,
    context::{introspect::SinkInfo, Context, FlagSet, State},
    operation::Operation,
    proplist::Proplist,
};
use pulse_glib::Mainloop;

use super::server::ServerEvent;

pub struct AudioThread {
    audio_thread: Option<JoinHandle<()>>,
}

impl AudioThread {
    pub fn run(sender: mpsc::Sender<ServerEvent>) -> Self {
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
}

#[derive(Debug)]
pub enum AudioServerEvent {
    Message(String),
    RequestQuit,
    PAEvent(PAEvent),
    PAQuitReady,
}

#[derive(Debug)]
pub enum PAEvent {
    ContextStateChanged,
    SinkInfo(String, u32),
    OperationCompleted,
}

pub struct PAState {
    main_loop: Mainloop,
    context: Context,
    sender: AudioServerEventSender,
    current_operation: Option<Box<dyn Any>>,
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

        Self {
            main_loop,
            context,
            sender,
            current_operation: None,
        }
    }

    fn list_pa_monitors(&mut self) {
        let mut s = self.sender.clone();
        let operation = self.context.introspect().get_sink_info_list(move |list| {
            match list {
                ListResult::Item(SinkInfo {
                    name: Some(name),
                    index,
                    ..
                }) => {
                    s.send_pa(PAEvent::SinkInfo(name.to_string(), *index));
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
                self.list_pa_monitors();
            }
            State::Failed => {
                eprintln!("PAContext state: Failed");
                // TODO: Send error.
            }
            State::Terminated => {
                eprintln!("PAContext state: Terminated");
                self.sender.send(AudioServerEvent::PAQuitReady);
            }
            _ => (),
        }
    }

    fn handle_pa_event(&mut self, event: PAEvent) {
        match event {
            PAEvent::ContextStateChanged => {
                self.handle_pa_context_state_change();
            }
            PAEvent::SinkInfo(name, id) => {
                println!("name: {}, id: {}", name, id);
            }
            PAEvent::OperationCompleted => {
                self.current_operation.take();
            }
        }
    }

    fn request_quit(&mut self) {
        self.context.disconnect();
    }
}

pub struct AudioServer {
    context: MainContext,
    server_event_sender: mpsc::Sender<ServerEvent>,
}

impl AudioServer {
    fn new(server_event_sender: mpsc::Sender<ServerEvent>) -> Self {
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
        self.server_event_sender.send(init_event).unwrap();

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
            .send(ServerEvent::AudioServerClosed)
            .unwrap();
    }
}
