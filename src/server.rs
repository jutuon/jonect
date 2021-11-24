/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

pub mod device;
pub mod audio;
pub mod ui;
pub mod message_router;

use self::device::{FromDeviceManagerToServerEvent, DeviceManagerEvent};

use {
    audio::{AudioThread, AudioServerEvent, EventToAudioServerSender},
};

use crate::{config::Config, server::{device::{DeviceManagerTask}, message_router::{Router, RouterSender}, ui::{UiConnectionManager, UiProtocolFromUiToServer}}, utils::{QuitSender, SendDownward, SendUpward}};

use tokio::{sync::{mpsc}, signal};

use tokio::runtime::Runtime;

pub struct AsyncServer {
    config: std::sync::Arc<Config>,
}

impl AsyncServer {
    pub fn new(
        config: Config,
    ) -> Self {
        Self {
            config: config.into(),
        }
    }

    pub async fn run(&mut self) {

        // Init message routing.

        let (router, mut r_sender, device_manager_receiver, ui_receiver) = Router::new();
        let (r_quit_sender, r_quit_receiver) = tokio::sync::oneshot::channel();

        let router_task_handle = tokio::spawn(router.run(r_quit_receiver));

        // Init other components.

        let mut at = AudioThread::start(r_sender.clone(), self.config.clone()).await;
        let dm_task_handle = DeviceManagerTask::task(r_sender.clone(), device_manager_receiver);
        let (ui_task_handle, ui_quit_sender) = UiConnectionManager::task(r_sender.clone(), ui_receiver);

        async fn send_shutdown_request(
            r_sender: &mut RouterSender,
            ui_sender: QuitSender,
        ) {
            r_sender.send_audio_server_event(AudioServerEvent::RequestQuit).await;
            r_sender.send_device_manager_event(DeviceManagerEvent::RequestQuit).await;
            ui_sender.send(()).unwrap();
        }

        let mut ctrl_c_listener_enabled = true;

        loop {
            tokio::select! {
                quit_request = signal::ctrl_c(), if ctrl_c_listener_enabled => {
                    match quit_request {
                        Ok(()) => {
                            send_shutdown_request(&mut r_sender, ui_quit_sender).await;
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

        at.join();
        dm_task_handle.await.unwrap();
        ui_task_handle.await.unwrap();

        // And finally close router.

        r_quit_sender.send(()).unwrap();
        router_task_handle.await.unwrap();
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
