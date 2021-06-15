mod app;

use self::app::App;

use super::Ui;

use crate::logic::{Event, Logic};
use crate::{config::Config, settings::SettingsManager};

use gio::{prelude::*, ApplicationFlags};
use glib::{MainContext, Sender};
use gtk::{prelude::*, ApplicationWindow, Label};

//use futures::channel::mpsc::{self, Sender};

use app::UiEvent;

pub const SEND_ERROR: &str = "Error: UiEvent channel is broken.";

pub struct GtkUi;

impl Ui for GtkUi {
    fn run(config: Config, settings: SettingsManager) {
        glib::set_program_name("Multidevice".into());
        glib::set_application_name("Multidevice");

        gtk::init().expect("GTK initialization failed.");

        let (sender, receiver) = MainContext::channel::<UiEvent>(glib::PRIORITY_DEFAULT);

        let logic = Logic::new(config, settings, LogicEventSender::new(sender.clone()));
        let mut app = App::new(sender, logic);

        receiver.attach(None, move |event| {
            match event {
                UiEvent::ButtonClicked(id) => app.handle_button(id),
                UiEvent::CloseMainWindow => app.handle_close_main_window(),
                UiEvent::LogicEvent(e) => app.handle_logic_event(e),
                UiEvent::Quit => {
                    app.quit();
                    gtk::main_quit();
                    return glib::Continue(false);
                }
            }

            glib::Continue(true)
        });

        gtk::main();
    }
}

pub struct LogicEventSender {
    sender: Sender<UiEvent>,
}

impl LogicEventSender {
    pub fn new(sender: Sender<UiEvent>) -> Self {
        Self { sender }
    }

    pub fn send(&mut self, event: Event) {
        self.sender
            .send(UiEvent::LogicEvent(event))
            .expect(SEND_ERROR);
    }
}
