pub mod device;
pub mod audio;
pub mod ui;
pub mod utils;

use self::device::{DeviceManager, FromDeviceManagerToServerEvent, DeviceManagerEvent};

use {
    audio::{AudioThread, FromAudioServerToServerEvent, AudioServerEvent, EventToAudioServerSender},
};

use crate::{config::Config, server::{device::{DeviceManagerEventSender, DeviceManagerTask}, ui::{FromUiToServerEvent, InternalFromServerToUiEvent, UiConnectionManager}}};

use tokio::{sync::{mpsc}};

use tokio::runtime::Runtime;



pub const EVENT_CHANNEL_SIZE: usize = 32;





// #[derive(Debug, Clone)]
// pub struct ServerEventSender {
//     sender: mpsc::Sender<FromUiToServerEvent>,
// }

// impl ServerEventSender {
//     pub fn new(sender: mpsc::Sender<FromUiToServerEvent>) -> Self {
//         Self {
//             sender,
//         }
//     }

//     pub fn blocking_send(&mut self, event: FromUiToServerEvent) {
//         self.sender.blocking_send(event).unwrap();
//     }
// }




// #[derive(Debug, Clone)]
// pub struct DMEventSender {
//     sender: mpsc::UnboundedSender<ServerEvent>,
// }

// impl DMEventSender {
//     pub fn new(sender: mpsc::UnboundedSender<ServerEvent>) -> Self {
//         Self {
//             sender,
//         }
//     }

//     pub fn send(&mut self, event: DMEvent) {
//         self.sender.send(ServerEvent::DMEvent(event)).unwrap();
//     }
// }


/*



enum DMState {
    Running { handle: JoinHandle<()>, dm_sender: DeviceManagerEventSender },
    Closed,
}

impl DMState {
    fn new(dm: DeviceManager, dm_sender: DeviceManagerEventSender) -> Self {
        DMState::Running {
            handle: tokio::spawn(dm.run()),
            dm_sender,
        }
    }

    async fn handle_dm_event(
        &mut self,
        e: EventFromDeviceManager,
        server_sender: &mut ServerEventSender,
    ) {
        match self {
            Self::Running { handle, .. } => {
                match e {
                    EventFromDeviceManager::DeviceManagerError(e) => {
                        eprintln!("DeviceManagerError {:?}", e);
                        *self = Self::Closed;
                        server_sender.send(FromUiToServerEvent::DMStateChange);
                    }
                    EventFromDeviceManager::TcpSupportDisabledBecauseOfError(e) => {
                        eprintln!("TcpSupportDisabledBecauseOfError {:?}", e);
                    }
                    EventFromDeviceManager::DMClosed => {
                        handle.await.unwrap();
                        *self = Self::Closed;
                        server_sender.send(FromUiToServerEvent::DMStateChange);
                    }
                }
            }
            Self::Closed => (),
        }
    }

    fn closed(&self) -> bool {
        if let Self::Closed = self {
            true
        } else {
            false
        }
    }

    fn send_event_if_running(&mut self, event: DeviceManagerEvent) {
        if let Self::Running { dm_sender, ..} = self {
            dm_sender.send(event);
        }
    }

    fn request_quit(&mut self) {
        self.send_event_if_running(DeviceManagerEvent::RequestQuit);
    }
}

*/

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
        let (dm_task_handle, mut dm_sender, mut dm_reveiver) = DeviceManagerTask::new();
        let (ui_task_handle, mut ui_sender, mut server_receiver) = UiConnectionManager::task();

        // Drop initial instance of the shutdown watch to make the receiver
        // to notice the shutdown.
        drop(shutdown_watch);

        async fn send_shutdown_request(
            at_sender: &mut EventToAudioServerSender,
            dm_sender: &mut DeviceManagerEventSender,
            ui_sender: &mut mpsc::Sender<InternalFromServerToUiEvent>,
        ) {
            at_sender.send(AudioServerEvent::RequestQuit);
            dm_sender.send(DeviceManagerEvent::RequestQuit).await;
            ui_sender.send(InternalFromServerToUiEvent::RequestQuit);
        }



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
                        FromDeviceManagerToServerEvent::DeviceManagerError(error) => {
                            eprintln!("Device manager error {:?}", error);
                            send_shutdown_request(&mut at_sender, &mut dm_sender, &mut ui_sender).await;
                            break;
                        }
                        FromDeviceManagerToServerEvent::TcpSupportDisabledBecauseOfError(error) => {
                            eprintln!("TCP support disabled {:?}", error);
                        }
                    }
                }

                Some(ui_event) = server_receiver.recv() => {
                    match ui_event {
                        FromUiToServerEvent::DisconnectRequest |
                        FromUiToServerEvent::DisconnectRequestResponse  => {
                            // The task handles these events.
                        }
                        FromUiToServerEvent::NotificationTest => {

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
