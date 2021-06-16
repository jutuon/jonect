use std::{
    sync::mpsc::{self},
    thread::{JoinHandle, Thread},
};

use glib::{MainContext, MainLoop, Sender};

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

#[derive(Debug)]
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
}
#[derive(Debug)]
pub enum AudioServerEvent {
    Message(String),
    RequestQuit,
}

pub struct AudioServer {
    context: MainContext,
    main_loop: Mainloop,
    server_event_sender: mpsc::Sender<ServerEvent>,
}

impl AudioServer {
    fn new(server_event_sender: mpsc::Sender<ServerEvent>) -> Self {
        let mut context = MainContext::new();
        if !context.acquire() {
            panic!("context.acquire() failed");
        }

        let main_loop = pulse_glib::Mainloop::new(Some(&mut context)).unwrap();

        let server = Self {
            context,
            main_loop,
            server_event_sender,
        };

        server
    }

    fn run(&mut self) {
        let (sender, receiver) = MainContext::channel::<AudioServerEvent>(glib::PRIORITY_DEFAULT);

        // Send init event.
        let init_event = ServerEvent::AudioServerInit(AudioServerEventSender::new(sender));
        self.server_event_sender.send(init_event).unwrap();

        let glib_main_loop = MainLoop::new(Some(&self.context), false);

        let glib_main_loop_clone = glib_main_loop.clone();
        receiver.attach(Some(&self.context), move |event| {
            match event {
                AudioServerEvent::RequestQuit => {
                    glib_main_loop_clone.quit();
                    return glib::Continue(false);
                }
                AudioServerEvent::Message(_) => (),
            }

            glib::Continue(true)
        });

        glib_main_loop.run();

        self.server_event_sender
            .send(ServerEvent::AudioServerClosed)
            .unwrap();
    }
}
