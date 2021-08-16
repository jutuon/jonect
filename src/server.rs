pub mod device;
pub mod audio;
pub mod ui;

use self::device::{FromDeviceManagerToServerEvent, DeviceManagerEvent};

use {
    audio::{AudioThread, FromAudioServerToServerEvent, AudioServerEvent, EventToAudioServerSender},
};

use crate::{config::Config, server::{device::{DeviceManagerTask}, ui::{UiConnectionManager, UiProtocolFromUiToServer}}, utils::{QuitSender, SendDownward, SendUpward}};

use tokio::{sync::{mpsc}, signal};

use tokio::runtime::Runtime;

pub struct AsyncServer {
    config: Config,
}

impl AsyncServer {
    pub fn new(
        config: Config,
    ) -> Self {
        Self {
            config,
        }
    }

    pub async fn run(&mut self) {

        let (shutdown_watch, mut shutdown_watch_receiver) = mpsc::channel(1);

        let (mut at, mut at_sender) = AudioThread::start(shutdown_watch.clone()).await;
        let (dm_task_handle, mut dm_sender, mut dm_reveiver) = DeviceManagerTask::task(shutdown_watch.clone());
        let (ui_task_handle, mut ui_sender, mut server_receiver, ui_quit_sender) = UiConnectionManager::task(shutdown_watch.clone());

        // Drop initial instance of the shutdown watch to make the receiver
        // to notice the shutdown.
        drop(shutdown_watch);

        async fn send_shutdown_request(
            at_sender: &mut EventToAudioServerSender,
            dm_sender: &mut SendDownward<DeviceManagerEvent>,
            ui_sender: QuitSender,
        ) {
            at_sender.send(AudioServerEvent::RequestQuit);
            dm_sender.send_down(DeviceManagerEvent::RequestQuit).await;
            ui_sender.send(()).unwrap();
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
                        UiProtocolFromUiToServer::NotificationTest => {
                            println!("UI notification");
                        }
                    }
                }
                quit_request = signal::ctrl_c(), if ctrl_c_listener_enabled => {
                    match quit_request {
                        Ok(()) => {
                            send_shutdown_request(&mut at_sender, &mut dm_sender, ui_quit_sender).await;
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
    pub fn run(config: Config) {
        let rt = match Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("{}", e);
                return;
            }
        };

        let mut server = AsyncServer::new(config);

        rt.block_on(server.run());
    }
}
