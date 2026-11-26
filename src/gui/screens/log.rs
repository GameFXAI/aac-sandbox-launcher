//! The Log screen: what the launcher did this session (Settings > Show log).

use crate::gui::theme::*;
use crate::gui::{App, Page};
use eframe::egui::{self, Align, Layout, Rect, RichText, UiBuilder, vec2};

impl App {
    pub(in crate::gui) fn log_tab(&mut self, ui: &mut egui::Ui, body: Rect) {
        let cardr = Rect::from_center_size(body.center() + vec2(0.0, 4.0), vec2(body.width() - 96.0, body.height() - 56.0));
        card(ui.painter(), cardr);
        let inner = cardr.shrink2(vec2(28.0, 22.0));
        ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Log").font(title_font(28.0)).color(INK));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Back").clicked() {
                        self.page = Page::Settings;
                    }
                    if ui.button("Copy").clicked() {
                        ui.ctx().copy_text(self.log.join("\n"));
                    }
                });
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                if self.log.is_empty() {
                    ui.label(RichText::new("Nothing yet.").color(MUTED));
                }
                for l in &self.log {
                    ui.label(RichText::new(l).monospace().color(INK));
                }
            });
        });
    }
}
