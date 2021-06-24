mod config;
mod logic;
mod settings;
mod ui;

use std::process;

use crate::{logic::{Logic, client::TestClient}, ui::{gtk_ui::GtkUi, Ui}};

fn main() {
    let config = config::parse_cmd_args();

    if config.test {
        println!("test");
        process::exit(0);
    }

    if let Some(config) = config.test_client_config {
        TestClient::new(config).run();
        return;
    }

    let settings = match settings::SettingsManager::load_or_create_settings_file() {
        Ok(settings) => settings,
        Err(e) => {
            eprintln!("Could not load the settings file. Error: {:?}", e);
            process::exit(1);
        }
    };

    println!("{:#?}", settings.get());

    GtkUi::run(config, settings);
}
