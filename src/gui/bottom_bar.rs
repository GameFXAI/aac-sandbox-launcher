//! The bar along the bottom: what is going on, in one line, and the big button that does the next thing.

use super::theme::*;
use super::widgets::big_button;
use super::{Action, App, Tab};
use crate::core::*;
use crate::{find, install, vault};
use eframe::egui::{self, Align2, Color32, FontId, Rect, Stroke, UiBuilder, pos2, vec2};
use std::time::Duration;

impl App {
    pub(in crate::gui) fn bottom_bar(&mut self, ui: &mut egui::Ui, rect: Rect, act: &mut Option<Action>) {
        let p = ui.painter().clone();
        vgrad(&p, rect, rgba(6, 8, 13, 225), rgba(3, 4, 7, 245));
        p.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, gold_a(80)));

        let mode = match self.cfg.mode {
            Mode::Standard => "Standard install",
            Mode::Full => "Full separation",
            Mode::Own => "Download",
        };
        let (dot, title, sub, button, action): (Color32, String, String, &str, Option<Action>) = if self.asking {
            (GOLD, "Asking the server".into(), self.log.last().cloned().unwrap_or_default(), "WAIT", None)
        } else if self.offer && self.tab == Tab::Play {
            (GOLD, "Before your first game".into(), "Bring your AA Classic settings over, or skip it: Settings can do it any time.".into(), "JUST PLAY", Some(Action::SkipOffer))
        } else if self.xfer.working && self.play_after_transfer {
            (GOLD, "Bringing your settings".into(), "The game starts right after.".into(), "WAIT", None)
        } else if self.signup && self.tab == Tab::Play {
            let sub = if self.baked_invite.is_some() { "PLAY makes your account, sets up your copy of the game and starts it." } else { "Have an account already? Use the link on the right." };
            match &self.hint {
                Some(h) => (RED, "New account".into(), h.clone(), "PLAY", Some(Action::SignUp)),
                None => (BLUE, "New account".into(), sub.into(), "PLAY", Some(Action::SignUp)),
            }
        } else if self.tab == Tab::Transfer && !self.installing && !self.busy {
            self.transfer_status()
        } else if self.installing {
            (GOLD, "Installing the game".into(), self.log.last().cloned().unwrap_or_default(), "STOP", Some(Action::Stop))
        } else if self.busy {
            (GOLD, "Installing your copy".into(), self.log.last().cloned().unwrap_or_default(), "INSTALLING", None)
        } else if let Some((c, _)) = &self.child {
            let _ = c;
            (BLUE, "Playing".into(), "The game is running. Have fun! Close it to play again.".into(), "PLAYING", None)
        } else {
            match &self.state {
                Err(e) => (RED, "AA Classic was not found".into(), e.clone(), "FIND IT", Some(Action::ChooseFolder)),
                Ok(State::NotSetUp) if self.cfg.mode == Mode::Own => {
                    let last = self.log.last().filter(|l| l.starts_with("install:") || l.starts_with("build pack:")).cloned();
                    if vault::builds(&self.paths).is_empty() {
                        (MUTED, "First: a build pack".into(), last.unwrap_or_else(|| "The program files (65 MB) come from a friend who has the game: ask for a build pack and import it.".into()), "IMPORT PACK", Some(Action::ImportPack))
                    } else if install::load_state(&self.paths).is_some() {
                        (GOLD, "Install not finished".into(), last.unwrap_or_else(|| "Goes on where it stopped.".into()), "CONTINUE", Some(Action::Install))
                    } else {
                        (MUTED, "Not installed yet".into(), last.unwrap_or_else(|| "Downloads the game data from AA Classic's file host: about 24 GB to download, 46 GB on disk. No account needed.".into()), "INSTALL GAME", Some(Action::Install))
                    }
                }
                Ok(State::NotSetUp) => {
                    // AA Classic on another drive: say where the copy goes and why
                    let other = match (&self.cfg.aac_dir, self.cfg.mode) {
                        (Some(d), Mode::Standard) if find::client_place(&self.paths.client(), d).is_some() => Some(find::place_note(&self.paths.root, &self.paths.client(), d)),
                        _ => None,
                    };
                    (
                        if self.hint.is_some() { RED } else { MUTED },
                        "One step before you play".into(),
                        self.hint.clone().or(other).unwrap_or_else(|| format!(
                            "The first press makes your own copy of the game ({}), then it starts. AA Classic stays exactly as it is.",
                            if self.cfg.mode == Mode::Full { "about 45 GB" } else { "about 65 MB, a minute or two" }
                        )),
                        "PLAY",
                        Some(Action::Play),
                    )
                }
                Ok(State::BinOutdated) | Ok(State::PakOutdated) => {
                    (GOLD, "AA Classic was updated".into(), "PLAY updates your copy to match it first (a minute or two).".into(), "PLAY", Some(Action::Play))
                }
                Ok(State::Ready) => match &self.hint {
                    Some(h) => (RED, "Ready to play".into(), h.clone(), "PLAY", Some(Action::Play)),
                    None => {
                        let sub = match &self.saved {
                            Some(s) => format!("Signed in as {} · your copy of the game is up to date", s.username),
                            None if self.cfg.username.trim().is_empty() => "Enter your name and password on the right, then press Play.".to_string(),
                            None => "Enter your password on the right, then press Play.".to_string(),
                        };
                        let _ = mode;
                        (GREEN, "Ready to play".into(), sub, "PLAY", Some(Action::Play))
                    }
                },
            }
        };

        // status, left
        let x = rect.left() + 40.0;
        let y = rect.top() + 30.0;
        for i in 1..4 {
            p.circle_filled(pos2(x, y), 4.0 + i as f32 * 2.5, Color32::from_rgba_unmultiplied(dot.r(), dot.g(), dot.b(), 18));
        }
        p.circle_filled(pos2(x, y), 4.5, dot);
        p.text(pos2(x + 18.0, y), Align2::LEFT_CENTER, &title, bold_font(17.0), INK);
        let sub_g = p.layout(sub, FontId::proportional(13.0), MUTED, rect.width() - 420.0);
        p.galley(pos2(x + 18.0, y + 14.0), sub_g, MUTED);
        if self.busy {
            let bar = Rect::from_min_size(pos2(x + 18.0, rect.bottom() - 22.0), vec2(rect.width() - 420.0, 6.0));
            p.rect_filled(bar, 3.0, rgba(255, 255, 255, 18));
            let f = match self.progress {
                Some(v) => v,
                None => ((ui.input(|i| i.time) as f32 * 0.6).sin() * 0.5 + 0.5) * 0.9 + 0.05,
            };
            let fill = Rect::from_min_size(bar.min, vec2(bar.width() * f.clamp(0.02, 1.0), bar.height()));
            hgrad(&p, fill, Color32::from_rgb(170, 128, 50), Color32::from_rgb(246, 214, 140));
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }

        // big button, right
        let b = Rect::from_min_size(pos2(rect.right() - 300.0, rect.center().y - 30.0), vec2(256.0, 60.0));
        let enabled = action.is_some();
        let resp = ui.scope_builder(UiBuilder::new().max_rect(b), |ui| big_button(ui, b.size(), button, enabled)).inner;
        if resp.clicked() {
            *act = action;
        }
    }
}
