pub mod gtk_ui;

use crate::{config::Config, settings::SettingsManager};

pub trait Ui {
    fn run(config: Config, settings: SettingsManager);
}
