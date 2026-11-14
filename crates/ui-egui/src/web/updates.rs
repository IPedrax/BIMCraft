//! In a browser there is no updater: the page is whatever version the site serves, and only the desktop phones home. These are
//! the names the desktop's `updates.rs` gives the window, doing nothing.

use egui::{Context, Ui};

use crate::{BimApp, Status};

pub(crate) fn failed(text: String) -> Status {
    Status { text, error: true }
}

impl BimApp {
    pub(crate) fn poll_updates(&mut self, _: &Context) {}

    pub(crate) fn installing(&self) -> bool {
        false
    }

    pub(crate) fn has_update_banner(&self) -> bool {
        false
    }

    pub(crate) fn installing_box(&self, _: &Context) {}

    pub(crate) fn update_banner(&mut self, _: &mut Ui) {}

    pub(crate) fn update_menu(&mut self, _: &mut Ui) {}

    pub(crate) fn install_update(&mut self) {}
}
