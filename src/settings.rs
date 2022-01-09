/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Settings file code. Settings file is not currently used for anything.

use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, prelude::*},
    path::Path,
};

/// File name for settings file.
const SETTINGS_FILE_NAME: &str = "jonect_settings.toml";

/// Settings
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Settings {
    test: bool,
}

impl Settings {
    /// Save settings.
    fn save(&self) -> Result<(), SettingsError> {
        let text = toml::to_string_pretty(self).map_err(SettingsError::SerializeToml)?;

        let mut settings_file =
            File::create(SETTINGS_FILE_NAME).map_err(SettingsError::Save)?;

        settings_file
            .write_all(text.as_bytes())
            .map_err(SettingsError::Save)
    }

    /// Load settings from default settings file.
    fn load() -> Result<Self, SettingsError> {
        let data = fs::read(SETTINGS_FILE_NAME).map_err(SettingsError::Read)?;

        let settings: Settings = toml::from_slice(&data).map_err(SettingsError::ParseToml)?;

        Ok(settings)
    }
}

/// Possible errors when `Settings` is loaded or saved.
#[derive(Debug)]
pub enum SettingsError {
    /// File reading errors.
    Read(io::Error),

    /// File saving errors.
    Save(io::Error),

    /// Settings file parsing errors.
    ParseToml(toml::de::Error),

    /// Settings serialization errors.
    SerializeToml(toml::ser::Error),
}


/// Keep track changes in settings to write settings file only when settings are
/// changed.
pub struct SettingsManager {
    changed: bool,
    settings: Settings,
}

impl SettingsManager {
    /// Create new `SettingsManager`. Loads settings file or creates a new
    /// settings file if it does not exist.
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

    /// Modify settings. Settings will be marked as modified when this is
    /// called.
    pub fn modify(&mut self) -> &mut Settings {
        self.changed = true;
        &mut self.settings
    }

    /// Get settings.
    pub fn get(&self) -> &Settings {
        &self.settings
    }

    /// Save settings if settings are marked as modified.
    pub fn save_if_changed(&self) -> Result<(), SettingsError> {
        if self.changed {
            self.settings.save()?;
        }

        Ok(())
    }
}
