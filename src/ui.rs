pub mod gtk_ui;

use crate::logic::Logic;

pub trait Ui {
    fn run(logic: Logic);
}
