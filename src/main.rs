/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

mod config;
mod settings;
mod ui;

use std::process;

use libjonect::{Logic, config::LogicConfig};
use ui::{gtk_ui::GtkUi, Ui};

use log::{info, error, debug};

pub struct StandardErrorLogger;

impl log::Log for StandardErrorLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn flush(&self) {}

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!(
                "{} Location: {}:{}",
                record.args(),
                record.module_path().unwrap_or_default(),
                record.line().unwrap_or_default(),
            );
        }
    }
}

/// Main function. Program starts here.
fn main() {
    log::set_logger(&StandardErrorLogger).unwrap();
    log::set_max_level(log::LevelFilter::Trace);

    let config = config::parse_cmd_args();

    if config.test {
        info!("test");
        process::exit(0);
    }

    if let Some(client_config) = config.test_client_config {
        Logic::run(LogicConfig {
            pa_source_name: config.pa_source_name,
            encode_opus: config.encode_opus,
            connect_address: Some(client_config.address),
            enable_connection_listening: false,
            enable_ping: false,
            enable_udp_audio_data_sending: config.udp_audio,
        }, None);
        return;
    }

    let settings = match settings::SettingsManager::load_or_create_settings_file() {
        Ok(settings) => settings,
        Err(e) => {
            error!("Could not load the settings file. Error: {:?}", e);
            process::exit(1);
        }
    };

    debug!("{:#?}", settings.get());

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
        enable_udp_audio_data_sending: config.udp_audio,
    }, None);
}
