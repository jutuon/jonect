/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

mod config;
mod settings;
mod ui;
mod client;
mod server;
mod utils;

use std::process;

use crate::{client::TestClient, server::Server};

use ui::{gtk_ui::GtkUi, Ui};

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

    if config.gui.is_some() {
        GtkUi::run(config, settings);
        return;
    }

    Server::run(config);
}
