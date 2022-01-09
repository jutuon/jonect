/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, prelude::*},
    path::Path,
};

const SETTINGS_FILE_NAME: &str = "jonect_settings.toml";

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Settings {
    test: bool,
}

impl Settings {
    fn save(&self) -> Result<(), SettingsError> {
        let text = toml::to_string_pretty(self).map_err(SettingsError::SerializeToml)?;

        let mut settings_file =
            File::create(SETTINGS_FILE_NAME).map_err(SettingsError::Save)?;

        settings_file
            .write_all(text.as_bytes())
            .map_err(SettingsError::Save)
    }

    fn load() -> Result<Self, SettingsError> {
        let data = fs::read(SETTINGS_FILE_NAME).map_err(SettingsError::Read)?;

        let settings: Settings = toml::from_slice(&data).map_err(SettingsError::ParseToml)?;

        Ok(settings)
    }
}

#[derive(Debug)]
pub enum SettingsError {
    Read(io::Error),
    Save(io::Error),
    ParseToml(toml::de::Error),
    SerializeToml(toml::ser::Error),
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
