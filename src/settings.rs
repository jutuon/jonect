/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, prelude::*},
    path::Path,
};

const SETTINGS_FILE_NAME: &str = "multidevice_settings.toml";

#[derive(Serialize, Deserialize, Debug)]
pub struct Settings {
    test: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { test: false }
    }
}

impl Settings {
    fn save(&self) -> Result<(), SettingsError> {
        let text = toml::to_string_pretty(self).map_err(SettingsError::SerializeTomlError)?;

        let mut settings_file =
            File::create(SETTINGS_FILE_NAME).map_err(SettingsError::SaveError)?;

        settings_file
            .write_all(text.as_bytes())
            .map_err(SettingsError::SaveError)
    }

    fn load() -> Result<Self, SettingsError> {
        let data = fs::read(SETTINGS_FILE_NAME).map_err(SettingsError::ReadError)?;

        let settings: Settings = toml::from_slice(&data).map_err(SettingsError::ParseTomlError)?;

        Ok(settings)
    }
}

#[derive(Debug)]
pub enum SettingsError {
    ReadError(io::Error),
    SaveError(io::Error),
    ParseTomlError(toml::de::Error),
    SerializeTomlError(toml::ser::Error),
}

pub struct SettingsManager {
    changed: bool,
    settings: Settings,
}

impl SettingsManager {
    pub fn load_or_create_settings_file() -> Result<SettingsManager, SettingsError> {
        let settings_path = Path::new(SETTINGS_FILE_NAME);

        if !settings_path.is_file() {
            // Create default settings file

            Settings::default().save()?;
        }

        let settings = Settings::load()?;

        Ok(SettingsManager {
            changed: false,
            settings,
        })
    }

    pub fn modify(&mut self) -> &mut Settings {
        self.changed = true;
        &mut self.settings
    }

    pub fn get(&self) -> &Settings {
        &self.settings
    }

    pub fn save_if_changed(&self) -> Result<(), SettingsError> {
        if self.changed {
            self.settings.save()?;
        }

        Ok(())
    }
}
