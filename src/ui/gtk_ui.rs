use super::Ui;

use crate::logic::Logic;

use gio::{prelude::*, ApplicationFlags};
use gtk::{prelude::*, ApplicationWindow, Label};

pub struct GtkUi;

impl Ui for GtkUi {
    fn run(logic: Logic) {
        let gtk_app = gtk::Application::new(
            Some("com.github.jutuon.multidevice-server"),
            ApplicationFlags::FLAGS_NONE,
        )
        .expect("GTK application creation failed");

        gtk_app.connect_activate(|app| {
            let window = ApplicationWindow::new(app);
            window.set_title("Multidevice");
            window.set_default_size(640, 480);
            let text = Label::new(Some("Multidevice"));
            window.add(&text);

            window.show_all();
        });

        gtk_app.run(&[]);
    }
}
