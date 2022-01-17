/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

mod config;
mod settings;
mod ui;
mod client;

use std::process;

use libjonect::{Server, config::ServerConfig};
use ui::{gtk_ui::GtkUi, Ui};

use crate::client::TestClient;

/// Main function. Program starts here.
fn main() {
    let config = config::parse_cmd_args();

    if config.test {
        println!("test");
        process::exit(0);
    }

    // Run test client mode.
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

    // Run GUI mode.
    if config.gui.is_some() {
        GtkUi::run(config, settings);
        return;
    }

    // Run server mode.
    Server::run(ServerConfig {
        pa_source_name: config.pa_source_name,
        encode_opus: config.encode_opus,
    });
}
