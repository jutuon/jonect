use crate::logic::{Event, Logic};

use gio::{prelude::*, ApplicationFlags};
use glib::{MainContext, Sender};
use gtk::{prelude::*, Application, ApplicationWindow, Button, Label, Window};

use super::SEND_ERROR;

pub enum UiEvent {
    ButtonClicked(&'static str),
    LogicEvent(Event),
    CloseMainWindow,
    Quit,
}

pub struct App {
    sender: Sender<UiEvent>,
    logic: Logic,
    text: Label,
    main_window: Window,
}

impl App {
    pub fn new(sender: Sender<UiEvent>, logic: Logic) -> Self {
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

        let text = Label::new(Some("Multidevice"));

        let gtk_box = gtk::Box::new(gtk::Orientation::Vertical, 10);
        gtk_box.set_margin_top(10);
        gtk_box.add(&text);
        gtk_box.add(&button);

        window.add(&gtk_box);
        window.show_all();

        App {
            sender,
            logic,
            text,
            main_window: window,
        }
    }

    pub fn handle_close_main_window(&mut self) {
        self.main_window.close();
        self.logic.request_quit();
    }

    pub fn handle_logic_event(&mut self, e: Event) {
        match e {
            Event::Message(s) => {
                self.text.set_text(&s);
                println!("{}", s);
            }
            Event::CloseProgram => {
                self.sender.send(UiEvent::Quit).expect(SEND_ERROR);
            }
        }
    }

    pub fn handle_button(&mut self, id: &'static str) {
        match id {
            "test" => {
                self.logic.send_message();
            }
            _ => (),
        }
    }

    pub fn quit(&mut self) {
        self.logic.join_logic_thread();
    }
}
