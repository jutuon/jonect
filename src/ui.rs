/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! UI code.

pub mod gtk_ui;

use crate::{settings::SettingsManager, config::Config};

/// Trait for starting different UI implementations.
pub trait Ui {
    fn run(config: Config, settings: SettingsManager);
}
