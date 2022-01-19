/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

mod config;
mod settings;
mod ui;

use std::process;

use libjonect::{Logic, config::LogicConfig};
use ui::{gtk_ui::GtkUi, Ui};

/// Main function. Program starts here.
fn main() {
    let config = config::parse_cmd_args();

    if config.test {
        println!("test");
        process::exit(0);
    }

    if let Some(client_config) = config.test_client_config {
        Logic::run(LogicConfig {
            pa_source_name: config.pa_source_name,
            encode_opus: config.encode_opus,
            connect_address: Some(client_config.address),
            enable_connection_listening: false,
            enable_ping: false,
        });
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

    Logic::run(LogicConfig {
        pa_source_name: config.pa_source_name,
        encode_opus: config.encode_opus,
        connect_address: None,
        enable_connection_listening: true,
        enable_ping: true,
    });
}
