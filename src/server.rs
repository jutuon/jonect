pub mod device;
pub mod audio;
pub mod ui;
pub mod utils;

use self::device::{FromDeviceManagerToServerEvent, DeviceManagerEvent};

use {
    audio::{AudioThread, FromAudioServerToServerEvent, AudioServerEvent, EventToAudioServerSender},
};

use crate::{config::Config, server::{device::{DeviceManagerTask, SendDownward, SendUpward}, ui::{UiProtocolServerMessage, ConnectionManagerMessage, UiConnectionManager}}};

use tokio::{sync::{mpsc}, signal};

use tokio::runtime::Runtime;



pub const EVENT_CHANNEL_SIZE: usize = 32;



/// Drop this type after component is closed.
pub type ShutdownWatch = mpsc::Sender<()>;

pub struct AsyncServer {
    //sender: FromServerToUiSender,
    //ui_event_receiver: mpsc::Receiver<FromUiToServerEvent>,
    config: Config,
}

impl AsyncServer {
    pub fn new(
        //sender: FromServerToUiSender,
        //ui_event_receiver: mpsc::Receiver<FromUiToServerEvent>,
        config: Config,
    ) -> Self {
        Self {
            //sender,
            //ui_event_receiver,
            config,
        }
    }

    pub async fn run(&mut self) {

        let (shutdown_watch, mut shutdown_watch_receiver) = mpsc::channel(1);

        let (mut at, mut at_sender) = AudioThread::start(shutdown_watch.clone()).await;
        let (dm_task_handle, mut dm_sender, mut dm_reveiver) = DeviceManagerTask::task(shutdown_watch.clone());
        let (ui_task_handle, mut ui_sender, mut server_receiver) = UiConnectionManager::task(shutdown_watch.clone());

        // Drop initial instance of the shutdown watch to make the receiver
        // to notice the shutdown.
        drop(shutdown_watch);

        async fn send_shutdown_request(
            at_sender: &mut EventToAudioServerSender,
            dm_sender: &mut SendDownward<DeviceManagerEvent>,
            ui_sender: &mut mpsc::Sender<ConnectionManagerMessage>,
        ) {
            at_sender.send(AudioServerEvent::RequestQuit);
            dm_sender.send_down(DeviceManagerEvent::RequestQuit).await;
            ui_sender.send(ConnectionManagerMessage::RequestQuit).await.unwrap();
        }

        let mut ctrl_c_listener_enabled = true;

        loop {

            tokio::select! {
                Some(at_event) = at.next_event() => {
                    match at_event {
                        event @ FromAudioServerToServerEvent::Init(_) => {
                            panic!("Event {:?} is not handled correctly!", event);
                        }
                    }
                }
                Some(dm_event) = dm_reveiver.recv() => {
                    match dm_event {
                        FromDeviceManagerToServerEvent::TcpSupportDisabledBecauseOfError(error) => {
                            eprintln!("TCP support disabled {:?}", error);
                        }
                    }
                }
                Some(ui_event) = server_receiver.recv() => {
                    match ui_event {
                        UiProtocolServerMessage::NotificationTest => {
                            println!("UI notification");
                        }
                    }
                }
                quit_request = signal::ctrl_c(), if ctrl_c_listener_enabled => {
                    match quit_request {
                        Ok(()) => {
                            send_shutdown_request(&mut at_sender, &mut dm_sender, &mut ui_sender).await;
                            break;
                        }
                        Err(e) => {
                            ctrl_c_listener_enabled = false;
                            eprintln!("Failed to listen CTRL+C. Error: {}", e);
                        }
                    }

                }
            }
        }

        // Quit started. Wait all components to close.

        let _ = shutdown_watch_receiver.recv().await;
        at.join();
        dm_task_handle.await.unwrap();
        ui_task_handle.await.unwrap();


        //let state = AudioServerStateWaitingEventSender::new(audio_thread);
        //let mut audio_server_state = AudioServerState::new(state);

/*



        let (dm_sender, dm_receiver) = mpsc::channel(EVENT_CHANNEL_SIZE);
        let dm = DeviceManager::new(dm_receiver, dm_sender.clone());
        let mut dm_state = DMState::new(dm, dm_sender);

        let mut quit_requested = false;

        loop {
            let event = self.receiver
                .next()
                .await
                .expect("Logic bug: self.receiver channel closed.");

            match event {
                FromUiToServerEvent::AudioEvent(e) => {
                    audio_server_state = audio_server_state.handle_event(e, &mut self.server_event_sender);
                }
                FromUiToServerEvent::DMEvent(e) => {
                    dm_state.handle_dm_event(e, &mut self.server_event_sender).await;
                }
                FromUiToServerEvent::DMStateChange => {
                    if dm_state.closed() && quit_requested {
                        self.server_event_sender.send(FromUiToServerEvent::QuitProgressCheck);
                    }
                }
                FromUiToServerEvent::AudioServerStateChange => {
                    match audio_server_state {
                        AudioServerState::WaitingEventSender {..} => (),
                        AudioServerState::Running(ref mut state) => {
                            if let Some(source_name) = self.config.pa_source_name.clone() {
                                state.send_event(AudioServerEvent::StartRecording { source_name })
                            }

                            self.sender.send(Event::InitEnd);
                        }
                        AudioServerState::Closed => {
                            if quit_requested {
                                self.server_event_sender.send(FromUiToServerEvent::QuitProgressCheck);
                            }
                        }
                    }
                }
                FromUiToServerEvent::RequestQuit => {
                    quit_requested = true;
                    audio_server_state.request_quit();
                    dm_state.request_quit();
                    // Send quit progress check event to handle case when all
                    // components are already closed.
                    self.server_event_sender.send(FromUiToServerEvent::QuitProgressCheck);
                }
                FromUiToServerEvent::QuitProgressCheck => {
                    if audio_server_state.closed() && dm_state.closed() {
                        break;
                    }
                }
                FromUiToServerEvent::SendMessage => {
                    self.sender.send(Event::Message("Test message".to_string()));
                }
            }
        }

        */
    }


}

pub struct Server;

impl Server {
    pub fn run(
        // mut sender: FromServerToUiSender,
        // receiver: mpsc::Receiver<FromUiToServerEvent>,
        // server_event_sender: ServerEventSender,
        config: Config,
    ) {
        // sender.send(Event::InitStart);

        let rt = match Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                //sender.send(Event::InitError);
                eprintln!("{}", e);
                return;
            }
        };

        let mut server = AsyncServer::new(config);

        rt.block_on(server.run());
    }

    // pub fn create_server_event_channel() -> (ServerEventSender, mpsc::Receiver<FromUiToServerEvent>) {
    //     let (sender, receiver) = mpsc::channel(EVENT_CHANNEL_SIZE);

    //     (ServerEventSender::new(sender), receiver)
    // }

    // pub fn create_dm_event_channel(server_event_sender: ServerEventSender) -> DMEventSender {
    //     DMEventSender::new(server_event_sender.sender)
    // }
}
