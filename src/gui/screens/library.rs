//! The Library screen: the account's replays, from the server. Not built yet and not in the navigation: a later
//! step adds `Tab::Library`, a nav entry in `App::top_bar`, its `Action`s and the `Event`s its background work sends.

use crate::gui::{Action, App};
use eframe::egui::{self, Rect};

impl App {
    /// Draws the Library screen into `body`. Draws nothing for now.
    #[allow(dead_code)]
    pub(in crate::gui) fn library_tab(&mut self, _ui: &mut egui::Ui, _body: Rect, _act: &mut Option<Action>) {}
}
