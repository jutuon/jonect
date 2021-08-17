use crate::server::ui::{UiProtocolFromServerToUi, UiProtocolFromUiToServer};

use super::logic::ServerConnectionHandle;

use gtk::gio::{prelude::*};
use gtk::glib::{Sender};
use gtk::{prelude::*, Button, Label, Window};

use super::SEND_ERROR;

#[derive(Debug)]
pub enum UiEvent {
    ButtonClicked(&'static str),
    LogicEvent(UiProtocolFromServerToUi),
    ServerDisconnected,
    ConnectToServerFailed,
    CloseMainWindow,
    Quit,
}

pub struct App {
    sender: Sender<UiEvent>,
    handle: ServerConnectionHandle,
    text: Label,
    main_window: Window,
}

impl App {
    pub fn new(sender: Sender<UiEvent>, handle: ServerConnectionHandle) -> Self {
        let window = Window::new(gtk::WindowType::Toplevel);
        window.set_title("Multidevice");
        window.set_default_size(640, 480);

        let s = sender.clone();
        window.connect_delete_event(move |_, _| {
            s.send(UiEvent::CloseMainWindow).expect(SEND_ERROR);
            Inhibit(false)
        });

        let button = Button::with_label("Test");
        let s = sender.clone();
        button.connect_clicked(move |_| {
            s.send(UiEvent::ButtonClicked("test")).expect(SEND_ERROR);
        });

        let button_ping = Button::with_label("Ping");
        let s = sender.clone();
        button_ping.connect_clicked(move |_| {
            s.send(UiEvent::ButtonClicked("ping")).expect(SEND_ERROR);
        });

        let text = Label::new(Some("Multidevice"));

        let gtk_box = gtk::Box::new(gtk::Orientation::Vertical, 10);
        gtk_box.set_margin_top(10);
        gtk_box.add(&text);
        gtk_box.add(&button);
        gtk_box.add(&button_ping);

        window.add(&gtk_box);
        window.show_all();

        App {
            sender,
            handle,
            text,
            main_window: window,
        }
    }

    pub fn handle_close_main_window(&mut self) {
        self.main_window.close();
        self.sender.send(UiEvent::Quit).expect(SEND_ERROR);
    }

    pub fn handle_logic_event(&mut self, e: UiProtocolFromServerToUi) {
        match e {
            UiProtocolFromServerToUi::Message(s) => {
                self.text.set_text(&s);
                println!("{}", s);
            }
        }
    }

    pub fn handle_connect_to_server_failed(&mut self) {
        eprintln!("Connecting to the server failed.");
    }

    pub fn handle_server_disconnect(&mut self) {
        eprintln!("Server disconnected.")
    }

    pub fn handle_button(&mut self, id: &'static str) {
        match id {
            "test" => {
                self.handle.send(UiProtocolFromUiToServer::NotificationTest);
            }
            "ping" => {
                self.handle.send(UiProtocolFromUiToServer::RunDeviceConnectionPing);
            }
            _ => (),
        }
    }

    pub fn quit(&mut self) {
        self.handle.quit();
    }
}
