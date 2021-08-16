mod app;
mod logic;

use self::app::App;
use self::logic::ServerConnectionHandle;

use super::Ui;

use crate::server::ui::{UiProtocolFromServerToUi};
use crate::{config::Config, settings::SettingsManager};

use gtk::gio::{prelude::*};
use gtk::glib::{MainContext, MainLoop, Sender};
use gtk::{prelude::*, Label};

//use futures::channel::mpsc::{self, Sender};

use app::UiEvent;

pub const SEND_ERROR: &str = "Error: UiEvent channel is broken.";

pub struct GtkUi;

impl Ui for GtkUi {
    fn run(config: Config, settings: SettingsManager) {
        gtk::glib::set_program_name("Multidevice".into());
        gtk::glib::set_application_name("Multidevice");

        // Without setting the global context to thread default context
        // the receiver.attatch() will panic.
        let context = MainContext::default();
        context.push_thread_default();

        gtk::init().expect("GTK initialization failed.");

        let (sender, receiver) = MainContext::channel::<UiEvent>(gtk::glib::PRIORITY_DEFAULT);

        let handle = ServerConnectionHandle::new(FromServerToUiSender::new(sender.clone()));
        let mut app = App::new(sender, handle);

        receiver.attach(None, move |event| {
            match event {
                UiEvent::ButtonClicked(id) => app.handle_button(id),
                UiEvent::CloseMainWindow => app.handle_close_main_window(),
                UiEvent::LogicEvent(e) => app.handle_logic_event(e),
                UiEvent::ServerDisconnected => app.handle_server_disconnect(),
                UiEvent::ConnectToServerFailed => app.handle_connect_to_server_failed(),
                UiEvent::Quit => {
                    app.quit();
                    gtk::main_quit();
                    return gtk::glib::Continue(false);
                }
            }

            gtk::glib::Continue(true)
        });

        gtk::main();
    }
}

pub struct FromServerToUiSender {
    sender: Sender<UiEvent>,
}

impl FromServerToUiSender {
    pub fn new(sender: Sender<UiEvent>) -> Self {
        Self { sender }
    }

    pub fn send(&mut self, event: UiProtocolFromServerToUi) {
        self.sender
            .send(UiEvent::LogicEvent(event))
            .expect(SEND_ERROR);
    }

    pub fn send_server_disconnected_message(&mut self) {
        self.sender
            .send(UiEvent::ServerDisconnected)
            .expect(SEND_ERROR);
    }

    pub fn send_connect_to_server_failed(&mut self) {
        self.sender
            .send(UiEvent::ConnectToServerFailed)
            .expect(SEND_ERROR);
    }
}
