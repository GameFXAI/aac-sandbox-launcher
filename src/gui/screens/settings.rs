//! The Settings screen (behind the gear): account, AA Classic folder, install mode, build packs, this launcher.

use crate::core::*;
use crate::gui::theme::*;
use crate::gui::widgets::mode_card;
use crate::gui::{Action, App, Tab};
use crate::{find, settings, update, vault};
use eframe::egui::{self, Align, Color32, Layout, Margin, Rect, RichText, Stroke, UiBuilder, vec2};

impl App {
    pub(in crate::gui) fn settings_tab(&mut self, ui: &mut egui::Ui, body: Rect, act: &mut Option<Action>) {
        let cardr = Rect::from_center_size(body.center() + vec2(0.0, 4.0), vec2(760.0, body.height() - 56.0));
        card(ui.painter(), cardr);
        let inner = cardr.shrink2(vec2(32.0, 26.0));
        let locked = self.busy || self.child.is_some();
        ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Settings").font(title_font(26.0)).color(INK));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(format!("Launcher {}", update::VERSION)).size(12.0).color(MUTED));
                });
            });
            ui.add_space(4.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(inner.width() - 12.0);

            ui.label(RichText::new("ACCOUNT").size(11.5).color(MUTED).extra_letter_spacing(1.2));
            ui.horizontal(|ui| {
                match &self.saved {
                    Some(s) => {
                        ui.label(RichText::new(format!("Signed in as {} (remembered on this PC; the password is not kept)", s.username)).color(INK));
                        if ui.button("Sign out").clicked() {
                            *act = Some(Action::SignOut);
                        }
                    }
                    None => {
                        ui.label(RichText::new("Not remembered on this PC: Play asks for the password.").color(INK));
                    }
                }
            });
            // the plan as the server last said it; the button is the one a real checkout will point elsewhere
            if let Some(l) = self.lib.data.clone() {
                let (name, line) = crate::gui::screens::library::plan_words(&l);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Plan: {name}")).font(bold_font(14.5)).color(if l.is_sub() { GOLD } else { INK }));
                    ui.label(RichText::new(line).size(12.5).color(MUTED));
                    if !l.upgrade_url.is_empty() {
                        let label = if l.is_sub() { "About your plan" } else { "Contact to upgrade" };
                        if ui.button(label).on_hover_text(l.upgrade_url.as_str()).clicked() {
                            *act = Some(Action::Upgrade);
                        }
                    }
                });
            }
            ui.add_space(6.0);

            ui.label(RichText::new("FROM AA CLASSIC").size(11.5).color(MUTED).extra_letter_spacing(1.2));
            ui.horizontal(|ui| {
                if ui.add_enabled(!locked, egui::Button::new("Bring my AA Classic settings…")).on_hover_text("Display, interface, keybinds, bars and gear: tick what comes over. Undo is on the same screen.").clicked() {
                    *act = Some(Action::OpenTransfer);
                }
                ui.label(RichText::new("Settings, keybinds, interface, bars and gear. AA Classic's files are only read.").size(12.5).color(MUTED));
            });
            ui.add_space(6.0);

            ui.label(RichText::new("AA CLASSIC FOLDER").size(11.5).color(MUTED).extra_letter_spacing(1.2));
            ui.horizontal(|ui| {
                let shown = self.cfg.aac_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "(not chosen)".into());
                egui::Frame::new().fill(Color32::from_rgb(12, 16, 23)).stroke(Stroke::new(1.0, Color32::from_rgb(54, 62, 78))).corner_radius(3).inner_margin(Margin::symmetric(10, 7)).show(ui, |ui| {
                    ui.set_width(inner.width() - 150.0);
                    ui.label(RichText::new(shown).monospace().color(INK));
                });
                if ui.add_enabled(!locked, egui::Button::new("Choose...")).clicked() {
                    *act = Some(Action::ChooseFolder);
                }
            });
            if let Some(d) = &self.cfg.aac_dir {
                if let Some(how) = self.aac_how {
                    ui.label(RichText::new(format!("Found through {how}.")).size(12.5).color(MUTED));
                }
                if self.cfg.mode == Mode::Standard {
                    let note = find::place_note(&self.paths.root, &self.paths.client(), d);
                    ui.add(egui::Label::new(RichText::new(note).size(12.5).color(INK)).wrap());
                }
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Only binNext and game_pak are read. Nothing in this folder is ever written.").size(12.5).color(MUTED));
                let hint = "Takes your display, quality and sound settings from AA Classic's system.cfg into this client's own. That file is only read. No server address, language or account data is taken.";
                if ui.add_enabled(!locked && settings::real_file().is_some(), egui::Button::new("Copy my graphics settings")).on_hover_text(hint).clicked() {
                    *act = Some(Action::CopySettings);
                }
            });
            ui.add_space(4.0);

            ui.label(RichText::new("INSTALL MODE").size(11.5).color(MUTED).extra_letter_spacing(1.2));
            let w = (inner.width() - 24.0) / 3.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                if mode_card(ui, w, self.cfg.mode == Mode::Standard, !locked, "Standard", "~65 MB", "Copies the program files, shares AA Classic's game data. Same drive only.") {
                    *act = Some(Action::SetMode(Mode::Standard));
                }
                if mode_card(ui, w, self.cfg.mode == Mode::Full, !locked, "Full separation", "~45 GB", "Copies everything from your AA Classic folder. Nothing is shared.") {
                    *act = Some(Action::SetMode(Mode::Full));
                }
                if mode_card(ui, w, self.cfg.mode == Mode::Own, !locked, "Download", "~46 GB", "No AA Classic needed. Program files come from a friend's build pack.") {
                    *act = Some(Action::SetMode(Mode::Own));
                }
            });
            ui.add_space(6.0);

            ui.label(RichText::new("CLIENT BUILD").size(11.5).color(MUTED).extra_letter_spacing(1.2));
            ui.horizontal(|ui| {
                let n = vault::builds(&self.paths).len();
                ui.label(RichText::new(match n {
                    0 => "none yet".to_string(),
                    1 => "1 build kept".to_string(),
                    n => format!("{n} builds kept"),
                }).color(INK));
                if ui.add_enabled(!locked, egui::Button::new("Import build pack...")).on_hover_text("A pack is a program: only import one from someone you trust.").clicked() {
                    *act = Some(Action::ImportPack);
                }
                if ui.add_enabled(!locked && self.paths.exe().is_file(), egui::Button::new("Export build pack...")).clicked() {
                    *act = Some(Action::ExportPack);
                }
                ui.label(RichText::new("Only import a pack from someone you trust.").size(12.5).color(MUTED));
            });
            ui.add_space(10.0);

            ui.label(RichText::new("THIS LAUNCHER").size(11.5).color(MUTED).extra_letter_spacing(1.2));
            ui.horizontal(|ui| {
                if ui.button("Open its folder").on_hover_text(self.paths.root.display().to_string()).clicked() {
                    *act = Some(Action::OpenFolder);
                }
                if ui.button("Show log").clicked() {
                    self.tab = Tab::Log;
                }
                if ui.button("Copy log").on_hover_text("Copies what the launcher did, to send to whoever helps you. It holds no password.").clicked() {
                    ui.ctx().copy_text(self.log.join("
"));
                }
                let l1 = if self.confirming(&Action::RemoveClient) { "Click again to remove" } else { "Remove client copy" };
                if ui.add_enabled(!locked, egui::Button::new(RichText::new(l1).color(RED))).on_hover_text("Deletes this launcher's copy of the game. AA Classic is not touched.").clicked() {
                    *act = Some(Action::RemoveClient);
                }
                let l2 = if self.confirming(&Action::RemoveAll) { "Click again to remove" } else { "Remove everything" };
                if ui.add_enabled(!locked, egui::Button::new(RichText::new(l2).color(RED))).on_hover_text("Deletes the copy and this launcher's settings, addons and logs. AA Classic is not touched.").clicked() {
                    *act = Some(Action::RemoveAll);
                }
            });
            });
        });
    }
}
